//! Codegen fuzz harness for the Turbo compiler.
//!
//! This binary has three modes. They share one deterministic, seeded
//! program generator, so any finding is reproducible from its seed alone.
//!
//! ## 1. Smoke mode (default — unchanged, CI-gating)
//!
//! ```text
//! cargo run --release --manifest-path turbo/fuzz/Cargo.toml --bin codegen-fuzz
//! TURBO_FUZZ_ITERS=1000 cargo run --release ... --bin codegen-fuzz
//! cargo run --release ... --bin codegen-fuzz -- 500    # bare positional N
//! ```
//!
//! Each iteration lexes → parses → runs sema, and if sema is clean, attempts
//! AOT object emission via Cranelift (no execution). The whole pipeline is
//! wrapped in `catch_unwind` so a single ICE doesn't kill the run. This is the
//! mode wired into `ci.yml`'s `fuzz-smoke` job and `nightly.yml`; its CLI
//! contract (no args + `TURBO_FUZZ_ITERS`, or a bare positional count) is
//! preserved exactly. Default: 200 iterations.
//!
//! **Gap this leaves:** smoke mode only *compiles* — it never *executes* the
//! generated code, so a miscompile (valid program, wrong output) is invisible,
//! and there is no cross-backend checking. The two modes below close that gap.
//!
//! ## 2. Execute mode
//!
//! ```text
//! cargo run --release ... --bin codegen-fuzz -- --execute 50
//! ```
//!
//! Generates *guaranteed-terminating* programs (bounded loops / recursion /
//! array folds), keeps the sema-clean ones, and actually **runs** each via
//! `turbolang run` (which drives `jit_run()` internally). A non-zero exit,
//! a fatal signal (segfault/abort), or a wall-clock timeout is a finding.
//!
//! ## 3. Differential mode (JIT vs AOT)
//!
//! ```text
//! cargo run --release ... --bin codegen-fuzz -- --diff 20
//! ```
//!
//! For each generated sema-clean program: run it via JIT (`turbolang run`)
//! and via AOT (`turbolang build` + execute the native binary), then compare
//! stdout byte-for-byte after ANSI stripping — mirroring the semantics of
//! `turbo/tests/parity/run_parity.sh`. Any stdout divergence, or a
//! backend-specific crash, is a miscompile finding.
//!
//! ## Why subprocess isolation (not in-process `jit_run`)
//!
//! Execute and diff modes shell out to the `turbolang` CLI binary rather than
//! calling `jit_run()` in-process. Three reasons:
//!   * **stdout capture** — the runtime prints via C `rt_print_*` straight to
//!     fd 1; capturing that in-process needs fragile, non-thread-safe `dup2`
//!     redirection. A child process pipe is trivial and robust.
//!   * **crash isolation** — a real miscompile can `SIGSEGV`/`SIGABRT`, which
//!     `catch_unwind` does *not* catch. Only a process boundary keeps the
//!     fuzzer alive to report the finding.
//!   * **timeouts** — you cannot safely kill a thread running native JIT code;
//!     you *can* kill a child process. Each run is guarded by a watchdog that
//!     kills the child past a deadline and flags it as a hang.
//!
//! The cost is subprocess-per-case latency, so execute/diff iteration counts
//! default low (50 / 20) and are configurable. The `turbolang` binary is found
//! via `$TURBOLANG_BIN`, else `../target/{release,debug}/turbolang` relative to
//! this crate (same discovery order as `run_parity.sh`).
//!
//! ## Corpus
//!
//! On any crash / divergence / hang, the offending program + seed + a detail
//! report are written to `turbo/fuzz/corpus/findings/` (git-ignored) for repro.
//! A small checked-in seed corpus lives in `turbo/fuzz/corpus/seed/`.

use std::env;
use std::fs;
use std::io::Read;
use std::panic;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--execute") | Some("execute") => {
            let iters = parse_mode_iters(args.get(1), 50);
            run_execute_mode(iters);
        }
        Some("--diff") | Some("diff") => {
            let iters = parse_mode_iters(args.get(1), 20);
            run_diff_mode(iters);
        }
        Some("--help") | Some("-h") => print_usage(),
        // A bare positional integer selects smoke mode with that iter count
        // (parity with the frontend `turbo-fuzz` harness). Anything else is a
        // usage error.
        Some(other) => match other.parse::<usize>() {
            Ok(n) => run_smoke_mode(n),
            Err(_) => {
                eprintln!("codegen-fuzz: unknown argument {other:?}");
                print_usage();
                std::process::exit(2);
            }
        },
        None => {
            // Legacy contract: no args → TURBO_FUZZ_ITERS (default 200).
            let iters = env::var("TURBO_FUZZ_ITERS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(200);
            run_smoke_mode(iters);
        }
    }
}

