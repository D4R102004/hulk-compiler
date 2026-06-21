//! Semantic diagnostics for HULK.
//!
//! This module defines the error types used by the semantic analyzer.
//! Each pass appends `SemanticError` instances to a shared vector.
//! Errors carry a source span and a severity (error or warning) so that
//! the CLI can report them appropriately.

use std::fmt;

use hulk_ast::SourceSpan;

use crate::types::Type;

// -----------------------------------------------------------------------------
// Severity
// -----------------------------------------------------------------------------

/// The severity of a semantic diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// A hard error that prevents compilation.
    Error,
    /// A non‑blocking warning that does not stop compilation.
    Warning,
}

// -----------------------------------------------------------------------------
// SemanticError
// -----------------------------------------------------------------------------

/// A semantic diagnostic: a kind, a source location, and a severity.
///
/// Modeled directly on `hulk_parser::ParseError` so that the CLI can render
/// both phases uniformly.
#[derive(Debug, Clone, PartialEq)]
pub struct SemanticError {
    pub kind: SemanticErrorKind,
    pub span: SourceSpan,
    pub severity: Severity,
}

impl SemanticError {
    /// Creates a new error with `Severity::Error`.
    pub fn error(kind: SemanticErrorKind, span: SourceSpan) -> Self {
        Self {
            kind,
            span,
            severity: Severity::Error,
        }
    }

    /// Creates a new warning with `Severity::Warning`.
    pub fn warning(kind: SemanticErrorKind, span: SourceSpan) -> Self {
        Self {
            kind,
            span,
            severity: Severity::Warning,
        }
    }
}

// -----------------------------------------------------------------------------
// SemanticErrorKind
// -----------------------------------------------------------------------------

/// The specific kind of semantic diagnostic.
///
/// These variants correspond to the checks performed in each pass:
/// - Name resolution and redefinition (Pass 0/2)
/// - Inheritance and protocol conformance (Pass 1)
/// - Type inference and checking (Pass 2/3)
/// - Optional quality‑of‑life warnings (Step 10)
#[derive(Debug, Clone, PartialEq)]
pub enum SemanticErrorKind {
    // ─── Name resolution ──────────────────────────────────────────────────
    /// An undefined variable name was used.
    UndefinedVariable(String),
    /// An undefined function (or builtin) was called.
    UndefinedFunction { name: String, arity: usize },
    /// An undefined type name was referenced.
    UndefinedType(String),
    /// A member (attribute or method) could not be found on a type.
    UnknownMember { ty: Type, member: String },

    // ─── Redeclaration ──────────────────────────────────────────────────
    /// A function was defined more than once (global namespace).
    DuplicateFunction(String),
    /// A type was defined more than once.
    DuplicateType(String),
    /// An attribute was defined more than once within the same type.
    DuplicateAttribute { ty: String, attribute: String },
    /// A parameter name was duplicated within a function/method parameter list.
    DuplicateParameter(String),

    // ─── Inheritance ──────────────────────────────────────────────────────
    /// Attempt to inherit from a builtin value type (`Number`, `String`, `Boolean`).
    InheritFromBuiltinValueType(String),
    /// Attempt to inherit from a type that does not exist.
    InheritFromUndefinedType(String),
    /// A cycle was detected in the inheritance graph.
    InheritanceCycle(Vec<String>),
    /// An overriding method does not match the parent's signature exactly.
    InvalidOverride { method: String, in_type: String, expected: String, found: String },

    // ─── Protocols ──────────────────────────────────────────────────────
    /// A type does not implement all methods required by a protocol.
    ProtocolNotImplemented { ty: Type, protocol: String, missing: Vec<String> },
    /// A protocol extension violates contravariant/covariant variance rules.
    InvalidProtocolVariance { method: String, reason: String },

    // ─── Typing ──────────────────────────────────────────────────────────
    /// The inferred type does not match the declared annotation.
    TypeMismatch { expected: Type, found: Type },
    /// A value of one type cannot be used where another type is expected.
    NotConforming { found: Type, expected: Type },
    /// A call supplies the wrong number of arguments.
    ArityMismatch { expected: usize, found: usize },
    /// An operator is applied to incompatible operand types.
    InvalidOperator { op: String, operand_types: Vec<Type> },
    /// A non‑boolean value was used as a condition.
    NonBooleanCondition(Type),
    /// A value that does not implement `Iterable` was used in a `for` loop.
    NotIterable(Type),
    /// Indexing was attempted on a non‑vector type.
    IndexOnNonVector(Type),
    /// The left‑hand side of an assignment is not assignable.
    InvalidAssignTarget,
    /// `self` was used as an assignment target.
    SelfIsNotAssignable,
    /// `base` was used outside an overriding method.
    BaseOutsideOverridingMethod,

    // ─── Inference ──────────────────────────────────────────────────────
    /// A symbol's type could not be inferred (needs an explicit annotation).
    CannotInferType { symbol: String },
    /// Multiple incompatible types were possible for the same symbol.
    AmbiguousInference { symbol: String, candidates: Vec<Type> },

