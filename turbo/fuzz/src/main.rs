//! Simple fuzz harness for the Turbo compiler frontend.
//!
//! Run: cargo run --manifest-path turbo/fuzz/Cargo.toml -- [iterations]
//! Default: 10,000 iterations
//!
//! This does NOT require nightly Rust or cargo-fuzz. It uses a deterministic
//! seed-based approach to generate varied inputs and feeds them through the
//! lexer, parser, and semantic analysis passes. Errors are fine; panics are bugs.

use std::env;

fn main() {
    let iterations: usize = env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(10_000);

    println!("Running {} fuzz iterations...", iterations);
    let mut crashes = 0;
    let mut crash_seeds: Vec<u64> = Vec::new();

    for seed in 0..iterations as u64 {
        let input = generate_input(seed);
        if !fuzz_one(&input) {
            crashes += 1;
            crash_seeds.push(seed);
            eprintln!(
                "CRASH at seed {} (gen={}): input = {:?}",
                seed,
                seed % 12,
                truncate(&input, 200)
            );
        }
        // Progress indicator every 1000 seeds
        if (seed + 1) % 1000 == 0 {
            eprintln!("  ... {} / {} done", seed + 1, iterations);
        }
    }

    println!("Completed {} iterations, {} crashes", iterations, crashes);
    if !crash_seeds.is_empty() {
        println!("Crash seeds: {:?}", crash_seeds);
    }
    if crashes > 0 {
        std::process::exit(1);
    }
}

/// Truncate a string for display purposes.
fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        // Floor to a char boundary so a mid-codepoint `max` can't panic the slice.
        let mut cut = max;
        while cut > 0 && !s.is_char_boundary(cut) {
            cut -= 1;
        }
        format!("{}...[{} bytes total]", &s[..cut], s.len())
    }
}

/// Run a single fuzz input through the compiler frontend.
/// Returns true if no panic occurred (errors are fine).
///
/// Each iteration runs on a dedicated thread with a 2 MB stack so that stack
/// overflows are turned into catchable panics instead of aborting the process.
fn fuzz_one(input: &str) -> bool {
    let input = input.to_owned();
    let handle = std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024) // 32 MB stack (debug-mode frames are large)
        .spawn(move || {
            let (tokens, _lex_errors) = turbo_lexer::tokenize(&input);
            let (module, _parse_errors) = turbo_parser::parse(tokens);
            let _sema_errors = turbo_sema::check(&module);
        })
        .expect("failed to spawn fuzz thread");

    handle.join().is_ok()
}

// ---------------------------------------------------------------------------
// Input generation
// ---------------------------------------------------------------------------

/// Simple deterministic PRNG (xorshift64).
struct Rng {
    state: u64,
}

impl Rng {
    fn new(seed: u64) -> Self {
        // Avoid zero state
        Self {
            state: seed.wrapping_add(1),
        }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    fn next_usize(&mut self, max: usize) -> usize {
        (self.next_u64() % max as u64) as usize
    }

    fn next_bool(&mut self) -> bool {
        self.next_u64() % 2 == 0
    }

    fn choose<'a>(&mut self, items: &'a [&str]) -> &'a str {
        items[self.next_usize(items.len())]
    }
}

/// Generate a fuzz input from a seed.
fn generate_input(seed: u64) -> String {
    let mut rng = Rng::new(seed);

    // Pick a generator strategy based on seed
    match seed % 12 {
        0 => gen_random_tokens(&mut rng),
        1 => gen_nested_parens(&mut rng),
        2 => gen_nested_braces(&mut rng),
        3 => gen_keyword_soup(&mut rng),
        4 => gen_mutated_valid_program(&mut rng),
        5 => gen_unicode_stress(&mut rng),
        6 => gen_boundary_string(&mut rng),
        7 => gen_partial_program(&mut rng),
        8 => gen_random_function_defs(&mut rng),
        9 => gen_deep_expressions(&mut rng),
        10 => gen_mismatched_brackets(&mut rng),
        11 => gen_string_stress(&mut rng),
        _ => unreachable!(),
    }
}

// ---------------------------------------------------------------------------
// Generator: random tokens
// ---------------------------------------------------------------------------

const TURBO_KEYWORDS: &[&str] = &[
    "fn", "let", "mut", "const", "if", "else", "while", "for", "in", "return", "true", "false",
    "match", "struct", "type", "impl", "trait", "pub", "import", "from", "async", "await", "spawn",
    "defer", "none", "some", "ok", "err", "break", "continue",
];

