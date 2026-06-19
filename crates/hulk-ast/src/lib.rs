//! Abstract Syntax Tree (AST) for the HULK programming language.
//!
//! The parser produces these nodes and the following compiler phases
//! (semantic analysis, type checking and code generation) consume them.
//!
//! Design rule: the AST keeps semantic information. Parentheses, separators
//! and grammar-only helper productions are intentionally omitted.

use std::fmt;

/// Source location used by AST nodes for diagnostics.
///
/// `line` and `col` are 1-based, matching the lexer spans.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SourceSpan {
    pub line: usize,
    pub col: usize,
}

impl SourceSpan {
    pub const fn new(line: usize, col: usize) -> Self {
        Self { line, col }
    }
}

/// Complete HULK program.
///
/// HULK programs are a sequence of declarations followed by one global
/// expression that acts as the entry point.
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub declarations: Vec<Declaration>,
    pub entry: Expr,
}

impl Program {
    pub fn new(declarations: Vec<Declaration>, entry: Expr) -> Self {
        Self { declarations, entry }
    }
}

/// A top-level declaration with its source span.
#[derive(Debug, Clone, PartialEq)]
pub struct Declaration {
    pub kind: DeclarationKind,
    pub span: SourceSpan,
}

impl Declaration {
    pub fn new(kind: DeclarationKind, span: SourceSpan) -> Self {
        Self { kind, span }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum DeclarationKind {
    Function(FunctionDecl),
    Type(TypeDecl),
    Protocol(ProtocolDecl),
}

/// Function declaration, either inline (`=> expr`) or full-form (`{ ... }`).
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionDecl {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Option<TypeRef>,
    pub body: Expr,
}

impl FunctionDecl {
    pub fn new(
        name: impl Into<String>,
        params: Vec<Param>,
        return_type: Option<TypeRef>,
        body: Expr,
    ) -> Self {
        Self {
            name: name.into(),
            params,
            return_type,
            body,
        }
    }
}

/// Type declaration: `type T(args) inherits Base(args) { ... }`.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeDecl {
    pub name: String,
    pub params: Vec<Param>,
    pub parent: Option<TypeParent>,
    pub members: Vec<TypeMember>,
}

impl TypeDecl {
    pub fn new(
        name: impl Into<String>,
        params: Vec<Param>,
        parent: Option<TypeParent>,
        members: Vec<TypeMember>,
    ) -> Self {
        Self {
            name: name.into(),
            params,
            parent,
            members,
        }
    }
}

/// Parent type and constructor arguments used by inheritance.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeParent {
    pub name: String,
    pub args: Vec<Expr>,
}

impl TypeParent {
    pub fn new(name: impl Into<String>, args: Vec<Expr>) -> Self {
        Self {
            name: name.into(),
            args,
        }
    }
}

/// Member of a HULK type.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeMember {
    pub kind: TypeMemberKind,
    pub span: SourceSpan,
}

impl TypeMember {
    pub fn new(kind: TypeMemberKind, span: SourceSpan) -> Self {
        Self { kind, span }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeMemberKind {
    Attribute(AttributeDecl),
    Method(FunctionDecl),
}

/// Attribute declaration inside a type body.
#[derive(Debug, Clone, PartialEq)]
pub struct AttributeDecl {
    pub name: String,
    pub type_annotation: Option<TypeRef>,
    pub initializer: Expr,
}

impl AttributeDecl {
    pub fn new(name: impl Into<String>, type_annotation: Option<TypeRef>, initializer: Expr) -> Self {
        Self {
            name: name.into(),
            type_annotation,
            initializer,
        }
    }
}

/// Protocol declaration, useful for the natural next HULK layer.
/// Keeping it in the AST avoids a redesign when protocols are implemented.
#[derive(Debug, Clone, PartialEq)]
pub struct ProtocolDecl {
    pub name: String,
    pub parents: Vec<TypeRef>,
    pub methods: Vec<ProtocolMethod>,
}

impl ProtocolDecl {
    pub fn new(
        name: impl Into<String>,
        parents: Vec<TypeRef>,
        methods: Vec<ProtocolMethod>,
    ) -> Self {
        Self {
            name: name.into(),
            parents,
            methods,
        }
    }
}

/// Method signature inside a protocol.
#[derive(Debug, Clone, PartialEq)]
pub struct ProtocolMethod {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: TypeRef,
}

impl ProtocolMethod {
    pub fn new(name: impl Into<String>, params: Vec<Param>, return_type: TypeRef) -> Self {
        Self {
            name: name.into(),
            params,
            return_type,
        }
    }
}

/// Function, method, type, or protocol parameter.
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub type_annotation: Option<TypeRef>,
}

impl Param {
    pub fn new(name: impl Into<String>, type_annotation: Option<TypeRef>) -> Self {
        Self {
            name: name.into(),
            type_annotation,
        }
    }
}

/// A reference to a type name as written in source code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeRef {
    pub name: String,
    pub args: Vec<TypeRef>,
}

