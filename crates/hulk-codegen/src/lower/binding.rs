//! Lowering of variable bindings and assignments.
//!
//! This module handles:
//! - Variable references (`Variable`): looking up a variable in the current
//!   lexical scope and loading its value.
//! - `let` expressions: introducing new variables in a fresh scope, with
//!   sequential binding evaluation (later bindings can refer to earlier ones).
//! - Assignment (`Assign`): storing a new value into an existing variable.
//!
//! The lexical scoping discipline mirrors `hulk_semantic::Environment`:
//! - Each `let` introduces a new scope that encloses its body.
//! - Bindings within a single `let` are processed left‑to‑right, and
//!   subsequent bindings can refer to earlier ones.
//! - A plain `Block` does not introduce a new scope (handled in `control.rs`).

use inkwell::values::BasicValueEnum;
use hulk_ast::{AssignExpr, AssignTarget, LetExpr, SourceSpan};
use hulk_semantic::Type;

use crate::error::CodegenError;
use crate::lower::LowerCtx;
use crate::lower::utils::{resolve_attribute_with_offset, resolve_type_ref_to_type, convert_to_protocol, is_heap_allocated_type, is_protocol_or_iterable};
use crate::lower::builtins::lookup_constant;
use crate::lower::index;
use super::lower_expr;

/// Lowers a variable reference.
///
/// Looks up the variable in the current lexical scope and loads its value
/// from the corresponding `alloca`. If the variable is not found, this
/// returns an error (which should never happen for a semantically verified
/// program).
///
/// # Parameters
/// - `ctx`: the lowering context.
/// - `name`: the variable name.
///
/// # Returns
/// The loaded value as an LLVM `BasicValueEnum`.
///
/// # Errors
/// - `CodegenError::Unsupported` if the variable is not in scope (should not
///   occur after semantic analysis).
pub fn lower_variable<'ctx>(
    ctx: &mut LowerCtx<'_, 'ctx>,
    name: &str,
    span: Option<SourceSpan>,
) -> Result<inkwell::values::BasicValueEnum<'ctx>, CodegenError> {
    // Built-in global constants lookup
    if let Some(val) = lookup_constant(name) {
        let float = ctx.codegen.context.f64_type().const_float(val);
        return Ok(float.into());
    }

    // General variable lookup in the current lexical scope.
    ctx.load_var(name, span)
}

/// Lowers a `let` expression with one or more bindings.
///
/// This function:
/// 1. Pushes a new lexical scope.
/// 2. Processes each binding in order:
///    - Lowers the initialiser expression (which may refer to previously
///      declared bindings in the same `let`).
///    - Allocates a stack slot (`alloca`) for the variable and stores the
///      initial value.
///    - Records the binding in the current scope.
/// 3. Lowers the body expression in the same scope.
/// 4. Pops the scope.
/// 5. Returns the body's value as the result of the `let` expression.
///
/// # Parameters
/// - `ctx`: the lowering context.
/// - `let_expr`: the `let` AST node.
///
/// # Returns
/// The value of the body expression.
///
/// # Errors
/// - Propagates errors from lowering the initialisers or body.
/// - Propagates errors from variable declaration (e.g., LLVM allocation
///   failures).
pub fn lower_let<'ctx>(
    ctx: &mut LowerCtx<'_, 'ctx>,
    let_expr: &LetExpr<Type>,
) -> Result<BasicValueEnum<'ctx>, CodegenError> {
    ctx.push_scope();

    for binding in &let_expr.bindings {
        // 1. Lower the initializer.
        let mut init_val = lower_expr(ctx, &binding.initializer)?;
        let init_ty = &binding.initializer.anno;

        // 2. Determine the declared type from the annotation, or fallback to initializer type.
        let declared_ty = if let Some(ann) = &binding.type_annotation {
            resolve_type_ref_to_type(ann, ctx.registry)
        } else {
            init_ty.clone()
        };
        // 3. If the declared type is a protocol and the initializer is concrete, convert.
        if is_protocol_or_iterable(&declared_ty, ctx.registry) {
            if let Type::Named(_) = init_ty {
                if !is_protocol_or_iterable(init_ty, ctx.registry) {
                    init_val = convert_to_protocol(ctx, init_val, init_ty, &declared_ty)?;
                }
            }
        }

        // 4. Declare the variable with the resolved semantic type.
        ctx.declare_var(&binding.name, init_val, declared_ty)?;
    }

    let body_val = lower_expr(ctx, &let_expr.body)?;
    ctx.pop_scope()?;
    Ok(body_val)
}

