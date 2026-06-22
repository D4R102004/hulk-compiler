//! Pass 2: Type Inference
//!
//! This pass assigns a concrete `Type` to every expression and to every
//! symbol declaration that was not explicitly annotated. It builds the
//! `TypedProgram` node by node, using a post‑order traversal where each
//! visit returns the freshly built typed node and its type.
//!
//! The inference is **bounded**: we do not implement a general fixed‑point
//! solver. Instead, we use a simple constraint‑collection strategy for
//! unannotated parameters and a one‑pass‑with‑placeholder algorithm for 
//! recursive functions.

use std::collections::{HashMap, HashSet};

use hulk_ast::{
    AssignExpr, AssignTarget, AttributeDecl, BinaryExpr, BinaryOp, BlockExpr, CallExpr, Declaration,
    DeclarationKind, DowncastExpr, ElifBranch, Expr, ExprKind, ForExpr, FunctionDecl, IfExpr,
    IndexExpr, LetBinding, LetExpr, Literal, MatchCase, MatchExpr, MemberExpr, NewExpr, Param,
    Pattern, Program, ProtocolDecl, ProtocolMethod, SourceSpan, TypeDecl, TypeMember, TypeMemberKind,
    TypeParent, TypeRef, TypeTestExpr, UnaryExpr, UnaryOp, VectorComprehension, VectorExpr,
    WhileExpr,
};

use crate::environment::Environment;
use crate::error::{SemanticError, SemanticErrorKind, Severity};
use crate::typed::{TypedExpr, TypedProgram};
use crate::types::registry::{AttributeInfo, FunctionSignature, MethodSignature, ParentLink, TypeInfo, TypeRegistry};
use crate::types::{Type, lowest_common_ancestor};

// -----------------------------------------------------------------------------
// Public entry point
// -----------------------------------------------------------------------------

/// Runs type inference on the untyped program.
///
/// # Arguments
/// * `program` – The untyped AST (`Program<()>`).
/// * `registry` – The registry (mutated to fill in inferred return types).
/// * `errors` – Vector to append inference errors.
///
/// # Returns
/// A fully typed program (`Program<Type>`) with every expression annotated.
pub fn run(
    program: &Program,
    registry: &mut TypeRegistry,
    errors: &mut Vec<SemanticError>,
) -> TypedProgram {
    let mut state = InferState::new(registry, errors);

    // Infer each declaration.
    let mut typed_decls = Vec::new();
    for decl in &program.declarations {
        let typed = state.infer_declaration(decl);
        typed_decls.push(typed);
    }

    // Infer the entry expression in a fresh environment (no local variables).
    let mut env = Environment::new();
    let typed_entry = state.infer_expr(&program.entry, &mut env);

    TypedProgram::new(typed_decls, typed_entry)
}

// -----------------------------------------------------------------------------
// Inference state
// -----------------------------------------------------------------------------

/// Mutable state for the inference pass.
struct InferState<'a> {
    registry: &'a mut TypeRegistry,
    errors: &'a mut Vec<SemanticError>,
    /// The type of `self` when inside a method body.
    self_type: Option<Type>,
    /// The name of the currently inferred method (for `base` resolution).
    current_method_name: Option<String>,
    /// The type owner of the currently inferred method.
    current_type_owner: Option<String>,
    /// Stack of function names currently being inferred (for recursion detection).
    recursion_stack: HashSet<String>,
    /// Constraint map for unannotated parameters: parameter name -> candidate types.
    /// Keyed by the parameter name (which is unique within a function/method).
    param_constraints: HashMap<String, Vec<Type>>,
}

impl<'a> InferState<'a> {
    fn new(registry: &'a mut TypeRegistry, errors: &'a mut Vec<SemanticError>) -> Self {
        Self {
            registry,
            errors,
            self_type: None,
            current_method_name: None,
            current_type_owner: None,
            recursion_stack: HashSet::new(),
            param_constraints: HashMap::new(),
        }
    }

    // -------------------------------------------------------------------------
    // Declaration inference
    // -------------------------------------------------------------------------

    fn infer_declaration(&mut self, decl: &Declaration) -> Declaration<Type> {
        let span = decl.span;
        match &decl.kind {
            DeclarationKind::Function(f) => {
                let typed = self.infer_function(f);
                Declaration::new(DeclarationKind::Function(typed), span)
            }
            DeclarationKind::Type(t) => {
                let typed = self.infer_type(t, span);
                Declaration::new(DeclarationKind::Type(typed), span)
            }
            DeclarationKind::Protocol(p) => {
                // Protocols have no bodies, so they remain untyped.
                Declaration::new(DeclarationKind::Protocol(p.clone()), span)
            }
        }
    }

    // -------------------------------------------------------------------------
    // Function inference (with recursive placeholder)
    // -------------------------------------------------------------------------

    /// Infers the type of a function or method declaration.
    ///
    /// If the function is a method (i.e., `self.self_type` is `Some`), the `self`
    /// symbol is bound in the environment before parameters are declared. This
    /// allows `self` to be shadowed by a parameter of the same name, matching
    /// HULK's semantics (§A.7.1). The body is inferred in this environment.
    ///
    /// The function's return type is inferred using the bounded placeholder
    /// strategy: if unannotated, the body's type is used (unless it remains
    /// `Unknown`, in which case `CannotInferType` is reported). Unannotated
    /// parameters are resolved from collected constraints.
    fn infer_function(&mut self, func: &FunctionDecl) -> FunctionDecl<Type> {
        let name = func.name.clone();
        let return_type_was_unknown = func.return_type.is_none();
        if return_type_was_unknown {
            if let Some(sig) = self.registry.functions.get_mut(&name) {
                sig.return_type = Type::Unknown;
            }
        }

        self.param_constraints.clear(); // Clear entries from previous functions
        self.recursion_stack.insert(name.clone());

        let mut env = Environment::new();

        // Bind `self` if this is a method body.
        // This must be done before parameters to allow parameters named `self` to shadow it.
        if let Some(self_ty) = &self.self_type {
            env.declare_with_self("self", self_ty.clone(), func.body.span, true);
        }

        // Declare parameters (may shadow `self` if a parameter is named `self`).
        for p in &func.params {
            let ty = p
                .type_annotation
                .as_ref()
                .map(|tr| self.resolve_type_ref(tr))
                .unwrap_or(Type::Unknown);
            env.declare(&p.name, ty.clone(), SourceSpan::new(0, 0));
            if p.type_annotation.is_none() {
                self.param_constraints.insert(p.name.clone(), Vec::new());
            }
        }

        // Infer the body.
        let typed_body = self.infer_expr(&func.body, &mut env);
        self.recursion_stack.remove(&name);

        // Resolve unannotated parameters.
        let mut resolved_param_types = Vec::new();
        for p in &func.params {
            let mut ty = p
                .type_annotation
                .as_ref()
                .map(|tr| self.resolve_type_ref(tr))
                .unwrap_or(Type::Unknown);
            if p.type_annotation.is_none() {
                if let Some(candidates) = self.param_constraints.get(&p.name) {
                    let mut unique = HashSet::new();
                    for c in candidates {
                        if !matches!(c, Type::Unknown | Type::Error) {
                            unique.insert(c.clone());
                        }
                    }
                    let unique_types: Vec<Type> = unique.into_iter().collect();
                    if unique_types.is_empty() {
                        self.errors.push(SemanticError::error(
                            SemanticErrorKind::CannotInferType { symbol: p.name.clone() },
                            func.body.span,
                        ));
                        ty = Type::Error;
                    } else if unique_types.len() == 1 {
                        ty = unique_types[0].clone();
                    } else {
                        self.errors.push(SemanticError::error(
                            SemanticErrorKind::AmbiguousInference {
                                symbol: p.name.clone(),
                                candidates: unique_types,
                            },
                            func.body.span,
                        ));
                        ty = Type::Error;
                    }
                }
            }
            resolved_param_types.push((p.name.clone(), ty));
        }

        // Update registry's FunctionSignature with resolved parameter types.
        if let Some(sig) = self.registry.functions.get_mut(&name) {
            for (i, (_, ty)) in resolved_param_types.iter().enumerate() {
                if i < sig.params.len() {
                    sig.params[i].1 = ty.clone();
                }
            }
        }

        // Return type resolution.
        if return_type_was_unknown {
            let body_type = typed_body.anno.clone();
            if matches!(body_type, Type::Unknown | Type::Error) {
                self.errors.push(SemanticError::error(
                    SemanticErrorKind::CannotInferType { symbol: name.clone() },
                    func.body.span,
                ));
            } else {
                if let Some(sig) = self.registry.functions.get_mut(&name) {
                    sig.return_type = body_type.clone();
                }
            }
        } else {
            let annotated = func.return_type.as_ref().unwrap();
            let ann_type = self.resolve_type_ref(annotated);
            if !typed_body.anno.conforms_to(&ann_type, self.registry) {
                self.errors.push(SemanticError::error(
                    SemanticErrorKind::TypeMismatch {
                        expected: ann_type,
                        found: typed_body.anno.clone(),
                    },
                    func.body.span,
                ));
            }
        }

        // Preserve original syntactic annotations in the typed AST.
        FunctionDecl::new(name, func.params.clone(), func.return_type.clone(), typed_body)
    }
    // -------------------------------------------------------------------------
    // Type inference (attributes and methods)
    // -------------------------------------------------------------------------

