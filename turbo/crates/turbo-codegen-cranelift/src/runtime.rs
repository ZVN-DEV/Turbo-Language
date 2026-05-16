//! Runtime functions linked into the JIT module.
//!
//! All `extern "C" fn rt_*` functions live here. They are completely
//! standalone — no Cranelift types, no `Ctx`, no compiler state.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::Mutex;

/// Build a `CString` from the given value, falling back to an empty string
/// if the input contains an interior nul byte.
fn cstring_or_empty(s: impl Into<Vec<u8>>) -> std::ffi::CString {
    std::ffi::CString::new(s).unwrap_or_else(|_| std::ffi::CString::new("").unwrap())
}

/// Compute an allocation layout for array-like structures, returning None on overflow.
/// Format: [refcount: i64][length: i64][elements: len * 8 bytes]
fn checked_array_layout(len: usize) -> Option<std::alloc::Layout> {
    let elem_bytes = len.checked_mul(8)?;
    let data_bytes = elem_bytes.checked_add(8)?; // +8 for length field
    let total_bytes = data_bytes.checked_add(8)?; // +8 for refcount header
    std::alloc::Layout::from_size_align(total_bytes, 8).ok()
}

// ── Allocation registry for ARC deallocation ────────────────────────
//
// Every rt_* allocation that uses refcounted headers registers the raw
// allocation pointer and its Layout here. When rt_release drops the
// refcount to zero, the registry is consulted to safely deallocate.

thread_local! {
    pub(crate) static ALLOC_REGISTRY: RefCell<HashMap<usize, std::alloc::Layout>> =
        RefCell::new(HashMap::new());
}

/// Register a raw allocation pointer and its layout for later deallocation.
fn register_alloc(raw_ptr: *mut u8, layout: std::alloc::Layout) {
    ALLOC_REGISTRY.with(|reg| {
        reg.borrow_mut().insert(raw_ptr as usize, layout);
    });
}

/// Remove and return the layout for a raw allocation pointer.
fn unregister_alloc(raw_ptr: *mut u8) -> Option<std::alloc::Layout> {
    ALLOC_REGISTRY.with(|reg| reg.borrow_mut().remove(&(raw_ptr as usize)))
}

// ── Thread-local string arena ────────────────────────────────────────
//
// Every rt_* function that returns a newly-allocated CString registers
// the pointer here via `arena_str()`. At the end of JIT execution (or
// after each HTTP request), `rt_arena_reset()` frees them all.

thread_local! {
    static STRING_ARENA: RefCell<Vec<*mut std::ffi::c_char>> = const { RefCell::new(Vec::new()) };
}

/// Leak a CString into the arena. Returns the raw pointer for FFI.
fn arena_str(cs: std::ffi::CString) -> *const u8 {
    let ptr = cs.into_raw();
    STRING_ARENA.with(|arena| {
        arena.borrow_mut().push(ptr);
    });
    ptr as *const u8
}

/// Free all strings in the arena. Called after JIT main() returns.
pub(crate) extern "C" fn rt_arena_reset() {
    STRING_ARENA.with(|arena| {
        let mut strings = arena.borrow_mut();
        for ptr in strings.drain(..) {
            unsafe {
                drop(std::ffi::CString::from_raw(ptr));
            }
        }
    });
}

pub(crate) extern "C" fn rt_print_str(s: *const u8) {
    if s.is_null() {
        println!();
        return;
    }
    let cstr = unsafe { std::ffi::CStr::from_ptr(s as *const std::ffi::c_char) };
    if let Ok(string) = cstr.to_str() {
        println!("{}", string);
    }
}

pub(crate) extern "C" fn rt_print_i64(n: i64) {
    println!("{}", n);
}

pub(crate) extern "C" fn rt_print_f64(n: f64) {
    println!("{}", n);
}

pub(crate) extern "C" fn rt_print_bool(b: i8) {
    println!("{}", if b != 0 { "true" } else { "false" });
}

