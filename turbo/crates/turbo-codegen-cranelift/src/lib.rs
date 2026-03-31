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
    /// Enum type: name of the enum. Unit-only enums are i64 tags; data enums are heap pointers.
    Enum(String),
    /// Function pointer: param types and return type
    Fn(Vec<TurboTy>, Box<TurboTy>),
    /// Result type (heap-allocated tagged union): ok_type, err_type
    Result(Box<TurboTy>, Box<TurboTy>),
    /// Optional type (heap-allocated tagged union): inner_type
    Optional(Box<TurboTy>),
    /// Agent type: heap-allocated struct with model/system/tools fields
    Agent(String),
    /// Future type: a spawned thread handle (pointer to JoinHandle)
    Future(Box<TurboTy>),
}

fn turbo_ty_from_type_expr(te: &TypeExpr, enum_variants: &HashMap<String, Vec<String>>) -> TurboTy {
    turbo_ty_from_type_expr_with_params(te, enum_variants, &[])
}

fn turbo_ty_from_type_expr_with_params(te: &TypeExpr, enum_variants: &HashMap<String, Vec<String>>, type_params: &[String]) -> TurboTy {
    match te {
        TypeExpr::Named(name) => {
            // Type parameters use Int representation (I64/ptr sized)
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
        TypeExpr::Optional(inner) => {
            let inner_tty = turbo_ty_from_type_expr(&inner.node, enum_variants);
            TurboTy::Optional(Box::new(inner_tty))
        }
        // Future<T> is a thread handle pointer (underlying value is i64/ptr)
        TypeExpr::Future(inner) => {
            let inner_tty = turbo_ty_from_type_expr_with_params(&inner.node, enum_variants, type_params);
            TurboTy::Future(Box::new(inner_tty))
        }
        #[allow(unreachable_patterns)] _ => TurboTy::Int,
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

/// Runtime function for assert_eq/assert_ne failure.
/// kind: 0 = assert_eq, 1 = assert_ne
/// actual and expected are C-string pointers (stringified values).
extern "C" fn rt_assert_eq_fail(kind: i64, actual: *const u8, expected: *const u8) {
    let actual_str = if actual.is_null() {
        "<null>"
    } else {
        unsafe { std::ffi::CStr::from_ptr(actual as *const std::ffi::c_char) }
            .to_str()
            .unwrap_or("<invalid>")
    };
    let expected_str = if expected.is_null() {
        "<null>"
    } else {
        unsafe { std::ffi::CStr::from_ptr(expected as *const std::ffi::c_char) }
            .to_str()
            .unwrap_or("<invalid>")
    };
    if kind == 0 {
        eprintln!("assertion failed: assert_eq({}, {})", actual_str, expected_str);
        eprintln!("  left:  {}", actual_str);
        eprintln!("  right: {}", expected_str);
    } else {
        eprintln!("assertion failed: assert_ne({}, {})", actual_str, expected_str);
        eprintln!("  both values are: {}", actual_str);
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
    let data_bytes = 8 + (len as usize) * 8; // 8 for length + 8 per element
    let total_bytes = 8 + data_bytes; // +8 for refcount header
    let layout = std::alloc::Layout::from_size_align(total_bytes, 8).unwrap();
    let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
    unsafe { *(ptr as *mut i64) = 1; } // refcount = 1
    let data_ptr = unsafe { ptr.add(8) }; // pointer past refcount header
    // Store length at the start of the data region
    unsafe { *(data_ptr as *mut i64) = len; }
    data_ptr
}

extern "C" fn rt_array_get(arr: *const u8, index: i64) -> i64 {
    let len = unsafe { *(arr as *const i64) };
    if index < 0 || index >= len {
        eprintln!("runtime error: array index {} out of bounds (length {})", index, len);
        std::process::exit(1);
    }
    unsafe { *((arr as *const i64).add(1 + index as usize)) }
}

extern "C" fn rt_array_set(arr: *mut u8, index: i64, value: i64) -> *mut u8 {
    // COW: check refcount before mutating
    let rc_ptr = unsafe { arr.sub(8) as *mut std::sync::atomic::AtomicI64 };
    let rc = unsafe { (*rc_ptr).load(std::sync::atomic::Ordering::Relaxed) };
    let target = if rc > 1 {
        // Copy-on-write: make a private copy
        let len = unsafe { *(arr as *const i64) };
        let data_size = (1 + len as usize) * 8;
        let total = 8 + data_size;
        let layout = std::alloc::Layout::from_size_align(total, 8).unwrap();
        let new_alloc = unsafe { std::alloc::alloc_zeroed(layout) };
        unsafe { *(new_alloc as *mut i64) = 1; } // new refcount = 1
        let new_data = unsafe { new_alloc.add(8) };
        unsafe { std::ptr::copy_nonoverlapping(arr, new_data, data_size); }
        // Decrement old refcount
        unsafe { (*rc_ptr).fetch_sub(1, std::sync::atomic::Ordering::Release); }
        new_data
    } else {
        arr
    };
    // Bounds check + set on the target (possibly new) array
    let len = unsafe { *(target as *const i64) };
    if index < 0 || index >= len {
        eprintln!("runtime error: array index {} out of bounds (length {})", index, len);
        std::process::exit(1);
    }
    unsafe { *((target as *mut i64).add(1 + index as usize)) = value; }
    target
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
    let data_size = (num_fields as usize) * 8;
    let total_size = 8 + data_size.max(8); // +8 for refcount header
    let layout = std::alloc::Layout::from_size_align(total_size, 8).unwrap();
    let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
    unsafe { *(ptr as *mut i64) = 1; } // refcount = 1
    unsafe { ptr.add(8) } // return pointer past refcount header
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
    let layout = std::alloc::Layout::from_size_align(8 + 16, 8).unwrap(); // +8 for refcount
    let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
    unsafe { *(ptr as *mut i64) = 1; } // refcount = 1
    let data_ptr = unsafe { ptr.add(8) };
    unsafe {
        *(data_ptr as *mut i64) = 0; // tag = ok
        *((data_ptr as *mut i64).add(1)) = value;
    }
    data_ptr
}

extern "C" fn rt_result_err(value: i64) -> *mut u8 {
    let layout = std::alloc::Layout::from_size_align(8 + 16, 8).unwrap(); // +8 for refcount
    let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
    unsafe { *(ptr as *mut i64) = 1; } // refcount = 1
    let data_ptr = unsafe { ptr.add(8) };
    unsafe {
        *(data_ptr as *mut i64) = 1; // tag = err
        *((data_ptr as *mut i64).add(1)) = value;
    }
    data_ptr
}

extern "C" fn rt_result_tag(result: *const u8) -> i64 {
    unsafe { *(result as *const i64) }
}

extern "C" fn rt_result_value(result: *const u8) -> i64 {
    unsafe { *((result as *const i64).add(1)) }
}

// ── Optional type runtime functions ──────────────────────────────────

extern "C" fn rt_option_some(value: i64) -> *mut u8 {
    let layout = std::alloc::Layout::from_size_align(8 + 16, 8).unwrap(); // +8 for refcount
    let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
    unsafe { *(ptr as *mut i64) = 1; } // refcount = 1
    let data_ptr = unsafe { ptr.add(8) };
    unsafe {
        *(data_ptr as *mut i64) = 1; // tag = some
        *((data_ptr as *mut i64).add(1)) = value;
    }
    data_ptr
}

extern "C" fn rt_option_none() -> *mut u8 {
    let layout = std::alloc::Layout::from_size_align(8 + 16, 8).unwrap(); // +8 for refcount
    let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
    unsafe { *(ptr as *mut i64) = 1; } // refcount = 1
    let data_ptr = unsafe { ptr.add(8) };
    unsafe {
        *(data_ptr as *mut i64) = 0; // tag = none
        *((data_ptr as *mut i64).add(1)) = 0;
    }
    data_ptr
}

extern "C" fn rt_option_tag(opt: *const u8) -> i64 {
    unsafe { *(opt as *const i64) }
}

extern "C" fn rt_option_value(opt: *const u8) -> i64 {
    unsafe { *((opt as *const i64).add(1)) }
}

// ── Standard library runtime functions ──────────────────────────────

extern "C" fn rt_str_split(s: *const u8, sep: *const u8) -> *mut u8 {
    let s = unsafe { std::ffi::CStr::from_ptr(s as *const std::ffi::c_char) }
        .to_str().unwrap_or("");
    let sep = unsafe { std::ffi::CStr::from_ptr(sep as *const std::ffi::c_char) }
        .to_str().unwrap_or("");
    let parts: Vec<&str> = s.split(sep).collect();
    let len = parts.len() as i64;
    // Array format: [refcount: i64][len: i64][ptr0: i64][ptr1: i64]...
    let data_size = 8 + (len as usize) * 8;
    let total = 8 + data_size; // +8 for refcount header
    let layout = std::alloc::Layout::from_size_align(total, 8).unwrap();
    let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
    unsafe { *(ptr as *mut i64) = 1; } // refcount = 1
    let data_ptr = unsafe { ptr.add(8) };
    unsafe { *(data_ptr as *mut i64) = len; }
    for (i, part) in parts.iter().enumerate() {
        let cs = std::ffi::CString::new(*part).unwrap();
        let p = cs.into_raw() as i64;
        unsafe { *((data_ptr as *mut i64).add(1 + i)) = p; }
    }
    data_ptr
}

extern "C" fn rt_str_trim(s: *const u8) -> *const u8 {
    let s = unsafe { std::ffi::CStr::from_ptr(s as *const std::ffi::c_char) }
        .to_str().unwrap_or("");
    let trimmed = s.trim();
    let cs = std::ffi::CString::new(trimmed).unwrap();
    cs.into_raw() as *const u8
}

extern "C" fn rt_str_upper(s: *const u8) -> *const u8 {
    let s = unsafe { std::ffi::CStr::from_ptr(s as *const std::ffi::c_char) }
        .to_str().unwrap_or("");
    let upper = s.to_uppercase();
    let cs = std::ffi::CString::new(upper).unwrap();
    cs.into_raw() as *const u8
}

extern "C" fn rt_str_lower(s: *const u8) -> *const u8 {
    let s = unsafe { std::ffi::CStr::from_ptr(s as *const std::ffi::c_char) }
        .to_str().unwrap_or("");
    let lower = s.to_lowercase();
    let cs = std::ffi::CString::new(lower).unwrap();
    cs.into_raw() as *const u8
}

extern "C" fn rt_str_starts_with(s: *const u8, prefix: *const u8) -> i8 {
    let s = unsafe { std::ffi::CStr::from_ptr(s as *const std::ffi::c_char) }
        .to_str().unwrap_or("");
    let prefix = unsafe { std::ffi::CStr::from_ptr(prefix as *const std::ffi::c_char) }
        .to_str().unwrap_or("");
    if s.starts_with(prefix) { 1 } else { 0 }
}

extern "C" fn rt_str_ends_with(s: *const u8, suffix: *const u8) -> i8 {
    let s = unsafe { std::ffi::CStr::from_ptr(s as *const std::ffi::c_char) }
        .to_str().unwrap_or("");
    let suffix = unsafe { std::ffi::CStr::from_ptr(suffix as *const std::ffi::c_char) }
        .to_str().unwrap_or("");
    if s.ends_with(suffix) { 1 } else { 0 }
}

extern "C" fn rt_str_replace(s: *const u8, from: *const u8, to: *const u8) -> *const u8 {
    let s = unsafe { std::ffi::CStr::from_ptr(s as *const std::ffi::c_char) }
        .to_str().unwrap_or("");
    let from = unsafe { std::ffi::CStr::from_ptr(from as *const std::ffi::c_char) }
        .to_str().unwrap_or("");
    let to_s = unsafe { std::ffi::CStr::from_ptr(to as *const std::ffi::c_char) }
        .to_str().unwrap_or("");
    let result = s.replace(from, to_s);
    let cs = std::ffi::CString::new(result).unwrap();
    cs.into_raw() as *const u8
}

extern "C" fn rt_str_char_at(s: *const u8, index: i64) -> *const u8 {
    let s = unsafe { std::ffi::CStr::from_ptr(s as *const std::ffi::c_char) }
        .to_str().unwrap_or("");
    if let Some(c) = s.chars().nth(index as usize) {
        let cs = std::ffi::CString::new(c.to_string()).unwrap();
        cs.into_raw() as *const u8
    } else {
        eprintln!("runtime error: string index {} out of bounds (length {})", index, s.chars().count());
        std::process::exit(1);
    }
}

/// contains(s, sub) -> bool — returns true if s contains sub
extern "C" fn rt_str_contains(s: *const u8, sub: *const u8) -> i8 {
    let s = unsafe { std::ffi::CStr::from_ptr(s as *const std::ffi::c_char) }
        .to_str().unwrap_or("");
    let sub = unsafe { std::ffi::CStr::from_ptr(sub as *const std::ffi::c_char) }
        .to_str().unwrap_or("");
    if s.contains(sub) { 1 } else { 0 }
}

/// index_of(s, sub) -> i64 — returns byte offset or -1 if not found
extern "C" fn rt_str_index_of(s: *const u8, sub: *const u8) -> i64 {
    let s = unsafe { std::ffi::CStr::from_ptr(s as *const std::ffi::c_char) }
        .to_str().unwrap_or("");
    let sub = unsafe { std::ffi::CStr::from_ptr(sub as *const std::ffi::c_char) }
        .to_str().unwrap_or("");
    match s.find(sub) {
        Some(pos) => pos as i64,
        None => -1,
    }
}

/// join(arr, sep) -> str — join string array elements with separator
extern "C" fn rt_str_join(arr: *const u8, sep: *const u8) -> *const u8 {
    let sep = unsafe { std::ffi::CStr::from_ptr(sep as *const std::ffi::c_char) }
        .to_str().unwrap_or("");
    // arr is a Turbo array: first 8 bytes = length, then 8 bytes per element (string pointers)
    let len = unsafe { *(arr as *const i64) } as usize;
    let mut parts = Vec::with_capacity(len);
    for i in 0..len {
        let elem_ptr = unsafe { *((arr as *const i64).add(1 + i)) } as *const u8;
        let elem = unsafe { std::ffi::CStr::from_ptr(elem_ptr as *const std::ffi::c_char) }
            .to_str().unwrap_or("");
        parts.push(elem.to_string());
    }
    let joined = parts.join(sep);
    let cs = std::ffi::CString::new(joined).unwrap();
    cs.into_raw() as *const u8
}

/// repeat(s, n) -> str — repeat string n times
extern "C" fn rt_str_repeat(s: *const u8, n: i64) -> *const u8 {
    let s = unsafe { std::ffi::CStr::from_ptr(s as *const std::ffi::c_char) }
        .to_str().unwrap_or("");
    let repeated = s.repeat(n.max(0) as usize);
    let cs = std::ffi::CString::new(repeated).unwrap();
    cs.into_raw() as *const u8
}

extern "C" fn rt_read_line() -> *const u8 {
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).unwrap_or(0);
    let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');
    let cs = std::ffi::CString::new(trimmed).unwrap();
    cs.into_raw() as *const u8
}

extern "C" fn rt_read_file(path: *const u8) -> *const u8 {
    let path = unsafe { std::ffi::CStr::from_ptr(path as *const std::ffi::c_char) }
        .to_str().unwrap_or("");
    match std::fs::read_to_string(path) {
        Ok(content) => {
            let cs = std::ffi::CString::new(content).unwrap();
            cs.into_raw() as *const u8
        }
        Err(e) => {
            eprintln!("runtime error: cannot read file '{}': {}", path, e);
            std::process::exit(1);
        }
    }
}

extern "C" fn rt_write_file(path: *const u8, content: *const u8) {
    let path = unsafe { std::ffi::CStr::from_ptr(path as *const std::ffi::c_char) }
        .to_str().unwrap_or("");
    let content = unsafe { std::ffi::CStr::from_ptr(content as *const std::ffi::c_char) }
        .to_str().unwrap_or("");
    if let Err(e) = std::fs::write(path, content) {
        eprintln!("runtime error: cannot write file '{}': {}", path, e);
        std::process::exit(1);
    }
}

extern "C" fn rt_pow(base: i64, exp: i64) -> i64 {
    if exp < 0 { return 0; }
    let mut result: i64 = 1;
    for _ in 0..exp {
        result = result.wrapping_mul(base);
    }
    result
}

extern "C" fn rt_sqrt(x: f64) -> f64 {
    x.sqrt()
}

// ── HTTP + JSON runtime functions ───────────────────────────────────

/// HTTP GET via system curl. Returns response body as a C string.
extern "C" fn rt_http_get(url: *const u8) -> *const u8 {
    let url = unsafe { std::ffi::CStr::from_ptr(url as *const std::ffi::c_char) }
        .to_str().unwrap_or("");
    let output = std::process::Command::new("curl")
        .arg("-s")
        .arg("-L")
        .arg(url)
        .output();
    match output {
        Ok(out) => {
            let body = String::from_utf8_lossy(&out.stdout).to_string();
            let cs = std::ffi::CString::new(body).unwrap_or_else(|_| {
                std::ffi::CString::new("").unwrap()
            });
            cs.into_raw() as *const u8
        }
        Err(e) => {
            let cs = std::ffi::CString::new(format!("error: {}", e)).unwrap();
            cs.into_raw() as *const u8
        }
    }
}

/// HTTP POST via system curl. Takes URL and body, returns response body as a C string.
extern "C" fn rt_http_post(url: *const u8, body: *const u8) -> *const u8 {
    let url = unsafe { std::ffi::CStr::from_ptr(url as *const std::ffi::c_char) }
        .to_str().unwrap_or("");
    let body_str = unsafe { std::ffi::CStr::from_ptr(body as *const std::ffi::c_char) }
        .to_str().unwrap_or("");
    let output = std::process::Command::new("curl")
        .arg("-s")
        .arg("-L")
        .arg("-X").arg("POST")
        .arg("-H").arg("Content-Type: application/json")
        .arg("-d").arg(body_str)
        .arg(url)
        .output();
    match output {
        Ok(out) => {
            let resp = String::from_utf8_lossy(&out.stdout).to_string();
            let cs = std::ffi::CString::new(resp).unwrap_or_else(|_| {
                std::ffi::CString::new("").unwrap()
            });
            cs.into_raw() as *const u8
        }
        Err(e) => {
            let cs = std::ffi::CString::new(format!("error: {}", e)).unwrap();
            cs.into_raw() as *const u8
        }
    }
}

/// Extract a top-level key from a JSON string. Returns the value as a string.
/// Handles string values, numbers, booleans, and null.
extern "C" fn rt_json_get(json: *const u8, key: *const u8) -> *const u8 {
    let json_str = unsafe { std::ffi::CStr::from_ptr(json as *const std::ffi::c_char) }
        .to_str().unwrap_or("");
    let key_str = unsafe { std::ffi::CStr::from_ptr(key as *const std::ffi::c_char) }
        .to_str().unwrap_or("");

    // Search for "key" in the JSON
    let search = format!("\"{}\"", key_str);
    if let Some(pos) = json_str.find(&search) {
        let after_key = &json_str[pos + search.len()..];
        // Skip whitespace and colon
        let trimmed = after_key.trim_start();
        if let Some(after_colon) = trimmed.strip_prefix(':') {
            let value_start = after_colon.trim_start();
            if value_start.starts_with('"') {
                // String value: find closing quote, handling escaped quotes
                let inner = &value_start[1..];
                let mut end = 0;
                let bytes = inner.as_bytes();
                while end < bytes.len() {
                    if bytes[end] == b'\\' {
                        end += 2; // skip escaped char
                    } else if bytes[end] == b'"' {
                        break;
                    } else {
                        end += 1;
                    }
                }
                let value = &inner[..end];
                let cs = std::ffi::CString::new(value).unwrap_or_else(|_| {
                    std::ffi::CString::new("").unwrap()
                });
                return cs.into_raw() as *const u8;
            } else {
                // Number, bool, or null: read until , } ] or whitespace
                let end = value_start.find(|c: char| c == ',' || c == '}' || c == ']' || c == ' ' || c == '\n' || c == '\r' || c == '\t')
                    .unwrap_or(value_start.len());
                let value = &value_start[..end];
                let cs = std::ffi::CString::new(value).unwrap_or_else(|_| {
                    std::ffi::CString::new("").unwrap()
                });
                return cs.into_raw() as *const u8;
            }
        }
    }
    // Key not found: return empty string
    let cs = std::ffi::CString::new("").unwrap();
    cs.into_raw() as *const u8
}

/// Build a JSON object string from a key and value: {"key": "value"}
extern "C" fn rt_json_stringify(key: *const u8, value: *const u8) -> *const u8 {
    let key_str = unsafe { std::ffi::CStr::from_ptr(key as *const std::ffi::c_char) }
        .to_str().unwrap_or("");
    let value_str = unsafe { std::ffi::CStr::from_ptr(value as *const std::ffi::c_char) }
        .to_str().unwrap_or("");
    let result = format!("{{\"{}\":\"{}\"}}",
        key_str.replace('\\', "\\\\").replace('"', "\\\""),
        value_str.replace('\\', "\\\\").replace('"', "\\\""));
    let cs = std::ffi::CString::new(result).unwrap();
    cs.into_raw() as *const u8
}

// ── HTTP server runtime functions ───────────────────────────────────

use std::sync::Mutex;

/// Route handler function pointer: (env_ptr, request_body_cstr) -> response_cstr
type RouteHandler = extern "C" fn(*const u8, *const u8) -> *const u8;

struct HttpServer {
    port: u16,
    routes: Vec<(String, String, RouteHandler, *const u8)>, // (method, path, handler_fn, env_ptr)
}

unsafe impl Send for HttpServer {}

static HTTP_SERVERS: Mutex<Vec<Box<HttpServer>>> = Mutex::new(Vec::new());

/// Create a new HTTP server. Returns a server id (index).
extern "C" fn rt_http_server(port: i64) -> i64 {
    let server = Box::new(HttpServer {
        port: port as u16,
        routes: Vec::new(),
    });
    let mut servers = HTTP_SERVERS.lock().unwrap();
    let id = servers.len() as i64;
    servers.push(server);
    id
}

/// Register a route handler on the server.
extern "C" fn rt_http_route(server_id: i64, method: *const u8, path: *const u8, handler: *const u8, env_ptr: *const u8) {
    let method = unsafe { std::ffi::CStr::from_ptr(method as *const std::ffi::c_char) }
        .to_str().unwrap_or("").to_string();
    let path = unsafe { std::ffi::CStr::from_ptr(path as *const std::ffi::c_char) }
        .to_str().unwrap_or("").to_string();
    let handler: RouteHandler = unsafe { std::mem::transmute(handler) };

    let mut servers = HTTP_SERVERS.lock().unwrap();
    if let Some(server) = servers.get_mut(server_id as usize) {
        server.routes.push((method, path, handler, env_ptr));
    }
}

/// Start the HTTP server. Blocks forever accepting connections.
extern "C" fn rt_http_listen(server_id: i64) {
    use std::io::{Read, Write, BufRead, BufReader};
    use std::net::TcpListener;

    let (port, routes) = {
        let servers = HTTP_SERVERS.lock().unwrap();
        let server = &servers[server_id as usize];
        let port = server.port;
        let routes: Vec<(String, String, RouteHandler, *const u8)> = server.routes.clone();
        (port, routes)
    };

    let addr = format!("0.0.0.0:{}", port);
    let listener = TcpListener::bind(&addr).expect("failed to bind HTTP server");

    for stream in listener.incoming() {
        if let Ok(stream) = stream {
            let mut reader = BufReader::new(&stream);
            let mut request_line = String::new();
            if reader.read_line(&mut request_line).is_err() { continue; }

            let parts: Vec<&str> = request_line.trim().split_whitespace().collect();
            if parts.len() < 2 { continue; }
            let method = parts[0];
            let path = parts[1];

            // Read headers
            let mut content_length: usize = 0;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() { break; }
                if line.trim().is_empty() { break; }
                if line.to_lowercase().starts_with("content-length:") {
                    content_length = line.split(':').nth(1).unwrap_or("0").trim().parse().unwrap_or(0);
                }
            }

            // Read body
            let mut body = vec![0u8; content_length];
            if content_length > 0 {
                let _ = reader.read_exact(&mut body);
            }

            // We need a mutable reference to stream for writing, drop the BufReader first
            drop(reader);
            let mut stream = stream;

            // Find matching route
            let mut matched = false;
            for (route_method, route_path, handler, env_ptr) in &routes {
                if route_method == method && route_path == path {
                    let body_str = String::from_utf8_lossy(&body);
                    let req_cstr = std::ffi::CString::new(body_str.as_ref()).unwrap_or_else(|_| {
                        std::ffi::CString::new("").unwrap()
                    });
                    let response_ptr = handler(*env_ptr, req_cstr.as_ptr() as *const u8);

                    let http_response = if response_ptr.is_null() {
                        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 0\r\n\r\n".to_string()
                    } else {
                        let resp = unsafe { std::ffi::CStr::from_ptr(response_ptr as *const std::ffi::c_char) }
                            .to_str().unwrap_or("");
                        if let Some((status, resp_body)) = resp.split_once(':') {
                            let code = status.parse::<u16>().unwrap_or(200);
                            let status_text = match code {
                                200 => "OK", 201 => "Created", 204 => "No Content",
                                400 => "Bad Request", 401 => "Unauthorized", 403 => "Forbidden",
                                404 => "Not Found", 500 => "Internal Server Error", _ => "OK",
                            };
                            format!(
                                "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nAccess-Control-Allow-Origin: *\r\nContent-Length: {}\r\n\r\n{}",
                                code, status_text, resp_body.len(), resp_body
                            )
                        } else {
                            format!(
                                "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{}",
                                resp.len(), resp
                            )
                        }
                    };

                    let _ = stream.write_all(http_response.as_bytes());
                    let _ = stream.flush();
                    matched = true;
                    break;
                }
            }

            if !matched {
                let not_found = "HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\n\r\nNot Found";
                let _ = stream.write_all(not_found.as_bytes());
                let _ = stream.flush();
            }
        }
    }
}

/// Build a response string in "STATUS:BODY" format.
extern "C" fn rt_respond(status: i64, body: *const u8) -> *const u8 {
    let body_str = unsafe { std::ffi::CStr::from_ptr(body as *const std::ffi::c_char) }
        .to_str().unwrap_or("");
    let response = format!("{}:{}", status, body_str);
    let cs = std::ffi::CString::new(response).unwrap_or_else(|_| {
        std::ffi::CString::new("200:").unwrap()
    });
    cs.into_raw() as *const u8
}

/// Extract body from request (identity — request is already the body string).
extern "C" fn rt_request_body(req: *const u8) -> *const u8 {
    req
}

// ── Async runtime functions ─────────────────────────────────────────

/// Sleep the current thread for `ms` milliseconds.
extern "C" fn rt_sleep_ms(ms: i64) {
    std::thread::sleep(std::time::Duration::from_millis(ms as u64));
}

/// Spawn a thunk with an args pointer on a new OS thread.
/// The thunk is `extern "C" fn(args_ptr: *mut u8) -> i64`.
/// Returns a pointer to a heap-allocated JoinHandle.
extern "C" fn rt_spawn_with_args(
    thunk: extern "C" fn(*mut u8) -> i64,
    args_ptr: *mut u8,
) -> *mut u8 {
    // Cast pointer to usize (which is Send) to pass across thread boundary
    let args_addr = args_ptr as usize;
    let handle = std::thread::spawn(move || thunk(args_addr as *mut u8));
    let boxed: Box<std::thread::JoinHandle<i64>> = Box::new(handle);
    Box::into_raw(boxed) as *mut u8
}

/// Await (join) a spawned thread handle and return its result.
extern "C" fn rt_await_handle(handle_ptr: *mut u8) -> i64 {
    if handle_ptr.is_null() {
        return 0;
    }
    let handle: Box<std::thread::JoinHandle<i64>> =
        unsafe { Box::from_raw(handle_ptr as *mut std::thread::JoinHandle<i64>) };
    handle.join().unwrap_or(0)
}

// ── Channel runtime functions ────────────────────────────────────────

/// Create a new channel. Returns a heap-allocated struct: [refcount: i64][sender_ptr: i64, receiver_ptr: i64].
extern "C" fn rt_channel_create() -> *mut u8 {
    let (tx, rx) = std::sync::mpsc::channel::<i64>();
    let tx_box = Box::into_raw(Box::new(tx)) as i64;
    let rx_box = Box::into_raw(Box::new(rx)) as i64;

    let ptr = unsafe {
        std::alloc::alloc_zeroed(std::alloc::Layout::from_size_align(8 + 16, 8).unwrap())
    };
    unsafe { *(ptr as *mut i64) = 1; } // refcount = 1
    let data_ptr = unsafe { ptr.add(8) };
    unsafe {
        *(data_ptr as *mut i64) = tx_box;
        *((data_ptr as *mut i64).add(1)) = rx_box;
    }
    data_ptr
}

/// Send a value on a channel.
extern "C" fn rt_channel_send(ch: *const u8, value: i64) {
    let tx_ptr = unsafe { *(ch as *const i64) } as *mut std::sync::mpsc::Sender<i64>;
    let tx = unsafe { &*tx_ptr };
    tx.send(value).ok();
}

/// Receive a value from a channel (blocking).
extern "C" fn rt_channel_recv(ch: *const u8) -> i64 {
    let rx_ptr = unsafe { *((ch as *const i64).add(1)) } as *mut std::sync::mpsc::Receiver<i64>;
    let rx = unsafe { &*rx_ptr };
    rx.recv().unwrap_or(0)
}

/// Clone a channel's sender for passing to spawned threads.
/// Returns a new channel handle with a cloned sender and the same receiver pointer.
extern "C" fn rt_channel_clone_sender(ch: *const u8) -> *mut u8 {
    let tx_ptr = unsafe { *(ch as *const i64) } as *mut std::sync::mpsc::Sender<i64>;
    let tx = unsafe { &*tx_ptr };
    let cloned = tx.clone();
    let new_tx = Box::into_raw(Box::new(cloned)) as i64;

    let rx_ptr = unsafe { *((ch as *const i64).add(1)) };
    let ptr = unsafe {
        std::alloc::alloc_zeroed(std::alloc::Layout::from_size_align(8 + 16, 8).unwrap())
    };
    unsafe { *(ptr as *mut i64) = 1; } // refcount = 1
    let data_ptr = unsafe { ptr.add(8) };
    unsafe {
        *(data_ptr as *mut i64) = new_tx;
        *((data_ptr as *mut i64).add(1)) = rx_ptr;
    }
    data_ptr
}

// ── Mutex runtime functions ─────────────────────────────────────────

/// Create a mutex wrapping an i64 value. Returns a pointer to an Arc<Mutex<i64>>.
extern "C" fn rt_mutex_create(value: i64) -> *mut u8 {
    let m = std::sync::Arc::new(std::sync::Mutex::new(value));
    std::sync::Arc::into_raw(m) as *mut u8
}

/// Get the current value inside a mutex.
extern "C" fn rt_mutex_get(m: *const u8) -> i64 {
    let arc = unsafe { std::sync::Arc::from_raw(m as *const std::sync::Mutex<i64>) };
    let val = *arc.lock().unwrap();
    let _ = std::sync::Arc::into_raw(arc); // don't drop
    val
}

/// Set the value inside a mutex.
extern "C" fn rt_mutex_set(m: *const u8, value: i64) {
    let arc = unsafe { std::sync::Arc::from_raw(m as *const std::sync::Mutex<i64>) };
    *arc.lock().unwrap() = value;
    let _ = std::sync::Arc::into_raw(arc); // don't drop
}

/// Clone a mutex handle (increments the Arc refcount). Returns a new pointer.
extern "C" fn rt_mutex_clone(m: *const u8) -> *mut u8 {
    let arc = unsafe { std::sync::Arc::from_raw(m as *const std::sync::Mutex<i64>) };
    let cloned = arc.clone();
    let _ = std::sync::Arc::into_raw(arc); // don't drop original
    std::sync::Arc::into_raw(cloned) as *mut u8
}

// ── HashMap runtime functions ───────────────────────────────────────

/// Create a new empty HashMap<String, String>. Returns an opaque pointer.
extern "C" fn rt_hashmap_new() -> *mut u8 {
    let map: HashMap<String, String> = HashMap::new();
    let boxed = Box::new(map);
    Box::into_raw(boxed) as *mut u8
}

/// Set a key-value pair in the hashmap.
extern "C" fn rt_hashmap_set(map_ptr: *mut u8, key: *const u8, value: *const u8) {
    let map = unsafe { &mut *(map_ptr as *mut HashMap<String, String>) };
    let key = unsafe { std::ffi::CStr::from_ptr(key as *const std::ffi::c_char) }
        .to_str().unwrap_or("").to_string();
    let value = unsafe { std::ffi::CStr::from_ptr(value as *const std::ffi::c_char) }
        .to_str().unwrap_or("").to_string();
    map.insert(key, value);
}

/// Get a value by key. Returns a C string pointer, or null if not found.
extern "C" fn rt_hashmap_get(map_ptr: *const u8, key: *const u8) -> *const u8 {
    let map = unsafe { &*(map_ptr as *const HashMap<String, String>) };
    let key = unsafe { std::ffi::CStr::from_ptr(key as *const std::ffi::c_char) }
        .to_str().unwrap_or("");
    match map.get(key) {
        Some(v) => {
            let cs = std::ffi::CString::new(v.as_str()).unwrap();
            cs.into_raw() as *const u8
        }
        None => std::ptr::null()
    }
}

/// Check if a key exists. Returns 1 (true) or 0 (false).
extern "C" fn rt_hashmap_has(map_ptr: *const u8, key: *const u8) -> i8 {
    let map = unsafe { &*(map_ptr as *const HashMap<String, String>) };
    let key = unsafe { std::ffi::CStr::from_ptr(key as *const std::ffi::c_char) }
        .to_str().unwrap_or("");
    if map.contains_key(key) { 1 } else { 0 }
}

/// Return the number of entries in the hashmap.
extern "C" fn rt_hashmap_len(map_ptr: *const u8) -> i64 {
    let map = unsafe { &*(map_ptr as *const HashMap<String, String>) };
    map.len() as i64
}

/// Return all keys as a [str] array (same format as rt_str_split).
extern "C" fn rt_hashmap_keys(map_ptr: *const u8) -> *mut u8 {
    let map = unsafe { &*(map_ptr as *const HashMap<String, String>) };
    let mut keys: Vec<&str> = map.keys().map(|k| k.as_str()).collect();
    keys.sort(); // deterministic order for testing
    let len = keys.len() as i64;
    // Array format: [refcount: i64][len: i64][ptr0: i64][ptr1: i64]...
    let data_size = 8 + (len as usize) * 8;
    let total = 8 + data_size; // +8 for refcount header
    let layout = std::alloc::Layout::from_size_align(total, 8).unwrap();
    let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
    unsafe { *(ptr as *mut i64) = 1; } // refcount = 1
    let data_ptr = unsafe { ptr.add(8) };
    unsafe { *(data_ptr as *mut i64) = len; }
    for (i, key) in keys.iter().enumerate() {
        let cs = std::ffi::CString::new(*key).unwrap();
        unsafe { *((data_ptr as *mut i64).add(1 + i)) = cs.into_raw() as i64; }
    }
    data_ptr
}

/// Remove a key from the hashmap.
extern "C" fn rt_hashmap_remove(map_ptr: *mut u8, key: *const u8) {
    let map = unsafe { &mut *(map_ptr as *mut HashMap<String, String>) };
    let key = unsafe { std::ffi::CStr::from_ptr(key as *const std::ffi::c_char) }
        .to_str().unwrap_or("");
    map.remove(key);
}

// ── ARC (Automatic Reference Counting) runtime functions ────────────

/// Increment the reference count of a heap-allocated object.
/// The refcount lives at data_ptr - 8 (the header before the data).
extern "C" fn rt_retain(data_ptr: *mut u8) {
    if data_ptr.is_null() { return; }
    let header = unsafe { data_ptr.sub(8) as *mut std::sync::atomic::AtomicI64 };
    unsafe { (*header).fetch_add(1, std::sync::atomic::Ordering::Relaxed); }
}

/// Decrement the reference count of a heap-allocated object.
/// When the refcount reaches 0, the memory could be freed.
/// For now (Sprint 17), we track but don't free — proper dealloc
/// requires storing allocation size in the header.
extern "C" fn rt_release(data_ptr: *mut u8) {
    if data_ptr.is_null() { return; }
    let header = unsafe { data_ptr.sub(8) as *mut std::sync::atomic::AtomicI64 };
    let _prev = unsafe { (*header).fetch_sub(1, std::sync::atomic::Ordering::Release) };
    if _prev == 1 {
        std::sync::atomic::fence(std::sync::atomic::Ordering::Acquire);
        // Refcount reached 0 — memory could be freed here.
        // TODO: store allocation size in header for proper dealloc.
        // For now we just let it leak (same as before ARC).
    }
}

// ── Runtime C source for AOT linking ────────────────────────────────

const RUNTIME_C: &str = include_str!("../runtime/turbo_rt.c");

// ── Codegen context (generic over Module type) ──────────────────────

/// Max depth for inlining recursive functions at call sites.
/// Depth 2 reduces function calls by ~4x while keeping JIT compile time low.
/// Higher depths generate too much IR for Cranelift to compile efficiently.
const MAX_INLINE_DEPTH: usize = 2;

#[allow(dead_code)]
struct Ctx<'a, M: Module> {
    builder: FunctionBuilder<'a>,
    module: &'a mut M,
    user_fns: &'a HashMap<String, FuncId>,
    fn_ret_types: &'a HashMap<String, TurboTy>,
    fn_asts: &'a HashMap<String, &'a FnDef>,
    fn_type_params: &'a HashMap<String, Vec<String>>,
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
    /// Data-carrying enum variant fields: (enum_name, variant_name) -> field TurboTys
    enum_variant_fields: &'a HashMap<(String, String), Vec<TurboTy>>,
    /// Max slots per data enum: enum_name -> max field count across all variants
    enum_max_slots: &'a HashMap<String, usize>,
    /// Map from closure span start offset to (synthetic function name, TurboTy::Fn, free_var_names)
    closure_fns: &'a HashMap<usize, (String, TurboTy, Vec<String>)>,
    /// Trait implementations: type_name -> set of trait names
    trait_impls: &'a HashMap<String, Vec<String>>,
    /// Current function inlining depth (0 = no inlining)
    inline_depth: usize,
    /// Capture info populated during Expr::Closure compilation
    closure_captures: &'a mut HashMap<usize, CaptureInfo>,
    /// Concrete field types for generic struct instances: var_name -> vec of (field_name, TurboTy)
    generic_struct_field_overrides: HashMap<String, Vec<(String, TurboTy)>>,
    /// Temporary: last struct literal's concrete field types (set during StructLit compilation, consumed by Let)
    last_struct_lit_concrete_fields: Option<Vec<(String, TurboTy)>>,
    /// Agent definitions: agent_name -> (model, tools, system_prompt)
    agent_defs: &'a HashMap<String, (String, Vec<String>, Option<String>)>,
    /// Spawn thunk map: spawn expr span start -> thunk function name
    spawn_thunks: &'a HashMap<usize, String>,
    /// Module-level constants: name -> AST expression (inlined at usage sites)
    constants: &'a HashMap<String, Spanned<Expr>>,
    /// Struct derives: struct_name -> vec of derived trait names
    struct_derives: &'a HashMap<String, Vec<String>>,
    /// Stack of loop contexts for break/continue: (header_block, exit_block)
    loop_stack: Vec<(cranelift::prelude::Block, cranelift::prelude::Block)>,
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

    /// Convert a value to an I8 boolean for use in `brif`.
    /// If the value is already I8 (e.g. from `icmp` or a bool variable),
    /// return it directly — avoiding a redundant `icmp(NotEqual, val, 0)`.
    fn to_bool(&mut self, val: Value) -> Value {
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
    jit_builder.symbol("rt_assert_eq_fail", rt_assert_eq_fail as *const u8);
    jit_builder.symbol("rt_div_by_zero", rt_div_by_zero as *const u8);
    jit_builder.symbol("rt_int_overflow", rt_int_overflow as *const u8);
    jit_builder.symbol("rt_str_concat", rt_str_concat as *const u8);
    jit_builder.symbol("rt_str_eq", rt_str_eq as *const u8);
    jit_builder.symbol("rt_array_alloc", rt_array_alloc as *const u8);
    jit_builder.symbol("rt_array_get", rt_array_get as *const u8);
    jit_builder.symbol("rt_array_set", rt_array_set as *const u8);
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
    jit_builder.symbol("rt_channel_clone_sender", rt_channel_clone_sender as *const u8);
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

    module.finalize_definitions()
        .map_err(|e| CodegenError { message: e.to_string() })?;

    let main_id = user_fns.get("main")
        .ok_or_else(|| CodegenError { message: "no `main` function found".to_string() })?;
    let main_ptr = module.get_finalized_function(*main_id);
    let main_fn: fn() = unsafe { std::mem::transmute(main_ptr) };
    main_fn();

    Ok(())
}

/// Compile a module and run a single named function (used for `turbo test --run-fn`).
/// The function is called via JIT and the process exits with the function's outcome
/// (0 on success, 1 on assertion failure).
pub fn jit_run_function(ast_module: &turbo_ast::Module, fn_name: &str) -> Result<(), CodegenError> {
    let mut flag_builder = settings::builder();
    flag_builder.set("use_colocated_libcalls", "false").unwrap();
    flag_builder.set("is_pic", "false").unwrap();
    flag_builder.set("opt_level", "speed_and_size").unwrap();
    flag_builder.set("enable_verifier", "false").unwrap();
    flag_builder.set("enable_alias_analysis", "true").unwrap();

    let isa_builder = cranelift_native::builder()
        .map_err(|e| CodegenError { message: e.to_string() })?;
    let isa = isa_builder
        .finish(settings::Flags::new(flag_builder))
        .map_err(|e| CodegenError { message: e.to_string() })?;

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
    jit_builder.symbol("rt_channel_clone_sender", rt_channel_clone_sender as *const u8);
    jit_builder.symbol("rt_mutex_create", rt_mutex_create as *const u8);
    jit_builder.symbol("rt_mutex_get", rt_mutex_get as *const u8);
    jit_builder.symbol("rt_mutex_set", rt_mutex_set as *const u8);
    jit_builder.symbol("rt_mutex_clone", rt_mutex_clone as *const u8);
    jit_builder.symbol("rt_retain", rt_retain as *const u8);
    jit_builder.symbol("rt_release", rt_release as *const u8);

    let mut module = JITModule::new(jit_builder);
    let user_fns = compile_module(&mut module, ast_module, ptr_type, Linkage::Local, false)?;

    module.finalize_definitions()
        .map_err(|e| CodegenError { message: e.to_string() })?;

    let func_id = user_fns.get(fn_name)
        .ok_or_else(|| CodegenError { message: format!("no function `{fn_name}` found") })?;
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
        .arg("-lm")
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

// ── Inlining helpers ────────────────────────────────────────────────

/// Returns true if an expression subtree contains any return statement.
/// Functions with returns can't be safely inlined (would need merge blocks).
fn has_return(expr: &Expr) -> bool {
    match expr {
        Expr::Block { stmts, tail_expr } => {
            for stmt in stmts {
                match &stmt.node {
                    Stmt::Return(_) => return true,
                    Stmt::Let { value, .. } => { if has_return(&value.node) { return true; } }
                    Stmt::Expr(e) => { if has_return(&e.node) { return true; } }
                    Stmt::Defer(e) => { if has_return(&e.node) { return true; } }
                }
            }
            tail_expr.as_ref().is_some_and(|t| has_return(&t.node))
        }
        Expr::If { condition, then_branch, else_branch } => {
            has_return(&condition.node) ||
            has_return(&then_branch.node) ||
            else_branch.as_ref().is_some_and(|e| has_return(&e.node))
        }
        Expr::While { condition, body } => {
            has_return(&condition.node) || has_return(&body.node)
        }
        Expr::ForIn { iterable, body, .. } => {
            has_return(&iterable.node) || has_return(&body.node)
        }
        Expr::BinaryOp { left, right, .. } => {
            has_return(&left.node) || has_return(&right.node)
        }
        Expr::UnaryOp { expr, .. } => has_return(&expr.node),
        Expr::Call { callee, args } => {
            has_return(&callee.node) || args.iter().any(|a| has_return(&a.node))
        }
        Expr::Assign { value, .. } | Expr::CompoundAssign { value, .. } => has_return(&value.node),
        Expr::Await(inner) | Expr::Spawn(inner) | Expr::Try(inner) => has_return(&inner.node),
        Expr::FieldAssign { object, value, .. } => {
            has_return(&object.node) || has_return(&value.node)
        }
        Expr::IndexAssign { object, index, value } => {
            has_return(&object.node) || has_return(&index.node) || has_return(&value.node)
        }
        Expr::Index { object, index } => {
            has_return(&object.node) || has_return(&index.node)
        }
        Expr::Closure { body, .. } => has_return(&body.node),
        Expr::Match { subject, arms } => {
            has_return(&subject.node) ||
            arms.iter().any(|a| has_return(&a.body.node))
        }
        Expr::OkExpr(e) | Expr::ErrExpr(e) | Expr::SomeExpr(e) => has_return(&e.node),
        Expr::NoneExpr => false,
        Expr::NullCoalesce { value, default } => {
            has_return(&value.node) || has_return(&default.node)
        }
        Expr::Interpolation(parts) => {
            parts.iter().any(|p| {
                if let InterpolPart::Expr(e) = p { has_return(&e.node) } else { false }
            })
        }
        _ => false,
    }
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
        Expr::If { condition, then_branch, else_branch } => {
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
        Expr::ForIn { var_name, iterable, body } => {
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
        Expr::IndexAssign { object, index, value } => {
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
        Expr::EnumVariant { .. } | Expr::IntLit(_) | Expr::FloatLit(_)
        | Expr::StringLit(_) | Expr::BoolLit(_) | Expr::Unit | Expr::NoneExpr
        | Expr::Break | Expr::Continue => {}
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
struct CaptureInfo {
    captures: Vec<(String, TurboTy)>,
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
        Expr::Closure { params, return_type, body } => {
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
        Expr::FieldAssign { object, value, .. } => {
            extract_closures_from_expr(object, out, counter);
            extract_closures_from_expr(value, out, counter);
        }
        Expr::IndexAssign { object, index, value } => {
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
                    for arg in args { extract_spawn_sites_from_expr(arg, out, counter); }
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
            if let Some(tail) = tail_expr { extract_spawn_sites_from_expr(tail, out, counter); }
        }
        Expr::If { condition, then_branch, else_branch } => {
            extract_spawn_sites_from_expr(condition, out, counter);
            extract_spawn_sites_from_expr(then_branch, out, counter);
            if let Some(e) = else_branch { extract_spawn_sites_from_expr(e, out, counter); }
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
        Expr::UnaryOp { expr, .. } => { extract_spawn_sites_from_expr(expr, out, counter); }
        Expr::Call { callee, args } => {
            extract_spawn_sites_from_expr(callee, out, counter);
            for arg in args { extract_spawn_sites_from_expr(arg, out, counter); }
        }
        Expr::Assign { value, .. } | Expr::CompoundAssign { value, .. } => {
            extract_spawn_sites_from_expr(value, out, counter);
        }
        Expr::FieldAssign { object, value, .. } => {
            extract_spawn_sites_from_expr(object, out, counter);
            extract_spawn_sites_from_expr(value, out, counter);
        }
        Expr::IndexAssign { object, index, value } => {
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
        Expr::FieldAccess { object, .. } => { extract_spawn_sites_from_expr(object, out, counter); }
        Expr::ArrayLit(elems) => { for e in elems { extract_spawn_sites_from_expr(e, out, counter); } }
        Expr::StructLit { fields, .. } => { for (_, e) in fields { extract_spawn_sites_from_expr(e, out, counter); } }
        Expr::Match { subject, arms } => {
            extract_spawn_sites_from_expr(subject, out, counter);
            for arm in arms {
                if let Some(ref guard) = arm.guard {
                    extract_spawn_sites_from_expr(guard, out, counter);
                }
                extract_spawn_sites_from_expr(&arm.body, out, counter);
            }
        }
        Expr::Closure { body, .. } => { extract_spawn_sites_from_expr(body, out, counter); }
        Expr::OkExpr(e) | Expr::ErrExpr(e) | Expr::SomeExpr(e) | Expr::Await(e) | Expr::Try(e) => {
            extract_spawn_sites_from_expr(e, out, counter);
        }
        Expr::NullCoalesce { value, default } => {
            extract_spawn_sites_from_expr(value, out, counter);
            extract_spawn_sites_from_expr(default, out, counter);
        }
        Expr::Interpolation(parts) => {
            for part in parts {
                if let InterpolPart::Expr(e) = part { extract_spawn_sites_from_expr(e, out, counter); }
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
    declare_rt_fn(module, &mut rt_fns, "rt_assert_eq_fail", &[types::I64, ptr_type, ptr_type], None)?;
    declare_rt_fn(module, &mut rt_fns, "rt_div_by_zero", &[], None)?;
    declare_rt_fn(module, &mut rt_fns, "rt_int_overflow", &[], None)?;
    declare_rt_fn(module, &mut rt_fns, "rt_str_concat", &[ptr_type, ptr_type], Some(ptr_type))?;
    declare_rt_fn(module, &mut rt_fns, "rt_str_eq", &[ptr_type, ptr_type], Some(types::I8))?;
    declare_rt_fn(module, &mut rt_fns, "rt_array_alloc", &[types::I64], Some(ptr_type))?;
    declare_rt_fn(module, &mut rt_fns, "rt_array_get", &[ptr_type, types::I64], Some(types::I64))?;
    declare_rt_fn(module, &mut rt_fns, "rt_array_set", &[ptr_type, types::I64, types::I64], Some(ptr_type))?;
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
    declare_rt_fn(module, &mut rt_fns, "rt_option_some", &[types::I64], Some(ptr_type))?;
    declare_rt_fn(module, &mut rt_fns, "rt_option_none", &[], Some(ptr_type))?;
    declare_rt_fn(module, &mut rt_fns, "rt_option_tag", &[ptr_type], Some(types::I64))?;
    declare_rt_fn(module, &mut rt_fns, "rt_option_value", &[ptr_type], Some(types::I64))?;
    // Stdlib runtime declarations
    declare_rt_fn(module, &mut rt_fns, "rt_str_split", &[ptr_type, ptr_type], Some(ptr_type))?;
    declare_rt_fn(module, &mut rt_fns, "rt_str_trim", &[ptr_type], Some(ptr_type))?;
    declare_rt_fn(module, &mut rt_fns, "rt_str_upper", &[ptr_type], Some(ptr_type))?;
    declare_rt_fn(module, &mut rt_fns, "rt_str_lower", &[ptr_type], Some(ptr_type))?;
    declare_rt_fn(module, &mut rt_fns, "rt_str_starts_with", &[ptr_type, ptr_type], Some(types::I8))?;
    declare_rt_fn(module, &mut rt_fns, "rt_str_ends_with", &[ptr_type, ptr_type], Some(types::I8))?;
    declare_rt_fn(module, &mut rt_fns, "rt_str_replace", &[ptr_type, ptr_type, ptr_type], Some(ptr_type))?;
    declare_rt_fn(module, &mut rt_fns, "rt_str_char_at", &[ptr_type, types::I64], Some(ptr_type))?;
    declare_rt_fn(module, &mut rt_fns, "rt_str_contains", &[ptr_type, ptr_type], Some(types::I8))?;
    declare_rt_fn(module, &mut rt_fns, "rt_str_index_of", &[ptr_type, ptr_type], Some(types::I64))?;
    declare_rt_fn(module, &mut rt_fns, "rt_str_join", &[ptr_type, ptr_type], Some(ptr_type))?;
    declare_rt_fn(module, &mut rt_fns, "rt_str_repeat", &[ptr_type, types::I64], Some(ptr_type))?;
    declare_rt_fn(module, &mut rt_fns, "rt_read_line", &[], Some(ptr_type))?;
    declare_rt_fn(module, &mut rt_fns, "rt_read_file", &[ptr_type], Some(ptr_type))?;
    declare_rt_fn(module, &mut rt_fns, "rt_write_file", &[ptr_type, ptr_type], None)?;
    declare_rt_fn(module, &mut rt_fns, "rt_pow", &[types::I64, types::I64], Some(types::I64))?;
    declare_rt_fn(module, &mut rt_fns, "rt_sqrt", &[types::F64], Some(types::F64))?;
    // Async runtime declarations
    declare_rt_fn(module, &mut rt_fns, "rt_sleep_ms", &[types::I64], None)?;
    declare_rt_fn(module, &mut rt_fns, "rt_spawn_with_args", &[ptr_type, ptr_type], Some(ptr_type))?;
    declare_rt_fn(module, &mut rt_fns, "rt_await_handle", &[ptr_type], Some(types::I64))?;
    // HTTP + JSON runtime declarations
    declare_rt_fn(module, &mut rt_fns, "rt_http_get", &[ptr_type], Some(ptr_type))?;
    declare_rt_fn(module, &mut rt_fns, "rt_http_post", &[ptr_type, ptr_type], Some(ptr_type))?;
    declare_rt_fn(module, &mut rt_fns, "rt_json_get", &[ptr_type, ptr_type], Some(ptr_type))?;
    declare_rt_fn(module, &mut rt_fns, "rt_json_stringify", &[ptr_type, ptr_type], Some(ptr_type))?;
    // HTTP server runtime declarations
    declare_rt_fn(module, &mut rt_fns, "rt_http_server", &[types::I64], Some(types::I64))?;
    declare_rt_fn(module, &mut rt_fns, "rt_http_route", &[types::I64, ptr_type, ptr_type, ptr_type, ptr_type], None)?;
    declare_rt_fn(module, &mut rt_fns, "rt_http_listen", &[types::I64], None)?;
    declare_rt_fn(module, &mut rt_fns, "rt_respond", &[types::I64, ptr_type], Some(ptr_type))?;
    declare_rt_fn(module, &mut rt_fns, "rt_request_body", &[ptr_type], Some(ptr_type))?;
    // Channel runtime declarations
    declare_rt_fn(module, &mut rt_fns, "rt_channel_create", &[], Some(ptr_type))?;
    declare_rt_fn(module, &mut rt_fns, "rt_channel_send", &[ptr_type, types::I64], None)?;
    declare_rt_fn(module, &mut rt_fns, "rt_channel_recv", &[ptr_type], Some(types::I64))?;
    declare_rt_fn(module, &mut rt_fns, "rt_channel_clone_sender", &[ptr_type], Some(ptr_type))?;
    // Mutex runtime declarations
    declare_rt_fn(module, &mut rt_fns, "rt_mutex_create", &[types::I64], Some(ptr_type))?;
    declare_rt_fn(module, &mut rt_fns, "rt_mutex_get", &[ptr_type], Some(types::I64))?;
    declare_rt_fn(module, &mut rt_fns, "rt_mutex_set", &[ptr_type, types::I64], None)?;
    declare_rt_fn(module, &mut rt_fns, "rt_mutex_clone", &[ptr_type], Some(ptr_type))?;
    // HashMap runtime declarations
    declare_rt_fn(module, &mut rt_fns, "rt_hashmap_new", &[], Some(ptr_type))?;
    declare_rt_fn(module, &mut rt_fns, "rt_hashmap_set", &[ptr_type, ptr_type, ptr_type], None)?;
    declare_rt_fn(module, &mut rt_fns, "rt_hashmap_get", &[ptr_type, ptr_type], Some(ptr_type))?;
    declare_rt_fn(module, &mut rt_fns, "rt_hashmap_has", &[ptr_type, ptr_type], Some(types::I8))?;
    declare_rt_fn(module, &mut rt_fns, "rt_hashmap_len", &[ptr_type], Some(types::I64))?;
    declare_rt_fn(module, &mut rt_fns, "rt_hashmap_keys", &[ptr_type], Some(ptr_type))?;
    declare_rt_fn(module, &mut rt_fns, "rt_hashmap_remove", &[ptr_type, ptr_type], None)?;
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
                let field_tys: Vec<TurboTy> = v.fields.iter()
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

    // Build struct field layouts from AST
    let mut struct_fields: HashMap<String, Vec<(String, TurboTy)>> = HashMap::new();
    for item in &ast_module.items {
        let Item::Struct(s) = &item.node else { continue };
        let tp_names: Vec<String> = s.type_param_names();
        let fields: Vec<(String, TurboTy)> = s.fields.iter()
            .map(|f| (f.name.clone(), turbo_ty_from_type_expr_with_params(&f.ty.node, &enum_variants, &tp_names)))
            .collect();
        struct_fields.insert(s.name.clone(), fields);
    }

    // Build struct derives map from AST
    let mut struct_derives: HashMap<String, Vec<String>> = HashMap::new();
    for item in &ast_module.items {
        let Item::Struct(s) = &item.node else { continue };
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
                (agent.model.clone(), agent.tools.clone(), agent.system_prompt.clone()),
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
                trait_impls.entry(imp.type_name.clone()).or_default().push(trait_name.clone());
            }
        }
    }
    // Also register @derive(Display) as Display trait impl
    for item in &ast_module.items {
        if let Item::Struct(s) = &item.node {
            if s.derives.contains(&"Display".to_string()) {
                let already = trait_impls.get(&s.name)
                    .map_or(false, |impls| impls.contains(&"Display".to_string()));
                if !already {
                    trait_impls.entry(s.name.clone()).or_default().push("Display".to_string());
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
        let Item::Function(f) = &item.node else { continue };
        let mut sig = module.make_signature();
        // Use fast calling convention for internal functions (not main)
        // — reduces prologue/epilogue overhead on the hot recursive path
        if f.name != "main" {
            sig.call_conv = CallConv::Fast;
        }
        for param in &f.params {
            sig.params.push(AbiParam::new(resolve_cl_type(&param.ty.node, ptr_type, &enum_variants, &f.type_param_names())?));
        }
        let ret_turbo = if let Some(ret_ty) = &f.return_type {
            let cl = resolve_cl_type(&ret_ty.node, ptr_type, &enum_variants, &f.type_param_names())?;
            sig.returns.push(AbiParam::new(cl));
            turbo_ty_from_type_expr_with_params(&ret_ty.node, &enum_variants, &f.type_param_names())
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

    // Build fn_asts map for inline expansion and fn_type_params for generics
    let mut fn_asts: HashMap<String, &FnDef> = HashMap::new();
    let mut fn_type_params: HashMap<String, Vec<String>> = HashMap::new();
    for item in &ast_module.items {
        let Item::Function(f) = &item.node else { continue };
        fn_asts.insert(f.name.clone(), f);
        fn_type_params.insert(f.name.clone(), f.type_param_names());
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
                    sig.params.push(AbiParam::new(resolve_cl_type(&param.ty.node, ptr_type, &enum_variants, &[])?));
                }
            }

            let ret_turbo = if let Some(ret_ty) = &method.return_type {
                let cl = resolve_cl_type(&ret_ty.node, ptr_type, &enum_variants, &[])?;
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

    // Declare default trait methods for impl blocks that don't override them
    // Collect (type_name, method_sig) pairs for default methods that need compilation
    let mut default_method_impls: Vec<(String, &turbo_ast::TraitMethodSig)> = Vec::new();
    for item in &ast_module.items {
        let Item::Impl(imp) = &item.node else { continue };
        let Some(trait_name) = &imp.trait_name else { continue };
        let Some(trait_def) = trait_defs.get(trait_name.as_str()) else { continue };
        let impl_method_names: Vec<String> = imp.methods.iter().map(|m| m.node.name.clone()).collect();
        for trait_method in &trait_def.methods {
            if trait_method.default_body.is_some() && !impl_method_names.contains(&trait_method.name) {
                let mangled = format!("{}__{}", imp.type_name, trait_method.name);
                if !user_fns.contains_key(&mangled) {
                    let mut sig = module.make_signature();
                    sig.call_conv = CallConv::Fast;
                    for param in &trait_method.params {
                        if param.name == "self" {
                            sig.params.push(AbiParam::new(ptr_type));
                        } else {
                            sig.params.push(AbiParam::new(resolve_cl_type(&param.ty.node, ptr_type, &enum_variants, &[])?));
                        }
                    }
                    let ret_turbo = if let Some(ret_ty) = &trait_method.return_type {
                        let cl = resolve_cl_type(&ret_ty.node, ptr_type, &enum_variants, &[])?;
                        sig.returns.push(AbiParam::new(cl));
                        turbo_ty_from_type_expr(&ret_ty.node, &enum_variants)
                    } else {
                        TurboTy::Unit
                    };
                    let id = module.declare_function(&mangled, Linkage::Local, &sig)
                        .map_err(|e| CodegenError { message: e.to_string() })?;
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
        let Item::Struct(s) = &item.node else { continue };
        if s.derives.contains(&"Display".to_string()) {
            let mangled = format!("{}__{}", s.name, "to_string");
            if !user_fns.contains_key(&mangled) {
                let mut sig = module.make_signature();
                sig.call_conv = CallConv::Fast;
                sig.params.push(AbiParam::new(ptr_type)); // self
                sig.returns.push(AbiParam::new(ptr_type)); // returns str
                let id = module.declare_function(&mangled, Linkage::Local, &sig)
                    .map_err(|e| CodegenError { message: e.to_string() })?;
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
            sig.params.push(AbiParam::new(resolve_cl_type(&param.ty.node, ptr_type, &enum_variants, &[])?));
            param_turbo_tys.push(turbo_ty_from_type_expr(&param.ty.node, &enum_variants));
        }
        let ret_turbo = if let Some(ret_ty) = closure.return_type {
            let cl = resolve_cl_type(&ret_ty.node, ptr_type, &enum_variants, &[])?;
            sig.returns.push(AbiParam::new(cl));
            turbo_ty_from_type_expr(&ret_ty.node, &enum_variants)
        } else {
            let has_inferred_params = closure.params.iter().any(|p| matches!(p.ty.node, TypeExpr::Inferred));
            if has_inferred_params {
                sig.returns.push(AbiParam::new(types::I64));
                TurboTy::Int
            } else {
                TurboTy::Unit
            }
        };
        let id = module.declare_function(&closure.name, Linkage::Local, &sig)
            .map_err(|e| CodegenError { message: e.to_string() })?;
        user_fns.insert(closure.name.clone(), id);
        fn_ret_types.insert(closure.name.clone(), ret_turbo.clone());
        closure_fns_map.insert(
            closure.span_start,
            (closure.name.clone(), TurboTy::Fn(param_turbo_tys, Box::new(ret_turbo)), closure.free_vars.clone()),
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
        let id = module.declare_function(&site.thunk_name, Linkage::Local, &sig)
            .map_err(|e| CodegenError { message: e.to_string() })?;
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
        let Item::Function(f) = &item.node else { continue };
        let func_id = user_fns[&f.name];

        cl_ctx.func.signature = module.make_signature();
        if f.name != "main" {
            cl_ctx.func.signature.call_conv = CallConv::Fast;
        }
        for param in &f.params {
            cl_ctx.func.signature.params.push(AbiParam::new(resolve_cl_type(&param.ty.node, ptr_type, &enum_variants, &f.type_param_names())?));
        }
        if let Some(ret_ty) = &f.return_type {
            cl_ctx.func.signature.returns.push(AbiParam::new(resolve_cl_type(&ret_ty.node, ptr_type, &enum_variants, &f.type_param_names())?));
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
                let cl_ty = resolve_cl_type(&param.ty.node, ptr_type, &enum_variants, &f.type_param_names())?;
                let turbo_ty = turbo_ty_from_type_expr_with_params(&param.ty.node, &enum_variants, &f.type_param_names());
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
                    cl_ctx.func.signature.params.push(AbiParam::new(resolve_cl_type(&param.ty.node, ptr_type, &enum_variants, &[])?));
                }
            }
            if let Some(ret_ty) = &method.return_type {
                cl_ctx.func.signature.returns.push(AbiParam::new(resolve_cl_type(&ret_ty.node, ptr_type, &enum_variants, &[])?));
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

            module.define_function(func_id, &mut cl_ctx)
                .map_err(|e| CodegenError { message: e.to_string() })?;
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
                cl_ctx.func.signature.params.push(AbiParam::new(resolve_cl_type(&param.ty.node, ptr_type, &enum_variants, &[])?));
            }
        }
        if let Some(ret_ty) = &trait_method.return_type {
            cl_ctx.func.signature.returns.push(AbiParam::new(resolve_cl_type(&ret_ty.node, ptr_type, &enum_variants, &[])?));
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

        module.define_function(func_id, &mut cl_ctx)
            .map_err(|e| CodegenError { message: e.to_string() })?;
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
            let fields = struct_fields.get(struct_name.as_str()).cloned().unwrap_or_default();

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
                let raw_val = cx.builder.ins().load(types::I64, MemFlags::new(), self_val, offset);

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
                        let float_val = cx.builder.ins().bitcast(types::F64, MemFlags::new(), raw_val);
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

        module.define_function(func_id, &mut cl_ctx)
            .map_err(|e| CodegenError { message: e.to_string() })?;
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
            cl_ctx.func.signature.params.push(AbiParam::new(resolve_cl_type(&param.ty.node, ptr_type, &enum_variants, &[])?));
        }
        if let Some(ret_ty) = closure.return_type {
            cl_ctx.func.signature.returns.push(AbiParam::new(resolve_cl_type(&ret_ty.node, ptr_type, &enum_variants, &[])?));
        } else {
            // For closures with inferred params, add i64 return to match the declaration
            let has_inferred_params = closure.params.iter().any(|p| matches!(p.ty.node, TypeExpr::Inferred));
            if has_inferred_params {
                cl_ctx.func.signature.returns.push(AbiParam::new(types::I64));
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
                    let raw_val = cx.builder.ins().load(types::I64, MemFlags::new(), env_ptr_val, offset);
                    let val = match cap_tty {
                        TurboTy::Bool => cx.builder.ins().ireduce(types::I8, raw_val),
                        TurboTy::Float => cx.builder.ins().bitcast(types::F64, MemFlags::new(), raw_val),
                        _ => raw_val,
                    };
                    cx.builder.def_var(var, val);
                    cx.vars.insert(cap_name.clone(), (var, cl_ty, cap_tty.clone()));
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
                let has_inferred = closure.params.iter().any(|p| matches!(p.ty.node, TypeExpr::Inferred));
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

        module.define_function(func_id, &mut cl_ctx)
            .map_err(|e| CodegenError { message: e.to_string() })?;
        module.clear_context(&mut cl_ctx);
    }

    // Compile spawn thunk function bodies
    // Each thunk: loads fn_ptr + args from an args struct, calls the target, returns result
    for site in &spawn_sites {
        let func_id = user_fns[&site.thunk_name];

        cl_ctx.func.signature = module.make_signature();
        // Default (SystemV/C ABI) calling convention — callable from rt_spawn_thunk
        cl_ctx.func.signature.params.push(AbiParam::new(ptr_type)); // args_struct_ptr
        cl_ctx.func.signature.returns.push(AbiParam::new(types::I64));

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
                let val = builder.ins().load(types::I64, MemFlags::new(), args_ptr, offset);
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
                let has_return = fn_ret_types.get(&site.callee_name)
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

        module.define_function(func_id, &mut cl_ctx)
            .map_err(|e| CodegenError { message: e.to_string() })?;
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
    let id = module.declare_function(name, Linkage::Import, &sig)
        .map_err(|e| CodegenError { message: e.to_string() })?;
    rt_fns.insert(name.to_string(), id);
    Ok(())
}

fn resolve_cl_type(ty: &TypeExpr, ptr_type: types::Type, enum_variants: &HashMap<String, Vec<String>>, type_params: &[String]) -> Result<types::Type, CodegenError> {
    resolve_cl_type_inner(ty, ptr_type, enum_variants, type_params, &HashMap::new())
}

/// Resolve Cranelift type, accounting for data-carrying enums that need pointer types.
#[allow(dead_code)]
fn resolve_cl_type_with_data(ty: &TypeExpr, ptr_type: types::Type, enum_variants: &HashMap<String, Vec<String>>, type_params: &[String], enum_max_slots: &HashMap<String, usize>) -> Result<types::Type, CodegenError> {
    resolve_cl_type_inner(ty, ptr_type, enum_variants, type_params, enum_max_slots)
}

fn resolve_cl_type_inner(ty: &TypeExpr, ptr_type: types::Type, enum_variants: &HashMap<String, Vec<String>>, type_params: &[String], enum_max_slots: &HashMap<String, usize>) -> Result<types::Type, CodegenError> {
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
        },
        TypeExpr::Unit => Err(CodegenError { message: "unit type has no runtime representation".to_string() }),
        TypeExpr::Array(_) => Ok(ptr_type), // Arrays are represented as pointers at runtime
        TypeExpr::FnType { .. } => Ok(ptr_type), // Function pointers are pointers
        TypeExpr::Result { .. } => Ok(ptr_type), // Result types are heap-allocated tagged unions
        TypeExpr::Optional(_) => Ok(ptr_type), // Optional types are heap-allocated tagged unions
        // Sprint 9: Future<T> compiles identically to T
        TypeExpr::Future(inner) => resolve_cl_type_inner(&inner.node, ptr_type, enum_variants, type_params, enum_max_slots),
        #[allow(unreachable_patterns)] _ => Ok(types::I64),
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
            // Check if this is a module-level constant — inline the value
            if let Some(const_expr) = cx.constants.get(name.as_str()) {
                let const_expr = const_expr.clone();
                return compile_expr(cx, &const_expr);
            }
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

            // String coercion: str + non-str or non-str + str
            if *op == BinOp::Add {
                if lhs_tty == TurboTy::Str && rhs_tty != TurboTy::Str {
                    let rhs_str = convert_to_str(cx, rhs, &rhs_tty)?;
                    return compile_str_concat(cx, lhs, rhs_str);
                }
                if rhs_tty == TurboTy::Str && lhs_tty != TurboTy::Str {
                    let lhs_str = convert_to_str(cx, lhs, &lhs_tty)?;
                    return compile_str_concat(cx, lhs_str, rhs);
                }
            }

            // Struct field-by-field equality comparison (@derive(Eq))
            if let TurboTy::Struct(ref struct_name) = lhs_tty {
                if matches!(op, BinOp::Eq | BinOp::NotEq) {
                    return compile_struct_eq(cx, lhs, rhs, struct_name, *op);
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
            let (var, _, _) = cx.vars.get(target)
                .ok_or_else(|| CodegenError { message: format!("undefined variable: {target}") })?;
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
            let (var, _, _) = cx.vars.get(target)
                .ok_or_else(|| CodegenError { message: format!("undefined variable: {target}") })?;
            let var = *var;
            let lhs = cx.builder.use_var(var);
            let result = compile_binop(cx, lhs, *op, rhs)?;
            cx.builder.def_var(var, result);
            Ok(None)
        }

        Expr::FieldAssign { object, field, value } => {
            let (obj_ptr, obj_tty) = compile_expr(cx, object)?.unwrap();
            let (val, _) = compile_expr(cx, value)?.unwrap();

            let struct_name = match &obj_tty {
                TurboTy::Struct(name) => name.clone(),
                _ => return Err(CodegenError { message: "field assignment on non-struct type".to_string() }),
            };

            let struct_layout = cx.struct_fields.get(&struct_name)
                .ok_or_else(|| CodegenError { message: format!("undefined struct: {struct_name}") })?
                .clone();

            let field_index = struct_layout.iter()
                .position(|(n, _)| n == field)
                .ok_or_else(|| CodegenError { message: format!("struct `{struct_name}` has no field `{field}`") })?;

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

            cx.builder.ins().store(MemFlags::new(), val, obj_ptr, offset);
            Ok(None)
        }

        Expr::IndexAssign { object, index, value } => {
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
                cx.builder.ins().bitcast(types::I64, MemFlags::new(), extended)
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
                        let inner_ret_tty = cx.fn_ret_types.get(callee_name.as_str())
                            .cloned().unwrap_or(TurboTy::Unit);

                        // Get the target function's address
                        let target_fid = *cx.user_fns.get(callee_name.as_str())
                            .ok_or_else(|| CodegenError { message: format!("spawn: unknown function `{}`", callee_name) })?;
                        let target_fref = cx.module.declare_func_in_func(target_fid, cx.builder.func);
                        let target_fn_ptr = cx.builder.ins().func_addr(cx.ptr_type, target_fref);

                        // Compile all arguments
                        let mut arg_vals = Vec::new();
                        for arg in args {
                            if let Some((val, tty)) = compile_expr(cx, arg)? {
                                let val = match tty {
                                    TurboTy::Bool => cx.builder.ins().sextend(types::I64, val),
                                    TurboTy::Float => cx.builder.ins().bitcast(types::I64, MemFlags::new(), val),
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
                        cx.builder.ins().store(MemFlags::new(), target_fn_ptr, args_ptr, 0);
                        // Store args at offsets 8, 16, 24, ...
                        for (i, val) in arg_vals.iter().enumerate() {
                            let offset = ((i + 1) * 8) as i32;
                            cx.builder.ins().store(MemFlags::new(), *val, args_ptr, offset);
                        }

                        // Get the thunk function address
                        let thunk_fid = *cx.user_fns.get(thunk_name.as_str())
                            .ok_or_else(|| CodegenError { message: format!("spawn: thunk `{}` not found", thunk_name) })?;
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

        Expr::ForIn { var_name, iterable, body } => compile_for_in(cx, var_name, iterable, body),

        Expr::Range { .. } => {
            Err(CodegenError { message: "range expressions can only be used in for-in loops".to_string() })
        }

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
                let arr_alloc_ref = cx.module.declare_func_in_func(arr_alloc_fid, cx.builder.func);
                let arr_call = cx.builder.ins().call(arr_alloc_ref, &[tools_len_val]);
                let arr_ptr = cx.builder.inst_results(arr_call)[0];
                for (i, tool_name) in tools.iter().enumerate() {
                    let tool_str = cx.create_string(tool_name)?;
                    let offset = cx.builder.ins().iconst(cx.ptr_type, (8 + i * 8) as i64);
                    let elem_ptr = cx.builder.ins().iadd(arr_ptr, offset);
                    cx.builder.ins().store(MemFlags::new(), tool_str, elem_ptr, 0);
                }
                cx.builder.ins().store(MemFlags::new(), arr_ptr, ptr, 16);

                return Ok(Some((ptr, TurboTy::Agent(name.clone()))));
            }

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

            // Track concrete field types for generic structs
            let mut concrete_fields: Vec<(String, TurboTy)> = Vec::new();

            // Store each field at its offset
            for (field_name, field_value) in fields {
                let field_index = struct_layout.iter()
                    .position(|(n, _)| n == field_name)
                    .ok_or_else(|| CodegenError { message: format!("struct `{name}` has no field `{field_name}`") })?;

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
                    cx.builder.ins().bitcast(types::I64, MemFlags::new(), extended)
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
                    let index = variants.iter().position(|v| v == field)
                        .ok_or_else(|| CodegenError { message: format!("enum `{name}` has no variant `{field}`") })?;

                    // Check if this is a data-carrying enum
                    if let Some(&max_slots) = cx.enum_max_slots.get(name.as_str()) {
                        // Allocate tagged union: [tag][slot0][slot1]...[slotN]
                        let total_slots = 1 + max_slots; // tag + payload
                        let num_fields_val = cx.builder.ins().iconst(types::I64, total_slots as i64);
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
                    _ => return Err(CodegenError { message: format!("agent has no field `{field}`") }),
                };
                let val = cx.builder.ins().load(types::I64, MemFlags::new(), obj_ptr, offset);
                return Ok(Some((val, tty)));
            }

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

            let mut field_tty = struct_layout[field_index].1.clone();

            // For generic structs, check if we have concrete field type overrides
            if let Expr::Ident(ref var_name) = object.node {
                if let Some(concrete_fields) = cx.generic_struct_field_overrides.get(var_name) {
                    if let Some((_, concrete_tty)) = concrete_fields.iter().find(|(n, _)| n == field) {
                        field_tty = concrete_tty.clone();
                    }
                }
            }

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
            let (closure_name, closure_ty, _free_vars) = cx.closure_fns.get(&span_start)
                .ok_or_else(|| CodegenError { message: "internal error: closure not found in pre-compiled map".to_string() })?;
            let closure_ty = closure_ty.clone();
            let closure_name = closure_name.clone();
            let func_id = *cx.user_fns.get(closure_name.as_str())
                .ok_or_else(|| CodegenError { message: format!("internal error: closure function {} not found", closure_name) })?;
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
            cx.closure_captures.insert(span_start, CaptureInfo { captures: captures.clone() });

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
                    let (var, _cl_ty, _turbo_ty) = cx.vars.get(cap_name)
                        .ok_or_else(|| CodegenError { message: format!("internal error: capture variable {} not found", cap_name) })?;
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
                        cx.builder.ins().bitcast(types::I64, MemFlags::new(), extended)
                    } else {
                        val
                    };
                    cx.builder.ins().store(MemFlags::new(), val, env_ptr, offset);
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
            cx.builder.ins().store(MemFlags::new(), fn_ptr, closure_ptr, 0);
            // Store env_ptr at offset 8
            cx.builder.ins().store(MemFlags::new(), env_ptr, closure_ptr, 8);

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

            cx.builder.ins().brif(is_some, some_block, &[], none_block, &[]);

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
                cx.builder.ins().bitcast(types::I64, MemFlags::new(), def_val)
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

        Expr::Break | Expr::Continue => {
            // TODO: break/continue codegen (requires loop context tracking)
            Ok(None)
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
                cx.generic_struct_field_overrides.insert(name.clone(), concrete_fields);
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
                        TurboTy::Array(_) | TurboTy::Struct(_) | TurboTy::Result(_, _) | TurboTy::Optional(_)
                    );
                    if needs_retain {
                        let retain_fid = cx.rt_fns["rt_retain"];
                        let retain_ref = cx.module.declare_func_in_func(retain_fid, cx.builder.func);
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
    let lhs_bool = cx.to_bool(lhs);

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
        "assert_eq" => compile_assert_eq(cx, args, false),
        "assert_ne" => compile_assert_eq(cx, args, true),
        "len" => compile_len(cx, args),
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
                                let num_fields_val = cx.builder.ins().iconst(types::I64, total_slots as i64);
                                let alloc_fid = cx.rt_fns["rt_struct_alloc"];
                                let alloc_fref = cx.module.declare_func_in_func(alloc_fid, cx.builder.func);
                                let call = cx.builder.ins().call(alloc_fref, &[num_fields_val]);
                                let ptr = cx.builder.inst_results(call)[0];

                                // Store tag at offset 0
                                let tag_val = cx.builder.ins().iconst(types::I64, variant_index as i64);
                                cx.builder.ins().store(MemFlags::new(), tag_val, ptr, 0);

                                // Get the field types for this variant
                                let _field_tys = cx.enum_variant_fields
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
                                        cx.builder.ins().bitcast(types::I64, MemFlags::new(), extended)
                                    } else if val_ty.bits() < 64 && val_ty.is_int() {
                                        cx.builder.ins().sextend(types::I64, val)
                                    } else {
                                        val
                                    };

                                    cx.builder.ins().store(MemFlags::new(), store_val, ptr, offset);
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
                    // Closure is a pair struct: [fn_ptr, env_ptr]
                    let closure_ptr = cx.builder.use_var(var);
                    let fn_ptr = cx.builder.ins().load(cx.ptr_type, MemFlags::new(), closure_ptr, 0);
                    let env_ptr = cx.builder.ins().load(cx.ptr_type, MemFlags::new(), closure_ptr, 8);

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
            }

            let func_id = *cx.user_fns.get(name.as_str())
                .ok_or_else(|| CodegenError { message: format!("undefined function: {name}") })?;

            let ret_tty = cx.fn_ret_types.get(name.as_str()).cloned().unwrap_or(TurboTy::Unit);
            let ret_is_result = matches!(&ret_tty, TurboTy::Result(_, _));
            let type_params = cx.fn_type_params.get(name.as_str()).cloned().unwrap_or_default();

            let func_ref = cx.module.declare_func_in_func(func_id, cx.builder.func);
            let sig = cx.builder.func.dfg.ext_funcs[func_ref].signature;
            let param_types: Vec<types::Type> = cx.builder.func.dfg.signatures[sig]
                .params.iter().map(|p| p.value_type).collect();
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
                    if !has_return(&callee_def.body.node) && callee_def.params.len() == arg_values.len() {
                        // Save and restore outer variable scope so inlined
                        // parameter bindings don't leak out.
                        let saved_vars = cx.vars.clone();
                        let saved_depth = cx.inline_depth;
                        cx.inline_depth += 1;

                        // Bind each parameter to the already-compiled argument value.
                        for (i, param) in callee_def.params.iter().enumerate() {
                            let cl_ty = resolve_cl_type(&param.ty.node, cx.ptr_type, cx.enum_variants, &type_params)?;
                            let turbo_ty = turbo_ty_from_type_expr(&param.ty.node, cx.enum_variants);
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
            TurboTy::Bool => {
                let ty = cx.builder.func.dfg.value_type(v);
                let v = if ty.bits() > 8 {
                    cx.builder.ins().ireduce(types::I8, v)
                } else {
                    v
                };
                cx.rt_call("rt_print_bool", &[v]);
            }
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
            TurboTy::Enum(ref enum_name) => {
                // For data enums, extract the tag from the tagged union pointer; for unit enums, use the value directly
                let tag_val = if cx.enum_max_slots.contains_key(enum_name.as_str()) {
                    // Data enum: load tag from ptr[0]
                    cx.builder.ins().load(types::I64, MemFlags::new(), v, 0)
                } else {
                    let v = if cx.builder.func.dfg.value_type(v).bits() < 64 {
                        cx.builder.ins().sextend(types::I64, v)
                    } else { v };
                    v
                };
                cx.rt_call("rt_print_i64", &[tag_val]);
            }
            TurboTy::Array(_) => {
                let ptr = cx.create_string("[array]")?;
                cx.rt_call("rt_print_str", &[ptr]);
            }
            TurboTy::Struct(ref name) => {
                // Check if struct implements Display trait
                let has_display = cx.trait_impls.get(name)
                    .map_or(false, |traits| traits.contains(&"Display".to_string()));
                if has_display {
                    let mangled = format!("{name}__to_string");
                    if let Some(&fid) = cx.user_fns.get(&mangled) {
                        let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
                        let call = cx.builder.ins().call(fref, &[v]);
                        let str_val = cx.builder.inst_results(call)[0];
                        cx.rt_call("rt_print_str", &[str_val]);
                    } else {
                        let ptr = cx.create_string(&format!("[struct {}]", name))?;
                        cx.rt_call("rt_print_str", &[ptr]);
                    }
                } else {
                    let ptr = cx.create_string(&format!("[struct {}]", name))?;
                    cx.rt_call("rt_print_str", &[ptr]);
                }
            }
            TurboTy::Fn(_, _) => {
                let ptr = cx.create_string("[function]")?;
                cx.rt_call("rt_print_str", &[ptr]);
            }
            TurboTy::Result(_, _) => {
                let ptr = cx.create_string("[result]")?;
                cx.rt_call("rt_print_str", &[ptr]);
            }
            TurboTy::Optional(_) => {
                let ptr = cx.create_string("[optional]")?;
                cx.rt_call("rt_print_str", &[ptr]);
            }
            TurboTy::Agent(ref name) => {
                let ptr = cx.create_string(&format!("[agent {}]", name))?;
                cx.rt_call("rt_print_str", &[ptr]);
            }
            TurboTy::Future(_) => {
                let ptr = cx.create_string("[future]")?;
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
    let cond_bool = cx.to_bool(cond);

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

fn compile_assert_eq<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>], is_ne: bool) -> Result<MaybeTyped, CodegenError> {
    if args.len() != 2 {
        let name = if is_ne { "assert_ne" } else { "assert_eq" };
        return Err(CodegenError { message: format!("{name}() requires exactly 2 arguments") });
    }

    let (left_val, left_tty) = compile_expr(cx, &args[0])?.unwrap();
    let (right_val, right_tty) = compile_expr(cx, &args[1])?.unwrap();

    // Compare based on type
    let cond = match &left_tty {
        TurboTy::Str => {
            let fid = cx.rt_fns["rt_str_eq"];
            let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
            let call = cx.builder.ins().call(fref, &[left_val, right_val]);
            cx.builder.inst_results(call)[0]
        }
        TurboTy::Float => {
            cx.builder.ins().fcmp(FloatCC::Equal, left_val, right_val)
        }
        TurboTy::Bool => {
            cx.builder.ins().icmp(IntCC::Equal, left_val, right_val)
        }
        _ => {
            // For Int, Enum (unit), etc: i64 comparison
            let lv = {
                let ty = cx.builder.func.dfg.value_type(left_val);
                if ty.bits() < 64 {
                    cx.builder.ins().sextend(types::I64, left_val)
                } else {
                    left_val
                }
            };
            let rv = {
                let ty = cx.builder.func.dfg.value_type(right_val);
                if ty.bits() < 64 {
                    cx.builder.ins().sextend(types::I64, right_val)
                } else {
                    right_val
                }
            };
            cx.builder.ins().icmp(IntCC::Equal, lv, rv)
        }
    };

    let fail_block = cx.builder.create_block();
    let ok_block = cx.builder.create_block();

    if is_ne {
        // assert_ne: fail if equal (cond == true)
        cx.builder.ins().brif(cond, fail_block, &[], ok_block, &[]);
    } else {
        // assert_eq: fail if not equal (cond == false)
        cx.builder.ins().brif(cond, ok_block, &[], fail_block, &[]);
    }

    cx.builder.switch_to_block(fail_block);
    cx.builder.seal_block(fail_block);

    // Convert both values to string for error message
    let left_str = convert_to_str(cx, left_val, &left_tty)?;
    let right_str = convert_to_str(cx, right_val, &right_tty)?;

    let kind_val = cx.builder.ins().iconst(types::I64, if is_ne { 1 } else { 0 });

    let fid = cx.rt_fns["rt_assert_eq_fail"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    cx.builder.ins().call(fref, &[kind_val, left_str, right_str]);
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

// ── Stdlib builtins ─────────────────────────────────────────────────

/// split(s, sep) -> [str] — calls rt_str_split, returns Array(Str)
fn compile_stdlib_split<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>]) -> Result<MaybeTyped, CodegenError> {
    let (s_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let (sep_val, _) = compile_expr(cx, &args[1])?.unwrap();
    let fid = cx.rt_fns["rt_str_split"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[s_val, sep_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Array(Box::new(TurboTy::Str)))))
}

/// Generic helper for str->str builtins (trim, upper, lower)
fn compile_stdlib_str1<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>], rt_name: &str) -> Result<MaybeTyped, CodegenError> {
    let (s_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let fid = cx.rt_fns[rt_name];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[s_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Str)))
}

/// Generic helper for (str, str)->bool builtins (starts_with, ends_with)
fn compile_stdlib_str_bool2<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>], rt_name: &str) -> Result<MaybeTyped, CodegenError> {
    let (s_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let (other_val, _) = compile_expr(cx, &args[1])?.unwrap();
    let fid = cx.rt_fns[rt_name];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[s_val, other_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Bool)))
}

/// replace(s, from, to) -> str
fn compile_stdlib_replace<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>]) -> Result<MaybeTyped, CodegenError> {
    let (s_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let (from_val, _) = compile_expr(cx, &args[1])?.unwrap();
    let (to_val, _) = compile_expr(cx, &args[2])?.unwrap();
    let fid = cx.rt_fns["rt_str_replace"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[s_val, from_val, to_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Str)))
}

/// char_at(s, index) -> str
fn compile_stdlib_char_at<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>]) -> Result<MaybeTyped, CodegenError> {
    let (s_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let (idx_val, _) = compile_expr(cx, &args[1])?.unwrap();
    // Ensure index is i64
    let idx_ty = cx.builder.func.dfg.value_type(idx_val);
    let idx_val = if idx_ty.bits() < 64 {
        cx.builder.ins().sextend(types::I64, idx_val)
    } else {
        idx_val
    };
    let fid = cx.rt_fns["rt_str_char_at"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[s_val, idx_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Str)))
}

/// index_of(s, sub) -> i64
fn compile_stdlib_index_of<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>]) -> Result<MaybeTyped, CodegenError> {
    let (s_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let (sub_val, _) = compile_expr(cx, &args[1])?.unwrap();
    let fid = cx.rt_fns["rt_str_index_of"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[s_val, sub_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Int)))
}

/// join(arr, sep) -> str
fn compile_stdlib_join<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>]) -> Result<MaybeTyped, CodegenError> {
    let (arr_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let (sep_val, _) = compile_expr(cx, &args[1])?.unwrap();
    let fid = cx.rt_fns["rt_str_join"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[arr_val, sep_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Str)))
}

/// repeat(s, n) -> str
fn compile_stdlib_repeat<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>]) -> Result<MaybeTyped, CodegenError> {
    let (s_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let (n_val, _) = compile_expr(cx, &args[1])?.unwrap();
    // Ensure n is i64
    let n_ty = cx.builder.func.dfg.value_type(n_val);
    let n_val = if n_ty.bits() < 64 {
        cx.builder.ins().sextend(types::I64, n_val)
    } else {
        n_val
    };
    let fid = cx.rt_fns["rt_str_repeat"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[s_val, n_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Str)))
}

/// read_line() -> str
fn compile_stdlib_read_line<M: Module>(cx: &mut Ctx<'_, M>) -> Result<MaybeTyped, CodegenError> {
    let fid = cx.rt_fns["rt_read_line"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Str)))
}

/// read_file(path) -> str
fn compile_stdlib_read_file<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>]) -> Result<MaybeTyped, CodegenError> {
    let (path_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let fid = cx.rt_fns["rt_read_file"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[path_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Str)))
}

/// write_file(path, content) -> ()
fn compile_stdlib_write_file<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>]) -> Result<MaybeTyped, CodegenError> {
    let (path_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let (content_val, _) = compile_expr(cx, &args[1])?.unwrap();
    let fid = cx.rt_fns["rt_write_file"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    cx.builder.ins().call(fref, &[path_val, content_val]);
    Ok(None)
}

/// pow(base, exp) -> i64
fn compile_stdlib_pow<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>]) -> Result<MaybeTyped, CodegenError> {
    let (base_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let (exp_val, _) = compile_expr(cx, &args[1])?.unwrap();
    // Ensure both are i64
    let base_ty = cx.builder.func.dfg.value_type(base_val);
    let base_val = if base_ty.bits() < 64 { cx.builder.ins().sextend(types::I64, base_val) } else { base_val };
    let exp_ty = cx.builder.func.dfg.value_type(exp_val);
    let exp_val = if exp_ty.bits() < 64 { cx.builder.ins().sextend(types::I64, exp_val) } else { exp_val };
    let fid = cx.rt_fns["rt_pow"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[base_val, exp_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Int)))
}

/// sqrt(x) -> f64
fn compile_stdlib_sqrt<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>]) -> Result<MaybeTyped, CodegenError> {
    let (x_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let fid = cx.rt_fns["rt_sqrt"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[x_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Float)))
}