    /// Infers the types of a type declaration: each attribute initializer,
    /// each method body, and the parent constructor arguments.
    ///
    /// Attributes are inferred in a scope containing only the type's own constructor
    /// parameters (not `self`, not sibling attributes). Methods are inferred with
    /// `self` bound to the type's name. Parent constructor arguments are inferred
    /// in a scope containing the inheriting type's own constructor parameters.
    /// Use the type declaration's span for error reporting.
    ///
    /// Errors for arity mismatches or type mismatches in parent constructor
    /// arguments are reported using the span of the entire type declaration.
    fn infer_type(&mut self, ty_decl: &TypeDecl, decl_span: SourceSpan) -> TypeDecl<Type> {
        let name = ty_decl.name.clone();
        let decl_span = decl_span; 

        // Infer each member.
        let mut typed_members = Vec::new();
        for member in &ty_decl.members {
            let typed = self.infer_type_member(member, &name);
            typed_members.push(typed);
        }

        // Infer parent constructor arguments (if any) in a scope containing
        // the inheriting type's constructor parameters.
        let typed_parent = ty_decl.parent.as_ref().map(|p| {
            let mut env = Environment::new();
            // Declare the type's own constructor parameters.
            for param in &ty_decl.params {
                let ty = param
                    .type_annotation
                    .as_ref()
                    .map(|tr| self.resolve_type_ref(tr))
                    .unwrap_or(Type::Unknown);
                env.declare(&param.name, ty, SourceSpan::new(0, 0));
            }
            let args: Vec<TypedExpr> = p
                .args
                .iter()
                .map(|e| self.infer_expr(e, &mut env))
                .collect();
            let arg_types: Vec<Type> = args.iter().map(|e| e.anno.clone()).collect();

            // Check arity and conformance against parent constructor.
            let parent_info = self.registry.lookup_type(&p.name);
            if let Some(info) = parent_info {
                if arg_types.len() != info.params.len() {
                    self.errors.push(SemanticError::error(
                        SemanticErrorKind::ArityMismatch {
                            expected: info.params.len(),
                            found: arg_types.len(),
                        },
                        decl_span,
                    ));
                } else {
                    for ((_, expected), found) in info.params.iter().zip(arg_types) {
                        if !found.conforms_to(expected, self.registry) {
                            self.errors.push(SemanticError::error(
                                SemanticErrorKind::NotConforming {
                                    found: found.clone(),
                                    expected: expected.clone(),
                                },
                                decl_span,
                            ));
                        }
                    }
                }
            }
            TypeParent::new(&p.name, args)
        });

        TypeDecl::new(name, ty_decl.params.clone(), typed_parent, typed_members)
    }

    fn infer_type_member(&mut self, member: &TypeMember, type_name: &str) -> TypeMember<Type> {
        let span = member.span;
        match &member.kind {
            TypeMemberKind::Attribute(attr) => {
                // Attribute initializer is inferred in a scope with only
                // the type's constructor parameters (not self, not siblings).
                let mut env = Environment::new();
                // The type's params are not in the AST easily; we use the registry.
                if let Some(info) = self.registry.lookup_type(type_name) {
                    for (name, ty) in &info.params {
                        env.declare(name, ty.clone(), SourceSpan::new(0, 0));
                    }
                }
                let typed_init = self.infer_expr(&attr.initializer, &mut env);
                // Check annotation if present.
                let annotated = attr.type_annotation.as_ref().map(|tr| self.resolve_type_ref(tr));
                if let Some(ann) = &annotated {
                    if !typed_init.anno.conforms_to(ann, self.registry) {
                        self.errors.push(SemanticError::error(
                            SemanticErrorKind::TypeMismatch {
                                expected: ann.clone(),
                                found: typed_init.anno.clone(),
                            },
                            attr.initializer.span,
                        ));
                    }
                }
                // Keep the original TypeRef annotation in the typed AST.
                let attr_typed = AttributeDecl::new(&attr.name, attr.type_annotation.clone(), typed_init);
                TypeMember::new(TypeMemberKind::Attribute(attr_typed), span)
            }
            TypeMemberKind::Method(method) => {
                // Infer method body with self bound.
                let old_self = self.self_type.clone();
                let old_method_name = self.current_method_name.clone();
                let old_owner = self.current_type_owner.clone();

                self.self_type = Some(Type::Named(type_name.to_string()));
                self.current_method_name = Some(method.name.clone());
                self.current_type_owner = Some(type_name.to_string());

                let typed_method = self.infer_function(method);

                self.self_type = old_self;
                self.current_method_name = old_method_name;
                self.current_type_owner = old_owner;

                TypeMember::new(TypeMemberKind::Method(typed_method), span)
            }
        }
    }

    // -------------------------------------------------------------------------
    // Expression inference (core dispatcher)
    // -------------------------------------------------------------------------

    fn infer_expr(&mut self, expr: &Expr, env: &mut Environment) -> TypedExpr {
        let span = expr.span;
        match &expr.kind {
            ExprKind::Literal(lit) => self.infer_literal(lit, span),
            ExprKind::Variable(name) => self.infer_variable(name, span, env),
            ExprKind::SelfRef => self.infer_self_ref(span),
            ExprKind::BaseRef => self.infer_base_ref(span, env),
            ExprKind::Unary(unary) => self.infer_unary(unary, env),
            ExprKind::Binary(binary) => self.infer_binary(binary, env),
            ExprKind::Let(let_expr) => self.infer_let(let_expr, env),
            ExprKind::Assign(assign) => self.infer_assign(assign, env),
            ExprKind::Block(block) => self.infer_block(block, env),
            ExprKind::If(if_expr) => self.infer_if(if_expr, env),
            ExprKind::While(while_expr) => self.infer_while(while_expr, env),
            ExprKind::For(for_expr) => self.infer_for(for_expr, env),
            ExprKind::Call(call) => self.infer_call(call, env),
            ExprKind::Member(member) => self.infer_member(member, env),
            ExprKind::New(new_expr) => self.infer_new(new_expr, span, env),
            ExprKind::TypeTest(type_test) => self.infer_type_test(type_test, span, env),
            ExprKind::Downcast(downcast) => self.infer_downcast(downcast, span, env),
            ExprKind::Vector(vector) => self.infer_vector(vector, env),
            ExprKind::Index(index) => self.infer_index(index, env),
            ExprKind::Match(match_expr) => self.infer_match(match_expr, env),
        }
    }

