//! Macro expansion pass for the HULK compiler.

mod collect;
mod error;
mod expand;
pub mod pattern; // Phase 2: structural pattern matching
mod substitute;

pub use error::{MacroError, MacroErrorKind};

/// Expands all macro declarations and macro calls in `program` in place.
///
/// After this call:
/// - All `DeclarationKind::Macro` entries are removed from `program.declarations`.
/// - All `ExprKind::MacroCall` nodes are replaced with their expanded forms.
/// - Any errors (undefined macros, arity mismatches, etc.) are returned.
///
/// Returns an empty `Vec` on success.
pub fn expand_program(program: &mut hulk_ast::Program) -> Vec<MacroError> {
    let mut errors = Vec::new();

    // Pass A: collect macro declarations (removes them from the program).
    let registry = collect::collect(program, &mut errors);

    if !errors.is_empty() {
        return errors;
    }

    // Pass B: expand all macro call sites.
    expand::expand(program, &registry, &mut errors);

    errors
}