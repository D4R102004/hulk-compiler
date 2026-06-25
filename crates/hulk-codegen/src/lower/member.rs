//! Lowering of member access: attributes and method references.

use hulk_ast::MemberExpr;
use hulk_semantic::Type;

use crate::error::CodegenError;
use crate::lower::LowerCtx;
use super::lower_expr;

/// Lowers a member access expression.
///
/// This handles attribute reads (`obj.attr`). If the member is a method,
/// it returns `Unsupported` (method references are handled in `call.rs`).
pub fn lower_member<'ctx>(
    ctx: &mut LowerCtx<'_, 'ctx>,
    member: &MemberExpr<Type>,
) -> Result<inkwell::values::BasicValueEnum<'ctx>, CodegenError> {
    // 1. Lower the object expression to get the object pointer.
    let obj_val = lower_expr(ctx, &member.object)?;
    let obj_ptr = obj_val.into_pointer_value();

    // 2. Determine the static type of the object.
    let obj_type = &member.object.anno;

    // 3. Resolve the attribute.
    let (attr_type, offset) = resolve_attribute(ctx, obj_type, &member.member)?;

    // 4. Compute the field address using byte offset.
    let ptr_type = ctx.codegen.context.ptr_type(Default::default());
    let obj = ctx
        .codegen
        .builder
        .build_pointer_cast(obj_ptr, ptr_type, "obj_i8")
        .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;

    let offset_val = ctx.codegen.context.i64_type().const_int(offset as u64, false);
    // Use safe build_gep instead of unsafe build_in_bounds_gep.
    let field_ptr_i8 = unsafe {
        ctx.codegen
            .builder
            .build_gep(ptr_type, obj, &[offset_val.into()], "field_ptr")
            .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?
    };
    // 5. Cast to the attribute's type pointer.
    let attr_llvm_ty = crate::lower::utils::llvm_type(ctx.codegen, &attr_type)?;
    let attr_ptr_type = ctx.codegen.context.ptr_type(Default::default());
    let field_ptr = ctx
        .codegen
        .builder
        .build_pointer_cast(field_ptr_i8, attr_ptr_type, "field_typed_ptr")
        .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;

    // 6. Load and return the attribute value.
    let val = ctx
        .codegen
        .builder
        .build_load(attr_llvm_ty, field_ptr, "attr_load")
        .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;

    Ok(val)
}

/// Resolves an attribute access: returns the attribute's type and byte offset.
fn resolve_attribute(
    ctx: &LowerCtx<'_, '_>,
    obj_type: &Type,
    member_name: &str,
) -> Result<(Type, usize), CodegenError> {
    // The object must be a named type.
    let type_name = match obj_type {
        Type::Named(name) => name,
        _ => {
            return Err(CodegenError::Unsupported {
                construct: format!("member access on non‑named type: {:?}", obj_type),
            });
        }
    };

    // Look up the type in the registry.
    let info = ctx
        .registry
        .lookup_type(type_name)
        .ok_or_else(|| CodegenError::Unsupported {
            construct: format!("type '{}' not found", type_name),
        })?;

    // Check if the member is an attribute.
    // The registry's `attributes` map is flattened after Pass 1, so it includes
    // inherited attributes as well.
    let attr_info = info
        .attributes
        .get(member_name)
        .ok_or_else(|| CodegenError::Unsupported {
            construct: format!("attribute '{}' not found in type '{}'", member_name, type_name),
        })?;

    let attr_type = attr_info
        .declared_type
        .as_ref()
        .ok_or_else(|| {
            CodegenError::LlvmVerification(format!("attribute '{}' has no declared type", member_name))
        })?
        .clone();

    // Get the byte offset from the type layout.
    let layout = ctx
        .codegen
        .type_layouts
        .get(type_name)
        .ok_or_else(|| CodegenError::Unsupported {
            construct: format!("no layout for type '{}'", type_name),
        })?;

    let offset = *layout
        .field_offsets
        .get(member_name)
        .ok_or_else(|| CodegenError::Unsupported {
            construct: format!("no offset for attribute '{}'", member_name),
        })?;

    Ok((attr_type, offset))
}