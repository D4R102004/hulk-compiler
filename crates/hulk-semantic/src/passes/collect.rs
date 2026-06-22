//! Pass 0: Declaration Collection
//!
//! This pass walks the AST and populates the `TypeRegistry` with every
//! global function, type, and protocol signature. It does not inspect
//! any function body, method body, or attribute initializer.
//!
//! The goal is to make every name visible before any body is checked,
//! solving the forward-reference problem (§A.3.1) structurally.

use std::collections::{HashMap, HashSet};

use hulk_ast::{
    AttributeDecl, Declaration, DeclarationKind, FunctionDecl, Param, ProtocolDecl,
    ProtocolMethod, TypeDecl, TypeMemberKind, TypeParent, TypeRef,
};
use hulk_ast::SourceSpan;

use crate::error::{SemanticError, SemanticErrorKind};
use crate::types::registry::{
    AttributeInfo, FunctionSignature, MethodSignature, ParentLink, ProtocolInfo, TypeInfo,
    TypeRegistry,
};
use crate::types::Type;

// -----------------------------------------------------------------------------
// Public entry point
// -----------------------------------------------------------------------------

/// Runs Pass 0: collects all global declarations into the registry.
///
/// # Arguments
/// * `program` – The untyped AST (`Program<()>`).
/// * `registry` – The registry to populate (must already contain builtins).
/// * `errors` – Vector to append any shape errors (duplicates, missing types).
///
/// # Note on Spans
/// Many AST nodes (e.g., `Param`, `ProtocolMethod`) do not have their own `span`
/// field. To still provide accurate error locations, this pass uses the `span`
/// of the enclosing `Declaration` as a fallback for any error that originates
/// inside that declaration. This is sufficient for the "shape errors" collected
/// here (duplicates, missing annotations), because the declaration span points
/// to the general area of the mistake. Future improvements could add finer-
/// grained spans to the AST, but this solution is practical and works within
/// the current AST design.
pub fn run(
    program: &hulk_ast::Program,
    registry: &mut TypeRegistry,
    errors: &mut Vec<SemanticError>,
) {
    for decl in &program.declarations {
        match &decl.kind {
            DeclarationKind::Function(f) => collect_function(f, decl.span, registry, errors),
            DeclarationKind::Type(t) => collect_type(t, decl.span, registry, errors),
            DeclarationKind::Protocol(p) => collect_protocol(p, decl.span, registry, errors),
        }
    }
}

// -----------------------------------------------------------------------------
// Function collection
// -----------------------------------------------------------------------------

/// Collects a global function declaration.
fn collect_function(
    func: &FunctionDecl,
    decl_span: SourceSpan,
    registry: &mut TypeRegistry,
    errors: &mut Vec<SemanticError>,
) {
    // Duplicate check: global function namespace.
    if registry.functions.contains_key(&func.name) {
        errors.push(SemanticError::error(
            SemanticErrorKind::DuplicateFunction(func.name.clone()),
            decl_span,
        ));
        return;
    }

    // Build parameter types. Unannotated params become `Type::Unknown`.
    let params: Vec<(String, Type)> = func
        .params
        .iter()
        .map(|p| {
            let ty = p
                .type_annotation
                .as_ref()
                .map(|tr| resolve_type_ref(tr))
                .unwrap_or(Type::Unknown);
            (p.name.clone(), ty)
        })
        .collect();

    // Check for duplicate parameter names within this function.
    check_duplicate_params(&params, errors, decl_span);

    let return_type = func
        .return_type
        .as_ref()
        .map(|tr| resolve_type_ref(tr))
        .unwrap_or(Type::Unknown);

    let sig = FunctionSignature {
        params,
        return_type,
        span: decl_span,
    };

    registry.functions.insert(func.name.clone(), sig);
}

// -----------------------------------------------------------------------------
// Type collection
// -----------------------------------------------------------------------------

