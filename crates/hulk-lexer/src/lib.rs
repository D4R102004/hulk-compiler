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
    Plus,
    Minus,
    Star,
    Slash,
    Caret,
    Percent,

    // ── String operators ──────────────────────────────────────────────
    /// Single concatenation `@`
    At,
    /// Concatenation with space `@@`
    AtAt,

    // ── Comparison operators ──────────────────────────────────────────
    EqEq,
    Neq,
    Lt,
    Gt,
    Leq,
    Geq,

    // ── Boolean operators ─────────────────────────────────────────────
    And,
    Or,
    Not,

    // ── Assignment ────────────────────────────────────────────────────
    /// Simple assignment `=`
    Assign,
    /// Destructive assignment `:=`
    ColonEq,

    // ── Delimiters ────────────────────────────────────────────────────
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Semicolon,
    Comma,
    Colon,
    Dot,
    /// Thin arrow `->` used in functor type annotations
    Arrow,
    /// Fat arrow `=>` used in inline functions
    FatArrow,
    /// Pipe `|` used in vector generator syntax `[x^2 | x in range(0,10)]`
    Pipe,

    // ── Keywords ──────────────────────────────────────────────────────
    Let,
    In,
    If,
    Elif,
    Else,
    While,
    For,
    Function,
    Type,
    Inherits,
    New,
    /// The `self` keyword — renamed to avoid clash with Rust's `Self`
    SelfKw,
    Base,
    Is,
    As,
    Protocol,
    Extends,
    /// `def` keyword for macro definitions
    Def,

    // ── Extra feature: pattern matching ───────────────────────────────
    Match,
    Case,
    Underscore,

    // ── Identifier & end-of-file ──────────────────────────────────────
    /// Any user-defined name: variable, function, type, etc.
    Ident(String),
    /// Signals the end of the token stream.
    Eof,
}

/// A location in the source file, used for error reporting.
///
/// Both `line` and `col` are 1-based, matching what humans
/// expect in error messages: "error at line 3, col 7".
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Span {
    /// Line number, starting at 1.
    pub line: usize,
    /// Column number, starting at 1.
    pub col: usize,
}

/// A single token produced by the lexer.
///
/// Bundles a [`TokenKind`] with the [`Span`] where it was found,
/// so every later compiler phase can report precise error locations.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    /// What kind of token this is.
    pub kind: TokenKind,
    /// Where in the source this token starts.
    pub span: Span,
}

/// An error produced during lexing.
///
/// The lexer is intentionally strict: it halts on the first
/// unrecognisable input rather than guessing.
#[derive(Debug, Clone, PartialEq)]
pub enum LexError {
    /// A character that belongs to no HULK token was encountered.
    ///
    /// # Example
    /// `#` or `$` in HULK source code.
    UnexpectedChar {
        /// The offending character.
        ch: char,
        /// Where it appeared.
        span: Span,
    },

    /// A string literal was opened but never closed before end-of-file.
    ///
    /// # Example
    /// `"hello` with no closing quote.
    UnterminatedString {
        /// Where the string literal started.
        span: Span,
    },
}
