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

// ── Runtime C source for AOT linking ────────────────────────────────

const RUNTIME_C: &str = include_str!("../runtime/turbo_rt.c");

// ── Codegen context (generic over Module type) ──────────────────────

/// Max depth for inlining recursive functions at call sites.
/// Depth 2 reduces function calls by ~4x while keeping JIT compile time low.
/// Higher depths generate too much IR for Cranelift to compile efficiently.
const MAX_INLINE_DEPTH: usize = 2;

#[allow(dead_code)]
pub(crate) struct Ctx<'a, M: Module> {
    pub(crate) builder: FunctionBuilder<'a>,
    pub(crate) module: &'a mut M,
    pub(crate) user_fns: &'a HashMap<String, FuncId>,
    pub(crate) fn_ret_types: &'a HashMap<String, TurboTy>,
    pub(crate) fn_asts: &'a HashMap<String, &'a FnDef>,
    pub(crate) fn_type_params: &'a HashMap<String, Vec<String>>,
    pub(crate) rt_fns: &'a HashMap<String, FuncId>,
    pub(crate) vars: HashMap<String, (Variable, types::Type, TurboTy)>,
    pub(crate) next_var: usize,
    pub(crate) data_desc: &'a mut DataDescription,
    pub(crate) string_counter: &'a mut usize,
    pub(crate) ptr_type: types::Type,
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
    /// Capture info populated during Expr::Closure compilation
    pub(crate) closure_captures: &'a mut HashMap<usize, CaptureInfo>,
    /// Concrete field types for generic struct instances: var_name -> vec of (field_name, TurboTy)
    pub(crate) generic_struct_field_overrides: HashMap<String, Vec<(String, TurboTy)>>,
    /// Temporary: last struct literal's concrete field types (set during StructLit compilation, consumed by Let)
    pub(crate) last_struct_lit_concrete_fields: Option<Vec<(String, TurboTy)>>,
    /// Agent definitions: agent_name -> (model, tools, system_prompt)
    pub(crate) agent_defs: &'a HashMap<String, (String, Vec<String>, Option<String>)>,
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
    pub(crate) fn fresh_var(&mut self, cl_ty: types::Type, turbo_ty: TurboTy) -> Variable {
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
        if ty == types::I8 {
            val
        } else {
            let zero = self.builder.ins().iconst(ty, 0);
            self.builder.ins().icmp(IntCC::NotEqual, val, zero)
        }
    }
}

// ── Public entry points ─────────────────────────────────────────────

pub fn jit_run(ast_module: &turbo_ast::Module) -> Result<(), CodegenError> {
    let mut flag_builder = settings::builder();
    flag_builder.set("use_colocated_libcalls", "false").unwrap();
    flag_builder.set("is_pic", "false").unwrap();
    flag_builder.set("opt_level", "speed_and_size").unwrap();
    flag_builder.set("enable_verifier", "false").unwrap();
    flag_builder.set("enable_alias_analysis", "true").unwrap();

    let isa_builder = cranelift_native::builder().map_err(|e| CodegenError {
        code: ErrorCode::E0405,
        message: e.to_string(),
    })?;
    let isa = isa_builder
        .finish(settings::Flags::new(flag_builder))
        .map_err(|e| CodegenError {
            code: ErrorCode::E0405,
            message: e.to_string(),
        })?;

    let ptr_type = isa.pointer_type();

    let mut jit_builder = JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());

    // Link runtime functions as symbols
    jit_builder.symbol("rt_print_str", rt_print_str as *const u8);
    jit_builder.symbol("rt_print_i64", rt_print_i64 as *const u8);
    jit_builder.symbol("rt_print_f64", rt_print_f64 as *const u8);
    jit_builder.symbol("rt_print_bool", rt_print_bool as *const u8);
    jit_builder.symbol("rt_panic", rt_panic as *const u8);
    jit_builder.symbol("rt_assert_fail", rt_assert_fail as *const u8);
    jit_builder.symbol("rt_assert_eq_fail", rt_assert_eq_fail as *const u8);
    jit_builder.symbol("rt_div_by_zero", rt_div_by_zero as *const u8);
    jit_builder.symbol("rt_int_overflow", rt_int_overflow as *const u8);
    jit_builder.symbol("rt_str_concat", rt_str_concat as *const u8);
    jit_builder.symbol("rt_str_eq", rt_str_eq as *const u8);
    jit_builder.symbol("rt_array_alloc", rt_array_alloc as *const u8);
    jit_builder.symbol("rt_array_get", rt_array_get as *const u8);
    jit_builder.symbol("rt_array_set", rt_array_set as *const u8);
    jit_builder.symbol("rt_array_len", rt_array_len as *const u8);
    jit_builder.symbol("rt_array_push", rt_array_push as *const u8);
    jit_builder.symbol("rt_str_len", rt_str_len as *const u8);
    jit_builder.symbol("rt_struct_alloc", rt_struct_alloc as *const u8);
    jit_builder.symbol("rt_i64_to_str", rt_i64_to_str as *const u8);
    jit_builder.symbol("rt_f64_to_str", rt_f64_to_str as *const u8);
    jit_builder.symbol("rt_bool_to_str", rt_bool_to_str as *const u8);
    jit_builder.symbol("rt_result_ok", rt_result_ok as *const u8);
    jit_builder.symbol("rt_result_err", rt_result_err as *const u8);
    jit_builder.symbol("rt_result_tag", rt_result_tag as *const u8);
    jit_builder.symbol("rt_result_value", rt_result_value as *const u8);
    jit_builder.symbol("rt_option_some", rt_option_some as *const u8);
    jit_builder.symbol("rt_option_none", rt_option_none as *const u8);
    jit_builder.symbol("rt_option_tag", rt_option_tag as *const u8);
    jit_builder.symbol("rt_option_value", rt_option_value as *const u8);
    // Stdlib runtime symbols
    jit_builder.symbol("rt_str_split", rt_str_split as *const u8);
    jit_builder.symbol("rt_str_trim", rt_str_trim as *const u8);
    jit_builder.symbol("rt_str_upper", rt_str_upper as *const u8);
    jit_builder.symbol("rt_str_lower", rt_str_lower as *const u8);
    jit_builder.symbol("rt_str_starts_with", rt_str_starts_with as *const u8);
    jit_builder.symbol("rt_str_ends_with", rt_str_ends_with as *const u8);
    jit_builder.symbol("rt_str_replace", rt_str_replace as *const u8);
    jit_builder.symbol("rt_str_char_at", rt_str_char_at as *const u8);
    jit_builder.symbol("rt_str_contains", rt_str_contains as *const u8);
    jit_builder.symbol("rt_str_index_of", rt_str_index_of as *const u8);
    jit_builder.symbol("rt_str_join", rt_str_join as *const u8);
    jit_builder.symbol("rt_str_repeat", rt_str_repeat as *const u8);
    jit_builder.symbol("rt_read_line", rt_read_line as *const u8);
    jit_builder.symbol("rt_read_file", rt_read_file as *const u8);
    jit_builder.symbol("rt_write_file", rt_write_file as *const u8);
    jit_builder.symbol("rt_pow", rt_pow as *const u8);
    jit_builder.symbol("rt_sqrt", rt_sqrt as *const u8);
    // Async runtime symbols
    jit_builder.symbol("rt_sleep_ms", rt_sleep_ms as *const u8);
    jit_builder.symbol("rt_spawn_with_args", rt_spawn_with_args as *const u8);
    jit_builder.symbol("rt_await_handle", rt_await_handle as *const u8);
    // HTTP + JSON builtins
    jit_builder.symbol("rt_http_get", rt_http_get as *const u8);
    jit_builder.symbol("rt_http_post", rt_http_post as *const u8);
    jit_builder.symbol("rt_json_get", rt_json_get as *const u8);
    jit_builder.symbol("rt_json_stringify", rt_json_stringify as *const u8);
    // HTTP server builtins
    jit_builder.symbol("rt_http_server", rt_http_server as *const u8);
    jit_builder.symbol("rt_http_route", rt_http_route as *const u8);
    jit_builder.symbol("rt_http_listen", rt_http_listen as *const u8);
    jit_builder.symbol("rt_respond", rt_respond as *const u8);
    jit_builder.symbol("rt_request_body", rt_request_body as *const u8);
    // Channel builtins
    jit_builder.symbol("rt_channel_create", rt_channel_create as *const u8);
    jit_builder.symbol("rt_channel_send", rt_channel_send as *const u8);
    jit_builder.symbol("rt_channel_recv", rt_channel_recv as *const u8);
    jit_builder.symbol(
        "rt_channel_clone_sender",
        rt_channel_clone_sender as *const u8,
    );
    // Mutex builtins
    jit_builder.symbol("rt_mutex_create", rt_mutex_create as *const u8);
    jit_builder.symbol("rt_mutex_get", rt_mutex_get as *const u8);
    jit_builder.symbol("rt_mutex_set", rt_mutex_set as *const u8);
    jit_builder.symbol("rt_mutex_clone", rt_mutex_clone as *const u8);
    // HashMap builtins
    jit_builder.symbol("rt_hashmap_new", rt_hashmap_new as *const u8);
    jit_builder.symbol("rt_hashmap_set", rt_hashmap_set as *const u8);
    jit_builder.symbol("rt_hashmap_get", rt_hashmap_get as *const u8);
    jit_builder.symbol("rt_hashmap_has", rt_hashmap_has as *const u8);
    jit_builder.symbol("rt_hashmap_len", rt_hashmap_len as *const u8);
    jit_builder.symbol("rt_hashmap_keys", rt_hashmap_keys as *const u8);
    jit_builder.symbol("rt_hashmap_remove", rt_hashmap_remove as *const u8);
    // ARC runtime
    jit_builder.symbol("rt_retain", rt_retain as *const u8);
    jit_builder.symbol("rt_release", rt_release as *const u8);

    let mut module = JITModule::new(jit_builder);
    let user_fns = compile_module(&mut module, ast_module, ptr_type, Linkage::Local, false)?;

    module.finalize_definitions().map_err(|e| CodegenError {
        code: ErrorCode::E0406,
        message: e.to_string(),
    })?;

    let main_id = user_fns.get("main").ok_or_else(|| CodegenError {
        code: ErrorCode::E0405,
        message: "no `main` function found".to_string(),
    })?;
    let main_ptr = module.get_finalized_function(*main_id);
    let main_fn: fn() = unsafe { std::mem::transmute(main_ptr) };
    main_fn();

    Ok(())
}

/// Compile a module and run a single named function (used for `turbolang test --run-fn`).
/// The function is called via JIT and the process exits with the function's outcome
/// (0 on success, 1 on assertion failure).
pub fn jit_run_function(ast_module: &turbo_ast::Module, fn_name: &str) -> Result<(), CodegenError> {
    let mut flag_builder = settings::builder();
    flag_builder.set("use_colocated_libcalls", "false").unwrap();
    flag_builder.set("is_pic", "false").unwrap();
    flag_builder.set("opt_level", "speed_and_size").unwrap();
    flag_builder.set("enable_verifier", "false").unwrap();
    flag_builder.set("enable_alias_analysis", "true").unwrap();

    let isa_builder = cranelift_native::builder().map_err(|e| CodegenError {
        code: ErrorCode::E0405,
        message: e.to_string(),
    })?;
    let isa = isa_builder
        .finish(settings::Flags::new(flag_builder))
        .map_err(|e| CodegenError {
            code: ErrorCode::E0405,
            message: e.to_string(),
        })?;

    let ptr_type = isa.pointer_type();

    let mut jit_builder = JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());

    // Link all runtime functions (same as jit_run)
    jit_builder.symbol("rt_print_str", rt_print_str as *const u8);
    jit_builder.symbol("rt_print_i64", rt_print_i64 as *const u8);
    jit_builder.symbol("rt_print_f64", rt_print_f64 as *const u8);
    jit_builder.symbol("rt_print_bool", rt_print_bool as *const u8);
    jit_builder.symbol("rt_panic", rt_panic as *const u8);
    jit_builder.symbol("rt_assert_fail", rt_assert_fail as *const u8);
    jit_builder.symbol("rt_assert_eq_fail", rt_assert_eq_fail as *const u8);
    jit_builder.symbol("rt_div_by_zero", rt_div_by_zero as *const u8);
    jit_builder.symbol("rt_int_overflow", rt_int_overflow as *const u8);
    jit_builder.symbol("rt_str_concat", rt_str_concat as *const u8);
    jit_builder.symbol("rt_str_eq", rt_str_eq as *const u8);
    jit_builder.symbol("rt_array_alloc", rt_array_alloc as *const u8);
    jit_builder.symbol("rt_array_get", rt_array_get as *const u8);
    jit_builder.symbol("rt_array_set", rt_array_set as *const u8);
    jit_builder.symbol("rt_array_len", rt_array_len as *const u8);
    jit_builder.symbol("rt_array_push", rt_array_push as *const u8);
    jit_builder.symbol("rt_str_len", rt_str_len as *const u8);
    jit_builder.symbol("rt_struct_alloc", rt_struct_alloc as *const u8);
    jit_builder.symbol("rt_i64_to_str", rt_i64_to_str as *const u8);
    jit_builder.symbol("rt_f64_to_str", rt_f64_to_str as *const u8);
    jit_builder.symbol("rt_bool_to_str", rt_bool_to_str as *const u8);
    jit_builder.symbol("rt_result_ok", rt_result_ok as *const u8);
    jit_builder.symbol("rt_result_err", rt_result_err as *const u8);
    jit_builder.symbol("rt_result_tag", rt_result_tag as *const u8);
    jit_builder.symbol("rt_result_value", rt_result_value as *const u8);
    jit_builder.symbol("rt_option_some", rt_option_some as *const u8);
    jit_builder.symbol("rt_option_none", rt_option_none as *const u8);
    jit_builder.symbol("rt_option_tag", rt_option_tag as *const u8);
    jit_builder.symbol("rt_option_value", rt_option_value as *const u8);
    jit_builder.symbol("rt_str_split", rt_str_split as *const u8);
    jit_builder.symbol("rt_str_trim", rt_str_trim as *const u8);
    jit_builder.symbol("rt_str_upper", rt_str_upper as *const u8);
    jit_builder.symbol("rt_str_lower", rt_str_lower as *const u8);
    jit_builder.symbol("rt_str_starts_with", rt_str_starts_with as *const u8);
    jit_builder.symbol("rt_str_ends_with", rt_str_ends_with as *const u8);
    jit_builder.symbol("rt_str_replace", rt_str_replace as *const u8);
    jit_builder.symbol("rt_str_char_at", rt_str_char_at as *const u8);
    jit_builder.symbol("rt_str_contains", rt_str_contains as *const u8);
    jit_builder.symbol("rt_str_index_of", rt_str_index_of as *const u8);
    jit_builder.symbol("rt_str_join", rt_str_join as *const u8);
    jit_builder.symbol("rt_str_repeat", rt_str_repeat as *const u8);
    jit_builder.symbol("rt_read_line", rt_read_line as *const u8);
    jit_builder.symbol("rt_read_file", rt_read_file as *const u8);
    jit_builder.symbol("rt_write_file", rt_write_file as *const u8);
    jit_builder.symbol("rt_pow", rt_pow as *const u8);
    jit_builder.symbol("rt_sqrt", rt_sqrt as *const u8);
    jit_builder.symbol("rt_sleep_ms", rt_sleep_ms as *const u8);
    jit_builder.symbol("rt_spawn_with_args", rt_spawn_with_args as *const u8);
    jit_builder.symbol("rt_await_handle", rt_await_handle as *const u8);
    jit_builder.symbol("rt_http_get", rt_http_get as *const u8);
    jit_builder.symbol("rt_http_post", rt_http_post as *const u8);
    jit_builder.symbol("rt_json_get", rt_json_get as *const u8);
    jit_builder.symbol("rt_json_stringify", rt_json_stringify as *const u8);
    jit_builder.symbol("rt_http_server", rt_http_server as *const u8);
    jit_builder.symbol("rt_http_route", rt_http_route as *const u8);
    jit_builder.symbol("rt_http_listen", rt_http_listen as *const u8);
    jit_builder.symbol("rt_respond", rt_respond as *const u8);
    jit_builder.symbol("rt_request_body", rt_request_body as *const u8);
    jit_builder.symbol("rt_channel_create", rt_channel_create as *const u8);
    jit_builder.symbol("rt_channel_send", rt_channel_send as *const u8);
    jit_builder.symbol("rt_channel_recv", rt_channel_recv as *const u8);
    jit_builder.symbol(
        "rt_channel_clone_sender",
        rt_channel_clone_sender as *const u8,
    );
    jit_builder.symbol("rt_mutex_create", rt_mutex_create as *const u8);
    jit_builder.symbol("rt_mutex_get", rt_mutex_get as *const u8);
    jit_builder.symbol("rt_mutex_set", rt_mutex_set as *const u8);
    jit_builder.symbol("rt_mutex_clone", rt_mutex_clone as *const u8);
    jit_builder.symbol("rt_retain", rt_retain as *const u8);
    jit_builder.symbol("rt_release", rt_release as *const u8);

    let mut module = JITModule::new(jit_builder);
    let user_fns = compile_module(&mut module, ast_module, ptr_type, Linkage::Local, false)?;

    module.finalize_definitions().map_err(|e| CodegenError {
        code: ErrorCode::E0405,
        message: e.to_string(),
    })?;

    let func_id = user_fns.get(fn_name).ok_or_else(|| CodegenError {
        code: ErrorCode::E0405,
        message: format!("no function `{fn_name}` found"),
    })?;
    let func_ptr = module.get_finalized_function(*func_id);
    let func: fn() = unsafe { std::mem::transmute(func_ptr) };
    func();

    Ok(())
}

pub fn aot_compile(
    ast_module: &turbo_ast::Module,
    output_path: &Path,
    optimize: bool,
) -> Result<(), CodegenError> {
    let mut flag_builder = settings::builder();
    flag_builder.set("use_colocated_libcalls", "false").unwrap();
    flag_builder.set("is_pic", "true").unwrap(); // Required for AOT linking on macOS
    if optimize {
        flag_builder.set("opt_level", "speed_and_size").unwrap();
        flag_builder.set("enable_verifier", "false").unwrap();
        flag_builder.set("enable_alias_analysis", "true").unwrap();
    }

    let isa_builder = cranelift_native::builder().map_err(|e| CodegenError {
        code: ErrorCode::E0405,
        message: e.to_string(),
    })?;
    let isa = isa_builder
        .finish(settings::Flags::new(flag_builder))
        .map_err(|e| CodegenError {
            code: ErrorCode::E0405,
            message: e.to_string(),
        })?;

    let ptr_type = isa.pointer_type();

    let obj_builder = ObjectBuilder::new(
        isa,
        "turbo_module",
        cranelift_module::default_libcall_names(),
    )
    .map_err(|e| CodegenError {
        code: ErrorCode::E0405,
        message: e.to_string(),
    })?;

    let mut module = ObjectModule::new(obj_builder);
    compile_module(&mut module, ast_module, ptr_type, Linkage::Export, true)?;

    let product = module.finish();
    let obj_bytes = product.emit().map_err(|e| CodegenError {
        code: ErrorCode::E0404,
        message: format!("failed to emit object: {e}"),
    })?;

    // Write object file and runtime to temp, then link with cc
    let tmp_dir = std::env::temp_dir().join(format!("turbo_aot_{}", std::process::id()));
    std::fs::create_dir_all(&tmp_dir).map_err(|e| CodegenError {
        code: ErrorCode::E0404,
        message: format!("failed to create temp dir: {e}"),
    })?;

    let obj_path = tmp_dir.join("turbo.o");
    let rt_path = tmp_dir.join("turbo_rt.c");

    std::fs::write(&obj_path, &obj_bytes).map_err(|e| CodegenError {
        code: ErrorCode::E0400,
        message: format!("failed to write object file: {e}"),
    })?;
    std::fs::write(&rt_path, RUNTIME_C).map_err(|e| CodegenError {
        code: ErrorCode::E0400,
        message: format!("failed to write runtime: {e}"),
    })?;

    let output = std::process::Command::new("cc")
        .arg(&rt_path)
        .arg(&obj_path)
        .arg("-lm")
        .arg("-o")
        .arg(output_path)
        .output()
        .map_err(|e| CodegenError {
            code: ErrorCode::E0404,
            message: format!("failed to run linker: {e}"),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(CodegenError {
            code: ErrorCode::E0404,
            message: format!("linker failed: {stderr}"),
        });
    }

    // Clean up temp directory and all files within
    let _ = std::fs::remove_dir_all(&tmp_dir);

    Ok(())
}

// ── Inlining helpers ────────────────────────────────────────────────

/// Returns true if an expression subtree contains any return statement.
/// Functions with returns can't be safely inlined (would need merge blocks).
fn has_return(expr: &Expr) -> bool {
    match expr {
        Expr::Block { stmts, tail_expr } => {
            for stmt in stmts {
                match &stmt.node {
                    Stmt::Return(_) => return true,
                    Stmt::Let { value, .. } => {
                        if has_return(&value.node) {
                            return true;
                        }
                    }
                    Stmt::Expr(e) => {
                        if has_return(&e.node) {
                            return true;
                        }
                    }
                    Stmt::Defer(e) => {
                        if has_return(&e.node) {
                            return true;
                        }
                    }
                }
            }
            tail_expr.as_ref().is_some_and(|t| has_return(&t.node))
        }
        Expr::If {
            condition,
            then_branch,
            else_branch,
        } => {
            has_return(&condition.node)
                || has_return(&then_branch.node)
                || else_branch.as_ref().is_some_and(|e| has_return(&e.node))
        }
        Expr::While { condition, body } => has_return(&condition.node) || has_return(&body.node),
        Expr::ForIn { iterable, body, .. } => has_return(&iterable.node) || has_return(&body.node),
        Expr::BinaryOp { left, right, .. } => has_return(&left.node) || has_return(&right.node),
        Expr::UnaryOp { expr, .. } => has_return(&expr.node),
        Expr::Call { callee, args } => {
            has_return(&callee.node) || args.iter().any(|a| has_return(&a.node))
        }
        Expr::Assign { value, .. } | Expr::CompoundAssign { value, .. } => has_return(&value.node),
        Expr::Await(inner) | Expr::Spawn(inner) | Expr::Try(inner) => has_return(&inner.node),
        Expr::FieldAssign { object, value, .. } => {
            has_return(&object.node) || has_return(&value.node)
        }
        Expr::IndexAssign {
            object,
            index,
            value,
        } => has_return(&object.node) || has_return(&index.node) || has_return(&value.node),
        Expr::Index { object, index } => has_return(&object.node) || has_return(&index.node),
        Expr::Closure { body, .. } => has_return(&body.node),
        Expr::Match { subject, arms } => {
            has_return(&subject.node) || arms.iter().any(|a| has_return(&a.body.node))
        }
        Expr::OkExpr(e) | Expr::ErrExpr(e) | Expr::SomeExpr(e) => has_return(&e.node),
        Expr::NoneExpr => false,
        Expr::NullCoalesce { value, default } => {
            has_return(&value.node) || has_return(&default.node)
        }
        Expr::Interpolation(parts) => parts.iter().any(|p| {
            if let InterpolPart::Expr(e) = p {
                has_return(&e.node)
            } else {
                false
            }
        }),
        _ => false,
    }
}

// ── Shared module compilation ───────────────────────────────────────

/// Convert a TurboTy to a Cranelift types::Type
pub(crate) fn turbo_ty_to_cl_type(tty: &TurboTy, ptr_type: types::Type) -> types::Type {
    match tty {
        TurboTy::Int => types::I64,
        TurboTy::Float => types::F64,
        TurboTy::Bool => types::I8,
        TurboTy::Str => ptr_type,
        TurboTy::Unit => types::I64,   // should not happen, but fallback
        TurboTy::Fn(_, _) => ptr_type, // function pointers are pointers
        TurboTy::Array(_) => ptr_type,
        TurboTy::Struct(_) => ptr_type,
        TurboTy::Enum(_) => types::I64, // both unit (tag) and data (ptr) enums fit in I64
        TurboTy::Result(_, _) => ptr_type,
        TurboTy::Optional(_) => ptr_type,
        TurboTy::Agent(_) => ptr_type, // heap-allocated struct pointer
        TurboTy::Future(_) => ptr_type, // thread handle pointer
    }
}

// ── Closure capture analysis ────────────────────────────────────────

/// Collect all free variable references in an expression.
/// `bound` contains names defined locally (parameters, let bindings).
/// Any Ident not in `bound` is a free variable (capture candidate).
fn collect_free_vars(expr: &Expr, bound: &mut Vec<String>, free: &mut Vec<String>) {
    match expr {
        Expr::Ident(name) => {
            if !bound.contains(name) && !free.contains(name) {
                free.push(name.clone());
            }
        }
        Expr::Block { stmts, tail_expr } => {
            let orig_len = bound.len();
            for stmt in stmts {
                match &stmt.node {
                    Stmt::Let { name, value, .. } => {
                        collect_free_vars(&value.node, bound, free);
                        bound.push(name.clone());
                    }
                    Stmt::Expr(e) => collect_free_vars(&e.node, bound, free),
                    Stmt::Return(Some(e)) => collect_free_vars(&e.node, bound, free),
                    Stmt::Return(None) => {}
                    Stmt::Defer(e) => collect_free_vars(&e.node, bound, free),
                }
            }
            if let Some(tail) = tail_expr {
                collect_free_vars(&tail.node, bound, free);
            }
            bound.truncate(orig_len);
        }
        Expr::BinaryOp { left, right, .. } => {
            collect_free_vars(&left.node, bound, free);
            collect_free_vars(&right.node, bound, free);
        }
        Expr::UnaryOp { expr: e, .. } => {
            collect_free_vars(&e.node, bound, free);
        }
        Expr::Call { callee, args } => {
            collect_free_vars(&callee.node, bound, free);
            for arg in args {
                collect_free_vars(&arg.node, bound, free);
            }
        }
        Expr::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_free_vars(&condition.node, bound, free);
            collect_free_vars(&then_branch.node, bound, free);
            if let Some(e) = else_branch {
                collect_free_vars(&e.node, bound, free);
            }
        }
        Expr::While { condition, body } => {
            collect_free_vars(&condition.node, bound, free);
            collect_free_vars(&body.node, bound, free);
        }
        Expr::ForIn {
            var_name,
            iterable,
            body,
        } => {
            collect_free_vars(&iterable.node, bound, free);
            let orig_len = bound.len();
            bound.push(var_name.clone());
            collect_free_vars(&body.node, bound, free);
            bound.truncate(orig_len);
        }
        Expr::Assign { target, value } => {
            if !bound.contains(target) && !free.contains(target) {
                free.push(target.clone());
            }
            collect_free_vars(&value.node, bound, free);
        }
        Expr::CompoundAssign { target, value, .. } => {
            if !bound.contains(target) && !free.contains(target) {
                free.push(target.clone());
            }
            collect_free_vars(&value.node, bound, free);
        }
        Expr::FieldAssign { object, value, .. } => {
            collect_free_vars(&object.node, bound, free);
            collect_free_vars(&value.node, bound, free);
        }
        Expr::IndexAssign {
            object,
            index,
            value,
        } => {
            collect_free_vars(&object.node, bound, free);
            collect_free_vars(&index.node, bound, free);
            collect_free_vars(&value.node, bound, free);
        }
        Expr::ArrayLit(elems) => {
            for e in elems {
                collect_free_vars(&e.node, bound, free);
            }
        }
        Expr::Index { object, index } => {
            collect_free_vars(&object.node, bound, free);
            collect_free_vars(&index.node, bound, free);
        }
        Expr::StructLit { fields, .. } => {
            for (_, v) in fields {
                collect_free_vars(&v.node, bound, free);
            }
        }
        Expr::FieldAccess { object, .. } => {
            collect_free_vars(&object.node, bound, free);
        }
        Expr::Match { subject, arms } => {
            collect_free_vars(&subject.node, bound, free);
            for arm in arms {
                let orig_len = bound.len();
                match &arm.pattern.node {
                    Pattern::Ok(name) | Pattern::Err(name) | Pattern::Some(name) => {
                        bound.push(name.clone());
                    }
                    Pattern::Ident(name) if name != "_" => {
                        bound.push(name.clone());
                    }
                    _ => {}
                }
                if let Some(ref guard) = arm.guard {
                    collect_free_vars(&guard.node, bound, free);
                }
                collect_free_vars(&arm.body.node, bound, free);
                bound.truncate(orig_len);
            }
        }
        Expr::Interpolation(parts) => {
            for part in parts {
                if let InterpolPart::Expr(e) = part {
                    collect_free_vars(&e.node, bound, free);
                }
            }
        }
        Expr::Closure { params, body, .. } => {
            let orig_len = bound.len();
            for p in params {
                bound.push(p.name.clone());
            }
            collect_free_vars(&body.node, bound, free);
            bound.truncate(orig_len);
        }
        Expr::Range { start, end } => {
            collect_free_vars(&start.node, bound, free);
            collect_free_vars(&end.node, bound, free);
        }
        Expr::OkExpr(e) | Expr::ErrExpr(e) | Expr::SomeExpr(e) => {
            collect_free_vars(&e.node, bound, free);
        }
        Expr::NullCoalesce { value, default } => {
            collect_free_vars(&value.node, bound, free);
            collect_free_vars(&default.node, bound, free);
        }
        Expr::Await(inner) | Expr::Spawn(inner) | Expr::Try(inner) => {
            collect_free_vars(&inner.node, bound, free);
        }
        Expr::EnumVariant { .. }
        | Expr::IntLit(_)
        | Expr::FloatLit(_)
        | Expr::StringLit(_)
        | Expr::BoolLit(_)
        | Expr::Unit
        | Expr::NoneExpr
        | Expr::Break
        | Expr::Continue => {}
    }
}

