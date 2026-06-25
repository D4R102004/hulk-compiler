//! Lowering of function and method calls.

use inkwell::values::BasicMetadataValueEnum;
use inkwell::types::BasicType;
use hulk_ast::{CallExpr, MemberExpr};
use hulk_semantic::Type;

use crate::error::CodegenError;
use crate::lower::LowerCtx;
use crate::lower::utils::llvm_type;
use super::lower_expr;

/// Lowers a call expression.
///
/// Handles three cases:
/// 1. Global function call: callee is a `Variable` (free function).
/// 2. Method call: callee is a `Member` expression (`obj.method(args)`).
///    Uses vtable dispatch (load vtable, index, indirect call).
/// 3. `base` call: callee is `BaseRef` – direct call to parent method.
///
/// # Parameters
/// - `ctx`: the lowering context.
/// - `call`: the call AST node.
///
/// # Returns
/// The value returned by the called function.
///
/// # Errors
/// - `Unsupported` if the callee is not a variable name or if the function
///   is not found in the module (should not happen if declared correctly).
/// - `LlvmVerification` if building the call instruction fails.
pub fn lower_call<'ctx>(
    ctx: &mut LowerCtx<'_, 'ctx>,
    call: &CallExpr<Type>,
) -> Result<inkwell::values::BasicValueEnum<'ctx>, CodegenError> {
    match &call.callee.kind {
        // ─── Global function call ──────────────────────────────────────────

        hulk_ast::ExprKind::Variable(name) => {
            if let Some(fn_val) = ctx.codegen.functions.get(name).copied() {
                // Lower arguments.
                let args: Vec<BasicMetadataValueEnum> = call
                    .args
                    .iter()
                    .map(|arg| lower_expr(ctx, arg))
                    .collect::<Result<Vec<_>, _>>()?
                    .into_iter()
                    .map(|val| val.into())
                    .collect();

                let call_site = ctx
                    .codegen
                    .builder
                    .build_call(fn_val, &args, "call")
                    .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;
                let result = call_site
                    .try_as_basic_value()
                    .unwrap_basic();
                return Ok(result);
            }
        }

        // ─── Method call (`obj.method(args)`) ─────────────────────────────

        hulk_ast::ExprKind::Member(member) => {
            return lower_method_call(ctx, member, call);
        }

        // ─── `base` call ──────────────────────────────────────────────────

        hulk_ast::ExprKind::BaseRef => {
            return lower_base_call(ctx, call);
        }

        _ => {}
    }

    // Generic function‑value call (e.g., a variable holding a method reference).
    let callee_val = lower_expr(ctx, &call.callee)?;
    match &call.callee.anno {
        Type::Function { .. } => lower_function_value(ctx, callee_val, &call.callee.anno, &call.args),
        _ => Err(CodegenError::Unsupported {
            construct: format!("unable to resolve call to type `{}`", call.callee.anno),
        }),
    }
}

