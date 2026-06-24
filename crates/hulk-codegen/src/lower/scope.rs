//! Lexical scope stack for local variables.

use std::collections::HashMap;

use inkwell::values::PointerValue;
use inkwell::types::BasicTypeEnum;

/// A stack of scopes, each mapping a variable name to its LLVM `alloca` pointer.
#[derive(Default)]
pub struct ScopeStack<'ctx> {
    scopes: Vec<HashMap<String,(PointerValue<'ctx>, BasicTypeEnum<'ctx>)>>,
}

impl<'ctx> ScopeStack<'ctx> {
    pub fn new() -> Self {
        Self { scopes: Vec::new() }
    }

    /// Pushes a new empty scope.
    pub fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    /// Pops the innermost scope.
    ///
    /// # Panics
    /// Panics if there is no scope to pop (i.e., the stack is empty).
    pub fn pop_scope(&mut self) {
        self.scopes.pop().expect("scope stack underflow");
    }

    /// Declares a variable in the innermost scope.
    ///
    /// Overwrites any existing binding with the same name in that scope.
    pub fn declare(&mut self, name: &str, ptr: PointerValue<'ctx>, ty: BasicTypeEnum<'ctx>) {
        let scope = self.scopes.last_mut().expect("no scope to declare into");
        scope.insert(name.to_string(), (ptr, ty));
    }

    /// Looks up a variable starting from the innermost scope outward.
    pub fn lookup(&self, name: &str) -> Option<(PointerValue<'ctx>, BasicTypeEnum<'ctx>)> {
        for scope in self.scopes.iter().rev() {
            if let Some(entry) = scope.get(name) {
                return Some(*entry);
            }
        }
        None
    }
}