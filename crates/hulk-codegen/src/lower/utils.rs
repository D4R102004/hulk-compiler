//! Shared helper functions used by multiple lowering submodules.

use inkwell::FloatPredicate;
use inkwell::values::BasicValueEnum;
use hulk_ast::TypeRef;
use hulk_semantic::{Type, TypeRegistry};

use crate::error::CodegenError;
use crate::lower::LowerCtx;
use crate::CodegenCtx;
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

/// Maps a HULK type to an LLVM type.
pub fn llvm_type<'ctx>(
    codegen: &CodegenCtx<'ctx>,
    registry: &TypeRegistry,
    ty: &Type,
) -> Result<inkwell::types::BasicTypeEnum<'ctx>, CodegenError> {
    match ty {
        Type::Number => Ok(codegen.context.f64_type().into()),
        Type::Boolean => Ok(codegen.context.bool_type().into()),
        Type::String | Type::Object | Type::Vector(_) => {
            Ok(codegen.context.ptr_type(Default::default()).into())
        }
        // Protocols and Iterable(T) share the same fat-pointer representation (§6.6).
        Type::Function { .. } | Type::Iterable(_) => {
            let ptr_type = codegen.context.ptr_type(Default::default());
            Ok(codegen.context.struct_type(&[ptr_type.into(), ptr_type.into()], false).into())
        }
        Type::Named(_) if registry.is_protocol(ty) => {
            let ptr_type = codegen.context.ptr_type(Default::default());
            Ok(codegen.context.struct_type(&[ptr_type.into(), ptr_type.into()], false).into())
        }
        Type::Named(_) => Ok(codegen.context.ptr_type(Default::default()).into()),
        _ => Err(CodegenError::Unsupported {
            construct: format!("type {} not supported", ty)
        })
    }
}