fn print_usage() {
    eprintln!(
        "codegen-fuzz — Turbo codegen fuzzer\n\
         \n\
         USAGE:\n\
         \x20 codegen-fuzz [N]            smoke: compile-only (default, TURBO_FUZZ_ITERS or 200)\n\
         \x20 codegen-fuzz --execute [N]  execute generated programs via `turbolang run` (jit_run)\n\
         \x20 codegen-fuzz --diff [N]     differential JIT vs AOT stdout comparison\n\
         \n\
         N overrides the iteration count; falls back to TURBO_FUZZ_ITERS, then a\n\
         mode default (smoke 200, execute 50, diff 20).\n\
         \n\
         ENV:\n\
         \x20 TURBOLANG_BIN               path to the turbolang binary (execute/diff modes)\n\
         \x20 TURBO_FUZZ_EXEC_TIMEOUT_MS  per-run wall-clock timeout (default 5000)"
    );
}

/// Resolve an iteration count for execute/diff modes: explicit positional arg
/// wins, then `TURBO_FUZZ_ITERS`, then the mode default.
fn parse_mode_iters(arg: Option<&String>, default: usize) -> usize {
    if let Some(a) = arg {
        if let Ok(n) = a.parse::<usize>() {
            return n;
        }
        eprintln!("codegen-fuzz: ignoring non-integer count {a:?}");
    }
    env::var("TURBO_FUZZ_ITERS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

// ---------------------------------------------------------------------------
// Smoke mode (default) — compile-only, CI-gating. Behavior unchanged.
// ---------------------------------------------------------------------------

fn run_smoke_mode(iters: usize) {
    println!("== codegen-fuzz: {iters} iterations ==");

    // Silence the panic hook so accidental codegen ICEs don't pollute
    // CI logs with backtraces — we still catch them via catch_unwind.
    let prev_hook = panic::take_hook();
    panic::set_hook(Box::new(|_info| {}));

    // Phase 0 (BL-1): deterministic codegen-robustness corpus. These programs
    // deliberately *bypass sema* and feed the backend unit-typed values in the
    // positions that used to `?.unwrap()` a compiled subexpression. Without the
    // fix each panics the process (exit 101); with it codegen returns a clean
    // `CodegenError`. Any panic here is a regression of the retired panic class.
    let corpus_crashes = run_robustness_corpus();

    let crashes = AtomicUsize::new(corpus_crashes);
    let sema_clean = AtomicUsize::new(0);
    let codegen_attempted = AtomicUsize::new(0);

    let mut crash_seeds: Vec<u64> = Vec::new();

    for seed in 0..iters as u64 {
        let input = generate_program(seed);

        // catch_unwind around the *whole* pipeline so any panic — lex,
        // parse, sema, or codegen — gets reported as a single crash.
        let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
            run_pipeline(&input, &sema_clean, &codegen_attempted)
        }));

        if result.is_err() {
            crashes.fetch_add(1, Ordering::Relaxed);
            crash_seeds.push(seed);
            // Print a short, single-line crash report (no backtrace) so
            // CI failures are easy to triage.
            eprintln!("CRASH at seed {seed}: input = {:?}", truncate(&input, 200));
            record_finding(
                "smoke_crash",
                seed,
                &input,
                "smoke-mode panic during compile",
            );
        }

        if (seed + 1) % 50 == 0 {
            eprintln!("  ... {} / {iters} done", seed + 1);
        }
    }

    panic::set_hook(prev_hook);

    let total_crashes = crashes.load(Ordering::Relaxed);
    let clean = sema_clean.load(Ordering::Relaxed);
    let cg = codegen_attempted.load(Ordering::Relaxed);

    println!(
        "== codegen-fuzz: {iters} iters, {clean} sema-clean, {cg} codegen attempts, {total_crashes} crashes =="
    );

    if !crash_seeds.is_empty() {
        println!("Crash seeds: {crash_seeds:?}");
    }
    if total_crashes > 0 {
        std::process::exit(1);
    }
}

