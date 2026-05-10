use super::*;
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::Module as CraneliftModule;
use std::ffi::{CStr, CString};

// ── Helper: compile & run a Turbo program via JIT ──────────────

fn jit_run_source(source: &str) {
    let (tokens, lex_errors) = turbo_lexer::tokenize(source);
    assert!(lex_errors.is_empty(), "Lex errors: {:?}", lex_errors);
    let (module, parse_errors) = turbo_parser::parse(tokens);
    assert!(parse_errors.is_empty(), "Parse errors: {:?}", parse_errors);
    let sema_result = turbo_sema::check(&module);
    assert!(
        sema_result.errors.is_empty(),
        "Sema errors: {:?}",
        sema_result.errors
    );
    jit_run(&module).expect("JIT compilation/execution failed");
}

// ================================================================
// Depth-limit sanity
// ================================================================

#[test]
fn test_max_codegen_depth_constant_is_256() {
    // Keep the codegen depth limit in lockstep with the parser's
    // MAX_PARSER_DEPTH so the two stages reject the same inputs.
    assert_eq!(MAX_CODEGEN_DEPTH, 256);
    assert_eq!(MAX_CODEGEN_DEPTH, turbo_parser::MAX_PARSER_DEPTH);
}

#[test]
fn test_codegen_rejects_pathologically_deep_ast() {
    // Build an AST directly (bypassing the parser) with >256 levels
    // of nested unary `-` on an int literal, and feed it straight
    // to `compile_expr`. The parser would have rejected this input
    // with E0516; the codegen limit ensures any other AST producer
    // (LSP quick-fixes, macros, tests) gets the same protection
    // instead of segfaulting.
    //
    // Runs in a thread with a large stack because `Drop` glue for
    // the deeply-nested `Box<Spanned<Expr>>` chain would itself
    // overflow the default test stack when the value falls out of
    // scope.
    let handler = std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(|| {
            let span = 0..1;
            let mut inner = Spanned::new(Expr::IntLit(1), span.clone());
            for _ in 0..(MAX_CODEGEN_DEPTH + 16) {
                inner = Spanned::new(
                    Expr::UnaryOp {
                        op: UnaryOp::Neg,
                        expr: Box::new(inner),
                    },
                    span.clone(),
                );
            }

            // Set up a minimal Cranelift function builder so we can
            // call `compile_expr` without spinning up a full JIT.
            use cranelift::prelude::settings::{self, Configurable};
            let mut flag_builder = settings::builder();
            flag_builder.set("use_colocated_libcalls", "false").unwrap();
            flag_builder.set("is_pic", "false").unwrap();
            let isa_builder = cranelift_native::builder().unwrap();
            let isa = isa_builder
                .finish(settings::Flags::new(flag_builder))
                .unwrap();
            let builder = JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());
            let mut module = JITModule::new(builder);
            let ptr_type = module.target_config().pointer_type();

            let mut ctx = module.make_context();
            let mut fn_ctx = FunctionBuilderContext::new();
            let builder = FunctionBuilder::new(&mut ctx.func, &mut fn_ctx);

            let mut data_desc = DataDescription::new();
            let mut string_counter = 0usize;
            let user_fns: HashMap<String, FuncId> = HashMap::new();
            let fn_ret_types: HashMap<String, TurboTy> = HashMap::new();
            let fn_asts: HashMap<String, &FnDef> = HashMap::new();
            let fn_type_params: HashMap<String, Vec<String>> = HashMap::new();
            let rt_fns: HashMap<String, FuncId> = HashMap::new();
            let struct_fields: HashMap<String, Vec<(String, TurboTy)>> = HashMap::new();
            let enum_variants: HashMap<String, Vec<String>> = HashMap::new();
            let enum_variant_fields: HashMap<(String, String), Vec<TurboTy>> = HashMap::new();
            let enum_max_slots: HashMap<String, usize> = HashMap::new();
            let closure_fns: HashMap<usize, (String, TurboTy, Vec<String>)> = HashMap::new();
            let trait_impls: HashMap<String, Vec<String>> = HashMap::new();
            let mut closure_captures: HashMap<usize, CaptureInfo> = HashMap::new();
            let spawn_thunks: HashMap<usize, String> = HashMap::new();
            let constants: HashMap<String, Spanned<Expr>> = HashMap::new();
            let struct_derives: HashMap<String, Vec<String>> = HashMap::new();

            let mut cx = Ctx {
                builder,
                module: &mut module,
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
                closure_fns: &closure_fns,
                trait_impls: &trait_impls,
                inline_depth: 0,
                expr_depth: 0,
                closure_captures: &mut closure_captures,
                generic_struct_field_overrides: HashMap::new(),
                last_struct_lit_concrete_fields: None,
                spawn_thunks: &spawn_thunks,
                constants: &constants,
                struct_derives: &struct_derives,
                loop_stack: Vec::new(),
            };

            let entry = cx.builder.create_block();
            cx.builder.switch_to_block(entry);
            cx.builder.seal_block(entry);

            let err = compile_expr(&mut cx, &inner)
                .expect_err("codegen should reject >256-deep expressions");
            assert_eq!(
                err.code,
                ErrorCode::E0516,
                "codegen depth error should use E0516, got {:?}: {}",
                err.code,
                err.message
            );
            assert!(
                err.message.contains("256"),
                "error should mention the limit value, got: {}",
                err.message
            );
        })
        .expect("failed to spawn test thread");
    handler.join().expect("test thread panicked");
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
    let result = rt_f64_to_str(2.5);
    let s = unsafe { CStr::from_ptr(result as *const std::ffi::c_char) };
    assert_eq!(s.to_str().unwrap(), "2.5");
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
    // Negative exponents and overflow now abort via process::exit(1), so
    // they can't be asserted in-process; see the runtime error messages in
    // rt_pow() itself for the exact text.
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

    let enum_ty = turbo_ty_from_type_expr(&TypeExpr::Named("Color".to_string()), &enum_variants);
    assert_eq!(enum_ty, TurboTy::Enum("Color".to_string()));

    let struct_ty = turbo_ty_from_type_expr(&TypeExpr::Named("Point".to_string()), &enum_variants);
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

    // Verify allocation is registered before final release
    let raw_ptr = unsafe { arr.sub(8) };
    let registered_before =
        ALLOC_REGISTRY.with(|reg| reg.borrow().contains_key(&(raw_ptr as usize)));
    assert!(
        registered_before,
        "allocation should be registered before final release"
    );

    rt_release(arr);
    // After final release, memory is freed — do NOT read rc_ptr (use-after-free).
    // Instead, verify the allocation was removed from the registry.
    let registered_after =
        ALLOC_REGISTRY.with(|reg| reg.borrow().contains_key(&(raw_ptr as usize)));
    assert!(
        !registered_after,
        "allocation should be unregistered after final release"
    );
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
    for name in &["int", "i32", "i64", "u32", "u64", "usize"] {
        let ty = turbo_ty_from_type_expr(&TypeExpr::Named(name.to_string()), &empty_enum);
        assert_eq!(ty, TurboTy::Int, "{} should map to TurboTy::Int", name);
    }
}

