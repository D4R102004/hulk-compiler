//! Pass 1: Hierarchy & Protocol Resolution
//!
//! This pass resolves every `inherits` and `extends` link collected in Pass 0
//! into a validated, linked tree. After this pass, `Type::conforms_to` becomes
//! well‑defined, and member lookup in later passes is a single `HashMap` access.
//!
//! Checks are performed in a specific order:
//! 1. Parent existence
//! 2. No inheriting from builtin value types (`Number`, `String`, `Boolean`)
//! 3. Cycle detection (returns the span of the type where the cycle starts)
//! 4. Override signature compatibility (class‑to‑class, no variance) — skipped if cycle
//! 5. Protocol extension variance (contravariant params, covariant return)
//! 6. Flatten attribute/method tables (parent‑to‑child) — skipped if cycle

use std::collections::{HashMap, HashSet, VecDeque};

use hulk_ast::SourceSpan;

use crate::error::{SemanticError, SemanticErrorKind};
use crate::types::registry::{MethodSignature, TypeInfo, TypeRegistry};
use crate::types::Type;

// -----------------------------------------------------------------------------
// Public entry point
// -----------------------------------------------------------------------------

/// Runs Pass 1: validates and links the type and protocol hierarchies.
///
/// # Arguments
/// * `registry` – The registry (mutated in place: parent links are resolved,
///   flattened method/attribute tables are built).
/// * `errors` – Vector to append any hierarchy‑related errors.
///
/// # Precondition
/// Pass 0 must have been run, so the registry contains all signatures.
/// The registry must already be seeded with builtins.
pub fn run(registry: &mut TypeRegistry, errors: &mut Vec<SemanticError>) {
    // Step 1: Resolve parent links and check existence.
    resolve_parent_links(registry, errors);

    // Step 2: Reject inheritance from builtin value types.
    check_builtin_inheritance(registry, errors);

    // Step 3: Detect cycles in the inheritance graph.
    let cycle = detect_cycles(registry);
    if let Some((cycle_path, span)) = cycle {
        errors.push(SemanticError::error(
            SemanticErrorKind::InheritanceCycle(cycle_path),
            span,
        ));
        // Cycle is a hard error. Skip flattening to prevent cascading errors;
        // protocol checks are independent and can continue. Override checks
        // depend on a valid hierarchy, so they are skipped when a cycle is present.
    }

    // Step 4: Check override signature compatibility (class‑to‑class).
    // Only if no cycle exists.
    if cycle.is_none() {
        check_overrides(registry, errors);
    }

    // Step 5: Check protocol extension variance (always, because independent).
    check_protocol_variance(registry, errors);

    // Step 6: Flatten attribute and method tables.
    // Only if no cycle exists.
    if cycle.is_none() {
        flatten_tables(registry, errors);
    }
}

// -----------------------------------------------------------------------------
// Step 1: Parent existence
// -----------------------------------------------------------------------------

/// Resolves parent links and checks that every parent type exists.
/// If a parent is undefined, we clear the parent link to avoid further errors.
fn resolve_parent_links(registry: &mut TypeRegistry, errors: &mut Vec<SemanticError>) {
    let type_names: Vec<String> = registry.types.keys().cloned().collect();
    for name in type_names {
        let parent_name = match registry.types.get(&name).and_then(|info| info.parent.as_ref()) {
            Some(parent) => &parent.name,
            None => continue,
        };
        if !registry.types.contains_key(parent_name) {
            errors.push(SemanticError::error(
                SemanticErrorKind::InheritFromUndefinedType(parent_name.clone()),
                registry.types[&name].span,
            ));
            // Clear the invalid parent.
            if let Some(info) = registry.types.get_mut(&name) {
                info.parent = None;
            }
        }
    }
}

// -----------------------------------------------------------------------------
// Step 2: No inheriting from builtin value types
// -----------------------------------------------------------------------------

