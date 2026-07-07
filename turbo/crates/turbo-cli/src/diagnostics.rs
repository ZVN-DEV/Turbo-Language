//! Error/warning diagnostic rendering via `ariadne`, plus the error-code
//! footer URL helper.

use ariadne::{Color, Label, Report, ReportKind, Source};
use turbo_ast::ErrorCode;

/// Print a rich error diagnostic using ariadne.
/// Produce a (message, help) pair for a lexer error span. A bare "unexpected
/// character" is unhelpful when the real problem is a numeric literal the lexer
/// matched but couldn't fit into `i64` (it returns `None`, which surfaces as a
/// lex error). Detect the all-digits case and say so precisely.
pub(crate) fn lex_error_message(snippet: &str) -> (String, &'static str) {
    let is_int_literal =
        !snippet.is_empty() && snippet.chars().all(|c| c.is_ascii_digit() || c == '_');
    if is_int_literal {
        (
            format!("integer literal `{snippet}` is too large for `i64` (max 9223372036854775807)"),
            "use a smaller value, or split the computation to stay within i64 range",
        )
    } else {
        (
            format!("unexpected character `{snippet}`"),
            "remove this character or check for typos",
        )
    }
}

pub(crate) fn report_error(
    source: &str,
    filename: &str,
    message: &str,
    span: &std::ops::Range<usize>,
    help: Option<&str>,
    code: Option<ErrorCode>,
) {
    // Clamp span to source bounds to avoid panics on edge-case spans
    let start = span.start.min(source.len());
    let end = span.end.min(source.len()).max(start);
    let clamped = start..end;

    let display_message = if let Some(c) = code {
        format!("error[{}]: {}", c.as_str(), message)
    } else {
        message.to_string()
    };

    let mut builder =
        Report::build(ReportKind::Error, filename, clamped.start).with_message(&display_message);

    builder = builder.with_label(
        Label::new((filename, clamped))
            .with_message(message)
            .with_color(Color::Red),
    );

    // Compose the footer. Real help text (if any) is rendered by ariadne as a
    // `Help:` block. The `more info: <url>` line is decoupled from it: when
    // there is help text the URL is appended under the Help block, but when
    // there is none it is emitted on its own line *after* the frame rather
    // than collapsing onto an empty `Help:` label (which read like a bug).
    let (help_block, standalone_footer) = compose_diagnostic_footer(help, code);
    if let Some(block) = help_block {
        builder = builder.with_help(block);
    }

    builder
        .finish()
        .eprint((filename, Source::from(source)))
        .unwrap();

    if let Some(footer) = standalone_footer {
        eprintln!("{footer}");
    }
}

/// Build the two footer pieces for a diagnostic, keeping the `Help:` label and
/// the `more info:` URL decoupled.
///
/// Returns `(help_block, standalone_footer)`:
/// - `help_block` is handed to ariadne's `with_help` and rendered as a
///   `Help:` block. It is only present when there is real help text; the
///   `more info:` line is appended under it as a continuation when a code is
///   also present.
/// - `standalone_footer` is printed on its own line after the frame. It
///   carries the `more info:` line when there is no help text, so the URL is
///   never glued onto an empty `Help:` label.
pub(crate) fn compose_diagnostic_footer(
    help: Option<&str>,
    code: Option<ErrorCode>,
) -> (Option<String>, Option<String>) {
    let more_info = code.map(|c| format!("more info: {}", error_code_url(c)));
    match (help, more_info) {
        (Some(h), Some(mi)) => (Some(format!("{h}\n  {mi}")), None),
        (Some(h), None) => (Some(h.to_string()), None),
        (None, Some(mi)) => (None, Some(format!("  {mi}"))),
        (None, None) => (None, None),
    }
}

/// Returns the canonical public URL for a given error code.
///
/// We currently point at the GitHub blob URL for the source-of-truth
/// markdown file. The `docs/errors/` tree is a parallel symlink farm
/// pointing back at `turbo-cli/src/errors/E0NNN.md`, so the GitHub URL
/// is guaranteed to resolve as long as the file exists in master.
///
/// TODO(P3): once `turbolang.dev/errors/E0NNN` is live (with a stable
/// redirect to the same content), flip this back to the short form.
pub(crate) fn error_code_url(code: ErrorCode) -> String {
    format!(
        "https://github.com/ZVN-DEV/Turbo-Language/blob/master/docs/errors/{}.md",
        code.as_str()
    )
}