    // -------------------------------------------------------------------------
    // Literals
    // -------------------------------------------------------------------------

    fn infer_literal(&self, lit: &Literal, span: SourceSpan) -> TypedExpr {
        let ty = match lit {
            Literal::Number(_) => Type::Number,
            Literal::String(_) => Type::String,
            Literal::Boolean(_) => Type::Boolean,
        };
        typed_expr(ExprKind::Literal(lit.clone()), ty, span)
    }

    // -------------------------------------------------------------------------
    // Variables and Self/Base
    // -------------------------------------------------------------------------

    /// Looks up a variable name in the current environment.
    ///
    /// Returns the variable's resolved type if found. If not found, checks the 
    /// registry for a zero‑arity function (global constant). Otherwise, reports 
    /// `UndefinedVariable`. 
    fn infer_variable(&mut self, name: &str, span: SourceSpan, env: &Environment) -> TypedExpr {
        if let Some(binding) = env.lookup(name) {
            let ty = binding.ty.clone();
            typed_expr(ExprKind::Variable(name.to_string()), ty, span)
        } else {
            // Check for global constants (zero-arity functions like PI, E)
            if let Some(sig) = self.registry.lookup_function(name) {
                if sig.params.is_empty() {
                    let ty = sig.return_type.clone();
                    return typed_expr(ExprKind::Variable(name.to_string()), ty, span);
                }
            }
            self.errors.push(SemanticError::error(
                SemanticErrorKind::UndefinedVariable(name.to_string()),
                span,
            ));
            typed_expr(ExprKind::Variable(name.to_string()), Type::Error, span)
        }
    }

    fn infer_self_ref(&mut self, span: SourceSpan) -> TypedExpr {
        if let Some(ty) = &self.self_type {
            typed_expr(ExprKind::SelfRef, ty.clone(), span)
        } else {
            self.errors.push(SemanticError::error(
                SemanticErrorKind::UndefinedVariable("self".to_string()),
                span,
            ));
            typed_expr(ExprKind::SelfRef, Type::Error, span)
        }
    }

    /// Resolves a `base` reference inside an overriding method.
    ///
    /// Returns the return type of the parent method if the current method overrides
    /// a parent method of the same name. Otherwise, reports
    /// `BaseOutsideOverridingMethod` and returns `Type::Error`.
    fn infer_base_ref(&mut self, span: SourceSpan, env: &Environment) -> TypedExpr {
        if let (Some(owner), Some(method_name)) = (&self.current_type_owner, &self.current_method_name) {
            if let Some(info) = self.registry.lookup_type(owner) {
                if let Some(parent) = &info.parent {
                    if let Some(parent_info) = self.registry.lookup_type(&parent.name) {
                        if let Some(parent_sig) = parent_info.methods.get(method_name) {
                            let return_type = parent_sig.return_type.clone();
                            return typed_expr(ExprKind::BaseRef, return_type, span);
                        }
                    }
                }
            }
            self.errors.push(SemanticError::error(
                SemanticErrorKind::BaseOutsideOverridingMethod,
                span,
            ));
            typed_expr(ExprKind::BaseRef, Type::Error, span)
        } else {
            self.errors.push(SemanticError::error(
                SemanticErrorKind::BaseOutsideOverridingMethod,
                span,
            ));
            typed_expr(ExprKind::BaseRef, Type::Error, span)
        }
    }

    // -------------------------------------------------------------------------
    // Unary and Binary operators
    // -------------------------------------------------------------------------

    /// Infers the type of a unary expression.
    ///
    /// For `-`, the operand must be `Number` (or `Unknown` during inference);
    /// for `!`, the operand must be `Boolean` (or `Unknown` during inference).
    /// If the operand is `Unknown` and it is an unannotated parameter, a constraint
    /// is added to pin it to the required type.
    fn infer_unary(&mut self, unary: &UnaryExpr, env: &mut Environment) -> TypedExpr {
        let type_infered_expr = self.infer_expr(&unary.expr, env);
        let op = unary.op;
        let operand_type = type_infered_expr.anno.clone();

        let result_type = match op {
            UnaryOp::Negate => {
                if matches!(operand_type, Type::Number | Type::Unknown) {
                    self.constrain_if_variable(&type_infered_expr, Type::Number);
                    Type::Number
                } else {
                    self.errors.push(SemanticError::error(
                        SemanticErrorKind::InvalidOperator {
                            op: "-".to_string(),
                            operand_types: vec![operand_type],
                        },
                        unary.expr.span,
                    ));
                    Type::Error
                }
            }
            UnaryOp::Not => {
                if matches!(operand_type, Type::Boolean | Type::Unknown) {
                    self.constrain_if_variable(&type_infered_expr, Type::Boolean);
                    Type::Boolean
                } else {
                    self.errors.push(SemanticError::error(
                        SemanticErrorKind::InvalidOperator {
                            op: "!".to_string(),
                            operand_types: vec![operand_type],
                        },
                        unary.expr.span,
                    ));
                    Type::Error
                }
            }
        };
        typed_expr(
            ExprKind::Unary(UnaryExpr {
                op,
                expr: Box::new(type_infered_expr),
            }),
            result_type,
            unary.expr.span,
        )
    }