/// Rejects inheritance from `Number`, `String`, or `Boolean` (§A.7.3).
fn check_builtin_inheritance(registry: &mut TypeRegistry, errors: &mut Vec<SemanticError>) {
    let builtin_value_types = ["Number", "String", "Boolean"];
    let type_names: Vec<String> = registry.types.keys().cloned().collect();
    for name in type_names {
        let parent_name = match registry.types.get(&name).and_then(|info| info.parent.as_ref()) {
            Some(parent) => &parent.name,
            None => continue,
        };
        if builtin_value_types.contains(&parent_name.as_str()) {
            errors.push(SemanticError::error(
                SemanticErrorKind::InheritFromBuiltinValueType(parent_name.clone()),
                registry.types[&name].span,
            ));
            // Clear the invalid parent.
            if let Some(info) = registry.types.get_mut(&name) {
                info.parent = None;
            }
        }
    }
}

// -----------------------------------------------------------------------------
// Step 3: Cycle detection (with span)
// -----------------------------------------------------------------------------

/// Detects a cycle in the inheritance graph and returns the cycle path
/// together with the span of the type where the cycle was first detected.
///
/// Returns `Some((cycle_path, span))` if a cycle is found, else `None`.
fn detect_cycles(registry: &TypeRegistry) -> Option<(Vec<String>, SourceSpan)> {
    let mut visited = HashSet::new();
    let mut recursion_stack = HashSet::new();
    let mut path = Vec::new();

    for name in registry.types.keys() {
        if !visited.contains(name) {
            if let Some((cycle, span)) = dfs_cycle(
                name,
                registry,
                &mut visited,
                &mut recursion_stack,
                &mut path,
            ) {
                return Some((cycle, span));
            }
        }
    }
    None
}

/// Depth‑first search helper for cycle detection.
/// Returns the cycle path and the span of the type that caused the cycle.
fn dfs_cycle(
    current: &str,
    registry: &TypeRegistry,
    visited: &mut HashSet<String>,
    recursion_stack: &mut HashSet<String>,
    path: &mut Vec<String>,
) -> Option<(Vec<String>, SourceSpan)> {
    if recursion_stack.contains(current) {
        // Found a cycle: construct the path from the current node to the start.
        let start_idx = path.iter().position(|n| n == current).unwrap();
        let cycle = path[start_idx..].to_vec();
        let span = registry
            .types
            .get(current)
            .map(|info| info.span)
            .unwrap_or_else(|| SourceSpan::new(0, 0));
        return Some((cycle, span));
    }
    if visited.contains(current) {
        return None;
    }

    visited.insert(current.to_string());
    recursion_stack.insert(current.to_string());
    path.push(current.to_string());

    if let Some(parent) = registry.parent_of(current) {
        if let Some(result) = dfs_cycle(&parent, registry, visited, recursion_stack, path) {
            return Some(result);
        }
    }

    path.pop();
    recursion_stack.remove(current);
    None
}

// -----------------------------------------------------------------------------
// Step 4: Override signature compatibility (class‑to‑class)
// -----------------------------------------------------------------------------