/// Render an operational/CLI error with the same envelope as the compile-time
/// diagnostics, but without an ariadne source frame (these errors — a missing
/// file, an unresolvable import — have no `.tb` span to point a caret at).
///
/// Reuses [`compose_diagnostic_footer`] / [`error_code_url`] so the `Help:`
/// block and the `more info:` footer are formatted identically to every other
/// diagnostic:
///
/// ```text
/// error[E06NN]: <message>
/// Help: <help>
///   more info: <url>
/// ```
pub(crate) fn report_codeful_error(message: &str, help: Option<&str>, code: ErrorCode) {
    eprintln!("\x1b[1;31merror[{}]\x1b[0m: {}", code.as_str(), message);
    let (help_block, standalone_footer) = compose_diagnostic_footer(help, Some(code));
    if let Some(block) = help_block {
        eprintln!("Help: {block}");
    }
    if let Some(footer) = standalone_footer {
        eprintln!("{footer}");
    }
}

/// Translate a [`std::io::Error`] into a jargon-free reason phrase, dropping
/// the `(os error N)` suffix that the error's `Display` appends for OS errors.
///
/// `io::ErrorKind`'s own `Display` ("is a directory", "read-only filesystem",
/// …) is already human-readable and never includes the raw errno; the two most
/// common kinds get an even friendlier phrasing. This mirrors the catch-all in
/// [`report_file_error`], which surfaces `err.kind()` rather than `err`, and is
/// used by the operational error paths (`init`, `bench`, lockfile writes) that
/// render an io error inline instead of through the E0611 envelope.
pub(crate) fn io_reason(err: &std::io::Error) -> String {
    use std::io::ErrorKind;
    match err.kind() {
        ErrorKind::NotFound => "no such file or directory".to_string(),
        ErrorKind::PermissionDenied => "permission denied".to_string(),
        other => other.to_string(),
    }
}

/// Render a file-not-found / unreadable-source error (E0611) and exit.
///
/// Drops the raw `(os error N)` jargon that `std::io::Error`'s `Display`
/// leaks: callers see a plain-language reason plus a `Help:` line and the
/// `more info:` footer, matching the quality of the compile diagnostics.
pub(crate) fn report_file_error(path: &std::path::Path, err: &std::io::Error) -> ! {
    use std::io::ErrorKind;
    let (message, help) = match err.kind() {
        ErrorKind::NotFound => (
            format!("could not find `{}` — check the path", path.display()),
            "make sure the file exists and the path is spelled correctly",
        ),
        ErrorKind::PermissionDenied => (
            format!("permission denied reading `{}`", path.display()),
            "check the file's permissions, or run as a user that can read it",
        ),
        // `err.kind()`'s Display ("is a directory", "invalid input", …) is
        // jargon-free — unlike `{err}`, it never appends "(os error N)".
        other => (
            format!("could not read `{}`: {other}", path.display()),
            "check that the path points to a readable file",
        ),
    };
    report_codeful_error(&message, Some(help), ErrorCode::E0611);
    std::process::exit(1);
}

/// Render an import-resolution failure (E0610) and exit. `message` is the
/// human-readable reason produced by [`resolve_imports`].
pub(crate) fn report_import_error(message: &str) -> ! {
    report_codeful_error(
        message,
        Some("check the import path, and that the file exists and parses cleanly"),
        ErrorCode::E0610,
    );
    std::process::exit(1);
}

pub(crate) fn report_warning(
    source: &str,
    filename: &str,
    message: &str,
    span: &std::ops::Range<usize>,
    code: Option<ErrorCode>,
) {
    let start = span.start.min(source.len());
    let end = span.end.min(source.len()).max(start);
    let clamped = start..end;

    let display_message = if let Some(c) = code {
        format!("warning[{}]: {}", c.as_str(), message)
    } else {
        message.to_string()
    };

    let builder = Report::build(ReportKind::Warning, filename, clamped.start)
        .with_message(&display_message)
        .with_label(
            Label::new((filename, clamped))
                .with_message(message)
                .with_color(Color::Yellow),
        );

    builder
        .finish()
        .eprint((filename, Source::from(source)))
        .unwrap();
}
