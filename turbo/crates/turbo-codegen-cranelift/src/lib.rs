//! Cranelift-based codegen backend for Turbo (JIT + AOT).
//!
//! This crate is the final stage of the compiler pipeline. It walks the
//! validated AST produced by `turbo_sema` and lowers it to Cranelift IR,
//! then either runs the result in-process (JIT) or emits a relocatable
//! object file that gets linked with the C runtime (`runtime/turbo_rt.c`)
//! into a native binary.
//!
//! # Pipeline position
//!
//! lexer → parser → sema → **codegen** → JIT execution / native binary
//!
//! # Public entry points
//!
//! * [`jit_run`] — JIT-compile and execute a `Module` end-to-end. Used by
//!   `turbolang run` and the REPL.
//! * [`jit_run_function`] — JIT-compile and run one named function (used
//!   by the test runner).
//! * [`aot_compile`] — emit a `.o` for a `Module` and link it with the C
//!   runtime to produce a stand-alone executable.
//! * [`wasm_compile`] — same as above but targeting WebAssembly.
//!
//! Built-in functions (`print`, `len`, `push`, `str_*`, `hashmap_*`, ...)
//! are dispatched in `compile_call` (in the private `expr` module) which
//! delegates to the tables in the private `builtins` module.

use cranelift::prelude::isa::CallConv;
use cranelift::prelude::*;
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{DataDescription, FuncId, Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};
use std::collections::HashMap;
use std::path::Path;
use turbo_ast::*;

mod turbo_types;
pub(crate) use turbo_types::*;

mod runtime;
pub(crate) use runtime::*;

mod builtins;
pub(crate) use builtins::*;

mod wasm_codegen;

mod jit;
pub use jit::{jit_run, jit_run_function};

mod aot;
pub use aot::{aot_compile, wasm_compile};

mod expr;
pub(crate) use expr::{compile_expr, retain_if_needed};

mod stmt;
pub(crate) use stmt::compile_stmt;

mod type_conv;
pub(crate) use type_conv::*;

mod closures;
pub(crate) use closures::{find_captures, has_return, CaptureInfo};

mod compile;
pub(crate) use compile::compile_module;

// ── Runtime C source for AOT linking ────────────────────────────────

const RUNTIME_C: &str = include_str!("../runtime/turbo_rt.c");
const RUNTIME_WASM_C: &str = include_str!("../runtime/turbo_rt_wasm.c");

// ── Codegen context (generic over Module type) ──────────────────────

/// Max depth for inlining recursive functions at call sites.
/// Depth 2 reduces function calls by ~4x while keeping JIT compile time low.
/// Higher depths generate too much IR for Cranelift to compile efficiently.
const MAX_INLINE_DEPTH: usize = 2;

/// Maximum expression recursion depth during codegen. Matches the parser
/// limit (`turbo_parser::MAX_PARSER_DEPTH`) so any AST the parser accepts
/// cannot then blow codegen's stack. Exceeding this limit produces an
/// `E0516` codegen error instead of a segfault.
pub const MAX_CODEGEN_DEPTH: usize = 256;

