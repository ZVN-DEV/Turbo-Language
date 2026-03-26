use cranelift::prelude::*;
use cranelift::prelude::isa::CallConv;
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{DataDescription, FuncId, Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};
use std::collections::HashMap;
use std::path::Path;
use turbo_ast::*;

#[derive(Debug)]
pub struct CodegenError {
    pub message: String,
}

impl std::fmt::Display for CodegenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "codegen error: {}", self.message)
    }
}

impl std::error::Error for CodegenError {}

/// Turbo-level type tag — needed because on ARM64 ptr_type == I64,
/// so Cranelift IR types alone can't distinguish strings from ints.
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
    Enum,
    /// Function pointer: param types and return type
    Fn(Vec<TurboTy>, Box<TurboTy>),
    /// Result type (heap-allocated tagged union): ok_type, err_type
    Result(Box<TurboTy>, Box<TurboTy>),
}

fn turbo_ty_from_type_expr(te: &TypeExpr, enum_variants: &HashMap<String, Vec<String>>) -> TurboTy {
    match te {
        TypeExpr::Named(name) => match name.as_str() {
            "i32" | "i64" | "u32" | "u64" => TurboTy::Int,
            "f32" | "f64" => TurboTy::Float,
            "bool" => TurboTy::Bool,
            "str" => TurboTy::Str,
            _ => {
                if enum_variants.contains_key(name.as_str()) {
                    TurboTy::Enum
                } else {
                    TurboTy::Struct(name.clone())
                }
            }
        },
        TypeExpr::Unit => TurboTy::Unit,
        TypeExpr::Array(inner) => {
            let inner_tty = turbo_ty_from_type_expr(&inner.node, enum_variants);
            TurboTy::Array(Box::new(inner_tty))
        }
        TypeExpr::FnType { params, ret } => {
            let param_tys: Vec<TurboTy> = params.iter()
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
    }
}

/// Compiled value with its Turbo type.
type Typed = (Value, TurboTy);
/// Optional compiled value (None = unit).
type MaybeTyped = Option<Typed>;

// ── Runtime functions linked into JIT ───────────────────────────────

extern "C" fn rt_print_str(s: *const u8) {
    if s.is_null() {
        println!();
        return;
    }
    let cstr = unsafe { std::ffi::CStr::from_ptr(s as *const std::ffi::c_char) };
    if let Ok(string) = cstr.to_str() {
        println!("{}", string);
    }
}

extern "C" fn rt_print_i64(n: i64) {
    println!("{}", n);
}

extern "C" fn rt_print_f64(n: f64) {
    println!("{}", n);
}

extern "C" fn rt_print_bool(b: i8) {
    println!("{}", if b != 0 { "true" } else { "false" });
}

extern "C" fn rt_panic(msg: *const u8) {
    if !msg.is_null() {
        let cstr = unsafe { std::ffi::CStr::from_ptr(msg as *const std::ffi::c_char) };
        if let Ok(s) = cstr.to_str() {
            eprintln!("panic: {}", s);
        }
    } else {
        eprintln!("panic: explicit panic");
    }
    std::process::exit(1);
}

extern "C" fn rt_assert_fail(msg: *const u8) {
    if !msg.is_null() {
        let cstr = unsafe { std::ffi::CStr::from_ptr(msg as *const std::ffi::c_char) };
        if let Ok(s) = cstr.to_str() {
            eprintln!("assertion failed: {}", s);
        }
    } else {
        eprintln!("assertion failed");
    }
    std::process::exit(1);
}

extern "C" fn rt_div_by_zero() {
    eprintln!("runtime error: division by zero");
    std::process::exit(1);
}

extern "C" fn rt_int_overflow() {
    eprintln!("runtime error: integer overflow");
    std::process::exit(1);
}

extern "C" fn rt_array_alloc(len: i64) -> *mut u8 {
    let total_bytes = 8 + (len as usize) * 8; // 8 for length + 8 per element
    let layout = std::alloc::Layout::from_size_align(total_bytes, 8).unwrap();
    let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
    // Store length at the start
    unsafe { *(ptr as *mut i64) = len; }
    ptr
}

extern "C" fn rt_array_get(arr: *const u8, index: i64) -> i64 {
    let len = unsafe { *(arr as *const i64) };
    if index < 0 || index >= len {
        eprintln!("runtime error: array index {} out of bounds (length {})", index, len);
        std::process::exit(1);
    }
    unsafe { *((arr as *const i64).add(1 + index as usize)) }
}

extern "C" fn rt_array_len(arr: *const u8) -> i64 {
    unsafe { *(arr as *const i64) }
}

extern "C" fn rt_str_len(s: *const u8) -> i64 {
    if s.is_null() { return 0; }
    let cstr = unsafe { std::ffi::CStr::from_ptr(s as *const std::ffi::c_char) };
    cstr.to_bytes().len() as i64
}

extern "C" fn rt_str_concat(a: *const u8, b: *const u8) -> *const u8 {
    let a_str = if a.is_null() {
        ""
    } else {
        unsafe { std::ffi::CStr::from_ptr(a as *const std::ffi::c_char) }
            .to_str()
            .unwrap_or("")
    };
    let b_str = if b.is_null() {
        ""
    } else {
        unsafe { std::ffi::CStr::from_ptr(b as *const std::ffi::c_char) }
            .to_str()
            .unwrap_or("")
    };
    let mut result = String::with_capacity(a_str.len() + b_str.len());
    result.push_str(a_str);
    result.push_str(b_str);
    let c_string = std::ffi::CString::new(result).unwrap();
    c_string.into_raw() as *const u8 // leaked — no GC
}

extern "C" fn rt_str_eq(a: *const u8, b: *const u8) -> i8 {
    let a_str = if a.is_null() {
        ""
    } else {
        unsafe { std::ffi::CStr::from_ptr(a as *const std::ffi::c_char) }
            .to_str()
            .unwrap_or("")
    };
    let b_str = if b.is_null() {
        ""
    } else {
        unsafe { std::ffi::CStr::from_ptr(b as *const std::ffi::c_char) }
            .to_str()
            .unwrap_or("")
    };
    if a_str == b_str { 1 } else { 0 }
}

extern "C" fn rt_struct_alloc(num_fields: i64) -> *mut u8 {
    let size = (num_fields as usize) * 8;
    let layout = std::alloc::Layout::from_size_align(size.max(8), 8).unwrap();
    unsafe { std::alloc::alloc_zeroed(layout) }
}

extern "C" fn rt_i64_to_str(n: i64) -> *const u8 {
    let s = format!("{}", n);
    let c = std::ffi::CString::new(s).unwrap();
    c.into_raw() as *const u8
}

extern "C" fn rt_f64_to_str(n: f64) -> *const u8 {
    let s = format!("{}", n);
    let c = std::ffi::CString::new(s).unwrap();
    c.into_raw() as *const u8
}

extern "C" fn rt_bool_to_str(b: i8) -> *const u8 {
    let s = if b != 0 { "true" } else { "false" };
    let c = std::ffi::CString::new(s).unwrap();
    c.into_raw() as *const u8
}

// ── Result type runtime functions ────────────────────────────────────

extern "C" fn rt_result_ok(value: i64) -> *mut u8 {
    let layout = std::alloc::Layout::from_size_align(16, 8).unwrap();
    let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
    unsafe {
        *(ptr as *mut i64) = 0; // tag = ok
        *((ptr as *mut i64).add(1)) = value;
    }
    ptr
}

extern "C" fn rt_result_err(value: i64) -> *mut u8 {
    let layout = std::alloc::Layout::from_size_align(16, 8).unwrap();
    let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
    unsafe {
        *(ptr as *mut i64) = 1; // tag = err
        *((ptr as *mut i64).add(1)) = value;
    }
    ptr
}

extern "C" fn rt_result_tag(result: *const u8) -> i64 {
    unsafe { *(result as *const i64) }
}

extern "C" fn rt_result_value(result: *const u8) -> i64 {
    unsafe { *((result as *const i64).add(1)) }
}

// ── Runtime C source for AOT linking ────────────────────────────────

const RUNTIME_C: &str = include_str!("../runtime/turbo_rt.c");

// ── Codegen context (generic over Module type) ──────────────────────

#[allow(dead_code)]
struct Ctx<'a, M: Module> {
    builder: FunctionBuilder<'a>,
    module: &'a mut M,
    user_fns: &'a HashMap<String, FuncId>,
    fn_ret_types: &'a HashMap<String, TurboTy>,
    rt_fns: &'a HashMap<String, FuncId>,
    vars: HashMap<String, (Variable, types::Type, TurboTy)>,
    next_var: usize,
    data_desc: &'a mut DataDescription,
    string_counter: &'a mut usize,
    ptr_type: types::Type,
    /// Struct field layouts: struct_name -> vec of (field_name, TurboTy)
    struct_fields: &'a HashMap<String, Vec<(String, TurboTy)>>,
    /// Enum variant lists: enum_name -> vec of variant names
    enum_variants: &'a HashMap<String, Vec<String>>,
    /// Map from closure span start offset to (synthetic function name, TurboTy::Fn)
    closure_fns: &'a HashMap<usize, (String, TurboTy)>,
}

