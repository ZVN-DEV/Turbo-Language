use std::path::Path;

/// Top-level declarations that trigger blank-line separation.
const TOP_LEVEL_KEYWORDS: &[&str] = &[
    "fn ", "struct ", "type ", "impl ", "trait ", "import ", "agent ", "tool ", "extern ",
    "@unsafe",
];

/// Format a Turbo source file in place (or check formatting).
pub fn format_file(path: &Path, check: bool) {
    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: could not read file `{}`: {e}", path.display());
            std::process::exit(1);
        }
    };

    let formatted = format_source(&source);

    if check {
        if source != formatted {
            eprintln!(
                "error: {} is not formatted (run `turbolang fmt {}` to fix)",
                path.display(),
                path.display()
            );
            std::process::exit(1);
        }
    } else if source != formatted {
        if let Err(e) = std::fs::write(path, &formatted) {
            eprintln!("error: could not write file `{}`: {e}", path.display());
            std::process::exit(1);
        }
        eprintln!("\x1b[32m\u{2713}\x1b[0m Formatted {}", path.display());
    } else {
        eprintln!("{} already formatted", path.display());
    }
}

/// Apply line-based formatting rules to Turbo source code.
///
/// Rules:
/// 1. Trim trailing whitespace from every line
/// 2. Normalize indentation to 4 spaces per indent level (track `{` / `}`)
/// 3. Ensure exactly one blank line between top-level items
/// 4. Ensure file ends with exactly one newline
pub fn format_source(source: &str) -> String {
    let lines: Vec<&str> = source.lines().collect();

    // Phase 1: trim trailing whitespace
    let trimmed: Vec<String> = lines.iter().map(|l| l.trim_end().to_string()).collect();

    // Phase 2: normalize intra-line spacing (collapse runs, space after commas/colons)
    let spaced_lines: Vec<String> = trimmed.iter().map(|l| normalize_line_spacing(l)).collect();

    // Phase 3: normalize indentation based on brace depth
    let reindented = reindent(&spaced_lines);

    // Phase 4: normalize blank lines between top-level items
    let spaced = normalize_top_level_spacing(&reindented);

    // Phase 5: ensure exactly one trailing newline
    let mut result = spaced.join("\n");
    // Trim trailing blank lines then add exactly one newline
    while result.ends_with('\n') {
        result.pop();
    }
    result.push('\n');
    result
}

