//! Lowering of `for` loops and vector comprehensions.

use inkwell::values::BasicValueEnum;

use hulk_ast::{ForExpr, VectorComprehension};
use hulk_semantic::Type;

use crate::error::CodegenError;
use crate::lower::call::lower_method_call;
use crate::lower::LowerCtx;
use super::lower_expr;

/// Lowers a `for` loop.
///
/// The loop iterates over the iterable expression, repeatedly calling `next()`
/// to test continuation and `current()` to obtain the element. The element is
/// bound to the loop variable in a fresh scope for each iteration.
///
/// The value of the `for` expression is the value of the last executed body,
/// or a default value (zero/null) if the body never executes.
pub fn lower_for<'ctx>(
    ctx: &mut LowerCtx<'_, 'ctx>,
    for_expr: &ForExpr<Type>,
) -> Result<BasicValueEnum<'ctx>, CodegenError> {
    // The iterable expression is lowered once, before the loop.
    let iterable_expr = &for_expr.iterable;
    let body_expr = &for_expr.body;

    // Determine the element type from the iterable's `current()` method.
    let elem_ty = ctx
        .registry
        .lookup_method(&iterable_expr.anno, "current")
        .map(|sig| sig.return_type)
        .ok_or_else(|| {
            CodegenError::Unsupported {
                construct: format!("iterable type `{}` does not support `current()`", iterable_expr.anno),
            }
        })?;
    let elem_llvm_ty = crate::lower::utils::llvm_type(ctx.codegen, &elem_ty)?;

    // Result storage: the loop's value is the last body value, or a default.
    let result_ty = crate::lower::utils::llvm_type(ctx.codegen, &body_expr.anno)?;
    let result_alloca = ctx
        .codegen
        .builder
        .build_alloca(result_ty, "for_result")
        .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;

    // Default value (zero/null) when body never executes.
    let default_val: BasicValueEnum<'ctx> = match &body_expr.anno {
        Type::Number => {
            let val = ctx.codegen.context.f64_type().const_float(0.0);
            BasicValueEnum::FloatValue(val)
        }
        Type::Boolean => {
            let val = ctx.codegen.context.bool_type().const_int(0, false);
            BasicValueEnum::IntValue(val)
        }
        Type::String | Type::Object | Type::Vector(_) => {
            let ptr_type = ctx.codegen.context.ptr_type(Default::default());
            let val = ptr_type.const_null();
            BasicValueEnum::PointerValue(val)
        }
        Type::Named(_) => {
            let ptr_type = ctx.codegen.context.ptr_type(Default::default());
            let val = ptr_type.const_null();
            BasicValueEnum::PointerValue(val)
        }
        Type::Iterable(_) => {
            // Fat pointer: { ptr, ptr } – null both.
            let ptr_type = ctx.codegen.context.ptr_type(Default::default());
            let null_ptr = ptr_type.const_null();
            let null_struct = ctx.codegen.context.const_struct(&[null_ptr.into(), null_ptr.into()], false);
            BasicValueEnum::StructValue(null_struct)
        }
        _ => {
            return Err(CodegenError::Unsupported {
                construct: format!("default value for type `{}` not implemented", body_expr.anno),
            });
        }
    };
    ctx.codegen
        .builder
        .build_store(result_alloca, default_val)
        .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;

    // Basic blocks.
    let parent_fn = ctx
        .codegen
        .builder
        .get_insert_block()
        .unwrap()
        .get_parent()
        .unwrap();
    let cond_bb = ctx.codegen.context.append_basic_block(parent_fn, "for_cond");
    let body_bb = ctx.codegen.context.append_basic_block(parent_fn, "for_body");
    let exit_bb = ctx.codegen.context.append_basic_block(parent_fn, "for_exit");

    // Jump to condition.
    ctx.codegen
        .builder
        .build_unconditional_branch(cond_bb)
        .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;

    // ── Condition block ─────────────────────────────────────────────────
    ctx.codegen.builder.position_at_end(cond_bb);

    // Call `next()` on the iterable; returns a boolean.
    let next_val = lower_method_call(
        ctx,
        (**iterable_expr).clone(),
        "next",
        &[],
        iterable_expr.span,
    )?;
    let next_bool = next_val.into_int_value();

    ctx.codegen
        .builder
        .build_conditional_branch(next_bool, body_bb, exit_bb)
        .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;

    // ── Body block ──────────────────────────────────────────────────────
    ctx.codegen.builder.position_at_end(body_bb);

    // Call `current()` to get the element.
    let current_val = lower_method_call(
        ctx,
        (**iterable_expr).clone(),
        "current",
        &[],
        iterable_expr.span,
    )?;

    // Bind the loop variable.
    ctx.push_scope();
    let var_ptr = ctx
        .codegen
        .builder
        .build_alloca(elem_llvm_ty, &for_expr.var)
        .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;
    ctx.codegen
        .builder
        .build_store(var_ptr, current_val)
        .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;
    ctx.scope_stack.declare(&for_expr.var, var_ptr, elem_llvm_ty);

    // Lower the body.
    let body_val = lower_expr(ctx, body_expr)?;

    // Store the body's value as the loop result.
    ctx.codegen
        .builder
        .build_store(result_alloca, body_val)
        .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;

    ctx.pop_scope();

    // Jump back to condition.
    ctx.codegen
        .builder
        .build_unconditional_branch(cond_bb)
        .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;

    // ── Exit block ──────────────────────────────────────────────────────
    ctx.codegen.builder.position_at_end(exit_bb);
    let result = ctx
        .codegen
        .builder
        .build_load(result_ty, result_alloca, "for_result_load")
        .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;
    Ok(result)
}

