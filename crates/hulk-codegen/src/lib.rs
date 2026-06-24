//! Code generation for HULK: turns a fully type-checked
//! `hulk_semantic::VerifiedProgram` into a native Linux x86_64 executable.
//!
//! The compiler is written in Rust and can be developed on any platform
//! (Windows, macOS, Linux). However, all generated HULK executables are
//! Linux x86_64 binaries — this is a cross-compiler in the strict sense
//!  when developed on Windows.
//!
//! The pipeline is: lower the typed AST into an LLVM module, verify and
//! optimize that module, emit it as a Linux x86_64 relocatable object file,
//! and link that object file with the `hulk-rt` runtime library using `clang`
//! (or `cc` on non-Windows) with the Linux target triple. This crate owns the
//! first three steps; linking is delegated to a thin driver function so the
//! compiler frontend can place the resulting executable wherever its own
//! contract requires it.

pub mod context;
pub mod emit;
pub mod error;
pub mod options;

use std::path::Path;

use inkwell::context::Context;
use inkwell::values::FunctionValue;

pub use context::CodegenCtx;
pub use error::CodegenError;
pub use options::{CodegenOptions, OptLevel};

/// Declares `hulk_rt_noop` as an external symbol so generated IR can call
/// it. Stands in for the `runtime_decls` module that later phases will use
/// to declare every `hulk-rt` entry point this crate calls into.
fn declare_smoke_runtime_fn<'ctx>(ctx: &CodegenCtx<'ctx>) -> FunctionValue<'ctx> {
    let fn_type = ctx.context.void_type().fn_type(&[], false);
    ctx.module.add_function("hulk_rt_noop", fn_type, None)
}

/// Builds the smoke-test module: a `main` function that calls
/// `hulk_rt_noop` and returns `0`. Exposed independently of `compile` so
/// the smoke example and unit tests can exercise it without needing a real
/// `VerifiedProgram`.
pub fn build_smoke_module(context: &Context) -> Result<CodegenCtx<'_>, CodegenError> {
    let ctx = CodegenCtx::new(context, "hulk_smoke");

    let noop = declare_smoke_runtime_fn(&ctx);

    let i32_t = ctx.context.i32_type();
    let main_fn = ctx.module.add_function("main", i32_t.fn_type(&[], false), None);
    let entry_bb = ctx.context.append_basic_block(main_fn, "entry");
    ctx.builder.position_at_end(entry_bb);
    ctx.builder
        .build_call(noop, &[], "call_noop")
        .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;
    ctx.builder
        .build_return(Some(&i32_t.const_int(0, false)))
        .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;

    ctx.module
        .verify()
        .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;

    Ok(ctx)
}

/// Writes `ctx`'s module to `path` as human-readable LLVM IR. A development
/// aid only — never part of the required build output.
pub fn emit_llvm_ir_to_file(ctx: &CodegenCtx, path: &Path) -> Result<(), CodegenError> {
    ctx.module
        .print_to_file(path)
        .map_err(|e| CodegenError::LlvmVerification(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoke_module_builds_and_verifies() {
        let context = Context::create();
        let ctx = build_smoke_module(&context).expect("smoke module should build and verify");
        assert!(ctx.module.get_function("main").is_some());
        assert!(ctx.module.get_function("hulk_rt_noop").is_some());
    }
}