impl<'a, M: Module> Ctx<'a, M> {
    fn fresh_var(&mut self, cl_ty: types::Type, turbo_ty: TurboTy) -> Variable {
        let var = Variable::new(self.next_var);
        self.next_var += 1;
        self.builder.declare_var(var, cl_ty);
        let _ = turbo_ty; // used by caller
        var
    }

    fn create_string(&mut self, s: &str) -> Result<Value, CodegenError> {
        if s.contains('\0') {
            return Err(CodegenError {
                message: "string literal contains null byte, which is not supported".to_string(),
            });
        }

        let name = format!(".str.{}", *self.string_counter);
        *self.string_counter += 1;

        let data_id = self.module.declare_data(&name, Linkage::Local, false, false)
            .map_err(|e| CodegenError { message: e.to_string() })?;

        self.data_desc.clear();
        let mut bytes = s.as_bytes().to_vec();
        bytes.push(0);
        self.data_desc.define(bytes.into_boxed_slice());

        self.module.define_data(data_id, self.data_desc)
            .map_err(|e| CodegenError { message: e.to_string() })?;

        let data_ref = self.module.declare_data_in_func(data_id, self.builder.func);
        let ptr = self.builder.ins().global_value(self.ptr_type, data_ref);
        Ok(ptr)
    }

    fn rt_call(&mut self, name: &str, args: &[Value]) {
        let fid = self.rt_fns[name];
        let fref = self.module.declare_func_in_func(fid, self.builder.func);
        self.builder.ins().call(fref, args);
    }
}

// ── Public entry points ─────────────────────────────────────────────

pub fn jit_run(ast_module: &turbo_ast::Module) -> Result<(), CodegenError> {
    let mut flag_builder = settings::builder();
    flag_builder.set("use_colocated_libcalls", "false").unwrap();
    flag_builder.set("is_pic", "false").unwrap();
    flag_builder.set("opt_level", "speed").unwrap();

    let isa_builder = cranelift_native::builder()
        .map_err(|e| CodegenError { message: e.to_string() })?;
    let isa = isa_builder
        .finish(settings::Flags::new(flag_builder))
        .map_err(|e| CodegenError { message: e.to_string() })?;

    let ptr_type = isa.pointer_type();

    let mut jit_builder = JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());

    // Link runtime functions as symbols
    jit_builder.symbol("rt_print_str", rt_print_str as *const u8);
    jit_builder.symbol("rt_print_i64", rt_print_i64 as *const u8);
    jit_builder.symbol("rt_print_f64", rt_print_f64 as *const u8);
    jit_builder.symbol("rt_print_bool", rt_print_bool as *const u8);
    jit_builder.symbol("rt_panic", rt_panic as *const u8);
    jit_builder.symbol("rt_assert_fail", rt_assert_fail as *const u8);
    jit_builder.symbol("rt_div_by_zero", rt_div_by_zero as *const u8);
    jit_builder.symbol("rt_int_overflow", rt_int_overflow as *const u8);
    jit_builder.symbol("rt_str_concat", rt_str_concat as *const u8);
    jit_builder.symbol("rt_str_eq", rt_str_eq as *const u8);
    jit_builder.symbol("rt_array_alloc", rt_array_alloc as *const u8);
    jit_builder.symbol("rt_array_get", rt_array_get as *const u8);
    jit_builder.symbol("rt_array_len", rt_array_len as *const u8);
    jit_builder.symbol("rt_str_len", rt_str_len as *const u8);
    jit_builder.symbol("rt_struct_alloc", rt_struct_alloc as *const u8);
    jit_builder.symbol("rt_i64_to_str", rt_i64_to_str as *const u8);
    jit_builder.symbol("rt_f64_to_str", rt_f64_to_str as *const u8);
    jit_builder.symbol("rt_bool_to_str", rt_bool_to_str as *const u8);
    jit_builder.symbol("rt_result_ok", rt_result_ok as *const u8);
    jit_builder.symbol("rt_result_err", rt_result_err as *const u8);
    jit_builder.symbol("rt_result_tag", rt_result_tag as *const u8);
    jit_builder.symbol("rt_result_value", rt_result_value as *const u8);

    let mut module = JITModule::new(jit_builder);
    let user_fns = compile_module(&mut module, ast_module, ptr_type, Linkage::Local, false)?;

    module.finalize_definitions()
        .map_err(|e| CodegenError { message: e.to_string() })?;

    let main_id = user_fns.get("main")
        .ok_or_else(|| CodegenError { message: "no `main` function found".to_string() })?;
    let main_ptr = module.get_finalized_function(*main_id);
    let main_fn: fn() = unsafe { std::mem::transmute(main_ptr) };
    main_fn();

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
        flag_builder.set("opt_level", "speed").unwrap();
    }

    let isa_builder = cranelift_native::builder()
        .map_err(|e| CodegenError { message: e.to_string() })?;
    let isa = isa_builder
        .finish(settings::Flags::new(flag_builder))
        .map_err(|e| CodegenError { message: e.to_string() })?;

    let ptr_type = isa.pointer_type();

    let obj_builder = ObjectBuilder::new(
        isa,
        "turbo_module",
        cranelift_module::default_libcall_names(),
    ).map_err(|e| CodegenError { message: e.to_string() })?;

    let mut module = ObjectModule::new(obj_builder);
    compile_module(&mut module, ast_module, ptr_type, Linkage::Export, true)?;

    let product = module.finish();
    let obj_bytes = product.emit()
        .map_err(|e| CodegenError { message: format!("failed to emit object: {e}") })?;

    // Write object file and runtime to temp, then link with cc
    let tmp_dir = std::env::temp_dir().join(format!("turbo_aot_{}", std::process::id()));
    std::fs::create_dir_all(&tmp_dir)
        .map_err(|e| CodegenError { message: format!("failed to create temp dir: {e}") })?;

    let obj_path = tmp_dir.join("turbo.o");
    let rt_path = tmp_dir.join("turbo_rt.c");

    std::fs::write(&obj_path, &obj_bytes)
        .map_err(|e| CodegenError { message: format!("failed to write object file: {e}") })?;
    std::fs::write(&rt_path, RUNTIME_C)
        .map_err(|e| CodegenError { message: format!("failed to write runtime: {e}") })?;

    let output = std::process::Command::new("cc")
        .arg(&rt_path)
        .arg(&obj_path)
        .arg("-o")
        .arg(output_path)
        .output()
        .map_err(|e| CodegenError { message: format!("failed to run linker: {e}") })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(CodegenError {
            message: format!("linker failed: {stderr}"),
        });
    }

    // Clean up temp directory and all files within
    let _ = std::fs::remove_dir_all(&tmp_dir);

    Ok(())
}

// ── Shared module compilation ───────────────────────────────────────

/// Convert a TurboTy to a Cranelift types::Type
fn turbo_ty_to_cl_type(tty: &TurboTy, ptr_type: types::Type) -> types::Type {
    match tty {
        TurboTy::Int => types::I64,
        TurboTy::Float => types::F64,
        TurboTy::Bool => types::I8,
        TurboTy::Str => ptr_type,
        TurboTy::Unit => types::I64, // should not happen, but fallback
        TurboTy::Fn(_, _) => ptr_type, // function pointers are pointers
        TurboTy::Array(_) => ptr_type,
        TurboTy::Struct(_) => ptr_type,
        TurboTy::Enum => types::I64,
        TurboTy::Result(_, _) => ptr_type,
    }
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
}