/// Lowers an assignment expression.
///
/// In Phase 3, only assignments to a simple variable target are supported.
/// For such a target:
/// 1. The right‑hand side is lowered to a value.
/// 2. The target variable's stack slot is looked up.
/// 3. The value is stored into that slot.
/// 4. The assignment expression itself returns the stored value (the same
///    convention used in C and many other languages).
///
/// # Parameters
/// - `ctx`: the lowering context.
/// - `assign`: the assignment AST node.
///
/// # Returns
/// The assigned value (the right‑hand side's value).
///
/// # Errors
/// - `CodegenError::Unsupported` if the target is not a variable (deferred
///   to later phases).
/// - Propagates errors from lowering the right‑hand side or looking up the
///   target variable.
pub fn lower_assign<'ctx>(
    ctx: &mut LowerCtx<'_, 'ctx>,
    assign: &AssignExpr<Type>,
) -> Result<inkwell::values::BasicValueEnum<'ctx>, CodegenError> {
    match &assign.target {
        AssignTarget::Variable(name) => {
            // 1. Look up the variable's LLVM pointer and semantic type.
            let (ptr, _llvm_ty, target_ty) = ctx.lookup_var(name, Some(assign.value.span))?;

            // 2. Lower the value to be stored.
            let mut stored_val = lower_expr(ctx, &assign.value)?;
            let val_ty = &assign.value.anno;

            // 3. If the target is a protocol and the source is a concrete class, convert.
            if ctx.registry.is_protocol(&target_ty) {
                if let Type::Named(_) = val_ty {
                    if !ctx.registry.is_protocol(val_ty) {
                        stored_val = convert_to_protocol(ctx, stored_val, val_ty, &target_ty)?;
                    }
                }
            }

            // 4. Load the old value (for release).
            let old_val = ctx
                .codegen
                .builder
                .build_load(
                    crate::lower::utils::llvm_type(ctx.codegen, ctx.registry, &target_ty)?,
                    ptr,
                    "old_var",
                )
                .map_err(|e| CodegenError::llvm_verification(e.to_string()))?;

            // 5. If the target type is heap-allocated, release the old value and retain the new.
            if is_heap_allocated_type(&target_ty, ctx.registry) {
                let release_fn = ctx
                    .codegen
                    .functions
                    .get("hulk_rt_release")
                    .cloned()
                    .ok_or_else(|| CodegenError::unsupported(
                        "hulk_rt_release not declared",
                        Some(assign.value.span),
                    ))?;
                ctx.codegen
                    .builder
                    .build_call(release_fn, &[old_val.into()], "release_old_var")
                    .map_err(|e| CodegenError::llvm_verification(e.to_string()))?;

                let retain_fn = ctx
                    .codegen
                    .functions
                    .get("hulk_rt_retain")
                    .cloned()
                    .ok_or_else(|| CodegenError::unsupported(
                        "hulk_rt_retain not declared",
                        Some(assign.value.span),
                    ))?;
                ctx.codegen
                    .builder
                    .build_call(retain_fn, &[stored_val.into()], "retain_new_var")
                    .map_err(|e| CodegenError::llvm_verification(e.to_string()))?;
            }

            // 6. Store the (possibly converted) value.
            ctx.store_var(name, stored_val, Some(assign.value.span))?;

            // 7. The assignment expression returns the stored value.
            Ok(stored_val)
        }
        AssignTarget::Member { object, field } => {
            // 1. Lower the object expression to get a pointer.
            let obj_val = lower_expr(ctx, object)?;
            let obj_ptr = obj_val.into_pointer_value();

            // 2. Determine the static type of the object and resolve the attribute.
            let obj_type = &object.anno;
            let (attr_type, offset) = resolve_attribute_with_offset(ctx, obj_type, field)?;

            // 3. Compute the field address using byte offset.
            let ptr_type = ctx.codegen.context.ptr_type(Default::default());
            let obj_i8 = ctx
                .codegen
                .builder
                .build_pointer_cast(obj_ptr, ptr_type, "obj_i8")
                .map_err(|e| CodegenError::llvm_verification(e.to_string()))?;
            let offset_val = ctx.codegen.context.i64_type().const_int(offset as u64, false);
            let field_ptr_i8 = unsafe {
                ctx.codegen
                    .builder
                    .build_gep(ctx.codegen.context.i8_type(), obj_i8, &[offset_val], "field_ptr")
                    .map_err(|e| CodegenError::llvm_verification(e.to_string()))?
            };

            // 4. Cast to the attribute's type pointer.
            let attr_ptr_type = ctx.codegen.context.ptr_type(Default::default());
            let field_ptr = ctx
                .codegen
                .builder
                .build_pointer_cast(field_ptr_i8, attr_ptr_type, "field_typed_ptr")
                .map_err(|e| CodegenError::llvm_verification(e.to_string()))?;

            // 5. Lower the new value.
            let val = lower_expr(ctx, &assign.value)?;

            // 6. Load the old value.
            let attr_llvm_ty = crate::lower::utils::llvm_type(ctx.codegen, ctx.registry, &attr_type)?;
            let old_val = ctx
                .codegen
                .builder
                .build_load(attr_llvm_ty, field_ptr, "old_attr")
                .map_err(|e| CodegenError::llvm_verification(e.to_string()))?;

            // 7. If the attribute is heap-allocated, release the old value and retain the new.
            if is_heap_allocated_type(&attr_type, ctx.registry) {
                let release_fn = ctx
                    .codegen
                    .functions
                    .get("hulk_rt_release")
                    .cloned()
                    .ok_or_else(|| CodegenError::unsupported(
                        "hulk_rt_release not declared",
                        Some(assign.value.span),
                    ))?;
                ctx.codegen
                    .builder
                    .build_call(release_fn, &[old_val.into()], "release_old_attr")
                    .map_err(|e| CodegenError::llvm_verification(e.to_string()))?;

                let retain_fn = ctx
                    .codegen
                    .functions
                    .get("hulk_rt_retain")
                    .cloned()
                    .ok_or_else(|| CodegenError::unsupported(
                        "hulk_rt_retain not declared",
                        Some(assign.value.span),
                    ))?;
                ctx.codegen
                    .builder
                    .build_call(retain_fn, &[val.into()], "retain_new_attr")
                    .map_err(|e| CodegenError::llvm_verification(e.to_string()))?;
            }

            // 8. Store the new value.
            ctx.codegen
                .builder
                .build_store(field_ptr, val)
                .map_err(|e| CodegenError::llvm_verification(e.to_string()))?;

            // 9. Assignment expression returns the value.
            Ok(val)
        }
        AssignTarget::Index { object, index } => {
            // Delegate to the index lowering utility.
            index::lower_index_set(ctx, object, index, &assign.value, assign.value.span)
        }
    }
}