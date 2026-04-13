//! Property-based tests for the Turbo parser using proptest.
//!
//! These tests verify three key invariants:
//!
//! 1. The parser never panics on any token sequence (it may return errors).
//! 2. Valid simple programs parse without errors.
//! 3. The tokenize -> parse pipeline never panics on arbitrary strings.

use proptest::prelude::*;
use turbo_lexer::{Spanned, Token};

/// Returns true if `name` is a Turbo keyword that cannot be used as an identifier.
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
    )
}

// ---------------------------------------------------------------------------
// Strategy: generate a random Token
// ---------------------------------------------------------------------------

fn arb_token() -> impl Strategy<Value = Token> {
    prop_oneof![
        // Identifiers — restricted to ASCII letters to avoid accidental keywords
        "[a-zA-Z_][a-zA-Z0-9_]{0,8}".prop_map(Token::Ident),
        // Integer literals
        any::<i64>().prop_map(Token::Int),
        // Float literals (as string representation — the Token::Float stores a String)
        (1.0f64..1e6)
            .prop_map(|f| format!("{:.2}", f))
            .prop_map(Token::Float),
        // String literals
        "[a-zA-Z0-9 _!@#$%^&*]{0,20}".prop_map(Token::String),
        // Boolean-ish keyword tokens
        Just(Token::True),
        Just(Token::False),
        // Operators
        Just(Token::Plus),
        Just(Token::Minus),
        Just(Token::Star),
        Just(Token::Slash),
        Just(Token::Percent),
        Just(Token::Eq),
        Just(Token::EqEq),
        Just(Token::NotEq),
        Just(Token::Less),
        Just(Token::Greater),
        Just(Token::LessEq),
        Just(Token::GreaterEq),
        Just(Token::And),
        Just(Token::Or),
        Just(Token::Bang),
        // Delimiters
        Just(Token::LParen),
        Just(Token::RParen),
        Just(Token::LBrace),
        Just(Token::RBrace),
        Just(Token::LBracket),
        Just(Token::RBracket),
        // Punctuation
        Just(Token::Comma),
        Just(Token::Colon),
        Just(Token::Semi),
        Just(Token::Arrow),
        Just(Token::FatArrow),
        Just(Token::Dot),
        Just(Token::DotDot),
        Just(Token::Question),
        Just(Token::QuestionDot),
        Just(Token::Pipe),
        Just(Token::Bar),
        Just(Token::At),
        // Compound assignment
        Just(Token::PlusEq),
        Just(Token::MinusEq),
        Just(Token::StarEq),
        Just(Token::SlashEq),
        // Keywords
        Just(Token::Fn),
        Just(Token::Let),
        Just(Token::Mut),
        Just(Token::If),
        Just(Token::Else),
        Just(Token::While),
        Just(Token::For),
        Just(Token::In),
        Just(Token::Return),
        Just(Token::Struct),
        Just(Token::Match),
        Just(Token::Import),
        Just(Token::From),
        Just(Token::Const),
        Just(Token::Impl),
        Just(Token::Trait),
        Just(Token::Pub),
        Just(Token::Defer),
        Just(Token::Spawn),
        Just(Token::Await),
        Just(Token::Async),
        Just(Token::Break),
        Just(Token::Continue),
        Just(Token::Newline),
        // Result/Option
        Just(Token::None),
        Just(Token::Some),
        Just(Token::Ok),
        Just(Token::Err),
        Just(Token::QuestionQuestion),
        Just(Token::Extern),
        Just(Token::TypeKw),
    ]
}

/// Wrap a token in a Spanned with a fake span.
fn arb_spanned_token() -> impl Strategy<Value = Spanned<Token>> {
    arb_token().prop_map(|tok| Spanned::new(tok, 0..1))
}

/// Generate a Vec<Spanned<Token>> of bounded length.
fn arb_token_stream(max_len: usize) -> impl Strategy<Value = Vec<Spanned<Token>>> {
    proptest::collection::vec(arb_spanned_token(), 0..max_len)
}

// ---------------------------------------------------------------------------
// Invariant 1: Parser never panics on any token sequence
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    #[test]
    fn parser_never_panics_on_random_tokens(tokens in arb_token_stream(100)) {
        // The parser may return errors but must never panic.
        let (_module, _errors) = turbo_parser::parse(tokens);
    }
}

// ---------------------------------------------------------------------------
// Invariant 2: Valid simple programs always parse successfully
// ---------------------------------------------------------------------------

/// Strategy that produces a valid Turbo let-binding source string:
///   fn main() { let <name> = <int> }
fn arb_valid_let_program() -> impl Strategy<Value = String> {
    ("[a-z][a-z0-9_]{0,6}", 0i64..10_000)
        .prop_filter("avoid keywords", |(name, _)| !is_keyword(name))
        .prop_map(|(name, val)| format!("fn main() {{ let {} = {} }}", name, val))
}

/// Strategy that produces a valid Turbo if-else source string:
///   fn main() { if true { <int> } else { <int> } }
fn arb_valid_if_else_program() -> impl Strategy<Value = String> {
    (0i64..10_000, 0i64..10_000)
        .prop_map(|(a, b)| format!("fn main() {{ if true {{ {} }} else {{ {} }} }}", a, b))
}

/// Strategy that produces a valid Turbo while-loop source string:
///   fn main() { let mut x = <int>  while x > 0 { x = x - 1 } }
fn arb_valid_while_program() -> impl Strategy<Value = String> {
    (1i64..100).prop_map(|start| {
        format!(
            "fn main() {{ let mut x = {} \n while x > 0 {{ x = x - 1 }} }}",
            start
        )
    })
}

