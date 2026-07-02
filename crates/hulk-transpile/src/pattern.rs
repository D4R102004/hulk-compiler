// hulk-transpile/src/pattern.rs
//
// Structural pattern matching for macro arguments (§A.14.5).
//
// This module provides the `try_match` function, which compares a macro
// pattern (representing an AST shape) with a concrete `Expr` AST node.
// On success, it returns a mapping from capture variable names to the
// matched sub‑expressions. The expansion engine in `substitute.rs` uses this
// to implement compile‑time `match` inside macro bodies.

use std::collections::HashMap;
use hulk_ast::{
    Expr, ExprKind, MacroPattern, MacroPatternBind,
};

/// Attempts to match a `pattern` against the concrete AST node `expr`.
///
/// # Returns
/// - `Some(bindings)` if the pattern matches, where `bindings` maps each
///   capture variable name to the `Expr` subtree that was matched.
/// - `None` if the pattern does not match.
///
/// # Matching rules
/// - `MacroPattern::Wildcard` always matches, producing no bindings.
/// - `MacroPattern::Literal(lit)` matches only if `expr` is a literal with
///   the same value.
/// - `MacroPattern::Bind` recursively matches its inner pattern; if successful
///   and the bind has a `name`, the matched expression is inserted into the map.
/// - `MacroPattern::BinaryExpr` matches an `ExprKind::Binary` node with the
///   given operator, then recursively matches the left and right operands
///   (using `MacroPatternBind` to capture them if named).
/// - `MacroPattern::UnaryExpr` matches an `ExprKind::Unary` node similarly.
///
/// # Duplicate bindings
/// If the same variable name appears more than once in a single pattern,
/// the match fails (returns `None`). This prevents ambiguous captures.
pub fn try_match(pattern: &MacroPattern, expr: &Expr) -> Option<HashMap<String, Expr>> {
    match pattern {
        MacroPattern::Wildcard => Some(HashMap::new()),

        MacroPattern::Literal(lit) => {
            if let ExprKind::Literal(ref e_lit) = expr.kind {
                if e_lit == lit {
                    return Some(HashMap::new());
                }
            }
            None
        }

        MacroPattern::Bind { name, ty: _, pattern: inner } => {
            let mut map = try_match(inner, expr)?;
            if let Some(ref n) = name {
                // Duplicate binding → fail the match.
                if map.contains_key(n) {
                    return None;
                }
                map.insert(n.clone(), expr.clone());
            }
            Some(map)
        }

        MacroPattern::BinaryExpr { op, left, right } => {
            if let ExprKind::Binary(ref bin) = expr.kind {
                if bin.op == *op {
                    let left_map = match_bind(left, &bin.left)?;
                    let right_map = match_bind(right, &bin.right)?;
                    return merge_maps(left_map, right_map);
                }
            }
            None
        }

        MacroPattern::UnaryExpr { op, operand } => {
            if let ExprKind::Unary(ref unary) = expr.kind {
                if unary.op == *op {
                    return match_bind(operand, &unary.expr);
                }
            }
            None
        }
    }
}

/// Matches a `MacroPatternBind` against an expression.
///
/// This first recursively matches the inner pattern. If that succeeds and the
/// bind has a `name`, the whole expression is inserted into the bindings map
/// under that name. The optional type annotation (`ty`) is ignored during matching.
fn match_bind(
    bind: &MacroPatternBind,
    expr: &Expr,
) -> Option<HashMap<String, Expr>> {
    let mut map = try_match(&bind.pattern, expr)?;
    if let Some(ref name) = bind.name {
        if map.contains_key(name) {
            return None;
        }
        map.insert(name.clone(), expr.clone());
    }
    Some(map)
}