/// sleep(ms) -> () — sleep the current thread for ms milliseconds
fn compile_builtin_sleep<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>]) -> Result<MaybeTyped, CodegenError> {
    let (ms_val, _) = compile_expr(cx, &args[0])?.unwrap();
    // Ensure it's i64
    let ms_ty = cx.builder.func.dfg.value_type(ms_val);
    let ms_val = if ms_ty.bits() < 64 { cx.builder.ins().sextend(types::I64, ms_val) } else { ms_val };
    let fid = cx.rt_fns["rt_sleep_ms"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    cx.builder.ins().call(fref, &[ms_val]);
    Ok(None)
}

// ── HTTP + JSON builtins ────────────────────────────────────────────

/// http_get(url) -> str
fn compile_builtin_http_get<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>]) -> Result<MaybeTyped, CodegenError> {
    let (url_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let fid = cx.rt_fns["rt_http_get"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[url_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Str)))
}

/// http_post(url, body) -> str
fn compile_builtin_http_post<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>]) -> Result<MaybeTyped, CodegenError> {
    let (url_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let (body_val, _) = compile_expr(cx, &args[1])?.unwrap();
    let fid = cx.rt_fns["rt_http_post"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[url_val, body_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Str)))
}

/// json_get(json_str, key) -> str
fn compile_builtin_json_get<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>]) -> Result<MaybeTyped, CodegenError> {
    let (json_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let (key_val, _) = compile_expr(cx, &args[1])?.unwrap();
    let fid = cx.rt_fns["rt_json_get"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[json_val, key_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Str)))
}

