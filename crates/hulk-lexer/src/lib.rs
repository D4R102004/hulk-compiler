//! Lexer for the HULK programming language.
//!
//! Converts raw source code (`&str`) into a flat list of [`Token`]s.
//! This is the first phase of the compiler pipeline:
//!
//! ```text
//! source code → Lexer → Vec<Token> → Parser
//! ```

/// Every distinct kind of token the HULK lexer can produce.
///
/// Variants that carry data store the literal value from source.
/// Variants without data are self-descriptive.
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
  // ── Literals ──────────────────────────────────────────────────────
  /// A numeric literal, e.g. `42` or `3.14`.
  Number(f64),
  /// A string literal, e.g. `"hello world"`
  StringLit(String),
  /// The boolean literal `true`.
  True,
  /// The boolean literal `false`.
  False,

  // ── Arithmetic operators ──────────────────────────────────────────
  Plus, Minus, Star, Slash, Caret, Percent,

  // ── String operators ──────────────────────────────────────────────
  /// Single concatenation `@`
  At,
  /// Concatenation with space `@@`
  AtAt,


  // ── Comparison operators ──────────────────────────────────────────
  EqEq, Neq, Lt, Gt, Leq, Geq,

  // ── Boolean operators ─────────────────────────────────────────────
  And, Or, Not,

  // ── Assignment ────────────────────────────────────────────────────
  /// Simple assignment `=`
  Assign,
  /// Destructive assignment `:=`
  ColonEq,

  // ── Delimiters ────────────────────────────────────────────────────
  LParen, RParen, LBrace, RBrace, LBracket, RBracket,
  Semicolon, Comma, Colon, Dot,
  /// Thin arrow `->` used in functor type annotations
  Arrow,
  /// Fat arrow `=>` used in inline functions
  FatArrow,
  /// Pipe `|` used in vector generator syntax `[x^2 | x in range(0,10)]`
  Pipe,

  // ── Keywords ──────────────────────────────────────────────────────
  Let, In, If, Elif, Else, While, For,
  Function, Type, Inherits, New,
  /// The `self` keyword — renamed to avoid clash with Rust's `Self`
  SelfKw,
  Base, Is, As, Protocol, Extends,
  /// `def` keyword for macro definitions
  Def,

  // ── Extra feature: pattern matching ───────────────────────────────
  Match, Case, Underscore,

  // ── Identifier & end-of-file ──────────────────────────────────────
  /// Any user-defined name: variable, function, type, etc.
  Ident(String),
  /// Signals the end of the token stream.
  Eof,
}