/// Normalize intra-line spacing: collapse multiple spaces to one, ensure space
/// after commas and colons (in type annotations), remove space before `)` and `]`,
/// remove space after `(` and `[`. Preserves string contents and leading indent.
fn normalize_line_spacing(line: &str) -> String {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with("//") {
        return line.to_string();
    }

    // Preserve leading whitespace (will be fixed by reindent anyway)
    let leading = line.len() - line.trim_start().len();
    let indent = &line[..leading];
    let content = &line[leading..];

    let mut result = String::with_capacity(content.len());
    let mut in_string = false;
    let mut escape_next = false;
    let mut generic_depth: u32 = 0; // track <...> nesting for generics
    let mut chars = content.chars().peekable();

    while let Some(c) = chars.next() {
        if escape_next {
            escape_next = false;
            result.push(c);
            continue;
        }
        if c == '\\' && in_string {
            escape_next = true;
            result.push(c);
            continue;
        }
        if c == '"' {
            in_string = !in_string;
            result.push(c);
            continue;
        }
        if in_string {
            result.push(c);
            continue;
        }

        // Line comment — pass through rest unchanged
        if c == '/' && chars.peek() == Some(&'/') {
            result.push(c);
            for rest in chars.by_ref() {
                result.push(rest);
            }
            break;
        }

        // Collapse multiple spaces to one
        if c == ' ' {
            // Skip if previous char is `(` or `[` (no space after open paren/bracket)
            if result.ends_with('(') || result.ends_with('[') {
                // eat all spaces
                while chars.peek() == Some(&' ') {
                    chars.next();
                }
                continue;
            }
            // eat extra spaces
            while chars.peek() == Some(&' ') {
                chars.next();
            }
            // Skip if next char is `)` or `]` (no space before close paren/bracket)
            if let Some(&next) = chars.peek() {
                if next == ')' || next == ']' {
                    continue;
                }
            }
            result.push(' ');
            continue;
        }

        // Ensure space after comma
        if c == ',' {
            result.push(',');
            // Eat existing spaces
            while chars.peek() == Some(&' ') {
                chars.next();
            }
            // Add exactly one space (unless next is newline/end)
            if chars.peek().is_some() {
                result.push(' ');
            }
            continue;
        }

        // Remove space before `)` and `]`
        if (c == ')' || c == ']') && result.ends_with(' ') {
            // Only trim if the previous non-space isn't a keyword
            let trimmed_result = result.trim_end();
            // Simple heuristic: don't trim if it would eat a keyword space
            let last_word_start =
                trimmed_result.rfind(|ch: char| !ch.is_alphanumeric() && ch != '_');
            let last_word = if let Some(pos) = last_word_start {
                &trimmed_result[pos + 1..]
            } else {
                trimmed_result
            };
            let is_keyword = matches!(
                last_word,
                "if" | "while" | "for" | "match" | "return" | "let" | "fn" | "else"
            );
            if !is_keyword {
                while result.ends_with(' ') {
                    result.pop();
                }
            }
            result.push(c);
            continue;
        }

        // Ensure space before `{` (e.g. `else{` -> `else {`, `){` -> `) {`)
        if c == '{' && !result.is_empty() && !result.ends_with(' ') && !result.ends_with('\t') {
            result.push(' ');
            result.push(c);
            continue;
        }

        // Operator spacing: ensure spaces around comparison/logical operators
        // Handle two-char operators first: ==, !=, <=, >=, &&, ||
        if let Some(&next) = chars.peek() {
            let two = format!("{}{}", c, next);
            if matches!(two.as_str(), "==" | "!=" | "<=" | ">=" | "&&" | "||") {
                // Ensure space before
                if !result.is_empty() && !result.ends_with(' ') {
                    result.push(' ');
                }
                result.push(c);
                result.push(next);
                chars.next();
                // Ensure space after
                while chars.peek() == Some(&' ') {
                    chars.next();
                }
                if chars.peek().is_some() {
                    result.push(' ');
                }
                continue;
            }
            // Skip `->` and `=>` (arrows) — don't add operator spacing around them
            if (c == '=' || c == '-') && next == '>' {
                result.push(c);
                result.push(next);
                chars.next();
                continue;
            }
        }

        // Single `=` (assignment): ensure spaces around it.
        // Two-char operators (==, =>) were already handled above with `continue`.
        if c == '=' {
            if !result.is_empty() && !result.ends_with(' ') {
                result.push(' ');
            }
            result.push('=');
            while chars.peek() == Some(&' ') {
                chars.next();
            }
            if chars.peek().is_some() {
                result.push(' ');
            }
            continue;
        }

        // Single-char `<` and `>`: distinguish generics from comparisons.
        // For `<`: if the preceding word starts with an uppercase letter
        // (e.g. `Vec<i64>`, `Option<str>`), treat as generic opener.
        // For `>`: if we're inside a generic (generic_depth > 0), close it.
        if c == '<' && !result.is_empty() {
            // Extract the word immediately before `<`
            let trimmed_res = result.trim_end();
            let word_start = trimmed_res
                .rfind(|ch: char| !ch.is_alphanumeric() && ch != '_')
                .map(|p| p + 1)
                .unwrap_or(0);
            let prev_word = &trimmed_res[word_start..];
            let prev_starts_upper = prev_word
                .chars()
                .next()
                .map(|ch| ch.is_uppercase())
                .unwrap_or(false);

            if prev_starts_upper {
                // Generic type — no spacing, track depth
                generic_depth += 1;
                result.push(c);
                continue;
            }

            // Comparison operator — add spaces
            let prev_char = result.chars().last().unwrap();
            let prev_is_value = prev_char.is_alphanumeric()
                || prev_char == '_'
                || prev_char == ')'
                || prev_char == ']';
            if prev_is_value {
                if !result.ends_with(' ') {
                    result.push(' ');
                }
                result.push(c);
                while chars.peek() == Some(&' ') {
                    chars.next();
                }
                if chars.peek().is_some() {
                    result.push(' ');
                }
                continue;
            }
        }

        if c == '>' && !result.is_empty() {
            if generic_depth > 0 {
                // Closing a generic — no spacing
                generic_depth -= 1;
                result.push(c);
                continue;
            }

            // Comparison operator — add spaces
            let prev_char = result.chars().last().unwrap();
            let prev_is_value = prev_char.is_alphanumeric()
                || prev_char == '_'
                || prev_char == ')'
                || prev_char == ']';
            if prev_is_value {
                if let Some(&next) = chars.peek() {
                    let next_is_value = next.is_alphanumeric()
                        || next == '_'
                        || next == '('
                        || next == '!'
                        || next == '-';
                    if next_is_value || next == ' ' {
                        if !result.ends_with(' ') {
                            result.push(' ');
                        }
                        result.push(c);
                        while chars.peek() == Some(&' ') {
                            chars.next();
                        }
                        if chars.peek().is_some() {
                            result.push(' ');
                        }
                        continue;
                    }
                }
            }
        }

        result.push(c);
    }

    format!("{indent}{result}")
}

