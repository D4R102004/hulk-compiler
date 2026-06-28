//! Lowering of literal expressions to LLVM constants.
//!
//! This module handles the three kinds of literals in HULK:
//! - `Number`: 64‑bit floating‑point constants (`f64`).
//! - `Boolean`: `true` and `false` as `i1` (1‑bit integer) values.
//! - `String`: emitted as a pair of global constants:
//!   1. A global byte array containing the string data.
//!   2. A global `HulkString` struct (full layout matching `hulk-rt::HulkString`, with `ref_count = -1` immortal sentinel) referencing that data.
//!
//! String literals are immutable and are never modified at runtime; they are
//! emitted once per unique string in the module.

use inkwell::module::Linkage;

use hulk_ast::Literal;

use crate::error::CodegenError;
use crate::lower::LowerCtx;

/// Lowers a literal expression to an LLVM constant value.
///
/// # Parameters
/// - `ctx`: the lowering context.
/// - `lit`: the literal AST node.
///
/// # Returns
/// - `Number`: an `f64` constant.
/// - `Boolean`: an `i1` constant (1 for `true`, 0 for `false`).
/// - `String`: a pointer to a global `HulkString` struct.
///
/// # Errors
/// This function never fails for valid literals; all errors are from LLVM
/// constant construction (which is infallible in this context).
pub fn lower_literal<'ctx>(
    ctx: &mut LowerCtx<'_, 'ctx>,
    lit: &Literal,
) -> Result<inkwell::values::BasicValueEnum<'ctx>, CodegenError> {
    match lit {
        Literal::Number(n) => {
            let float = ctx.codegen.context.f64_type().const_float(*n);
            Ok(float.into())
        }
        Literal::Boolean(b) => {
            let bool_val = ctx.codegen.context.bool_type().const_int(if *b { 1 } else { 0 }, false);
            Ok(bool_val.into())
        }
        Literal::String(s) => {
            // ─── String data global ──────────────────────────────────────
            let id = ctx.codegen.next_string_literal_id();
            let data_bytes = s.as_bytes();
            let data_const = ctx.codegen.context.const_string(data_bytes, false);
            let data_global = ctx.codegen.module.add_global(
                data_const.get_type(), None, &format!("str_data_{id}"));
            data_global.set_initializer(&data_const);
            data_global.set_constant(true);
            data_global.set_linkage(Linkage::Private);
            data_global.set_unnamed_addr(true);

            // ─── HulkString header global ───────────────────────────────
            // WHY: layout must match HulkString in hulk-rt exactly:
            //   ObjHeader { i64 ref_count, u8 gc_mark, u8 type_tag, [6 x i8] pad, ptr next, ptr vtable }
            //   followed by i64 len, ptr data  (offsets 32 and 40).
            // ref_count = -1 is the immortal sentinel; hulk_rt_retain/release check it
            // before any write, so the global can remain in read-only .rodata.
            let i8_type = ctx.codegen.context.i8_type();
            let i64_type = ctx.codegen.context.i64_type();
            let ptr_type = ctx.codegen.context.ptr_type(Default::default());
            let pad6 = i8_type.array_type(6);
            let struct_type = ctx.codegen.context.struct_type(&[
                i64_type.into(),  // ref_count
                i8_type.into(),   // gc_mark
                i8_type.into(),   // type_tag
                pad6.into(),      // [6 x i8] padding → aligns next to offset 16
                ptr_type.into(),  // next
                ptr_type.into(),  // vtable
                i64_type.into(),  // len  (offset 32)
                ptr_type.into(),  // data (offset 40)
            ], false);
            let zero_i8 = i8_type.const_int(0, false);
            let null_ptr = ptr_type.const_null();
            let len_const = i64_type.const_int(s.len() as u64, false);
            let data_ptr = data_global.as_pointer_value();
            let struct_const = ctx.codegen.context.const_struct(&[
                i64_type.const_int(-1i64 as u64, false).into(),  // ref_count = -1 (immortal)
                zero_i8.into(),   // gc_mark = 0
                zero_i8.into(),   // type_tag = 0 (TAG_STRING)
                pad6.const_zero().into(),
                null_ptr.into(),  // next = null
                null_ptr.into(),  // vtable = null
                len_const.into(), // len
                data_ptr.into(),  // data
            ], false);

            let global = ctx.codegen.module.add_global(struct_type, None, &format!("str_{id}"));
            global.set_initializer(&struct_const);
            global.set_constant(true);
            global.set_linkage(Linkage::Private);
            global.set_unnamed_addr(true);

            Ok(global.as_pointer_value().into())
        }
    }
}