/// Determine which variables a closure captures from its enclosing scope.
fn find_captures(closure_params: &[Param], body: &Expr, outer_vars: &[String]) -> Vec<String> {
    let mut bound: Vec<String> = closure_params.iter().map(|p| p.name.clone()).collect();
    let mut free = Vec::new();
    collect_free_vars(body, &mut bound, &mut free);
    free.retain(|name| outer_vars.contains(name));
    free
}

/// Info about a closure's captures, determined at creation site during compilation
#[derive(Debug, Clone)]
pub(crate) struct CaptureInfo {
    pub(crate) captures: Vec<(String, TurboTy)>,
}

// ── Closure extraction ──────────────────────────────────────────────

/// A pre-extracted closure with its metadata
struct ExtractedClosure<'a> {
    /// Byte offset of the `|` token in source -- used as a unique key
    span_start: usize,
    /// Synthetic function name (e.g. `__closure_0`)
    name: String,
    /// Closure parameters
    params: &'a [Param],
    /// Declared return type (if any)
    return_type: &'a Option<Spanned<TypeExpr>>,
    /// Closure body
    body: &'a Spanned<Expr>,
    /// Free variable names referenced in the body (potential captures)
    free_vars: Vec<String>,
}

/// Walk an expression tree and collect all closure nodes.
fn extract_closures_from_expr<'a>(
    expr: &'a Spanned<Expr>,
    out: &mut Vec<ExtractedClosure<'a>>,
    counter: &mut usize,
) {
    match &expr.node {
        Expr::Closure {
            params,
            return_type,
            body,
        } => {
            let name = format!("__closure_{}", *counter);
            *counter += 1;
            let mut bound: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
            let mut free_vars = Vec::new();
            collect_free_vars(&body.node, &mut bound, &mut free_vars);
            out.push(ExtractedClosure {
                span_start: expr.span.start,
                name,
                params,
                return_type,
                body,
                free_vars,
            });
            // Also scan the closure body for nested closures
            extract_closures_from_expr(body, out, counter);
        }
        Expr::Block { stmts, tail_expr } => {
            for stmt in stmts {
                match &stmt.node {
                    Stmt::Let { value, .. } => extract_closures_from_expr(value, out, counter),
                    Stmt::Expr(e) => extract_closures_from_expr(e, out, counter),
                    Stmt::Return(Some(e)) => extract_closures_from_expr(e, out, counter),
                    Stmt::Return(None) => {}
                    Stmt::Defer(e) => extract_closures_from_expr(e, out, counter),
                }
            }
            if let Some(tail) = tail_expr {
                extract_closures_from_expr(tail, out, counter);
            }
        }
        Expr::If {
            condition,
            then_branch,
            else_branch,
        } => {
            extract_closures_from_expr(condition, out, counter);
            extract_closures_from_expr(then_branch, out, counter);
            if let Some(e) = else_branch {
                extract_closures_from_expr(e, out, counter);
            }
        }
        Expr::While { condition, body } => {
            extract_closures_from_expr(condition, out, counter);
            extract_closures_from_expr(body, out, counter);
        }
        Expr::ForIn { iterable, body, .. } => {
            extract_closures_from_expr(iterable, out, counter);
            extract_closures_from_expr(body, out, counter);
        }
        Expr::BinaryOp { left, right, .. } => {
            extract_closures_from_expr(left, out, counter);
            extract_closures_from_expr(right, out, counter);
        }
        Expr::UnaryOp { expr, .. } => {
            extract_closures_from_expr(expr, out, counter);
        }
        Expr::Call { callee, args } => {
            extract_closures_from_expr(callee, out, counter);
            for arg in args {
                extract_closures_from_expr(arg, out, counter);
            }
        }
        Expr::Assign { value, .. } | Expr::CompoundAssign { value, .. } => {
            extract_closures_from_expr(value, out, counter);
        }
        Expr::FieldAssign { object, value, .. } => {
            extract_closures_from_expr(object, out, counter);
            extract_closures_from_expr(value, out, counter);
        }
        Expr::IndexAssign {
            object,
            index,
            value,
        } => {
            extract_closures_from_expr(object, out, counter);
            extract_closures_from_expr(index, out, counter);
            extract_closures_from_expr(value, out, counter);
        }
        Expr::OkExpr(value) | Expr::ErrExpr(value) | Expr::SomeExpr(value) => {
            extract_closures_from_expr(value, out, counter);
        }
        Expr::NullCoalesce { value, default } => {
            extract_closures_from_expr(value, out, counter);
            extract_closures_from_expr(default, out, counter);
        }
        Expr::Await(inner) | Expr::Spawn(inner) | Expr::Try(inner) => {
            extract_closures_from_expr(inner, out, counter);
        }
        _ => {} // Literals, Ident, Unit, NoneExpr, etc. -- no sub-expressions with closures
    }
}

/// Extract all closures from the entire module
fn extract_all_closures(ast_module: &turbo_ast::Module) -> Vec<ExtractedClosure<'_>> {
    let mut closures = Vec::new();
    let mut counter = 0;
    for item in &ast_module.items {
        match &item.node {
            Item::Function(f) => {
                extract_closures_from_expr(&f.body, &mut closures, &mut counter);
            }
            Item::Impl(imp) => {
                for method in &imp.methods {
                    extract_closures_from_expr(&method.node.body, &mut closures, &mut counter);
                }
            }
            _ => {}
        }
    }
    closures
}

// ── Spawn site extraction ───────────────────────────────────────────

/// A pre-extracted spawn site: `spawn fn_call(args...)`
struct SpawnSite {
    span_start: usize,
    thunk_name: String,
    callee_name: String,
    num_args: usize,
}

fn extract_spawn_sites_from_expr(
    expr: &Spanned<Expr>,
    out: &mut Vec<SpawnSite>,
    counter: &mut usize,
) {
    match &expr.node {
        Expr::Spawn(inner) => {
            if let Expr::Call { callee, args } = &inner.node {
                if let Expr::Ident(name) = &callee.node {
                    out.push(SpawnSite {
                        span_start: expr.span.start,
                        thunk_name: format!("__spawn_thunk_{}", *counter),
                        callee_name: name.clone(),
                        num_args: args.len(),
                    });
                    *counter += 1;
                    for arg in args {
                        extract_spawn_sites_from_expr(arg, out, counter);
                    }
                    return;
                }
            }
            extract_spawn_sites_from_expr(inner, out, counter);
        }
        Expr::Block { stmts, tail_expr } => {
            for stmt in stmts {
                match &stmt.node {
                    Stmt::Let { value, .. } => extract_spawn_sites_from_expr(value, out, counter),
                    Stmt::Expr(e) => extract_spawn_sites_from_expr(e, out, counter),
                    Stmt::Return(Some(e)) => extract_spawn_sites_from_expr(e, out, counter),
                    Stmt::Return(None) => {}
                    Stmt::Defer(e) => extract_spawn_sites_from_expr(e, out, counter),
                }
            }
            if let Some(tail) = tail_expr {
                extract_spawn_sites_from_expr(tail, out, counter);
            }
        }
        Expr::If {
            condition,
            then_branch,
            else_branch,
        } => {
            extract_spawn_sites_from_expr(condition, out, counter);
            extract_spawn_sites_from_expr(then_branch, out, counter);
            if let Some(e) = else_branch {
                extract_spawn_sites_from_expr(e, out, counter);
            }
        }
        Expr::While { condition, body } => {
            extract_spawn_sites_from_expr(condition, out, counter);
            extract_spawn_sites_from_expr(body, out, counter);
        }
        Expr::ForIn { iterable, body, .. } => {
            extract_spawn_sites_from_expr(iterable, out, counter);
            extract_spawn_sites_from_expr(body, out, counter);
        }
        Expr::BinaryOp { left, right, .. } => {
            extract_spawn_sites_from_expr(left, out, counter);
            extract_spawn_sites_from_expr(right, out, counter);
        }
        Expr::UnaryOp { expr, .. } => {
            extract_spawn_sites_from_expr(expr, out, counter);
        }
        Expr::Call { callee, args } => {
            extract_spawn_sites_from_expr(callee, out, counter);
            for arg in args {
                extract_spawn_sites_from_expr(arg, out, counter);
            }
        }
        Expr::Assign { value, .. } | Expr::CompoundAssign { value, .. } => {
            extract_spawn_sites_from_expr(value, out, counter);
        }
        Expr::FieldAssign { object, value, .. } => {
            extract_spawn_sites_from_expr(object, out, counter);
            extract_spawn_sites_from_expr(value, out, counter);
        }
        Expr::IndexAssign {
            object,
            index,
            value,
        } => {
            extract_spawn_sites_from_expr(object, out, counter);
            extract_spawn_sites_from_expr(index, out, counter);
            extract_spawn_sites_from_expr(value, out, counter);
        }
        Expr::Index { object, index } => {
            extract_spawn_sites_from_expr(object, out, counter);
            extract_spawn_sites_from_expr(index, out, counter);
        }
        Expr::Range { start, end } => {
            extract_spawn_sites_from_expr(start, out, counter);
            extract_spawn_sites_from_expr(end, out, counter);
        }
        Expr::FieldAccess { object, .. } => {
            extract_spawn_sites_from_expr(object, out, counter);
        }
        Expr::ArrayLit(elems) => {
            for e in elems {
                extract_spawn_sites_from_expr(e, out, counter);
            }
        }
        Expr::StructLit { fields, .. } => {
            for (_, e) in fields {
                extract_spawn_sites_from_expr(e, out, counter);
            }
        }
        Expr::Match { subject, arms } => {
            extract_spawn_sites_from_expr(subject, out, counter);
            for arm in arms {
                if let Some(ref guard) = arm.guard {
                    extract_spawn_sites_from_expr(guard, out, counter);
                }
                extract_spawn_sites_from_expr(&arm.body, out, counter);
            }
        }
        Expr::Closure { body, .. } => {
            extract_spawn_sites_from_expr(body, out, counter);
        }
        Expr::OkExpr(e) | Expr::ErrExpr(e) | Expr::SomeExpr(e) | Expr::Await(e) | Expr::Try(e) => {
            extract_spawn_sites_from_expr(e, out, counter);
        }
        Expr::NullCoalesce { value, default } => {
            extract_spawn_sites_from_expr(value, out, counter);
            extract_spawn_sites_from_expr(default, out, counter);
        }
        Expr::Interpolation(parts) => {
            for part in parts {
                if let InterpolPart::Expr(e) = part {
                    extract_spawn_sites_from_expr(e, out, counter);
                }
            }
        }
        _ => {}
    }
}

fn extract_all_spawn_sites(ast_module: &turbo_ast::Module) -> Vec<SpawnSite> {
    let mut sites = Vec::new();
    let mut counter = 0;
    for item in &ast_module.items {
        match &item.node {
            Item::Function(f) => extract_spawn_sites_from_expr(&f.body, &mut sites, &mut counter),
            Item::Impl(imp) => {
                for method in &imp.methods {
                    extract_spawn_sites_from_expr(&method.node.body, &mut sites, &mut counter);
                }
            }
            _ => {}
        }
    }
    sites
}

