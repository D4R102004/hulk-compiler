//! Lowering of vector expressions: literals, comprehensions, and indexing.

use inkwell::values::BasicValueEnum;
use hulk_ast::{Expr, SourceSpan, VectorExpr, VectorComprehension};
use hulk_semantic::Type;

use crate::error::CodegenError;
use crate::lower::LowerCtx;
use crate::runtime_decls::ensure_decl;
use crate::lower::utils::ensure_boxed;
use super::lower_expr;
use super::call::lower_method_call;

/// Dispatches to the appropriate vector lowering routine.
pub fn lower_vector<'ctx>(
    ctx: &mut LowerCtx<'_, 'ctx>,
    vector: &VectorExpr<Type>,
    expr_anno: &Type,
    span: SourceSpan,
) -> Result<BasicValueEnum<'ctx>, CodegenError> {
    match vector {
        VectorExpr::Literal(items) => lower_vector_literal(ctx, items, expr_anno, span),
        VectorExpr::Comprehension(comp) => lower_vector_comprehension(ctx, comp).map_err(|e| e.with_span(span)),
    }
}

/// Lowers a vector literal `[item1, item2, ...]` to a runtime vector.
///
/// Allocates a fixed-size vector via `hulk_rt_vector_new`, stores each
/// element after lowering and optionally boxing, and retains each element
/// in the vector's storage.
fn lower_vector_literal<'ctx>(
    ctx: &mut LowerCtx<'_, 'ctx>,
    items: &[Expr<Type>],
    elem_type: &Type,
    span: SourceSpan,
) -> Result<inkwell::values::BasicValueEnum<'ctx>, CodegenError> {
    let len = items.len() as u64;
    let len_val = ctx.codegen.context.i64_type().const_int(len, false);

    let new_fn = ctx
        .codegen
        .functions
        .get("hulk_rt_vector_new")
        .copied()
        .ok_or_else(|| CodegenError::unsupported("hulk_rt_vector_new not declared", Some(span)))?;

    let call = ctx
        .codegen
        .builder
        .build_call(new_fn, &[len_val.into()], "vec_new")
        .map_err(|e| CodegenError::llvm_verification(e.to_string()))?;
    let vec_ptr = call
        .try_as_basic_value()
        .unwrap_basic()
        .into_pointer_value();

    let set_fn = ctx
        .codegen
        .functions
        .get("hulk_rt_vector_set")
        .copied()
        .ok_or_else(|| CodegenError::unsupported("hulk_rt_vector_set not declared", Some(span)))?;

    let retain_fn = ctx
        .codegen
        .functions
        .get("hulk_rt_retain")
        .copied()
        .ok_or_else(|| CodegenError::unsupported("hulk_rt_retain not declared", Some(span)))?;

    for (i, item) in items.iter().enumerate() {
        let idx_val = ctx.codegen.context.i64_type().const_int(i as u64, false);

        let mut elem_val = lower_expr(ctx, item)?;
        elem_val = ensure_boxed(ctx, elem_val, &item.anno, elem_type)?;

        ctx.codegen
            .builder
            .build_call(set_fn, &[vec_ptr.into(), idx_val.into(), elem_val.into()], "vec_set")
            .map_err(|e| CodegenError::llvm_verification(e.to_string()))?;

        ctx.codegen
            .builder
            .build_call(retain_fn, &[elem_val.into()], "retain_elem")
            .map_err(|e| CodegenError::llvm_verification(e.to_string()))?;
    }

    Ok(vec_ptr.into())
}

/// Lowers a vector comprehension `[expr | var in iterable]`.
///
/// The comprehension collects the result of evaluating `expr` for each element
/// of the iterable into a dynamic vector, then converts it to a fixed‑size
/// `HulkVector` at the end.
fn lower_vector_comprehension<'ctx>(
    ctx: &mut LowerCtx<'_, 'ctx>,
    comp: &VectorComprehension<Type>,
) -> Result<BasicValueEnum<'ctx>, CodegenError> {
    let iterable_expr = &comp.iterable;
    let head_expr = &comp.expr;

    // Determine the element type of the comprehension (the head expression's type).
    let elem_ty = head_expr.anno.clone();
    let elem_llvm_ty = crate::lower::utils::llvm_type(ctx.codegen, ctx.registry, &elem_ty)?;

    // Create a dynamic vector.
    let dyn_new_fn = ensure_decl(ctx.codegen, "hulk_rt_dynamic_vector_new")?;
    let dyn_vec = ctx
        .codegen
        .builder
        .build_call(dyn_new_fn, &[], "dyn_vec")
        .map_err(|e| CodegenError::llvm_verification(e.to_string()))?
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
        .map_err(|e| CodegenError::llvm_verification(e.to_string()))?;

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
        .map_err(|e| CodegenError::llvm_verification(e.to_string()))?;

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
        .map_err(|e| CodegenError::llvm_verification(e.to_string()))?;
    ctx.codegen
        .builder
        .build_store(var_ptr, current_val)
        .map_err(|e| CodegenError::llvm_verification(e.to_string()))?;
    ctx.scope_stack.declare(&comp.var, var_ptr, elem_llvm_ty, elem_ty.clone());

    // Lower the head expression.
    let mut head_val = lower_expr(ctx, head_expr)?;

    // If the element type is primitive (Number, Boolean), box it for storage in vector.
    if matches!(elem_ty, Type::Number | Type::Boolean) {
        head_val = crate::lower::utils::box_primitive(ctx, head_val, &elem_ty)?;
    }

    // Append to dynamic vector.
    let append_fn = ensure_decl(ctx.codegen, "hulk_rt_dynamic_vector_append")?;
    ctx.codegen
        .builder
        .build_call(append_fn, &[dyn_vec.into(), head_val.into()], "append")
        .map_err(|e| CodegenError::llvm_verification(e.to_string()))?;

    // Jump back to condition.
    ctx.codegen
        .builder
        .build_unconditional_branch(cond_bb)
        .map_err(|e| CodegenError::llvm_verification(e.to_string()))?;

    // ── Exit block ──────────────────────────────────────────────────────
    ctx.codegen.builder.position_at_end(exit_bb);

    let to_vec_fn = ensure_decl(ctx.codegen, "hulk_rt_dynamic_vector_to_vector")?;
    let result = ctx
        .codegen
        .builder
        .build_call(to_vec_fn, &[dyn_vec.into()], "comp_result")
        .map_err(|e| CodegenError::llvm_verification(e.to_string()))?
        .try_as_basic_value()
        .basic()
        .unwrap();
    Ok(result)
}