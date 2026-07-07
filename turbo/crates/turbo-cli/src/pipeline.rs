//! The core compile/execute commands: `run`, `check`, `test`, `bench`, and
//! `build`, plus their shared file-collection and size-guard helpers.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use turbo_ast::Item;

use crate::diagnostics::{
    io_reason, lex_error_message, report_error, report_file_error, report_import_error,
    report_warning,
};
use crate::explain::{parse_help, sema_help};
use crate::imports::resolve_imports;
use crate::project::read_project_name;

/// Maximum source file size: 50 MB. Files larger than this are rejected
/// to prevent denial-of-service via memory exhaustion.
pub(crate) const MAX_SOURCE_FILE_SIZE: u64 = 50 * 1024 * 1024;

/// Check that a source file does not exceed the maximum allowed size.
/// Prints an error and exits if the file is too large.
pub(crate) fn check_file_size(path: &std::path::Path) {
    match std::fs::metadata(path) {
        Ok(meta) => {
            if meta.len() > MAX_SOURCE_FILE_SIZE {
                eprintln!(
                    "\x1b[1;31merror\x1b[0m: file `{}` is too large ({:.1} MB, max {} MB)",
                    path.display(),
                    meta.len() as f64 / (1024.0 * 1024.0),
                    MAX_SOURCE_FILE_SIZE / (1024 * 1024),
                );
                std::process::exit(1);
            }
        }
        Err(e) => report_file_error(path, &e),
    }
}

pub(crate) fn run_file(path: &std::path::Path, verbose: bool) {
    check_file_size(path);

    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => report_file_error(path, &e),
    };

    let filename = path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| "<unknown>".to_string());

    // Lex
    let lex_start = std::time::Instant::now();
    let (tokens, lex_errors) = turbo_lexer::tokenize(&source);
    let lex_time = lex_start.elapsed();

    if !lex_errors.is_empty() {
        for span in &lex_errors {
            let snippet = &source[span.clone()];
            let (msg, help) = lex_error_message(snippet);
            report_error(&source, &filename, &msg, span, Some(help), None);
        }
        std::process::exit(1);
    }

    if verbose {
        eprintln!("--- Tokens ({} total, {:.2?}) ---", tokens.len(), lex_time);
        for tok in &tokens {
            eprintln!("  {:?} @ {:?}", tok.value, tok.span);
        }
        eprintln!();
    }

    // Parse
    let parse_start = std::time::Instant::now();
    let (mut module, parse_errors) = turbo_parser::parse(tokens);
    let parse_time = parse_start.elapsed();

    if !parse_errors.is_empty() {
        for err in &parse_errors {
            report_error(
                &source,
                &filename,
                &err.message,
                &err.span,
                parse_help(&err.message).as_deref(),
                Some(err.code),
            );
        }
        std::process::exit(1);
    }

    // Resolve imports
    let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let canonical_self = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let mut loading = HashSet::new();
    loading.insert(canonical_self);
    if let Err(e) = resolve_imports(&mut module, base_dir, &mut loading) {
        report_import_error(&e);
    }

    if module.items.is_empty() {
        report_error(
            &source,
            &filename,
            "no functions defined",
            &(0..source.len().min(1)),
            Some("add a `fn main() { ... }` function to get started"),
            None,
        );
        std::process::exit(1);
    }

    if verbose {
        eprintln!(
            "--- AST ({} items, {:.2?}) ---",
            module.items.len(),
            parse_time
        );
        for item in &module.items {
            eprintln!("  {:#?}", item.node);
        }
        eprintln!();
    }

    // Semantic analysis
    let sema_start = std::time::Instant::now();
    let sema_result = turbo_sema::check(&module);
    let sema_time = sema_start.elapsed();

    if verbose {
        eprintln!(
            "--- Sema ({} errors, {} warnings, {:.2?}) ---",
            sema_result.errors.len(),
            sema_result.warnings.len(),
            sema_time
        );
    }

    // Display warnings (non-fatal)
    for w in &sema_result.warnings {
        report_warning(&source, &filename, &w.message, &w.span, Some(w.code));
    }

    if !sema_result.errors.is_empty() {
        for err in &sema_result.errors {
            let help = sema_help(&err.message);
            report_error(
                &source,
                &filename,
                &err.message,
                &err.span,
                help.as_deref(),
                Some(err.code),
            );
        }
        std::process::exit(1);
    }

    // Compile & run
    let codegen_start = std::time::Instant::now();
    match turbo_codegen_cranelift::jit_run(&module) {
        Ok(()) => {
            if verbose {
                let codegen_time = codegen_start.elapsed();
                eprintln!("\n--- Timing ---");
                eprintln!("  Lex:     {:.2?}", lex_time);
                eprintln!("  Parse:   {:.2?}", parse_time);
                eprintln!("  Sema:    {:.2?}", sema_time);
                eprintln!("  Codegen: {:.2?}", codegen_time);
                eprintln!(
                    "  Total:   {:.2?}",
                    lex_time + parse_time + sema_time + codegen_time
                );
            }
        }
        Err(e) => {
            eprintln!("\x1b[1;31merror\x1b[0m: {e}");
            std::process::exit(1);
        }
    }
}

