//! Lowering of typed HULK expressions to LLVM IR.
//!
//! This module implements Phase 3 of the code generation pipeline:
//! converting the typed AST (`hulk_ast::Expr<Type>`) into LLVM IR
//! instructions. It handles literals, variables, unary/binary operators,
//! control flow (`if`/`while`), blocks, `let` bindings, and assignments to
//! variable targets. More complex constructs (calls, member access, object
//! creation, vectors, and pattern matching) are deferred to later phases and
//! are currently stubbed as `CodegenError::Unsupported`.
//!
//! The design follows the standard LLVM front‑end pattern: every local
//! variable is allocated on the stack via `alloca`, and the `mem2reg` pass
//! (Phase 8) later promotes these to SSA values. This avoids manual SSA
//! bookkeeping and keeps the lowering code simple and correct.

use inkwell::values::{BasicValueEnum, PointerValue};
use inkwell::types::BasicTypeEnum;

use hulk_ast::Expr;
use hulk_semantic::{Type, TypeRegistry};

use crate::context::CodegenCtx;
use crate::error::CodegenError;
use crate::lower::scope::ScopeStack; 

// ─── Submodules ───────────────────────────────────────────────────────────

pub mod utils;
pub mod scope;
pub mod binding;
pub mod control;
pub mod literal;
pub mod operators;
pub mod vector;
pub mod object;
pub mod call;

// ─── Lowering context ────────────────────────────────────────────────────

/// Context for lowering a single expression tree.
///
/// This struct holds all the mutable state needed during expression lowering:
/// the LLVM context, the current module and builder, a stack of lexical
/// scopes for local variables, and a read‑only reference to the type registry
/// from semantic analysis.
pub struct LowerCtx<'a, 'ctx> {
    /// The LLVM context, module, and instruction builder.
    pub codegen: &'a mut CodegenCtx<'ctx>,
    /// Stack of lexical scopes, mirroring `hulk_semantic::Environment`.
    pub scope_stack: ScopeStack<'ctx>,
    /// Read‑only registry from semantic analysis.
    pub registry: &'a TypeRegistry,
}

impl<'a, 'ctx> LowerCtx<'a, 'ctx> {
    /// Creates a new lowering context.
    pub fn new(codegen: &'a mut CodegenCtx<'ctx>, registry: &'a TypeRegistry) -> Self {
        Self {
            codegen,
            scope_stack: ScopeStack::new(),
            registry,
        }
    }

    /// Pushes a new lexical scope.
    pub fn push_scope(&mut self) {
        self.scope_stack.push_scope();
    }

    /// Pops the innermost lexical scope.
    pub fn pop_scope(&mut self) {
        self.scope_stack.pop_scope();
    }

    /// Declares a variable in the current scope and initialises it with `value`.
    pub fn declare_var(&mut self, name: &str, value: BasicValueEnum<'ctx>) -> Result<(), CodegenError> {
        let ty = value.get_type();
        let ptr = self.codegen.builder.build_alloca(ty, name)
            .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;
        self.codegen.builder.build_store(ptr, value)
            .map_err(|e| CodegenError::LlvmVerification(e.to_string()))?;
        self.scope_stack.declare(name, ptr, ty);
        Ok(())
    }

    /// Looks up a variable's pointer in the scope stack.
    pub fn lookup_var(&self, name: &str) -> Result<(PointerValue<'ctx>, BasicTypeEnum<'ctx>), CodegenError> {
        self.scope_stack.lookup(name)
            .ok_or_else(|| CodegenError::Unsupported {
                construct: format!("undefined variable `{}` (should have been caught by semantic analysis)", name)
            })
    }

    /// Loads a variable's value.
    pub fn load_var(&self, name: &str) -> Result<BasicValueEnum<'ctx>, CodegenError> {
        let (ptr, ty) = self.scope_stack.lookup(name)
            .ok_or_else(|| CodegenError::Unsupported {
                construct: format!("undefined variable `{}`", name)
            })?;
        self.codegen.builder.build_load(ty, ptr, name)
            .map_err(|e| CodegenError::LlvmVerification(e.to_string()))
    }
}

