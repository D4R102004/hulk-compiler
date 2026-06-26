//! LL(1) predictive parser for HULK.
//!
//! This crate consumes the token stream produced by `hulk-lexer` and builds the
//! semantic Abstract Syntax Tree defined in `hulk-ast`.
//!
//! The implementation is a hand-written LL(1) parser: it reads the input from
//! left to right, builds a leftmost derivation, and every grammar decision is
//! made with one token of lookahead. The usual expression grammar is transformed
//! by eliminating left recursion and encoding precedence as LL(1) levels
//! (`or`, `and`, `equality`, `comparison`, `term`, `factor`, ...). Repetition
//! tails such as `E' -> + T E' | epsilon` are implemented as loops.
//!
//! The resulting AST intentionally omits grammar-only helper nodes and keeps only
//! semantic structure.

use std::fmt;

use hulk_ast::{
    AssignExpr, AssignTarget, AttributeDecl, BinaryOp, BlockExpr, Declaration, DeclarationKind,
    DowncastExpr, ElifBranch, Expr, ExprKind, ForExpr, FunctionDecl, IfExpr, IndexExpr, LetBinding,
    LetExpr, Literal, MatchCase, MatchExpr, MemberExpr, NewExpr, Param, Pattern, Program,
    ProtocolDecl, ProtocolMethod, SourceSpan, TypeDecl, TypeMember, TypeMemberKind, TypeParent,
    TypeRef, TypeTestExpr, UnaryOp, VectorComprehension, VectorExpr, WhileExpr,
};
use hulk_lexer::{Span, Token, TokenKind};

/// Parses a complete HULK program from a token stream using the LL(1) parser.
pub fn parse(tokens: Vec<Token>) -> Result<Program, ParseError> {
    Ll1Parser::new(tokens).parse_program()
}

/// Public name that makes the chosen parsing strategy explicit.
///
/// `Parser` is kept as a type alias for compatibility with older code that was
/// already importing `hulk_parser::Parser`.
pub type Parser = Ll1Parser;

/// Error produced by the parser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub kind: ParseErrorKind,
    pub span: SourceSpan,
}

impl ParseError {
    fn new(kind: ParseErrorKind, span: SourceSpan) -> Self {
        Self { kind, span }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ParseErrorKind::UnexpectedToken { expected, found } => write!(
                f,
                "parse error at line {}, col {}: expected {}, found {}",
                self.span.line, self.span.col, expected, found
            ),
            ParseErrorKind::ExpectedExpression { found } => write!(
                f,
                "parse error at line {}, col {}: expected expression, found {}",
                self.span.line, self.span.col, found
            ),
            ParseErrorKind::ExpectedIdentifier { found } => write!(
                f,
                "parse error at line {}, col {}: expected identifier, found {}",
                self.span.line, self.span.col, found
            ),
            ParseErrorKind::InvalidAssignmentTarget => write!(
                f,
                "parse error at line {}, col {}: invalid assignment target",
                self.span.line, self.span.col
            ),
            ParseErrorKind::Message(message) => write!(
                f,
                "parse error at line {}, col {}: {}",
                self.span.line, self.span.col, message
            ),
        }
    }
}

impl std::error::Error for ParseError {}

/// Specific parser error kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseErrorKind {
    UnexpectedToken { expected: String, found: String },
    ExpectedExpression { found: String },
    ExpectedIdentifier { found: String },
    InvalidAssignmentTarget,
    Message(String),
}

/// Predictive LL(1) recursive-descent parser.
///
/// The code mirrors a table-driven LL(1) parser, but instead of storing semantic
/// actions in a separate parsing table, every non-terminal has a Rust method.
/// The `check_*` helper methods below are the FIRST/FOLLOW predicates used to
/// choose the single valid production for the current lookahead token.
pub struct Ll1Parser {
    tokens: Vec<Token>,
    current: usize,
}

impl Ll1Parser {
    /// Creates a parser. If the caller forgot to append EOF, the parser adds it.
    pub fn new(mut tokens: Vec<Token>) -> Self {
        let needs_eof = tokens
            .last()
            .map(|token| !same_variant(&token.kind, &TokenKind::Eof))
            .unwrap_or(true);

        if needs_eof {
            tokens.push(Token {
                kind: TokenKind::Eof,
                span: Span { line: 0, col: 0 },
            });
        }

        Self { tokens, current: 0 }
    }

    /// Parses a full HULK program: zero or more declarations followed by the
    /// mandatory global expression that works as entry point.
    pub fn parse_program(&mut self) -> Result<Program, ParseError> {
        let mut declarations = Vec::new();

        while self.lookahead_starts_declaration() {
            declarations.push(self.parse_declaration()?);
        }

        if !self.lookahead_starts_expression() {
            return Err(ParseError::new(
                ParseErrorKind::ExpectedExpression {
                    found: token_kind_name(&self.peek().kind),
                },
                self.peek_span(),
            ));
        }

        let entry = self.parse_expression()?;
        self.consume_optional_semicolons();
        self.consume(&TokenKind::Eof, "end of file")?;

        Ok(Program::new(declarations, entry))
    }

