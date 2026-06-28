//! JIT compilation entry points.
//!
//! Contains `jit_run()` and `jit_run_function()` which compile a Turbo module
//! via Cranelift JIT and execute it in-process.

use super::*;

// libm symbols for user `extern "C"` declarations (floor, ceil, sqrt, etc.).
// On Linux, Cranelift's process-symbol lookup may not find these because
// Rust binaries don't always pull libm symbols into the dynamic export table.
// Declaring them here forces the linker to resolve them, then `register_libm_symbols`
// hands them to the JIT builder explicitly so user code can call them.
unsafe extern "C" {
    fn floor(x: f64) -> f64;
    fn ceil(x: f64) -> f64;
    fn round(x: f64) -> f64;
    fn trunc(x: f64) -> f64;
    fn sqrt(x: f64) -> f64;
    fn fabs(x: f64) -> f64;
    fn sin(x: f64) -> f64;
    fn cos(x: f64) -> f64;
    fn tan(x: f64) -> f64;
    fn asin(x: f64) -> f64;
    fn acos(x: f64) -> f64;
    fn atan(x: f64) -> f64;
    fn atan2(y: f64, x: f64) -> f64;
    fn log(x: f64) -> f64;
    fn log2(x: f64) -> f64;
    fn log10(x: f64) -> f64;
    fn exp(x: f64) -> f64;
    fn pow(base: f64, exponent: f64) -> f64;
}

fn register_libm_symbols(jit_builder: &mut JITBuilder) {
    jit_builder.symbol("floor", floor as *const u8);
    jit_builder.symbol("ceil", ceil as *const u8);
    jit_builder.symbol("round", round as *const u8);
    jit_builder.symbol("trunc", trunc as *const u8);
    jit_builder.symbol("sqrt", sqrt as *const u8);
    jit_builder.symbol("fabs", fabs as *const u8);
    jit_builder.symbol("sin", sin as *const u8);
    jit_builder.symbol("cos", cos as *const u8);
    jit_builder.symbol("tan", tan as *const u8);
    jit_builder.symbol("asin", asin as *const u8);
    jit_builder.symbol("acos", acos as *const u8);
    jit_builder.symbol("atan", atan as *const u8);
    jit_builder.symbol("atan2", atan2 as *const u8);
    jit_builder.symbol("log", log as *const u8);
    jit_builder.symbol("log2", log2 as *const u8);
    jit_builder.symbol("log10", log10 as *const u8);
    jit_builder.symbol("exp", exp as *const u8);
    jit_builder.symbol("pow", pow as *const u8);
}

// ── Public entry points ─────────────────────────────────────────────

