//! Declarations of `hulk-rt` symbols as external LLVM functions.
//!
//! These are used by the lowering code to call into the runtime library.

use inkwell::values::FunctionValue;

use crate::context::CodegenCtx;

/// Declares `hulk_rt_alloc(size: i64) -> ptr`.
pub fn declare_alloc<'ctx>(ctx: &CodegenCtx<'ctx>) -> FunctionValue<'ctx> {
    let i64_type = ctx.context.i64_type();
    let ptr_type = ctx.context.ptr_type(Default::default());
    let fn_type = ptr_type.fn_type(&[i64_type.into()], false);
    ctx.module.add_function("hulk_rt_alloc", fn_type, None)
}

/// Declares `hulk_rt_string_concat(a: ptr, b: ptr) -> ptr`.
pub fn declare_string_concat<'ctx>(ctx: &CodegenCtx<'ctx>) -> FunctionValue<'ctx> {
    let ptr_type = ctx.context.ptr_type(Default::default());
    let fn_type = ptr_type.fn_type(&[ptr_type.into(), ptr_type.into()], false);
    ctx.module.add_function("hulk_rt_string_concat", fn_type, None)
}

/// Declares `hulk_rt_string_concat_space(a: ptr, b: ptr) -> ptr`.
pub fn declare_string_concat_space<'ctx>(ctx: &CodegenCtx<'ctx>) -> FunctionValue<'ctx> {
    let ptr_type = ctx.context.ptr_type(Default::default());
    let fn_type = ptr_type.fn_type(&[ptr_type.into(), ptr_type.into()], false);
    ctx.module.add_function("hulk_rt_string_concat_space", fn_type, None)
}

/// Declares `hulk_rt_number_to_string(num: f64) -> ptr`.
pub fn declare_number_to_string<'ctx>(ctx: &CodegenCtx<'ctx>) -> FunctionValue<'ctx> {
    let f64_type = ctx.context.f64_type();
    let ptr_type = ctx.context.ptr_type(Default::default());
    let fn_type = ptr_type.fn_type(&[f64_type.into()], false);
    ctx.module.add_function("hulk_rt_number_to_string", fn_type, None)
}

/// Declares `hulk_rt_bool_to_string(b: i1) -> ptr`.
pub fn declare_bool_to_string<'ctx>(ctx: &CodegenCtx<'ctx>) -> FunctionValue<'ctx> {
    let bool_type = ctx.context.bool_type();
    let ptr_type = ctx.context.ptr_type(Default::default());
    let fn_type = ptr_type.fn_type(&[bool_type.into()], false);
    ctx.module.add_function("hulk_rt_bool_to_string", fn_type, None)
}