#[allow(dead_code)]
pub(crate) struct Ctx<'a, M: Module> {
    pub(crate) builder: FunctionBuilder<'a>,
    pub(crate) module: &'a mut M,
    pub(crate) user_fns: &'a HashMap<String, FuncId>,
    pub(crate) fn_ret_types: &'a HashMap<String, TurboTy>,
    pub(crate) fn_asts: &'a HashMap<String, &'a FnDef>,
    pub(crate) fn_type_params: &'a HashMap<String, Vec<String>>,
    pub(crate) rt_fns: &'a HashMap<String, FuncId>,
    pub(crate) vars: HashMap<String, (Variable, cranelift::prelude::types::Type, TurboTy)>,
    pub(crate) next_var: usize,
    pub(crate) data_desc: &'a mut DataDescription,
    pub(crate) string_counter: &'a mut usize,
    pub(crate) ptr_type: cranelift::prelude::types::Type,
    /// Struct field layouts: struct_name -> vec of (field_name, TurboTy)
    pub(crate) struct_fields: &'a HashMap<String, Vec<(String, TurboTy)>>,
    /// Enum variant lists: enum_name -> vec of variant names
    pub(crate) enum_variants: &'a HashMap<String, Vec<String>>,
    /// Data-carrying enum variant fields: (enum_name, variant_name) -> field TurboTys
    pub(crate) enum_variant_fields: &'a HashMap<(String, String), Vec<TurboTy>>,
    /// Max slots per data enum: enum_name -> max field count across all variants
    pub(crate) enum_max_slots: &'a HashMap<String, usize>,
    /// Map from closure span start offset to (synthetic function name, TurboTy::Fn, free_var_names)
    pub(crate) closure_fns: &'a HashMap<usize, (String, TurboTy, Vec<String>)>,
    /// Trait implementations: type_name -> set of trait names
    pub(crate) trait_impls: &'a HashMap<String, Vec<String>>,
    /// Current function inlining depth (0 = no inlining)
    pub(crate) inline_depth: usize,
    /// Current expression recursion depth during codegen. Used by
    /// `compile_expr` to reject pathologically deep ASTs before they
    /// overflow the native stack.
    pub(crate) expr_depth: usize,
    /// Capture info populated during Expr::Closure compilation
    pub(crate) closure_captures: &'a mut HashMap<usize, CaptureInfo>,
    /// Concrete field types for generic struct instances: var_name -> vec of (field_name, TurboTy)
    pub(crate) generic_struct_field_overrides: HashMap<String, Vec<(String, TurboTy)>>,
    /// Temporary: last struct literal's concrete field types (set during StructLit compilation, consumed by Let)
    pub(crate) last_struct_lit_concrete_fields: Option<Vec<(String, TurboTy)>>,
    /// Spawn thunk map: spawn expr span start -> thunk function name
    pub(crate) spawn_thunks: &'a HashMap<usize, String>,
    /// Module-level constants: name -> AST expression (inlined at usage sites)
    pub(crate) constants: &'a HashMap<String, Spanned<Expr>>,
    /// Struct derives: struct_name -> vec of derived trait names
    pub(crate) struct_derives: &'a HashMap<String, Vec<String>>,
    /// Stack of loop contexts for break/continue: (header_block, exit_block)
    pub(crate) loop_stack: Vec<(cranelift::prelude::Block, cranelift::prelude::Block)>,
}

impl<'a, M: Module> Ctx<'a, M> {
    pub(crate) fn fresh_var(
        &mut self,
        cl_ty: cranelift::prelude::types::Type,
        turbo_ty: TurboTy,
    ) -> Variable {
        let var = Variable::new(self.next_var);
        self.next_var += 1;
        self.builder.declare_var(var, cl_ty);
        let _ = turbo_ty; // used by caller
        var
    }

    pub(crate) fn create_string(&mut self, s: &str) -> Result<Value, CodegenError> {
        if s.contains('\0') {
            return Err(CodegenError {
                code: ErrorCode::E0403,
                message: "string literal contains null byte, which is not supported".to_string(),
            });
        }

        let name = format!(".str.{}", *self.string_counter);
        *self.string_counter += 1;

        let data_id = self
            .module
            .declare_data(&name, Linkage::Local, false, false)
            .map_err(|e| CodegenError {
                code: ErrorCode::E0405,
                message: e.to_string(),
            })?;

        self.data_desc.clear();
        let mut bytes = s.as_bytes().to_vec();
        bytes.push(0);
        self.data_desc.define(bytes.into_boxed_slice());

        self.module
            .define_data(data_id, self.data_desc)
            .map_err(|e| CodegenError {
                code: ErrorCode::E0405,
                message: e.to_string(),
            })?;

        let data_ref = self.module.declare_data_in_func(data_id, self.builder.func);
        let ptr = self.builder.ins().global_value(self.ptr_type, data_ref);
        Ok(ptr)
    }

    pub(crate) fn rt_call(&mut self, name: &str, args: &[Value]) {
        let fid = self.rt_fns[name];
        let fref = self.module.declare_func_in_func(fid, self.builder.func);
        self.builder.ins().call(fref, args);
    }

    /// Convert a value to an I8 boolean for use in `brif`.
    /// If the value is already I8 (e.g. from `icmp` or a bool variable),
    /// return it directly — avoiding a redundant `icmp(NotEqual, val, 0)`.
    #[allow(clippy::wrong_self_convention)]
    pub(crate) fn to_bool(&mut self, val: Value) -> Value {
        let ty = self.builder.func.dfg.value_type(val);
        if ty == cranelift::prelude::types::I8 {
            val
        } else {
            let zero = self.builder.ins().iconst(ty, 0);
            self.builder.ins().icmp(IntCC::NotEqual, val, zero)
        }
    }
}

// ── Unit tests ─────────────────────────────────────────────────────

#[cfg(test)]
#[path = "tests_codegen.rs"]
mod tests;
