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
use std::collections::{HashMap, HashSet};
use std::path::Path;
use turbo_ast::*;

mod turbo_types;
pub(crate) use turbo_types::*;

mod runtime;
pub(crate) use runtime::*;
// Public so the CLI can install the program's CLI args before `jit_run`
// (the JIT twin of the AOT `main(argc, argv)` -> rt_set_args path).
pub use runtime::set_program_args;

mod builtins;
pub(crate) use builtins::*;

mod wasm_codegen;

mod jit;
pub use jit::{jit_run, jit_run_function};

mod ffi;

mod aot;
pub use aot::{aot_compile, wasm_compile};

mod expr;
pub(crate) use expr::{
    compile_expr, expr_produces_owned_rc_temp, expr_result_borrows_existing_rc,
    generic_origin_for_value, generic_return_retain_flag_for_value, is_rc_managed_type,
    mark_generic_value_origin, mark_generic_value_origin_with_retain_flag,
    release_expr_temp_if_needed, release_if_needed, release_mutable_param_vars,
    retain_generic_return_if_needed, retain_if_needed,
};
pub(crate) use expr::{retain_array_elements_if_needed, retain_array_prefix_if_needed};

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
/// Shared overflow/cap guard header `#include`d by both C runtimes. The C
/// sources are written to a temp dir and compiled there, so this header must
/// be written alongside them or the `#include "turbo_rt_guards.h"` fails.
const RUNTIME_GUARDS_H: &str = include_str!("../runtime/turbo_rt_guards.h");

// ── Vendored SQLite (AOT linking) ───────────────────────────────────
//
// `turbo_rt.c` pulls in `turbo_rt_sqlite.c` only under `-DTURBO_WITH_SQLITE`,
// which `aot.rs` sets when the program uses SQLite builtins. The shim's
// `#include "sqlite3.h"` is satisfied by writing the vendored header next to
// the runtime in the temp build dir.

/// The SQLite AOT/C shim source (twin of the JIT `rt_sqlite_*` in runtime.rs).
const RUNTIME_SQLITE_C: &str = include_str!("../runtime/turbo_rt_sqlite.c");
/// The vendored SQLite public-domain header. Embedded because the AOT shim
/// `#include`s it at compile time (needed for the common native path).
const SQLITE3_H: &str = include_str!("../runtime/vendor/sqlite3.h");
/// Prebuilt native SQLite object produced by `build.rs` (host target). Used to
/// avoid recompiling the ~9 MB amalgamation on every native `turbolang build`.
const SQLITE3_AOT_OBJECT: &[u8] = include_bytes!(env!("TURBO_SQLITE_AOT_OBJECT"));
/// Path to the vendored amalgamation source, baked in at build time. Only read
/// when *cross-compiling* a SQLite program (the prebuilt object above is
/// host-arch). We do NOT `include_str!` the 9 MB source — that would bloat
/// every `turbolang` install for a rarely-used path — so cross-compiling a
/// SQLite program requires this repo/source tree to be present.
const SQLITE3_C_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/runtime/vendor/sqlite3.c");

/// SQLite compile flags. MUST stay in sync with the `cflags` list in
/// `build.rs` and the C ASan harness (`runtime/tests.sh`).
const SQLITE_CFLAGS: &[&str] = &[
    "-DSQLITE_THREADSAFE=1",
    "-DSQLITE_OMIT_LOAD_EXTENSION",
    "-DSQLITE_OMIT_DEPRECATED",
    "-DSQLITE_DQS=0",
    "-DSQLITE_DEFAULT_MEMSTATUS=0",
    "-DSQLITE_OMIT_SHARED_CACHE",
];

/// True when the module calls any `sqlite_*` builtin, so AOT should link the
/// SQLite engine. Walks the whole AST — every expression and statement — so a
/// call nested anywhere (loops, closures, match arms, interpolations) is
/// detected. Being complete matters: a missed call site would leave AOT
/// without the engine and fail at link with an undefined `rt_sqlite_*`.
pub(crate) fn module_uses_sqlite(module: &turbo_ast::Module) -> bool {
    module.items.iter().any(|item| match &item.node {
        turbo_ast::Item::Function(f) => sqlite_walk_expr(&f.body),
        _ => false,
    })
}

fn sqlite_walk_stmt(stmt: &turbo_ast::Spanned<turbo_ast::Stmt>) -> bool {
    use turbo_ast::Stmt;
    match &stmt.node {
        Stmt::Let { value, .. } | Stmt::LetDestructure { value, .. } => sqlite_walk_expr(value),
        Stmt::Expr(e) | Stmt::Defer(e) => sqlite_walk_expr(e),
        Stmt::Return(e) => e.as_ref().is_some_and(sqlite_walk_expr),
    }
}

