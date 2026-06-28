//! Codegen smoke fuzz harness for the Turbo compiler.
//!
//! Run:
//!     cargo run --release --manifest-path turbo/fuzz/Cargo.toml --bin codegen-fuzz
//!
//! Override iteration count via env var:
//!     TURBO_FUZZ_ITERS=1000 cargo run --release ... --bin codegen-fuzz
//!
//! Default: 200 iterations.
//!
//! The harness extends the existing frontend fuzz harness (`turbo-fuzz`)
//! with a codegen step. Each iteration:
//!
//!   1. Generates a random program via a small mutator over a curated
//!      set of valid Turbo programs (so we get syntactically plausible
//!      input most of the time).
//!   2. Lexes → parses → runs sema.
//!   3. If sema is clean, attempts AOT object emission via the Cranelift
//!      backend (no link, no execution).
//!   4. Wraps the whole pipeline in `panic::catch_unwind` so a single
//!      ICE doesn't kill the corpus.
//!
//! Errors (lex/parse/sema/codegen `Result::Err`) are *expected* and not
//! counted as crashes. Only panics, aborts, or unwinds count.
//!
//! Designed to run as a CI smoke job: 200 iters at release-mode is
//! roughly 30 seconds on a modern laptop.

use std::env;
use std::panic;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

fn main() {
    let iters: usize = env::var("TURBO_FUZZ_ITERS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(200);

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
