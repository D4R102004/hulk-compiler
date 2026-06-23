//! Type registry – the global, context‑independent knowledge base.
//!
//! This module builds and maintains the registry of all types, protocols,
//! and functions known to the HULK program (both builtins and user‑defined).
//! It is constructed once during semantic analysis and remains read‑only
//! after Pass 1.

use std::collections::HashMap;

use hulk_ast::{Expr, SourceSpan};

use super::Type;

// -----------------------------------------------------------------------------
// Core registry structures
// -----------------------------------------------------------------------------

/// The global registry of types, protocols, and functions.
///
/// Three separate maps because HULK never resolves a function name and a
/// type name through the same syntax (`f(...)` vs. `new T(...)`), so there
/// is no ambiguity to arbitrate between them.
#[derive(Debug, Clone)]
pub struct TypeRegistry {
    /// All user‑defined and builtin types, keyed by their name.
    /// Includes classes (`type`) and builtin types (`Number`, `Object`, etc.).
    pub types: HashMap<String, TypeInfo>,

    /// All user‑defined and builtin protocols, keyed by protocol name.
    pub protocols: HashMap<String, ProtocolInfo>,

    /// All user‑defined and builtin global functions, keyed by function name.
    /// Builtins such as `print`, `sqrt`, `sin`, `cos`, `exp`, `log`, `rand`,
    /// `range`, `PI`, `E` are seeded here.
    pub functions: HashMap<String, FunctionSignature>,
}

/// Information about a user‑defined `type` declaration.
#[derive(Debug, Clone)]
pub struct TypeInfo {
    pub name: String,
    /// Constructor parameters (type arguments) of the type.
    pub params: Vec<(String, Type)>,
    /// Parent type and its constructor arguments, as written in source.
    pub parent: Option<ParentLink>,
    /// Own attributes (not yet including inherited ones).
    pub attributes: HashMap<String, AttributeInfo>,
    /// Own methods (not yet including inherited ones).
    pub methods: HashMap<String, MethodSignature>,
    /// Flattened method table (own + inherited), built during Pass 1.
    pub flattened_methods: HashMap<String, MethodSignature>,
    /// Flag to mark builtin value types (`Number`, `String`, `Boolean`)
    /// so that inheritance from them can be rejected.
    pub is_builtin_value: bool,
    pub span: SourceSpan,
}

/// The `inherits Base(args)` clause, as written in source.
#[derive(Debug, Clone)]
pub struct ParentLink {
    pub name: String,
    pub args: Vec<Expr>, // untyped Expr<()>, collected before inference
}

/// Information about an attribute (field) of a type.
#[derive(Debug, Clone)]
pub struct AttributeInfo {
    pub declared_type: Option<Type>, // None until inferred in Pass 2
    pub span: SourceSpan,
}

/// Signature of a method (or protocol method).
#[derive(Debug, Clone)]
pub struct MethodSignature {
    pub params: Vec<(String, Type)>,
    pub return_type: Type,
    pub defined_in: String, // owning type name, for `base` resolution
    pub span: SourceSpan,
}

/// Information about a `protocol` declaration.
#[derive(Debug, Clone)]
pub struct ProtocolInfo {
    pub name: String,
    pub extends: Vec<String>, // names of protocols this one extends
    pub methods: HashMap<String, MethodSignature>,
    pub flattened_methods: HashMap<String, MethodSignature>,
    pub span: SourceSpan,
}

/// Signature of a global function (builtin or user‑defined).
#[derive(Debug, Clone)]
pub struct FunctionSignature {
    pub params: Vec<(String, Type)>,
    pub return_type: Type,
    pub span: SourceSpan,
}

// -----------------------------------------------------------------------------
// Builtin seeding
// -----------------------------------------------------------------------------

