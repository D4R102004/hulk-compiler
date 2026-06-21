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

use crate::error::Severity;

// -----------------------------------------------------------------------------
// Public API re-exports
// -----------------------------------------------------------------------------

pub use error::{SemanticError, SemanticErrorKind};
pub use environment::{Binding, Environment};
pub use types::{Type, TypeRegistry, seeded_registry};
pub use typed::{TypedExpr, TypedProgram};

// -----------------------------------------------------------------------------
// Main structure
// -----------------------------------------------------------------------------

/// The result of a successful semantic analysis.
///
/// Contains the global type registry (with all resolved signatures), the fully
/// typed AST, and any non‑fatal warnings that were emitted during analysis.
///
/// This is the structure that `hulk-codegen` will consume.
#[derive(Debug, Clone)]
pub struct VerifiedProgram {
    /// The complete global knowledge base: types, protocols, and functions.
    pub registry: TypeRegistry,
    /// The fully typed program tree, guaranteed by the type system to have
    /// a resolved `Type` for every expression.
    pub typed_program: TypedProgram,
    /// Non‑fatal diagnostics (warnings) that were emitted during analysis.
    /// These do not block compilation but should be surfaced to the user.
    pub warnings: Vec<SemanticError>,
}

// -----------------------------------------------------------------------------
// Main entry point
// -----------------------------------------------------------------------------

/// Performs full semantic analysis on an untyped HULK program.
///
/// This runs the four‑pass pipeline:
/// 1. Declaration collection (Pass 0) – builds the global registry.
/// 2. Hierarchy resolution (Pass 1) – resolves inheritance and protocol links.
/// 3. Type inference (Pass 2) – builds the typed tree.
/// 4. Type checking (Pass 3) – final consistency sweep.
///
/// Returns `Ok(VerifiedProgram)` if no hard errors are present; warnings are
/// always collected and returned inside `VerifiedProgram`. If any hard error
/// is encountered, returns `Err(Vec<SemanticError>)` containing all errors.
pub fn analyze(program: &hulk_ast::Program) -> Result<VerifiedProgram, Vec<SemanticError>> {
    let mut errors = Vec::new();
    let mut registry = seeded_registry();

    passes::collect(program, &mut registry, &mut errors);
    passes::hierarchy(&mut registry, &mut errors);

    // Early exit: a broken hierarchy invalidates `conforms_to` itself.
    // Continuing would only produce misleading cascade errors.
    if errors.iter().any(|e| e.severity == Severity::Error) {
        return Err(errors);
    }

    let typed_program = passes::infer(program, &mut registry, &mut errors);
    passes::check(&typed_program, &registry, &mut errors);

    // Separate errors from warnings.
    let (hard_errors, warnings): (Vec<_>, Vec<_>) = errors
        .into_iter()
        .partition(|e| e.severity == Severity::Error);

    if hard_errors.is_empty() {
        Ok(VerifiedProgram {
            registry,
            typed_program,
            warnings,
        })
    } else {
        Err(hard_errors)
    }
}