    /// Infers the type of a binary expression.
    ///
    /// Each operator has a fixed required operand type (or set of types).
    /// If an operand is `Type::Unknown` and corresponds to an unannotated parameter,
    /// a constraint is recorded so that the parameter can be inferred later.
    ///
    /// - Arithmetic operators require both operands to be `Number`.
    /// - Comparisons (`<`, `<=`, `>`, `>=`, `==`, `!=`) require both operands `Number`.
    /// - Logical operators (`&`, `|`) require both operands `Boolean`.
    /// - Concatenation (`@`, `@@`) accepts operands of type `Number`, `String`, or `Boolean`.
    ///   No constraint is recorded because the required type is not unique.
    fn infer_binary(&mut self, binary: &BinaryExpr, env: &mut Environment) -> TypedExpr {
        let left = self.infer_expr(&binary.left, env);
        let right = self.infer_expr(&binary.right, env);
        let left_type = left.anno.clone();
        let right_type = right.anno.clone();

        let op = binary.op;
        let result_type = match op {
            BinaryOp::Add | BinaryOp::Subtract | BinaryOp::Multiply | BinaryOp::Divide
            | BinaryOp::Modulo | BinaryOp::Power => {
                let left_ok = matches!(left_type, Type::Number | Type::Unknown);
                let right_ok = matches!(right_type, Type::Number | Type::Unknown);
                if left_ok && right_ok {
                    self.constrain_if_variable(&left, Type::Number);
                    self.constrain_if_variable(&right, Type::Number);
                    Type::Number
                } else {
                    self.errors.push(SemanticError::error(
                        SemanticErrorKind::InvalidOperator {
                            op: format!("{:?}", op),
                            operand_types: vec![left_type, right_type],
                        },
                        binary.left.span,
                    ));
                    Type::Error
                }
            }
            BinaryOp::Equal | BinaryOp::NotEqual | BinaryOp::Less | BinaryOp::LessEqual
            | BinaryOp::Greater | BinaryOp::GreaterEqual => {
                let left_ok = matches!(left_type, Type::Number | Type::Unknown);
                let right_ok = matches!(right_type, Type::Number | Type::Unknown);
                if left_ok && right_ok {
                    self.constrain_if_variable(&left, Type::Number);
                    self.constrain_if_variable(&right, Type::Number);
                    Type::Boolean
                } else {
                    self.errors.push(SemanticError::error(
                        SemanticErrorKind::InvalidOperator {
                            op: format!("{:?}", op),
                            operand_types: vec![left_type, right_type],
                        },
                        binary.left.span,
                    ));
                    Type::Error
                }
            }
            BinaryOp::And | BinaryOp::Or => {
                let left_ok = matches!(left_type, Type::Boolean | Type::Unknown);
                let right_ok = matches!(right_type, Type::Boolean | Type::Unknown);
                if left_ok && right_ok {
                    self.constrain_if_variable(&left, Type::Boolean);
                    self.constrain_if_variable(&right, Type::Boolean);
                    Type::Boolean
                } else {
                    self.errors.push(SemanticError::error(
                        SemanticErrorKind::InvalidOperator {
                            op: format!("{:?}", op),
                            operand_types: vec![left_type, right_type],
                        },
                        binary.left.span,
                    ));
                    Type::Error
                }
            }
            BinaryOp::Concat | BinaryOp::ConcatSpace => {
                let allowed = |t: &Type| matches!(t, Type::Number | Type::String | Type::Boolean | Type::Unknown);
                if allowed(&left_type) && allowed(&right_type) {
                    // No constraint: the required type is not unique (could be Number, String, or Boolean).
                    Type::String
                } else {
                    let op_str = if matches!(op, BinaryOp::Concat) { "@" } else { "@@" };
                    self.errors.push(SemanticError::error(
                        SemanticErrorKind::InvalidOperator {
                            op: op_str.to_string(),
                            operand_types: vec![left_type, right_type],
                        },
                        binary.left.span,
                    ));
                    Type::Error
                }
            }
        };
        typed_expr(
            ExprKind::Binary(BinaryExpr {
                op,
                left: Box::new(left),
                right: Box::new(right),
            }),
            result_type,
            binary.left.span,
        )
    }
    // -------------------------------------------------------------------------
    // Let expression
    // -------------------------------------------------------------------------

    /// Infers the type of a `let` expression.
    ///
    /// Creates a new lexical scope for the bindings. Each binding's initializer
    /// is inferred in the current environment (before the binding is declared),
    /// then the binding is added to the newly created scope. Subsequent bindings
    /// can see earlier ones. The body is inferred in the same scope, and the
    /// scope is popped after the body.
    ///
    /// The expression's type is the type of the body.
    fn infer_let(&mut self, let_expr: &LetExpr, env: &mut Environment) -> TypedExpr {
        // 1. Push a new scope for all let bindings.
        env.push_scope();

        let mut typed_bindings = Vec::new();

        // 2. Process bindings left‑to‑right.
        for binding in &let_expr.bindings {
            // 2a. Infer initializer in the current environment (before declaring this binding).
            let typed_init = self.infer_expr(&binding.initializer, env);
            let init_type = typed_init.anno.clone();

            // 2b. Determine the declared type: annotation or inferred.
            let declared_type = if let Some(ann) = &binding.type_annotation {
                let ann_type = self.resolve_type_ref(ann);
                if !init_type.conforms_to(&ann_type, self.registry) {
                    self.errors.push(SemanticError::error(
                        SemanticErrorKind::TypeMismatch {
                            expected: ann_type.clone(),
                            found: init_type,
                        },
                        binding.initializer.span,
                    ));
                    ann_type
                } else {
                    ann_type
                }
            } else {
                init_type
            };

            // 2c. Declare the binding in the current (new) scope.
            env.declare(&binding.name, declared_type.clone(), binding.initializer.span);

            // 2d. Store the typed binding.
            typed_bindings.push(LetBinding::new(&binding.name, binding.type_annotation.clone(), typed_init));
        }

        // 3. Infer the body in the same scope (all bindings are now visible).
        let typed_body = self.infer_expr(&let_expr.body, env);

        // 4. Pop the scope after the body has been inferred.
        env.pop_scope();

        // 5. The let expression's type is the body's type.
        let body_type = typed_body.anno.clone();
        let let_typed = LetExpr::new(typed_bindings, typed_body);
        
        typed_expr(ExprKind::Let(let_typed), body_type, let_expr.body.span)
    }

    // -------------------------------------------------------------------------
    // Assignment
    // -------------------------------------------------------------------------

    /// Infers the type of a destructive assignment (`:=`).
    ///
    /// The target is resolved first to determine its declared type. The value is inferred,
    /// and its type must conform to the target's type. The whole expression's type is the
    /// type of the value (the assigned value).
    ///
    /// Errors are reported for: assigning to `self` (`SelfIsNotAssignable`), undefined
    /// variables, unknown members, index on non‑vector, and type mismatches.
    fn infer_assign(&mut self, assign: &AssignExpr, env: &mut Environment) -> TypedExpr {
        let typed_value = self.infer_expr(&assign.value, env);
        let value_type = typed_value.anno.clone();

        // Resolve target, passing the value's span as a fallback for target errors.
        let (target_ty, typed_target) = self.infer_assign_target(&assign.target, env, assign.value.span);

        if !value_type.conforms_to(&target_ty, self.registry) {
            self.errors.push(SemanticError::error(
                SemanticErrorKind::TypeMismatch {
                    expected: target_ty,
                    found: value_type.clone(),
                },
                assign.value.span,
            ));
        }

        let typed_assign = AssignExpr::new(typed_target, typed_value);
        typed_expr(ExprKind::Assign(typed_assign), value_type, assign.value.span)
    }