    fn parse_declaration(&mut self) -> Result<Declaration, ParseError> {
        let span = self.peek_span();

        match &self.peek().kind {
            TokenKind::Function => {
                self.advance();
                let function = self.parse_function_declaration_after_keyword()?;
                Ok(Declaration::new(DeclarationKind::Function(function), span))
            }
            TokenKind::Type => {
                self.advance();
                let type_decl = self.parse_type_declaration_after_keyword()?;
                Ok(Declaration::new(DeclarationKind::Type(type_decl), span))
            }
            TokenKind::Protocol => {
                self.advance();
                let protocol_decl = self.parse_protocol_declaration_after_keyword()?;
                Ok(Declaration::new(
                    DeclarationKind::Protocol(protocol_decl),
                    span,
                ))
            }
            _ => Err(self.error_unexpected("declaration")),
        }
    }

    fn parse_function_declaration_after_keyword(&mut self) -> Result<FunctionDecl, ParseError> {
        let name = self.consume_identifier()?;
        self.parse_function_tail(name)
    }

    fn parse_function_tail(&mut self, name: String) -> Result<FunctionDecl, ParseError> {
        let params = self.parse_param_list()?;
        let return_type = if self.match_kind(&TokenKind::Colon) {
            Some(self.parse_type_ref()?)
        } else {
            None
        };

        let body = if self.match_kind(&TokenKind::FatArrow) {
            let expression = self.parse_expression()?;
            self.match_kind(&TokenKind::Semicolon);
            expression
        } else if self.check(&TokenKind::LBrace) {
            self.parse_block_expression()?
        } else {
            return Err(self.error_unexpected("`=>` or function block"));
        };

        Ok(FunctionDecl::new(name, params, return_type, body))
    }

    fn parse_type_declaration_after_keyword(&mut self) -> Result<TypeDecl, ParseError> {
        let name = self.consume_identifier()?;
        let params = if self.check(&TokenKind::LParen) {
            self.parse_param_list()?
        } else {
            Vec::new()
        };

        let parent = if self.match_kind(&TokenKind::Inherits) {
            let parent_name = self.consume_identifier()?;
            let args = if self.match_kind(&TokenKind::LParen) {
                self.parse_argument_list_after_lparen()?
            } else {
                Vec::new()
            };
            Some(TypeParent::new(parent_name, args))
        } else {
            None
        };

        self.consume(&TokenKind::LBrace, "`{` before type body")?;
        let mut members = Vec::new();

        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            if self.match_kind(&TokenKind::Semicolon) {
                continue;
            }
            members.push(self.parse_type_member()?);
        }

        self.consume(&TokenKind::RBrace, "`}` after type body")?;

