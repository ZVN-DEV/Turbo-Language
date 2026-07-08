//! Runtime functions linked into the JIT module.
//!
//! All `extern "C" fn rt_*` functions live here. They are completely
//! standalone — no Cranelift types, no `Ctx`, no compiler state.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::Mutex;

const F64_FORMAT: &[u8] = b"%.15g\0";
const RT_RC_IMMORTAL: i64 = i64::MAX;

/// Build a `CString` from the given value, falling back to an empty string
/// if the input contains an interior nul byte.
fn cstring_or_empty(s: impl Into<Vec<u8>>) -> std::ffi::CString {
    std::ffi::CString::new(s).unwrap_or_else(|_| std::ffi::CString::new("").unwrap())
}

/// Base URL for the public per-error-code documentation. Must stay in sync
/// with `error_code_url()` in `turbo-cli/src/main.rs` and the matching
/// `RT_ERROR_DOC_URL_BASE` in `turbo_rt.c`, so the `more info:` footer points
/// at the same place from compile errors, JIT traps, and AOT traps alike.
const RT_ERROR_DOC_URL_BASE: &str =
    "https://github.com/ZVN-DEV/Turbo-Language/blob/master/docs/errors";

/// Print a styled runtime-error envelope to stderr and terminate the process.
///
/// This is the JIT (`turbolang run`) twin of `rt_runtime_error` in
/// `turbo_rt.c`; the two MUST emit byte-identical output so a program run
/// through the JIT and the same program built to a native binary produce the
/// identical diagnostic (JIT ≡ AOT). The envelope mirrors the compile-time
/// diagnostics:
///
/// ```text
/// runtime error[E06NN]: <message>
/// Help: <help>
///   more info: <doc url>
/// ```
///
/// ANSI color is emitted only when stderr is a terminal, so piped output
/// (the integration-test harness, the web playground) stays clean.
fn runtime_error(code: &str, message: &str, help: &str) -> ! {
    use std::io::IsTerminal;
    if std::io::stderr().is_terminal() {
        eprintln!("\x1b[1;31mruntime error[{code}]\x1b[0m: {message}");
        eprintln!("\x1b[1;36mHelp\x1b[0m: {help}");
    } else {
        eprintln!("runtime error[{code}]: {message}");
        eprintln!("Help: {help}");
    }
    eprintln!("  more info: {RT_ERROR_DOC_URL_BASE}/{code}.md");
    std::process::exit(1);
}

/// Styled index-out-of-bounds trap for array access (E0602). Shared by
/// `rt_array_get`, `rt_array_set`, and `rt_array_oob_exit`.
fn array_oob(index: i64, len: i64) -> ! {
    let message = format!("array index {index} out of bounds (length {len})");
    let help = format!("valid indices are 0..{len} (exclusive); check with `if i < len(arr)`");
    runtime_error("E0602", &message, &help);
}

/// Styled index-out-of-bounds trap for string indexing (E0602).
fn str_index_oob(index: i64, len: usize) -> ! {
    let message = format!("string index {index} out of bounds (length {len})");
    let help = format!(
        "valid indices are 0..{len} (exclusive); check the index against the string length"
    );
    runtime_error("E0602", &message, &help);
}

/// C-stdlib functions the float/date-formatting twins call. The `libc` crate
/// exposes `snprintf`/`localtime`/`strftime` on Unix but not on windows-msvc,
/// even though the UCRT provides the symbols. Binding them directly here keeps
/// the Unix path a plain re-export (byte-identical behavior) while giving the
/// Windows build the same C formatting — critical for float/date output parity
/// with the fixtures and the C runtime.
mod cstd {
    #[cfg(unix)]
    pub use libc::{localtime, snprintf, strftime};

    #[cfg(windows)]
    pub use win::{localtime, snprintf, strftime};

    #[cfg(windows)]
    mod win {
        use libc::{c_char, c_int, size_t, time_t, tm};
        unsafe extern "C" {
            pub fn snprintf(buf: *mut c_char, size: size_t, format: *const c_char, ...) -> c_int;
            pub fn localtime(time: *const time_t) -> *mut tm;
            pub fn strftime(
                s: *mut c_char,
                max: size_t,
                format: *const c_char,
                tm: *const tm,
            ) -> size_t;
        }
    }
}

fn format_f64(n: f64) -> String {
    let n = if n == 0.0 { 0.0 } else { n };
    let mut buf = [0 as libc::c_char; 64];
    let formatted = unsafe {
        cstd::snprintf(
            buf.as_mut_ptr(),
            buf.len(),
            F64_FORMAT.as_ptr() as *const libc::c_char,
            n,
        );
        std::ffi::CStr::from_ptr(buf.as_ptr())
            .to_string_lossy()
            .into_owned()
    };
    // BL-26: under `%g` a whole-valued float renders without a fractional
    // part (`2.0` -> "2"), which is indistinguishable from the int `2`.
    // Append a trailing `.0` so floats are always unambiguous. We only do
    // this when the rendered text is a bare integer (`[-]?[0-9]+`) — anything
    // already carrying a `.`/`e`/`E` (fractional or exponential form) or a
    // special value (inf/-inf/nan) contains a non-digit and is left as-is.
    // The AOT C runtime (`rt_format_f64`) applies the identical rule so JIT
    // and AOT stay byte-identical.
    if f64_text_is_integral(&formatted) {
        formatted + ".0"
    } else {
        formatted
    }
}

/// True when `s` is a bare integer literal (`[-]?[0-9]+`), i.e. a whole-valued
/// float that `%g` rendered with no fractional part. Returns false for
/// fractional/exponential forms and for `inf`/`-inf`/`nan`.
fn f64_text_is_integral(s: &str) -> bool {
    let digits = s.strip_prefix('-').unwrap_or(s);
    !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
}

/// Compute an allocation layout for array-like structures, returning None on overflow.
/// Format: [cap: i64][refcount: i64][length: i64][elements: cap * 8 bytes]
/// `cap` is the element capacity (>= len). We allocate for `cap` elements.
fn checked_array_layout(cap: usize) -> Option<std::alloc::Layout> {
    let elem_bytes = cap.checked_mul(8)?;
    let data_bytes = elem_bytes.checked_add(8)?; // +8 for length field
    let total_bytes = data_bytes.checked_add(16)?; // +16 for cap + refcount header
    std::alloc::Layout::from_size_align(total_bytes, 8).ok()
}

/// Compute the shared ARC allocation layout for non-array objects.
/// Format: [cap: i64][refcount: i64][data bytes...]
fn checked_rc_layout(data_size: usize) -> Option<std::alloc::Layout> {
    let total_bytes = data_size.checked_add(16)?;
    std::alloc::Layout::from_size_align(total_bytes, 8).ok()
}

fn rc_alloc(data_size: usize, cap: i64) -> *mut u8 {
    let layout = match checked_rc_layout(data_size) {
        Some(l) => l,
        None => {
            eprintln!("runtime error: ARC allocation overflow");
            std::process::exit(1);
        }
    };
    let raw = unsafe { std::alloc::alloc_zeroed(layout) };
    if raw.is_null() {
        eprintln!("turbo: fatal: memory allocation failed");
        std::process::exit(1);
    }
    register_alloc(raw, layout);
    unsafe {
        *(raw as *mut i64) = cap;
        *(raw.add(8) as *mut i64) = 1;
        raw.add(16)
    }
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

// ── Legacy thread-local string arena ─────────────────────────────────
//
// String-returning rt_* functions now allocate refcounted strings through
// `arena_str()`. The arena reset path remains for compatibility with any older
// raw entries, but normal string reclamation is handled by rt_release.

struct ArenaEntry {
    ptr: *mut u8,
    cap: usize,
    is_raw: bool,
}

thread_local! {
    static STRING_ARENA: RefCell<Vec<ArenaEntry>> = const { RefCell::new(Vec::new()) };
}

/// Copy a CString into the shared ARC string layout. Returns the data pointer.
fn arena_str(cs: std::ffi::CString) -> *const u8 {
    let bytes = cs.as_bytes_with_nul();
    let ptr = rc_alloc(bytes.len(), 0);
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
    }
    ptr as *const u8
}

/// Number of strings currently held in the per-thread arena. The HTTP
/// request loop records this at the start of each request and uses it as a
/// high-water mark to reclaim only the strings allocated *during* that
/// request (see [`arena_reset_to`]).
fn arena_mark() -> usize {
    STRING_ARENA.with(|arena| arena.borrow().len())
}

/// Free and drop every arena string from index `mark` onward, leaving the
/// earlier entries untouched.
///
/// This is the surgical core of the per-request reclamation the JIT HTTP
/// server performs (BL-25 A1). By recording the arena length at the start of
/// a request ([`arena_mark`]) and truncating back to it once the response is
/// written, only strings allocated *during* that request are freed; strings
/// allocated before the mark — e.g. server state captured at startup, which
/// on a worker thread lives in the main thread's arena anyway — are
/// preserved, so the reset can never free memory that must outlive the
/// request. This mirrors the AOT runtime's per-request bump arena
/// (`rt_arena_begin` / `rt_arena_end` in `turbo_rt.c`).
fn arena_reset_to(mark: usize) {
    STRING_ARENA.with(|arena| {
        let mut strings = arena.borrow_mut();
        if mark >= strings.len() {
            return;
        }
        for entry in strings.drain(mark..) {
            unsafe {
                if entry.is_raw {
                    let layout = std::alloc::Layout::from_size_align(entry.cap, 1).unwrap();
                    std::alloc::dealloc(entry.ptr, layout);
                } else {
                    drop(std::ffi::CString::from_raw(
                        entry.ptr as *mut std::ffi::c_char,
                    ));
                }
            }
        }
    });
}

/// Free all strings in the arena. Called after JIT `main()` returns (the
/// non-server `turbolang run` path) to release every runtime-allocated string
/// at once.
pub(crate) extern "C" fn rt_arena_reset() {
    arena_reset_to(0);
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
    println!("{}", format_f64(n));
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
    runtime_error(
        "E0601",
        "division by zero",
        "guard the divisor: `if b != 0 { ... }`",
    );
}

pub(crate) extern "C" fn rt_int_overflow() {
    runtime_error(
        "E0603",
        "integer overflow",
        "the result does not fit in a 64-bit signed integer; check the operands' magnitude",
    );
}

