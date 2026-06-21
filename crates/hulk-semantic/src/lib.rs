//! Semantic analysis for HULK.
//!
//! This crate consumes the untyped AST produced by `hulk-parser` and
//! performs name resolution, type inference, and type checking.
//! The output is a fully typed AST (`Program<Type>`) ready for code generation.

#![deny(missing_docs)]

mod error;
mod environment;
mod typed;
mod types;
mod passes;

// -----------------------------------------------------------------------------
// Public API re-exports
// -----------------------------------------------------------------------------

pub use error::{SemanticError, SemanticErrorKind};
pub use environment::{Binding, Environment};
pub use types::{Type, TypeRegistry};
pub use typed::{TypedExpr, TypedProgram, VerifiedProgram};

// -----------------------------------------------------------------------------
// Main entry point
// -----------------------------------------------------------------------------

/// Performs full semantic analysis on an untyped HULK program.
///
/// This runs the four‑pass pipeline:
/// - Pass 0: collect declarations
/// - Pass 1: resolve inheritance and protocol hierarchies
/// - Pass 2: type inference (builds `Program<Type>`)
/// - Pass 3: type checking
///
/// Returns `Ok(VerifiedProgram)` on success, or a vector of errors otherwise.
///
/// # Note
/// This function will be fully implemented once all underlying passes are
/// written (see Step 11 of the implementation plan).
pub fn analyze(_program: &hulk_ast::Program) -> Result<VerifiedProgram, Vec<SemanticError>> {
    // TODO: Step 11 – implement the full pipeline once all modules exist.
    unimplemented!("Semantic analysis pipeline will be implemented in Step 11")
}
