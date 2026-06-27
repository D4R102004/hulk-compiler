//! Object layout and vtable construction for HULK types.
//!
//! This module builds the runtime representation of HULK classes:
//! - The LLVM struct type for each concrete type, including the object header.
//! - Field offsets for attribute access.
//! - Vtable slot indices for virtual dispatch.
//! - Vtable globals (built after methods are declared).
//! - GC field maps (for the tracing collector).

use std::collections::HashMap;

use inkwell::types::{BasicTypeEnum, StructType};
use inkwell::values::GlobalValue;

use hulk_semantic::{topological_order, TypeInfo, TypeRegistry};

use crate::context::CodegenCtx;
use crate::error::CodegenError;
use crate::lower::utils::llvm_type;

/// Layout information for a single HULK type.
#[derive(Clone)]
pub struct TypeLayout<'ctx> {
    /// The LLVM struct type representing an instance of this type.
    pub struct_ty: StructType<'ctx>,
    /// Field offsets in bytes: attribute name -> byte offset from the start of the struct.
    pub field_offsets: HashMap<String, usize>,
    /// Method slot indices: method name -> vtable slot index (0‑based).
    pub method_slots: HashMap<String, usize>,
    /// The vtable global (built later, initially None).
    pub vtable_global: Option<GlobalValue<'ctx>>,
    /// The total size of the struct in bytes.
    pub size: usize,
}

impl<'ctx> TypeLayout<'ctx> {
    pub fn new(struct_ty: StructType<'ctx>, size: usize) -> Self {
        Self {
            struct_ty,
            field_offsets: HashMap::new(),
            method_slots: HashMap::new(),
            vtable_global: None,
            size,
        }
    }
}

/// Builds type layouts for all user‑defined types in the registry.
///
/// This function walks the inheritance hierarchy parent‑before‑child, creates
/// an LLVM struct type for each type, and computes field offsets.
/// It also records method slot indices based on the flattened method order.
///
/// The layouts are stored in `ctx.type_layouts` for later use.
pub fn build_layouts(registry: &TypeRegistry, ctx: &mut CodegenCtx) -> Result<(), CodegenError> {
    let mut layouts = HashMap::new();

    // Compute topological order of types (parents before children).
    let order = topological_order(registry);

    for type_name in order {
        let info = registry
            .lookup_type(&type_name)
            .ok_or_else(|| CodegenError::llvm_verification(format!("type '{}' not in registry", type_name)))?;

        // Skip builtin value types and other special types that have no user‑defined layout.
        if info.is_builtin_value
            || type_name == "Object"
            || type_name == "Vector"
            || type_name == "Range"
            || type_name == "Number"
            || type_name == "String"
            || type_name == "Boolean"
        {
            continue;
        }

        let (struct_ty, field_offsets, size) = build_struct_type(&type_name, info, registry, ctx)?;

        let mut layout = TypeLayout::new(struct_ty, size);
        layout.field_offsets = field_offsets;

        // Record method slots using the flattened method order.
        let methods = if !info.flattened_methods.is_empty() {
            &info.flattened_methods
        } else {
            &info.methods
        };
        for (idx, method_name) in methods.keys().enumerate() {
            layout.method_slots.insert(method_name.clone(), idx);
        }

        layouts.insert(type_name.clone(), layout);
    }

    ctx.type_layouts = layouts;
    Ok(())
}

/// Collects the full inheritance chain from the root down to (but excluding) the given type.
fn collect_ancestors(type_name: &str, registry: &TypeRegistry) -> Vec<String> {
    let mut ancestors = Vec::new();
    let mut current = type_name;
    while let Some(info) = registry.lookup_type(current) {
        if let Some(parent) = &info.parent {
            ancestors.insert(0, parent.name.clone());
            current = &parent.name;
        } else {
            break;
        }
    }
    ancestors
}