pub(crate) extern "C" fn rt_panic(msg: *const u8) {
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

pub(crate) extern "C" fn rt_assert_fail(msg: *const u8) {
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
pub(crate) extern "C" fn rt_assert_eq_fail(kind: i64, actual: *const u8, expected: *const u8) {
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
        eprintln!(
            "assertion failed: assert_eq({}, {})",
            actual_str, expected_str
        );
        eprintln!("  left:  {}", actual_str);
        eprintln!("  right: {}", expected_str);
    } else {
        eprintln!(
            "assertion failed: assert_ne({}, {})",
            actual_str, expected_str
        );
        eprintln!("  both values are: {}", actual_str);
    }
    std::process::exit(1);
}

pub(crate) extern "C" fn rt_div_by_zero() {
    eprintln!("runtime error: division by zero");
    std::process::exit(1);
}

pub(crate) extern "C" fn rt_int_overflow() {
    eprintln!("runtime error: integer overflow");
    std::process::exit(1);
}

pub(crate) extern "C" fn rt_array_alloc(len: i64) -> *mut u8 {
    if len < 0 {
        eprintln!("runtime error: negative array length {}", len);
        std::process::exit(1);
    }
    let layout = match checked_array_layout(len as usize) {
        Some(l) => l,
        None => {
            eprintln!("runtime error: array allocation overflow (length {})", len);
            std::process::exit(1);
        }
    };
    let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
    if ptr.is_null() {
        eprintln!("turbo: fatal: memory allocation failed");
        std::process::exit(1);
    }
    register_alloc(ptr, layout);
    unsafe {
        *(ptr as *mut i64) = 1;
    } // refcount = 1
    let data_ptr = unsafe { ptr.add(8) }; // pointer past refcount header
                                          // Store length at the start of the data region
    unsafe {
        *(data_ptr as *mut i64) = len;
    }
    data_ptr
}

pub(crate) extern "C" fn rt_array_get(arr: *const u8, index: i64) -> i64 {
    let len = unsafe { *(arr as *const i64) };
    if index < 0 || index >= len {
        eprintln!(
            "runtime error: array index {} out of bounds (length {})",
            index, len
        );
        std::process::exit(1);
    }
    unsafe { *((arr as *const i64).add(1 + index as usize)) }
}

pub(crate) extern "C" fn rt_array_set(arr: *mut u8, index: i64, value: i64) -> *mut u8 {
    // COW: check refcount before mutating
    let rc_ptr = unsafe { arr.sub(8) as *mut std::sync::atomic::AtomicI64 };
    let rc = unsafe { (*rc_ptr).load(std::sync::atomic::Ordering::Acquire) };
    let target = if rc > 1 {
        // Copy-on-write: make a private copy
        let len = unsafe { *(arr as *const i64) };
        let data_size = (1 + len as usize) * 8; // length field + elements, for copy
        let layout = match checked_array_layout(len as usize) {
            Some(l) => l,
            None => {
                eprintln!("runtime error: COW array copy overflow");
                std::process::exit(1);
            }
        };
        let new_alloc = unsafe { std::alloc::alloc_zeroed(layout) };
        if new_alloc.is_null() {
            eprintln!("turbo: fatal: memory allocation failed");
            std::process::exit(1);
        }
        register_alloc(new_alloc, layout);
        unsafe {
            *(new_alloc as *mut i64) = 1;
        } // new refcount = 1
        let new_data = unsafe { new_alloc.add(8) };
        unsafe {
            std::ptr::copy_nonoverlapping(arr, new_data, data_size);
        }
        // Decrement old refcount
        unsafe {
            (*rc_ptr).fetch_sub(1, std::sync::atomic::Ordering::Release);
        }
        new_data
    } else {
        arr
    };
    // Bounds check + set on the target (possibly new) array
    let len = unsafe { *(target as *const i64) };
    if index < 0 || index >= len {
        eprintln!(
            "runtime error: array index {} out of bounds (length {})",
            index, len
        );
        std::process::exit(1);
    }
    unsafe {
        *((target as *mut i64).add(1 + index as usize)) = value;
    }
    target
}

pub(crate) extern "C" fn rt_array_len(arr: *const u8) -> i64 {
    unsafe { *(arr as *const i64) }
}

pub(crate) extern "C" fn rt_array_push(arr: *const u8, value: i64) -> *mut u8 {
    let old_len = unsafe { *(arr as *const i64) } as usize;
    let new_len = match old_len.checked_add(1) {
        Some(n) => n,
        None => {
            eprintln!("runtime error: array push overflow");
            std::process::exit(1);
        }
    };
    let layout = match checked_array_layout(new_len) {
        Some(l) => l,
        None => {
            eprintln!(
                "runtime error: array allocation overflow (length {})",
                new_len
            );
            std::process::exit(1);
        }
    };
    let new_alloc = unsafe { std::alloc::alloc_zeroed(layout) };
    if new_alloc.is_null() {
        eprintln!("turbo: fatal: memory allocation failed");
        std::process::exit(1);
    }
    register_alloc(new_alloc, layout);
    unsafe {
        *(new_alloc as *mut i64) = 1; // refcount = 1
    }
    let new_data = unsafe { new_alloc.add(8) };
    unsafe {
        *(new_data as *mut i64) = new_len as i64; // set length
                                                  // Copy old elements
        std::ptr::copy_nonoverlapping(arr.add(8), new_data.add(8), old_len * 8);
        // Append new element
        *((new_data as *mut i64).add(1 + old_len)) = value;
    }
    new_data
}

pub(crate) extern "C" fn rt_str_len(s: *const u8) -> i64 {
    if s.is_null() {
        return 0;
    }
    let cstr = unsafe { std::ffi::CStr::from_ptr(s as *const std::ffi::c_char) };
    cstr.to_bytes().len() as i64
}

pub(crate) extern "C" fn rt_str_concat(a: *const u8, b: *const u8) -> *const u8 {
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
    let c_string =
        cstring_or_empty(result);
    arena_str(c_string)
}

pub(crate) extern "C" fn rt_str_eq(a: *const u8, b: *const u8) -> i8 {
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
    if a_str == b_str {
        1
    } else {
        0
    }
}

pub(crate) extern "C" fn rt_struct_alloc(num_fields: i64) -> *mut u8 {
    if num_fields < 0 {
        eprintln!("runtime error: negative struct field count {}", num_fields);
        std::process::exit(1);
    }
    let data_size = match (num_fields as usize).checked_mul(8) {
        Some(s) => s.max(8),
        None => {
            eprintln!("runtime error: struct allocation overflow");
            std::process::exit(1);
        }
    };
    let total_size = data_size + 8; // +8 for refcount header
    let layout = std::alloc::Layout::from_size_align(total_size, 8).unwrap();
    let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
    if ptr.is_null() {
        eprintln!("turbo: fatal: memory allocation failed");
        std::process::exit(1);
    }
    register_alloc(ptr, layout);
    unsafe {
        *(ptr as *mut i64) = 1;
    } // refcount = 1
    unsafe { ptr.add(8) } // return pointer past refcount header
}

pub(crate) extern "C" fn rt_i64_to_str(n: i64) -> *const u8 {
    let s = format!("{}", n);
    let c = cstring_or_empty(s);
    arena_str(c)
}

pub(crate) extern "C" fn rt_f64_to_str(n: f64) -> *const u8 {
    let s = format!("{}", n);
    let c = cstring_or_empty(s);
    arena_str(c)
}

pub(crate) extern "C" fn rt_bool_to_str(b: i8) -> *const u8 {
    let s = if b != 0 { "true" } else { "false" };
    let c = cstring_or_empty(s);
    arena_str(c)
}

// ── Result type runtime functions ────────────────────────────────────

pub(crate) extern "C" fn rt_result_ok(value: i64) -> *mut u8 {
    let layout = std::alloc::Layout::from_size_align(8 + 16, 8).unwrap(); // +8 for refcount
    let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
    if ptr.is_null() {
        eprintln!("turbo: fatal: memory allocation failed");
        std::process::exit(1);
    }
    register_alloc(ptr, layout);
    unsafe {
        *(ptr as *mut i64) = 1;
    } // refcount = 1
    let data_ptr = unsafe { ptr.add(8) };
    unsafe {
        *(data_ptr as *mut i64) = 0; // tag = ok
        *((data_ptr as *mut i64).add(1)) = value;
    }
    data_ptr
}

pub(crate) extern "C" fn rt_result_err(value: i64) -> *mut u8 {
    let layout = std::alloc::Layout::from_size_align(8 + 16, 8).unwrap(); // +8 for refcount
    let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
    if ptr.is_null() {
        eprintln!("turbo: fatal: memory allocation failed");
        std::process::exit(1);
    }
    register_alloc(ptr, layout);
    unsafe {
        *(ptr as *mut i64) = 1;
    } // refcount = 1
    let data_ptr = unsafe { ptr.add(8) };
    unsafe {
        *(data_ptr as *mut i64) = 1; // tag = err
        *((data_ptr as *mut i64).add(1)) = value;
    }
    data_ptr
}

pub(crate) extern "C" fn rt_result_tag(result: *const u8) -> i64 {
    unsafe { *(result as *const i64) }
}

pub(crate) extern "C" fn rt_result_value(result: *const u8) -> i64 {
    unsafe { *((result as *const i64).add(1)) }
}

// ── Optional type runtime functions ──────────────────────────────────

pub(crate) extern "C" fn rt_option_some(value: i64) -> *mut u8 {
    let layout = std::alloc::Layout::from_size_align(8 + 16, 8).unwrap(); // +8 for refcount
    let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
    if ptr.is_null() {
        eprintln!("turbo: fatal: memory allocation failed");
        std::process::exit(1);
    }
    register_alloc(ptr, layout);
    unsafe {
        *(ptr as *mut i64) = 1;
    } // refcount = 1
    let data_ptr = unsafe { ptr.add(8) };
    unsafe {
        *(data_ptr as *mut i64) = 1; // tag = some
        *((data_ptr as *mut i64).add(1)) = value;
    }
    data_ptr
}

pub(crate) extern "C" fn rt_option_none() -> *mut u8 {
    let layout = std::alloc::Layout::from_size_align(8 + 16, 8).unwrap(); // +8 for refcount
    let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
    if ptr.is_null() {
        eprintln!("turbo: fatal: memory allocation failed");
        std::process::exit(1);
    }
    register_alloc(ptr, layout);
    unsafe {
        *(ptr as *mut i64) = 1;
    } // refcount = 1
    let data_ptr = unsafe { ptr.add(8) };
    unsafe {
        *(data_ptr as *mut i64) = 0; // tag = none
        *((data_ptr as *mut i64).add(1)) = 0;
    }
    data_ptr
}

pub(crate) extern "C" fn rt_option_tag(opt: *const u8) -> i64 {
    unsafe { *(opt as *const i64) }
}

pub(crate) extern "C" fn rt_option_value(opt: *const u8) -> i64 {
    unsafe { *((opt as *const i64).add(1)) }
}

// ── Standard library runtime functions ──────────────────────────────

pub(crate) extern "C" fn rt_str_split(s: *const u8, sep: *const u8) -> *mut u8 {
    let s = unsafe { std::ffi::CStr::from_ptr(s as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");
    let sep = unsafe { std::ffi::CStr::from_ptr(sep as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");
    let parts: Vec<&str> = s.split(sep).collect();
    let len = parts.len() as i64;
    // Array format: [refcount: i64][len: i64][ptr0: i64][ptr1: i64]...
    let layout = match checked_array_layout(len as usize) {
        Some(l) => l,
        None => {
            eprintln!("runtime error: split result too large");
            std::process::exit(1);
        }
    };
    let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
    if ptr.is_null() {
        eprintln!("turbo: fatal: memory allocation failed");
        std::process::exit(1);
    }
    register_alloc(ptr, layout);
    unsafe {
        *(ptr as *mut i64) = 1;
    } // refcount = 1
    let data_ptr = unsafe { ptr.add(8) };
    unsafe {
        *(data_ptr as *mut i64) = len;
    }
    for (i, part) in parts.iter().enumerate() {
        let cs =
            cstring_or_empty(*part);
        let p = arena_str(cs) as i64;
        unsafe {
            *((data_ptr as *mut i64).add(1 + i)) = p;
        }
    }
    data_ptr
}

pub(crate) extern "C" fn rt_str_trim(s: *const u8) -> *const u8 {
    let s = unsafe { std::ffi::CStr::from_ptr(s as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");
    let trimmed = s.trim();
    let cs =
        cstring_or_empty(trimmed);
    arena_str(cs)
}

pub(crate) extern "C" fn rt_str_upper(s: *const u8) -> *const u8 {
    let s = unsafe { std::ffi::CStr::from_ptr(s as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");
    let upper = s.to_uppercase();
    let cs = cstring_or_empty(upper);
    arena_str(cs)
}

pub(crate) extern "C" fn rt_str_lower(s: *const u8) -> *const u8 {
    let s = unsafe { std::ffi::CStr::from_ptr(s as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");
    let lower = s.to_lowercase();
    let cs = cstring_or_empty(lower);
    arena_str(cs)
}

pub(crate) extern "C" fn rt_str_starts_with(s: *const u8, prefix: *const u8) -> i8 {
    let s = unsafe { std::ffi::CStr::from_ptr(s as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");
    let prefix = unsafe { std::ffi::CStr::from_ptr(prefix as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");
    if s.starts_with(prefix) {
        1
    } else {
        0
    }
}

pub(crate) extern "C" fn rt_str_ends_with(s: *const u8, suffix: *const u8) -> i8 {
    let s = unsafe { std::ffi::CStr::from_ptr(s as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");
    let suffix = unsafe { std::ffi::CStr::from_ptr(suffix as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");
    if s.ends_with(suffix) {
        1
    } else {
        0
    }
}

pub(crate) extern "C" fn rt_str_replace(s: *const u8, from: *const u8, to: *const u8) -> *const u8 {
    let s = unsafe { std::ffi::CStr::from_ptr(s as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");
    let from = unsafe { std::ffi::CStr::from_ptr(from as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");
    let to_s = unsafe { std::ffi::CStr::from_ptr(to as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");
    let result = s.replace(from, to_s);
    let cs = cstring_or_empty(result);
    arena_str(cs)
}

pub(crate) extern "C" fn rt_str_char_at(s: *const u8, index: i64) -> *const u8 {
    let s = unsafe { std::ffi::CStr::from_ptr(s as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");
    if index < 0 {
        eprintln!("runtime error: char_at index {} is negative", index);
        std::process::exit(1);
    }
    if let Some(c) = s.chars().nth(index as usize) {
        arena_str(cstring_or_empty(c.to_string()))
    } else {
        eprintln!(
            "runtime error: string index {} out of bounds (length {})",
            index,
            s.chars().count()
        );
        std::process::exit(1);
    }
}

/// contains(s, sub) -> bool — returns true if s contains sub
pub(crate) extern "C" fn rt_str_contains(s: *const u8, sub: *const u8) -> i8 {
    let s = unsafe { std::ffi::CStr::from_ptr(s as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");
    let sub = unsafe { std::ffi::CStr::from_ptr(sub as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");
    if s.contains(sub) {
        1
    } else {
        0
    }
}

/// index_of(s, sub) -> i64 — returns byte offset or -1 if not found
pub(crate) extern "C" fn rt_str_index_of(s: *const u8, sub: *const u8) -> i64 {
    let s = unsafe { std::ffi::CStr::from_ptr(s as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");
    let sub = unsafe { std::ffi::CStr::from_ptr(sub as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");
    match s.find(sub) {
        Some(pos) => pos as i64,
        None => -1,
    }
}

/// join(arr, sep) -> str — join string array elements with separator
pub(crate) extern "C" fn rt_str_join(arr: *const u8, sep: *const u8) -> *const u8 {
    let sep = unsafe { std::ffi::CStr::from_ptr(sep as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");
    // arr is a Turbo array: first 8 bytes = length, then 8 bytes per element (string pointers)
    let len = unsafe { *(arr as *const i64) } as usize;
    let mut parts = Vec::with_capacity(len);
    for i in 0..len {
        let elem_ptr = unsafe { *((arr as *const i64).add(1 + i)) } as *const u8;
        let elem = unsafe { std::ffi::CStr::from_ptr(elem_ptr as *const std::ffi::c_char) }
            .to_str()
            .unwrap_or("");
        parts.push(elem.to_string());
    }
    let joined = parts.join(sep);
    let cs = cstring_or_empty(joined);
    arena_str(cs)
}

/// Practical cap on a single repeat() allocation. 256 MB is larger than any
/// legitimate string use-case but keeps a hostile input from aborting the
/// process via allocator panic.
const RT_STR_REPEAT_MAX_BYTES: usize = 256 * 1024 * 1024;

/// repeat(s, n) -> str — repeat string n times
pub(crate) extern "C" fn rt_str_repeat(s: *const u8, n: i64) -> *const u8 {
    // Mirrors `rt_str_repeat` in turbo_rt.c — rejects non-positive counts,
    // zero-length inputs, length*count overflow, and unreasonably large
    // totals before allocating. Rust's Vec allocator aborts on
    // capacity_overflow; since this function is extern "C" (unwind-forbidden)
    // the abort would take down the whole JIT process, so we cap up-front.
    let s = unsafe { std::ffi::CStr::from_ptr(s as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");
    if n <= 0 || s.is_empty() {
        return rt_empty_cstr();
    }
    let count = n as usize;
    let len = s.len();
    if count > (usize::MAX - 1) / len {
        eprintln!("[rt_str_repeat] overflow: len={} count={}", len, count);
        return rt_empty_cstr();
    }
    let total = len * count;
    if total > RT_STR_REPEAT_MAX_BYTES {
        eprintln!(
            "[rt_str_repeat] refusing allocation: len={} count={} total={} > cap {}",
            len, count, total, RT_STR_REPEAT_MAX_BYTES
        );
        return rt_empty_cstr();
    }
    let repeated = s.repeat(count);
    arena_str(
        cstring_or_empty(repeated),
    )
}

pub(crate) extern "C" fn rt_read_line() -> *const u8 {
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).unwrap_or(0);
    let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');
    let cs =
        cstring_or_empty(trimmed);
    arena_str(cs)
}

pub(crate) extern "C" fn rt_read_file(path: *const u8) -> *const u8 {
    let path = unsafe { std::ffi::CStr::from_ptr(path as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");
    match std::fs::read_to_string(path) {
        Ok(content) => {
            let cs = cstring_or_empty(content);
            arena_str(cs)
        }
        Err(e) => {
            eprintln!("runtime error: cannot read file '{}': {}", path, e);
            std::process::exit(1);
        }
    }
}

pub(crate) extern "C" fn rt_write_file(path: *const u8, content: *const u8) {
    let path = unsafe { std::ffi::CStr::from_ptr(path as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");
    let content = unsafe { std::ffi::CStr::from_ptr(content as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");
    if let Err(e) = std::fs::write(path, content) {
        eprintln!("runtime error: cannot write file '{}': {}", path, e);
        std::process::exit(1);
    }
}

/// try_read_file(path) -> str ! str
///
/// Returns an `ok(contents)` Result on success, or an `err(message)` Result
/// on any I/O failure. Never panics — this is the fallible counterpart to
/// `rt_read_file` (v0.8.0 "Safe Core").
pub(crate) extern "C" fn rt_try_read_file(path: *const u8) -> *mut u8 {
    let path_str = unsafe { std::ffi::CStr::from_ptr(path as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");
    match std::fs::read_to_string(path_str) {
        Ok(content) => {
            let cs = cstring_or_empty(content);
            let ptr = arena_str(cs) as i64;
            rt_result_ok(ptr)
        }
        Err(e) => {
            let cs = cstring_or_empty(e.to_string());
            let ptr = arena_str(cs) as i64;
            rt_result_err(ptr)
        }
    }
}

/// try_write_file(path, content) -> bool ! str
pub(crate) extern "C" fn rt_try_write_file(path: *const u8, content: *const u8) -> *mut u8 {
    let path_str = unsafe { std::ffi::CStr::from_ptr(path as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");
    let content_str = unsafe { std::ffi::CStr::from_ptr(content as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");
    match std::fs::write(path_str, content_str) {
        Ok(()) => rt_result_ok(1),
        Err(e) => {
            let cs = cstring_or_empty(e.to_string());
            let ptr = arena_str(cs) as i64;
            rt_result_err(ptr)
        }
    }
}

/// JIT-side twin of `rt_exec` in `turbo_rt.c`. Mirrors the C hardening:
/// rejects commands containing shell metacharacters and executes the
/// tokenized argv directly (no `/bin/sh -c`). Historical `sh -c` path
/// was an RCE vector — do NOT reintroduce it.
const RT_EXEC_META: &[char] = &[';', '|', '&', '$', '`', '(', ')', '<', '>', '\n', '\\'];
const RT_EXEC_MAX_ARGS: usize = 64;

pub(crate) extern "C" fn rt_exec(cmd: *const u8) -> *const u8 {
    let cmd = unsafe { std::ffi::CStr::from_ptr(cmd as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");
    if cmd.chars().any(|c| RT_EXEC_META.contains(&c)) {
        eprintln!(
            "rt_exec: refusing command with shell metacharacter: {}",
            cmd
        );
        return rt_empty_cstr();
    }
    let tokens: Vec<&str> = cmd.split_whitespace().collect();
    if tokens.is_empty() || tokens.len() > RT_EXEC_MAX_ARGS {
        eprintln!(
            "rt_exec: refusing empty or oversized command ({} tokens)",
            tokens.len()
        );
        return rt_empty_cstr();
    }
    let output = std::process::Command::new(tokens[0])
        .args(&tokens[1..])
        .output();
    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            let combined = if stderr.is_empty() {
                stdout.into_owned()
            } else {
                format!("{}{}", stdout, stderr)
            };
            let cs = cstring_or_empty(combined);
            arena_str(cs)
        }
        Err(e) => {
            let msg = format!("error: exec failed: {}", e);
            let cs =
                cstring_or_empty(msg);
            arena_str(cs)
        }
    }
}

pub(crate) extern "C" fn rt_env_get(name: *const u8) -> *const u8 {
    let name = unsafe { std::ffi::CStr::from_ptr(name as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");
    let val = std::env::var(name).unwrap_or_default();
    let cs = cstring_or_empty(val);
    arena_str(cs)
}

pub(crate) extern "C" fn rt_pow(base: i64, exp: i64) -> i64 {
    // Mirrors `rt_pow` in turbo_rt.c: reject negative exponents and trap on
    // overflow instead of silently wrapping.
    if exp < 0 {
        eprintln!("runtime error: negative exponent in pow");
        std::process::exit(1);
    }
    let mut result: i64 = 1;
    for _ in 0..exp {
        match result.checked_mul(base) {
            Some(v) => result = v,
            None => {
                eprintln!("runtime error: integer overflow in pow");
                std::process::exit(1);
            }
        }
    }
    result
}

pub(crate) extern "C" fn rt_sqrt(x: f64) -> f64 {
    x.sqrt()
}

// ── Math builtins ──────────────────────────────────────────────────

pub(crate) extern "C" fn rt_floor(x: f64) -> i64 {
    x.floor() as i64
}

pub(crate) extern "C" fn rt_ceil(x: f64) -> i64 {
    x.ceil() as i64
}

pub(crate) extern "C" fn rt_round(x: f64) -> i64 {
    x.round() as i64
}

pub(crate) extern "C" fn rt_sin(x: f64) -> f64 {
    x.sin()
}

pub(crate) extern "C" fn rt_cos(x: f64) -> f64 {
    x.cos()
}

pub(crate) extern "C" fn rt_tan(x: f64) -> f64 {
    x.tan()
}

pub(crate) extern "C" fn rt_log_builtin(x: f64) -> f64 {
    x.ln()
}

pub(crate) extern "C" fn rt_log2_builtin(x: f64) -> f64 {
    x.log2()
}

pub(crate) extern "C" fn rt_log10(x: f64) -> f64 {
    x.log10()
}

pub(crate) extern "C" fn rt_exp(x: f64) -> f64 {
    x.exp()
}

thread_local! {
    static XORSHIFT_STATE: Cell<u64> = Cell::new(0);
}

fn xorshift64_next() -> u64 {
    XORSHIFT_STATE.with(|cell| {
        let mut s = cell.get();
        if s == 0 {
            use std::collections::hash_map::RandomState;
            use std::hash::{BuildHasher, Hasher};
            let rs = RandomState::new();
            let mut h = rs.build_hasher();
            h.write_u64(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos() as u64,
            );
            s = h.finish();
            if s == 0 {
                s = 1;
            }
        }
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        cell.set(s);
        s
    })
}

pub(crate) extern "C" fn rt_random() -> f64 {
    (xorshift64_next() as f64) / (u64::MAX as f64)
}

pub(crate) extern "C" fn rt_random_range(min_val: i64, max_val: i64) -> i64 {
    if max_val < min_val {
        return min_val;
    }
    let range = (max_val as u64).wrapping_sub(min_val as u64).wrapping_add(1);
    min_val + (xorshift64_next() % range) as i64
}

// ── System builtins ────────────────────────────────────────────────

pub(crate) extern "C" fn rt_exit(code: i64) {
    std::process::exit(code as i32);
}

pub(crate) extern "C" fn rt_args() -> *mut u8 {
    // Return an empty array for now
    rt_array_alloc(0)
}

// ── String parsing builtins ────────────────────────────────────────

pub(crate) extern "C" fn rt_substring(s: *const u8, start: i64, end: i64) -> *const u8 {
    let s_str = if s.is_null() {
        ""
    } else {
        unsafe {
            std::ffi::CStr::from_ptr(s as *const i8)
                .to_str()
                .unwrap_or("")
        }
    };
    let slen = s_str.len() as i64;
    let start = start.max(0).min(slen) as usize;
    let end = end.max(0).min(slen) as usize;
    if start >= end {
        return arena_str(cstring_or_empty(""));
    }
    let sub = &s_str[start..end];
    arena_str(cstring_or_empty(sub))
}

pub(crate) extern "C" fn rt_pad_left(s: *const u8, width: i64, pad_char: *const u8) -> *const u8 {
    let s_str = if s.is_null() {
        ""
    } else {
        unsafe {
            std::ffi::CStr::from_ptr(s as *const i8)
                .to_str()
                .unwrap_or("")
        }
    };
    let pad_str = if pad_char.is_null() {
        " "
    } else {
        unsafe {
            std::ffi::CStr::from_ptr(pad_char as *const i8)
                .to_str()
                .unwrap_or(" ")
        }
    };
    let c = pad_str.chars().next().unwrap_or(' ');
    let slen = s_str.len() as i64;
    if slen >= width {
        return arena_str(
            cstring_or_empty(s_str),
        );
    }
    let pad_count = (width - slen) as usize;
    let mut result = String::with_capacity(width as usize);
    for _ in 0..pad_count {
        result.push(c);
    }
    result.push_str(s_str);
    arena_str(
        cstring_or_empty(result),
    )
}

pub(crate) extern "C" fn rt_pad_right(s: *const u8, width: i64, pad_char: *const u8) -> *const u8 {
    let s_str = if s.is_null() {
        ""
    } else {
        unsafe {
            std::ffi::CStr::from_ptr(s as *const i8)
                .to_str()
                .unwrap_or("")
        }
    };
    let pad_str = if pad_char.is_null() {
        " "
    } else {
        unsafe {
            std::ffi::CStr::from_ptr(pad_char as *const i8)
                .to_str()
                .unwrap_or(" ")
        }
    };
    let c = pad_str.chars().next().unwrap_or(' ');
    let slen = s_str.len() as i64;
    if slen >= width {
        return arena_str(
            cstring_or_empty(s_str),
        );
    }
    let pad_count = (width - slen) as usize;
    let mut result = String::with_capacity(width as usize);
    result.push_str(s_str);
    for _ in 0..pad_count {
        result.push(c);
    }
    arena_str(
        cstring_or_empty(result),
    )
}

pub(crate) extern "C" fn rt_str_to_int(s: *const u8) -> *mut u8 {
    let s_str = if s.is_null() {
        ""
    } else {
        unsafe {
            std::ffi::CStr::from_ptr(s as *const i8)
                .to_str()
                .unwrap_or("")
        }
    };
    match s_str.parse::<i64>() {
        Ok(val) => rt_result_ok(val),
        Err(_) => {
            let msg = format!("cannot parse '{}' as integer", s_str);
            let cs = cstring_or_empty(msg);
            let ptr = arena_str(cs);
            rt_result_err(ptr as i64)
        }
    }
}

pub(crate) extern "C" fn rt_str_to_float(s: *const u8) -> *mut u8 {
    let s_str = if s.is_null() {
        ""
    } else {
        unsafe {
            std::ffi::CStr::from_ptr(s as *const i8)
                .to_str()
                .unwrap_or("")
        }
    };
    match s_str.parse::<f64>() {
        Ok(val) => {
            let bits = val.to_bits() as i64;
            rt_result_ok(bits)
        }
        Err(_) => {
            let msg = format!("cannot parse '{}' as float", s_str);
            let cs = cstring_or_empty(msg);
            let ptr = arena_str(cs);
            rt_result_err(ptr as i64)
        }
    }
}

// ── HTTP + JSON runtime functions ───────────────────────────────────

/// Maximum HTTP request body we will accept from a client, in bytes.
/// Mirrors `RT_HTTP_MAX_BODY` in the C runtime (`turbo_rt.c`).
const RT_HTTP_MAX_BODY: usize = 32 * 1024 * 1024;

/// Allocate an empty C string, used as a safe return value from error paths
/// in string/HTTP helpers. Never panics.
fn rt_empty_cstr() -> *const u8 {
    arena_str(cstring_or_empty(""))
}

/// Validate that `url` is a well-formed http:// or https:// URL safe to hand
/// to curl. Mirrors `rt_url_is_http` in the C runtime — reject anything that
/// could be interpreted as a curl flag or a non-http(s) scheme (file://,
/// gopher://, dict://, etc.). This is the JIT-side twin of the hardening in
/// `turbo_rt.c`.
fn rt_url_is_http(url: &str) -> bool {
    if url.is_empty() {
        return false;
    }
    // Anything that looks like a flag gets rejected — curl would otherwise
    // interpret it as an argument even with -- in some historical builds.
    if url.starts_with('-') {
        return false;
    }
    let lower = url.to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

/// HTTP GET via system curl. Returns response body as a C string.
/// Hardened: rejects non-http(s) schemes and flag-shaped inputs, pins the
/// protocol allowlist, bounds total time, and uses `--` to prevent flag
/// injection. Keep in sync with `rt_http_get` in `turbo_rt.c`.
pub(crate) extern "C" fn rt_http_get(url: *const u8) -> *const u8 {
    if url.is_null() {
        eprintln!("[rt_http] blocked non-http(s) URL: (null)");
        return rt_empty_cstr();
    }
    let url = unsafe { std::ffi::CStr::from_ptr(url as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");
    if !rt_url_is_http(url) {
        eprintln!("[rt_http] blocked non-http(s) URL: {}", url);
        return rt_empty_cstr();
    }
    let output = std::process::Command::new("curl")
        .arg("-s")
        .arg("-L")
        .arg("--proto")
        .arg("=http,https")
        .arg("--max-time")
        .arg("30")
        .arg("--max-redirs")
        .arg("5")
        .arg("--")
        .arg(url)
        .output();
    match output {
        Ok(out) => {
            let body = String::from_utf8_lossy(&out.stdout).to_string();
            let cs = cstring_or_empty(body);
            arena_str(cs)
        }
        Err(e) => {
            eprintln!("[rt_http] curl exec failed: {}", e);
            rt_empty_cstr()
        }
    }
}

/// HTTP POST via system curl. Takes URL and body, returns response body as a C string.
/// Hardened with the same scheme validation, protocol pinning, and flag-injection
/// guards as `rt_http_get`. Keep in sync with `rt_http_post` in `turbo_rt.c`.
pub(crate) extern "C" fn rt_http_post(url: *const u8, body: *const u8) -> *const u8 {
    if url.is_null() {
        eprintln!("[rt_http] blocked non-http(s) URL: (null)");
        return rt_empty_cstr();
    }
    let url = unsafe { std::ffi::CStr::from_ptr(url as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");
    if !rt_url_is_http(url) {
        eprintln!("[rt_http] blocked non-http(s) URL: {}", url);
        return rt_empty_cstr();
    }
    let body_str = if body.is_null() {
        ""
    } else {
        unsafe { std::ffi::CStr::from_ptr(body as *const std::ffi::c_char) }
            .to_str()
            .unwrap_or("")
    };
    let output = std::process::Command::new("curl")
        .arg("-s")
        .arg("-L")
        .arg("--proto")
        .arg("=http,https")
        .arg("--max-time")
        .arg("30")
        .arg("--max-redirs")
        .arg("5")
        .arg("-X")
        .arg("POST")
        .arg("-H")
        .arg("Content-Type: application/json")
        .arg("-d")
        .arg(body_str)
        .arg("--")
        .arg(url)
        .output();
    match output {
        Ok(out) => {
            let resp = String::from_utf8_lossy(&out.stdout).to_string();
            let cs = cstring_or_empty(resp);
            arena_str(cs)
        }
        Err(e) => {
            eprintln!("[rt_http] curl exec failed: {}", e);
            rt_empty_cstr()
        }
    }
}

/// Extract a top-level key from a JSON string. Returns the value as a string.
/// Handles string values, numbers, booleans, and null.
pub(crate) extern "C" fn rt_json_get(json: *const u8, key: *const u8) -> *const u8 {
    let json_str = unsafe { std::ffi::CStr::from_ptr(json as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");
    let key_str = unsafe { std::ffi::CStr::from_ptr(key as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");

    let parsed: Result<serde_json::Value, _> = serde_json::from_str(json_str);
    match parsed {
        Ok(val) => {
            if let Some(v) = val.get(key_str) {
                let result = match v {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Null => "null".to_string(),
                    other => other.to_string(),
                };
                let cs = cstring_or_empty(result);
                arena_str(cs)
            } else {
                arena_str(cstring_or_empty(""))
            }
        }
        Err(_) => arena_str(cstring_or_empty("")),
    }
}

/// Build a JSON object string from a key and value: {"key": "value"}
pub(crate) extern "C" fn rt_json_stringify(key: *const u8, value: *const u8) -> *const u8 {
    let key_str = unsafe { std::ffi::CStr::from_ptr(key as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");
    let value_str = unsafe { std::ffi::CStr::from_ptr(value as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");
    let mut map = serde_json::Map::new();
    map.insert(
        key_str.to_string(),
        serde_json::Value::String(value_str.to_string()),
    );
    let result = serde_json::Value::Object(map).to_string();
    let cs =
        cstring_or_empty(result);
    arena_str(cs)
}

pub(crate) extern "C" fn rt_json_root(json: *const u8) -> *const u8 {
    let json_str = unsafe { std::ffi::CStr::from_ptr(json as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("")
        .trim();
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(json_str);
    match parsed {
        Ok(serde_json::Value::String(s)) => {
            let cs =
                cstring_or_empty(s);
            arena_str(cs)
        }
        Ok(other) => {
            let cs = cstring_or_empty(other.to_string());
            arena_str(cs)
        }
        Err(_) => {
            let cs = cstring_or_empty(json_str);
            arena_str(cs)
        }
    }
}

pub(crate) extern "C" fn rt_str_to_i64(s: *const u8) -> i64 {
    if s.is_null() {
        return 0;
    }
    unsafe { std::ffi::CStr::from_ptr(s as *const std::ffi::c_char) }
        .to_str()
        .ok()
        .and_then(|v| v.trim().parse::<i64>().ok())
        .unwrap_or(0)
}

pub(crate) extern "C" fn rt_str_to_f64(s: *const u8) -> f64 {
    if s.is_null() {
        return 0.0;
    }
    unsafe { std::ffi::CStr::from_ptr(s as *const std::ffi::c_char) }
        .to_str()
        .ok()
        .and_then(|v| v.trim().parse::<f64>().ok())
        .unwrap_or(0.0)
}

pub(crate) extern "C" fn rt_str_to_bool(s: *const u8) -> i8 {
    if s.is_null() {
        return 0;
    }
    let value = unsafe { std::ffi::CStr::from_ptr(s as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if value == "true" {
        1
    } else {
        0
    }
}

// ── HTTP server runtime functions ───────────────────────────────────

/// Route handler function pointer: (env_ptr, request_body_cstr) -> response_cstr
pub(crate) type RouteHandler = extern "C" fn(*const u8, *const u8) -> *const u8;

pub(crate) struct HttpServer {
    port: u16,
    /// Interface to bind to. Defaults to "127.0.0.1"; `rt_http_server_public`
    /// sets this to "0.0.0.0" for opt-in external exposure.
    bind_host: &'static str,
    routes: Vec<(String, String, RouteHandler, *const u8)>, // (method, path, handler_fn, env_ptr)
}

unsafe impl Send for HttpServer {}

pub(crate) static HTTP_SERVERS: Mutex<Vec<HttpServer>> = Mutex::new(Vec::new());
const RT_HTTP_MAX_SERVERS: usize = 16;
const RT_HTTP_MAX_ROUTES: usize = 64;
const RT_HTTP_MAX_ACTIVE_CONNECTIONS: usize = 256;
/// Maximum bytes for a single HTTP header line.
const RT_HTTP_MAX_HEADER_LINE: usize = 8192;
/// Maximum total bytes for all HTTP headers combined.
const RT_HTTP_MAX_HEADERS_TOTAL: usize = 65536;
const RT_RESPONSE_SEP: char = '\u{1f}';

/// Create a new HTTP server bound to localhost. Returns a server id (index).
pub(crate) extern "C" fn rt_http_server(port: i64) -> i64 {
    let server = HttpServer {
        port: port as u16,
        bind_host: "127.0.0.1",
        routes: Vec::new(),
    };
    let mut servers = HTTP_SERVERS.lock().unwrap();
    if servers.len() >= RT_HTTP_MAX_SERVERS {
        eprintln!("runtime error: max {} HTTP servers", RT_HTTP_MAX_SERVERS);
        std::process::exit(1);
    }
    let id = servers.len() as i64;
    servers.push(server);
    id
}

/// Create a new HTTP server bound to all interfaces (0.0.0.0). Callers
/// opt in explicitly; meant for deliberately public services.
pub(crate) extern "C" fn rt_http_server_public(port: i64) -> i64 {
    let server = HttpServer {
        port: port as u16,
        bind_host: "0.0.0.0",
        routes: Vec::new(),
    };
    let mut servers = HTTP_SERVERS.lock().unwrap();
    if servers.len() >= RT_HTTP_MAX_SERVERS {
        eprintln!("runtime error: max {} HTTP servers", RT_HTTP_MAX_SERVERS);
        std::process::exit(1);
    }
    let id = servers.len() as i64;
    servers.push(server);
    id
}

/// Register a route handler on the server.
pub(crate) extern "C" fn rt_http_route(
    server_id: i64,
    method: *const u8,
    path: *const u8,
    handler: *const u8,
    env_ptr: *const u8,
) {
    let method = unsafe { std::ffi::CStr::from_ptr(method as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("")
        .to_string();
    let path = unsafe { std::ffi::CStr::from_ptr(path as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("")
        .to_string();
    let handler: RouteHandler = unsafe { std::mem::transmute(handler) };

    let mut servers = HTTP_SERVERS.lock().unwrap();
    if let Some(server) = servers.get_mut(server_id as usize) {
        if server.routes.len() >= RT_HTTP_MAX_ROUTES {
            eprintln!(
                "runtime error: max {} routes per HTTP server",
                RT_HTTP_MAX_ROUTES
            );
            std::process::exit(1);
        }
        server.routes.push((method, path, handler, env_ptr));
    } else {
        eprintln!("runtime error: invalid HTTP server id {}", server_id);
    }
}

/// Wrapper to allow sending route data across threads.
/// Safety: JIT function pointers and env pointers are valid for the process lifetime.
struct SendableRoutes(Vec<(String, String, RouteHandler, *const u8)>);
unsafe impl Send for SendableRoutes {}
unsafe impl Sync for SendableRoutes {}

struct ActiveConnectionGuard(std::sync::Arc<std::sync::atomic::AtomicUsize>);

impl Drop for ActiveConnectionGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
    }
}

/// Read a line from a BufReader with an upper bound on bytes read.
/// Returns Ok(n) where n is bytes read (0 = EOF), or Err on I/O error
/// or if the line exceeds `max_bytes`.
fn bounded_read_line(
    reader: &mut std::io::BufReader<std::net::TcpStream>,
    buf: &mut String,
    max_bytes: usize,
) -> std::io::Result<usize> {
    use std::io::BufRead;
    let n = reader.read_line(buf)?;
    if buf.len() > max_bytes {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "header line too long",
        ));
    }
    Ok(n)
}

fn parse_rt_response(resp: &str) -> Option<(u16, &str, &str)> {
    let mut typed = resp.splitn(3, RT_RESPONSE_SEP);
    if let (Some(status), Some(content_type), Some(body)) =
        (typed.next(), typed.next(), typed.next())
    {
        if let Ok(code) = status.parse::<u16>() {
            return Some((code, content_type, body));
        }
    }

    resp.split_once(':').and_then(|(status, body)| {
        status
            .parse::<u16>()
            .ok()
            .map(|code| (code, "text/plain", body))
    })
}

/// Handle a single HTTP connection with keep-alive support.
pub(crate) fn handle_http_connection(
    stream: std::net::TcpStream,
    routes: &[(String, String, RouteHandler, *const u8)],
) {
    use std::io::{Read, Write};

    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(10)));
    let write_stream = match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    };
    let mut reader = std::io::BufReader::new(stream);
    let mut writer = write_stream;

    loop {
        // Read request line (bounded)
        let mut request_line = String::new();
        match bounded_read_line(&mut reader, &mut request_line, RT_HTTP_MAX_HEADER_LINE) {
            Ok(0) => break, // Connection closed
            Err(_) => {
                let _ = writer.write_all(
                    b"HTTP/1.1 431 Request Header Fields Too Large\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                );
                return;
            }
            _ => {}
        }

        let parts: Vec<&str> = request_line.split_whitespace().collect();
        if parts.len() < 2 {
            break;
        }
        let method = parts[0];
        let raw_path = parts[1];

        // Split path and query string
        let (path, query) = if let Some(idx) = raw_path.find('?') {
            (&raw_path[..idx], &raw_path[idx + 1..])
        } else {
            (raw_path, "")
        };

        // Read headers (bounded per-line and total)
        let mut content_length: usize = 0;
        let mut headers_raw = String::new();
        let mut keep_alive = true; // HTTP/1.1 default
        loop {
            let mut line = String::new();
            match bounded_read_line(&mut reader, &mut line, RT_HTTP_MAX_HEADER_LINE) {
                Ok(0) => break,
                Err(_) => {
                    let _ = writer.write_all(
                        b"HTTP/1.1 431 Request Header Fields Too Large\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    );
                    return;
                }
                _ => {}
            }
            if line.trim().is_empty() {
                break;
            }
            if headers_raw.len() + line.len() > RT_HTTP_MAX_HEADERS_TOTAL {
                let _ = writer.write_all(
                    b"HTTP/1.1 431 Request Header Fields Too Large\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                );
                return;
            }
            let lower = line.to_lowercase();
            if lower.starts_with("content-length:") {
                // Parse as i64 first so we can cleanly reject negative values
                // and overflow instead of silently collapsing to usize::MAX
                // or OOM'ing on a later `vec![0u8; N]`. Mirrors the
                // strtoll + bounds logic in `turbo_rt.c`.
                let raw: i64 = line
                    .split(':')
                    .nth(1)
                    .unwrap_or("0")
                    .trim()
                    .parse()
                    .unwrap_or(-1);
                if raw < 0 || raw as u64 > RT_HTTP_MAX_BODY as u64 {
                    eprintln!("[rt_http] rejecting Content-Length: {}", raw);
                    let _ = writer.write_all(
                        b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    );
                    return;
                }
                content_length = raw as usize;
            }
            if lower.starts_with("connection:") {
                let val = line.split(':').nth(1).unwrap_or("").trim().to_lowercase();
                keep_alive = val != "close";
            }
            headers_raw.push_str(&line);
        }

        // Read body
        let mut body = vec![0u8; content_length];
        if content_length > 0 && reader.read_exact(&mut body).is_err() {
            break;
        }

        let conn_header = if keep_alive { "keep-alive" } else { "close" };

        // Find matching route
        let mut matched = false;
        for (route_method, route_path, handler, env_ptr) in routes {
            if route_method == method && route_path == path {
                let body_str = String::from_utf8_lossy(&body);
                let req_structured = format!(
                    "{}\x01{}\x01{}\x01{}\x01{}",
                    method,
                    path,
                    query,
                    headers_raw.trim(),
                    body_str
                );
                let req_cstr = cstring_or_empty(req_structured);
                let response_ptr = handler(*env_ptr, req_cstr.as_ptr() as *const u8);

                if response_ptr.is_null() {
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nConnection: {}\r\nContent-Length: 0\r\n\r\n",
                        conn_header
                    );
                    let _ = writer.write_all(resp.as_bytes());
                } else {
                    let resp = unsafe {
                        std::ffi::CStr::from_ptr(response_ptr as *const std::ffi::c_char)
                    }
                    .to_str()
                    .unwrap_or("");
                    if let Some((code, content_type, resp_body)) = parse_rt_response(resp) {
                        let status_text = match code {
                            200 => "OK",
                            201 => "Created",
                            204 => "No Content",
                            301 | 302 => "Redirect",
                            400 => "Bad Request",
                            401 => "Unauthorized",
                            403 => "Forbidden",
                            404 => "Not Found",
                            500 => "Internal Server Error",
                            _ => "OK",
                        };
                        // Sanitize content_type to prevent header injection
                        let safe_ct = content_type.replace(['\r', '\n'], "");
                        let http_resp = format!(
                            "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nConnection: {}\r\nContent-Length: {}\r\n\r\n{}",
                            code,
                            status_text,
                            safe_ct,
                            conn_header,
                            resp_body.len(),
                            resp_body
                        );
                        let _ = writer.write_all(http_resp.as_bytes());
                    } else {
                        let http_resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nConnection: {}\r\nContent-Length: {}\r\n\r\n{}",
                            conn_header, resp.len(), resp
                        );
                        let _ = writer.write_all(http_resp.as_bytes());
                    }
                }
                matched = true;
                break;
            }
        }

        if !matched {
            let not_found = format!(
                "HTTP/1.1 404 Not Found\r\nConnection: {}\r\nContent-Length: 9\r\n\r\nNot Found",
                conn_header
            );
            let _ = writer.write_all(not_found.as_bytes());
        }

        let _ = writer.flush();

        if !keep_alive {
            break;
        }
    }
}

/// Start the HTTP server. Spawns a thread per connection with keep-alive.
pub(crate) extern "C" fn rt_http_listen(server_id: i64) {
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    let (host, port, routes) = {
        let servers = HTTP_SERVERS.lock().unwrap();
        let Some(server) = servers.get(server_id as usize) else {
            eprintln!("runtime error: invalid HTTP server id {}", server_id);
            return;
        };
        let host = server.bind_host;
        let port = server.port;
        let routes: Vec<(String, String, RouteHandler, *const u8)> = server.routes.clone();
        (host, port, routes)
    };

    let routes = Arc::new(SendableRoutes(routes));
    let active_connections = Arc::new(AtomicUsize::new(0));
    let addr = format!("{}:{}", host, port);
    let listener = TcpListener::bind(&addr).expect("failed to bind HTTP server");

    for stream in listener.incoming().flatten() {
        if active_connections.fetch_add(1, Ordering::AcqRel) >= RT_HTTP_MAX_ACTIVE_CONNECTIONS {
            active_connections.fetch_sub(1, Ordering::AcqRel);
            let mut stream = stream;
            let _ = std::io::Write::write_all(
                &mut stream,
                b"HTTP/1.1 503 Service Unavailable\r\nContent-Type: text/plain\r\nConnection: close\r\nContent-Length: 18\r\n\r\nserver overloaded\n",
            );
            continue;
        }
        let routes = Arc::clone(&routes);
        let active_connections = Arc::clone(&active_connections);
        std::thread::spawn(move || {
            let _guard = ActiveConnectionGuard(active_connections);
            handle_http_connection(stream, &routes.0);
        });
    }
}

/// Build a response string in "STATUS<sep>text/plain<sep>BODY" format.
pub(crate) extern "C" fn rt_respond(status: i64, body: *const u8) -> *const u8 {
    let content_type = rt_alloc_string("text/plain");
    rt_respond_typed(status, content_type, body)
}

pub(crate) extern "C" fn rt_respond_typed(
    status: i64,
    content_type: *const u8,
    body: *const u8,
) -> *const u8 {
    let content_type_str =
        unsafe { std::ffi::CStr::from_ptr(content_type as *const std::ffi::c_char) }
            .to_str()
            .unwrap_or("text/plain");
    let body_str = unsafe { std::ffi::CStr::from_ptr(body as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");
    let response =
        format!("{status}{RT_RESPONSE_SEP}{content_type_str}{RT_RESPONSE_SEP}{body_str}");
    let cs = cstring_or_empty(response);
    arena_str(cs)
}

/// Helper: extract Nth field from structured request (fields separated by \x01)
fn req_field(req: *const u8, index: usize) -> *const u8 {
    if req.is_null() {
        return rt_alloc_string("");
    }
    let cstr = unsafe { std::ffi::CStr::from_ptr(req as *const std::ffi::c_char) };
    let s = cstr.to_str().unwrap_or("");
    let parts: Vec<&str> = s.splitn(5, '\x01').collect();
    let field = parts.get(index).copied().unwrap_or("");
    rt_alloc_string(field)
}

fn rt_alloc_string(s: &str) -> *const u8 {
    let cs = cstring_or_empty(s);
    arena_str(cs)
}

/// Extract HTTP method from structured request
pub(crate) extern "C" fn rt_request_method(req: *const u8) -> *const u8 {
    req_field(req, 0)
}

/// Extract path from structured request
pub(crate) extern "C" fn rt_request_path(req: *const u8) -> *const u8 {
    req_field(req, 1)
}

/// Extract query parameter by key from structured request
pub(crate) extern "C" fn rt_request_query(req: *const u8, key: *const u8) -> *const u8 {
    if req.is_null() || key.is_null() {
        return rt_alloc_string("");
    }
    let cstr = unsafe { std::ffi::CStr::from_ptr(req as *const std::ffi::c_char) };
    let s = cstr.to_str().unwrap_or("");
    let parts: Vec<&str> = s.splitn(5, '\x01').collect();
    let qs = parts.get(2).copied().unwrap_or("");

    let key_cstr = unsafe { std::ffi::CStr::from_ptr(key as *const std::ffi::c_char) };
    let key_str = key_cstr.to_str().unwrap_or("");

    for pair in qs.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            if k == key_str {
                return rt_alloc_string(v);
            }
        }
    }
    rt_alloc_string("")
}

/// Extract header value by name (case-insensitive) from structured request
pub(crate) extern "C" fn rt_request_header(req: *const u8, name: *const u8) -> *const u8 {
    if req.is_null() || name.is_null() {
        return rt_alloc_string("");
    }
    let cstr = unsafe { std::ffi::CStr::from_ptr(req as *const std::ffi::c_char) };
    let s = cstr.to_str().unwrap_or("");
    let parts: Vec<&str> = s.splitn(5, '\x01').collect();
    let headers = parts.get(3).copied().unwrap_or("");

    let name_cstr = unsafe { std::ffi::CStr::from_ptr(name as *const std::ffi::c_char) };
    let name_str = name_cstr.to_str().unwrap_or("").to_lowercase();

    for line in headers.split("\r\n") {
        if let Some((k, v)) = line.split_once(':') {
            if k.trim().to_lowercase() == name_str {
                return rt_alloc_string(v.trim());
            }
        }
    }
    rt_alloc_string("")
}

/// Extract body from structured request (field 4) with backward compat
pub(crate) extern "C" fn rt_request_body(req: *const u8) -> *const u8 {
    if req.is_null() {
        return rt_alloc_string("");
    }
    let cstr = unsafe { std::ffi::CStr::from_ptr(req as *const std::ffi::c_char) };
    let s = cstr.to_str().unwrap_or("");
    // If structured request (contains \x01), extract body field
    if s.contains('\x01') {
        return req_field(req, 4);
    }
    // Backward compat: plain string is the body
    req
}

// ── Async runtime functions ─────────────────────────────────────────

/// Sleep the current thread for `ms` milliseconds.
pub(crate) extern "C" fn rt_sleep_ms(ms: i64) {
    std::thread::sleep(std::time::Duration::from_millis(ms as u64));
}

/// Spawn a thunk with an args pointer on a new OS thread.
/// The thunk is `extern "C" fn(args_ptr: *mut u8) -> i64`.
/// Returns a pointer to a heap-allocated JoinHandle.
pub(crate) extern "C" fn rt_spawn_with_args(
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
pub(crate) extern "C" fn rt_await_handle(handle_ptr: *mut u8) -> i64 {
    if handle_ptr.is_null() {
        return 0;
    }
    let handle: Box<std::thread::JoinHandle<i64>> =
        unsafe { Box::from_raw(handle_ptr as *mut std::thread::JoinHandle<i64>) };
    handle.join().unwrap_or(0)
}

// ── Channel runtime functions ────────────────────────────────────────

/// Create a new channel. Returns a heap-allocated struct: [refcount: i64][sender_ptr: i64, receiver_ptr: i64].
pub(crate) extern "C" fn rt_channel_create() -> *mut u8 {
    let (tx, rx) = std::sync::mpsc::channel::<i64>();
    let tx_box = Box::into_raw(Box::new(tx)) as i64;
    let rx_box = Box::into_raw(Box::new(rx)) as i64;

    let layout = std::alloc::Layout::from_size_align(8 + 16, 8).unwrap();
    let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
    if ptr.is_null() {
        eprintln!("turbo: fatal: memory allocation failed");
        std::process::exit(1);
    }
    register_alloc(ptr, layout);
    unsafe {
        *(ptr as *mut i64) = 1;
    } // refcount = 1
    let data_ptr = unsafe { ptr.add(8) };
    unsafe {
        *(data_ptr as *mut i64) = tx_box;
        *((data_ptr as *mut i64).add(1)) = rx_box;
    }
    data_ptr
}

/// Send a value on a channel.
pub(crate) extern "C" fn rt_channel_send(ch: *const u8, value: i64) {
    let tx_ptr = unsafe { *(ch as *const i64) } as *mut std::sync::mpsc::Sender<i64>;
    let tx = unsafe { &*tx_ptr };
    tx.send(value).ok();
}

/// Receive a value from a channel (blocking).
pub(crate) extern "C" fn rt_channel_recv(ch: *const u8) -> i64 {
    let rx_ptr = unsafe { *((ch as *const i64).add(1)) } as *mut std::sync::mpsc::Receiver<i64>;
    let rx = unsafe { &*rx_ptr };
    rx.recv().unwrap_or(0)
}

/// Clone a channel's sender for passing to spawned threads.
/// Returns a new channel handle with a cloned sender and the same receiver pointer.
pub(crate) extern "C" fn rt_channel_clone_sender(ch: *const u8) -> *mut u8 {
    let tx_ptr = unsafe { *(ch as *const i64) } as *mut std::sync::mpsc::Sender<i64>;
    let tx = unsafe { &*tx_ptr };
    let cloned = tx.clone();
    let new_tx = Box::into_raw(Box::new(cloned)) as i64;

    let rx_ptr = unsafe { *((ch as *const i64).add(1)) };
    let layout = std::alloc::Layout::from_size_align(8 + 16, 8).unwrap();
    let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
    if ptr.is_null() {
        eprintln!("turbo: fatal: memory allocation failed");
        std::process::exit(1);
    }
    register_alloc(ptr, layout);
    unsafe {
        *(ptr as *mut i64) = 1;
    } // refcount = 1
    let data_ptr = unsafe { ptr.add(8) };
    unsafe {
        *(data_ptr as *mut i64) = new_tx;
        *((data_ptr as *mut i64).add(1)) = rx_ptr;
    }
    data_ptr
}

// ── Mutex runtime functions ─────────────────────────────────────────

/// Create a mutex wrapping an i64 value. Returns a pointer to an Arc<Mutex<i64>>.
pub(crate) extern "C" fn rt_mutex_create(value: i64) -> *mut u8 {
    let m = std::sync::Arc::new(std::sync::Mutex::new(value));
    std::sync::Arc::into_raw(m) as *mut u8
}

/// Get the current value inside a mutex.
pub(crate) extern "C" fn rt_mutex_get(m: *const u8) -> i64 {
    let arc = unsafe { std::sync::Arc::from_raw(m as *const std::sync::Mutex<i64>) };
    let val = *arc.lock().unwrap();
    let _ = std::sync::Arc::into_raw(arc); // don't drop
    val
}

/// Set the value inside a mutex.
pub(crate) extern "C" fn rt_mutex_set(m: *const u8, value: i64) {
    let arc = unsafe { std::sync::Arc::from_raw(m as *const std::sync::Mutex<i64>) };
    *arc.lock().unwrap() = value;
    let _ = std::sync::Arc::into_raw(arc); // don't drop
}

/// Clone a mutex handle (increments the Arc refcount). Returns a new pointer.
pub(crate) extern "C" fn rt_mutex_clone(m: *const u8) -> *mut u8 {
    let arc = unsafe { std::sync::Arc::from_raw(m as *const std::sync::Mutex<i64>) };
    let cloned = arc.clone();
    let _ = std::sync::Arc::into_raw(arc); // don't drop original
    std::sync::Arc::into_raw(cloned) as *mut u8
}

// ── HashMap runtime functions ───────────────────────────────────────

/// Create a new empty HashMap<String, String>. Returns an opaque pointer.
pub(crate) extern "C" fn rt_hashmap_new() -> *mut u8 {
    let map: HashMap<String, String> = HashMap::new();
    let boxed = Box::new(map);
    Box::into_raw(boxed) as *mut u8
}

/// Set a key-value pair in the hashmap.
pub(crate) extern "C" fn rt_hashmap_set(map_ptr: *mut u8, key: *const u8, value: *const u8) {
    let map = unsafe { &mut *(map_ptr as *mut HashMap<String, String>) };
    let key = unsafe { std::ffi::CStr::from_ptr(key as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("")
        .to_string();
    let value = unsafe { std::ffi::CStr::from_ptr(value as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("")
        .to_string();
    map.insert(key, value);
}

/// Get a value by key. Returns a C string pointer, or null if not found.
pub(crate) extern "C" fn rt_hashmap_get(map_ptr: *const u8, key: *const u8) -> *const u8 {
    let map = unsafe { &*(map_ptr as *const HashMap<String, String>) };
    let key = unsafe { std::ffi::CStr::from_ptr(key as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");
    match map.get(key) {
        Some(v) => {
            let cs = cstring_or_empty(v.as_str());
            arena_str(cs)
        }
        None => std::ptr::null(),
    }
}

/// Check if a key exists. Returns 1 (true) or 0 (false).
pub(crate) extern "C" fn rt_hashmap_has(map_ptr: *const u8, key: *const u8) -> i8 {
    let map = unsafe { &*(map_ptr as *const HashMap<String, String>) };
    let key = unsafe { std::ffi::CStr::from_ptr(key as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");
    if map.contains_key(key) {
        1
    } else {
        0
    }
}

/// Return the number of entries in the hashmap.
pub(crate) extern "C" fn rt_hashmap_len(map_ptr: *const u8) -> i64 {
    let map = unsafe { &*(map_ptr as *const HashMap<String, String>) };
    map.len() as i64
}

/// Return all keys as a [str] array (same format as rt_str_split).
pub(crate) extern "C" fn rt_hashmap_keys(map_ptr: *const u8) -> *mut u8 {
    let map = unsafe { &*(map_ptr as *const HashMap<String, String>) };
    let mut keys: Vec<&str> = map.keys().map(|k| k.as_str()).collect();
    keys.sort(); // deterministic order for testing
    let len = keys.len() as i64;
    // Array format: [refcount: i64][len: i64][ptr0: i64][ptr1: i64]...
    let layout = match checked_array_layout(len as usize) {
        Some(l) => l,
        None => {
            eprintln!("runtime error: hashmap keys overflow");
            std::process::exit(1);
        }
    };
    let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
    if ptr.is_null() {
        eprintln!("turbo: fatal: memory allocation failed");
        std::process::exit(1);
    }
    register_alloc(ptr, layout);
    unsafe {
        *(ptr as *mut i64) = 1;
    } // refcount = 1
    let data_ptr = unsafe { ptr.add(8) };
    unsafe {
        *(data_ptr as *mut i64) = len;
    }
    for (i, key) in keys.iter().enumerate() {
        let cs =
            cstring_or_empty(*key);
        unsafe {
            *((data_ptr as *mut i64).add(1 + i)) = arena_str(cs) as i64;
        }
    }
    data_ptr
}

/// Remove a key from the hashmap.
pub(crate) extern "C" fn rt_hashmap_remove(map_ptr: *mut u8, key: *const u8) {
    let map = unsafe { &mut *(map_ptr as *mut HashMap<String, String>) };
    let key = unsafe { std::ffi::CStr::from_ptr(key as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");
    map.remove(key);
}

/// Set a key → int pair. Stringifies the int into the existing
/// str→str storage so v0.8.0 doesn't need a tagged-union refactor.
/// Returns the same map pointer so callers can write
/// `m = hashmap_set_int(m, k, v)`.
pub(crate) extern "C" fn rt_hashmap_set_int(
    map_ptr: *mut u8,
    key: *const u8,
    value: i64,
) -> *mut u8 {
    let map = unsafe { &mut *(map_ptr as *mut HashMap<String, String>) };
    let key = unsafe { std::ffi::CStr::from_ptr(key as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("")
        .to_string();
    map.insert(key, value.to_string());
    map_ptr
}

/// Get an int value by key. Returns 0 on miss (callers that need to
/// distinguish missing from a stored 0 should guard with hashmap_has).
pub(crate) extern "C" fn rt_hashmap_get_int(map_ptr: *const u8, key: *const u8) -> i64 {
    let map = unsafe { &*(map_ptr as *const HashMap<String, String>) };
    let key = unsafe { std::ffi::CStr::from_ptr(key as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");
    match map.get(key) {
        Some(v) => v.parse::<i64>().unwrap_or(0),
        None => 0,
    }
}

// ── ARC (Automatic Reference Counting) runtime functions ────────────

/// Increment the reference count of a heap-allocated object.
/// The refcount lives at data_ptr - 8 (the header before the data).
pub(crate) extern "C" fn rt_retain(data_ptr: *mut u8) {
    if data_ptr.is_null() {
        return;
    }
    let header = unsafe { data_ptr.sub(8) as *mut std::sync::atomic::AtomicI64 };
    unsafe {
        (*header).fetch_add(1, std::sync::atomic::Ordering::AcqRel);
    }
}

/// Decrement the reference count of a heap-allocated object.
/// When the refcount reaches 0, the memory is freed using the layout
/// stored in the thread-local allocation registry.
pub(crate) extern "C" fn rt_release(data_ptr: *mut u8) {
    if data_ptr.is_null() {
        return;
    }
    let header = unsafe { data_ptr.sub(8) as *mut std::sync::atomic::AtomicI64 };
    let prev = unsafe { (*header).fetch_sub(1, std::sync::atomic::Ordering::Release) };
    if prev == 1 {
        std::sync::atomic::fence(std::sync::atomic::Ordering::Acquire);
        // Refcount reached 0 — free the allocation.
        let raw_ptr = unsafe { data_ptr.sub(8) };
        if let Some(layout) = unregister_alloc(raw_ptr) {
            unsafe {
                std::alloc::dealloc(raw_ptr, layout);
            }
        }
        // If not in the registry (e.g. allocated by C runtime or from
        // a different thread), we silently skip — better than UB.
    }
}

// ── Filesystem builtins ────────────────────────────────────────────

pub(crate) extern "C" fn rt_file_exists(path: *const u8) -> i64 {
    if path.is_null() {
        return 0;
    }
    let path_str = unsafe { std::ffi::CStr::from_ptr(path as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");
    if std::path::Path::new(path_str).exists() {
        1
    } else {
        0
    }
}

pub(crate) extern "C" fn rt_delete_file(path: *const u8) -> i64 {
    if path.is_null() {
        return 0;
    }
    let path_str = unsafe { std::ffi::CStr::from_ptr(path as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");
    if std::fs::remove_file(path_str).is_ok() || std::fs::remove_dir(path_str).is_ok() {
        1
    } else {
        0
    }
}

pub(crate) extern "C" fn rt_list_dir(path: *const u8) -> *mut u8 {
    let path_str = if path.is_null() {
        "."
    } else {
        unsafe { std::ffi::CStr::from_ptr(path as *const std::ffi::c_char) }
            .to_str()
            .unwrap_or(".")
    };
    let mut entries = Vec::new();
    if let Ok(dir) = std::fs::read_dir(path_str) {
        for entry in dir.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                entries.push(name.to_string());
            }
        }
    }
    let len = entries.len() as i64;
    let layout = match checked_array_layout(len as usize) {
        Some(l) => l,
        None => {
            eprintln!("runtime error: list_dir result too large");
            std::process::exit(1);
        }
    };
    let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
    if ptr.is_null() {
        eprintln!("turbo: fatal: memory allocation failed");
        std::process::exit(1);
    }
    register_alloc(ptr, layout);
    unsafe {
        *(ptr as *mut i64) = 1; // refcount
    }
    let data_ptr = unsafe { ptr.add(8) };
    unsafe {
        *(data_ptr as *mut i64) = len;
    }
    for (i, name) in entries.iter().enumerate() {
        let cs = cstring_or_empty(name.as_str());
        unsafe {
            *((data_ptr as *mut i64).add(1 + i)) = arena_str(cs) as i64;
        }
    }
    data_ptr
}

pub(crate) extern "C" fn rt_mkdir(path: *const u8) -> i64 {
    if path.is_null() {
        return 0;
    }
    let path_str = unsafe { std::ffi::CStr::from_ptr(path as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");
    if std::fs::create_dir_all(path_str).is_ok() {
        1
    } else {
        0
    }
}

pub(crate) extern "C" fn rt_path_join(a: *const u8, b: *const u8) -> *const u8 {
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
    let result = if a_str.is_empty() {
        b_str.to_string()
    } else if a_str.ends_with('/') {
        format!("{}{}", a_str, b_str)
    } else {
        format!("{}/{}", a_str, b_str)
    };
    let cs = cstring_or_empty(result);
    arena_str(cs)
}

pub(crate) extern "C" fn rt_path_dir(path: *const u8) -> *const u8 {
    let path_str = if path.is_null() {
        ""
    } else {
        unsafe { std::ffi::CStr::from_ptr(path as *const std::ffi::c_char) }
            .to_str()
            .unwrap_or("")
    };
    let result = match std::path::Path::new(path_str).parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_str().unwrap_or(".").to_string(),
        _ => ".".to_string(),
    };
    let cs =
        cstring_or_empty(result);
    arena_str(cs)
}

pub(crate) extern "C" fn rt_path_base(path: *const u8) -> *const u8 {
    let path_str = if path.is_null() {
        ""
    } else {
        unsafe { std::ffi::CStr::from_ptr(path as *const std::ffi::c_char) }
            .to_str()
            .unwrap_or("")
    };
    let result = std::path::Path::new(path_str)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    let cs = cstring_or_empty(result);
    arena_str(cs)
}

pub(crate) extern "C" fn rt_path_ext(path: *const u8) -> *const u8 {
    let path_str = if path.is_null() {
        ""
    } else {
        unsafe { std::ffi::CStr::from_ptr(path as *const std::ffi::c_char) }
            .to_str()
            .unwrap_or("")
    };
    let result = std::path::Path::new(path_str)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_string();
    let cs = cstring_or_empty(result);
    arena_str(cs)
}

// ── Collection builtins ────────────────────────────────────────────

pub(crate) extern "C" fn rt_sort_int(arr: *const u8) -> *mut u8 {
    let len = unsafe { *(arr as *const i64) };
    let layout = match checked_array_layout(len as usize) {
        Some(l) => l,
        None => {
            eprintln!("runtime error: sort result too large");
            std::process::exit(1);
        }
    };
    let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
    if ptr.is_null() {
        eprintln!("turbo: fatal: memory allocation failed");
        std::process::exit(1);
    }
    register_alloc(ptr, layout);
    unsafe {
        *(ptr as *mut i64) = 1;
    } // refcount
    let data_ptr = unsafe { ptr.add(8) };
    unsafe {
        *(data_ptr as *mut i64) = len;
    }
    // Copy elements
    let src = unsafe { (arr as *const i64).add(1) };
    let dst = unsafe { (data_ptr as *mut i64).add(1) };
    unsafe {
        std::ptr::copy_nonoverlapping(src, dst, len as usize);
    }
    // Sort as i64
    let slice = unsafe { std::slice::from_raw_parts_mut(dst, len as usize) };
    slice.sort();
    data_ptr
}

pub(crate) extern "C" fn rt_sort_str(arr: *const u8) -> *mut u8 {
    let len = unsafe { *(arr as *const i64) };
    let layout = match checked_array_layout(len as usize) {
        Some(l) => l,
        None => {
            eprintln!("runtime error: sort result too large");
            std::process::exit(1);
        }
    };
    let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
    if ptr.is_null() {
        eprintln!("turbo: fatal: memory allocation failed");
        std::process::exit(1);
    }
    register_alloc(ptr, layout);
    unsafe {
        *(ptr as *mut i64) = 1;
    } // refcount
    let data_ptr = unsafe { ptr.add(8) };
    unsafe {
        *(data_ptr as *mut i64) = len;
    }
    // Copy elements
    let src = unsafe { (arr as *const i64).add(1) };
    let dst = unsafe { (data_ptr as *mut i64).add(1) };
    unsafe {
        std::ptr::copy_nonoverlapping(src, dst, len as usize);
    }
    // Sort as string pointers
    let slice = unsafe { std::slice::from_raw_parts_mut(dst, len as usize) };
    slice.sort_by(|a, b| {
        let a_str = if *a == 0 {
            ""
        } else {
            unsafe { std::ffi::CStr::from_ptr(*a as *const std::ffi::c_char) }
                .to_str()
                .unwrap_or("")
        };
        let b_str = if *b == 0 {
            ""
        } else {
            unsafe { std::ffi::CStr::from_ptr(*b as *const std::ffi::c_char) }
                .to_str()
                .unwrap_or("")
        };
        a_str.cmp(b_str)
    });
    data_ptr
}

pub(crate) extern "C" fn rt_reverse(arr: *const u8) -> *mut u8 {
    let len = unsafe { *(arr as *const i64) };
    let layout = match checked_array_layout(len as usize) {
        Some(l) => l,
        None => {
            eprintln!("runtime error: reverse result too large");
            std::process::exit(1);
        }
    };
    let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
    if ptr.is_null() {
        eprintln!("turbo: fatal: memory allocation failed");
        std::process::exit(1);
    }
    register_alloc(ptr, layout);
    unsafe {
        *(ptr as *mut i64) = 1;
    } // refcount
    let data_ptr = unsafe { ptr.add(8) };
    unsafe {
        *(data_ptr as *mut i64) = len;
    }
    let src = unsafe { (arr as *const i64).add(1) };
    let dst = unsafe { (data_ptr as *mut i64).add(1) };
    for i in 0..len as usize {
        unsafe {
            *dst.add(i) = *src.add(len as usize - 1 - i);
        }
    }
    data_ptr
}

pub(crate) extern "C" fn rt_array_contains_int(arr: *const u8, val: i64) -> i64 {
    let len = unsafe { *(arr as *const i64) };
    let elems = unsafe { (arr as *const i64).add(1) };
    for i in 0..len as usize {
        if unsafe { *elems.add(i) } == val {
            return 1;
        }
    }
    0
}

pub(crate) extern "C" fn rt_array_contains_str(arr: *const u8, val: *const u8) -> i64 {
    let len = unsafe { *(arr as *const i64) };
    let elems = unsafe { (arr as *const i64).add(1) };
    let val_str = if val.is_null() {
        ""
    } else {
        unsafe { std::ffi::CStr::from_ptr(val as *const std::ffi::c_char) }
            .to_str()
            .unwrap_or("")
    };
    for i in 0..len as usize {
        let elem_ptr = unsafe { *elems.add(i) } as *const u8;
        let elem_str = if elem_ptr.is_null() {
            ""
        } else {
            unsafe { std::ffi::CStr::from_ptr(elem_ptr as *const std::ffi::c_char) }
                .to_str()
                .unwrap_or("")
        };
        if elem_str == val_str {
            return 1;
        }
    }
    0
}

pub(crate) extern "C" fn rt_slice(arr: *const u8, start: i64, end: i64) -> *mut u8 {
    let len = unsafe { *(arr as *const i64) };
    let start = start.max(0).min(len);
    let end = end.max(start).min(len);
    let new_len = end - start;
    let layout = match checked_array_layout(new_len as usize) {
        Some(l) => l,
        None => {
            eprintln!("runtime error: slice result too large");
            std::process::exit(1);
        }
    };
    let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
    if ptr.is_null() {
        eprintln!("turbo: fatal: memory allocation failed");
        std::process::exit(1);
    }
    register_alloc(ptr, layout);
    unsafe {
        *(ptr as *mut i64) = 1;
    } // refcount
    let data_ptr = unsafe { ptr.add(8) };
    unsafe {
        *(data_ptr as *mut i64) = new_len;
    }
    let src = unsafe { (arr as *const i64).add(1 + start as usize) };
    let dst = unsafe { (data_ptr as *mut i64).add(1) };
    unsafe {
        std::ptr::copy_nonoverlapping(src, dst, new_len as usize);
    }
    data_ptr
}

// ── Date/Time builtins ─────────────────────────────────────────────

pub(crate) extern "C" fn rt_time_now() -> f64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    now.as_secs_f64()
}

pub(crate) extern "C" fn rt_time_ms() -> i64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    now.as_millis() as i64
}

pub(crate) extern "C" fn rt_format_time(timestamp: f64, fmt: *const u8) -> *const u8 {
    let fmt_str = if fmt.is_null() {
        "%Y-%m-%d %H:%M:%S"
    } else {
        unsafe { std::ffi::CStr::from_ptr(fmt as *const std::ffi::c_char) }
            .to_str()
            .unwrap_or("%Y-%m-%d %H:%M:%S")
    };
    // Use libc-compatible formatting: convert to time_t then format
    // For the JIT we use a simple approach: convert known format specifiers
    let secs = timestamp as i64;
    let naive = chrono_like_format(secs, fmt_str);
    let cs = cstring_or_empty(naive);
    arena_str(cs)
}

/// Simple strftime-like formatter without pulling in chrono.
fn chrono_like_format(epoch_secs: i64, fmt: &str) -> String {
    // Use libc localtime + strftime via FFI
    unsafe {
        let t = epoch_secs as libc::time_t;
        let tm = libc::localtime(&t);
        if tm.is_null() {
            return String::new();
        }
        let fmt_c =
            cstring_or_empty(fmt);
        let mut buf = [0u8; 256];
        let n = libc::strftime(
            buf.as_mut_ptr() as *mut libc::c_char,
            buf.len(),
            fmt_c.as_ptr(),
            tm,
        );
        if n == 0 {
            return String::new();
        }
        String::from_utf8_lossy(&buf[..n]).to_string()
    }
}