    /// Resolves the target of an assignment and returns its type and a typed target.
    ///
    /// The target can be a variable, a member access, or an index access.
    /// For variables, the name is looked up in the environment; `self` is not assignable.
    /// For members, the member is looked up on the object's type.
    /// For indices, the object must be a `Vector` and the index must be `Number`.
    ///
    /// The `span` parameter is used as a fallback for errors that do not have a more
    /// specific span (e.g., `SelfIsNotAssignable`, `UndefinedVariable`).
    fn infer_assign_target(
        &mut self,
        target: &AssignTarget,
        env: &mut Environment,
        span: SourceSpan,
    ) -> (Type, AssignTarget<Type>) {
        match target {
            AssignTarget::Variable(name) => {
                if let Some(binding) = env.lookup(name) {
                    if binding.is_self {
                        self.errors.push(SemanticError::error(
                            SemanticErrorKind::SelfIsNotAssignable,
                            span,
                        ));
                        (Type::Error, AssignTarget::Variable(name.clone()))
                    } else {
                        let ty = binding.ty.clone();
                        (ty, AssignTarget::Variable(name.clone()))
                    }
                } else {
                    // Check if the name is a global function or type
                    if self.registry.lookup_function(name).is_some()
                        || self.registry.lookup_type(name).is_some()
                    {
                        self.errors.push(SemanticError::error(
                            SemanticErrorKind::InvalidAssignTarget,
                            span,
                        ));
                        (Type::Error, AssignTarget::Variable(name.clone()))
                    } else {
                        self.errors.push(SemanticError::error(
                            SemanticErrorKind::UndefinedVariable(name.clone()),
                            span,
                        ));
                        (Type::Error, AssignTarget::Variable(name.clone()))
                    }
                }
            }
            AssignTarget::Member { object, field } => {
                let typed_obj = self.infer_expr(object, env);
                if let Some((_, member_type)) = self.lookup_member(&typed_obj.anno, field) {
                    (member_type, AssignTarget::Member { object: Box::new(typed_obj), field: field.clone() })
                } else {
                    self.errors.push(SemanticError::error(
                        SemanticErrorKind::UnknownMember { ty: typed_obj.anno.clone(), member: field.clone() },
                        object.span,
                    ));
                    (Type::Error, AssignTarget::Member { object: Box::new(typed_obj), field: field.clone() })
                }
            }
            AssignTarget::Index { object, index } => {
                let typed_obj = self.infer_expr(object, env);
                let typed_idx = self.infer_expr(index, env);
                if let Type::Vector(inner) = &typed_obj.anno {
                    if matches!(typed_idx.anno, Type::Number) {
                        let elem_type = *inner.clone();
                        (elem_type, AssignTarget::Index { object: Box::new(typed_obj), index: Box::new(typed_idx) })
                    } else {
                        self.errors.push(SemanticError::error(
                            SemanticErrorKind::TypeMismatch {
                                expected: Type::Number,
                                found: typed_idx.anno.clone(),
                            },
                            index.span,
                        ));
                        (Type::Error, AssignTarget::Index { object: Box::new(typed_obj), index: Box::new(typed_idx) })
                    }
                } else {
                    self.errors.push(SemanticError::error(
                        SemanticErrorKind::IndexOnNonVector(typed_obj.anno.clone()),
                        object.span,
                    ));
                    (Type::Error, AssignTarget::Index { object: Box::new(typed_obj), index: Box::new(typed_idx) })
                }
            }
        }
    }

    // -------------------------------------------------------------------------
    // Block
    // -------------------------------------------------------------------------

    fn infer_block(&mut self, block: &BlockExpr, env: &mut Environment) -> TypedExpr {
        let mut typed_exprs = Vec::new();
        let mut last_type = Type::Object; // default for empty block
        for expr in &block.expressions {
            let typed = self.infer_expr(expr, env);
            last_type = typed.anno.clone();
            typed_exprs.push(typed);
        }
        typed_expr(ExprKind::Block(BlockExpr::new(typed_exprs)), last_type, block.expressions.first().map(|e| e.span).unwrap_or(SourceSpan::new(0, 0)))
    }

    // -------------------------------------------------------------------------
    // If
    // -------------------------------------------------------------------------

    fn infer_if(&mut self, if_expr: &IfExpr, env: &mut Environment) -> TypedExpr {
        let cond = self.infer_expr(&if_expr.condition, env);
        if !matches!(cond.anno, Type::Boolean) {
            self.errors.push(SemanticError::error(
                SemanticErrorKind::NonBooleanCondition(cond.anno.clone()),
                if_expr.condition.span,
            ));
        }
        let then_branch = self.infer_expr(&if_expr.then_branch, env);
        let mut elif_branches = Vec::new();
        for elif in &if_expr.elif_branches {
            let elif_cond = self.infer_expr(&elif.condition, env);
            if !matches!(elif_cond.anno, Type::Boolean) {
                self.errors.push(SemanticError::error(
                    SemanticErrorKind::NonBooleanCondition(elif_cond.anno.clone()),
                    elif.condition.span,
                ));
            }
            let elif_body = self.infer_expr(&elif.body, env);
            elif_branches.push(ElifBranch::new(elif_cond, elif_body));
        }
        let else_branch = self.infer_expr(&if_expr.else_branch, env);

        // Compute LCA of all branch types.
        let mut all_types = vec![then_branch.anno.clone()];
        for elif in &elif_branches {
            all_types.push(elif.body.anno.clone());
        }
        all_types.push(else_branch.anno.clone());
        let result_type = lowest_common_ancestor(&all_types, self.registry);

        let if_typed = IfExpr::new(cond, then_branch, elif_branches, else_branch);
        typed_expr(ExprKind::If(if_typed), result_type, if_expr.condition.span)
    }

    // -------------------------------------------------------------------------
    // While
    // -------------------------------------------------------------------------

    fn infer_while(&mut self, while_expr: &WhileExpr, env: &mut Environment) -> TypedExpr {
        let cond = self.infer_expr(&while_expr.condition, env);
        if !matches!(cond.anno, Type::Boolean) {
            self.errors.push(SemanticError::error(
                SemanticErrorKind::NonBooleanCondition(cond.anno.clone()),
                while_expr.condition.span,
            ));
        }
        let body = self.infer_expr(&while_expr.body, env);
        let result_type = body.anno.clone();
        let while_typed = WhileExpr::new(cond, body);
        typed_expr(ExprKind::While(while_typed), result_type, while_expr.condition.span)
    }

    // -------------------------------------------------------------------------
    // For
    // -------------------------------------------------------------------------

    fn infer_for(&mut self, for_expr: &ForExpr, env: &mut Environment) -> TypedExpr {
        let iterable = self.infer_expr(&for_expr.iterable, env);
        let iterable_type = iterable.anno.clone();

        // Check if iterable implements Iterable protocol or is a Vector/Iterable type.
        let element_type = if let Type::Iterable(inner) = &iterable_type {
            *inner.clone()
        } else if let Type::Vector(inner) = &iterable_type {
            *inner.clone()
        } else if self.is_iterable(&iterable_type) {
            // If it's a type that implements the Iterable protocol, get the covariant
            // return type of its current() method.
            match self.iterable_element_type(&iterable_type) {
                Some(ty) => ty,
                None => {
                    self.errors.push(SemanticError::error(
                        SemanticErrorKind::NotIterable(iterable_type.clone()),
                        for_expr.iterable.span,
                    ));
                    Type::Error
                }
            }
        } else {
            self.errors.push(SemanticError::error(
                SemanticErrorKind::NotIterable(iterable_type.clone()),
                for_expr.iterable.span,
            ));
            Type::Error
        };

        // Push scope, declare loop variable.
        env.push_scope();
        env.declare(&for_expr.var, element_type.clone(), for_expr.iterable.span);
        let body = self.infer_expr(&for_expr.body, env);
        env.pop_scope();

        let result_type = body.anno.clone();
        let for_typed = ForExpr::new(&for_expr.var, iterable, body);
        typed_expr(ExprKind::For(for_typed), result_type, for_expr.iterable.span)
    }

    // Helper: check if a type implements Iterable protocol.
    fn is_iterable(&self, ty: &Type) -> bool {
        match ty {
            Type::Named(name) => self.registry.implements_protocol(name, "Iterable"),
            _ => false,
        }
    }

    // Helper: get the covariant element type of an iterable.
    fn iterable_element_type(&self, ty: &Type) -> Option<Type> {
        // For a type that implements Iterable, we need the return type of current().
        match ty {
            Type::Named(name) => {
                if let Some(info) = self.registry.lookup_type(name) {
                    if let Some(sig) = info.methods.get("current") {
                        // Covariant: the method's return type is the element type.
                        return Some(sig.return_type.clone());
                    }
                }
                None
            }
            _ => None,
        }
    }

    // -------------------------------------------------------------------------
    // Call
    // -------------------------------------------------------------------------