/// Reindent lines based on brace-counting (`{` increases, `}` decreases).
/// We track depth and emit 4 spaces per level.
fn reindent(lines: &[String]) -> Vec<String> {
    let mut result = Vec::with_capacity(lines.len());
    let mut depth: i32 = 0;

    for line in lines {
        let stripped = line.trim();

        // Skip empty lines (preserve them as-is for spacing pass)
        if stripped.is_empty() {
            result.push(String::new());
            continue;
        }

        // Lines starting with `}` decrease depth *before* indenting
        let close_first = stripped.starts_with('}');
        if close_first {
            depth -= 1;
            if depth < 0 {
                depth = 0;
            }
        }

        // Build indented line
        let indent = "    ".repeat(depth as usize);
        result.push(format!("{indent}{stripped}"));

        // Count net brace change for *this* line (excluding the leading `}` already handled)
        let net = net_brace_change(stripped, close_first);
        depth += net;
        if depth < 0 {
            depth = 0;
        }
    }

    result
}

/// Count net brace change in a line, skipping braces inside strings and comments.
/// If `skip_leading_close` is true, we skip the first `}` (already accounted for).
fn net_brace_change(line: &str, skip_leading_close: bool) -> i32 {
    let mut net: i32 = 0;
    let mut in_string = false;
    let mut escape_next = false;
    let mut skipped_leading = !skip_leading_close;
    let mut in_line_comment = false;
    let mut chars = line.chars().peekable();

    while let Some(c) = chars.next() {
        if in_line_comment {
            break;
        }
        if escape_next {
            escape_next = false;
            continue;
        }
        if c == '\\' && in_string {
            escape_next = true;
            continue;
        }
        if c == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        // Check for line comment
        if c == '/' && chars.peek() == Some(&'/') {
            in_line_comment = true;
            continue;
        }
        if c == '{' {
            net += 1;
        } else if c == '}' {
            if !skipped_leading {
                skipped_leading = true;
                continue;
            }
            net -= 1;
        }
    }

    net
}

/// Ensure exactly one blank line between top-level items (depth 0 declarations).
fn normalize_top_level_spacing(lines: &[String]) -> Vec<String> {
    let mut result: Vec<String> = Vec::with_capacity(lines.len());

    for (i, line) in lines.iter().enumerate() {
        let stripped = line.trim();

        // If this line is a top-level declaration and it's not the first non-blank content
        if i > 0 && is_top_level_start(stripped) && !line.starts_with("    ") {
            // Remove consecutive blank lines before this, then ensure exactly one
            while result.last().map(|l| l.is_empty()).unwrap_or(false) {
                result.pop();
            }
            // Only add a blank line if there's preceding content
            if !result.is_empty() {
                result.push(String::new());
            }
        }

        // Collapse multiple consecutive blank lines into one
        if stripped.is_empty() && result.last().map(|l| l.is_empty()).unwrap_or(false) {
            continue; // skip duplicate blank line
        }

        result.push(line.clone());
    }

    // Remove leading blank lines
    while result.first().map(|l| l.is_empty()).unwrap_or(false) {
        result.remove(0);
    }

    // Remove trailing blank lines
    while result.last().map(|l| l.is_empty()).unwrap_or(false) {
        result.pop();
    }

    result
}