fn sqlite_walk_expr(e: &turbo_ast::Spanned<turbo_ast::Expr>) -> bool {
    use turbo_ast::{Expr, InterpolPart};
    let w = sqlite_walk_expr;
    match &e.node {
        // Leaves.
        Expr::IntLit(_)
        | Expr::FloatLit(_)
        | Expr::StringLit(_)
        | Expr::BoolLit(_)
        | Expr::Unit
        | Expr::Ident(_)
        | Expr::EnumVariant { .. }
        | Expr::NoneExpr
        | Expr::Break
        | Expr::Continue => false,
        // Single-child.
        Expr::UnaryOp { expr, .. }
        | Expr::Cast { expr, .. }
        | Expr::Await(expr)
        | Expr::Spawn(expr)
        | Expr::Try(expr)
        | Expr::OkExpr(expr)
        | Expr::ErrExpr(expr)
        | Expr::SomeExpr(expr) => w(expr),
        Expr::Assign { value, .. } | Expr::CompoundAssign { value, .. } => w(value),
        Expr::FieldAccess { object, .. } | Expr::OptionalChain { object, .. } => w(object),
        // Two-child.
        Expr::BinaryOp { left, right, .. } => w(left) || w(right),
        Expr::Range { start, end } => w(start) || w(end),
        Expr::Index { object, index } => w(object) || w(index),
        Expr::While { condition, body } => w(condition) || w(body),
        Expr::ForIn { iterable, body, .. } => w(iterable) || w(body),
        Expr::NullCoalesce { value, default } => w(value) || w(default),
        Expr::FieldAssign { object, value, .. } => w(object) || w(value),
        // Three-child.
        Expr::IndexAssign {
            object,
            index,
            value,
        } => w(object) || w(index) || w(value),
        Expr::If {
            condition,
            then_branch,
            else_branch,
        } => w(condition) || w(then_branch) || else_branch.as_deref().is_some_and(w),
        Expr::IfLet {
            value,
            then_branch,
            else_branch,
            ..
        } => w(value) || w(then_branch) || else_branch.as_deref().is_some_and(w),
        // Calls — the site we actually care about, plus recursion.
        Expr::Call { callee, args } => {
            if let Expr::Ident(name) = &callee.node {
                if name.starts_with("sqlite_") {
                    return true;
                }
            }
            w(callee) || args.iter().any(w)
        }
        Expr::Block { stmts, tail_expr } => {
            stmts.iter().any(sqlite_walk_stmt) || tail_expr.as_deref().is_some_and(w)
        }
        Expr::ArrayLit(items) => items.iter().any(w),
        Expr::StructLit { fields, .. } => fields.iter().any(|(_, v)| w(v)),
        Expr::MapLit(entries) => entries.iter().any(|(k, v)| w(k) || w(v)),
        Expr::Match { subject, arms } => {
            w(subject)
                || arms
                    .iter()
                    .any(|arm| arm.guard.as_ref().is_some_and(w) || w(&arm.body))
        }
        Expr::Interpolation(parts) => parts.iter().any(|p| match p {
            InterpolPart::Lit(_) => false,
            InterpolPart::Expr(inner) => w(inner),
        }),
        Expr::Closure { body, .. } => w(body),
    }
}

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
    /// Names declared in an `extern "C" { ... }` block. A call to one of these
    /// is routed through the FFI function declaration (honoring its declared
    /// `f64`/`f32` return) instead of any same-named native builtin.
    pub(crate) extern_fns: &'a HashSet<String>,
    pub(crate) fn_ret_types: &'a HashMap<String, TurboTy>,
    pub(crate) fn_asts: &'a HashMap<String, &'a FnDef>,
    pub(crate) fn_type_params: &'a HashMap<String, Vec<String>>,
    pub(crate) rt_fns: &'a HashMap<String, FuncId>,
    pub(crate) vars: HashMap<String, (Variable, cranelift::prelude::types::Type, TurboTy)>,
    pub(crate) borrowed_param_vars: Vec<Variable>,
    pub(crate) mutable_param_vars: Vec<(Variable, TurboTy)>,
    /// For generic functions, maps a type-parameter name to the hidden runtime
    /// flag that says whether this instantiation's concrete type is RC-managed.
    /// Landmine: generic impl methods and first-class values of generic
    /// functions are sema-rejected today. If sema allows them later, their
    /// method/adaptor ABIs must thread these hidden flags too.
    pub(crate) generic_rc_flags: HashMap<String, Value>,
    /// Tracks values that are aliases of generic parameters, so return lowering
    /// can retain only borrowed generic values and not owned call results.
    pub(crate) generic_value_origins: HashMap<Value, String>,
    /// Optional per-value flag saying whether a generic-origin value still
    /// needs a return retain. Missing means "yes"; call results that already
    /// retained their return can mark this as false and control-flow merges can
    /// thread it dynamically.
    pub(crate) generic_value_retain_flags: HashMap<Value, Value>,
    pub(crate) generic_var_origins: HashMap<String, String>,
    pub(crate) return_type_param: Option<String>,
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
    /// Whether the current function is @unsafe (skips bounds checks on array access)
    pub(crate) is_unsafe: bool,
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
        let mut bytes = Vec::with_capacity(16 + s.len() + 1);
        bytes.extend_from_slice(&0i64.to_le_bytes());
        bytes.extend_from_slice(&i64::MAX.to_le_bytes());
        bytes.extend_from_slice(s.as_bytes());
        bytes.push(0);
        self.data_desc.set_align(8);
        self.data_desc.define(bytes.into_boxed_slice());

        self.module
            .define_data(data_id, self.data_desc)
            .map_err(|e| CodegenError {
                code: ErrorCode::E0405,
                message: e.to_string(),
            })?;

        let data_ref = self.module.declare_data_in_func(data_id, self.builder.func);
        let raw_ptr = self.builder.ins().global_value(self.ptr_type, data_ref);
        Ok(self.builder.ins().iadd_imm(raw_ptr, 16))
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