/// One pipeline pass. Errors at any stage are fine; only panics matter
/// (and panics are caught by the outer `catch_unwind`).
fn run_pipeline(input: &str, sema_clean: &AtomicUsize, codegen_attempted: &AtomicUsize) {
    let (tokens, _lex_errors) = turbo_lexer::tokenize(input);
    let (module, parse_errors) = turbo_parser::parse(tokens);
    if !parse_errors.is_empty() {
        return;
    }
    let sema_result = turbo_sema::check(&module);
    if !sema_result.errors.is_empty() {
        return;
    }
    sema_clean.fetch_add(1, Ordering::Relaxed);

    // We have a sema-clean module. Try to AOT-compile it to a tmp file
    // (no execution, no linking we can avoid — `aot_compile` invokes cc
    // internally to produce a binary, so we point it at a unique tmp
    // path and ignore the result).
    codegen_attempted.fetch_add(1, Ordering::Relaxed);

    let tmp_dir = env::temp_dir();
    let out_path: PathBuf = tmp_dir.join(format!("turbo_codegen_fuzz_{}", std::process::id()));
    // Errors here are fine — we only care about panics. Linker failures
    // on garbage are expected.
    let _ = turbo_codegen_cranelift::aot_compile(&module, &out_path, false, None, &[]);
    let _ = std::fs::remove_file(&out_path);
}

/// Truncate a string for crash reports.
fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...[{} bytes total]", &s[..max], s.len())
    }
}

// ---------------------------------------------------------------------------
// BL-1 codegen-robustness corpus.
//
// Every program below is *parseable* but puts a unit-typed (`print(...)` →
// void) value where codegen must read a real value — exactly the positions
// that used to `?.unwrap()` the `Option` returned by compiling a subexpression
// (builtin args, control-flow heads, short-circuit operands, method receiver,
// enum data args, UFCS first arg). Sema rejects them in the real pipeline, so
// here we deliberately run them *without sema* to make the defensive
// `ok_or_else` paths execute. A clean `Err` is success; a panic is a
// regression of the retired panic class.
// ---------------------------------------------------------------------------

const CODEGEN_ROBUSTNESS_CORPUS: &[&str] = &[
    // builtin first-argument position
    "fn main() { let v = len(print(0)) }",
    "fn main() { let v = abs(print(0)) }",
    "fn main() { let v = sqrt(print(0)) }",
    "fn main() { let v = upper(print(0)) }",
    "fn main() { let v = lower(print(0)) }",
    "fn main() { let v = trim(print(0)) }",
    "fn main() { let v = split(print(0), \",\") }",
    "fn main() { let v = char_at(print(0), 0) }",
    "fn main() { let v = repeat(print(0), 2) }",
    "fn main() { let v = substring(print(0), 0, 1) }",
    "fn main() { let v = pad_left(print(0), 4, \"x\") }",
    "fn main() { let v = str_to_int(print(0)) }",
    "fn main() { let v = to_str(print(0)) }",
    "fn main() { let v = type_of(print(0)) }",
    "fn main() { let v = clone(print(0)) }",
    "fn main() { let v = sort(print(0)) }",
    "fn main() { let v = reverse(print(0)) }",
    "fn main() { let v = slice(print(0), 0, 1) }",
    "fn main() { let v = array_contains(print(0), 1) }",
    "fn main() { let v = hashmap_get(print(0), \"k\") }",
    "fn main() { let v = hashmap_has(print(0), \"k\") }",
    "fn main() { let v = hashmap_keys(print(0)) }",
    "fn main() { let v = read_file(print(0)) }",
    "fn main() { let v = file_exists(print(0)) }",
    "fn main() { let v = path_dir(print(0)) }",
    "fn main() { let v = http_get(print(0)) }",
    "fn main() { let v = json_get(print(0), \"k\") }",
    "fn main() { let v = to_json(print(0)) }",
    "fn main() { exit(print(0)) }",
    "fn main() { sleep(print(0)) }",
    "fn main() { panic(print(0)) }",
    "fn main() { assert(print(0)) }",
    // builtin non-first-argument position
    "fn main() { let v = min(1, print(0)) }",
    "fn main() { let v = max(1, print(0)) }",
    "fn main() { let v = pow(2, print(0)) }",
    "fn main() { let v = push([1], print(0)) }",
    "fn main() { let v = replace(\"a\", print(0), \"b\") }",
    "fn main() { let v = substring(\"abc\", print(0), 2) }",
    "fn main() { let v = substring(\"abc\", 0, print(0)) }",
    "fn main() { let v = join([\"a\"], print(0)) }",
    "fn main() { write_file(\"p\", print(0)) }",
    "fn main() { let v = http_post(\"u\", print(0)) }",
    "fn main() { assert_eq(1, print(0)) }",
    // closure-taking builtins (first arg is the unit value)
    "fn main() { let v = map(print(0), |x: int| -> int { x }) }",
    "fn main() { let v = filter(print(0), |x: int| -> bool { true }) }",
    "fn main() { let v = reduce(print(0), 0, |a: int, b: int| -> int { a }) }",
    // control-flow heads
    "fn main() { if print(0) { print(1) } }",
    "fn main() { while print(0) { print(1) } }",
    "fn main() { for x in print(0) { print(x) } }",
    "fn main() { match print(0) { _ => print(1) } }",
    // expr.rs sites: short-circuit operands, method receiver, enum data arg,
    // UFCS first arg
    "fn main() { let b = print(0) && true }",
    "fn main() { let b = true || print(0) }",
    "fn main() { let x = print(0).foo() }",
    "fn main() { let x = notafn(print(0)) }",
    "type Shape { Circle(i64) }\nfn main() { let s = Shape.Circle(print(0)) }",
];

