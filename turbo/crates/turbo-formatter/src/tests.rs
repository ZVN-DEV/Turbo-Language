use super::*;

/// Corpus of small Turbo programs exercising a broad slice of the grammar.
/// Used to assert idempotency and parse-stability invariants in bulk.
const CORPUS: &[&str] = &[
    "fn main() {\n    print(\"Hello, world!\")\n}\n",
    "fn add(a: i64, b: i64) -> i64 {\n    a + b\n}\n",
    "fn main() {\n    let x = 1 + 2 * 3\n    print(x)\n}\n",
    "fn main() {\n    if x > 10 {\n        print(x)\n    } else {\n        print(0)\n    }\n}\n",
    "fn main() {\n    for i in 0..10 {\n        print(i)\n    }\n}\n",
    "fn main() {\n    let mut n = 0\n    while n < 5 {\n        n = n + 1\n    }\n}\n",
    "struct Point {\n    x: i64,\n    y: i64,\n}\n",
    "type Shape {\n    Circle(f64),\n    Rectangle(f64, f64),\n}\n",
    "fn area(s: Shape) -> f64 {\n    match s {\n        Circle(r) => 3.14 * r * r\n        Rectangle(w, h) => w * h\n    }\n}\n",
    "impl Counter {\n    fn increment(self) -> Counter {\n        Counter { count: self.count + 1 }\n    }\n}\n",
    "fn main() {\n    let p = Point { x: 1, y: 2 }\n    print(p.x)\n}\n",
    "fn main() {\n    let xs = [1, 2, 3]\n    print(xs[0])\n}\n",
    "fn main() {\n    let double = |x: i64| -> i64 { x * 2 }\n    print(double(5))\n}\n",
    "fn main() {\n    let c = Counter { count: 0 }\n    c = c.increment()\n}\n",
    "fn main() {\n    let mut xs = [1, 2, 3]\n    xs.push(4)\n    print(len(xs))\n}\n",
    "fn main() {\n    print(\"x = {x}, y = {y}\")\n}\n",
    "fn main() {\n    let a = -5\n    let b = !true\n    let c = (1 + 2) * 3\n}\n",
    "@derive(Eq, Clone)\nstruct Pair {\n    a: i64,\n    b: i64,\n}\n",
    "import { sqrt, pow } from \"math\"\n\nfn main() {\n    print(sqrt(4.0))\n}\n",
    "// top comment\nfn main() {\n    // inner comment\n    let x = 1 // trailing\n    print(x)\n}\n",
    "fn main() {\n    let r = if c { 1 } else { 2 }\n    print(r)\n}\n",
    "@test fn test_math() {\n    assert(1 + 1 == 2, \"math\")\n}\n",
    "fn main() {\n    let s = \"hello\\nworld\"\n    print(s)\n}\n",
];

fn assert_idempotent(input: &str) {
    let once = format_source(input);
    let twice = format_source(&once);
    assert_eq!(
        once, twice,
        "format is not idempotent for input:\n{input}\n--- once ---\n{once}\n--- twice ---\n{twice}"
    );
}

#[test]
fn corpus_is_idempotent() {
    for input in CORPUS {
        assert_idempotent(input);
        // Also: formatting the *formatted* form is a no-op.
        let formatted = format_source(input);
        assert_eq!(format_source(&formatted), formatted);
    }
}

#[test]
fn corpus_is_parse_stable() {
    // parse(format(x)) must equal parse(x) modulo spans.
    for input in CORPUS {
        let formatted = format_source(input);
        assert!(
            ast_equivalent(input, &formatted),
            "parse(format(x)) != parse(x) for:\n{input}\n--- got ---\n{formatted}"
        );
    }
}

#[test]
fn formatting_preserves_comments_in_corpus() {
    for input in CORPUS {
        let formatted = format_source(input);
        assert!(
            comments_preserved(input, &formatted),
            "comments not preserved for:\n{input}\n--- got ---\n{formatted}"
        );
    }
}

#[test]
fn refuses_unparseable_input() {
    let bad = "fn main( {\n    print(\n}\n";
    assert_eq!(
        format_source(bad),
        bad,
        "unparseable input must be returned unchanged"
    );
}

