//! Lowering of function and method calls.

use inkwell::values::BasicMetadataValueEnum;
use hulk_ast::CallExpr;
use hulk_semantic::Type;

use crate::error::CodegenError;
use crate::lower::LowerCtx;
use super::lower_expr;

/// Lowers a function call expression.
///
/// This handles calls to user‑defined global functions. The callee must be a
/// `Variable` expression whose name matches a function declared in the module.
/// If the callee is a `Member` expression (method call), this returns
/// `Unsupported` (method calls are handled in Phase 5).
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
    // Only handle calls where the callee is a variable (function name).
    if let hulk_ast::ExprKind::Variable(name) = &call.callee.kind {
        // Look up the function in the module.
        if let Some(fn_val) = ctx.codegen.functions.get(name).copied() {
            // Lower all arguments and convert to metadata values.
            let args: Vec<BasicMetadataValueEnum> = call
                .args
                .iter()
                .map(|arg| lower_expr(ctx, arg))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .map(|val| val.into())
                .collect();

            // Build the call instruction.
            let call_site = ctx
                .codegen
                .builder
                .build_call(fn_val, &args, "call")
                .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;

            // Extract the return value. HULK functions always return a value,
            // so `unwrap_basic()` is safe.
            let result = call_site
                .try_as_basic_value()
                .unwrap_basic();
            
            return Ok(result);
        }
    }

    // If we reach here, the callee is not a simple variable function call.
    // (Method calls are deferred to Phase 5.)
    Err(CodegenError::Unsupported {
        construct: "call to non‑function or unsupported callee".into(),
    })
}