/// Run the deterministic BL-1 codegen-robustness corpus (sema bypassed).
/// Returns the number of panics (regressions); 0 means the panic class stays
/// retired.
fn run_robustness_corpus() -> usize {
    let mut crashes = 0usize;
    let mut reached = 0usize;
    let tmp_dir = env::temp_dir();

    for (i, src) in CODEGEN_ROBUSTNESS_CORPUS.iter().enumerate() {
        let (tokens, _lex_errors) = turbo_lexer::tokenize(src);
        let (module, parse_errors) = turbo_parser::parse(tokens);
        if !parse_errors.is_empty() {
            eprintln!(
                "  bl1-corpus[{i}] did not parse (skipped): {:?}",
                truncate(src, 80)
            );
            continue;
        }
        reached += 1;

        let out_path: PathBuf =
            tmp_dir.join(format!("turbo_bl1_corpus_{}_{i}", std::process::id()));
        // Sema is deliberately NOT run: codegen must face the unit value
        // directly. An `Err` is the expected, healthy outcome; only a panic
        // (the pre-BL-1 `?.unwrap()` behavior) counts as a crash.
        let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
            let _ = turbo_codegen_cranelift::aot_compile(&module, &out_path, false, None, &[]);
        }));
        let _ = std::fs::remove_file(&out_path);

        if result.is_err() {
            crashes += 1;
            eprintln!("BL1-CORPUS CRASH at index {i}: {:?}", truncate(src, 120));
        }
    }

    println!(
        "== bl1-corpus: {} programs, {reached} reached codegen, {crashes} crashes ==",
        CODEGEN_ROBUSTNESS_CORPUS.len()
    );
    crashes
}

// ---------------------------------------------------------------------------
// Input generation — small mutator over curated valid programs.
//
// We deliberately bias toward sema-clean inputs so the codegen path
// actually gets exercised. The frontend harness (`turbo-fuzz`) already
// covers totally random / adversarial inputs.
// ---------------------------------------------------------------------------

const SEED_PROGRAMS: &[&str] = &[
    "fn main() {\n    let x = 42\n    print(x)\n}",
    "fn add(a: i64, b: i64) -> i64 {\n    return a + b\n}\nfn main() {\n    print(add(1, 2))\n}",
    "fn fib(n: i64) -> i64 {\n    if n <= 1 { return n }\n    return fib(n - 1) + fib(n - 2)\n}\nfn main() { print(fib(10)) }",
    "fn main() {\n    let mut i = 0\n    while i < 5 {\n        print(i)\n        i += 1\n    }\n}",
    "fn main() {\n    let arr = [1, 2, 3, 4, 5]\n    for x in arr { print(x) }\n}",
    "struct Point { x: i64, y: i64 }\nfn main() {\n    let p = Point { x: 1, y: 2 }\n    print(p.x)\n}",
    "fn main() {\n    match 2 {\n        1 => print(\"one\")\n        2 => print(\"two\")\n        _ => print(\"other\")\n    }\n}",
    "fn greet(name: str) -> str { return \"Hello, \" + name }\nfn main() { print(greet(\"world\")) }",
    "fn main() {\n    let s = \"interpolation: {1 + 2}\"\n    print(s)\n}",
    "fn main() {\n    let xs = [1, 2, 3]\n    let n = len(xs)\n    print(n)\n}",
];

/// Deterministic xorshift64 PRNG.
struct Rng {
    state: u64,
}

impl Rng {
    fn new(seed: u64) -> Self {
        Self {
            state: seed.wrapping_add(0x9E37_79B9_7F4A_7C15),
        }
    }
    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }
    fn next_usize(&mut self, max: usize) -> usize {
        if max == 0 {
            return 0;
        }
        (self.next_u64() % max as u64) as usize
    }
}