fn compile_module<M: Module>(
    module: &mut M,
    ast_module: &turbo_ast::Module,
    ptr_type: types::Type,
    main_linkage: Linkage,
    rename_main: bool,
) -> Result<HashMap<String, FuncId>, CodegenError> {
    // Declare runtime functions
    let mut rt_fns: HashMap<String, FuncId> = HashMap::new();
    declare_rt_fn(module, &mut rt_fns, "rt_print_str", &[ptr_type], None)?;
    declare_rt_fn(module, &mut rt_fns, "rt_print_i64", &[types::I64], None)?;
    declare_rt_fn(module, &mut rt_fns, "rt_print_f64", &[types::F64], None)?;
    declare_rt_fn(module, &mut rt_fns, "rt_print_bool", &[types::I8], None)?;
    declare_rt_fn(module, &mut rt_fns, "rt_panic", &[ptr_type], None)?;
    declare_rt_fn(module, &mut rt_fns, "rt_assert_fail", &[ptr_type], None)?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_assert_eq_fail",
        &[types::I64, ptr_type, ptr_type],
        None,
    )?;
    declare_rt_fn(module, &mut rt_fns, "rt_div_by_zero", &[], None)?;
    declare_rt_fn(module, &mut rt_fns, "rt_int_overflow", &[], None)?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_str_concat",
        &[ptr_type, ptr_type],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_str_eq",
        &[ptr_type, ptr_type],
        Some(types::I8),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_array_alloc",
        &[types::I64],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_array_get",
        &[ptr_type, types::I64],
        Some(types::I64),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_array_set",
        &[ptr_type, types::I64, types::I64],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_array_len",
        &[ptr_type],
        Some(types::I64),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_array_push",
        &[ptr_type, types::I64],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_str_len",
        &[ptr_type],
        Some(types::I64),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_struct_alloc",
        &[types::I64],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_i64_to_str",
        &[types::I64],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_f64_to_str",
        &[types::F64],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_bool_to_str",
        &[types::I8],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_result_ok",
        &[types::I64],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_result_err",
        &[types::I64],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_result_tag",
        &[ptr_type],
        Some(types::I64),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_result_value",
        &[ptr_type],
        Some(types::I64),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_option_some",
        &[types::I64],
        Some(ptr_type),
    )?;
    declare_rt_fn(module, &mut rt_fns, "rt_option_none", &[], Some(ptr_type))?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_option_tag",
        &[ptr_type],
        Some(types::I64),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_option_value",
        &[ptr_type],
        Some(types::I64),
    )?;
    // Stdlib runtime declarations
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_str_split",
        &[ptr_type, ptr_type],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_str_trim",
        &[ptr_type],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_str_upper",
        &[ptr_type],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_str_lower",
        &[ptr_type],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_str_starts_with",
        &[ptr_type, ptr_type],
        Some(types::I8),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_str_ends_with",
        &[ptr_type, ptr_type],
        Some(types::I8),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_str_replace",
        &[ptr_type, ptr_type, ptr_type],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_str_char_at",
        &[ptr_type, types::I64],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_str_contains",
        &[ptr_type, ptr_type],
        Some(types::I8),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_str_index_of",
        &[ptr_type, ptr_type],
        Some(types::I64),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_str_join",
        &[ptr_type, ptr_type],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_str_repeat",
        &[ptr_type, types::I64],
        Some(ptr_type),
    )?;
    declare_rt_fn(module, &mut rt_fns, "rt_read_line", &[], Some(ptr_type))?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_read_file",
        &[ptr_type],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_write_file",
        &[ptr_type, ptr_type],
        None,
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_pow",
        &[types::I64, types::I64],
        Some(types::I64),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_sqrt",
        &[types::F64],
        Some(types::F64),
    )?;
    // Async runtime declarations
    declare_rt_fn(module, &mut rt_fns, "rt_sleep_ms", &[types::I64], None)?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_spawn_with_args",
        &[ptr_type, ptr_type],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_await_handle",
        &[ptr_type],
        Some(types::I64),
    )?;
    // HTTP + JSON runtime declarations
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_http_get",
        &[ptr_type],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_http_post",
        &[ptr_type, ptr_type],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_json_get",
        &[ptr_type, ptr_type],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_json_stringify",
        &[ptr_type, ptr_type],
        Some(ptr_type),
    )?;
    // HTTP server runtime declarations
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_http_server",
        &[types::I64],
        Some(types::I64),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_http_route",
        &[types::I64, ptr_type, ptr_type, ptr_type, ptr_type],
        None,
    )?;
    declare_rt_fn(module, &mut rt_fns, "rt_http_listen", &[types::I64], None)?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_respond",
        &[types::I64, ptr_type],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_request_body",
        &[ptr_type],
        Some(ptr_type),
    )?;
    // Channel runtime declarations
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_channel_create",
        &[],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_channel_send",
        &[ptr_type, types::I64],
        None,
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_channel_recv",
        &[ptr_type],
        Some(types::I64),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_channel_clone_sender",
        &[ptr_type],
        Some(ptr_type),
    )?;
    // Mutex runtime declarations
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_mutex_create",
        &[types::I64],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_mutex_get",
        &[ptr_type],
        Some(types::I64),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_mutex_set",
        &[ptr_type, types::I64],
        None,
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_mutex_clone",
        &[ptr_type],
        Some(ptr_type),
    )?;
    // HashMap runtime declarations
    declare_rt_fn(module, &mut rt_fns, "rt_hashmap_new", &[], Some(ptr_type))?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_hashmap_set",
        &[ptr_type, ptr_type, ptr_type],
        None,
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_hashmap_get",
        &[ptr_type, ptr_type],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_hashmap_has",
        &[ptr_type, ptr_type],
        Some(types::I8),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_hashmap_len",
        &[ptr_type],
        Some(types::I64),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_hashmap_keys",
        &[ptr_type],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_hashmap_remove",
        &[ptr_type, ptr_type],
        None,
    )?;
    // ARC runtime declarations
    declare_rt_fn(module, &mut rt_fns, "rt_retain", &[ptr_type], None)?;
    declare_rt_fn(module, &mut rt_fns, "rt_release", &[ptr_type], None)?;

    // Build enum variants map
    let mut enum_variants: HashMap<String, Vec<String>> = HashMap::new();
    let mut enum_variant_fields: HashMap<(String, String), Vec<TurboTy>> = HashMap::new();
    let mut enum_max_slots: HashMap<String, usize> = HashMap::new();
    for item in &ast_module.items {
        if let Item::Enum(e) = &item.node {
            let variant_names: Vec<String> = e.variants.iter().map(|v| v.name.clone()).collect();
            let tp_names: Vec<String> = e.type_param_names();
            let mut max_fields: usize = 0;
            for v in &e.variants {
                let field_tys: Vec<TurboTy> = v
                    .fields
                    .iter()
                    .map(|f| {
                        turbo_ty_from_type_expr_with_params(&f.node, &enum_variants, &tp_names)
                    })
                    .collect();
                if !field_tys.is_empty() {
                    max_fields = max_fields.max(field_tys.len());
                    enum_variant_fields.insert((e.name.clone(), v.name.clone()), field_tys);
                }
            }
            if max_fields > 0 {
                enum_max_slots.insert(e.name.clone(), max_fields);
            }
            enum_variants.insert(e.name.clone(), variant_names);
        }
    }

    // Build struct field layouts from AST
    let mut struct_fields: HashMap<String, Vec<(String, TurboTy)>> = HashMap::new();
    for item in &ast_module.items {
        let Item::Struct(s) = &item.node else {
            continue;
        };
        let tp_names: Vec<String> = s.type_param_names();
        let fields: Vec<(String, TurboTy)> = s
            .fields
            .iter()
            .map(|f| {
                (
                    f.name.clone(),
                    turbo_ty_from_type_expr_with_params(&f.ty.node, &enum_variants, &tp_names),
                )
            })
            .collect();
        struct_fields.insert(s.name.clone(), fields);
    }

    // Build struct derives map from AST
    let mut struct_derives: HashMap<String, Vec<String>> = HashMap::new();
    for item in &ast_module.items {
        let Item::Struct(s) = &item.node else {
            continue;
        };
        if !s.derives.is_empty() {
            struct_derives.insert(s.name.clone(), s.derives.clone());
        }
    }

    // Build agent definitions map from AST
    let mut agent_defs: HashMap<String, (String, Vec<String>, Option<String>)> = HashMap::new();
    for item in &ast_module.items {
        if let Item::Agent(agent) = &item.node {
            agent_defs.insert(
                agent.name.clone(),
                (
                    agent.model.clone(),
                    agent.tools.clone(),
                    agent.system_prompt.clone(),
                ),
            );
        }
    }

    // Build constants map from AST
    let mut constants_map: HashMap<String, Spanned<Expr>> = HashMap::new();
    for item in &ast_module.items {
        if let Item::Const(c) = &item.node {
            constants_map.insert(c.name.clone(), c.value.clone());
        }
    }

    // Build trait implementations map: type_name -> vec of trait names
    let mut trait_impls: HashMap<String, Vec<String>> = HashMap::new();
    for item in &ast_module.items {
        if let Item::Impl(imp) = &item.node {
            if let Some(trait_name) = &imp.trait_name {
                trait_impls
                    .entry(imp.type_name.clone())
                    .or_default()
                    .push(trait_name.clone());
            }
        }
    }
    // Also register @derive(Display) as Display trait impl
    for item in &ast_module.items {
        if let Item::Struct(s) = &item.node {
            if s.derives.contains(&"Display".to_string()) {
                let already = trait_impls
                    .get(&s.name)
                    .is_some_and(|impls| impls.contains(&"Display".to_string()));
                if !already {
                    trait_impls
                        .entry(s.name.clone())
                        .or_default()
                        .push("Display".to_string());
                }
            }
        }
    }

    // Build trait definitions map: trait_name -> TraitDef (for default method bodies)
    let mut trait_defs: HashMap<String, &turbo_ast::TraitDef> = HashMap::new();
    for item in &ast_module.items {
        if let Item::Trait(t) = &item.node {
            trait_defs.insert(t.name.clone(), t);
        }
    }

    // Declare all user functions + build return type map
    let mut user_fns: HashMap<String, FuncId> = HashMap::new();
    let mut fn_ret_types: HashMap<String, TurboTy> = HashMap::new();

    for item in &ast_module.items {
        let Item::Function(f) = &item.node else {
            continue;
        };
        let mut sig = module.make_signature();
        // Use fast calling convention for internal functions (not main)
        // — reduces prologue/epilogue overhead on the hot recursive path
        if f.name != "main" {
            sig.call_conv = CallConv::Fast;
        }
        for param in &f.params {
            sig.params.push(AbiParam::new(resolve_cl_type(
                &param.ty.node,
                ptr_type,
                &enum_variants,
                &f.type_param_names(),
            )?));
        }
        let ret_turbo = if let Some(ret_ty) = &f.return_type {
            let cl = resolve_cl_type(
                &ret_ty.node,
                ptr_type,
                &enum_variants,
                &f.type_param_names(),
            )?;
            sig.returns.push(AbiParam::new(cl));
            turbo_ty_from_type_expr_with_params(&ret_ty.node, &enum_variants, &f.type_param_names())
        } else {
            TurboTy::Unit
        };
        let linkage = if f.name == "main" {
            main_linkage
        } else {
            Linkage::Local
        };
        let sym_name = if f.name == "main" && rename_main {
            "turbo_main"
        } else {
            &f.name
        };
        let id = module
            .declare_function(sym_name, linkage, &sig)
            .map_err(|e| CodegenError {
                code: ErrorCode::E0405,
                message: e.to_string(),
            })?;
        user_fns.insert(f.name.clone(), id);
        fn_ret_types.insert(f.name.clone(), ret_turbo);
    }

    // Build fn_asts map for inline expansion and fn_type_params for generics
    let mut fn_asts: HashMap<String, &FnDef> = HashMap::new();
    let mut fn_type_params: HashMap<String, Vec<String>> = HashMap::new();
    for item in &ast_module.items {
        let Item::Function(f) = &item.node else {
            continue;
        };
        fn_asts.insert(f.name.clone(), f);
        fn_type_params.insert(f.name.clone(), f.type_param_names());
    }

    // Declare all methods from impl blocks
    for item in &ast_module.items {
        let Item::Impl(imp) = &item.node else {
            continue;
        };
        for method_spanned in &imp.methods {
            let method = &method_spanned.node;
            let mangled = format!("{}__{}", imp.type_name, method.name);

            let mut sig = module.make_signature();
            sig.call_conv = CallConv::Fast;

            for param in &method.params {
                if param.name == "self" {
                    sig.params.push(AbiParam::new(ptr_type));
                } else {
                    sig.params.push(AbiParam::new(resolve_cl_type(
                        &param.ty.node,
                        ptr_type,
                        &enum_variants,
                        &[],
                    )?));
                }
            }

            let ret_turbo = if let Some(ret_ty) = &method.return_type {
                let cl = resolve_cl_type(&ret_ty.node, ptr_type, &enum_variants, &[])?;
                sig.returns.push(AbiParam::new(cl));
                turbo_ty_from_type_expr(&ret_ty.node, &enum_variants)
            } else {
                TurboTy::Unit
            };

            let id = module
                .declare_function(&mangled, Linkage::Local, &sig)
                .map_err(|e| CodegenError {
                    code: ErrorCode::E0405,
                    message: e.to_string(),
                })?;
            user_fns.insert(mangled.clone(), id);
            fn_ret_types.insert(mangled, ret_turbo);
        }
    }

    // Declare default trait methods for impl blocks that don't override them
    // Collect (type_name, method_sig) pairs for default methods that need compilation
    let mut default_method_impls: Vec<(String, &turbo_ast::TraitMethodSig)> = Vec::new();
    for item in &ast_module.items {
        let Item::Impl(imp) = &item.node else {
            continue;
        };
        let Some(trait_name) = &imp.trait_name else {
            continue;
        };
        let Some(trait_def) = trait_defs.get(trait_name.as_str()) else {
            continue;
        };
        let impl_method_names: Vec<String> =
            imp.methods.iter().map(|m| m.node.name.clone()).collect();
        for trait_method in &trait_def.methods {
            if trait_method.default_body.is_some()
                && !impl_method_names.contains(&trait_method.name)
            {
                let mangled = format!("{}__{}", imp.type_name, trait_method.name);
                if !user_fns.contains_key(&mangled) {
                    let mut sig = module.make_signature();
                    sig.call_conv = CallConv::Fast;
                    for param in &trait_method.params {
                        if param.name == "self" {
                            sig.params.push(AbiParam::new(ptr_type));
                        } else {
                            sig.params.push(AbiParam::new(resolve_cl_type(
                                &param.ty.node,
                                ptr_type,
                                &enum_variants,
                                &[],
                            )?));
                        }
                    }
                    let ret_turbo = if let Some(ret_ty) = &trait_method.return_type {
                        let cl = resolve_cl_type(&ret_ty.node, ptr_type, &enum_variants, &[])?;
                        sig.returns.push(AbiParam::new(cl));
                        turbo_ty_from_type_expr(&ret_ty.node, &enum_variants)
                    } else {
                        TurboTy::Unit
                    };
                    let id = module
                        .declare_function(&mangled, Linkage::Local, &sig)
                        .map_err(|e| CodegenError {
                            code: ErrorCode::E0400,
                            message: e.to_string(),
                        })?;
                    user_fns.insert(mangled.clone(), id);
                    fn_ret_types.insert(mangled, ret_turbo);
                    default_method_impls.push((imp.type_name.clone(), trait_method));
                }
            }
        }
    }

    // Declare @derive(Display) auto-generated to_string methods
    let mut derive_display_structs: Vec<String> = Vec::new();
    for item in &ast_module.items {
        let Item::Struct(s) = &item.node else {
            continue;
        };
        if s.derives.contains(&"Display".to_string()) {
            let mangled = format!("{}__{}", s.name, "to_string");
            if !user_fns.contains_key(&mangled) {
                let mut sig = module.make_signature();
                sig.call_conv = CallConv::Fast;
                sig.params.push(AbiParam::new(ptr_type)); // self
                sig.returns.push(AbiParam::new(ptr_type)); // returns str
                let id = module
                    .declare_function(&mangled, Linkage::Local, &sig)
                    .map_err(|e| CodegenError {
                        code: ErrorCode::E0405,
                        message: e.to_string(),
                    })?;
                user_fns.insert(mangled.clone(), id);
                fn_ret_types.insert(mangled, TurboTy::Str);
                derive_display_structs.push(s.name.clone());
            }
        }
    }

    // Extract and compile closures
    let extracted_closures = extract_all_closures(ast_module);
    let mut closure_fns_map: HashMap<usize, (String, TurboTy, Vec<String>)> = HashMap::new();
    let mut closure_captures_map: HashMap<usize, CaptureInfo> = HashMap::new();

    // Declare all closure functions (with env_ptr as first hidden parameter)
    for closure in &extracted_closures {
        let mut sig = module.make_signature();
        sig.call_conv = CallConv::Fast;
        // First parameter is always the env pointer (hidden from user)
        sig.params.push(AbiParam::new(ptr_type));
        let mut param_turbo_tys = Vec::new();
        for param in closure.params.iter() {
            sig.params.push(AbiParam::new(resolve_cl_type(
                &param.ty.node,
                ptr_type,
                &enum_variants,
                &[],
            )?));
            param_turbo_tys.push(turbo_ty_from_type_expr(&param.ty.node, &enum_variants));
        }
        let ret_turbo = if let Some(ret_ty) = closure.return_type {
            let cl = resolve_cl_type(&ret_ty.node, ptr_type, &enum_variants, &[])?;
            sig.returns.push(AbiParam::new(cl));
            turbo_ty_from_type_expr(&ret_ty.node, &enum_variants)
        } else {
            let has_inferred_params = closure
                .params
                .iter()
                .any(|p| matches!(p.ty.node, TypeExpr::Inferred));
            if has_inferred_params {
                sig.returns.push(AbiParam::new(types::I64));
                TurboTy::Int
            } else {
                TurboTy::Unit
            }
        };
        let id = module
            .declare_function(&closure.name, Linkage::Local, &sig)
            .map_err(|e| CodegenError {
                code: ErrorCode::E0405,
                message: e.to_string(),
            })?;
        user_fns.insert(closure.name.clone(), id);
        fn_ret_types.insert(closure.name.clone(), ret_turbo.clone());
        closure_fns_map.insert(
            closure.span_start,
            (
                closure.name.clone(),
                TurboTy::Fn(param_turbo_tys, Box::new(ret_turbo)),
                closure.free_vars.clone(),
            ),
        );
    }

    // Extract and declare spawn thunks
    let spawn_sites = extract_all_spawn_sites(ast_module);
    let mut spawn_thunk_map: HashMap<usize, String> = HashMap::new();

    for site in &spawn_sites {
        // Each spawn thunk: takes a pointer to args struct, returns i64.
        // Uses default (SystemV/C ABI) calling convention so rt_spawn_thunk can call it.
        let mut sig = module.make_signature();
        // Single parameter: pointer to args struct [fn_ptr, arg0, arg1, ...]
        sig.params.push(AbiParam::new(ptr_type));
        sig.returns.push(AbiParam::new(types::I64));
        let id = module
            .declare_function(&site.thunk_name, Linkage::Local, &sig)
            .map_err(|e| CodegenError {
                code: ErrorCode::E0405,
                message: e.to_string(),
            })?;
        user_fns.insert(site.thunk_name.clone(), id);
        fn_ret_types.insert(site.thunk_name.clone(), TurboTy::Int);
        spawn_thunk_map.insert(site.span_start, site.thunk_name.clone());
    }

    // Define all user functions (and closures)
    let mut cl_ctx = module.make_context();
    let mut data_desc = DataDescription::new();
    let mut string_counter: usize = 0;

    // NOTE: Closure bodies are compiled AFTER function bodies, so that
    // Expr::Closure can determine capture types from the enclosing scope.

    for item in &ast_module.items {
        let Item::Function(f) = &item.node else {
            continue;
        };
        let func_id = user_fns[&f.name];

        cl_ctx.func.signature = module.make_signature();
        if f.name != "main" {
            cl_ctx.func.signature.call_conv = CallConv::Fast;
        }
        for param in &f.params {
            cl_ctx
                .func
                .signature
                .params
                .push(AbiParam::new(resolve_cl_type(
                    &param.ty.node,
                    ptr_type,
                    &enum_variants,
                    &f.type_param_names(),
                )?));
        }
        if let Some(ret_ty) = &f.return_type {
            cl_ctx
                .func
                .signature
                .returns
                .push(AbiParam::new(resolve_cl_type(
                    &ret_ty.node,
                    ptr_type,
                    &enum_variants,
                    &f.type_param_names(),
                )?));
        }

        let mut fn_ctx = FunctionBuilderContext::new();
        {
            let builder = FunctionBuilder::new(&mut cl_ctx.func, &mut fn_ctx);
            let mut cx = Ctx {
                builder,
                module,
                user_fns: &user_fns,
                fn_ret_types: &fn_ret_types,
                fn_asts: &fn_asts,
                fn_type_params: &fn_type_params,
                rt_fns: &rt_fns,
                vars: HashMap::new(),
                next_var: 0,
                data_desc: &mut data_desc,
                string_counter: &mut string_counter,
                ptr_type,
                struct_fields: &struct_fields,
                enum_variants: &enum_variants,
                enum_variant_fields: &enum_variant_fields,
                enum_max_slots: &enum_max_slots,
                closure_fns: &closure_fns_map,
                trait_impls: &trait_impls,
                inline_depth: 0,
                closure_captures: &mut closure_captures_map,
                generic_struct_field_overrides: HashMap::new(),
                last_struct_lit_concrete_fields: None,
                agent_defs: &agent_defs,
                spawn_thunks: &spawn_thunk_map,
                constants: &constants_map,
                struct_derives: &struct_derives,
                loop_stack: Vec::new(),
            };

            let entry = cx.builder.create_block();
            cx.builder.append_block_params_for_function_params(entry);
            cx.builder.switch_to_block(entry);
            cx.builder.seal_block(entry);

            // Ensure the entry block is always in the layout, even if the
            // function body is empty (no instructions). Without this,
            // FunctionBuilder::is_unreachable() misidentifies the entry block
            // as unreachable because layout.entry_block() returns None.
            cx.builder.ensure_inserted_block();

            // Define parameters as variables
            for (i, param) in f.params.iter().enumerate() {
                let cl_ty = resolve_cl_type(
                    &param.ty.node,
                    ptr_type,
                    &enum_variants,
                    &f.type_param_names(),
                )?;
                let turbo_ty = turbo_ty_from_type_expr_with_params(
                    &param.ty.node,
                    &enum_variants,
                    &f.type_param_names(),
                );
                let var = cx.fresh_var(cl_ty, turbo_ty.clone());
                let val = cx.builder.block_params(entry)[i];
                cx.builder.def_var(var, val);
                cx.vars.insert(param.name.clone(), (var, cl_ty, turbo_ty));
            }

            let result = compile_expr(&mut cx, &f.body)?;

            if !cx.builder.is_unreachable() {
                if f.return_type.is_some() {
                    if let Some((val, _)) = result {
                        cx.builder.ins().return_(&[val]);
                    } else {
                        // Function claims to return a value but body returns unit.
                        // This should have been caught by sema; emit a trap as a safety net.
                        cx.builder.ins().trap(TrapCode::unwrap_user(1));
                    }
                } else {
                    cx.builder.ins().return_(&[]);
                }
            }

            cx.builder.finalize();
        }

        module
            .define_function(func_id, &mut cl_ctx)
            .map_err(|e| CodegenError {
                code: ErrorCode::E0405,
                message: e.to_string(),
            })?;
        module.clear_context(&mut cl_ctx);
    }

    // Define all methods from impl blocks
    for item in &ast_module.items {
        let Item::Impl(imp) = &item.node else {
            continue;
        };
        for method_spanned in &imp.methods {
            let method = &method_spanned.node;
            let mangled = format!("{}__{}", imp.type_name, method.name);
            let func_id = user_fns[&mangled];

            cl_ctx.func.signature = module.make_signature();
            cl_ctx.func.signature.call_conv = CallConv::Fast;

            for param in &method.params {
                if param.name == "self" {
                    cl_ctx.func.signature.params.push(AbiParam::new(ptr_type));
                } else {
                    cl_ctx
                        .func
                        .signature
                        .params
                        .push(AbiParam::new(resolve_cl_type(
                            &param.ty.node,
                            ptr_type,
                            &enum_variants,
                            &[],
                        )?));
                }
            }
            if let Some(ret_ty) = &method.return_type {
                cl_ctx
                    .func
                    .signature
                    .returns
                    .push(AbiParam::new(resolve_cl_type(
                        &ret_ty.node,
                        ptr_type,
                        &enum_variants,
                        &[],
                    )?));
            }

            let mut fn_ctx = FunctionBuilderContext::new();
            {
                let builder = FunctionBuilder::new(&mut cl_ctx.func, &mut fn_ctx);
                let mut cx = Ctx {
                    builder,
                    module,
                    user_fns: &user_fns,
                    fn_ret_types: &fn_ret_types,
                    fn_asts: &fn_asts,
                    fn_type_params: &fn_type_params,
                    rt_fns: &rt_fns,
                    vars: HashMap::new(),
                    next_var: 0,
                    data_desc: &mut data_desc,
                    string_counter: &mut string_counter,
                    ptr_type,
                    struct_fields: &struct_fields,
                    enum_variants: &enum_variants,
                    enum_variant_fields: &enum_variant_fields,
                    enum_max_slots: &enum_max_slots,
                    closure_fns: &closure_fns_map,
                    trait_impls: &trait_impls,
                    inline_depth: 0,
                    closure_captures: &mut closure_captures_map,
                    generic_struct_field_overrides: HashMap::new(),
                    last_struct_lit_concrete_fields: None,
                    agent_defs: &agent_defs,
                    spawn_thunks: &spawn_thunk_map,
                    constants: &constants_map,
                    struct_derives: &struct_derives,
                    loop_stack: Vec::new(),
                };

                let entry = cx.builder.create_block();
                cx.builder.append_block_params_for_function_params(entry);
                cx.builder.switch_to_block(entry);
                cx.builder.seal_block(entry);
                cx.builder.ensure_inserted_block();

                // Define parameters as variables
                for (i, param) in method.params.iter().enumerate() {
                    let (cl_ty, turbo_ty) = if param.name == "self" {
                        (ptr_type, TurboTy::Struct(imp.type_name.clone()))
                    } else {
                        let cl_ty = resolve_cl_type(&param.ty.node, ptr_type, &enum_variants, &[])?;
                        let turbo_ty = turbo_ty_from_type_expr(&param.ty.node, &enum_variants);
                        (cl_ty, turbo_ty)
                    };
                    let var = cx.fresh_var(cl_ty, turbo_ty.clone());
                    let val = cx.builder.block_params(entry)[i];
                    cx.builder.def_var(var, val);
                    cx.vars.insert(param.name.clone(), (var, cl_ty, turbo_ty));
                }

                let result = compile_expr(&mut cx, &method.body)?;

                if !cx.builder.is_unreachable() {
                    if method.return_type.is_some() {
                        if let Some((val, _)) = result {
                            cx.builder.ins().return_(&[val]);
                        } else {
                            cx.builder.ins().trap(TrapCode::unwrap_user(1));
                        }
                    } else {
                        cx.builder.ins().return_(&[]);
                    }
                }

                cx.builder.finalize();
            }

            module
                .define_function(func_id, &mut cl_ctx)
                .map_err(|e| CodegenError {
                    code: ErrorCode::E0405,
                    message: e.to_string(),
                })?;
            module.clear_context(&mut cl_ctx);
        }
    }

    // Define default trait method bodies
    for (type_name, trait_method) in &default_method_impls {
        let mangled = format!("{}__{}", type_name, trait_method.name);
        let func_id = user_fns[&mangled];
        let default_body = trait_method.default_body.as_ref().unwrap();

        cl_ctx.func.signature = module.make_signature();
        cl_ctx.func.signature.call_conv = CallConv::Fast;

        for param in &trait_method.params {
            if param.name == "self" {
                cl_ctx.func.signature.params.push(AbiParam::new(ptr_type));
            } else {
                cl_ctx
                    .func
                    .signature
                    .params
                    .push(AbiParam::new(resolve_cl_type(
                        &param.ty.node,
                        ptr_type,
                        &enum_variants,
                        &[],
                    )?));
            }
        }
        if let Some(ret_ty) = &trait_method.return_type {
            cl_ctx
                .func
                .signature
                .returns
                .push(AbiParam::new(resolve_cl_type(
                    &ret_ty.node,
                    ptr_type,
                    &enum_variants,
                    &[],
                )?));
        }

        let mut fn_ctx = FunctionBuilderContext::new();
        {
            let builder = FunctionBuilder::new(&mut cl_ctx.func, &mut fn_ctx);
            let mut cx = Ctx {
                builder,
                module,
                user_fns: &user_fns,
                fn_ret_types: &fn_ret_types,
                fn_asts: &fn_asts,
                fn_type_params: &fn_type_params,
                rt_fns: &rt_fns,
                vars: HashMap::new(),
                next_var: 0,
                data_desc: &mut data_desc,
                string_counter: &mut string_counter,
                ptr_type,
                struct_fields: &struct_fields,
                enum_variants: &enum_variants,
                enum_variant_fields: &enum_variant_fields,
                enum_max_slots: &enum_max_slots,
                closure_fns: &closure_fns_map,
                trait_impls: &trait_impls,
                inline_depth: 0,
                closure_captures: &mut closure_captures_map,
                generic_struct_field_overrides: HashMap::new(),
                last_struct_lit_concrete_fields: None,
                agent_defs: &agent_defs,
                spawn_thunks: &spawn_thunk_map,
                constants: &constants_map,
                struct_derives: &struct_derives,
                loop_stack: Vec::new(),
            };

            let entry = cx.builder.create_block();
            cx.builder.append_block_params_for_function_params(entry);
            cx.builder.switch_to_block(entry);
            cx.builder.seal_block(entry);
            cx.builder.ensure_inserted_block();

            // Define parameters as variables
            for (i, param) in trait_method.params.iter().enumerate() {
                let (cl_ty, turbo_ty) = if param.name == "self" {
                    (ptr_type, TurboTy::Struct(type_name.clone()))
                } else {
                    let cl_ty = resolve_cl_type(&param.ty.node, ptr_type, &enum_variants, &[])?;
                    let turbo_ty = turbo_ty_from_type_expr(&param.ty.node, &enum_variants);
                    (cl_ty, turbo_ty)
                };
                let var = cx.fresh_var(cl_ty, turbo_ty.clone());
                let val = cx.builder.block_params(entry)[i];
                cx.builder.def_var(var, val);
                cx.vars.insert(param.name.clone(), (var, cl_ty, turbo_ty));
            }

            let result = compile_expr(&mut cx, default_body)?;

            if !cx.builder.is_unreachable() {
                if trait_method.return_type.is_some() {
                    if let Some((val, _)) = result {
                        cx.builder.ins().return_(&[val]);
                    } else {
                        cx.builder.ins().trap(TrapCode::unwrap_user(1));
                    }
                } else {
                    cx.builder.ins().return_(&[]);
                }
            }

            cx.builder.finalize();
        }

        module
            .define_function(func_id, &mut cl_ctx)
            .map_err(|e| CodegenError {
                code: ErrorCode::E0405,
                message: e.to_string(),
            })?;
        module.clear_context(&mut cl_ctx);
    }

    // Define @derive(Display) auto-generated to_string methods
    for struct_name in &derive_display_structs {
        let mangled = format!("{}__{}", struct_name, "to_string");
        let func_id = user_fns[&mangled];

        cl_ctx.func.signature = module.make_signature();
        cl_ctx.func.signature.call_conv = CallConv::Fast;
        cl_ctx.func.signature.params.push(AbiParam::new(ptr_type)); // self
        cl_ctx.func.signature.returns.push(AbiParam::new(ptr_type)); // returns str

        let mut fn_ctx = FunctionBuilderContext::new();
        {
            let builder = FunctionBuilder::new(&mut cl_ctx.func, &mut fn_ctx);
            let mut cx = Ctx {
                builder,
                module,
                user_fns: &user_fns,
                fn_ret_types: &fn_ret_types,
                fn_asts: &fn_asts,
                fn_type_params: &fn_type_params,
                rt_fns: &rt_fns,
                vars: HashMap::new(),
                next_var: 0,
                data_desc: &mut data_desc,
                string_counter: &mut string_counter,
                ptr_type,
                struct_fields: &struct_fields,
                enum_variants: &enum_variants,
                enum_variant_fields: &enum_variant_fields,
                enum_max_slots: &enum_max_slots,
                closure_fns: &closure_fns_map,
                trait_impls: &trait_impls,
                inline_depth: 0,
                closure_captures: &mut closure_captures_map,
                generic_struct_field_overrides: HashMap::new(),
                last_struct_lit_concrete_fields: None,
                agent_defs: &agent_defs,
                spawn_thunks: &spawn_thunk_map,
                constants: &constants_map,
                struct_derives: &struct_derives,
                loop_stack: Vec::new(),
            };

            let entry = cx.builder.create_block();
            cx.builder.append_block_params_for_function_params(entry);
            cx.builder.switch_to_block(entry);
            cx.builder.seal_block(entry);
            cx.builder.ensure_inserted_block();

            // Self is the first param
            let self_val = cx.builder.block_params(entry)[0];

            // Build "StructName { field1: val1, field2: val2 }" string
            let fields = struct_fields
                .get(struct_name.as_str())
                .cloned()
                .unwrap_or_default();

            // Start with "StructName { "
            let mut result = cx.create_string(&format!("{} {{ ", struct_name))?;

            let concat_fid = cx.rt_fns["rt_str_concat"];

            for (i, (field_name, field_ty)) in fields.iter().enumerate() {
                // Add "field_name: "
                let prefix = if i > 0 {
                    format!(", {}: ", field_name)
                } else {
                    format!("{}: ", field_name)
                };
                let prefix_str = cx.create_string(&prefix)?;
                let concat_ref = cx.module.declare_func_in_func(concat_fid, cx.builder.func);
                let call = cx.builder.ins().call(concat_ref, &[result, prefix_str]);
                result = cx.builder.inst_results(call)[0];

                // Load field value from struct
                let offset = (i * 8) as i32;
                let raw_val = cx
                    .builder
                    .ins()
                    .load(types::I64, MemFlags::new(), self_val, offset);

                // Convert field value to string based on type
                let field_str = match field_ty {
                    TurboTy::Int => {
                        let fid = cx.rt_fns["rt_i64_to_str"];
                        let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
                        let call = cx.builder.ins().call(fref, &[raw_val]);
                        cx.builder.inst_results(call)[0]
                    }
                    TurboTy::Str => {
                        // raw_val is already a pointer to a string
                        raw_val
                    }
                    TurboTy::Bool => {
                        let bool_val = cx.builder.ins().ireduce(types::I8, raw_val);
                        let fid = cx.rt_fns["rt_bool_to_str"];
                        let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
                        let call = cx.builder.ins().call(fref, &[bool_val]);
                        cx.builder.inst_results(call)[0]
                    }
                    TurboTy::Float => {
                        let float_val =
                            cx.builder
                                .ins()
                                .bitcast(types::F64, MemFlags::new(), raw_val);
                        let fid = cx.rt_fns["rt_f64_to_str"];
                        let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
                        let call = cx.builder.ins().call(fref, &[float_val]);
                        cx.builder.inst_results(call)[0]
                    }
                    _ => {
                        // For other types, just show a placeholder
                        cx.create_string("...")?
                    }
                };

                // Concat field value string
                let concat_ref = cx.module.declare_func_in_func(concat_fid, cx.builder.func);
                let call = cx.builder.ins().call(concat_ref, &[result, field_str]);
                result = cx.builder.inst_results(call)[0];
            }

            // Add closing " }"
            let suffix = cx.create_string(" }")?;
            let concat_ref = cx.module.declare_func_in_func(concat_fid, cx.builder.func);
            let call = cx.builder.ins().call(concat_ref, &[result, suffix]);
            result = cx.builder.inst_results(call)[0];

            cx.builder.ins().return_(&[result]);

            cx.builder.finalize();
        }

        module
            .define_function(func_id, &mut cl_ctx)
            .map_err(|e| CodegenError {
                code: ErrorCode::E0405,
                message: e.to_string(),
            })?;
        module.clear_context(&mut cl_ctx);
    }

    // Compile closure function bodies (after function bodies, so capture info is available)
    for closure in &extracted_closures {
        let func_id = user_fns[&closure.name];

        cl_ctx.func.signature = module.make_signature();
        cl_ctx.func.signature.call_conv = CallConv::Fast;
        // First parameter is always the env pointer
        cl_ctx.func.signature.params.push(AbiParam::new(ptr_type));
        for param in closure.params.iter() {
            cl_ctx
                .func
                .signature
                .params
                .push(AbiParam::new(resolve_cl_type(
                    &param.ty.node,
                    ptr_type,
                    &enum_variants,
                    &[],
                )?));
        }
        if let Some(ret_ty) = closure.return_type {
            cl_ctx
                .func
                .signature
                .returns
                .push(AbiParam::new(resolve_cl_type(
                    &ret_ty.node,
                    ptr_type,
                    &enum_variants,
                    &[],
                )?));
        } else {
            // For closures with inferred params, add i64 return to match the declaration
            let has_inferred_params = closure
                .params
                .iter()
                .any(|p| matches!(p.ty.node, TypeExpr::Inferred));
            if has_inferred_params {
                cl_ctx
                    .func
                    .signature
                    .returns
                    .push(AbiParam::new(types::I64));
            }
        }

        let mut fn_ctx = FunctionBuilderContext::new();
        {
            let builder = FunctionBuilder::new(&mut cl_ctx.func, &mut fn_ctx);
            let mut cx = Ctx {
                builder,
                module,
                user_fns: &user_fns,
                fn_ret_types: &fn_ret_types,
                fn_asts: &fn_asts,
                fn_type_params: &fn_type_params,
                rt_fns: &rt_fns,
                vars: HashMap::new(),
                next_var: 0,
                data_desc: &mut data_desc,
                string_counter: &mut string_counter,
                ptr_type,
                struct_fields: &struct_fields,
                enum_variants: &enum_variants,
                enum_variant_fields: &enum_variant_fields,
                enum_max_slots: &enum_max_slots,
                closure_fns: &closure_fns_map,
                trait_impls: &trait_impls,
                inline_depth: 0,
                closure_captures: &mut closure_captures_map,
                generic_struct_field_overrides: HashMap::new(),
                last_struct_lit_concrete_fields: None,
                agent_defs: &agent_defs,
                spawn_thunks: &spawn_thunk_map,
                constants: &constants_map,
                struct_derives: &struct_derives,
                loop_stack: Vec::new(),
            };

            let entry = cx.builder.create_block();
            cx.builder.append_block_params_for_function_params(entry);
            cx.builder.switch_to_block(entry);
            cx.builder.seal_block(entry);

            // Block param 0 is the env pointer
            let env_ptr_val = cx.builder.block_params(entry)[0];

            // Load captured variables from the environment struct
            let capture_info = cx.closure_captures.get(&closure.span_start).cloned();
            if let Some(ref info) = capture_info {
                for (cap_idx, (cap_name, cap_tty)) in info.captures.iter().enumerate() {
                    let cl_ty = turbo_ty_to_cl_type(cap_tty, ptr_type);
                    let var = cx.fresh_var(cl_ty, cap_tty.clone());
                    let offset = (cap_idx * 8) as i32;
                    let raw_val =
                        cx.builder
                            .ins()
                            .load(types::I64, MemFlags::new(), env_ptr_val, offset);
                    let val = match cap_tty {
                        TurboTy::Bool => cx.builder.ins().ireduce(types::I8, raw_val),
                        TurboTy::Float => {
                            cx.builder
                                .ins()
                                .bitcast(types::F64, MemFlags::new(), raw_val)
                        }
                        _ => raw_val,
                    };
                    cx.builder.def_var(var, val);
                    cx.vars
                        .insert(cap_name.clone(), (var, cl_ty, cap_tty.clone()));
                }
            }

            // Define user parameters as variables (shifted by 1 due to env_ptr)
            for (i, param) in closure.params.iter().enumerate() {
                let cl_ty = resolve_cl_type(&param.ty.node, ptr_type, &enum_variants, &[])?;
                let turbo_ty = turbo_ty_from_type_expr(&param.ty.node, &enum_variants);
                let var = cx.fresh_var(cl_ty, turbo_ty.clone());
                let val = cx.builder.block_params(entry)[i + 1]; // +1 for env_ptr
                cx.builder.def_var(var, val);
                cx.vars.insert(param.name.clone(), (var, cl_ty, turbo_ty));
            }

            let result = compile_expr(&mut cx, closure.body)?;

            if !cx.builder.is_unreachable() {
                let has_inferred = closure
                    .params
                    .iter()
                    .any(|p| matches!(p.ty.node, TypeExpr::Inferred));
                if closure.return_type.is_some() || has_inferred {
                    if let Some((val, _)) = result {
                        cx.builder.ins().return_(&[val]);
                    } else {
                        cx.builder.ins().return_(&[]);
                    }
                } else {
                    cx.builder.ins().return_(&[]);
                }
            }

            cx.builder.finalize();
        }

        module
            .define_function(func_id, &mut cl_ctx)
            .map_err(|e| CodegenError {
                code: ErrorCode::E0405,
                message: e.to_string(),
            })?;
        module.clear_context(&mut cl_ctx);
    }

    // Compile spawn thunk function bodies
    // Each thunk: loads fn_ptr + args from an args struct, calls the target, returns result
    for site in &spawn_sites {
        let func_id = user_fns[&site.thunk_name];

        cl_ctx.func.signature = module.make_signature();
        // Default (SystemV/C ABI) calling convention — callable from rt_spawn_thunk
        cl_ctx.func.signature.params.push(AbiParam::new(ptr_type)); // args_struct_ptr
        cl_ctx
            .func
            .signature
            .returns
            .push(AbiParam::new(types::I64));

        let mut fn_ctx = FunctionBuilderContext::new();
        {
            let mut builder = FunctionBuilder::new(&mut cl_ctx.func, &mut fn_ctx);
            let entry = builder.create_block();
            builder.append_block_params_for_function_params(entry);
            builder.switch_to_block(entry);
            builder.seal_block(entry);
            builder.ensure_inserted_block();

            let args_ptr = builder.block_params(entry)[0];

            // Load fn_ptr from offset 0
            let fn_ptr = builder.ins().load(ptr_type, MemFlags::new(), args_ptr, 0);

            // Load each argument from the struct (offset 8, 16, 24, ...)
            let mut arg_vals = Vec::new();
            for i in 0..site.num_args {
                let offset = ((i + 1) * 8) as i32;
                let val = builder
                    .ins()
                    .load(types::I64, MemFlags::new(), args_ptr, offset);
                arg_vals.push(val);
            }

            // Build the call signature for the target function (Fast calling convention)
            let target_func_id = user_fns.get(&site.callee_name);
            if let Some(&target_fid) = target_func_id {
                // Use direct call to the target function — but we can't because
                // the fn_ptr is loaded dynamically. Use call_indirect instead.
                let mut callee_sig = module.make_signature();
                callee_sig.call_conv = CallConv::Fast;
                for _ in 0..site.num_args {
                    callee_sig.params.push(AbiParam::new(types::I64));
                }
                // Check if the target function has a return type
                let has_return = fn_ret_types
                    .get(&site.callee_name)
                    .map(|t| *t != TurboTy::Unit)
                    .unwrap_or(false);
                if has_return {
                    callee_sig.returns.push(AbiParam::new(types::I64));
                }
                let sig_ref = builder.import_signature(callee_sig);
                let call = builder.ins().call_indirect(sig_ref, fn_ptr, &arg_vals);
                let results = builder.inst_results(call);
                if !results.is_empty() {
                    let result = results[0];
                    builder.ins().return_(&[result]);
                } else {
                    let zero = builder.ins().iconst(types::I64, 0);
                    builder.ins().return_(&[zero]);
                }
                let _ = target_fid; // used indirectly via fn_ptr
            } else {
                // Unknown function — return 0
                let zero = builder.ins().iconst(types::I64, 0);
                builder.ins().return_(&[zero]);
            }

            builder.finalize();
        }

        module
            .define_function(func_id, &mut cl_ctx)
            .map_err(|e| CodegenError {
                code: ErrorCode::E0405,
                message: e.to_string(),
            })?;
        module.clear_context(&mut cl_ctx);
    }

    Ok(user_fns)
}