/// Walk an expression tree and collect all closure nodes.
fn extract_closures_from_expr<'a>(
    expr: &'a Spanned<Expr>,
    out: &mut Vec<ExtractedClosure<'a>>,
    counter: &mut usize,
) {
    match &expr.node {
        Expr::Closure { params, return_type, body } => {
            let name = format!("__closure_{}", *counter);
            *counter += 1;
            out.push(ExtractedClosure {
                span_start: expr.span.start,
                name,
                params,
                return_type,
                body,
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
                }
            }
            if let Some(tail) = tail_expr {
                extract_closures_from_expr(tail, out, counter);
            }
        }
        Expr::If { condition, then_branch, else_branch } => {
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
        Expr::OkExpr(value) | Expr::ErrExpr(value) => {
            extract_closures_from_expr(value, out, counter);
        }
        _ => {} // Literals, Ident, Unit, etc. -- no sub-expressions with closures
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
    declare_rt_fn(module, &mut rt_fns, "rt_div_by_zero", &[], None)?;
    declare_rt_fn(module, &mut rt_fns, "rt_int_overflow", &[], None)?;
    declare_rt_fn(module, &mut rt_fns, "rt_str_concat", &[ptr_type, ptr_type], Some(ptr_type))?;
    declare_rt_fn(module, &mut rt_fns, "rt_str_eq", &[ptr_type, ptr_type], Some(types::I8))?;
    declare_rt_fn(module, &mut rt_fns, "rt_array_alloc", &[types::I64], Some(ptr_type))?;
    declare_rt_fn(module, &mut rt_fns, "rt_array_get", &[ptr_type, types::I64], Some(types::I64))?;
    declare_rt_fn(module, &mut rt_fns, "rt_array_len", &[ptr_type], Some(types::I64))?;
    declare_rt_fn(module, &mut rt_fns, "rt_str_len", &[ptr_type], Some(types::I64))?;
    declare_rt_fn(module, &mut rt_fns, "rt_struct_alloc", &[types::I64], Some(ptr_type))?;
    declare_rt_fn(module, &mut rt_fns, "rt_i64_to_str", &[types::I64], Some(ptr_type))?;
    declare_rt_fn(module, &mut rt_fns, "rt_f64_to_str", &[types::F64], Some(ptr_type))?;
    declare_rt_fn(module, &mut rt_fns, "rt_bool_to_str", &[types::I8], Some(ptr_type))?;
    declare_rt_fn(module, &mut rt_fns, "rt_result_ok", &[types::I64], Some(ptr_type))?;
    declare_rt_fn(module, &mut rt_fns, "rt_result_err", &[types::I64], Some(ptr_type))?;
    declare_rt_fn(module, &mut rt_fns, "rt_result_tag", &[ptr_type], Some(types::I64))?;
    declare_rt_fn(module, &mut rt_fns, "rt_result_value", &[ptr_type], Some(types::I64))?;

    // Build enum variants map
    let mut enum_variants: HashMap<String, Vec<String>> = HashMap::new();
    for item in &ast_module.items {
        if let Item::Enum(e) = &item.node {
            enum_variants.insert(e.name.clone(), e.variants.clone());
        }
    }

    // Build struct field layouts from AST
    let mut struct_fields: HashMap<String, Vec<(String, TurboTy)>> = HashMap::new();
    for item in &ast_module.items {
        let Item::Struct(s) = &item.node else { continue };
        let fields: Vec<(String, TurboTy)> = s.fields.iter()
            .map(|f| (f.name.clone(), turbo_ty_from_type_expr(&f.ty.node, &enum_variants)))
            .collect();
        struct_fields.insert(s.name.clone(), fields);
    }

    // Declare all user functions + build return type map
    let mut user_fns: HashMap<String, FuncId> = HashMap::new();
    let mut fn_ret_types: HashMap<String, TurboTy> = HashMap::new();

    for item in &ast_module.items {
        let Item::Function(f) = &item.node else { continue };
        let mut sig = module.make_signature();
        // Use fast calling convention for internal functions (not main)
        // — reduces prologue/epilogue overhead on the hot recursive path
        if f.name != "main" {
            sig.call_conv = CallConv::Fast;
        }
        for param in &f.params {
            sig.params.push(AbiParam::new(resolve_cl_type(&param.ty.node, ptr_type, &enum_variants)?));
        }
        let ret_turbo = if let Some(ret_ty) = &f.return_type {
            let cl = resolve_cl_type(&ret_ty.node, ptr_type, &enum_variants)?;
            sig.returns.push(AbiParam::new(cl));
            turbo_ty_from_type_expr(&ret_ty.node, &enum_variants)
        } else {
            TurboTy::Unit
        };
        let linkage = if f.name == "main" { main_linkage } else { Linkage::Local };
        let sym_name = if f.name == "main" && rename_main { "turbo_main" } else { &f.name };
        let id = module.declare_function(sym_name, linkage, &sig)
            .map_err(|e| CodegenError { message: e.to_string() })?;
        user_fns.insert(f.name.clone(), id);
        fn_ret_types.insert(f.name.clone(), ret_turbo);
    }

    // Declare all methods from impl blocks
    for item in &ast_module.items {
        let Item::Impl(imp) = &item.node else { continue };
        for method_spanned in &imp.methods {
            let method = &method_spanned.node;
            let mangled = format!("{}__{}", imp.type_name, method.name);

            let mut sig = module.make_signature();
            sig.call_conv = CallConv::Fast;

            for param in &method.params {
                if param.name == "self" {
                    sig.params.push(AbiParam::new(ptr_type));
                } else {
                    sig.params.push(AbiParam::new(resolve_cl_type(&param.ty.node, ptr_type, &enum_variants)?));
                }
            }

            let ret_turbo = if let Some(ret_ty) = &method.return_type {
                let cl = resolve_cl_type(&ret_ty.node, ptr_type, &enum_variants)?;
                sig.returns.push(AbiParam::new(cl));
                turbo_ty_from_type_expr(&ret_ty.node, &enum_variants)
            } else {
                TurboTy::Unit
            };

            let id = module.declare_function(&mangled, Linkage::Local, &sig)
                .map_err(|e| CodegenError { message: e.to_string() })?;
            user_fns.insert(mangled.clone(), id);
            fn_ret_types.insert(mangled, ret_turbo);
        }
    }

    // Extract and compile closures
    let extracted_closures = extract_all_closures(ast_module);
    let mut closure_fns_map: HashMap<usize, (String, TurboTy)> = HashMap::new();

    // Declare all closure functions
    for closure in &extracted_closures {
        let mut sig = module.make_signature();
        sig.call_conv = CallConv::Fast;
        let mut param_turbo_tys = Vec::new();
        for param in closure.params.iter() {
            sig.params.push(AbiParam::new(resolve_cl_type(&param.ty.node, ptr_type, &enum_variants)?));
            param_turbo_tys.push(turbo_ty_from_type_expr(&param.ty.node, &enum_variants));
        }
        let ret_turbo = if let Some(ret_ty) = closure.return_type {
            let cl = resolve_cl_type(&ret_ty.node, ptr_type, &enum_variants)?;
            sig.returns.push(AbiParam::new(cl));
            turbo_ty_from_type_expr(&ret_ty.node, &enum_variants)
        } else {
            TurboTy::Unit
        };
        let id = module.declare_function(&closure.name, Linkage::Local, &sig)
            .map_err(|e| CodegenError { message: e.to_string() })?;
        user_fns.insert(closure.name.clone(), id);
        fn_ret_types.insert(closure.name.clone(), ret_turbo.clone());
        closure_fns_map.insert(
            closure.span_start,
            (closure.name.clone(), TurboTy::Fn(param_turbo_tys, Box::new(ret_turbo))),
        );
    }

    // Define all user functions (and closures)
    let mut cl_ctx = module.make_context();
    let mut data_desc = DataDescription::new();
    let mut string_counter: usize = 0;

    // Compile closure function bodies first
    for closure in &extracted_closures {
        let func_id = user_fns[&closure.name];

        cl_ctx.func.signature = module.make_signature();
        cl_ctx.func.signature.call_conv = CallConv::Fast;
        for param in closure.params.iter() {
            cl_ctx.func.signature.params.push(AbiParam::new(resolve_cl_type(&param.ty.node, ptr_type, &enum_variants)?));
        }
        if let Some(ret_ty) = closure.return_type {
            cl_ctx.func.signature.returns.push(AbiParam::new(resolve_cl_type(&ret_ty.node, ptr_type, &enum_variants)?));
        }

        let mut fn_ctx = FunctionBuilderContext::new();
        {
            let builder = FunctionBuilder::new(&mut cl_ctx.func, &mut fn_ctx);
            let mut cx = Ctx {
                builder,
                module,
                user_fns: &user_fns,
                fn_ret_types: &fn_ret_types,
                rt_fns: &rt_fns,
                vars: HashMap::new(),
                next_var: 0,
                data_desc: &mut data_desc,
                string_counter: &mut string_counter,
                ptr_type,
                struct_fields: &struct_fields,
                enum_variants: &enum_variants,
                closure_fns: &closure_fns_map,
            };

            let entry = cx.builder.create_block();
            cx.builder.append_block_params_for_function_params(entry);
            cx.builder.switch_to_block(entry);
            cx.builder.seal_block(entry);

            // Define parameters as variables
            for (i, param) in closure.params.iter().enumerate() {
                let cl_ty = resolve_cl_type(&param.ty.node, ptr_type, &enum_variants)?;
                let turbo_ty = turbo_ty_from_type_expr(&param.ty.node, &enum_variants);
                let var = cx.fresh_var(cl_ty, turbo_ty.clone());
                let val = cx.builder.block_params(entry)[i];
                cx.builder.def_var(var, val);
                cx.vars.insert(param.name.clone(), (var, cl_ty, turbo_ty));
            }

            let result = compile_expr(&mut cx, closure.body)?;

            if !cx.builder.is_unreachable() {
                if closure.return_type.is_some() {
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

        module.define_function(func_id, &mut cl_ctx)
            .map_err(|e| CodegenError { message: e.to_string() })?;
        module.clear_context(&mut cl_ctx);
    }

    for item in &ast_module.items {
        let Item::Function(f) = &item.node else { continue };
        let func_id = user_fns[&f.name];

        cl_ctx.func.signature = module.make_signature();
        if f.name != "main" {
            cl_ctx.func.signature.call_conv = CallConv::Fast;
        }
        for param in &f.params {
            cl_ctx.func.signature.params.push(AbiParam::new(resolve_cl_type(&param.ty.node, ptr_type, &enum_variants)?));
        }
        if let Some(ret_ty) = &f.return_type {
            cl_ctx.func.signature.returns.push(AbiParam::new(resolve_cl_type(&ret_ty.node, ptr_type, &enum_variants)?));
        }

        let mut fn_ctx = FunctionBuilderContext::new();
        {
            let builder = FunctionBuilder::new(&mut cl_ctx.func, &mut fn_ctx);
            let mut cx = Ctx {
                builder,
                module,
                user_fns: &user_fns,
                fn_ret_types: &fn_ret_types,
                rt_fns: &rt_fns,
                vars: HashMap::new(),
                next_var: 0,
                data_desc: &mut data_desc,
                string_counter: &mut string_counter,
                ptr_type,
                struct_fields: &struct_fields,
                enum_variants: &enum_variants,
                closure_fns: &closure_fns_map,
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
                let cl_ty = resolve_cl_type(&param.ty.node, ptr_type, &enum_variants)?;
                let turbo_ty = turbo_ty_from_type_expr(&param.ty.node, &enum_variants);
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

        module.define_function(func_id, &mut cl_ctx)
            .map_err(|e| CodegenError { message: e.to_string() })?;
        module.clear_context(&mut cl_ctx);
    }

    // Define all methods from impl blocks
    for item in &ast_module.items {
        let Item::Impl(imp) = &item.node else { continue };
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
                    cl_ctx.func.signature.params.push(AbiParam::new(resolve_cl_type(&param.ty.node, ptr_type, &enum_variants)?));
                }
            }
            if let Some(ret_ty) = &method.return_type {
                cl_ctx.func.signature.returns.push(AbiParam::new(resolve_cl_type(&ret_ty.node, ptr_type, &enum_variants)?));
            }

            let mut fn_ctx = FunctionBuilderContext::new();
            {
                let builder = FunctionBuilder::new(&mut cl_ctx.func, &mut fn_ctx);
                let mut cx = Ctx {
                    builder,
                    module,
                    user_fns: &user_fns,
                    fn_ret_types: &fn_ret_types,
                    rt_fns: &rt_fns,
                    vars: HashMap::new(),
                    next_var: 0,
                    data_desc: &mut data_desc,
                    string_counter: &mut string_counter,
                    ptr_type,
                    struct_fields: &struct_fields,
                    enum_variants: &enum_variants,
                    closure_fns: &closure_fns_map,
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
                        let cl_ty = resolve_cl_type(&param.ty.node, ptr_type, &enum_variants)?;
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

            module.define_function(func_id, &mut cl_ctx)
                .map_err(|e| CodegenError { message: e.to_string() })?;
            module.clear_context(&mut cl_ctx);
        }
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
    let id = module.declare_function(name, Linkage::Import, &sig)
        .map_err(|e| CodegenError { message: e.to_string() })?;
    rt_fns.insert(name.to_string(), id);
    Ok(())
}

fn resolve_cl_type(ty: &TypeExpr, ptr_type: types::Type, enum_variants: &HashMap<String, Vec<String>>) -> Result<types::Type, CodegenError> {
    match ty {
        TypeExpr::Named(name) => match name.as_str() {
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
                    Ok(types::I64) // enums are represented as i64 tags
                } else {
                    Ok(ptr_type) // Struct types are represented as pointers at runtime
                }
            }
        },
        TypeExpr::Unit => Err(CodegenError { message: "unit type has no runtime representation".to_string() }),
        TypeExpr::Array(_) => Ok(ptr_type), // Arrays are represented as pointers at runtime
        TypeExpr::FnType { .. } => Ok(ptr_type), // Function pointers are pointers
        TypeExpr::Result { .. } => Ok(ptr_type), // Result types are heap-allocated tagged unions
    }
}

// ── Expression compilation ──────────────────────────────────────────

fn compile_expr<M: Module>(cx: &mut Ctx<'_, M>, expr: &Spanned<Expr>) -> Result<MaybeTyped, CodegenError> {
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
            let (var, _cl_ty, turbo_ty) = cx.vars.get(name)
                .ok_or_else(|| CodegenError { message: format!("undefined variable: {name}") })?;
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

            let result = compile_binop(cx, lhs, *op, rhs)?;

            // Comparison/logical ops produce Bool, arithmetic preserves input type
            let result_tty = match op {
                BinOp::Eq | BinOp::NotEq | BinOp::Less | BinOp::LessEq
                | BinOp::Greater | BinOp::GreaterEq | BinOp::And | BinOp::Or => TurboTy::Bool,
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

        Expr::If { condition, then_branch, else_branch } => {
            compile_if(cx, condition, then_branch, else_branch.as_deref())
        }

        Expr::Block { stmts, tail_expr } => {
            let saved_vars = cx.vars.clone();

            for stmt in stmts {
                compile_stmt(cx, stmt)?;
            }
            let result = if let Some(tail) = tail_expr {
                compile_expr(cx, tail)
            } else {
                Ok(None)
            };

            // Restore variable scope: this ensures inner `let` bindings
            // that shadow outer names don't leak out of the block.
            // Actual SSA values in Cranelift variables are unaffected —
            // only the name-to-Variable mapping is restored.
            cx.vars = saved_vars;

            result
        }

        Expr::Assign { target, value } => {
            let (val, tty) = compile_expr(cx, value)?.unwrap();
            let (var, _, _) = cx.vars.get(target)
                .ok_or_else(|| CodegenError { message: format!("undefined variable: {target}") })?;
            let var = *var;
            cx.builder.def_var(var, val);
            // Update the turbo type in case it changed (shouldn't in Phase 1, but safe)
            if let Some(entry) = cx.vars.get_mut(target) {
                entry.2 = tty;
            }
            Ok(None)
        }

        Expr::CompoundAssign { target, op, value } => {
            let (rhs, _) = compile_expr(cx, value)?.unwrap();
            let (var, _, _) = cx.vars.get(target)
                .ok_or_else(|| CodegenError { message: format!("undefined variable: {target}") })?;
            let var = *var;
            let lhs = cx.builder.use_var(var);
            let result = compile_binop(cx, lhs, *op, rhs)?;
            cx.builder.def_var(var, result);
            Ok(None)
        }

        Expr::While { condition, body } => compile_while(cx, condition, body),

        Expr::ForIn { var_name, iterable, body } => compile_for_in(cx, var_name, iterable, body),

        Expr::Range { .. } => {
            Err(CodegenError { message: "range expressions can only be used in for-in loops".to_string() })
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
            let struct_layout = cx.struct_fields.get(name)
                .ok_or_else(|| CodegenError { message: format!("undefined struct: {name}") })?
                .clone();

            let num_fields = struct_layout.len() as i64;
            let num_fields_val = cx.builder.ins().iconst(types::I64, num_fields);

            // Call rt_struct_alloc to allocate memory
            let alloc_fid = cx.rt_fns["rt_struct_alloc"];
            let alloc_fref = cx.module.declare_func_in_func(alloc_fid, cx.builder.func);
            let call = cx.builder.ins().call(alloc_fref, &[num_fields_val]);
            let ptr = cx.builder.inst_results(call)[0];

            // Store each field at its offset
            for (field_name, field_value) in fields {
                let field_index = struct_layout.iter()
                    .position(|(n, _)| n == field_name)
                    .ok_or_else(|| CodegenError { message: format!("struct `{name}` has no field `{field_name}`") })?;

                let (val, _tty) = compile_expr(cx, field_value)?.unwrap();
                let offset = (field_index * 8) as i32;

                // Widen smaller types to 64-bit for uniform storage
                let val_ty = cx.builder.func.dfg.value_type(val);
                let val = if val_ty.bits() < 64 && val_ty.is_int() {
                    cx.builder.ins().sextend(types::I64, val)
                } else if val_ty.is_float() && val_ty.bits() == 64 {
                    cx.builder.ins().bitcast(types::I64, MemFlags::new(), val)
                } else if val_ty.is_float() && val_ty.bits() == 32 {
                    let extended = cx.builder.ins().fpromote(types::F64, val);
                    cx.builder.ins().bitcast(types::I64, MemFlags::new(), extended)
                } else {
                    val
                };

                cx.builder.ins().store(MemFlags::new(), val, ptr, offset);
            }

            Ok(Some((ptr, TurboTy::Struct(name.clone()))))
        }

        Expr::FieldAccess { object, field } => {
            // Check if this is actually an enum variant access: EnumName.VariantName
            if let Expr::Ident(ref name) = object.node {
                if let Some(variants) = cx.enum_variants.get(name.as_str()) {
                    let index = variants.iter().position(|v| v == field)
                        .ok_or_else(|| CodegenError { message: format!("enum `{name}` has no variant `{field}`") })?;
                    let val = cx.builder.ins().iconst(types::I64, index as i64);
                    return Ok(Some((val, TurboTy::Enum)));
                }
            }

            let (obj_ptr, obj_tty) = compile_expr(cx, object)?.unwrap();

            let struct_name = match &obj_tty {
                TurboTy::Struct(name) => name.clone(),
                _ => return Err(CodegenError { message: format!("field access on non-struct type") }),
            };

            let struct_layout = cx.struct_fields.get(&struct_name)
                .ok_or_else(|| CodegenError { message: format!("undefined struct: {struct_name}") })?
                .clone();

            let field_index = struct_layout.iter()
                .position(|(n, _)| n == field)
                .ok_or_else(|| CodegenError { message: format!("struct `{struct_name}` has no field `{field}`") })?;

            let field_tty = struct_layout[field_index].1.clone();
            let offset = (field_index * 8) as i32;

            // Load from the struct pointer
            let raw_val = cx.builder.ins().load(types::I64, MemFlags::new(), obj_ptr, offset);

            // Convert back to the appropriate type
            let (val, tty) = match &field_tty {
                TurboTy::Int => (raw_val, TurboTy::Int),
                TurboTy::Bool => {
                    let truncated = cx.builder.ins().ireduce(types::I8, raw_val);
                    (truncated, TurboTy::Bool)
                }
                TurboTy::Float => {
                    let f = cx.builder.ins().bitcast(types::F64, MemFlags::new(), raw_val);
                    (f, TurboTy::Float)
                }
                TurboTy::Str => (raw_val, TurboTy::Str),
                TurboTy::Struct(name) => (raw_val, TurboTy::Struct(name.clone())),
                _ => (raw_val, field_tty),
            };

            Ok(Some((val, tty)))
        }

        Expr::EnumVariant { enum_name, variant } => {
            let variants = cx.enum_variants.get(enum_name.as_str())
                .ok_or_else(|| CodegenError { message: format!("undefined enum: {enum_name}") })?;
            let index = variants.iter().position(|v| v == variant)
                .ok_or_else(|| CodegenError { message: format!("enum `{enum_name}` has no variant `{variant}`") })?;
            let val = cx.builder.ins().iconst(types::I64, index as i64);
            Ok(Some((val, TurboTy::Enum)))
        }

        Expr::Match { subject, arms } => compile_match(cx, subject, arms),

        Expr::Interpolation(parts) => compile_interpolation(cx, parts),

        Expr::Closure { .. } => {
            // Look up the pre-compiled closure function by span start
            let span_start = expr.span.start;
            let (closure_name, closure_ty) = cx.closure_fns.get(&span_start)
                .ok_or_else(|| CodegenError { message: "internal error: closure not found in pre-compiled map".to_string() })?;
            let closure_ty = closure_ty.clone();
            let func_id = *cx.user_fns.get(closure_name.as_str())
                .ok_or_else(|| CodegenError { message: format!("internal error: closure function {} not found", closure_name) })?;
            let func_ref = cx.module.declare_func_in_func(func_id, cx.builder.func);
            let ptr = cx.builder.ins().func_addr(cx.ptr_type, func_ref);
            Ok(Some((ptr, closure_ty)))
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
            Ok(Some((ptr, TurboTy::Result(Box::new(TurboTy::Int), Box::new(TurboTy::Int)))))
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
            Ok(Some((ptr, TurboTy::Result(Box::new(TurboTy::Int), Box::new(TurboTy::Int)))))
        }
    }
}

// ── Statement compilation ───────────────────────────────────────────

fn compile_stmt<M: Module>(cx: &mut Ctx<'_, M>, stmt: &Spanned<Stmt>) -> Result<(), CodegenError> {
    match &stmt.node {
        Stmt::Let { name, value, .. } => {
            let result = compile_expr(cx, value)?;
            let (cl_ty, turbo_ty, val) = if let Some((v, tty)) = result {
                (cx.builder.func.dfg.value_type(v), tty, Some(v))
            } else {
                (types::I64, TurboTy::Unit, None)
            };
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
            _ => return Err(CodegenError { message: format!("unsupported float op: {op:?}") }),
        };
        Ok(result)
    } else {
        // Widen mismatched integer widths
        let rhs_ty = cx.builder.func.dfg.value_type(rhs);
        let (lhs, rhs) = if lhs_ty.bits() != rhs_ty.bits() {
            let target = if lhs_ty.bits() > rhs_ty.bits() { lhs_ty } else { rhs_ty };
            let lhs = if lhs_ty.bits() < target.bits() {
                cx.builder.ins().sextend(target, lhs)
            } else { lhs };
            let rhs = if rhs_ty.bits() < target.bits() {
                cx.builder.ins().sextend(target, rhs)
            } else { rhs };
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
            BinOp::LessEq => cx.builder.ins().icmp(IntCC::SignedLessThanOrEqual, lhs, rhs),
            BinOp::Greater => cx.builder.ins().icmp(IntCC::SignedGreaterThan, lhs, rhs),
            BinOp::GreaterEq => cx.builder.ins().icmp(IntCC::SignedGreaterThanOrEqual, lhs, rhs),
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

    cx.builder.ins().brif(is_zero, trap_block, &[], ok_block, &[]);

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

    cx.builder.ins().brif(is_overflow, trap_block, &[], ok_block, &[]);

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

    let lhs_ty = cx.builder.func.dfg.value_type(lhs);
    let lhs_bool = {
        let zero = cx.builder.ins().iconst(lhs_ty, 0);
        cx.builder.ins().icmp(IntCC::NotEqual, lhs, zero)
    };

    let eval_rhs_block = cx.builder.create_block();
    let merge_block = cx.builder.create_block();
    cx.builder.append_block_param(merge_block, types::I8);

    match op {
        BinOp::And => {
            let false_val = cx.builder.ins().iconst(types::I8, 0);
            cx.builder.ins().brif(lhs_bool, eval_rhs_block, &[], merge_block, &[false_val]);
        }
        BinOp::Or => {
            let true_val = cx.builder.ins().iconst(types::I8, 1);
            cx.builder.ins().brif(lhs_bool, merge_block, &[true_val], eval_rhs_block, &[]);
        }
        _ => unreachable!(),
    }

    cx.builder.switch_to_block(eval_rhs_block);
    cx.builder.seal_block(eval_rhs_block);
    let (rhs, _) = compile_expr(cx, right)?.unwrap();

    let rhs_ty = cx.builder.func.dfg.value_type(rhs);
    let rhs_as_i8 = if rhs_ty == types::I8 {
        rhs
    } else {
        let zero = cx.builder.ins().iconst(rhs_ty, 0);
        cx.builder.ins().icmp(IntCC::NotEqual, rhs, zero)
    };

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
    if let Expr::FieldAccess { ref object, ref field } = callee.node {
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
                let ret_tty = cx.fn_ret_types.get(&mangled).cloned().unwrap_or(TurboTy::Unit);
                if results.is_empty() {
                    return Ok(None);
                } else {
                    return Ok(Some((results[0], ret_tty)));
                }
            }
        }
        return Err(CodegenError { message: format!("no method `{field}` found") });
    }

    let Expr::Ident(name) = &callee.node else {
        return Err(CodegenError { message: "indirect function calls not yet supported".to_string() });
    };

    match name.as_str() {
        "print" => compile_print(cx, args),
        "panic" => compile_panic(cx, args),
        "assert" => compile_assert(cx, args),
        "len" => compile_len(cx, args),
        "abs" => compile_abs(cx, args),
        "min" => compile_min(cx, args),
        "max" => compile_max(cx, args),
        "to_str" => compile_to_str_builtin(cx, args),
        _ => {
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
                        let ret_tty = cx.fn_ret_types.get(&mangled).cloned().unwrap_or(TurboTy::Unit);
                        if results.is_empty() {
                            return Ok(None);
                        } else {
                            return Ok(Some((results[0], ret_tty)));
                        }
                    }
                }
            }

            // Check if the callee is a variable with a function pointer type (closure)
            if let Some((var, _cl_ty, turbo_ty)) = cx.vars.get(name).cloned() {
                if let TurboTy::Fn(ref param_tys, ref ret_ty) = turbo_ty {
                    // Indirect call through function pointer
                    let fn_ptr = cx.builder.use_var(var);

                    // Build the Cranelift signature for the call
                    let mut sig = cx.module.make_signature();
                    sig.call_conv = CallConv::Fast;
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

                    let mut arg_values = Vec::new();
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
            }

            let func_id = *cx.user_fns.get(name.as_str())
                .ok_or_else(|| CodegenError { message: format!("undefined function: {name}") })?;

            let ret_tty = cx.fn_ret_types.get(name.as_str()).cloned().unwrap_or(TurboTy::Unit);

            let func_ref = cx.module.declare_func_in_func(func_id, cx.builder.func);
            let sig = cx.builder.func.dfg.ext_funcs[func_ref].signature;
            let param_types: Vec<types::Type> = cx.builder.func.dfg.signatures[sig]
                .params.iter().map(|p| p.value_type).collect();
            let mut arg_values = Vec::new();
            for (i, arg) in args.iter().enumerate() {
                if let Some((val, _)) = compile_expr(cx, arg)? {
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
                }
            }

            let call = cx.builder.ins().call(func_ref, &arg_values);
            let results = cx.builder.inst_results(call);
            if results.is_empty() {
                Ok(None)
            } else {
                Ok(Some((results[0], ret_tty)))
            }
        }
    }
}

fn compile_print<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>]) -> Result<MaybeTyped, CodegenError> {
    if args.is_empty() {
        let ptr = cx.create_string("")?;
        cx.rt_call("rt_print_str", &[ptr]);
        return Ok(None);
    }

    let result = compile_expr(cx, &args[0])?;

    if let Some((v, tty)) = result {
        match tty {
            TurboTy::Str => cx.rt_call("rt_print_str", &[v]),
            TurboTy::Float => cx.rt_call("rt_print_f64", &[v]),
            TurboTy::Bool => cx.rt_call("rt_print_bool", &[v]),
            TurboTy::Int => {
                let ty = cx.builder.func.dfg.value_type(v);
                let v = if ty.bits() < 64 {
                    cx.builder.ins().sextend(types::I64, v)
                } else { v };
                cx.rt_call("rt_print_i64", &[v]);
            }
            TurboTy::Unit => {
                let ptr = cx.create_string("()")?;
                cx.rt_call("rt_print_str", &[ptr]);
            }
            TurboTy::Enum => {
                // Print enum value as its integer tag
                let v = if cx.builder.func.dfg.value_type(v).bits() < 64 {
                    cx.builder.ins().sextend(types::I64, v)
                } else { v };
                cx.rt_call("rt_print_i64", &[v]);
            }
            TurboTy::Array(_) => {
                let ptr = cx.create_string("[array]")?;
                cx.rt_call("rt_print_str", &[ptr]);
            }
            TurboTy::Struct(name) => {
                let ptr = cx.create_string(&format!("[struct {}]", name))?;
                cx.rt_call("rt_print_str", &[ptr]);
            }
            TurboTy::Fn(_, _) => {
                let ptr = cx.create_string("[function]")?;
                cx.rt_call("rt_print_str", &[ptr]);
            }
            TurboTy::Result(_, _) => {
                let ptr = cx.create_string("[result]")?;
                cx.rt_call("rt_print_str", &[ptr]);
            }
        }
    } else {
        let ptr = cx.create_string("()")?;
        cx.rt_call("rt_print_str", &[ptr]);
    }

    Ok(None)
}

fn compile_panic<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>]) -> Result<MaybeTyped, CodegenError> {
    let msg = if !args.is_empty() {
        compile_expr(cx, &args[0])?.unwrap().0
    } else {
        cx.create_string("explicit panic")?
    };

    cx.rt_call("rt_panic", &[msg]);
    cx.builder.ins().trap(TrapCode::unwrap_user(1));

    let new_block = cx.builder.create_block();
    cx.builder.switch_to_block(new_block);
    cx.builder.seal_block(new_block);

    Ok(None)
}