/// Returns the type and byte offset of an attribute, or an error.
pub fn resolve_attribute_with_offset(
    ctx: &LowerCtx<'_, '_>,
    obj_type: &Type,
    member_name: &str,
) -> Result<(Type, usize), CodegenError> {
    let type_name = match obj_type {
        Type::Named(name) => name,
        _ => {
            return Err(CodegenError::Unsupported {
                construct: format!("attribute access on non‑named type: {:?}", obj_type),
            });
        }
    };
    let info = ctx
        .registry
        .lookup_type(type_name)
        .ok_or_else(|| CodegenError::Unsupported {
            construct: format!("type '{}' not found", type_name),
        })?;
    let attr_info = info
        .attributes
        .get(member_name)
        .ok_or_else(|| CodegenError::Unsupported {
            construct: format!("attribute '{}' not found", member_name),
        })?;
    let attr_type = attr_info
        .declared_type
        .as_ref()
        .ok_or_else(|| {
            CodegenError::LlvmVerification(format!("attribute '{}' has no declared type", member_name))
        })?
        .clone();
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

/// Returns `true` if values of `ty` are stored as heap pointers and must be
/// reference‑counted.
pub fn is_heap_allocated_type(ty: &Type, _registry: &TypeRegistry) -> bool {
    match ty {
        Type::String | Type::Object | Type::Vector(_) | Type::Iterable(_) => true,
        Type::Named(_) => true,
        _ => false,
    }
}

/// Converts a concrete object pointer to a protocol fat pointer.
///
/// The fat pointer is a struct `{ data: ptr, itable: ptr }`.
///
/// # Parameters
/// - `ctx`: The lowering context.
/// - `value`: The concrete object pointer.
/// - `concrete_ty`: The static type of the concrete object.
/// - `protocol_ty`: The target protocol type.
///
/// # Returns
/// A `BasicValueEnum` containing the fat pointer struct.
pub fn convert_to_protocol<'ctx>(
    ctx: &mut LowerCtx<'_, 'ctx>,
    value: BasicValueEnum<'ctx>,
    concrete_ty: &Type,
    protocol_ty: &Type,
) -> Result<BasicValueEnum<'ctx>, CodegenError> {
    let obj_ptr = value.into_pointer_value();

    let protocol_name = match protocol_ty {
        Type::Named(name) => name,
        Type::Iterable(_) => "Iterable",
        _ => {
            return Err(CodegenError::Unsupported {
                construct: format!("cannot convert to protocol type: {}", protocol_ty),
            });
        }
    };

    let concrete_name = match concrete_ty {
        Type::Named(name) => name,
        _ => {
            return Err(CodegenError::Unsupported {
                construct: format!("cannot convert from non-named type: {}", concrete_ty),
            });
        }
    };

    // Look up the itable.
    let itable_global = ctx
        .codegen
        .itables
        .get(&(concrete_name.clone(), (*protocol_name).to_string()))
        .ok_or_else(|| {
            CodegenError::Unsupported {
                construct: format!("itable not found for ({}, {})", concrete_name, protocol_name),
            }
        })?;
    let itable_ptr = itable_global.as_pointer_value();

    // Build the fat pointer struct.
    let ptr_type = ctx.codegen.context.ptr_type(Default::default());
    let fat_ptr_ty = ctx.codegen.context.struct_type(&[ptr_type.into(), ptr_type.into()], false);
    let fat_ptr_alloca = ctx
        .codegen
        .builder
        .build_alloca(fat_ptr_ty, "fat_ptr")
        .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;

    let zero = ctx.codegen.context.i32_type().const_int(0, false);
    let one = ctx.codegen.context.i32_type().const_int(1, false);

    // Store data pointer at index 0.
    let data_ptr_ptr = unsafe {
        ctx.codegen
            .builder
            .build_gep(fat_ptr_ty, fat_ptr_alloca, &[zero, zero], "data_ptr_ptr")
            .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?
    };
    ctx.codegen
        .builder
        .build_store(data_ptr_ptr, obj_ptr)
        .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;

    // Store itable pointer at index 1.
    let itable_ptr_ptr = unsafe {
        ctx.codegen
            .builder
            .build_gep(fat_ptr_ty, fat_ptr_alloca, &[zero, one], "itable_ptr_ptr")
            .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?
    };
    ctx.codegen
        .builder
        .build_store(itable_ptr_ptr, itable_ptr)
        .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;

    // Load the fat pointer.
    let fat_ptr = ctx
        .codegen
        .builder
        .build_load(fat_ptr_ty, fat_ptr_alloca, "fat_ptr_val")
        .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;
    Ok(fat_ptr)
}
 
/// Attempts to resolve a syntactic `TypeRef` to a semantic `Type` using the registry.
///
/// This is a simplified version of the resolver used in the inference pass.
/// It only needs to determine if the type is a protocol, so it handles
/// builtins and user‑defined types by name.
pub fn resolve_type_ref_to_type(tr: &TypeRef, registry: &TypeRegistry) -> Type {
    match tr.name.as_str() {
        "Number" => Type::Number,
        "String" => Type::String,
        "Boolean" => Type::Boolean,
        "Object" => Type::Object,
        "Vector" if !tr.args.is_empty() => {
            // We don't need the inner type for protocol detection.
            Type::Vector(Box::new(Type::Unknown))
        }
        "Iterable" if !tr.args.is_empty() => {
            Type::Iterable(Box::new(Type::Unknown))
        }
        name => {
            // It could be a user‑defined type or protocol.
            if registry.types.contains_key(name) || registry.protocols.contains_key(name) {
                Type::Named(name.to_string())
            } else {
                Type::Unknown
            }
        }
    }
}