/// Returns a `TypeRegistry` pre‑populated with all HULK builtins.
///
/// This is called once at the start of `analyze` so that every builtin
/// name is already present before the user's declarations are collected.
pub fn seeded_registry() -> TypeRegistry {
    let mut registry = TypeRegistry {
        types: HashMap::new(),
        protocols: HashMap::new(),
        functions: HashMap::new(),
    };

    // ─── Builtin types ─────────────────────────────────────────────────────

    // Object: root of the hierarchy
    let object_type = TypeInfo {
        name: "Object".to_string(),
        params: Vec::new(),
        parent: None,
        attributes: HashMap::new(),
        methods: HashMap::new(),
        flattened_methods: HashMap::new(),
        is_builtin_value: false,
        span: SourceSpan::new(0, 0),
    };
    registry.types.insert("Object".to_string(), object_type);

    // Number, String, Boolean – implicitly inherit Object, but are value types
    for name in ["Number", "String", "Boolean"] {
        let info = TypeInfo {
            name: name.to_string(),
            params: Vec::new(),
            parent: Some(ParentLink {
                name: "Object".to_string(),
                args: Vec::new(),
            }),
            attributes: HashMap::new(),
            methods: HashMap::new(),
            flattened_methods: HashMap::new(),
            is_builtin_value: true, // prevents inheriting from them
            span: SourceSpan::new(0, 0),
        };
        registry.types.insert(name.to_string(), info);
    }

    // ─── Builtin protocols ────────────────────────────────────────────────

    // Iterable protocol
    let iterable_methods = HashMap::from([
        (
            "next".to_string(),
            MethodSignature {
                params: Vec::new(),
                return_type: Type::Boolean,
                defined_in: "Iterable".to_string(),
                span: SourceSpan::new(0, 0),
            },
        ),
        (
            "current".to_string(),
            MethodSignature {
                params: Vec::new(),
                return_type: Type::Object,
                defined_in: "Iterable".to_string(),
                span: SourceSpan::new(0, 0),
            },
        ),
    ]);
    let iterable_protocol = ProtocolInfo {
        name: "Iterable".to_string(),
        extends: Vec::new(),
        methods: iterable_methods,
        flattened_methods: HashMap::new(),
        span: SourceSpan::new(0, 0),
    };
    registry.protocols.insert("Iterable".to_string(), iterable_protocol);

    // Enumerable protocol
    let enumerable_methods = HashMap::from([(
        "iter".to_string(),
        MethodSignature {
            params: Vec::new(),
            return_type: Type::Iterable(Box::new(Type::Object)),
            defined_in: "Enumerable".to_string(),
            span: SourceSpan::new(0, 0),
        },
    )]);
    let enumerable_protocol = ProtocolInfo {
        name: "Enumerable".to_string(),
        extends: Vec::new(),
        methods: enumerable_methods,
        flattened_methods: HashMap::new(),
        span: SourceSpan::new(0, 0),
    };
    registry.protocols.insert("Enumerable".to_string(), enumerable_protocol);

    // ─── Builtin type: Range ──────────────────────────────────────────────

    // Range(min: Number, max: Number) implements Iterable with current(): Number
    let range_type = TypeInfo {
        name: "Range".to_string(),
        params: vec![
            ("min".to_string(), Type::Number),
            ("max".to_string(), Type::Number),
        ],
        parent: Some(ParentLink {
            name: "Object".to_string(),
            args: Vec::new(),
        }),
        attributes: HashMap::new(),
        methods: HashMap::from([
            (
                "current".to_string(),
                MethodSignature {
                    params: Vec::new(),
                    return_type: Type::Number,
                    defined_in: "Range".to_string(),
                    span: SourceSpan::new(0, 0),
                },
            ),
            (
                "next".to_string(),
                MethodSignature {
                    params: Vec::new(),
                    return_type: Type::Boolean,
                    defined_in: "Range".to_string(),
                    span: SourceSpan::new(0, 0),
                },
            ),
        ]),
        flattened_methods: HashMap::new(),
        is_builtin_value: false,
        span: SourceSpan::new(0, 0),
    };
    registry.types.insert("Range".to_string(), range_type);

    // ─── Builtin functions ────────────────────────────────────────────────

    // print: (x: Object) -> Object
    registry.functions.insert(
        "print".to_string(),
        FunctionSignature {
            params: vec![("x".to_string(), Type::Object)],
            return_type: Type::Object,
            span: SourceSpan::new(0, 0),
        },
    );

    // sqrt, sin, cos, exp: (x: Number) -> Number
    for name in ["sqrt", "sin", "cos", "exp"] {
        registry.functions.insert(
            name.to_string(),
            FunctionSignature {
                params: vec![("x".to_string(), Type::Number)],
                return_type: Type::Number,
                span: SourceSpan::new(0, 0),
            },
        );
    }

    // log: (base: Number, x: Number) -> Number
    registry.functions.insert(
        "log".to_string(),
        FunctionSignature {
            params: vec![
                ("base".to_string(), Type::Number),
                ("x".to_string(), Type::Number),
            ],
            return_type: Type::Number,
            span: SourceSpan::new(0, 0),
        },
    );

    // rand: () -> Number
    registry.functions.insert(
        "rand".to_string(),
        FunctionSignature {
            params: Vec::new(),
            return_type: Type::Number,
            span: SourceSpan::new(0, 0),
        },
    );

    // range: (min: Number, max: Number) -> Range
    registry.functions.insert(
        "range".to_string(),
        FunctionSignature {
            params: vec![
                ("min".to_string(), Type::Number),
                ("max".to_string(), Type::Number),
            ],
            return_type: Type::Named("Range".to_string()),
            span: SourceSpan::new(0, 0),
        },
    );

    // Constants PI and E: modeled as zero-argument functions returning Number
    for name in ["PI", "E"] {
        registry.functions.insert(
            name.to_string(),
            FunctionSignature {
                params: Vec::new(),
                return_type: Type::Number,
                span: SourceSpan::new(0, 0),
            },
        );
    }

    registry
}

