//! CLI-level integration tests for `turbolang bench` and the `@bench` attribute
//! (BL-21).
//!
//! Regression guards for the bug where `bench` reported "0/N benchmarks passed"
//! on a benchmark that ran perfectly: the AOT half went through the normal build
//! path, which rejected `@bench` (`unknown attribute '@bench'`), so the headline
//! always read as total failure on correct input. These drive the real
//! `turbolang` binary (via `CARGO_BIN_EXE_turbolang`).

use std::process::Command;

/// Strip ANSI SGR escape sequences so assertions match regardless of whether
/// the binary decided stderr was a terminal.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for tc in chars.by_ref() {
                    if tc.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Absolute path to the shared `@bench` fixture used across these tests.
fn fixture() -> String {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/phase1/bench_attribute.tb"
    )
    .to_string()
}

#[test]
fn bench_does_not_report_zero_passed_for_a_valid_benchmark() {
    // `-n 1` keeps the test fast: one JIT run, one AOT run.
    let output = Command::new(env!("CARGO_BIN_EXE_turbolang"))
        .args(["bench", &fixture(), "-n", "1"])
        .output()
        .expect("failed to spawn turbolang binary");
    let stderr = strip_ansi(&String::from_utf8_lossy(&output.stderr));

    // The headline must never read as a total failure for a benchmark that
    // produced a valid timing.
    assert!(
        !stderr.contains("0/1"),
        "bench reported a 0/1 failure on a valid benchmark:\n{stderr}"
    );
    assert!(
        !stderr.contains("benchmarks passed"),
        "bench still uses the old pass/fail-on-parity headline:\n{stderr}"
    );

    // It must lead with the timing — the thing the user came for.
    assert!(
        stderr.contains("JIT:") && stderr.contains("median"),
        "bench did not report a JIT timing:\n{stderr}"
    );

    // Headline counts completed benchmarks (valid timing), not AOT parity.
    assert!(
        stderr.contains("1/1 benchmarks completed"),
        "bench did not count the valid benchmark as completed:\n{stderr}"
    );

    // The per-benchmark label is the FUNCTION name, not the file name.
    assert!(
        stderr.contains("--- bench_fib ---"),
        "bench labeled by file name instead of the @bench function name:\n{stderr}"
    );
    assert!(
        !stderr.contains("--- bench_attribute ---"),
        "bench still labels by the file stem:\n{stderr}"
    );

    // AOT parity is a separate annotation, never the headline.
    assert!(
        stderr.contains("AOT parity:"),
        "bench did not emit a separate AOT parity line:\n{stderr}"
    );
}

#[test]
fn build_accepts_the_bench_attribute() {
    let mut out_bin = std::env::temp_dir();
    out_bin.push(format!("turbo_bench_attr_build_{}", std::process::id()));

    let output = Command::new(env!("CARGO_BIN_EXE_turbolang"))
        .args(["build", &fixture(), "-o", out_bin.to_str().unwrap()])
        .output()
        .expect("failed to spawn turbolang binary");
    let stderr = strip_ansi(&String::from_utf8_lossy(&output.stderr));

    // The core BL-21 fix: the build/AOT path must no longer reject `@bench`.
    assert!(
        !stderr.contains("unknown attribute"),
        "build still rejects the @bench attribute:\n{stderr}"
    );
    assert!(
        !stderr.contains("E0001"),
        "build emitted a parse error envelope on a @bench file:\n{stderr}"
    );

    // Best-effort cleanup.
    let _ = std::fs::remove_file(&out_bin);
}
