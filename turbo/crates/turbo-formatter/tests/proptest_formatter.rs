//! Property-based tests for the Turbo source formatter (`format_source`).
//!
//! Example-based tests live in `src/tests.rs`; these complement them by
//! generating a wide space of *valid* TurboLang programs and asserting the
//! formatter's core invariants hold across all of them:
//!
//! 1. **Idempotence** — `format(format(x)) == format(x)`.
//! 2. **Parse stability** — `format(x)` parses with zero parse errors.
//! 3. **Never panics** — `format` on arbitrary/garbage input must return, never
//!    panic.
//! 4. **Semantic preservation** — `parse(x)` and `parse(format(x))` have the
//!    same top-level items (count + names + kinds).
//!
//! The valid-program generators reuse the parser proptest suite's approach
//! (see `turbo-parser/tests/proptest_parser.rs`): build source strings from
//! keyword-avoiding identifiers so the lexer/parser accept them cleanly, and
//! seed a handful of richer program shapes (functions with params/returns,
//! if/while, structs) rather than a single let-only template.

use proptest::prelude::*;
use turbo_ast::{Item, Module};
use turbo_formatter::format_source;

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Returns true if `name` is a Turbo keyword that cannot be used as an
/// identifier. Kept in sync with the parser proptest suite's list.
fn is_keyword(name: &str) -> bool {
    matches!(
        name,
        "fn" | "let"
            | "mut"
            | "if"
            | "else"
            | "while"
            | "for"
            | "in"
            | "return"
            | "match"
            | "struct"
            | "enum"
            | "true"
            | "false"
            | "import"
            | "from"
            | "const"
            | "impl"
            | "trait"
            | "pub"
            | "defer"
            | "spawn"
            | "await"
            | "async"
            | "break"
            | "continue"
            | "none"
            | "some"
            | "ok"
            | "err"
            | "int"
            | "float"
            | "bool"
            | "str"
            | "extern"
            | "type"
            | "unsafe"
            | "as"
            | "self"
    )
}

/// Parse a source string, returning the `Module` only if there were zero lex
/// and parse errors. Used by the semantic-preservation property.
fn parse_clean(source: &str) -> Option<Module> {
    let (tokens, lex_errors) = turbo_lexer::tokenize(source);
    if !lex_errors.is_empty() {
        return None;
    }
    let (module, parse_errors) = turbo_parser::parse(tokens);
    if !parse_errors.is_empty() {
        return None;
    }
    Some(module)
}

/// A stable, span-free fingerprint of a top-level item: its kind + name. Two
/// programs with the same ordered vector of fingerprints declare the same
/// top-level surface, which is what "semantic preservation" checks without
/// requiring full-body AST equality plumbing.
fn item_fingerprint(item: &Item) -> String {
    match item {
        Item::Function(f) => format!("fn:{}", f.name),
        Item::Struct(s) => format!("struct:{}", s.name),
        Item::Enum(e) => format!("enum:{}", e.name),
        Item::Impl(i) => format!("impl:{}", i.type_name),
        Item::Trait(t) => format!("trait:{}", t.name),
        Item::Import { path, .. } => format!("import:{path}"),
        Item::Const(c) => format!("const:{}", c.name),
        Item::Extern(e) => format!("extern:{}", e.abi),
    }
}

fn item_fingerprints(module: &Module) -> Vec<String> {
    module
        .items
        .iter()
        .map(|spanned| item_fingerprint(&spanned.node))
        .collect()
}

// ---------------------------------------------------------------------------
// Valid-program generators
//
// Each strategy yields a syntactically valid TurboLang program as a String.
// They deliberately keep bodies simple so the generated space stays
// maintainable while still exercising a broad slice of item/statement shapes.
// ---------------------------------------------------------------------------

/// A single lowercase identifier that is guaranteed not to collide with a
/// keyword.
fn arb_ident() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_]{0,6}".prop_filter("avoid keywords", |s| !is_keyword(s))
}

