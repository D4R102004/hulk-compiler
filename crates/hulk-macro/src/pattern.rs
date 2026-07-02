// hulk-macro/src/pattern.rs
//
// Structural pattern matching for macro arguments (§A.14.5).
//
// This module provides the `try_match` function, which compares a macro
// pattern (representing an AST shape) with a concrete `Expr` AST node.
// On success, it returns a mapping from capture variable names to the
// matched sub‑expressions. The expansion engine in `substitute.rs` uses this
// to implement compile‑time `match` inside macro bodies.
//
// NOTE: This file is a placeholder. The matching logic is not yet implemented;
// `try_match` currently returns `None` for every pattern. Once implemented,
// it will be called from `substitute.rs` when encountering an `ExprKind::Match`
// whose scrutinee is a concrete AST node (i.e., inside an expanding macro).

use std::collections::HashMap;
use hulk_ast::{BinaryOp, Expr, Literal, TypeRef, UnaryOp};
use crate::error::MacroError;

/// A pattern that can be matched against an expression AST at compile time.
#[derive(Debug, Clone)]
pub enum MacroPattern {
    /// Matches any expression (wildcard).
    Wildcard,
    /// Matches a specific literal value (integer, string, boolean, etc.).
    Literal(Literal),
    /// Matches a binary expression with a specific operator, and binds
    /// the left and right operands (which themselves may contain patterns).
    BinaryExpr {
        op: BinaryOp,
        left: Box<MacroPatternBind>,
        right: Box<MacroPatternBind>,
    },
    /// Matches a unary expression with a specific operator.
    UnaryExpr {
        op: UnaryOp,
        operand: Box<MacroPatternBind>,
    },
    // Future: constructor patterns, list patterns, etc.
}

/// A binding site in a macro pattern. It may include a variable name to
/// capture the matched subtree, an optional type constraint (not yet used
/// by matching, but available for consistency checks), and a sub‑pattern.
#[derive(Debug, Clone)]
pub struct MacroPatternBind {
    /// If `Some`, the matched sub‑expression will be bound to this name
    /// in the substitution map (allowing it to be used in the case body).
    pub name: Option<String>,
    /// Optional type annotation (for documentation / future type checking).
    pub ty: Option<TypeRef>,
    /// The shape this binding must match.
    pub pattern: MacroPattern,
}

/// Attempts to match a `pattern` against the concrete AST node `expr`.
///
/// # Returns
/// - `Some(bindings)` if the pattern matches, where `bindings` maps each
///   capture variable name to the `Expr` subtree that was matched.
/// - `None` if the pattern does not match.
///
/// # How it will work:
/// - `MacroPattern::Wildcard` always matches, producing no bindings.
/// - `MacroPattern::Literal(lit)` matches only if `expr` is a literal with
///   the same value.
/// - `MacroPattern::BinaryExpr` matches an `ExprKind::Binary` node with the
///   given operator, then recursively matches the left and right sides. The
///   bindings from both sides are merged (duplicate variable names are not
///   allowed in a single pattern, and will cause a match failure).
/// - `MacroPattern::UnaryExpr` matches an `ExprKind::Unary` node similarly.
/// - `MacroPatternBind` captures the matched subtree under `name` if present.
///
/// # Integration
/// In `substitute.rs`, when expanding a macro and encountering a
/// `MacroMatchExpr` (an expression of the form `match(scrutinee) { cases }`),
/// the engine will:
/// 1. Ensure `scrutinee` has already been substituted to a concrete `Expr`.
/// 2. For each case, convert the pattern AST into a `MacroPattern` (using a
///    helper from this module).
/// 3. Call `try_match(&pattern, &scrutinee_expr)`.
/// 4. If it returns bindings, merge them into the `SubstMap` and expand the
///    corresponding case body.
pub fn try_match(
    pattern: &MacroPattern,
    _expr: &Expr,
) -> Option<HashMap<String, Expr>> {
    // Placeholder: no patterns match yet. The real implementation will be added
    // after the basic macro system (Phase 1) is stable.
    match pattern {
        MacroPattern::Wildcard => Some(HashMap::new()), // always matches, no bindings
        MacroPattern::Literal(_) => None,               // TODO
        MacroPattern::BinaryExpr { .. } => None,        // TODO
        MacroPattern::UnaryExpr { .. } => None,         // TODO
    }
}

/// Converts a parsed match‑case pattern (produced by the parser for macro bodies)
/// into a `MacroPattern` for compile‑time matching.
///
/// # Parameters
/// - `pat_ast`: the pattern AST node as returned by the parser (e.g., a `PatternExpr`).
///
/// # Returns
/// - `Ok(MacroPattern)` on successful conversion.
/// - `Err(MacroError)` if the pattern is malformed or contains unsupported constructs.
///
/// This function will be implemented in when implementing structural pattern matching.
///  It is called from `substitute.rs` when expanding a macro‑local `match` expression.
pub fn pattern_from_ast(_pat_ast: &Expr) -> Result<MacroPattern, MacroError> {
    // TODO: implement AST → pattern conversion
    // Mapping examples:
    //   `(x:Number + 0)` → BinaryExpr{op:Add, left:Bind(Some("x"), Number, Wildcard), right:Bind(None, None, Literal(0))}
    todo!("Convert parsed pattern to MacroPattern")
}