// ── Helpers ─────────────────────────────────────────────────────────

fn declare_rt_fn<M: Module>(
    module: &mut M,
    rt_fns: &mut HashMap<String, FuncId>,
    name: &str,
    params: &[types::Type],
    ret: Option<types::Type>,
) -> Result<(), CodegenError> {
    let mut sig = module.make_signature();
    for &p in params {
        sig.params.push(AbiParam::new(p));
    }
    if let Some(r) = ret {
        sig.returns.push(AbiParam::new(r));
    }
    let id = module
        .declare_function(name, Linkage::Import, &sig)
        .map_err(|e| CodegenError {
            code: ErrorCode::E0405,
            message: e.to_string(),
        })?;
    rt_fns.insert(name.to_string(), id);
    Ok(())
}

fn resolve_cl_type(
    ty: &TypeExpr,
    ptr_type: types::Type,
    enum_variants: &HashMap<String, Vec<String>>,
    type_params: &[String],
) -> Result<types::Type, CodegenError> {
    resolve_cl_type_inner(ty, ptr_type, enum_variants, type_params, &HashMap::new())
}

/// Resolve Cranelift type, accounting for data-carrying enums that need pointer types.
#[allow(dead_code)]
fn resolve_cl_type_with_data(
    ty: &TypeExpr,
    ptr_type: types::Type,
    enum_variants: &HashMap<String, Vec<String>>,
    type_params: &[String],
    enum_max_slots: &HashMap<String, usize>,
) -> Result<types::Type, CodegenError> {
    resolve_cl_type_inner(ty, ptr_type, enum_variants, type_params, enum_max_slots)
}

fn resolve_cl_type_inner(
    ty: &TypeExpr,
    ptr_type: types::Type,
    enum_variants: &HashMap<String, Vec<String>>,
    type_params: &[String],
    enum_max_slots: &HashMap<String, usize>,
) -> Result<types::Type, CodegenError> {
    match ty {
        TypeExpr::Named(name) => {
            // Type parameters are represented as I64 (same size as ptr on 64-bit)
            if type_params.contains(name) {
                return Ok(types::I64);
            }
            match name.as_str() {
                "i32" => Ok(types::I32),
                "i64" => Ok(types::I64),
                "u32" => Ok(types::I32),
                "u64" => Ok(types::I64),
                "f32" => Ok(types::F32),
                "f64" => Ok(types::F64),
                "bool" => Ok(types::I8),
                "str" => Ok(ptr_type),
                _ => {
                    if enum_variants.contains_key(name.as_str()) {
                        // Data-carrying enums are heap-allocated pointers
                        if enum_max_slots.contains_key(name.as_str()) {
                            Ok(ptr_type)
                        } else {
                            Ok(types::I64) // unit-only enums are i64 tags
                        }
                    } else {
                        Ok(ptr_type) // Struct types are represented as pointers at runtime
                    }
                }
            }
        }
        TypeExpr::Unit => Err(CodegenError {
            code: ErrorCode::E0400,
            message: "unit type has no runtime representation".to_string(),
        }),
        TypeExpr::Array(_) => Ok(ptr_type), // Arrays are represented as pointers at runtime
        TypeExpr::FnType { .. } => Ok(ptr_type), // Function pointers are pointers
        TypeExpr::Result { .. } => Ok(ptr_type), // Result types are heap-allocated tagged unions
        TypeExpr::Optional(_) => Ok(ptr_type), // Optional types are heap-allocated tagged unions
        // Sprint 9: Future<T> compiles identically to T
        TypeExpr::Future(inner) => resolve_cl_type_inner(
            &inner.node,
            ptr_type,
            enum_variants,
            type_params,
            enum_max_slots,
        ),
        #[allow(unreachable_patterns)]
        _ => Ok(types::I64),
    }
}

// ── Expression compilation ──────────────────────────────────────────