/// Merges two binding maps. Returns `None` if they share any key.
fn merge_maps(
    mut left: HashMap<String, Expr>,
    right: HashMap<String, Expr>,
) -> Option<HashMap<String, Expr>> {
    for (k, v) in right {
        if left.contains_key(&k) {
            return None;
        }
        left.insert(k, v);
    }
    Some(left)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hulk_ast::{
        BinaryOp, Expr, Literal, MacroPattern, MacroPatternBind, SourceSpan, UnaryOp,
    };

    fn s() -> SourceSpan {
        SourceSpan::new(1, 1)
    }

    fn num(n: f64) -> Expr {
        Expr::number(n, s())
    }

    fn bool_lit(b: bool) -> Expr {
        Expr::boolean(b, s())
    }

    fn string_lit(st: &str) -> Expr {
        Expr::string(st, s())
    }

    fn _var(name: &str) -> Expr {
        Expr::variable(name, s())
    }

    fn bin(op: BinaryOp, left: Expr, right: Expr) -> Expr {
        Expr::binary(op, left, right, s())
    }

    fn un(op: UnaryOp, expr: Expr) -> Expr {
        Expr::unary(op, expr, s())
    }

    // Helper to create a Bind pattern with a name and inner pattern.
    fn bind(name: &str, pattern: MacroPattern) -> MacroPattern {
        MacroPattern::Bind {
            name: Some(name.to_string()),
            ty: None,
            pattern: Box::new(pattern),
        }
    }

    // Helper to create a Bind pattern with type annotation.
    fn bind_ty(name: &str, ty: &str, pattern: MacroPattern) -> MacroPattern {
        MacroPattern::Bind {
            name: Some(name.to_string()),
            ty: Some(hulk_ast::TypeRef::named(ty)),
            pattern: Box::new(pattern),
        }
    }

    #[test]
    fn wildcard_matches_anything() {
        let pattern = MacroPattern::Wildcard;
        let expr = num(42.0);
        let result = try_match(&pattern, &expr);
        assert_eq!(result, Some(HashMap::new()));
    }

    #[test]
    fn literal_number_matches_exact() {
        let pattern = MacroPattern::Literal(Literal::Number(42.0));
        assert!(try_match(&pattern, &num(42.0)).is_some());
        assert!(try_match(&pattern, &num(43.0)).is_none());
        assert!(try_match(&pattern, &bool_lit(true)).is_none());
    }

    #[test]
    fn literal_string_matches_exact() {
        let pattern = MacroPattern::Literal(Literal::String("hello".to_string()));
        assert!(try_match(&pattern, &string_lit("hello")).is_some());
        assert!(try_match(&pattern, &string_lit("world")).is_none());
    }

    #[test]
    fn literal_bool_matches_exact() {
        let pattern = MacroPattern::Literal(Literal::Boolean(true));
        assert!(try_match(&pattern, &bool_lit(true)).is_some());
        assert!(try_match(&pattern, &bool_lit(false)).is_none());
    }

    #[test]
    fn bind_captures_expression() {
        let pattern = bind("x", MacroPattern::Wildcard);
        let expr = num(42.0);
        let result = try_match(&pattern, &expr);
        let mut expected = HashMap::new();
        expected.insert("x".to_string(), expr.clone());
        assert_eq!(result, Some(expected));
    }

    #[test]
    fn bind_with_type_ignores_type_during_matching() {
        let pattern = bind_ty("x", "Number", MacroPattern::Wildcard);
        let expr = string_lit("hello");
        let result = try_match(&pattern, &expr);
        let mut expected = HashMap::new();
        expected.insert("x".to_string(), expr.clone());
        assert_eq!(result, Some(expected));
    }

    #[test]
    fn bind_on_compound_pattern_captures_whole() {
        let pattern = MacroPattern::Bind {
            name: Some("whole".to_string()),
            ty: None,
            pattern: Box::new(MacroPattern::BinaryExpr {
                op: BinaryOp::Add,
                left: Box::new(MacroPatternBind {
                    name: Some("left".to_string()),
                    ty: None,
                    pattern: MacroPattern::Wildcard,
                }),
                right: Box::new(MacroPatternBind {
                    name: Some("right".to_string()),
                    ty: None,
                    pattern: MacroPattern::Wildcard,
                }),
            }),
        };
        let expr = bin(BinaryOp::Add, num(1.0), num(2.0));
        let result = try_match(&pattern, &expr);
        let mut expected = HashMap::new();
        expected.insert("whole".to_string(), expr.clone());
        expected.insert("left".to_string(), num(1.0));
        expected.insert("right".to_string(), num(2.0));
        assert_eq!(result, Some(expected));
    }

    #[test]
    fn binary_expr_matches_operator_and_binds_operands() {
        let pattern = MacroPattern::BinaryExpr {
            op: BinaryOp::Add,
            left: Box::new(MacroPatternBind {
                name: Some("a".to_string()),
                ty: None,
                pattern: MacroPattern::Wildcard,
            }),
            right: Box::new(MacroPatternBind {
                name: Some("b".to_string()),
                ty: None,
                pattern: MacroPattern::Wildcard,
            }),
        };
        let expr = bin(BinaryOp::Add, num(1.0), num(2.0));
        let result = try_match(&pattern, &expr);
        let mut expected = HashMap::new();
        expected.insert("a".to_string(), num(1.0));
        expected.insert("b".to_string(), num(2.0));
        assert_eq!(result, Some(expected));

        // Wrong operator
        let expr2 = bin(BinaryOp::Subtract, num(1.0), num(2.0));
        assert!(try_match(&pattern, &expr2).is_none());
    }

    #[test]
    fn binary_expr_with_nested_patterns() {
        let pattern = MacroPattern::BinaryExpr {
            op: BinaryOp::Multiply,
            left: Box::new(MacroPatternBind {
                name: None,
                ty: None,
                pattern: MacroPattern::BinaryExpr {
                    op: BinaryOp::Add,
                    left: Box::new(MacroPatternBind {
                        name: Some("x".to_string()),
                        ty: None,
                        pattern: MacroPattern::Wildcard,
                    }),
                    right: Box::new(MacroPatternBind {
                        name: Some("y".to_string()),
                        ty: None,
                        pattern: MacroPattern::Wildcard,
                    }),
                },
            }),
            right: Box::new(MacroPatternBind {
                name: Some("z".to_string()),
                ty: None,
                pattern: MacroPattern::Wildcard,
            }),
        };
        let expr = bin(
            BinaryOp::Multiply,
            bin(BinaryOp::Add, num(1.0), num(2.0)),
            num(3.0),
        );
        let result = try_match(&pattern, &expr);
        let mut expected = HashMap::new();
        expected.insert("x".to_string(), num(1.0));
        expected.insert("y".to_string(), num(2.0));
        expected.insert("z".to_string(), num(3.0));
        assert_eq!(result, Some(expected));
    }

    #[test]
    fn unary_expr_matches_operator_and_binds_operand() {
        let pattern = MacroPattern::UnaryExpr {
            op: UnaryOp::Negate,
            operand: Box::new(MacroPatternBind {
                name: Some("x".to_string()),
                ty: None,
                pattern: MacroPattern::Wildcard,
            }),
        };
        let expr = un(UnaryOp::Negate, num(42.0));
        let result = try_match(&pattern, &expr);
        let mut expected = HashMap::new();
        expected.insert("x".to_string(), num(42.0));
        assert_eq!(result, Some(expected));

        // Wrong operator
        let expr2 = un(UnaryOp::Not, bool_lit(true));
        assert!(try_match(&pattern, &expr2).is_none());
    }

    #[test]
    fn duplicate_bindings_are_rejected() {
        // Pattern: (x + x) – binding `x` twice
        let pattern = MacroPattern::BinaryExpr {
            op: BinaryOp::Add,
            left: Box::new(MacroPatternBind {
                name: Some("x".to_string()),
                ty: None,
                pattern: MacroPattern::Wildcard,
            }),
            right: Box::new(MacroPatternBind {
                name: Some("x".to_string()),
                ty: None,
                pattern: MacroPattern::Wildcard,
            }),
        };
        let expr = bin(BinaryOp::Add, num(1.0), num(2.0));
        assert!(try_match(&pattern, &expr).is_none());
    }

    #[test]
    fn duplicate_bindings_across_nested_patterns() {
        // Pattern: (x + (x * 2)) – x appears twice
        let pattern = MacroPattern::BinaryExpr {
            op: BinaryOp::Add,
            left: Box::new(MacroPatternBind {
                name: Some("x".to_string()),
                ty: None,
                pattern: MacroPattern::Wildcard,
            }),
            right: Box::new(MacroPatternBind {
                name: None,
                ty: None,
                pattern: MacroPattern::BinaryExpr {
                    op: BinaryOp::Multiply,
                    left: Box::new(MacroPatternBind {
                        name: Some("x".to_string()),
                        ty: None,
                        pattern: MacroPattern::Wildcard,
                    }),
                    right: Box::new(MacroPatternBind {
                        name: Some("y".to_string()),
                        ty: None,
                        pattern: MacroPattern::Wildcard,
                    }),
                },
            }),
        };
        let expr = bin(
            BinaryOp::Add,
            num(1.0),
            bin(BinaryOp::Multiply, num(2.0), num(3.0)),
        );
        assert!(try_match(&pattern, &expr).is_none());
    }

    #[test]
    fn wildcard_does_not_bind() {
        let pattern = MacroPattern::BinaryExpr {
            op: BinaryOp::Add,
            left: Box::new(MacroPatternBind {
                name: None,
                ty: None,
                pattern: MacroPattern::Wildcard,
            }),
            right: Box::new(MacroPatternBind {
                name: None,
                ty: None,
                pattern: MacroPattern::Wildcard,
            }),
        };
        let expr = bin(BinaryOp::Add, num(1.0), num(2.0));
        let result = try_match(&pattern, &expr);
        assert_eq!(result, Some(HashMap::new()));
    }
}