    /// Resolves a function or method call.
    ///
    /// If the callee is a variable name, it is treated as a global function call
    /// and looked up in the registry. If the callee is a member access, it is
    /// treated as a method call, and the method is resolved on the object's type.
    /// Reports arity and type mismatches for arguments.
    fn infer_call(&mut self, call: &CallExpr, env: &mut Environment) -> TypedExpr {
        let typed_callee = self.infer_expr(&call.callee, env);

        // Global function call: callee is a bare variable.
        if let ExprKind::Variable(name) = &call.callee.kind {
            if let Some(sig) = self.registry.lookup_function(name) {
                // Clone before mutating self.
                let params: Vec<(String, Type)> = sig.params.clone();
                let return_type = sig.return_type.clone();

                let mut typed_args = Vec::new();
                for arg in &call.args {
                    typed_args.push(self.infer_expr(arg, env));
                }
                if typed_args.len() != params.len() {
                    self.errors.push(SemanticError::error(
                        SemanticErrorKind::ArityMismatch {
                            expected: params.len(),
                            found: typed_args.len(),
                        },
                        call.callee.span,
                    ));
                } else {
                    for (arg, (_, param_type)) in typed_args.iter().zip(&params) {
                        // Add constraint if argument is a variable and parameter type is concrete.
                        if let ExprKind::Variable(var_name) = &arg.kind {
                            if !matches!(param_type, Type::Unknown) {
                                self.add_constraint_if_parameter(var_name, param_type.clone());
                            }
                        }
                        if !arg.anno.conforms_to(&param_type, self.registry) {
                            self.errors.push(SemanticError::error(
                                SemanticErrorKind::NotConforming {
                                    found: arg.anno.clone(),
                                    expected: param_type.clone(),
                                },
                                arg.span,
                            ));
                        }
                    }
                }
                return typed_expr(
                    ExprKind::Call(CallExpr { callee: Box::new(typed_callee), args: typed_args }),
                    return_type,
                    call.callee.span,
                );
            }
        }

        // Method call: callee is a member access.
        if let ExprKind::Member(member) = &call.callee.kind {
            let typed_obj = self.infer_expr(&member.object, env);
            let method_name = &member.member;
            if let Some(method_sig) = self.lookup_method(&typed_obj.anno, method_name) {
                // Clone before mutating self.
                let params: Vec<(String, Type)> = method_sig.params.clone();
                let return_type = method_sig.return_type.clone();

                let mut typed_args = Vec::new();
                for arg in &call.args {
                    typed_args.push(self.infer_expr(arg, env));
                }
                if typed_args.len() != params.len() {
                    self.errors.push(SemanticError::error(
                        SemanticErrorKind::ArityMismatch {
                            expected: params.len(),
                            found: typed_args.len(),
                        },
                        call.callee.span,
                    ));
                } else {
                    for (arg, (_, param_type)) in typed_args.iter().zip(&params) {
                        // Add constraint if argument is a variable and parameter type is concrete.
                        if let ExprKind::Variable(var_name) = &arg.kind {
                            if !matches!(param_type, Type::Unknown) {
                                self.add_constraint_if_parameter(var_name, param_type.clone());
                            }
                        }
                        if !arg.anno.conforms_to(&param_type, self.registry) {
                            self.errors.push(SemanticError::error(
                                SemanticErrorKind::NotConforming {
                                    found: arg.anno.clone(),
                                    expected: param_type.clone(),
                                },
                                arg.span,
                            ));
                        }
                    }
                }
                let typed_member = typed_expr(
                    ExprKind::Member(MemberExpr {
                        object: Box::new(typed_obj),
                        member: method_name.clone(),
                    }),
                    return_type.clone(),
                    member.object.span,
                );
                return typed_expr(
                    ExprKind::Call(CallExpr { callee: Box::new(typed_member), args: typed_args }),
                    return_type,
                    call.callee.span,
                );
            } else {
                self.errors.push(SemanticError::error(
                    SemanticErrorKind::UnknownMember { ty: typed_obj.anno.clone(), member: method_name.clone() },
                    call.callee.span,
                ));
                return typed_expr(
                    ExprKind::Call(CallExpr { callee: Box::new(typed_callee), args: Vec::new() }),
                    Type::Error,
                    call.callee.span,
                );
            }
        }

        // Fallback: unrecognized callee.
        self.errors.push(SemanticError::error(
            SemanticErrorKind::UndefinedFunction { name: "unknown".to_string(), arity: call.args.len() },
            call.callee.span,
        ));
        let mut typed_args = Vec::new();
        for arg in &call.args {
            typed_args.push(self.infer_expr(arg, env));
        }
        typed_expr(
            ExprKind::Call(CallExpr { callee: Box::new(typed_callee), args: typed_args }),
            Type::Error,
            call.callee.span,
        )
    }

    // -------------------------------------------------------------------------
    // Member
    // -------------------------------------------------------------------------

    /// Infers the type of a member access (`obj.member`).
    ///
    /// The object is inferred first. If the member is found (attribute or method),
    /// its type is returned. If the object expression is a variable with an
    /// `Unknown` type, and the member uniquely identifies a specific type in the
    /// registry, a constraint is added to infer the object's type.
    ///
    /// Attributes are private and will be checked in a later pass.
    fn infer_member(&mut self, member: &MemberExpr, env: &mut Environment) -> TypedExpr {
        let typed_obj = self.infer_expr(&member.object, env);
        let obj_type = typed_obj.anno.clone();

        if let Some((owner_type, member_type)) = self.lookup_member(&obj_type, &member.member) {
            // If the object is a variable with Unknown type, add a constraint.
            if matches!(obj_type, Type::Unknown) {
                self.constrain_if_variable(&typed_obj, owner_type);
            }
            let typed_member = MemberExpr::new(typed_obj, &member.member);
            typed_expr(ExprKind::Member(typed_member), member_type, member.object.span)
        } else {
            self.errors.push(SemanticError::error(
                SemanticErrorKind::UnknownMember { ty: obj_type, member: member.member.clone() },
                member.object.span,
            ));
            typed_expr(
                        ExprKind::Member(MemberExpr {
                            object: Box::new(typed_obj),
                            member: member.member.clone(),
                        }),
                        Type::Error,
                        member.object.span,
                    )
        }
    }

    // -------------------------------------------------------------------------
    // New
    // -------------------------------------------------------------------------

    /// Infers the type of a `new` expression.
    ///
    /// The type name is resolved in the registry. Each constructor argument is
    /// inferred in the current environment. The arity and type of each argument
    /// are checked against the type's constructor parameters. If the type does
    /// not exist, an `UndefinedType` error is reported. The expression's type is
    /// the resolved `Named` type.
    ///
    /// Errors are reported using the `span` of the `new` expression itself.
    fn infer_new(&mut self, new_expr: &NewExpr, span: SourceSpan, env: &mut Environment) -> TypedExpr {
        let type_name = new_expr.type_name.name.clone();

        // Look up the type in the registry; clone needed data before mutating self.
        let params = match self.registry.lookup_type(&type_name) {
            Some(info) => info.params.clone(),
            None => {
                self.errors.push(SemanticError::error(
                    SemanticErrorKind::UndefinedType(type_name.clone()),
                    span,
                ));
                return typed_expr(
                    ExprKind::New(NewExpr::new(new_expr.type_name.clone(), Vec::new())),
                    Type::Error,
                    span,
                );
            }
        };

        // Infer each argument.
        let mut typed_args = Vec::new();
        for arg in &new_expr.args {
            typed_args.push(self.infer_expr(arg, env));
        }

        // Check arity.
        if typed_args.len() != params.len() {
            self.errors.push(SemanticError::error(
                SemanticErrorKind::ArityMismatch {
                    expected: params.len(),
                    found: typed_args.len(),
                },
                span,
            ));
        } else {
            // Check conformance of each argument.
            for (arg, (_, param_type)) in typed_args.iter().zip(&params) {
                if !arg.anno.conforms_to(param_type, self.registry) {
                    self.errors.push(SemanticError::error(
                        SemanticErrorKind::NotConforming {
                            found: arg.anno.clone(),
                            expected: param_type.clone(),
                        },
                        arg.span,
                    ));
                }
            }
        }

        let result_type = Type::Named(type_name);
        typed_expr(
            ExprKind::New(NewExpr::new(new_expr.type_name.clone(), typed_args)),
            result_type,
            span,
        )
    }

