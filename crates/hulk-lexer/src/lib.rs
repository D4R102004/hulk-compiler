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

/// The HULK lexer.
///
/// Created with [`Lexer::new`] and consumed by [`Lexer::tokenize`].
///
/// # Example
/// ```
/// let mut lexer = Lexer::new("1 + 2;");
/// let tokens = lexer.tokenize().unwrap();
/// ```
pub struct Lexer {
    /// Source code as individual characters for index-based access.
    source: Vec<char>,
    /// Index of the next character to be read.
    current: usize,
    /// Current line number (1-based).
    line: usize,
    /// Current column number (1-based).
    col: usize,
}

impl Lexer {
    /// Creates a new [`Lexer`] ready to tokenize `source`.
    ///
    /// # Arguments
    /// * `source` - The raw HULK source code to tokenize.
    ///
    /// # Example
    /// ```
    /// let mut lexer = Lexer::new("1 + 2;");
    /// ```
    pub fn new(source: &str) -> Self {
        Self {
            source: source.chars().collect(),
            current: 0,
            line: 1,
            col: 1,
        }
    }
    /// Returns `true` if all characters have been consumed.
    fn is_at_end(&self) -> bool {
        self.current >= self.source.len()
    }
    /// Returns the current character without consuming it.
    ///
    /// Returns `'\0'` as a sentinel when the source is exhausted,
    /// so callers can check `peek() != '\0'` without a separate
    /// bounds check.
    fn peek(&self) -> char {
        if self.is_at_end() {
            '\0'
        } else {
            self.source[self.current]
        }
    }

    /// Returns the character at `current + 1` without consuming it.
    ///
    /// Used to distinguish two-character tokens like `==`, `:=`, `=>`.
    /// Returns `'\0'` as a sentinel when the lookahead is out of bounds.
    fn peek_next(&self) -> char {
        if self.current + 1 >= self.source.len() {
            '\0'
        } else {
            self.source[self.current + 1]
        }
    }
    /// Consumes the current character and advances the lexer state.
    ///
    /// Updates `line` and `col` for accurate error reporting.
    /// Increments `line` and resets `col` on newlines.
    ///
    /// # Returns
    /// The consumed character, or `'\0'` if already at end of source.
    fn advance(&mut self) -> char {
        let ch = self.peek();
        if !self.is_at_end() {
            self.current += 1;
            if ch == '\n' {
                self.line += 1;
                self.col = 1;
            } else {
                self.col += 1;
            }
            ch
        } else {
            '\0'
        }
    }

    /// Returns a [`Span`] representing the current cursor position.
    ///
    /// Used to stamp every token with where it started in the source.
    fn current_span(&self) -> Span {
        Span {
            line: self.line,
            col: self.col,
        }
    }
}