        Ok(TypeDecl::new(name, params, parent, members))
    }

    fn parse_type_member(&mut self) -> Result<TypeMember, ParseError> {
        let span = self.peek_span();

        if self.match_kind(&TokenKind::Function) {
            let method = self.parse_function_declaration_after_keyword()?;
            return Ok(TypeMember::new(TypeMemberKind::Method(method), span));
        }

        let name = self.parse_name()?;

        if self.check(&TokenKind::LParen) {
            let method = self.parse_function_tail(name)?;
            return Ok(TypeMember::new(TypeMemberKind::Method(method), span));
        }

        let type_annotation = if self.match_kind(&TokenKind::Colon) {
            Some(self.parse_type_ref()?)
        } else {
            None
        };

        self.consume(&TokenKind::Assign, "`=` in attribute declaration")?;
        let initializer = self.parse_expression()?;
        self.consume(&TokenKind::Semicolon, "`;` after attribute declaration")?;

        Ok(TypeMember::new(
            TypeMemberKind::Attribute(AttributeDecl::new(name, type_annotation, initializer)),
            span,
        ))
    }

    fn parse_protocol_declaration_after_keyword(&mut self) -> Result<ProtocolDecl, ParseError> {
        let name = self.consume_identifier()?;
        let mut parents = Vec::new();

        if self.match_kind(&TokenKind::Extends) {
            loop {
                parents.push(self.parse_type_ref()?);
                if !self.match_kind(&TokenKind::Comma) {
                    break;
                }
            }
        }

        self.consume(&TokenKind::LBrace, "`{` before protocol body")?;
        let mut methods = Vec::new();

        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            if self.match_kind(&TokenKind::Semicolon) {
                continue;
            }

            let method_name = self.parse_name()?;
            let params = self.parse_param_list()?;
            self.consume(&TokenKind::Colon, "return type in protocol method")?;
            let return_type = self.parse_type_ref()?;
            self.consume(&TokenKind::Semicolon, "`;` after protocol method")?;
            methods.push(ProtocolMethod::new(method_name, params, return_type));
        }

        self.consume(&TokenKind::RBrace, "`}` after protocol body")?;

        Ok(ProtocolDecl::new(name, parents, methods))
    }

    fn parse_param_list(&mut self) -> Result<Vec<Param>, ParseError> {
        self.consume(&TokenKind::LParen, "`(` before parameter list")?;
        let mut params = Vec::new();

        if !self.check(&TokenKind::RParen) {
            loop {
                let name = self.parse_name()?;
                let type_annotation = if self.match_kind(&TokenKind::Colon) {
                    Some(self.parse_type_ref()?)
                } else {
                    None
                };
                params.push(Param::new(name, type_annotation));

                if !self.match_kind(&TokenKind::Comma) {
                    break;
                }
            }
        }

        self.consume(&TokenKind::RParen, "`)` after parameter list")?;
        Ok(params)
    }

    fn parse_argument_list_after_lparen(&mut self) -> Result<Vec<Expr>, ParseError> {
        let mut args = Vec::new();

        if !self.check(&TokenKind::RParen) {
            loop {
                args.push(self.parse_expression()?);
                if !self.match_kind(&TokenKind::Comma) {
                    break;
                }
            }
        }

        self.consume(&TokenKind::RParen, "`)` after argument list")?;
        Ok(args)
    }

    fn parse_type_ref(&mut self) -> Result<TypeRef, ParseError> {
        let name = self.consume_identifier()?;
        let mut ty = if self.match_kind(&TokenKind::Lt) {
            let mut args = Vec::new();
            loop {
                args.push(self.parse_type_ref()?);
                if !self.match_kind(&TokenKind::Comma) {
                    break;
                }
            }
            self.consume(&TokenKind::Gt, "`>` after type arguments")?;
            TypeRef::with_args(name, args)
        } else {
            TypeRef::named(name)
        };

        loop {
            if self.match_kind(&TokenKind::Star) {
                ty = TypeRef::with_args("Iterable", vec![ty]);
            } else if self.match_kind(&TokenKind::LBracket) {
                self.consume(&TokenKind::RBracket, "`]` after vector type suffix")?;
                ty = TypeRef::with_args("Vector", vec![ty]);
            } else {
                break;
            }
        }

        Ok(ty)
    }

    /// Lowest-precedence expression entry point.
    fn parse_expression(&mut self) -> Result<Expr, ParseError> {
        self.parse_assignment()
    }

    fn parse_assignment(&mut self) -> Result<Expr, ParseError> {
        let expr = self.parse_or()?;

        if self.match_kind(&TokenKind::ColonEq) {
            let span = expr.span;
            let value = self.parse_assignment()?;
            let target = Self::assignment_target_from_expr(expr)?;
            return Ok(Expr::new(
                ExprKind::Assign(AssignExpr::new(target, value)),
                span,
            ));
        }

        Ok(expr)
    }

    fn parse_assignment_without_or(&mut self) -> Result<Expr, ParseError> {
        let expr = self.parse_and()?;

        if self.match_kind(&TokenKind::ColonEq) {
            let span = expr.span;
            let value = self.parse_assignment_without_or()?;
            let target = Self::assignment_target_from_expr(expr)?;
            return Ok(Expr::new(
                ExprKind::Assign(AssignExpr::new(target, value)),
                span,
            ));
        }

        Ok(expr)
    }

    fn assignment_target_from_expr(expr: Expr) -> Result<AssignTarget, ParseError> {
        let span = expr.span;
        match expr.kind {
            ExprKind::Variable(name) => Ok(AssignTarget::Variable(name)),
            ExprKind::Member(member) => Ok(AssignTarget::Member {
                object: member.object,
                field: member.member,
            }),
            ExprKind::Index(index) => Ok(AssignTarget::Index {
                object: index.object,
                index: index.index,
            }),
            _ => Err(ParseError::new(
                ParseErrorKind::InvalidAssignmentTarget,
                span,
            )),
        }
    }

    fn parse_or(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_and()?;

        while self.match_kind(&TokenKind::Or) {
            let span = expr.span;
            let right = self.parse_and()?;
            expr = Expr::binary(BinaryOp::Or, expr, right, span);
        }

        Ok(expr)
    }

    fn parse_and(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_equality()?;

        while self.match_kind(&TokenKind::And) {
            let span = expr.span;
            let right = self.parse_equality()?;
            expr = Expr::binary(BinaryOp::And, expr, right, span);
        }

        Ok(expr)
    }

    fn parse_equality(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_comparison()?;

        loop {
            let op = if self.match_kind(&TokenKind::EqEq) {
                Some(BinaryOp::Equal)
            } else if self.match_kind(&TokenKind::Neq) {
                Some(BinaryOp::NotEqual)
            } else {
                None
            };

            match op {
                Some(op) => {
                    let span = expr.span;
                    let right = self.parse_comparison()?;
                    expr = Expr::binary(op, expr, right, span);
                }
                None => break,
            }
        }

        Ok(expr)
    }

    fn parse_comparison(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_type_test()?;

        loop {
            let op = if self.match_kind(&TokenKind::Lt) {
                Some(BinaryOp::Less)
            } else if self.match_kind(&TokenKind::Leq) {
                Some(BinaryOp::LessEqual)
            } else if self.match_kind(&TokenKind::Gt) {
                Some(BinaryOp::Greater)
            } else if self.match_kind(&TokenKind::Geq) {
                Some(BinaryOp::GreaterEqual)
            } else {
                None
            };

            match op {
                Some(op) => {
                    let span = expr.span;
                    let right = self.parse_type_test()?;
                    expr = Expr::binary(op, expr, right, span);
                }
                None => break,
            }
        }

        Ok(expr)
    }

    fn parse_type_test(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_concat()?;

        loop {
            if self.match_kind(&TokenKind::Is) {
                let span = expr.span;
                let type_name = self.parse_type_ref()?;
                expr = Expr::new(ExprKind::TypeTest(TypeTestExpr::new(expr, type_name)), span);
            } else if self.match_kind(&TokenKind::As) {
                let span = expr.span;
                let type_name = self.parse_type_ref()?;
                expr = Expr::new(ExprKind::Downcast(DowncastExpr::new(expr, type_name)), span);
            } else {
                break;
            }
        }

        Ok(expr)
    }

    fn parse_concat(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_term()?;

        loop {
            let op = if self.match_kind(&TokenKind::At) {
                Some(BinaryOp::Concat)
            } else if self.match_kind(&TokenKind::AtAt) {
                Some(BinaryOp::ConcatSpace)
            } else {
                None
            };

            match op {
                Some(op) => {
                    let span = expr.span;
                    let right = self.parse_term()?;
                    expr = Expr::binary(op, expr, right, span);
                }
                None => break,
            }
        }

        Ok(expr)
    }

    fn parse_term(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_factor()?;

        loop {
            let op = if self.match_kind(&TokenKind::Plus) {
                Some(BinaryOp::Add)
            } else if self.match_kind(&TokenKind::Minus) {
                Some(BinaryOp::Subtract)
            } else {
                None
            };

            match op {
                Some(op) => {
                    let span = expr.span;
                    let right = self.parse_factor()?;
                    expr = Expr::binary(op, expr, right, span);
                }
                None => break,
            }
        }

        Ok(expr)
    }

    fn parse_factor(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_unary()?;

        loop {
            let op = if self.match_kind(&TokenKind::Star) {
                Some(BinaryOp::Multiply)
            } else if self.match_kind(&TokenKind::Slash) {
                Some(BinaryOp::Divide)
            } else if self.match_kind(&TokenKind::Percent) {
                Some(BinaryOp::Modulo)
            } else {
                None
            };

            match op {
                Some(op) => {
                    let span = expr.span;
                    let right = self.parse_unary()?;
                    expr = Expr::binary(op, expr, right, span);
                }
                None => break,
            }
        }

        Ok(expr)
    }

    fn parse_unary(&mut self) -> Result<Expr, ParseError> {
        if self.match_kind(&TokenKind::Minus) {
            let span = self.previous_span();
            let expr = self.parse_unary()?;
            return Ok(Expr::unary(UnaryOp::Negate, expr, span));
        }

        if self.match_kind(&TokenKind::Not) {
            let span = self.previous_span();
            let expr = self.parse_unary()?;
            return Ok(Expr::unary(UnaryOp::Not, expr, span));
        }

        self.parse_power()
    }

    fn parse_power(&mut self) -> Result<Expr, ParseError> {
        let expr = self.parse_postfix()?;

        if self.match_kind(&TokenKind::Caret) {
            let span = expr.span;
            let right = self.parse_unary()?;
            return Ok(Expr::binary(BinaryOp::Power, expr, right, span));
        }

        Ok(expr)
    }

    fn parse_postfix(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_primary()?;

        loop {
            if self.match_kind(&TokenKind::LParen) {
                let span = expr.span;
                // WHY: `base(args)` is method-delegation syntax (§A.7.4). Promote
                // Variable("base") to BaseRef so the semantic pass handles delegation.
                // In all other positions (e.g. `base.foo`, `let base = ...`) the
                // Variable node remains, allowing regular variable lookup.
                let is_base_var = matches!(&expr.kind, ExprKind::Variable(n) if n == "base");
                let callee = if is_base_var {
                    Expr::new(ExprKind::BaseRef, expr.span)
                } else {
                    expr
                };
                let args = self.parse_argument_list_after_lparen()?;
                expr = Expr::call(callee, args, span);
            } else if self.match_kind(&TokenKind::Dot) {
                let span = expr.span;
                let member = self.parse_name()?;
                expr = Expr::new(ExprKind::Member(MemberExpr::new(expr, member)), span);
            } else if self.match_kind(&TokenKind::LBracket) {
                let span = expr.span;
                let index = self.parse_expression()?;
                self.consume(&TokenKind::RBracket, "`]` after index expression")?;
                expr = Expr::new(ExprKind::Index(IndexExpr::new(expr, index)), span);
            } else {
                break;
            }
        }

        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        if !self.lookahead_starts_primary() {
            return Err(ParseError::new(
                ParseErrorKind::ExpectedExpression {
                    found: token_kind_name(&self.peek().kind),
                },
                self.peek_span(),
            ));
        }

        let token = self.advance();
        let span = token_span(token.span);

        match token.kind {
            TokenKind::Number(value) => Ok(Expr::number(value, span)),
            TokenKind::StringLit(value) => Ok(Expr::string(value, span)),
            TokenKind::True => Ok(Expr::boolean(true, span)),
            TokenKind::False => Ok(Expr::boolean(false, span)),
            TokenKind::Ident(name) => Ok(Expr::variable(name, span)),
            TokenKind::SelfKw => Ok(Expr::new(ExprKind::SelfRef, span)),
            // WHY: `base` is a symbol (§A.7.4), not a keyword — it can be shadowed by a
            // variable (like `let base: Printer = ...`). Emit Variable("base") here;
            // parse_postfix promotes it to BaseRef only when immediately followed by `(`
            // (the method-delegation call site).
            TokenKind::Base => Ok(Expr::variable("base".to_string(), span)),
            TokenKind::LParen => {
                let expr = self.parse_expression()?;
                self.consume(&TokenKind::RParen, "`)` after expression")?;
                Ok(expr)
            }
            TokenKind::LBrace => self.finish_block_expression(span),
            TokenKind::LBracket => self.finish_vector_expression(span),
            TokenKind::Let => self.finish_let_expression(span),
            TokenKind::If => self.finish_if_expression(span),
            TokenKind::While => self.finish_while_expression(span),
            TokenKind::For => self.finish_for_expression(span),
            TokenKind::New => self.finish_new_expression(span),
            TokenKind::Match => self.finish_match_expression(span),
            other => Err(ParseError::new(
                ParseErrorKind::ExpectedExpression {
                    found: token_kind_name(&other),
                },
                span,
            )),
        }
    }

    fn parse_block_expression(&mut self) -> Result<Expr, ParseError> {
        let brace = self.consume(&TokenKind::LBrace, "`{` before expression block")?;
        self.finish_block_expression(token_span(brace.span))
    }

    fn finish_block_expression(&mut self, span: SourceSpan) -> Result<Expr, ParseError> {
        let mut expressions = Vec::new();

        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            if self.match_kind(&TokenKind::Semicolon) {
                continue;
            }

            expressions.push(self.parse_expression()?);

            if self.match_kind(&TokenKind::Semicolon) {
                continue;
            }

            if !self.check(&TokenKind::RBrace) {
                self.consume(&TokenKind::Semicolon, "`;` between block expressions")?;
            }
        }

        self.consume(&TokenKind::RBrace, "`}` after expression block")?;
        Ok(Expr::new(
            ExprKind::Block(BlockExpr::new(expressions)),
            span,
        ))
    }

    fn finish_vector_expression(&mut self, span: SourceSpan) -> Result<Expr, ParseError> {
        if self.match_kind(&TokenKind::RBracket) {
            return Ok(Expr::new(
                ExprKind::Vector(VectorExpr::Literal(Vec::new())),
                span,
            ));
        }

        let first = self.parse_assignment_without_or()?;

        if self.match_kind(&TokenKind::Or) {
            let var = self.parse_name()?;
            self.consume(&TokenKind::In, "`in` in vector comprehension")?;
            let iterable = self.parse_expression()?;
            self.consume(&TokenKind::RBracket, "`]` after vector comprehension")?;

            return Ok(Expr::new(
                ExprKind::Vector(VectorExpr::Comprehension(VectorComprehension::new(
                    first, var, iterable,
                ))),
                span,
            ));
        }

        let mut items = vec![first];
        while self.match_kind(&TokenKind::Comma) {
            if self.check(&TokenKind::RBracket) {
                break;
            }
            items.push(self.parse_expression()?);
        }

        self.consume(&TokenKind::RBracket, "`]` after vector literal")?;
        Ok(Expr::new(
            ExprKind::Vector(VectorExpr::Literal(items)),
            span,
        ))
    }

    fn finish_let_expression(&mut self, span: SourceSpan) -> Result<Expr, ParseError> {
        let mut bindings = Vec::new();

        loop {
            let name = self.parse_name()?;
            let type_annotation = if self.match_kind(&TokenKind::Colon) {
                Some(self.parse_type_ref()?)
            } else {
                None
            };

            self.consume(&TokenKind::Assign, "`=` in let binding")?;
            let initializer = self.parse_expression()?;
            bindings.push(LetBinding::new(name, type_annotation, initializer));

            if !self.match_kind(&TokenKind::Comma) {
                break;
            }
        }

        self.consume(&TokenKind::In, "`in` after let bindings")?;
        let body = self.parse_expression()?;

        Ok(Expr::new(ExprKind::Let(LetExpr::new(bindings, body)), span))
    }

    fn finish_if_expression(&mut self, span: SourceSpan) -> Result<Expr, ParseError> {
        let condition = self.parse_parenthesized_expression("if condition")?;
        let then_branch = self.parse_expression()?;
        let mut elif_branches = Vec::new();

        while self.match_kind(&TokenKind::Elif) {
            let elif_condition = self.parse_parenthesized_expression("elif condition")?;
            let body = self.parse_expression()?;
            elif_branches.push(ElifBranch::new(elif_condition, body));
        }

        self.consume(&TokenKind::Else, "`else` branch in if expression")?;
        let else_branch = self.parse_expression()?;

        Ok(Expr::new(
            ExprKind::If(IfExpr::new(
                condition,
                then_branch,
                elif_branches,
                else_branch,
            )),
            span,
        ))
    }

    fn finish_while_expression(&mut self, span: SourceSpan) -> Result<Expr, ParseError> {
        let condition = self.parse_parenthesized_expression("while condition")?;
        let body = self.parse_expression()?;

        Ok(Expr::new(
            ExprKind::While(WhileExpr::new(condition, body)),
            span,
        ))
    }

    fn finish_for_expression(&mut self, span: SourceSpan) -> Result<Expr, ParseError> {
        self.consume(&TokenKind::LParen, "`(` before for binding")?;
        let var = self.parse_name()?;
        self.consume(&TokenKind::In, "`in` inside for binding")?;
        let iterable = self.parse_expression()?;
        self.consume(&TokenKind::RParen, "`)` after for binding")?;
        let body = self.parse_expression()?;

        Ok(Expr::new(
            ExprKind::For(ForExpr::new(var, iterable, body)),
            span,
        ))
    }

    fn finish_new_expression(&mut self, span: SourceSpan) -> Result<Expr, ParseError> {
        let type_name = self.parse_type_ref()?;
        let args = if self.match_kind(&TokenKind::LParen) {
            self.parse_argument_list_after_lparen()?
        } else {
            Vec::new()
        };

        Ok(Expr::new(
            ExprKind::New(NewExpr::new(type_name, args)),
            span,
        ))
    }

    fn finish_match_expression(&mut self, span: SourceSpan) -> Result<Expr, ParseError> {
        let value = self.parse_expression()?;
        self.consume(&TokenKind::LBrace, "`{` before match cases")?;
        let mut cases = Vec::new();

        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            self.consume(&TokenKind::Case, "`case` in match expression")?;
            let pattern = self.parse_pattern()?;
            self.consume(&TokenKind::FatArrow, "`=>` after match pattern")?;
            let body = self.parse_expression()?;
            self.match_kind(&TokenKind::Semicolon);
            cases.push(MatchCase::new(pattern, body));
        }

        self.consume(&TokenKind::RBrace, "`}` after match cases")?;
        Ok(Expr::new(
            ExprKind::Match(MatchExpr::new(value, cases)),
            span,
        ))
    }

    fn parse_pattern(&mut self) -> Result<Pattern, ParseError> {
        let token = self.advance();

        match token.kind {
            TokenKind::Underscore => Ok(Pattern::Wildcard),
            TokenKind::Number(value) => Ok(Pattern::Literal(Literal::Number(value))),
            TokenKind::StringLit(value) => Ok(Pattern::Literal(Literal::String(value))),
            TokenKind::True => Ok(Pattern::Literal(Literal::Boolean(true))),
            TokenKind::False => Ok(Pattern::Literal(Literal::Boolean(false))),
            TokenKind::Ident(name) => {
                if self.match_kind(&TokenKind::Colon) {
                    let alias = Some(name);
                    let type_name = self.parse_type_ref()?;
                    Ok(Pattern::Type(type_name, alias))
                } else {
                    Ok(Pattern::Variable(name))
                }
            }
            other => Err(ParseError::new(
                ParseErrorKind::Message(format!(
                    "invalid match pattern: {}",
                    token_kind_name(&other)
                )),
                token_span(token.span),
            )),
        }
    }

    fn parse_parenthesized_expression(&mut self, context: &str) -> Result<Expr, ParseError> {
        self.consume(&TokenKind::LParen, &format!("`(` before {context}"))?;
        let expr = self.parse_expression()?;
        self.consume(&TokenKind::RParen, &format!("`)` after {context}"))?;
        Ok(expr)
    }

    /// FIRST(Declaration) = { function, type, protocol }.
    fn lookahead_starts_declaration(&self) -> bool {
        matches!(
            &self.peek().kind,
            TokenKind::Function | TokenKind::Type | TokenKind::Protocol
        )
    }

    /// FIRST(Expr) for HULK's expression grammar.
    fn lookahead_starts_expression(&self) -> bool {
        self.lookahead_starts_primary()
            || matches!(&self.peek().kind, TokenKind::Minus | TokenKind::Not)
    }

    /// FIRST(Primary), after expression precedence has been factored out.
    fn lookahead_starts_primary(&self) -> bool {
        matches!(
            &self.peek().kind,
            TokenKind::Number(_)
                | TokenKind::StringLit(_)
                | TokenKind::True
                | TokenKind::False
                | TokenKind::Ident(_)
                | TokenKind::SelfKw
                | TokenKind::Base
                | TokenKind::LParen
                | TokenKind::LBrace
                | TokenKind::LBracket
                | TokenKind::Let
                | TokenKind::If
                | TokenKind::While
                | TokenKind::For
                | TokenKind::New
                | TokenKind::Match
        )
    }

    fn consume_identifier(&mut self) -> Result<String, ParseError> {
        let token = self.advance();
        match token.kind {
            TokenKind::Ident(name) => Ok(name),
            other => Err(ParseError::new(
                ParseErrorKind::ExpectedIdentifier {
                    found: token_kind_name(&other),
                },
                token_span(token.span),
            )),
        }
    }

    /// Parses an identifier name, also accepting `base` as a valid identifier.
    ///
    /// WHY: HULK spec §A.7.1 describes `self` as "not a keyword, which means it
    /// can be hidden by a let expression or method argument." The spec similarly
    /// describes `base` as a "symbol", not a keyword (§A.7.4). Following the
    /// SoulNG / C# contextual keyword pattern: the lexer emits TokenKind::Base
    /// unconditionally, but the parser accepts it as a plain identifier name in
    /// all positions except method-delegation calls (`base(args)`).
    fn parse_name(&mut self) -> Result<String, ParseError> {
        let token = self.advance();
        match token.kind {
            TokenKind::Ident(name) => Ok(name),
            TokenKind::Base => Ok("base".to_string()),
            other => Err(ParseError::new(
                ParseErrorKind::ExpectedIdentifier {
                    found: token_kind_name(&other),
                },
                token_span(token.span),
            )),
        }
    }

    fn consume(&mut self, kind: &TokenKind, expected: &str) -> Result<Token, ParseError> {
        if self.check(kind) {
            Ok(self.advance())
        } else {
            Err(self.error_unexpected(expected))
        }
    }

    fn consume_optional_semicolons(&mut self) {
        while self.match_kind(&TokenKind::Semicolon) {}
    }

    fn match_kind(&mut self, kind: &TokenKind) -> bool {
        if self.check(kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn check(&self, kind: &TokenKind) -> bool {
        same_variant(&self.peek().kind, kind)
    }

    fn advance(&mut self) -> Token {
        let token = self.peek().clone();
        if !self.is_at_end() {
            self.current += 1;
        }
        token
    }

    fn is_at_end(&self) -> bool {
        self.check(&TokenKind::Eof)
    }

    fn peek(&self) -> &Token {
        let last_index = self.tokens.len().saturating_sub(1);
        let index = self.current.min(last_index);
        &self.tokens[index]
    }

    fn previous_span(&self) -> SourceSpan {
        let index = self.current.saturating_sub(1);
        token_span(self.tokens[index].span)
    }

    fn peek_span(&self) -> SourceSpan {
        token_span(self.peek().span)
    }

    fn error_unexpected(&self, expected: &str) -> ParseError {
        let token = self.peek();
        ParseError::new(
            ParseErrorKind::UnexpectedToken {
                expected: expected.to_string(),
                found: token_kind_name(&token.kind),
            },
            token_span(token.span),
        )
    }
}

fn same_variant(a: &TokenKind, b: &TokenKind) -> bool {
    std::mem::discriminant(a) == std::mem::discriminant(b)
}

fn token_span(span: Span) -> SourceSpan {
    SourceSpan::new(span.line, span.col)
}

fn token_kind_name(kind: &TokenKind) -> String {
    match kind {
        TokenKind::Number(value) => format!("number literal `{value}`"),
        TokenKind::StringLit(value) => format!("string literal {value:?}"),
        TokenKind::True => "`true`".to_string(),
        TokenKind::False => "`false`".to_string(),
        TokenKind::Plus => "`+`".to_string(),
        TokenKind::Minus => "`-`".to_string(),
        TokenKind::Star => "`*`".to_string(),
        TokenKind::Slash => "`/`".to_string(),
        TokenKind::Caret => "`^`".to_string(),
        TokenKind::Percent => "`%`".to_string(),
        TokenKind::At => "`@`".to_string(),
        TokenKind::AtAt => "`@@`".to_string(),
        TokenKind::EqEq => "`==`".to_string(),
        TokenKind::Neq => "`!=`".to_string(),
        TokenKind::Lt => "`<`".to_string(),
        TokenKind::Gt => "`>`".to_string(),
        TokenKind::Leq => "`<=`".to_string(),
        TokenKind::Geq => "`>=`".to_string(),
        TokenKind::And => "`&`".to_string(),
        TokenKind::Or => "`|`".to_string(),
        TokenKind::Not => "`!`".to_string(),
        TokenKind::Assign => "`=`".to_string(),
        TokenKind::ColonEq => "`:=`".to_string(),
        TokenKind::LParen => "`(`".to_string(),
        TokenKind::RParen => "`)`".to_string(),
        TokenKind::LBrace => "`{`".to_string(),
        TokenKind::RBrace => "`}`".to_string(),
        TokenKind::LBracket => "`[`".to_string(),
        TokenKind::RBracket => "`]`".to_string(),
        TokenKind::Semicolon => "`;`".to_string(),
        TokenKind::Comma => "`,`".to_string(),
        TokenKind::Colon => "`:`".to_string(),
        TokenKind::Dot => "`.`".to_string(),
        TokenKind::Arrow => "`->`".to_string(),
        TokenKind::FatArrow => "`=>`".to_string(),
        TokenKind::Let => "`let`".to_string(),
        TokenKind::In => "`in`".to_string(),
        TokenKind::If => "`if`".to_string(),
        TokenKind::Elif => "`elif`".to_string(),
        TokenKind::Else => "`else`".to_string(),
        TokenKind::While => "`while`".to_string(),
        TokenKind::For => "`for`".to_string(),
        TokenKind::Function => "`function`".to_string(),
        TokenKind::Type => "`type`".to_string(),
        TokenKind::Inherits => "`inherits`".to_string(),
        TokenKind::New => "`new`".to_string(),
        TokenKind::SelfKw => "`self`".to_string(),
        TokenKind::Base => "`base`".to_string(),
        TokenKind::Is => "`is`".to_string(),
        TokenKind::As => "`as`".to_string(),
        TokenKind::Protocol => "`protocol`".to_string(),
        TokenKind::Extends => "`extends`".to_string(),
        TokenKind::Def => "`def`".to_string(),
        TokenKind::Match => "`match`".to_string(),
        TokenKind::Case => "`case`".to_string(),
        TokenKind::Underscore => "`_`".to_string(),
        TokenKind::Ident(name) => format!("identifier `{name}`"),
        TokenKind::Eof => "end of file".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hulk_lexer::Lexer;

    fn parse_source(source: &str) -> Program {
        let tokens = Lexer::new(source).tokenize().expect("valid tokens");
        parse(tokens).expect("valid parse")
    }

    fn parse_source_with_ll1_parser(source: &str) -> Program {
        let tokens = Lexer::new(source).tokenize().expect("valid tokens");
        Ll1Parser::new(tokens).parse_program().expect("valid parse")
    }

    #[test]
    fn parses_arithmetic_precedence_inside_call() {
        let program = parse_source("print(1 + 2 * 3);");

        match program.entry.kind {
            ExprKind::Call(call) => match &call.args[0].kind {
                ExprKind::Binary(binary) => {
                    assert_eq!(binary.op, BinaryOp::Add);
                    assert!(matches!(binary.right.kind, ExprKind::Binary(_)));
                }
                other => panic!("expected binary argument, got {other:?}"),
            },
            other => panic!("expected call entry, got {other:?}"),
        }
    }

    #[test]
    fn parses_function_declaration_and_entry_expression() {
        let program =
            parse_source("function tan(x: Number): Number => sin(x) / cos(x); print(tan(PI));");

        assert_eq!(program.declarations.len(), 1);
        match &program.declarations[0].kind {
            DeclarationKind::Function(function) => {
                assert_eq!(function.name, "tan");
                assert_eq!(function.params.len(), 1);
                assert_eq!(
                    function.return_type.as_ref().map(ToString::to_string),
                    Some("Number".to_string())
                );
            }
            other => panic!("expected function declaration, got {other:?}"),
        }
    }

    #[test]
    fn parses_let_expression_with_type_annotation() {
        let program = parse_source("let x: Number = 5 in x + 1;");

        match program.entry.kind {
            ExprKind::Let(let_expr) => {
                assert_eq!(let_expr.bindings[0].name, "x");
                assert_eq!(
                    let_expr.bindings[0]
                        .type_annotation
                        .as_ref()
                        .map(ToString::to_string),
                    Some("Number".to_string())
                );
                assert!(matches!(let_expr.body.kind, ExprKind::Binary(_)));
            }
            other => panic!("expected let entry, got {other:?}"),
        }
    }

    #[test]
    fn parses_type_with_attribute_and_methods() {
        let program = parse_source(
            r#"
            type A {
                value: Number = 42;
                f() => "Hello";
                g(): String => "World";
            }
            print(new A().f());
            "#,
        );

        assert_eq!(program.declarations.len(), 1);
        match &program.declarations[0].kind {
            DeclarationKind::Type(type_decl) => {
                assert_eq!(type_decl.name, "A");
                assert_eq!(type_decl.members.len(), 3);
            }
            other => panic!("expected type declaration, got {other:?}"),
        }
    }

    #[test]
    fn parses_control_flow_expressions() {
        let program = parse_source(
            r#"
            let a = 10 in while (a >= 0) {
                print(a);
                a := a - 1;
            };
            "#,
        );

        match program.entry.kind {
            ExprKind::Let(let_expr) => assert!(matches!(let_expr.body.kind, ExprKind::While(_))),
            other => panic!("expected let entry, got {other:?}"),
        }
    }

    #[test]
    fn public_ll1_parser_type_parses_programs() {
        let program = parse_source_with_ll1_parser("print(42);");
        assert!(matches!(program.entry.kind, ExprKind::Call(_)));
    }

    #[test]
    fn parses_vector_comprehension() {
        let program = parse_source("let xs = [x^2 | x in range(1, 10)] in xs[0];");

        match program.entry.kind {
            ExprKind::Let(let_expr) => {
                assert!(matches!(
                    let_expr.bindings[0].initializer.kind,
                    ExprKind::Vector(VectorExpr::Comprehension(_))
                ));
                assert!(matches!(let_expr.body.kind, ExprKind::Index(_)));
            }
            other => panic!("expected let entry, got {other:?}"),
        }
    }

    #[test]
    fn parses_or_inside_parenthesized_vector_comprehension_head() {
        let program = parse_source("let xs = [(x | y) | x in values] in xs[0];");

        match program.entry.kind {
            ExprKind::Let(let_expr) => {
                assert!(matches!(
                    let_expr.bindings[0].initializer.kind,
                    ExprKind::Vector(VectorExpr::Comprehension(_))
                ));
            }
            other => panic!("expected let entry, got {other:?}"),
        }
    }

    #[test]
    fn rejects_invalid_assignment_target() {
        let tokens = Lexer::new("(1 + 2) := 3;")
            .tokenize()
            .expect("valid tokens");
        let error = parse(tokens).expect_err("invalid assignment target");
        assert_eq!(error.kind, ParseErrorKind::InvalidAssignmentTarget);
    }
}
