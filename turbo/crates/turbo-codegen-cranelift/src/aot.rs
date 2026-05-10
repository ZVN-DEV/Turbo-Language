//! AOT (Ahead-of-Time) and WASM compilation entry points.
//!
//! Contains `aot_compile()` for producing native binaries via Cranelift + cc linker,
//! `wasm_compile()` for producing .wasm files via C transpilation + wasm-ld,
//! and cross-compilation helpers.

use super::*;

pub fn aot_compile(
    ast_module: &turbo_ast::Module,
    output_path: &Path,
    optimize: bool,
    target: Option<&str>,
    link_libs: &[String],
) -> Result<(), CodegenError> {
    let mut flag_builder = settings::builder();
    flag_builder.set("use_colocated_libcalls", "false").unwrap();
    flag_builder.set("is_pic", "true").unwrap(); // Required for AOT linking on macOS
    if optimize {
        flag_builder.set("opt_level", "speed_and_size").unwrap();
        let verifier = if cfg!(debug_assertions) { "true" } else { "false" };
        flag_builder.set("enable_verifier", verifier).unwrap();
        flag_builder.set("enable_alias_analysis", "true").unwrap();
    }

    let isa = if let Some(target_str) = target {
        let triple: target_lexicon::Triple = target_str.parse().map_err(|e| CodegenError {
            code: ErrorCode::E0405,
            message: format!("invalid target triple '{target_str}': {e}"),
        })?;
        cranelift_codegen::isa::lookup(triple)
            .map_err(|e| CodegenError {
                code: ErrorCode::E0405,
                message: format!("unsupported target '{target_str}': {e}"),
            })?
            .finish(settings::Flags::new(flag_builder))
            .map_err(|e| CodegenError {
                code: ErrorCode::E0405,
                message: format!("failed to create ISA for '{target_str}': {e}"),
            })?
    } else {
        let isa_builder = cranelift_native::builder().map_err(|e| CodegenError {
            code: ErrorCode::E0405,
            message: e.to_string(),
        })?;
        isa_builder
            .finish(settings::Flags::new(flag_builder))
            .map_err(|e| CodegenError {
                code: ErrorCode::E0405,
                message: e.to_string(),
            })?
    };

    let ptr_type = isa.pointer_type();

    let obj_builder = ObjectBuilder::new(
        isa,
        "turbo_module",
        cranelift_module::default_libcall_names(),
    )
    .map_err(|e| CodegenError {
        code: ErrorCode::E0405,
        message: e.to_string(),
    })?;

    let mut module = ObjectModule::new(obj_builder);
    compile_module(&mut module, ast_module, ptr_type, Linkage::Export, true)?;

    let product = module.finish();
    let obj_bytes = product.emit().map_err(|e| CodegenError {
        code: ErrorCode::E0404,
        message: format!("failed to emit object: {e}"),
    })?;

    // Write object file and runtime to temp, then link with cc
    let tmp_dir = std::env::temp_dir().join(format!("turbo_aot_{}", std::process::id()));
    std::fs::create_dir_all(&tmp_dir).map_err(|e| CodegenError {
        code: ErrorCode::E0404,
        message: format!("failed to create temp dir: {e}"),
    })?;

    let obj_path = tmp_dir.join("turbo.o");
    let rt_path = tmp_dir.join("turbo_rt.c");

    std::fs::write(&obj_path, &obj_bytes).map_err(|e| CodegenError {
        code: ErrorCode::E0400,
        message: format!("failed to write object file: {e}"),
    })?;
    std::fs::write(&rt_path, RUNTIME_C).map_err(|e| CodegenError {
        code: ErrorCode::E0400,
        message: format!("failed to write runtime: {e}"),
    })?;

    let (cc_cmd, cc_args) = resolve_cross_compiler(target)?;
    let mut cmd = std::process::Command::new(&cc_cmd);
    for arg in &cc_args {
        cmd.arg(arg);
    }
    cmd.arg(&rt_path)
        .arg(&obj_path)
        .arg("-lm")
        .arg("-o")
        .arg(output_path);

    // Linux targets need explicit -lpthread
    if target.is_some_and(|t| t.contains("linux")) {
        cmd.arg("-lpthread");
    }

    // Additional user-specified link libraries (e.g. --link m --link pthread).
    //
    // Track 4 audit (v0.8.0 "Safe Core"): the ONLY user-controlled values
    // formatted into linker args in this function are these `--link` names.
    // Everything else (cc_cmd/cc_args from resolve_cross_compiler, the temp
    // obj/rt paths, hardcoded -lm/-lpthread, the wasm sysroot/target strings
    // further down) is either compiler-internal or derived from validated
    // sources. So we validate each lib name here against a strict allowlist
    // to prevent linker-flag injection (e.g. "m -o /etc/passwd").
    for lib in link_libs {
        if !is_valid_lib_name(lib) {
            return Err(CodegenError {
                code: ErrorCode::E0404,
                message: format!(
                    "invalid library name '{}' in --link; names must match [A-Za-z0-9_.+-]+",
                    lib
                ),
            });
        }
        cmd.arg(format!("-l{}", lib));
    }

    let output = cmd.output().map_err(|e| CodegenError {
        code: ErrorCode::E0404,
        message: format!("failed to run linker '{}': {e}", cc_cmd),
    })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(CodegenError {
            code: ErrorCode::E0404,
            message: format!("linker failed: {stderr}"),
        });
    }

    // Clean up temp directory and all files within
    let _ = std::fs::remove_dir_all(&tmp_dir);

    Ok(())
}