    // -------------------------------------------------------------------------
    // TypeTest (is)
    // -------------------------------------------------------------------------

    /// Infers the type of an `is` type test expression.
    ///
    /// The receiver expression is inferred first. If its type is a builtin value
    /// type (`Number`, `String`, `Boolean`), a `TypeMismatch` error is reported
    /// because such types have no dynamic subtyping. The target type is resolved
    /// in the registry; an `UndefinedType` error is reported if it does not exist.
    /// The expression's type is always `Boolean`.
    ///
    /// The `span` parameter is the span of the entire `is` expression, used for
    /// error reporting when the target type is undefined.
    fn infer_type_test(&mut self, type_test: &TypeTestExpr, span: SourceSpan, env: &mut Environment) -> TypedExpr {
        let type_infered_expr = self.infer_expr(&type_test.expr, env);
        let expr_type = type_infered_expr.anno.clone();
        // Check receiver is Object-rooted (not builtin value).
        match expr_type {
            Type::Number | Type::String | Type::Boolean => {
                self.errors.push(SemanticError::error(
                    SemanticErrorKind::TypeMismatch {
                        expected: Type::Object,
                        found: expr_type,
                    },
                    type_test.expr.span,
                ));
            }
            _ => {}
        }
        // Check type exists.
        let target_name = type_test.type_name.name.clone();
        if !self.registry.lookup_type(&target_name).is_some() && !self.registry.lookup_protocol(&target_name).is_some() {
            self.errors.push(SemanticError::error(
                SemanticErrorKind::UndefinedType(target_name),
                span,
            ));
        }
        typed_expr(ExprKind::TypeTest(TypeTestExpr::new(type_infered_expr, type_test.type_name.clone())), Type::Boolean, type_test.expr.span)
    }

    // -------------------------------------------------------------------------
    // Downcast (as)
    // -------------------------------------------------------------------------
    
    /// Infers the type of an `as` downcast expression.
    ///
    /// The receiver expression is inferred first. Like `is`, if the receiver type
    /// is a builtin value type, a `TypeMismatch` error is reported. The target type
    /// is resolved in the registry; an `UndefinedType` error is reported if it does
    /// not exist. The expression's static type is the target type.
    ///
    /// If the receiver static type and the target type are unrelated in the
    /// hierarchy (neither conforms to the other), an `UnreachableDowncast` warning
    /// is emitted, as the cast can never succeed at runtime. This warning does not
    /// block compilation.
    ///
    /// The `span` parameter is the span of the entire `as` expression, used for
    /// error reporting when the target type is undefined.
    fn infer_downcast(&mut self, downcast: &DowncastExpr, span: SourceSpan, env: &mut Environment) -> TypedExpr {
        let type_infered_expr = self.infer_expr(&downcast.expr, env);
        let expr_type = type_infered_expr.anno.clone();
        // Receiver must be Object-rooted.
        match &expr_type {
            Type::Number | Type::String | Type::Boolean => {
                self.errors.push(SemanticError::error(
                    SemanticErrorKind::TypeMismatch {
                        expected: Type::Object,
                        found: expr_type.clone(),
                    },
                    downcast.expr.span,
                ));
            }
            _ => {}
        }
        let target_name = downcast.type_name.name.clone();
        let target_type = self.resolve_type_ref(&downcast.type_name);
        if !self.registry.lookup_type(&target_name).is_some() && !self.registry.lookup_protocol(&target_name).is_some() {
            self.errors.push(SemanticError::error(
                SemanticErrorKind::UndefinedType(target_name),
                span,
            ));
        }
        // Optional warning: unreachable downcast.
        if !expr_type.conforms_to(&target_type, self.registry) && !target_type.conforms_to(&expr_type, self.registry) {
            self.errors.push(SemanticError::warning(
                SemanticErrorKind::UnreachableDowncast { from: expr_type.clone(), to: target_type.clone() },
                downcast.expr.span,
            ));
        }
        typed_expr(ExprKind::Downcast(DowncastExpr::new(type_infered_expr, downcast.type_name.clone())), target_type, downcast.expr.span)
    }

    // -------------------------------------------------------------------------
    // Vector
    // -------------------------------------------------------------------------

    fn infer_vector(&mut self, vector: &VectorExpr, env: &mut Environment) -> TypedExpr {
        match vector {
            VectorExpr::Literal(items) => {
                let mut typed_items = Vec::new();
                for item in items {
                    typed_items.push(self.infer_expr(item, env));
                }
                let item_types: Vec<Type> = typed_items.iter().map(|e| e.anno.clone()).collect();
                let elem_type = if item_types.is_empty() {
                    Type::Unknown
                } else {
                    lowest_common_ancestor(&item_types, self.registry)
                };
                let result_type = Type::Vector(Box::new(elem_type));
                typed_expr(ExprKind::Vector(VectorExpr::Literal(typed_items)), result_type, items.first().map(|e| e.span).unwrap_or(SourceSpan::new(0, 0)))
            }
            VectorExpr::Comprehension(comp) => {
                // Infer iterable.
                let typed_iterable = self.infer_expr(&comp.iterable, env);
                // Get element type (same as for loop).
                let elem_type = if let Type::Iterable(inner) = &typed_iterable.anno {
                    *inner.clone()
                } else if let Type::Vector(inner) = &typed_iterable.anno {
                    *inner.clone()
                } else if self.is_iterable(&typed_iterable.anno) {
                    match self.iterable_element_type(&typed_iterable.anno) {
                        Some(ty) => ty,
                        None => {
                            self.errors.push(SemanticError::error(
                                SemanticErrorKind::NotIterable(typed_iterable.anno.clone()),
                                comp.iterable.span,
                            ));
                            Type::Error
                        }
                    }
                } else {
                    self.errors.push(SemanticError::error(
                        SemanticErrorKind::NotIterable(typed_iterable.anno.clone()),
                        comp.iterable.span,
                    ));
                    Type::Error
                };
                // Push scope for the bound variable.
                env.push_scope();
                env.declare(&comp.var, elem_type.clone(), comp.iterable.span);
                let typed_head = self.infer_expr(&comp.expr, env);
                env.pop_scope();
                let result_type = Type::Vector(Box::new(typed_head.anno.clone()));
                let comp_typed = VectorComprehension::new(typed_head, &comp.var, typed_iterable);
                typed_expr(ExprKind::Vector(VectorExpr::Comprehension(comp_typed)), result_type, comp.iterable.span)
            }
        }
    }

    // -------------------------------------------------------------------------
    // Index
    // -------------------------------------------------------------------------