/// json_stringify(key, value) -> str
fn compile_builtin_json_stringify<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>]) -> Result<MaybeTyped, CodegenError> {
    let (key_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let (value_val, _) = compile_expr(cx, &args[1])?.unwrap();
    let fid = cx.rt_fns["rt_json_stringify"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[key_val, value_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Str)))
}

// ── HTTP server builtins ────────────────────────────────────────────

/// http_server(port) -> i64 (server id)
fn compile_builtin_http_server<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>]) -> Result<MaybeTyped, CodegenError> {
    let (port_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let fid = cx.rt_fns["rt_http_server"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[port_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Int)))
}

/// route(server_id, method, path, handler_closure)
/// Extracts fn_ptr and env_ptr from the closure pair and passes to rt_http_route.
fn compile_builtin_route<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>]) -> Result<MaybeTyped, CodegenError> {
    let (server_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let (method_val, _) = compile_expr(cx, &args[1])?.unwrap();
    let (path_val, _) = compile_expr(cx, &args[2])?.unwrap();
    let (closure_ptr, _) = compile_expr(cx, &args[3])?.unwrap();

    // Extract fn_ptr and env_ptr from the closure pair struct (offset 0 = fn_ptr, offset 8 = env_ptr)
    let fn_ptr = cx.builder.ins().load(cx.ptr_type, MemFlags::new(), closure_ptr, 0);
    let env_ptr = cx.builder.ins().load(cx.ptr_type, MemFlags::new(), closure_ptr, 8);

    let fid = cx.rt_fns["rt_http_route"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    cx.builder.ins().call(fref, &[server_val, method_val, path_val, fn_ptr, env_ptr]);
    Ok(None)
}

/// http_listen(server_id) -> () — starts the server, blocks forever
fn compile_builtin_http_listen<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>]) -> Result<MaybeTyped, CodegenError> {
    let (server_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let fid = cx.rt_fns["rt_http_listen"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    cx.builder.ins().call(fref, &[server_val]);
    Ok(None)
}

/// respond(status, body) -> str — builds "STATUS:BODY" format response
fn compile_builtin_respond<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>]) -> Result<MaybeTyped, CodegenError> {
    let (status_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let (body_val, _) = compile_expr(cx, &args[1])?.unwrap();
    let fid = cx.rt_fns["rt_respond"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[status_val, body_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Str)))
}