/// Generate a (mostly valid) program from a seed. About half the time
/// we return a clean seed program; the rest of the time we apply 1-5
/// small mutations to stress codegen on near-valid input.
fn generate_program(seed: u64) -> String {
    let mut rng = Rng::new(seed);
    let base = SEED_PROGRAMS[rng.next_usize(SEED_PROGRAMS.len())];

    // 50% clean, 50% mutated
    if seed % 2 == 0 {
        return base.to_string();
    }

    let mut bytes: Vec<u8> = base.bytes().collect();
    let mutations = rng.next_usize(5) + 1;
    for _ in 0..mutations {
        if bytes.is_empty() {
            break;
        }
        match rng.next_usize(4) {
            0 => {
                // Delete a byte.
                let idx = rng.next_usize(bytes.len());
                bytes.remove(idx);
            }
            1 => {
                // Insert a printable ASCII byte.
                let idx = rng.next_usize(bytes.len() + 1);
                let b = 0x20u8 + (rng.next_u64() % 95) as u8;
                bytes.insert(idx, b);
            }
            2 => {
                // Replace a byte.
                let idx = rng.next_usize(bytes.len());
                bytes[idx] = 0x20u8 + (rng.next_u64() % 95) as u8;
            }
            _ => {
                // Swap two bytes.
                if bytes.len() >= 2 {
                    let a = rng.next_usize(bytes.len());
                    let b = rng.next_usize(bytes.len());
                    bytes.swap(a, b);
                }
            }
        }
    }

    String::from_utf8_lossy(&bytes).into_owned()
}

// ===========================================================================
// Execute mode — actually RUN generated programs and catch miscompiles.
// ===========================================================================

fn run_execute_mode(iters: usize) {
    println!("== codegen-fuzz --execute: {iters} iterations ==");
    let turbolang = match find_turbolang() {
        Some(p) => p,
        None => {
            eprintln!(
                "error: could not find the `turbolang` binary.\n\
                 Build it first:  cargo build --release --manifest-path turbo/Cargo.toml\n\
                 or set TURBOLANG_BIN=/path/to/turbolang"
            );
            std::process::exit(2);
        }
    };
    println!("   using turbolang: {}", turbolang.display());
    let timeout = exec_timeout();

    let mut ran = 0usize;
    let mut skipped_sema = 0usize;
    let mut findings: Vec<u64> = Vec::new();

    for seed in 0..iters as u64 {
        let src = gen_exec_program(seed);
        if !is_sema_clean(&src) {
            skipped_sema += 1;
            continue;
        }

        let tb = match write_program(&src, seed) {
            Some(p) => p,
            None => continue,
        };
        let outcome = run_with_timeout(Command::new(&turbolang).arg("run").arg(&tb), timeout);
        let _ = fs::remove_file(&tb);

        match outcome {
            Ok(o) if o.ok() => ran += 1,
            Ok(o) => {
                ran += 1;
                let detail = format!(
                    "execute-mode failure\nseed = {seed}\n{}\n\n--- program ---\n{src}\n",
                    o.describe("jit-run")
                );
                let path = record_finding("execute_fail", seed, &src, &detail);
                findings.push(seed);
                eprintln!(
                    "FINDING (execute) seed {seed}: {} {}",
                    o.describe("jit-run"),
                    path.map(|p| format!("[{}]", p.display()))
                        .unwrap_or_default()
                );
            }
            Err(e) => eprintln!("  seed {seed}: could not spawn turbolang: {e}"),
        }

        if (seed + 1) % 25 == 0 {
            eprintln!(
                "  ... {} / {iters} done ({ran} ran, {skipped_sema} sema-skipped)",
                seed + 1
            );
        }
    }

    println!(
        "== codegen-fuzz --execute: {iters} iters, {ran} executed, {skipped_sema} sema-skipped, {} findings ==",
        findings.len()
    );
    if !findings.is_empty() {
        println!("Finding seeds: {findings:?}");
        std::process::exit(1);
    }
}

// ===========================================================================
// Differential mode — JIT vs AOT stdout parity on generated programs.
// ===========================================================================