/// Boxes a primitive value into a `HulkBox` heap object.
///
/// # Parameters
/// - `ctx`: The lowering context.
/// - `value`: The primitive value to box.
/// - `ty`: The semantic type of the value (must be `Number` or `Boolean`).
///
/// # Returns
/// A pointer to the newly allocated box.
pub fn box_primitive<'ctx>(
    ctx: &mut LowerCtx<'_, 'ctx>,
    value: BasicValueEnum<'ctx>,
    ty: &Type,
) -> Result<BasicValueEnum<'ctx>, CodegenError> {
    let alloc_fn = ctx
        .codegen
        .functions
        .get("hulk_rt_alloc")
        .cloned()
        .ok_or_else(|| CodegenError::Unsupported {
            construct: "hulk_rt_alloc not declared".into(),
        })?;

    // Allocate 16 bytes (tag + padding + payload)
    let size = ctx.codegen.context.i64_type().const_int(16, false);
    let call = ctx
        .codegen
        .builder
        .build_call(alloc_fn, &[size.into()], "box_alloc")
        .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;
    let box_ptr = call
        .try_as_basic_value()
        .basic()
        .unwrap()
        .into_pointer_value();

    // Store tag
    let tag = match ty {
        Type::Number => 0,
        Type::Boolean => 1,
        _ => return Err(CodegenError::Unsupported {
            construct: format!("boxing of type {} not supported", ty),
        }),
    };
    let tag_ptr = unsafe {
        ctx.codegen
            .builder
            .build_gep(
                ctx.codegen.context.i8_type(),
                box_ptr,
                &[ctx.codegen.context.i32_type().const_int(0, false)],
                "tag_ptr",
            )
            .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?
    };
    ctx.codegen
        .builder
        .build_store(tag_ptr, ctx.codegen.context.i8_type().const_int(tag, false))
        .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;

    // Store payload at offset 8
    let payload_ptr = unsafe {
        ctx.codegen
            .builder
            .build_gep(
                ctx.codegen.context.i8_type(),
                box_ptr,
                &[ctx.codegen.context.i32_type().const_int(8, false)],
                "payload_ptr",
            )
            .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?
    };

    match ty {
        Type::Number => {
            let float_val = value.into_float_value();
            let payload_typed_ptr = ctx.codegen.builder.build_pointer_cast(
                payload_ptr,
                ctx.codegen.context.ptr_type(Default::default()),
                "payload_typed_ptr",
            ).map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;
            ctx.codegen
                .builder
                .build_store(payload_typed_ptr, float_val)
                .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;
        }
        Type::Boolean => {
            let int_val = value.into_int_value();
            let ext = ctx.codegen.builder.build_int_z_extend(
                int_val,
                ctx.codegen.context.i64_type(),
                "bool_ext",
            ).map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;
            let payload_typed_ptr = ctx.codegen.builder.build_pointer_cast(
                payload_ptr,
                ctx.codegen.context.ptr_type(Default::default()),
                "payload_typed_ptr",
            ).map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;
            ctx.codegen
                .builder
                .build_store(payload_typed_ptr, ext)
                .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;
        }
        _ => unreachable!(),
    }

    Ok(box_ptr.into())
}

/// Ensures a value is boxed if the target type is a pointer type (`Object` or `Vector` element).
///
/// This is the single entry point for boxing at dynamic boundaries.
/// It delegates to `box_primitive` when boxing is required.
pub fn ensure_boxed<'ctx>(
    ctx: &mut LowerCtx<'_, 'ctx>,
    value: BasicValueEnum<'ctx>,
    src_ty: &Type,
    target_ty: &Type,
) -> Result<BasicValueEnum<'ctx>, CodegenError> {
    if matches!(target_ty, Type::Object) && matches!(src_ty, Type::Number | Type::Boolean) {
        return box_primitive(ctx, value, src_ty);
    }
    Ok(value)
}

// src/lower/utils.rs — new shared helper
pub fn is_protocol_or_iterable(ty: &Type, registry: &TypeRegistry) -> bool {
    match ty {
        Type::Named(_) => registry.is_protocol(ty),
        Type::Iterable(_) => true,
        _ => false,
    }
}