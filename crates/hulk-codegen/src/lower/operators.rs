//! Lowering of unary and binary operators to LLVM IR.
//!
//! This module handles all HULK operators:
//! - Unary: `-` (negate) and `!` (logical not)
//! - Binary: arithmetic (`+`, `-`, `*`, `/`, `%`, `^`),
//!   comparisons (`==`, `!=`, `<`, `<=`, `>`, `>=`),
//!   logical (`&`, `|`), and concatenation (`@`, `@@`).
//!
//! All numeric operations are performed on 64‑bit floating‑point values
//! (`f64`). Booleans are represented as `i1` (1‑bit integers). Strings are
//! pointers to a `HulkString` struct.
//!
//! Concatenation automatically stringifies `Number` and `Boolean` operands
//! via calls to the runtime library (`hulk_rt_number_to_string` and
//! `hulk_rt_bool_to_string`). The `@@` operator inserts a literal space
//! between the two operands.

use inkwell::FloatPredicate;
use hulk_ast::{BinaryOp, UnaryExpr, BinaryExpr, UnaryOp};
use hulk_semantic::Type;

use crate::error::CodegenError;
use crate::lower::LowerCtx;
use crate::runtime_decls::{declare_string_concat, declare_string_concat_space};
use crate::lower::utils::{cmp_float, to_string};
use super::lower_expr;

/// Lowers a unary expression.
///
/// # Supported operators
/// - `Negate` (`-`): negates a floating‑point number.
/// - `Not` (`!`): logical negation of a boolean.
///
/// # Errors
/// - If the operand is not of the expected type (float for negate, bool for not).
/// - If LLVM instruction emission fails.
pub fn lower_unary<'ctx>(
    ctx: &mut LowerCtx<'_, 'ctx>,
    unary: &UnaryExpr<Type>,
) -> Result<inkwell::values::BasicValueEnum<'ctx>, CodegenError> {
    let operand = lower_expr(ctx, &unary.expr)?;

    match unary.op {
        UnaryOp::Negate => {
            let float_val = operand
                .into_float_value();
            let neg = ctx
                .codegen
                .builder
                .build_float_neg(float_val, "neg")
                .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;
            Ok(neg.into())
        }
        UnaryOp::Not => {
            let int_val = operand
                .into_int_value();
            let not = ctx
                .codegen
                .builder
                .build_not(int_val, "not")
                .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;
            Ok(not.into())
        }
    }
}

