//! LLVM backend for the Turbo language compiler.
//!
//! Uses inkwell (LLVM 18) to compile Turbo AST to native code via LLVM IR.
//! Mirrors the Cranelift backend's semantics for all supported AST nodes.

use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::passes::PassBuilderOptions;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine,
};
#[allow(unused_imports)]
use inkwell::types::{BasicMetadataTypeEnum, BasicType, BasicTypeEnum, FunctionType};
use inkwell::values::{
    BasicMetadataValueEnum, BasicValueEnum, FunctionValue, IntValue, PointerValue,
};
use inkwell::{AddressSpace, IntPredicate, FloatPredicate, OptimizationLevel};

use std::collections::HashMap;
use std::path::Path;
use turbo_ast::*;

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

// ── Turbo-level type tag ────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
enum TurboTy {
    Int,
    Float,
    Bool,
    Str,
    Unit,
    Array(Box<TurboTy>),
    Struct(String),
    Enum(String),
    Fn(Vec<TurboTy>, Box<TurboTy>),
    Result(Box<TurboTy>, Box<TurboTy>),
    Optional(Box<TurboTy>),
    Agent(String),
    Future(Box<TurboTy>),
}

type Typed<'ctx> = (BasicValueEnum<'ctx>, TurboTy);
type MaybeTyped<'ctx> = Option<Typed<'ctx>>;

// ── Type conversion helpers ─────────────────────────────────────────

fn turbo_ty_from_type_expr(
    te: &TypeExpr,
    enum_variants: &HashMap<String, Vec<String>>,
) -> TurboTy {
    turbo_ty_from_type_expr_with_params(te, enum_variants, &[])
}

fn turbo_ty_from_type_expr_with_params(
    te: &TypeExpr,
    enum_variants: &HashMap<String, Vec<String>>,
    type_params: &[String],
) -> TurboTy {
    match te {
        TypeExpr::Named(name) => {
            if type_params.contains(name) {
                return TurboTy::Int;
            }
            match name.as_str() {
                "i32" | "i64" | "u32" | "u64" => TurboTy::Int,
                "f32" | "f64" => TurboTy::Float,
                "bool" => TurboTy::Bool,
                "str" => TurboTy::Str,
                _ => {
                    if enum_variants.contains_key(name.as_str()) {
                        TurboTy::Enum(name.clone())
                    } else {
                        TurboTy::Struct(name.clone())
                    }
                }
            }
        }
        TypeExpr::Unit => TurboTy::Unit,
        TypeExpr::Array(inner) => {
            TurboTy::Array(Box::new(turbo_ty_from_type_expr(&inner.node, enum_variants)))
        }
        TypeExpr::FnType { params, ret } => {
            let param_tys: Vec<TurboTy> = params
                .iter()
                .map(|p| turbo_ty_from_type_expr(&p.node, enum_variants))
                .collect();
            let ret_ty = turbo_ty_from_type_expr(&ret.node, enum_variants);
            TurboTy::Fn(param_tys, Box::new(ret_ty))
        }
        TypeExpr::Result { ok_type, err_type } => {
            let ok_tty = turbo_ty_from_type_expr(&ok_type.node, enum_variants);
            let err_tty = turbo_ty_from_type_expr(&err_type.node, enum_variants);
            TurboTy::Result(Box::new(ok_tty), Box::new(err_tty))
        }
        TypeExpr::Optional(inner) => {
            TurboTy::Optional(Box::new(turbo_ty_from_type_expr(&inner.node, enum_variants)))
        }
        TypeExpr::Future(inner) => {
            TurboTy::Future(Box::new(turbo_ty_from_type_expr_with_params(
                &inner.node,
                enum_variants,
                type_params,
            )))
        }
        _ => TurboTy::Int,
    }
}

/// Convert a TurboTy to an LLVM BasicTypeEnum.
fn turbo_ty_to_llvm<'ctx>(
    tty: &TurboTy,
    context: &'ctx Context,
) -> BasicTypeEnum<'ctx> {
    match tty {
        TurboTy::Int => context.i64_type().into(),
        TurboTy::Float => context.f64_type().into(),
        TurboTy::Bool => context.i8_type().into(),
        TurboTy::Str => context.ptr_type(AddressSpace::default()).into(),
        TurboTy::Unit => context.i64_type().into(),
        TurboTy::Fn(_, _) => context.ptr_type(AddressSpace::default()).into(),
        TurboTy::Array(_) => context.ptr_type(AddressSpace::default()).into(),
        TurboTy::Struct(_) => context.ptr_type(AddressSpace::default()).into(),
        // NOTE: Enum types are i64 for unit-only enums. For data-carrying enums,
        // use turbo_ty_to_llvm_with_enums() which checks enum_max_slots.
        TurboTy::Enum(_) => context.i64_type().into(),
        TurboTy::Result(_, _) => context.ptr_type(AddressSpace::default()).into(),
        TurboTy::Optional(_) => context.ptr_type(AddressSpace::default()).into(),
        TurboTy::Agent(_) => context.ptr_type(AddressSpace::default()).into(),
        TurboTy::Future(_) => context.ptr_type(AddressSpace::default()).into(),
    }
}

/// Resolve a TypeExpr to a TurboTy, then to an LLVM type.
#[allow(dead_code)]
fn resolve_llvm_type<'ctx>(
    ty: &TypeExpr,
    context: &'ctx Context,
    enum_variants: &HashMap<String, Vec<String>>,
    type_params: &[String],
) -> BasicTypeEnum<'ctx> {
    let tty = turbo_ty_from_type_expr_with_params(ty, enum_variants, type_params);
    turbo_ty_to_llvm(&tty, context)
}

/// Like turbo_ty_to_llvm but correctly handles data-carrying enums as pointers.
fn turbo_ty_to_llvm_ctx<'ctx>(
    tty: &TurboTy,
    context: &'ctx Context,
    enum_max_slots: &HashMap<String, usize>,
) -> BasicTypeEnum<'ctx> {
    if let TurboTy::Enum(ref name) = tty {
        if enum_max_slots.contains_key(name) {
            return context.ptr_type(AddressSpace::default()).into();
        }
    }
    turbo_ty_to_llvm(tty, context)
}

/// Resolve a TypeExpr to an LLVM type, data-enum-aware.
fn resolve_llvm_type_ctx<'ctx>(
    ty: &TypeExpr,
    context: &'ctx Context,
    enum_variants: &HashMap<String, Vec<String>>,
    enum_max_slots: &HashMap<String, usize>,
    type_params: &[String],
) -> BasicTypeEnum<'ctx> {
    let tty = turbo_ty_from_type_expr_with_params(ty, enum_variants, type_params);
    turbo_ty_to_llvm_ctx(&tty, context, enum_max_slots)
}

// ── Codegen context ─────────────────────────────────────────────────

#[allow(dead_code)]
struct Ctx<'a, 'ctx> {
    context: &'ctx Context,
    module: &'a Module<'ctx>,
    builder: &'a Builder<'ctx>,
    /// Currently compiled function
    current_fn: FunctionValue<'ctx>,
    /// User-defined functions
    user_fns: &'a HashMap<String, FunctionValue<'ctx>>,
    /// Function return types
    fn_ret_types: &'a HashMap<String, TurboTy>,
    /// Function ASTs (for inlining)
    fn_asts: &'a HashMap<String, &'a FnDef>,
    /// Function type params
    fn_type_params: &'a HashMap<String, Vec<String>>,
    /// Runtime functions
    rt_fns: &'a HashMap<String, FunctionValue<'ctx>>,
    /// Variable allocas: name -> (alloca ptr, turbo type)
    vars: HashMap<String, (PointerValue<'ctx>, TurboTy)>,
    /// String literal counter for unique names
    string_counter: &'a mut usize,
    /// Struct field layouts
    struct_fields: &'a HashMap<String, Vec<(String, TurboTy)>>,
    /// Enum variant lists
    enum_variants: &'a HashMap<String, Vec<String>>,
    /// Data-carrying enum variant fields
    enum_variant_fields: &'a HashMap<(String, String), Vec<TurboTy>>,
    /// Max slots per data enum
    enum_max_slots: &'a HashMap<String, usize>,
    /// Module-level constants
    constants: &'a HashMap<String, Spanned<Expr>>,
    /// Loop stack for break/continue: (header_block, exit_block)
    loop_stack: Vec<(inkwell::basic_block::BasicBlock<'ctx>, inkwell::basic_block::BasicBlock<'ctx>)>,
}

impl<'a, 'ctx> Ctx<'a, 'ctx> {
    fn create_string(&mut self, s: &str) -> Result<PointerValue<'ctx>, CodegenError> {
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

    fn rt_call(
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
    fn to_bool(&self, val: BasicValueEnum<'ctx>) -> IntValue<'ctx> {
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
    fn create_entry_block_alloca(
        &self,
        ty: BasicTypeEnum<'ctx>,
        name: &str,
    ) -> PointerValue<'ctx> {
        let entry = self.current_fn.get_first_basic_block().unwrap();
        let builder = self.context.create_builder();
        match entry.get_first_instruction() {
            Some(first_instr) => builder.position_before(&first_instr),
            None => builder.position_at_end(entry),
        }
        builder
            .build_alloca(ty, name)
            .expect("build_alloca failed")
    }
}

// ── Public entry point ──────────────────────────────────────────────

pub fn aot_compile(
    ast_module: &turbo_ast::Module,
    output_path: &Path,
) -> Result<(), CodegenError> {
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
                    .map(|f| turbo_ty_from_type_expr_with_params(&f.node, &enum_variants, &tp_names))
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
                resolve_llvm_type_ctx(&p.ty.node, context, &enum_variants, &enum_max_slots, &tp_names).into()
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

    // Declare methods from impl blocks
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
                        resolve_llvm_type_ctx(&p.ty.node, context, &enum_variants, &enum_max_slots, &[]).into()
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
            let llvm_ty = resolve_llvm_type_ctx(&param.ty.node, context, &enum_variants, &enum_max_slots, &tp_names);
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

        let mut cx = Ctx {
            context,
            module,
            builder,
            current_fn: func,
            user_fns: &user_fns,
            fn_ret_types: &fn_ret_types,
            fn_asts: &fn_asts,
            fn_type_params: &fn_type_params,
            rt_fns: &rt_fns,
            vars,
            string_counter: &mut string_counter,
            struct_fields: &struct_fields,
            enum_variants: &enum_variants,
            enum_variant_fields: &enum_variant_fields,
            enum_max_slots: &enum_max_slots,
            constants: &constants_map,
            loop_stack: Vec::new(),
        };

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
                    let llvm_ty = resolve_llvm_type_ctx(&param.ty.node, context, &enum_variants, &enum_max_slots, &[]);
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

            let mut cx = Ctx {
                context,
                module,
                builder,
                current_fn: func,
                user_fns: &user_fns,
                fn_ret_types: &fn_ret_types,
                fn_asts: &fn_asts,
                fn_type_params: &fn_type_params,
                rt_fns: &rt_fns,
                vars,
                string_counter: &mut string_counter,
                struct_fields: &struct_fields,
                enum_variants: &enum_variants,
                enum_variant_fields: &enum_variant_fields,
                enum_max_slots: &enum_max_slots,
                constants: &constants_map,
                loop_stack: Vec::new(),
            };

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

    // Verify the module
    module.verify().map_err(|e| CodegenError {
        code: ErrorCode::E0405,
        message: format!("LLVM module verification failed: {}", e.to_string_lossy()),
    })?;

    Ok(())
}

// ── Helpers ─────────────────────────────────────────────────────────

/// Look up a variant name across all enums and return its tag index.
fn lookup_variant_tag(
    enum_variants: &HashMap<String, Vec<String>>,
    name: &str,
) -> Option<usize> {
    for variants in enum_variants.values() {
        if let Some(idx) = variants.iter().position(|v| v == name) {
            return Some(idx);
        }
    }
    None
}

// ── Expression compilation ──────────────────────────────────────────

fn compile_expr<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    expr: &Spanned<Expr>,
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    match &expr.node {
        Expr::IntLit(n) => {
            let val = cx.context.i64_type().const_int(*n as u64, true);
            Ok(Some((val.into(), TurboTy::Int)))
        }

        Expr::FloatLit(f) => {
            let val = cx.context.f64_type().const_float(*f);
            Ok(Some((val.into(), TurboTy::Float)))
        }

        Expr::BoolLit(b) => {
            let val = cx.context.i8_type().const_int(*b as u64, false);
            Ok(Some((val.into(), TurboTy::Bool)))
        }

        Expr::StringLit(s) => {
            let ptr = cx.create_string(s)?;
            Ok(Some((ptr.into(), TurboTy::Str)))
        }

        Expr::Unit => Ok(None),

        Expr::Ident(name) => {
            // Check constants first
            if let Some(const_expr) = cx.constants.get(name.as_str()) {
                let const_expr = const_expr.clone();
                return compile_expr(cx, &const_expr);
            }
            let (alloca, turbo_ty) = cx.vars.get(name).ok_or_else(|| CodegenError {
                code: ErrorCode::E0401,
                message: format!("undefined variable: {name}"),
            })?;
            let turbo_ty = turbo_ty.clone();
            let llvm_ty = turbo_ty_to_llvm_ctx(&turbo_ty, cx.context, cx.enum_max_slots);
            let val = cx
                .builder
                .build_load(llvm_ty, *alloca, name)
                .expect("build_load failed");
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
                        let result = cx.rt_call(
                            "rt_str_concat",
                            &[lhs.into(), rhs.into()],
                        ).unwrap();
                        return Ok(Some((result, TurboTy::Str)));
                    }
                    BinOp::Eq | BinOp::NotEq => {
                        let result = cx
                            .rt_call("rt_str_eq", &[lhs.into(), rhs.into()])
                            .unwrap();
                        let result_int = result.into_int_value();
                        if *op == BinOp::NotEq {
                            let one = cx.context.i8_type().const_int(1, false);
                            let flipped = cx
                                .builder
                                .build_xor(result_int, one, "neq")
                                .expect("build_xor failed");
                            return Ok(Some((flipped.into(), TurboTy::Bool)));
                        }
                        return Ok(Some((result, TurboTy::Bool)));
                    }
                    _ => {}
                }
            }