/// Lowers a vector comprehension `[expr | var in iterable]`.
///
/// The comprehension collects the result of evaluating `expr` for each element
/// of the iterable into a dynamic vector, then converts it to a fixed‑size
/// `HulkVector` at the end.
pub fn lower_vector_comprehension<'ctx>(
    ctx: &mut LowerCtx<'_, 'ctx>,
    comp: &VectorComprehension<Type>,
) -> Result<BasicValueEnum<'ctx>, CodegenError> {
    let iterable_expr = &comp.iterable;
    let head_expr = &comp.expr;

    // Determine the element type of the comprehension (the head expression's type).
    let elem_ty = head_expr.anno.clone();
    let elem_llvm_ty = crate::lower::utils::llvm_type(ctx.codegen, &elem_ty)?;

    // Create a dynamic vector.
    let dyn_new_fn = ctx
        .codegen
        .functions
        .get("hulk_rt_dynamic_vector_new")
        .cloned()
        .ok_or_else(|| CodegenError::Unsupported {
            construct: "hulk_rt_dynamic_vector_new not declared".into(),
        })?;
    let dyn_vec = ctx
        .codegen
        .builder
        .build_call(dyn_new_fn, &[], "dyn_vec")
        .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?
        .try_as_basic_value()
        .basic()
        .unwrap()
        .into_pointer_value();

    // Loop over the iterable, appending the head expression.
    let parent_fn = ctx
        .codegen
        .builder
        .get_insert_block()
        .unwrap()
        .get_parent()
        .unwrap();
    let cond_bb = ctx.codegen.context.append_basic_block(parent_fn, "comp_cond");
    let body_bb = ctx.codegen.context.append_basic_block(parent_fn, "comp_body");
    let exit_bb = ctx.codegen.context.append_basic_block(parent_fn, "comp_exit");

    ctx.codegen
        .builder
        .build_unconditional_branch(cond_bb)
        .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;

    // ── Condition block ─────────────────────────────────────────────────
    ctx.codegen.builder.position_at_end(cond_bb);
    let next_val = lower_method_call(
        ctx,
        (**iterable_expr).clone(),
        "next",
        &[],
        iterable_expr.span,
    )?;
    let next_bool = next_val.into_int_value();
    ctx.codegen
        .builder
        .build_conditional_branch(next_bool, body_bb, exit_bb)
        .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;

    // ── Body block ──────────────────────────────────────────────────────
    ctx.codegen.builder.position_at_end(body_bb);

    let current_val = lower_method_call(
        ctx,
        (**iterable_expr).clone(),
        "current",
        &[],
        iterable_expr.span,
    )?;

    // Bind the loop variable.
    ctx.push_scope();
    let var_ptr = ctx
        .codegen
        .builder
        .build_alloca(elem_llvm_ty, &comp.var)
        .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;
    ctx.codegen
        .builder
        .build_store(var_ptr, current_val)
        .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;
    ctx.scope_stack.declare(&comp.var, var_ptr, elem_llvm_ty);

    // Lower the head expression.
    let head_val = lower_expr(ctx, head_expr)?;
    ctx.pop_scope();

    // Append to dynamic vector.
    let append_fn = ctx
        .codegen
        .functions
        .get("hulk_rt_dynamic_vector_append")
        .cloned()
        .ok_or_else(|| CodegenError::Unsupported {
            construct: "hulk_rt_dynamic_vector_append not declared".into(),
        })?;
    ctx.codegen
        .builder
        .build_call(append_fn, &[dyn_vec.into(), head_val.into()], "append")
        .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;

    // Jump back to condition.
    ctx.codegen
        .builder
        .build_unconditional_branch(cond_bb)
        .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;

    // ── Exit block ──────────────────────────────────────────────────────
    ctx.codegen.builder.position_at_end(exit_bb);

    let to_vec_fn = ctx
        .codegen
        .functions
        .get("hulk_rt_dynamic_vector_to_vector")
        .cloned()
        .ok_or_else(|| CodegenError::Unsupported {
            construct: "hulk_rt_dynamic_vector_to_vector not declared".into(),
        })?;
    let result = ctx
        .codegen
        .builder
        .build_call(to_vec_fn, &[dyn_vec.into()], "comp_result")
        .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?
        .try_as_basic_value()
        .basic()
        .unwrap();
    Ok(result)
}