pub(crate) fn check_file(path: &std::path::Path) {
    check_file_size(path);

    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => report_file_error(path, &e),
    };

    let filename = path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| "<unknown>".to_string());

    // Lex
    let (tokens, lex_errors) = turbo_lexer::tokenize(&source);

    if !lex_errors.is_empty() {
        for span in &lex_errors {
            let snippet = &source[span.clone()];
            let (msg, help) = lex_error_message(snippet);
            report_error(&source, &filename, &msg, span, Some(help), None);
        }
        std::process::exit(1);
    }

    // Parse
    let (mut module, parse_errors) = turbo_parser::parse(tokens);

    if !parse_errors.is_empty() {
        for err in &parse_errors {
            report_error(
                &source,
                &filename,
                &err.message,
                &err.span,
                parse_help(&err.message).as_deref(),
                Some(err.code),
            );
        }
        std::process::exit(1);
    }

    // Resolve imports
    let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let canonical_self = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let mut loading = HashSet::new();
    loading.insert(canonical_self);
    if let Err(e) = resolve_imports(&mut module, base_dir, &mut loading) {
        report_import_error(&e);
    }

    // Semantic analysis
    let sema_result = turbo_sema::check(&module);

    for w in &sema_result.warnings {
        report_warning(&source, &filename, &w.message, &w.span, Some(w.code));
    }

    if !sema_result.errors.is_empty() {
        for err in &sema_result.errors {
            let help = sema_help(&err.message);
            report_error(
                &source,
                &filename,
                &err.message,
                &err.span,
                help.as_deref(),
                Some(err.code),
            );
        }
        std::process::exit(1);
    }

    eprintln!("\x1b[32m\u{2713}\x1b[0m No errors in `{}`", filename);
}

