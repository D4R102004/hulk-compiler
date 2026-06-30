//! Lambda expression lowering.
//!
//! WHY: Type::Function uses a uniform { env_ptr, fn_ptr } fat-pointer
//! representation (same ABI as protocols/Iterable — see utils.rs llvm_type).
//! This is the standard "uniform calling convention" (cf. Rust RFC 1558,
//! OCaml): a function with no captures is simply a closure whose env_ptr is
//! null. lower_function_value in call.rs always destructures the same
//! { env_ptr, fn_ptr } shape regardless of whether env_ptr is used.

use hulk_ast::LambdaExpr;
use hulk_semantic::Type;
use inkwell::types::{BasicMetadataTypeEnum, BasicType};
use inkwell::values::BasicValueEnum;

use super::lower_expr;
use crate::error::CodegenError;
use crate::lower::utils::llvm_type;
use crate::lower::LowerCtx;

pub fn lower_lambda<'ctx>(
    ctx: &mut LowerCtx<'_, 'ctx>,
    lambda: &LambdaExpr<Type>,
    lambda_type: &Type,
) -> Result<BasicValueEnum<'ctx>, CodegenError> {
    let (param_types, return_type) = match lambda_type {
        Type::Function {
            params,
            return_type,
        } => (params.as_slice(), return_type.as_ref()),
        _ => {
            return Err(CodegenError::unsupported(
                format!("lambda expression has non-Function type `{}`", lambda_type),
                None,
            ))
        }
    };

    let ptr_type = ctx.codegen.context.ptr_type(Default::default());

    // Build LLVM param types: (ptr env_ignored, user_params...) to match the
    // calling convention that lower_function_value uses when calling through the
    // { env_ptr, fn_ptr } fat pointer.
    let mut llvm_params: Vec<BasicMetadataTypeEnum> = vec![ptr_type.into()];
    for ty in param_types {
        llvm_params.push(llvm_type(ctx.codegen, ctx.registry, ty)?.into());
    }
    let llvm_return = llvm_type(ctx.codegen, ctx.registry, return_type)?;
    let fn_type = llvm_return.fn_type(&llvm_params, false);

    let name = format!("lambda_{}", ctx.codegen.next_lambda_id());
    let lambda_fn = ctx.codegen.module.add_function(&name, fn_type, None);

    // Save the outer function's insertion point so we can restore it after
    // emitting the lambda body.
    let saved_bb = ctx.codegen.builder.get_insert_block();

    let entry_bb = ctx.codegen.context.append_basic_block(lambda_fn, "entry");
    ctx.codegen.builder.position_at_end(entry_bb);

    // Bind user params (index 0 = env_ptr is ignored; user params start at 1).
    ctx.push_scope();
    let param_values = lambda_fn.get_params();
    for (i, param) in lambda.params.iter().enumerate() {
        let param_ty = &param_types[i];
        let param_llvm_ty = llvm_type(ctx.codegen, ctx.registry, param_ty)?;
        let param_val = param_values[i + 1];
        let alloca = ctx
            .codegen
            .builder
            .build_alloca(param_llvm_ty, &param.name)
            .map_err(|e| CodegenError::llvm_verification(e.to_string()))?;
        ctx.codegen
            .builder
            .build_store(alloca, param_val)
            .map_err(|e| CodegenError::llvm_verification(e.to_string()))?;
        ctx.scope_stack
            .declare(&param.name, alloca, param_llvm_ty, param_ty.clone(), false);
    }

    let body_val = lower_expr(ctx, &lambda.body)?;
    ctx.pop_scope()?;

    ctx.codegen
        .builder
        .build_return(Some(&body_val))
        .map_err(|e| CodegenError::llvm_verification(e.to_string()))?;

    // Restore the outer function's insertion point.
    if let Some(bb) = saved_bb {
        ctx.codegen.builder.position_at_end(bb);
    }

    // Return { null_env_ptr, fn_addr } — matches llvm_type(Type::Function).
    let null_ptr = ptr_type.const_null();
    let fn_addr = lambda_fn.as_global_value().as_pointer_value();
    let fat_ptr = ctx
        .codegen
        .context
        .const_struct(&[null_ptr.into(), fn_addr.into()], false);
    Ok(fat_ptr.into())
}
