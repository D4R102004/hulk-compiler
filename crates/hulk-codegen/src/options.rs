//! Compilation options threaded through `hulk_codegen::compile`.

use std::path::PathBuf;

/// Optimization level requested for the generated module.
///
/// The smoke-test path always uses `None` — there is nothing in a one basic
/// block, no-arithmetic module worth optimizing. Real lowering phases wire
/// this into the actual LLVM pass pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OptLevel {
    None,
    #[default]
    Default,
    Aggressive,
}

/// Options controlling a single `compile()` invocation.
#[derive(Debug, Clone, Default)]
pub struct CodegenOptions {
    /// If set, the generated LLVM IR is also written to this path as
    /// human-readable text (`.ll`). A development and debugging aid only —
    /// never required for a normal build.
    pub emit_llvm_path: Option<PathBuf>,
    /// Where the final linked native executable should be written. The
    /// compiler driver defaults this to `./output` in the current working
    /// directory; it is only overridden here for tests and tooling such as
    /// the Phase 1 smoke example.
    pub output_path: PathBuf,
    pub opt_level: OptLevel,
}

impl CodegenOptions {
    pub fn with_output_path(output_path: impl Into<PathBuf>) -> Self {
        Self {
            output_path: output_path.into(),
            ..Default::default()
        }
    }
}
