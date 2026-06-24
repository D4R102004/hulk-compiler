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
pub mod lower;
pub mod runtime_decls;

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

pub fn compile(
    verified: &hulk_semantic::VerifiedProgram,
    opts: &options::CodegenOptions,
) -> Result<(), error::CodegenError> {
    let context = inkwell::context::Context::create();
    let mut codegen = context::CodegenCtx::new(&context, "hulk_main");

    // Declare runtime functions (needed for strings, concat, etc.)
    let _alloc = runtime_decls::declare_alloc(&codegen);
    let _concat = runtime_decls::declare_string_concat(&codegen);
    let _concat_space = runtime_decls::declare_string_concat_space(&codegen);
    let _num_to_str = runtime_decls::declare_number_to_string(&codegen);
    let _bool_to_str = runtime_decls::declare_bool_to_string(&codegen);

    // Create main function that returns i32.
    let i32_type = context.i32_type();
    let main_fn = codegen.module.add_function("main", i32_type.fn_type(&[], false), None);
    let entry_bb = context.append_basic_block(main_fn, "entry");
    codegen.builder.position_at_end(entry_bb);

    // Lower the entry expression (ignore its value, but execute for side effects).
    {
        let mut lower_ctx = lower::LowerCtx::new(&mut codegen, &verified.registry);
        match lower::lower_expr(&mut lower_ctx, &verified.typed_program.entry) {
            Ok(_) => {}
            Err(e) => return Err(e),
        }
    }

    // Return 0.
    codegen.builder.build_return(Some(&i32_type.const_int(0, false)))
        .map_err(|e| error::CodegenError::LlvmVerification(e.to_string()))?;

    // Verify module.
    codegen.module.verify()
        .map_err(|e| error::CodegenError::LlvmVerification(e.to_string()))?;

    // Emit object file and link (using existing emit logic).
    emit::init_all_targets()?;
    let machine = emit::linux_x86_64_target_machine()?;
    let obj_path = opts.output_path.with_extension("o");
    emit::write_object_file(&machine, &codegen.module, &obj_path)?;

    // TODO: Link with hulk-rt.

    Ok(())
}

#[cfg(test)]
mod tests {
    //! Integration tests for the full HULK compiler pipeline: lex → parse → analyze → codegen.
    //!
    //! These tests validate that a real source string can be compiled to a valid Linux x86_64
    //! object file. They also check that semantic errors are caught and that unsupported
    //! constructs (in Phase 3) are properly rejected.

    use std::io::Read;
    use std::path::PathBuf;
    use inkwell::context::Context;

    use hulk_lexer::Lexer;
    use hulk_parser::parse;
    use hulk_semantic::analyze;
    use crate::{compile, CodegenOptions, build_smoke_module};
    use tempfile::tempdir;

    /// Compiles a HULK source string to an object file and an executable path (the latter is
    /// a placeholder since linking is not yet implemented in Phase 3). Returns the path to
    /// the generated object file.
    ///
    /// # Panics
    /// Panics if lexing, parsing, semantic analysis, or code generation fails.
    fn compile_source_to_obj(src: &str) -> (tempfile::TempDir, PathBuf) {
        let tokens = Lexer::new(src).tokenize().expect("lex failed");
        let program = parse(tokens).expect("parse failed");
        let verified = analyze(&program).expect("semantic analysis failed");

        let temp_dir = tempdir().expect("create temp dir");
        let output_path = temp_dir.path().join("output");
        let opts = CodegenOptions::with_output_path(output_path);
        compile(&verified, &opts).expect("code generation failed");

        let obj_path = temp_dir.path().join("output.o");
        assert!(obj_path.exists(), "object file not created");
        (temp_dir, obj_path)
    }

    /// Checks that a file is a valid ELF binary by reading its magic number.
    fn is_elf(path: &PathBuf) -> bool {
        let mut file = std::fs::File::open(path).unwrap();
        let mut header = [0u8; 4];
        file.read_exact(&mut header).unwrap();
        header == [0x7f, b'E', b'L', b'F']
    }