// ── Cross-compilation helpers ──────────────────────────────────────

/// Determine the C compiler + flags for a given target.
fn resolve_cross_compiler(target: Option<&str>) -> Result<(String, Vec<String>), CodegenError> {
    let target = match target {
        None => return Ok(("cc".to_string(), vec![])),
        Some(t) => t,
    };

    let (zig_target, gnu_cc, env_key) = match target {
        t if t.contains("aarch64") && t.contains("linux") => (
            "aarch64-linux-gnu",
            "aarch64-linux-gnu-gcc",
            "TURBO_CC_LINUX_ARM64",
        ),
        t if t.contains("x86_64") && t.contains("linux") => (
            "x86_64-linux-gnu",
            "x86_64-linux-gnu-gcc",
            "TURBO_CC_LINUX_X86",
        ),
        _ => return Ok(("cc".to_string(), vec![])), // native or macOS cross — try system cc
    };

    // 1. Check env var override
    if let Ok(cc) = std::env::var(env_key) {
        if !cc.is_empty() {
            return Ok((cc, vec![]));
        }
    }

    // 2. Check for Zig
    if which_exists("zig") {
        return Ok((
            "zig".to_string(),
            vec!["cc".to_string(), format!("--target={zig_target}")],
        ));
    }

    // 3. Check for GNU cross-compiler
    if which_exists(gnu_cc) {
        return Ok((gnu_cc.to_string(), vec![]));
    }

    // 4. Error with helpful message
    Err(CodegenError {
        code: ErrorCode::E0404,
        message: format!(
            "no cross-compiler found for target '{target}'\n\n  \
             Install one of:\n    \
             - Zig (recommended): brew install zig\n    \
             - GCC cross-compiler: brew install {gnu_cc}\n\n  \
             Or set {env_key} to a cross-compiler binary."
        ),
    })
}