fn compile_assert<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>]) -> Result<MaybeTyped, CodegenError> {
    if args.is_empty() {
        return Err(CodegenError { message: "assert() requires at least one argument".to_string() });
    }

    let (cond, _) = compile_expr(cx, &args[0])?.unwrap();

    let cond_ty = cx.builder.func.dfg.value_type(cond);
    let cond_bool = {
        let zero = cx.builder.ins().iconst(cond_ty, 0);
        cx.builder.ins().icmp(IntCC::NotEqual, cond, zero)
    };

    let fail_block = cx.builder.create_block();
    let ok_block = cx.builder.create_block();

    cx.builder.ins().brif(cond_bool, ok_block, &[], fail_block, &[]);

    cx.builder.switch_to_block(fail_block);
    cx.builder.seal_block(fail_block);

    let msg = if args.len() > 1 {
        compile_expr(cx, &args[1])?.unwrap().0
    } else {
        cx.create_string("assertion failed")?
    };

    cx.rt_call("rt_assert_fail", &[msg]);
    cx.builder.ins().trap(TrapCode::unwrap_user(1));

    cx.builder.switch_to_block(ok_block);
    cx.builder.seal_block(ok_block);

    Ok(None)
}

fn compile_len<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>]) -> Result<MaybeTyped, CodegenError> {
    if args.is_empty() {
        return Err(CodegenError { message: "len() requires exactly 1 argument".to_string() });
    }
    let (val, tty) = compile_expr(cx, &args[0])?.unwrap();
    if tty == TurboTy::Str {
        let len_fid = cx.rt_fns["rt_str_len"];
        let len_ref = cx.module.declare_func_in_func(len_fid, cx.builder.func);
        let call = cx.builder.ins().call(len_ref, &[val]);
        let result = cx.builder.inst_results(call)[0];
        Ok(Some((result, TurboTy::Int)))
    } else {
        let len_fid = cx.rt_fns["rt_array_len"];
        let len_ref = cx.module.declare_func_in_func(len_fid, cx.builder.func);
        let call = cx.builder.ins().call(len_ref, &[val]);
        let result = cx.builder.inst_results(call)[0];
        Ok(Some((result, TurboTy::Int)))
    }
}