            // String coercion: str + non-str or non-str + str
            if *op == BinOp::Add {
                if lhs_tty == TurboTy::Str && rhs_tty != TurboTy::Str {
                    let rhs_str = convert_to_str(cx, rhs, &rhs_tty)?;
                    let result = cx
                        .rt_call("rt_str_concat", &[lhs.into(), rhs_str.into()])
                        .unwrap();
                    return Ok(Some((result, TurboTy::Str)));
                }
                if rhs_tty == TurboTy::Str && lhs_tty != TurboTy::Str {
                    let lhs_str = convert_to_str(cx, lhs, &lhs_tty)?;
                    let result = cx
                        .rt_call("rt_str_concat", &[lhs_str.into(), rhs.into()])
                        .unwrap();
                    return Ok(Some((result, TurboTy::Str)));
                }
            }

            let result = compile_binop(cx, lhs, *op, rhs)?;
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
                UnaryOp::Neg => match val {
                    BasicValueEnum::FloatValue(fv) => cx
                        .builder
                        .build_float_neg(fv, "fneg")
                        .expect("build_float_neg failed")
                        .into(),
                    BasicValueEnum::IntValue(iv) => cx
                        .builder
                        .build_int_neg(iv, "ineg")
                        .expect("build_int_neg failed")
                        .into(),
                    _ => {
                        return Err(CodegenError {
                            code: ErrorCode::E0403,
                            message: "cannot negate this type".to_string(),
                        })
                    }
                },
                UnaryOp::Not => {
                    let iv = val.into_int_value();
                    let one = cx.context.i8_type().const_int(1, false);
                    cx.builder
                        .build_xor(iv, one, "not")
                        .expect("build_xor failed")
                        .into()
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

            // Emit deferred expressions in LIFO order
            for defer_expr in deferred.iter().rev() {
                let block = cx.builder.get_insert_block().unwrap();
                if block.get_terminator().is_none() {
                    compile_expr(cx, defer_expr)?;
                }
            }

            cx.vars = saved_vars;
            result
        }

        Expr::Assign { target, value } => {
            let (val, tty) = compile_expr(cx, value)?.unwrap();
            let (alloca, _) = cx.vars.get(target).ok_or_else(|| CodegenError {
                code: ErrorCode::E0401,
                message: format!("undefined variable: {target}"),
            })?;
            let alloca = *alloca;
            cx.builder
                .build_store(alloca, val)
                .expect("build_store failed");
            // Update type
            if let Some(entry) = cx.vars.get_mut(target) {
                entry.1 = tty;
            }
            Ok(None)
        }

        Expr::CompoundAssign { target, op, value } => {
            let (rhs, _) = compile_expr(cx, value)?.unwrap();
            let (alloca, turbo_ty) = cx.vars.get(target).ok_or_else(|| CodegenError {
                code: ErrorCode::E0401,
                message: format!("undefined variable: {target}"),
            })?;
            let alloca = *alloca;
            let turbo_ty = turbo_ty.clone();
            let llvm_ty = turbo_ty_to_llvm_ctx(&turbo_ty, cx.context, cx.enum_max_slots);
            let lhs = cx
                .builder
                .build_load(llvm_ty, alloca, target)
                .expect("build_load failed");
            let result = compile_binop(cx, lhs, *op, rhs)?;
            cx.builder
                .build_store(alloca, result)
                .expect("build_store failed");
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

            let offset = field_index as u64 * 8;
            let obj_ptr_val = obj_ptr.into_pointer_value();

            // GEP to field offset
            let field_ptr = unsafe {
                cx.builder
                    .build_gep(
                        cx.context.i8_type(),
                        obj_ptr_val,
                        &[cx.context.i64_type().const_int(offset, false)],
                        "field_ptr",
                    )
                    .expect("build_gep failed")
            };

            // Widen to i64 for uniform storage
            let store_val = widen_for_storage(cx, val);
            cx.builder
                .build_store(field_ptr, store_val)
                .expect("build_store failed");
            Ok(None)
        }

        Expr::IndexAssign {
            object,
            index,
            value,
        } => {
            let (arr, _) = compile_expr(cx, object)?.unwrap();
            let (idx, _) = compile_expr(cx, index)?.unwrap();
            let (val, _) = compile_expr(cx, value)?.unwrap();

            let store_val = widen_for_storage(cx, val);
            let new_arr = cx
                .rt_call(
                    "rt_array_set",
                    &[arr.into(), idx.into(), store_val.into()],
                )
                .unwrap();

            // Update the variable to point to the (possibly new) array
            if let Expr::Ident(name) = &object.node {
                if let Some((alloca, _)) = cx.vars.get(name) {
                    cx.builder
                        .build_store(*alloca, new_arr)
                        .expect("build_store failed");
                }
            }

            Ok(None)
        }

        Expr::While { condition, body } => compile_while(cx, condition, body),

        Expr::ForIn {
            var_name,
            iterable,
            body,
        } => compile_for_in(cx, var_name, iterable, body),

        Expr::ArrayLit(elems) => {
            let len = elems.len() as u64;
            let len_val = cx.context.i64_type().const_int(len, false);
            let arr = cx
                .rt_call("rt_array_alloc", &[len_val.into()])
                .unwrap();

            let mut elem_tty = TurboTy::Int;
            for (i, elem) in elems.iter().enumerate() {
                let (val, tty) = compile_expr(cx, elem)?.unwrap();
                if i == 0 {
                    elem_tty = tty;
                }
                let idx = cx.context.i64_type().const_int(i as u64, false);
                let store_val = widen_for_storage(cx, val);
                cx.rt_call("rt_array_set", &[arr.into(), idx.into(), store_val.into()]);
            }

            Ok(Some((arr, TurboTy::Array(Box::new(elem_tty)))))
        }

        Expr::Index { object, index } => {
            let (obj, obj_tty) = compile_expr(cx, object)?.unwrap();
            let (idx, _) = compile_expr(cx, index)?.unwrap();

            let elem_tty = match &obj_tty {
                TurboTy::Array(inner) => *inner.clone(),
                _ => TurboTy::Int,
            };

            let raw = cx
                .rt_call("rt_array_get", &[obj.into(), idx.into()])
                .unwrap();

            // Narrow the result back from i64 to the element type
            let result = narrow_from_storage(cx, raw, &elem_tty);
            Ok(Some((result, elem_tty)))
        }

        Expr::StructLit { name, fields } => {
            let struct_layout = cx
                .struct_fields
                .get(name)
                .ok_or_else(|| CodegenError {
                    code: ErrorCode::E0400,
                    message: format!("undefined struct: {name}"),
                })?
                .clone();

            let num_fields = struct_layout.len() as u64;
            let num_fields_val = cx.context.i64_type().const_int(num_fields, false);
            let ptr = cx
                .rt_call("rt_struct_alloc", &[num_fields_val.into()])
                .unwrap()
                .into_pointer_value();

            for (field_name, field_expr) in fields {
                let (val, _) = compile_expr(cx, field_expr)?.unwrap();
                let field_index = struct_layout
                    .iter()
                    .position(|(n, _)| n == field_name)
                    .ok_or_else(|| CodegenError {
                        code: ErrorCode::E0400,
                        message: format!("struct `{name}` has no field `{field_name}`"),
                    })?;

                let offset = field_index as u64 * 8;
                let field_ptr = unsafe {
                    cx.builder
                        .build_gep(
                            cx.context.i8_type(),
                            ptr,
                            &[cx.context.i64_type().const_int(offset, false)],
                            "field_ptr",
                        )
                        .expect("build_gep failed")
                };

                let store_val = widen_for_storage(cx, val);
                cx.builder
                    .build_store(field_ptr, store_val)
                    .expect("build_store failed");
            }

            Ok(Some((ptr.into(), TurboTy::Struct(name.clone()))))
        }