pub(crate) fn compile_expr<M: Module>(
    cx: &mut Ctx<'_, M>,
    expr: &Spanned<Expr>,
) -> Result<MaybeTyped, CodegenError> {
    match &expr.node {
        Expr::IntLit(n) => {
            let val = cx.builder.ins().iconst(types::I64, *n);
            Ok(Some((val, TurboTy::Int)))
        }
        Expr::FloatLit(f) => {
            let val = cx.builder.ins().f64const(*f);
            Ok(Some((val, TurboTy::Float)))
        }
        Expr::BoolLit(b) => {
            let val = cx.builder.ins().iconst(types::I8, *b as i64);
            Ok(Some((val, TurboTy::Bool)))
        }
        Expr::StringLit(s) => {
            let ptr = cx.create_string(s)?;
            Ok(Some((ptr, TurboTy::Str)))
        }
        Expr::Unit => Ok(None),

        Expr::Ident(name) => {
            // Check if this is a module-level constant — inline the value
            if let Some(const_expr) = cx.constants.get(name.as_str()) {
                let const_expr = const_expr.clone();
                return compile_expr(cx, &const_expr);
            }
            let (var, _cl_ty, turbo_ty) = cx.vars.get(name).ok_or_else(|| CodegenError {
                code: ErrorCode::E0401,
                message: format!("undefined variable: {name}"),
            })?;
            let turbo_ty = turbo_ty.clone();
            let val = cx.builder.use_var(*var);
            Ok(Some((val, turbo_ty)))
        }

        Expr::BinaryOp { left, op, right } => {
            // Short-circuit for && and ||
            if *op == BinOp::And || *op == BinOp::Or {
                return compile_short_circuit(cx, left, *op, right);
            }

            let (lhs, lhs_tty) = compile_expr(cx, left)?.unwrap();
            let (rhs, rhs_tty) = compile_expr(cx, right)?.unwrap();

            // String operations
            if lhs_tty == TurboTy::Str && rhs_tty == TurboTy::Str {
                match op {
                    BinOp::Add => {
                        return compile_str_concat(cx, lhs, rhs);
                    }
                    BinOp::Eq | BinOp::NotEq => {
                        return compile_str_compare(cx, lhs, rhs, *op);
                    }
                    _ => {}
                }
            }

            // Mixed str + non-str is rejected by sema — no implicit coercion

            // Struct field-by-field equality comparison (@derive(Eq))
            if let TurboTy::Struct(ref struct_name) = lhs_tty {
                if matches!(op, BinOp::Eq | BinOp::NotEq) {
                    return compile_struct_eq(cx, lhs, rhs, struct_name, *op);
                }
            }

            let result = compile_binop(cx, lhs, *op, rhs)?;

            // Comparison/logical ops produce Bool, arithmetic preserves input type
            let result_tty = match op {
                BinOp::Eq
                | BinOp::NotEq
                | BinOp::Less
                | BinOp::LessEq
                | BinOp::Greater
                | BinOp::GreaterEq
                | BinOp::And
                | BinOp::Or => TurboTy::Bool,
                _ => lhs_tty,
            };
            Ok(Some((result, result_tty)))
        }

        Expr::UnaryOp { op, expr: inner } => {
            let (val, tty) = compile_expr(cx, inner)?.unwrap();
            let result = match op {
                UnaryOp::Neg => {
                    let ty = cx.builder.func.dfg.value_type(val);
                    if ty.is_float() {
                        cx.builder.ins().fneg(val)
                    } else {
                        cx.builder.ins().ineg(val)
                    }
                }
                UnaryOp::Not => {
                    let one = cx.builder.ins().iconst(types::I8, 1);
                    cx.builder.ins().bxor(val, one)
                }
            };
            let result_tty = match op {
                UnaryOp::Not => TurboTy::Bool,
                UnaryOp::Neg => tty,
            };
            Ok(Some((result, result_tty)))
        }

        Expr::Call { callee, args } => compile_call(cx, callee, args),

        Expr::If {
            condition,
            then_branch,
            else_branch,
        } => compile_if(cx, condition, then_branch, else_branch.as_deref()),

        Expr::Block { stmts, tail_expr } => {
            let saved_vars = cx.vars.clone();

            // Collect defer expressions while compiling statements
            let mut deferred: Vec<&Spanned<Expr>> = Vec::new();
            for stmt in stmts {
                if let Stmt::Defer(ref defer_expr) = stmt.node {
                    deferred.push(defer_expr);
                }
                compile_stmt(cx, stmt)?;
            }
            let result = if let Some(tail) = tail_expr {
                compile_expr(cx, tail)
            } else {
                Ok(None)
            };

            // Emit deferred expressions in LIFO order (reverse)
            for defer_expr in deferred.iter().rev() {
                if !cx.builder.is_unreachable() {
                    compile_expr(cx, defer_expr)?;
                }
            }

            // Restore variable scope: this ensures inner `let` bindings
            // that shadow outer names don't leak out of the block.
            // Actual SSA values in Cranelift variables are unaffected —
            // only the name-to-Variable mapping is restored.
            cx.vars = saved_vars;

            result
        }

        Expr::Assign { target, value } => {
            let (val, tty) = compile_expr(cx, value)?.unwrap();
            let (var, _, _) = cx.vars.get(target).ok_or_else(|| CodegenError {
                code: ErrorCode::E0401,
                message: format!("undefined variable: {target}"),
            })?;
            let var = *var;
            cx.builder.def_var(var, val);
            // Update the turbo type in case it changed
            if let Some(entry) = cx.vars.get_mut(target) {
                entry.2 = tty;
            }
            Ok(None)
        }

        Expr::CompoundAssign { target, op, value } => {
            let (rhs, _) = compile_expr(cx, value)?.unwrap();
            let (var, _, _) = cx.vars.get(target).ok_or_else(|| CodegenError {
                code: ErrorCode::E0401,
                message: format!("undefined variable: {target}"),
            })?;
            let var = *var;
            let lhs = cx.builder.use_var(var);
            let result = compile_binop(cx, lhs, *op, rhs)?;
            cx.builder.def_var(var, result);
            Ok(None)
        }

        Expr::FieldAssign {
            object,
            field,
            value,
        } => {
            let (obj_ptr, obj_tty) = compile_expr(cx, object)?.unwrap();
            let (val, _) = compile_expr(cx, value)?.unwrap();

            let struct_name = match &obj_tty {
                TurboTy::Struct(name) => name.clone(),
                _ => {
                    return Err(CodegenError {
                        code: ErrorCode::E0400,
                        message: "field assignment on non-struct type".to_string(),
                    })
                }
            };

            let struct_layout = cx
                .struct_fields
                .get(&struct_name)
                .ok_or_else(|| CodegenError {
                    code: ErrorCode::E0400,
                    message: format!("undefined struct: {struct_name}"),
                })?
                .clone();

            let field_index = struct_layout
                .iter()
                .position(|(n, _)| n == field)
                .ok_or_else(|| CodegenError {
                    code: ErrorCode::E0400,
                    message: format!("struct `{struct_name}` has no field `{field}`"),
                })?;

            let offset = (field_index * 8) as i32;

            // Widen smaller types to 64-bit for uniform storage
            let val_ty = cx.builder.func.dfg.value_type(val);
            let val = if val_ty.bits() < 64 && val_ty.is_int() {
                cx.builder.ins().sextend(types::I64, val)
            } else if val_ty.is_float() && val_ty.bits() == 64 {
                cx.builder.ins().bitcast(types::I64, MemFlags::new(), val)
            } else if val_ty.is_float() && val_ty.bits() == 32 {
                let extended = cx.builder.ins().fpromote(types::F64, val);
                cx.builder
                    .ins()
                    .bitcast(types::I64, MemFlags::new(), extended)
            } else {
                val
            };

            cx.builder
                .ins()
                .store(MemFlags::new(), val, obj_ptr, offset);
            Ok(None)
        }

        Expr::IndexAssign {
            object,
            index,
            value,
        } => {
            let (arr, _arr_tty) = compile_expr(cx, object)?.unwrap();
            let (idx, _) = compile_expr(cx, index)?.unwrap();
            let (val, _) = compile_expr(cx, value)?.unwrap();

            // Widen smaller types to 64-bit for uniform storage
            let val_ty = cx.builder.func.dfg.value_type(val);
            let val = if val_ty.bits() < 64 && val_ty.is_int() {
                cx.builder.ins().sextend(types::I64, val)
            } else if val_ty.is_float() && val_ty.bits() == 64 {
                cx.builder.ins().bitcast(types::I64, MemFlags::new(), val)
            } else if val_ty.is_float() && val_ty.bits() == 32 {
                let extended = cx.builder.ins().fpromote(types::F64, val);
                cx.builder
                    .ins()
                    .bitcast(types::I64, MemFlags::new(), extended)
            } else {
                val
            };

            // COW-aware set: returns potentially new pointer
            let set_fid = cx.rt_fns["rt_array_set"];
            let set_ref = cx.module.declare_func_in_func(set_fid, cx.builder.func);
            let call = cx.builder.ins().call(set_ref, &[arr, idx, val]);
            let new_arr = cx.builder.inst_results(call)[0];

            // Update the variable to point to the (possibly new) array
            if let Expr::Ident(name) = &object.node {
                if let Some((var, _cl_ty, _tty)) = cx.vars.get(name) {
                    let var = *var;
                    cx.builder.def_var(var, new_arr);
                }
            }

            Ok(None)
        }

        Expr::While { condition, body } => compile_while(cx, condition, body),

        // Await: if the inner value is a Future (thread handle), join it.
        // Otherwise (direct function call), just pass through.
        Expr::Await(inner) => {
            let result = compile_expr(cx, inner)?;
            if let Some((val, tty)) = result {
                match tty {
                    TurboTy::Future(inner_tty) => {
                        // This is a spawned thread handle — join it
                        let fid = cx.rt_fns["rt_await_handle"];
                        let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
                        let call = cx.builder.ins().call(fref, &[val]);
                        let result_val = cx.builder.inst_results(call)[0];
                        Ok(Some((result_val, *inner_tty)))
                    }
                    _ => {
                        // Not a future — pass through (sync await)
                        Ok(Some((val, tty)))
                    }
                }
            } else {
                Ok(None)
            }
        }

        // Spawn: create an args struct, get thunk fn_ptr, call rt_spawn_with_args
        Expr::Spawn(inner) => {
            let span_start = expr.span.start;
            if let Some(thunk_name) = cx.spawn_thunks.get(&span_start).cloned() {
                if let Expr::Call { callee, args } = &inner.node {
                    if let Expr::Ident(callee_name) = &callee.node {
                        // Determine the return type of the spawned function
                        let inner_ret_tty = cx
                            .fn_ret_types
                            .get(callee_name.as_str())
                            .cloned()
                            .unwrap_or(TurboTy::Unit);

                        // Get the target function's address
                        let target_fid =
                            *cx.user_fns
                                .get(callee_name.as_str())
                                .ok_or_else(|| CodegenError {
                                    code: ErrorCode::E0402,
                                    message: format!("spawn: unknown function `{}`", callee_name),
                                })?;
                        let target_fref =
                            cx.module.declare_func_in_func(target_fid, cx.builder.func);
                        let target_fn_ptr = cx.builder.ins().func_addr(cx.ptr_type, target_fref);

                        // Compile all arguments
                        let mut arg_vals = Vec::new();
                        for arg in args {
                            if let Some((val, tty)) = compile_expr(cx, arg)? {
                                let val = match tty {
                                    TurboTy::Bool => cx.builder.ins().sextend(types::I64, val),
                                    TurboTy::Float => {
                                        cx.builder.ins().bitcast(types::I64, MemFlags::new(), val)
                                    }
                                    _ => val,
                                };
                                arg_vals.push(val);
                            }
                        }

                        // Allocate args struct: [fn_ptr, arg0, arg1, ...]
                        let num_slots = (1 + arg_vals.len()) as i64;
                        let num_slots_val = cx.builder.ins().iconst(types::I64, num_slots);
                        let alloc_fid = cx.rt_fns["rt_struct_alloc"];
                        let alloc_fref = cx.module.declare_func_in_func(alloc_fid, cx.builder.func);
                        let call = cx.builder.ins().call(alloc_fref, &[num_slots_val]);
                        let args_ptr = cx.builder.inst_results(call)[0];

                        // Store fn_ptr at offset 0
                        cx.builder
                            .ins()
                            .store(MemFlags::new(), target_fn_ptr, args_ptr, 0);
                        // Store args at offsets 8, 16, 24, ...
                        for (i, val) in arg_vals.iter().enumerate() {
                            let offset = ((i + 1) * 8) as i32;
                            cx.builder
                                .ins()
                                .store(MemFlags::new(), *val, args_ptr, offset);
                        }

                        // Get the thunk function address
                        let thunk_fid =
                            *cx.user_fns
                                .get(thunk_name.as_str())
                                .ok_or_else(|| CodegenError {
                                    code: ErrorCode::E0405,
                                    message: format!("spawn: thunk `{}` not found", thunk_name),
                                })?;
                        let thunk_fref = cx.module.declare_func_in_func(thunk_fid, cx.builder.func);
                        let thunk_fn_ptr = cx.builder.ins().func_addr(cx.ptr_type, thunk_fref);

                        // Call rt_spawn_with_args(thunk_ptr, args_ptr) -> handle
                        let spawn_fid = cx.rt_fns["rt_spawn_with_args"];
                        let spawn_fref = cx.module.declare_func_in_func(spawn_fid, cx.builder.func);
                        let call = cx.builder.ins().call(spawn_fref, &[thunk_fn_ptr, args_ptr]);
                        let handle = cx.builder.inst_results(call)[0];

                        return Ok(Some((handle, TurboTy::Future(Box::new(inner_ret_tty)))));
                    }
                }
            }
            // Fallback: synchronous execution (backward compat)
            compile_expr(cx, inner)
        }

        // Try operator: expr? — unwrap Ok, propagate Err (simplified: just evaluate inner)
        // Try operator: expr? — unwrap Ok, propagate Err
        Expr::Try(inner) => {
            // Compile the inner expression (must produce a Result pointer)
            let (result_ptr, _result_tty) = compile_expr(cx, inner)?.unwrap();

            // Get the tag: 0 = ok, 1 = err
            let tag_fid = cx.rt_fns["rt_result_tag"];
            let tag_fref = cx.module.declare_func_in_func(tag_fid, cx.builder.func);
            let tag_call = cx.builder.ins().call(tag_fref, &[result_ptr]);
            let tag = cx.builder.inst_results(tag_call)[0];

            // Branch: if tag == 0 (ok), continue; else return the Result as-is
            let zero = cx.builder.ins().iconst(types::I64, 0);
            let is_ok = cx.builder.ins().icmp(IntCC::Equal, tag, zero);

            let ok_block = cx.builder.create_block();
            let err_block = cx.builder.create_block();

            cx.builder.ins().brif(is_ok, ok_block, &[], err_block, &[]);

            // err_block: propagate the error by returning the Result pointer
            cx.builder.switch_to_block(err_block);
            cx.builder.seal_block(err_block);
            cx.builder.ins().return_(&[result_ptr]);

            // ok_block: extract the ok value and continue
            cx.builder.switch_to_block(ok_block);
            cx.builder.seal_block(ok_block);
            let val_fid = cx.rt_fns["rt_result_value"];
            let val_fref = cx.module.declare_func_in_func(val_fid, cx.builder.func);
            let val_call = cx.builder.ins().call(val_fref, &[result_ptr]);
            let ok_value = cx.builder.inst_results(val_call)[0];

            Ok(Some((ok_value, TurboTy::Int)))
        }

        Expr::ForIn {
            var_name,
            iterable,
            body,
        } => compile_for_in(cx, var_name, iterable, body),

        Expr::Range { .. } => Err(CodegenError {
            code: ErrorCode::E0400,
            message: "range expressions can only be used in for-in loops".to_string(),
        }),

        Expr::Break => {
            if let Some(&(_header, exit)) = cx.loop_stack.last() {
                cx.builder.ins().jump(exit, &[]);
                // Create an unreachable block so subsequent code has somewhere to go
                let dead_block = cx.builder.create_block();
                cx.builder.switch_to_block(dead_block);
                cx.builder.seal_block(dead_block);
            }
            Ok(None)
        }

        Expr::Continue => {
            if let Some(&(header, _exit)) = cx.loop_stack.last() {
                cx.builder.ins().jump(header, &[]);
                // Create an unreachable block so subsequent code has somewhere to go
                let dead_block = cx.builder.create_block();
                cx.builder.switch_to_block(dead_block);
                cx.builder.seal_block(dead_block);
            }
            Ok(None)
        }

        Expr::ArrayLit(elements) => {
            let len = elements.len() as i64;
            let len_val = cx.builder.ins().iconst(types::I64, len);

            let alloc_fid = cx.rt_fns["rt_array_alloc"];
            let alloc_ref = cx.module.declare_func_in_func(alloc_fid, cx.builder.func);
            let call = cx.builder.ins().call(alloc_ref, &[len_val]);
            let arr_ptr = cx.builder.inst_results(call)[0];

            let mut elem_tty = TurboTy::Int; // default; overridden by first element
            for (i, elem) in elements.iter().enumerate() {
                let (val, tty) = compile_expr(cx, elem)?.unwrap();
                if i == 0 {
                    elem_tty = tty;
                }
                let offset = cx.builder.ins().iconst(cx.ptr_type, (8 + i * 8) as i64);
                let elem_ptr = cx.builder.ins().iadd(arr_ptr, offset);
                cx.builder.ins().store(MemFlags::new(), val, elem_ptr, 0);
            }

            Ok(Some((arr_ptr, TurboTy::Array(Box::new(elem_tty)))))
        }

        Expr::Index { object, index } => {
            let (arr, arr_tty) = compile_expr(cx, object)?.unwrap();
            let (idx, _) = compile_expr(cx, index)?.unwrap();

            let get_fid = cx.rt_fns["rt_array_get"];
            let get_ref = cx.module.declare_func_in_func(get_fid, cx.builder.func);
            let call = cx.builder.ins().call(get_ref, &[arr, idx]);
            let raw = cx.builder.inst_results(call)[0];

            // Extract the element TurboTy from the array type
            let elem_tty = match arr_tty {
                TurboTy::Array(inner) => *inner,
                _ => TurboTy::Int, // fallback
            };

            // rt_array_get returns raw i64 bits; convert to the correct type
            let (result, result_tty) = match &elem_tty {
                TurboTy::Bool => {
                    let truncated = cx.builder.ins().ireduce(types::I8, raw);
                    (truncated, elem_tty)
                }
                TurboTy::Float => {
                    let f = cx.builder.ins().bitcast(types::F64, MemFlags::new(), raw);
                    (f, elem_tty)
                }
                _ => (raw, elem_tty), // Int, Str, Struct, Enum — raw i64 is correct
            };
            Ok(Some((result, result_tty)))
        }

        Expr::StructLit { name, fields } => {
            // Check if this is an agent instantiation
            if let Some((model, tools, system_prompt)) = cx.agent_defs.get(name).cloned() {
                // Allocate a struct with 3 slots: [model, system, tools]
                let num_fields_val = cx.builder.ins().iconst(types::I64, 3);
                let alloc_fid = cx.rt_fns["rt_struct_alloc"];
                let alloc_fref = cx.module.declare_func_in_func(alloc_fid, cx.builder.func);
                let call = cx.builder.ins().call(alloc_fref, &[num_fields_val]);
                let ptr = cx.builder.inst_results(call)[0];

                // Slot 0: model string
                let model_val = cx.create_string(&model)?;
                cx.builder.ins().store(MemFlags::new(), model_val, ptr, 0);

                // Slot 1: system prompt string
                let system_str = system_prompt.as_deref().unwrap_or("");
                let system_val = cx.create_string(system_str)?;
                cx.builder.ins().store(MemFlags::new(), system_val, ptr, 8);

                // Slot 2: tools array (array of tool name strings)
                let tools_len = tools.len() as i64;
                let tools_len_val = cx.builder.ins().iconst(types::I64, tools_len);
                let arr_alloc_fid = cx.rt_fns["rt_array_alloc"];
                let arr_alloc_ref = cx
                    .module
                    .declare_func_in_func(arr_alloc_fid, cx.builder.func);
                let arr_call = cx.builder.ins().call(arr_alloc_ref, &[tools_len_val]);
                let arr_ptr = cx.builder.inst_results(arr_call)[0];
                for (i, tool_name) in tools.iter().enumerate() {
                    let tool_str = cx.create_string(tool_name)?;
                    let offset = cx.builder.ins().iconst(cx.ptr_type, (8 + i * 8) as i64);
                    let elem_ptr = cx.builder.ins().iadd(arr_ptr, offset);
                    cx.builder
                        .ins()
                        .store(MemFlags::new(), tool_str, elem_ptr, 0);
                }
                cx.builder.ins().store(MemFlags::new(), arr_ptr, ptr, 16);

                return Ok(Some((ptr, TurboTy::Agent(name.clone()))));
            }

            let struct_layout = cx
                .struct_fields
                .get(name)
                .ok_or_else(|| CodegenError {
                    code: ErrorCode::E0400,
                    message: format!("undefined struct: {name}"),
                })?
                .clone();

            let num_fields = struct_layout.len() as i64;
            let num_fields_val = cx.builder.ins().iconst(types::I64, num_fields);

            // Call rt_struct_alloc to allocate memory
            let alloc_fid = cx.rt_fns["rt_struct_alloc"];
            let alloc_fref = cx.module.declare_func_in_func(alloc_fid, cx.builder.func);
            let call = cx.builder.ins().call(alloc_fref, &[num_fields_val]);
            let ptr = cx.builder.inst_results(call)[0];

            // Track concrete field types for generic structs
            let mut concrete_fields: Vec<(String, TurboTy)> = Vec::new();

            // Store each field at its offset
            for (field_name, field_value) in fields {
                let field_index = struct_layout
                    .iter()
                    .position(|(n, _)| n == field_name)
                    .ok_or_else(|| CodegenError {
                        code: ErrorCode::E0400,
                        message: format!("struct `{name}` has no field `{field_name}`"),
                    })?;

                let (val, tty) = compile_expr(cx, field_value)?.unwrap();
                concrete_fields.push((field_name.clone(), tty));
                let offset = (field_index * 8) as i32;

                // Widen smaller types to 64-bit for uniform storage
                let val_ty = cx.builder.func.dfg.value_type(val);
                let val = if val_ty.bits() < 64 && val_ty.is_int() {
                    cx.builder.ins().sextend(types::I64, val)
                } else if val_ty.is_float() && val_ty.bits() == 64 {
                    cx.builder.ins().bitcast(types::I64, MemFlags::new(), val)
                } else if val_ty.is_float() && val_ty.bits() == 32 {
                    let extended = cx.builder.ins().fpromote(types::F64, val);
                    cx.builder
                        .ins()
                        .bitcast(types::I64, MemFlags::new(), extended)
                } else {
                    val
                };

                cx.builder.ins().store(MemFlags::new(), val, ptr, offset);
            }

            // Store concrete field types for generic struct instances
            cx.last_struct_lit_concrete_fields = Some(concrete_fields);

            Ok(Some((ptr, TurboTy::Struct(name.clone()))))
        }

        Expr::FieldAccess { object, field } => {
            // Check if this is actually an enum variant access: EnumName.VariantName
            if let Expr::Ident(ref name) = object.node {
                if let Some(variants) = cx.enum_variants.get(name.as_str()) {
                    let index =
                        variants
                            .iter()
                            .position(|v| v == field)
                            .ok_or_else(|| CodegenError {
                                code: ErrorCode::E0400,
                                message: format!("enum `{name}` has no variant `{field}`"),
                            })?;

                    // Check if this is a data-carrying enum
                    if let Some(&max_slots) = cx.enum_max_slots.get(name.as_str()) {
                        // Allocate tagged union: [tag][slot0][slot1]...[slotN]
                        let total_slots = 1 + max_slots; // tag + payload
                        let num_fields_val =
                            cx.builder.ins().iconst(types::I64, total_slots as i64);
                        let alloc_fid = cx.rt_fns["rt_struct_alloc"];
                        let alloc_fref = cx.module.declare_func_in_func(alloc_fid, cx.builder.func);
                        let call = cx.builder.ins().call(alloc_fref, &[num_fields_val]);
                        let ptr = cx.builder.inst_results(call)[0];
                        // Store tag
                        let tag_val = cx.builder.ins().iconst(types::I64, index as i64);
                        cx.builder.ins().store(MemFlags::new(), tag_val, ptr, 0);
                        return Ok(Some((ptr, TurboTy::Enum(name.clone()))));
                    } else {
                        let val = cx.builder.ins().iconst(types::I64, index as i64);
                        return Ok(Some((val, TurboTy::Enum(name.clone()))));
                    }
                }
            }

            let (obj_ptr, obj_tty) = compile_expr(cx, object)?.unwrap();

            // Handle agent field access: model (slot 0), system (slot 1), tools (slot 2)
            if let TurboTy::Agent(_) = &obj_tty {
                let (offset, tty) = match field.as_str() {
                    "model" => (0i32, TurboTy::Str),
                    "system" => (8i32, TurboTy::Str),
                    "tools" => (16i32, TurboTy::Array(Box::new(TurboTy::Str))),
                    _ => {
                        return Err(CodegenError {
                            code: ErrorCode::E0400,
                            message: format!("agent has no field `{field}`"),
                        })
                    }
                };
                let val = cx
                    .builder
                    .ins()
                    .load(types::I64, MemFlags::new(), obj_ptr, offset);
                return Ok(Some((val, tty)));
            }

            let struct_name = match &obj_tty {
                TurboTy::Struct(name) => name.clone(),
                _ => {
                    return Err(CodegenError {
                        code: ErrorCode::E0400,
                        message: "field access on non-struct type".to_string(),
                    })
                }
            };

            let struct_layout = cx
                .struct_fields
                .get(&struct_name)
                .ok_or_else(|| CodegenError {
                    code: ErrorCode::E0400,
                    message: format!("undefined struct: {struct_name}"),
                })?
                .clone();

            let field_index = struct_layout
                .iter()
                .position(|(n, _)| n == field)
                .ok_or_else(|| CodegenError {
                    code: ErrorCode::E0400,
                    message: format!("struct `{struct_name}` has no field `{field}`"),
                })?;

            let mut field_tty = struct_layout[field_index].1.clone();

            // For generic structs, check if we have concrete field type overrides
            if let Expr::Ident(ref var_name) = object.node {
                if let Some(concrete_fields) = cx.generic_struct_field_overrides.get(var_name) {
                    if let Some((_, concrete_tty)) =
                        concrete_fields.iter().find(|(n, _)| n == field)
                    {
                        field_tty = concrete_tty.clone();
                    }
                }
            }

            let offset = (field_index * 8) as i32;

            // Load from the struct pointer
            let raw_val = cx
                .builder
                .ins()
                .load(types::I64, MemFlags::new(), obj_ptr, offset);

            // Convert back to the appropriate type
            let (val, tty) = match &field_tty {
                TurboTy::Int => (raw_val, TurboTy::Int),
                TurboTy::Bool => {
                    let truncated = cx.builder.ins().ireduce(types::I8, raw_val);
                    (truncated, TurboTy::Bool)
                }
                TurboTy::Float => {
                    let f = cx
                        .builder
                        .ins()
                        .bitcast(types::F64, MemFlags::new(), raw_val);
                    (f, TurboTy::Float)
                }
                TurboTy::Str => (raw_val, TurboTy::Str),
                TurboTy::Struct(name) => (raw_val, TurboTy::Struct(name.clone())),
                _ => (raw_val, field_tty),
            };

            Ok(Some((val, tty)))
        }

        Expr::EnumVariant { enum_name, variant } => {
            let variants =
                cx.enum_variants
                    .get(enum_name.as_str())
                    .ok_or_else(|| CodegenError {
                        code: ErrorCode::E0400,
                        message: format!("undefined enum: {enum_name}"),
                    })?;
            let index = variants
                .iter()
                .position(|v| v == variant)
                .ok_or_else(|| CodegenError {
                    code: ErrorCode::E0400,
                    message: format!("enum `{enum_name}` has no variant `{variant}`"),
                })?;

            // Check if this is a data-carrying enum
            if let Some(&max_slots) = cx.enum_max_slots.get(enum_name.as_str()) {
                // Allocate tagged union: [tag][slot0][slot1]...[slotN]
                let total_slots = 1 + max_slots;
                let num_fields_val = cx.builder.ins().iconst(types::I64, total_slots as i64);
                let alloc_fid = cx.rt_fns["rt_struct_alloc"];
                let alloc_fref = cx.module.declare_func_in_func(alloc_fid, cx.builder.func);
                let call = cx.builder.ins().call(alloc_fref, &[num_fields_val]);
                let ptr = cx.builder.inst_results(call)[0];
                let tag_val = cx.builder.ins().iconst(types::I64, index as i64);
                cx.builder.ins().store(MemFlags::new(), tag_val, ptr, 0);
                Ok(Some((ptr, TurboTy::Enum(enum_name.clone()))))
            } else {
                let val = cx.builder.ins().iconst(types::I64, index as i64);
                Ok(Some((val, TurboTy::Enum(enum_name.clone()))))
            }
        }

        Expr::Match { subject, arms } => compile_match(cx, subject, arms),

        Expr::Interpolation(parts) => compile_interpolation(cx, parts),

        Expr::Closure { params, .. } => {
            // Look up the pre-declared closure function by span start
            let span_start = expr.span.start;
            let (closure_name, closure_ty, _free_vars) = cx
                .closure_fns
                .get(&span_start)
                .ok_or_else(|| CodegenError {
                    code: ErrorCode::E0400,
                    message: "internal error: closure not found in pre-compiled map".to_string(),
                })?;
            let closure_ty = closure_ty.clone();
            let closure_name = closure_name.clone();
            let func_id = *cx
                .user_fns
                .get(closure_name.as_str())
                .ok_or_else(|| CodegenError {
                    code: ErrorCode::E0400,
                    message: format!(
                        "internal error: closure function {} not found",
                        closure_name
                    ),
                })?;
            let func_ref = cx.module.declare_func_in_func(func_id, cx.builder.func);
            let fn_ptr = cx.builder.ins().func_addr(cx.ptr_type, func_ref);

            // Determine captures: free variables that exist in the current scope
            let outer_var_names: Vec<String> = cx.vars.keys().cloned().collect();
            let capture_names = find_captures(params, &expr.node, &outer_var_names);

            // Build capture info with types
            let mut captures: Vec<(String, TurboTy)> = Vec::new();
            for cap_name in &capture_names {
                if let Some((_var, _cl_ty, turbo_ty)) = cx.vars.get(cap_name) {
                    captures.push((cap_name.clone(), turbo_ty.clone()));
                }
            }

            // Store capture info for the closure body compiler
            cx.closure_captures.insert(
                span_start,
                CaptureInfo {
                    captures: captures.clone(),
                },
            );

            // Allocate environment struct for captured variables
            let num_captures = captures.len() as i64;
            let env_ptr = if num_captures > 0 {
                let num_fields_val = cx.builder.ins().iconst(types::I64, num_captures);
                let alloc_fid = cx.rt_fns["rt_struct_alloc"];
                let alloc_fref = cx.module.declare_func_in_func(alloc_fid, cx.builder.func);
                let call = cx.builder.ins().call(alloc_fref, &[num_fields_val]);
                let env_ptr = cx.builder.inst_results(call)[0];

                // Store each captured variable into the env struct
                for (cap_idx, (cap_name, _cap_tty)) in captures.iter().enumerate() {
                    let (var, _cl_ty, _turbo_ty) =
                        cx.vars.get(cap_name).ok_or_else(|| CodegenError {
                            code: ErrorCode::E0400,
                            message: format!(
                                "internal error: capture variable {} not found",
                                cap_name
                            ),
                        })?;
                    let val = cx.builder.use_var(*var);
                    let offset = (cap_idx * 8) as i32;

                    // Widen to i64 for uniform storage
                    let val_ty = cx.builder.func.dfg.value_type(val);
                    let val = if val_ty.bits() < 64 && val_ty.is_int() {
                        cx.builder.ins().sextend(types::I64, val)
                    } else if val_ty.is_float() && val_ty.bits() == 64 {
                        cx.builder.ins().bitcast(types::I64, MemFlags::new(), val)
                    } else if val_ty.is_float() && val_ty.bits() == 32 {
                        let extended = cx.builder.ins().fpromote(types::F64, val);
                        cx.builder
                            .ins()
                            .bitcast(types::I64, MemFlags::new(), extended)
                    } else {
                        val
                    };
                    cx.builder
                        .ins()
                        .store(MemFlags::new(), val, env_ptr, offset);
                }
                env_ptr
            } else {
                // No captures: null env pointer
                cx.builder.ins().iconst(cx.ptr_type, 0)
            };

            // Allocate closure pair struct: [fn_ptr, env_ptr]
            let two = cx.builder.ins().iconst(types::I64, 2);
            let alloc_fid = cx.rt_fns["rt_struct_alloc"];
            let alloc_fref = cx.module.declare_func_in_func(alloc_fid, cx.builder.func);
            let call = cx.builder.ins().call(alloc_fref, &[two]);
            let closure_ptr = cx.builder.inst_results(call)[0];

            // Store fn_ptr at offset 0
            cx.builder
                .ins()
                .store(MemFlags::new(), fn_ptr, closure_ptr, 0);
            // Store env_ptr at offset 8
            cx.builder
                .ins()
                .store(MemFlags::new(), env_ptr, closure_ptr, 8);

            Ok(Some((closure_ptr, closure_ty)))
        }

        Expr::OkExpr(value) => {
            let (val, _tty) = compile_expr(cx, value)?.unwrap();
            // Widen to i64 if needed (bools, etc.)
            let val_ty = cx.builder.func.dfg.value_type(val);
            let val = if val_ty.is_int() && val_ty.bits() < 64 {
                cx.builder.ins().sextend(types::I64, val)
            } else if val_ty.is_float() {
                cx.builder.ins().bitcast(types::I64, MemFlags::new(), val)
            } else {
                val
            };
            let fid = cx.rt_fns["rt_result_ok"];
            let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
            let call = cx.builder.ins().call(fref, &[val]);
            let ptr = cx.builder.inst_results(call)[0];
            Ok(Some((
                ptr,
                TurboTy::Result(Box::new(TurboTy::Int), Box::new(TurboTy::Int)),
            )))
        }

        Expr::ErrExpr(value) => {
            let (val, _tty) = compile_expr(cx, value)?.unwrap();
            // Widen to i64 if needed
            let val_ty = cx.builder.func.dfg.value_type(val);
            let val = if val_ty.is_int() && val_ty.bits() < 64 {
                cx.builder.ins().sextend(types::I64, val)
            } else if val_ty.is_float() {
                cx.builder.ins().bitcast(types::I64, MemFlags::new(), val)
            } else {
                val
            };
            let fid = cx.rt_fns["rt_result_err"];
            let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
            let call = cx.builder.ins().call(fref, &[val]);
            let ptr = cx.builder.inst_results(call)[0];
            Ok(Some((
                ptr,
                TurboTy::Result(Box::new(TurboTy::Int), Box::new(TurboTy::Int)),
            )))
        }

        Expr::SomeExpr(value) => {
            let (val, _tty) = compile_expr(cx, value)?.unwrap();
            // Widen to i64 if needed (bools, etc.)
            let val_ty = cx.builder.func.dfg.value_type(val);
            let val = if val_ty.is_int() && val_ty.bits() < 64 {
                cx.builder.ins().sextend(types::I64, val)
            } else if val_ty.is_float() {
                cx.builder.ins().bitcast(types::I64, MemFlags::new(), val)
            } else {
                val
            };
            let fid = cx.rt_fns["rt_option_some"];
            let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
            let call = cx.builder.ins().call(fref, &[val]);
            let ptr = cx.builder.inst_results(call)[0];
            Ok(Some((ptr, TurboTy::Optional(Box::new(TurboTy::Int)))))
        }

        Expr::NoneExpr => {
            let fid = cx.rt_fns["rt_option_none"];
            let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
            let call = cx.builder.ins().call(fref, &[]);
            let ptr = cx.builder.inst_results(call)[0];
            Ok(Some((ptr, TurboTy::Optional(Box::new(TurboTy::Int)))))
        }

        Expr::NullCoalesce { value, default } => {
            // Compile the optional value
            let (opt_val, _opt_tty) = compile_expr(cx, value)?.unwrap();

            // Extract tag
            let tag_fid = cx.rt_fns["rt_option_tag"];
            let tag_fref = cx.module.declare_func_in_func(tag_fid, cx.builder.func);
            let tag_call = cx.builder.ins().call(tag_fref, &[opt_val]);
            let tag = cx.builder.inst_results(tag_call)[0];

            // Check if tag == 1 (some)
            let one = cx.builder.ins().iconst(types::I64, 1);
            let is_some = cx.builder.ins().icmp(IntCC::Equal, tag, one);

            let some_block = cx.builder.create_block();
            let none_block = cx.builder.create_block();
            let merge_block = cx.builder.create_block();

            cx.builder
                .ins()
                .brif(is_some, some_block, &[], none_block, &[]);

            // Some path: extract value
            cx.builder.switch_to_block(some_block);
            cx.builder.seal_block(some_block);
            let val_fid = cx.rt_fns["rt_option_value"];
            let val_fref = cx.module.declare_func_in_func(val_fid, cx.builder.func);
            let val_call = cx.builder.ins().call(val_fref, &[opt_val]);
            let some_val = cx.builder.inst_results(val_call)[0];
            cx.builder.ins().jump(merge_block, &[some_val]);

            // None path: compile default
            cx.builder.switch_to_block(none_block);
            cx.builder.seal_block(none_block);
            let (def_val, def_tty) = compile_expr(cx, default)?.unwrap();
            // Widen default to i64 if needed for consistency
            let def_ty = cx.builder.func.dfg.value_type(def_val);
            let def_val = if def_ty.is_int() && def_ty.bits() < 64 {
                cx.builder.ins().sextend(types::I64, def_val)
            } else if def_ty.is_float() {
                cx.builder
                    .ins()
                    .bitcast(types::I64, MemFlags::new(), def_val)
            } else {
                def_val
            };
            cx.builder.ins().jump(merge_block, &[def_val]);

            // Merge block
            cx.builder.append_block_param(merge_block, types::I64);
            cx.builder.switch_to_block(merge_block);
            cx.builder.seal_block(merge_block);
            let result = cx.builder.block_params(merge_block)[0];

            Ok(Some((result, def_tty)))
        }
    }
}