/// Lowers a typed expression to an LLVM value.
///
/// This is the main entry point for expression lowering. It matches on the
/// `ExprKind` and delegates to the appropriate submodule for the actual
/// lowering logic. The submodules (`literal`, `operators`, `control`,
/// `binding`, etc.) each handle a specific category of language constructs.
///
/// # Returns
/// An LLVM `BasicValueEnum` representing the computed value of the
/// expression. The exact type depends on the expression's static HULK type
/// (e.g., `f64` for `Number`, `i1` for `Boolean`, a pointer for `String`).
///
/// # Errors
/// Returns `CodegenError::Unsupported` for constructs that are not yet
/// implemented, or `CodegenError::LlvmVerification` if LLVM
/// instruction emission fails.
pub fn lower_expr<'ctx>(
    ctx: &mut LowerCtx<'_, 'ctx>,
    expr: &Expr<Type>,
) -> Result<BasicValueEnum<'ctx>, CodegenError> {
    use hulk_ast::ExprKind;

    match &expr.kind {
        // ─── Literals ─────────────────────────────────────────────────────

        ExprKind::Literal(lit) => literal::lower_literal(ctx, lit),

        // ─── Variables and special references ────────────────────────────

        ExprKind::Variable(name) => binding::lower_variable(ctx, name),
        ExprKind::SelfRef | ExprKind::BaseRef => {
            Err(CodegenError::Unsupported {
                construct: format!("{:?} not supported", expr.kind)
            })
        }

        // ─── Unary and binary operators ──────────────────────────────────

        ExprKind::Unary(unary) => operators::lower_unary(ctx, unary),
        ExprKind::Binary(binary) => operators::lower_binary(ctx, binary, &expr.anno),

        // ─── Control flow ────────────────────────────────────────────────

        ExprKind::Block(block) => control::lower_block(ctx, block),
        ExprKind::If(if_expr) => control::lower_if(ctx, if_expr, &expr.anno),
        ExprKind::While(while_expr) => control::lower_while(ctx, while_expr, &expr.anno),

        // ─── Bindings and assignments ────────────────────────────────────

        ExprKind::Let(let_expr) => binding::lower_let(ctx, let_expr),
        ExprKind::Assign(assign) => binding::lower_assign(ctx, assign),

        // ─── Deferred to later phases ────────────────────────────────────

        ExprKind::Call(_) => {
            Err(CodegenError::Unsupported {
                construct: "calls not yet supported".into()
            })
        }
        ExprKind::Member(_) => {
            Err(CodegenError::Unsupported {
                construct: "member access not yet supported".into()
            })
        }
        ExprKind::New(_) => {
            Err(CodegenError::Unsupported {
                construct: "object construction not yet supported".into()
            })
        }
        ExprKind::TypeTest(_) | ExprKind::Downcast(_) => {
            Err(CodegenError::Unsupported {
                construct: "type tests/downcasts not yet supported".into()
            })
        }
        ExprKind::Vector(_) => {
            Err(CodegenError::Unsupported {
                construct: "vectors not yet supported".into()
            })
        }
        ExprKind::Index(_) => {
            Err(CodegenError::Unsupported {
                construct: "indexing not yet supported".into()
            })
        }
        ExprKind::Match(_) => {
            Err(CodegenError::Unsupported {
                construct: "match not yet supported".into()
            })
        }
        // ─── Catch-all for unhandled cases ───────────────────────────────
        _ => Err(CodegenError::Unsupported {
            construct: format!("lowering of {:?} not yet implemented", expr.kind)
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hulk_ast::{
        AssignExpr, AssignTarget, BinaryExpr, BinaryOp, BlockExpr, ElifBranch, Expr, ExprKind,
        IfExpr, LetBinding, LetExpr, Literal, SourceSpan, UnaryExpr, UnaryOp, WhileExpr,
    };
    use hulk_semantic::{seeded_registry, Type};
    use inkwell::context::Context;

    /// Dummy span for tests.
    fn dummy_span() -> SourceSpan {
        SourceSpan::new(0, 0)
    }

    /// Helper: create a typed number literal.
    fn num(n: f64) -> Expr<Type> {
        Expr {
            kind: ExprKind::Literal(Literal::Number(n)),
            anno: Type::Number,
            span: dummy_span(),
        }
    }

    /// Helper: create a typed boolean literal.
    fn bool_lit(b: bool) -> Expr<Type> {
        Expr {
            kind: ExprKind::Literal(Literal::Boolean(b)),
            anno: Type::Boolean,
            span: dummy_span(),
        }
    }

    /// Helper: create a typed string literal.
    fn string_lit(s: &str) -> Expr<Type> {
        Expr {
            kind: ExprKind::Literal(Literal::String(s.to_string())),
            anno: Type::String,
            span: dummy_span(),
        }
    }

    /// Helper: create a typed variable.
    fn var(name: &str, ty: Type) -> Expr<Type> {
        Expr {
            kind: ExprKind::Variable(name.to_string()),
            anno: ty,
            span: dummy_span(),
        }
    }

    /// Helper: create a typed binary expression.
    fn bin_op(left: Expr<Type>, op: BinaryOp, right: Expr<Type>, ty: Type) -> Expr<Type> {
        Expr {
            kind: ExprKind::Binary(BinaryExpr {
                op,
                left: Box::new(left),
                right: Box::new(right),
            }),
            anno: ty,
            span: dummy_span(),
        }
    }

    /// Helper: create a typed unary expression.
    fn unary_op(op: UnaryOp, operand: Expr<Type>, ty: Type) -> Expr<Type> {
        Expr {
            kind: ExprKind::Unary(UnaryExpr {
                op,
                expr: Box::new(operand),
            }),
            anno: ty,
            span: dummy_span(),
        }
    }

    /// Helper: create a typed let expression.
    fn let_expr(bindings: Vec<(String, Expr<Type>)>, body: Expr<Type>, ty: Type) -> Expr<Type> {
        let let_bindings = bindings
            .into_iter()
            .map(|(name, init)| LetBinding {
                name,
                type_annotation: None,
                initializer: *Box::new(init),
            })
            .collect();
        Expr {
            kind: ExprKind::Let(LetExpr {
                bindings: let_bindings,
                body: Box::new(body),
            }),
            anno: ty,
            span: dummy_span(),
        }
    }

    /// Helper: create a typed block.
    fn block(exprs: Vec<Expr<Type>>, ty: Type) -> Expr<Type> {
        Expr {
            kind: ExprKind::Block(BlockExpr { expressions: exprs }),
            anno: ty,
            span: dummy_span(),
        }
    }

    /// Helper: create a typed if expression.
    fn if_expr(
        cond: Expr<Type>,
        then_branch: Expr<Type>,
        elifs: Vec<ElifBranch<Type>>,
        else_branch: Expr<Type>,
        ty: Type,
    ) -> Expr<Type> {
        Expr {
            kind: ExprKind::If(IfExpr {
                condition: Box::new(cond),
                then_branch: Box::new(then_branch),
                elif_branches: elifs,
                else_branch: Box::new(else_branch),
            }),
            anno: ty,
            span: dummy_span(),
        }
    }

    /// Helper: create a typed while expression.
    fn while_expr(cond: Expr<Type>, body: Expr<Type>, ty: Type) -> Expr<Type> {
        Expr {
            kind: ExprKind::While(WhileExpr {
                condition: Box::new(cond),
                body: Box::new(body),
            }),
            anno: ty,
            span: dummy_span(),
        }
    }

    /// Helper: create a typed assignment.
    fn assign(target: AssignTarget<Type>, value: Expr<Type>, ty: Type) -> Expr<Type> {
        Expr {
            kind: ExprKind::Assign(AssignExpr {
                target,
                value: Box::new(value),
            }),
            anno: ty,
            span: dummy_span(),
        }
    }

    /// Helper: create a typed self reference.
    fn self_ref(ty: Type) -> Expr<Type> {
        Expr {
            kind: ExprKind::SelfRef,
            anno: ty,
            span: dummy_span(),
        }
    }

    /// Helper: create a typed call.
    fn call(callee: Expr<Type>, args: Vec<Expr<Type>>, ty: Type) -> Expr<Type> {
        Expr {
            kind: ExprKind::Call(hulk_ast::CallExpr {
                callee: Box::new(callee),
                args,
            }),
            anno: ty,
            span: dummy_span(),
        }
    }

    /// Helper: lower an expression and return the LLVM IR string.
    fn lower_expr_to_ir(expr: Expr<Type>) -> String {
        let context = Context::create();
        let mut codegen = CodegenCtx::new(&context, "test");
        let i32_type = context.i32_type();
        let main_fn = codegen.module.add_function("main", i32_type.fn_type(&[], false), None);
        let entry_bb = context.append_basic_block(main_fn, "entry");
        codegen.builder.position_at_end(entry_bb);

        let registry = seeded_registry();
        // Lower the expression in a separate scope so the borrow ends.
        {
            let mut lower_ctx = LowerCtx::new(&mut codegen, &registry);
            let val = lower_expr(&mut lower_ctx, &expr).expect("lowering failed");
            // Store the result so it's a real operand of a `store` instruction
            let slot = lower_ctx.codegen.builder
                .build_alloca(val.get_type(), "test_result")
                .expect("alloca failed");
            lower_ctx.codegen.builder.build_store(slot, val).expect("store failed");
        }

        // Now we can use codegen freely.
        codegen
            .builder
            .build_return(Some(&i32_type.const_int(0, false)))
            .expect("return failed");
        codegen.module.verify().expect("module verification failed");
        codegen.module.print_to_string().to_string()
    }

    /// Assert that the IR contains a substring.
    fn assert_ir_contains(ir: &str, expected: &str) {
        assert!(
            ir.contains(expected),
            "IR did not contain '{}'",
            expected
        );
    }

    // ─── Literals ─────────────────────────────────────────────────────────

    #[test]
    fn test_literal_number() {
        let expr = num(42.0);
        let ir = lower_expr_to_ir(expr);
        assert_ir_contains(&ir, "double 4.200000e+01");
    }

    #[test]
    fn test_literal_bool() {
        let expr = bool_lit(true);
        let ir = lower_expr_to_ir(expr);
        assert_ir_contains(&ir, "i1 true");
    }

    #[test]
    fn test_literal_string() {
        let expr = string_lit("hello");
        let ir = lower_expr_to_ir(expr);
        assert_ir_contains(&ir, "@str_data_0 = private unnamed_addr constant [5 x i8] c\"hello\"");
        assert_ir_contains(&ir, "@str_0 = private unnamed_addr constant { i64, ptr } { i64 5, ptr @str_data_0 }");
 }

    // ─── Variables and Let ──────────────────────────────────────────────

    #[test]
    fn test_variable_and_let() {
        // let x = 5 in x
        let init = num(5.0);
        let body = var("x", Type::Number);
        let expr = let_expr(vec![("x".to_string(), init)], body, Type::Number);
        let ir = lower_expr_to_ir(expr);
        assert_ir_contains(&ir, "alloca double");
        assert_ir_contains(&ir, "store double 5.000000e+00");
        assert_ir_contains(&ir, "load double");
    }

    #[test]
    fn test_let_shadowing() {
        // let a = 1 in let a = 2 in a
        let inner_init = num(2.0);
        let inner_body = var("a", Type::Number);
        let inner_let = let_expr(vec![("a".to_string(), inner_init)], inner_body, Type::Number);
        let outer_let = let_expr(vec![("a".to_string(), num(1.0))], inner_let, Type::Number);
        let ir = lower_expr_to_ir(outer_let);
        assert_ir_contains(&ir, "alloca double");
        assert_ir_contains(&ir, "load double");
    }

    #[test]
    fn test_block() {
        // { 1; 2; 3 } -> 3
        let exprs = vec![num(1.0), num(2.0), num(3.0)];
        let block_expr = block(exprs, Type::Number);
        let ir = lower_expr_to_ir(block_expr);
        assert_ir_contains(&ir, "double 3.000000e+00");
    }

    // ─── Unary ────────────────────────────────────────────────────────────

    #[test]
    fn test_unary_negate() {
        // let x = 5 in -x
        let neg = unary_op(UnaryOp::Negate, var("x", Type::Number), Type::Number);
        let expr = let_expr(vec![("x".to_string(), num(5.0))], neg, Type::Number);
        let ir = lower_expr_to_ir(expr);
        assert_ir_contains(&ir, "load double, ptr %x");
        assert_ir_contains(&ir, "fneg double");
    }

    #[test]
    fn test_unary_not() {
        // let b = true in !b
        let not_expr = unary_op(UnaryOp::Not, var("b", Type::Boolean), Type::Boolean);
        let expr = let_expr(vec![("b".to_string(), bool_lit(true))], not_expr, Type::Boolean);
        let ir = lower_expr_to_ir(expr);
        assert_ir_contains(&ir, "load i1, ptr %b");
        assert_ir_contains(&ir, "xor i1");
    }

    // ─── Binary ──────────────────────────────────────────────────────────

    #[test]
    fn test_binary_add() {
        // let a = 2 in let b = 3 in a + b
        let sum = bin_op(var("a", Type::Number), BinaryOp::Add, var("b", Type::Number), Type::Number);
        let with_b = let_expr(vec![("b".to_string(), num(3.0))], sum, Type::Number);
        let expr = let_expr(vec![("a".to_string(), num(2.0))], with_b, Type::Number);
        let ir = lower_expr_to_ir(expr);
        assert_ir_contains(&ir, "load double, ptr %a");
        assert_ir_contains(&ir, "load double, ptr %b");
        assert_ir_contains(&ir, "fadd double");
    }
    #[test]
    fn test_binary_power() {
        let expr = bin_op(num(2.0), BinaryOp::Power, num(3.0), Type::Number);
        let ir = lower_expr_to_ir(expr);
        assert_ir_contains(
            &ir,
            "call double @llvm.pow.f64(double 2.000000e+00, double 3.000000e+00)",
        );
    }

    #[test]
    fn test_binary_concat() {
        let left = string_lit("Hello");
        let right = string_lit("World");
        let expr = bin_op(left, BinaryOp::Concat, right, Type::String);
        let ir = lower_expr_to_ir(expr);
        assert_ir_contains(&ir, "call ptr @hulk_rt_string_concat(ptr @str_0, ptr @str_1)");
 }

    #[test]
    fn test_binary_concat_space() {
        let left = num(42.0);
        let right = bool_lit(true);
        let expr = bin_op(left, BinaryOp::ConcatSpace, right, Type::String);
        let ir = lower_expr_to_ir(expr);
        assert_ir_contains(&ir, "call ptr @hulk_rt_number_to_string(double 4.200000e+01)");
        assert_ir_contains(&ir, "call ptr @hulk_rt_bool_to_string(i1 true)");
        assert_ir_contains(&ir, "call ptr @hulk_rt_string_concat_space");
    }

    #[test]
    fn test_binary_compare() {
        // let a = 5 in let b = 3 in a < b
        let cmp = bin_op(var("a", Type::Number), BinaryOp::Less, var("b", Type::Number), Type::Boolean);
        let with_b = let_expr(vec![("b".to_string(), num(3.0))], cmp, Type::Boolean);
        let expr = let_expr(vec![("a".to_string(), num(5.0))], with_b, Type::Boolean);
        let ir = lower_expr_to_ir(expr);
        assert_ir_contains(&ir, "load double, ptr %a");
        assert_ir_contains(&ir, "load double, ptr %b");
        assert_ir_contains(&ir, "fcmp olt double");
    }

    // ─── If ──────────────────────────────────────────────────────────────

    #[test]
    fn test_if_simple() {
        let cond = bool_lit(true);
        let then_branch = num(1.0);
        let else_branch = num(2.0);
        let expr = if_expr(cond, then_branch, vec![], else_branch, Type::Number);
        let ir = lower_expr_to_ir(expr);
        assert_ir_contains(&ir, "br i1 true, label %if_then, label %if_else");
        assert_ir_contains(&ir, "if_then:");
        assert_ir_contains(&ir, "store double 1.000000e+00, ptr %if_result");
        assert_ir_contains(&ir, "if_else:");
        assert_ir_contains(&ir, "store double 2.000000e+00, ptr %if_result");
        assert_ir_contains(&ir, "if_merge:");
        assert_ir_contains(&ir, "load double, ptr %if_result");
    }

    #[test]
    fn test_if_with_elif() {
        let cond1 = bool_lit(false);
        let then1 = num(1.0);
        let elif_cond = bool_lit(true);
        let elif_body = num(2.0);
        let else_branch = num(3.0);
        let expr = if_expr(
            cond1,
            then1,
            vec![ElifBranch {
                condition: *Box::new(elif_cond),
                body: *Box::new(elif_body),
            }],
            else_branch,
            Type::Number,
        );
        let ir = lower_expr_to_ir(expr);
        assert_ir_contains(&ir, "br i1 false, label %if_then, label %if_else");
        assert_ir_contains(&ir, "elif_then_0:");
        assert_ir_contains(&ir, "store double 2.000000e+00");
        assert_ir_contains(&ir, "elif_else_0:");
        assert_ir_contains(&ir, "store double 3.000000e+00");
        assert_ir_contains(&ir, "load double, ptr %if_result");
    }

    // ─── While ───────────────────────────────────────────────────────────

    #[test]
    fn test_while_loop() {
        let cond = bool_lit(true);
        let body = num(5.0);
        let expr = while_expr(cond, body, Type::Number);
        let ir = lower_expr_to_ir(expr);
        assert_ir_contains(&ir, "while_cond:");
        assert_ir_contains(&ir, "br i1 true, label %while_body, label %while_exit");
        assert_ir_contains(&ir, "while_body:");
        assert_ir_contains(&ir, "store double 5.000000e+00, ptr %while_result");
        assert_ir_contains(&ir, "br label %while_cond");
        assert_ir_contains(&ir, "while_exit:");
        assert_ir_contains(&ir, "load double, ptr %while_result");
    }

    // ─── Assign ──────────────────────────────────────────────────────────

    #[test]
    fn test_assign_variable() {
        // let x = 0 in x := 5; x
        let init = num(0.0);
        let assign_expr = assign(
            AssignTarget::Variable("x".to_string()),
            num(5.0),
            Type::Number,
        );
        let body = block(vec![assign_expr, var("x", Type::Number)], Type::Number);
        let expr = let_expr(vec![("x".to_string(), init)], body, Type::Number);
        let ir = lower_expr_to_ir(expr);
        assert_ir_contains(&ir, "store double 5.000000e+00, ptr %x");
        assert_ir_contains(&ir, "load double, ptr %x");
    }

    // ─── Unsupported constructs ──────────────────────────────────────────

    #[test]
    fn test_unsupported_self_ref() {
        let expr = self_ref(Type::Object);
        let result = std::panic::catch_unwind(|| {
            let _ir = lower_expr_to_ir(expr);
        });
        assert!(result.is_err(), "SelfRef should panic or return Unsupported");
    }

    #[test]
    fn test_unsupported_call() {
        let callee = var("print", Type::Object);
        let call_expr = call(callee, vec![num(42.0)], Type::Object);
        let result = std::panic::catch_unwind(|| {
            let _ir = lower_expr_to_ir(call_expr);
        });
        assert!(result.is_err(), "Call should be unsupported in Phase 3");
    }

    // ─── Edge cases ──────────────────────────────────────────────────────

    #[test]
    fn test_empty_block() {
        let block_expr = block(vec![], Type::Boolean);
        let ir = lower_expr_to_ir(block_expr);
        assert_ir_contains(&ir, "i1 false");
    }

    #[test]
    fn test_while_default_value() {
        let cond = bool_lit(false);
        let body = num(5.0);
        let expr = while_expr(cond, body, Type::Number);
        let ir = lower_expr_to_ir(expr);
        assert_ir_contains(&ir, "store double 0.000000e+00, ptr %while_result");
        assert_ir_contains(&ir, "load double, ptr %while_result");
    }
}