/// Builds the LLVM struct type for a single type, including the object header,
/// all inherited fields, and own attributes. Returns the struct type, a mapping
/// from attribute name to byte offset, and the total size.
fn build_struct_type<'ctx>(
    type_name: &str,
    info: &TypeInfo,
    registry: &TypeRegistry,
    ctx: &CodegenCtx<'ctx>,
    ) -> Result<(StructType<'ctx>, HashMap<String, usize>, usize), CodegenError> {
    let context = ctx.context;
    let data_layout = ctx.target_machine.get_target_data();

    // ─── 1. Build a flat list of (attribute_name, llvm_type) in inheritance order ──

    let mut attr_names = Vec::new();
    let mut attr_tys = Vec::new();

    // Helper to add attributes from a given type.
    fn add_attributes_from_type<'a>(
        type_info: &TypeInfo,
        attr_names: &mut Vec<String>,
        attr_tys: &mut Vec<BasicTypeEnum<'a>>,
        ctx: &CodegenCtx<'a>,
        registry: &TypeRegistry,
    ) -> Result<(), CodegenError> {
        for (name, attr) in &type_info.attributes {
            let ty = attr
                .declared_type
                .as_ref()
                .ok_or_else(|| CodegenError::llvm_verification(format!("attribute '{}' has no declared type", name)))?;
            let llvm_ty = llvm_type(ctx, registry, ty)?;
            attr_names.push(name.clone());
            attr_tys.push(llvm_ty);
        }
        Ok(())
    }

    // Collect all ancestors (root → immediate parent).
    let ancestors = collect_ancestors(type_name, registry);
    for ancestor_name in ancestors {
        let ancestor_info = registry
            .lookup_type(&ancestor_name)
            .ok_or_else(|| CodegenError::llvm_verification(format!("ancestor '{}' not found", ancestor_name)))?;
        add_attributes_from_type(ancestor_info, &mut attr_names, &mut attr_tys, ctx, registry)?;
    }

    // Add own attributes.
    add_attributes_from_type(info, &mut attr_names, &mut attr_tys, ctx, registry)?;

    // ─── 2. Build the struct type ──────────────────────────────────────────

    // Header fields: ref_count (i64), gc_mark (i1), next (ptr), vtable (ptr)
    let i64_type = context.i64_type();
    let i1_type = context.bool_type(); // gc_mark in memory
    let ptr_type = context.ptr_type(Default::default());

    let mut field_tys = Vec::new();
    field_tys.push(i64_type.into());
    field_tys.push(i1_type.into());
    field_tys.push(ptr_type.into());
    field_tys.push(ptr_type.into());

    // Append all attribute types.
    field_tys.extend(attr_tys);

    let struct_ty = context.struct_type(&field_tys, false);

    // ─── 3. Compute offsets for each attribute ─────────────────────────────

    let mut field_offsets = HashMap::new();
    // Header fields have indices 0..3; attributes start at index 4.
    for (idx, name) in attr_names.iter().enumerate() {
        let field_idx = 4 + idx;
        let offset = data_layout
            .offset_of_element(&struct_ty, field_idx as u32)
            .ok_or_else(|| CodegenError::llvm_verification(format!("offset computation failed for '{}'", name)))?;
        field_offsets.insert(name.clone(), offset as usize);
    }

    // ─── 4. Compute total size ─────────────────────────────────────────────

    // Total allocation size including end-padding for array placement
    let size = data_layout.get_abi_size (&struct_ty) as usize;

    Ok((struct_ty, field_offsets, size))
}

/// Builds vtable globals for all types.
///
/// Must be called after all methods have been declared, because vtables
/// reference the method function declarations stored in `ctx.functions`.
pub fn build_vtables(ctx: &mut CodegenCtx, registry: &TypeRegistry) -> Result<(), CodegenError> {
    let type_names: Vec<_> = ctx.type_layouts.keys().cloned().collect();
    for type_name in type_names {
        let info = registry
            .lookup_type(&type_name)
            .ok_or_else(|| CodegenError::llvm_verification(format!("type '{}' not found", type_name)))?;

        let methods = if !info.flattened_methods.is_empty() {
            &info.flattened_methods
        } else {
            &info.methods
        };

        let mut fn_ptrs = Vec::new();
        for method_name in methods.keys() {
            let owner = owning_type_for_method(&type_name, method_name, registry)
                .ok_or_else(|| CodegenError::llvm_verification(format!(
                    "method '{}' has no declaring type in the ancestor chain of '{}'",
                    method_name, type_name
                )))?;
            let qualified_name = format!("{}::{}", owner, method_name);
            let fn_value = ctx
                .functions
                .get(&qualified_name)
                .cloned()
                .ok_or_else(|| CodegenError::llvm_verification(format!("method '{}' not declared", qualified_name)))?;
                        let fn_ptr = fn_value.as_global_value().as_pointer_value();
            fn_ptrs.push(fn_ptr.into());
        }

        let ptr_type = ctx.context.ptr_type(Default::default());
        let vtable_type = ptr_type.array_type(fn_ptrs.len() as u32);
        let vtable_global = ctx
            .module
            .add_global(vtable_type, None, &format!("{}__vtable", type_name));
        let const_array = ptr_type.const_array(&fn_ptrs);
        vtable_global.set_initializer(&const_array);
        vtable_global.set_constant(true);

        if let Some(layout) = ctx.type_layouts.get_mut(&type_name) {
            layout.vtable_global = Some(vtable_global);
        }
    }
    Ok(())
}

/// Returns the name of the type that actually declares or overrides
/// `method_name`, searching `type_name` and then its ancestors in order.
pub fn owning_type_for_method(
    type_name: &str,
    method_name: &str,
    registry: &TypeRegistry,
) -> Option<String> {
    let mut current = type_name.to_string();
    loop {
        let info = registry.lookup_type(&current)?;
        if info.methods.contains_key(method_name) {
            return Some(current);
        }
        current = info.parent.as_ref()?.name.clone();
    }
}

// Determines if a given type has any subtypes in the current compilation unit
pub fn has_subtypes(type_name: &str, registry: &TypeRegistry) -> bool {
    registry.types.values().any(|info| {
        info.parent.as_ref().map_or(false, |p| p.name == type_name)
    })
}

#[cfg(test)]
mod tests {
    // TODO: Add unit tests for layout building.
}