//! CLI-level integration tests for the BL-26 polish cluster.
//!
//! Three small, contained behaviours, all driven through the real `turbolang`
//! binary via the `CARGO_BIN_EXE_turbolang` path Cargo hands to integration
//! tests:
//!
//!  1. `turbolang test` renders a clean, deterministic summary — TTY-gated
//!     color (so captured/piped output carries no ANSI escapes), stable result
//!     ordering, and a total elapsed-time line.
//!  2. Operational IO errors (here: `turbolang bench` on a missing file) are
//!     translated to plain language and never leak the raw `(os error N)`
//!     jargon that `std::io::Error`'s `Display` appends.
//!  3. The REPL no longer reports a binding used on a *later* line as an
//!     unused variable.
//!
//! Note: the fixtures use integer arithmetic only — no `.tb` float output — so
//! these assertions are independent of any float-formatting changes.

use std::io::Write;
use std::process::{Command, Stdio};

/// Strip ANSI SGR escape sequences. Used where we want to assert on the *text*
/// regardless of whether the binary decided its stream was a terminal.
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

/// Create a unique temp directory for a test and return its path.
fn unique_temp_dir(tag: &str) -> std::path::PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "turbo-bl26-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

#[test]
fn test_summary_is_deterministic_ordered_and_timed() {
    let dir = unique_temp_dir("summary");
    let src = dir.join("suite_test.tb");
    // Declaration order: aaa (pass), bbb (fail), ccc (pass). Integer asserts
    // only, so no float output is involved.
    std::fs::write(
        &src,
        "@test fn test_aaa() { assert(1 + 1 == 2, \"ok\") }\n\
         @test fn test_bbb() { assert(1 == 2, \"boom\") }\n\
         @test fn test_ccc() { assert(2 + 2 == 4, \"ok\") }\n",
    )
    .expect("write suite");

    let output = Command::new(env!("CARGO_BIN_EXE_turbolang"))
        .args(["test", src.to_str().unwrap()])
        .output()
        .expect("failed to spawn turbolang binary");

    // `output()` pipes stderr, so the binary sees a non-terminal: the captured
    // bytes must be free of ANSI escapes for byte-stable assertions.
    let raw = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        !raw.contains('\x1b'),
        "captured (non-TTY) test summary leaked ANSI escapes:\n{raw:?}"
    );

    let stderr = strip_ansi(&raw);

    // Plain PASS/FAIL tags (color was correctly gated off).
    assert!(
        stderr.contains("PASS  test_aaa"),
        "missing PASS for test_aaa:\n{stderr}"
    );
    assert!(
        stderr.contains("FAIL  test_bbb"),
        "missing FAIL for test_bbb:\n{stderr}"
    );
    assert!(
        stderr.contains("PASS  test_ccc"),
        "missing PASS for test_ccc:\n{stderr}"
    );

    // Stable ordering: results appear in declaration order.
    let p_aaa = stderr.find("test_aaa").expect("test_aaa present");
    let p_bbb = stderr.find("test_bbb").expect("test_bbb present");
    let p_ccc = stderr.find("test_ccc").expect("test_ccc present");
    assert!(
        p_aaa < p_bbb && p_bbb < p_ccc,
        "test results are not in stable declaration order:\n{stderr}"
    );

    // Counts plus a total elapsed-time line ("... in <secs>s").
    assert!(
        stderr.contains("2 passed, 1 failed in "),
        "summary missing counts + total-time line:\n{stderr}"
    );
    let summary_line = stderr
        .lines()
        .rev()
        .find(|l| l.contains("passed,"))
        .expect("summary line present");
    assert!(
        summary_line.trim_end().ends_with('s'),
        "total-time line does not end in a seconds suffix: {summary_line:?}"
    );

    // Exit code semantics unchanged: a failing test means non-zero exit.
    assert!(
        !output.status.success(),
        "expected non-zero exit when a test fails"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn bench_missing_file_is_translated_without_os_error_jargon() {
    let dir = unique_temp_dir("bench");
    let missing = dir.join("not_here.tb");

    let output = Command::new(env!("CARGO_BIN_EXE_turbolang"))
        .args(["bench", missing.to_str().unwrap(), "-n", "1"])
        .output()
        .expect("failed to spawn turbolang binary");
    let stderr = strip_ansi(&String::from_utf8_lossy(&output.stderr));

    assert!(
        stderr.contains("could not read"),
        "missing plain-language read failure:\n{stderr}"
    );
    assert!(
        stderr.contains("no such file or directory"),
        "missing translated IO reason:\n{stderr}"
    );
    assert!(
        !stderr.contains("os error"),
        "raw `(os error N)` jargon leaked from the bench read path:\n{stderr}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn repl_does_not_flag_a_later_used_binding_as_unused() {
    // Feed the REPL a binding on one line that is referenced on the next.
    // Pre-fix, the per-entry warning pass reported `x` as unused the moment it
    // was bound; post-fix the E0515 warning is suppressed for REPL entries.
    let mut child = Command::new(env!("CARGO_BIN_EXE_turbolang"))
        .arg("repl")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn turbolang repl");

    child
        .stdin
        .take()
        .expect("repl stdin")
        .write_all(b"let x = 5\nx + 1\n:quit\n")
        .expect("write to repl stdin");

    let output = child.wait_with_output().expect("await repl");
    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    let stderr = strip_ansi(&String::from_utf8_lossy(&output.stderr));

    assert!(
        !stderr.contains("unused variable"),
        "REPL spuriously flagged a later-used binding as unused:\nstderr:\n{stderr}"
    );
    // Proves the binding was actually carried across entries and evaluated.
    assert!(
        stdout.contains('6'),
        "REPL did not evaluate `x + 1` across lines:\nstdout:\n{stdout}"
    );
}