        Expr::FieldAccess { object, field } => {
            // Check if this is an enum variant access: EnumName.VariantName
            if let Expr::Ident(ref name) = object.node {
                if let Some(variants) = cx.enum_variants.get(name.as_str()) {
                    let index = variants
                        .iter()
                        .position(|v| v == field)
                        .ok_or_else(|| CodegenError {
                            code: ErrorCode::E0400,
                            message: format!("enum `{name}` has no variant `{field}`"),
                        })?;

                    if let Some(&max_slots) = cx.enum_max_slots.get(name.as_str()) {
                        // Data-carrying enum: allocate tagged union
                        let total_slots = 1 + max_slots;
                        let num_fields_val = cx
                            .context
                            .i64_type()
                            .const_int(total_slots as u64, false);
                        let ptr = cx
                            .rt_call("rt_struct_alloc", &[num_fields_val.into()])
                            .unwrap()
                            .into_pointer_value();
                        let tag_val = cx
                            .context
                            .i64_type()
                            .const_int(index as u64, false);
                        cx.builder
                            .build_store(ptr, tag_val)
                            .expect("build_store failed");
                        return Ok(Some((ptr.into(), TurboTy::Enum(name.clone()))));
                    } else {
                        let val = cx
                            .context
                            .i64_type()
                            .const_int(index as u64, false);
                        return Ok(Some((val.into(), TurboTy::Enum(name.clone()))));
                    }
                }
            }

            let (obj, obj_tty) = compile_expr(cx, object)?.unwrap();

            let struct_name = match &obj_tty {
                TurboTy::Struct(name) => name.clone(),
                _ => {
                    return Err(CodegenError {
                        code: ErrorCode::E0400,
                        message: format!("field access on non-struct type: {field}"),
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

            let (field_index, (_, field_tty)) = struct_layout
                .iter()
                .enumerate()
                .find(|(_, (n, _))| n == field)
                .ok_or_else(|| CodegenError {
                    code: ErrorCode::E0400,
                    message: format!("struct `{struct_name}` has no field `{field}`"),
                })?;

            let field_tty = field_tty.clone();
            let offset = field_index as u64 * 8;
            let obj_ptr = obj.into_pointer_value();

            let field_ptr = unsafe {
                cx.builder
                    .build_gep(
                        cx.context.i8_type(),
                        obj_ptr,
                        &[cx.context.i64_type().const_int(offset, false)],
                        "field_ptr",
                    )
                    .expect("build_gep failed")
            };

            // Load as i64 then narrow to the field type
            let raw = cx
                .builder
                .build_load(cx.context.i64_type(), field_ptr, field)
                .expect("build_load failed");
            let result = narrow_from_storage(cx, raw, &field_tty);
            Ok(Some((result, field_tty)))
        }

        Expr::EnumVariant { enum_name, variant } => {
            let variants = cx.enum_variants.get(enum_name).ok_or_else(|| CodegenError {
                code: ErrorCode::E0400,
                message: format!("undefined enum: {enum_name}"),
            })?;
            let variant_index = variants
                .iter()
                .position(|v| v == variant)
                .ok_or_else(|| CodegenError {
                    code: ErrorCode::E0400,
                    message: format!("enum `{enum_name}` has no variant `{variant}`"),
                })?;
            let val = cx
                .context
                .i64_type()
                .const_int(variant_index as u64, false);
            Ok(Some((val.into(), TurboTy::Enum(enum_name.clone()))))
        }

        Expr::Match { subject, arms } => compile_match(cx, subject, arms),

        Expr::Interpolation(parts) => {
            let empty_str = cx.create_string("")?;
            let mut result: BasicValueEnum<'ctx> = empty_str.into();

            for part in parts {
                match part {
                    InterpolPart::Lit(s) => {
                        let lit_ptr = cx.create_string(s)?;
                        result = cx
                            .rt_call("rt_str_concat", &[result.into(), lit_ptr.into()])
                            .unwrap();
                    }
                    InterpolPart::Expr(e) => {
                        let (val, tty) = compile_expr(cx, e)?.unwrap();
                        let str_val = convert_to_str(cx, val, &tty)?;
                        result = cx
                            .rt_call("rt_str_concat", &[result.into(), str_val.into()])
                            .unwrap();
                    }
                }
            }

            Ok(Some((result, TurboTy::Str)))
        }

        Expr::Range { start, end } => {
            // Ranges are only used inside for-in, but if used standalone, return a tuple-like thing
            let (start_val, _) = compile_expr(cx, start)?.unwrap();
            let (end_val, _) = compile_expr(cx, end)?.unwrap();
            // Store as array [start, end]
            let len_val = cx.context.i64_type().const_int(2, false);
            let arr = cx
                .rt_call("rt_array_alloc", &[len_val.into()])
                .unwrap();
            let idx0 = cx.context.i64_type().const_int(0, false);
            let idx1 = cx.context.i64_type().const_int(1, false);
            cx.rt_call("rt_array_set", &[arr.into(), idx0.into(), start_val.into()]);
            cx.rt_call("rt_array_set", &[arr.into(), idx1.into(), end_val.into()]);
            Ok(Some((arr, TurboTy::Array(Box::new(TurboTy::Int)))))
        }

        Expr::OkExpr(inner) => {
            let (val, _) = compile_expr(cx, inner)?.unwrap();
            let val_i64 = widen_for_storage(cx, val);
            let result = cx
                .rt_call("rt_result_ok", &[val_i64.into()])
                .unwrap();
            Ok(Some((result, TurboTy::Result(Box::new(TurboTy::Int), Box::new(TurboTy::Str)))))
        }

        Expr::ErrExpr(inner) => {
            let (val, _) = compile_expr(cx, inner)?.unwrap();
            let val_i64 = widen_for_storage(cx, val);
            let result = cx
                .rt_call("rt_result_err", &[val_i64.into()])
                .unwrap();
            Ok(Some((result, TurboTy::Result(Box::new(TurboTy::Int), Box::new(TurboTy::Str)))))
        }

        Expr::SomeExpr(inner) => {
            let (val, _) = compile_expr(cx, inner)?.unwrap();
            let val_i64 = widen_for_storage(cx, val);
            let result = cx
                .rt_call("rt_option_some", &[val_i64.into()])
                .unwrap();
            Ok(Some((result, TurboTy::Optional(Box::new(TurboTy::Int)))))
        }

        Expr::NoneExpr => {
            let result = cx.rt_call("rt_option_none", &[]).unwrap();
            Ok(Some((result, TurboTy::Optional(Box::new(TurboTy::Int)))))
        }

        Expr::NullCoalesce { value, default } => {
            let (val, val_tty) = compile_expr(cx, value)?.unwrap();
            // Get the tag
            let tag = cx
                .rt_call("rt_option_tag", &[val.into()])
                .unwrap()
                .into_int_value();
            let zero = cx.context.i64_type().const_int(0, false);
            let is_none = cx
                .builder
                .build_int_compare(IntPredicate::EQ, tag, zero, "is_none")
                .expect("build_int_compare failed");

            let then_block = cx
                .context
                .append_basic_block(cx.current_fn, "coalesce_none");
            let else_block = cx
                .context
                .append_basic_block(cx.current_fn, "coalesce_some");
            let merge_block = cx
                .context
                .append_basic_block(cx.current_fn, "coalesce_merge");

            cx.builder
                .build_conditional_branch(is_none, then_block, else_block)
                .expect("build_conditional_branch failed");

            // None case: use default
            cx.builder.position_at_end(then_block);
            let (default_val, default_tty) = compile_expr(cx, default)?.unwrap();
            let then_end_block = cx.builder.get_insert_block().unwrap();
            cx.builder
                .build_unconditional_branch(merge_block)
                .expect("build_unconditional_branch failed");

            // Some case: unwrap
            cx.builder.position_at_end(else_block);
            let unwrapped = cx
                .rt_call("rt_option_value", &[val.into()])
                .unwrap();
            let inner_tty = match &val_tty {
                TurboTy::Optional(inner) => *inner.clone(),
                _ => TurboTy::Int,
            };
            let unwrapped = narrow_from_storage(cx, unwrapped, &inner_tty);
            let else_end_block = cx.builder.get_insert_block().unwrap();
            cx.builder
                .build_unconditional_branch(merge_block)
                .expect("build_unconditional_branch failed");

            cx.builder.position_at_end(merge_block);
            let phi = cx
                .builder
                .build_phi(default_val.get_type(), "coalesce")
                .expect("build_phi failed");
            phi.add_incoming(&[
                (&default_val, then_end_block),
                (&unwrapped, else_end_block),
            ]);

            Ok(Some((phi.as_basic_value(), default_tty)))
        }

        Expr::Closure { .. } => {
            todo!("LLVM: Closure not yet implemented")
        }

        Expr::Await(inner) => {
            let result = compile_expr(cx, inner)?;
            if let Some((val, tty)) = result {
                match tty {
                    TurboTy::Future(inner_tty) => {
                        let joined = cx
                            .rt_call("rt_await_handle", &[val.into()])
                            .unwrap();
                        let narrowed = narrow_from_storage(cx, joined, &inner_tty);
                        Ok(Some((narrowed, *inner_tty)))
                    }
                    _ => Ok(Some((val, tty))),
                }
            } else {
                Ok(None)
            }
        }

        Expr::Spawn(_) => {
            todo!("LLVM: Spawn not yet implemented")
        }

        Expr::Try(inner) => {
            let (val, val_tty) = compile_expr(cx, inner)?.unwrap();
            // Check tag: 0 = Ok, 1 = Err
            let tag = cx
                .rt_call("rt_result_tag", &[val.into()])
                .unwrap()
                .into_int_value();
            let one = cx.context.i64_type().const_int(1, false);
            let is_err = cx
                .builder
                .build_int_compare(IntPredicate::EQ, tag, one, "is_err")
                .expect("build_int_compare failed");

            let err_block = cx
                .context
                .append_basic_block(cx.current_fn, "try_err");
            let ok_block = cx
                .context
                .append_basic_block(cx.current_fn, "try_ok");

            cx.builder
                .build_conditional_branch(is_err, err_block, ok_block)
                .expect("build_conditional_branch failed");

            // Error path: propagate
            cx.builder.position_at_end(err_block);
            let err_val = cx
                .rt_call("rt_result_value", &[val.into()])
                .unwrap();
            let err_result = cx
                .rt_call("rt_result_err", &[err_val.into()])
                .unwrap();
            cx.builder
                .build_return(Some(&err_result))
                .expect("build_return failed");

            // Ok path: unwrap
            cx.builder.position_at_end(ok_block);
            let ok_val = cx
                .rt_call("rt_result_value", &[val.into()])
                .unwrap();
            let inner_tty = match &val_tty {
                TurboTy::Result(ok, _) => *ok.clone(),
                _ => TurboTy::Int,
            };
            let narrowed = narrow_from_storage(cx, ok_val, &inner_tty);
            Ok(Some((narrowed, inner_tty)))
        }

        Expr::Break => {
            if let Some((_, exit_block)) = cx.loop_stack.last() {
                cx.builder
                    .build_unconditional_branch(*exit_block)
                    .expect("build_unconditional_branch failed");
                // Create unreachable block for subsequent code
                let dead_block = cx
                    .context
                    .append_basic_block(cx.current_fn, "after_break");
                cx.builder.position_at_end(dead_block);
            }
            Ok(None)
        }

        Expr::Continue => {
            if let Some((header_block, _)) = cx.loop_stack.last() {
                cx.builder
                    .build_unconditional_branch(*header_block)
                    .expect("build_unconditional_branch failed");
                let dead_block = cx
                    .context
                    .append_basic_block(cx.current_fn, "after_continue");
                cx.builder.position_at_end(dead_block);
            }
            Ok(None)
        }
    }
}

// ── Statement compilation ───────────────────────────────────────────

fn compile_stmt<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    stmt: &Spanned<Stmt>,
) -> Result<(), CodegenError> {
    match &stmt.node {
        Stmt::Let { name, value, .. } => {
            let result = compile_expr(cx, value)?;
            let (llvm_ty, turbo_ty, val) = if let Some((v, tty)) = result {
                (v.get_type(), tty, Some(v))
            } else {
                (
                    cx.context.i64_type().as_basic_type_enum(),
                    TurboTy::Unit,
                    None,
                )
            };
            let alloca = cx.create_entry_block_alloca(llvm_ty, name);
            if let Some(v) = val {
                cx.builder
                    .build_store(alloca, v)
                    .expect("build_store failed");
            }
            cx.vars.insert(name.clone(), (alloca, turbo_ty));
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
                    cx.builder
                        .build_return(Some(&v))
                        .expect("build_return failed");
                } else {
                    cx.builder.build_return(None).expect("build_return failed");
                }
            } else {
                cx.builder.build_return(None).expect("build_return failed");
            }
            // Create dead block for subsequent code
            let dead_block = cx
                .context
                .append_basic_block(cx.current_fn, "after_return");
            cx.builder.position_at_end(dead_block);
            Ok(())
        }
        Stmt::Defer(_) => {
            // Handled at block level
            Ok(())
        }
    }
}

// ── Binary operations ───────────────────────────────────────────────

fn compile_binop<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    lhs: BasicValueEnum<'ctx>,
    op: BinOp,
    rhs: BasicValueEnum<'ctx>,
) -> Result<BasicValueEnum<'ctx>, CodegenError> {
    // Float operations
    if let (BasicValueEnum::FloatValue(lf), BasicValueEnum::FloatValue(rf)) = (lhs, rhs) {
        let result = match op {
            BinOp::Add => cx
                .builder
                .build_float_add(lf, rf, "fadd")
                .expect("build_float_add failed")
                .into(),
            BinOp::Sub => cx
                .builder
                .build_float_sub(lf, rf, "fsub")
                .expect("build_float_sub failed")
                .into(),
            BinOp::Mul => cx
                .builder
                .build_float_mul(lf, rf, "fmul")
                .expect("build_float_mul failed")
                .into(),
            BinOp::Div => cx
                .builder
                .build_float_div(lf, rf, "fdiv")
                .expect("build_float_div failed")
                .into(),
            BinOp::Mod => cx
                .builder
                .build_float_rem(lf, rf, "fmod")
                .expect("build_float_rem failed")
                .into(),
            BinOp::Eq => cx
                .builder
                .build_float_compare(FloatPredicate::OEQ, lf, rf, "feq")
                .expect("build_float_compare failed")
                .into(),
            BinOp::NotEq => cx
                .builder
                .build_float_compare(FloatPredicate::ONE, lf, rf, "fneq")
                .expect("build_float_compare failed")
                .into(),
            BinOp::Less => cx
                .builder
                .build_float_compare(FloatPredicate::OLT, lf, rf, "flt")
                .expect("build_float_compare failed")
                .into(),
            BinOp::LessEq => cx
                .builder
                .build_float_compare(FloatPredicate::OLE, lf, rf, "fle")
                .expect("build_float_compare failed")
                .into(),
            BinOp::Greater => cx
                .builder
                .build_float_compare(FloatPredicate::OGT, lf, rf, "fgt")
                .expect("build_float_compare failed")
                .into(),
            BinOp::GreaterEq => cx
                .builder
                .build_float_compare(FloatPredicate::OGE, lf, rf, "fge")
                .expect("build_float_compare failed")
                .into(),
            _ => {
                return Err(CodegenError {
                    code: ErrorCode::E0403,
                    message: format!("unsupported float op: {op:?}"),
                })
            }
        };
        // Widen i1 comparison results to i8 for consistent Bool representation
        let result = widen_i1_to_i8(cx, result);
        return Ok(result);
    }

    // Integer operations
    let li = lhs.into_int_value();
    let ri = rhs.into_int_value();

    // Widen mismatched widths
    let (li, ri) = if li.get_type().get_bit_width() != ri.get_type().get_bit_width() {
        let target_bits = li
            .get_type()
            .get_bit_width()
            .max(ri.get_type().get_bit_width());
        let target_type = cx.context.custom_width_int_type(target_bits);
        let li = if li.get_type().get_bit_width() < target_bits {
            cx.builder
                .build_int_s_extend(li, target_type, "sext")
                .expect("build_int_s_extend failed")
        } else {
            li
        };
        let ri = if ri.get_type().get_bit_width() < target_bits {
            cx.builder
                .build_int_s_extend(ri, target_type, "sext")
                .expect("build_int_s_extend failed")
        } else {
            ri
        };
        (li, ri)
    } else {
        (li, ri)
    };

    let result: BasicValueEnum = match op {
        BinOp::Add => cx
            .builder
            .build_int_add(li, ri, "iadd")
            .expect("build_int_add failed")
            .into(),
        BinOp::Sub => cx
            .builder
            .build_int_sub(li, ri, "isub")
            .expect("build_int_sub failed")
            .into(),
        BinOp::Mul => cx
            .builder
            .build_int_mul(li, ri, "imul")
            .expect("build_int_mul failed")
            .into(),
        BinOp::Div => {
            emit_div_zero_check(cx, ri);
            cx.builder
                .build_int_signed_div(li, ri, "sdiv")
                .expect("build_int_signed_div failed")
                .into()
        }
        BinOp::Mod => {
            emit_div_zero_check(cx, ri);
            cx.builder
                .build_int_signed_rem(li, ri, "srem")
                .expect("build_int_signed_rem failed")
                .into()
        }
        BinOp::Eq => cx
            .builder
            .build_int_compare(IntPredicate::EQ, li, ri, "ieq")
            .expect("build_int_compare failed")
            .into(),
        BinOp::NotEq => cx
            .builder
            .build_int_compare(IntPredicate::NE, li, ri, "ineq")
            .expect("build_int_compare failed")
            .into(),
        BinOp::Less => cx
            .builder
            .build_int_compare(IntPredicate::SLT, li, ri, "ilt")
            .expect("build_int_compare failed")
            .into(),
        BinOp::LessEq => cx
            .builder
            .build_int_compare(IntPredicate::SLE, li, ri, "ile")
            .expect("build_int_compare failed")
            .into(),
        BinOp::Greater => cx
            .builder
            .build_int_compare(IntPredicate::SGT, li, ri, "igt")
            .expect("build_int_compare failed")
            .into(),
        BinOp::GreaterEq => cx
            .builder
            .build_int_compare(IntPredicate::SGE, li, ri, "ige")
            .expect("build_int_compare failed")
            .into(),
        BinOp::And => cx
            .builder
            .build_and(li, ri, "and")
            .expect("build_and failed")
            .into(),
        BinOp::Or => cx
            .builder
            .build_or(li, ri, "or")
            .expect("build_or failed")
            .into(),
    };
    // Widen i1 comparison results to i8 for consistent Bool representation
    let result = widen_i1_to_i8(cx, result);
    Ok(result)
}