/// Strategy that produces a valid Turbo function with return type:
///   fn add(a: int, b: int) -> int { a + b }
fn arb_valid_fn_with_return() -> impl Strategy<Value = String> {
    ("[a-z][a-z]{0,4}",)
        .prop_filter("avoid keywords", |(name,)| !is_keyword(name))
        .prop_map(|(name,)| format!("fn {}(a: int, b: int) -> int {{ a + b }}", name))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn valid_let_programs_parse_ok(source in arb_valid_let_program()) {
        let (tokens, lex_errors) = turbo_lexer::tokenize(&source);
        prop_assert!(lex_errors.is_empty(), "Lex errors: {:?}", lex_errors);

        let (_module, parse_errors) = turbo_parser::parse(tokens);
        prop_assert!(
            parse_errors.is_empty(),
            "Parse errors for `{}`: {:?}",
            source,
            parse_errors
        );
    }

    #[test]
    fn valid_if_else_programs_parse_ok(source in arb_valid_if_else_program()) {
        let (tokens, lex_errors) = turbo_lexer::tokenize(&source);
        prop_assert!(lex_errors.is_empty(), "Lex errors: {:?}", lex_errors);

        let (_module, parse_errors) = turbo_parser::parse(tokens);
        prop_assert!(
            parse_errors.is_empty(),
            "Parse errors for `{}`: {:?}",
            source,
            parse_errors
        );
    }

    #[test]
    fn valid_while_programs_parse_ok(source in arb_valid_while_program()) {
        let (tokens, lex_errors) = turbo_lexer::tokenize(&source);
        prop_assert!(lex_errors.is_empty(), "Lex errors: {:?}", lex_errors);

        let (_module, parse_errors) = turbo_parser::parse(tokens);
        prop_assert!(
            parse_errors.is_empty(),
            "Parse errors for `{}`: {:?}",
            source,
            parse_errors
        );
    }

    #[test]
    fn valid_fn_with_return_parses_ok(source in arb_valid_fn_with_return()) {
        let (tokens, lex_errors) = turbo_lexer::tokenize(&source);
        prop_assert!(lex_errors.is_empty(), "Lex errors: {:?}", lex_errors);

        let (_module, parse_errors) = turbo_parser::parse(tokens);
        prop_assert!(
            parse_errors.is_empty(),
            "Parse errors for `{}`: {:?}",
            source,
            parse_errors
        );
    }
}

// ---------------------------------------------------------------------------
// Invariant 3: Roundtrip — tokenize then parse never panics on any string
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    #[test]
    fn tokenize_then_parse_never_panics(source in ".*") {
        let (tokens, _lex_errors) = turbo_lexer::tokenize(&source);
        // Parser must not panic even on nonsensical token streams from garbage
        // input. Errors are fine.
        let (_module, _errors) = turbo_parser::parse(tokens);
    }

    #[test]
    fn tokenize_then_parse_never_panics_ascii(source in "[a-zA-Z0-9 \\t\\n{}()\\[\\];:=+\\-*/<>!,.?|&@\"']{0,200}") {
        let (tokens, _lex_errors) = turbo_lexer::tokenize(&source);
        let (_module, _errors) = turbo_parser::parse(tokens);
    }

    #[test]
    fn tokenize_then_parse_turbo_like_source(source in arb_turbo_like_source()) {
        let (tokens, _lex_errors) = turbo_lexer::tokenize(&source);
        let (_module, _errors) = turbo_parser::parse(tokens);
    }
}

/// Strategy that generates strings that look more like Turbo source code —
/// random sequences of keywords, identifiers, operators, and delimiters
/// separated by whitespace.
fn arb_turbo_like_source() -> impl Strategy<Value = String> {
    let fragment = prop_oneof![
        Just("fn".to_string()),
        Just("let".to_string()),
        Just("mut".to_string()),
        Just("if".to_string()),
        Just("else".to_string()),
        Just("while".to_string()),
        Just("for".to_string()),
        Just("in".to_string()),
        Just("return".to_string()),
        Just("match".to_string()),
        Just("struct".to_string()),
        Just("enum".to_string()),
        Just("true".to_string()),
        Just("false".to_string()),
        Just("import".to_string()),
        Just("from".to_string()),
        Just("const".to_string()),
        Just("impl".to_string()),
        Just("trait".to_string()),
        Just("pub".to_string()),
        Just("async".to_string()),
        Just("await".to_string()),
        Just("spawn".to_string()),
        Just("defer".to_string()),
        Just("break".to_string()),
        Just("continue".to_string()),
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
        Just("!=".to_string()),
        Just("->".to_string()),
        Just("=>".to_string()),
        Just(":".to_string()),
        Just(",".to_string()),
        Just(".".to_string()),
        Just("..".to_string()),
        Just("|".to_string()),
        Just("&&".to_string()),
        Just("||".to_string()),
        Just("<".to_string()),
        Just(">".to_string()),
        Just("!".to_string()),
        Just("?".to_string()),
        Just("@".to_string()),
        "[a-z]{1,6}".prop_map(|s| s),
        (0i64..1000).prop_map(|n| n.to_string()),
        Just("\"hello\"".to_string()),
        Just("\n".to_string()),
    ];
    proptest::collection::vec(fragment, 0..50).prop_map(|fragments| fragments.join(" "))
}