// ── Statement compilation ───────────────────────────────────────────

fn compile_stmt<M: Module>(cx: &mut Ctx<'_, M>, stmt: &Spanned<Stmt>) -> Result<(), CodegenError> {
    match &stmt.node {
        Stmt::Let { name, value, .. } => {
            // Clear any stale concrete fields from a previous struct lit
            cx.last_struct_lit_concrete_fields = None;
            // Check if the RHS is a variable reference (for COW retain)
            let rhs_is_ident = matches!(&value.node, Expr::Ident(_));
            let result = compile_expr(cx, value)?;
            // If the value was a struct literal, capture concrete field types for generic structs
            if let Some(concrete_fields) = cx.last_struct_lit_concrete_fields.take() {
                cx.generic_struct_field_overrides
                    .insert(name.clone(), concrete_fields);
            }
            let (cl_ty, turbo_ty, val) = if let Some((v, tty)) = result {
                (cx.builder.func.dfg.value_type(v), tty, Some(v))
            } else {
                (types::I64, TurboTy::Unit, None)
            };
            // COW: if the RHS is another variable with a heap type, retain it
            // so the shared object has correct refcount for both references
            if rhs_is_ident {
                if let Some(v) = val {
                    let needs_retain = matches!(
                        &turbo_ty,
                        TurboTy::Array(_)
                            | TurboTy::Struct(_)
                            | TurboTy::Result(_, _)
                            | TurboTy::Optional(_)
                    );
                    if needs_retain {
                        let retain_fid = cx.rt_fns["rt_retain"];
                        let retain_ref =
                            cx.module.declare_func_in_func(retain_fid, cx.builder.func);
                        cx.builder.ins().call(retain_ref, &[v]);
                    }
                }
            }
            let var = Variable::new(cx.next_var);
            cx.next_var += 1;
            cx.builder.declare_var(var, cl_ty);
            if let Some(v) = val {
                cx.builder.def_var(var, v);
            }
            cx.vars.insert(name.clone(), (var, cl_ty, turbo_ty));
            Ok(())
        }
        Stmt::Expr(e) => {
            compile_expr(cx, e)?;
            Ok(())
        }
        Stmt::Return(value) => {
            if let Some(val_expr) = value {
                let result = compile_expr(cx, val_expr)?;
                if let Some((v, _)) = result {
                    cx.builder.ins().return_(&[v]);
                } else {
                    cx.builder.ins().return_(&[]);
                }
            } else {
                cx.builder.ins().return_(&[]);
            }
            let new_block = cx.builder.create_block();
            cx.builder.switch_to_block(new_block);
            cx.builder.seal_block(new_block);
            Ok(())
        }
        Stmt::Defer(_) => {
            // Defer statements are handled at the block level (compile_expr for Block)
            // — they are collected and emitted in reverse order at the end of the block.
            Ok(())
        }
    }
}

// ── Binary operations ───────────────────────────────────────────────

fn compile_binop<M: Module>(
    cx: &mut Ctx<'_, M>,
    lhs: Value,
    op: BinOp,
    rhs: Value,
) -> Result<Value, CodegenError> {
    let lhs_ty = cx.builder.func.dfg.value_type(lhs);

    if lhs_ty.is_float() {
        let result = match op {
            BinOp::Add => cx.builder.ins().fadd(lhs, rhs),
            BinOp::Sub => cx.builder.ins().fsub(lhs, rhs),
            BinOp::Mul => cx.builder.ins().fmul(lhs, rhs),
            BinOp::Div => cx.builder.ins().fdiv(lhs, rhs),
            BinOp::Eq => cx.builder.ins().fcmp(FloatCC::Equal, lhs, rhs),
            BinOp::NotEq => cx.builder.ins().fcmp(FloatCC::NotEqual, lhs, rhs),
            BinOp::Less => cx.builder.ins().fcmp(FloatCC::LessThan, lhs, rhs),
            BinOp::LessEq => cx.builder.ins().fcmp(FloatCC::LessThanOrEqual, lhs, rhs),
            BinOp::Greater => cx.builder.ins().fcmp(FloatCC::GreaterThan, lhs, rhs),
            BinOp::GreaterEq => cx.builder.ins().fcmp(FloatCC::GreaterThanOrEqual, lhs, rhs),
            _ => {
                return Err(CodegenError {
                    code: ErrorCode::E0403,
                    message: format!("unsupported float op: {op:?}"),
                })
            }
        };
        Ok(result)
    } else {
        // Widen mismatched integer widths
        let rhs_ty = cx.builder.func.dfg.value_type(rhs);
        let (lhs, rhs) = if lhs_ty.bits() != rhs_ty.bits() {
            let target = if lhs_ty.bits() > rhs_ty.bits() {
                lhs_ty
            } else {
                rhs_ty
            };
            let lhs = if lhs_ty.bits() < target.bits() {
                cx.builder.ins().sextend(target, lhs)
            } else {
                lhs
            };
            let rhs = if rhs_ty.bits() < target.bits() {
                cx.builder.ins().sextend(target, rhs)
            } else {
                rhs
            };
            (lhs, rhs)
        } else {
            (lhs, rhs)
        };

        let result = match op {
            BinOp::Add => cx.builder.ins().iadd(lhs, rhs),
            BinOp::Sub => cx.builder.ins().isub(lhs, rhs),
            BinOp::Mul => cx.builder.ins().imul(lhs, rhs),
            BinOp::Div | BinOp::Mod => {
                emit_div_zero_check(cx, rhs);
                emit_int_overflow_check(cx, lhs, rhs);
                if op == BinOp::Div {
                    cx.builder.ins().sdiv(lhs, rhs)
                } else {
                    cx.builder.ins().srem(lhs, rhs)
                }
            }
            BinOp::Eq => cx.builder.ins().icmp(IntCC::Equal, lhs, rhs),
            BinOp::NotEq => cx.builder.ins().icmp(IntCC::NotEqual, lhs, rhs),
            BinOp::Less => cx.builder.ins().icmp(IntCC::SignedLessThan, lhs, rhs),
            BinOp::LessEq => cx
                .builder
                .ins()
                .icmp(IntCC::SignedLessThanOrEqual, lhs, rhs),
            BinOp::Greater => cx.builder.ins().icmp(IntCC::SignedGreaterThan, lhs, rhs),
            BinOp::GreaterEq => cx
                .builder
                .ins()
                .icmp(IntCC::SignedGreaterThanOrEqual, lhs, rhs),
            BinOp::And => cx.builder.ins().band(lhs, rhs),
            BinOp::Or => cx.builder.ins().bor(lhs, rhs),
        };
        Ok(result)
    }
}

fn emit_div_zero_check<M: Module>(cx: &mut Ctx<'_, M>, divisor: Value) {
    let ty = cx.builder.func.dfg.value_type(divisor);
    let zero = cx.builder.ins().iconst(ty, 0);
    let is_zero = cx.builder.ins().icmp(IntCC::Equal, divisor, zero);

    let trap_block = cx.builder.create_block();
    let ok_block = cx.builder.create_block();

    cx.builder
        .ins()
        .brif(is_zero, trap_block, &[], ok_block, &[]);

    cx.builder.switch_to_block(trap_block);
    cx.builder.seal_block(trap_block);
    cx.rt_call("rt_div_by_zero", &[]);
    cx.builder.ins().trap(TrapCode::unwrap_user(1));

    cx.builder.switch_to_block(ok_block);
    cx.builder.seal_block(ok_block);
}

fn emit_int_overflow_check<M: Module>(cx: &mut Ctx<'_, M>, dividend: Value, divisor: Value) {
    let ty = cx.builder.func.dfg.value_type(dividend);

    // Check if divisor is -1
    let neg_one = cx.builder.ins().iconst(ty, -1i64);
    let is_neg_one = cx.builder.ins().icmp(IntCC::Equal, divisor, neg_one);

    // Check if dividend is INT_MIN (0x8000...0)
    let int_min = cx.builder.ins().iconst(ty, i64::MIN);
    let is_int_min = cx.builder.ins().icmp(IntCC::Equal, dividend, int_min);

    // Both conditions must be true for overflow
    let is_overflow = cx.builder.ins().band(is_neg_one, is_int_min);

    let trap_block = cx.builder.create_block();
    let ok_block = cx.builder.create_block();

    cx.builder
        .ins()
        .brif(is_overflow, trap_block, &[], ok_block, &[]);

    cx.builder.switch_to_block(trap_block);
    cx.builder.seal_block(trap_block);
    cx.rt_call("rt_int_overflow", &[]);
    cx.builder.ins().trap(TrapCode::unwrap_user(1));

    cx.builder.switch_to_block(ok_block);
    cx.builder.seal_block(ok_block);
}

// ── Short-circuit && / || ───────────────────────────────────────────

fn compile_short_circuit<M: Module>(
    cx: &mut Ctx<'_, M>,
    left: &Spanned<Expr>,
    op: BinOp,
    right: &Spanned<Expr>,
) -> Result<MaybeTyped, CodegenError> {
    let (lhs, _) = compile_expr(cx, left)?.unwrap();
    let lhs_bool = cx.to_bool(lhs);

    let eval_rhs_block = cx.builder.create_block();
    let merge_block = cx.builder.create_block();
    cx.builder.append_block_param(merge_block, types::I8);

    match op {
        BinOp::And => {
            let false_val = cx.builder.ins().iconst(types::I8, 0);
            cx.builder
                .ins()
                .brif(lhs_bool, eval_rhs_block, &[], merge_block, &[false_val]);
        }
        BinOp::Or => {
            let true_val = cx.builder.ins().iconst(types::I8, 1);
            cx.builder
                .ins()
                .brif(lhs_bool, merge_block, &[true_val], eval_rhs_block, &[]);
        }
        _ => unreachable!(),
    }

    cx.builder.switch_to_block(eval_rhs_block);
    cx.builder.seal_block(eval_rhs_block);
    let (rhs, _) = compile_expr(cx, right)?.unwrap();

    let rhs_as_i8 = cx.to_bool(rhs);

    cx.builder.ins().jump(merge_block, &[rhs_as_i8]);

    cx.builder.switch_to_block(merge_block);
    cx.builder.seal_block(merge_block);

    let result = cx.builder.block_params(merge_block)[0];
    Ok(Some((result, TurboTy::Bool)))
}

// ── Function calls ──────────────────────────────────────────────────

fn compile_call<M: Module>(
    cx: &mut Ctx<'_, M>,
    callee: &Spanned<Expr>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    // Handle method calls: expr.method(args)
    if let Expr::FieldAccess {
        ref object,
        ref field,
    } = callee.node
    {
        let (obj_val, obj_tty) = compile_expr(cx, object)?.unwrap();
        if let TurboTy::Struct(ref type_name) = obj_tty {
            let mangled = format!("{}__{}", type_name, field);
            if let Some(&fid) = cx.user_fns.get(&mangled) {
                let mut arg_vals = vec![obj_val];
                for arg in args {
                    if let Some((v, _)) = compile_expr(cx, arg)? {
                        arg_vals.push(v);
                    }
                }
                let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
                let call = cx.builder.ins().call(fref, &arg_vals);
                let results = cx.builder.inst_results(call);
                let ret_tty = cx
                    .fn_ret_types
                    .get(&mangled)
                    .cloned()
                    .unwrap_or(TurboTy::Unit);
                if results.is_empty() {
                    return Ok(None);
                } else {
                    return Ok(Some((results[0], ret_tty)));
                }
            }
        }
        return Err(CodegenError {
            code: ErrorCode::E0400,
            message: format!("no method `{field}` found"),
        });
    }

    let Expr::Ident(name) = &callee.node else {
        return Err(CodegenError {
            code: ErrorCode::E0400,
            message: "indirect function calls not yet supported".to_string(),
        });
    };

    match name.as_str() {
        "print" => compile_print(cx, args),
        "panic" => compile_panic(cx, args),
        "assert" => compile_assert(cx, args),
        "assert_eq" => compile_assert_eq(cx, args, false),
        "assert_ne" => compile_assert_eq(cx, args, true),
        "len" => compile_len(cx, args),
        "push" => compile_builtin_push(cx, args),
        "abs" => compile_abs(cx, args),
        "min" => compile_min(cx, args),
        "max" => compile_max(cx, args),
        "to_str" => compile_to_str_builtin(cx, args),
        // Stdlib string builtins
        "split" => compile_stdlib_split(cx, args),
        "trim" => compile_stdlib_str1(cx, args, "rt_str_trim"),
        "upper" => compile_stdlib_str1(cx, args, "rt_str_upper"),
        "lower" => compile_stdlib_str1(cx, args, "rt_str_lower"),
        "starts_with" => compile_stdlib_str_bool2(cx, args, "rt_str_starts_with"),
        "ends_with" => compile_stdlib_str_bool2(cx, args, "rt_str_ends_with"),
        "replace" => compile_stdlib_replace(cx, args),
        "char_at" => compile_stdlib_char_at(cx, args),
        "contains" => compile_stdlib_str_bool2(cx, args, "rt_str_contains"),
        "index_of" => compile_stdlib_index_of(cx, args),
        "join" => compile_stdlib_join(cx, args),
        "repeat" => compile_stdlib_repeat(cx, args),
        // Stdlib I/O builtins
        "read_line" => compile_stdlib_read_line(cx),
        "read_file" => compile_stdlib_read_file(cx, args),
        "write_file" => compile_stdlib_write_file(cx, args),
        // Stdlib math builtins
        "pow" => compile_stdlib_pow(cx, args),
        "sqrt" => compile_stdlib_sqrt(cx, args),
        // Async builtins
        "sleep" => compile_builtin_sleep(cx, args),
        "map" => compile_builtin_map(cx, args),
        "filter" => compile_builtin_filter(cx, args),
        "reduce" => compile_builtin_reduce(cx, args),
        // HTTP + JSON builtins
        "http_get" => compile_builtin_http_get(cx, args),
        "http_post" => compile_builtin_http_post(cx, args),
        "json_get" => compile_builtin_json_get(cx, args),
        "json_stringify" => compile_builtin_json_stringify(cx, args),
        // HTTP server builtins
        "http_server" => compile_builtin_http_server(cx, args),
        "route" => compile_builtin_route(cx, args),
        "http_listen" => compile_builtin_http_listen(cx, args),
        "respond" => compile_builtin_respond(cx, args),
        "request_body" => compile_builtin_request_body(cx, args),
        "to_json" => compile_builtin_to_json(cx, args),
        "to_json_array" => compile_builtin_to_json_array(cx, args),
        // Channel builtins
        "channel" => compile_builtin_channel(cx),
        "send" => compile_builtin_send(cx, args),
        "recv" => compile_builtin_recv(cx, args),
        // Mutex builtins
        "mutex" => compile_builtin_mutex(cx, args),
        "mutex_get" => compile_builtin_mutex_get(cx, args),
        "mutex_set" => compile_builtin_mutex_set(cx, args),
        // Derive builtins
        "clone" => compile_clone(cx, args),
        // HashMap builtins
        "hashmap" => compile_builtin_hashmap(cx),
        "hashmap_set" => compile_builtin_hashmap_set(cx, args),
        "hashmap_get" => compile_builtin_hashmap_get(cx, args),
        "hashmap_has" => compile_builtin_hashmap_has(cx, args),
        "hashmap_len" => compile_builtin_hashmap_len(cx, args),
        "hashmap_keys" => compile_builtin_hashmap_keys(cx, args),
        "hashmap_remove" => compile_builtin_hashmap_remove(cx, args),
        // Unsafe builtins — raw pointer operations
        "deref" => compile_builtin_deref(cx, args),
        "store" => compile_builtin_store(cx, args),
        _ => {
            // Check if this is an enum variant construction via UFCS rewrite:
            // Parser transforms Shape.Circle(5.0) into Call { callee: Ident("Circle"), args: [Ident("Shape"), 5.0] }
            if !args.is_empty() {
                if let Expr::Ident(ref first_name) = args[0].node {
                    if let Some(variants) = cx.enum_variants.get(first_name.as_str()) {
                        if let Some(variant_index) = variants.iter().position(|v| v == name) {
                            // This is an enum variant construction with data
                            let data_args = &args[1..]; // skip the enum type name
                            let enum_name = first_name;

                            if let Some(&max_slots) = cx.enum_max_slots.get(enum_name.as_str()) {
                                // Data-carrying enum: allocate tagged union
                                let total_slots = 1 + max_slots; // tag + payload
                                let num_fields_val =
                                    cx.builder.ins().iconst(types::I64, total_slots as i64);
                                let alloc_fid = cx.rt_fns["rt_struct_alloc"];
                                let alloc_fref =
                                    cx.module.declare_func_in_func(alloc_fid, cx.builder.func);
                                let call = cx.builder.ins().call(alloc_fref, &[num_fields_val]);
                                let ptr = cx.builder.inst_results(call)[0];

                                // Store tag at offset 0
                                let tag_val =
                                    cx.builder.ins().iconst(types::I64, variant_index as i64);
                                cx.builder.ins().store(MemFlags::new(), tag_val, ptr, 0);

                                // Get the field types for this variant
                                let _field_tys = cx
                                    .enum_variant_fields
                                    .get(&(enum_name.clone(), name.to_string()))
                                    .cloned()
                                    .unwrap_or_default();

                                // Store each field at offset (i+1)*8
                                for (i, arg) in data_args.iter().enumerate() {
                                    let (val, _tty) = compile_expr(cx, arg)?.unwrap();
                                    let offset = ((i + 1) * 8) as i32;

                                    // Widen/convert to i64 for uniform storage
                                    let val_ty = cx.builder.func.dfg.value_type(val);
                                    let store_val = if val_ty.is_float() && val_ty.bits() == 64 {
                                        cx.builder.ins().bitcast(types::I64, MemFlags::new(), val)
                                    } else if val_ty.is_float() && val_ty.bits() == 32 {
                                        let extended = cx.builder.ins().fpromote(types::F64, val);
                                        cx.builder.ins().bitcast(
                                            types::I64,
                                            MemFlags::new(),
                                            extended,
                                        )
                                    } else if val_ty.bits() < 64 && val_ty.is_int() {
                                        cx.builder.ins().sextend(types::I64, val)
                                    } else {
                                        val
                                    };

                                    cx.builder
                                        .ins()
                                        .store(MemFlags::new(), store_val, ptr, offset);
                                }

                                return Ok(Some((ptr, TurboTy::Enum(enum_name.clone()))));
                            } else {
                                // Unit-only enum, but called with args (shouldn't happen after sema check)
                                let val = cx.builder.ins().iconst(types::I64, variant_index as i64);
                                return Ok(Some((val, TurboTy::Enum(enum_name.clone()))));
                            }
                        }
                    }
                }
            }

            // Check if this is a method call (UFCS: parser rewrites obj.method(args) -> method(obj, args))
            if cx.user_fns.get(name.as_str()).is_none() && !args.is_empty() {
                // Compile first arg to get its type, then check for method
                let (first_val, first_tty) = compile_expr(cx, &args[0])?.unwrap();
                if let TurboTy::Struct(ref type_name) = first_tty {
                    let mangled = format!("{}__{}", type_name, name);
                    if let Some(&fid) = cx.user_fns.get(&mangled) {
                        let mut arg_vals = vec![first_val];
                        for arg in &args[1..] {
                            if let Some((v, _)) = compile_expr(cx, arg)? {
                                arg_vals.push(v);
                            }
                        }
                        let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
                        let call = cx.builder.ins().call(fref, &arg_vals);
                        let results = cx.builder.inst_results(call);
                        let ret_tty = cx
                            .fn_ret_types
                            .get(&mangled)
                            .cloned()
                            .unwrap_or(TurboTy::Unit);
                        if results.is_empty() {
                            return Ok(None);
                        } else {
                            return Ok(Some((results[0], ret_tty)));
                        }
                    }
                }
            }

            // Check if the callee is a variable with a function pointer type (closure)
            if let Some((var, _cl_ty, TurboTy::Fn(ref param_tys, ref ret_ty))) =
                cx.vars.get(name).cloned()
            {
                // Closure is a pair struct: [fn_ptr, env_ptr]
                let closure_ptr = cx.builder.use_var(var);
                let fn_ptr = cx
                    .builder
                    .ins()
                    .load(cx.ptr_type, MemFlags::new(), closure_ptr, 0);
                let env_ptr = cx
                    .builder
                    .ins()
                    .load(cx.ptr_type, MemFlags::new(), closure_ptr, 8);

                // Build the Cranelift signature: env_ptr first, then user params
                let mut sig = cx.module.make_signature();
                sig.call_conv = CallConv::Fast;
                sig.params.push(AbiParam::new(cx.ptr_type)); // env_ptr
                for param_tty in param_tys {
                    let cl_ty = turbo_ty_to_cl_type(param_tty, cx.ptr_type);
                    sig.params.push(AbiParam::new(cl_ty));
                }
                let ret_tty = *ret_ty.clone();
                if ret_tty != TurboTy::Unit {
                    let cl_ret = turbo_ty_to_cl_type(&ret_tty, cx.ptr_type);
                    sig.returns.push(AbiParam::new(cl_ret));
                }

                let sig_ref = cx.builder.import_signature(sig);

                let mut arg_values = vec![env_ptr]; // env_ptr is first hidden arg
                for arg in args {
                    if let Some((val, _)) = compile_expr(cx, arg)? {
                        arg_values.push(val);
                    }
                }

                let call = cx.builder.ins().call_indirect(sig_ref, fn_ptr, &arg_values);
                let results = cx.builder.inst_results(call);
                if results.is_empty() {
                    return Ok(None);
                } else {
                    return Ok(Some((results[0], ret_tty)));
                }
            }

            let func_id = *cx.user_fns.get(name.as_str()).ok_or_else(|| CodegenError {
                code: ErrorCode::E0402,
                message: format!("undefined function: {name}"),
            })?;

            let ret_tty = cx
                .fn_ret_types
                .get(name.as_str())
                .cloned()
                .unwrap_or(TurboTy::Unit);
            let ret_is_result = matches!(&ret_tty, TurboTy::Result(_, _));
            let type_params = cx
                .fn_type_params
                .get(name.as_str())
                .cloned()
                .unwrap_or_default();

            let func_ref = cx.module.declare_func_in_func(func_id, cx.builder.func);
            let sig = cx.builder.func.dfg.ext_funcs[func_ref].signature;
            let param_types: Vec<types::Type> = cx.builder.func.dfg.signatures[sig]
                .params
                .iter()
                .map(|p| p.value_type)
                .collect();
            let mut arg_values = Vec::new();
            let mut arg_ttys = Vec::new();
            for (i, arg) in args.iter().enumerate() {
                if let Some((val, tty)) = compile_expr(cx, arg)? {
                    let val = if i < param_types.len() {
                        let expected = param_types[i];
                        let actual = cx.builder.func.dfg.value_type(val);
                        if actual != expected && actual.is_int() && expected.is_int() {
                            if actual.bits() > expected.bits() {
                                cx.builder.ins().ireduce(expected, val)
                            } else {
                                cx.builder.ins().sextend(expected, val)
                            }
                        } else {
                            val
                        }
                    } else {
                        val
                    };
                    arg_values.push(val);
                    arg_ttys.push(tty);
                }
            }

            // For generic functions, infer the actual return TurboTy from args.
            let actual_ret_tty = if !type_params.is_empty() {
                if let Some(f_def) = cx.fn_asts.get(name.as_str()) {
                    if let Some(ret_ty) = &f_def.return_type {
                        if let TypeExpr::Named(ref ret_name) = ret_ty.node {
                            if type_params.contains(ret_name) {
                                // Find which param has this type parameter
                                let mut inferred = None;
                                for (i, param) in f_def.params.iter().enumerate() {
                                    if let TypeExpr::Named(ref pname) = param.ty.node {
                                        if pname == ret_name {
                                            if i < arg_ttys.len() {
                                                inferred = Some(arg_ttys[i].clone());
                                            }
                                            break;
                                        }
                                    }
                                }
                                inferred.unwrap_or(ret_tty)
                            } else {
                                ret_tty
                            }
                        } else {
                            ret_tty
                        }
                    } else {
                        ret_tty
                    }
                } else {
                    ret_tty
                }
            } else {
                ret_tty
            };

            // For generic functions, widen bool args (I8) to I64 since
            // the generic function's parameter is compiled as I64.
            if !type_params.is_empty() {
                for val in &mut arg_values {
                    let vty = cx.builder.func.dfg.value_type(*val);
                    if vty.bits() < 64 {
                        *val = cx.builder.ins().sextend(types::I64, *val);
                    }
                }
            }

            // Try inline expansion: inline the callee body at this call site
            // if we haven't exceeded the depth limit and the function is inlineable.
            // Skip inlining for generic functions (type parameter inference needs
            // normal call path) and for Result-returning functions (heap-allocated
            // tagged unions require proper call/return semantics).
            if cx.inline_depth < MAX_INLINE_DEPTH && type_params.is_empty() && !ret_is_result {
                if let Some(callee_def) = cx.fn_asts.get(name.as_str()).cloned() {
                    if !has_return(&callee_def.body.node)
                        && callee_def.params.len() == arg_values.len()
                    {
                        // Save and restore outer variable scope so inlined
                        // parameter bindings don't leak out.
                        let saved_vars = cx.vars.clone();
                        let saved_depth = cx.inline_depth;
                        cx.inline_depth += 1;

                        // Bind each parameter to the already-compiled argument value.
                        for (i, param) in callee_def.params.iter().enumerate() {
                            let cl_ty = resolve_cl_type(
                                &param.ty.node,
                                cx.ptr_type,
                                cx.enum_variants,
                                &type_params,
                            )?;
                            let turbo_ty =
                                turbo_ty_from_type_expr(&param.ty.node, cx.enum_variants);
                            let var = cx.fresh_var(cl_ty, turbo_ty.clone());
                            cx.builder.def_var(var, arg_values[i]);
                            cx.vars.insert(param.name.clone(), (var, cl_ty, turbo_ty));
                        }

                        let result = compile_expr(cx, &callee_def.body)?;

                        cx.vars = saved_vars;
                        cx.inline_depth = saved_depth;

                        return Ok(result);
                    }
                }
            }

            // Fall back to a normal call instruction.
            let call = cx.builder.ins().call(func_ref, &arg_values);
            let results = cx.builder.inst_results(call);
            if results.is_empty() {
                Ok(None)
            } else {
                Ok(Some((results[0], actual_ret_tty)))
            }
        }
    }
}