/// request_body(req) -> str — extracts body from request (identity for now)
fn compile_builtin_request_body<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>]) -> Result<MaybeTyped, CodegenError> {
    let (req_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let fid = cx.rt_fns["rt_request_body"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[req_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Str)))
}

// ── to_json / to_json_array builtins ────────────────────────────────

/// to_json(val) -> str — serialize a struct to a JSON string at codegen time
/// Uses struct field layout to generate field-by-field concatenation.
fn compile_builtin_to_json<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>]) -> Result<MaybeTyped, CodegenError> {
    let (val, tty) = compile_expr(cx, &args[0])?.unwrap();

    if let TurboTy::Struct(ref struct_name) = tty {
        compile_struct_to_json(cx, val, struct_name)
    } else {
        // For non-structs, just convert to string
        let str_val = convert_to_str(cx, val, &tty)?;
        Ok(Some((str_val, TurboTy::Str)))
    }
}

/// Generate JSON string from a struct pointer: {"field1":val1,"field2":val2,...}
fn compile_struct_to_json<M: Module>(
    cx: &mut Ctx<'_, M>,
    struct_ptr: Value,
    struct_name: &str,
) -> Result<MaybeTyped, CodegenError> {
    let struct_layout = cx.struct_fields.get(struct_name)
        .ok_or_else(|| CodegenError { message: format!("undefined struct: {struct_name}") })?
        .clone();

    let concat_fid = cx.rt_fns["rt_str_concat"];

    // Start with "{"
    let mut result = cx.create_string("{")?;

    for (i, (field_name, field_ty)) in struct_layout.iter().enumerate() {
        // Add comma separator between fields (and the key)
        let prefix = if i > 0 {
            format!(",\"{}\":", field_name)
        } else {
            format!("\"{}\":", field_name)
        };
        let prefix_str = cx.create_string(&prefix)?;
        let concat_ref = cx.module.declare_func_in_func(concat_fid, cx.builder.func);
        let call = cx.builder.ins().call(concat_ref, &[result, prefix_str]);
        result = cx.builder.inst_results(call)[0];

        // Load field value from struct
        let offset = (i * 8) as i32;
        let raw_val = cx.builder.ins().load(types::I64, MemFlags::new(), struct_ptr, offset);

        // For string fields, wrap the value in quotes; for numeric/bool, emit raw
        let field_json_str = match field_ty {
            TurboTy::Str => {
                let quote_str = cx.create_string("\"")?;
                let concat_ref = cx.module.declare_func_in_func(concat_fid, cx.builder.func);
                let call = cx.builder.ins().call(concat_ref, &[quote_str, raw_val]);
                let with_open_quote = cx.builder.inst_results(call)[0];
                let quote_str2 = cx.create_string("\"")?;
                let concat_ref2 = cx.module.declare_func_in_func(concat_fid, cx.builder.func);
                let call2 = cx.builder.ins().call(concat_ref2, &[with_open_quote, quote_str2]);
                cx.builder.inst_results(call2)[0]
            }
            TurboTy::Int => {
                let fid = cx.rt_fns["rt_i64_to_str"];
                let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
                let call = cx.builder.ins().call(fref, &[raw_val]);
                cx.builder.inst_results(call)[0]
            }
            TurboTy::Bool => {
                let bool_val = cx.builder.ins().ireduce(types::I8, raw_val);
                let fid = cx.rt_fns["rt_bool_to_str"];
                let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
                let call = cx.builder.ins().call(fref, &[bool_val]);
                cx.builder.inst_results(call)[0]
            }
            TurboTy::Float => {
                let float_val = cx.builder.ins().bitcast(types::F64, MemFlags::new(), raw_val);
                let fid = cx.rt_fns["rt_f64_to_str"];
                let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
                let call = cx.builder.ins().call(fref, &[float_val]);
                cx.builder.inst_results(call)[0]
            }
            _ => {
                convert_to_str(cx, raw_val, field_ty)?
            }
        };

        // Concat the field value
        let concat_ref = cx.module.declare_func_in_func(concat_fid, cx.builder.func);
        let call = cx.builder.ins().call(concat_ref, &[result, field_json_str]);
        result = cx.builder.inst_results(call)[0];
    }

    // Close with "}"
    let suffix = cx.create_string("}")?;
    let concat_ref = cx.module.declare_func_in_func(concat_fid, cx.builder.func);
    let call = cx.builder.ins().call(concat_ref, &[result, suffix]);
    result = cx.builder.inst_results(call)[0];

    Ok(Some((result, TurboTy::Str)))
}

