//! Contextual help generation (`parse_help` / `sema_help`) and the
//! `turbolang explain E0NNN` command, including the embedded error-doc table.
//!
//! This module lives directly under `src/` so the `include_str!("errors/…")`
//! table in `detailed_explanation` resolves against `src/errors/`.

use turbo_ast::ErrorCode;

/// Generate contextual help text for common parse error patterns.
pub(crate) fn parse_help(message: &str) -> Option<String> {
    if message.contains("import") || message.contains("`from`") || message.contains("path string") {
        return Some("imports look like `import { sqrt, pi } from \"./math.tb\"`".to_string());
    }
    None
}

/// Generate contextual help text for common sema error patterns.
pub(crate) fn sema_help(message: &str) -> Option<String> {
    if message.contains("undefined variable") {
        // Extract variable name from backticks
        if let Some(name) = extract_backtick_name(message) {
            return Some(format!(
                "did you mean to declare `{name}` with `let {name} = ...`?"
            ));
        }
        return Some("check the variable name for typos, or declare it with `let`".to_string());
    }
    if message.contains("undefined function") {
        if let Some(name) = extract_backtick_name(message) {
            return Some(format!("define `{name}` with `fn {name}(...) {{ ... }}`"));
        }
        return Some("check the function name for typos, or define it with `fn`".to_string());
    }
    if message.contains("cannot assign to immutable variable") {
        return Some("declare with `let mut` to make it mutable".to_string());
    }
    if message.contains("no `main` function found") {
        return Some("add a `fn main() { ... }` as the entry point".to_string());
    }
    if message.contains("mismatched types in arithmetic") {
        return Some(
            "both sides of an arithmetic operation must have the same numeric type".to_string(),
        );
    }
    if message.contains("cannot perform arithmetic on") {
        return Some("arithmetic operators (`+`, `-`, `*`, `/`, `%`) only work on numeric types (`i32`, `i64`, `f32`, `f64`)".to_string());
    }
    if message.contains("type annotation") && message.contains("doesn't match") {
        return Some("either change the type annotation or the assigned value".to_string());
    }
    if message.contains("should return") && message.contains("but body returns") {
        return Some(
            "make sure the last expression in the function body matches the declared return type"
                .to_string(),
        );
    }
    if message.contains("if/else branches have different types") {
        return Some(
            "both branches of an if/else expression must produce the same type".to_string(),
        );
    }
    if message.contains("if condition must be `bool`")
        || message.contains("while condition must be `bool`")
    {
        return Some(
            "conditions must be `bool`; use a comparison like `x > 0` instead".to_string(),
        );
    }
    if message.contains("match is not exhaustive") {
        // The sema message already names the missing variants after
        // `missing variants:` — turn them into an actionable suggestion.
        if let Some(rest) = message.split("missing variants:").nth(1) {
            let missing: Vec<&str> = rest
                .split(',')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .collect();
            if let Some(first) = missing.first() {
                if missing.len() == 1 {
                    return Some(format!(
                        "add an arm '{first} => ...' or a catch-all '_ => ...'"
                    ));
                }
                return Some(format!(
                    "add arms for {} or a catch-all '_ => ...'",
                    missing.join(", ")
                ));
            }
        }
        return Some(
            "add a match arm for each remaining case, or a catch-all '_ => ...'".to_string(),
        );
    }
    if message.contains("has no field") {
        // The sema message embeds the struct's field list after
        // `available fields:` and, when close, a `did you mean` suggestion.
        if let Some(struct_name) = extract_backtick_name(message) {
            let fields = message.split("available fields:").nth(1).map(|s| {
                s.trim()
                    .trim_end_matches(')')
                    .trim()
                    .split(',')
                    .map(|f| format!("'{}'", f.trim()))
                    .collect::<Vec<_>>()
                    .join(", ")
            });
            let suggestion = if message.contains("did you mean") {
                nth_backtick_name(message, 3)
            } else {
                None
            };
            match (fields, suggestion) {
                (Some(fields), Some(sug)) => {
                    return Some(format!(
                        "'{struct_name}' has fields {fields} — did you mean '{sug}'?"
                    ))
                }
                (Some(fields), None) => {
                    return Some(format!("'{struct_name}' has fields {fields}"))
                }
                (None, Some(sug)) => return Some(format!("did you mean '{sug}'?")),
                (None, None) => {}
            }
        }
        return Some("check the field name against the struct definition".to_string());
    }
    if message.contains("argument(s) but") {
        // The user-function arity site embeds the full signature after
        // `signature ` — echo it plus what was actually passed.
        if let (Some(name), Some(params)) = (
            extract_backtick_name(message),
            parse_signature_params(message),
        ) {
            let count = if params.trim().is_empty() {
                0
            } else {
                params.split(',').count()
            };
            let noun = if count == 1 { "arg" } else { "args" };
            return Some(match parse_count_after(message, "but ") {
                Some(passed) => {
                    format!("'{name}' takes {count} {noun} ({params}); you passed {passed}")
                }
                None => format!("'{name}' takes {count} {noun} ({params})"),
            });
        }
        return Some(
            "check the function signature for the correct number of arguments".to_string(),
        );
    }
    if message.contains("argument") && message.contains("expects") && message.contains("found") {
        return Some(
            "the argument type doesn't match the parameter type in the function signature"
                .to_string(),
        );
    }
    None
}