// ── abs/min/max/to_str builtins ─────────────────────────────────────

fn compile_abs<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>]) -> Result<MaybeTyped, CodegenError> {
    let (val, _) = compile_expr(cx, &args[0])?.unwrap();
    let zero = cx.builder.ins().iconst(types::I64, 0);
    let is_neg = cx.builder.ins().icmp(IntCC::SignedLessThan, val, zero);
    let neg_val = cx.builder.ins().ineg(val);
    let result = cx.builder.ins().select(is_neg, neg_val, val);
    Ok(Some((result, TurboTy::Int)))
}

fn compile_min<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>]) -> Result<MaybeTyped, CodegenError> {
    let (a, _) = compile_expr(cx, &args[0])?.unwrap();
    let (b, _) = compile_expr(cx, &args[1])?.unwrap();
    let cmp = cx.builder.ins().icmp(IntCC::SignedLessThan, a, b);
    let result = cx.builder.ins().select(cmp, a, b);
    Ok(Some((result, TurboTy::Int)))
}

fn compile_max<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>]) -> Result<MaybeTyped, CodegenError> {
    let (a, _) = compile_expr(cx, &args[0])?.unwrap();
    let (b, _) = compile_expr(cx, &args[1])?.unwrap();
    let cmp = cx.builder.ins().icmp(IntCC::SignedGreaterThan, a, b);
    let result = cx.builder.ins().select(cmp, a, b);
    Ok(Some((result, TurboTy::Int)))
}