/// Lowers a method call using vtable dispatch.
fn lower_method_call<'ctx>(
    ctx: &mut LowerCtx<'_, 'ctx>,
    member: &MemberExpr<Type>,
    call: &CallExpr<Type>,
) -> Result<inkwell::values::BasicValueEnum<'ctx>, CodegenError> {
    // 1. Lower the object expression to get a pointer.
    let obj_val = lower_expr(ctx, &member.object)?;
    let obj_ptr = obj_val.into_pointer_value();

    // 2. Determine the static type of the object.
    let obj_type = &member.object.anno;
    let type_name = match obj_type {
        Type::Named(name) => name,
        _ => {
            return Err(CodegenError::Unsupported {
                construct: format!("method call on non‑named type: {:?}", obj_type),
            });
        }
    };

    // 3. Look up the type layout to get the method slot index.
    let layout = ctx
        .codegen
        .type_layouts
        .get(type_name)
        .ok_or_else(|| CodegenError::Unsupported {
            construct: format!("no layout for type '{}'", type_name),
        })?;

    let slot_idx = *layout
        .method_slots
        .get(&member.member)
        .ok_or_else(|| CodegenError::Unsupported {
            construct: format!("method '{}' not found in type '{}'", member.member, type_name),
        })?;

    // 4. Load the vtable pointer from the object header (field index 3).
    let i32_type = ctx.codegen.context.i32_type();
    let ptr_type = ctx.codegen.context.ptr_type(Default::default());
    let vtable_ptr_ptr = unsafe {
        ctx.codegen
            .builder
            .build_gep(
                layout.struct_ty,
                obj_ptr,
                &[i32_type.const_int(0, false), i32_type.const_int(3, false)],
                "vtable_ptr_ptr",
            )
            .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?
    };
    let vtable_ptr = ctx
        .codegen
        .builder
        .build_load(ptr_type, vtable_ptr_ptr, "vtable_ptr")
        .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?
        .into_pointer_value();

    // 5. Load the function pointer from the vtable at the slot index.
    let slot_val = ctx.codegen.context.i32_type().const_int(slot_idx as u64, false);
    let fn_ptr_ptr = unsafe {
        ctx.codegen
            .builder
            .build_gep(ptr_type, vtable_ptr, &[slot_val.into()], "fn_ptr_ptr")
            .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?
    };
    let fn_ptr = ctx
        .codegen
        .builder
        .build_load(ptr_type, fn_ptr_ptr, "fn_ptr")
        .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?
        .into_pointer_value();

    // 6. Prepare arguments: first argument is `self` (the object pointer), then the rest.
    let mut args = Vec::new();
    args.push(obj_ptr.into());
    for arg in &call.args {
        args.push(lower_expr(ctx, arg)?.into());
    }

    // 7. Build an indirect call.
    // Retrieve the method's declared function type from the module.
    let qualified_name = format!("{}::{}", type_name, member.member);
    let fn_decl = ctx
        .codegen
        .functions
        .get(&qualified_name)
        .cloned()
        .ok_or_else(|| {
            CodegenError::Unsupported {
                construct: format!("method '{}' not declared", qualified_name),
            }
        })?;
    let fn_type = fn_decl.get_type();

    // Build the indirect call. The function pointer is already the correct type.
    let call_site = ctx
        .codegen
        .builder
        .build_indirect_call(fn_type, fn_ptr, &args, "method_call")
        .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;
    let result = call_site
        .try_as_basic_value()
        .unwrap_basic();
    Ok(result)
}

/// Lowers a `base` call (direct call to the parent's method).
fn lower_base_call<'ctx>(
    ctx: &mut LowerCtx<'_, 'ctx>,
    call: &CallExpr<Type>,
) -> Result<inkwell::values::BasicValueEnum<'ctx>, CodegenError> {
    let (current_type, current_method) = match (&ctx.current_type, &ctx.current_method) {
        (Some(ty), Some(meth)) => (ty, meth),
        _ => {
            return Err(CodegenError::Unsupported {
                construct: "base call outside of an overriding method".into(),
            });
        }
    };

    // Look up the parent type.
    let parent_name = ctx
        .registry
        .parent_of(current_type)
        .ok_or_else(|| {
            CodegenError::Unsupported {
                construct: format!("type '{}' has no parent", current_type),
            }
        })?;

    // Look up the parent method signature.
    let parent_info = ctx
        .registry
        .lookup_type(&parent_name)
        .ok_or_else(|| {
            CodegenError::Unsupported {
                construct: format!("parent type '{}' not found", parent_name),
            }
        })?;
    if !parent_info.methods.contains_key(current_method) {
        return Err(CodegenError::Unsupported {
            construct: format!("method '{}' not found in parent type '{}'", current_method, parent_name),
        });
    }

    // Get the function from the module using the qualified name.
    let qualified_name = format!("{}::{}", parent_name, current_method);
    let fn_val = ctx
        .codegen
        .functions
        .get(&qualified_name)
        .cloned()
        .ok_or_else(|| {
            CodegenError::Unsupported {
                construct: format!("parent method '{}' not declared", qualified_name),
            }
        })?;

    // Prepare arguments: `self` is the first argument. We need to load `self` from the scope.
    let self_ptr = ctx
        .scope_stack
        .lookup("self")
        .ok_or_else(|| {
            CodegenError::Unsupported {
                construct: "self not in scope".into(),
            }
        })?
        .0;
    let mut args = Vec::new();
    args.push(self_ptr.into());
    for arg in &call.args {
        args.push(lower_expr(ctx, arg)?.into());
    }

    // Build a direct call.
    let call_site = ctx
        .codegen
        .builder
        .build_call(fn_val, &args, "base_call")
        .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;
    let result = call_site
        .try_as_basic_value()
        .unwrap_basic();
    Ok(result)
}