/// If a value is i1 (LLVM comparison result), widen it to i8 (Turbo Bool type).
fn widen_i1_to_i8<'a, 'ctx>(
    cx: &Ctx<'a, 'ctx>,
    val: BasicValueEnum<'ctx>,
) -> BasicValueEnum<'ctx> {
    if let BasicValueEnum::IntValue(iv) = val {
        if iv.get_type().get_bit_width() == 1 {
            return cx
                .builder
                .build_int_z_extend(iv, cx.context.i8_type(), "i1_to_i8")
                .expect("build_int_z_extend failed")
                .into();
        }
    }
    val
}

fn emit_div_zero_check<'a, 'ctx>(cx: &mut Ctx<'a, 'ctx>, divisor: IntValue<'ctx>) {
    let zero = divisor.get_type().const_int(0, false);
    let is_zero = cx
        .builder
        .build_int_compare(IntPredicate::EQ, divisor, zero, "divzero")
        .expect("build_int_compare failed");

    let trap_block = cx
        .context
        .append_basic_block(cx.current_fn, "div_trap");
    let ok_block = cx
        .context
        .append_basic_block(cx.current_fn, "div_ok");

    cx.builder
        .build_conditional_branch(is_zero, trap_block, ok_block)
        .expect("build_conditional_branch failed");

    cx.builder.position_at_end(trap_block);
    cx.rt_call("rt_div_by_zero", &[]);
    cx.builder
        .build_unreachable()
        .expect("build_unreachable failed");

    cx.builder.position_at_end(ok_block);
}

// ── Short-circuit && / || ───────────────────────────────────────────

fn compile_short_circuit<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    left: &Spanned<Expr>,
    op: BinOp,
    right: &Spanned<Expr>,
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    let (lhs, _) = compile_expr(cx, left)?.unwrap();
    let lhs_bool = cx.to_bool(lhs);

    let eval_rhs_block = cx
        .context
        .append_basic_block(cx.current_fn, "sc_rhs");
    let merge_block = cx
        .context
        .append_basic_block(cx.current_fn, "sc_merge");

    let current_block = cx.builder.get_insert_block().unwrap();

    match op {
        BinOp::And => {
            cx.builder
                .build_conditional_branch(lhs_bool, eval_rhs_block, merge_block)
                .expect("build_conditional_branch failed");
        }
        BinOp::Or => {
            cx.builder
                .build_conditional_branch(lhs_bool, merge_block, eval_rhs_block)
                .expect("build_conditional_branch failed");
        }
        _ => unreachable!(),
    }

    cx.builder.position_at_end(eval_rhs_block);
    let (rhs, _) = compile_expr(cx, right)?.unwrap();
    let rhs_bool = cx.to_bool(rhs);
    let rhs_end_block = cx.builder.get_insert_block().unwrap();
    cx.builder
        .build_unconditional_branch(merge_block)
        .expect("build_unconditional_branch failed");

    cx.builder.position_at_end(merge_block);
    let phi = cx
        .builder
        .build_phi(cx.context.bool_type(), "sc_result")
        .expect("build_phi failed");

    match op {
        BinOp::And => {
            let false_val = cx.context.bool_type().const_int(0, false);
            phi.add_incoming(&[(&false_val, current_block), (&rhs_bool, rhs_end_block)]);
        }
        BinOp::Or => {
            let true_val = cx.context.bool_type().const_int(1, false);
            phi.add_incoming(&[(&true_val, current_block), (&rhs_bool, rhs_end_block)]);
        }
        _ => unreachable!(),
    }

    // Widen i1 back to i8 for consistent Bool representation
    let result = cx
        .builder
        .build_int_z_extend(phi.as_basic_value().into_int_value(), cx.context.i8_type(), "sc_zext")
        .expect("build_int_z_extend failed");
    Ok(Some((result.into(), TurboTy::Bool)))
}

// ── If/else ─────────────────────────────────────────────────────────

fn compile_if<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    condition: &Spanned<Expr>,
    then_branch: &Spanned<Expr>,
    else_branch: Option<&Spanned<Expr>>,
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    let (cond, _) = compile_expr(cx, condition)?.unwrap();
    let cond_bool = cx.to_bool(cond);

    let then_block = cx
        .context
        .append_basic_block(cx.current_fn, "then");
    let else_block = cx
        .context
        .append_basic_block(cx.current_fn, "else");
    let merge_block = cx
        .context
        .append_basic_block(cx.current_fn, "ifmerge");

    cx.builder
        .build_conditional_branch(cond_bool, then_block, else_block)
        .expect("build_conditional_branch failed");

    // Then branch
    cx.builder.position_at_end(then_block);
    let then_result = compile_expr(cx, then_branch)?;
    let then_end_block = cx.builder.get_insert_block().unwrap();
    let then_needs_jump = then_end_block.get_terminator().is_none();
    if then_needs_jump {
        cx.builder
            .build_unconditional_branch(merge_block)
            .expect("build_unconditional_branch failed");
    }

    // Else branch
    cx.builder.position_at_end(else_block);
    let else_result = if let Some(else_expr) = else_branch {
        compile_expr(cx, else_expr)?
    } else {
        None
    };
    let else_end_block = cx.builder.get_insert_block().unwrap();
    let else_needs_jump = else_end_block.get_terminator().is_none();
    if else_needs_jump {
        cx.builder
            .build_unconditional_branch(merge_block)
            .expect("build_unconditional_branch failed");
    }

    // Merge block
    cx.builder.position_at_end(merge_block);

    if let (Some((then_val, then_tty)), Some((else_val, _))) = (then_result, else_result) {
        if then_needs_jump && else_needs_jump {
            let phi = cx
                .builder
                .build_phi(then_val.get_type(), "ifphi")
                .expect("build_phi failed");
            phi.add_incoming(&[
                (&then_val, then_end_block),
                (&else_val, else_end_block),
            ]);
            Ok(Some((phi.as_basic_value(), then_tty)))
        } else if then_needs_jump {
            // Only then branch reaches merge
            Ok(Some((then_val, then_tty)))
        } else if else_needs_jump {
            // Only else branch reaches merge
            Ok(Some((else_val, then_tty)))
        } else {
            // Neither branch reaches merge (both return/break)
            Ok(None)
        }
    } else {
        Ok(None)
    }
}

// ── While loop ──────────────────────────────────────────────────────