    /// Tries to compile a source string and expects a specific `CodegenError` kind.
    /// For Phase 3, we only check that unsupported constructs cause an error.
    fn expect_codegen_error(src: &str, expected_msg: &str) {
        let tokens = Lexer::new(src).tokenize().expect("lex failed");
        let program = parse(tokens).expect("parse failed");
        let verified = analyze(&program).expect("semantic analysis should succeed");
        let temp_dir = tempdir().expect("create temp dir");
        let output_path = temp_dir.path().join("output");
        let opts = CodegenOptions::with_output_path(output_path);
        let result = compile(&verified, &opts);
        let err = result.expect_err("expected codegen error");
        let err_str = err.to_string();
        assert!(
            err_str.contains(expected_msg),
            "expected error message containing '{}', got: {}",
            expected_msg,
            err_str
        );
    }

    // ─── Positive tests ──────────────────────────────────────────────────────

    #[test]
    fn test_simple_arithmetic() {
        let src = "let x = 5 in x + 3;";
        let (_tmp_dir, obj) = compile_source_to_obj(src);
        assert!(is_elf(&obj));
    }

    #[test]
    fn test_if_expression() {
        let src = "if (true) 1 else 2;";
        let (_tmp_dir, obj) = compile_source_to_obj(src);
        assert!(is_elf(&obj));
    }

    #[test]
    fn test_if_elif_else() {
        let src = "
            let x = 42 in
            if (x < 10) 1
            elif (x < 50) 2
            else 3;
        ";
        let (_tmp_dir, obj) = compile_source_to_obj(src);
        assert!(is_elf(&obj));
    }

    #[test]
    fn test_while_loop() {
        let src = "
            let x = 0 in
            while (x < 5) x := x + 1;
        ";
        let (_tmp_dir, obj) = compile_source_to_obj(src);
        assert!(is_elf(&obj));
    }

    #[test]
    fn test_string_literal_and_concat() {
        let src = "
            let a = \"Hello\" in
            let b = \"World\" in
            a @ \" \" @ b;
        ";
        let (_tmp_dir, obj) = compile_source_to_obj(src);
        assert!(is_elf(&obj));
    }

    #[test]
    fn test_let_shadowing() {
        let src = "
            let a = 1 in
            let a = 2 in
            a;
        ";
        let (_tmp_dir, obj) = compile_source_to_obj(src);
        assert!(is_elf(&obj));
    }

    #[test]
    fn test_block_expression() {
        let src = "
            {
                let x = 10 in x + 20;
            }
        ";
        let (_tmp_dir, obj) = compile_source_to_obj(src);
        assert!(is_elf(&obj));
    }

    #[test]
    fn test_assign_variable() {
        let src = "
            let x = 0 in
            {
                x := 42;
                x;
            }
        ";
        let (_tmp_dir, obj) = compile_source_to_obj(src);
        assert!(is_elf(&obj));
    }

    // ─── Negative tests (semantic errors) ──────────────────────────────────

    #[test]
    fn test_semantic_type_mismatch() {
        let src = "let x: Number = \"hello\" in print(x);";
        let tokens = Lexer::new(src).tokenize().unwrap();
        let program = parse(tokens).unwrap();
        let result = analyze(&program);
        assert!(result.is_err(), "expected semantic error");
    }

    #[test]
    fn test_undefined_variable() {
        let src = "print(x);";
        let tokens = Lexer::new(src).tokenize().unwrap();
        let program = parse(tokens).unwrap();
        let result = analyze(&program);
        assert!(result.is_err(), "expected semantic error");
    }

    // ─── Codegen unsupported constructs (Phase 3) ──────────────────────────

    #[test]
    fn test_unsupported_function_call() {
        let src = "
            function f() => 42;
            f();
        ";
        // Since function calls are not yet supported, codegen should fail with Unsupported.
        expect_codegen_error(src, "calls not yet supported");
    }

    #[test]
    fn test_unsupported_member_access() {
        let src = "
            type A {
                f(): Number => 0;
            }
            let x = new A() in x.f;
        ";
        // Member access is not supported in Phase 3.
        expect_codegen_error(src, "object construction not yet supported");
    }

    // ─── Additional check: object file is valid ELF ─────────────────────────

    #[test]
    fn test_object_file_valid_elf() {
        let src = "let x = 1 in x;";
        let (_tmp_dir, obj) = compile_source_to_obj(src);
        assert!(is_elf(&obj));
    }

    // ─── Verify smoke module builds and contains expected functions ────────────

    #[test]
    fn smoke_module_builds_and_verifies() {
        let context = Context::create();
        let ctx = build_smoke_module(&context).expect("smoke module should build and verify");
        assert!(ctx.module.get_function("main").is_some());
        assert!(ctx.module.get_function("hulk_rt_noop").is_some());
    }
}