/// Extract the first name enclosed in backticks from a message.
pub(crate) fn extract_backtick_name(message: &str) -> Option<&str> {
    nth_backtick_name(message, 1)
}

/// Extract the `n`th (1-based) backtick-enclosed name from a message.
pub(crate) fn nth_backtick_name(message: &str, n: usize) -> Option<&str> {
    let mut rest = message;
    let mut seen = 0;
    loop {
        let open = rest.find('`')? + 1;
        let close = rest[open..].find('`')? + open;
        seen += 1;
        if seen == n {
            return Some(&rest[open..close]);
        }
        rest = &rest[close + 1..];
    }
}

/// Extract the parameter list from an arity message's embedded
/// `signature `name(params)`` clause (returns `params` without the parens).
pub(crate) fn parse_signature_params(message: &str) -> Option<&str> {
    let after = message.split("signature `").nth(1)?;
    let sig = after.split('`').next()?;
    let open = sig.find('(')?;
    let close = sig.rfind(')')?;
    (close > open).then(|| sig[open + 1..close].trim())
}

/// Parse the run of digits immediately following `marker` in `message`.
pub(crate) fn parse_count_after(message: &str, marker: &str) -> Option<usize> {
    let after = message.split(marker).nth(1)?;
    let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

// =============================================================================
// turbolang explain -- Print description for an error code
// =============================================================================

/// Returns a detailed, markdown-formatted explanation for the given error
/// code. Explanations live in `errors/E0NNN.md` and are embedded at compile
/// time via `include_str!` so the binary is self-contained.
pub(crate) fn detailed_explanation(code: ErrorCode) -> Option<&'static str> {
    match code {
        // Parse errors (E0001-E0099)
        ErrorCode::E0001 => Some(include_str!("errors/E0001.md")),
        ErrorCode::E0002 => Some(include_str!("errors/E0002.md")),
        ErrorCode::E0003 => Some(include_str!("errors/E0003.md")),
        ErrorCode::E0007 => Some(include_str!("errors/E0007.md")),
        // Type errors (E0100-E0199)
        ErrorCode::E0100 => Some(include_str!("errors/E0100.md")),
        ErrorCode::E0101 => Some(include_str!("errors/E0101.md")),
        ErrorCode::E0102 => Some(include_str!("errors/E0102.md")),
        ErrorCode::E0103 => Some(include_str!("errors/E0103.md")),
        ErrorCode::E0104 => Some(include_str!("errors/E0104.md")),
        ErrorCode::E0105 => Some(include_str!("errors/E0105.md")),
        ErrorCode::E0106 => Some(include_str!("errors/E0106.md")),
        ErrorCode::E0107 => Some(include_str!("errors/E0107.md")),
        ErrorCode::E0108 => Some(include_str!("errors/E0108.md")),
        ErrorCode::E0109 => Some(include_str!("errors/E0109.md")),
        ErrorCode::E0110 => Some(include_str!("errors/E0110.md")),
        ErrorCode::E0111 => Some(include_str!("errors/E0111.md")),
        ErrorCode::E0112 => Some(include_str!("errors/E0112.md")),
        ErrorCode::E0113 => Some(include_str!("errors/E0113.md")),
        ErrorCode::E0114 => Some(include_str!("errors/E0114.md")),
        ErrorCode::E0115 => Some(include_str!("errors/E0115.md")),
        ErrorCode::E0116 => Some(include_str!("errors/E0116.md")),
        ErrorCode::E0117 => Some(include_str!("errors/E0117.md")),
        ErrorCode::E0118 => Some(include_str!("errors/E0118.md")),
        ErrorCode::E0119 => Some(include_str!("errors/E0119.md")),
        ErrorCode::E0120 => Some(include_str!("errors/E0120.md")),
        ErrorCode::E0121 => Some(include_str!("errors/E0121.md")),
        ErrorCode::E0122 => Some(include_str!("errors/E0122.md")),
        ErrorCode::E0123 => Some(include_str!("errors/E0123.md")),
        ErrorCode::E0124 => Some(include_str!("errors/E0124.md")),
        ErrorCode::E0125 => Some(include_str!("errors/E0125.md")),
        ErrorCode::E0126 => Some(include_str!("errors/E0126.md")),
        ErrorCode::E0127 => Some(include_str!("errors/E0127.md")),
        ErrorCode::E0128 => Some(include_str!("errors/E0128.md")),
        ErrorCode::E0129 => Some(include_str!("errors/E0129.md")),
        ErrorCode::E0130 => Some(include_str!("errors/E0130.md")),
        ErrorCode::E0131 => Some(include_str!("errors/E0131.md")),
        ErrorCode::E0132 => Some(include_str!("errors/E0132.md")),
        ErrorCode::E0133 => Some(include_str!("errors/E0133.md")),
        ErrorCode::E0134 => Some(include_str!("errors/E0134.md")),
        ErrorCode::E0135 => Some(include_str!("errors/E0135.md")),
        ErrorCode::E0136 => Some(include_str!("errors/E0136.md")),
        ErrorCode::E0137 => Some(include_str!("errors/E0137.md")),
        // Pattern/match errors (E0200-E0299)
        ErrorCode::E0200 => Some(include_str!("errors/E0200.md")),
        ErrorCode::E0201 => Some(include_str!("errors/E0201.md")),
        ErrorCode::E0202 => Some(include_str!("errors/E0202.md")),
        // Name resolution errors (E0300-E0399)
        ErrorCode::E0300 => Some(include_str!("errors/E0300.md")),
        ErrorCode::E0301 => Some(include_str!("errors/E0301.md")),
        ErrorCode::E0302 => Some(include_str!("errors/E0302.md")),
        ErrorCode::E0303 => Some(include_str!("errors/E0303.md")),
        ErrorCode::E0304 => Some(include_str!("errors/E0304.md")),
        ErrorCode::E0305 => Some(include_str!("errors/E0305.md")),
        ErrorCode::E0306 => Some(include_str!("errors/E0306.md")),
        ErrorCode::E0307 => Some(include_str!("errors/E0307.md")),
        ErrorCode::E0308 => Some(include_str!("errors/E0308.md")),
        ErrorCode::E0309 => Some(include_str!("errors/E0309.md")),
        ErrorCode::E0310 => Some(include_str!("errors/E0310.md")),
        ErrorCode::E0311 => Some(include_str!("errors/E0311.md")),
        ErrorCode::E0313 => Some(include_str!("errors/E0313.md")),
        ErrorCode::E0314 => Some(include_str!("errors/E0314.md")),
        ErrorCode::E0315 => Some(include_str!("errors/E0315.md")),
        ErrorCode::E0316 => Some(include_str!("errors/E0316.md")),
        ErrorCode::E0317 => Some(include_str!("errors/E0317.md")),
        ErrorCode::E0318 => Some(include_str!("errors/E0318.md")),
        ErrorCode::E0319 => Some(include_str!("errors/E0319.md")),
        ErrorCode::E0323 => Some(include_str!("errors/E0323.md")),
        ErrorCode::E0324 => Some(include_str!("errors/E0324.md")),
        // Codegen errors (E0400-E0499)
        ErrorCode::E0400 => Some(include_str!("errors/E0400.md")),
        ErrorCode::E0401 => Some(include_str!("errors/E0401.md")),
        ErrorCode::E0402 => Some(include_str!("errors/E0402.md")),
        ErrorCode::E0403 => Some(include_str!("errors/E0403.md")),
        ErrorCode::E0404 => Some(include_str!("errors/E0404.md")),
        ErrorCode::E0405 => Some(include_str!("errors/E0405.md")),
        ErrorCode::E0406 => Some(include_str!("errors/E0406.md")),
        ErrorCode::E0407 => Some(include_str!("errors/E0407.md")),
        // Misc errors (E0500-E0599)
        ErrorCode::E0501 => Some(include_str!("errors/E0501.md")),
        ErrorCode::E0502 => Some(include_str!("errors/E0502.md")),
        ErrorCode::E0503 => Some(include_str!("errors/E0503.md")),
        ErrorCode::E0504 => Some(include_str!("errors/E0504.md")),
        ErrorCode::E0505 => Some(include_str!("errors/E0505.md")),
        ErrorCode::E0506 => Some(include_str!("errors/E0506.md")),
        ErrorCode::E0507 => Some(include_str!("errors/E0507.md")),
        ErrorCode::E0508 => Some(include_str!("errors/E0508.md")),
        ErrorCode::E0509 => Some(include_str!("errors/E0509.md")),
        ErrorCode::E0510 => Some(include_str!("errors/E0510.md")),
        ErrorCode::E0512 => Some(include_str!("errors/E0512.md")),
        ErrorCode::E0513 => Some(include_str!("errors/E0513.md")),
        ErrorCode::E0514 => Some(include_str!("errors/E0514.md")),
        ErrorCode::E0515 => Some(include_str!("errors/E0515.md")),
        ErrorCode::E0516 => Some(include_str!("errors/E0516.md")),
        ErrorCode::E0530 => Some(include_str!("errors/E0530.md")),
        // Runtime & operational errors (E0600-E0699)
        ErrorCode::E0601 => Some(include_str!("errors/E0601.md")),
        ErrorCode::E0602 => Some(include_str!("errors/E0602.md")),
        ErrorCode::E0603 => Some(include_str!("errors/E0603.md")),
        ErrorCode::E0610 => Some(include_str!("errors/E0610.md")),
        ErrorCode::E0611 => Some(include_str!("errors/E0611.md")),
    }
}