impl TypeRef {
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            args: Vec::new(),
        }
    }

    pub fn with_args(name: impl Into<String>, args: Vec<TypeRef>) -> Self {
        Self {
            name: name.into(),
            args,
        }
    }
}

impl fmt::Display for TypeRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.args.is_empty() {
            write!(f, "{}", self.name)
        } else {
            let args = self
                .args
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            write!(f, "{}<{}>", self.name, args)
        }
    }
}

/// Expression node.
#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: SourceSpan,
}

impl Expr {
    pub fn new(kind: ExprKind, span: SourceSpan) -> Self {
        Self { kind, span }
    }

    pub fn literal(literal: Literal, span: SourceSpan) -> Self {
        Self::new(ExprKind::Literal(literal), span)
    }

    pub fn number(value: f64, span: SourceSpan) -> Self {
        Self::literal(Literal::Number(value), span)
    }

    pub fn string(value: impl Into<String>, span: SourceSpan) -> Self {
        Self::literal(Literal::String(value.into()), span)
    }

    pub fn boolean(value: bool, span: SourceSpan) -> Self {
        Self::literal(Literal::Boolean(value), span)
    }

    pub fn variable(name: impl Into<String>, span: SourceSpan) -> Self {
        Self::new(ExprKind::Variable(name.into()), span)
    }

    pub fn unary(op: UnaryOp, expr: Expr, span: SourceSpan) -> Self {
        Self::new(
            ExprKind::Unary(UnaryExpr {
                op,
                expr: Box::new(expr),
            }),
            span,
        )
    }

    pub fn binary(op: BinaryOp, left: Expr, right: Expr, span: SourceSpan) -> Self {
        Self::new(
            ExprKind::Binary(BinaryExpr {
                op,
                left: Box::new(left),
                right: Box::new(right),
            }),
            span,
        )
    }

    pub fn call(callee: Expr, args: Vec<Expr>, span: SourceSpan) -> Self {
        Self::new(
            ExprKind::Call(CallExpr {
                callee: Box::new(callee),
                args,
            }),
            span,
        )
    }
}

/// Every expression form supported by the AST.
///
/// HULK is expression-based, so assignment, loops and blocks also appear here.
#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    Literal(Literal),
    Variable(String),
    SelfRef,
    BaseRef,
    Unary(UnaryExpr),
    Binary(BinaryExpr),
    Let(LetExpr),
    Assign(AssignExpr),
    Block(BlockExpr),
    If(IfExpr),
    While(WhileExpr),
    For(ForExpr),
    Call(CallExpr),
    Member(MemberExpr),
    New(NewExpr),
    TypeTest(TypeTestExpr),
    Downcast(DowncastExpr),
    Vector(VectorExpr),
    Index(IndexExpr),
    /// Extra-feature friendly node for `match` expressions.
    Match(MatchExpr),
}

/// Literal values.
#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Number(f64),
    String(String),
    Boolean(bool),
}

/// Unary expression.
#[derive(Debug, Clone, PartialEq)]
pub struct UnaryExpr {
    pub op: UnaryOp,
    pub expr: Box<Expr>,
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Negate,
    Not,
}

