//! LLVM backend for the Turbo language compiler.
//!
//! Uses inkwell (LLVM 18) to compile Turbo AST to native code via LLVM IR.
//! Mirrors the Cranelift backend's semantics for all supported AST nodes.

mod builtins;
mod ctx;
mod expr;
mod helpers;
mod stmt;
mod types;

use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::passes::PassBuilderOptions;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine,
};
#[allow(unused_imports)]
use inkwell::types::{BasicMetadataTypeEnum, BasicType, BasicTypeEnum, FunctionType};
use inkwell::values::{BasicMetadataValueEnum, BasicValueEnum, FunctionValue, PointerValue};
use inkwell::{AddressSpace, IntPredicate, OptimizationLevel};

use std::collections::HashMap;
use std::path::Path;
use turbo_ast::*;

use ctx::Ctx;
use expr::{compile_expr, convert_to_str, narrow_from_storage};
use helpers::{extract_all_closures_llvm, extract_all_spawn_sites_llvm};
use types::{
    turbo_ty_from_type_expr, turbo_ty_from_type_expr_with_params, turbo_ty_to_llvm,
    turbo_ty_to_llvm_ctx, resolve_llvm_type_ctx, TurboTy,
};

// ── Runtime C source for AOT linking ────────────────────────────────

const RUNTIME_C: &str = include_str!("../../turbo-codegen-cranelift/runtime/turbo_rt.c");

// ── Error type ──────────────────────────────────────────────────────

#[derive(Debug)]
pub struct CodegenError {
    pub code: ErrorCode,
    pub message: String,
}

impl std::fmt::Display for CodegenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "codegen error: {}", self.message)
    }
}

impl std::error::Error for CodegenError {}

// ── Public entry point ──────────────────────────────────────────────

pub fn aot_compile(ast_module: &turbo_ast::Module, output_path: &Path) -> Result<(), CodegenError> {
    let context = Context::create();
    let module = context.create_module("turbo_module");
    let builder = context.create_builder();

    // Initialize target
    Target::initialize_all(&InitializationConfig::default());

    let target_triple = TargetMachine::get_default_triple();
    let target = Target::from_triple(&target_triple).map_err(|e| CodegenError {
        code: ErrorCode::E0405,
        message: format!("failed to get target: {}", e.to_string_lossy()),
    })?;
    let target_machine = target
        .create_target_machine(
            &target_triple,
            "generic",
            "",
            OptimizationLevel::Aggressive,
            RelocMode::PIC,
            CodeModel::Default,
        )
        .ok_or_else(|| CodegenError {
            code: ErrorCode::E0405,
            message: "failed to create target machine".to_string(),
        })?;

    module.set_triple(&target_triple);
    module.set_data_layout(&target_machine.get_target_data().get_data_layout());

    compile_module(&context, &module, &builder, ast_module)?;

    // Run optimization passes
    module
        .run_passes("default<O2>", &target_machine, PassBuilderOptions::create())
        .map_err(|e| CodegenError {
            code: ErrorCode::E0405,
            message: format!("LLVM optimization failed: {}", e.to_string_lossy()),
        })?;

    // Emit object file
    let tmp_dir = std::env::temp_dir().join(format!("turbo_llvm_aot_{}", std::process::id()));
    std::fs::create_dir_all(&tmp_dir).map_err(|e| CodegenError {
        code: ErrorCode::E0404,
        message: format!("failed to create temp dir: {e}"),
    })?;

    let obj_path = tmp_dir.join("turbo.o");
    let rt_path = tmp_dir.join("turbo_rt.c");

    target_machine
        .write_to_file(&module, FileType::Object, &obj_path)
        .map_err(|e| CodegenError {
            code: ErrorCode::E0404,
            message: format!("failed to emit object: {}", e.to_string_lossy()),
        })?;

    std::fs::write(&rt_path, RUNTIME_C).map_err(|e| CodegenError {
        code: ErrorCode::E0400,
        message: format!("failed to write runtime: {e}"),
    })?;

    // Link with cc
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

    let _ = std::fs::remove_dir_all(&tmp_dir);

    Ok(())
}

// ── Module compilation ──────────────────────────────────────────────

