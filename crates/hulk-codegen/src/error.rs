//! Error type returned by `hulk_codegen::compile` and its building blocks.

use std::fmt;

/// Every way code generation can fail.
///
/// Reaching this type at all means `hulk-semantic`'s `analyze` already
/// accepted the program — these are failures in turning an already
/// type-checked program into machine code, not language-level errors. They
/// are reported and handled separately from the lexical/syntax/semantic
/// error classes the compiler driver already knows about.
#[derive(Debug)]
pub enum CodegenError {
    /// The LLVM module failed `Module::verify()`. Always an internal bug in
    /// a lowering function, never a property of the input HULK program.
    LlvmVerification(String),
    /// A filesystem operation (writing `.ll`/`.o`/the linked binary) failed.
    Io(std::io::Error),
    /// Object emission via the LLVM target machine failed.
    TargetEmission(String),
    /// The system linker driver could not be invoked, or returned non-zero.
    Link {
        driver: String,
        status: Option<i32>,
        stderr: String,
    },
    /// A construct that passed semantic analysis has no lowering yet.
    /// Distinguishes "not implemented yet, in a known and tracked way" from
    /// a genuine compiler bug.
    Unsupported { construct: String },
}

impl fmt::Display for CodegenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LlvmVerification(msg) => {
                write!(f, "internal error: LLVM module failed verification: {msg}")
            }
            Self::Io(err) => write!(f, "I/O error during code generation: {err}"),
            Self::TargetEmission(msg) => write!(f, "failed to emit object code: {msg}"),
            Self::Link {
                driver,
                status,
                stderr,
            } => write!(f, "linker `{driver}` failed (exit status {status:?}):\n{stderr}"),
            Self::Unsupported { construct } => {
                write!(f, "internal error: no code generation support yet for: {construct}")
            }
        }
    }
}

impl std::error::Error for CodegenError {}

impl From<std::io::Error> for CodegenError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}
