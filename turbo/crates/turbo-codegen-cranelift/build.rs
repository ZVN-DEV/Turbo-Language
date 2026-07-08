//! Build script: compile the vendored SQLite amalgamation and link it into
//! whatever binary embeds this crate (notably `turbolang`).
//!
//! # Why this exists
//!
//! The JIT runtime (`turbolang run`) executes user programs *in-process*
//! using the Rust `rt_*` functions in `src/runtime.rs`. The SQLite builtins'
//! JIT twins (`rt_sqlite_*` in `runtime.rs`) call the `sqlite3_*` C API
//! directly via FFI, so the SQLite engine must be linked into the host
//! binary. We compile the vendored amalgamation (`runtime/vendor/sqlite3.c`)
//! into a static archive and hand it to the linker here.
//!
//! The AOT path (`src/aot.rs`) links SQLite separately at `turbolang build`
//! time — it embeds the object we also drop in `OUT_DIR` (see
//! `SQLITE_AOT_OBJECT` in `src/lib.rs`).
//!
//! # No new crate dependencies
//!
//! We deliberately do NOT use the `cc` crate. Like `src/aot.rs`, we shell out
//! to the system C compiler via `std::process::Command`, keeping the
//! dependency graph unchanged.
//!
//! The compile flags below MUST stay in sync with `SQLITE_CFLAGS` in
//! `src/lib.rs` (used by the AOT/cross path) and the C ASan test harness
//! (`runtime/tests.sh`).

use std::path::Path;
use std::process::Command;

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR");

    let vendor_dir = Path::new(&manifest_dir).join("runtime").join("vendor");
    let sqlite_c = vendor_dir.join("sqlite3.c");
    let sqlite_h = vendor_dir.join("sqlite3.h");

    // Only rebuild when the vendored source, the header, or this script change.
    println!("cargo:rerun-if-changed={}", sqlite_c.display());
    println!("cargo:rerun-if-changed={}", sqlite_h.display());
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=CC");

    let host = std::env::var("HOST").unwrap_or_default();
    let target = std::env::var("TARGET").unwrap_or_default();
    let is_windows = target.contains("windows");
    let is_msvc = target.contains("msvc");

    // `cc` (the POSIX default) is not present on Windows. Default to clang,
    // which is preinstalled and on PATH on the GitHub windows runners and, on a
    // windows-msvc host, emits COFF objects that link.exe consumes. An explicit
    // $CC override still wins on every platform.
    let cc = std::env::var("CC").unwrap_or_else(|_| {
        if is_windows {
            "clang".to_string()
        } else {
            "cc".to_string()
        }
    });

    let obj_path = Path::new(&out_dir).join("sqlite3.o");
    // rustc's `static=turbosqlite` resolves to `turbosqlite.lib` on msvc and
    // `libturbosqlite.a` everywhere else, so the archive must be named to match.
    let lib_path = if is_msvc {
        Path::new(&out_dir).join("turbosqlite.lib")
    } else {
        Path::new(&out_dir).join("libturbosqlite.a")
    };

    // Compile flags — the `-D` defines MUST stay in sync with `SQLITE_CFLAGS`
    // in src/lib.rs.
    let mut cflags: Vec<&str> = vec!["-O2"];
    // Position-independent code is required for the Unix AOT/static-link path;
    // it is irrelevant on Windows (all code is relocatable) and clang warns
    // "argument unused" if it is passed there.
    if !is_windows {
        cflags.push("-fPIC");
    }
    cflags.extend_from_slice(&[
        "-DSQLITE_THREADSAFE=1",
        "-DSQLITE_OMIT_LOAD_EXTENSION",
        "-DSQLITE_OMIT_DEPRECATED",
        "-DSQLITE_DQS=0",
        "-DSQLITE_DEFAULT_MEMSTATUS=0",
        "-DSQLITE_OMIT_SHARED_CACHE",
    ]);

    let mut compile = Command::new(&cc);
    compile.args(&cflags);

    // Cross-compilation: a bare `cc` compiles for the HOST arch, but cargo
    // may be building a different TARGET (the release matrix cross-builds
    // x86_64-apple-darwin on arm64 runners). A wrong-arch object is silently
    // ignored by the linker and surfaces as undefined `_sqlite3_*` symbols.
    if !target.is_empty() && target != host {
        if target.contains("apple-darwin") {
            let arch = if target.starts_with("aarch64") {
                "arm64"
            } else {
                "x86_64"
            };
            compile.arg("-arch").arg(arch);
        } else {
            // clang accepts a bare triple via --target; gcc-based cross
            // setups are expected to provide a target-specific $CC instead.
            compile.arg(format!("--target={target}"));
        }
    }

    compile.arg("-c").arg(&sqlite_c).arg("-o").arg(&obj_path);

    let status = compile
        .status()
        .unwrap_or_else(|e| panic!("failed to spawn C compiler '{cc}' for sqlite3.c: {e}"));
    if !status.success() {
        panic!("cc failed compiling vendored sqlite3.c (exit {status})");
    }

    // Archive the object so cargo can link it as a static library. Remove any
    // stale archive first so the archiver never appends to an old member set.
    // The archiver and its argument syntax differ by toolchain:
    //   - Unix / windows-gnu: `ar crs libturbosqlite.a sqlite3.o`
    //   - windows-msvc:       `llvm-lib /OUT:turbosqlite.lib sqlite3.o`
    //     (llvm-lib ships with the clang we default to and produces a COFF
    //     archive that link.exe accepts). Honors an $AR override on either path.
    let _ = std::fs::remove_file(&lib_path);
    let ar_status = if is_msvc {
        let ar = std::env::var("AR").unwrap_or_else(|_| "llvm-lib".to_string());
        Command::new(&ar)
            .arg("/NOLOGO")
            .arg(format!("/OUT:{}", lib_path.display()))
            .arg(&obj_path)
            .status()
            .unwrap_or_else(|e| panic!("failed to spawn '{ar}' to archive sqlite3.o: {e}"))
    } else {
        let ar = std::env::var("AR").unwrap_or_else(|_| "ar".to_string());
        Command::new(&ar)
            .arg("crs")
            .arg(&lib_path)
            .arg(&obj_path)
            .status()
            .unwrap_or_else(|e| panic!("failed to spawn '{ar}' to archive sqlite3.o: {e}"))
    };
    if !ar_status.success() {
        panic!("archiver failed archiving sqlite3.o (exit {ar_status})");
    }

    println!("cargo:rustc-link-search=native={out_dir}");
    println!("cargo:rustc-link-lib=static=turbosqlite");

    // SQLite needs the C math library everywhere and dlopen/pthread on Linux.
    // (macOS provides these via libSystem; Windows provides them via the CRT
    // and kernel32, both of which rustc links automatically.)
    if target.contains("linux") {
        println!("cargo:rustc-link-lib=dylib=dl");
        println!("cargo:rustc-link-lib=dylib=pthread");
        println!("cargo:rustc-link-lib=dylib=m");
    }

    // Expose the object path so src/lib.rs can embed it for the AOT path.
    println!(
        "cargo:rustc-env=TURBO_SQLITE_AOT_OBJECT={}",
        obj_path.display()
    );
}