/// Collects a type declaration (including its members).
fn collect_type(
    ty_decl: &TypeDecl,
    decl_span: SourceSpan,
    registry: &mut TypeRegistry,
    errors: &mut Vec<SemanticError>,
) {
    // Duplicate check: type namespace (shared with protocols, per §A.10.2).
    if registry.types.contains_key(&ty_decl.name) {
        errors.push(SemanticError::error(
            SemanticErrorKind::DuplicateType(ty_decl.name.clone()),
            decl_span,
        ));
        return;
    }

    // Constructor parameters.
    let params: Vec<(String, Type)> = ty_decl
        .params
        .iter()
        .map(|p| {
            let ty = p
                .type_annotation
                .as_ref()
                .map(|tr| resolve_type_ref(tr))
                .unwrap_or(Type::Unknown);
            (p.name.clone(), ty)
        })
        .collect();
    check_duplicate_params(&params, errors, decl_span);

    // Parent link (unresolved at this point; will be resolved in Pass 1).
    let parent = ty_decl.parent.as_ref().map(|p| ParentLink {
        name: p.name.clone(),
        args: p.args.clone(), // untyped Expr<()>
    });

    // Collect own members: attributes and methods.
    let mut attributes = HashMap::new();
    let mut methods = HashMap::new();

    for member in &ty_decl.members {
        match &member.kind {
            TypeMemberKind::Attribute(attr) => {
                // Duplicate attribute check.
                if attributes.contains_key(&attr.name) {
                    errors.push(SemanticError::error(
                        SemanticErrorKind::DuplicateAttribute {
                            ty: ty_decl.name.clone(),
                            attribute: attr.name.clone(),
                        },
                        member.span,
                    ));
                    continue; // skip duplicate to avoid further errors
                }
                let declared_type = attr.type_annotation.as_ref().map(|tr| resolve_type_ref(tr));
                let info = AttributeInfo {
                    declared_type,
                    span: member.span,
                };
                attributes.insert(attr.name.clone(), info);
            }
            TypeMemberKind::Method(method) => {
                // Duplicate method check.
                if methods.contains_key(&method.name) {
                    errors.push(SemanticError::error(
                        SemanticErrorKind::DuplicateMethod {
                            ty: ty_decl.name.clone(),
                            method: method.name.clone(),
                        },
                        member.span,
                    ));
                    continue;
                }
                // Build method signature.
                let params: Vec<(String, Type)> = method
                    .params
                    .iter()
                    .map(|p| {
                        let ty = p
                            .type_annotation
                            .as_ref()
                            .map(|tr| resolve_type_ref(tr))
                            .unwrap_or(Type::Unknown);
                        (p.name.clone(), ty)
                    })
                    .collect();
                check_duplicate_params(&params, errors, member.span);

                let return_type = method
                    .return_type
                    .as_ref()
                    .map(|tr| resolve_type_ref(tr))
                    .unwrap_or(Type::Unknown);

                let sig = MethodSignature {
                    params,
                    return_type,
                    defined_in: ty_decl.name.clone(),
                    span: member.span,
                };
                methods.insert(method.name.clone(), sig);
            }
        }
    }

    // Build the final TypeInfo. The `flattened_methods` will be filled in Pass 1.
    let info = TypeInfo {
        name: ty_decl.name.clone(),
        params,
        parent,
        attributes,
        methods,
        flattened_methods: HashMap::new(), // filled in Pass 1
        is_builtin_value: false,           // user types are never builtin value types
        span: decl_span,
    };

    registry.types.insert(ty_decl.name.clone(), info);
}

// -----------------------------------------------------------------------------
// Protocol collection
// -----------------------------------------------------------------------------