fn compile_while<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    condition: &Spanned<Expr>,
    body: &Spanned<Expr>,
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    let header_block = cx
        .context
        .append_basic_block(cx.current_fn, "while_header");
    let body_block = cx
        .context
        .append_basic_block(cx.current_fn, "while_body");
    let exit_block = cx
        .context
        .append_basic_block(cx.current_fn, "while_exit");

    cx.builder
        .build_unconditional_branch(header_block)
        .expect("build_unconditional_branch failed");

    cx.builder.position_at_end(header_block);
    let (cond, _) = compile_expr(cx, condition)?.unwrap();
    let cond_bool = cx.to_bool(cond);
    cx.builder
        .build_conditional_branch(cond_bool, body_block, exit_block)
        .expect("build_conditional_branch failed");

    cx.builder.position_at_end(body_block);
    cx.loop_stack.push((header_block, exit_block));
    compile_expr(cx, body)?;
    cx.loop_stack.pop();

    let body_end = cx.builder.get_insert_block().unwrap();
    if body_end.get_terminator().is_none() {
        cx.builder
            .build_unconditional_branch(header_block)
            .expect("build_unconditional_branch failed");
    }

    cx.builder.position_at_end(exit_block);
    Ok(None)
}

// ── For-in loop ─────────────────────────────────────────────────────

fn compile_for_in<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    var_name: &str,
    iterable: &Spanned<Expr>,
    body: &Spanned<Expr>,
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    match &iterable.node {
        Expr::Range { start, end } => compile_for_in_range(cx, var_name, start, end, body),
        _ => compile_for_in_array(cx, var_name, iterable, body),
    }
}

fn compile_for_in_range<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    var_name: &str,
    start: &Spanned<Expr>,
    end: &Spanned<Expr>,
    body: &Spanned<Expr>,
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    let (range_start, _) = compile_expr(cx, start)?.unwrap();
    let (range_end, _) = compile_expr(cx, end)?.unwrap();

    let alloca = cx.create_entry_block_alloca(cx.context.i64_type().into(), var_name);
    cx.builder
        .build_store(alloca, range_start)
        .expect("build_store failed");
    cx.vars
        .insert(var_name.to_string(), (alloca, TurboTy::Int));

    let header_block = cx
        .context
        .append_basic_block(cx.current_fn, "forin_header");
    let body_block = cx
        .context
        .append_basic_block(cx.current_fn, "forin_body");
    let continue_block = cx
        .context
        .append_basic_block(cx.current_fn, "forin_continue");
    let exit_block = cx
        .context
        .append_basic_block(cx.current_fn, "forin_exit");

    cx.builder
        .build_unconditional_branch(header_block)
        .expect("build_unconditional_branch failed");

    cx.builder.position_at_end(header_block);
    let current_i = cx
        .builder
        .build_load(cx.context.i64_type(), alloca, "i")
        .expect("build_load failed")
        .into_int_value();
    let range_end_i = range_end.into_int_value();
    let cond = cx
        .builder
        .build_int_compare(IntPredicate::SLT, current_i, range_end_i, "forin_cond")
        .expect("build_int_compare failed");
    cx.builder
        .build_conditional_branch(cond, body_block, exit_block)
        .expect("build_conditional_branch failed");

    cx.builder.position_at_end(body_block);
    cx.loop_stack.push((continue_block, exit_block));
    compile_expr(cx, body)?;
    cx.loop_stack.pop();

    let body_end = cx.builder.get_insert_block().unwrap();
    if body_end.get_terminator().is_none() {
        cx.builder
            .build_unconditional_branch(continue_block)
            .expect("build_unconditional_branch failed");
    }

    cx.builder.position_at_end(continue_block);
    let updated_i = cx
        .builder
        .build_load(cx.context.i64_type(), alloca, "i_cur")
        .expect("build_load failed")
        .into_int_value();
    let one = cx.context.i64_type().const_int(1, false);
    let next_i = cx
        .builder
        .build_int_add(updated_i, one, "next_i")
        .expect("build_int_add failed");
    cx.builder
        .build_store(alloca, next_i)
        .expect("build_store failed");
    cx.builder
        .build_unconditional_branch(header_block)
        .expect("build_unconditional_branch failed");

    cx.builder.position_at_end(exit_block);
    Ok(None)
}

fn compile_for_in_array<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    var_name: &str,
    iterable: &Spanned<Expr>,
    body: &Spanned<Expr>,
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    let (arr, arr_tty) = compile_expr(cx, iterable)?.unwrap();
    let elem_tty = match &arr_tty {
        TurboTy::Array(inner) => *inner.clone(),
        _ => TurboTy::Int,
    };

    let arr_len = cx
        .rt_call("rt_array_len", &[arr.into()])
        .unwrap()
        .into_int_value();

    // Index counter
    let idx_alloca = cx.create_entry_block_alloca(cx.context.i64_type().into(), "__forin_idx");
    cx.builder
        .build_store(idx_alloca, cx.context.i64_type().const_int(0, false))
        .expect("build_store failed");

    // Loop variable
    let elem_llvm_ty = turbo_ty_to_llvm_ctx(&elem_tty, cx.context, cx.enum_max_slots);
    let var_alloca = cx.create_entry_block_alloca(elem_llvm_ty, var_name);
    cx.vars
        .insert(var_name.to_string(), (var_alloca, elem_tty.clone()));

    let header_block = cx
        .context
        .append_basic_block(cx.current_fn, "forin_arr_header");
    let body_block = cx
        .context
        .append_basic_block(cx.current_fn, "forin_arr_body");
    let continue_block = cx
        .context
        .append_basic_block(cx.current_fn, "forin_arr_continue");
    let exit_block = cx
        .context
        .append_basic_block(cx.current_fn, "forin_arr_exit");

    cx.builder
        .build_unconditional_branch(header_block)
        .expect("build_unconditional_branch failed");

    cx.builder.position_at_end(header_block);
    let idx = cx
        .builder
        .build_load(cx.context.i64_type(), idx_alloca, "idx")
        .expect("build_load failed")
        .into_int_value();
    let cond = cx
        .builder
        .build_int_compare(IntPredicate::SLT, idx, arr_len, "forin_arr_cond")
        .expect("build_int_compare failed");
    cx.builder
        .build_conditional_branch(cond, body_block, exit_block)
        .expect("build_conditional_branch failed");

    cx.builder.position_at_end(body_block);
    // Load element
    let idx2 = cx
        .builder
        .build_load(cx.context.i64_type(), idx_alloca, "idx")
        .expect("build_load failed");
    let raw_elem = cx
        .rt_call("rt_array_get", &[arr.into(), idx2.into()])
        .unwrap();
    let elem = narrow_from_storage(cx, raw_elem, &elem_tty);
    cx.builder
        .build_store(var_alloca, elem)
        .expect("build_store failed");

    cx.loop_stack.push((continue_block, exit_block));
    compile_expr(cx, body)?;
    cx.loop_stack.pop();

    let body_end = cx.builder.get_insert_block().unwrap();
    if body_end.get_terminator().is_none() {
        cx.builder
            .build_unconditional_branch(continue_block)
            .expect("build_unconditional_branch failed");
    }

    cx.builder.position_at_end(continue_block);
    let idx3 = cx
        .builder
        .build_load(cx.context.i64_type(), idx_alloca, "idx")
        .expect("build_load failed")
        .into_int_value();
    let one = cx.context.i64_type().const_int(1, false);
    let next = cx
        .builder
        .build_int_add(idx3, one, "next_idx")
        .expect("build_int_add failed");
    cx.builder
        .build_store(idx_alloca, next)
        .expect("build_store failed");
    cx.builder
        .build_unconditional_branch(header_block)
        .expect("build_unconditional_branch failed");

    cx.builder.position_at_end(exit_block);
    Ok(None)
}

// ── Match expression ────────────────────────────────────────────────

