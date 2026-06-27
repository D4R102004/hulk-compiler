//! Lowering of `match` expressions and patterns.
//!
//! Each `match` expression is lowered to a linear sequence of pattern checks.
//! - Literal patterns compare by value (`==` for numbers and booleans,
//!   `hulk_rt_string_equals` for strings).
//! - Type patterns call `hulk_rt_downcast_check` and bind the aliased variable
//!   to the downcasted pointer.
//! - Wildcard and variable patterns always match.
//! - If no pattern matches, `hulk_rt_match_fail()` is called (non‑returning).

use inkwell::values::{BasicValueEnum, IntValue, PointerValue};
use inkwell::FloatPredicate;
use hulk_ast::{Expr, MatchExpr, Pattern, Literal};
use hulk_semantic::Type;

use crate::error::CodegenError;
use crate::lower::LowerCtx;
use crate::lower::utils::llvm_type;
use super::lower_expr;

/// Lowers a `match` expression.
///
/// The scrutinee is evaluated once. Each case is tested sequentially; the first
/// matching case executes its body and the result becomes the value of the
/// whole `match`. If no case matches, `hulk_rt_match_fail` is called.
pub fn lower_match<'ctx>(
    ctx: &mut LowerCtx<'_, 'ctx>,
    match_expr: &MatchExpr<Type>,
) -> Result<BasicValueEnum<'ctx>, CodegenError> {
    let result_ty = llvm_type(ctx.codegen, &match_expr.value.anno)?;
    let result_alloca = ctx
        .codegen
        .builder
        .build_alloca(result_ty, "match_result")
        .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;

    // Store a dummy default value (zero/null) – it is only used if the
    // match is non‑exhaustive, which will call `match_fail` instead.
    let dummy = match &match_expr.value.anno {
        Type::Number => ctx.codegen.context.f64_type().const_float(0.0).into(),
        Type::Boolean => ctx.codegen.context.bool_type().const_int(0, false).into(),
        Type::String | Type::Object | Type::Vector(_) => {
            let ptr_type = ctx.codegen.context.ptr_type(Default::default());
            ptr_type.const_null().into()
        }
        Type::Named(_) => {
            let ptr_type = ctx.codegen.context.ptr_type(Default::default());
            ptr_type.const_null().into()
        }
        Type::Iterable(_) => {
            let ptr_type = ctx.codegen.context.ptr_type(Default::default());
            let struct_ty = ctx.codegen.context.struct_type(&[ptr_type.into(), ptr_type.into()], false);
            let null_ptr = ptr_type.const_null();
            ctx.codegen.context.const_struct(&[null_ptr.into(), null_ptr.into()], false).into()
        }
        _ => {
            return Err(CodegenError::Unsupported {
                construct: format!("match result type `{}` not supported", match_expr.value.anno),
            });
        }
    };
    ctx.codegen
        .builder
        .build_store(result_alloca, dummy)
        .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;

    let parent_fn = ctx
        .codegen
        .builder
        .get_insert_block()
        .unwrap()
        .get_parent()
        .unwrap();
    let merge_bb = ctx.codegen.context.append_basic_block(parent_fn, "match_merge");

    // Lower the scrutinee once.
    let scrutinee_val = lower_expr(ctx, &match_expr.value)?;
    let scrutinee_ty = &match_expr.value.anno;

    // For each case, generate a pattern check block and a body block.
    let mut case_check_blocks = Vec::new();
    let mut case_body_blocks = Vec::new();
    for (i, case) in match_expr.cases.iter().enumerate() {
        let check_bb = ctx.codegen.context.append_basic_block(parent_fn, &format!("case_{}_check", i));
        let body_bb = ctx.codegen.context.append_basic_block(parent_fn, &format!("case_{}_body", i));
        case_check_blocks.push(check_bb);
        case_body_blocks.push(body_bb);
    }
    // A block for the final failure (non‑exhaustive match).
    let fail_bb = ctx.codegen.context.append_basic_block(parent_fn, "match_fail");

    // Branch from current block to the first check block.
    ctx.codegen
        .builder
        .build_unconditional_branch(case_check_blocks[0])
        .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;

    // Generate each case.
    for (i, case) in match_expr.cases.iter().enumerate() {
        let check_bb = case_check_blocks[i];
        let body_bb = case_body_blocks[i];
        let next_bb = if i + 1 < case_check_blocks.len() {
            case_check_blocks[i + 1]
        } else {
            fail_bb
        };

        ctx.codegen.builder.position_at_end(check_bb);

        // Lower the pattern; returns a boolean condition and the bindings.
        let (cond, bindings) = lower_pattern(ctx, &case.pattern, &scrutinee_val, scrutinee_ty)?;

        // If pattern matches, jump to body; otherwise to next check or fail.
        ctx.codegen
            .builder
            .build_conditional_branch(cond, body_bb, next_bb)
            .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;

        // Body block.
        ctx.codegen.builder.position_at_end(body_bb);

        // Push a new scope for the case body, and declare any bindings.
        ctx.push_scope();
        for (name, val) in bindings {
            let ty = val.get_type();
            let ptr = ctx
                .codegen
                .builder
                .build_alloca(ty, &name)
                .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;
            ctx.codegen
                .builder
                .build_store(ptr, val)
                .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;
            ctx.scope_stack.declare(&name, ptr, ty);
        }

        // Lower the case body.
        let body_val = lower_expr(ctx, &case.body)?;

        // Store the body value in the result alloca.
        ctx.codegen
            .builder
            .build_store(result_alloca, body_val)
            .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;

        ctx.pop_scope();

        // Jump to the merge block.
        ctx.codegen
            .builder
            .build_unconditional_branch(merge_bb)
            .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;
    }

    // Fail block: call `hulk_rt_match_fail()` (which does not return).
    ctx.codegen.builder.position_at_end(fail_bb);
    let fail_fn = ctx
        .codegen
        .functions
        .get("hulk_rt_match_fail")
        .cloned()
        .ok_or_else(|| CodegenError::Unsupported {
            construct: "hulk_rt_match_fail not declared".into(),
        })?;
    ctx.codegen
        .builder
        .build_call(fail_fn, &[], "match_fail_call")
        .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;
    ctx.codegen
        .builder
        .build_unreachable()
        .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;

    // Merge block: load the result and return it.
    ctx.codegen.builder.position_at_end(merge_bb);
    let result = ctx
        .codegen
        .builder
        .build_load(result_ty, result_alloca, "match_result_load")
        .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;
    Ok(result)
}