/// to_json_array(arr) -> str — serialize an array of structs to JSON array string
/// Generates [item1,item2,...] by iterating and calling to_json on each element.
fn compile_builtin_to_json_array<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>]) -> Result<MaybeTyped, CodegenError> {
    let (arr_ptr, arr_tty) = compile_expr(cx, &args[0])?.unwrap();

    let elem_tty = match &arr_tty {
        TurboTy::Array(inner) => *inner.clone(),
        _ => return Err(CodegenError { message: "to_json_array() argument must be an array".to_string() }),
    };

    let struct_name = match &elem_tty {
        TurboTy::Struct(name) => name.clone(),
        _ => return Err(CodegenError { message: "to_json_array() requires an array of structs".to_string() }),
    };

    let concat_fid = cx.rt_fns["rt_str_concat"];

    // Get array length
    let len_fid = cx.rt_fns["rt_array_len"];
    let len_ref = cx.module.declare_func_in_func(len_fid, cx.builder.func);
    let len_call = cx.builder.ins().call(len_ref, &[arr_ptr]);
    let arr_len = cx.builder.inst_results(len_call)[0];

    // Start with "["
    let open_bracket = cx.create_string("[")?;

    // result_var accumulates the JSON string; idx_var is the loop counter
    let result_var = cx.fresh_var(cx.ptr_type, TurboTy::Str);
    cx.builder.def_var(result_var, open_bracket);

    let idx_var = cx.fresh_var(types::I64, TurboTy::Int);
    let zero = cx.builder.ins().iconst(types::I64, 0);
    cx.builder.def_var(idx_var, zero);

    let header_block = cx.builder.create_block();
    let body_block = cx.builder.create_block();
    let exit_block = cx.builder.create_block();

    cx.builder.ins().jump(header_block, &[]);

    // Header: check idx < len
    cx.builder.switch_to_block(header_block);
    let idx = cx.builder.use_var(idx_var);
    let cond = cx.builder.ins().icmp(IntCC::SignedLessThan, idx, arr_len);
    cx.builder.ins().brif(cond, body_block, &[], exit_block, &[]);

    // Body: get element, serialize, concat
    cx.builder.switch_to_block(body_block);
    cx.builder.seal_block(body_block);

    let current_idx = cx.builder.use_var(idx_var);

    // Add comma before element if idx > 0
    let needs_comma = cx.builder.ins().icmp(IntCC::SignedGreaterThan, current_idx, zero);
    let comma_block = cx.builder.create_block();
    let no_comma_block = cx.builder.create_block();
    let merge_block = cx.builder.create_block();
    cx.builder.append_block_param(merge_block, cx.ptr_type);

    cx.builder.ins().brif(needs_comma, comma_block, &[], no_comma_block, &[]);

    // comma_block: append ","
    cx.builder.switch_to_block(comma_block);
    cx.builder.seal_block(comma_block);
    let comma_str = cx.create_string(",")?;
    let concat_ref = cx.module.declare_func_in_func(concat_fid, cx.builder.func);
    let with_comma_result = cx.builder.use_var(result_var);
    let call = cx.builder.ins().call(concat_ref, &[with_comma_result, comma_str]);
    let after_comma = cx.builder.inst_results(call)[0];
    cx.builder.ins().jump(merge_block, &[after_comma]);

    // no_comma_block: pass through
    cx.builder.switch_to_block(no_comma_block);
    cx.builder.seal_block(no_comma_block);
    let no_comma_result = cx.builder.use_var(result_var);
    cx.builder.ins().jump(merge_block, &[no_comma_result]);

    // merge_block
    cx.builder.switch_to_block(merge_block);
    cx.builder.seal_block(merge_block);
    let merged_result = cx.builder.block_params(merge_block)[0];

    // Get the element from the array
    let get_fid = cx.rt_fns["rt_array_get"];
    let get_ref = cx.module.declare_func_in_func(get_fid, cx.builder.func);
    let idx_val = cx.builder.use_var(idx_var);
    let get_call = cx.builder.ins().call(get_ref, &[arr_ptr, idx_val]);
    let elem_ptr = cx.builder.inst_results(get_call)[0];

    // Serialize the struct element to JSON (inline the field iteration)
    let struct_layout = cx.struct_fields.get(&struct_name)
        .ok_or_else(|| CodegenError { message: format!("undefined struct: {struct_name}") })?
        .clone();

    let inner_concat_fid = cx.rt_fns["rt_str_concat"];
    let mut elem_json = cx.create_string("{")?;

    for (fi, (fname, fty)) in struct_layout.iter().enumerate() {
        let prefix = if fi > 0 {
            format!(",\"{}\":", fname)
        } else {
            format!("\"{}\":", fname)
        };
        let prefix_str = cx.create_string(&prefix)?;
        let inner_concat_ref = cx.module.declare_func_in_func(inner_concat_fid, cx.builder.func);
        let c = cx.builder.ins().call(inner_concat_ref, &[elem_json, prefix_str]);
        elem_json = cx.builder.inst_results(c)[0];

        let foffset = (fi * 8) as i32;
        let raw_val = cx.builder.ins().load(types::I64, MemFlags::new(), elem_ptr, foffset);

        let field_json_str = match fty {
            TurboTy::Str => {
                let q = cx.create_string("\"")?;
                let cr = cx.module.declare_func_in_func(inner_concat_fid, cx.builder.func);
                let c1 = cx.builder.ins().call(cr, &[q, raw_val]);
                let wq = cx.builder.inst_results(c1)[0];
                let q2 = cx.create_string("\"")?;
                let cr2 = cx.module.declare_func_in_func(inner_concat_fid, cx.builder.func);
                let c2 = cx.builder.ins().call(cr2, &[wq, q2]);
                cx.builder.inst_results(c2)[0]
            }
            TurboTy::Int => {
                let fid = cx.rt_fns["rt_i64_to_str"];
                let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
                let c = cx.builder.ins().call(fref, &[raw_val]);
                cx.builder.inst_results(c)[0]
            }
            TurboTy::Bool => {
                let bool_val = cx.builder.ins().ireduce(types::I8, raw_val);
                let fid = cx.rt_fns["rt_bool_to_str"];
                let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
                let c = cx.builder.ins().call(fref, &[bool_val]);
                cx.builder.inst_results(c)[0]
            }
            TurboTy::Float => {
                let float_val = cx.builder.ins().bitcast(types::F64, MemFlags::new(), raw_val);
                let fid = cx.rt_fns["rt_f64_to_str"];
                let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
                let c = cx.builder.ins().call(fref, &[float_val]);
                cx.builder.inst_results(c)[0]
            }
            _ => {
                convert_to_str(cx, raw_val, fty)?
            }
        };

        let cr = cx.module.declare_func_in_func(inner_concat_fid, cx.builder.func);
        let c = cx.builder.ins().call(cr, &[elem_json, field_json_str]);
        elem_json = cx.builder.inst_results(c)[0];
    }

    let close_brace = cx.create_string("}")?;
    let cr = cx.module.declare_func_in_func(inner_concat_fid, cx.builder.func);
    let c = cx.builder.ins().call(cr, &[elem_json, close_brace]);
    elem_json = cx.builder.inst_results(c)[0];

    // Concat element JSON to accumulated result
    let concat_ref2 = cx.module.declare_func_in_func(concat_fid, cx.builder.func);
    let call2 = cx.builder.ins().call(concat_ref2, &[merged_result, elem_json]);
    let new_result = cx.builder.inst_results(call2)[0];
    cx.builder.def_var(result_var, new_result);

    // Increment idx
    let cur_idx = cx.builder.use_var(idx_var);
    let one = cx.builder.ins().iconst(types::I64, 1);
    let next_idx = cx.builder.ins().iadd(cur_idx, one);
    cx.builder.def_var(idx_var, next_idx);
    cx.builder.ins().jump(header_block, &[]);

    cx.builder.seal_block(header_block);

    // Exit: close with "]"
    cx.builder.switch_to_block(exit_block);
    cx.builder.seal_block(exit_block);

    let final_result = cx.builder.use_var(result_var);
    let close_bracket = cx.create_string("]")?;
    let concat_ref3 = cx.module.declare_func_in_func(concat_fid, cx.builder.func);
    let call3 = cx.builder.ins().call(concat_ref3, &[final_result, close_bracket]);
    let result = cx.builder.inst_results(call3)[0];

    Ok(Some((result, TurboTy::Str)))
}