fn run_diff_mode(iters: usize) {
    println!("== codegen-fuzz --diff (JIT vs AOT): {iters} iterations ==");
    let turbolang = match find_turbolang() {
        Some(p) => p,
        None => {
            eprintln!(
                "error: could not find the `turbolang` binary.\n\
                 Build it first:  cargo build --release --manifest-path turbo/Cargo.toml\n\
                 or set TURBOLANG_BIN=/path/to/turbolang"
            );
            std::process::exit(2);
        }
    };
    println!("   using turbolang: {}", turbolang.display());
    let timeout = exec_timeout();

    let mut compared = 0usize;
    let mut skipped_sema = 0usize;
    let mut findings: Vec<u64> = Vec::new();

    for seed in 0..iters as u64 {
        let src = gen_exec_program(seed);
        if !is_sema_clean(&src) {
            skipped_sema += 1;
            continue;
        }

        let tb = match write_program(&src, seed) {
            Some(p) => p,
            None => continue,
        };
        let bin = tb.with_extension("bin");

        // 1. JIT
        let jit = run_with_timeout(Command::new(&turbolang).arg("run").arg(&tb), timeout);
        // 2. AOT build
        let build = run_with_timeout(
            Command::new(&turbolang)
                .arg("build")
                .arg(&tb)
                .arg("-o")
                .arg(&bin),
            timeout,
        );
        // 3. AOT run
        let aot = if matches!(&build, Ok(b) if b.ok()) {
            Some(run_with_timeout(&mut Command::new(&bin), timeout))
        } else {
            None
        };

        let divergence = classify_diff(jit.as_ref(), build.as_ref(), aot.as_ref());
        compared += 1;

        let _ = fs::remove_file(&tb);
        let _ = fs::remove_file(&bin);

        if let Some(reason) = divergence {
            let detail = format!(
                "differential JIT vs AOT divergence\nseed = {seed}\n{reason}\n\n--- program ---\n{src}\n"
            );
            let path = record_finding("diff", seed, &src, &detail);
            findings.push(seed);
            eprintln!(
                "FINDING (diff) seed {seed}: {reason} {}",
                path.map(|p| format!("[{}]", p.display()))
                    .unwrap_or_default()
            );
        }

        if (seed + 1) % 10 == 0 {
            eprintln!(
                "  ... {} / {iters} done ({compared} compared, {skipped_sema} sema-skipped)",
                seed + 1
            );
        }
    }

    println!(
        "== codegen-fuzz --diff: {iters} iters, {compared} compared, {skipped_sema} sema-skipped, {} divergences ==",
        findings.len()
    );
    if !findings.is_empty() {
        println!("Divergence seeds: {findings:?}");
        std::process::exit(1);
    }
}

/// Compare JIT and AOT results, mirroring `run_parity.sh` success semantics:
/// both must run cleanly (exit 0) and produce byte-identical stdout after ANSI
/// stripping. Returns `Some(reason)` on divergence, `None` on parity.
fn classify_diff(
    jit: Result<&RunOutcome, &std::io::Error>,
    build: Result<&RunOutcome, &std::io::Error>,
    aot: Option<&Result<RunOutcome, std::io::Error>>,
) -> Option<String> {
    let jit = match jit {
        Ok(o) => o,
        Err(e) => return Some(format!("could not spawn JIT run: {e}")),
    };
    let build = match build {
        Ok(o) => o,
        Err(e) => return Some(format!("could not spawn AOT build: {e}")),
    };

    // A miscompile can present as one backend refusing to build/run what the
    // other happily executes. Treat that asymmetry as a divergence.
    if !jit.ok() && !build.ok() {
        // Both backends reject it the same way — not a divergence.
        return None;
    }
    if !build.ok() {
        return Some(format!(
            "JIT {} but AOT build failed: {}",
            jit.describe("jit"),
            build.describe("build")
        ));
    }
    let aot = match aot {
        Some(Ok(o)) => o,
        Some(Err(e)) => return Some(format!("could not spawn AOT binary: {e}")),
        None => return Some("AOT build succeeded but was not run".to_string()),
    };

    if !jit.ok() || !aot.ok() {
        return Some(format!(
            "backend crash asymmetry: JIT {}, AOT {}",
            jit.describe("jit"),
            aot.describe("aot")
        ));
    }

    let jit_out = strip_ansi(&jit.stdout);
    let aot_out = strip_ansi(&aot.stdout);
    if jit_out != aot_out {
        return Some(format!(
            "stdout differs:\n  JIT: {:?}\n  AOT: {:?}",
            truncate(&String::from_utf8_lossy(&jit_out), 300),
            truncate(&String::from_utf8_lossy(&aot_out), 300),
        ));
    }
    None
}

// ---------------------------------------------------------------------------
// Subprocess runner with a watchdog timeout.
// ---------------------------------------------------------------------------

/// Outcome of running a child process to completion (or killing it on timeout).
struct RunOutcome {
    /// Exit code, if the process exited normally.
    code: Option<i32>,
    /// Terminating signal number, if killed by a signal (Unix).
    signal: Option<i32>,
    /// True if the watchdog killed it for exceeding the deadline.
    timed_out: bool,
    stdout: Vec<u8>,
    #[allow(dead_code)]
    stderr: Vec<u8>,
}