    fn infer_index(&mut self, index: &IndexExpr, env: &mut Environment) -> TypedExpr {
        let typed_obj = self.infer_expr(&index.object, env);
        let typed_idx = self.infer_expr(&index.index, env);
        let obj_type = typed_obj.anno.clone();
        let idx_type = typed_idx.anno.clone();

        if let Type::Vector(inner) = &obj_type {
            if matches!(idx_type, Type::Number) {
                let result_type = *inner.clone();
                typed_expr(ExprKind::Index(IndexExpr::new(typed_obj, typed_idx)), result_type, index.object.span)
            } else {
                self.errors.push(SemanticError::error(
                    SemanticErrorKind::TypeMismatch {
                        expected: Type::Number,
                        found: idx_type,
                    },
                    index.index.span,
                ));
                typed_expr(ExprKind::Index(IndexExpr::new(typed_obj, typed_idx)), Type::Error, index.object.span)
            }
        } else {
            self.errors.push(SemanticError::error(
                SemanticErrorKind::IndexOnNonVector(obj_type),
                index.object.span,
            ));
            typed_expr(ExprKind::Index(IndexExpr::new(typed_obj, typed_idx)), Type::Error, index.object.span)
        }
    }

    // -------------------------------------------------------------------------
    // Match (project extension)
    // -------------------------------------------------------------------------

    /// Infers the type of a `match` expression (project extension).
    ///
    /// The scrutinee is inferred once, then each case pattern is checked against its type.
    /// Each case body is inferred in its own environment that includes any bindings from the pattern.
    /// The whole expression's type is the LCA of all case body types.
    ///
    /// A warning is emitted if no catch‑all pattern (wildcard or variable) is present.
    fn infer_match(&mut self, match_expr: &MatchExpr, env: &mut Environment) -> TypedExpr {
        let typed_value = self.infer_expr(&match_expr.value, env);
        let value_type = typed_value.anno.clone();

        let mut typed_cases = Vec::new();
        let mut case_types = Vec::new();
        let mut has_catch_all = false;

        for case in &match_expr.cases {
            // Pass the body span as the fallback for pattern errors.
            let (pattern_typed, mut case_env, catch) =
                self.infer_pattern(&case.pattern, &value_type, env, case.body.span);

            let typed_body = self.infer_expr(&case.body, &mut case_env);
            typed_cases.push(MatchCase::new(pattern_typed, typed_body.clone()));
            case_types.push(typed_body.anno.clone());

            if catch {
                has_catch_all = true;
            }
        }

        if !has_catch_all {
            self.errors.push(SemanticError::warning(
                SemanticErrorKind::NonExhaustiveMatch,
                match_expr.value.span,
            ));
        }

        let result_type = lowest_common_ancestor(&case_types, self.registry);
        let match_typed = MatchExpr::new(typed_value, typed_cases);
        typed_expr(ExprKind::Match(match_typed), result_type, match_expr.value.span)
    }

    /// Checks a pattern against the scrutinee type and produces a typed pattern,
    /// an environment for the case body, and a flag indicating whether the pattern
    /// is a catch‑all (wildcard or bare variable).
    ///
    /// For literal patterns, the literal type must match the scrutinee exactly.
    /// For variable patterns, the variable is bound to the scrutinee type.
    /// For type patterns, the scrutinee must be `Object`‑rooted and the alias
    /// (if present) is bound to the narrowed type.
    ///
    /// The `span` parameter is used for error reporting and should point to the
    /// location of the pattern or its enclosing case body.
    fn infer_pattern(
        &mut self,
        pattern: &Pattern,
        scrutinee_type: &Type,
        env: &Environment,
        span: SourceSpan,
    ) -> (Pattern, Environment, bool) {
        let mut case_env = env.clone();
        let catch_all = match pattern {
            Pattern::Wildcard => (Pattern::Wildcard, true),

            Pattern::Literal(lit) => {
                let lit_type = match lit {
                    Literal::Number(_) => Type::Number,
                    Literal::String(_) => Type::String,
                    Literal::Boolean(_) => Type::Boolean,
                };
                if lit_type != *scrutinee_type {
                    self.errors.push(SemanticError::error(
                        SemanticErrorKind::TypeMismatch {
                            expected: scrutinee_type.clone(),
                            found: lit_type,
                        },
                        span,
                    ));
                }
                (Pattern::Literal(lit.clone()), false)
            }

            Pattern::Variable(name) => {
                case_env.declare(name, scrutinee_type.clone(), span);
                (Pattern::Variable(name.clone()), true)
            }

            Pattern::Type(type_ref, alias) => {
                // Scrutinee must be Object‑rooted (not a builtin value type).
                match scrutinee_type {
                    Type::Number | Type::String | Type::Boolean => {
                        self.errors.push(SemanticError::error(
                            SemanticErrorKind::TypeMismatch {
                                expected: Type::Object,
                                found: scrutinee_type.clone(),
                            },
                            span,
                        ));
                    }
                    _ => {}
                }

                let target_type = self.resolve_type_ref(type_ref);
                if let Some(alias_name) = alias {
                    case_env.declare(alias_name, target_type.clone(), span);
                }
                (Pattern::Type(type_ref.clone(), alias.clone()), false)
            }
        };
        (catch_all.0, case_env, catch_all.1)
    }

    // -------------------------------------------------------------------------
    // Helper: resolve TypeRef to Type
    // -------------------------------------------------------------------------

    fn resolve_type_ref(&self, tr: &TypeRef) -> Type {
        match tr.name.as_str() {
            "Number" => Type::Number,
            "String" => Type::String,
            "Boolean" => Type::Boolean,
            "Object" => Type::Object,
            _ => {
                if tr.args.is_empty() {
                    Type::Named(tr.name.clone())
                } else {
                    let args: Vec<Type> = tr.args.iter().map(|arg| self.resolve_type_ref(arg)).collect();
                    match tr.name.as_str() {
                        "Vector" if !args.is_empty() => Type::Vector(Box::new(args[0].clone())),
                        "Iterable" if !args.is_empty() => Type::Iterable(Box::new(args[0].clone())),
                        _ => Type::Named(tr.name.clone()),
                    }
                }
            }
        }
    }

    /// -----------------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------------

    // Helper to look up a method on a type.
    fn lookup_method(&self, ty: &Type, method_name: &str) -> Option<&MethodSignature> {
        match ty {
            Type::Named(name) => {
                if let Some(info) = self.registry.lookup_type(name) {
                    // Use flattened methods.
                    if let Some(sig) = info.flattened_methods.get(method_name) {
                        return Some(sig);
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// Looks up a member (attribute or method) on a type.
    /// Returns `Some((owner_type, member_type))` if found, where `owner_type`
    /// is the type that defines the member and `member_type` is the type of the
    /// member (attribute type or method return type).
    fn lookup_member(&self, ty: &Type, member_name: &str) -> Option<(Type, Type)> {
        match ty {
            Type::Named(name) => {
                if let Some(info) = self.registry.lookup_type(name) {
                    // Check attributes first.
                    if let Some(attr) = info.attributes.get(member_name) {
                        if let Some(declared) = &attr.declared_type {
                            return Some((Type::Named(name.clone()), declared.clone()));
                        }
                    }
                    // Check methods.
                    if let Some(method) = info.flattened_methods.get(member_name) {
                        return Some((Type::Named(name.clone()), method.return_type.clone()));
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// If `name` is an unannotated parameter, record that it must be compatible with `ty`.
    fn add_constraint_if_parameter(&mut self, name: &str, ty: Type) {
        if let Some(constraints) = self.param_constraints.get_mut(name) {
            constraints.push(ty);
        }
    }

    /// If the expression is a variable, and it is an unannotated parameter, add a constraint.
    fn constrain_if_variable(&mut self, expr: &TypedExpr, required_type: Type) {
        if let ExprKind::Variable(name) = &expr.kind {
            self.add_constraint_if_parameter(name, required_type);
        }
    }
}

/// Helper to create a typed expression with the given kind, annotation, and span.
fn typed_expr(kind: ExprKind<Type>, anno: Type, span: SourceSpan) -> TypedExpr {
    TypedExpr { kind, anno, span }
}