/// Binary expression.
#[derive(Debug, Clone, PartialEq)]
pub struct BinaryExpr {
    pub op: BinaryOp,
    pub left: Box<Expr>,
    pub right: Box<Expr>,
}

/// Binary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Or,
    And,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    Power,
    Concat,
    ConcatSpace,
}

/// `let a = expr, b: T = expr in body`.
#[derive(Debug, Clone, PartialEq)]
pub struct LetExpr {
    pub bindings: Vec<LetBinding>,
    pub body: Box<Expr>,
}

impl LetExpr {
    pub fn new(bindings: Vec<LetBinding>, body: Expr) -> Self {
        Self {
            bindings,
            body: Box::new(body),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LetBinding {
    pub name: String,
    pub type_annotation: Option<TypeRef>,
    pub initializer: Expr,
}

impl LetBinding {
    pub fn new(name: impl Into<String>, type_annotation: Option<TypeRef>, initializer: Expr) -> Self {
        Self {
            name: name.into(),
            type_annotation,
            initializer,
        }
    }
}

/// Destructive assignment: `target := value`.
#[derive(Debug, Clone, PartialEq)]
pub struct AssignExpr {
    pub target: AssignTarget,
    pub value: Box<Expr>,
}

impl AssignExpr {
    pub fn new(target: AssignTarget, value: Expr) -> Self {
        Self {
            target,
            value: Box::new(value),
        }
    }
}

/// Valid left-hand sides for destructive assignment.
#[derive(Debug, Clone, PartialEq)]
pub enum AssignTarget {
    Variable(String),
    Member { object: Box<Expr>, field: String },
    Index { object: Box<Expr>, index: Box<Expr> },
}

/// Expression block: `{ expr1; expr2; ... }`.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockExpr {
    pub expressions: Vec<Expr>,
}

impl BlockExpr {
    pub fn new(expressions: Vec<Expr>) -> Self {
        Self { expressions }
    }
}

/// Conditional expression with optional `elif` branches and final `else`.
#[derive(Debug, Clone, PartialEq)]
pub struct IfExpr {
    pub condition: Box<Expr>,
    pub then_branch: Box<Expr>,
    pub elif_branches: Vec<ElifBranch>,
    pub else_branch: Box<Expr>,
}

impl IfExpr {
    pub fn new(
        condition: Expr,
        then_branch: Expr,
        elif_branches: Vec<ElifBranch>,
        else_branch: Expr,
    ) -> Self {
        Self {
            condition: Box::new(condition),
            then_branch: Box::new(then_branch),
            elif_branches,
            else_branch: Box::new(else_branch),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ElifBranch {
    pub condition: Expr,
    pub body: Expr,
}

impl ElifBranch {
    pub fn new(condition: Expr, body: Expr) -> Self {
        Self { condition, body }
    }
}

/// `while (condition) body`.
#[derive(Debug, Clone, PartialEq)]
pub struct WhileExpr {
    pub condition: Box<Expr>,
    pub body: Box<Expr>,
}

impl WhileExpr {
    pub fn new(condition: Expr, body: Expr) -> Self {
        Self {
            condition: Box::new(condition),
            body: Box::new(body),
        }
    }
}

/// `for (var in iterable) body`.
#[derive(Debug, Clone, PartialEq)]
pub struct ForExpr {
    pub var: String,
    pub iterable: Box<Expr>,
    pub body: Box<Expr>,
}

impl ForExpr {
    pub fn new(var: impl Into<String>, iterable: Expr, body: Expr) -> Self {
        Self {
            var: var.into(),
            iterable: Box::new(iterable),
            body: Box::new(body),
        }
    }
}

/// Function call or method call.
///
/// The callee is an expression so the same node supports both `f(x)` and
/// `obj.f(x)` after dot access has been parsed as a [`MemberExpr`].
#[derive(Debug, Clone, PartialEq)]
pub struct CallExpr {
    pub callee: Box<Expr>,
    pub args: Vec<Expr>,
}

/// Dot access: `object.member`.
#[derive(Debug, Clone, PartialEq)]
pub struct MemberExpr {
    pub object: Box<Expr>,
    pub member: String,
}

impl MemberExpr {
    pub fn new(object: Expr, member: impl Into<String>) -> Self {
        Self {
            object: Box::new(object),
            member: member.into(),
        }
    }
}

/// Object construction: `new Type(args)`.
#[derive(Debug, Clone, PartialEq)]
pub struct NewExpr {
    pub type_name: TypeRef,
    pub args: Vec<Expr>,
}

impl NewExpr {
    pub fn new(type_name: TypeRef, args: Vec<Expr>) -> Self {
        Self { type_name, args }
    }
}

/// Dynamic type test: `expr is Type`.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeTestExpr {
    pub expr: Box<Expr>,
    pub type_name: TypeRef,
}

impl TypeTestExpr {
    pub fn new(expr: Expr, type_name: TypeRef) -> Self {
        Self {
            expr: Box::new(expr),
            type_name,
        }
    }
}

/// Downcast: `expr as Type`.
#[derive(Debug, Clone, PartialEq)]
pub struct DowncastExpr {
    pub expr: Box<Expr>,
    pub type_name: TypeRef,
}

impl DowncastExpr {
    pub fn new(expr: Expr, type_name: TypeRef) -> Self {
        Self {
            expr: Box::new(expr),
            type_name,
        }
    }
}

/// Vector literal or vector comprehension.
#[derive(Debug, Clone, PartialEq)]
pub enum VectorExpr {
    Literal(Vec<Expr>),
    Comprehension(VectorComprehension),
}

#[derive(Debug, Clone, PartialEq)]
pub struct VectorComprehension {
    pub expr: Box<Expr>,
    pub var: String,
    pub iterable: Box<Expr>,
}

impl VectorComprehension {
    pub fn new(expr: Expr, var: impl Into<String>, iterable: Expr) -> Self {
        Self {
            expr: Box::new(expr),
            var: var.into(),
            iterable: Box::new(iterable),
        }
    }
}

/// Indexing expression: `object[index]`.
#[derive(Debug, Clone, PartialEq)]
pub struct IndexExpr {
    pub object: Box<Expr>,
    pub index: Box<Expr>,
}

impl IndexExpr {
    pub fn new(object: Expr, index: Expr) -> Self {
        Self {
            object: Box::new(object),
            index: Box::new(index),
        }
    }
}

/// Pattern matching expression for the planned extension.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchExpr {
    pub value: Box<Expr>,
    pub cases: Vec<MatchCase>,
}

impl MatchExpr {
    pub fn new(value: Expr, cases: Vec<MatchCase>) -> Self {
        Self {
            value: Box::new(value),
            cases,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchCase {
    pub pattern: Pattern,
    pub body: Expr,
}

impl MatchCase {
    pub fn new(pattern: Pattern, body: Expr) -> Self {
        Self { pattern, body }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    Wildcard,
    Literal(Literal),
    Variable(String),
    Type(TypeRef, Option<String>),
}

/// Generic AST visitor.
///
/// This trait is intentionally small. Specific compiler passes can implement
/// traversal manually or use [`walk_expr`] for the recursive part.
pub trait AstVisitor {
    type Output;

    fn visit_program(&mut self, program: &Program) -> Self::Output;
    fn visit_declaration(&mut self, declaration: &Declaration) -> Self::Output;
    fn visit_expr(&mut self, expr: &Expr) -> Self::Output;
}

/// Walks an expression in pre-order and calls `visit` for each node.
///
/// Useful for tests and simple analyses. Full semantic passes will usually
/// implement their own visitor because they need scopes and synthesized values.
pub fn walk_expr(expr: &Expr, visit: &mut impl FnMut(&Expr)) {
    visit(expr);

    match &expr.kind {
        ExprKind::Literal(_) | ExprKind::Variable(_) | ExprKind::SelfRef | ExprKind::BaseRef => {}
        ExprKind::Unary(unary) => walk_expr(&unary.expr, visit),
        ExprKind::Binary(binary) => {
            walk_expr(&binary.left, visit);
            walk_expr(&binary.right, visit);
        }
        ExprKind::Let(let_expr) => {
            for binding in &let_expr.bindings {
                walk_expr(&binding.initializer, visit);
            }
            walk_expr(&let_expr.body, visit);
        }
        ExprKind::Assign(assign) => {
            walk_assign_target(&assign.target, visit);
            walk_expr(&assign.value, visit);
        }
        ExprKind::Block(block) => {
            for expression in &block.expressions {
                walk_expr(expression, visit);
            }
        }
        ExprKind::If(if_expr) => {
            walk_expr(&if_expr.condition, visit);
            walk_expr(&if_expr.then_branch, visit);
            for elif in &if_expr.elif_branches {
                walk_expr(&elif.condition, visit);
                walk_expr(&elif.body, visit);
            }
            walk_expr(&if_expr.else_branch, visit);
        }
        ExprKind::While(while_expr) => {
            walk_expr(&while_expr.condition, visit);
            walk_expr(&while_expr.body, visit);
        }
        ExprKind::For(for_expr) => {
            walk_expr(&for_expr.iterable, visit);
            walk_expr(&for_expr.body, visit);
        }
        ExprKind::Call(call) => {
            walk_expr(&call.callee, visit);
            for arg in &call.args {
                walk_expr(arg, visit);
            }
        }
        ExprKind::Member(member) => walk_expr(&member.object, visit),
        ExprKind::New(new_expr) => {
            for arg in &new_expr.args {
                walk_expr(arg, visit);
            }
        }
        ExprKind::TypeTest(type_test) => walk_expr(&type_test.expr, visit),
        ExprKind::Downcast(downcast) => walk_expr(&downcast.expr, visit),
        ExprKind::Vector(vector) => match vector {
            VectorExpr::Literal(items) => {
                for item in items {
                    walk_expr(item, visit);
                }
            }
            VectorExpr::Comprehension(comprehension) => {
                walk_expr(&comprehension.expr, visit);
                walk_expr(&comprehension.iterable, visit);
            }
        },
        ExprKind::Index(index) => {
            walk_expr(&index.object, visit);
            walk_expr(&index.index, visit);
        }
        ExprKind::Match(match_expr) => {
            walk_expr(&match_expr.value, visit);
            for case in &match_expr.cases {
                walk_expr(&case.body, visit);
            }
        }
    }
}

fn walk_assign_target(target: &AssignTarget, visit: &mut impl FnMut(&Expr)) {
    match target {
        AssignTarget::Variable(_) => {}
        AssignTarget::Member { object, .. } => walk_expr(object, visit),
        AssignTarget::Index { object, index } => {
            walk_expr(object, visit);
            walk_expr(index, visit);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s() -> SourceSpan {
        SourceSpan::new(1, 1)
    }

    #[test]
    fn builds_let_expression() {
        let expr = Expr::new(
            ExprKind::Let(LetExpr::new(
                vec![LetBinding::new("x", None, Expr::number(5.0, s()))],
                Expr::binary(
                    BinaryOp::Add,
                    Expr::variable("x", s()),
                    Expr::number(1.0, s()),
                    s(),
                ),
            )),
            s(),
        );

        match expr.kind {
            ExprKind::Let(let_expr) => {
                assert_eq!(let_expr.bindings[0].name, "x");
                assert!(matches!(let_expr.body.kind, ExprKind::Binary(_)));
            }
            other => panic!("expected let expression, got {other:?}"),
        }
    }

    #[test]
    fn type_ref_display_without_args() {
        assert_eq!(TypeRef::named("Number").to_string(), "Number");
    }

    #[test]
    fn type_ref_display_with_args() {
        let t = TypeRef::with_args("Iterable", vec![TypeRef::named("Number")]);
        assert_eq!(t.to_string(), "Iterable<Number>");
    }

    #[test]
    fn walk_expr_visits_children() {
        let expr = Expr::binary(
            BinaryOp::Multiply,
            Expr::binary(
                BinaryOp::Add,
                Expr::number(3.0, s()),
                Expr::number(2.0, s()),
                s(),
            ),
            Expr::number(5.0, s()),
            s(),
        );

        let mut count = 0;
        walk_expr(&expr, &mut |_| count += 1);

        assert_eq!(count, 5);
    }
}