impl RunOutcome {
    /// A "clean" run: exited 0, no signal, no timeout.
    fn ok(&self) -> bool {
        !self.timed_out && self.signal.is_none() && self.code == Some(0)
    }

    fn describe(&self, label: &str) -> String {
        if self.timed_out {
            format!("{label} TIMED OUT")
        } else if let Some(sig) = self.signal {
            format!("{label} killed by signal {sig}")
        } else {
            format!("{label} exit {}", self.code.unwrap_or(-1))
        }
    }
}

/// Run `cmd` to completion, capturing stdout/stderr, killing it if it runs
/// past `timeout`. Reader threads drain the pipes so a chatty child can't
/// deadlock on a full pipe buffer while we poll for the deadline.
fn run_with_timeout(cmd: &mut Command, timeout: Duration) -> std::io::Result<RunOutcome> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn()?;

    let mut out = child.stdout.take().expect("piped stdout");
    let mut err = child.stderr.take().expect("piped stderr");
    let out_h = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = out.read_to_end(&mut b);
        b
    });
    let err_h = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = err.read_to_end(&mut b);
        b
    });

    let deadline = Instant::now() + timeout;
    let mut timed_out = false;
    let status = loop {
        match child.try_wait()? {
            Some(s) => break s,
            None => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    timed_out = true;
                    break child.wait()?;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    };

    let stdout = out_h.join().unwrap_or_default();
    let stderr = err_h.join().unwrap_or_default();

    Ok(RunOutcome {
        code: status.code(),
        signal: signal_of(&status),
        timed_out,
        stdout,
        stderr,
    })
}

#[cfg(unix)]
fn signal_of(status: &std::process::ExitStatus) -> Option<i32> {
    use std::os::unix::process::ExitStatusExt;
    status.signal()
}

#[cfg(not(unix))]
fn signal_of(_status: &std::process::ExitStatus) -> Option<i32> {
    None
}