/// Lowers a single pattern and returns a condition (i1) indicating whether it matches,
/// plus a list of (variable_name, value) bindings that must be declared in the case body.
///
/// # Parameters
/// - `ctx`: the lowering context.
/// - `pattern`: the pattern to match.
/// - `scrutinee_val`: the LLVM value of the scrutinee expression.
/// - `scrutinee_ty`: the static type of the scrutinee.
fn lower_pattern<'ctx>(
    ctx: &mut LowerCtx<'_, 'ctx>,
    pattern: &Pattern,
    scrutinee_val: &BasicValueEnum<'ctx>,
    scrutinee_ty: &Type,
) -> Result<(IntValue<'ctx>, Vec<(String, BasicValueEnum<'ctx>)>), CodegenError> {
    let bool_ty = ctx.codegen.context.bool_type();
    let true_val = bool_ty.const_int(1, false);
    let false_val = bool_ty.const_int(0, false);

    match pattern {
        Pattern::Wildcard => Ok((true_val, Vec::new())),

        Pattern::Variable(name) => {
            // Always matches, binds the scrutinee value.
            Ok((true_val, vec![(name.clone(), *scrutinee_val)]))
        }

        Pattern::Literal(lit) => {
            let cond = match lit {
                Literal::Number(n) => {
                    let c = ctx.codegen.context.f64_type().const_float(*n);
                    let cmp = ctx.codegen.builder.build_float_compare(
                        FloatPredicate::OEQ,
                        scrutinee_val.clone().into_float_value(),
                        c,
                        "lit_cmp",
                    ).map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;
                    cmp
                }
                Literal::Boolean(b) => {
                    let c = ctx.codegen.context.bool_type().const_int(if *b { 1 } else { 0 }, false);
                    let cmp = ctx.codegen.builder.build_int_compare(
                        inkwell::IntPredicate::EQ,
                        scrutinee_val.clone().into_int_value(),
                        c,
                        "lit_cmp",
                    ).map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;
                    cmp
                }
                Literal::String(s) => {
                    // Construct a typed literal string expression to lower.
                    let lit_expr = Expr {
                        kind: ExprKind::Literal(Literal::String(s.clone())),
                        anno: Type::String,
                        span: hulk_ast::SourceSpan::new(0, 0), // dummy span
                    };
                    let lit_val = lower_expr(ctx, &lit_expr)?;
                    let str_eq_fn = ctx
                        .codegen
                        .functions
                        .get("hulk_rt_string_equals")
                        .cloned()
                        .ok_or_else(|| CodegenError::Unsupported {
                            construct: "hulk_rt_string_equals not declared".into(),
                        })?;
                    let call = ctx.codegen.builder.build_call(
                        str_eq_fn,
                        &[scrutinee_val.clone().into(), lit_val.into()],
                        "str_eq",
                    ).map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;
                    call.try_as_basic_value().basic().unwrap().into_int_value()
                }
            };
            Ok((cond, Vec::new()))
        }

        Pattern::Type(type_ref, alias) => {
            // Type pattern: check if scrutinee is an instance of the type.
            let target_type_name = &type_ref.name;
            let target_vtable = ctx
                .codegen
                .type_layouts
                .get(target_type_name)
                .and_then(|layout| layout.vtable_global)
                .ok_or_else(|| CodegenError::Unsupported {
                    construct: format!("vtable for type `{}` not found", target_type_name),
                })?;
            let target_vtable_ptr = target_vtable.as_pointer_value();

            let downcast_fn = ctx
                .codegen
                .functions
                .get("hulk_rt_downcast_check")
                .cloned()
                .ok_or_else(|| CodegenError::Unsupported {
                    construct: "hulk_rt_downcast_check not declared".into(),
                })?;
            let obj_ptr = scrutinee_val.clone().into_pointer_value();
            let call = ctx.codegen.builder.build_call(
                downcast_fn,
                &[obj_ptr.into(), target_vtable_ptr.into()],
                "downcast_check",
            ).map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;
            let is_ok = call.try_as_basic_value().basic().unwrap().into_int_value();

            // If alias is present, bind the downcasted pointer.
            let mut bindings = Vec::new();
            if let Some(alias_name) = alias {
                let ptr_type = ctx.codegen.context.ptr_type(Default::default());
                let cast_ptr = ctx.codegen.builder.build_pointer_cast(
                    obj_ptr,
                    ptr_type,
                    "downcast_ptr",
                ).map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;
                bindings.push((alias_name.clone(), cast_ptr.into()));
            }
            Ok((is_ok, bindings))
        }
    }
}