const TURBO_OPERATORS: &[&str] = &[
    "==", "!=", "<=", ">=", "&&", "||", "->", "=>", "|>", "..", "?.", "??", "+=", "-=", "*=", "/=",
    "|", "+", "-", "*", "/", "%", "=", "<", ">", "!", "?", ".",
];

const TURBO_DELIMITERS: &[&str] = &["(", ")", "{", "}", "[", "]", ",", ":", ";", "@"];

fn gen_random_tokens(rng: &mut Rng) -> String {
    let count = rng.next_usize(100) + 1;
    let mut out = String::new();

    let all_tokens: Vec<&str> = TURBO_KEYWORDS
        .iter()
        .chain(TURBO_OPERATORS.iter())
        .chain(TURBO_DELIMITERS.iter())
        .copied()
        .collect();

    for i in 0..count {
        if i > 0 {
            out.push(' ');
        }
        let choice = rng.next_usize(10);
        match choice {
            0..=3 => out.push_str(rng.choose(&all_tokens)),
            4 => {
                // Random identifier
                let len = rng.next_usize(20) + 1;
                for j in 0..len {
                    let c = if j == 0 {
                        (b'a' + (rng.next_u64() % 26) as u8) as char
                    } else {
                        let n = rng.next_u64() % 36;
                        if n < 26 {
                            (b'a' + n as u8) as char
                        } else {
                            (b'0' + (n - 26) as u8) as char
                        }
                    };
                    out.push(c);
                }
            }
            5 => {
                // Random integer
                let v = rng.next_u64() % 1_000_000;
                out.push_str(&v.to_string());
            }
            6 => {
                // Random float
                let a = rng.next_u64() % 1000;
                let b = rng.next_u64() % 1000;
                out.push_str(&format!("{}.{}", a, b));
            }
            7 => {
                // Random string literal
                out.push('"');
                let len = rng.next_usize(30);
                for _ in 0..len {
                    let c = (b'a' + (rng.next_u64() % 26) as u8) as char;
                    out.push(c);
                }
                // Sometimes don't close the string
                if rng.next_usize(5) > 0 {
                    out.push('"');
                }
            }
            8 => {
                // Newline
                out.push('\n');
            }
            _ => {
                // Comment
                out.push_str("// a comment\n");
            }
        }
    }

    out
}

// ---------------------------------------------------------------------------
// Generator: deeply nested parentheses
// ---------------------------------------------------------------------------

fn gen_nested_parens(rng: &mut Rng) -> String {
    let depth = rng.next_usize(300) + 1;
    let mut out = String::new();
    for _ in 0..depth {
        out.push('(');
    }
    out.push_str("42");
    for _ in 0..depth {
        out.push(')');
    }
    out
}

// ---------------------------------------------------------------------------
// Generator: deeply nested braces
// ---------------------------------------------------------------------------

fn gen_nested_braces(rng: &mut Rng) -> String {
    let depth = rng.next_usize(300) + 1;
    let mut out = String::new();
    out.push_str("fn main() ");
    for _ in 0..depth {
        out.push_str("{ if true ");
    }
    out.push_str("{ let x = 1 }");
    for _ in 0..depth {
        out.push('}');
    }
    out
}

// ---------------------------------------------------------------------------
// Generator: keyword soup (random sequences of keywords)
// ---------------------------------------------------------------------------

fn gen_keyword_soup(rng: &mut Rng) -> String {
    let count = rng.next_usize(80) + 5;
    let mut out = String::new();
    for i in 0..count {
        if i > 0 {
            out.push(' ');
        }
        out.push_str(rng.choose(TURBO_KEYWORDS));
    }
    out
}

// ---------------------------------------------------------------------------
// Generator: mutated valid programs
// ---------------------------------------------------------------------------

const VALID_PROGRAMS: &[&str] = &[
    "fn main() {\n    let x = 42\n    print(x)\n}",
    "fn add(a: i64, b: i64) -> i64 {\n    return a + b\n}\nfn main() {\n    let result = add(1, 2)\n}",
    "struct Point {\n    x: i64\n    y: i64\n}\nfn main() {\n    let p = Point { x: 1, y: 2 }\n}",
    "fn fib(n: i64) -> i64 {\n    if n <= 1 {\n        return n\n    }\n    return fib(n - 1) + fib(n - 2)\n}",
    "fn main() {\n    let mut i = 0\n    while i < 10 {\n        i += 1\n    }\n}",
    "fn main() {\n    let arr = [1, 2, 3, 4, 5]\n    for x in arr {\n        print(x)\n    }\n}",
    "fn main() {\n    match 42 {\n        1 => print(\"one\")\n        2 => print(\"two\")\n        _ => print(\"other\")\n    }\n}",
    "fn greet(name: str) -> str {\n    return \"Hello, \" + name\n}",
];

