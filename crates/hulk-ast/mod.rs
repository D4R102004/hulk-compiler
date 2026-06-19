//! Abstract Syntax Tree for the mandatory HULK subset up to static type checking.
//!
//! Design goals:
//! - HULK is expression-based, so `Expr` is the central node.
//! - Declarations are top-level only: functions and types.
//! - Type annotations are optional and represented with `Option<TypeRef>`.
//! - Semantic passes can annotate every expression using `inferred_type`.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub column: usize,
}

impl Span {
    pub fn new(start: usize, end: usize, line: usize, column: usize) -> Self {
        Self { start, end, line, column }
    }

    pub fn dummy() -> Self {
        Self::default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}

impl Ident {
    pub fn new(name: impl Into<String>, span: Span) -> Self {
        Self { name: name.into(), span }
    }

    pub fn dummy(name: impl Into<String>) -> Self {
        Self::new(name, Span::dummy())
    }
}

impl fmt::Display for Ident {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeRef {
    pub name: Ident,
}

impl TypeRef {
    pub fn named(name: impl Into<String>) -> Self {
        Self { name: Ident::dummy(name) }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub declarations: Vec<Decl>,
    pub entry: Expr,
}

impl Program {
    pub fn new(declarations: Vec<Decl>, entry: Expr) -> Self {
        Self { declarations, entry }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Decl {
    pub kind: DeclKind,
    pub span: Span,
}

impl Decl {
    pub fn new(kind: DeclKind, span: Span) -> Self {
        Self { kind, span }
    }

    pub fn dummy(kind: DeclKind) -> Self {
        Self::new(kind, Span::dummy())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum DeclKind {
    Function(FunctionDecl),
    Type(TypeDecl),
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionDecl {
    pub name: Ident,
    pub params: Vec<Param>,
    pub return_type: Option<TypeRef>,
    pub body: Expr,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: Ident,
    pub type_annotation: Option<TypeRef>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeDecl {
    pub name: Ident,
    pub params: Vec<Param>,
    pub parent: Option<ParentType>,
    pub members: Vec<TypeMember>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParentType {
    pub name: Ident,
    pub args: Vec<Expr>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeMember {
    Attribute(AttributeDecl),
    Method(FunctionDecl),
}

#[derive(Debug, Clone, PartialEq)]
pub struct AttributeDecl {
    pub name: Ident,
    pub type_annotation: Option<TypeRef>,
    pub initializer: Expr,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
    pub inferred_type: Option<TypeRef>,
}

impl Expr {
    pub fn new(kind: ExprKind, span: Span) -> Self {
        Self { kind, span, inferred_type: None }
    }

    pub fn dummy(kind: ExprKind) -> Self {
        Self::new(kind, Span::dummy())
    }

    pub fn boxed(self) -> Box<Self> {
        Box::new(self)
    }

    pub fn with_type(mut self, ty: TypeRef) -> Self {
        self.inferred_type = Some(ty);
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    Literal(Literal),
    Variable(Ident),

    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },

    // `callee(args...)`, where callee can be a variable, member access or future functor.
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },

    // `object.member`, used for method calls and private attribute access inside methods.
    MemberAccess {
        object: Box<Expr>,
        member: Ident,
    },

    Block(Vec<Expr>),

    Let {
        bindings: Vec<LetBinding>,
        body: Box<Expr>,
    },

    Assign {
        target: AssignTarget,
        value: Box<Expr>,
    },

    If {
        branches: Vec<IfBranch>,
        else_branch: Box<Expr>,
    },

    While {
        condition: Box<Expr>,
        body: Box<Expr>,
    },

    For {
        var: Ident,
        iterable: Box<Expr>,
        body: Box<Expr>,
    },

    New {
        type_name: Ident,
        args: Vec<Expr>,
    },

    Is {
        expr: Box<Expr>,
        target_type: TypeRef,
    },

    As {
        expr: Box<Expr>,
        target_type: TypeRef,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Number(f64),
    String(String),
    Boolean(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Negate,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,

    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Equal,
    NotEqual,

    And,
    Or,

    Concat,       // @
    SpacedConcat, // @@
}

#[derive(Debug, Clone, PartialEq)]
pub struct LetBinding {
    pub name: Ident,
    pub type_annotation: Option<TypeRef>,
    pub initializer: Expr,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IfBranch {
    pub condition: Expr,
    pub body: Expr,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AssignTarget {
    Variable(Ident),
    Member {
        object: Box<Expr>,
        member: Ident,
    },
}

pub trait AstVisitor {
    fn visit_program(&mut self, program: &Program) {
        walk_program(self, program);
    }

    fn visit_decl(&mut self, decl: &Decl) {
        walk_decl(self, decl);
    }

    fn visit_expr(&mut self, expr: &Expr) {
        walk_expr(self, expr);
    }
}

pub fn walk_program<V: AstVisitor + ?Sized>(visitor: &mut V, program: &Program) {
    for decl in &program.declarations {
        visitor.visit_decl(decl);
    }
    visitor.visit_expr(&program.entry);
}

pub fn walk_decl<V: AstVisitor + ?Sized>(visitor: &mut V, decl: &Decl) {
    match &decl.kind {
        DeclKind::Function(function) => visitor.visit_expr(&function.body),
        DeclKind::Type(type_decl) => {
            if let Some(parent) = &type_decl.parent {
                for arg in &parent.args {
                    visitor.visit_expr(arg);
                }
            }

            for member in &type_decl.members {
                match member {
                    TypeMember::Attribute(attribute) => visitor.visit_expr(&attribute.initializer),
                    TypeMember::Method(method) => visitor.visit_expr(&method.body),
                }
            }
        }
    }
}

pub fn walk_expr<V: AstVisitor + ?Sized>(visitor: &mut V, expr: &Expr) {
    match &expr.kind {
        ExprKind::Literal(_) | ExprKind::Variable(_) | ExprKind::New { args: _, type_name: _ } => {
            if let ExprKind::New { args, .. } = &expr.kind {
                for arg in args {
                    visitor.visit_expr(arg);
                }
            }
        }
        ExprKind::Unary { expr, .. } => visitor.visit_expr(expr),
        ExprKind::Binary { left, right, .. } => {
            visitor.visit_expr(left);
            visitor.visit_expr(right);
        }
        ExprKind::Call { callee, args } => {
            visitor.visit_expr(callee);
            for arg in args {
                visitor.visit_expr(arg);
            }
        }
        ExprKind::MemberAccess { object, .. } => visitor.visit_expr(object),
        ExprKind::Block(expressions) => {
            for expression in expressions {
                visitor.visit_expr(expression);
            }
        }
        ExprKind::Let { bindings, body } => {
            for binding in bindings {
                visitor.visit_expr(&binding.initializer);
            }
            visitor.visit_expr(body);
        }
        ExprKind::Assign { target, value } => {
            match target {
                AssignTarget::Variable(_) => {}
                AssignTarget::Member { object, .. } => visitor.visit_expr(object),
            }
            visitor.visit_expr(value);
        }
        ExprKind::If { branches, else_branch } => {
            for branch in branches {
                visitor.visit_expr(&branch.condition);
                visitor.visit_expr(&branch.body);
            }
            visitor.visit_expr(else_branch);
        }
        ExprKind::While { condition, body } => {
            visitor.visit_expr(condition);
            visitor.visit_expr(body);
        }
        ExprKind::For { iterable, body, .. } => {
            visitor.visit_expr(iterable);
            visitor.visit_expr(body);
        }
        ExprKind::Is { expr, .. } | ExprKind::As { expr, .. } => visitor.visit_expr(expr),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_print_42_program() {
        let print = Expr::dummy(ExprKind::Variable(Ident::dummy("print")));
        let forty_two = Expr::dummy(ExprKind::Literal(Literal::Number(42.0)));
        let entry = Expr::dummy(ExprKind::Call {
            callee: print.boxed(),
            args: vec![forty_two],
        });

        let program = Program::new(vec![], entry);
        assert_eq!(program.declarations.len(), 0);
    }

    #[test]
    fn visitor_walks_nested_expressions() {
        struct Counter(usize);
        impl AstVisitor for Counter {
            fn visit_expr(&mut self, expr: &Expr) {
                self.0 += 1;
                walk_expr(self, expr);
            }
        }

        let one = Expr::dummy(ExprKind::Literal(Literal::Number(1.0)));
        let two = Expr::dummy(ExprKind::Literal(Literal::Number(2.0)));
        let sum = Expr::dummy(ExprKind::Binary {
            op: BinaryOp::Add,
            left: one.boxed(),
            right: two.boxed(),
        });
        let program = Program::new(vec![], sum);

        let mut counter = Counter(0);
        counter.visit_program(&program);
        assert_eq!(counter.0, 3);
    }
}