#[test]
fn block_comments_left_untouched() {
    // Block comments cannot be round-tripped, so the file is returned unchanged.
    let input = "fn main() {\n    /* block */\n    let x=1\n}\n";
    assert_eq!(format_source(input), input);
}

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
    let mut max_consecutive = 0;
    let mut current = 0;
    for line in output.lines() {
        if line.is_empty() {
            current += 1;
            max_consecutive = max_consecutive.max(current);
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
fn test_nested_braces_expanded() {
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
fn test_braces_in_interpolation_preserved() {
    let input = "fn main() {\nprint(\"{hello}\")\n}\n";
    let output = format_source(input);
    assert!(output.contains("print(\"{hello}\")"), "got:\n{output}");
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
    assert_eq!(format_source(input), input);
}

#[test]
fn test_messy_file_full() {
    let input = "fn add(a: i64,    b: i64) -> i64 {\n      a + b\n}\nfn main() {\n  let result = add(3, 7)\n    print(result)\n}\n";
    let output = format_source(input);
    let expected = "fn add(a: i64, b: i64) -> i64 {\n    a + b\n}\n\nfn main() {\n    let result = add(3, 7)\n    print(result)\n}\n";
    assert_eq!(output, expected);
}

#[test]
fn test_spacing_in_function_signatures() {
    let input = "fn    main(    ) {\n    print(\"hello\")\n}\n";
    let output = format_source(input);
    assert_eq!(output.lines().next(), Some("fn main() {"));
}

#[test]
fn test_spacing_after_commas() {
    let input = "fn main() {\n    let arr = [1,2,3]\n}\n";
    let output = format_source(input);
    assert!(output.contains("[1, 2, 3]"), "got:\n{output}");
}

#[test]
fn test_spacing_preserves_strings() {
    let input = "fn main() {\n    print(\"hello    world\")\n}\n";
    let output = format_source(input);
    assert!(output.contains("\"hello    world\""), "got:\n{output}");
}

#[test]
fn test_brace_spacing_after_else() {
    let input = "fn main() {\n    if true {\n        1\n    } else{\n        2\n    }\n}\n";
    let output = format_source(input);
    assert!(output.contains("} else {"), "got:\n{output}");
}

#[test]
fn test_brace_spacing_after_paren() {
    let input = "fn main(){\n    print(\"hi\")\n}\n";
    let output = format_source(input);
    assert_eq!(output.lines().next(), Some("fn main() {"));
}

#[test]
fn test_brace_spacing_preserves_existing() {
    let input = "fn main() {\n    if x > 10 {\n        print(x)\n    }\n}\n";
    assert_eq!(format_source(input), input);
}

#[test]
fn test_operator_spacing_comparison() {
    let input = "fn main() {\n    if x>10 {\n        print(x)\n    }\n}\n";
    let output = format_source(input);
    assert!(output.contains("if x > 10 {"), "got:\n{output}");
}

#[test]
fn test_operator_spacing_less_than() {
    let input = "fn main() {\n    if x<10 {\n        print(x)\n    }\n}\n";
    let output = format_source(input);
    assert!(output.contains("if x < 10 {"), "got:\n{output}");
}

#[test]
fn test_operator_spacing_double_equals() {
    let input = "fn main() {\n    if x==10 {\n        print(x)\n    }\n}\n";
    let output = format_source(input);
    assert!(output.contains("if x == 10 {"), "got:\n{output}");
}

#[test]
fn test_operator_spacing_not_equals() {
    let input = "fn main() {\n    if x!=10 {\n        print(x)\n    }\n}\n";
    let output = format_source(input);
    assert!(output.contains("if x != 10 {"), "got:\n{output}");
}

#[test]
fn test_operator_spacing_and_or() {
    let input = "fn main() {\n    if a&&b||c {\n        print(x)\n    }\n}\n";
    let output = format_source(input);
    assert!(output.contains("a && b || c"), "got:\n{output}");
}

#[test]
fn test_arithmetic_operators_spaced() {
    // The headline regression the old line-based formatter could not fix.
    let input = "fn main() {\n    let z = a+b*c-d\n}\n";
    let output = format_source(input);
    assert!(output.contains("let z = a + b * c - d"), "got:\n{output}");
}

#[test]
fn test_colon_in_params_spaced() {
    let input = "fn f(a:i64, b:str) -> i64 {\n    a\n}\n";
    let output = format_source(input);
    assert!(
        output.contains("fn f(a: i64, b: str) -> i64 {"),
        "got:\n{output}"
    );
}

#[test]
fn test_return_arrow_spaced() {
    let input = "fn f()->i64 {\n    1\n}\n";
    let output = format_source(input);
    assert!(output.contains("fn f() -> i64 {"), "got:\n{output}");
}

#[test]
fn test_operator_spacing_preserves_arrows() {
    let input = "fn add(a: i64) -> i64 {\n    a\n}\n";
    let output = format_source(input);
    assert!(output.contains("-> i64"), "got:\n{output}");
}

#[test]
fn test_operator_spacing_in_strings_preserved() {
    let input = "fn main() {\n    print(\"x>10\")\n}\n";
    let output = format_source(input);
    assert!(output.contains("\"x>10\""), "got:\n{output}");
}

#[test]
fn test_string_literals_with_special_chars_preserved() {
    let input = "fn main() {\n    let s = \"hello\\nworld\\t!\"\n}\n";
    let output = format_source(input);
    assert!(output.contains("\"hello\\nworld\\t!\""), "got:\n{output}");
}

#[test]
fn test_assignment_operator_spacing() {
    let input = "fn main() {\n    let x=42\n}\n";
    let output = format_source(input);
    assert!(output.contains("let x = 42"), "got:\n{output}");
}

#[test]
fn test_comments_preserved_during_formatting() {
    let input =
        "fn main() {\n    // This is a comment with special chars: <>, ==, {}\n    let x = 1\n}\n";
    let output = format_source(input);
    assert!(
        output.contains("// This is a comment with special chars: <>, ==, {}"),
        "got:\n{output}"
    );
}

#[test]
fn test_inline_if_expanded() {
    // AC#5: inline blocks must be expanded onto their own lines.
    let input = "fn main() {\n    if c { break }\n}\n";
    let output = format_source(input);
    let expected = "fn main() {\n    if c {\n        break\n    }\n}\n";
    assert_eq!(output, expected);
}

#[test]
fn test_method_call_roundtrips() {
    let input = "fn main() {\n    let c = Counter { count: 0 }\n    c = c.increment()\n}\n";
    let output = format_source(input);
    assert!(output.contains("c = c.increment()"), "got:\n{output}");
}

#[test]
fn test_cow_push_roundtrips() {
    let input = "fn main() {\n    let mut xs = [1, 2, 3]\n    xs.push(4)\n}\n";
    let output = format_source(input);
    assert!(output.contains("xs.push(4)"), "got:\n{output}");
}