// ── map/filter/reduce builtins ──────────────────────────────────────

/// compile_builtin_map: map(arr, fn) -> [U]
/// Allocates a new array of the same length, iterates the source array,
/// calls fn_ptr on each element via call_indirect, and stores results.
fn compile_builtin_map<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>]) -> Result<MaybeTyped, CodegenError> {
    let (arr_ptr, _arr_tty) = compile_expr(cx, &args[0])?.unwrap();
    let (closure_ptr, fn_tty) = compile_expr(cx, &args[1])?.unwrap();

    let (param_tty, ret_tty) = match &fn_tty {
        TurboTy::Fn(params, ret) => (params[0].clone(), *ret.clone()),
        _ => (TurboTy::Int, TurboTy::Int),
    };

    // Extract fn_ptr and env_ptr from closure pair struct
    let fn_ptr = cx.builder.ins().load(cx.ptr_type, MemFlags::new(), closure_ptr, 0);
    let env_ptr = cx.builder.ins().load(cx.ptr_type, MemFlags::new(), closure_ptr, 8);

    let len_fid = cx.rt_fns["rt_array_len"];
    let len_ref = cx.module.declare_func_in_func(len_fid, cx.builder.func);
    let call = cx.builder.ins().call(len_ref, &[arr_ptr]);
    let arr_len = cx.builder.inst_results(call)[0];

    let alloc_fid = cx.rt_fns["rt_array_alloc"];
    let alloc_ref = cx.module.declare_func_in_func(alloc_fid, cx.builder.func);
    let alloc_call = cx.builder.ins().call(alloc_ref, &[arr_len]);
    let result_ptr = cx.builder.inst_results(alloc_call)[0];

    let mut sig = cx.module.make_signature();
    sig.call_conv = CallConv::Fast;
    sig.params.push(AbiParam::new(cx.ptr_type)); // env_ptr
    let param_cl_ty = turbo_ty_to_cl_type(&param_tty, cx.ptr_type);
    sig.params.push(AbiParam::new(param_cl_ty));
    if ret_tty != TurboTy::Unit {
        let ret_cl_ty = turbo_ty_to_cl_type(&ret_tty, cx.ptr_type);
        sig.returns.push(AbiParam::new(ret_cl_ty));
    }
    let sig_ref = cx.builder.import_signature(sig);

    let idx_var = cx.fresh_var(types::I64, TurboTy::Int);
    let zero = cx.builder.ins().iconst(types::I64, 0);
    cx.builder.def_var(idx_var, zero);

    let header_block = cx.builder.create_block();
    let body_block = cx.builder.create_block();
    let exit_block = cx.builder.create_block();

    cx.builder.ins().jump(header_block, &[]);

    cx.builder.switch_to_block(header_block);
    let idx = cx.builder.use_var(idx_var);
    let cond = cx.builder.ins().icmp(IntCC::SignedLessThan, idx, arr_len);
    cx.builder.ins().brif(cond, body_block, &[], exit_block, &[]);

    cx.builder.switch_to_block(body_block);
    cx.builder.seal_block(body_block);

    let get_fid = cx.rt_fns["rt_array_get"];
    let get_ref = cx.module.declare_func_in_func(get_fid, cx.builder.func);
    let idx_val = cx.builder.use_var(idx_var);
    let get_call = cx.builder.ins().call(get_ref, &[arr_ptr, idx_val]);
    let raw_elem = cx.builder.inst_results(get_call)[0];

    let typed_elem = match &param_tty {
        TurboTy::Bool => cx.builder.ins().ireduce(types::I8, raw_elem),
        TurboTy::Float => cx.builder.ins().bitcast(types::F64, MemFlags::new(), raw_elem),
        _ => raw_elem,
    };

    let indirect_call = cx.builder.ins().call_indirect(sig_ref, fn_ptr, &[env_ptr, typed_elem]);
    let mapped_val = cx.builder.inst_results(indirect_call)[0];

    let store_val = match &ret_tty {
        TurboTy::Bool => cx.builder.ins().sextend(types::I64, mapped_val),
        TurboTy::Float => cx.builder.ins().bitcast(types::I64, MemFlags::new(), mapped_val),
        _ => mapped_val,
    };

    let set_fid = cx.rt_fns["rt_array_set"];
    let set_ref = cx.module.declare_func_in_func(set_fid, cx.builder.func);
    let idx_val2 = cx.builder.use_var(idx_var);
    cx.builder.ins().call(set_ref, &[result_ptr, idx_val2, store_val]);

    let current_idx = cx.builder.use_var(idx_var);
    let one = cx.builder.ins().iconst(types::I64, 1);
    let next_idx = cx.builder.ins().iadd(current_idx, one);
    cx.builder.def_var(idx_var, next_idx);
    cx.builder.ins().jump(header_block, &[]);

    cx.builder.seal_block(header_block);

    cx.builder.switch_to_block(exit_block);
    cx.builder.seal_block(exit_block);

    let result_elem_tty = ret_tty;
    Ok(Some((result_ptr, TurboTy::Array(Box::new(result_elem_tty)))))
}