fn which_exists(cmd: &str) -> bool {
    std::process::Command::new("which")
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

// ── WASM compilation ────────────────────────────────────────────────

/// Discover `wasm-ld` linker binary.
fn find_wasm_ld() -> Result<String, CodegenError> {
    // 1. WASM_LD env var
    if let Ok(p) = std::env::var("WASM_LD") {
        if std::path::Path::new(&p).exists() {
            return Ok(p);
        }
    }
    // 2. Common Homebrew locations
    for path in &[
        "/opt/homebrew/opt/llvm@18/bin/wasm-ld",
        "/opt/homebrew/opt/llvm/bin/wasm-ld",
        "/usr/local/opt/llvm@18/bin/wasm-ld",
        "/usr/local/opt/llvm/bin/wasm-ld",
    ] {
        if std::path::Path::new(path).exists() {
            return Ok(path.to_string());
        }
    }
    // 3. PATH
    if let Ok(output) = std::process::Command::new("which").arg("wasm-ld").output() {
        if output.status.success() {
            let p = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !p.is_empty() {
                return Ok(p);
            }
        }
    }
    Err(CodegenError {
        code: ErrorCode::E0404,
        message:
            "wasm-ld not found. Install LLVM (e.g. `brew install llvm@18`) or set WASM_LD env var."
                .to_string(),
    })
}

/// Discover a WASM-capable clang binary.
fn find_wasm_clang() -> Result<String, CodegenError> {
    // 1. WASM_CLANG env var
    if let Ok(p) = std::env::var("WASM_CLANG") {
        if std::path::Path::new(&p).exists() {
            return Ok(p);
        }
    }
    // 2. Common Homebrew locations
    for path in &[
        "/opt/homebrew/opt/llvm@18/bin/clang",
        "/opt/homebrew/opt/llvm/bin/clang",
        "/usr/local/opt/llvm@18/bin/clang",
        "/usr/local/opt/llvm/bin/clang",
    ] {
        if std::path::Path::new(path).exists() {
            return Ok(path.to_string());
        }
    }
    Err(CodegenError {
        code: ErrorCode::E0404,
        message: "WASM-capable clang not found. Install LLVM (e.g. `brew install llvm@18`) or set WASM_CLANG env var.".to_string(),
    })
}

/// Discover the WASI sysroot directory.
fn find_wasi_sysroot() -> Result<String, CodegenError> {
    // 1. WASI_SYSROOT env var
    if let Ok(p) = std::env::var("WASI_SYSROOT") {
        if std::path::Path::new(&p).exists() {
            return Ok(p);
        }
    }
    // 2. Common Homebrew locations
    for path in &[
        "/opt/homebrew/opt/wasi-libc/share/wasi-sysroot",
        "/usr/local/opt/wasi-libc/share/wasi-sysroot",
        "/opt/wasi-sdk/share/wasi-sysroot",
    ] {
        if std::path::Path::new(path).exists() {
            return Ok(path.to_string());
        }
    }
    Err(CodegenError {
        code: ErrorCode::E0404,
        message: "WASI sysroot not found. Install wasi-libc (e.g. `brew install wasi-libc`) or set WASI_SYSROOT env var.".to_string(),
    })
}

/// C stub that provides the `__wasm_first_page_end` symbol needed by wasi-libc 32+.
/// wasm-ld from LLVM 18 does not define this symbol automatically.
const WASM_STUB_C: &str = r#"
__attribute__((used, visibility("hidden")))
unsigned char __wasm_first_page_end __attribute__((section(".data")));
"#;

/// Compile a Turbo module to a `.wasm` file targeting WASI.
///
/// Since Cranelift does not support wasm32 as a compilation target,
/// this function translates the AST to C code, compiles each C file
/// to wasm32 object files with clang, and links with wasm-ld.
pub fn wasm_compile(
    ast_module: &turbo_ast::Module,
    output_path: &Path,
    use_wasi: bool,
) -> Result<(), CodegenError> {
    // Discover external tools
    let wasm_ld = find_wasm_ld()?;
    let wasm_clang = find_wasm_clang()?;
    let wasi_sysroot = find_wasi_sysroot()?;

    // Generate C source from the AST
    let c_source = wasm_codegen::generate_c(ast_module);

    // Write C source, runtime, and stub to temp dir
    let tmp_dir = std::env::temp_dir().join(format!("turbo_wasm_{}", std::process::id()));
    std::fs::create_dir_all(&tmp_dir).map_err(|e| CodegenError {
        code: ErrorCode::E0404,
        message: format!("failed to create temp dir: {e}"),
    })?;

    let program_c_path = tmp_dir.join("turbo_program.c");
    let rt_src_path = tmp_dir.join("turbo_rt_wasm.c");
    let stub_c_path = tmp_dir.join("wasm_stub.c");
    let program_o_path = tmp_dir.join("turbo_program.o");
    let rt_o_path = tmp_dir.join("turbo_rt_wasm.o");
    let stub_o_path = tmp_dir.join("wasm_stub.o");

    std::fs::write(&program_c_path, &c_source).map_err(|e| CodegenError {
        code: ErrorCode::E0400,
        message: format!("failed to write generated C source: {e}"),
    })?;
    std::fs::write(&rt_src_path, RUNTIME_WASM_C).map_err(|e| CodegenError {
        code: ErrorCode::E0400,
        message: format!("failed to write wasm runtime source: {e}"),
    })?;
    std::fs::write(&stub_c_path, WASM_STUB_C).map_err(|e| CodegenError {
        code: ErrorCode::E0400,
        message: format!("failed to write wasm stub: {e}"),
    })?;

    let target = "wasm32-wasip1";

    // Helper closure to compile a C file to a wasm32 object
    let compile_c = |src: &std::path::Path, obj: &std::path::Path| -> Result<(), CodegenError> {
        let output = std::process::Command::new(&wasm_clang)
            .arg(format!("--target={target}"))
            .arg(format!("--sysroot={wasi_sysroot}"))
            .arg("-O2")
            .arg("-c")
            .arg(src)
            .arg("-o")
            .arg(obj)
            .output()
            .map_err(|e| CodegenError {
                code: ErrorCode::E0404,
                message: format!("failed to run clang: {e}"),
            })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(CodegenError {
                code: ErrorCode::E0404,
                message: format!(
                    "clang failed compiling {}: {stderr}",
                    src.file_name().unwrap_or_default().to_string_lossy()
                ),
            });
        }
        Ok(())
    };

    // Compile each C file to wasm32 object
    compile_c(&rt_src_path, &rt_o_path)?;
    compile_c(&program_c_path, &program_o_path)?;
    compile_c(&stub_c_path, &stub_o_path)?;

    // Link with wasm-ld
    let wasi_lib_dir = format!("{wasi_sysroot}/lib/wasm32-wasip1");
    let crt1_path = format!("{wasi_lib_dir}/crt1.o");

    let mut cmd = std::process::Command::new(&wasm_ld);

    if use_wasi {
        cmd.arg(&crt1_path);
    }
    cmd.arg(&rt_o_path)
        .arg(&program_o_path)
        .arg(&stub_o_path)
        .arg("-o")
        .arg(output_path);

    if use_wasi {
        cmd.arg(format!("-L{wasi_lib_dir}"))
            .arg("-lc")
            .arg("--entry=_start");
    } else {
        cmd.arg("--no-entry")
            .arg("--export=main")
            .arg("--allow-undefined");
    }

    let link_output = cmd.output().map_err(|e| CodegenError {
        code: ErrorCode::E0404,
        message: format!("failed to run wasm-ld: {e}"),
    })?;

    if !link_output.status.success() {
        let stderr = String::from_utf8_lossy(&link_output.stderr);
        if std::env::var("TURBO_DEBUG").is_ok() {
            eprintln!("debug: generated C source at {}", program_c_path.display());
            eprintln!("debug: temp dir preserved at {}", tmp_dir.display());
        } else {
            let _ = std::fs::remove_dir_all(&tmp_dir);
        }
        return Err(CodegenError {
            code: ErrorCode::E0404,
            message: format!("wasm-ld failed: {stderr}"),
        });
    }

    // Clean up temp directory
    if std::env::var("TURBO_DEBUG").is_ok() {
        eprintln!("debug: temp dir preserved at {}", tmp_dir.display());
    } else {
        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    Ok(())
}

// ── Linker input validation ─────────────────────────────────────────

/// Returns true if `name` is a safe library name to format into a `-l<name>`
/// linker argument. Accepts only ASCII letters, digits, `_`, `.`, `+`, `-`.
/// Rejects the empty string so we never emit a bare `-l`.
///
/// This is a defence-in-depth check against linker-flag injection via the
/// `--link` CLI option. A name like `"m -o /etc/passwd"` would otherwise be
/// spliced straight into the `cc` argv and interpreted as additional flags.
/// The regex `^[A-Za-z0-9_.+-]+$` implicitly rejects any name starting with
/// `-` or `@` (since neither is in the charset), but we keep the allowlist
/// tight rather than blocklisting known-bad prefixes.
fn is_valid_lib_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '+' | '-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_injection_in_link_name() {
        assert!(!is_valid_lib_name(""));
        assert!(!is_valid_lib_name("m -o /tmp/x"));
        assert!(!is_valid_lib_name("@/etc/passwd"));
        assert!(!is_valid_lib_name("../../lib"));
        assert!(!is_valid_lib_name("-Wl,evil"));
    }

    #[test]
    fn accepts_normal_lib_names() {
        assert!(is_valid_lib_name("m"));
        assert!(is_valid_lib_name("ssl"));
        assert!(is_valid_lib_name("c++"));
        assert!(is_valid_lib_name("z.1"));
        assert!(is_valid_lib_name("my_lib-2.3+foo"));
    }
}