/// Check if a trimmed line starts a top-level item.
fn is_top_level_start(trimmed: &str) -> bool {
    TOP_LEVEL_KEYWORDS.iter().any(|kw| trimmed.starts_with(kw))
        || trimmed.starts_with("pub ")
            && TOP_LEVEL_KEYWORDS
                .iter()
                .any(|kw| trimmed[4..].starts_with(kw))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trailing_whitespace_removed() {
        let input = "fn main() {   \n    print(\"hello\")  \n}  \n";
        let output = format_source(input);
        for line in output.lines() {
            assert_eq!(line, line.trim_end(), "trailing whitespace found");
        }
    }

    #[test]
    fn test_indentation_normalized() {
        let input = "fn main() {\n  let x = 1\n      let y = 2\n}\n";
        let output = format_source(input);
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines[0], "fn main() {");
        assert_eq!(lines[1], "    let x = 1");
        assert_eq!(lines[2], "    let y = 2");
        assert_eq!(lines[3], "}");
    }

    #[test]
    fn test_blank_lines_between_functions() {
        let input = "fn foo() {\n}\nfn bar() {\n}\n";
        let output = format_source(input);
        let lines: Vec<&str> = output.lines().collect();
        // Should have blank line between the two functions
        assert_eq!(lines[0], "fn foo() {");
        assert_eq!(lines[1], "}");
        assert_eq!(lines[2], "");
        assert_eq!(lines[3], "fn bar() {");
        assert_eq!(lines[4], "}");
    }

    #[test]
    fn test_excessive_blank_lines_collapsed() {
        let input = "fn foo() {\n}\n\n\n\n\nfn bar() {\n}\n";
        let output = format_source(input);
        // Count consecutive blank lines
        let mut max_consecutive = 0;
        let mut current = 0;
        for line in output.lines() {
            if line.is_empty() {
                current += 1;
                if current > max_consecutive {
                    max_consecutive = current;
                }
            } else {
                current = 0;
            }
        }
        assert!(
            max_consecutive <= 1,
            "found {max_consecutive} consecutive blank lines"
        );
    }

    #[test]
    fn test_nested_braces() {
        let input = "fn main() {\nif true {\nlet x = 1\n}\n}\n";
        let output = format_source(input);
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines[0], "fn main() {");
        assert_eq!(lines[1], "    if true {");
        assert_eq!(lines[2], "        let x = 1");
        assert_eq!(lines[3], "    }");
        assert_eq!(lines[4], "}");
    }

    #[test]
    fn test_braces_in_strings_ignored() {
        let input = "fn main() {\nprint(\"{hello}\")\n}\n";
        let output = format_source(input);
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines[0], "fn main() {");
        assert_eq!(lines[1], "    print(\"{hello}\")");
        assert_eq!(lines[2], "}");
    }

    #[test]
    fn test_file_ends_with_single_newline() {
        let input = "fn main() {\n}\n\n\n";
        let output = format_source(input);
        assert!(output.ends_with('\n'));
        assert!(!output.ends_with("\n\n"));
    }

    #[test]
    fn test_already_formatted() {
        let input = "fn main() {\n    print(\"hello\")\n}\n";
        let output = format_source(input);
        assert_eq!(input, output);
    }

    #[test]
    fn test_messy_file_full() {
        let input = r#"fn add(a: i64,    b: i64) -> i64 {
      a + b
}
fn main() {
  let result = add(3, 7)
    print(result)
}
"#;
        let output = format_source(input);
        let expected = r#"fn add(a: i64, b: i64) -> i64 {
    a + b
}