pub fn jit_run(ast_module: &turbo_ast::Module) -> Result<(), CodegenError> {
    let mut flag_builder = settings::builder();
    flag_builder.set("use_colocated_libcalls", "false").unwrap();
    flag_builder.set("is_pic", "false").unwrap();
    flag_builder.set("opt_level", "speed").unwrap();
    let verifier = if cfg!(debug_assertions) {
        "true"
    } else {
        "false"
    };
    flag_builder.set("enable_verifier", verifier).unwrap();
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
    jit_builder.symbol("rt_str_concat_inplace", rt_str_concat_inplace as *const u8);
    jit_builder.symbol("rt_str_eq", rt_str_eq as *const u8);
    jit_builder.symbol("rt_array_alloc", rt_array_alloc as *const u8);
    jit_builder.symbol("rt_array_oob_exit", rt_array_oob_exit as *const u8);
    jit_builder.symbol("rt_array_get", rt_array_get as *const u8);
    jit_builder.symbol("rt_array_set", rt_array_set as *const u8);
    jit_builder.symbol("rt_array_len", rt_array_len as *const u8);
    jit_builder.symbol("rt_array_push", rt_array_push as *const u8);
    jit_builder.symbol("rt_str_len", rt_str_len as *const u8);
    jit_builder.symbol("rt_struct_alloc", rt_struct_alloc as *const u8);
    jit_builder.symbol("rt_struct_cow", rt_struct_cow as *const u8);
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
    jit_builder.symbol("rt_try_read_file", rt_try_read_file as *const u8);
    jit_builder.symbol("rt_try_write_file", rt_try_write_file as *const u8);
    jit_builder.symbol("rt_exec", rt_exec as *const u8);
    jit_builder.symbol("rt_env_get", rt_env_get as *const u8);
    jit_builder.symbol("rt_pow", rt_pow as *const u8);
    jit_builder.symbol("rt_sqrt", rt_sqrt as *const u8);
    // Math builtins
    jit_builder.symbol("rt_floor", rt_floor as *const u8);
    jit_builder.symbol("rt_ceil", rt_ceil as *const u8);
    jit_builder.symbol("rt_round", rt_round as *const u8);
    jit_builder.symbol("rt_sin", rt_sin as *const u8);
    jit_builder.symbol("rt_cos", rt_cos as *const u8);
    jit_builder.symbol("rt_tan", rt_tan as *const u8);
    jit_builder.symbol("rt_log_builtin", rt_log_builtin as *const u8);
    jit_builder.symbol("rt_log2_builtin", rt_log2_builtin as *const u8);
    jit_builder.symbol("rt_log10", rt_log10 as *const u8);
    jit_builder.symbol("rt_exp", rt_exp as *const u8);
    jit_builder.symbol("rt_random", rt_random as *const u8);
    jit_builder.symbol("rt_random_range", rt_random_range as *const u8);
    // System builtins
    jit_builder.symbol("rt_exit", rt_exit as *const u8);
    jit_builder.symbol("rt_args", rt_args as *const u8);
    // String parsing builtins
    jit_builder.symbol("rt_substring", rt_substring as *const u8);
    jit_builder.symbol("rt_pad_left", rt_pad_left as *const u8);
    jit_builder.symbol("rt_pad_right", rt_pad_right as *const u8);
    jit_builder.symbol("rt_str_to_int", rt_str_to_int as *const u8);
    jit_builder.symbol("rt_str_to_float", rt_str_to_float as *const u8);
    // Async runtime symbols
    jit_builder.symbol("rt_sleep_ms", rt_sleep_ms as *const u8);
    jit_builder.symbol("rt_spawn_with_args", rt_spawn_with_args as *const u8);
    jit_builder.symbol("rt_await_handle", rt_await_handle as *const u8);
    // HTTP + JSON builtins
    jit_builder.symbol("rt_http_get", rt_http_get as *const u8);
    jit_builder.symbol("rt_http_post", rt_http_post as *const u8);
    jit_builder.symbol(
        "rt_http_post_with_headers",
        rt_http_post_with_headers as *const u8,
    );
    jit_builder.symbol("rt_json_get", rt_json_get as *const u8);
    jit_builder.symbol("rt_json_stringify", rt_json_stringify as *const u8);
    jit_builder.symbol("rt_json_build", rt_json_build as *const u8);
    jit_builder.symbol("rt_json_root", rt_json_root as *const u8);
    jit_builder.symbol("rt_float_to_int", rt_float_to_int as *const u8);
    jit_builder.symbol("rt_int_to_float", rt_int_to_float as *const u8);
    jit_builder.symbol("rt_str_from_char", rt_str_from_char as *const u8);
    jit_builder.symbol("rt_str_to_i64", rt_str_to_i64 as *const u8);
    jit_builder.symbol("rt_str_to_f64", rt_str_to_f64 as *const u8);
    jit_builder.symbol("rt_str_to_bool", rt_str_to_bool as *const u8);
    // HTTP server builtins
    jit_builder.symbol("rt_http_server", rt_http_server as *const u8);
    jit_builder.symbol("rt_http_server_public", rt_http_server_public as *const u8);
    jit_builder.symbol("rt_http_route", rt_http_route as *const u8);
    jit_builder.symbol("rt_http_listen", rt_http_listen as *const u8);
    jit_builder.symbol("rt_respond", rt_respond as *const u8);
    jit_builder.symbol("rt_respond_typed", rt_respond_typed as *const u8);
    jit_builder.symbol("rt_request_body", rt_request_body as *const u8);
    jit_builder.symbol("rt_request_method", rt_request_method as *const u8);
    jit_builder.symbol("rt_request_path", rt_request_path as *const u8);
    jit_builder.symbol("rt_request_query", rt_request_query as *const u8);
    jit_builder.symbol("rt_request_header", rt_request_header as *const u8);
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
    jit_builder.symbol("rt_mutex_update", rt_mutex_update as *const u8);
    jit_builder.symbol("rt_mutex_clone", rt_mutex_clone as *const u8);
    // HashMap builtins
    jit_builder.symbol("rt_hashmap_new", rt_hashmap_new as *const u8);
    jit_builder.symbol("rt_hashmap_set", rt_hashmap_set as *const u8);
    jit_builder.symbol("rt_hashmap_get", rt_hashmap_get as *const u8);
    jit_builder.symbol("rt_hashmap_has", rt_hashmap_has as *const u8);
    jit_builder.symbol("rt_hashmap_len", rt_hashmap_len as *const u8);
    jit_builder.symbol("rt_hashmap_keys", rt_hashmap_keys as *const u8);
    jit_builder.symbol("rt_hashmap_remove", rt_hashmap_remove as *const u8);
    jit_builder.symbol("rt_hashmap_set_int", rt_hashmap_set_int as *const u8);
    jit_builder.symbol("rt_hashmap_get_int", rt_hashmap_get_int as *const u8);
    jit_builder.symbol("rt_hashmap_inc", rt_hashmap_inc as *const u8);
    // ARC runtime
    jit_builder.symbol("rt_retain", rt_retain as *const u8);
    jit_builder.symbol("rt_release", rt_release as *const u8);
    // Filesystem builtins
    jit_builder.symbol("rt_file_exists", rt_file_exists as *const u8);
    jit_builder.symbol("rt_delete_file", rt_delete_file as *const u8);
    jit_builder.symbol("rt_list_dir", rt_list_dir as *const u8);
    jit_builder.symbol("rt_mkdir", rt_mkdir as *const u8);
    jit_builder.symbol("rt_path_join", rt_path_join as *const u8);
    jit_builder.symbol("rt_path_dir", rt_path_dir as *const u8);
    jit_builder.symbol("rt_path_base", rt_path_base as *const u8);
    jit_builder.symbol("rt_path_ext", rt_path_ext as *const u8);
    // Collection builtins
    jit_builder.symbol("rt_sort_int", rt_sort_int as *const u8);
    jit_builder.symbol("rt_sort_str", rt_sort_str as *const u8);
    jit_builder.symbol("rt_reverse", rt_reverse as *const u8);
    jit_builder.symbol("rt_array_contains_int", rt_array_contains_int as *const u8);
    jit_builder.symbol("rt_array_contains_str", rt_array_contains_str as *const u8);
    jit_builder.symbol("rt_slice", rt_slice as *const u8);
    // Date/Time builtins
    jit_builder.symbol("rt_time_now", rt_time_now as *const u8);
    jit_builder.symbol("rt_time_ms", rt_time_ms as *const u8);
    jit_builder.symbol("rt_format_time", rt_format_time as *const u8);
    // libm for user extern "C" declarations
    register_libm_symbols(&mut jit_builder);

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
    // Free all runtime-allocated strings
    crate::runtime::rt_arena_reset();

    Ok(())
}