fn compile_to_str_builtin<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>]) -> Result<MaybeTyped, CodegenError> {
    let (val, tty) = compile_expr(cx, &args[0])?.unwrap();
    let str_val = convert_to_str(cx, val, &tty)?;
    Ok(Some((str_val, TurboTy::Str)))
}

// ── If expression ───────────────────────────────────────────────────

fn compile_if<M: Module>(
    cx: &mut Ctx<'_, M>,
    condition: &Spanned<Expr>,
    then_branch: &Spanned<Expr>,
    else_branch: Option<&Spanned<Expr>>,
) -> Result<MaybeTyped, CodegenError> {
    let (cond, _) = compile_expr(cx, condition)?.unwrap();

    let cond_ty = cx.builder.func.dfg.value_type(cond);
    let cond_bool = {
        let zero = cx.builder.ins().iconst(cond_ty, 0);
        cx.builder.ins().icmp(IntCC::NotEqual, cond, zero)
    };

    let then_block = cx.builder.create_block();
    let else_block = cx.builder.create_block();
    let merge_block = cx.builder.create_block();

    cx.builder.ins().brif(cond_bool, then_block, &[], else_block, &[]);

    // Then
    cx.builder.switch_to_block(then_block);
    cx.builder.seal_block(then_block);
    let then_result = compile_expr(cx, then_branch)?;
    let then_needs_jump = !cx.builder.is_unreachable();
    if then_needs_jump {
        if let Some((v, _)) = then_result {
            cx.builder.ins().jump(merge_block, &[v]);
        } else {
            cx.builder.ins().jump(merge_block, &[]);
        }
    }

    // Else
    cx.builder.switch_to_block(else_block);
    cx.builder.seal_block(else_block);
    let else_result = if let Some(else_expr) = else_branch {
        compile_expr(cx, else_expr)?
    } else {
        None
    };
    let else_needs_jump = !cx.builder.is_unreachable();
    if else_needs_jump {
        if let Some((v, _)) = else_result {
            cx.builder.ins().jump(merge_block, &[v]);
        } else {
            cx.builder.ins().jump(merge_block, &[]);
        }
    }

    // Merge
    cx.builder.switch_to_block(merge_block);
    cx.builder.seal_block(merge_block);

    if let (Some((then_val, then_tty)), Some(_)) = (then_result, else_result) {
        let ty = cx.builder.func.dfg.value_type(then_val);
        cx.builder.append_block_param(merge_block, ty);
        let param = cx.builder.block_params(merge_block)[0];
        Ok(Some((param, then_tty)))
    } else {
        Ok(None)
    }
}

// ── String operations ───────────────────────────────────────────────

fn compile_str_concat<M: Module>(
    cx: &mut Ctx<'_, M>,
    lhs: Value,
    rhs: Value,
) -> Result<MaybeTyped, CodegenError> {
    let fid = cx.rt_fns["rt_str_concat"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[lhs, rhs]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Str)))
}

fn compile_str_compare<M: Module>(
    cx: &mut Ctx<'_, M>,
    lhs: Value,
    rhs: Value,
    op: BinOp,
) -> Result<MaybeTyped, CodegenError> {
    let fid = cx.rt_fns["rt_str_eq"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[lhs, rhs]);
    let result = cx.builder.inst_results(call)[0];
    let result = if op == BinOp::NotEq {
        let one = cx.builder.ins().iconst(types::I8, 1);
        cx.builder.ins().bxor(result, one)
    } else {
        result
    };
    Ok(Some((result, TurboTy::Bool)))
}

// ── String interpolation ────────────────────────────────────────────