    // ─── Non‑blocking warnings (quality‑of‑life) ──────────────────────
    /// A downcast (`as`) can never succeed because the types are unrelated.
    UnreachableDowncast { from: Type, to: Type },
    /// A `match` expression does not cover all possible cases.
    NonExhaustiveMatch,
}

// -----------------------------------------------------------------------------
// Display implementations
// -----------------------------------------------------------------------------

impl fmt::Display for SemanticError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let prefix = match self.severity {
            Severity::Error => "semantic error",
            Severity::Warning => "semantic warning",
        };
        write!(
            f,
            "{} at line {}, col {}: {}",
            prefix,
            self.span.line,
            self.span.col,
            self.kind
        )
    }
}

impl fmt::Display for SemanticErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // Name resolution
            Self::UndefinedVariable(name) => {
                write!(f, "undefined variable `{}`", name)
            }
            Self::UndefinedFunction { name, arity } => {
                write!(f, "undefined function `{}` with {} argument(s)", name, arity)
            }
            Self::UndefinedType(name) => {
                write!(f, "undefined type `{}`", name)
            }
            Self::UnknownMember { ty, member } => {
                write!(f, "type `{}` has no member `{}`", ty, member)
            }

            // Redeclaration
            Self::DuplicateFunction(name) => {
                write!(f, "duplicate function `{}`", name)
            }
            Self::DuplicateType(name) => {
                write!(f, "duplicate type `{}`", name)
            }
            Self::DuplicateAttribute { ty, attribute } => {
                write!(f, "duplicate attribute `{}` in type `{}`", attribute, ty)
            }
            Self::DuplicateParameter(name) => {
                write!(f, "duplicate parameter `{}`", name)
            }

            // Inheritance
            Self::InheritFromBuiltinValueType(name) => {
                write!(f, "cannot inherit from builtin value type `{}`", name)
            }
            Self::InheritFromUndefinedType(name) => {
                write!(f, "cannot inherit from undefined type `{}`", name)
            }
            Self::InheritanceCycle(cycle) => {
                write!(f, "inheritance cycle detected: {}", cycle.join(" -> "))
            }
            Self::InvalidOverride { method, in_type, expected, found } => {
                write!(
                    f,
                    "invalid override of method `{}` in type `{}`: expected `{}`, found `{}`",
                    method, in_type, expected, found
                )
            }

            // Protocols
            Self::ProtocolNotImplemented { ty, protocol, missing } => {
                let missing_list = missing.join(", ");
                write!(
                    f,
                    "type `{}` does not implement protocol `{}`; missing methods: {}",
                    ty, protocol, missing_list
                )
            }
            Self::InvalidProtocolVariance { method, reason } => {
                write!(f, "invalid protocol variance for `{}`: {}", method, reason)
            }

            // Typing
            Self::TypeMismatch { expected, found } => {
                write!(f, "type mismatch: expected `{}`, found `{}`", expected, found)
            }
            Self::NotConforming { found, expected } => {
                write!(f, "type `{}` does not conform to `{}`", found, expected)
            }
            Self::ArityMismatch { expected, found } => {
                write!(f, "arity mismatch: expected {}, found {}", expected, found)
            }
            Self::InvalidOperator { op, operand_types } => {
                let types: Vec<String> = operand_types.iter().map(|t| t.to_string()).collect();
                write!(f, "invalid operator `{}` for types {}", op, types.join(", "))
            }
            Self::NonBooleanCondition(ty) => {
                write!(f, "non‑boolean condition of type `{}`", ty)
            }
            Self::NotIterable(ty) => {
                write!(f, "type `{}` is not iterable", ty)
            }
            Self::IndexOnNonVector(ty) => {
                write!(f, "indexing applied to non‑vector type `{}`", ty)
            }
            Self::InvalidAssignTarget => {
                write!(f, "invalid assignment target")
            }
            Self::SelfIsNotAssignable => {
                write!(f, "`self` is not assignable")
            }
            Self::BaseOutsideOverridingMethod => {
                write!(f, "`base` can only be used inside an overriding method")
            }

            // Inference
            Self::CannotInferType { symbol } => {
                write!(f, "cannot infer type for `{}`; add an explicit annotation", symbol)
            }
            Self::AmbiguousInference { symbol, candidates } => {
                let types: Vec<String> = candidates.iter().map(|t| t.to_string()).collect();
                write!(
                    f,
                    "ambiguous inference for `{}`: possible types are {}",
                    symbol,
                    types.join(", ")
                )
            }

            // Non‑blocking warnings
            Self::UnreachableDowncast { from, to } => {
                write!(f, "unreachable downcast: `{}` as `{}` can never succeed", from, to)
            }
            Self::NonExhaustiveMatch => {
                write!(f, "non‑exhaustive match: no catch‑all pattern")
            }
        }
    }
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    // TODO: Tests for Display formatting.
}