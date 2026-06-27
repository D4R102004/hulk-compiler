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

/// Declares `hulk_rt_downcast_check(obj: ptr, target_vtable: ptr) -> i1`.
pub fn declare_downcast_check<'ctx>(ctx: &CodegenCtx<'ctx>) -> FunctionValue<'ctx> {
    let ptr_type = ctx.context.ptr_type(Default::default());
    let bool_type = ctx.context.bool_type();
    let fn_type = bool_type.fn_type(&[ptr_type.into(), ptr_type.into()], false);
    ctx.module.add_function("hulk_rt_downcast_check", fn_type, None)
}

/// Declares `hulk_rt_downcast_fail() -> !` (noreturn).
pub fn declare_downcast_fail<'ctx>(ctx: &CodegenCtx<'ctx>) -> FunctionValue<'ctx> {
    let void_type = ctx.context.void_type();
    let fn_type = void_type.fn_type(&[], false);
    ctx.module.add_function("hulk_rt_downcast_fail", fn_type, None)
}

// ─── Vector builtin methods ───────────────────────────────────────────────

/// Declares `hulk_rt_vector_size(vec: ptr) -> i64`.
pub fn declare_vector_size<'ctx>(ctx: &CodegenCtx<'ctx>) -> FunctionValue<'ctx> {
    let ptr_type = ctx.context.ptr_type(Default::default());
    let i64_type = ctx.context.i64_type();
    let fn_type = i64_type.fn_type(&[ptr_type.into()], false);
    ctx.module.add_function("hulk_rt_vector_size", fn_type, None)
}

/// Declares `hulk_rt_vector_get(vec: ptr, index: i64) -> ptr`.
pub fn declare_vector_get<'ctx>(ctx: &CodegenCtx<'ctx>) -> FunctionValue<'ctx> {
    let ptr_type = ctx.context.ptr_type(Default::default());
    let i64_type = ctx.context.i64_type();
    let fn_type = ptr_type.fn_type(&[ptr_type.into(), i64_type.into()], false);
    ctx.module.add_function("hulk_rt_vector_get", fn_type, None)
}

/// Declares `hulk_rt_vector_set(vec: ptr, index: i64, value: ptr) -> void`.
pub fn declare_vector_set<'ctx>(ctx: &CodegenCtx<'ctx>) -> FunctionValue<'ctx> {
    let ptr_type = ctx.context.ptr_type(Default::default());
    let i64_type = ctx.context.i64_type();
    let void_type = ctx.context.void_type();
    let fn_type = void_type.fn_type(&[ptr_type.into(), i64_type.into(), ptr_type.into()], false);
    ctx.module.add_function("hulk_rt_vector_set", fn_type, None)
}

/// Declares `hulk_rt_vector_next(vec: ptr) -> i1`.
pub fn declare_vector_next<'ctx>(ctx: &CodegenCtx<'ctx>) -> FunctionValue<'ctx> {
    let ptr_type = ctx.context.ptr_type(Default::default());
    let bool_type = ctx.context.bool_type();
    let fn_type = bool_type.fn_type(&[ptr_type.into()], false);
    ctx.module.add_function("hulk_rt_vector_next", fn_type, None)
}

/// Declares `hulk_rt_vector_current(vec: ptr) -> ptr`.
pub fn declare_vector_current<'ctx>(ctx: &CodegenCtx<'ctx>) -> FunctionValue<'ctx> {
    let ptr_type = ctx.context.ptr_type(Default::default());
    let fn_type = ptr_type.fn_type(&[ptr_type.into()], false);
    ctx.module.add_function("hulk_rt_vector_current", fn_type, None)
}

// ─── Range builtin methods ────────────────────────────────────────────────

/// Declares `hulk_rt_range_next(rng: ptr) -> i1`.
pub fn declare_range_next<'ctx>(ctx: &CodegenCtx<'ctx>) -> FunctionValue<'ctx> {
    let ptr_type = ctx.context.ptr_type(Default::default());
    let bool_type = ctx.context.bool_type();
    let fn_type = bool_type.fn_type(&[ptr_type.into()], false);
    ctx.module.add_function("hulk_rt_range_next", fn_type, None)
}

/// Declares `hulk_rt_range_current(rng: ptr) -> f64`.
pub fn declare_range_current<'ctx>(ctx: &CodegenCtx<'ctx>) -> FunctionValue<'ctx> {
    let ptr_type = ctx.context.ptr_type(Default::default());
    let f64_type = ctx.context.f64_type();
    let fn_type = f64_type.fn_type(&[ptr_type.into()], false);
    ctx.module.add_function("hulk_rt_range_current", fn_type, None)
}

// ─── Match fail trap ──────────────────────────────────────────────────────

/// Declares `hulk_rt_match_fail() -> !` (noreturn).
pub fn declare_match_fail<'ctx>(ctx: &CodegenCtx<'ctx>) -> FunctionValue<'ctx> {
    let void_type = ctx.context.void_type();
    let fn_type = void_type.fn_type(&[], false);
    ctx.module.add_function("hulk_rt_match_fail", fn_type, None)
}