fn compile_match<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    subject: &Spanned<Expr>,
    arms: &[MatchArm],
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    let (subject_val, subject_tty) = compile_expr(cx, subject)?.unwrap();

    let merge_block = cx
        .context
        .append_basic_block(cx.current_fn, "match_merge");

    let mut arm_blocks: Vec<inkwell::basic_block::BasicBlock<'ctx>> = Vec::new();
    for i in 0..arms.len() {
        arm_blocks.push(
            cx.context
                .append_basic_block(cx.current_fn, &format!("match_arm_{i}")),
        );
    }
    let default_block = cx
        .context
        .append_basic_block(cx.current_fn, "match_default");

    // Build chain of comparisons
    let mut phi_incoming: Vec<(BasicValueEnum<'ctx>, inkwell::basic_block::BasicBlock<'ctx>)> =
        Vec::new();
    let mut first_arm_tty: Option<TurboTy> = None;

    for (i, arm) in arms.iter().enumerate() {
        let arm_block = arm_blocks[i];
        let next_block = if i + 1 < arms.len() {
            cx.context
                .append_basic_block(cx.current_fn, &format!("match_test_{}", i + 1))
        } else {
            default_block
        };

        // Only branch from the first test block for the first arm
        if i == 0 {
            // We're still at the end of the block before the match
        }

        // Build test
        let matches = match &arm.pattern.node {
            Pattern::Wildcard => None,
            Pattern::Ident(name) => {
                // Check if this ident is an enum variant name
                let variant_tag = lookup_variant_tag(cx.enum_variants, name);
                if let Some(tag_val) = variant_tag {
                    let pat_val = cx.context.i64_type().const_int(tag_val as u64, false);
                    if let TurboTy::Enum(ref enum_name) = subject_tty {
                        if cx.enum_max_slots.contains_key(enum_name) {
                            // Data enum: load tag from ptr
                            let ptr = subject_val.into_pointer_value();
                            let tag = cx
                                .builder
                                .build_load(cx.context.i64_type(), ptr, "tag")
                                .expect("build_load failed")
                                .into_int_value();
                            Some(
                                cx.builder
                                    .build_int_compare(IntPredicate::EQ, tag, pat_val, "var_eq")
                                    .expect("build_int_compare failed"),
                            )
                        } else {
                            // Unit enum: direct tag compare
                            let tag = subject_val.into_int_value();
                            Some(
                                cx.builder
                                    .build_int_compare(IntPredicate::EQ, tag, pat_val, "var_eq")
                                    .expect("build_int_compare failed"),
                            )
                        }
                    } else {
                        // Subject is int, compare directly
                        let tag = subject_val.into_int_value();
                        Some(
                            cx.builder
                                .build_int_compare(IntPredicate::EQ, tag, pat_val, "var_eq")
                                .expect("build_int_compare failed"),
                        )
                    }
                } else {
                    None // catch-all bind
                }
            }
            Pattern::IntLit(n) => {
                let pat_val = cx.context.i64_type().const_int(*n as u64, true);
                let subject_int = subject_val.into_int_value();
                Some(
                    cx.builder
                        .build_int_compare(IntPredicate::EQ, subject_int, pat_val, "pat_eq")
                        .expect("build_int_compare failed"),
                )
            }
            Pattern::BoolLit(b) => {
                let pat_val = cx.context.i8_type().const_int(*b as u64, false);
                let subject_int = subject_val.into_int_value();
                let subject_i8 = if subject_int.get_type().get_bit_width() > 8 {
                    cx.builder
                        .build_int_truncate(subject_int, cx.context.i8_type(), "trunc")
                        .expect("build_int_truncate failed")
                } else {
                    subject_int
                };
                Some(
                    cx.builder
                        .build_int_compare(IntPredicate::EQ, subject_i8, pat_val, "pat_eq")
                        .expect("build_int_compare failed"),
                )
            }
            Pattern::StringLit(s) => {
                let pat_ptr = cx.create_string(s)?;
                let eq = cx
                    .rt_call("rt_str_eq", &[subject_val.into(), pat_ptr.into()])
                    .unwrap()
                    .into_int_value();
                let zero = cx.context.i8_type().const_int(0, false);
                Some(
                    cx.builder
                        .build_int_compare(IntPredicate::NE, eq, zero, "pat_eq")
                        .expect("build_int_compare failed"),
                )
            }
            Pattern::Ok(_) => {
                // Result tag 0 = Ok
                let tag = cx
                    .rt_call("rt_result_tag", &[subject_val.into()])
                    .unwrap()
                    .into_int_value();
                let zero = cx.context.i64_type().const_int(0, false);
                Some(
                    cx.builder
                        .build_int_compare(IntPredicate::EQ, tag, zero, "is_ok")
                        .expect("build_int_compare failed"),
                )
            }
            Pattern::Err(_) => {
                let tag = cx
                    .rt_call("rt_result_tag", &[subject_val.into()])
                    .unwrap()
                    .into_int_value();
                let one = cx.context.i64_type().const_int(1, false);
                Some(
                    cx.builder
                        .build_int_compare(IntPredicate::EQ, tag, one, "is_err")
                        .expect("build_int_compare failed"),
                )
            }
            Pattern::Some(_) => {
                let tag = cx
                    .rt_call("rt_option_tag", &[subject_val.into()])
                    .unwrap()
                    .into_int_value();
                let one = cx.context.i64_type().const_int(1, false);
                Some(
                    cx.builder
                        .build_int_compare(IntPredicate::EQ, tag, one, "is_some")
                        .expect("build_int_compare failed"),
                )
            }
            Pattern::None => {
                let tag = cx
                    .rt_call("rt_option_tag", &[subject_val.into()])
                    .unwrap()
                    .into_int_value();
                let zero = cx.context.i64_type().const_int(0, false);
                Some(
                    cx.builder
                        .build_int_compare(IntPredicate::EQ, tag, zero, "is_none")
                        .expect("build_int_compare failed"),
                )
            }
            Pattern::VariantDestructure { variant, .. } => {
                // Match on enum tag
                if let TurboTy::Enum(ref enum_name) = subject_tty {
                    if let Some(variants) = cx.enum_variants.get(enum_name) {
                        if let Some(idx) = variants.iter().position(|v| v == variant) {
                            let tag_val = cx.context.i64_type().const_int(idx as u64, false);
                            // For data enums, load tag from heap
                            if cx.enum_max_slots.contains_key(enum_name) {
                                let ptr = subject_val.into_pointer_value();
                                let tag = cx
                                    .builder
                                    .build_load(cx.context.i64_type(), ptr, "tag")
                                    .expect("build_load failed")
                                    .into_int_value();
                                Some(
                                    cx.builder
                                        .build_int_compare(
                                            IntPredicate::EQ,
                                            tag,
                                            tag_val,
                                            "var_eq",
                                        )
                                        .expect("build_int_compare failed"),
                                )
                            } else {
                                let tag = subject_val.into_int_value();
                                Some(
                                    cx.builder
                                        .build_int_compare(
                                            IntPredicate::EQ,
                                            tag,
                                            tag_val,
                                            "var_eq",
                                        )
                                        .expect("build_int_compare failed"),
                                )
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
        };

        if let Some(cond) = matches {
            cx.builder
                .build_conditional_branch(cond, arm_block, next_block)
                .expect("build_conditional_branch failed");
        } else {
            // Wildcard or Ident: always matches
            cx.builder
                .build_unconditional_branch(arm_block)
                .expect("build_unconditional_branch failed");
        }

        // Compile arm body
        cx.builder.position_at_end(arm_block);

        // Bind pattern variables
        let saved_vars = cx.vars.clone();
        match &arm.pattern.node {
            Pattern::Ident(name) if name != "_" && lookup_variant_tag(cx.enum_variants, name).is_none() => {
                // Catch-all bind: bind subject to name
                let llvm_ty = subject_val.get_type();
                let alloca = cx.create_entry_block_alloca(llvm_ty, name);
                cx.builder
                    .build_store(alloca, subject_val)
                    .expect("build_store failed");
                cx.vars
                    .insert(name.clone(), (alloca, subject_tty.clone()));
            }
            Pattern::Ok(name) | Pattern::Some(name) => {
                let inner = if matches!(arm.pattern.node, Pattern::Ok(_)) {
                    cx.rt_call("rt_result_value", &[subject_val.into()])
                        .unwrap()
                } else {
                    cx.rt_call("rt_option_value", &[subject_val.into()])
                        .unwrap()
                };
                let alloca =
                    cx.create_entry_block_alloca(cx.context.i64_type().into(), name);
                cx.builder
                    .build_store(alloca, inner)
                    .expect("build_store failed");
                cx.vars.insert(name.clone(), (alloca, TurboTy::Int));
            }
            Pattern::Err(name) => {
                let inner = cx
                    .rt_call("rt_result_value", &[subject_val.into()])
                    .unwrap();
                let alloca =
                    cx.create_entry_block_alloca(cx.context.i64_type().into(), name);
                cx.builder
                    .build_store(alloca, inner)
                    .expect("build_store failed");
                cx.vars.insert(name.clone(), (alloca, TurboTy::Int));
            }
            Pattern::VariantDestructure { variant, bindings } => {
                // Bind destructured fields
                if let TurboTy::Enum(ref enum_name) = subject_tty {
                    if cx.enum_max_slots.contains_key(enum_name) {
                        let ptr = subject_val.into_pointer_value();
                        for (j, binding_name) in bindings.iter().enumerate() {
                            if binding_name == "_" {
                                continue;
                            }
                            let offset = ((j + 1) * 8) as u64;
                            let field_ptr = unsafe {
                                cx.builder
                                    .build_gep(
                                        cx.context.i8_type(),
                                        ptr,
                                        &[cx.context.i64_type().const_int(offset, false)],
                                        "vf_ptr",
                                    )
                                    .expect("build_gep failed")
                            };
                            let field_val = cx
                                .builder
                                .build_load(cx.context.i64_type(), field_ptr, binding_name)
                                .expect("build_load failed");
                            // Determine field type
                            let field_tty = cx
                                .enum_variant_fields
                                .get(&(enum_name.clone(), variant.clone()))
                                .and_then(|tys| tys.get(j).cloned())
                                .unwrap_or(TurboTy::Int);
                            let field_val = narrow_from_storage(cx, field_val, &field_tty);
                            let alloca = cx.create_entry_block_alloca(
                                field_val.get_type(),
                                binding_name,
                            );
                            cx.builder
                                .build_store(alloca, field_val)
                                .expect("build_store failed");
                            cx.vars
                                .insert(binding_name.clone(), (alloca, field_tty));
                        }
                    }
                }
            }
            _ => {}
        }

        let arm_result = compile_expr(cx, &arm.body)?;
        cx.vars = saved_vars;

        let arm_end_block = cx.builder.get_insert_block().unwrap();
        if arm_end_block.get_terminator().is_none() {
            if let Some((val, ref tty)) = arm_result {
                if first_arm_tty.is_none() {
                    first_arm_tty = Some(tty.clone());
                }
                phi_incoming.push((val, arm_end_block));
            }
            cx.builder
                .build_unconditional_branch(merge_block)
                .expect("build_unconditional_branch failed");
        }

        // Position at next test block if needed
        if i + 1 < arms.len() {
            cx.builder.position_at_end(next_block);
        }
    }

    // Default block (unreachable)
    cx.builder.position_at_end(default_block);
    cx.builder
        .build_unreachable()
        .expect("build_unreachable failed");

    // Merge
    cx.builder.position_at_end(merge_block);

    if !phi_incoming.is_empty() && first_arm_tty.is_some() {
        let first_type = phi_incoming[0].0.get_type();
        let phi = cx
            .builder
            .build_phi(first_type, "match_result")
            .expect("build_phi failed");
        for (val, block) in &phi_incoming {
            phi.add_incoming(&[(val, *block)]);
        }
        Ok(Some((phi.as_basic_value(), first_arm_tty.unwrap())))
    } else {
        Ok(None)
    }
}

// ── Function calls ──────────────────────────────────────────────────

fn compile_call<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    callee: &Spanned<Expr>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    // Method calls: expr.method(args)
    if let Expr::FieldAccess {
        ref object,
        ref field,
    } = callee.node
    {
        let (obj_val, obj_tty) = compile_expr(cx, object)?.unwrap();
        if let TurboTy::Struct(ref type_name) = obj_tty {
            let mangled = format!("{}__{}", type_name, field);
            if let Some(&func) = cx.user_fns.get(&mangled) {
                let mut arg_vals: Vec<BasicMetadataValueEnum> = vec![obj_val.into()];
                for arg in args {
                    if let Some((v, _)) = compile_expr(cx, arg)? {
                        arg_vals.push(v.into());
                    }
                }
                let call = cx
                    .builder
                    .build_direct_call(func, &arg_vals, "")
                    .expect("build_direct_call failed");
                let ret_tty = cx
                    .fn_ret_types
                    .get(&mangled)
                    .cloned()
                    .unwrap_or(TurboTy::Unit);
                return match call.try_as_basic_value().left() {
                    Some(val) => Ok(Some((val, ret_tty))),
                    None => Ok(None),
                };
            }
        }
        // String/array method calls
        return compile_method_call(cx, obj_val, &obj_tty, field, args);
    }

    let Expr::Ident(name) = &callee.node else {
        return Err(CodegenError {
            code: ErrorCode::E0400,
            message: "indirect function calls not yet supported in LLVM backend".to_string(),
        });
    };

    match name.as_str() {
        "print" => compile_print(cx, args),
        "panic" => compile_panic(cx, args),
        "assert" => compile_assert(cx, args),
        "assert_eq" => compile_assert_eq(cx, args, false),
        "assert_ne" => compile_assert_eq(cx, args, true),
        "len" => compile_len(cx, args),
        "abs" => compile_abs(cx, args),
        "min" => compile_min_max(cx, args, true),
        "max" => compile_min_max(cx, args, false),
        "to_str" => compile_to_str_builtin(cx, args),
        // Stdlib
        "split" => compile_stdlib_2arg_rt(cx, args, "rt_str_split", TurboTy::Array(Box::new(TurboTy::Str))),
        "trim" => compile_stdlib_1arg_rt(cx, args, "rt_str_trim", TurboTy::Str),
        "upper" => compile_stdlib_1arg_rt(cx, args, "rt_str_upper", TurboTy::Str),
        "lower" => compile_stdlib_1arg_rt(cx, args, "rt_str_lower", TurboTy::Str),
        "starts_with" => compile_stdlib_2arg_rt(cx, args, "rt_str_starts_with", TurboTy::Bool),
        "ends_with" => compile_stdlib_2arg_rt(cx, args, "rt_str_ends_with", TurboTy::Bool),
        "contains" => compile_stdlib_2arg_rt(cx, args, "rt_str_contains", TurboTy::Bool),
        "index_of" => compile_stdlib_2arg_rt(cx, args, "rt_str_index_of", TurboTy::Int),
        "replace" => {
            if args.len() >= 3 {
                let (a, _) = compile_expr(cx, &args[0])?.unwrap();
                let (b, _) = compile_expr(cx, &args[1])?.unwrap();
                let (c, _) = compile_expr(cx, &args[2])?.unwrap();
                let result = cx
                    .rt_call("rt_str_replace", &[a.into(), b.into(), c.into()])
                    .unwrap();
                Ok(Some((result, TurboTy::Str)))
            } else {
                Ok(None)
            }
        }
        "char_at" => compile_stdlib_2arg_rt(cx, args, "rt_str_char_at", TurboTy::Str),
        "join" => compile_stdlib_2arg_rt(cx, args, "rt_str_join", TurboTy::Str),
        "repeat" => compile_stdlib_2arg_rt(cx, args, "rt_str_repeat", TurboTy::Str),
        "read_line" => {
            let result = cx.rt_call("rt_read_line", &[]).unwrap();
            Ok(Some((result, TurboTy::Str)))
        }
        "read_file" => compile_stdlib_1arg_rt(cx, args, "rt_read_file", TurboTy::Str),
        "write_file" => {
            if args.len() >= 2 {
                let (a, _) = compile_expr(cx, &args[0])?.unwrap();
                let (b, _) = compile_expr(cx, &args[1])?.unwrap();
                cx.rt_call("rt_write_file", &[a.into(), b.into()]);
            }
            Ok(None)
        }
        "pow" => {
            if args.len() >= 2 {
                let (a, _) = compile_expr(cx, &args[0])?.unwrap();
                let (b, _) = compile_expr(cx, &args[1])?.unwrap();
                let result = cx
                    .rt_call("rt_pow", &[a.into(), b.into()])
                    .unwrap();
                Ok(Some((result, TurboTy::Int)))
            } else {
                Ok(None)
            }
        }
        "sqrt" => {
            if !args.is_empty() {
                let (a, _) = compile_expr(cx, &args[0])?.unwrap();
                let result = cx.rt_call("rt_sqrt", &[a.into()]).unwrap();
                Ok(Some((result, TurboTy::Float)))
            } else {
                Ok(None)
            }
        }
        "sleep" => {
            if !args.is_empty() {
                let (a, _) = compile_expr(cx, &args[0])?.unwrap();
                cx.rt_call("rt_sleep_ms", &[a.into()]);
            }
            Ok(None)
        }
        "http_get" => compile_stdlib_1arg_rt(cx, args, "rt_http_get", TurboTy::Str),
        "http_post" => compile_stdlib_2arg_rt(cx, args, "rt_http_post", TurboTy::Str),
        "json_get" => compile_stdlib_2arg_rt(cx, args, "rt_json_get", TurboTy::Str),
        "channel" => {
            let result = cx.rt_call("rt_channel_create", &[]).unwrap();
            Ok(Some((result, TurboTy::Struct("Channel".to_string()))))
        }
        "send" => {
            if args.len() >= 2 {
                let (ch, _) = compile_expr(cx, &args[0])?.unwrap();
                let (val, _) = compile_expr(cx, &args[1])?.unwrap();
                let val_i64 = widen_for_storage(cx, val);
                cx.rt_call("rt_channel_send", &[ch.into(), val_i64.into()]);
            }
            Ok(None)
        }
        "recv" => {
            if !args.is_empty() {
                let (ch, _) = compile_expr(cx, &args[0])?.unwrap();
                let result = cx.rt_call("rt_channel_recv", &[ch.into()]).unwrap();
                Ok(Some((result, TurboTy::Int)))
            } else {
                Ok(None)
            }
        }
        "mutex" => {
            if !args.is_empty() {
                let (val, _) = compile_expr(cx, &args[0])?.unwrap();
                let val_i64 = widen_for_storage(cx, val);
                let result = cx
                    .rt_call("rt_mutex_create", &[val_i64.into()])
                    .unwrap();
                Ok(Some((result, TurboTy::Struct("Mutex".to_string()))))
            } else {
                Ok(None)
            }
        }
        "mutex_get" => {
            if !args.is_empty() {
                let (m, _) = compile_expr(cx, &args[0])?.unwrap();
                let result = cx.rt_call("rt_mutex_get", &[m.into()]).unwrap();
                Ok(Some((result, TurboTy::Int)))
            } else {
                Ok(None)
            }
        }
        "mutex_set" => {
            if args.len() >= 2 {
                let (m, _) = compile_expr(cx, &args[0])?.unwrap();
                let (val, _) = compile_expr(cx, &args[1])?.unwrap();
                let val_i64 = widen_for_storage(cx, val);
                cx.rt_call("rt_mutex_set", &[m.into(), val_i64.into()]);
            }
            Ok(None)
        }
        "hashmap" => {
            let result = cx.rt_call("rt_hashmap_new", &[]).unwrap();
            Ok(Some((result, TurboTy::Struct("HashMap".to_string()))))
        }
        "hashmap_set" => {
            if args.len() >= 3 {
                let (m, _) = compile_expr(cx, &args[0])?.unwrap();
                let (k, _) = compile_expr(cx, &args[1])?.unwrap();
                let (v, _) = compile_expr(cx, &args[2])?.unwrap();
                cx.rt_call("rt_hashmap_set", &[m.into(), k.into(), v.into()]);
            }
            Ok(None)
        }
        "hashmap_get" => compile_stdlib_2arg_rt(cx, args, "rt_hashmap_get", TurboTy::Str),
        "hashmap_has" => compile_stdlib_2arg_rt(cx, args, "rt_hashmap_has", TurboTy::Bool),
        "hashmap_len" => compile_stdlib_1arg_rt(cx, args, "rt_hashmap_len", TurboTy::Int),
        "hashmap_keys" => compile_stdlib_1arg_rt(cx, args, "rt_hashmap_keys", TurboTy::Array(Box::new(TurboTy::Str))),
        "hashmap_remove" => {
            if args.len() >= 2 {
                let (m, _) = compile_expr(cx, &args[0])?.unwrap();
                let (k, _) = compile_expr(cx, &args[1])?.unwrap();
                cx.rt_call("rt_hashmap_remove", &[m.into(), k.into()]);
            }
            Ok(None)
        }
        _ => {
            // Check enum variant construction
            if !args.is_empty() {
                if let Expr::Ident(ref first_name) = args[0].node {
                    if let Some(variants) = cx.enum_variants.get(first_name.as_str()) {
                        if let Some(variant_index) = variants.iter().position(|v| v == name) {
                            let data_args = &args[1..];
                            let enum_name = first_name;

                            if let Some(&max_slots) = cx.enum_max_slots.get(enum_name.as_str()) {
                                let total_slots = 1 + max_slots;
                                let num_fields_val = cx
                                    .context
                                    .i64_type()
                                    .const_int(total_slots as u64, false);
                                let ptr = cx
                                    .rt_call("rt_struct_alloc", &[num_fields_val.into()])
                                    .unwrap()
                                    .into_pointer_value();

                                // Store tag at offset 0
                                let tag_val = cx
                                    .context
                                    .i64_type()
                                    .const_int(variant_index as u64, false);
                                cx.builder
                                    .build_store(ptr, tag_val)
                                    .expect("build_store failed");

                                // Store fields at offsets 8, 16, ...
                                for (j, arg) in data_args.iter().enumerate() {
                                    let (val, _) = compile_expr(cx, arg)?.unwrap();
                                    let offset = ((j + 1) * 8) as u64;
                                    let field_ptr = unsafe {
                                        cx.builder
                                            .build_gep(
                                                cx.context.i8_type(),
                                                ptr,
                                                &[cx.context
                                                    .i64_type()
                                                    .const_int(offset, false)],
                                                "var_field_ptr",
                                            )
                                            .expect("build_gep failed")
                                    };
                                    let store_val = widen_for_storage(cx, val);
                                    cx.builder
                                        .build_store(field_ptr, store_val)
                                        .expect("build_store failed");
                                }

                                return Ok(Some((ptr.into(), TurboTy::Enum(enum_name.clone()))));
                            } else {
                                let val = cx
                                    .context
                                    .i64_type()
                                    .const_int(variant_index as u64, false);
                                return Ok(Some((val.into(), TurboTy::Enum(enum_name.clone()))));
                            }
                        }
                    }
                }
            }

            // UFCS method call: parser rewrites obj.method(args) -> method(obj, args)
            if cx.user_fns.get(name.as_str()).is_none() && !args.is_empty() {
                let (first_val, first_tty) = compile_expr(cx, &args[0])?.unwrap();
                if let TurboTy::Struct(ref type_name) = first_tty {
                    let mangled = format!("{}__{}", type_name, name);
                    if let Some(&func) = cx.user_fns.get(&mangled) {
                        let mut arg_vals: Vec<BasicMetadataValueEnum> = vec![first_val.into()];
                        for arg in &args[1..] {
                            if let Some((v, _)) = compile_expr(cx, arg)? {
                                arg_vals.push(v.into());
                            }
                        }
                        let call = cx
                            .builder
                            .build_direct_call(func, &arg_vals, "")
                            .expect("build_direct_call failed");
                        let ret_tty = cx
                            .fn_ret_types
                            .get(&mangled)
                            .cloned()
                            .unwrap_or(TurboTy::Unit);
                        return match call.try_as_basic_value().left() {
                            Some(val) => Ok(Some((val, ret_tty))),
                            None => Ok(None),
                        };
                    }
                }
            }

            // Regular user function call
            let func = *cx.user_fns.get(name.as_str()).ok_or_else(|| CodegenError {
                code: ErrorCode::E0402,
                message: format!("undefined function: {name}"),
            })?;

            let ret_tty = cx
                .fn_ret_types
                .get(name.as_str())
                .cloned()
                .unwrap_or(TurboTy::Unit);

            let mut arg_vals: Vec<BasicMetadataValueEnum> = Vec::new();
            for (i, arg) in args.iter().enumerate() {
                if let Some((val, _)) = compile_expr(cx, arg)? {
                    // Type coercion: match parameter types
                    let expected_type = func.get_type().get_param_types();
                    if i < expected_type.len() {
                        let val = coerce_arg(cx, val, expected_type[i]);
                        arg_vals.push(val.into());
                    } else {
                        arg_vals.push(val.into());
                    }
                }
            }

            let call = cx
                .builder
                .build_direct_call(func, &arg_vals, "")
                .expect("build_direct_call failed");

            match call.try_as_basic_value().left() {
                Some(val) => Ok(Some((val, ret_tty))),
                None => Ok(None),
            }
        }
    }
}

// ── Built-in functions ──────────────────────────────────────────────

fn compile_print<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    if args.is_empty() {
        let ptr = cx.create_string("")?;
        cx.rt_call("rt_print_str", &[ptr.into()]);
        return Ok(None);
    }

    let result = compile_expr(cx, &args[0])?;
    if let Some((v, tty)) = result {
        match tty {
            TurboTy::Str => {
                cx.rt_call("rt_print_str", &[v.into()]);
            }
            TurboTy::Float => {
                cx.rt_call("rt_print_f64", &[v.into()]);
            }
            TurboTy::Bool => {
                let iv = v.into_int_value();
                let iv = if iv.get_type().get_bit_width() > 8 {
                    cx.builder
                        .build_int_truncate(iv, cx.context.i8_type(), "tobool")
                        .expect("build_int_truncate failed")
                } else {
                    iv
                };
                cx.rt_call("rt_print_bool", &[iv.into()]);
            }
            TurboTy::Int => {
                let iv = v.into_int_value();
                let iv = if iv.get_type().get_bit_width() < 64 {
                    cx.builder
                        .build_int_s_extend(iv, cx.context.i64_type(), "ext")
                        .expect("build_int_s_extend failed")
                } else {
                    iv
                };
                cx.rt_call("rt_print_i64", &[iv.into()]);
            }
            TurboTy::Unit => {
                let ptr = cx.create_string("()")?;
                cx.rt_call("rt_print_str", &[ptr.into()]);
            }
            _ => {
                // For other types, print as int
                let ptr = cx.create_string("<value>")?;
                cx.rt_call("rt_print_str", &[ptr.into()]);
            }
        }
    }
    Ok(None)
}

fn compile_panic<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    if args.is_empty() {
        let ptr = cx.create_string("explicit panic")?;
        cx.rt_call("rt_panic", &[ptr.into()]);
    } else {
        let (val, _) = compile_expr(cx, &args[0])?.unwrap();
        cx.rt_call("rt_panic", &[val.into()]);
    }
    cx.builder
        .build_unreachable()
        .expect("build_unreachable failed");
    let dead = cx
        .context
        .append_basic_block(cx.current_fn, "after_panic");
    cx.builder.position_at_end(dead);
    Ok(None)
}

fn compile_assert<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    if args.is_empty() {
        return Ok(None);
    }
    let (cond, _) = compile_expr(cx, &args[0])?.unwrap();
    let cond_bool = cx.to_bool(cond);

    let fail_block = cx
        .context
        .append_basic_block(cx.current_fn, "assert_fail");
    let ok_block = cx
        .context
        .append_basic_block(cx.current_fn, "assert_ok");

    cx.builder
        .build_conditional_branch(cond_bool, ok_block, fail_block)
        .expect("build_conditional_branch failed");

    cx.builder.position_at_end(fail_block);
    if args.len() > 1 {
        let (msg, _) = compile_expr(cx, &args[1])?.unwrap();
        cx.rt_call("rt_assert_fail", &[msg.into()]);
    } else {
        let ptr = cx.create_string("assertion failed")?;
        cx.rt_call("rt_assert_fail", &[ptr.into()]);
    }
    cx.builder
        .build_unreachable()
        .expect("build_unreachable failed");

    cx.builder.position_at_end(ok_block);
    Ok(None)
}