fn compile_module<'ctx>(
    context: &'ctx Context,
    module: &Module<'ctx>,
    builder: &Builder<'ctx>,
    ast_module: &turbo_ast::Module,
) -> Result<(), CodegenError> {
    let ptr_type = context.ptr_type(AddressSpace::default());
    let i64_type = context.i64_type();
    let i8_type = context.i8_type();
    let f64_type = context.f64_type();
    let void_type = context.void_type();

    // ── Declare runtime functions ───────────────────────────────────

    let mut rt_fns: HashMap<String, FunctionValue<'ctx>> = HashMap::new();

    macro_rules! declare_rt {
        ($name:expr, ($($param:expr),*) -> void) => {
            let fn_type = void_type.fn_type(&[$($param.into()),*], false);
            let func = module.add_function($name, fn_type, Some(inkwell::module::Linkage::External));
            rt_fns.insert($name.to_string(), func);
        };
        ($name:expr, ($($param:expr),*) -> $ret:expr) => {
            let fn_type: FunctionType = $ret.fn_type(&[$($param.into()),*], false);
            let func = module.add_function($name, fn_type, Some(inkwell::module::Linkage::External));
            rt_fns.insert($name.to_string(), func);
        };
    }

    // Core runtime
    declare_rt!("rt_print_str", (ptr_type) -> void);
    declare_rt!("rt_print_i64", (i64_type) -> void);
    declare_rt!("rt_print_f64", (f64_type) -> void);
    declare_rt!("rt_print_bool", (i8_type) -> void);
    declare_rt!("rt_panic", (ptr_type) -> void);
    declare_rt!("rt_assert_fail", (ptr_type) -> void);
    declare_rt!("rt_assert_eq_fail", (i64_type, ptr_type, ptr_type) -> void);
    declare_rt!("rt_div_by_zero", () -> void);
    declare_rt!("rt_int_overflow", () -> void);
    declare_rt!("rt_str_concat", (ptr_type, ptr_type) -> ptr_type);
    declare_rt!("rt_str_eq", (ptr_type, ptr_type) -> i8_type);
    declare_rt!("rt_array_alloc", (i64_type) -> ptr_type);
    declare_rt!("rt_array_get", (ptr_type, i64_type) -> i64_type);
    declare_rt!("rt_array_set", (ptr_type, i64_type, i64_type) -> ptr_type);
    declare_rt!("rt_array_len", (ptr_type) -> i64_type);
    declare_rt!("rt_str_len", (ptr_type) -> i64_type);
    declare_rt!("rt_struct_alloc", (i64_type) -> ptr_type);
    declare_rt!("rt_i64_to_str", (i64_type) -> ptr_type);
    declare_rt!("rt_f64_to_str", (f64_type) -> ptr_type);
    declare_rt!("rt_bool_to_str", (i8_type) -> ptr_type);
    declare_rt!("rt_result_ok", (i64_type) -> ptr_type);
    declare_rt!("rt_result_err", (i64_type) -> ptr_type);
    declare_rt!("rt_result_tag", (ptr_type) -> i64_type);
    declare_rt!("rt_result_value", (ptr_type) -> i64_type);
    declare_rt!("rt_option_some", (i64_type) -> ptr_type);
    declare_rt!("rt_option_none", () -> ptr_type);
    declare_rt!("rt_option_tag", (ptr_type) -> i64_type);
    declare_rt!("rt_option_value", (ptr_type) -> i64_type);
    // Stdlib
    declare_rt!("rt_str_split", (ptr_type, ptr_type) -> ptr_type);
    declare_rt!("rt_str_trim", (ptr_type) -> ptr_type);
    declare_rt!("rt_str_upper", (ptr_type) -> ptr_type);
    declare_rt!("rt_str_lower", (ptr_type) -> ptr_type);
    declare_rt!("rt_str_starts_with", (ptr_type, ptr_type) -> i8_type);
    declare_rt!("rt_str_ends_with", (ptr_type, ptr_type) -> i8_type);
    declare_rt!("rt_str_replace", (ptr_type, ptr_type, ptr_type) -> ptr_type);
    declare_rt!("rt_str_char_at", (ptr_type, i64_type) -> ptr_type);
    declare_rt!("rt_str_contains", (ptr_type, ptr_type) -> i8_type);
    declare_rt!("rt_str_index_of", (ptr_type, ptr_type) -> i64_type);
    declare_rt!("rt_str_join", (ptr_type, ptr_type) -> ptr_type);
    declare_rt!("rt_str_repeat", (ptr_type, i64_type) -> ptr_type);
    declare_rt!("rt_read_line", () -> ptr_type);
    declare_rt!("rt_read_file", (ptr_type) -> ptr_type);
    declare_rt!("rt_write_file", (ptr_type, ptr_type) -> void);
    declare_rt!("rt_pow", (i64_type, i64_type) -> i64_type);
    declare_rt!("rt_sqrt", (f64_type) -> f64_type);
    // Async
    declare_rt!("rt_sleep_ms", (i64_type) -> void);
    declare_rt!("rt_spawn_with_args", (ptr_type, ptr_type) -> ptr_type);
    declare_rt!("rt_await_handle", (ptr_type) -> i64_type);
    // HTTP + JSON
    declare_rt!("rt_http_get", (ptr_type) -> ptr_type);
    declare_rt!("rt_http_post", (ptr_type, ptr_type) -> ptr_type);
    declare_rt!("rt_json_get", (ptr_type, ptr_type) -> ptr_type);
    declare_rt!("rt_json_stringify", (ptr_type, ptr_type) -> ptr_type);
    // HTTP server
    declare_rt!("rt_http_server", (i64_type) -> i64_type);
    declare_rt!("rt_http_route", (i64_type, ptr_type, ptr_type, ptr_type, ptr_type) -> void);
    declare_rt!("rt_http_listen", (i64_type) -> void);
    declare_rt!("rt_respond", (i64_type, ptr_type) -> ptr_type);
    declare_rt!("rt_request_body", (ptr_type) -> ptr_type);
    // Channels
    declare_rt!("rt_channel_create", () -> ptr_type);
    declare_rt!("rt_channel_send", (ptr_type, i64_type) -> void);
    declare_rt!("rt_channel_recv", (ptr_type) -> i64_type);
    declare_rt!("rt_channel_clone_sender", (ptr_type) -> ptr_type);
    // Mutex
    declare_rt!("rt_mutex_create", (i64_type) -> ptr_type);
    declare_rt!("rt_mutex_get", (ptr_type) -> i64_type);
    declare_rt!("rt_mutex_set", (ptr_type, i64_type) -> void);
    declare_rt!("rt_mutex_clone", (ptr_type) -> ptr_type);
    // HashMap
    declare_rt!("rt_hashmap_new", () -> ptr_type);
    declare_rt!("rt_hashmap_set", (ptr_type, ptr_type, ptr_type) -> void);
    declare_rt!("rt_hashmap_get", (ptr_type, ptr_type) -> ptr_type);
    declare_rt!("rt_hashmap_has", (ptr_type, ptr_type) -> i8_type);
    declare_rt!("rt_hashmap_len", (ptr_type) -> i64_type);
    declare_rt!("rt_hashmap_keys", (ptr_type) -> ptr_type);
    declare_rt!("rt_hashmap_remove", (ptr_type, ptr_type) -> void);
    // ARC
    declare_rt!("rt_retain", (ptr_type) -> void);
    declare_rt!("rt_release", (ptr_type) -> void);

    // ── Build enum/struct metadata ──────────────────────────────────

    let mut enum_variants: HashMap<String, Vec<String>> = HashMap::new();
    let mut enum_variant_fields: HashMap<(String, String), Vec<TurboTy>> = HashMap::new();
    let mut enum_max_slots: HashMap<String, usize> = HashMap::new();
    for item in &ast_module.items {
        if let Item::Enum(e) = &item.node {
            let variant_names: Vec<String> = e.variants.iter().map(|v| v.name.clone()).collect();
            let tp_names = e.type_param_names();
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

    let mut struct_fields: HashMap<String, Vec<(String, TurboTy)>> = HashMap::new();
    for item in &ast_module.items {
        if let Item::Struct(s) = &item.node {
            let tp_names = s.type_param_names();
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
    }

    // Build constants map
    let mut constants_map: HashMap<String, Spanned<Expr>> = HashMap::new();
    for item in &ast_module.items {
        if let Item::Const(c) = &item.node {
            constants_map.insert(c.name.clone(), c.value.clone());
        }
    }

    // ── Build struct derives map ────────────────────────────────────
    let mut struct_derives: HashMap<String, Vec<String>> = HashMap::new();
    for item in &ast_module.items {
        if let Item::Struct(s) = &item.node {
            if !s.derives.is_empty() {
                struct_derives.insert(s.name.clone(), s.derives.clone());
            }
        }
    }

    // ── Build trait impls map ───────────────────────────────────────
    let mut trait_impls: HashMap<String, Vec<String>> = HashMap::new();
    for item in &ast_module.items {
        if let Item::Impl(imp) = &item.node {
            if let Some(ref trait_name) = imp.trait_name {
                trait_impls
                    .entry(imp.type_name.clone())
                    .or_default()
                    .push(trait_name.clone());
            }
        }
    }
    // @derive(Display) counts as implementing Display
    for (sname, derives) in &struct_derives {
        if derives.contains(&"Display".to_string()) {
            trait_impls
                .entry(sname.clone())
                .or_default()
                .push("Display".to_string());
        }
    }

    // ── Extract closures and spawn sites ───────────────────────────
    let all_closures = extract_all_closures_llvm(ast_module);
    let all_spawn_sites = extract_all_spawn_sites_llvm(ast_module);

    // Build lookup maps
    let mut closure_fns: HashMap<usize, (String, TurboTy, Vec<String>)> = HashMap::new();
    let mut spawn_thunks_map: HashMap<usize, String> = HashMap::new();

    for site in &all_spawn_sites {
        spawn_thunks_map.insert(site.span_start, site.thunk_name.clone());
    }

    // ── Declare all user functions ──────────────────────────────────

    let mut user_fns: HashMap<String, FunctionValue<'ctx>> = HashMap::new();
    let mut fn_ret_types: HashMap<String, TurboTy> = HashMap::new();
    let mut fn_asts: HashMap<String, &FnDef> = HashMap::new();
    let mut fn_type_params: HashMap<String, Vec<String>> = HashMap::new();

    for item in &ast_module.items {
        let Item::Function(f) = &item.node else {
            continue;
        };

        let tp_names = f.type_param_names();
        let param_types: Vec<BasicMetadataTypeEnum<'ctx>> = f
            .params
            .iter()
            .map(|p| {
                resolve_llvm_type_ctx(
                    &p.ty.node,
                    context,
                    &enum_variants,
                    &enum_max_slots,
                    &tp_names,
                )
                .into()
            })
            .collect();

        let ret_turbo = if let Some(ret_ty) = &f.return_type {
            turbo_ty_from_type_expr_with_params(&ret_ty.node, &enum_variants, &tp_names)
        } else {
            TurboTy::Unit
        };

        let fn_type = if ret_turbo == TurboTy::Unit {
            void_type.fn_type(&param_types, false)
        } else {
            let ret_llvm = turbo_ty_to_llvm_ctx(&ret_turbo, context, &enum_max_slots);
            ret_llvm.fn_type(&param_types, false)
        };

        // For AOT, rename main -> turbo_main (the C runtime provides the real main)
        let sym_name = if f.name == "main" {
            "turbo_main"
        } else {
            &f.name
        };

        let func = module.add_function(sym_name, fn_type, None);
        user_fns.insert(f.name.clone(), func);
        fn_ret_types.insert(f.name.clone(), ret_turbo);
        fn_asts.insert(f.name.clone(), f);
        fn_type_params.insert(f.name.clone(), tp_names);
    }

    // Declare methods from impl blocks (including trait impls + default trait methods)
    for item in &ast_module.items {
        let Item::Impl(imp) = &item.node else {
            continue;
        };
        for method_spanned in &imp.methods {
            let method = &method_spanned.node;
            let mangled = format!("{}__{}", imp.type_name, method.name);

            let param_types: Vec<BasicMetadataTypeEnum<'ctx>> = method
                .params
                .iter()
                .map(|p| {
                    if p.name == "self" {
                        ptr_type.into()
                    } else {
                        resolve_llvm_type_ctx(
                            &p.ty.node,
                            context,
                            &enum_variants,
                            &enum_max_slots,
                            &[],
                        )
                        .into()
                    }
                })
                .collect();

            let ret_turbo = if let Some(ret_ty) = &method.return_type {
                turbo_ty_from_type_expr(&ret_ty.node, &enum_variants)
            } else {
                TurboTy::Unit
            };

            let fn_type = if ret_turbo == TurboTy::Unit {
                void_type.fn_type(&param_types, false)
            } else {
                let ret_llvm = turbo_ty_to_llvm_ctx(&ret_turbo, context, &enum_max_slots);
                ret_llvm.fn_type(&param_types, false)
            };

            let func = module.add_function(&mangled, fn_type, None);
            user_fns.insert(mangled.clone(), func);
            fn_ret_types.insert(mangled, ret_turbo);
        }
    }

    // Declare default trait methods (methods defined in trait body that aren't overridden by impl)
    let mut trait_defs: HashMap<String, &turbo_ast::TraitDef> = HashMap::new();
    for item in &ast_module.items {
        if let Item::Trait(t) = &item.node {
            trait_defs.insert(t.name.clone(), t);
        }
    }
    for item in &ast_module.items {
        if let Item::Impl(imp) = &item.node {
            let trait_name = match &imp.trait_name {
                Some(t) => t.clone(),
                None => continue,
            };
            let trait_def = match trait_defs.get(&trait_name) {
                Some(t) => *t,
                None => continue,
            };
            let implemented: std::collections::HashSet<String> =
                imp.methods.iter().map(|m| m.node.name.clone()).collect();
            for trait_method in &trait_def.methods {
                if implemented.contains(&trait_method.name) {
                    continue;
                }
                let body_expr = match &trait_method.default_body {
                    Some(b) => b,
                    None => continue,
                };
                let mangled = format!("{}__{}", imp.type_name, trait_method.name);
                let param_types: Vec<BasicMetadataTypeEnum<'ctx>> = trait_method
                    .params
                    .iter()
                    .map(|p| {
                        if p.name == "self" {
                            ptr_type.into()
                        } else {
                            resolve_llvm_type_ctx(
                                &p.ty.node,
                                context,
                                &enum_variants,
                                &enum_max_slots,
                                &[],
                            )
                            .into()
                        }
                    })
                    .collect();
                let ret_turbo = if let Some(ref rt) = trait_method.return_type {
                    turbo_ty_from_type_expr(&rt.node, &enum_variants)
                } else {
                    TurboTy::Unit
                };
                let fn_llvm = if ret_turbo == TurboTy::Unit {
                    void_type.fn_type(&param_types, false)
                } else {
                    let rl = turbo_ty_to_llvm_ctx(&ret_turbo, context, &enum_max_slots);
                    rl.fn_type(&param_types, false)
                };
                let func = module.add_function(&mangled, fn_llvm, None);
                user_fns.insert(mangled.clone(), func);
                fn_ret_types.insert(mangled.clone(), ret_turbo);
                // Store the body for compilation later; we use fn_asts to point to a trait method
                // We'll compile the body inline below using fn_asts for the type_name context
                let _ = body_expr; // will compile body in "define bodies" loop
            }
        }
    }

    // Declare derived methods (Display to_string, Eq ==, Clone clone)
    for (struct_name, derives) in &struct_derives {
        for derive_name in derives {
            match derive_name.as_str() {
                "Display" => {
                    let mangled = format!("{struct_name}__to_string");
                    if !user_fns.contains_key(&mangled) {
                        let fn_type = ptr_type.fn_type(&[ptr_type.into()], false);
                        let func = module.add_function(&mangled, fn_type, None);
                        user_fns.insert(mangled.clone(), func);
                        fn_ret_types.insert(mangled, TurboTy::Str);
                    }
                }
                "Eq" => {
                    let mangled = format!("{struct_name}__eq");
                    if !user_fns.contains_key(&mangled) {
                        let fn_type = i8_type.fn_type(&[ptr_type.into(), ptr_type.into()], false);
                        let func = module.add_function(&mangled, fn_type, None);
                        user_fns.insert(mangled.clone(), func);
                        fn_ret_types.insert(mangled, TurboTy::Bool);
                    }
                }
                "Clone" => {
                    let mangled = format!("{struct_name}__clone");
                    if !user_fns.contains_key(&mangled) {
                        let fn_type = ptr_type.fn_type(&[ptr_type.into()], false);
                        let func = module.add_function(&mangled, fn_type, None);
                        user_fns.insert(mangled.clone(), func);
                        fn_ret_types.insert(mangled, TurboTy::Struct(struct_name.clone()));
                    }
                }
                _ => {}
            }
        }
    }

    // Declare pre-extracted closures
    for cl in &all_closures {
        let mut param_tys: Vec<BasicMetadataTypeEnum<'ctx>> = vec![ptr_type.into()]; // env_ptr first
        let mut turbo_param_tys: Vec<TurboTy> = Vec::new();
        for p in cl.params {
            let tty = turbo_ty_from_type_expr(&p.ty.node, &enum_variants);
            param_tys.push(turbo_ty_to_llvm(&tty, context).into());
            turbo_param_tys.push(tty);
        }
        let ret_turbo = if let Some(ref rt) = cl.return_type {
            turbo_ty_from_type_expr(&rt.node, &enum_variants)
        } else {
            TurboTy::Int
        };
        let fn_llvm = if ret_turbo == TurboTy::Unit {
            void_type.fn_type(&param_tys, false)
        } else {
            let rl = turbo_ty_to_llvm(&ret_turbo, context);
            rl.fn_type(&param_tys, false)
        };
        let func = module.add_function(&cl.name, fn_llvm, None);
        user_fns.insert(cl.name.clone(), func);
        let closure_turbo_ty = TurboTy::Fn(turbo_param_tys, Box::new(ret_turbo));
        fn_ret_types.insert(
            cl.name.clone(),
            match &closure_turbo_ty {
                TurboTy::Fn(_, r) => *r.clone(),
                _ => TurboTy::Int,
            },
        );
        closure_fns.insert(
            cl.span_start,
            (cl.name.clone(), closure_turbo_ty, cl.free_vars.clone()),
        );
    }

    // Declare spawn thunks
    for site in &all_spawn_sites {
        // Thunk signature: fn(__spawn_thunk_N)(args_ptr: *) -> i64
        let fn_type = i64_type.fn_type(&[ptr_type.into()], false);
        let func = module.add_function(&site.thunk_name, fn_type, None);
        user_fns.insert(site.thunk_name.clone(), func);
        fn_ret_types.insert(site.thunk_name.clone(), TurboTy::Unit);
    }

    // ── Define all function bodies ──────────────────────────────────

    let mut string_counter: usize = 0;

    for item in &ast_module.items {
        let Item::Function(f) = &item.node else {
            continue;
        };
        let func = user_fns[&f.name];
        let tp_names = f.type_param_names();

        let entry = context.append_basic_block(func, "entry");
        builder.position_at_end(entry);

        let mut vars: HashMap<String, (PointerValue<'ctx>, TurboTy)> = HashMap::new();

        // Create allocas for parameters
        for (i, param) in f.params.iter().enumerate() {
            let llvm_ty = resolve_llvm_type_ctx(
                &param.ty.node,
                context,
                &enum_variants,
                &enum_max_slots,
                &tp_names,
            );
            let turbo_ty =
                turbo_ty_from_type_expr_with_params(&param.ty.node, &enum_variants, &tp_names);
            let alloca = builder
                .build_alloca(llvm_ty, &param.name)
                .expect("build_alloca failed");
            let param_val = func.get_nth_param(i as u32).unwrap();
            builder
                .build_store(alloca, param_val)
                .expect("build_store failed");
            vars.insert(param.name.clone(), (alloca, turbo_ty));
        }

        macro_rules! make_ctx {
            ($vars:expr, $current_fn:expr) => {
                Ctx {
                    context,
                    module,
                    builder,
                    current_fn: $current_fn,
                    user_fns: &user_fns,
                    fn_ret_types: &fn_ret_types,
                    fn_asts: &fn_asts,
                    fn_type_params: &fn_type_params,
                    rt_fns: &rt_fns,
                    vars: $vars,
                    string_counter: &mut string_counter,
                    struct_fields: &struct_fields,
                    enum_variants: &enum_variants,
                    enum_variant_fields: &enum_variant_fields,
                    enum_max_slots: &enum_max_slots,
                    constants: &constants_map,
                    loop_stack: Vec::new(),
                    closure_fns: &closure_fns,
                    spawn_thunks: &spawn_thunks_map,
                    struct_derives: &struct_derives,
                    trait_impls: &trait_impls,
                    concrete_struct_fields: std::collections::HashMap::new(),
                }
            };
        }

        let mut cx = make_ctx!(vars, func);

        let result = compile_expr(&mut cx, &f.body)?;

        // Only emit a terminator if the current block doesn't have one
        let current_block = builder.get_insert_block().unwrap();
        if current_block.get_terminator().is_none() {
            if f.return_type.is_some() {
                if let Some((val, _)) = result {
                    builder
                        .build_return(Some(&val))
                        .expect("build_return failed");
                } else {
                    builder.build_return(None).expect("build_return failed");
                }
            } else {
                builder.build_return(None).expect("build_return failed");
            }
        }
    }

    macro_rules! make_ctx_global {
        ($vars:expr, $current_fn:expr) => {
            Ctx {
                context,
                module,
                builder,
                current_fn: $current_fn,
                user_fns: &user_fns,
                fn_ret_types: &fn_ret_types,
                fn_asts: &fn_asts,
                fn_type_params: &fn_type_params,
                rt_fns: &rt_fns,
                vars: $vars,
                string_counter: &mut string_counter,
                struct_fields: &struct_fields,
                enum_variants: &enum_variants,
                enum_variant_fields: &enum_variant_fields,
                enum_max_slots: &enum_max_slots,
                constants: &constants_map,
                loop_stack: Vec::new(),
                closure_fns: &closure_fns,
                spawn_thunks: &spawn_thunks_map,
                struct_derives: &struct_derives,
                trait_impls: &trait_impls,
                concrete_struct_fields: HashMap::new(),
            }
        };
    }

    // Define method bodies from impl blocks
    for item in &ast_module.items {
        let Item::Impl(imp) = &item.node else {
            continue;
        };
        for method_spanned in &imp.methods {
            let method = &method_spanned.node;
            let mangled = format!("{}__{}", imp.type_name, method.name);
            let func = user_fns[&mangled];

            let entry = context.append_basic_block(func, "entry");
            builder.position_at_end(entry);

            let mut vars: HashMap<String, (PointerValue<'ctx>, TurboTy)> = HashMap::new();

            for (i, param) in method.params.iter().enumerate() {
                let (llvm_ty, turbo_ty) = if param.name == "self" {
                    (
                        ptr_type.as_basic_type_enum(),
                        TurboTy::Struct(imp.type_name.clone()),
                    )
                } else {
                    let llvm_ty = resolve_llvm_type_ctx(
                        &param.ty.node,
                        context,
                        &enum_variants,
                        &enum_max_slots,
                        &[],
                    );
                    let turbo_ty = turbo_ty_from_type_expr(&param.ty.node, &enum_variants);
                    (llvm_ty, turbo_ty)
                };
                let alloca = builder
                    .build_alloca(llvm_ty, &param.name)
                    .expect("build_alloca failed");
                let param_val = func.get_nth_param(i as u32).unwrap();
                builder
                    .build_store(alloca, param_val)
                    .expect("build_store failed");
                vars.insert(param.name.clone(), (alloca, turbo_ty));
            }

            let mut cx = make_ctx_global!(vars, func);

            let result = compile_expr(&mut cx, &method.body)?;

            let current_block = builder.get_insert_block().unwrap();
            if current_block.get_terminator().is_none() {
                if method.return_type.is_some() {
                    if let Some((val, _)) = result {
                        builder
                            .build_return(Some(&val))
                            .expect("build_return failed");
                    } else {
                        builder.build_return(None).expect("build_return failed");
                    }
                } else {
                    builder.build_return(None).expect("build_return failed");
                }
            }
        }
    }

    // Define default trait method bodies
    for item in &ast_module.items {
        if let Item::Impl(imp) = &item.node {
            let trait_name = match &imp.trait_name {
                Some(t) => t.clone(),
                None => continue,
            };
            let trait_def = match trait_defs.get(&trait_name) {
                Some(t) => *t,
                None => continue,
            };
            let implemented: std::collections::HashSet<String> =
                imp.methods.iter().map(|m| m.node.name.clone()).collect();
            for trait_method in &trait_def.methods {
                if implemented.contains(&trait_method.name) {
                    continue;
                }
                let body_expr = match &trait_method.default_body {
                    Some(b) => b,
                    None => continue,
                };
                let mangled = format!("{}__{}", imp.type_name, trait_method.name);
                let func = match user_fns.get(&mangled) {
                    Some(f) => *f,
                    None => continue,
                };

                let entry = context.append_basic_block(func, "entry");
                builder.position_at_end(entry);

                let mut vars: HashMap<String, (PointerValue<'ctx>, TurboTy)> = HashMap::new();
                for (i, param) in trait_method.params.iter().enumerate() {
                    let (llvm_ty, turbo_ty) = if param.name == "self" {
                        (
                            ptr_type.as_basic_type_enum(),
                            TurboTy::Struct(imp.type_name.clone()),
                        )
                    } else {
                        let lt = resolve_llvm_type_ctx(
                            &param.ty.node,
                            context,
                            &enum_variants,
                            &enum_max_slots,
                            &[],
                        );
                        let tt = turbo_ty_from_type_expr(&param.ty.node, &enum_variants);
                        (lt, tt)
                    };
                    let alloca = builder
                        .build_alloca(llvm_ty, &param.name)
                        .expect("alloca failed");
                    builder
                        .build_store(alloca, func.get_nth_param(i as u32).unwrap())
                        .expect("store failed");
                    vars.insert(param.name.clone(), (alloca, turbo_ty));
                }

                let mut cx = make_ctx_global!(vars, func);
                let result = compile_expr(&mut cx, body_expr)?;
                let cur = builder.get_insert_block().unwrap();
                if cur.get_terminator().is_none() {
                    if trait_method.return_type.is_some() {
                        if let Some((val, _)) = result {
                            builder.build_return(Some(&val)).expect("return failed");
                        } else {
                            builder.build_return(None).expect("return failed");
                        }
                    } else {
                        builder.build_return(None).expect("return failed");
                    }
                }
            }
        }
    }

    // Define derived method bodies
    for (struct_name, derives) in &struct_derives {
        let fields = struct_fields.get(struct_name).cloned().unwrap_or_default();

        for derive_name in derives {
            match derive_name.as_str() {
                "Display" => {
                    let mangled = format!("{struct_name}__to_string");
                    let func = match user_fns.get(&mangled) {
                        Some(f) => *f,
                        None => continue,
                    };
                    // Already declared. Define: concatenate all fields as "StructName { f1: v1, f2: v2 }"
                    let entry = context.append_basic_block(func, "entry");
                    builder.position_at_end(entry);
                    let vars: HashMap<String, (PointerValue<'ctx>, TurboTy)> = {
                        let mut m = HashMap::new();
                        let self_alloca = builder.build_alloca(ptr_type, "self").expect("alloca");
                        builder
                            .build_store(self_alloca, func.get_nth_param(0).unwrap())
                            .expect("store");
                        m.insert(
                            "self".to_string(),
                            (self_alloca, TurboTy::Struct(struct_name.clone())),
                        );
                        m
                    };
                    let mut cx = make_ctx_global!(vars, func);

                    // Build display string: "StructName { field1: val1, field2: val2 }"
                    let mut parts: Vec<BasicValueEnum<'ctx>> = Vec::new();
                    let prefix = cx
                        .create_string(&format!("{struct_name} {{ "))
                        .expect("str");
                    parts.push(prefix.into());

                    let self_ptr = cx
                        .builder
                        .build_load(ptr_type, cx.vars["self"].0, "self_ptr")
                        .expect("load self")
                        .into_pointer_value();

                    for (fi, (fname, ftty)) in fields.iter().enumerate() {
                        if fi > 0 {
                            let sep = cx.create_string(", ").expect("str");
                            parts.push(sep.into());
                        }
                        let flabel = cx.create_string(&format!("{fname}: ")).expect("str");
                        parts.push(flabel.into());

                        let offset = fi as u64 * 8;
                        let field_ptr = unsafe {
                            cx.builder
                                .build_gep(
                                    i8_type,
                                    self_ptr,
                                    &[i64_type.const_int(offset, false)],
                                    "fp",
                                )
                                .expect("gep")
                        };
                        let raw = cx
                            .builder
                            .build_load(i64_type, field_ptr, "fv")
                            .expect("load");
                        let fval = narrow_from_storage(&cx, raw.into(), ftty);
                        let fstr =
                            convert_to_str(&mut cx, fval, ftty).unwrap_or_else(|_| raw.into());
                        parts.push(fstr);
                    }

                    let suffix = cx.create_string(" }").expect("str");
                    parts.push(suffix.into());

                    // Concatenate all parts
                    let mut result_str: BasicValueEnum<'ctx> =
                        cx.create_string("").expect("str").into();
                    for part in parts {
                        result_str = cx
                            .rt_call("rt_str_concat", &[result_str.into(), part.into()])
                            .unwrap();
                    }
                    cx.builder.build_return(Some(&result_str)).expect("return");
                }
                "Eq" => {
                    let mangled = format!("{struct_name}__eq");
                    let func = match user_fns.get(&mangled) {
                        Some(f) => *f,
                        None => continue,
                    };
                    let entry = context.append_basic_block(func, "entry");
                    builder.position_at_end(entry);
                    // eq(self, other) -> bool: compare each field
                    let self_ptr = func.get_nth_param(0).unwrap().into_pointer_value();
                    let other_ptr = func.get_nth_param(1).unwrap().into_pointer_value();

                    // Compare all fields; return false at first mismatch
                    let merge_block = context.append_basic_block(func, "eq_merge");
                    let mut phi_sources: Vec<(
                        BasicValueEnum<'ctx>,
                        inkwell::basic_block::BasicBlock<'ctx>,
                    )> = Vec::new();

                    let mut cur_block = entry;
                    for (fi, (_, ftty)) in fields.iter().enumerate() {
                        let offset = fi as u64 * 8;
                        let fp1 = unsafe {
                            builder
                                .build_gep(
                                    i8_type,
                                    self_ptr,
                                    &[i64_type.const_int(offset, false)],
                                    "fp1",
                                )
                                .expect("gep")
                        };
                        let fp2 = unsafe {
                            builder
                                .build_gep(
                                    i8_type,
                                    other_ptr,
                                    &[i64_type.const_int(offset, false)],
                                    "fp2",
                                )
                                .expect("gep")
                        };
                        let v1 = builder
                            .build_load(i64_type, fp1, "v1")
                            .expect("load")
                            .into_int_value();
                        let v2 = builder
                            .build_load(i64_type, fp2, "v2")
                            .expect("load")
                            .into_int_value();

                        let cmp = if *ftty == TurboTy::Str {
                            let pv1 = builder.build_int_to_ptr(v1, ptr_type, "p1").expect("itp");
                            let pv2 = builder.build_int_to_ptr(v2, ptr_type, "p2").expect("itp");
                            let eq = builder
                                .build_direct_call(
                                    rt_fns["rt_str_eq"],
                                    &[pv1.into(), pv2.into()],
                                    "seq",
                                )
                                .expect("call");
                            let eq_i = eq.try_as_basic_value().left().unwrap().into_int_value();
                            let zero = i8_type.const_int(0, false);
                            builder
                                .build_int_compare(IntPredicate::NE, eq_i, zero, "cmp")
                                .expect("cmp")
                        } else {
                            builder
                                .build_int_compare(IntPredicate::EQ, v1, v2, "cmp")
                                .expect("cmp")
                        };

                        let next_block = context.append_basic_block(func, "eq_next");
                        let false_val = i8_type.const_int(0, false);
                        phi_sources.push((false_val.into(), cur_block));
                        builder
                            .build_conditional_branch(cmp, next_block, merge_block)
                            .expect("br");
                        builder.position_at_end(next_block);
                        cur_block = next_block;
                    }
                    let true_val = i8_type.const_int(1, false);
                    phi_sources.push((true_val.into(), cur_block));
                    builder.build_unconditional_branch(merge_block).expect("br");
                    builder.position_at_end(merge_block);
                    let phi = builder.build_phi(i8_type, "eq_result").expect("phi");
                    for (val, block) in &phi_sources {
                        phi.add_incoming(&[(val, *block)]);
                    }
                    builder
                        .build_return(Some(&phi.as_basic_value()))
                        .expect("return");
                }
                "Clone" => {
                    let mangled = format!("{struct_name}__clone");
                    let func = match user_fns.get(&mangled) {
                        Some(f) => *f,
                        None => continue,
                    };
                    let entry = context.append_basic_block(func, "entry");
                    builder.position_at_end(entry);
                    // clone(self) -> Self: alloc new struct, copy all fields
                    let self_ptr = func.get_nth_param(0).unwrap().into_pointer_value();
                    let nf = fields.len() as u64;
                    let new_ptr = builder
                        .build_direct_call(
                            rt_fns["rt_struct_alloc"],
                            &[i64_type.const_int(nf, false).into()],
                            "clone_ptr",
                        )
                        .expect("call")
                        .try_as_basic_value()
                        .left()
                        .unwrap()
                        .into_pointer_value();
                    for fi in 0..fields.len() {
                        let offset = fi as u64 * 8;
                        let sp = unsafe {
                            builder
                                .build_gep(
                                    i8_type,
                                    self_ptr,
                                    &[i64_type.const_int(offset, false)],
                                    "sp",
                                )
                                .expect("gep")
                        };
                        let dp = unsafe {
                            builder
                                .build_gep(
                                    i8_type,
                                    new_ptr,
                                    &[i64_type.const_int(offset, false)],
                                    "dp",
                                )
                                .expect("gep")
                        };
                        let val = builder.build_load(i64_type, sp, "val").expect("load");
                        builder.build_store(dp, val).expect("store");
                    }
                    builder
                        .build_return(Some(&BasicValueEnum::PointerValue(new_ptr)))
                        .expect("return");
                }
                _ => {}
            }
        }
    }

    // Define closure bodies
    for cl in &all_closures {
        let func = match user_fns.get(&cl.name) {
            Some(f) => *f,
            None => continue,
        };
        let entry = context.append_basic_block(func, "entry");
        builder.position_at_end(entry);

        let mut vars: HashMap<String, (PointerValue<'ctx>, TurboTy)> = HashMap::new();

        // First param is env_ptr
        let env_ptr_val = func.get_nth_param(0).unwrap().into_pointer_value();

        // Closure params start at index 1
        for (i, param) in cl.params.iter().enumerate() {
            let tty = turbo_ty_from_type_expr(&param.ty.node, &enum_variants);
            let llvm_ty = turbo_ty_to_llvm(&tty, context);
            let alloca = builder.build_alloca(llvm_ty, &param.name).expect("alloca");
            let param_val = func.get_nth_param((i + 1) as u32).unwrap();
            builder.build_store(alloca, param_val).expect("store");
            vars.insert(param.name.clone(), (alloca, tty));
        }

        // Load captured variables from env_ptr
        // We need to find what was captured -- we'll populate during compile by scanning free_vars
        // For now, load each captured var from env struct at compile time
        // The free_vars list tells us which outer vars are captured
        for (cap_idx, cap_name) in cl.free_vars.iter().enumerate() {
            let cap_tty = cl
                .capture_types
                .get(cap_idx)
                .cloned()
                .unwrap_or(TurboTy::Int);
            let offset = cap_idx as u64 * 8;
            let field_ptr = unsafe {
                builder
                    .build_gep(
                        i8_type,
                        env_ptr_val,
                        &[i64_type.const_int(offset, false)],
                        "cap_ptr",
                    )
                    .expect("gep")
            };
            let raw = builder
                .build_load(i64_type, field_ptr, cap_name)
                .expect("load");
            // If the captured variable is a string (ptr), convert i64 -> ptr
            if matches!(cap_tty, TurboTy::Str | TurboTy::Array(_)) {
                let ptr_val = builder
                    .build_int_to_ptr(
                        raw.into_int_value(),
                        ptr_type,
                        &format!("cap_ptr_{cap_name}"),
                    )
                    .expect("itp");
                let alloca = builder
                    .build_alloca(ptr_type, &format!("cap_{cap_name}"))
                    .expect("alloca");
                builder.build_store(alloca, ptr_val).expect("store");
                vars.insert(cap_name.clone(), (alloca, cap_tty));
            } else {
                let alloca = builder
                    .build_alloca(i64_type, &format!("cap_{cap_name}"))
                    .expect("alloca");
                builder.build_store(alloca, raw).expect("store");
                vars.insert(cap_name.clone(), (alloca, cap_tty));
            }
        }

        let mut cx = make_ctx_global!(vars, func);
        let result = compile_expr(&mut cx, cl.body)?;
        let cur = builder.get_insert_block().unwrap();
        if cur.get_terminator().is_none() {
            let ret_turbo = if let Some(ref rt) = cl.return_type {
                turbo_ty_from_type_expr(&rt.node, &enum_variants)
            } else {
                TurboTy::Int
            }; // Default to Int to match function declaration
            if ret_turbo != TurboTy::Unit {
                if let Some((val, _)) = result {
                    builder.build_return(Some(&val)).expect("return");
                } else {
                    // No value from body -- return a zero/null of the expected type
                    let dummy: BasicValueEnum = match &ret_turbo {
                        TurboTy::Int => i64_type.const_int(0, false).into(),
                        TurboTy::Bool => i8_type.const_int(0, false).into(),
                        TurboTy::Float => context.f64_type().const_float(0.0).into(),
                        _ => ptr_type.const_null().into(),
                    };
                    builder.build_return(Some(&dummy)).expect("return");
                }
            } else {
                builder.build_return(None).expect("return");
            }
        }
    }

    // Define spawn thunk bodies
    for site in &all_spawn_sites {
        let func = match user_fns.get(&site.thunk_name) {
            Some(f) => *f,
            None => continue,
        };
        let entry = context.append_basic_block(func, "entry");
        builder.position_at_end(entry);
        // args_ptr points to [fn_ptr, arg0, arg1, ...]
        let args_ptr = func.get_nth_param(0).unwrap().into_pointer_value();

        // Load fn_ptr from offset 0
        let fn_ptr_i64 = builder
            .build_load(i64_type, args_ptr, "fn_ptr_i64")
            .expect("load");
        let fn_ptr = builder
            .build_int_to_ptr(fn_ptr_i64.into_int_value(), ptr_type, "fn_ptr")
            .expect("itp");

        // Load each arg from offsets 8, 16, ...
        let target_fn = user_fns.get(&site.callee_name);
        let mut arg_vals: Vec<BasicMetadataValueEnum<'ctx>> = Vec::new();
        for i in 0..site.num_args {
            let offset = (i + 1) as u64 * 8;
            let ap = unsafe {
                builder
                    .build_gep(
                        i8_type,
                        args_ptr,
                        &[i64_type.const_int(offset, false)],
                        "ap",
                    )
                    .expect("gep")
            };
            let av = builder
                .build_load(i64_type, ap, &format!("arg{i}"))
                .expect("load");

            // If we know the target function's parameter types, coerce appropriately
            let val: BasicValueEnum = if let Some(tf) = target_fn {
                let param_types = tf.get_type().get_param_types();
                if i < param_types.len() {
                    let av_val = av.into_int_value();
                    match param_types[i] {
                        BasicTypeEnum::IntType(it) if it.get_bit_width() < 64 => builder
                            .build_int_truncate(av_val, it, "trunc")
                            .expect("trunc")
                            .into(),
                        BasicTypeEnum::FloatType(ft) => builder
                            .build_bit_cast(av.into_int_value(), ft, "f2i")
                            .expect("bc")
                            .into(),
                        BasicTypeEnum::PointerType(_) => builder
                            .build_int_to_ptr(av_val, ptr_type, "itp")
                            .expect("itp")
                            .into(),
                        _ => av.into(),
                    }
                } else {
                    av.into()
                }
            } else {
                av.into()
            };
            arg_vals.push(val.into());
        }

        // Call the target function via fn_ptr and return the result
        let result = if let Some(tf) = target_fn {
            let call = builder
                .build_direct_call(*tf, &arg_vals, "spawn_result")
                .expect("call");
            call.try_as_basic_value().left()
        } else {
            let fn_type = i64_type.fn_type(&vec![i64_type.into(); site.num_args], false);
            let call = builder
                .build_indirect_call(fn_type, fn_ptr, &arg_vals, "spawn_result")
                .expect("indirect_call");
            call.try_as_basic_value().left()
        };
        if let Some(val) = result {
            // Widen result to i64 for return
            let ret_val: BasicValueEnum = match val {
                BasicValueEnum::IntValue(iv) => {
                    if iv.get_type().get_bit_width() < 64 {
                        builder
                            .build_int_s_extend(iv, i64_type, "widen")
                            .expect("ext")
                            .into()
                    } else {
                        iv.into()
                    }
                }
                BasicValueEnum::FloatValue(fv) => {
                    builder.build_bit_cast(fv, i64_type, "f2i").expect("bc")
                }
                BasicValueEnum::PointerValue(pv) => builder
                    .build_ptr_to_int(pv, i64_type, "p2i")
                    .expect("pti")
                    .into(),
                other => other,
            };
            builder.build_return(Some(&ret_val)).expect("return");
        } else {
            builder
                .build_return(Some(&i64_type.const_int(0, false)))
                .expect("return");
        }
    }

    // Verify the module
    module.verify().map_err(|e| CodegenError {
        code: ErrorCode::E0405,
        message: format!("LLVM module verification failed: {}", e.to_string_lossy()),
    })?;

    Ok(())
}