/// Collects a protocol declaration.
///
/// All protocol methods must be fully typed (no `Type::Unknown` allowed),
/// because protocols have no body to infer from. This is checked immediately.
fn collect_protocol(
    protocol: &ProtocolDecl,
    decl_span: SourceSpan,
    registry: &mut TypeRegistry,
    errors: &mut Vec<SemanticError>,
) {
    // Duplicate check: protocols share the type namespace with classes.
    if registry.protocols.contains_key(&protocol.name) {
        errors.push(SemanticError::error(
            SemanticErrorKind::DuplicateType(protocol.name.clone()),
            decl_span,
        ));
        return;
    }

    let mut method_sigs = HashMap::new();

    for method in &protocol.methods {
        // Every protocol method must have fully annotated parameters.
        for param in &method.params {
            if param.type_annotation.is_none() {
                errors.push(SemanticError::error(
                    SemanticErrorKind::MissingTypeAnnotation {
                        symbol: param.name.clone(),
                        context: format!("protocol method `{}`", method.name),
                    },
                    decl_span,
                ));
            }
        }

        // Build parameter types. If any parameter is missing an annotation,
        // we treat it as `Type::Unknown` here, but we already reported an error.
        let params: Vec<(String, Type)> = method
            .params
            .iter()
            .map(|p| {
                let ty = p
                    .type_annotation
                    .as_ref()
                    .map(|tr| resolve_type_ref(tr))
                    .unwrap_or(Type::Unknown);
                (p.name.clone(), ty)
            })
            .collect();

        // Return type must be present (it is mandatory in the AST, but we still convert).
        let return_type = resolve_type_ref(&method.return_type);

        let sig = MethodSignature {
            params,
            return_type,
            defined_in: protocol.name.clone(),
            span: decl_span,
        };

        // Check duplicate method names inside the protocol.
        if method_sigs.contains_key(&method.name) {
            errors.push(SemanticError::error(
                SemanticErrorKind::DuplicateMethod {
                    ty: protocol.name.clone(),
                    method: method.name.clone(),
                },
                decl_span,
            ));
        } else {
            method_sigs.insert(method.name.clone(), sig);
        }
    }

    let info = ProtocolInfo {
        name: protocol.name.clone(),
        extends: protocol.parents.iter().map(|p| p.name.clone()).collect(),
        methods: method_sigs,
        span: decl_span,
    };

    registry.protocols.insert(protocol.name.clone(), info);
}

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

/// Converts a syntactic `TypeRef` to a semantic `Type` as much as possible
/// without a registry. Builtins are mapped to their variants, user-defined
/// types become `Type::Named`, and generic arguments are recursively resolved.
///
/// This is only used during collection to store the type *as written* in the
/// registry. Actual resolution of user-defined types (existence, inheritance)
/// happens in Pass 1.
fn resolve_type_ref(tr: &TypeRef) -> Type {
    match tr.name.as_str() {
        "Number" => Type::Number,
        "String" => Type::String,
        "Boolean" => Type::Boolean,
        "Object" => Type::Object,
        _ => {
            if tr.args.is_empty() {
                Type::Named(tr.name.clone())
            } else {
                // Recursively resolve arguments.
                let args: Vec<Type> = tr.args.iter().map(|arg| resolve_type_ref(arg)).collect();
                // Handle built‑in parametric types: `Vector<T>` and `Iterable<T>`.
                // The parser already rewrote `T[]` → `Vector<T>` and `T*` → `Iterable<T>`.
                match tr.name.as_str() {
                    "Vector" if !args.is_empty() => Type::Vector(Box::new(args[0].clone())),
                    "Iterable" if !args.is_empty() => Type::Iterable(Box::new(args[0].clone())),
                    // For other named types with arguments (if any), we store the name and
                    // arguments in the `args` field of `TypeRef`, but our `Type` enum does
                    // not support user‑defined generics. We simply treat it as a plain
                    // `Named` type, losing the generic arguments. This is sufficient for
                    // HULK's current design (only `Vector` and `Iterable` are parametric).
                    _ => Type::Named(tr.name.clone()),
                }
            }
        }
    }
}

/// Checks for duplicate parameter names within a parameter list and pushes errors.
fn check_duplicate_params(
    params: &[(String, Type)],
    errors: &mut Vec<SemanticError>,
    fallback_span: SourceSpan,
) {
    let mut seen = HashSet::new();
    for (name, _) in params {
        if !seen.insert(name) {
            errors.push(SemanticError::error(
                SemanticErrorKind::DuplicateParameter(name.clone()),
                fallback_span,
            ));
        }
    }
}