pub(crate) fn test_file(file: Option<PathBuf>) {
    // Collect test files: either a specific file, or all *_test.tb / test_*.tb in tests/
    let files: Vec<PathBuf> = if let Some(f) = file {
        if f.is_dir() {
            collect_test_files(&f)
        } else {
            vec![f]
        }
    } else {
        // Look for tests/ directory
        let tests_dir = Path::new("tests");
        if tests_dir.is_dir() {
            collect_test_files(tests_dir)
        } else {
            eprintln!("\x1b[1;31merror\x1b[0m: no file specified and no `tests/` directory found");
            eprintln!("  Usage: turbolang test <file.tb>");
            std::process::exit(1);
        }
    };

    if files.is_empty() {
        eprintln!("No test files found.");
        std::process::exit(0);
    }

    let turbo_exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("turbolang"));

    // Color the PASS/FAIL tags only when stderr is a terminal, so captured
    // output (CI, the integration harness) stays deterministic and free of
    // ANSI escapes. Mirrors the runtime's `is_terminal()` gating.
    use std::io::IsTerminal;
    let use_color = std::io::stderr().is_terminal();
    let pass_tag = if use_color {
        "\x1b[32mPASS\x1b[0m"
    } else {
        "PASS"
    };
    let fail_tag = if use_color {
        "\x1b[31mFAIL\x1b[0m"
    } else {
        "FAIL"
    };

    let suite_start = std::time::Instant::now();
    let mut total_passed = 0u32;
    let mut total_failed = 0u32;

    // Test files are collected in sorted order (see `collect_test_files`) and
    // each file's tests run in declaration order, so the result listing below
    // is stable across runs.
    for path in &files {
        let source = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => report_file_error(path, &e),
        };

        let filename = path
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| "<unknown>".to_string());

        // Lex
        let (tokens, lex_errors) = turbo_lexer::tokenize(&source);
        if !lex_errors.is_empty() {
            for span in &lex_errors {
                let snippet = &source[span.clone()];
                let (msg, help) = lex_error_message(snippet);
                report_error(&source, &filename, &msg, span, Some(help), None);
            }
            std::process::exit(1);
        }

        // Parse
        let (mut module, parse_errors) = turbo_parser::parse(tokens);
        if !parse_errors.is_empty() {
            for err in &parse_errors {
                report_error(
                    &source,
                    &filename,
                    &err.message,
                    &err.span,
                    parse_help(&err.message).as_deref(),
                    Some(err.code),
                );
            }
            std::process::exit(1);
        }

        // Resolve imports
        let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
        let canonical_self = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let mut loading = HashSet::new();
        loading.insert(canonical_self);
        if let Err(e) = resolve_imports(&mut module, base_dir, &mut loading) {
            report_import_error(&e);
        }

        // Collect @test function names
        let test_names: Vec<String> = module
            .items
            .iter()
            .filter_map(|item| {
                if let Item::Function(f) = &item.node {
                    if f.is_test {
                        Some(f.name.clone())
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect();

        if test_names.is_empty() {
            continue; // No tests in this file
        }

        // Semantic analysis in test mode (main not required)
        let sema_result = turbo_sema::check_test(&module);
        for w in &sema_result.warnings {
            report_warning(&source, &filename, &w.message, &w.span, Some(w.code));
        }
        if !sema_result.errors.is_empty() {
            for err in &sema_result.errors {
                let help = sema_help(&err.message);
                report_error(
                    &source,
                    &filename,
                    &err.message,
                    &err.span,
                    help.as_deref(),
                    Some(err.code),
                );
            }
            std::process::exit(1);
        }

        // Run each test in its own subprocess
        let canonical_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        for name in &test_names {
            let output = std::process::Command::new(&turbo_exe)
                .arg("test-run-fn")
                .arg(&canonical_path)
                .arg("--func")
                .arg(name)
                .output();

            match output {
                Ok(result) => {
                    if result.status.success() {
                        eprintln!("  {pass_tag}  {name}");
                        total_passed += 1;
                    } else {
                        // Print captured stderr (assertion failure messages)
                        let stderr = String::from_utf8_lossy(&result.stderr);
                        for line in stderr.lines() {
                            if !line.is_empty() {
                                eprintln!("        {line}");
                            }
                        }
                        eprintln!("  {fail_tag}  {name}");
                        total_failed += 1;
                    }
                }
                Err(e) => {
                    eprintln!("  {fail_tag}  {name} (failed to spawn: {})", io_reason(&e));
                    total_failed += 1;
                }
            }
        }
    }

    // Summary: counts plus the total wall-clock time the suite took. The time
    // is formatted with Rust's `{:.2}` (not the language's float printer), so
    // it is unaffected by `.tb` float-formatting changes.
    let elapsed = suite_start.elapsed().as_secs_f64();
    eprintln!("{total_passed} passed, {total_failed} failed in {elapsed:.2}s");

    if total_failed > 0 {
        std::process::exit(1);
    }
}

/// Run benchmark files and report timing.
/// If a file is given, benchmark that file. Otherwise, look for bench_*.tb
/// files in a `benchmarks/` directory.
pub(crate) fn bench_file(file: Option<PathBuf>, iterations: u32, quiet: bool) {
    let files: Vec<PathBuf> = if let Some(f) = file {
        if f.is_dir() {
            collect_bench_files(&f)
        } else {
            vec![f]
        }
    } else {
        let bench_dir = Path::new("benchmarks");
        if bench_dir.is_dir() {
            collect_bench_files(bench_dir)
        } else {
            eprintln!(
                "\x1b[1;31merror\x1b[0m: no file specified and no `benchmarks/` directory found"
            );
            eprintln!("  Usage: turbolang bench <file.tb>");
            std::process::exit(1);
        }
    };

    if files.is_empty() {
        eprintln!("No benchmark files found.");
        std::process::exit(0);
    }

    let turbo_exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("turbolang"));

    eprintln!("\x1b[1mTurbo Benchmark Suite\x1b[0m");
    eprintln!("\x1b[90m=====================\x1b[0m");
    eprintln!("\x1b[90mIterations: {iterations}\x1b[0m");
    eprintln!();

    let mut total = 0u32;
    let mut passed = 0u32;

    for path in &files {
        // Read the source once: used to find the benchmark's label (the
        // `@bench` function name) and to surface an expected-output hint.
        let source = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!(
                    "  \x1b[31merror\x1b[0m: could not read `{}`: {}",
                    path.display(),
                    io_reason(&e)
                );
                continue;
            }
        };

        // Label by the `@bench` function name when present, else the file stem.
        let file_stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "<unknown>".to_string());
        let label = bench_label(&source).unwrap_or_else(|| file_stem.clone());

        total += 1;
        eprintln!("\x1b[1;36m--- {label} ---\x1b[0m");

        // Show expected output from comment if present
        let expected: Option<String> = source
            .lines()
            .find(|l| l.starts_with("// Expected output:"))
            .map(|l| {
                l.trim_start_matches("// Expected output:")
                    .trim()
                    .to_string()
            });
        if let Some(ref exp) = expected {
            eprintln!("  \x1b[90mexpected: {exp}\x1b[0m");
        }

        // JIT mode: run N iterations, report median. A benchmark "completes"
        // when it produces a valid timing — i.e. the program actually ran to a
        // clean exit. AOT parity is reported separately and never gates this.
        let mut jit_times = Vec::new();
        let mut jit_output = String::new();
        let mut jit_ok = false;
        // Warm-up: one untimed run to prime the OS page cache before the timed
        // loop, so the recorded median reflects steady-state execution rather
        // than a cold-start outlier. (Kept symmetric with the AOT warm-up below,
        // where it matters far more — see that note.)
        let _ = std::process::Command::new(&turbo_exe)
            .arg("run")
            .arg(path)
            .output();
        for _ in 0..iterations {
            let start = std::time::Instant::now();
            let output = std::process::Command::new(&turbo_exe)
                .arg("run")
                .arg(path)
                .output();
            let elapsed = start.elapsed();
            match output {
                Ok(result) if result.status.success() => {
                    jit_times.push(elapsed);
                    jit_ok = true;
                    if jit_output.is_empty() {
                        jit_output = String::from_utf8_lossy(&result.stdout).trim().to_string();
                    }
                }
                Ok(result) => {
                    // The program failed to compile or run: a real failure, so
                    // there is no valid timing. Report once and stop retrying.
                    eprintln!("  \x1b[31mFAIL\x1b[0m  benchmark did not run");
                    for line in String::from_utf8_lossy(&result.stderr)
                        .lines()
                        .filter(|l| !l.is_empty())
                    {
                        eprintln!("        {line}");
                    }
                    break;
                }
                Err(e) => {
                    eprintln!("  \x1b[31merror\x1b[0m: failed to run JIT: {e}");
                    break;
                }
            }
        }

        if !jit_ok {
            // No valid timing — not counted as completed.
            eprintln!();
            continue;
        }

        // Lead with the timing: it's the thing the user came for.
        jit_times.sort();
        let median = jit_times[jit_times.len() / 2];
        if quiet {
            eprintln!(
                "  \x1b[33mJIT:\x1b[0m  \x1b[90m{:.3}s median ({} runs)\x1b[0m",
                median.as_secs_f64(),
                jit_times.len()
            );
        } else {
            eprintln!(
                "  \x1b[33mJIT:\x1b[0m  {} \x1b[90m({:.3}s median, {} runs)\x1b[0m",
                jit_output,
                median.as_secs_f64(),
                jit_times.len()
            );
        }
        passed += 1;

        // AOT mode: build then run N iterations, report median. The AOT-vs-JIT
        // output comparison is a SEPARATE, non-fatal annotation ("AOT parity"),
        // never the headline pass/fail.
        let tmp_bin = std::env::temp_dir().join(format!("turbo_bench_{file_stem}"));
        let build_result = std::process::Command::new(&turbo_exe)
            .arg("build")
            .arg(path)
            .arg("-o")
            .arg(&tmp_bin)
            .output();

        match build_result {
            Ok(result) if result.status.success() => {
                let mut aot_times = Vec::new();
                let mut aot_output = String::new();
                let mut aot_ok = false;
                // Warm-up: the first launch of a freshly built binary pays a
                // one-time OS cold-start (page-fault-in, and on macOS a Gatekeeper
                // assessment of the new executable) that is NOT representative of
                // the binary's actual run time — it can be several times the
                // steady-state cost. Without discarding it, the AOT median (which
                // at low iteration counts lands on that cold sample) made AOT look
                // ~5x slower than JIT even though the two backends run at parity.
                // This run is intentionally untimed.
                let _ = std::process::Command::new(&tmp_bin).output();
                for _ in 0..iterations {
                    let start = std::time::Instant::now();
                    let output = std::process::Command::new(&tmp_bin).output();
                    let elapsed = start.elapsed();
                    match output {
                        Ok(result) if result.status.success() => {
                            aot_times.push(elapsed);
                            aot_ok = true;
                            if aot_output.is_empty() {
                                aot_output =
                                    String::from_utf8_lossy(&result.stdout).trim().to_string();
                            }
                        }
                        Ok(_) => break,
                        Err(e) => {
                            eprintln!("  \x1b[31merror\x1b[0m: failed to run AOT binary: {e}");
                            break;
                        }
                    }
                }

                if aot_ok {
                    aot_times.sort();
                    let median = aot_times[aot_times.len() / 2];
                    if quiet {
                        eprintln!(
                            "  \x1b[33mAOT (run only):\x1b[0m  \x1b[90m{:.3}s median ({} runs)\x1b[0m",
                            median.as_secs_f64(),
                            aot_times.len()
                        );
                    } else {
                        eprintln!(
                            "  \x1b[33mAOT (run only):\x1b[0m  {} \x1b[90m({:.3}s median, {} runs)\x1b[0m",
                            aot_output,
                            median.as_secs_f64(),
                            aot_times.len()
                        );
                    }

                    if jit_output == aot_output {
                        eprintln!("  \x1b[90mAOT parity:\x1b[0m \x1b[32mok\x1b[0m");
                    } else {
                        eprintln!("  \x1b[90mAOT parity:\x1b[0m \x1b[31moutputs differ\x1b[0m");
                        eprintln!("    JIT: {jit_output}");
                        eprintln!("    AOT: {aot_output}");
                    }
                } else {
                    eprintln!("  \x1b[90mAOT parity: skipped (AOT run failed)\x1b[0m");
                }

                // Cleanup temp binary
                std::fs::remove_file(&tmp_bin).ok();
            }
            _ => {
                eprintln!("  \x1b[90mAOT parity: skipped (build unavailable)\x1b[0m");
            }
        }

        eprintln!();
    }

    eprintln!("\x1b[1mResults: {passed}/{total} benchmarks completed\x1b[0m");
    eprintln!(
        "\x1b[90m(\"completed\" = produced a valid JIT timing; AOT parity is annotated per benchmark)\x1b[0m"
    );
    eprintln!(
        "\x1b[90mnote: AOT figures are execution-only (steady state). The AOT build (a one-time\n      cc compile+link) is performed once, separately, and is NOT included in the\n      median above. JIT figures include code generation, which AOT amortizes away.\x1b[0m"
    );
}