// ── Unit tests ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::{CStr, CString};

    // ── Helper: compile & run a Turbo program via JIT ──────────────

    fn jit_run_source(source: &str) {
        let (tokens, lex_errors) = turbo_lexer::tokenize(source);
        assert!(lex_errors.is_empty(), "Lex errors: {:?}", lex_errors);
        let (module, parse_errors) = turbo_parser::parse(tokens);
        assert!(parse_errors.is_empty(), "Parse errors: {:?}", parse_errors);
        let sema_errors = turbo_sema::check(&module);
        assert!(sema_errors.is_empty(), "Sema errors: {:?}", sema_errors);
        jit_run(&module).expect("JIT compilation/execution failed");
    }

    // ================================================================
    // 1. Runtime function tests — direct calls to extern "C" functions
    // ================================================================

    #[test]
    fn test_rt_array_alloc_basic() {
        let arr = rt_array_alloc(5);
        assert!(!arr.is_null());
        // Length should be stored at the start
        let len = unsafe { *(arr as *const i64) };
        assert_eq!(len, 5);
        // All elements should be zero-initialized
        for i in 0..5 {
            let val = unsafe { *((arr as *const i64).add(1 + i)) };
            assert_eq!(val, 0, "element {} should be zero", i);
        }
    }

    #[test]
    fn test_rt_array_get_set() {
        let arr = rt_array_alloc(3);
        // Set values
        let arr = rt_array_set(arr, 0, 42);
        let arr = rt_array_set(arr, 1, 100);
        let arr = rt_array_set(arr, 2, -7);
        // Get values back
        assert_eq!(rt_array_get(arr, 0), 42);
        assert_eq!(rt_array_get(arr, 1), 100);
        assert_eq!(rt_array_get(arr, 2), -7);
    }

    #[test]
    fn test_rt_array_len() {
        let arr = rt_array_alloc(10);
        assert_eq!(rt_array_len(arr), 10);

        let arr2 = rt_array_alloc(0);
        assert_eq!(rt_array_len(arr2), 0);
    }

    #[test]
    fn test_rt_array_cow_on_shared() {
        // Allocate array and set initial values
        let arr = rt_array_alloc(2);
        let arr = rt_array_set(arr, 0, 10);
        let arr = rt_array_set(arr, 1, 20);

        // Increment refcount to simulate sharing
        rt_retain(arr);

        // Mutating should COW — return a new pointer
        let arr2 = rt_array_set(arr, 0, 99);
        assert_ne!(arr as *const u8, arr2 as *const u8);

        // Original should be unchanged
        assert_eq!(rt_array_get(arr, 0), 10);
        // New copy should have the mutation
        assert_eq!(rt_array_get(arr2, 0), 99);
        // Unmodified element should be copied
        assert_eq!(rt_array_get(arr2, 1), 20);
    }

    #[test]
    fn test_rt_struct_alloc() {
        let s = rt_struct_alloc(3);
        assert!(!s.is_null());
        // Fields should be zero-initialized
        for i in 0..3 {
            let val = unsafe { *((s as *const i64).add(i)) };
            assert_eq!(val, 0, "field {} should be zero", i);
        }
        // Set and read fields via raw pointer
        unsafe {
            *(s as *mut i64) = 42;
            *((s as *mut i64).add(1)) = 100;
            *((s as *mut i64).add(2)) = -5;
        }
        assert_eq!(unsafe { *(s as *const i64) }, 42);
        assert_eq!(unsafe { *((s as *const i64).add(1)) }, 100);
        assert_eq!(unsafe { *((s as *const i64).add(2)) }, -5);
    }

    #[test]
    fn test_rt_result_ok_and_accessors() {
        let r = rt_result_ok(42);
        assert_eq!(rt_result_tag(r), 0); // tag 0 = ok
        assert_eq!(rt_result_value(r), 42);
    }

    #[test]
    fn test_rt_result_err_and_accessors() {
        let r = rt_result_err(99);
        assert_eq!(rt_result_tag(r), 1); // tag 1 = err
        assert_eq!(rt_result_value(r), 99);
    }

    #[test]
    fn test_rt_option_some_and_accessors() {
        let o = rt_option_some(77);
        assert_eq!(rt_option_tag(o), 1); // tag 1 = some
        assert_eq!(rt_option_value(o), 77);
    }

    #[test]
    fn test_rt_option_none_and_accessors() {
        let o = rt_option_none();
        assert_eq!(rt_option_tag(o), 0); // tag 0 = none
        assert_eq!(rt_option_value(o), 0);
    }

    // ── String runtime functions ────────────────────────────────────

    #[test]
    fn test_rt_str_concat() {
        let a = CString::new("hello ").unwrap();
        let b = CString::new("world").unwrap();
        let result = rt_str_concat(a.as_ptr() as *const u8, b.as_ptr() as *const u8);
        let s = unsafe { CStr::from_ptr(result as *const std::ffi::c_char) };
        assert_eq!(s.to_str().unwrap(), "hello world");
    }

    #[test]
    fn test_rt_str_concat_with_null() {
        let a = CString::new("hi").unwrap();
        let result = rt_str_concat(a.as_ptr() as *const u8, std::ptr::null());
        let s = unsafe { CStr::from_ptr(result as *const std::ffi::c_char) };
        assert_eq!(s.to_str().unwrap(), "hi");

        let result2 = rt_str_concat(std::ptr::null(), a.as_ptr() as *const u8);
        let s2 = unsafe { CStr::from_ptr(result2 as *const std::ffi::c_char) };
        assert_eq!(s2.to_str().unwrap(), "hi");
    }

    #[test]
    fn test_rt_str_eq() {
        let a = CString::new("hello").unwrap();
        let b = CString::new("hello").unwrap();
        let c = CString::new("world").unwrap();
        assert_eq!(
            rt_str_eq(a.as_ptr() as *const u8, b.as_ptr() as *const u8),
            1
        );
        assert_eq!(
            rt_str_eq(a.as_ptr() as *const u8, c.as_ptr() as *const u8),
            0
        );
    }

    #[test]
    fn test_rt_str_len() {
        let s = CString::new("hello").unwrap();
        assert_eq!(rt_str_len(s.as_ptr() as *const u8), 5);
        assert_eq!(rt_str_len(std::ptr::null()), 0);
    }

    #[test]
    fn test_rt_str_upper_lower() {
        let s = CString::new("Hello World").unwrap();
        let upper = rt_str_upper(s.as_ptr() as *const u8);
        let upper_str = unsafe { CStr::from_ptr(upper as *const std::ffi::c_char) };
        assert_eq!(upper_str.to_str().unwrap(), "HELLO WORLD");

        let lower = rt_str_lower(s.as_ptr() as *const u8);
        let lower_str = unsafe { CStr::from_ptr(lower as *const std::ffi::c_char) };
        assert_eq!(lower_str.to_str().unwrap(), "hello world");
    }

    #[test]
    fn test_rt_str_trim() {
        let s = CString::new("  hello  ").unwrap();
        let trimmed = rt_str_trim(s.as_ptr() as *const u8);
        let t = unsafe { CStr::from_ptr(trimmed as *const std::ffi::c_char) };
        assert_eq!(t.to_str().unwrap(), "hello");
    }

    #[test]
    fn test_rt_str_starts_ends_with() {
        let s = CString::new("hello world").unwrap();
        let prefix = CString::new("hello").unwrap();
        let suffix = CString::new("world").unwrap();
        let bad = CString::new("xyz").unwrap();

        assert_eq!(
            rt_str_starts_with(s.as_ptr() as *const u8, prefix.as_ptr() as *const u8),
            1
        );
        assert_eq!(
            rt_str_starts_with(s.as_ptr() as *const u8, bad.as_ptr() as *const u8),
            0
        );
        assert_eq!(
            rt_str_ends_with(s.as_ptr() as *const u8, suffix.as_ptr() as *const u8),
            1
        );
        assert_eq!(
            rt_str_ends_with(s.as_ptr() as *const u8, bad.as_ptr() as *const u8),
            0
        );
    }

    #[test]
    fn test_rt_str_contains_and_index_of() {
        let s = CString::new("hello world").unwrap();
        let sub = CString::new("world").unwrap();
        let missing = CString::new("xyz").unwrap();

        assert_eq!(
            rt_str_contains(s.as_ptr() as *const u8, sub.as_ptr() as *const u8),
            1
        );
        assert_eq!(
            rt_str_contains(s.as_ptr() as *const u8, missing.as_ptr() as *const u8),
            0
        );
        assert_eq!(
            rt_str_index_of(s.as_ptr() as *const u8, sub.as_ptr() as *const u8),
            6
        );
        assert_eq!(
            rt_str_index_of(s.as_ptr() as *const u8, missing.as_ptr() as *const u8),
            -1
        );
    }

    #[test]
    fn test_rt_str_replace() {
        let s = CString::new("hello world").unwrap();
        let from = CString::new("world").unwrap();
        let to = CString::new("rust").unwrap();
        let result = rt_str_replace(
            s.as_ptr() as *const u8,
            from.as_ptr() as *const u8,
            to.as_ptr() as *const u8,
        );
        let r = unsafe { CStr::from_ptr(result as *const std::ffi::c_char) };
        assert_eq!(r.to_str().unwrap(), "hello rust");
    }

    #[test]
    fn test_rt_str_repeat() {
        let s = CString::new("ab").unwrap();
        let result = rt_str_repeat(s.as_ptr() as *const u8, 3);
        let r = unsafe { CStr::from_ptr(result as *const std::ffi::c_char) };
        assert_eq!(r.to_str().unwrap(), "ababab");

        // Zero repetitions
        let result0 = rt_str_repeat(s.as_ptr() as *const u8, 0);
        let r0 = unsafe { CStr::from_ptr(result0 as *const std::ffi::c_char) };
        assert_eq!(r0.to_str().unwrap(), "");
    }

    #[test]
    fn test_rt_str_split() {
        let s = CString::new("a,b,c").unwrap();
        let sep = CString::new(",").unwrap();
        let arr = rt_str_split(s.as_ptr() as *const u8, sep.as_ptr() as *const u8);
        assert_eq!(rt_array_len(arr), 3);
        // Check each element
        let elem0_ptr = unsafe { *((arr as *const i64).add(1)) } as *const u8;
        let elem0 = unsafe { CStr::from_ptr(elem0_ptr as *const std::ffi::c_char) };
        assert_eq!(elem0.to_str().unwrap(), "a");

        let elem2_ptr = unsafe { *((arr as *const i64).add(3)) } as *const u8;
        let elem2 = unsafe { CStr::from_ptr(elem2_ptr as *const std::ffi::c_char) };
        assert_eq!(elem2.to_str().unwrap(), "c");
    }

    // ── Conversion runtime functions ────────────────────────────────

    #[test]
    fn test_rt_i64_to_str() {
        let result = rt_i64_to_str(42);
        let s = unsafe { CStr::from_ptr(result as *const std::ffi::c_char) };
        assert_eq!(s.to_str().unwrap(), "42");

        let neg = rt_i64_to_str(-100);
        let s2 = unsafe { CStr::from_ptr(neg as *const std::ffi::c_char) };
        assert_eq!(s2.to_str().unwrap(), "-100");
    }

    #[test]
    fn test_rt_f64_to_str() {
        let result = rt_f64_to_str(3.14);
        let s = unsafe { CStr::from_ptr(result as *const std::ffi::c_char) };
        assert_eq!(s.to_str().unwrap(), "3.14");
    }

    #[test]
    fn test_rt_bool_to_str() {
        let t = rt_bool_to_str(1);
        let s = unsafe { CStr::from_ptr(t as *const std::ffi::c_char) };
        assert_eq!(s.to_str().unwrap(), "true");

        let f = rt_bool_to_str(0);
        let s2 = unsafe { CStr::from_ptr(f as *const std::ffi::c_char) };
        assert_eq!(s2.to_str().unwrap(), "false");
    }

    // ── Math runtime functions ──────────────────────────────────────

    #[test]
    fn test_rt_pow() {
        assert_eq!(rt_pow(2, 10), 1024);
        assert_eq!(rt_pow(3, 0), 1);
        assert_eq!(rt_pow(5, 1), 5);
        assert_eq!(rt_pow(2, -1), 0); // negative exponent returns 0
    }

    #[test]
    fn test_rt_sqrt() {
        assert!((rt_sqrt(4.0) - 2.0).abs() < f64::EPSILON);
        assert!((rt_sqrt(9.0) - 3.0).abs() < f64::EPSILON);
        assert!((rt_sqrt(2.0) - std::f64::consts::SQRT_2).abs() < 1e-10);
    }

    // ── ARC runtime functions ──────────────────────────────────────

    #[test]
    fn test_rt_retain_release() {
        let arr = rt_array_alloc(1);
        // Refcount starts at 1 (set by rt_array_alloc)
        let rc_ptr = unsafe { arr.sub(8) as *const i64 };
        assert_eq!(unsafe { *rc_ptr }, 1);

        rt_retain(arr);
        assert_eq!(unsafe { *rc_ptr }, 2);

        rt_release(arr);
        assert_eq!(unsafe { *rc_ptr }, 1);

        // Null should not crash
        rt_retain(std::ptr::null_mut());
        rt_release(std::ptr::null_mut());
    }

    // ── Mutex runtime functions ────────────────────────────────────

    #[test]
    fn test_rt_mutex_create_get_set() {
        let m = rt_mutex_create(42);
        assert_eq!(rt_mutex_get(m), 42);
        rt_mutex_set(m, 100);
        assert_eq!(rt_mutex_get(m), 100);
    }

    #[test]
    fn test_rt_mutex_clone() {
        let m = rt_mutex_create(10);
        let m2 = rt_mutex_clone(m);

        // Both should see the same value
        assert_eq!(rt_mutex_get(m), 10);
        assert_eq!(rt_mutex_get(m2), 10);

        // Mutating through one clone should be visible from the other
        rt_mutex_set(m, 99);
        assert_eq!(rt_mutex_get(m2 as *const u8), 99);
    }

    // ── HashMap runtime functions ──────────────────────────────────

    #[test]
    fn test_rt_hashmap_basic() {
        let map = rt_hashmap_new();
        assert_eq!(rt_hashmap_len(map), 0);

        let key = CString::new("name").unwrap();
        let val = CString::new("turbo").unwrap();
        rt_hashmap_set(map, key.as_ptr() as *const u8, val.as_ptr() as *const u8);

        assert_eq!(rt_hashmap_len(map), 1);
        assert_eq!(rt_hashmap_has(map, key.as_ptr() as *const u8), 1);

        let got = rt_hashmap_get(map, key.as_ptr() as *const u8);
        assert!(!got.is_null());
        let got_str = unsafe { CStr::from_ptr(got as *const std::ffi::c_char) };
        assert_eq!(got_str.to_str().unwrap(), "turbo");

        // Key not present
        let missing = CString::new("nope").unwrap();
        assert_eq!(rt_hashmap_has(map, missing.as_ptr() as *const u8), 0);
        assert!(rt_hashmap_get(map, missing.as_ptr() as *const u8).is_null());
    }

    #[test]
    fn test_rt_hashmap_remove_and_keys() {
        let map = rt_hashmap_new();
        let k1 = CString::new("a").unwrap();
        let k2 = CString::new("b").unwrap();
        let v = CString::new("1").unwrap();
        rt_hashmap_set(map, k1.as_ptr() as *const u8, v.as_ptr() as *const u8);
        rt_hashmap_set(map, k2.as_ptr() as *const u8, v.as_ptr() as *const u8);

        assert_eq!(rt_hashmap_len(map), 2);

        let keys = rt_hashmap_keys(map);
        assert_eq!(rt_array_len(keys), 2);

        rt_hashmap_remove(map, k1.as_ptr() as *const u8);
        assert_eq!(rt_hashmap_len(map), 1);
        assert_eq!(rt_hashmap_has(map, k1.as_ptr() as *const u8), 0);
        assert_eq!(rt_hashmap_has(map, k2.as_ptr() as *const u8), 1);
    }

    // ── JSON runtime functions ─────────────────────────────────────

    #[test]
    fn test_rt_json_get_string_value() {
        let json = CString::new(r#"{"name":"turbo","version":"1.0"}"#).unwrap();
        let key = CString::new("name").unwrap();
        let result = rt_json_get(json.as_ptr() as *const u8, key.as_ptr() as *const u8);
        let s = unsafe { CStr::from_ptr(result as *const std::ffi::c_char) };
        assert_eq!(s.to_str().unwrap(), "turbo");
    }

    #[test]
    fn test_rt_json_get_number_value() {
        let json = CString::new(r#"{"count":42}"#).unwrap();
        let key = CString::new("count").unwrap();
        let result = rt_json_get(json.as_ptr() as *const u8, key.as_ptr() as *const u8);
        let s = unsafe { CStr::from_ptr(result as *const std::ffi::c_char) };
        assert_eq!(s.to_str().unwrap(), "42");
    }

    #[test]
    fn test_rt_json_get_missing_key() {
        let json = CString::new(r#"{"a":1}"#).unwrap();
        let key = CString::new("missing").unwrap();
        let result = rt_json_get(json.as_ptr() as *const u8, key.as_ptr() as *const u8);
        let s = unsafe { CStr::from_ptr(result as *const std::ffi::c_char) };
        assert_eq!(s.to_str().unwrap(), "");
    }

    #[test]
    fn test_rt_json_stringify() {
        let key = CString::new("lang").unwrap();
        let val = CString::new("turbo").unwrap();
        let result = rt_json_stringify(key.as_ptr() as *const u8, val.as_ptr() as *const u8);
        let s = unsafe { CStr::from_ptr(result as *const std::ffi::c_char) };
        assert_eq!(s.to_str().unwrap(), r#"{"lang":"turbo"}"#);
    }

    // ── Type conversion tests ──────────────────────────────────────

    #[test]
    fn test_turbo_ty_from_named_types() {
        let empty_enum: HashMap<String, Vec<String>> = HashMap::new();

        let int_ty = turbo_ty_from_type_expr(&TypeExpr::Named("i64".to_string()), &empty_enum);
        assert_eq!(int_ty, TurboTy::Int);

        let float_ty = turbo_ty_from_type_expr(&TypeExpr::Named("f64".to_string()), &empty_enum);
        assert_eq!(float_ty, TurboTy::Float);

        let bool_ty = turbo_ty_from_type_expr(&TypeExpr::Named("bool".to_string()), &empty_enum);
        assert_eq!(bool_ty, TurboTy::Bool);

        let str_ty = turbo_ty_from_type_expr(&TypeExpr::Named("str".to_string()), &empty_enum);
        assert_eq!(str_ty, TurboTy::Str);

        let unit_ty = turbo_ty_from_type_expr(&TypeExpr::Unit, &empty_enum);
        assert_eq!(unit_ty, TurboTy::Unit);
    }

    #[test]
    fn test_turbo_ty_struct_vs_enum() {
        let mut enum_variants: HashMap<String, Vec<String>> = HashMap::new();
        enum_variants.insert(
            "Color".to_string(),
            vec!["Red".to_string(), "Green".to_string()],
        );

        let enum_ty =
            turbo_ty_from_type_expr(&TypeExpr::Named("Color".to_string()), &enum_variants);
        assert_eq!(enum_ty, TurboTy::Enum("Color".to_string()));

        let struct_ty =
            turbo_ty_from_type_expr(&TypeExpr::Named("Point".to_string()), &enum_variants);
        assert_eq!(struct_ty, TurboTy::Struct("Point".to_string()));
    }

    #[test]
    fn test_turbo_ty_array() {
        let empty_enum: HashMap<String, Vec<String>> = HashMap::new();
        let arr_ty = turbo_ty_from_type_expr(
            &TypeExpr::Array(Box::new(Spanned {
                node: TypeExpr::Named("i64".to_string()),
                span: 0..0,
            })),
            &empty_enum,
        );
        assert_eq!(arr_ty, TurboTy::Array(Box::new(TurboTy::Int)));
    }

    #[test]
    fn test_turbo_ty_result_and_optional() {
        let empty_enum: HashMap<String, Vec<String>> = HashMap::new();

        let result_ty = turbo_ty_from_type_expr(
            &TypeExpr::Result {
                ok_type: Box::new(Spanned {
                    node: TypeExpr::Named("i64".to_string()),
                    span: 0..0,
                }),
                err_type: Box::new(Spanned {
                    node: TypeExpr::Named("str".to_string()),
                    span: 0..0,
                }),
            },
            &empty_enum,
        );
        assert_eq!(
            result_ty,
            TurboTy::Result(Box::new(TurboTy::Int), Box::new(TurboTy::Str))
        );

        let opt_ty = turbo_ty_from_type_expr(
            &TypeExpr::Optional(Box::new(Spanned {
                node: TypeExpr::Named("i64".to_string()),
                span: 0..0,
            })),
            &empty_enum,
        );
        assert_eq!(opt_ty, TurboTy::Optional(Box::new(TurboTy::Int)));
    }

    #[test]
    fn test_turbo_ty_type_params_use_int() {
        let empty_enum: HashMap<String, Vec<String>> = HashMap::new();
        let type_params = vec!["T".to_string()];
        let ty = turbo_ty_from_type_expr_with_params(
            &TypeExpr::Named("T".to_string()),
            &empty_enum,
            &type_params,
        );
        assert_eq!(ty, TurboTy::Int);
    }

    // ── CodegenError tests ─────────────────────────────────────────

    #[test]
    fn test_codegen_error_display() {
        let err = CodegenError {
            code: ErrorCode::E0400,
            message: "something broke".to_string(),
        };
        assert_eq!(format!("{}", err), "codegen error: something broke");
    }

    // ================================================================
    // 2. End-to-end JIT compilation tests
    // ================================================================

    #[test]
    fn test_jit_basic_arithmetic() {
        // assert_eq will process-exit on failure, so we only test
        // programs that should succeed
        jit_run_source(
            r#"fn main() {
                assert_eq(2 + 3, 5)
                assert_eq(10 - 4, 6)
                assert_eq(3 * 7, 21)
                assert_eq(20 / 4, 5)
            }"#,
        );
    }

    #[test]
    fn test_jit_let_bindings() {
        jit_run_source(
            r#"fn main() {
                let x = 10
                let y = 20
                assert_eq(x + y, 30)
            }"#,
        );
    }

    #[test]
    fn test_jit_mutable_variable() {
        jit_run_source(
            r#"fn main() {
                let mut x = 1
                x = 42
                assert_eq(x, 42)
            }"#,
        );
    }

    #[test]
    fn test_jit_string_operations() {
        jit_run_source(
            r#"fn main() {
                let s = "hello" + " " + "world"
                assert_eq(s, "hello world")
            }"#,
        );
    }

    #[test]
    fn test_jit_function_call() {
        jit_run_source(
            r#"fn add(a: i64, b: i64) -> i64 { a + b }
            fn main() {
                assert_eq(add(3, 4), 7)
            }"#,
        );
    }

    #[test]
    fn test_jit_if_else() {
        jit_run_source(
            r#"fn main() {
                let x = if true { 10 } else { 20 }
                assert_eq(x, 10)
                let y = if false { 10 } else { 20 }
                assert_eq(y, 20)
            }"#,
        );
    }

    #[test]
    fn test_jit_while_loop() {
        jit_run_source(
            r#"fn main() {
                let mut sum = 0
                let mut i = 1
                while i <= 10 {
                    sum = sum + i
                    i = i + 1
                }
                assert_eq(sum, 55)
            }"#,
        );
    }

    #[test]
    fn test_jit_for_loop() {
        jit_run_source(
            r#"fn main() {
                let mut sum = 0
                for i in 0..5 {
                    sum = sum + i
                }
                assert_eq(sum, 10)
            }"#,
        );
    }

    #[test]
    fn test_jit_array_literal() {
        jit_run_source(
            r#"fn main() {
                let arr = [10, 20, 30]
                assert_eq(arr[0], 10)
                assert_eq(arr[1], 20)
                assert_eq(arr[2], 30)
                assert_eq(len(arr), 3)
            }"#,
        );
    }

    #[test]
    fn test_jit_struct_basic() {
        jit_run_source(
            r#"struct Point { x: i64, y: i64 }
            fn main() {
                let p = Point { x: 3, y: 4 }
                assert_eq(p.x, 3)
                assert_eq(p.y, 4)
            }"#,
        );
    }

    #[test]
    fn test_jit_enum_basic() {
        jit_run_source(
            r#"type Color { Red, Green, Blue }
            fn main() {
                let c = Color.Red
                let g = Color.Green
                assert_ne(c, g)
            }"#,
        );
    }

    #[test]
    fn test_jit_match_expression() {
        jit_run_source(
            r#"fn main() {
                let x = 2
                let result = match x {
                    1 => 10
                    2 => 20
                    _ => 0
                }
                assert_eq(result, 20)
            }"#,
        );
    }

    #[test]
    fn test_jit_recursion() {
        jit_run_source(
            r#"fn fib(n: i64) -> i64 {
                if n <= 1 { n } else { fib(n - 1) + fib(n - 2) }
            }
            fn main() {
                assert_eq(fib(10), 55)
            }"#,
        );
    }

    #[test]
    fn test_jit_no_main_error() {
        let source = "fn helper() -> i64 { 42 }";
        let (tokens, _) = turbo_lexer::tokenize(source);
        let (module, _) = turbo_parser::parse(tokens);
        let result = jit_run(&module);
        assert!(result.is_err());
        assert!(
            result.unwrap_err().message.contains("main"),
            "Error should mention missing main"
        );
    }

    #[test]
    fn test_jit_boolean_logic() {
        jit_run_source(
            r#"fn main() {
                assert_eq(true && true, true)
                assert_eq(true && false, false)
                assert_eq(false || true, true)
                assert_eq(false || false, false)
                assert_eq(!true, false)
                assert_eq(!false, true)
            }"#,
        );
    }

    #[test]
    fn test_jit_comparison_operators() {
        jit_run_source(
            r#"fn main() {
                assert_eq(1 < 2, true)
                assert_eq(2 > 1, true)
                assert_eq(1 <= 1, true)
                assert_eq(1 >= 1, true)
                assert_eq(1 == 1, true)
                assert_eq(1 != 2, true)
            }"#,
        );
    }

    #[test]
    fn test_jit_string_builtins() {
        jit_run_source(
            r#"fn main() {
                assert_eq(len("hello"), 5)
                assert_eq(upper("hello"), "HELLO")
                assert_eq(lower("HELLO"), "hello")
                assert_eq(trim("  hi  "), "hi")
                assert_eq(contains("hello world", "world"), true)
                assert_eq(starts_with("hello", "hel"), true)
                assert_eq(ends_with("hello", "llo"), true)
            }"#,
        );
    }

    #[test]
    fn test_jit_nested_function_calls() {
        jit_run_source(
            r#"fn double(x: i64) -> i64 { x * 2 }
            fn add_one(x: i64) -> i64 { x + 1 }
            fn main() {
                assert_eq(double(add_one(4)), 10)
                assert_eq(add_one(double(4)), 9)
            }"#,
        );
    }

    // ================================================================
    // Additional runtime function tests
    // ================================================================

    #[test]
    fn test_rt_str_join() {
        // Build a string array: [len=2]["hello", "world"]
        let arr = rt_array_alloc(2);
        let s1 = CString::new("hello").unwrap();
        let s2 = CString::new("world").unwrap();
        let arr = rt_array_set(arr, 0, s1.into_raw() as i64);
        let arr = rt_array_set(arr, 1, s2.into_raw() as i64);

        let sep = CString::new(", ").unwrap();
        let result = rt_str_join(arr, sep.as_ptr() as *const u8);
        let s = unsafe { CStr::from_ptr(result as *const std::ffi::c_char) };
        assert_eq!(s.to_str().unwrap(), "hello, world");
    }

    #[test]
    fn test_rt_str_char_at() {
        let s = CString::new("abcde").unwrap();
        let ch = rt_str_char_at(s.as_ptr() as *const u8, 2);
        let r = unsafe { CStr::from_ptr(ch as *const std::ffi::c_char) };
        assert_eq!(r.to_str().unwrap(), "c");

        let ch0 = rt_str_char_at(s.as_ptr() as *const u8, 0);
        let r0 = unsafe { CStr::from_ptr(ch0 as *const std::ffi::c_char) };
        assert_eq!(r0.to_str().unwrap(), "a");
    }

    #[test]
    fn test_rt_str_eq_with_nulls() {
        // Both null should be equal
        assert_eq!(rt_str_eq(std::ptr::null(), std::ptr::null()), 1);

        // Null vs non-null with empty string
        let empty = CString::new("").unwrap();
        assert_eq!(rt_str_eq(std::ptr::null(), empty.as_ptr() as *const u8), 1);
        assert_eq!(rt_str_eq(empty.as_ptr() as *const u8, std::ptr::null()), 1);

        // Null vs non-empty
        let hello = CString::new("hello").unwrap();
        assert_eq!(rt_str_eq(std::ptr::null(), hello.as_ptr() as *const u8), 0);
    }

    #[test]
    fn test_rt_str_concat_both_null() {
        let result = rt_str_concat(std::ptr::null(), std::ptr::null());
        let s = unsafe { CStr::from_ptr(result as *const std::ffi::c_char) };
        assert_eq!(s.to_str().unwrap(), "");
    }

    #[test]
    fn test_rt_str_repeat_negative() {
        let s = CString::new("x").unwrap();
        let result = rt_str_repeat(s.as_ptr() as *const u8, -5);
        let r = unsafe { CStr::from_ptr(result as *const std::ffi::c_char) };
        assert_eq!(r.to_str().unwrap(), "");
    }

    #[test]
    fn test_rt_array_set_no_cow_when_unshared() {
        let arr = rt_array_alloc(3);
        // refcount is 1, so set should return same pointer (no COW)
        let arr2 = rt_array_set(arr, 0, 42);
        assert_eq!(arr as *const u8, arr2 as *const u8);
    }

    #[test]
    fn test_rt_struct_alloc_single_field() {
        let s = rt_struct_alloc(1);
        assert!(!s.is_null());
        // Should be zero-initialized
        assert_eq!(unsafe { *(s as *const i64) }, 0);
        // Set and read back
        unsafe {
            *(s as *mut i64) = 999;
        }
        assert_eq!(unsafe { *(s as *const i64) }, 999);
    }

    #[test]
    fn test_rt_result_ok_negative_value() {
        let r = rt_result_ok(-42);
        assert_eq!(rt_result_tag(r), 0);
        assert_eq!(rt_result_value(r), -42);
    }

    #[test]
    fn test_rt_option_some_zero() {
        // Some(0) should still have tag=1 (some)
        let o = rt_option_some(0);
        assert_eq!(rt_option_tag(o), 1);
        assert_eq!(rt_option_value(o), 0);
    }

    #[test]
    fn test_rt_retain_release_multiple() {
        let arr = rt_array_alloc(2);
        let rc_ptr = unsafe { arr.sub(8) as *const i64 };
        assert_eq!(unsafe { *rc_ptr }, 1);

        rt_retain(arr);
        rt_retain(arr);
        assert_eq!(unsafe { *rc_ptr }, 3);

        rt_release(arr);
        assert_eq!(unsafe { *rc_ptr }, 2);

        rt_release(arr);
        assert_eq!(unsafe { *rc_ptr }, 1);

        rt_release(arr);
        assert_eq!(unsafe { *rc_ptr }, 0);
    }

    #[test]
    fn test_rt_hashmap_overwrite() {
        let map = rt_hashmap_new();
        let key = CString::new("k").unwrap();
        let v1 = CString::new("first").unwrap();
        let v2 = CString::new("second").unwrap();

        rt_hashmap_set(map, key.as_ptr() as *const u8, v1.as_ptr() as *const u8);
        rt_hashmap_set(map, key.as_ptr() as *const u8, v2.as_ptr() as *const u8);

        assert_eq!(rt_hashmap_len(map), 1);
        let got = rt_hashmap_get(map, key.as_ptr() as *const u8);
        let s = unsafe { CStr::from_ptr(got as *const std::ffi::c_char) };
        assert_eq!(s.to_str().unwrap(), "second");
    }

    #[test]
    fn test_rt_hashmap_empty_keys() {
        let map = rt_hashmap_new();
        let keys = rt_hashmap_keys(map);
        assert_eq!(rt_array_len(keys), 0);
    }

    #[test]
    fn test_rt_pow_large_exponent() {
        assert_eq!(rt_pow(2, 20), 1048576);
        assert_eq!(rt_pow(1, 100), 1);
        assert_eq!(rt_pow(0, 5), 0);
    }

    #[test]
    fn test_rt_sqrt_zero_and_one() {
        assert!((rt_sqrt(0.0) - 0.0).abs() < f64::EPSILON);
        assert!((rt_sqrt(1.0) - 1.0).abs() < f64::EPSILON);
        assert!(rt_sqrt(f64::NAN).is_nan());
    }

    #[test]
    fn test_rt_i64_to_str_zero() {
        let result = rt_i64_to_str(0);
        let s = unsafe { CStr::from_ptr(result as *const std::ffi::c_char) };
        assert_eq!(s.to_str().unwrap(), "0");
    }

    #[test]
    fn test_rt_f64_to_str_negative() {
        let result = rt_f64_to_str(-1.5);
        let s = unsafe { CStr::from_ptr(result as *const std::ffi::c_char) };
        assert_eq!(s.to_str().unwrap(), "-1.5");
    }

    #[test]
    fn test_rt_str_split_single_element() {
        let s = CString::new("hello").unwrap();
        let sep = CString::new(",").unwrap();
        let arr = rt_str_split(s.as_ptr() as *const u8, sep.as_ptr() as *const u8);
        assert_eq!(rt_array_len(arr), 1);
        let elem_ptr = unsafe { *((arr as *const i64).add(1)) } as *const u8;
        let elem = unsafe { CStr::from_ptr(elem_ptr as *const std::ffi::c_char) };
        assert_eq!(elem.to_str().unwrap(), "hello");
    }

    #[test]
    fn test_rt_str_replace_no_match() {
        let s = CString::new("hello").unwrap();
        let from = CString::new("xyz").unwrap();
        let to = CString::new("abc").unwrap();
        let result = rt_str_replace(
            s.as_ptr() as *const u8,
            from.as_ptr() as *const u8,
            to.as_ptr() as *const u8,
        );
        let r = unsafe { CStr::from_ptr(result as *const std::ffi::c_char) };
        assert_eq!(r.to_str().unwrap(), "hello");
    }

    #[test]
    fn test_rt_str_contains_empty_substring() {
        let s = CString::new("hello").unwrap();
        let empty = CString::new("").unwrap();
        // Every string contains the empty string
        assert_eq!(
            rt_str_contains(s.as_ptr() as *const u8, empty.as_ptr() as *const u8),
            1
        );
    }

    #[test]
    fn test_rt_str_index_of_at_start() {
        let s = CString::new("hello").unwrap();
        let sub = CString::new("hel").unwrap();
        assert_eq!(
            rt_str_index_of(s.as_ptr() as *const u8, sub.as_ptr() as *const u8),
            0
        );
    }

    #[test]
    fn test_rt_json_get_boolean_value() {
        let json = CString::new(r#"{"active":true}"#).unwrap();
        let key = CString::new("active").unwrap();
        let result = rt_json_get(json.as_ptr() as *const u8, key.as_ptr() as *const u8);
        let s = unsafe { CStr::from_ptr(result as *const std::ffi::c_char) };
        assert_eq!(s.to_str().unwrap(), "true");
    }

    #[test]
    fn test_rt_json_stringify_with_special_chars() {
        let key = CString::new("msg").unwrap();
        let val = CString::new(r#"he said "hi""#).unwrap();
        let result = rt_json_stringify(key.as_ptr() as *const u8, val.as_ptr() as *const u8);
        let s = unsafe { CStr::from_ptr(result as *const std::ffi::c_char) };
        // Quotes should be escaped
        assert!(s.to_str().unwrap().contains("\\\""));
    }

    #[test]
    fn test_rt_mutex_initial_zero() {
        let m = rt_mutex_create(0);
        assert_eq!(rt_mutex_get(m), 0);
    }

    #[test]
    fn test_rt_str_len_empty() {
        let s = CString::new("").unwrap();
        assert_eq!(rt_str_len(s.as_ptr() as *const u8), 0);
    }

    // ================================================================
    // Additional end-to-end JIT tests
    // ================================================================

    #[test]
    fn test_jit_result_type() {
        jit_run_source(
            r#"fn safe_div(a: i64, b: i64) -> i64 ! str {
                if b == 0 {
                    err("division by zero")
                } else {
                    ok(a / b)
                }
            }
            fn main() {
                match safe_div(10, 2) {
                    ok(v) => assert_eq(v, 5)
                    err(e) => assert(false, "should not be error")
                }
                match safe_div(10, 0) {
                    ok(v) => assert(false, "should not be ok")
                    err(e) => assert_eq(e, "division by zero")
                }
            }"#,
        );
    }

    #[test]
    fn test_jit_optional_type() {
        jit_run_source(
            r#"fn main() {
                let x = some(42)
                let val = x ?? 0
                assert_eq(val, 42)

                let y = none
                let val2 = y ?? 99
                assert_eq(val2, 99)
            }"#,
        );
    }

    #[test]
    fn test_jit_early_return() {
        jit_run_source(
            r#"fn find_first_positive(a: i64, b: i64, c: i64) -> i64 {
                if a > 0 { return a }
                if b > 0 { return b }
                if c > 0 { return c }
                0
            }
            fn main() {
                assert_eq(find_first_positive(0, 0, 5), 5)
                assert_eq(find_first_positive(3, 0, 5), 3)
                assert_eq(find_first_positive(0, 7, 5), 7)
                assert_eq(find_first_positive(0, 0, 0), 0)
            }"#,
        );
    }

    #[test]
    fn test_jit_else_if_chain() {
        jit_run_source(
            r#"fn classify(x: i64) -> str {
                if x > 100 {
                    "big"
                } else if x > 10 {
                    "medium"
                } else if x > 0 {
                    "small"
                } else {
                    "zero or negative"
                }
            }
            fn main() {
                assert_eq(classify(200), "big")
                assert_eq(classify(50), "medium")
                assert_eq(classify(5), "small")
                assert_eq(classify(0), "zero or negative")
            }"#,
        );
    }

    #[test]
    fn test_jit_for_in_array() {
        jit_run_source(
            r#"fn main() {
                let arr = [10, 20, 30]
                let mut sum = 0
                for x in arr {
                    sum = sum + x
                }
                assert_eq(sum, 60)
            }"#,
        );
    }

    #[test]
    fn test_jit_struct_field_mutation() {
        jit_run_source(
            r#"struct Counter { val: i64 }
            fn main() {
                let mut c = Counter { val: 0 }
                c.val = 42
                assert_eq(c.val, 42)
            }"#,
        );
    }

    #[test]
    fn test_jit_impl_methods() {
        jit_run_source(
            r#"struct Rect { w: i64, h: i64 }
            impl Rect {
                fn area(self) -> i64 { self.w * self.h }
            }
            fn main() {
                let r = Rect { w: 5, h: 3 }
                assert_eq(r.area(), 15)
            }"#,
        );
    }

    #[test]
    fn test_jit_closure_basic() {
        jit_run_source(
            r#"fn main() {
                let double = |x: i64| -> i64 { x * 2 }
                assert_eq(double(5), 10)
                let add = |a: i64, b: i64| -> i64 { a + b }
                assert_eq(add(3, 7), 10)
            }"#,
        );
    }

    #[test]
    fn test_jit_modulo_operator() {
        jit_run_source(
            r#"fn main() {
                assert_eq(10 % 3, 1)
                assert_eq(15 % 5, 0)
                assert_eq(7 % 2, 1)
            }"#,
        );
    }

    #[test]
    fn test_jit_explicit_str_conversion() {
        jit_run_source(
            r#"fn main() {
                let s = "count: " + to_str(42)
                assert_eq(s, "count: 42")
                let s2 = "flag: " + to_str(true)
                assert_eq(s2, "flag: true")
            }"#,
        );
    }

    #[test]
    fn test_jit_const_values() {
        jit_run_source(
            r#"const MAX = 100
            fn main() {
                assert_eq(MAX, 100)
            }"#,
        );
    }

    #[test]
    fn test_jit_break_continue() {
        jit_run_source(
            r#"fn main() {
                let mut sum = 0
                for i in 0..10 {
                    if i == 5 { break }
                    sum = sum + i
                }
                assert_eq(sum, 10)

                let mut odd_sum = 0
                for i in 0..10 {
                    if i % 2 == 0 { continue }
                    odd_sum = odd_sum + i
                }
                assert_eq(odd_sum, 25)
            }"#,
        );
    }

    #[test]
    fn test_jit_match_with_enum() {
        jit_run_source(
            r#"type Dir { North, South, East, West }
            fn to_num(d: Dir) -> i64 {
                match d {
                    North => 1
                    South => 2
                    East => 3
                    West => 4
                }
            }
            fn main() {
                assert_eq(to_num(Dir.North), 1)
                assert_eq(to_num(Dir.West), 4)
            }"#,
        );
    }

    #[test]
    fn test_jit_pipe_operator() {
        jit_run_source(
            r#"fn double(x: i64) -> i64 { x * 2 }
            fn add_ten(x: i64) -> i64 { x + 10 }
            fn main() {
                let result = 5 |> double |> add_ten
                assert_eq(result, 20)
            }"#,
        );
    }

    #[test]
    fn test_jit_higher_order_map() {
        jit_run_source(
            r#"fn main() {
                let nums = [1, 2, 3]
                let doubled = map(nums, |x: i64| -> i64 { x * 2 })
                assert_eq(doubled[0], 2)
                assert_eq(doubled[1], 4)
                assert_eq(doubled[2], 6)
            }"#,
        );
    }

    #[test]
    fn test_jit_nested_loops() {
        jit_run_source(
            r#"fn main() {
                let mut total = 0
                for i in 0..3 {
                    for j in 0..3 {
                        total = total + 1
                    }
                }
                assert_eq(total, 9)
            }"#,
        );
    }

    #[test]
    fn test_jit_multiple_functions() {
        jit_run_source(
            r#"fn square(x: i64) -> i64 { x * x }
            fn cube(x: i64) -> i64 { x * x * x }
            fn bigger(a: i64, b: i64) -> i64 {
                if a > b { a } else { b }
            }
            fn main() {
                assert_eq(square(5), 25)
                assert_eq(cube(3), 27)
                assert_eq(bigger(10, 20), 20)
                assert_eq(bigger(square(3), cube(2)), 9)
            }"#,
        );
    }

    #[test]
    fn test_jit_compound_assignment() {
        jit_run_source(
            r#"fn main() {
                let mut x = 10
                x += 5
                assert_eq(x, 15)
                x -= 3
                assert_eq(x, 12)
            }"#,
        );
    }

    #[test]
    fn test_jit_string_methods() {
        jit_run_source(
            r#"fn main() {
                assert_eq("hello".upper(), "HELLO")
                assert_eq("  hi  ".trim(), "hi")
                assert_eq("hello world".replace("world", "turbo"), "hello turbo")
                assert_eq("ha".repeat(3), "hahaha")
            }"#,
        );
    }

    // ================================================================
    // Additional type conversion tests
    // ================================================================

    #[test]
    fn test_turbo_ty_fn_type() {
        let empty_enum: HashMap<String, Vec<String>> = HashMap::new();
        let fn_ty = turbo_ty_from_type_expr(
            &TypeExpr::FnType {
                params: vec![
                    Spanned {
                        node: TypeExpr::Named("i64".to_string()),
                        span: 0..0,
                    },
                    Spanned {
                        node: TypeExpr::Named("str".to_string()),
                        span: 0..0,
                    },
                ],
                ret: Box::new(Spanned {
                    node: TypeExpr::Named("bool".to_string()),
                    span: 0..0,
                }),
            },
            &empty_enum,
        );
        assert_eq!(
            fn_ty,
            TurboTy::Fn(vec![TurboTy::Int, TurboTy::Str], Box::new(TurboTy::Bool))
        );
    }

    #[test]
    fn test_turbo_ty_nested_array() {
        let empty_enum: HashMap<String, Vec<String>> = HashMap::new();
        // [[i64]] — array of array of int
        let nested = turbo_ty_from_type_expr(
            &TypeExpr::Array(Box::new(Spanned {
                node: TypeExpr::Array(Box::new(Spanned {
                    node: TypeExpr::Named("i64".to_string()),
                    span: 0..0,
                })),
                span: 0..0,
            })),
            &empty_enum,
        );
        assert_eq!(
            nested,
            TurboTy::Array(Box::new(TurboTy::Array(Box::new(TurboTy::Int))))
        );
    }

    #[test]
    fn test_turbo_ty_all_int_aliases() {
        let empty_enum: HashMap<String, Vec<String>> = HashMap::new();
        for name in &["i32", "i64", "u32", "u64"] {
            let ty = turbo_ty_from_type_expr(&TypeExpr::Named(name.to_string()), &empty_enum);
            assert_eq!(ty, TurboTy::Int, "{} should map to TurboTy::Int", name);
        }
    }

    #[test]
    fn test_turbo_ty_float_aliases() {
        let empty_enum: HashMap<String, Vec<String>> = HashMap::new();
        for name in &["f32", "f64"] {
            let ty = turbo_ty_from_type_expr(&TypeExpr::Named(name.to_string()), &empty_enum);
            assert_eq!(ty, TurboTy::Float, "{} should map to TurboTy::Float", name);
        }
    }

    #[test]
    fn test_codegen_error_is_std_error() {
        let err = CodegenError {
            code: ErrorCode::E0400,
            message: "test".to_string(),
        };
        // Verify it implements std::error::Error
        let _: &dyn std::error::Error = &err;
        assert_eq!(err.to_string(), "codegen error: test");
    }
}