// -----------------------------------------------------------------------------
// Query helpers
// -----------------------------------------------------------------------------

impl TypeRegistry {
    /// Look up a type by name.
    pub fn lookup_type(&self, name: &str) -> Option<&TypeInfo> {
        self.types.get(name)
    }

    /// Mutable lookup for a type.
    pub fn lookup_type_mut(&mut self, name: &str) -> Option<&mut TypeInfo> {
        self.types.get_mut(name)
    }

    /// Look up a protocol by name.
    pub fn lookup_protocol(&self, name: &str) -> Option<&ProtocolInfo> {
        self.protocols.get(name)
    }

    /// Look up a function by name.
    pub fn lookup_function(&self, name: &str) -> Option<&FunctionSignature> {
        self.functions.get(name)
    }

    /// Returns `true` if the given `Type` is a protocol name.
    pub fn is_protocol(&self, ty: &Type) -> bool {
        match ty {
            Type::Named(name) => self.protocols.contains_key(name),
            _ => false,
        }
    }

    /// Returns `true` if `ancestor` is a nominal ancestor of `descendant`.
    ///
    /// Walks the parent chain of `descendant` and checks if `ancestor`
    /// appears at any point. Handles builtin value types, which have
    /// `Object` as their parent.
    pub fn is_ancestor(&self, ancestor: &str, descendant: &str) -> bool {
        let mut current = descendant;
        while let Some(info) = self.lookup_type(current) {
            if current == ancestor {
                return true;
            }
            if let Some(parent) = &info.parent {
                current = &parent.name;
            } else {
                break;
            }
        }
        // If ancestor is Object and we reached the root, we already
        // would have returned true when current == ancestor.
        false
    }

    /// Returns `true` if the type named `type_name` structurally
    /// implements the protocol named `protocol_name`.
    ///
    /// This implements the structural conformance rules of §A.10.4.
    /// For each method in the protocol's flattened method set, the type
    /// must have a method with:
    ///   - the same name
    ///   - the same number of parameters
    ///   - contravariant parameter types (protocol param <= type param)
    ///   - covariant return type (type return <= protocol return)
    pub fn implements_protocol(&self, type_name: &str, protocol_name: &str) -> bool {
        let type_info = match self.lookup_type(type_name) {
            Some(info) => info,
            None => return false,
        };
        let protocol_info = match self.lookup_protocol(protocol_name) {
            Some(info) => info,
            None => return false,
        };

        // Use the flattened method table of the type (includes inherited)
        // If not yet flattened (should have been done in Pass 1), fall back
        // to own methods.
        let type_methods = if !type_info.flattened_methods.is_empty() {
            &type_info.flattened_methods
        } else {
            &type_info.methods
        };

        for (method_name, proto_sig) in &protocol_info.methods {
            let type_sig = match type_methods.get(method_name) {
                Some(sig) => sig,
                None => return false, // missing method
            };

            // Same arity
            if type_sig.params.len() != proto_sig.params.len() {
                return false;
            }

            // Check contravariance of parameters:
            // For each parameter, protocol param type P must conform to type param type T
            // i.e. P <= T
            for ((_, p_type), (_, t_type)) in proto_sig.params.iter().zip(&type_sig.params) {
                if !p_type.conforms_to(t_type, self) {
                    return false;
                }
            }

            // Check covariance of return type:
            // type return type R must conform to protocol return type P
            // i.e. R <= P
            if !type_sig.return_type.conforms_to(&proto_sig.return_type, self) {
                return false;
            }
        }

        // All methods matched and variance rules satisfied.
        true
    }

    /// Returns the parent name of a type, if any.
    pub fn parent_of(&self, type_name: &str) -> Option<String> {
        self.lookup_type(type_name)
            .and_then(|info| info.parent.as_ref())
            .map(|parent| parent.name.clone())
    }
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Type;