/// Find the name of the first `@bench` function in a source file, if any.
/// Used to label benchmark output by function name instead of file name.
pub(crate) fn bench_label(source: &str) -> Option<String> {
    let (tokens, _lex_errors) = turbo_lexer::tokenize(source);
    let (module, _parse_errors) = turbo_parser::parse(tokens);
    module.items.iter().find_map(|item| match &item.node {
        Item::Function(f) if f.is_bench => Some(f.name.clone()),
        _ => None,
    })
}

/// Collect benchmark files from a directory: files matching bench_*.tb
pub(crate) fn collect_bench_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "tb").unwrap_or(false) {
                let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                if stem.starts_with("bench_") {
                    files.push(path);
                }
            }
        }
    }
    files.sort();
    files
}

/// Internal: compile a file and run a single named function via JIT.
/// Used by `turbolang test` to run each @test in its own subprocess.
pub(crate) fn test_run_fn(path: &std::path::Path, fn_name: &str) {
    check_file_size(path);

    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => report_file_error(path, &e),
    };

    let filename = path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| "<unknown>".to_string());

    // Lex
    let (tokens, lex_errors) = turbo_lexer::tokenize(&source);
    if !lex_errors.is_empty() {
        for span in &lex_errors {
            let snippet = &source[span.clone()];
            let (msg, help) = lex_error_message(snippet);
            report_error(&source, &filename, &msg, span, Some(help), None);
        }
        std::process::exit(1);
    }

    // Parse
    let (mut module, parse_errors) = turbo_parser::parse(tokens);
    if !parse_errors.is_empty() {
        for err in &parse_errors {
            report_error(
                &source,
                &filename,
                &err.message,
                &err.span,
                parse_help(&err.message).as_deref(),
                Some(err.code),
            );
        }
        std::process::exit(1);
    }

    // Resolve imports
    let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let canonical_self = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let mut loading = HashSet::new();
    loading.insert(canonical_self);
    if let Err(e) = resolve_imports(&mut module, base_dir, &mut loading) {
        report_import_error(&e);
    }

    // Semantic analysis in test mode
    let sema_result = turbo_sema::check_test(&module);
    for w in &sema_result.warnings {
        eprintln!("warning: {}", w.message);
    }
    if !sema_result.errors.is_empty() {
        for err in &sema_result.errors {
            eprintln!("error: {}", err.message);
        }
        std::process::exit(1);
    }

    // Compile and run
    match turbo_codegen_cranelift::jit_run_function(&module, fn_name) {
        Ok(()) => {}
        Err(e) => {
            eprintln!("\x1b[1;31merror\x1b[0m: {e}");
            std::process::exit(1);
        }
    }
}