/// compile_builtin_filter: filter(arr, fn) -> [T]
/// Allocates same-size array, filters elements by predicate, patches length.
fn compile_builtin_filter<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>]) -> Result<MaybeTyped, CodegenError> {
    let (arr_ptr, arr_tty) = compile_expr(cx, &args[0])?.unwrap();
    let (closure_ptr, fn_tty) = compile_expr(cx, &args[1])?.unwrap();

    let elem_tty = match &arr_tty {
        TurboTy::Array(inner) => *inner.clone(),
        _ => TurboTy::Int,
    };

    let param_tty = match &fn_tty {
        TurboTy::Fn(params, _) => params[0].clone(),
        _ => TurboTy::Int,
    };

    // Extract fn_ptr and env_ptr from closure pair struct
    let fn_ptr = cx.builder.ins().load(cx.ptr_type, MemFlags::new(), closure_ptr, 0);
    let env_ptr = cx.builder.ins().load(cx.ptr_type, MemFlags::new(), closure_ptr, 8);

    let len_fid = cx.rt_fns["rt_array_len"];
    let len_ref = cx.module.declare_func_in_func(len_fid, cx.builder.func);
    let call = cx.builder.ins().call(len_ref, &[arr_ptr]);
    let arr_len = cx.builder.inst_results(call)[0];

    let alloc_fid = cx.rt_fns["rt_array_alloc"];
    let alloc_ref = cx.module.declare_func_in_func(alloc_fid, cx.builder.func);
    let alloc_call = cx.builder.ins().call(alloc_ref, &[arr_len]);
    let result_ptr = cx.builder.inst_results(alloc_call)[0];

    let mut sig = cx.module.make_signature();
    sig.call_conv = CallConv::Fast;
    sig.params.push(AbiParam::new(cx.ptr_type)); // env_ptr
    let param_cl_ty = turbo_ty_to_cl_type(&param_tty, cx.ptr_type);
    sig.params.push(AbiParam::new(param_cl_ty));
    sig.returns.push(AbiParam::new(types::I8));
    let sig_ref = cx.builder.import_signature(sig);

    let idx_var = cx.fresh_var(types::I64, TurboTy::Int);
    let zero = cx.builder.ins().iconst(types::I64, 0);
    cx.builder.def_var(idx_var, zero);

    let out_var = cx.fresh_var(types::I64, TurboTy::Int);
    let zero2 = cx.builder.ins().iconst(types::I64, 0);
    cx.builder.def_var(out_var, zero2);

    let header_block = cx.builder.create_block();
    let body_block = cx.builder.create_block();
    let store_block = cx.builder.create_block();
    let inc_block = cx.builder.create_block();
    let exit_block = cx.builder.create_block();

    cx.builder.ins().jump(header_block, &[]);

    cx.builder.switch_to_block(header_block);
    let idx = cx.builder.use_var(idx_var);
    let cond = cx.builder.ins().icmp(IntCC::SignedLessThan, idx, arr_len);
    cx.builder.ins().brif(cond, body_block, &[], exit_block, &[]);

    cx.builder.switch_to_block(body_block);
    cx.builder.seal_block(body_block);

    let get_fid = cx.rt_fns["rt_array_get"];
    let get_ref = cx.module.declare_func_in_func(get_fid, cx.builder.func);
    let idx_val = cx.builder.use_var(idx_var);
    let get_call = cx.builder.ins().call(get_ref, &[arr_ptr, idx_val]);
    let raw_elem = cx.builder.inst_results(get_call)[0];

    let typed_elem = match &param_tty {
        TurboTy::Bool => cx.builder.ins().ireduce(types::I8, raw_elem),
        TurboTy::Float => cx.builder.ins().bitcast(types::F64, MemFlags::new(), raw_elem),
        _ => raw_elem,
    };

    let indirect_call = cx.builder.ins().call_indirect(sig_ref, fn_ptr, &[env_ptr, typed_elem]);
    let pred_result = cx.builder.inst_results(indirect_call)[0];

    cx.builder.ins().brif(pred_result, store_block, &[], inc_block, &[]);

    cx.builder.switch_to_block(store_block);
    cx.builder.seal_block(store_block);

    let set_fid = cx.rt_fns["rt_array_set"];
    let set_ref = cx.module.declare_func_in_func(set_fid, cx.builder.func);
    let out_idx = cx.builder.use_var(out_var);
    cx.builder.ins().call(set_ref, &[result_ptr, out_idx, raw_elem]);

    let one = cx.builder.ins().iconst(types::I64, 1);
    let next_out = cx.builder.ins().iadd(out_idx, one);
    cx.builder.def_var(out_var, next_out);

    cx.builder.ins().jump(inc_block, &[]);

    cx.builder.switch_to_block(inc_block);
    cx.builder.seal_block(inc_block);

    let current_idx = cx.builder.use_var(idx_var);
    let one2 = cx.builder.ins().iconst(types::I64, 1);
    let next_idx = cx.builder.ins().iadd(current_idx, one2);
    cx.builder.def_var(idx_var, next_idx);
    cx.builder.ins().jump(header_block, &[]);

    cx.builder.seal_block(header_block);

    cx.builder.switch_to_block(exit_block);
    cx.builder.seal_block(exit_block);

    let final_count = cx.builder.use_var(out_var);
    cx.builder.ins().store(MemFlags::new(), final_count, result_ptr, 0);

    Ok(Some((result_ptr, TurboTy::Array(Box::new(elem_tty)))))
}

/// compile_builtin_reduce: reduce(arr, init, fn) -> U
/// Folds through the array calling fn(acc, elem) for each element.
fn compile_builtin_reduce<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>]) -> Result<MaybeTyped, CodegenError> {
    let (arr_ptr, _arr_tty) = compile_expr(cx, &args[0])?.unwrap();
    let (init_val, init_tty) = compile_expr(cx, &args[1])?.unwrap();
    let (closure_ptr, fn_tty) = compile_expr(cx, &args[2])?.unwrap();

    let (acc_tty, elem_tty, ret_tty) = match &fn_tty {
        TurboTy::Fn(params, ret) => (params[0].clone(), params[1].clone(), *ret.clone()),
        _ => (TurboTy::Int, TurboTy::Int, TurboTy::Int),
    };

    // Extract fn_ptr and env_ptr from closure pair struct
    let fn_ptr = cx.builder.ins().load(cx.ptr_type, MemFlags::new(), closure_ptr, 0);
    let env_ptr = cx.builder.ins().load(cx.ptr_type, MemFlags::new(), closure_ptr, 8);

    let acc_cl_ty = turbo_ty_to_cl_type(&acc_tty, cx.ptr_type);
    let elem_cl_ty = turbo_ty_to_cl_type(&elem_tty, cx.ptr_type);
    let ret_cl_ty = turbo_ty_to_cl_type(&ret_tty, cx.ptr_type);

    let len_fid = cx.rt_fns["rt_array_len"];
    let len_ref = cx.module.declare_func_in_func(len_fid, cx.builder.func);
    let call = cx.builder.ins().call(len_ref, &[arr_ptr]);
    let arr_len = cx.builder.inst_results(call)[0];

    let mut sig = cx.module.make_signature();
    sig.call_conv = CallConv::Fast;
    sig.params.push(AbiParam::new(cx.ptr_type)); // env_ptr
    sig.params.push(AbiParam::new(acc_cl_ty));
    sig.params.push(AbiParam::new(elem_cl_ty));
    sig.returns.push(AbiParam::new(ret_cl_ty));
    let sig_ref = cx.builder.import_signature(sig);

    let acc_var = cx.fresh_var(acc_cl_ty, acc_tty.clone());
    cx.builder.def_var(acc_var, init_val);

    let idx_var = cx.fresh_var(types::I64, TurboTy::Int);
    let zero = cx.builder.ins().iconst(types::I64, 0);
    cx.builder.def_var(idx_var, zero);

    let header_block = cx.builder.create_block();
    let body_block = cx.builder.create_block();
    let exit_block = cx.builder.create_block();

    cx.builder.ins().jump(header_block, &[]);

    cx.builder.switch_to_block(header_block);
    let idx = cx.builder.use_var(idx_var);
    let cond = cx.builder.ins().icmp(IntCC::SignedLessThan, idx, arr_len);
    cx.builder.ins().brif(cond, body_block, &[], exit_block, &[]);

    cx.builder.switch_to_block(body_block);
    cx.builder.seal_block(body_block);

    let get_fid = cx.rt_fns["rt_array_get"];
    let get_ref = cx.module.declare_func_in_func(get_fid, cx.builder.func);
    let idx_val = cx.builder.use_var(idx_var);
    let get_call = cx.builder.ins().call(get_ref, &[arr_ptr, idx_val]);
    let raw_elem = cx.builder.inst_results(get_call)[0];

    let typed_elem = match &elem_tty {
        TurboTy::Bool => cx.builder.ins().ireduce(types::I8, raw_elem),
        TurboTy::Float => cx.builder.ins().bitcast(types::F64, MemFlags::new(), raw_elem),
        _ => raw_elem,
    };

    let current_acc = cx.builder.use_var(acc_var);
    let indirect_call = cx.builder.ins().call_indirect(sig_ref, fn_ptr, &[env_ptr, current_acc, typed_elem]);
    let new_acc = cx.builder.inst_results(indirect_call)[0];
    cx.builder.def_var(acc_var, new_acc);

    let current_idx = cx.builder.use_var(idx_var);
    let one = cx.builder.ins().iconst(types::I64, 1);
    let next_idx = cx.builder.ins().iadd(current_idx, one);
    cx.builder.def_var(idx_var, next_idx);
    cx.builder.ins().jump(header_block, &[]);

    cx.builder.seal_block(header_block);

    cx.builder.switch_to_block(exit_block);
    cx.builder.seal_block(exit_block);

    let final_acc = cx.builder.use_var(acc_var);
    Ok(Some((final_acc, init_tty)))
}

// ── If expression ───────────────────────────────────────────────────

