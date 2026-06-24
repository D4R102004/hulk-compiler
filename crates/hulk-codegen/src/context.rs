//! Shared mutable state threaded through every lowering function.

use std::collections::HashMap;

use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::values::FunctionValue;

/// Everything a lowering function needs: the LLVM context that owns every
/// type and value it creates, the module being built, the instruction
/// builder, and the symbol tables accumulated so far.
///
/// At this stage only `functions` exists. Later phases add a `types` table
/// (per-type struct layouts and vtables) and a lexical scope stack mirroring
/// `hulk_semantic::Environment`'s push/pop discipline.
pub struct CodegenCtx<'ctx> {
    pub context: &'ctx Context,
    pub module: Module<'ctx>,
    pub builder: Builder<'ctx>,
    pub functions: HashMap<String, FunctionValue<'ctx>>,
    /// Monotonically increasing id used to give every string-literal global a unique name
    string_literal_count: u32,
}

impl<'ctx> CodegenCtx<'ctx> {
    pub fn new(context: &'ctx Context, module_name: &str) -> Self {
        let module = context.create_module(module_name);
        let builder = context.create_builder();
        Self {
            context,
            module,
            builder,
            functions: HashMap::new(),
            string_literal_count: 0,
        }
    }

    pub fn next_string_literal_id(&mut self) -> u32 {
        let id = self.string_literal_count;
        self.string_literal_count += 1;
        id
     }
}