/// A capitalized type/struct name (never a keyword — capitalization avoids the
/// lowercase keyword set entirely).
fn arb_type_name() -> impl Strategy<Value = String> {
    "[A-Z][a-zA-Z0-9]{0,6}"
}

/// One of the primitive type keywords usable in signatures.
fn arb_prim_type() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("int".to_string()),
        Just("float".to_string()),
        Just("bool".to_string()),
        Just("str".to_string()),
    ]
}

/// `fn <name>() { let <v> = <int> }`
fn arb_let_fn() -> impl Strategy<Value = String> {
    (arb_ident(), arb_ident(), 0i64..10_000)
        .prop_map(|(fname, var, val)| format!("fn {fname}() {{ let {var} = {val} }}"))
}

/// `fn <name>(a: int, b: int) -> int { a + b }`
fn arb_binop_fn() -> impl Strategy<Value = String> {
    (arb_ident(), arb_prim_type()).prop_map(|(fname, _ty)| {
        // Keep the body int-typed so it always type-checks-shaped for the parser;
        // sema isn't run here, only parse stability matters.
        format!("fn {fname}(a: int, b: int) -> int {{ a + b }}")
    })
}

/// `fn <name>() { if true { <int> } else { <int> } }`
fn arb_if_fn() -> impl Strategy<Value = String> {
    (arb_ident(), 0i64..1000, 0i64..1000)
        .prop_map(|(fname, a, b)| format!("fn {fname}() {{ if true {{ {a} }} else {{ {b} }} }}"))
}

/// `fn <name>() { let mut x = <n>  while x > 0 { x = x - 1 } }`
fn arb_while_fn() -> impl Strategy<Value = String> {
    (arb_ident(), 1i64..50).prop_map(|(fname, start)| {
        format!("fn {fname}() {{ let mut x = {start} \n while x > 0 {{ x = x - 1 }} }}")
    })
}

/// `struct <Name> { <f>: int, <g>: str }`
fn arb_struct_item() -> impl Strategy<Value = String> {
    (arb_type_name(), arb_ident(), arb_ident(), arb_prim_type()).prop_map(|(name, f1, f2, ty)| {
        // Ensure the two field names differ so the struct is well-formed.
        let f2 = if f1 == f2 { format!("{f2}_2") } else { f2 };
        format!("struct {name} {{ {f1}: int, {f2}: {ty} }}")
    })
}

/// `const <NAME> = <int>`
fn arb_const_item() -> impl Strategy<Value = String> {
    (arb_ident(), 0i64..10_000).prop_map(|(name, val)| format!("const {name} = {val}"))
}

/// One valid top-level item drawn from the shapes above.
fn arb_item() -> impl Strategy<Value = String> {
    prop_oneof![
        arb_let_fn(),
        arb_binop_fn(),
        arb_if_fn(),
        arb_while_fn(),
        arb_struct_item(),
        arb_const_item(),
    ]
}

/// A whole valid program: 1..=6 items joined by messy whitespace so the
/// formatter has real work to do (collapsing blanks, re-indenting, spacing).
fn arb_valid_program() -> impl Strategy<Value = String> {
    proptest::collection::vec(arb_item(), 1..6).prop_map(|items| {
        // Interleave irregular separators to stress the formatter.
        items.join("\n\n\n")
    })
}

// ---------------------------------------------------------------------------
// Garbage-input generators (for the never-panics property)
// ---------------------------------------------------------------------------