/// Normalize a user-supplied error code into the canonical `E0NNN` form.
///
/// Accepts the conventional spelling plus the common shorthands a user is
/// likely to type: `100`, `e100`, `E100`, `e0100` and `E0100` all resolve to
/// `E0100`. Anything that isn't `E?<digits>` is upper-cased and returned as-is
/// so genuinely unknown input still falls through to the "unknown code" path.
pub(crate) fn normalize_error_code(input: &str) -> String {
    let upper = input.trim().to_uppercase();
    let digits = upper.strip_prefix('E').unwrap_or(&upper);
    if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
        if let Ok(n) = digits.parse::<u32>() {
            return format!("E{n:04}");
        }
    }
    upper
}

pub(crate) fn explain_error(code_str: &str) {
    // Accept lowercase input (`e0100`) and shorthands (`100`, `E100`) — the
    // codes are conventionally `E0NNN` but making users match the exact form is
    // needless friction.
    let normalized = normalize_error_code(code_str);
    if let Some(code) = ErrorCode::parse(&normalized) {
        println!(
            "\x1b[1;33m{}\x1b[0m: \x1b[1m{}\x1b[0m\n",
            code.as_str(),
            code.description()
        );
        if let Some(detail) = detailed_explanation(code) {
            println!("{detail}");
        }
    } else {
        eprintln!("\x1b[1;31merror\x1b[0m: unknown error code `{code_str}`");
        eprintln!("  Error codes range from E0001 to E0611.");
        eprintln!("  Example: turbolang explain E0100");
        std::process::exit(1);
    }
}

// =============================================================================
// turbolang doc -- Generate markdown documentation from source
// =============================================================================