/// Per-run wall-clock timeout (env `TURBO_FUZZ_EXEC_TIMEOUT_MS`, default 5s).
fn exec_timeout() -> Duration {
    let ms = env::var("TURBO_FUZZ_EXEC_TIMEOUT_MS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(5000);
    Duration::from_millis(ms)
}

/// Locate the `turbolang` binary: `$TURBOLANG_BIN` first, else
/// `../target/{release,debug}/turbolang` relative to this crate — the same
/// discovery order as `tests/parity/run_parity.sh`.
fn find_turbolang() -> Option<PathBuf> {
    if let Ok(p) = env::var("TURBOLANG_BIN") {
        let pb = PathBuf::from(p);
        if pb.exists() {
            return Some(pb);
        }
    }
    // CARGO_MANIFEST_DIR == turbo/fuzz ; the CLI builds to turbo/target/...
    let target = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .join("target");
    for profile in ["release", "debug"] {
        let cand = target.join(profile).join("turbolang");
        if cand.exists() {
            return Some(cand);
        }
    }
    None
}

/// Strip CSI SGR escape sequences (`ESC [ ... m`) so JIT/AOT stdout compare on
/// semantic content only — matches `strip_ansi` in `run_parity.sh`.
fn strip_ansi(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            // Skip until the final byte of the CSI sequence (a letter).
            let mut j = i + 2;
            while j < bytes.len() && !bytes[j].is_ascii_alphabetic() {
                j += 1;
            }
            i = j + 1; // consume the final letter (e.g. 'm')
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

/// Run lex → parse → sema and report whether the module is sema-clean.
fn is_sema_clean(src: &str) -> bool {
    let (tokens, _lex) = turbo_lexer::tokenize(src);
    let (module, parse_errors) = turbo_parser::parse(tokens);
    if !parse_errors.is_empty() {
        return false;
    }
    turbo_sema::check(&module).errors.is_empty()
}

/// Write a program to a unique temp `.tb` file for subprocess consumption.
fn write_program(src: &str, seed: u64) -> Option<PathBuf> {
    let path = env::temp_dir().join(format!("turbo_fuzz_exec_{}_{seed}.tb", std::process::id()));
    fs::write(&path, src).ok()?;
    Some(path)
}

// ---------------------------------------------------------------------------
// Corpus: persist interesting cases for repro.
// ---------------------------------------------------------------------------

/// Directory for runtime-written findings: `turbo/fuzz/corpus/findings/`.
/// Located relative to the crate so it is cwd-independent.
fn corpus_findings_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("corpus")
        .join("findings")
}

/// Persist an interesting case (crash / divergence / hang): writes the program
/// source and a `.txt` detail report. Returns the path to the `.tb` on success.
fn record_finding(kind: &str, seed: u64, src: &str, detail: &str) -> Option<PathBuf> {
    let dir = corpus_findings_dir();
    fs::create_dir_all(&dir).ok()?;
    let stem = format!("{kind}_seed{seed}");
    let tb = dir.join(format!("{stem}.tb"));
    fs::write(&tb, src).ok()?;
    fs::write(dir.join(format!("{stem}.txt")), detail).ok()?;
    Some(tb)
}

// ---------------------------------------------------------------------------
// Execution-safe program generator.
//
// Unlike `generate_program` (byte-mutations that mostly get rejected by sema),
// this produces *valid, guaranteed-terminating* Turbo programs with
// deterministic output — so JIT and AOT should agree byte-for-byte, and any
// divergence is a genuine miscompile. Every construct is bounded:
//   * arithmetic over small i64 literals (+ - *, no div → no div-by-zero)
//   * `while` loops with a fixed, small trip count
//   * `for` over fixed-size array literals
//   * bounded recursion (fib(n), n ≤ 18)
// ---------------------------------------------------------------------------

fn gen_exec_program(seed: u64) -> String {
    let mut rng = Rng::new(seed ^ 0xF00D_BABE_1234_5678);
    match seed % 4 {
        0 => gen_arith_program(&mut rng),
        1 => gen_loop_program(&mut rng),
        2 => gen_fold_program(&mut rng),
        _ => gen_recursion_program(&mut rng),
    }
}

/// A small arithmetic expression tree over `+ - *` and literals `0..=20`,
/// depth-limited so evaluation is trivial and total.
fn gen_expr(rng: &mut Rng, depth: usize) -> String {
    if depth == 0 || rng.next_usize(3) == 0 {
        return (rng.next_u64() % 21).to_string();
    }
    let op = match rng.next_usize(3) {
        0 => "+",
        1 => "-",
        _ => "*",
    };
    let l = gen_expr(rng, depth - 1);
    let r = gen_expr(rng, depth - 1);
    format!("({l} {op} {r})")
}

fn gen_arith_program(rng: &mut Rng) -> String {
    let a = gen_expr(rng, 3);
    let b = gen_expr(rng, 3);
    format!(
        "fn main() {{\n\
         \x20   let a = {a}\n\
         \x20   let b = {b}\n\
         \x20   print(a)\n\
         \x20   print(b)\n\
         \x20   print(a + b)\n\
         \x20   print(a - b)\n\
         \x20   print(a * b)\n\
         }}\n"
    )
}

fn gen_loop_program(rng: &mut Rng) -> String {
    let n = rng.next_usize(30) + 1;
    let k = rng.next_u64() % 7;
    format!(
        "fn main() {{\n\
         \x20   let mut sum = 0\n\
         \x20   let mut i = 0\n\
         \x20   while i < {n} {{\n\
         \x20       sum = sum + i * {k}\n\
         \x20       i = i + 1\n\
         \x20   }}\n\
         \x20   print(sum)\n\
         \x20   print(i)\n\
         }}\n"
    )
}

fn gen_fold_program(rng: &mut Rng) -> String {
    let count = rng.next_usize(6) + 1;
    let mut items = Vec::with_capacity(count);
    for _ in 0..count {
        items.push((rng.next_u64() % 50).to_string());
    }
    let arr = items.join(", ");
    format!(
        "fn sq(x: i64) -> i64 {{ return x * x }}\n\
         fn main() {{\n\
         \x20   let arr = [{arr}]\n\
         \x20   let mut acc = 0\n\
         \x20   for x in arr {{ acc = acc + sq(x) }}\n\
         \x20   print(acc)\n\
         \x20   print(len(arr))\n\
         }}\n"
    )
}

fn gen_recursion_program(rng: &mut Rng) -> String {
    let n = rng.next_usize(14) + 5; // fib(5..=18): small, fast
    format!(
        "fn fib(n: i64) -> i64 {{\n\
         \x20   if n <= 1 {{ return n }}\n\
         \x20   return fib(n - 1) + fib(n - 2)\n\
         }}\n\
         fn main() {{\n\
         \x20   let n = {n}\n\
         \x20   print(fib(n))\n\
         \x20   match n % 3 {{\n\
         \x20       0 => print(\"zero\")\n\
         \x20       1 => print(\"one\")\n\
         \x20       _ => print(\"two\")\n\
         \x20   }}\n\
         }}\n"
    )
}