/// Lowers a call to a function‑typed value (fat pointer `{ self_ptr, fn_ptr }`).
///
/// # Parameters
/// - `ctx`: the lowering context.
/// - `callee_val`: the LLVM value of the callee (must be a struct of two pointers).
/// - `callee_ty`: the static type of the callee, expected to be `Type::Function`.
/// - `args`: the AST arguments to the call.
///
/// # Returns
/// The value returned by the called function.
fn lower_function_value<'ctx>(
    ctx: &mut LowerCtx<'_, 'ctx>,
    callee_val: inkwell::values::BasicValueEnum<'ctx>,
    callee_ty: &Type,
    args: &[hulk_ast::Expr<Type>],
) -> Result<inkwell::values::BasicValueEnum<'ctx>, CodegenError> {
    // Extract the function type details.
    let (param_types, return_type) = match callee_ty {
        Type::Function { params, return_type } => (params, return_type.as_ref()),
        _ => {
            return Err(CodegenError::Unsupported {
                construct: "expected function type".into(),
            });
        }
    };

    // Ensure the callee value is a struct of two pointers (fat pointer).
    let struct_val = callee_val.into_struct_value();

    // Extract self_ptr and fn_ptr using `build_extract_value`.
    let self_ptr = ctx
        .codegen
        .builder
        .build_extract_value(struct_val, 0, "self_ptr")
        .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?
        .into_pointer_value();
    let fn_ptr = ctx
        .codegen
        .builder
        .build_extract_value(struct_val, 1, "fn_ptr")
        .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?
        .into_pointer_value();

    // Build the function type for the indirect call.
    let llvm_param_types: Vec<inkwell::types::BasicMetadataTypeEnum> = param_types
        .iter()
        .map(|ty| llvm_type(ctx.codegen, ty).map(|t| t.into()))
        .collect::<Result<Vec<_>, _>>()?;
    let llvm_return = llvm_type(ctx.codegen, return_type)?;

    // Method function pointers expect `self` as the first parameter.
    let self_ptr_type = ctx.codegen.context.ptr_type(Default::default());
    let mut all_param_types = vec![self_ptr_type.into()];
    all_param_types.extend(llvm_param_types);
    let fn_type = llvm_return.fn_type(&all_param_types, false);

    // Cast fn_ptr to the correct function pointer type.
    let fn_ptr_typed = ctx
        .codegen
        .builder
        .build_pointer_cast(fn_ptr, ctx.codegen.context.ptr_type(Default::default()), "fn_ptr_typed") 
        .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;

    // Prepare arguments: first self_ptr, then the rest.
    let mut call_args = Vec::new();
    call_args.push(self_ptr.into());
    for arg_expr in args {
        call_args.push(lower_expr(ctx, arg_expr)?.into());
    }

    // Build indirect call.
    let call_site = ctx
        .codegen
        .builder
        .build_indirect_call(fn_type, fn_ptr_typed, &call_args, "call_fat_ptr")
        .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;
    let result = call_site.try_as_basic_value().unwrap_basic();
    Ok(result)
}