fn compile_if<M: Module>(
    cx: &mut Ctx<'_, M>,
    condition: &Spanned<Expr>,
    then_branch: &Spanned<Expr>,
    else_branch: Option<&Spanned<Expr>>,
) -> Result<MaybeTyped, CodegenError> {
    let (cond, _) = compile_expr(cx, condition)?.unwrap();
    let cond_bool = cx.to_bool(cond);

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

// ── Struct field-by-field equality (@derive(Eq)) ────────────────────

fn compile_struct_eq<M: Module>(
    cx: &mut Ctx<'_, M>,
    lhs_ptr: Value,
    rhs_ptr: Value,
    struct_name: &str,
    op: BinOp,
) -> Result<MaybeTyped, CodegenError> {
    let struct_layout = cx.struct_fields.get(struct_name)
        .ok_or_else(|| CodegenError { message: format!("undefined struct: {struct_name}") })?
        .clone();

    if struct_layout.is_empty() {
        // No fields: always equal
        let result = if op == BinOp::Eq {
            cx.builder.ins().iconst(types::I8, 1)
        } else {
            cx.builder.ins().iconst(types::I8, 0)
        };
        return Ok(Some((result, TurboTy::Bool)));
    }

    // Compare field by field, short-circuiting on first mismatch
    // We use a chain of basic blocks: for each field, if mismatch -> result false, else -> check next
    let merge_block = cx.builder.create_block();
    cx.builder.append_block_param(merge_block, types::I8);

    for (i, (_, field_tty)) in struct_layout.iter().enumerate() {
        let offset = (i * 8) as i32;

        let lhs_raw = cx.builder.ins().load(types::I64, MemFlags::new(), lhs_ptr, offset);
        let rhs_raw = cx.builder.ins().load(types::I64, MemFlags::new(), rhs_ptr, offset);

        let fields_eq = match field_tty {
            TurboTy::Str => {
                // Use rt_str_eq for string fields
                let fid = cx.rt_fns["rt_str_eq"];
                let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
                let call = cx.builder.ins().call(fref, &[lhs_raw, rhs_raw]);
                cx.builder.inst_results(call)[0]
            }
            TurboTy::Float => {
                // Bitcast back to f64 and compare
                let lhs_f = cx.builder.ins().bitcast(types::F64, MemFlags::new(), lhs_raw);
                let rhs_f = cx.builder.ins().bitcast(types::F64, MemFlags::new(), rhs_raw);
                cx.builder.ins().fcmp(FloatCC::Equal, lhs_f, rhs_f)
            }
            TurboTy::Bool => {
                // Compare the raw i64 values (booleans are stored widened to i64)
                cx.builder.ins().icmp(IntCC::Equal, lhs_raw, rhs_raw)
            }
            _ => {
                // Int, Enum, Struct (pointer equality for nested structs without derive)
                cx.builder.ins().icmp(IntCC::Equal, lhs_raw, rhs_raw)
            }
        };

        if i < struct_layout.len() - 1 {
            // Not the last field: if mismatch, jump to merge with false; else continue
            let next_block = cx.builder.create_block();
            let false_val = cx.builder.ins().iconst(types::I8, 0);
            cx.builder.ins().brif(fields_eq, next_block, &[], merge_block, &[false_val]);
            cx.builder.switch_to_block(next_block);
            cx.builder.seal_block(next_block);
        } else {
            // Last field: jump to merge with the comparison result
            cx.builder.ins().jump(merge_block, &[fields_eq]);
        }
    }

    cx.builder.switch_to_block(merge_block);
    cx.builder.seal_block(merge_block);

    let result = cx.builder.block_params(merge_block)[0];
    let result = if op == BinOp::NotEq {
        let one = cx.builder.ins().iconst(types::I8, 1);
        cx.builder.ins().bxor(result, one)
    } else {
        result
    };
    Ok(Some((result, TurboTy::Bool)))
}

// ── Struct clone (@derive(Clone)) ───────────────────────────────────

fn compile_clone<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (src_ptr, src_tty) = compile_expr(cx, &args[0])?.unwrap();

    let struct_name = match &src_tty {
        TurboTy::Struct(name) => name.clone(),
        _ => return Err(CodegenError { message: "clone() expects a struct argument".to_string() }),
    };

    let struct_layout = cx.struct_fields.get(&struct_name)
        .ok_or_else(|| CodegenError { message: format!("undefined struct: {struct_name}") })?
        .clone();

    let num_fields = struct_layout.len() as i64;
    let num_fields_val = cx.builder.ins().iconst(types::I64, num_fields);

    // Allocate a new struct
    let alloc_fid = cx.rt_fns["rt_struct_alloc"];
    let alloc_fref = cx.module.declare_func_in_func(alloc_fid, cx.builder.func);
    let call = cx.builder.ins().call(alloc_fref, &[num_fields_val]);
    let new_ptr = cx.builder.inst_results(call)[0];

    // Copy each field from source to destination
    for (i, (_field_name, _field_tty)) in struct_layout.iter().enumerate() {
        let offset = (i * 8) as i32;
        let val = cx.builder.ins().load(types::I64, MemFlags::new(), src_ptr, offset);
        cx.builder.ins().store(MemFlags::new(), val, new_ptr, offset);
    }

    Ok(Some((new_ptr, TurboTy::Struct(struct_name))))
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
        TurboTy::Enum(ref enum_name) => {
            let tag_val = if cx.enum_max_slots.contains_key(enum_name.as_str()) {
                cx.builder.ins().load(types::I64, MemFlags::new(), val, 0)
            } else {
                let val = if cx.builder.func.dfg.value_type(val).bits() < 64 {
                    cx.builder.ins().sextend(types::I64, val)
                } else {
                    val
                };
                val
            };
            let fid = cx.rt_fns["rt_i64_to_str"];
            let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
            let call = cx.builder.ins().call(fref, &[tag_val]);
            Ok(cx.builder.inst_results(call)[0])
        }
        TurboTy::Array(_) => {
            cx.create_string("[array]")
        }
        TurboTy::Struct(ref name) => {
            // Check if struct implements Display trait
            let has_display = cx.trait_impls.get(name)
                .map_or(false, |traits| traits.contains(&"Display".to_string()));
            if has_display {
                let mangled = format!("{name}__to_string");
                if let Some(&fid) = cx.user_fns.get(&mangled) {
                    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
                    let call = cx.builder.ins().call(fref, &[val]);
                    Ok(cx.builder.inst_results(call)[0])
                } else {
                    cx.create_string(&format!("[struct {}]", name))
                }
            } else {
                cx.create_string(&format!("[struct {}]", name))
            }
        }
        TurboTy::Fn(_, _) => {
            cx.create_string("[function]")
        }
        TurboTy::Result(_, _) => {
            cx.create_string("[result]")
        }
        TurboTy::Optional(_) => {
            cx.create_string("[optional]")
        }
        TurboTy::Agent(ref name) => {
            cx.create_string(&format!("[agent {}]", name))
        }
        TurboTy::Future(_) => {
            cx.create_string("[future]")
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
    let cond_bool = cx.to_bool(cond);

    cx.builder.ins().brif(cond_bool, body_block, &[], exit_block, &[]);

    // Body
    cx.builder.switch_to_block(body_block);
    cx.builder.seal_block(body_block);
    cx.loop_stack.push((header_block, exit_block));
    compile_expr(cx, body)?;
    cx.loop_stack.pop();

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

    // Create blocks: header, body, continue (increment), exit
    let header_block = cx.builder.create_block();
    let body_block = cx.builder.create_block();
    let continue_block = cx.builder.create_block();
    let exit_block = cx.builder.create_block();

    cx.builder.ins().jump(header_block, &[]);

    // Header: check i < end
    // Do NOT seal header yet -- it has predecessors (entry + continue back edge + possible continue jumps)
    cx.builder.switch_to_block(header_block);

    let current_i = cx.builder.use_var(var);
    let cond = cx.builder.ins().icmp(IntCC::SignedLessThan, current_i, range_end);
    cx.builder.ins().brif(cond, body_block, &[], exit_block, &[]);

    // Body
    cx.builder.switch_to_block(body_block);
    cx.builder.seal_block(body_block);

    cx.loop_stack.push((continue_block, exit_block));
    compile_expr(cx, body)?;
    cx.loop_stack.pop();

    // Fall through to continue block
    if !cx.builder.is_unreachable() {
        cx.builder.ins().jump(continue_block, &[]);
    }

    // Continue block: increment i = i + 1, then jump to header
    cx.builder.switch_to_block(continue_block);
    // Don't seal continue_block yet -- it can have predecessors from body fallthrough + continue jumps
    let current_i = cx.builder.use_var(var);
    let one = cx.builder.ins().iconst(types::I64, 1);
    let next_i = cx.builder.ins().iadd(current_i, one);
    cx.builder.def_var(var, next_i);
    cx.builder.ins().jump(header_block, &[]);

    // NOW seal the header and continue block (all predecessors are known)
    cx.builder.seal_block(continue_block);
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
    let continue_block = cx.builder.create_block();
    let exit_block = cx.builder.create_block();

    cx.builder.ins().jump(header_block, &[]);

    // Header: check idx < len
    // Do NOT seal header yet -- it has predecessors (entry + continue back edge)
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
    cx.loop_stack.push((continue_block, exit_block));
    compile_expr(cx, body)?;
    cx.loop_stack.pop();

    // Fall through to continue block
    if !cx.builder.is_unreachable() {
        cx.builder.ins().jump(continue_block, &[]);
    }

    // Continue block: increment index, then jump to header
    cx.builder.switch_to_block(continue_block);
    let current_idx = cx.builder.use_var(idx_var);
    let one = cx.builder.ins().iconst(types::I64, 1);
    let next_idx = cx.builder.ins().iadd(current_idx, one);
    cx.builder.def_var(idx_var, next_idx);
    cx.builder.ins().jump(header_block, &[]);

    // NOW seal header and continue block (all predecessors are known)
    cx.builder.seal_block(continue_block);
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
        let has_guard = arm.guard.is_some();
        // A catch-all pattern is only truly unconditional when it has no guard.
        let is_catchall_pattern = matches!(&arm.pattern.node, Pattern::Wildcard)
            || matches!(&arm.pattern.node, Pattern::Ident(name)
                if lookup_variant_tag_static(cx.enum_variants, name).is_none());
        let is_catchall = is_catchall_pattern && !has_guard;

        if is_catchall {
            // Unconditional arm -- bind variable if ident pattern, compile body
            let saved_vars = cx.vars.clone();
            if let Pattern::Ident(name) = &arm.pattern.node {
                let cl_ty = cx.builder.func.dfg.value_type(subj_val);
                let var = Variable::new(cx.next_var);
                cx.next_var += 1;
                cx.builder.declare_var(var, cl_ty);
                cx.builder.def_var(var, subj_val);
                cx.vars.insert(name.clone(), (var, cl_ty, subj_tty.clone()));
            }
            let body_result = compile_expr(cx, &arm.body)?;
            cx.vars = saved_vars;
            emit_match_arm_jump(cx, merge_block, body_result, &mut has_result, &mut result_turbo_ty);
            hit_catchall = true;

            if !is_last {
                let dead_block = cx.builder.create_block();
                cx.builder.switch_to_block(dead_block);
                cx.builder.seal_block(dead_block);
            }
            break;
        }

        // For catch-all patterns with a guard, skip pattern condition check.
        let next_block = cx.builder.create_block();

        if is_catchall_pattern && has_guard {
            let match_block = cx.builder.create_block();
            cx.builder.ins().jump(match_block, &[]);
            cx.builder.switch_to_block(match_block);
            cx.builder.seal_block(match_block);
        } else {
            // Conditional arm: compute whether the pattern matches
            let matches_cond = match &arm.pattern.node {
                Pattern::Ident(name) => {
                    let tag_val = lookup_variant_tag_static(cx.enum_variants, name).unwrap();
                    let pat_val = cx.builder.ins().iconst(types::I64, tag_val as i64);
                    let actual_tag = if let TurboTy::Enum(ref enum_name) = subj_tty {
                        if cx.enum_max_slots.contains_key(enum_name.as_str()) {
                            cx.builder.ins().load(types::I64, MemFlags::new(), subj_val, 0)
                        } else {
                            subj_val
                        }
                    } else {
                        subj_val
                    };
                    cx.builder.ins().icmp(IntCC::Equal, actual_tag, pat_val)
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
                Pattern::Wildcard => unreachable!(),
                Pattern::Ok(_binding) => {
                    let tag_fid = cx.rt_fns["rt_result_tag"];
                    let tag_fref = cx.module.declare_func_in_func(tag_fid, cx.builder.func);
                    let tag_call = cx.builder.ins().call(tag_fref, &[subj_val]);
                    let tag = cx.builder.inst_results(tag_call)[0];
                    let zero = cx.builder.ins().iconst(types::I64, 0);
                    cx.builder.ins().icmp(IntCC::Equal, tag, zero)
                }
                Pattern::Err(_binding) => {
                    let tag_fid = cx.rt_fns["rt_result_tag"];
                    let tag_fref = cx.module.declare_func_in_func(tag_fid, cx.builder.func);
                    let tag_call = cx.builder.ins().call(tag_fref, &[subj_val]);
                    let tag = cx.builder.inst_results(tag_call)[0];
                    let one = cx.builder.ins().iconst(types::I64, 1);
                    cx.builder.ins().icmp(IntCC::Equal, tag, one)
                }
                Pattern::Some(_binding) => {
                    let tag_fid = cx.rt_fns["rt_option_tag"];
                    let tag_fref = cx.module.declare_func_in_func(tag_fid, cx.builder.func);
                    let tag_call = cx.builder.ins().call(tag_fref, &[subj_val]);
                    let tag = cx.builder.inst_results(tag_call)[0];
                    let one = cx.builder.ins().iconst(types::I64, 1);
                    cx.builder.ins().icmp(IntCC::Equal, tag, one)
                }
                Pattern::None => {
                    let tag_fid = cx.rt_fns["rt_option_tag"];
                    let tag_fref = cx.module.declare_func_in_func(tag_fid, cx.builder.func);
                    let tag_call = cx.builder.ins().call(tag_fref, &[subj_val]);
                    let tag = cx.builder.inst_results(tag_call)[0];
                    let zero = cx.builder.ins().iconst(types::I64, 0);
                    cx.builder.ins().icmp(IntCC::Equal, tag, zero)
                }
                Pattern::VariantDestructure { variant, .. } => {
                    let tag_val = lookup_variant_tag_static(cx.enum_variants, variant).unwrap();
                    let pat_val = cx.builder.ins().iconst(types::I64, tag_val as i64);
                    let actual_tag = cx.builder.ins().load(types::I64, MemFlags::new(), subj_val, 0);
                    cx.builder.ins().icmp(IntCC::Equal, actual_tag, pat_val)
                }
            };

            let match_block = cx.builder.create_block();
            cx.builder.ins().brif(matches_cond, match_block, &[], next_block, &[]);
            cx.builder.switch_to_block(match_block);
            cx.builder.seal_block(match_block);
        }

        // Bind pattern variables in the match block
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

                let turbo_ty = match &subj_tty {
                    TurboTy::Result(ok_tty, err_tty) => {
                        if matches!(&arm.pattern.node, Pattern::Ok(_)) {
                            *ok_tty.clone()
                        } else {
                            *err_tty.clone()
                        }
                    }
                    _ => TurboTy::Int,
                };
                cx.vars.insert(binding.clone(), (var, types::I64, turbo_ty));
            }
            Pattern::Some(binding) => {
                let val_fid = cx.rt_fns["rt_option_value"];
                let val_fref = cx.module.declare_func_in_func(val_fid, cx.builder.func);
                let val_call = cx.builder.ins().call(val_fref, &[subj_val]);
                let raw_val = cx.builder.inst_results(val_call)[0];

                let var = Variable::new(cx.next_var);
                cx.next_var += 1;
                cx.builder.declare_var(var, types::I64);
                cx.builder.def_var(var, raw_val);

                let turbo_ty = match &subj_tty {
                    TurboTy::Optional(inner_tty) => *inner_tty.clone(),
                    _ => TurboTy::Int,
                };
                cx.vars.insert(binding.clone(), (var, types::I64, turbo_ty));
            }
            Pattern::VariantDestructure { variant, bindings } => {
                let field_tys = if let TurboTy::Enum(ref enum_name) = subj_tty {
                    cx.enum_variant_fields
                        .get(&(enum_name.clone(), variant.clone()))
                        .cloned()
                        .unwrap_or_default()
                } else {
                    Vec::new()
                };

                for (i, binding) in bindings.iter().enumerate() {
                    let offset = ((i + 1) * 8) as i32;
                    let raw_val = cx.builder.ins().load(types::I64, MemFlags::new(), subj_val, offset);

                    let field_tty = if i < field_tys.len() {
                        field_tys[i].clone()
                    } else {
                        TurboTy::Int
                    };

                    let (val, cl_ty) = match &field_tty {
                        TurboTy::Float => {
                            let f = cx.builder.ins().bitcast(types::F64, MemFlags::new(), raw_val);
                            (f, types::F64)
                        }
                        TurboTy::Bool => {
                            let b = cx.builder.ins().ireduce(types::I8, raw_val);
                            (b, types::I8)
                        }
                        _ => (raw_val, types::I64),
                    };

                    let var = Variable::new(cx.next_var);
                    cx.next_var += 1;
                    cx.builder.declare_var(var, cl_ty);
                    cx.builder.def_var(var, val);
                    cx.vars.insert(binding.clone(), (var, cl_ty, field_tty));
                }
            }
            Pattern::Ident(name) if is_catchall_pattern => {
                // Catch-all ident with guard: bind subject value as variable
                let cl_ty = cx.builder.func.dfg.value_type(subj_val);
                let var = Variable::new(cx.next_var);
                cx.next_var += 1;
                cx.builder.declare_var(var, cl_ty);
                cx.builder.def_var(var, subj_val);
                cx.vars.insert(name.clone(), (var, cl_ty, subj_tty.clone()));
            }
            _ => {}
        }

        // If there is a guard, evaluate it: true -> body, false -> next arm
        if let Some(ref guard) = arm.guard {
            let guard_result = compile_expr(cx, guard)?;
            if let Some((guard_val, _)) = guard_result {
                let body_block = cx.builder.create_block();
                cx.builder.ins().brif(guard_val, body_block, &[], next_block, &[]);
                cx.builder.switch_to_block(body_block);
                cx.builder.seal_block(body_block);
            }
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

// ── Channel builtins ────────────────────────────────────────────────

/// channel() -> Channel (pointer)
fn compile_builtin_channel<M: Module>(cx: &mut Ctx<'_, M>) -> Result<MaybeTyped, CodegenError> {
    let fid = cx.rt_fns["rt_channel_create"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Int)))
}

/// send(ch, value) -> ()
fn compile_builtin_send<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>]) -> Result<MaybeTyped, CodegenError> {
    let (ch_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let (value_val, _) = compile_expr(cx, &args[1])?.unwrap();
    // Ensure value is i64
    let val_ty = cx.builder.func.dfg.value_type(value_val);
    let value_val = if val_ty.bits() < 64 {
        cx.builder.ins().sextend(types::I64, value_val)
    } else {
        value_val
    };
    let fid = cx.rt_fns["rt_channel_send"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    cx.builder.ins().call(fref, &[ch_val, value_val]);
    Ok(None)
}

/// recv(ch) -> i64
fn compile_builtin_recv<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>]) -> Result<MaybeTyped, CodegenError> {
    let (ch_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let fid = cx.rt_fns["rt_channel_recv"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[ch_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Int)))
}

// ── Mutex builtins ──────────────────────────────────────────────────

/// mutex(value) -> Mutex (pointer)
fn compile_builtin_mutex<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>]) -> Result<MaybeTyped, CodegenError> {
    let (value_val, _) = compile_expr(cx, &args[0])?.unwrap();
    // Ensure value is i64
    let val_ty = cx.builder.func.dfg.value_type(value_val);
    let value_val = if val_ty.bits() < 64 {
        cx.builder.ins().sextend(types::I64, value_val)
    } else {
        value_val
    };
    let fid = cx.rt_fns["rt_mutex_create"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[value_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Int)))
}

/// mutex_get(m) -> i64
fn compile_builtin_mutex_get<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>]) -> Result<MaybeTyped, CodegenError> {
    let (m_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let fid = cx.rt_fns["rt_mutex_get"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[m_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Int)))
}

/// mutex_set(m, value) -> ()
fn compile_builtin_mutex_set<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>]) -> Result<MaybeTyped, CodegenError> {
    let (m_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let (value_val, _) = compile_expr(cx, &args[1])?.unwrap();
    // Ensure value is i64
    let val_ty = cx.builder.func.dfg.value_type(value_val);
    let value_val = if val_ty.bits() < 64 {
        cx.builder.ins().sextend(types::I64, value_val)
    } else {
        value_val
    };
    let fid = cx.rt_fns["rt_mutex_set"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    cx.builder.ins().call(fref, &[m_val, value_val]);
    Ok(None)
}

// ── HashMap builtins ────────────────────────────────────────────────

/// hashmap() -> HashMap (opaque pointer)
fn compile_builtin_hashmap<M: Module>(cx: &mut Ctx<'_, M>) -> Result<MaybeTyped, CodegenError> {
    let fid = cx.rt_fns["rt_hashmap_new"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Int)))
}

/// hashmap_set(map, key, value) -> ()
fn compile_builtin_hashmap_set<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>]) -> Result<MaybeTyped, CodegenError> {
    let (map_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let (key_val, _) = compile_expr(cx, &args[1])?.unwrap();
    let (value_val, _) = compile_expr(cx, &args[2])?.unwrap();
    let fid = cx.rt_fns["rt_hashmap_set"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    cx.builder.ins().call(fref, &[map_val, key_val, value_val]);
    Ok(None)
}

/// hashmap_get(map, key) -> str
fn compile_builtin_hashmap_get<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>]) -> Result<MaybeTyped, CodegenError> {
    let (map_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let (key_val, _) = compile_expr(cx, &args[1])?.unwrap();
    let fid = cx.rt_fns["rt_hashmap_get"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[map_val, key_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Str)))
}

/// hashmap_has(map, key) -> bool
fn compile_builtin_hashmap_has<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>]) -> Result<MaybeTyped, CodegenError> {
    let (map_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let (key_val, _) = compile_expr(cx, &args[1])?.unwrap();
    let fid = cx.rt_fns["rt_hashmap_has"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[map_val, key_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Bool)))
}

/// hashmap_len(map) -> i64
fn compile_builtin_hashmap_len<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>]) -> Result<MaybeTyped, CodegenError> {
    let (map_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let fid = cx.rt_fns["rt_hashmap_len"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[map_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Int)))
}

/// hashmap_keys(map) -> [str]
fn compile_builtin_hashmap_keys<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>]) -> Result<MaybeTyped, CodegenError> {
    let (map_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let fid = cx.rt_fns["rt_hashmap_keys"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[map_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Array(Box::new(TurboTy::Str)))))
}

/// hashmap_remove(map, key) -> ()
fn compile_builtin_hashmap_remove<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>]) -> Result<MaybeTyped, CodegenError> {
    let (map_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let (key_val, _) = compile_expr(cx, &args[1])?.unwrap();
    let fid = cx.rt_fns["rt_hashmap_remove"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    cx.builder.ins().call(fref, &[map_val, key_val]);
    Ok(None)
}

// ── Unsafe builtins — raw pointer operations ────────────────────────

/// deref(addr: i64) -> i64 — load an i64 from the given memory address
fn compile_builtin_deref<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>]) -> Result<MaybeTyped, CodegenError> {
    let (addr_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let result = cx.builder.ins().load(types::I64, MemFlags::new(), addr_val, 0);
    Ok(Some((result, TurboTy::Int)))
}

/// store(addr: i64, value: i64) — store an i64 at the given memory address
fn compile_builtin_store<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>]) -> Result<MaybeTyped, CodegenError> {
    let (addr_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let (val, _) = compile_expr(cx, &args[1])?.unwrap();
    cx.builder.ins().store(MemFlags::new(), val, addr_val, 0);
    Ok(None)
}