fn compile_assert_eq<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    args: &[Spanned<Expr>],
    negate: bool,
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    if args.len() < 2 {
        return Ok(None);
    }
    let (a, _) = compile_expr(cx, &args[0])?.unwrap();
    let (b, _) = compile_expr(cx, &args[1])?.unwrap();

    let ai = a.into_int_value();
    let bi = b.into_int_value();
    let pred = if negate { IntPredicate::NE } else { IntPredicate::EQ };
    let eq = cx
        .builder
        .build_int_compare(pred, ai, bi, "assert_eq")
        .expect("build_int_compare failed");

    let fail_block = cx
        .context
        .append_basic_block(cx.current_fn, "assert_eq_fail");
    let ok_block = cx
        .context
        .append_basic_block(cx.current_fn, "assert_eq_ok");

    cx.builder
        .build_conditional_branch(eq, ok_block, fail_block)
        .expect("build_conditional_branch failed");

    cx.builder.position_at_end(fail_block);
    // Simple assert fail for now
    let ptr = cx.create_string("assertion failed: values not equal")?;
    cx.rt_call("rt_assert_fail", &[ptr.into()]);
    cx.builder
        .build_unreachable()
        .expect("build_unreachable failed");

    cx.builder.position_at_end(ok_block);
    Ok(None)
}

