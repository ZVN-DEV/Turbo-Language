//! CLI-level integration tests for the operational error envelope (BL-13b).
//!
//! These drive the real `turbolang` binary (via the `CARGO_BIN_EXE_turbolang`
//! path Cargo provides to integration tests) and assert that file-not-found
//! and import-resolution failures render the full `error[E06NN]:` envelope —
//! a code, a `Help:` line, a `more info:` footer — and never leak the raw
//! `(os error N)` jargon that `std::io::Error`'s Display appends.

use std::process::Command;

/// Strip ANSI SGR escape sequences so assertions match regardless of whether
/// the binary decided stderr was a terminal.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip "[ ... <final byte>"
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

fn run_turbolang(args: &[&str]) -> (String, bool) {
    let output = Command::new(env!("CARGO_BIN_EXE_turbolang"))
        .args(args)
        .output()
        .expect("failed to spawn turbolang binary");
    let stderr = strip_ansi(&String::from_utf8_lossy(&output.stderr));
    (stderr, output.status.success())
}

#[test]
fn file_not_found_renders_e0611_without_os_error_jargon() {
    let (stderr, ok) = run_turbolang(&["run", "definitely_does_not_exist_zzz.tb"]);

    assert!(!ok, "expected a non-zero exit for a missing file");
    assert!(
        stderr.contains("error[E0611]"),
        "missing E0611 code in: {stderr}"
    );
    assert!(
        stderr.contains("could not find"),
        "missing plain-language reason in: {stderr}"
    );
    assert!(
        !stderr.contains("os error"),
        "raw `(os error N)` jargon leaked in: {stderr}"
    );
    assert!(stderr.contains("Help:"), "missing Help: line in: {stderr}");
    assert!(
        stderr.contains("more info: https://"),
        "missing `more info:` footer in: {stderr}"
    );
}

#[test]
fn unresolvable_import_renders_e0610_without_os_error_jargon() {
    // Build an isolated temp source file that imports a non-existent module.
    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "turbo-op-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let src = dir.join("main.tb");
    std::fs::write(
        &src,
        "import { thing } from \"does_not_exist_module.tb\"\nfn main() { print(1) }\n",
    )
    .expect("write temp source");

    let (stderr, ok) = run_turbolang(&["run", src.to_str().unwrap()]);

    // Best-effort cleanup.
    let _ = std::fs::remove_dir_all(&dir);

    assert!(!ok, "expected a non-zero exit for an unresolvable import");
    assert!(
        stderr.contains("error[E0610]"),
        "missing E0610 code in: {stderr}"
    );
    assert!(
        stderr.contains("could not resolve import"),
        "missing import reason in: {stderr}"
    );
    assert!(
        !stderr.contains("os error"),
        "raw `(os error N)` jargon leaked in: {stderr}"
    );
    assert!(
        stderr.contains("more info: https://"),
        "missing `more info:` footer in: {stderr}"
    );
}