pub(crate) extern "C" fn rt_array_alloc(len: i64) -> *mut u8 {
    if len < 0 {
        eprintln!("runtime error: negative array length {}", len);
        std::process::exit(1);
    }
    let cap = len as usize;
    let layout = match checked_array_layout(cap) {
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
    // Layout: [cap: 8][refcount: 8][length: 8][data...]
    unsafe {
        *(ptr as *mut i64) = cap as i64; // capacity
        *(ptr.add(8) as *mut i64) = 1; // refcount = 1
    }
    let data_ptr = unsafe { ptr.add(16) }; // pointer past cap+refcount header
    unsafe {
        *(data_ptr as *mut i64) = len; // length
    }
    data_ptr
}

pub(crate) extern "C" fn rt_array_get(arr: *const u8, index: i64) -> i64 {
    let len = unsafe { *(arr as *const i64) };
    if index < 0 || index >= len {
        array_oob(index, len);
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
        let cap = len as usize;
        let layout = match checked_array_layout(cap) {
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
            *(new_alloc as *mut i64) = cap as i64; // capacity
            *(new_alloc.add(8) as *mut i64) = 1; // refcount = 1
        }
        let new_data = unsafe { new_alloc.add(16) };
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
        array_oob(index, len);
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

    // Check if we can grow in place (sole owner + capacity available)
    let rc = unsafe { *(arr.sub(8) as *const i64) };
    let cap = unsafe { *(arr.sub(16) as *const i64) } as usize;

    if rc == 1 && cap > old_len {
        // Fast path: write in place
        unsafe {
            *((arr as *mut i64).add(1 + old_len)) = value;
            *(arr as *mut i64) = new_len as i64;
        }
        return arr as *mut u8;
    }

    // Slow path: allocate with doubled capacity
    let new_cap = if cap > 0 { cap * 2 } else { 4 };
    let new_cap = new_cap.max(new_len);
    let layout = match checked_array_layout(new_cap) {
        Some(l) => l,
        None => match checked_array_layout(new_len) {
            Some(l) => l,
            None => {
                eprintln!(
                    "runtime error: array allocation overflow (length {})",
                    new_len
                );
                std::process::exit(1);
            }
        },
    };
    let new_alloc = unsafe { std::alloc::alloc_zeroed(layout) };
    if new_alloc.is_null() {
        eprintln!("turbo: fatal: memory allocation failed");
        std::process::exit(1);
    }
    register_alloc(new_alloc, layout);
    unsafe {
        *(new_alloc as *mut i64) = new_cap as i64; // capacity
        *(new_alloc.add(8) as *mut i64) = 1; // refcount = 1
    }
    let new_data = unsafe { new_alloc.add(16) };
    unsafe {
        *(new_data as *mut i64) = new_len as i64;
        std::ptr::copy_nonoverlapping(arr.add(8), new_data.add(8), old_len * 8);
        *((new_data as *mut i64).add(1 + old_len)) = value;
    }
    // NOTE: we do NOT touch the old array's refcount here. `push` borrows its
    // input to copy from; ownership of the old value stays with the caller. The
    // `xs = push(xs, v)` assignment site is responsible for releasing the old
    // `xs` (it already does, guarded by an old != new pointer check). Decrementing
    // here as well double-counted a single overwrite: with no aliasing the old
    // array leaked (refcount hit 0 without freeing), and with an alias
    // (`let b = xs; xs.push(v)`) it freed an array `b` still pointed at — a
    // use-after-free. Leaving the refcount alone makes both cases correct.
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
    let c_string = cstring_or_empty(result);
    arena_str(c_string)
}

pub(crate) extern "C" fn rt_str_copy(s: *const u8) -> *const u8 {
    if s.is_null() {
        return rt_alloc_string("");
    }
    let value = unsafe { std::ffi::CStr::from_ptr(s as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");
    rt_alloc_string(value)
}

pub(crate) extern "C" fn rt_str_concat_inplace(a: *const u8, b: *const u8) -> *const u8 {
    // The `s = s + x` assignment is rewritten to this in-place concat for speed.
    // The previous implementation mutated the most-recent arena buffer when it
    // equalled `a` — but Turbo strings carry no refcount, so it could not tell
    // whether that buffer was aliased by another live binding. Code like
    //   let mut s = "x" + "y"; let alias = s; s = s + "z"
    // silently corrupted `alias` (it observed "xyz"). Strings are values and
    // must never be mutated through an alias, so we always allocate a fresh
    // result. (A future safe rope/builder can restore O(1) appends without the
    // aliasing hazard.)
    rt_str_concat(a, b)
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
    let total_size = data_size + 16; // +16 for cap + refcount header
    let layout = std::alloc::Layout::from_size_align(total_size, 8).unwrap();
    let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
    if ptr.is_null() {
        eprintln!("turbo: fatal: memory allocation failed");
        std::process::exit(1);
    }
    register_alloc(ptr, layout);
    // Layout: [cap: 8 (unused for structs)][refcount: 8][data...]
    unsafe {
        *(ptr as *mut i64) = 0; // cap = 0 (not an array)
        *(ptr.add(8) as *mut i64) = 1; // refcount = 1
    }
    unsafe { ptr.add(16) } // return pointer past cap+refcount header
}

/// Copy-on-write guard for struct field assignment.
///
/// Structs carry the same `[cap][refcount]` allocation header as arrays
/// (see `rt_struct_alloc`), so a `let b = a` / `mut`-param / array-element
/// copy can leave two live bindings pointing at one allocation. Mutating a
/// field through either would alias the other. This mirrors the copy-on-write
/// dance in `rt_array_set`: if the refcount is > 1, allocate a private copy,
/// memcpy the `num_fields` data slots, drop our reference to the shared
/// original, and return the copy. When the refcount is 1 (sole owner) the
/// original pointer is returned unchanged and the field store proceeds in
/// place. `num_fields` matches the count passed to `rt_struct_alloc`.
pub(crate) extern "C" fn rt_struct_cow(s: *mut u8, num_fields: i64) -> *mut u8 {
    if s.is_null() {
        return s;
    }
    let rc_ptr = unsafe { s.sub(8) as *mut std::sync::atomic::AtomicI64 };
    let rc = unsafe { (*rc_ptr).load(std::sync::atomic::Ordering::Acquire) };
    if rc <= 1 {
        return s;
    }
    if num_fields < 0 {
        eprintln!("runtime error: negative struct field count {}", num_fields);
        std::process::exit(1);
    }
    let data_size = match (num_fields as usize).checked_mul(8) {
        Some(s) => s.max(8),
        None => {
            eprintln!("runtime error: struct COW copy overflow");
            std::process::exit(1);
        }
    };
    let total_size = data_size + 16; // +16 for cap + refcount header
    let layout = std::alloc::Layout::from_size_align(total_size, 8).unwrap();
    let new_alloc = unsafe { std::alloc::alloc_zeroed(layout) };
    if new_alloc.is_null() {
        eprintln!("turbo: fatal: memory allocation failed");
        std::process::exit(1);
    }
    register_alloc(new_alloc, layout);
    unsafe {
        *(new_alloc as *mut i64) = 0; // cap = 0 (not an array)
        *(new_alloc.add(8) as *mut i64) = 1; // refcount = 1
    }
    let new_data = unsafe { new_alloc.add(16) };
    unsafe {
        std::ptr::copy_nonoverlapping(s, new_data, data_size);
    }
    // Drop our reference to the shared original now that this binding owns a copy.
    unsafe {
        (*rc_ptr).fetch_sub(1, std::sync::atomic::Ordering::Release);
    }
    new_data
}

pub(crate) extern "C" fn rt_array_oob_exit(index: i64, len: i64) {
    array_oob(index, len);
}

fn fast_i64_to_string(n: i64) -> String {
    let mut buf = [0u8; 20];
    let neg = n < 0;
    let mut v = if neg {
        (n as i128).unsigned_abs() as u64
    } else {
        n as u64
    };
    let mut pos = 20;
    if v == 0 {
        pos -= 1;
        buf[pos] = b'0';
    } else {
        while v > 0 {
            pos -= 1;
            buf[pos] = b'0' + (v % 10) as u8;
            v /= 10;
        }
    }
    if neg {
        pos -= 1;
        buf[pos] = b'-';
    }
    unsafe { std::str::from_utf8_unchecked(&buf[pos..20]) }.to_owned()
}

pub(crate) extern "C" fn rt_i64_to_str(n: i64) -> *const u8 {
    let s = fast_i64_to_string(n);
    let c = cstring_or_empty(s);
    arena_str(c)
}

pub(crate) extern "C" fn rt_f64_to_str(n: f64) -> *const u8 {
    let s = format_f64(n);
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
    let layout = std::alloc::Layout::from_size_align(16 + 16, 8).unwrap();
    let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
    if ptr.is_null() {
        eprintln!("turbo: fatal: memory allocation failed");
        std::process::exit(1);
    }
    register_alloc(ptr, layout);
    unsafe {
        *(ptr as *mut i64) = 0; // cap = 0 (not an array)
        *((ptr as *mut i64).add(1)) = 1; // refcount = 1
    }
    let data_ptr = unsafe { ptr.add(16) };
    unsafe {
        *(data_ptr as *mut i64) = 0; // tag = ok
        *((data_ptr as *mut i64).add(1)) = value;
    }
    data_ptr
}

pub(crate) extern "C" fn rt_result_err(value: i64) -> *mut u8 {
    let layout = std::alloc::Layout::from_size_align(16 + 16, 8).unwrap();
    let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
    if ptr.is_null() {
        eprintln!("turbo: fatal: memory allocation failed");
        std::process::exit(1);
    }
    register_alloc(ptr, layout);
    unsafe {
        *(ptr as *mut i64) = 0; // cap = 0
        *((ptr as *mut i64).add(1)) = 1; // refcount = 1
    }
    let data_ptr = unsafe { ptr.add(16) };
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
    let layout = std::alloc::Layout::from_size_align(16 + 16, 8).unwrap();
    let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
    if ptr.is_null() {
        eprintln!("turbo: fatal: memory allocation failed");
        std::process::exit(1);
    }
    register_alloc(ptr, layout);
    unsafe {
        *(ptr as *mut i64) = 0; // cap = 0
        *(ptr.add(8) as *mut i64) = 1; // refcount = 1
    }
    let data_ptr = unsafe { ptr.add(16) };
    unsafe {
        *(data_ptr as *mut i64) = 1; // tag = some
        *((data_ptr as *mut i64).add(1)) = value;
    }
    data_ptr
}

pub(crate) extern "C" fn rt_option_none() -> *mut u8 {
    let layout = std::alloc::Layout::from_size_align(16 + 16, 8).unwrap();
    let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
    if ptr.is_null() {
        eprintln!("turbo: fatal: memory allocation failed");
        std::process::exit(1);
    }
    register_alloc(ptr, layout);
    unsafe {
        *(ptr as *mut i64) = 0; // cap = 0
        *(ptr.add(8) as *mut i64) = 1; // refcount = 1
    }
    let data_ptr = unsafe { ptr.add(16) };
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
        *(ptr as *mut i64) = len; // cap
        *(ptr.add(8) as *mut i64) = 1; // refcount
    }
    let data_ptr = unsafe { ptr.add(16) };
    unsafe {
        *(data_ptr as *mut i64) = len;
    }
    for (i, part) in parts.iter().enumerate() {
        let cs = cstring_or_empty(*part);
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
    let cs = cstring_or_empty(trimmed);
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
        str_index_oob(index, s.chars().count());
    }
    if let Some(c) = s.chars().nth(index as usize) {
        arena_str(cstring_or_empty(c.to_string()))
    } else {
        str_index_oob(index, s.chars().count());
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
    arena_str(cstring_or_empty(repeated))
}

pub(crate) extern "C" fn rt_read_line() -> *const u8 {
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).unwrap_or(0);
    let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');
    let cs = cstring_or_empty(trimmed);
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
            let cs = cstring_or_empty(msg);
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
    static XORSHIFT_STATE: Cell<u64> = const { Cell::new(0) };
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
    let range = (max_val as u64)
        .wrapping_sub(min_val as u64)
        .wrapping_add(1);
    min_val + (xorshift64_next() % range) as i64
}

// ── System builtins ────────────────────────────────────────────────

pub(crate) extern "C" fn rt_exit(code: i64) {
    std::process::exit(code as i32);
}

// ── CLI argument storage (JIT) ───────────────────────────────────────
//
// The CLI (`turbolang run <file> -- <args>`) installs the program's own
// arguments here via `set_program_args` before calling `jit_run`. `rt_args`
// then materializes them as a Turbo `[str]`. This is the JIT twin of the AOT
// path, where `main(argc, argv)` in turbo_rt.c calls `rt_set_args`; both
// expose the same argv convention — the program's own arguments, excluding
// the binary path (AOT) / the `.tb` source file (JIT).

static PROGRAM_ARGS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Install the program's CLI arguments for `args()` to return.
///
/// Called by the CLI before `jit_run`. The list is exactly the program's own
/// arguments (the trailing args after the source file / `--`), matching what
/// the AOT runtime exposes from `argv[1..]`. With no args the list is empty
/// and `args()` returns an empty `[str]`.
pub fn set_program_args(args: Vec<String>) {
    if let Ok(mut slot) = PROGRAM_ARGS.lock() {
        *slot = args;
    }
}

pub(crate) extern "C" fn rt_args() -> *mut u8 {
    // Build a Turbo `[str]` from the CLI arguments installed by the CLI via
    // `set_program_args`. Mirrors `rt_str_split`'s array-of-string layout
    // ([len][ptr0][ptr1]...), with each string interned in the thread-local
    // arena so it's freed by `rt_arena_reset` after main returns.
    let args = PROGRAM_ARGS.lock().map(|g| g.clone()).unwrap_or_default();
    let len = args.len() as i64;
    let layout = match checked_array_layout(len as usize) {
        Some(l) => l,
        None => {
            eprintln!("runtime error: too many CLI arguments ({})", len);
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
        *(ptr as *mut i64) = len; // cap
        *(ptr.add(8) as *mut i64) = 1; // refcount
    }
    let data_ptr = unsafe { ptr.add(16) };
    unsafe {
        *(data_ptr as *mut i64) = len; // length
    }
    for (i, arg) in args.iter().enumerate() {
        let cs = cstring_or_empty(arg.as_str());
        let p = arena_str(cs) as i64;
        unsafe {
            *((data_ptr as *mut i64).add(1 + i)) = p;
        }
    }
    data_ptr
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
    // Character-indexed (matches `rt_str_char_at`) and panic-proof. Slicing a
    // `&str` at raw byte offsets would panic if an index landed inside a
    // multi-byte UTF-8 sequence — and because this is `extern "C"`, that panic
    // becomes a non-unwinding process abort straight through the JIT frames.
    let chars: Vec<char> = s_str.chars().collect();
    let clen = chars.len() as i64;
    let start = start.max(0).min(clen) as usize;
    let end = end.max(0).min(clen) as usize;
    if start >= end {
        return arena_str(cstring_or_empty(""));
    }
    let sub: String = chars[start..end].iter().collect();
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
        return arena_str(cstring_or_empty(s_str));
    }
    let pad_count = (width - slen) as usize;
    let mut result = String::with_capacity(width as usize);
    for _ in 0..pad_count {
        result.push(c);
    }
    result.push_str(s_str);
    arena_str(cstring_or_empty(result))
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
        return arena_str(cstring_or_empty(s_str));
    }
    let pad_count = (width - slen) as usize;
    let mut result = String::with_capacity(width as usize);
    result.push_str(s_str);
    for _ in 0..pad_count {
        result.push(c);
    }
    arena_str(cstring_or_empty(result))
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

// ── SSRF guard: block loopback / private / link-local destinations ──────
//
// `rt_url_is_http` only validates the *scheme*. By itself that still lets a
// program reach internal services and — most dangerously — cloud
// instance-metadata endpoints (169.254.169.254), a classic SSRF pivot. The
// helpers below additionally inspect the host. This is a direct port of the
// matching block in `turbo_rt.c` (`rt_url_extract_host`, `rt_host_ipv4`,
// `rt_ipv4_is_blocked`, `rt_host_is_blocked`, `rt_http_url_blocked_reason`)
// so `turbolang run` (JIT) and `turbolang build && ./prog` (AOT) behave
// identically: both block, and both honor `TURBO_ALLOW_PRIVATE_HOSTS=1`.
//
// Default: ON (block private hosts). Opt out with TURBO_ALLOW_PRIVATE_HOSTS=1.
//
// Scope / known gap (kept in sync with the C note): we parse numeric IP
// literals (dotted-quad plus the inet_aton shorthand/octal/hex forms attackers
// use to smuggle private IPs) and the `localhost` name. We deliberately do NOT
// resolve arbitrary DNS names here (no DNS lookup): doing so would add a TOCTOU
// window versus curl's own resolution. A hostname that resolves to a private
// address via DNS rebinding is therefore not caught — layer network egress
// controls for that.

/// Parse a leading C-style unsigned integer the way `strtoul(p, &end, 0)`
/// does: base auto-detected from the prefix (`0x`/`0X` → hex, leading `0` →
/// octal, otherwise decimal). Skips leading ASCII whitespace and an optional
/// sign (a `-` wraps, matching `strtoul`). Returns `(value, bytes_consumed)`,
/// or `None` if no digits were consumed (`end == p` in the C sense). Used to
/// reproduce the inet_aton numeric-IP forms exactly.
fn parse_c_ulong(s: &str) -> Option<(u64, usize)> {
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    let mut neg = false;
    if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
        neg = bytes[i] == b'-';
        i += 1;
    }
    let mut base: u64 = 10;
    if i < bytes.len() && bytes[i] == b'0' {
        if i + 2 < bytes.len()
            && (bytes[i + 1] == b'x' || bytes[i + 1] == b'X')
            && bytes[i + 2].is_ascii_hexdigit()
        {
            base = 16;
            i += 2; // skip "0x"; digits follow
        } else {
            base = 8; // leave the '0' — it is a valid octal digit (value 0)
        }
    }
    let digits_start = i;
    let mut value: u64 = 0;
    while i < bytes.len() {
        let c = bytes[i];
        let digit = match base {
            16 => (c as char).to_digit(16),
            8 => {
                if (b'0'..=b'7').contains(&c) {
                    Some((c - b'0') as u32)
                } else {
                    None
                }
            }
            _ => {
                if c.is_ascii_digit() {
                    Some((c - b'0') as u32)
                } else {
                    None
                }
            }
        };
        match digit {
            Some(d) => {
                value = value.wrapping_mul(base).wrapping_add(d as u64);
                i += 1;
            }
            None => break,
        }
    }
    if i == digits_start {
        return None; // no digits consumed
    }
    if neg {
        value = value.wrapping_neg();
    }
    Some((value, i))
}

/// inet_aton-style numeric IPv4 parser. Accepts 1–4 dotted parts, each
/// decimal/octal/hex, exactly like the C resolver (and thus curl). Returns the
/// address in host byte order, or `None` if `host` is not a numeric IPv4
/// literal. Mirrors `rt_host_ipv4` in `turbo_rt.c`.
fn rt_host_ipv4(host: &str) -> Option<u32> {
    if host.is_empty() {
        return None;
    }
    let mut parts: [u64; 4] = [0; 4];
    let mut n = 0usize;
    let mut rest = host;
    while n < 4 {
        let (v, consumed) = parse_c_ulong(rest)?;
        if v > 0xffff_ffff {
            return None; // part too large
        }
        parts[n] = v;
        n += 1;
        rest = &rest[consumed..];
        if let Some(r) = rest.strip_prefix('.') {
            if r.is_empty() {
                return None; // trailing dot
            }
            rest = r;
        } else {
            break;
        }
    }
    if !rest.is_empty() {
        return None; // trailing junk
    }
    let addr: u64 = match n {
        1 => parts[0],
        2 => {
            // a.b -> a.bbbbbb
            if parts[0] > 0xff || parts[1] > 0xff_ffff {
                return None;
            }
            (parts[0] << 24) | parts[1]
        }
        3 => {
            // a.b.c -> a.b.cccc
            if parts[0] > 0xff || parts[1] > 0xff || parts[2] > 0xffff {
                return None;
            }
            (parts[0] << 24) | (parts[1] << 16) | parts[2]
        }
        4 => {
            // a.b.c.d
            if parts[0] > 0xff || parts[1] > 0xff || parts[2] > 0xff || parts[3] > 0xff {
                return None;
            }
            (parts[0] << 24) | (parts[1] << 16) | (parts[2] << 8) | parts[3]
        }
        _ => return None,
    };
    Some(addr as u32)
}

/// True if the host-byte-order IPv4 address is loopback / private / link-local
/// (incl. the 169.254.169.254 metadata endpoint). Mirrors `rt_ipv4_is_blocked`.
fn rt_ipv4_is_blocked(a: u32) -> bool {
    let o1 = (a >> 24) & 0xff;
    let o2 = (a >> 16) & 0xff;
    if o1 == 0 {
        return true; // 0.0.0.0/8 "this host"
    }
    if o1 == 127 {
        return true; // 127.0.0.0/8 loopback
    }
    if o1 == 10 {
        return true; // 10.0.0.0/8 private
    }
    if o1 == 172 && (16..=31).contains(&o2) {
        return true; // 172.16.0.0/12 private
    }
    if o1 == 192 && o2 == 168 {
        return true; // 192.168.0.0/16 private
    }
    if o1 == 169 && o2 == 254 {
        return true; // 169.254.0.0/16 link-local incl. metadata
    }
    false
}

/// True if the (scheme-stripped, port-stripped, bracket-stripped) host should
/// be blocked. Handles `localhost`, IPv6 textual literals, and the numeric
/// IPv4 forms. Mirrors `rt_host_is_blocked` in `turbo_rt.c`.
fn rt_host_is_blocked(host: &str) -> bool {
    if host.is_empty() {
        return false;
    }
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    if host.contains(':') {
        // IPv6 textual literal (brackets already stripped by the caller).
        if host == "::1" {
            return true; // loopback
        }
        if host == "::" {
            return true; // unspecified
        }
        let lower = host.to_ascii_lowercase();
        if lower.starts_with("fe80:") {
            return true; // link-local
        }
        if lower.starts_with("fc") || lower.starts_with("fd") {
            return true; // fc00::/7 unique-local
        }
        // IPv4-mapped form e.g. ::ffff:127.0.0.1 — classify the trailing quad.
        if let Some(idx) = host.rfind(':') {
            if let Some(v4) = rt_host_ipv4(&host[idx + 1..]) {
                if rt_ipv4_is_blocked(v4) {
                    return true;
                }
            }
        }
        return false;
    }
    if let Some(v4) = rt_host_ipv4(host) {
        return rt_ipv4_is_blocked(v4);
    }
    false // a regular domain name — not resolved here (documented gap)
}

/// Extract the host portion of an http(s) URL: strips scheme, userinfo
/// (`user:pass@`), port, and IPv6 brackets. Returns `None` if no host can be
/// isolated, or if the host is `>= 256` bytes — longer than any valid DNS name
/// (<= 253) or numeric literal. The caller treats `None` as fail-closed
/// (blocked) so an attacker cannot pad an over-length numeric IP past the
/// limit to skip the check. Mirrors `rt_url_extract_host` in `turbo_rt.c`.
fn rt_url_extract_host(url: &str) -> Option<String> {
    let after_scheme = if url
        .get(..7)
        .is_some_and(|s| s.eq_ignore_ascii_case("http://"))
    {
        &url[7..]
    } else if url
        .get(..8)
        .is_some_and(|s| s.eq_ignore_ascii_case("https://"))
    {
        &url[8..]
    } else {
        return None;
    };
    // authority ends at the first '/', '?', or '#'
    let auth_end = after_scheme
        .find(['/', '?', '#'])
        .unwrap_or(after_scheme.len());
    let authority = &after_scheme[..auth_end];
    // userinfo: host starts after the last '@' inside the authority
    let host_part = match authority.rfind('@') {
        Some(idx) => &authority[idx + 1..],
        None => authority,
    };
    let host = if let Some(stripped) = host_part.strip_prefix('[') {
        // IPv6 literal: up to the closing ']'
        match stripped.find(']') {
            Some(end) => &stripped[..end],
            None => stripped,
        }
    } else {
        // strip ":port"
        match host_part.find(':') {
            Some(idx) => &host_part[..idx],
            None => host_part,
        }
    };
    let len = host.len();
    if len == 0 || len >= 256 {
        return None;
    }
    Some(host.to_string())
}

/// Returns `None` if the URL is allowed, or `Some(reason)` if it should be
/// blocked. Combines the scheme check with the SSRF host check. Fail-closed:
/// a host that cannot be isolated (empty or over-length) is blocked unless
/// `TURBO_ALLOW_PRIVATE_HOSTS=1`. Mirrors `rt_http_url_blocked_reason` in
/// `turbo_rt.c` — keep the two in lockstep.
fn rt_http_url_blocked_reason(url: &str) -> Option<&'static str> {
    if !rt_url_is_http(url) {
        return Some("non-http(s) scheme");
    }
    // Opt-out for trusted environments. Match the C runtime's strictness:
    // exactly the string "1".
    if std::env::var("TURBO_ALLOW_PRIVATE_HOSTS").as_deref() == Ok("1") {
        return None;
    }
    match rt_url_extract_host(url) {
        Some(host) => {
            if rt_host_is_blocked(&host) {
                Some("private/loopback host blocked (set TURBO_ALLOW_PRIVATE_HOSTS=1 to allow)")
            } else {
                None
            }
        }
        // Fail closed: could not isolate a host (empty, or longer than a valid
        // DNS name). Block rather than allow so an over-length numeric IP can't
        // be smuggled past the guard.
        None => Some(
            "unparseable or over-length host blocked (set TURBO_ALLOW_PRIVATE_HOSTS=1 to allow)",
        ),
    }
}

/// HTTP GET via system curl. Returns response body as a C string.
/// Hardened: rejects non-http(s) schemes and flag-shaped inputs, blocks
/// loopback/private/link-local hosts (SSRF guard, opt out with
/// `TURBO_ALLOW_PRIVATE_HOSTS=1`), pins the protocol allowlist, bounds total
/// time, and uses `--` to prevent flag injection. The host guard now exists in
/// both runtimes — keep in sync with `rt_http_get` in `turbo_rt.c`.
pub(crate) extern "C" fn rt_http_get(url: *const u8) -> *const u8 {
    if url.is_null() {
        eprintln!("[rt_http] blocked URL (non-http(s) scheme): (null)");
        return rt_empty_cstr();
    }
    let url = unsafe { std::ffi::CStr::from_ptr(url as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");
    if let Some(reason) = rt_http_url_blocked_reason(url) {
        eprintln!("[rt_http] blocked URL ({}): {}", reason, url);
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
/// Hardened with the same scheme validation, SSRF host guard, protocol pinning,
/// and flag-injection guards as `rt_http_get`. The host guard now exists in both
/// runtimes — keep in sync with `rt_http_post` in `turbo_rt.c`.
pub(crate) extern "C" fn rt_http_post(url: *const u8, body: *const u8) -> *const u8 {
    if url.is_null() {
        eprintln!("[rt_http] blocked URL (non-http(s) scheme): (null)");
        return rt_empty_cstr();
    }
    let url = unsafe { std::ffi::CStr::from_ptr(url as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");
    if let Some(reason) = rt_http_url_blocked_reason(url) {
        eprintln!("[rt_http] blocked URL ({}): {}", reason, url);
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

/// HTTP POST with custom headers. `headers` is a newline-separated string of
/// headers. Hardened with the same scheme validation and SSRF host guard as
/// `rt_http_get` (opt out with `TURBO_ALLOW_PRIVATE_HOSTS=1`). The host guard
/// now exists in both runtimes — keep in sync with `rt_http_post_with_headers`
/// in `turbo_rt.c`.
pub(crate) extern "C" fn rt_http_post_with_headers(
    url: *const u8,
    body: *const u8,
    headers: *const u8,
) -> *const u8 {
    if url.is_null() {
        eprintln!("[rt_http] blocked URL (non-http(s) scheme): (null)");
        return rt_empty_cstr();
    }
    let url = unsafe { std::ffi::CStr::from_ptr(url as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");
    if let Some(reason) = rt_http_url_blocked_reason(url) {
        eprintln!("[rt_http] blocked URL ({}): {}", reason, url);
        return rt_empty_cstr();
    }
    let body_str = if body.is_null() {
        ""
    } else {
        unsafe { std::ffi::CStr::from_ptr(body as *const std::ffi::c_char) }
            .to_str()
            .unwrap_or("")
    };
    let headers_str = if headers.is_null() {
        ""
    } else {
        unsafe { std::ffi::CStr::from_ptr(headers as *const std::ffi::c_char) }
            .to_str()
            .unwrap_or("")
    };
    let mut cmd = std::process::Command::new("curl");
    cmd.arg("-s")
        .arg("-L")
        .arg("--proto")
        .arg("=http,https")
        .arg("--max-time")
        .arg("30")
        .arg("--max-redirs")
        .arg("5")
        .arg("-X")
        .arg("POST");
    for header in headers_str.split('\n') {
        let h = header.trim();
        if !h.is_empty() {
            cmd.arg("-H").arg(h);
        }
    }
    let output = cmd.arg("-d").arg(body_str).arg("--").arg(url).output();
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
    let cs = cstring_or_empty(result);
    arena_str(cs)
}

/// Build a JSON object from key-value pairs separated by \x1F (unit separator).
/// Format: "key1\x1Fvalue1\x1Fkey2\x1Fvalue2"
pub(crate) extern "C" fn rt_json_build(pairs: *const u8) -> *const u8 {
    let pairs_str = if pairs.is_null() {
        ""
    } else {
        unsafe { std::ffi::CStr::from_ptr(pairs as *const std::ffi::c_char) }
            .to_str()
            .unwrap_or("")
    };
    let parts: Vec<&str> = pairs_str.split('\x1F').collect();
    let mut map = serde_json::Map::new();
    let mut i = 0;
    while i + 1 < parts.len() {
        map.insert(
            parts[i].to_string(),
            serde_json::Value::String(parts[i + 1].to_string()),
        );
        i += 2;
    }
    let result = serde_json::Value::Object(map).to_string();
    let cs = cstring_or_empty(result);
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
            let cs = cstring_or_empty(s);
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

pub(crate) extern "C" fn rt_float_to_int(f: f64) -> i64 {
    f as i64
}

pub(crate) extern "C" fn rt_int_to_float(i: i64) -> f64 {
    i as f64
}

pub(crate) extern "C" fn rt_str_from_char(code: i64) -> *const u8 {
    let c = (code & 0xFF) as u8;
    let s = String::from(c as char);
    let cs = cstring_or_empty(s);
    arena_str(cs)
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
/// Maximum bytes for a single HTTP header line.
const RT_HTTP_MAX_HEADER_LINE: usize = 8192;
const RT_RESPONSE_SEP: char = '\u{1f}';

// ── HTTP server tunables (http_config) ──────────────────────────────────
//
// JIT twin of the C runtime's `g_http_config` (turbo_rt.c). Set at startup via
// `http_config(key, value)` before `http_listen`; read by the accept loop and
// per-connection handlers once listening. Stored as atomics since worker
// threads read them concurrently, though there is no writer after listen
// begins. Keep the keys, defaults, and validation in lockstep with the C side.
use std::sync::atomic::AtomicI64;
static HTTP_CFG_MAX_BODY: AtomicI64 = AtomicI64::new(RT_HTTP_MAX_BODY as i64);
static HTTP_CFG_MAX_HEADER: AtomicI64 = AtomicI64::new(16 * 1024);
static HTTP_CFG_MAX_CONN: AtomicI64 = AtomicI64::new(256);
static HTTP_CFG_READ_TIMEOUT_MS: AtomicI64 = AtomicI64::new(10000);
static HTTP_CFG_WRITE_TIMEOUT_MS: AtomicI64 = AtomicI64::new(10000);
static HTTP_CFG_KEEPALIVE_MAX: AtomicI64 = AtomicI64::new(1000);
static HTTP_CFG_IDLE_TIMEOUT_MS: AtomicI64 = AtomicI64::new(10000);

/// Graceful-shutdown flag, set from the SIGTERM/SIGINT handler installed in
/// `rt_http_listen`.
static HTTP_SHUTDOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

const RT_HTTP_CFG_MAX_HEADER_LIMIT: i64 = 16 * 1024 * 1024;
const RT_HTTP_CFG_MAX_CONN_LIMIT: i64 = 1_000_000;
const RT_HTTP_CFG_MIN_HEADER: i64 = 256;

/// `http_config(key, value)` -> 1 on success, 0 on unknown key or bad value.
/// Must be called before `http_listen`. Twin of C `rt_http_config`.
pub(crate) extern "C" fn rt_http_config(key: *const u8, value: i64) -> i64 {
    if key.is_null() {
        eprintln!("runtime error: http_config: null key");
        return 0;
    }
    let key_str = unsafe { std::ffi::CStr::from_ptr(key as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");
    if value < 1 {
        eprintln!(
            "runtime error: http_config: '{}' must be >= 1 (got {})",
            key_str, value
        );
        return 0;
    }
    use std::sync::atomic::Ordering;
    match key_str {
        "max_body_bytes" => {
            HTTP_CFG_MAX_BODY.store(value, Ordering::Relaxed);
            1
        }
        "max_header_bytes" => {
            if !(RT_HTTP_CFG_MIN_HEADER..=RT_HTTP_CFG_MAX_HEADER_LIMIT).contains(&value) {
                eprintln!(
                    "runtime error: http_config: 'max_header_bytes' must be in [{}, {}] (got {})",
                    RT_HTTP_CFG_MIN_HEADER, RT_HTTP_CFG_MAX_HEADER_LIMIT, value
                );
                return 0;
            }
            HTTP_CFG_MAX_HEADER.store(value, Ordering::Relaxed);
            1
        }
        "max_connections" => {
            if value > RT_HTTP_CFG_MAX_CONN_LIMIT {
                eprintln!(
                    "runtime error: http_config: 'max_connections' must be <= {} (got {})",
                    RT_HTTP_CFG_MAX_CONN_LIMIT, value
                );
                return 0;
            }
            HTTP_CFG_MAX_CONN.store(value, Ordering::Relaxed);
            1
        }
        "read_timeout_ms" => {
            HTTP_CFG_READ_TIMEOUT_MS.store(value, Ordering::Relaxed);
            1
        }
        "write_timeout_ms" => {
            HTTP_CFG_WRITE_TIMEOUT_MS.store(value, Ordering::Relaxed);
            1
        }
        "keepalive_max_requests" => {
            HTTP_CFG_KEEPALIVE_MAX.store(value, Ordering::Relaxed);
            1
        }
        "idle_timeout_ms" => {
            HTTP_CFG_IDLE_TIMEOUT_MS.store(value, Ordering::Relaxed);
            1
        }
        _ => {
            eprintln!("runtime error: http_config: unknown key '{}'", key_str);
            0
        }
    }
}

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
    use std::sync::atomic::Ordering;

    // Snapshot config once per connection (set before listen, never mutated).
    let max_body = HTTP_CFG_MAX_BODY.load(Ordering::Relaxed).max(0) as u64;
    let max_header = HTTP_CFG_MAX_HEADER.load(Ordering::Relaxed).max(0) as usize;
    let read_timeout_ms = HTTP_CFG_READ_TIMEOUT_MS.load(Ordering::Relaxed).max(0) as u64;
    let write_timeout_ms = HTTP_CFG_WRITE_TIMEOUT_MS.load(Ordering::Relaxed).max(0) as u64;
    let idle_timeout_ms = HTTP_CFG_IDLE_TIMEOUT_MS.load(Ordering::Relaxed).max(0) as u64;
    let keepalive_max = HTTP_CFG_KEEPALIVE_MAX.load(Ordering::Relaxed).max(0) as u64;
    let ms = |v: u64| std::time::Duration::from_millis(if v == 0 { 1 } else { v });

    // Slowloris-on-write protection: bound how long a single response write may
    // block on a slow-reading client.
    if write_timeout_ms > 0 {
        let _ = stream.set_write_timeout(Some(ms(write_timeout_ms)));
    }
    let write_stream = match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    };
    let mut reader = std::io::BufReader::new(stream);
    let mut writer = write_stream;
    let mut requests_served: u64 = 0;

    loop {
        // During graceful shutdown, stop serving new requests on this
        // (now idle) keep-alive connection.
        if HTTP_SHUTDOWN.load(Ordering::Relaxed) {
            break;
        }
        // Idle keep-alive wait uses the (typically longer) idle timeout; the
        // active read timeout is applied once a request has started arriving.
        let _ = reader.get_ref().set_read_timeout(Some(ms(idle_timeout_ms)));
        // BL-25 A1: record the per-thread string-arena high-water mark at the
        // start of every request. Each string-returning rt_* call invoked by
        // the handler appends to the thread-local arena; without a reset the
        // server would leak every such string for the process lifetime (the
        // JIT only drained the arena after `main()` returned, which a server's
        // `main()` never does). We truncate back to this mark once the
        // response is written, reclaiming exactly the request-scoped strings.
        let req_arena_mark = arena_mark();

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

        // A request has started arriving — switch from the idle keep-alive
        // timeout to the (typically shorter) active read timeout so a slow
        // trickle of header/body bytes cannot hold the worker forever.
        let _ = reader.get_ref().set_read_timeout(Some(ms(read_timeout_ms)));

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
            if headers_raw.len() + line.len() > max_header {
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
                // strtoll + bounds logic in `turbo_rt.c`. A malformed/negative
                // length is 400 Bad Request; a well-formed length above the
                // configured cap is 413 Payload Too Large.
                let raw: i64 = line
                    .split(':')
                    .nth(1)
                    .unwrap_or("0")
                    .trim()
                    .parse()
                    .unwrap_or(-1);
                if raw < 0 {
                    eprintln!("[rt_http] rejecting Content-Length: {}", raw);
                    let _ = writer.write_all(
                        b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    );
                    return;
                }
                if raw as u64 > max_body {
                    eprintln!("[rt_http] rejecting Content-Length: {} (over cap)", raw);
                    let _ = writer.write_all(
                        b"HTTP/1.1 413 Payload Too Large\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
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

        // This is the last request on the connection if the client asked to
        // close, we hit the per-connection keep-alive request cap, or a
        // graceful shutdown is in progress.
        requests_served += 1;
        if keepalive_max > 0 && requests_served >= keepalive_max {
            keep_alive = false;
        }
        if HTTP_SHUTDOWN.load(Ordering::Relaxed) {
            keep_alive = false;
        }
        let conn_header = if keep_alive { "keep-alive" } else { "close" };

        // A failed response write (dead peer / write timeout) tears the
        // connection down rather than looping on a broken socket.
        let mut conn_dead = false;

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
                    conn_dead |= writer.write_all(resp.as_bytes()).is_err();
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
                        conn_dead |= writer.write_all(http_resp.as_bytes()).is_err();
                    } else {
                        let http_resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nConnection: {}\r\nContent-Length: {}\r\n\r\n{}",
                            conn_header, resp.len(), resp
                        );
                        conn_dead |= writer.write_all(http_resp.as_bytes()).is_err();
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
            conn_dead |= writer.write_all(not_found.as_bytes()).is_err();
        }

        conn_dead |= writer.flush().is_err();

        // BL-25 A1: reclaim every string this request allocated in the arena,
        // truncating back to the mark taken at the top of the loop. Strings
        // allocated before the mark are untouched, so persistent server state
        // (captured at startup) is never freed. The early `break`/`return`
        // paths above run before any handler call, so they allocate no arena
        // strings and need no reset.
        arena_reset_to(req_arena_mark);

        if conn_dead || !keep_alive {
            break;
        }
    }
}

/// SIGTERM/SIGINT handler: flip the graceful-shutdown flag. Only an atomic
/// store — async-signal-safe.
#[cfg(unix)]
extern "C" fn rt_http_shutdown_signal(_sig: libc::c_int) {
    HTTP_SHUTDOWN.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Install the SIGTERM/SIGINT handlers once for the process (twin of the C
/// runtime's `rt_http_install_signal_handlers`). Rust's std already sets
/// SIGPIPE to SIG_IGN at startup, so response writes to a dead peer surface as
/// EPIPE errors rather than killing the process.
///
/// POSIX-only: `sigaction` and friends have no libc-crate binding on Windows.
/// The Windows build gets the no-op stub below — the HTTP server is not a
/// supported Windows target this cycle (see `aot_compile`), so a JIT-hosted
/// server there simply runs without a graceful-shutdown signal handler.
#[cfg(unix)]
fn rt_http_install_signal_handlers() {
    static SIG_ONCE: std::sync::Once = std::sync::Once::new();
    SIG_ONCE.call_once(|| unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = rt_http_shutdown_signal as *const () as usize;
        libc::sigemptyset(&mut sa.sa_mask);
        sa.sa_flags = 0; // no SA_RESTART / no SA_SIGINFO
        libc::sigaction(libc::SIGTERM, &sa, std::ptr::null_mut());
        libc::sigaction(libc::SIGINT, &sa, std::ptr::null_mut());
    });
}

/// Windows stub: no POSIX signal API to install. The `HTTP_SHUTDOWN` flag is
/// still honored by the accept loop; this simply skips signal-handler wiring.
#[cfg(not(unix))]
fn rt_http_install_signal_handlers() {}

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

    rt_http_install_signal_handlers();

    let routes = Arc::new(SendableRoutes(routes));
    let active_connections = Arc::new(AtomicUsize::new(0));
    let addr = format!("{}:{}", host, port);
    let listener = TcpListener::bind(&addr).expect("failed to bind HTTP server");
    // Non-blocking accept so the loop can poll the graceful-shutdown flag
    // between connections (there is no event loop; a short sleep bounds the
    // idle CPU cost). Each accepted stream is switched back to blocking for
    // the per-connection handler, which applies its own read/write timeouts.
    let _ = listener.set_nonblocking(true);
    let max_conn = HTTP_CFG_MAX_CONN.load(Ordering::Relaxed).max(0) as usize;

    while !HTTP_SHUTDOWN.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _addr)) => {
                let _ = stream.set_nonblocking(false);
                if active_connections.fetch_add(1, Ordering::AcqRel) >= max_conn {
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
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(_) => {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
    }

    // Graceful shutdown: stop accepting, then drain in-flight connections up to
    // a bounded deadline before exiting 0. Detached workers observe
    // HTTP_SHUTDOWN and close after finishing their current request.
    drop(listener);
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(10000);
    while active_connections.load(Ordering::Acquire) > 0 && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    std::process::exit(0);
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
    rt_alloc_string(s)
}

// ── Async runtime functions ─────────────────────────────────────────

/// Sleep the current thread for `ms` milliseconds.
pub(crate) extern "C" fn rt_sleep_ms(ms: i64) {
    std::thread::sleep(std::time::Duration::from_millis(ms as u64));
}

/// Copy a NUL-terminated C string into a fresh refcounted heap string
/// (refcount 1) independent of any request-scoped allocation. Used by the
/// spawn arena-escape fix so a string argument handed to another thread
/// survives past the request that produced it.
unsafe fn rc_copy_cstr(ptr: *const u8) -> *const u8 {
    if ptr.is_null() {
        return ptr;
    }
    let cstr = std::ffi::CStr::from_ptr(ptr as *const std::ffi::c_char);
    let bytes = cstr.to_bytes_with_nul();
    let dst = rc_alloc(bytes.len(), 0);
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), dst, bytes.len());
    dst as *const u8
}

/// Spawn a thunk on a new OS thread with a packed args struct
/// `[fn_ptr, arg0, arg1, ...]` (`num_args + 1` eight-byte slots). The thunk is
/// `extern "C" fn(args_ptr: *mut u8) -> i64`. Returns a pointer to a
/// heap-allocated JoinHandle.
///
/// Arena-escape fix (issue #56, twin of the C runtime `rt_spawn_with_args`):
/// the args struct is allocated by the caller via `rt_struct_alloc` and, for a
/// spawn inside an HTTP request handler, its string arguments are request-
/// scoped — freed once the handler returns. Since the spawned thread outlives
/// the request, we copy the struct into a stable heap buffer and deep-copy each
/// flagged string argument into an independent refcounted allocation before
/// crossing the thread boundary. `ptr_mask` bit i marks arg slot i as a string
/// pointer; `num_args` is the argument count. (The JIT has no arena sentinel,
/// so flagged string args are copied unconditionally — a copy is always safe.)
pub(crate) extern "C" fn rt_spawn_with_args(
    thunk: extern "C" fn(*mut u8) -> i64,
    args_ptr: *mut u8,
    ptr_mask: i64,
    num_args: i64,
) -> *mut u8 {
    let num_args = if num_args < 0 { 0 } else { num_args as usize };
    let slots = num_args + 1;

    // Copy the args struct into a stable heap buffer owned by this call.
    let mut buf: Vec<i64> = vec![0i64; slots];
    if !args_ptr.is_null() {
        unsafe {
            std::ptr::copy_nonoverlapping(args_ptr as *const i64, buf.as_mut_ptr(), slots);
        }
    }
    // Deep-copy flagged string arguments so they outlive the request arena.
    for i in 0..num_args {
        if (ptr_mask >> i) & 1 == 1 {
            let s = buf[i + 1] as usize as *const u8;
            if !s.is_null() {
                buf[i + 1] = unsafe { rc_copy_cstr(s) } as i64;
            }
        }
    }

    // Hand the buffer to the thread as a raw pointer, reclaiming it after the
    // thunk consumes it. Copied string args are released by generated thunk
    // cleanup after the callee returns.
    let data_ptr = buf.as_mut_ptr();
    let cap = buf.capacity();
    let len = buf.len();
    std::mem::forget(buf);
    let args_addr = data_ptr as usize;

    let handle = std::thread::spawn(move || {
        let result = thunk(args_addr as *mut u8);
        unsafe {
            drop(Vec::from_raw_parts(args_addr as *mut i64, len, cap));
        }
        result
    });
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

    let layout = std::alloc::Layout::from_size_align(16 + 16, 8).unwrap();
    let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
    if ptr.is_null() {
        eprintln!("turbo: fatal: memory allocation failed");
        std::process::exit(1);
    }
    register_alloc(ptr, layout);
    unsafe {
        *(ptr as *mut i64) = 0; // cap = 0
        *(ptr.add(8) as *mut i64) = 1; // refcount = 1
    }
    let data_ptr = unsafe { ptr.add(16) };
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
    let layout = std::alloc::Layout::from_size_align(16 + 16, 8).unwrap();
    let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
    if ptr.is_null() {
        eprintln!("turbo: fatal: memory allocation failed");
        std::process::exit(1);
    }
    register_alloc(ptr, layout);
    unsafe {
        *(ptr as *mut i64) = 0; // cap = 0
        *(ptr.add(8) as *mut i64) = 1; // refcount = 1
    }
    let data_ptr = unsafe { ptr.add(16) };
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

/// Mutex update callback ABI: `(env_ptr, old_value) -> new_value`.
/// Closures are compiled with `CallConv::Fast`, which is ABI-compatible with
/// `extern "C"` for integer/pointer signatures on the supported targets — the
/// same assumption `RouteHandler` relies on for HTTP route callbacks.
pub(crate) type MutexUpdateFn = extern "C" fn(*const u8, i64) -> i64;

/// Atomically read-modify-write the value inside a mutex. The closure
/// (`fn_ptr` + `env_ptr` pair) runs while the lock guard is held, so a
/// read AND a write happen as one critical section (e.g. a shared counter).
/// Returns the new value. The `MutexGuard` is RAII: even if the callback
/// unwinds, the lock is released as the guard drops. The closure must not
/// touch the same mutex (`std::sync::Mutex` is non-reentrant — it deadlocks).
pub(crate) extern "C" fn rt_mutex_update(
    m: *const u8,
    fn_ptr: *const u8,
    env_ptr: *const u8,
) -> i64 {
    let arc = unsafe { std::sync::Arc::from_raw(m as *const std::sync::Mutex<i64>) };
    let cb: MutexUpdateFn = unsafe { std::mem::transmute(fn_ptr) };
    let new_val = {
        let mut guard = arc.lock().unwrap();
        let new = cb(env_ptr, *guard);
        *guard = new;
        new
    };
    let _ = std::sync::Arc::into_raw(arc); // don't drop
    new_val
}

/// Clone a mutex handle (increments the Arc refcount). Returns a new pointer.
pub(crate) extern "C" fn rt_mutex_clone(m: *const u8) -> *mut u8 {
    let arc = unsafe { std::sync::Arc::from_raw(m as *const std::sync::Mutex<i64>) };
    let cloned = arc.clone();
    let _ = std::sync::Arc::into_raw(arc); // don't drop original
    std::sync::Arc::into_raw(cloned) as *mut u8
}

// ── HashMap runtime functions ───────────────────────────────────────
//
// BL-25 A2 note: unlike the AOT C runtime, the JIT hashmap stores its keys
// and values as Rust `String`s owned by a boxed `HashMap` (global allocator),
// completely independent of the per-request string arena. The per-request
// arena reset (`arena_reset_to`, above) only frees entries the request pushed
// into `STRING_ARENA` — the value copies handed back by `rt_hashmap_get` /
// `rt_hashmap_keys` — never the map's own owned key/value storage. A server
// hashmap created at startup and mutated inside a handler therefore keeps its
// data across requests with no use-after-free, so the JIT needs no equivalent
// of the AOT `persistent`/arena-scoping fix.
//
// BL-26 note: the map handle is shared across `spawn`ed OS threads by its raw
// `i64` pointer. Forming `&mut *ptr` (or even two `&*ptr`) to the same boxed
// `HashMap` from two threads is a data race / UB — concurrent inc/set lost
// updates and concurrent rehash corrupted the table (segfault). The map is
// therefore stored behind a `Mutex` and every operation holds the lock for its
// whole duration, so access is serialized and no aliasing `&mut` ever crosses a
// thread boundary. The handle the codegen sees is unchanged — still an opaque
// `i64` pointer; only its pointee gains the lock. Single-threaded semantics are
// identical (a lock/unlock per op). No operation calls back into another
// hashmap op while holding the lock, so the non-reentrant `Mutex` cannot
// deadlock, and every value handed back to the program is an owned copy (an
// arena string or a fresh array), never a borrow into the locked map.

/// The pointee behind a JIT hashmap handle: a `HashMap<String, String>` guarded
/// by a `Mutex` so concurrent `spawn` access is data-race-free (BL-26).
type HashMapHandle = Mutex<HashMap<String, String>>;

/// Lock the hashmap behind `map_ptr` for the duration of one operation.
///
/// The handle box is created by [`rt_hashmap_new`] and intentionally never
/// freed, so it lives for the whole process — the returned guard's `'static`
/// borrow is sound. A poisoned lock (a thread panicking mid-op) is recovered
/// rather than re-panicked: a panic inside these `extern "C"` functions already
/// aborts the process, so poisoning is effectively unreachable, and recovering
/// avoids turning it into a second abort.
fn lock_hashmap(map_ptr: *const u8) -> std::sync::MutexGuard<'static, HashMap<String, String>> {
    let handle: &'static HashMapHandle = unsafe { &*(map_ptr as *const HashMapHandle) };
    handle
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Create a new empty HashMap<String, String>. Returns an opaque pointer.
pub(crate) extern "C" fn rt_hashmap_new() -> *mut u8 {
    let map: HashMap<String, String> = HashMap::new();
    let boxed: Box<HashMapHandle> = Box::new(Mutex::new(map));
    Box::into_raw(boxed) as *mut u8
}

/// Set a key-value pair in the hashmap.
pub(crate) extern "C" fn rt_hashmap_set(map_ptr: *mut u8, key: *const u8, value: *const u8) {
    let key = unsafe { std::ffi::CStr::from_ptr(key as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("")
        .to_string();
    let value = unsafe { std::ffi::CStr::from_ptr(value as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("")
        .to_string();
    lock_hashmap(map_ptr).insert(key, value);
}

/// Get a value by key. Returns a C string pointer, or null if not found.
pub(crate) extern "C" fn rt_hashmap_get(map_ptr: *const u8, key: *const u8) -> *const u8 {
    let key = unsafe { std::ffi::CStr::from_ptr(key as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");
    // Clone out an owned value under the lock, then release it before allocating
    // the arena string — the returned pointer must never borrow into the map.
    let found = lock_hashmap(map_ptr).get(key).cloned();
    match found {
        Some(v) => arena_str(cstring_or_empty(v)),
        None => std::ptr::null(),
    }
}

/// Check if a key exists. Returns 1 (true) or 0 (false).
pub(crate) extern "C" fn rt_hashmap_has(map_ptr: *const u8, key: *const u8) -> i8 {
    let key = unsafe { std::ffi::CStr::from_ptr(key as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");
    if lock_hashmap(map_ptr).contains_key(key) {
        1
    } else {
        0
    }
}

/// Return the number of entries in the hashmap.
pub(crate) extern "C" fn rt_hashmap_len(map_ptr: *const u8) -> i64 {
    lock_hashmap(map_ptr).len() as i64
}

/// Return all keys as a [str] array (same format as rt_str_split).
pub(crate) extern "C" fn rt_hashmap_keys(map_ptr: *const u8) -> *mut u8 {
    // Snapshot the keys as owned strings under the lock, then release it before
    // sorting / allocating the result array (the returned strings are arena
    // copies, never borrows into the map).
    let mut keys: Vec<String> = lock_hashmap(map_ptr).keys().cloned().collect();
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
        *(ptr as *mut i64) = len; // cap
        *(ptr.add(8) as *mut i64) = 1; // refcount
    }
    let data_ptr = unsafe { ptr.add(16) };
    unsafe {
        *(data_ptr as *mut i64) = len;
    }
    for (i, key) in keys.iter().enumerate() {
        let cs = cstring_or_empty(key.as_str());
        unsafe {
            *((data_ptr as *mut i64).add(1 + i)) = arena_str(cs) as i64;
        }
    }
    data_ptr
}

/// Remove a key from the hashmap.
pub(crate) extern "C" fn rt_hashmap_remove(map_ptr: *mut u8, key: *const u8) {
    let key = unsafe { std::ffi::CStr::from_ptr(key as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");
    lock_hashmap(map_ptr).remove(key);
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
    let key = unsafe { std::ffi::CStr::from_ptr(key as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("")
        .to_string();
    lock_hashmap(map_ptr).insert(key, value.to_string());
    map_ptr
}

/// Get an int value by key. Returns 0 on miss (callers that need to
/// distinguish missing from a stored 0 should guard with hashmap_has).
pub(crate) extern "C" fn rt_hashmap_get_int(map_ptr: *const u8, key: *const u8) -> i64 {
    let key = unsafe { std::ffi::CStr::from_ptr(key as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("");
    match lock_hashmap(map_ptr).get(key) {
        Some(v) => v.parse::<i64>().unwrap_or(0),
        None => 0,
    }
}

/// Fused increment: add `delta` to the int value at `key` (a missing key
/// counts as 0), store the result, and return the new value. Mirrors the AOT
/// `rt_hashmap_inc` fast path with a single `entry()` lookup. The JIT keeps the
/// shared String storage, so the observable behaviour (get_int reads the
/// value, get returns its decimal string) stays identical to AOT.
pub(crate) extern "C" fn rt_hashmap_inc(map_ptr: *mut u8, key: *const u8, delta: i64) -> i64 {
    let key = unsafe { std::ffi::CStr::from_ptr(key as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or("")
        .to_string();
    // The whole read-modify-write runs under one held lock, so concurrent
    // increments on a shared map serialize and never lose updates (BL-26).
    let mut map = lock_hashmap(map_ptr);
    let slot = map.entry(key).or_insert_with(|| "0".to_string());
    let new_val = slot.parse::<i64>().unwrap_or(0) + delta;
    *slot = new_val.to_string();
    new_val
}

// ── Generic HashMap<K,V> runtime (Tier 1.2) ─────────────────────────
//
// JIT twin of the AOT descriptor-based map in turbo_rt.c. A typed map stores
// raw i64 value slots keyed by an i64 or a String, guarded by a Mutex so
// concurrent `spawn` access is data-race-free (BL-26). rc-heap values are
// retained on insert and released on overwrite / remove, mirroring
// rt_hashmap_gset / rt_hashmap_gremove. The handle box is leaked (never
// freed), matching the legacy JIT map: the map object itself is leak-but-safe;
// only its rc values are refcounted. `key_kind`: 0 = str, 1 = int.

#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum GKey {
    Int(i64),
    Str(String),
}

struct GMap {
    key_kind: u8,
    val_is_rc: bool,
    entries: HashMap<GKey, i64>,
}

type GMapHandle = Mutex<GMap>;

fn lock_gmap(map_ptr: *const u8) -> std::sync::MutexGuard<'static, GMap> {
    let handle: &'static GMapHandle = unsafe { &*(map_ptr as *const GMapHandle) };
    handle
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Build a `GKey` from the raw ABI key: a bare `i64` for int-keyed maps, or a
/// C-string pointer (copied into an owned `String`) for str-keyed maps.
fn gkey_from(key_kind: u8, key: i64) -> GKey {
    if key_kind == 1 {
        GKey::Int(key)
    } else {
        let s = unsafe { std::ffi::CStr::from_ptr(key as *const std::ffi::c_char) }
            .to_str()
            .unwrap_or("")
            .to_string();
        GKey::Str(s)
    }
}

/// Create a typed map. `key_kind`: 0 = str, 1 = int; `val_is_rc`: values are
/// rc-heap pointers needing retain/release.
pub(crate) extern "C" fn rt_hashmap_new_typed(key_kind: i64, val_is_rc: i64) -> *mut u8 {
    let gmap = GMap {
        key_kind: key_kind as u8,
        val_is_rc: val_is_rc != 0,
        entries: HashMap::new(),
    };
    let boxed: Box<GMapHandle> = Box::new(Mutex::new(gmap));
    Box::into_raw(boxed) as *mut u8
}

/// Insert or overwrite. `value` is the raw 8-byte slot. rc-heap values are
/// retained here and released when they are overwritten.
pub(crate) extern "C" fn rt_hashmap_gset(map_ptr: *mut u8, key: i64, value: i64) {
    let mut map = lock_gmap(map_ptr);
    let is_rc = map.val_is_rc;
    let k = gkey_from(map.key_kind, key);
    if is_rc {
        // Retain the new value before releasing any old one so a self-overwrite
        // (value aliases the stored pointer) never transiently hits zero.
        rt_retain(value as *mut u8);
    }
    if let Some(old) = map.entries.insert(k, value) {
        if is_rc {
            rt_release(old as *mut u8);
        }
    }
}

/// Look up `key`, returning an Optional: some(value) on hit (rc values are
/// retained into the returned Optional), none on miss.
pub(crate) extern "C" fn rt_hashmap_gget(map_ptr: *mut u8, key: i64) -> *mut u8 {
    let map = lock_gmap(map_ptr);
    let k = gkey_from(map.key_kind, key);
    match map.entries.get(&k) {
        Some(&v) => {
            if map.val_is_rc {
                rt_retain(v as *mut u8);
            }
            rt_option_some(v)
        }
        None => rt_option_none(),
    }
}

/// Number of entries in a typed map.
pub(crate) extern "C" fn rt_hashmap_glen(map_ptr: *const u8) -> i64 {
    lock_gmap(map_ptr).entries.len() as i64
}

/// Whether `key` is present. Returns 1 or 0.
pub(crate) extern "C" fn rt_hashmap_ghas(map_ptr: *const u8, key: i64) -> i8 {
    let map = lock_gmap(map_ptr);
    let k = gkey_from(map.key_kind, key);
    if map.entries.contains_key(&k) {
        1
    } else {
        0
    }
}

/// Remove `key`, releasing its rc-heap value if present.
pub(crate) extern "C" fn rt_hashmap_gremove(map_ptr: *mut u8, key: i64) {
    let mut map = lock_gmap(map_ptr);
    let is_rc = map.val_is_rc;
    let k = gkey_from(map.key_kind, key);
    if let Some(old) = map.entries.remove(&k) {
        if is_rc {
            rt_release(old as *mut u8);
        }
    }
}

/// Return all keys as an array: `[str]` for str keys (arena copies), `[int]`
/// for int keys. Sorted for deterministic output, matching rt_hashmap_gkeys.
pub(crate) extern "C" fn rt_hashmap_gkeys(map_ptr: *const u8) -> *mut u8 {
    let mut keys: Vec<GKey> = {
        let map = lock_gmap(map_ptr);
        map.entries.keys().cloned().collect()
    };
    keys.sort();
    let len = keys.len() as i64;
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
        *(ptr as *mut i64) = len; // cap
        *(ptr.add(8) as *mut i64) = 1; // refcount
    }
    let data_ptr = unsafe { ptr.add(16) };
    unsafe {
        *(data_ptr as *mut i64) = len;
    }
    for (i, key) in keys.iter().enumerate() {
        let slot = match key {
            GKey::Int(n) => *n,
            GKey::Str(s) => arena_str(cstring_or_empty(s.as_str())) as i64,
        };
        unsafe {
            *((data_ptr as *mut i64).add(1 + i)) = slot;
        }
    }
    data_ptr
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
        if (*header).load(std::sync::atomic::Ordering::Acquire) == RT_RC_IMMORTAL {
            return;
        }
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
    unsafe {
        if (*header).load(std::sync::atomic::Ordering::Acquire) == RT_RC_IMMORTAL {
            return;
        }
    }
    let prev = unsafe { (*header).fetch_sub(1, std::sync::atomic::Ordering::Release) };
    if prev == 1 {
        std::sync::atomic::fence(std::sync::atomic::Ordering::Acquire);
        // Refcount reached 0 — free the allocation.
        // Raw allocation base is at data_ptr - 16 (cap + refcount header)
        let raw_ptr = unsafe { data_ptr.sub(16) };
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
        *(ptr as *mut i64) = len; // cap
        *(ptr.add(8) as *mut i64) = 1; // refcount
    }
    let data_ptr = unsafe { ptr.add(16) };
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
    let cs = cstring_or_empty(result);
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
        *(ptr as *mut i64) = len; // cap
        *(ptr.add(8) as *mut i64) = 1; // refcount
    }
    let data_ptr = unsafe { ptr.add(16) };
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
        *(ptr as *mut i64) = len; // cap
        *(ptr.add(8) as *mut i64) = 1; // refcount
    }
    let data_ptr = unsafe { ptr.add(16) };
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
        *(ptr as *mut i64) = len; // cap
        *(ptr.add(8) as *mut i64) = 1; // refcount
    }
    let data_ptr = unsafe { ptr.add(16) };
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
        *(ptr as *mut i64) = new_len; // cap
        *(ptr.add(8) as *mut i64) = 1; // refcount
    }
    let data_ptr = unsafe { ptr.add(16) };
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
        let tm = cstd::localtime(&t);
        if tm.is_null() {
            return String::new();
        }
        let fmt_c = cstring_or_empty(fmt);
        let mut buf = [0u8; 256];
        let n = cstd::strftime(
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

// ── SQLite builtins (JIT twins) ──────────────────────────────────────
//
// These are the JIT-side implementations of the `rt_sqlite_*` functions.
// Their AOT twins live in `runtime/turbo_rt_sqlite.c` and MUST behave
// identically (JIT ≡ AOT), which the parity harness enforces. Both sides call
// the same vendored SQLite C API — here via the FFI block below, whose symbols
// are linked in by `build.rs` (which compiles `runtime/vendor/sqlite3.c` into
// the host binary).
//
// String producers (`rt_sqlite_column_str`, `rt_sqlite_error`) return their
// result through `arena_str`, exactly like `rt_str_upper` and friends, so the
// arena/refcount ownership matches every other runtime string. We never hand
// back sqlite's internal pointers — they are always copied. Fallible functions
// build a `Result` with `rt_result_ok` / `rt_result_err`, mirroring
// `rt_try_read_file`.

mod sqlite_ffi {
    use std::ffi::{c_char, c_double, c_int, c_void};

    // SQLite result / open constants (see sqlite3.h).
    pub const SQLITE_OK: c_int = 0;
    pub const SQLITE_ROW: c_int = 100;
    pub const SQLITE_DONE: c_int = 101;
    pub const SQLITE_OPEN_READWRITE: c_int = 0x00000002;
    pub const SQLITE_OPEN_CREATE: c_int = 0x00000004;

    // SQLITE_TRANSIENT == ((sqlite3_destructor_type)-1): tells sqlite to make
    // its own copy of bound text immediately.
    pub const SQLITE_TRANSIENT: isize = -1;

    unsafe extern "C" {
        pub fn sqlite3_open_v2(
            filename: *const c_char,
            pp_db: *mut *mut c_void,
            flags: c_int,
            z_vfs: *const c_char,
        ) -> c_int;
        pub fn sqlite3_close(db: *mut c_void) -> c_int;
        pub fn sqlite3_exec(
            db: *mut c_void,
            sql: *const c_char,
            callback: *const c_void,
            arg: *mut c_void,
            errmsg: *mut *mut c_char,
        ) -> c_int;
        pub fn sqlite3_errmsg(db: *mut c_void) -> *const c_char;
        pub fn sqlite3_free(p: *mut c_void);
        pub fn sqlite3_prepare_v2(
            db: *mut c_void,
            z_sql: *const c_char,
            n_byte: c_int,
            pp_stmt: *mut *mut c_void,
            pz_tail: *mut *const c_char,
        ) -> c_int;
        pub fn sqlite3_bind_int64(stmt: *mut c_void, idx: c_int, v: i64) -> c_int;
        pub fn sqlite3_bind_text(
            stmt: *mut c_void,
            idx: c_int,
            text: *const c_char,
            n: c_int,
            destructor: isize,
        ) -> c_int;
        pub fn sqlite3_bind_double(stmt: *mut c_void, idx: c_int, v: c_double) -> c_int;
        pub fn sqlite3_step(stmt: *mut c_void) -> c_int;
        pub fn sqlite3_column_int64(stmt: *mut c_void, i: c_int) -> i64;
        pub fn sqlite3_column_double(stmt: *mut c_void, i: c_int) -> c_double;
        pub fn sqlite3_column_text(stmt: *mut c_void, i: c_int) -> *const u8;
        pub fn sqlite3_column_count(stmt: *mut c_void) -> c_int;
        pub fn sqlite3_finalize(stmt: *mut c_void) -> c_int;
    }
}

/// Copy a possibly-NULL sqlite C string into a fresh arena string. NULL (an
/// SQL NULL column) becomes the empty string. Mirrors `rt_sqlite_dup` in the C
/// twin.
fn sqlite_dup(ptr: *const std::ffi::c_char) -> *const u8 {
    if ptr.is_null() {
        return arena_str(cstring_or_empty(""));
    }
    let bytes = unsafe { std::ffi::CStr::from_ptr(ptr) }.to_bytes().to_vec();
    arena_str(cstring_or_empty(bytes))
}

/// `sqlite_open(path: str) -> i64 ! str`
pub(crate) extern "C" fn rt_sqlite_open(path: *const u8) -> *mut u8 {
    use sqlite_ffi::*;
    let path_c = if path.is_null() {
        c":memory:".as_ptr()
    } else {
        path as *const std::ffi::c_char
    };
    let mut db: *mut std::ffi::c_void = std::ptr::null_mut();
    let rc = unsafe {
        sqlite3_open_v2(
            path_c,
            &mut db,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            std::ptr::null(),
        )
    };
    if rc != SQLITE_OK {
        let msg = if db.is_null() {
            arena_str(cstring_or_empty("unable to open database"))
        } else {
            sqlite_dup(unsafe { sqlite3_errmsg(db) })
        };
        if !db.is_null() {
            unsafe { sqlite3_close(db) };
        }
        return rt_result_err(msg as i64);
    }
    rt_result_ok(db as i64)
}

/// `sqlite_exec(h: i64, sql: str) -> unit ! str`
pub(crate) extern "C" fn rt_sqlite_exec(h: i64, sql: *const u8) -> *mut u8 {
    use sqlite_ffi::*;
    let db = h as *mut std::ffi::c_void;
    let sql_c = if sql.is_null() {
        c"".as_ptr()
    } else {
        sql as *const std::ffi::c_char
    };
    let mut errmsg: *mut std::ffi::c_char = std::ptr::null_mut();
    let rc = unsafe {
        sqlite3_exec(
            db,
            sql_c,
            std::ptr::null(),
            std::ptr::null_mut(),
            &mut errmsg,
        )
    };
    if rc != SQLITE_OK {
        let msg = if errmsg.is_null() {
            sqlite_dup(unsafe { sqlite3_errmsg(db) })
        } else {
            sqlite_dup(errmsg)
        };
        if !errmsg.is_null() {
            unsafe { sqlite3_free(errmsg as *mut std::ffi::c_void) };
        }
        return rt_result_err(msg as i64);
    }
    rt_result_ok(0)
}

/// `sqlite_prepare(h: i64, sql: str) -> i64 ! str`
pub(crate) extern "C" fn rt_sqlite_prepare(h: i64, sql: *const u8) -> *mut u8 {
    use sqlite_ffi::*;
    let db = h as *mut std::ffi::c_void;
    let sql_c = if sql.is_null() {
        c"".as_ptr()
    } else {
        sql as *const std::ffi::c_char
    };
    let mut stmt: *mut std::ffi::c_void = std::ptr::null_mut();
    let rc = unsafe { sqlite3_prepare_v2(db, sql_c, -1, &mut stmt, std::ptr::null_mut()) };
    if rc != SQLITE_OK {
        let msg = sqlite_dup(unsafe { sqlite3_errmsg(db) });
        return rt_result_err(msg as i64);
    }
    rt_result_ok(stmt as i64)
}

/// `sqlite_bind_int(stmt, idx, v) -> i64` (sqlite rc; idx is 1-based)
pub(crate) extern "C" fn rt_sqlite_bind_int(stmt: i64, idx: i64, v: i64) -> i64 {
    unsafe { sqlite_ffi::sqlite3_bind_int64(stmt as *mut _, idx as i32, v) as i64 }
}

/// `sqlite_bind_str(stmt, idx, s) -> i64`
pub(crate) extern "C" fn rt_sqlite_bind_str(stmt: i64, idx: i64, s: *const u8) -> i64 {
    use sqlite_ffi::*;
    let s_c = if s.is_null() {
        c"".as_ptr()
    } else {
        s as *const std::ffi::c_char
    };
    unsafe { sqlite3_bind_text(stmt as *mut _, idx as i32, s_c, -1, SQLITE_TRANSIENT) as i64 }
}

/// `sqlite_bind_float(stmt, idx, f) -> i64`
pub(crate) extern "C" fn rt_sqlite_bind_float(stmt: i64, idx: i64, f: f64) -> i64 {
    unsafe { sqlite_ffi::sqlite3_bind_double(stmt as *mut _, idx as i32, f) as i64 }
}

/// `sqlite_step(stmt) -> 1 row / 0 done / -1 error`
pub(crate) extern "C" fn rt_sqlite_step(stmt: i64) -> i64 {
    let rc = unsafe { sqlite_ffi::sqlite3_step(stmt as *mut _) };
    if rc == sqlite_ffi::SQLITE_ROW {
        1
    } else if rc == sqlite_ffi::SQLITE_DONE {
        0
    } else {
        -1
    }
}

/// `sqlite_column_int(stmt, i) -> i64` (0-based column index)
pub(crate) extern "C" fn rt_sqlite_column_int(stmt: i64, i: i64) -> i64 {
    unsafe { sqlite_ffi::sqlite3_column_int64(stmt as *mut _, i as i32) }
}

/// `sqlite_column_str(stmt, i) -> str`
pub(crate) extern "C" fn rt_sqlite_column_str(stmt: i64, i: i64) -> *const u8 {
    let text = unsafe { sqlite_ffi::sqlite3_column_text(stmt as *mut _, i as i32) };
    sqlite_dup(text as *const std::ffi::c_char)
}

/// `sqlite_column_float(stmt, i) -> f64`
pub(crate) extern "C" fn rt_sqlite_column_float(stmt: i64, i: i64) -> f64 {
    unsafe { sqlite_ffi::sqlite3_column_double(stmt as *mut _, i as i32) }
}

/// `sqlite_column_count(stmt) -> i64`
pub(crate) extern "C" fn rt_sqlite_column_count(stmt: i64) -> i64 {
    unsafe { sqlite_ffi::sqlite3_column_count(stmt as *mut _) as i64 }
}

/// `sqlite_finalize(stmt) -> i64` (sqlite rc)
pub(crate) extern "C" fn rt_sqlite_finalize(stmt: i64) -> i64 {
    unsafe { sqlite_ffi::sqlite3_finalize(stmt as *mut _) as i64 }
}

/// `sqlite_error(h) -> str` (last error message for the connection)
pub(crate) extern "C" fn rt_sqlite_error(h: i64) -> *const u8 {
    if h == 0 {
        return arena_str(cstring_or_empty("invalid database handle"));
    }
    sqlite_dup(unsafe { sqlite_ffi::sqlite3_errmsg(h as *mut _) })
}

/// `sqlite_close(h) -> i64` (sqlite rc)
pub(crate) extern "C" fn rt_sqlite_close(h: i64) -> i64 {
    unsafe { sqlite_ffi::sqlite3_close(h as *mut _) as i64 }
}

#[cfg(test)]
mod ssrf_tests {
    use super::{
        rt_host_ipv4, rt_host_is_blocked, rt_http_url_blocked_reason, rt_ipv4_is_blocked,
        rt_url_extract_host,
    };

    #[test]
    fn numeric_ipv4_forms_parse_like_inet_aton() {
        // dotted-quad
        assert_eq!(rt_host_ipv4("127.0.0.1"), Some(0x7f00_0001));
        // decimal integer form
        assert_eq!(rt_host_ipv4("2130706433"), Some(0x7f00_0001));
        // hex form
        assert_eq!(rt_host_ipv4("0x7f000001"), Some(0x7f00_0001));
        // octal form (0177 == 127)
        assert_eq!(rt_host_ipv4("0177.0.0.1"), Some(0x7f00_0001));
        // two-part shorthand a.b -> 127.0.0.1
        assert_eq!(rt_host_ipv4("127.1"), Some(0x7f00_0001));
        // metadata endpoint
        assert_eq!(rt_host_ipv4("169.254.169.254"), Some(0xa9fe_a9fe));
        // not numeric
        assert_eq!(rt_host_ipv4("example.com"), None);
        // trailing junk / too many parts
        assert_eq!(rt_host_ipv4("1.2.3.4.5"), None);
    }

    #[test]
    fn blocked_ranges() {
        for ip in [
            "127.0.0.1",
            "10.0.0.1",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.1",
            "169.254.169.254",
            "0.0.0.0",
        ] {
            let v4 = rt_host_ipv4(ip).expect("numeric");
            assert!(rt_ipv4_is_blocked(v4), "{ip} should be blocked");
        }
        // public addresses are allowed
        let v4 = rt_host_ipv4("8.8.8.8").unwrap();
        assert!(!rt_ipv4_is_blocked(v4));
        // 172.15/172.32 are outside the private /12
        assert!(!rt_ipv4_is_blocked(rt_host_ipv4("172.15.0.1").unwrap()));
        assert!(!rt_ipv4_is_blocked(rt_host_ipv4("172.32.0.1").unwrap()));
    }

    #[test]
    fn hostname_and_ipv6_classification() {
        assert!(rt_host_is_blocked("localhost"));
        assert!(rt_host_is_blocked("LOCALHOST"));
        assert!(rt_host_is_blocked("::1"));
        assert!(rt_host_is_blocked("::"));
        assert!(rt_host_is_blocked("fe80::1"));
        assert!(rt_host_is_blocked("fc00::1"));
        assert!(rt_host_is_blocked("fd12:3456::1"));
        assert!(rt_host_is_blocked("::ffff:127.0.0.1"));
        assert!(!rt_host_is_blocked("example.com"));
        // A public IPv6 whose tail after the last ':' is not a numeric literal
        // is allowed.
        assert!(!rt_host_is_blocked("2001:db8::cafe"));
        // Parity note: like the C runtime, the IPv4-mapped fallback classifies
        // the tail after the last ':' via the numeric-IPv4 parser. A tail that
        // happens to be a small decimal (e.g. "8888" -> 0.0.34.184, in
        // 0.0.0.0/8) is therefore conservatively blocked in BOTH runtimes. We
        // assert that here to lock the JIT/AOT behaviour together.
        assert!(rt_host_is_blocked("2001:4860:4860::8888"));
    }

    #[test]
    fn host_extraction() {
        assert_eq!(
            rt_url_extract_host("http://127.0.0.1/").as_deref(),
            Some("127.0.0.1")
        );
        assert_eq!(
            rt_url_extract_host("https://user:pass@example.com:8443/path?q=1").as_deref(),
            Some("example.com")
        );
        assert_eq!(
            rt_url_extract_host("http://[::1]:8080/").as_deref(),
            Some("::1")
        );
        assert_eq!(
            rt_url_extract_host("http://169.254.169.254/latest").as_deref(),
            Some("169.254.169.254")
        );
        // non-http scheme → None
        assert_eq!(rt_url_extract_host("file:///etc/passwd"), None);
        // over-length host (>= 256 bytes) → None (fail closed at the caller)
        let long = format!("http://{}/", "a".repeat(300));
        assert_eq!(rt_url_extract_host(&long), None);
    }

    #[test]
    fn blocked_reason_default_on() {
        // Non-http scheme is always rejected regardless of the opt-out.
        assert_eq!(
            rt_http_url_blocked_reason("file:///etc/passwd"),
            Some("non-http(s) scheme")
        );
        // The host-blocking assertions below assume the opt-out is NOT active;
        // skip them if a developer has it exported in their shell.
        if std::env::var("TURBO_ALLOW_PRIVATE_HOSTS").as_deref() == Ok("1") {
            return;
        }
        assert!(rt_http_url_blocked_reason("http://169.254.169.254/").is_some());
        assert!(rt_http_url_blocked_reason("http://127.0.0.1:9/").is_some());
        assert!(rt_http_url_blocked_reason("http://2130706433/").is_some());
        assert!(rt_http_url_blocked_reason("http://0x7f000001/").is_some());
        assert!(rt_http_url_blocked_reason("http://localhost/").is_some());
        // over-length host fails closed
        let long = format!("http://{}/", "9".repeat(300));
        assert!(rt_http_url_blocked_reason(&long).is_some());
        // a public host is allowed
        assert_eq!(rt_http_url_blocked_reason("https://example.com/"), None);
    }
}

#[cfg(test)]
mod hashmap_concurrency_tests {
    //! BL-26 regression: the JIT hashmap is shared across `spawn`ed threads by
    //! its raw `i64` handle. Before the map was put behind a `Mutex`, each op
    //! formed `&mut *ptr` to the same boxed `HashMap`, so two threads mutating
    //! the same map was a data race / UB — lost updates and table corruption
    //! (segfault). These tests hammer one shared map from many OS threads and
    //! assert an exact, deterministic result; they crash/fail reliably against
    //! the unlocked code and pass reliably once every op holds the lock.
    use super::{
        rt_hashmap_get_int, rt_hashmap_inc, rt_hashmap_len, rt_hashmap_new, rt_hashmap_set_int,
        HashMapHandle,
    };
    use std::ffi::CString;

    /// Reconstruct and drop the leaked handle box so the test doesn't leak.
    fn free_map(map: *mut u8) {
        // SAFETY: `map` was produced by `rt_hashmap_new`, which boxes a
        // `HashMapHandle`, and is no longer referenced by any thread.
        unsafe {
            drop(Box::from_raw(map as *mut HashMapHandle));
        }
    }

    #[test]
    fn concurrent_inc_on_shared_key_has_no_lost_updates() {
        const N: usize = 8;
        const K: i64 = 50_000;

        let map = rt_hashmap_new();
        let map_addr = map as usize;
        // The key CString must outlive every worker thread.
        let key = CString::new("shared").unwrap();
        let key_addr = key.as_ptr() as usize;

        let handles: Vec<_> = (0..N)
            .map(|_| {
                std::thread::spawn(move || {
                    let map_ptr = map_addr as *mut u8;
                    let key_ptr = key_addr as *const u8;
                    for _ in 0..K {
                        rt_hashmap_inc(map_ptr, key_ptr, 1);
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().expect("worker thread panicked (map corruption)");
        }

        let total = rt_hashmap_get_int(map as *const u8, key.as_ptr() as *const u8);
        assert_eq!(
            total,
            N as i64 * K,
            "lost updates: concurrent rt_hashmap_inc raced"
        );
        assert_eq!(rt_hashmap_len(map as *const u8), 1);
        drop(key);
        free_map(map);
    }

    #[test]
    fn concurrent_distinct_inserts_do_not_corrupt_the_table() {
        // Each thread inserts its own disjoint key range, so the map grows
        // (and rehashes) under concurrent mutation from every thread — the path
        // most likely to segfault when access is unsynchronized.
        const N: usize = 8;
        const K: usize = 4_000;

        let map = rt_hashmap_new();
        let map_addr = map as usize;

        let handles: Vec<_> = (0..N)
            .map(|tid| {
                std::thread::spawn(move || {
                    let map_ptr = map_addr as *mut u8;
                    for i in 0..K {
                        let k = CString::new(format!("t{tid}_{i}")).unwrap();
                        rt_hashmap_set_int(map_ptr, k.as_ptr() as *const u8, (tid * K + i) as i64);
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().expect("worker thread panicked (map corruption)");
        }

        assert_eq!(
            rt_hashmap_len(map as *const u8),
            (N * K) as i64,
            "concurrent inserts dropped or corrupted entries"
        );
        // Spot-check that values survived intact across the concurrent rehashes.
        for tid in 0..N {
            for i in [0usize, K / 2, K - 1] {
                let k = CString::new(format!("t{tid}_{i}")).unwrap();
                assert_eq!(
                    rt_hashmap_get_int(map as *const u8, k.as_ptr() as *const u8),
                    (tid * K + i) as i64
                );
            }
        }
        free_map(map);
    }
}
