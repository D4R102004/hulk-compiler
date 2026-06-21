//! Lexical scoping environment for HULK.
//!
//! This module implements the inherited attribute: a stack of scopes
//! (hash maps from variable names to their types and source locations)
//! that is threaded top‑down through every visitor during type inference
//! and checking.
//!
//! The environment is used by Pass 2 (inference) and Pass 3 (checking) to
//! resolve variable references and to track which names are in scope.
//! It is not persisted between passes; a fresh `Environment` is created
//! for each traversal.

use std::collections::HashMap;

use hulk_ast::SourceSpan;

use crate::types::Type;

// -----------------------------------------------------------------------------
// Binding
// -----------------------------------------------------------------------------

/// A variable binding in the environment: its type and source location.
#[derive(Debug, Clone, PartialEq)]
pub struct Binding {
    pub ty: Type,
    pub span: SourceSpan,
}

impl Binding {
    pub fn new(ty: Type, span: SourceSpan) -> Self {
        Self { ty, span }
    }
}

// -----------------------------------------------------------------------------
// Environment
// -----------------------------------------------------------------------------

/// A stack of scopes, each mapping variable names to their bindings.
///
/// This is the inherited attribute passed down through the AST visitors.
/// It is created anew for each pass that needs name resolution.
///
/// # Scope discipline
/// - `push_scope` / `pop_scope` are called around every construct that
///   introduces a new lexical scope: `let` body, function/method body,
///   `for` loop body, and `match` case body.
/// - Plain `{ ... }` blocks do **not** push a scope — they are pure
///   sequencing and do not affect name resolution.
#[derive(Debug, Clone)]
pub struct Environment {
    scopes: Vec<HashMap<String, Binding>>,
}

impl Environment {
    /// Creates a new environment with one root scope (initially empty).
    ///
    /// The root scope exists so that `declare` always has a scope to insert into.
    /// HULK has no global variables, so the root scope remains empty in practice.
    pub fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
        }
    }

    /// Pushes a new empty scope onto the stack.
    ///
    /// Must be paired with a subsequent `pop_scope`.
    pub fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    /// Pops the innermost scope, discarding all its bindings.
    ///
    /// # Panics
    /// Panics if called when there is only one scope left (the root).
    /// This prevents accidental removal of the root scope.
    pub fn pop_scope(&mut self) {
        if self.scopes.len() <= 1 {
            panic!("cannot pop the root scope");
        }
        self.scopes.pop();
    }

    /// Declares a variable in the innermost scope.
    ///
    /// If a binding with the same name already exists in that scope, it is
    /// overwritten (rebinding). This is intentional and matches HULK's
    /// rule that `let a = 7, a = 7*6 in ...` is valid.
    ///
    /// # Note
    /// Duplicate declarations within the same parameter list or `for`/`match`
    /// binding are caught earlier by the collection pass (Pass 0) and should
    /// never reach this function. This function does not check for duplicates.
    pub fn declare(&mut self, name: &str, ty: Type, span: SourceSpan) {
        let scope = self.scopes.last_mut().expect("at least one scope exists");
        scope.insert(name.to_string(), Binding::new(ty, span));
    }

    /// Looks up a variable name starting from the innermost scope outward.
    ///
    /// Returns the first matching `Binding` if found, or `None` if the name
    /// is not bound in any scope.
    ///
    /// This implements lexical shadowing exactly as described in §A.4.5:
    /// an inner binding hides an outer one.
    pub fn lookup(&self, name: &str) -> Option<&Binding> {
        for scope in self.scopes.iter().rev() {
            if let Some(binding) = scope.get(name) {
                return Some(binding);
            }
        }
        None
    }

    /// Returns `true` if the name is declared in any scope.
    ///
    /// This is a convenience wrapper around `lookup`, useful for existence checks.
    pub fn is_declared(&self, name: &str) -> bool {
        self.lookup(name).is_some()
    }

    /// Returns the current nesting depth (number of scopes).
    pub fn depth(&self) -> usize {
        self.scopes.len()
    }
}

impl Default for Environment {
    fn default() -> Self {
        Self::new()
    }
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    // TODO: Test fixtures and tests
}