/// Lowers a binary expression.
///
/// # Supported operators
/// - **Arithmetic**: `Add`, `Subtract`, `Multiply`, `Divide`, `Modulo`, `Power`
///   – all operate on `f64` values.
/// - **Comparisons**: `Equal`, `NotEqual`, `Less`, `LessEqual`, `Greater`,
///   `GreaterEqual` – compare two `f64` values and return `i1` (boolean).
/// - **Logical**: `And`, `Or` – operate on `i1` booleans.
/// - **Concatenation**: `Concat` (`@`) and `ConcatSpace` (`@@`) – stringify
///   operands and concatenate them (with or without a space).
///
/// # Parameters
/// - `binary`: the binary expression node.
/// - `result_type`: the static type of the whole expression (used for
///   future boxing, but currently ignored in Phase 3).
///
/// # Errors
/// - If an operand is not of the expected type for the operator.
/// - If an LLVM intrinsic or runtime function call fails.
pub fn lower_binary<'ctx>(
    ctx: &mut LowerCtx<'_, 'ctx>,
    binary: &BinaryExpr<Type>,
    _result_type: &Type, // Reserved for future boxing logic.
) -> Result<inkwell::values::BasicValueEnum<'ctx>, CodegenError> {
    let left = lower_expr(ctx, &binary.left)?;
    let right = lower_expr(ctx, &binary.right)?;

    let result = match binary.op {
        // ─── Arithmetic operators ────────────────────────────────────────

        BinaryOp::Add => {
            let lf = left.into_float_value();
            let rf = right.into_float_value();
            ctx.codegen.builder.build_float_add(lf, rf, "add")
                .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?
                .into()
        }
        BinaryOp::Subtract => {
            let lf = left.into_float_value();
            let rf = right.into_float_value();
            ctx.codegen.builder.build_float_sub(lf, rf, "sub")
                .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?
                .into()
        }
        BinaryOp::Multiply => {
            let lf = left.into_float_value();
            let rf = right.into_float_value();
            ctx.codegen.builder.build_float_mul(lf, rf, "mul")
                .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?
                .into()
        }
        BinaryOp::Divide => {
            let lf = left.into_float_value();
            let rf = right.into_float_value();
            ctx.codegen.builder.build_float_div(lf, rf, "div")
                .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?
                .into()
        }
        BinaryOp::Modulo => {
            // LLVM has no direct `fmod` instruction; call the `llvm.fmod.f64` intrinsic.
            let lf = left.into_float_value();
            let rf = right.into_float_value();
            let fmod_fn = ctx.codegen.module.get_function("llvm.fmod.f64")
                .unwrap_or_else(|| {
                    let f64_type = ctx.codegen.context.f64_type();
                    let fn_type = f64_type.fn_type(&[f64_type.into(), f64_type.into()], false);
                    ctx.codegen.module.add_function("llvm.fmod.f64", fn_type, None)
                });
            let call = ctx.codegen.builder.build_call(fmod_fn, &[lf.into(), rf.into()], "mod")
                .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?
                .try_as_basic_value().unwrap_basic();
            call.into()
        }
        BinaryOp::Power => {
            // Call `llvm.pow.f64` intrinsic.
            let lf = left.into_float_value();
            let rf = right.into_float_value();
            let pow_fn = ctx.codegen.module.get_function("llvm.pow.f64")
                .unwrap_or_else(|| {
                    let f64_type = ctx.codegen.context.f64_type();
                    let fn_type = f64_type.fn_type(&[f64_type.into(), f64_type.into()], false);
                    ctx.codegen.module.add_function("llvm.pow.f64", fn_type, None)
                });
            let call = ctx.codegen.builder.build_call(pow_fn, &[lf.into(), rf.into()], "pow")
                .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?
                .try_as_basic_value().unwrap_basic();
            call.into()
        }

        // ─── Comparison operators ──────────────────────────────────────

        BinaryOp::Equal => {
            cmp_float(ctx, FloatPredicate::OEQ, left, right, "eq")?
        }
        BinaryOp::NotEqual => {
            cmp_float(ctx, FloatPredicate::ONE, left, right, "ne")?
        }
        BinaryOp::Less => {
            cmp_float(ctx, FloatPredicate::OLT, left, right, "lt")?
        }
        BinaryOp::LessEqual => {
            cmp_float(ctx, FloatPredicate::OLE, left, right, "le")?
        }
        BinaryOp::Greater => {
            cmp_float(ctx, FloatPredicate::OGT, left, right, "gt")?
        }
        BinaryOp::GreaterEqual => {
            cmp_float(ctx, FloatPredicate::OGE, left, right, "ge")?
        }

        // ─── Logical operators ─────────────────────────────────────────

        BinaryOp::And => {
            let li = left.into_int_value();
            let ri = right.into_int_value();
            ctx.codegen.builder.build_and(li, ri, "and")
                .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?
                .into()
        }
        BinaryOp::Or => {
            let li = left.into_int_value();
            let ri = right.into_int_value();
            ctx.codegen.builder.build_or(li, ri, "or")
                .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?
                .into()
        }

        // ─── Concatenation operators ──────────────────────────────────

        BinaryOp::Concat | BinaryOp::ConcatSpace => {
            // Auto‑stringify operands.
            let left_str = to_string(ctx, left, &binary.left.anno)?;
            let right_str = to_string(ctx, right, &binary.right.anno)?;
            let concat_fn = if matches!(binary.op, BinaryOp::Concat) {
                declare_string_concat(ctx.codegen)
            } else {
                declare_string_concat_space(ctx.codegen)
            };
            let call = ctx.codegen.builder.build_call(concat_fn, &[left_str.into(), right_str.into()], "concat")
                .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?
                .try_as_basic_value()
                .unwrap_basic();
            call.into()
        }
    };

    Ok(result)
}