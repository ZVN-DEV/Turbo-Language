//! Codegen context struct and its impl block.

use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::values::{
    BasicMetadataValueEnum, BasicValueEnum, FunctionValue, IntValue, PointerValue,
};
use inkwell::{AddressSpace, IntPredicate};
use std::collections::HashMap;
use turbo_ast::*;

use crate::types::TurboTy;
use crate::CodegenError;

// ── Codegen context ─────────────────────────────────────────────────

#[allow(dead_code)]
pub(crate) struct Ctx<'a, 'ctx> {
    pub(crate) context: &'ctx Context,
    pub(crate) module: &'a Module<'ctx>,
    pub(crate) builder: &'a Builder<'ctx>,
    /// Currently compiled function
    pub(crate) current_fn: FunctionValue<'ctx>,
    /// User-defined functions
    pub(crate) user_fns: &'a HashMap<String, FunctionValue<'ctx>>,
    /// Function return types
    pub(crate) fn_ret_types: &'a HashMap<String, TurboTy>,
    /// Function ASTs (for inlining)
    pub(crate) fn_asts: &'a HashMap<String, &'a FnDef>,
    /// Function type params
    pub(crate) fn_type_params: &'a HashMap<String, Vec<String>>,
    /// Runtime functions
    pub(crate) rt_fns: &'a HashMap<String, FunctionValue<'ctx>>,
    /// Variable allocas: name -> (alloca ptr, turbo type)
    pub(crate) vars: HashMap<String, (PointerValue<'ctx>, TurboTy)>,
    /// String literal counter for unique names
    pub(crate) string_counter: &'a mut usize,
    /// Struct field layouts
    pub(crate) struct_fields: &'a HashMap<String, Vec<(String, TurboTy)>>,
    /// Enum variant lists
    pub(crate) enum_variants: &'a HashMap<String, Vec<String>>,
    /// Data-carrying enum variant fields
    pub(crate) enum_variant_fields: &'a HashMap<(String, String), Vec<TurboTy>>,
    /// Max slots per data enum
    pub(crate) enum_max_slots: &'a HashMap<String, usize>,
    /// Module-level constants
    pub(crate) constants: &'a HashMap<String, Spanned<Expr>>,
    /// Loop stack for break/continue: (header_block, exit_block)
    pub(crate) loop_stack: Vec<(
        inkwell::basic_block::BasicBlock<'ctx>,
        inkwell::basic_block::BasicBlock<'ctx>,
    )>,
    /// Closure functions: span_start -> (fn_name, TurboTy::Fn, free_var_names)
    pub(crate) closure_fns: &'a HashMap<usize, (String, TurboTy, Vec<String>)>,
    /// Spawn thunks: span_start -> thunk_fn_name
    pub(crate) spawn_thunks: &'a HashMap<usize, String>,
    /// Struct derives: struct_name -> vec of trait names
    pub(crate) struct_derives: &'a HashMap<String, Vec<String>>,
    /// Trait impls: type_name -> vec of trait names
    pub(crate) trait_impls: &'a HashMap<String, Vec<String>>,
    /// Concrete field types for generic struct instances: var_name -> [(field, type)]
    pub(crate) concrete_struct_fields: HashMap<String, Vec<(String, TurboTy)>>,
}

impl<'a, 'ctx> Ctx<'a, 'ctx> {
    pub(crate) fn create_string(&mut self, s: &str) -> Result<PointerValue<'ctx>, CodegenError> {
        let name = format!(".str.{}", *self.string_counter);
        *self.string_counter += 1;
        let val = self
            .builder
            .build_global_string_ptr(s, &name)
            .map_err(|e| CodegenError {
                code: ErrorCode::E0405,
                message: e.to_string(),
            })?;
        Ok(val.as_pointer_value())
    }

    pub(crate) fn rt_call(
        &self,
        name: &str,
        args: &[BasicMetadataValueEnum<'ctx>],
    ) -> Option<BasicValueEnum<'ctx>> {
        let func = self.rt_fns[name];
        let call = self
            .builder
            .build_direct_call(func, args, "")
            .expect("build_direct_call failed");
        call.try_as_basic_value().left()
    }

    /// Convert a value to i1 boolean for use in conditional branches.
    /// LLVM requires `i1` for branch conditions, unlike Cranelift which uses `i8`.
    pub(crate) fn to_bool(&self, val: BasicValueEnum<'ctx>) -> IntValue<'ctx> {
        match val {
            BasicValueEnum::IntValue(iv) => {
                let ty = iv.get_type();
                let zero = ty.const_int(0, false);
                self.builder
                    .build_int_compare(IntPredicate::NE, iv, zero, "tobool")
                    .expect("build_int_compare failed")
            }
            _ => {
                // For non-int types, assume truthy
                self.context.bool_type().const_int(1, false)
            }
        }
    }

    /// Create an alloca in the entry block of the current function.
    pub(crate) fn create_entry_block_alloca(
        &self,
        ty: inkwell::types::BasicTypeEnum<'ctx>,
        name: &str,
    ) -> PointerValue<'ctx> {
        let entry = self.current_fn.get_first_basic_block().unwrap();
        let builder = self.context.create_builder();
        match entry.get_first_instruction() {
            Some(first_instr) => builder.position_before(&first_instr),
            None => builder.position_at_end(entry),
        }
        builder.build_alloca(ty, name).expect("build_alloca failed")
    }
}