/// Checks that every overriding method in a child type has the exact same
/// signature (parameter types and return type) as the parent's method.
/// This is stricter than protocol variance (§A.7.4).
fn check_overrides(registry: &mut TypeRegistry, errors: &mut Vec<SemanticError>) {
    let type_names: Vec<String> = registry.types.keys().cloned().collect();
    for name in type_names {
        let parent_name = match registry.types.get(&name).and_then(|info| info.parent.as_ref()) {
            Some(parent) => &parent.name,
            None => continue,
        };
        // If parent is unresolved (cleared earlier), skip.
        if !registry.types.contains_key(parent_name) {
            continue;
        }
        let parent_methods = &registry.types[parent_name].methods;
        let own_methods = registry.types[&name].methods.clone(); // clone to avoid borrow issues

        for (method_name, own_sig) in own_methods {
            if let Some(parent_sig) = parent_methods.get(&method_name) {
                // Must have exactly same signature (no variance).
                let mut mismatch = false;
                let mut expected = String::new();
                let mut found = String::new();

                if own_sig.params.len() != parent_sig.params.len() {
                    mismatch = true;
                    expected = format!(
                        "({}) -> {}",
                        parent_sig
                            .params
                            .iter()
                            .map(|(_, t)| t.to_string())
                            .collect::<Vec<_>>()
                            .join(", "),
                        parent_sig.return_type
                    );
                    found = format!(
                        "({}) -> {}",
                        own_sig
                            .params
                            .iter()
                            .map(|(_, t)| t.to_string())
                            .collect::<Vec<_>>()
                            .join(", "),
                        own_sig.return_type
                    );
                } else {
                    // Check each parameter type and return type for equality.
                    for ((_, p_type), (_, o_type)) in parent_sig.params.iter().zip(&own_sig.params) {
                        if p_type != o_type {
                            mismatch = true;
                            break;
                        }
                    }
                    if !mismatch && parent_sig.return_type != own_sig.return_type {
                        mismatch = true;
                    }
                    if mismatch {
                        expected = format!(
                            "({}) -> {}",
                            parent_sig
                                .params
                                .iter()
                                .map(|(_, t)| t.to_string())
                                .collect::<Vec<_>>()
                                .join(", "),
                            parent_sig.return_type
                        );
                        found = format!(
                            "({}) -> {}",
                            own_sig
                                .params
                                .iter()
                                .map(|(_, t)| t.to_string())
                                .collect::<Vec<_>>()
                                .join(", "),
                            own_sig.return_type
                        );
                    }
                }

                if mismatch {
                    errors.push(SemanticError::error(
                        SemanticErrorKind::InvalidOverride {
                            method: method_name.clone(),
                            in_type: name.clone(),
                            expected,
                            found,
                        },
                        own_sig.span,
                    ));
                }
            }
        }
    }
}

// -----------------------------------------------------------------------------
// Step 5: Protocol extension variance
// -----------------------------------------------------------------------------

/// Checks that a protocol extension (`extends`) respects variance rules:
/// - No method may be removed.
/// - Parameter types are contravariant (parent param <= child param).
/// - Return type is covariant (child return <= parent return).
fn check_protocol_variance(registry: &mut TypeRegistry, errors: &mut Vec<SemanticError>) {
    let protocol_names: Vec<String> = registry.protocols.keys().cloned().collect();
    for name in protocol_names {
        let protocol = registry.protocols.get(&name).unwrap().clone();
        for parent_name in &protocol.extends {
            if !registry.protocols.contains_key(parent_name) {
                errors.push(SemanticError::error(
                    SemanticErrorKind::UndefinedType(parent_name.clone()),
                    protocol.span,
                ));
                continue;
            }
            let parent_methods = &registry.protocols[parent_name].methods;
            let own_methods = &registry.protocols[&name].methods;

            // A protocol extension may not remove a method.
            for (method_name, parent_sig) in parent_methods {
                let own_sig = match own_methods.get(method_name) {
                    Some(sig) => sig,
                    None => {
                        errors.push(SemanticError::error(
                            SemanticErrorKind::InvalidProtocolVariance {
                                method: method_name.clone(),
                                reason: format!("method `{}` is required by parent protocol `{}` but not defined", method_name, parent_name),
                            },
                            protocol.span,
                        ));
                        continue;
                    }
                };
                // Check contravariance of parameters (parent param <= child param)
                // and covariance of return (child return <= parent return).
                let mut reason = String::new();
                let mut ok = true;
                if own_sig.params.len() != parent_sig.params.len() {
                    reason = format!("parameter count mismatch: expected {}, found {}", parent_sig.params.len(), own_sig.params.len());
                    ok = false;
                } else {
                    for ((_, p_type), (_, o_type)) in parent_sig.params.iter().zip(&own_sig.params) {
                        if !p_type.conforms_to(o_type, registry) {
                            reason = format!("parameter type mismatch: expected {} (or supertype) for parameter of type {}, but got {}", p_type, p_type, o_type);
                            ok = false;
                            break;
                        }
                    }
                    if ok && !own_sig.return_type.conforms_to(&parent_sig.return_type, registry) {
                        reason = format!("return type mismatch: expected {} (or subtype) but got {}", parent_sig.return_type, own_sig.return_type);
                        ok = false;
                    }
                }
                if !ok {
                    errors.push(SemanticError::error(
                        SemanticErrorKind::InvalidProtocolVariance {
                            method: method_name.clone(),
                            reason,
                        },
                        own_sig.span,
                    ));
                }
            }
        }
    }
}

