//! Shared helper functions used by multiple lowering submodules.

use inkwell::FloatPredicate;
use hulk_semantic::Type;

use crate::error::CodegenError;
use crate::lower::LowerCtx;
use crate::runtime_decls;

/// Compares two floating-point values.
pub fn cmp_float<'ctx>(
    ctx: &mut LowerCtx<'_, 'ctx>,
    pred: FloatPredicate,
    left: inkwell::values::BasicValueEnum<'ctx>,
    right: inkwell::values::BasicValueEnum<'ctx>,
    name: &str,
) -> Result<inkwell::values::BasicValueEnum<'ctx>, CodegenError> {
    let lf = left.into_float_value();
    let rf = right.into_float_value();
    let cmp = ctx.codegen.builder.build_float_compare(pred, lf, rf, name)
        .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;
    Ok(cmp.into())
}

/// Converts a value to a string pointer.
pub fn to_string<'ctx>(
    ctx: &mut LowerCtx<'_, 'ctx>,
    val: inkwell::values::BasicValueEnum<'ctx>,
    ty: &Type,
) -> Result<inkwell::values::PointerValue<'ctx>, CodegenError> {
    match ty {
        Type::String => Ok(val.into_pointer_value()),
        Type::Number => {
            let f = val.into_float_value();
            let fn_val = runtime_decls::declare_number_to_string(ctx.codegen);
            let call = ctx.codegen.builder.build_call(fn_val, &[f.into()], "num_to_str")
                .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?
                .try_as_basic_value()
                .unwrap_basic();
            Ok(call.into_pointer_value())
        }
        Type::Boolean => {
            let b = val.into_int_value();
            let fn_val = runtime_decls::declare_bool_to_string(ctx.codegen);
            let call = ctx.codegen.builder.build_call(fn_val, &[b.into()], "bool_to_str")
                .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?
                .try_as_basic_value()
                .unwrap_basic();
            Ok(call.into_pointer_value())
        }
        _ => Err(CodegenError::Unsupported {
            construct: format!("cannot convert type {} to string", ty)
        })
    }
}

/// Boxes a value if the target type is `Object`.
///
/// Currently a placeholder; full implementation in Phase 4/5.
pub fn box_if_needed<'ctx>(
    _ctx: &mut LowerCtx<'_, 'ctx>,
    val: inkwell::values::BasicValueEnum<'ctx>,
    _src_ty: &Type,
    _target_ty: &Type,
) -> Result<inkwell::values::BasicValueEnum<'ctx>, CodegenError> {
    // TODO: Implement boxing for Object-typed targets.
    Ok(val)
}

/// Maps a HULK type to an LLVM type (Phase 3 subset).
pub fn llvm_type<'ctx>(
    ctx: &mut LowerCtx<'_, 'ctx>,
    ty: &Type,
) -> Result<inkwell::types::BasicTypeEnum<'ctx>, CodegenError> {
    match ty {
        Type::Number => Ok(ctx.codegen.context.f64_type().into()),
        Type::Boolean => Ok(ctx.codegen.context.bool_type().into()),
        Type::String => Ok(ctx.codegen.context.ptr_type(Default::default()).into()),
        Type::Object => Ok(ctx.codegen.context.ptr_type(Default::default()).into()),
        _ => Err(CodegenError::Unsupported {
            construct: format!("type {} not supported in Phase 3", ty)
        })
    }
}