fn main() {
    let result = add(3, 7)
    print(result)
}
"#;
        assert_eq!(output, expected);
    }

    #[test]
    fn test_spacing_in_function_signatures() {
        let input = "fn    main(    ) {\n    print(\"hello\")\n}\n";
        let output = format_source(input);
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines[0], "fn main() {");
    }

    #[test]
    fn test_spacing_after_commas() {
        let input = "fn main() {\n    let arr = [1,2,3]\n}\n";
        let output = format_source(input);
        assert!(output.contains("[1, 2, 3]"));
    }

    #[test]
    fn test_spacing_preserves_strings() {
        let input = "fn main() {\n    print(\"hello    world\")\n}\n";
        let output = format_source(input);
        assert!(output.contains("\"hello    world\""));
    }

    #[test]
    fn test_brace_spacing_after_else() {
        let input = "fn main() {\n    if true {\n        1\n    } else{\n        2\n    }\n}\n";
        let output = format_source(input);
        assert!(
            output.contains("} else {"),
            "Expected `else {{`, got:\n{}",
            output
        );
    }

    #[test]
    fn test_brace_spacing_after_paren() {
        let input = "fn main(){\n    print(\"hi\")\n}\n";
        let output = format_source(input);
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines[0], "fn main() {");
    }

    #[test]
    fn test_brace_spacing_preserves_existing() {
        // Already correct spacing should be preserved
        let input = "fn main() {\n    if x > 10 {\n        print(x)\n    }\n}\n";
        let output = format_source(input);
        assert_eq!(input, output);
    }

    #[test]
    fn test_operator_spacing_comparison() {
        let input = "fn main() {\n    if x>10 {\n        print(x)\n    }\n}\n";
        let output = format_source(input);
        assert!(
            output.contains("if x > 10 {"),
            "Expected `x > 10`, got:\n{}",
            output
        );
    }

    #[test]
    fn test_operator_spacing_less_than() {
        let input = "fn main() {\n    if x<10 {\n        print(x)\n    }\n}\n";
        let output = format_source(input);
        assert!(
            output.contains("if x < 10 {"),
            "Expected `x < 10`, got:\n{}",
            output
        );
    }

    #[test]
    fn test_operator_spacing_double_equals() {
        let input = "fn main() {\n    if x==10 {\n        print(x)\n    }\n}\n";
        let output = format_source(input);
        assert!(
            output.contains("if x == 10 {"),
            "Expected `x == 10`, got:\n{}",
            output
        );
    }

    #[test]
    fn test_operator_spacing_not_equals() {
        let input = "fn main() {\n    if x!=10 {\n        print(x)\n    }\n}\n";
        let output = format_source(input);
        assert!(
            output.contains("if x != 10 {"),
            "Expected `x != 10`, got:\n{}",
            output
        );
    }

    #[test]
    fn test_operator_spacing_and_or() {
        let input = "fn main() {\n    if a&&b||c {\n        print(x)\n    }\n}\n";
        let output = format_source(input);
        assert!(
            output.contains("a && b || c"),
            "Expected `a && b || c`, got:\n{}",
            output
        );
    }

    #[test]
    fn test_operator_spacing_preserves_arrows() {
        // `->` and `=>` should not be broken apart by operator spacing
        let input = "fn add(a: i64) -> i64 {\n    a\n}\n";
        let output = format_source(input);
        assert!(
            output.contains("-> i64"),
            "Arrow should be preserved, got:\n{}",
            output
        );
    }

    #[test]
    fn test_operator_spacing_preserves_generics() {
        // `<` and `>` in generics should not get extra spaces
        let input = "fn main() {\n    let x: Vec<i64> = []\n}\n";
        let output = format_source(input);
        assert!(
            output.contains("Vec<i64>"),
            "Generics should be preserved, got:\n{}",
            output
        );
    }

    #[test]
    fn test_operator_spacing_in_strings_preserved() {
        let input = "fn main() {\n    print(\"x>10\")\n}\n";
        let output = format_source(input);
        assert!(
            output.contains("\"x>10\""),
            "Operators inside strings should be preserved, got:\n{}",
            output
        );
    }
}