fn compile_interpolation<M: Module>(
    cx: &mut Ctx<'_, M>,
    parts: &[turbo_ast::InterpolPart],
) -> Result<MaybeTyped, CodegenError> {
    let mut result: Option<Value> = None;

    for part in parts {
        let part_str = match part {
            turbo_ast::InterpolPart::Lit(s) => {
                cx.create_string(s)?
            }
            turbo_ast::InterpolPart::Expr(expr) => {
                let (val, tty) = compile_expr(cx, expr)?.unwrap();
                convert_to_str(cx, val, &tty)?
            }
        };

        result = Some(match result {
            None => part_str,
            Some(acc) => {
                let fid = cx.rt_fns["rt_str_concat"];
                let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
                let call = cx.builder.ins().call(fref, &[acc, part_str]);
                cx.builder.inst_results(call)[0]
            }
        });
    }

    match result {
        Some(val) => Ok(Some((val, TurboTy::Str))),
        None => {
            let ptr = cx.create_string("")?;
            Ok(Some((ptr, TurboTy::Str)))
        }
    }
}

fn convert_to_str<M: Module>(
    cx: &mut Ctx<'_, M>,
    val: Value,
    tty: &TurboTy,
) -> Result<Value, CodegenError> {
    match tty {
        TurboTy::Str => Ok(val),
        TurboTy::Int => {
            let ty = cx.builder.func.dfg.value_type(val);
            let val = if ty.bits() < 64 {
                cx.builder.ins().sextend(types::I64, val)
            } else {
                val
            };
            let fid = cx.rt_fns["rt_i64_to_str"];
            let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
            let call = cx.builder.ins().call(fref, &[val]);
            Ok(cx.builder.inst_results(call)[0])
        }
        TurboTy::Float => {
            let fid = cx.rt_fns["rt_f64_to_str"];
            let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
            let call = cx.builder.ins().call(fref, &[val]);
            Ok(cx.builder.inst_results(call)[0])
        }
        TurboTy::Bool => {
            let fid = cx.rt_fns["rt_bool_to_str"];
            let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
            let call = cx.builder.ins().call(fref, &[val]);
            Ok(cx.builder.inst_results(call)[0])
        }
        TurboTy::Unit => {
            cx.create_string("()")
        }
        TurboTy::Enum => {
            let val = if cx.builder.func.dfg.value_type(val).bits() < 64 {
                cx.builder.ins().sextend(types::I64, val)
            } else {
                val
            };
            let fid = cx.rt_fns["rt_i64_to_str"];
            let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
            let call = cx.builder.ins().call(fref, &[val]);
            Ok(cx.builder.inst_results(call)[0])
        }
        TurboTy::Array(_) => {
            cx.create_string("[array]")
        }
        TurboTy::Struct(name) => {
            cx.create_string(&format!("[struct {}]", name))
        }
        TurboTy::Fn(_, _) => {
            cx.create_string("[function]")
        }
        TurboTy::Result(_, _) => {
            cx.create_string("[result]")
        }
    }
}

// ── While loop ──────────────────────────────────────────────────────

fn compile_while<M: Module>(
    cx: &mut Ctx<'_, M>,
    condition: &Spanned<Expr>,
    body: &Spanned<Expr>,
) -> Result<MaybeTyped, CodegenError> {
    let header_block = cx.builder.create_block();
    let body_block = cx.builder.create_block();
    let exit_block = cx.builder.create_block();

    cx.builder.ins().jump(header_block, &[]);

    // Header: evaluate condition
    cx.builder.switch_to_block(header_block);
    let (cond, _) = compile_expr(cx, condition)?.unwrap();

    let cond_ty = cx.builder.func.dfg.value_type(cond);
    let cond_bool = {
        let zero = cx.builder.ins().iconst(cond_ty, 0);
        cx.builder.ins().icmp(IntCC::NotEqual, cond, zero)
    };

    cx.builder.ins().brif(cond_bool, body_block, &[], exit_block, &[]);

    // Body
    cx.builder.switch_to_block(body_block);
    cx.builder.seal_block(body_block);
    compile_expr(cx, body)?;

    if !cx.builder.is_unreachable() {
        cx.builder.ins().jump(header_block, &[]);
    }

    cx.builder.seal_block(header_block);

    cx.builder.switch_to_block(exit_block);
    cx.builder.seal_block(exit_block);

    Ok(None)
}

// ── For-in loop ─────────────────────────────────────────────────────

fn compile_for_in<M: Module>(
    cx: &mut Ctx<'_, M>,
    var_name: &str,
    iterable: &Spanned<Expr>,
    body: &Spanned<Expr>,
) -> Result<MaybeTyped, CodegenError> {
    match &iterable.node {
        Expr::Range { start, end } => compile_for_in_range(cx, var_name, start, end, body),
        _ => compile_for_in_array(cx, var_name, iterable, body),
    }
}

fn compile_for_in_range<M: Module>(
    cx: &mut Ctx<'_, M>,
    var_name: &str,
    start: &Spanned<Expr>,
    end: &Spanned<Expr>,
    body: &Spanned<Expr>,
) -> Result<MaybeTyped, CodegenError> {
    let (range_start, _) = compile_expr(cx, start)?.unwrap();
    let (range_end, _) = compile_expr(cx, end)?.unwrap();

    // Create loop variable
    let var = Variable::new(cx.next_var);
    cx.next_var += 1;
    cx.builder.declare_var(var, types::I64);
    cx.builder.def_var(var, range_start);
    cx.vars.insert(var_name.to_string(), (var, types::I64, TurboTy::Int));

    // Create blocks: header, body, exit
    let header_block = cx.builder.create_block();
    let body_block = cx.builder.create_block();
    let exit_block = cx.builder.create_block();

    cx.builder.ins().jump(header_block, &[]);

    // Header: check i < end
    // Do NOT seal header yet -- it has two predecessors (entry + back edge)
    cx.builder.switch_to_block(header_block);

    let current_i = cx.builder.use_var(var);
    let cond = cx.builder.ins().icmp(IntCC::SignedLessThan, current_i, range_end);
    cx.builder.ins().brif(cond, body_block, &[], exit_block, &[]);

    // Body
    cx.builder.switch_to_block(body_block);
    cx.builder.seal_block(body_block);

    compile_expr(cx, body)?;

    // Increment: i = i + 1
    if !cx.builder.is_unreachable() {
        let current_i = cx.builder.use_var(var);
        let one = cx.builder.ins().iconst(types::I64, 1);
        let next_i = cx.builder.ins().iadd(current_i, one);
        cx.builder.def_var(var, next_i);

        // Back edge
        cx.builder.ins().jump(header_block, &[]);
    }

    // NOW seal the header (both predecessors are known)
    cx.builder.seal_block(header_block);

    // Exit
    cx.builder.switch_to_block(exit_block);
    cx.builder.seal_block(exit_block);

    Ok(None)
}

fn compile_for_in_array<M: Module>(
    cx: &mut Ctx<'_, M>,
    var_name: &str,
    iterable: &Spanned<Expr>,
    body: &Spanned<Expr>,
) -> Result<MaybeTyped, CodegenError> {
    // Compile the array expression
    let (arr_ptr, arr_tty) = compile_expr(cx, iterable)?.unwrap();

    // Get array length via rt_array_len
    let len_fid = cx.rt_fns["rt_array_len"];
    let len_ref = cx.module.declare_func_in_func(len_fid, cx.builder.func);
    let call = cx.builder.ins().call(len_ref, &[arr_ptr]);
    let arr_len = cx.builder.inst_results(call)[0];

    // Determine element TurboTy from the array type
    let elem_tty = match arr_tty {
        TurboTy::Array(ref inner) => *inner.clone(),
        _ => TurboTy::Int, // fallback
    };

    // Determine Cranelift type for the element
    let elem_cl_ty = match &elem_tty {
        TurboTy::Float => types::F64,
        TurboTy::Bool => types::I8,
        _ => types::I64, // Int, Str are all i64/ptr-sized
    };

    // Create index counter variable
    let idx_var = Variable::new(cx.next_var);
    cx.next_var += 1;
    cx.builder.declare_var(idx_var, types::I64);
    let zero = cx.builder.ins().iconst(types::I64, 0);
    cx.builder.def_var(idx_var, zero);

    // Create element variable for the loop body
    let elem_var = Variable::new(cx.next_var);
    cx.next_var += 1;
    cx.builder.declare_var(elem_var, elem_cl_ty);
    let default_val = match &elem_tty {
        TurboTy::Float => cx.builder.ins().f64const(0.0),
        _ => cx.builder.ins().iconst(elem_cl_ty, 0),
    };
    cx.builder.def_var(elem_var, default_val);
    cx.vars.insert(var_name.to_string(), (elem_var, elem_cl_ty, elem_tty.clone()));

    // Loop blocks
    let header_block = cx.builder.create_block();
    let body_block = cx.builder.create_block();
    let exit_block = cx.builder.create_block();

    cx.builder.ins().jump(header_block, &[]);

    // Header: check idx < len
    // Do NOT seal header yet -- it has two predecessors (entry + back edge)
    cx.builder.switch_to_block(header_block);
    let idx = cx.builder.use_var(idx_var);
    let cond = cx.builder.ins().icmp(IntCC::SignedLessThan, idx, arr_len);
    cx.builder.ins().brif(cond, body_block, &[], exit_block, &[]);

    // Body: load element, execute body
    cx.builder.switch_to_block(body_block);
    cx.builder.seal_block(body_block);

    // Load element: arr[idx] via rt_array_get
    let get_fid = cx.rt_fns["rt_array_get"];
    let get_ref = cx.module.declare_func_in_func(get_fid, cx.builder.func);
    let idx_val = cx.builder.use_var(idx_var);
    let get_call = cx.builder.ins().call(get_ref, &[arr_ptr, idx_val]);
    let raw_elem = cx.builder.inst_results(get_call)[0];

    // rt_array_get returns raw i64 bits; convert to the correct type
    let typed_elem = match &elem_tty {
        TurboTy::Bool => cx.builder.ins().ireduce(types::I8, raw_elem),
        TurboTy::Float => cx.builder.ins().bitcast(types::F64, MemFlags::new(), raw_elem),
        _ => raw_elem,
    };
    cx.builder.def_var(elem_var, typed_elem);

    // Compile loop body
    compile_expr(cx, body)?;

    // Increment index
    if !cx.builder.is_unreachable() {
        let current_idx = cx.builder.use_var(idx_var);
        let one = cx.builder.ins().iconst(types::I64, 1);
        let next_idx = cx.builder.ins().iadd(current_idx, one);
        cx.builder.def_var(idx_var, next_idx);

        // Back edge
        cx.builder.ins().jump(header_block, &[]);
    }

    // NOW seal the header (both predecessors are known)
    cx.builder.seal_block(header_block);

    // Exit
    cx.builder.switch_to_block(exit_block);
    cx.builder.seal_block(exit_block);

    Ok(None)
}