/// Random "token soup": Turbo-ish fragments joined by spaces. Mostly invalid
/// programs, exercising the formatter's error-recovery / safety-gate path.
fn arb_token_soup() -> impl Strategy<Value = String> {
    let fragment = prop_oneof![
        Just("fn".to_string()),
        Just("let".to_string()),
        Just("mut".to_string()),
        Just("if".to_string()),
        Just("else".to_string()),
        Just("while".to_string()),
        Just("struct".to_string()),
        Just("return".to_string()),
        Just("match".to_string()),
        Just("{".to_string()),
        Just("}".to_string()),
        Just("(".to_string()),
        Just(")".to_string()),
        Just("[".to_string()),
        Just("]".to_string()),
        Just("+".to_string()),
        Just("-".to_string()),
        Just("*".to_string()),
        Just("/".to_string()),
        Just("=".to_string()),
        Just("==".to_string()),
        Just("->".to_string()),
        Just("=>".to_string()),
        Just(":".to_string()),
        Just(",".to_string()),
        Just(".".to_string()),
        Just("\"unterminated".to_string()),
        Just("r\"raw".to_string()),
        Just("// comment".to_string()),
        Just("/* block".to_string()),
        Just("\"{interp}\"".to_string()),
        "[a-z]{1,6}".prop_map(|s| s),
        (0i64..1000).prop_map(|n| n.to_string()),
        Just("\n".to_string()),
        Just("\t".to_string()),
    ];
    proptest::collection::vec(fragment, 0..60).prop_map(|f| f.join(" "))
}

// ---------------------------------------------------------------------------
// Property 1: Idempotence
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    #[test]
    fn format_is_idempotent(source in arb_valid_program()) {
        let once = format_source(&source);
        let twice = format_source(&once);
        prop_assert_eq!(
            &once, &twice,
            "format is not idempotent.\n--- source ---\n{}\n--- once ---\n{}\n--- twice ---\n{}",
            source, once, twice
        );
    }
}

// ---------------------------------------------------------------------------
// Property 2: Parse stability — formatted output parses with zero errors
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    #[test]
    fn formatted_output_parses_clean(source in arb_valid_program()) {
        // The generated program itself must parse (guards against a bad
        // generator masking a real formatter bug).
        prop_assume!(parse_clean(&source).is_some());

        let formatted = format_source(&source);
        let (tokens, lex_errors) = turbo_lexer::tokenize(&formatted);
        prop_assert!(
            lex_errors.is_empty(),
            "formatted output has lex errors: {:?}\n--- source ---\n{}\n--- formatted ---\n{}",
            lex_errors, source, formatted
        );
        let (_module, parse_errors) = turbo_parser::parse(tokens);
        prop_assert!(
            parse_errors.is_empty(),
            "formatted output has parse errors: {:?}\n--- source ---\n{}\n--- formatted ---\n{}",
            parse_errors, source, formatted
        );
    }
}

// ---------------------------------------------------------------------------
// Property 3: Never panics on arbitrary input
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    #[test]
    fn format_never_panics_on_garbage(source in ".*") {
        // Must return (any string), never panic.
        let _ = format_source(&source);
    }

    #[test]
    fn format_never_panics_on_token_soup(source in arb_token_soup()) {
        let _ = format_source(&source);
    }

    #[test]
    fn format_never_panics_on_unicode(
        source in "[\\u{0000}-\\u{10FFFF}]{0,80}"
    ) {
        let _ = format_source(&source);
    }
}

// ---------------------------------------------------------------------------
// Property 4: Semantic preservation — same top-level items before and after
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    #[test]
    fn format_preserves_top_level_items(source in arb_valid_program()) {
        let before = match parse_clean(&source) {
            Some(m) => m,
            // Skip inputs the generator produced that don't parse cleanly; the
            // parse-stability property covers formatter-introduced breakage.
            None => return Ok(()),
        };

        let formatted = format_source(&source);
        let after = match parse_clean(&formatted) {
            Some(m) => m,
            None => {
                return Err(TestCaseError::fail(format!(
                    "formatted output no longer parses cleanly.\n--- source ---\n{source}\n--- formatted ---\n{formatted}"
                )));
            }
        };

        let fp_before = item_fingerprints(&before);
        let fp_after = item_fingerprints(&after);
        prop_assert_eq!(
            &fp_before, &fp_after,
            "top-level items changed across formatting.\n--- source ---\n{}\n--- formatted ---\n{}\nbefore: {:?}\nafter:  {:?}",
            source, formatted, fp_before, fp_after
        );
    }
}