fn compile_len<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    if args.is_empty() {
        return Ok(None);
    }
    let (val, tty) = compile_expr(cx, &args[0])?.unwrap();
    let result = match tty {
        TurboTy::Str => cx.rt_call("rt_str_len", &[val.into()]).unwrap(),
        TurboTy::Array(_) => cx.rt_call("rt_array_len", &[val.into()]).unwrap(),
        _ => cx.context.i64_type().const_int(0, false).into(),
    };
    Ok(Some((result, TurboTy::Int)))
}

fn compile_abs<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    if args.is_empty() {
        return Ok(None);
    }
    let (val, tty) = compile_expr(cx, &args[0])?.unwrap();
    match tty {
        TurboTy::Int => {
            let iv = val.into_int_value();
            let zero = cx.context.i64_type().const_int(0, false);
            let neg = cx
                .builder
                .build_int_neg(iv, "neg")
                .expect("build_int_neg failed");
            let is_neg = cx
                .builder
                .build_int_compare(IntPredicate::SLT, iv, zero, "is_neg")
                .expect("build_int_compare failed");
            let result: BasicValueEnum = cx
                .builder
                .build_select(is_neg, BasicValueEnum::IntValue(neg), BasicValueEnum::IntValue(iv), "abs")
                .expect("build_select failed");
            Ok(Some((result, TurboTy::Int)))
        }
        TurboTy::Float => {
            // Use llvm.fabs intrinsic via negation + select
            let fv = val.into_float_value();
            let zero = cx.context.f64_type().const_float(0.0);
            let neg = cx
                .builder
                .build_float_neg(fv, "fneg")
                .expect("build_float_neg failed");
            let is_neg = cx
                .builder
                .build_float_compare(FloatPredicate::OLT, fv, zero, "is_neg")
                .expect("build_float_compare failed");
            let result: BasicValueEnum = cx
                .builder
                .build_select(is_neg, BasicValueEnum::FloatValue(neg), BasicValueEnum::FloatValue(fv), "fabs")
                .expect("build_select failed");
            Ok(Some((result, TurboTy::Float)))
        }
        _ => Ok(Some((val, tty))),
    }
}

fn compile_min_max<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    args: &[Spanned<Expr>],
    is_min: bool,
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    if args.len() < 2 {
        return Ok(None);
    }
    let (a, tty) = compile_expr(cx, &args[0])?.unwrap();
    let (b, _) = compile_expr(cx, &args[1])?.unwrap();

    let cmp_pred = if is_min {
        IntPredicate::SLT
    } else {
        IntPredicate::SGT
    };

    let ai = a.into_int_value();
    let bi = b.into_int_value();
    let cond = cx
        .builder
        .build_int_compare(cmp_pred, ai, bi, "cmp")
        .expect("build_int_compare failed");
    let result: BasicValueEnum = cx
        .builder
        .build_select(cond, BasicValueEnum::IntValue(ai), BasicValueEnum::IntValue(bi), if is_min { "min" } else { "max" })
        .expect("build_select failed");
    Ok(Some((result, tty)))
}

fn compile_to_str_builtin<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    if args.is_empty() {
        return Ok(None);
    }
    let (val, tty) = compile_expr(cx, &args[0])?.unwrap();
    let str_val = convert_to_str(cx, val, &tty)?;
    Ok(Some((str_val, TurboTy::Str)))
}

fn compile_stdlib_1arg_rt<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    args: &[Spanned<Expr>],
    rt_name: &str,
    ret_tty: TurboTy,
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    if args.is_empty() {
        return Ok(None);
    }
    let (a, _) = compile_expr(cx, &args[0])?.unwrap();
    let result = cx.rt_call(rt_name, &[a.into()]).unwrap();
    Ok(Some((result, ret_tty)))
}

fn compile_stdlib_2arg_rt<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    args: &[Spanned<Expr>],
    rt_name: &str,
    ret_tty: TurboTy,
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    if args.len() < 2 {
        return Ok(None);
    }
    let (a, _) = compile_expr(cx, &args[0])?.unwrap();
    let (b, _) = compile_expr(cx, &args[1])?.unwrap();
    let result = cx.rt_call(rt_name, &[a.into(), b.into()]).unwrap();
    Ok(Some((result, ret_tty)))
}

#[allow(unused_variables)]
fn compile_method_call<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    obj: BasicValueEnum<'ctx>,
    obj_tty: &TurboTy,
    field: &str,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    Err(CodegenError {
        code: ErrorCode::E0400,
        message: format!("LLVM: method call `{field}` not yet implemented"),
    })
}

// ── Value conversion helpers ────────────────────────────────────────

fn convert_to_str<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    val: BasicValueEnum<'ctx>,
    tty: &TurboTy,
) -> Result<BasicValueEnum<'ctx>, CodegenError> {
    match tty {
        TurboTy::Str => Ok(val),
        TurboTy::Int => {
            let iv = val.into_int_value();
            let iv = if iv.get_type().get_bit_width() < 64 {
                cx.builder
                    .build_int_s_extend(iv, cx.context.i64_type(), "ext")
                    .expect("build_int_s_extend failed")
            } else {
                iv
            };
            Ok(cx.rt_call("rt_i64_to_str", &[iv.into()]).unwrap())
        }
        TurboTy::Float => Ok(cx.rt_call("rt_f64_to_str", &[val.into()]).unwrap()),
        TurboTy::Bool => {
            let iv = val.into_int_value();
            let iv = if iv.get_type().get_bit_width() > 8 {
                cx.builder
                    .build_int_truncate(iv, cx.context.i8_type(), "trunc")
                    .expect("build_int_truncate failed")
            } else {
                iv
            };
            Ok(cx.rt_call("rt_bool_to_str", &[iv.into()]).unwrap())
        }
        _ => {
            let ptr = cx.create_string("<value>")?;
            Ok(ptr.into())
        }
    }
}

/// Widen a value to i64 for uniform heap storage.
fn widen_for_storage<'a, 'ctx>(
    cx: &Ctx<'a, 'ctx>,
    val: BasicValueEnum<'ctx>,
) -> BasicValueEnum<'ctx> {
    match val {
        BasicValueEnum::IntValue(iv) => {
            if iv.get_type().get_bit_width() < 64 {
                cx.builder
                    .build_int_s_extend(iv, cx.context.i64_type(), "widen")
                    .expect("build_int_s_extend failed")
                    .into()
            } else {
                val
            }
        }
        BasicValueEnum::FloatValue(fv) => {
            // bitcast f64 -> i64 for storage
            cx.builder
                .build_bit_cast(fv, cx.context.i64_type(), "f2i")
                .expect("build_bitcast failed")
        }
        BasicValueEnum::PointerValue(pv) => {
            // ptr -> i64 for storage in uniform-width arrays/structs
            cx.builder
                .build_ptr_to_int(pv, cx.context.i64_type(), "ptr2i")
                .expect("build_ptr_to_int failed")
                .into()
        }
        _ => val,
    }
}

/// Narrow a value from i64 storage back to its actual type.
fn narrow_from_storage<'a, 'ctx>(
    cx: &Ctx<'a, 'ctx>,
    val: BasicValueEnum<'ctx>,
    tty: &TurboTy,
) -> BasicValueEnum<'ctx> {
    match tty {
        TurboTy::Bool => {
            let iv = val.into_int_value();
            cx.builder
                .build_int_truncate(iv, cx.context.i8_type(), "narrow_bool")
                .expect("build_int_truncate failed")
                .into()
        }
        TurboTy::Float => {
            let iv = val.into_int_value();
            cx.builder
                .build_bit_cast(iv, cx.context.f64_type(), "i2f")
                .expect("build_bitcast failed")
        }
        TurboTy::Str | TurboTy::Array(_) | TurboTy::Struct(_)
        | TurboTy::Result(_, _) | TurboTy::Optional(_) | TurboTy::Agent(_) | TurboTy::Future(_) => {
            // i64 -> ptr via inttoptr
            let iv = val.into_int_value();
            cx.builder
                .build_int_to_ptr(iv, cx.context.ptr_type(AddressSpace::default()), "i2ptr")
                .expect("build_int_to_pointer failed")
                .into()
        }
        _ => val, // Int, Enum, etc. stay as i64
    }
}

/// Coerce argument value to match the expected LLVM type.
fn coerce_arg<'a, 'ctx>(
    cx: &Ctx<'a, 'ctx>,
    val: BasicValueEnum<'ctx>,
    expected: BasicTypeEnum<'ctx>,
) -> BasicValueEnum<'ctx> {
    let actual = val.get_type();
    if actual == expected {
        return val;
    }

    // Int width mismatch
    if let (BasicTypeEnum::IntType(actual_int), BasicTypeEnum::IntType(expected_int)) =
        (actual, expected)
    {
        if actual_int.get_bit_width() < expected_int.get_bit_width() {
            return cx
                .builder
                .build_int_s_extend(val.into_int_value(), expected_int, "coerce_ext")
                .expect("build_int_s_extend failed")
                .into();
        }
        if actual_int.get_bit_width() > expected_int.get_bit_width() {
            return cx
                .builder
                .build_int_truncate(val.into_int_value(), expected_int, "coerce_trunc")
                .expect("build_int_truncate failed")
                .into();
        }
    }

    // Pointer to int coercion
    if let (BasicTypeEnum::PointerType(_), BasicTypeEnum::IntType(expected_int)) =
        (actual, expected)
    {
        return cx
            .builder
            .build_ptr_to_int(val.into_pointer_value(), expected_int, "coerce_ptr2int")
            .expect("build_ptr_to_int failed")
            .into();
    }

    // Int to pointer coercion
    if let (BasicTypeEnum::IntType(_), BasicTypeEnum::PointerType(expected_ptr)) =
        (actual, expected)
    {
        return cx
            .builder
            .build_int_to_ptr(val.into_int_value(), expected_ptr, "coerce_int2ptr")
            .expect("build_int_to_ptr failed")
            .into();
    }

    val
}