// ─── String equality ──────────────────────────────────────────────────────

/// Declares `hulk_rt_string_equals(a: ptr, b: ptr) -> i1`.
pub fn declare_string_equals<'ctx>(ctx: &CodegenCtx<'ctx>) -> FunctionValue<'ctx> {
    let ptr_type = ctx.context.ptr_type(Default::default());
    let bool_type = ctx.context.bool_type();
    let fn_type = bool_type.fn_type(&[ptr_type.into(), ptr_type.into()], false);
    ctx.module.add_function("hulk_rt_string_equals", fn_type, None)
}

// ─── Dynamic vector helpers for comprehensions ──────────────────────────

/// Declares `hulk_rt_dynamic_vector_new() -> ptr`.
pub fn declare_dynamic_vector_new<'ctx>(ctx: &CodegenCtx<'ctx>) -> FunctionValue<'ctx> {
    let ptr_type = ctx.context.ptr_type(Default::default());
    let fn_type = ptr_type.fn_type(&[], false);
    ctx.module.add_function("hulk_rt_dynamic_vector_new", fn_type, None)
}

/// Declares `hulk_rt_dynamic_vector_append(vec: ptr, value: ptr) -> void`.
pub fn declare_dynamic_vector_append<'ctx>(ctx: &CodegenCtx<'ctx>) -> FunctionValue<'ctx> {
    let void_type = ctx.context.void_type();
    let ptr_type = ctx.context.ptr_type(Default::default());
    let fn_type = void_type.fn_type(&[ptr_type.into(), ptr_type.into()], false);
    ctx.module.add_function("hulk_rt_dynamic_vector_append", fn_type, None)
}

/// Declares `hulk_rt_dynamic_vector_to_vector(vec: ptr) -> ptr`.
pub fn declare_dynamic_vector_to_vector<'ctx>(ctx: &CodegenCtx<'ctx>) -> FunctionValue<'ctx> {
    let ptr_type = ctx.context.ptr_type(Default::default());
    let fn_type = ptr_type.fn_type(&[ptr_type.into()], false);
    ctx.module.add_function("hulk_rt_dynamic_vector_to_vector", fn_type, None)
}

// ─── Declare all runtime functions ───────────────────────────────────────────

/// Declares all standard runtime functions and inserts them into `ctx.functions`.
pub fn declare_all(ctx: &mut CodegenCtx) {
    // Basic runtime functions
    let alloc = declare_alloc(ctx);
    ctx.functions.insert("hulk_rt_alloc".to_string(), alloc);
    let concat = declare_string_concat(ctx);
    ctx.functions.insert("hulk_rt_string_concat".to_string(), concat);
    let concat_space = declare_string_concat_space(ctx);
    ctx.functions.insert("hulk_rt_string_concat_space".to_string(), concat_space);
    let num_to_str = declare_number_to_string(ctx);
    ctx.functions.insert("hulk_rt_number_to_string".to_string(), num_to_str);
    let bool_to_str = declare_bool_to_string(ctx);
    ctx.functions.insert("hulk_rt_bool_to_string".to_string(), bool_to_str);
    let downcast_check = declare_downcast_check(ctx);
    ctx.functions.insert("hulk_rt_downcast_check".to_string(), downcast_check);
    let downcast_fail = declare_downcast_fail(ctx);
    ctx.functions.insert("hulk_rt_downcast_fail".to_string(), downcast_fail);

    // Vector methods
    let vec_size = declare_vector_size(ctx);
    ctx.functions.insert("Vector::size".to_string(), vec_size);
    let vec_get = declare_vector_get(ctx);
    ctx.functions.insert("Vector::get".to_string(), vec_get);
    let vec_set = declare_vector_set(ctx);
    ctx.functions.insert("Vector::set".to_string(), vec_set);
    let vec_next = declare_vector_next(ctx);
    ctx.functions.insert("Vector::next".to_string(), vec_next);
    let vec_current = declare_vector_current(ctx);
    ctx.functions.insert("Vector::current".to_string(), vec_current);

    // Range methods
    let range_next = declare_range_next(ctx);
    ctx.functions.insert("Range::next".to_string(), range_next);
    let range_current = declare_range_current(ctx);
    ctx.functions.insert("Range::current".to_string(), range_current);

    // Match fail trap
    let match_fail = declare_match_fail(ctx);
    ctx.functions.insert("hulk_rt_match_fail".to_string(), match_fail);

    // String equality
    let str_eq = declare_string_equals(ctx);
    ctx.functions.insert("hulk_rt_string_equals".to_string(), str_eq);

    // Dynamic vector helpers for comprehensions
    let dyn_new = declare_dynamic_vector_new(ctx);
    ctx.functions.insert("hulk_rt_dynamic_vector_new".to_string(), dyn_new);
    let dyn_append = declare_dynamic_vector_append(ctx);
    ctx.functions.insert("hulk_rt_dynamic_vector_append".to_string(), dyn_append);
    let dyn_to_vec = declare_dynamic_vector_to_vector(ctx);
    ctx.functions.insert("hulk_rt_dynamic_vector_to_vector".to_string(), dyn_to_vec);
}