#[test]
fn test_turbo_ty_float_aliases() {
    let empty_enum: HashMap<String, Vec<String>> = HashMap::new();
    for name in &["float", "f32", "f64"] {
        let ty = turbo_ty_from_type_expr(&TypeExpr::Named(name.to_string()), &empty_enum);
        assert_eq!(ty, TurboTy::Float, "{} should map to TurboTy::Float", name);
    }
}

#[test]
fn test_turbo_ty_narrow_types() {
    let empty_enum: HashMap<String, Vec<String>> = HashMap::new();
    let i8_ty = turbo_ty_from_type_expr(&TypeExpr::Named("i8".to_string()), &empty_enum);
    assert_eq!(i8_ty, TurboTy::I8, "i8 should map to TurboTy::I8");
    let i16_ty = turbo_ty_from_type_expr(&TypeExpr::Named("i16".to_string()), &empty_enum);
    assert_eq!(i16_ty, TurboTy::I16, "i16 should map to TurboTy::I16");
    let u8_ty = turbo_ty_from_type_expr(&TypeExpr::Named("u8".to_string()), &empty_enum);
    assert_eq!(u8_ty, TurboTy::U8, "u8 should map to TurboTy::U8");
    let u16_ty = turbo_ty_from_type_expr(&TypeExpr::Named("u16".to_string()), &empty_enum);
    assert_eq!(u16_ty, TurboTy::U16, "u16 should map to TurboTy::U16");
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

// ── TASK-10: rt_release deallocation tests ──────────────────────────

#[test]
fn test_rt_release_frees_on_zero_refcount() {
    // Allocate an array, verify it's registered, release it, verify it's freed.
    let arr = rt_array_alloc(3);
    let raw = unsafe { arr.sub(8) };

    // Verify registered
    let before = ALLOC_REGISTRY.with(|reg| reg.borrow().contains_key(&(raw as usize)));
    assert!(before, "new allocation should be in the registry");

    // Release: refcount 1 -> 0, should free
    rt_release(arr);
    let after = ALLOC_REGISTRY.with(|reg| reg.borrow().contains_key(&(raw as usize)));
    assert!(!after, "freed allocation should be removed from registry");
}

#[test]
fn test_rt_release_null_is_safe() {
    // Releasing a null pointer should not panic or crash.
    rt_release(std::ptr::null_mut());
}

#[test]
fn test_rt_release_struct_frees_on_zero() {
    let s = rt_struct_alloc(4);
    let raw = unsafe { s.sub(8) };

    let before = ALLOC_REGISTRY.with(|reg| reg.borrow().contains_key(&(raw as usize)));
    assert!(before);

    rt_release(s);
    let after = ALLOC_REGISTRY.with(|reg| reg.borrow().contains_key(&(raw as usize)));
    assert!(!after);
}

#[test]
fn test_rt_release_result_frees_on_zero() {
    let r = rt_result_ok(42);
    let raw = unsafe { r.sub(8) };

    let before = ALLOC_REGISTRY.with(|reg| reg.borrow().contains_key(&(raw as usize)));
    assert!(before);

    rt_release(r);
    let after = ALLOC_REGISTRY.with(|reg| reg.borrow().contains_key(&(raw as usize)));
    assert!(!after);
}

extern "C" fn typed_http_test_handler(_env: *const u8, _req: *const u8) -> *const u8 {
    static RESPONSE: &[u8] = b"201\x1fapplication/json\x1f{\"ok\":true}\0";
    RESPONSE.as_ptr()
}

#[test]
fn test_typed_http_response_does_not_add_cors_by_default() {
    use std::io::{Read, Write};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let server = std::thread::spawn(move || {
        let routes = vec![(
            "GET".to_string(),
            "/typed".to_string(),
            typed_http_test_handler as RouteHandler,
            std::ptr::null(),
        )];
        let (stream, _) = listener.accept().unwrap();
        handle_http_connection(stream, &routes);
    });

    let mut client = std::net::TcpStream::connect(addr).unwrap();
    client
        .write_all(b"GET /typed HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .unwrap();
    client.shutdown(std::net::Shutdown::Write).unwrap();

    let mut response = String::new();
    client.read_to_string(&mut response).unwrap();
    server.join().unwrap();

    assert!(response.contains("HTTP/1.1 201 Created\r\n"));
    assert!(response.contains("Content-Type: application/json\r\n"));
    assert!(response.contains("Connection: close\r\n"));
    assert!(!response.contains("Access-Control-Allow-Origin"));
}