fn gen_mutated_valid_program(rng: &mut Rng) -> String {
    let program = VALID_PROGRAMS[rng.next_usize(VALID_PROGRAMS.len())];
    let mut chars: Vec<u8> = program.bytes().collect();

    // Apply 1-20 random mutations
    let mutations = rng.next_usize(20) + 1;
    for _ in 0..mutations {
        if chars.is_empty() {
            break;
        }
        let op = rng.next_usize(5);
        match op {
            0 => {
                // Delete a random byte
                let idx = rng.next_usize(chars.len());
                chars.remove(idx);
            }
            1 => {
                // Insert a random byte
                let idx = rng.next_usize(chars.len() + 1);
                let byte = (rng.next_u64() % 128) as u8;
                chars.insert(idx, byte);
            }
            2 => {
                // Replace a random byte
                let idx = rng.next_usize(chars.len());
                chars[idx] = (rng.next_u64() % 128) as u8;
            }
            3 => {
                // Duplicate a chunk
                let start = rng.next_usize(chars.len());
                let len = rng.next_usize(20).min(chars.len() - start);
                let chunk: Vec<u8> = chars[start..start + len].to_vec();
                let insert_at = rng.next_usize(chars.len() + 1);
                for (i, b) in chunk.into_iter().enumerate() {
                    if insert_at + i <= chars.len() {
                        chars.insert(insert_at + i, b);
                    }
                }
            }
            _ => {
                // Swap two bytes
                if chars.len() >= 2 {
                    let a = rng.next_usize(chars.len());
                    let b = rng.next_usize(chars.len());
                    chars.swap(a, b);
                }
            }
        }
    }

    String::from_utf8_lossy(&chars).into_owned()
}

// ---------------------------------------------------------------------------
// Generator: unicode stress
// ---------------------------------------------------------------------------

fn gen_unicode_stress(rng: &mut Rng) -> String {
    let mut out = String::new();
    let patterns: &[&str] = &[
        "\u{200B}",  // zero-width space
        "\u{200C}",  // zero-width non-joiner
        "\u{200D}",  // zero-width joiner
        "\u{FEFF}",  // BOM
        "\u{202A}",  // LTR embedding
        "\u{202B}",  // RTL embedding
        "\u{202C}",  // pop directional
        "\u{0000}",  // null byte
        "\u{FFFD}",  // replacement char
        "\u{0301}",  // combining acute accent
        "\u{1F600}", // emoji grinning face
        "\u{1F4A9}", // pile of poo
        "\u{E0001}", // tag latin small letter a
        "\u{FE0F}",  // variation selector
        "\u{D7FF}",  // just below surrogate range (valid)
        "let",
        "fn",
        "42",
        "(",
        ")",
        "{",
        "}",
        "\"hello\"",
        "\n",
    ];

    let count = rng.next_usize(200) + 10;
    for _ in 0..count {
        out.push_str(rng.choose(patterns));
        if rng.next_bool() {
            out.push(' ');
        }
    }

    out
}

// ---------------------------------------------------------------------------
// Generator: boundary strings
// ---------------------------------------------------------------------------

fn gen_boundary_string(rng: &mut Rng) -> String {
    match rng.next_usize(6) {
        0 => String::new(),   // empty input
        1 => "x".to_string(), // single character
        2 => {
            // Very long identifier
            let len = rng.next_usize(100_000) + 50_000;
            "a".repeat(len)
        }
        3 => {
            // Repeated pattern
            let pattern = rng.choose(&["fn ", "( ", ") ", "{ ", "} ", "+ ", "let x = 1\n"]);
            let reps = rng.next_usize(5000) + 1000;
            pattern.repeat(reps)
        }
        4 => {
            // All whitespace
            " \t\n\r".repeat(rng.next_usize(1000) + 100)
        }
        _ => {
            // All null bytes
            "\0".repeat(rng.next_usize(1000) + 1)
        }
    }
}

// ---------------------------------------------------------------------------
// Generator: partially valid program (valid start, corrupt middle)
// ---------------------------------------------------------------------------

fn gen_partial_program(rng: &mut Rng) -> String {
    let program = VALID_PROGRAMS[rng.next_usize(VALID_PROGRAMS.len())];
    let bytes = program.as_bytes();
    // Take some valid prefix
    let cut = rng.next_usize(bytes.len());
    let prefix = String::from_utf8_lossy(&bytes[..cut]).into_owned();

    // Append garbage
    let garbage_len = rng.next_usize(200) + 10;
    let mut garbage = String::new();
    for _ in 0..garbage_len {
        let c = (rng.next_u64() % 128) as u8;
        garbage.push(c as char);
    }

    format!("{}{}", prefix, garbage)
}