// -----------------------------------------------------------------------------
// Step 6: Flatten attribute and method tables
// -----------------------------------------------------------------------------

/// Flattens attribute and method tables for every type.
///
/// This copies inherited members from the parent into the child, then overwrites
/// with the child's own members. The result is that each `TypeInfo`'s `attributes`
/// and `methods` maps contain the full set of accessible members.
///
/// # Precondition
/// No cycles exist in the inheritance graph (ensured by `detect_cycles`).
fn flatten_tables(registry: &mut TypeRegistry, errors: &mut Vec<SemanticError>) {
    // Compute topological order: parents before children.
    let order = topological_order(registry);

    for name in order {
        // Skip types that are already flattened or have no parent.
        let parent_name = match registry.types.get(&name).and_then(|info| info.parent.as_ref()) {
            Some(parent) => &parent.name,
            None => continue,
        };
        // If parent is not in registry (should not happen if previous checks passed), skip.
        if !registry.types.contains_key(parent_name) {
            continue;
        }
        // Borrow check: we need to modify the child and read from the parent.
        // We'll clone the parent's maps and then overwrite with child's own.
        let parent_attrs = registry.types[parent_name].attributes.clone();
        let parent_methods = registry.types[parent_name].methods.clone();

        let child_info = registry.types.get_mut(&name).unwrap();
        // Copy parent's attributes, then overwrite with child's own.
        let mut combined_attrs = parent_attrs;
        for (k, v) in child_info.attributes.iter() {
            combined_attrs.insert(k.clone(), v.clone());
        }
        child_info.attributes = combined_attrs;

        // Same for methods.
        let mut combined_methods = parent_methods;
        for (k, v) in child_info.methods.iter() {
            combined_methods.insert(k.clone(), v.clone());
        }
        child_info.methods = combined_methods;

        // Also fill flattened_methods for compatibility with later passes.
        child_info.flattened_methods = child_info.methods.clone();
    }
}

/// Returns a topological order of types (parents before children) using Kahn's algorithm.
fn topological_order(registry: &TypeRegistry) -> Vec<String> {
    let mut in_degree: HashMap<String, usize> = HashMap::new();
    let mut graph: HashMap<String, Vec<String>> = HashMap::new();

    // Initialize.
    for name in registry.types.keys() {
        in_degree.entry(name.clone()).or_insert(0);
        graph.entry(name.clone()).or_default();
    }

    // Build edges from parent to child.
    for (name, info) in &registry.types {
        if let Some(parent) = &info.parent {
            if registry.types.contains_key(&parent.name) {
                graph.entry(parent.name.clone()).or_default().push(name.clone());
                *in_degree.entry(name.clone()).or_insert(0) += 1;
            }
        }
    }

    // Kahn's algorithm.
    let mut queue: VecDeque<String> = VecDeque::new();
    for (name, &deg) in &in_degree {
        if deg == 0 {
            queue.push_back(name.clone());
        }
    }

    let mut order = Vec::new();
    while let Some(name) = queue.pop_front() {
        order.push(name.clone());
        if let Some(children) = graph.get(&name) {
            for child in children {
                if let Some(deg) = in_degree.get_mut(child) {
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push_back(child.clone());
                    }
                }
            }
        }
    }

    // If there are cycles, the order will be incomplete, but we already checked for cycles.
    order
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    // TODO: Add tests
}