    // Helper: build a minimal registry with a protocol P (method f(): Number)
    // and a type T with optional custom methods.
    fn build_registry_with_protocol_and_type(
        type_methods: HashMap<String, MethodSignature>,
    ) -> TypeRegistry {
        let mut registry = TypeRegistry {
            types: HashMap::new(),
            protocols: HashMap::new(),
            functions: HashMap::new(),
        };

        // Protocol P: f(): Number
        let p_methods = HashMap::from([(
            "f".to_string(),
            MethodSignature {
                params: Vec::new(),
                return_type: Type::Number,
                defined_in: "P".to_string(),
                span: SourceSpan::new(0, 0),
            },
        )]);
        registry.protocols.insert(
            "P".to_string(),
            ProtocolInfo {
                name: "P".to_string(),
                extends: Vec::new(),
                methods: p_methods.clone(),
                flattened_methods: p_methods,
                span: SourceSpan::new(0, 0),
            },
        );

        // Type T with given methods.
        registry.types.insert(
            "T".to_string(),
            TypeInfo {
                name: "T".to_string(),
                params: Vec::new(),
                parent: None,
                attributes: HashMap::new(),
                methods: type_methods.clone(),
                flattened_methods: type_methods,
                is_builtin_value: false,
                span: SourceSpan::new(0, 0),
            },
        );

        registry
    }

    #[test]
    fn implements_protocol_missing_method() {
        // Type T has no method `f`.
        let registry = build_registry_with_protocol_and_type(HashMap::new());
        assert!(!registry.implements_protocol("T", "P"));
    }

    #[test]
    fn implements_protocol_wrong_arity() {
        // Type T has method `f` but with one parameter, while protocol expects zero.
        let mut methods = HashMap::new();
        methods.insert(
            "f".to_string(),
            MethodSignature {
                params: vec![("x".to_string(), Type::Number)],
                return_type: Type::Number,
                defined_in: "T".to_string(),
                span: SourceSpan::new(0, 0),
            },
        );
        let registry = build_registry_with_protocol_and_type(methods);
        assert!(!registry.implements_protocol("T", "P"));
    }

    #[test]
    fn implements_protocol_contravariant_violation() {
        // Protocol P: f() expects no parameters.
        // Type T: f() with one parameter of type Number -> protocol param (none) <= type param (Number) is false?
        // Actually, for contravariance, protocol param type P must conform to type param type T.
        // Since there are no protocol params, there's nothing to check. To get a violation, we need a protocol
        // with a parameter and a type with a parameter of a type that does NOT conform.
        // Let's redefine: protocol P has f(x: Number), type T has f(x: String) -> String does not conform to Number.
        let mut registry = TypeRegistry {
            types: HashMap::new(),
            protocols: HashMap::new(),
            functions: HashMap::new(),
        };

        let p_methods = HashMap::from([(
            "f".to_string(),
            MethodSignature {
                params: vec![("x".to_string(), Type::Number)],
                return_type: Type::Number,
                defined_in: "P".to_string(),
                span: SourceSpan::new(0, 0),
            },
        )]);
        registry.protocols.insert(
            "P".to_string(),
            ProtocolInfo {
                name: "P".to_string(),
                extends: Vec::new(),
                methods: p_methods.clone(),
                flattened_methods: p_methods,
                span: SourceSpan::new(0, 0),
            },
        );

        let mut type_methods = HashMap::new();
        type_methods.insert(
            "f".to_string(),
            MethodSignature {
                params: vec![("x".to_string(), Type::String)],
                return_type: Type::Number,
                defined_in: "T".to_string(),
                span: SourceSpan::new(0, 0),
            },
        );
        registry.types.insert(
            "T".to_string(),
            TypeInfo {
                name: "T".to_string(),
                params: Vec::new(),
                parent: None,
                attributes: HashMap::new(),
                methods: type_methods.clone(),
                flattened_methods: type_methods,
                is_builtin_value: false,
                span: SourceSpan::new(0, 0),
            },
        );

        // Protocol param (Number) must conform to type param (String) -> false.
        assert!(!registry.implements_protocol("T", "P"));
    }

    #[test]
    fn implements_protocol_covariant_violation() {
        // Protocol P: f(): Number
        // Type T: f(): String -> String does not conform to Number.
        let mut methods = HashMap::new();
        methods.insert(
            "f".to_string(),
            MethodSignature {
                params: Vec::new(),
                return_type: Type::String,
                defined_in: "T".to_string(),
                span: SourceSpan::new(0, 0),
            },
        );
        let registry = build_registry_with_protocol_and_type(methods);
        assert!(!registry.implements_protocol("T", "P"));
    }

    #[test]
    fn is_ancestor_self_is_true() {
        let mut registry = seeded_registry();
        // Insert a user type T.
        registry.types.insert(
            "T".to_string(),
            TypeInfo {
                name: "T".to_string(),
                params: Vec::new(),
                parent: None,
                attributes: HashMap::new(),
                methods: HashMap::new(),
                flattened_methods: HashMap::new(),
                is_builtin_value: false,
                span: SourceSpan::new(0, 0),
            },
        );
        assert!(registry.is_ancestor("T", "T"));
        // Also for builtins:
        assert!(registry.is_ancestor("Number", "Number"));
        assert!(registry.is_ancestor("Object", "Object"));
    }
}