// ---------------------------------------------------------------------------
// Generator: random function definitions
// ---------------------------------------------------------------------------

fn gen_random_function_defs(rng: &mut Rng) -> String {
    let count = rng.next_usize(10) + 1;
    let mut out = String::new();

    let types = &["i64", "i32", "bool", "str", "f64"];

    for i in 0..count {
        out.push_str("fn ");
        out.push_str(&format!("func_{}", i));
        out.push('(');

        let params = rng.next_usize(10);
        for p in 0..params {
            if p > 0 {
                out.push_str(", ");
            }
            out.push_str(&format!("p{}: {}", p, rng.choose(types)));
        }
        out.push(')');

        if rng.next_bool() {
            out.push_str(" -> ");
            out.push_str(rng.choose(types));
        }

        out.push_str(" {\n");

        // Random body statements
        let stmts = rng.next_usize(5);
        for _ in 0..stmts {
            let stmt_kind = rng.next_usize(4);
            match stmt_kind {
                0 => out.push_str(&format!("    let x = {}\n", rng.next_u64() % 1000)),
                1 => out.push_str("    return 0\n"),
                2 => out.push_str("    if true { let y = 1 }\n"),
                _ => out.push_str("    let mut z = 0\n"),
            }
        }

        out.push_str("}\n\n");
    }

    out
}

// ---------------------------------------------------------------------------
// Generator: deeply nested expressions
// ---------------------------------------------------------------------------

fn gen_deep_expressions(rng: &mut Rng) -> String {
    let depth = rng.next_usize(300) + 1;
    let ops = &["+", "-", "*", "/", "%", "==", "!=", "<", ">", "<=", ">="];
    let mut out = String::from("fn main() { let x = ");

    for _ in 0..depth {
        if rng.next_bool() {
            out.push('(');
        }
        out.push_str(&format!("{} ", rng.next_u64() % 100));
        out.push_str(rng.choose(ops));
        out.push(' ');
    }
    out.push('1');
    // Close some parens (intentionally may not match)
    for _ in 0..depth / 2 {
        out.push(')');
    }
    out.push_str(" }");

    out
}

// ---------------------------------------------------------------------------
// Generator: mismatched brackets
// ---------------------------------------------------------------------------

fn gen_mismatched_brackets(rng: &mut Rng) -> String {
    let count = rng.next_usize(100) + 10;
    let brackets = &["(", ")", "{", "}", "[", "]"];
    let mut out = String::from("fn main() ");

    for _ in 0..count {
        out.push_str(rng.choose(brackets));
        if rng.next_bool() {
            out.push_str("let x = 1 ");
        }
        if rng.next_usize(5) == 0 {
            out.push('\n');
        }
    }

    out
}

// ---------------------------------------------------------------------------
// Generator: string literal stress
// ---------------------------------------------------------------------------

fn gen_string_stress(rng: &mut Rng) -> String {
    let mut out = String::from("fn main() { let s = ");

    match rng.next_usize(6) {
        0 => {
            // Unclosed string
            out.push('"');
            for _ in 0..rng.next_usize(100) {
                out.push((b'a' + (rng.next_u64() % 26) as u8) as char);
            }
        }
        1 => {
            // String with many escape sequences
            out.push('"');
            for _ in 0..rng.next_usize(100) {
                out.push('\\');
                out.push(
                    rng.choose(&["n", "t", "r", "\\", "\"", "0", "x", "u"])
                        .chars()
                        .next()
                        .unwrap(),
                );
            }
            out.push('"');
        }
        2 => {
            // Triple-quoted string
            out.push_str("\"\"\"");
            for _ in 0..rng.next_usize(200) {
                let c = (b'a' + (rng.next_u64() % 26) as u8) as char;
                out.push(c);
                if rng.next_usize(10) == 0 {
                    out.push('\n');
                }
            }
            // Sometimes close it
            if rng.next_bool() {
                out.push_str("\"\"\"");
            }
        }
        3 => {
            // Empty string
            out.push_str("\"\"");
        }
        4 => {
            // String with embedded quotes
            out.push('"');
            for _ in 0..rng.next_usize(20) {
                if rng.next_bool() {
                    out.push_str("\\\"");
                } else {
                    out.push('a');
                }
            }
            out.push('"');
        }
        _ => {
            // Very long string
            out.push('"');
            for _ in 0..rng.next_usize(10_000) + 5000 {
                out.push('x');
            }
            out.push('"');
        }
    }

    out.push_str(" }");
    out
}