/// Collect test files from a directory: files matching *_test.tb or test_*.tb
pub(crate) fn collect_test_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "tb").unwrap_or(false) {
                files.push(path);
            }
        }
    }
    // Also recurse into subdirectories
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                files.extend(collect_test_files(&path));
            }
        }
    }
    files.sort();
    files
}

/// Map user-friendly target names to target-lexicon triples.
pub(crate) fn resolve_target_triple(target: Option<&str>) -> Option<&str> {
    match target? {
        "linux-arm64" | "linux-aarch64" => Some("aarch64-unknown-linux-gnu"),
        "linux-x86" | "linux-x64" | "linux-x86_64" => Some("x86_64-unknown-linux-gnu"),
        "macos-arm64" => Some("aarch64-apple-darwin"),
        "macos-x86" | "macos-x64" => Some("x86_64-apple-darwin"),
        "wasm" | "wasm32-wasi" | "wasm32" => None, // handled by existing wasm path
        other => Some(other),                      // raw triple passthrough
    }
}

pub(crate) fn build_file(
    path: &std::path::Path,
    output: Option<&std::path::Path>,
    verbose: bool,
    target: Option<&str>,
    link_libs: &[String],
) {
    check_file_size(path);

    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => report_file_error(path, &e),
    };

    let filename = path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| "<unknown>".to_string());

    let is_wasm = matches!(target, Some("wasm" | "wasm32-wasi" | "wasm32"));

    // Default output: project name from turbo.toml if available, else filename without .tb
    let default_output = if output.is_none() {
        let base = read_project_name().unwrap_or_else(|| {
            path.file_stem()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("a.out"))
        });
        if is_wasm {
            base.with_extension("wasm")
        } else {
            base
        }
    } else {
        path.file_stem()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("a.out"))
    };
    let output_path = output.unwrap_or(&default_output);

    // Lex
    let lex_start = std::time::Instant::now();
    let (tokens, lex_errors) = turbo_lexer::tokenize(&source);
    let lex_time = lex_start.elapsed();

    if !lex_errors.is_empty() {
        for span in &lex_errors {
            let snippet = &source[span.clone()];
            let (msg, help) = lex_error_message(snippet);
            report_error(&source, &filename, &msg, span, Some(help), None);
        }
        std::process::exit(1);
    }

    if verbose {
        eprintln!("--- Tokens ({} total, {:.2?}) ---", tokens.len(), lex_time);
    }

    // Parse
    let parse_start = std::time::Instant::now();
    let (mut module, parse_errors) = turbo_parser::parse(tokens);
    let parse_time = parse_start.elapsed();

    if !parse_errors.is_empty() {
        for err in &parse_errors {
            report_error(
                &source,
                &filename,
                &err.message,
                &err.span,
                parse_help(&err.message).as_deref(),
                Some(err.code),
            );
        }
        std::process::exit(1);
    }

    // Resolve imports
    let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let canonical_self = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let mut loading = HashSet::new();
    loading.insert(canonical_self);
    if let Err(e) = resolve_imports(&mut module, base_dir, &mut loading) {
        report_import_error(&e);
    }

    if module.items.is_empty() {
        report_error(
            &source,
            &filename,
            "no functions defined",
            &(0..source.len().min(1)),
            Some("add a `fn main() { ... }` function to get started"),
            None,
        );
        std::process::exit(1);
    }

    if verbose {
        eprintln!(
            "--- AST ({} items, {:.2?}) ---",
            module.items.len(),
            parse_time
        );
    }

    // Semantic analysis
    let sema_start = std::time::Instant::now();
    let sema_result = turbo_sema::check(&module);
    let sema_time = sema_start.elapsed();

    for w in &sema_result.warnings {
        report_warning(&source, &filename, &w.message, &w.span, Some(w.code));
    }

    if !sema_result.errors.is_empty() {
        for err in &sema_result.errors {
            let help = sema_help(&err.message);
            report_error(
                &source,
                &filename,
                &err.message,
                &err.span,
                help.as_deref(),
                Some(err.code),
            );
        }
        std::process::exit(1);
    }

    // Compile
    let codegen_start = std::time::Instant::now();
    let (codegen_result, backend_name): (Result<(), String>, &str) = if is_wasm {
        let use_wasi = !matches!(target, Some("wasm32"));
        let r = turbo_codegen_cranelift::wasm_compile(&module, output_path, use_wasi)
            .map_err(|e| e.to_string());
        (r, "Cranelift/WASM")
    } else {
        let cross_target = resolve_target_triple(target);
        let r = turbo_codegen_cranelift::aot_compile(
            &module,
            output_path,
            true,
            cross_target,
            link_libs,
        )
        .map_err(|e| e.to_string());
        (r, "Cranelift")
    };
    match codegen_result {
        Ok(()) => {
            let codegen_time = codegen_start.elapsed();
            if let Some(t) = target {
                eprintln!(
                    "\x1b[32m\u{2713}\x1b[0m Compiled to {} ({}, target: {})",
                    output_path.display(),
                    backend_name,
                    t
                );
            } else {
                eprintln!(
                    "\x1b[32m\u{2713}\x1b[0m Compiled to {} ({})",
                    output_path.display(),
                    backend_name
                );
            }
            if verbose {
                eprintln!("\n--- Timing ---");
                eprintln!("  Lex:     {:.2?}", lex_time);
                eprintln!("  Parse:   {:.2?}", parse_time);
                eprintln!("  Sema:    {:.2?}", sema_time);
                eprintln!("  Codegen: {:.2?}", codegen_time);
                eprintln!(
                    "  Total:   {:.2?}",
                    lex_time + parse_time + sema_time + codegen_time
                );
            }
        }
        Err(e) => {
            eprintln!("\x1b[1;31merror\x1b[0m: {e}");
            std::process::exit(1);
        }
    }
}
