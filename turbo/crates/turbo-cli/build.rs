//! Build-time exhaustiveness check for the public error-docs tree.
//!
//! There are two parallel sources of error-code documentation:
//!
//! 1. **Source of truth**: `turbo/crates/turbo-cli/src/errors/E0NNN.md`,
//!    which the CLI embeds via `include_str!` so `turbolang explain E0NNN`
//!    keeps working in a release binary with no filesystem dependency.
//! 2. **Public docs**: `docs/errors/E0NNN.md` at the repo root, which is
//!    populated by symlinks pointing back at the source-of-truth files.
//!    This tree is what the CLI's `more info:` footer URL resolves to —
//!    currently the GitHub blob URL
//!    `https://github.com/ZVN-DEV/Turbo-Language/blob/master/docs/errors/E0NNN.md`,
//!    eventually the short form `https://turbolang.dev/errors/E0NNN`
//!    (aspirational; see the `TODO(P3)` in `main.rs::error_code_url`).
//!
//! ## Brittle parser caveats — DO NOT use these forms for new variants
//!
//! `collect_error_codes()` is a hand-rolled line scanner. It will
//! silently miss any of the following variant shapes — if you need them,
//! update this script *first*:
//!
//!   - `#[cfg(test)] E0999,` (any cfg-gated variant)
//!   - `E0999 = 0x1234,` (explicit discriminant assignment)
//!   - macro-generated variants (e.g. `define_codes! { ... }`)
//!   - variants whose name doesn't match `E\d{4}` exactly
//!   - any line that wraps the variant onto a continuation
//!
//! Today none of these shapes exist in `errors.rs`. Keep it that way,
//! or extend the parser. Pulling in `syn` would be heavier than this
//! whole script.
//!
//! The `include_str!` table in `main.rs` already enforces source-of-truth
//! exhaustiveness — adding a new `ErrorCode` variant without a `.md`
//! breaks the build. This script enforces the *other half*: every variant
//! of `turbo_ast::ErrorCode` must also have a `docs/errors/E0NNN.md` entry.
//!
//! It does so by parsing `turbo/crates/turbo-ast/src/errors.rs` for the
//! `E0NNN` variant identifiers and asserting each one resolves to an
//! existing file under `docs/errors/`.

use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    // Cargo sets CARGO_MANIFEST_DIR at runtime for build scripts;
    // use the runtime env var (not the compile-time `env!()` macro)
    // so the path is always correct regardless of repo location on disk.
    let manifest_dir = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR")
            .expect("CARGO_MANIFEST_DIR not set — are we running outside Cargo?"),
    );

    // manifest_dir = <repo>/turbo/crates/turbo-cli
    // errors.rs is a sibling crate: ../turbo-ast/src/errors.rs
    // docs/errors is at the repo root: ../../../docs/errors
    let crates_dir = manifest_dir
        .parent() // <repo>/turbo/crates
        .expect("turbo-cli/build.rs: CARGO_MANIFEST_DIR has no parent");
    let errors_rs = crates_dir.join("turbo-ast/src/errors.rs");

    let repo_root = crates_dir
        .parent() // <repo>/turbo
        .and_then(Path::parent) // <repo>
        .expect("turbo-cli/build.rs: failed to resolve repo root from CARGO_MANIFEST_DIR");
    let docs_dir = repo_root.join("docs/errors");

    println!("cargo:rerun-if-changed={}", errors_rs.display());
    println!("cargo:rerun-if-changed={}", docs_dir.display());

    let source = match fs::read_to_string(&errors_rs) {
        Ok(s) => s,
        Err(e) => {
            // Don't fail catastrophically on packaged crate builds (e.g.
            // crates.io tarballs) where the sibling crate may be absent.
            // Print a notice and skip — the runtime `include_str!` table
            // is still authoritative.
            println!(
                "cargo:warning=turbo-cli/build.rs: skipping docs/errors check \
                 ({}: {})",
                errors_rs.display(),
                e
            );
            return;
        }
    };

    let codes = collect_error_codes(&source);
    if codes.is_empty() {
        panic!(
            "turbo-cli/build.rs: parsed zero error codes from {} — \
             did the format of errors.rs change?",
            errors_rs.display()
        );
    }

    let mut missing = Vec::new();
    for code in &codes {
        let path = docs_dir.join(format!("{code}.md"));
        // `Path::exists` follows symlinks, which is what we want — the
        // docs/errors entries are intentionally symlinks pointing back at
        // the source-of-truth markdown files.
        if !path.exists() {
            missing.push(path);
        }
    }

    if !missing.is_empty() {
        let pretty: Vec<String> = missing.iter().map(|p| p.display().to_string()).collect();
        panic!(
            "turbo-cli/build.rs: missing public docs/errors entries for {} \
             error code(s):\n  {}\n\nAdd a symlink for each missing file:\n  \
             cd docs/errors && ln -s ../../turbo/crates/turbo-cli/src/errors/E0NNN.md E0NNN.md",
            missing.len(),
            pretty.join("\n  "),
        );
    }
}

/// Walk `errors.rs` and pull out every `E0NNN` identifier that appears
/// inside the `pub enum ErrorCode { ... }` block. Using a hand-rolled
/// scan keeps the build script free of `syn`/`proc-macro2` dependencies.
fn collect_error_codes(source: &str) -> Vec<String> {
    let Some(enum_start) = source.find("pub enum ErrorCode") else {
        return Vec::new();
    };
    let after_enum = &source[enum_start..];
    let Some(brace) = after_enum.find('{') else {
        return Vec::new();
    };
    let body = &after_enum[brace + 1..];

    // Find the matching closing brace by depth-tracking — variants don't
    // contain their own braces today, but doc-comment markdown might.
    let mut depth = 1;
    let mut end = body.len();
    for (i, ch) in body.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = i;
                    break;
                }
            }
            _ => {}
        }
    }
    let enum_body = &body[..end];

    let mut out = Vec::new();
    for line in enum_body.lines() {
        let trimmed = line.trim();
        // Variant lines look like `E0123,` — match exactly that shape so
        // we don't accidentally pick up identifiers inside doc comments.
        let Some(name) = trimmed.strip_suffix(',') else {
            continue;
        };
        if is_error_code_ident(name) {
            out.push(name.to_string());
        }
    }
    out.sort();
    out.dedup();
    out
}

fn is_error_code_ident(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.len() != 5 {
        return false;
    }
    if bytes[0] != b'E' {
        return false;
    }
    bytes[1..].iter().all(|b| b.is_ascii_digit())
}