// ── Match expression ────────────────────────────────────────────────

fn compile_match<M: Module>(
    cx: &mut Ctx<'_, M>,
    subject: &Spanned<Expr>,
    arms: &[MatchArm],
) -> Result<MaybeTyped, CodegenError> {
    let (subj_val, subj_tty) = compile_expr(cx, subject)?.unwrap();

    if arms.is_empty() {
        return Ok(None);
    }

    let merge_block = cx.builder.create_block();
    let mut has_result = false;
    let mut result_turbo_ty = TurboTy::Unit;
    let mut hit_catchall = false;

    for (i, arm) in arms.iter().enumerate() {
        let is_last = i == arms.len() - 1;
        let is_catchall = matches!(&arm.pattern.node, Pattern::Wildcard)
            || matches!(&arm.pattern.node, Pattern::Ident(name)
                if lookup_variant_tag_static(cx.enum_variants, name).is_none());

        if is_catchall {
            // Unconditional arm -- compile body and jump to merge
            let body_result = compile_expr(cx, &arm.body)?;
            emit_match_arm_jump(cx, merge_block, body_result, &mut has_result, &mut result_turbo_ty);
            hit_catchall = true;

            // Create a dead block if there are more arms after this
            if !is_last {
                let dead_block = cx.builder.create_block();
                cx.builder.switch_to_block(dead_block);
                cx.builder.seal_block(dead_block);
            }
            break;
        }

        // Conditional arm: compute whether the pattern matches
        let matches_cond = match &arm.pattern.node {
            Pattern::Ident(name) => {
                // Must be an enum variant (checked above: non-variant idents are catchall)
                let tag_val = lookup_variant_tag_static(cx.enum_variants, name).unwrap();
                let pat_val = cx.builder.ins().iconst(types::I64, tag_val as i64);
                cx.builder.ins().icmp(IntCC::Equal, subj_val, pat_val)
            }
            Pattern::IntLit(n) => {
                let pat_val = cx.builder.ins().iconst(types::I64, *n);
                cx.builder.ins().icmp(IntCC::Equal, subj_val, pat_val)
            }
            Pattern::BoolLit(b) => {
                let pat_val = cx.builder.ins().iconst(types::I8, *b as i64);
                let subj_narrowed = if cx.builder.func.dfg.value_type(subj_val) != types::I8 {
                    cx.builder.ins().ireduce(types::I8, subj_val)
                } else {
                    subj_val
                };
                cx.builder.ins().icmp(IntCC::Equal, subj_narrowed, pat_val)
            }
            Pattern::StringLit(s) => {
                let pat_ptr = cx.create_string(s)?;
                let fid = cx.rt_fns["rt_str_eq"];
                let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
                let call = cx.builder.ins().call(fref, &[subj_val, pat_ptr]);
                let eq_result = cx.builder.inst_results(call)[0];
                let zero = cx.builder.ins().iconst(types::I8, 0);
                cx.builder.ins().icmp(IntCC::NotEqual, eq_result, zero)
            }
            Pattern::Wildcard => unreachable!(), // handled above
            Pattern::Ok(binding) => {
                // Extract tag from result pointer
                let tag_fid = cx.rt_fns["rt_result_tag"];
                let tag_fref = cx.module.declare_func_in_func(tag_fid, cx.builder.func);
                let tag_call = cx.builder.ins().call(tag_fref, &[subj_val]);
                let tag = cx.builder.inst_results(tag_call)[0];
                let zero = cx.builder.ins().iconst(types::I64, 0);
                cx.builder.ins().icmp(IntCC::Equal, tag, zero)
            }
            Pattern::Err(binding) => {
                // Extract tag from result pointer
                let tag_fid = cx.rt_fns["rt_result_tag"];
                let tag_fref = cx.module.declare_func_in_func(tag_fid, cx.builder.func);
                let tag_call = cx.builder.ins().call(tag_fref, &[subj_val]);
                let tag = cx.builder.inst_results(tag_call)[0];
                let one = cx.builder.ins().iconst(types::I64, 1);
                cx.builder.ins().icmp(IntCC::Equal, tag, one)
            }
        };

        let match_block = cx.builder.create_block();
        let next_block = cx.builder.create_block();

        cx.builder.ins().brif(matches_cond, match_block, &[], next_block, &[]);

        // Compile the arm body
        cx.builder.switch_to_block(match_block);
        cx.builder.seal_block(match_block);

        // For ok/err patterns, extract the value and bind as a variable
        let saved_vars = cx.vars.clone();
        match &arm.pattern.node {
            Pattern::Ok(binding) | Pattern::Err(binding) => {
                let val_fid = cx.rt_fns["rt_result_value"];
                let val_fref = cx.module.declare_func_in_func(val_fid, cx.builder.func);
                let val_call = cx.builder.ins().call(val_fref, &[subj_val]);
                let raw_val = cx.builder.inst_results(val_call)[0];

                let var = Variable::new(cx.next_var);
                cx.next_var += 1;
                cx.builder.declare_var(var, types::I64);
                cx.builder.def_var(var, raw_val);

                // Get the inner type from the subject's Result(ok_tty, err_tty)
                let turbo_ty = match &subj_tty {
                    TurboTy::Result(ok_tty, err_tty) => {
                        if matches!(&arm.pattern.node, Pattern::Ok(_)) {
                            *ok_tty.clone()
                        } else {
                            *err_tty.clone()
                        }
                    }
                    _ => TurboTy::Int, // fallback
                };
                cx.vars.insert(binding.clone(), (var, types::I64, turbo_ty));
            }
            _ => {}
        }

        let body_result = compile_expr(cx, &arm.body)?;
        cx.vars = saved_vars;
        emit_match_arm_jump(cx, merge_block, body_result, &mut has_result, &mut result_turbo_ty);

        // Continue to next arm's check
        cx.builder.switch_to_block(next_block);
        cx.builder.seal_block(next_block);
    }

    // If no catchall was reached, we're in the fallthrough block (no arm matched).
    if !hit_catchall && !cx.builder.is_unreachable() {
        // Call rt_panic with a clear message instead of a bare trap
        let msg = cx.create_string("non-exhaustive match")?;
        cx.rt_call("rt_panic", &[msg]);
        cx.builder.ins().trap(TrapCode::unwrap_user(1));
    }

    cx.builder.switch_to_block(merge_block);
    cx.builder.seal_block(merge_block);

    if has_result {
        let param = cx.builder.block_params(merge_block)[0];
        Ok(Some((param, result_turbo_ty)))
    } else {
        Ok(None)
    }
}

/// Helper: emit the jump from a compiled match arm body to the merge block.
fn emit_match_arm_jump<M: Module>(
    cx: &mut Ctx<'_, M>,
    merge_block: cranelift::prelude::Block,
    body_result: MaybeTyped,
    has_result: &mut bool,
    result_turbo_ty: &mut TurboTy,
) {
    let needs_jump = !cx.builder.is_unreachable();
    if let Some((val, tty)) = body_result {
        if !*has_result {
            *has_result = true;
            let cl_ty = cx.builder.func.dfg.value_type(val);
            *result_turbo_ty = tty;
            cx.builder.append_block_param(merge_block, cl_ty);
        }
        if needs_jump {
            cx.builder.ins().jump(merge_block, &[val]);
        }
    } else if needs_jump {
        cx.builder.ins().jump(merge_block, &[]);
    }
}

/// Look up the integer tag for a variant name across all known enums.
fn lookup_variant_tag_static(
    enum_variants: &HashMap<String, Vec<String>>,
    variant_name: &str,
) -> Option<usize> {
    for (_enum_name, variants) in enum_variants.iter() {
        if let Some(pos) = variants.iter().position(|v| v == variant_name) {
            return Some(pos);
        }
    }
    None
}