/// Compile a module and run a single named function (used for `turbolang test --run-fn`).
/// The function is called via JIT and the process exits with the function's outcome
/// (0 on success, 1 on assertion failure).
pub fn jit_run_function(ast_module: &turbo_ast::Module, fn_name: &str) -> Result<(), CodegenError> {
    let mut flag_builder = settings::builder();
    flag_builder.set("use_colocated_libcalls", "false").unwrap();
    flag_builder.set("is_pic", "false").unwrap();
    flag_builder.set("opt_level", "speed").unwrap();
    let verifier = if cfg!(debug_assertions) {
        "true"
    } else {
        "false"
    };
    flag_builder.set("enable_verifier", verifier).unwrap();
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
    jit_builder.symbol("rt_str_concat_inplace", rt_str_concat_inplace as *const u8);
    jit_builder.symbol("rt_str_eq", rt_str_eq as *const u8);
    jit_builder.symbol("rt_array_alloc", rt_array_alloc as *const u8);
    jit_builder.symbol("rt_array_oob_exit", rt_array_oob_exit as *const u8);
    jit_builder.symbol("rt_array_get", rt_array_get as *const u8);
    jit_builder.symbol("rt_array_set", rt_array_set as *const u8);
    jit_builder.symbol("rt_array_len", rt_array_len as *const u8);
    jit_builder.symbol("rt_array_push", rt_array_push as *const u8);
    jit_builder.symbol("rt_str_len", rt_str_len as *const u8);
    jit_builder.symbol("rt_struct_alloc", rt_struct_alloc as *const u8);
    jit_builder.symbol("rt_struct_cow", rt_struct_cow as *const u8);
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
    jit_builder.symbol("rt_try_read_file", rt_try_read_file as *const u8);
    jit_builder.symbol("rt_try_write_file", rt_try_write_file as *const u8);
    jit_builder.symbol("rt_exec", rt_exec as *const u8);
    jit_builder.symbol("rt_env_get", rt_env_get as *const u8);
    jit_builder.symbol("rt_pow", rt_pow as *const u8);
    jit_builder.symbol("rt_sqrt", rt_sqrt as *const u8);
    // Math builtins
    jit_builder.symbol("rt_floor", rt_floor as *const u8);
    jit_builder.symbol("rt_ceil", rt_ceil as *const u8);
    jit_builder.symbol("rt_round", rt_round as *const u8);
    jit_builder.symbol("rt_sin", rt_sin as *const u8);
    jit_builder.symbol("rt_cos", rt_cos as *const u8);
    jit_builder.symbol("rt_tan", rt_tan as *const u8);
    jit_builder.symbol("rt_log_builtin", rt_log_builtin as *const u8);
    jit_builder.symbol("rt_log2_builtin", rt_log2_builtin as *const u8);
    jit_builder.symbol("rt_log10", rt_log10 as *const u8);
    jit_builder.symbol("rt_exp", rt_exp as *const u8);
    jit_builder.symbol("rt_random", rt_random as *const u8);
    jit_builder.symbol("rt_random_range", rt_random_range as *const u8);
    // System builtins
    jit_builder.symbol("rt_exit", rt_exit as *const u8);
    jit_builder.symbol("rt_args", rt_args as *const u8);
    // String parsing builtins
    jit_builder.symbol("rt_substring", rt_substring as *const u8);
    jit_builder.symbol("rt_pad_left", rt_pad_left as *const u8);
    jit_builder.symbol("rt_pad_right", rt_pad_right as *const u8);
    jit_builder.symbol("rt_str_to_int", rt_str_to_int as *const u8);
    jit_builder.symbol("rt_str_to_float", rt_str_to_float as *const u8);
    jit_builder.symbol("rt_sleep_ms", rt_sleep_ms as *const u8);
    jit_builder.symbol("rt_spawn_with_args", rt_spawn_with_args as *const u8);
    jit_builder.symbol("rt_await_handle", rt_await_handle as *const u8);
    jit_builder.symbol("rt_http_get", rt_http_get as *const u8);
    jit_builder.symbol("rt_http_post", rt_http_post as *const u8);
    jit_builder.symbol(
        "rt_http_post_with_headers",
        rt_http_post_with_headers as *const u8,
    );
    jit_builder.symbol("rt_json_get", rt_json_get as *const u8);
    jit_builder.symbol("rt_json_stringify", rt_json_stringify as *const u8);
    jit_builder.symbol("rt_json_build", rt_json_build as *const u8);
    jit_builder.symbol("rt_json_root", rt_json_root as *const u8);
    jit_builder.symbol("rt_float_to_int", rt_float_to_int as *const u8);
    jit_builder.symbol("rt_int_to_float", rt_int_to_float as *const u8);
    jit_builder.symbol("rt_str_from_char", rt_str_from_char as *const u8);
    jit_builder.symbol("rt_str_to_i64", rt_str_to_i64 as *const u8);
    jit_builder.symbol("rt_str_to_f64", rt_str_to_f64 as *const u8);
    jit_builder.symbol("rt_str_to_bool", rt_str_to_bool as *const u8);
    jit_builder.symbol("rt_http_server", rt_http_server as *const u8);
    jit_builder.symbol("rt_http_server_public", rt_http_server_public as *const u8);
    jit_builder.symbol("rt_http_route", rt_http_route as *const u8);
    jit_builder.symbol("rt_http_listen", rt_http_listen as *const u8);
    jit_builder.symbol("rt_respond", rt_respond as *const u8);
    jit_builder.symbol("rt_respond_typed", rt_respond_typed as *const u8);
    jit_builder.symbol("rt_request_body", rt_request_body as *const u8);
    jit_builder.symbol("rt_request_method", rt_request_method as *const u8);
    jit_builder.symbol("rt_request_path", rt_request_path as *const u8);
    jit_builder.symbol("rt_request_query", rt_request_query as *const u8);
    jit_builder.symbol("rt_request_header", rt_request_header as *const u8);
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
    jit_builder.symbol("rt_mutex_update", rt_mutex_update as *const u8);
    jit_builder.symbol("rt_mutex_clone", rt_mutex_clone as *const u8);
    jit_builder.symbol("rt_hashmap_new", rt_hashmap_new as *const u8);
    jit_builder.symbol("rt_hashmap_set", rt_hashmap_set as *const u8);
    jit_builder.symbol("rt_hashmap_get", rt_hashmap_get as *const u8);
    jit_builder.symbol("rt_hashmap_has", rt_hashmap_has as *const u8);
    jit_builder.symbol("rt_hashmap_len", rt_hashmap_len as *const u8);
    jit_builder.symbol("rt_hashmap_keys", rt_hashmap_keys as *const u8);
    jit_builder.symbol("rt_hashmap_remove", rt_hashmap_remove as *const u8);
    jit_builder.symbol("rt_hashmap_set_int", rt_hashmap_set_int as *const u8);
    jit_builder.symbol("rt_hashmap_get_int", rt_hashmap_get_int as *const u8);
    jit_builder.symbol("rt_hashmap_inc", rt_hashmap_inc as *const u8);
    jit_builder.symbol("rt_retain", rt_retain as *const u8);
    jit_builder.symbol("rt_release", rt_release as *const u8);
    // Filesystem builtins
    jit_builder.symbol("rt_file_exists", rt_file_exists as *const u8);
    jit_builder.symbol("rt_delete_file", rt_delete_file as *const u8);
    jit_builder.symbol("rt_list_dir", rt_list_dir as *const u8);
    jit_builder.symbol("rt_mkdir", rt_mkdir as *const u8);
    jit_builder.symbol("rt_path_join", rt_path_join as *const u8);
    jit_builder.symbol("rt_path_dir", rt_path_dir as *const u8);
    jit_builder.symbol("rt_path_base", rt_path_base as *const u8);
    jit_builder.symbol("rt_path_ext", rt_path_ext as *const u8);
    // Collection builtins
    jit_builder.symbol("rt_sort_int", rt_sort_int as *const u8);
    jit_builder.symbol("rt_sort_str", rt_sort_str as *const u8);
    jit_builder.symbol("rt_reverse", rt_reverse as *const u8);
    jit_builder.symbol("rt_array_contains_int", rt_array_contains_int as *const u8);
    jit_builder.symbol("rt_array_contains_str", rt_array_contains_str as *const u8);
    jit_builder.symbol("rt_slice", rt_slice as *const u8);
    // Date/Time builtins
    jit_builder.symbol("rt_time_now", rt_time_now as *const u8);
    jit_builder.symbol("rt_time_ms", rt_time_ms as *const u8);
    jit_builder.symbol("rt_format_time", rt_format_time as *const u8);
    // libm for user extern "C" declarations
    register_libm_symbols(&mut jit_builder);

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
    // Free all runtime-allocated strings
    crate::runtime::rt_arena_reset();

    Ok(())
}
