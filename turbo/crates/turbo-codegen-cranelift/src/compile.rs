//! Module-level compilation orchestration.
//!
//! Contains `compile_module()` — the core function that walks a validated AST
//! module, declares all runtime functions, user functions, methods, closures,
//! and spawn thunks, then compiles every function body into Cranelift IR.
//!
//! Also contains the `declare_rt_fn` helper for registering C runtime
//! function signatures with the Cranelift module.

use cranelift::prelude::isa::CallConv;
use cranelift::prelude::*;
use cranelift_module::{DataDescription, FuncId, Linkage, Module};
use std::collections::{HashMap, HashSet};
use turbo_ast::*;

use crate::closures::{extract_all_closures, extract_all_spawn_sites, CaptureInfo};
use crate::expr::compile_expr;
use crate::turbo_types::*;
use crate::type_conv::{coerce_value, resolve_cl_type, resolve_cl_type_ffi, turbo_ty_to_cl_type};
use crate::Ctx;

// ── Runtime function declaration helper ─────────────────────────────

pub(crate) fn declare_rt_fn<M: Module>(
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

// ── Module compilation ──────────────────────────────────────────────

pub(crate) fn compile_module<M: Module>(
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
        "rt_str_copy",
        &[ptr_type],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_str_concat_inplace",
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
        "rt_array_oob_exit",
        &[types::I64, types::I64],
        None,
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
        "rt_struct_cow",
        &[ptr_type, types::I64],
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
        "rt_try_read_file",
        &[ptr_type],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_try_write_file",
        &[ptr_type, ptr_type],
        Some(ptr_type),
    )?;
    declare_rt_fn(module, &mut rt_fns, "rt_exec", &[ptr_type], Some(ptr_type))?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_env_get",
        &[ptr_type],
        Some(ptr_type),
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
    // Math builtins
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_floor",
        &[types::F64],
        Some(types::I64),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_ceil",
        &[types::F64],
        Some(types::I64),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_round",
        &[types::F64],
        Some(types::I64),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_sin",
        &[types::F64],
        Some(types::F64),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_cos",
        &[types::F64],
        Some(types::F64),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_tan",
        &[types::F64],
        Some(types::F64),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_log_builtin",
        &[types::F64],
        Some(types::F64),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_log2_builtin",
        &[types::F64],
        Some(types::F64),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_log10",
        &[types::F64],
        Some(types::F64),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_exp",
        &[types::F64],
        Some(types::F64),
    )?;
    declare_rt_fn(module, &mut rt_fns, "rt_random", &[], Some(types::F64))?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_random_range",
        &[types::I64, types::I64],
        Some(types::I64),
    )?;
    // System builtins
    declare_rt_fn(module, &mut rt_fns, "rt_exit", &[types::I64], None)?;
    declare_rt_fn(module, &mut rt_fns, "rt_args", &[], Some(ptr_type))?;
    // String parsing builtins
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_substring",
        &[ptr_type, types::I64, types::I64],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_pad_left",
        &[ptr_type, types::I64, ptr_type],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_pad_right",
        &[ptr_type, types::I64, ptr_type],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_str_to_int",
        &[ptr_type],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_str_to_float",
        &[ptr_type],
        Some(ptr_type),
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
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_json_root",
        &[ptr_type],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_http_post_with_headers",
        &[ptr_type, ptr_type, ptr_type],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_json_build",
        &[ptr_type],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_float_to_int",
        &[types::F64],
        Some(types::I64),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_int_to_float",
        &[types::I64],
        Some(types::F64),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_str_from_char",
        &[types::I64],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_str_to_i64",
        &[ptr_type],
        Some(types::I64),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_str_to_f64",
        &[ptr_type],
        Some(types::F64),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_str_to_bool",
        &[ptr_type],
        Some(types::I8),
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
        "rt_http_server_public",
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
        "rt_respond_typed",
        &[types::I64, ptr_type, ptr_type],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_request_body",
        &[ptr_type],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_request_method",
        &[ptr_type],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_request_path",
        &[ptr_type],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_request_query",
        &[ptr_type, ptr_type],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_request_header",
        &[ptr_type, ptr_type],
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
    // rt_mutex_update(mutex, closure_fn_ptr, closure_env_ptr) -> new_value
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_mutex_update",
        &[ptr_type, ptr_type, ptr_type],
        Some(types::I64),
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
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_hashmap_set_int",
        &[ptr_type, ptr_type, types::I64],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_hashmap_get_int",
        &[ptr_type, ptr_type],
        Some(types::I64),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_hashmap_inc",
        &[ptr_type, ptr_type, types::I64],
        Some(types::I64),
    )?;
    // ARC runtime declarations
    declare_rt_fn(module, &mut rt_fns, "rt_retain", &[ptr_type], None)?;
    declare_rt_fn(module, &mut rt_fns, "rt_release", &[ptr_type], None)?;
    // Filesystem builtins
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_file_exists",
        &[ptr_type],
        Some(types::I64),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_delete_file",
        &[ptr_type],
        Some(types::I64),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_list_dir",
        &[ptr_type],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_mkdir",
        &[ptr_type],
        Some(types::I64),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_path_join",
        &[ptr_type, ptr_type],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_path_dir",
        &[ptr_type],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_path_base",
        &[ptr_type],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_path_ext",
        &[ptr_type],
        Some(ptr_type),
    )?;
    // Collection builtins
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_sort_int",
        &[ptr_type],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_sort_str",
        &[ptr_type],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_reverse",
        &[ptr_type],
        Some(ptr_type),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_array_contains_int",
        &[ptr_type, types::I64],
        Some(types::I64),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_array_contains_str",
        &[ptr_type, ptr_type],
        Some(types::I64),
    )?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_slice",
        &[ptr_type, types::I64, types::I64],
        Some(ptr_type),
    )?;
    // Date/Time builtins
    declare_rt_fn(module, &mut rt_fns, "rt_time_now", &[], Some(types::F64))?;
    declare_rt_fn(module, &mut rt_fns, "rt_time_ms", &[], Some(types::I64))?;
    declare_rt_fn(
        module,
        &mut rt_fns,
        "rt_format_time",
        &[types::F64, ptr_type],
        Some(ptr_type),
    )?;

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
    // Real Cranelift param types per function — needed so spawn thunks can
    // build a call_indirect signature whose register classes match the
    // callee's true Fast-ABI signature (e.g. F64 params land in float regs).
    let mut fn_param_cl_types: HashMap<String, Vec<types::Type>> = HashMap::new();

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
        let mut param_cl_types = Vec::with_capacity(f.params.len());
        for param in &f.params {
            let cl = resolve_cl_type(
                &param.ty.node,
                ptr_type,
                &enum_variants,
                &f.type_param_names(),
            )?;
            sig.params.push(AbiParam::new(cl));
            param_cl_types.push(cl);
        }
        fn_param_cl_types.insert(f.name.clone(), param_cl_types);
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

    // Declare extern (FFI) functions
    let mut extern_fn_names: HashSet<String> = HashSet::new();
    for item in &ast_module.items {
        let Item::Extern(ext) = &item.node else {
            continue;
        };
        for fn_sig_spanned in &ext.functions {
            let f = &fn_sig_spanned.node;
            let mut sig = module.make_signature();
            // Use default calling convention (system C ABI) — do NOT use CallConv::Fast

            // Extern functions follow the platform C ABI, so `f32` must stay a
            // real 32-bit `float` (resolve_cl_type_ffi) rather than the
            // internal uniform-F64 float slot.
            for param in &f.params {
                sig.params.push(AbiParam::new(resolve_cl_type_ffi(
                    &param.ty.node,
                    ptr_type,
                    &enum_variants,
                    &[],
                )?));
            }

            let ret_turbo = if let Some(ret_ty) = &f.return_type {
                let cl = resolve_cl_type_ffi(&ret_ty.node, ptr_type, &enum_variants, &[])?;
                sig.returns.push(AbiParam::new(cl));
                turbo_ty_from_type_expr_with_params(&ret_ty.node, &enum_variants, &[])
            } else {
                TurboTy::Unit
            };

            let id = module
                .declare_function(&f.name, Linkage::Import, &sig)
                .map_err(|e| CodegenError {
                    code: ErrorCode::E0405,
                    message: e.to_string(),
                })?;

            user_fns.insert(f.name.clone(), id);
            fn_ret_types.insert(f.name.clone(), ret_turbo);
            extern_fn_names.insert(f.name.clone());
        }
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
        // Determine whether this closure returns a value.
        // - Explicit return type: use it.
        // - Inferred params (e.g. used in .map/.filter): assume i64 return.
        // - Expression body (Block with only a tail_expr, no stmts): the
        //   expression result is the return value — this covers arrow closures
        //   `(x: i64) => x * 2` and pipe-closure expression bodies `|x: i64| x * 2`.
        let is_expression_body = matches!(
            &closure.body.node,
            Expr::Block { stmts, tail_expr: Some(_) } if stmts.is_empty()
        );
        let ret_turbo = if let Some(ret_ty) = closure.return_type {
            let cl = resolve_cl_type(&ret_ty.node, ptr_type, &enum_variants, &[])?;
            sig.returns.push(AbiParam::new(cl));
            turbo_ty_from_type_expr(&ret_ty.node, &enum_variants)
        } else {
            let has_inferred_params = closure
                .params
                .iter()
                .any(|p| matches!(p.ty.node, TypeExpr::Inferred));
            if has_inferred_params || is_expression_body {
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

    // Declare env-first adapters for first-class function values.
    //
    // A named function is compiled with a plain `(params...) -> ret` signature,
    // but every function value must be callable through the uniform env-first
    // closure ABI (`(env_ptr, params...) -> ret`, `CallConv::Fast`). For each
    // eligible top-level function we declare an adapter `__fnval$<name>` that
    // takes (and ignores) a leading env pointer and forwards to the real
    // function. `Expr::Ident(name)` in value position then produces the pair
    // `[addr(__fnval$<name>), null]`. Generation is eager (one thin adapter per
    // eligible function) — simple and robust; unused adapters are dead code.
    // Eligibility must match turbo-sema's `named_fn_value_ty`: non-`main`,
    // non-generic, non-async, non-`@unsafe`, non-FFI. `@unsafe` functions are
    // excluded so an unsafe call can't escape the unsafe-context check by
    // hiding behind a value (turbo-sema rejects the value form with E0530).
    let fnval_adapter_targets: Vec<&FnDef> = ast_module
        .items
        .iter()
        .filter_map(|item| match &item.node {
            Item::Function(f)
                if f.name != "main"
                    && f.type_param_names().is_empty()
                    && !f.is_async
                    && !f.is_unsafe
                    && !extern_fn_names.contains(&f.name) =>
            {
                Some(f)
            }
            _ => None,
        })
        .collect();

    for f in &fnval_adapter_targets {
        let adapter_name = format!("__fnval${}", f.name);
        let mut sig = module.make_signature();
        sig.call_conv = CallConv::Fast;
        sig.params.push(AbiParam::new(ptr_type)); // hidden env pointer (ignored)
        for param in &f.params {
            sig.params.push(AbiParam::new(resolve_cl_type(
                &param.ty.node,
                ptr_type,
                &enum_variants,
                &[],
            )?));
        }
        if let Some(ret_ty) = &f.return_type {
            sig.returns.push(AbiParam::new(resolve_cl_type(
                &ret_ty.node,
                ptr_type,
                &enum_variants,
                &[],
            )?));
        }
        let id = module
            .declare_function(&adapter_name, Linkage::Local, &sig)
            .map_err(|e| CodegenError {
                code: ErrorCode::E0405,
                message: e.to_string(),
            })?;
        user_fns.insert(adapter_name, id);
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
                extern_fns: &extern_fn_names,
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
                expr_depth: 0,
                closure_captures: &mut closure_captures_map,
                generic_struct_field_overrides: HashMap::new(),
                last_struct_lit_concrete_fields: None,
                spawn_thunks: &spawn_thunk_map,
                constants: &constants_map,
                struct_derives: &struct_derives,
                loop_stack: Vec::new(),
                is_unsafe: f.is_unsafe,
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
                if let Some(ret_ty_expr) = &f.return_type {
                    if let Some((val, val_tty)) = result {
                        // Coerce return value to match the declared return type
                        let ret_tty = turbo_ty_from_type_expr(&ret_ty_expr.node, &enum_variants);
                        let (val, _) = coerce_value(&mut cx, val, &val_tty, &ret_tty);
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
                    extern_fns: &extern_fn_names,
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
                    expr_depth: 0,
                    closure_captures: &mut closure_captures_map,
                    generic_struct_field_overrides: HashMap::new(),
                    last_struct_lit_concrete_fields: None,
                    spawn_thunks: &spawn_thunk_map,
                    constants: &constants_map,
                    struct_derives: &struct_derives,
                    loop_stack: Vec::new(),
                    is_unsafe: method.is_unsafe,
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
                    if let Some(ret_ty_expr) = &method.return_type {
                        if let Some((val, val_tty)) = result {
                            let ret_tty =
                                turbo_ty_from_type_expr(&ret_ty_expr.node, &enum_variants);
                            let (val, _) = coerce_value(&mut cx, val, &val_tty, &ret_tty);
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
                extern_fns: &extern_fn_names,
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
                expr_depth: 0,
                closure_captures: &mut closure_captures_map,
                generic_struct_field_overrides: HashMap::new(),
                last_struct_lit_concrete_fields: None,
                spawn_thunks: &spawn_thunk_map,
                constants: &constants_map,
                struct_derives: &struct_derives,
                loop_stack: Vec::new(),
                is_unsafe: false,
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
                extern_fns: &extern_fn_names,
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
                expr_depth: 0,
                closure_captures: &mut closure_captures_map,
                generic_struct_field_overrides: HashMap::new(),
                last_struct_lit_concrete_fields: None,
                spawn_thunks: &spawn_thunk_map,
                constants: &constants_map,
                struct_derives: &struct_derives,
                loop_stack: Vec::new(),
                is_unsafe: false,
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
        // Mirror the return type logic from the declaration site above.
        let is_expression_body = matches!(
            &closure.body.node,
            Expr::Block { stmts, tail_expr: Some(_) } if stmts.is_empty()
        );
        // Record the closure's return Cranelift type so the body's result can
        // be coerced to it before `return_`. Without this, a closure whose body
        // is a comparison/bool (`|x| x % 2 == 0`) returns an I8 while the
        // signature declares I64 — malformed IR the Cranelift verifier rejects
        // (and which release builds, with the verifier off, would JIT unverified).
        let closure_ret_cl_ty: Option<types::Type> = if let Some(ret_ty) = closure.return_type {
            let t = resolve_cl_type(&ret_ty.node, ptr_type, &enum_variants, &[])?;
            cl_ctx.func.signature.returns.push(AbiParam::new(t));
            Some(t)
        } else {
            // For closures with inferred params or expression bodies, add i64 return
            let has_inferred_params = closure
                .params
                .iter()
                .any(|p| matches!(p.ty.node, TypeExpr::Inferred));
            if has_inferred_params || is_expression_body {
                cl_ctx
                    .func
                    .signature
                    .returns
                    .push(AbiParam::new(types::I64));
                Some(types::I64)
            } else {
                None
            }
        };

        let mut fn_ctx = FunctionBuilderContext::new();
        {
            let builder = FunctionBuilder::new(&mut cl_ctx.func, &mut fn_ctx);
            let mut cx = Ctx {
                builder,
                module,
                user_fns: &user_fns,
                extern_fns: &extern_fn_names,
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
                expr_depth: 0,
                closure_captures: &mut closure_captures_map,
                generic_struct_field_overrides: HashMap::new(),
                last_struct_lit_concrete_fields: None,
                spawn_thunks: &spawn_thunk_map,
                constants: &constants_map,
                struct_derives: &struct_derives,
                loop_stack: Vec::new(),
                is_unsafe: false,
            };

            let entry = cx.builder.create_block();
            cx.builder.append_block_params_for_function_params(entry);
            cx.builder.switch_to_block(entry);
            cx.builder.seal_block(entry);
            // Force the entry block into the layout even if the body emits no
            // instructions before its tail (e.g. `|x| x`), so `is_unreachable()`
            // doesn't misreport it as unreachable (layout.entry_block() == None).
            // The regular-function path does the same.
            cx.builder.ensure_inserted_block();

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
                match (result, closure_ret_cl_ty) {
                    (Some((val, _)), Some(want_ty)) => {
                        // Coerce the body value to the declared return ABI type.
                        let have_ty = cx.builder.func.dfg.value_type(val);
                        let val = if have_ty == want_ty {
                            val
                        } else if have_ty.is_int() && want_ty.is_int() {
                            if have_ty.bits() < want_ty.bits() {
                                cx.builder.ins().uextend(want_ty, val)
                            } else {
                                cx.builder.ins().ireduce(want_ty, val)
                            }
                        } else if (have_ty.is_float() && want_ty == types::I64)
                            || (have_ty == types::I64 && want_ty.is_float())
                        {
                            // Float payload <-> i64 ABI slot (reinterpret bits).
                            cx.builder.ins().bitcast(want_ty, MemFlags::new(), val)
                        } else {
                            val
                        };
                        cx.builder.ins().return_(&[val]);
                    }
                    (_, None) => {
                        cx.builder.ins().return_(&[]);
                    }
                    (None, Some(_)) => {
                        // Signature expects a value but the body produced none;
                        // sema should prevent this — emit a zero as a safety net.
                        let zero = cx.builder.ins().iconst(types::I64, 0);
                        cx.builder.ins().return_(&[zero]);
                    }
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

            // Real Cranelift param types of the spawned callee. Args are stored
            // in the struct as raw i64 slots; for float params we must move the
            // bits back into a float register so the callee's Fast-ABI signature
            // reads them from the correct register class.
            let callee_param_cl: Vec<types::Type> = fn_param_cl_types
                .get(&site.callee_name)
                .cloned()
                .unwrap_or_else(|| vec![types::I64; site.num_args]);

            // Load each argument from the struct (offset 8, 16, 24, ...)
            let mut arg_vals = Vec::new();
            for i in 0..site.num_args {
                let offset = ((i + 1) * 8) as i32;
                let mut val = builder
                    .ins()
                    .load(types::I64, MemFlags::new(), args_ptr, offset);
                // If the callee expects a float here, reinterpret the i64 bits
                // as the float (the packing side bitcast F64 -> I64 on the way in).
                if let Some(want) = callee_param_cl.get(i) {
                    if want.is_float() && want.bits() == 64 {
                        val = builder.ins().bitcast(*want, MemFlags::new(), val);
                    }
                }
                arg_vals.push(val);
            }

            // Build the call signature for the target function (Fast calling convention)
            let target_func_id = user_fns.get(&site.callee_name);
            if let Some(&target_fid) = target_func_id {
                // Use direct call to the target function — but we can't because
                // the fn_ptr is loaded dynamically. Use call_indirect instead.
                let mut callee_sig = module.make_signature();
                callee_sig.call_conv = CallConv::Fast;
                for i in 0..site.num_args {
                    let p = callee_param_cl.get(i).copied().unwrap_or(types::I64);
                    callee_sig.params.push(AbiParam::new(p));
                }
                // Check if the target function has a return type, and whether
                // that return is a float (so we declare the right register class).
                let ret_turbo = fn_ret_types.get(&site.callee_name);
                let has_return = ret_turbo.map(|t| *t != TurboTy::Unit).unwrap_or(false);
                let ret_is_float = matches!(ret_turbo, Some(TurboTy::Float));
                if has_return {
                    let ret_cl = if ret_is_float { types::F64 } else { types::I64 };
                    callee_sig.returns.push(AbiParam::new(ret_cl));
                }
                let sig_ref = builder.import_signature(callee_sig);
                let call = builder.ins().call_indirect(sig_ref, fn_ptr, &arg_vals);
                let results = builder.inst_results(call);
                if !results.is_empty() {
                    let mut result = results[0];
                    // The thunk always returns i64 to rt_spawn_thunk; if the
                    // callee returned a float, move its bits into the i64 slot.
                    if ret_is_float {
                        result = builder.ins().bitcast(types::I64, MemFlags::new(), result);
                    }
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

    // Compile env-first adapter bodies for first-class function values. Each
    // adapter drops its leading env pointer and directly calls the real
    // function with the remaining arguments, returning its result unchanged.
    for f in &fnval_adapter_targets {
        let adapter_name = format!("__fnval${}", f.name);
        let adapter_fid = user_fns[&adapter_name];
        let target_fid = user_fns[&f.name];

        cl_ctx.func.signature = module.make_signature();
        cl_ctx.func.signature.call_conv = CallConv::Fast;
        cl_ctx.func.signature.params.push(AbiParam::new(ptr_type)); // env (ignored)
        for param in &f.params {
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
        let has_return = f.return_type.is_some();
        if let Some(ret_ty) = &f.return_type {
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
            let mut builder = FunctionBuilder::new(&mut cl_ctx.func, &mut fn_ctx);
            let entry = builder.create_block();
            builder.append_block_params_for_function_params(entry);
            builder.switch_to_block(entry);
            builder.seal_block(entry);
            builder.ensure_inserted_block();

            // Forward every user parameter (skip block_params[0], the env ptr).
            let forwarded: Vec<Value> = builder.block_params(entry)[1..].to_vec();
            let target_ref = module.declare_func_in_func(target_fid, builder.func);
            let call = builder.ins().call(target_ref, &forwarded);
            if has_return {
                let results = builder.inst_results(call).to_vec();
                builder.ins().return_(&results);
            } else {
                builder.ins().return_(&[]);
            }
            builder.finalize();
        }

        module
            .define_function(adapter_fid, &mut cl_ctx)
            .map_err(|e| CodegenError {
                code: ErrorCode::E0405,
                message: e.to_string(),
            })?;
        module.clear_context(&mut cl_ctx);
    }

    Ok(user_fns)
}
