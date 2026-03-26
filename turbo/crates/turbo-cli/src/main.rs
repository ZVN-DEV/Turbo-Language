use clap::{Parser, Subcommand};
use std::path::PathBuf;
use ariadne::{Color, Label, Report, ReportKind, Source};

mod playground;

#[derive(Parser)]
#[command(name = "turbo", version, about = "The Turbo programming language compiler")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Compile and run a Turbo source file
    Run {
        /// Path to the .tb source file
        file: PathBuf,

        /// Show verbose output (tokens, AST, timing)
        #[arg(long, short)]
        verbose: bool,
    },
    /// Compile a Turbo source file to a native binary
    Build {
        /// Path to the .tb source file
        file: PathBuf,

        /// Output binary path (default: filename without .tb extension)
        #[arg(long, short)]
        output: Option<PathBuf>,

        /// Show verbose output
        #[arg(long, short)]
        verbose: bool,
    },
    /// Launch the Turbo Playground in your browser
    Playground {
        /// Port to serve on
        #[arg(long, short, default_value = "3000")]
        port: u16,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Run { file, verbose } => run_file(&file, verbose),
        Commands::Build { file, output, verbose } => build_file(&file, output.as_deref(), verbose),
        Commands::Playground { port } => playground::serve(port),
    }
}

/// Print a rich error diagnostic using ariadne.
fn report_error(source: &str, filename: &str, message: &str, span: &std::ops::Range<usize>, help: Option<&str>) {
    // Clamp span to source bounds to avoid panics on edge-case spans
    let start = span.start.min(source.len());
    let end = span.end.min(source.len()).max(start);
    let clamped = start..end;

    let mut builder = Report::build(ReportKind::Error, filename, clamped.start)
        .with_message(message);

    builder = builder.with_label(
        Label::new((filename, clamped))
            .with_message(message)
            .with_color(Color::Red),
    );

    if let Some(help_text) = help {
        builder = builder.with_help(help_text);
    }

    builder
        .finish()
        .eprint((filename, Source::from(source)))
        .unwrap();
}

fn run_file(path: &std::path::Path, verbose: bool) {
    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("\x1b[1;31merror\x1b[0m: could not read file `{}`: {e}", path.display());
            std::process::exit(1);
        }
    };

    let filename = path.file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| "<unknown>".to_string());

    // Lex
    let lex_start = std::time::Instant::now();
    let (tokens, lex_errors) = turbo_lexer::tokenize(&source);
    let lex_time = lex_start.elapsed();

    if !lex_errors.is_empty() {
        for span in &lex_errors {
            let snippet = &source[span.clone()];
            report_error(
                &source,
                &filename,
                &format!("unexpected character `{snippet}`"),
                span,
                Some("remove this character or check for typos"),
            );
        }
        std::process::exit(1);
    }

    if verbose {
        eprintln!("--- Tokens ({} total, {:.2?}) ---", tokens.len(), lex_time);
        for tok in &tokens {
            eprintln!("  {:?} @ {:?}", tok.value, tok.span);
        }
        eprintln!();
    }

    // Parse
    let parse_start = std::time::Instant::now();
    let (module, parse_errors) = turbo_parser::parse(tokens);
    let parse_time = parse_start.elapsed();

    if !parse_errors.is_empty() {
        for err in &parse_errors {
            report_error(
                &source,
                &filename,
                &err.message,
                &err.span,
                None,
            );
        }
        std::process::exit(1);
    }

    if module.items.is_empty() {
        report_error(
            &source,
            &filename,
            "no functions defined",
            &(0..source.len().min(1)),
            Some("add a `fn main() { ... }` function to get started"),
        );
        std::process::exit(1);
    }

    if verbose {
        eprintln!("--- AST ({} items, {:.2?}) ---", module.items.len(), parse_time);
        for item in &module.items {
            eprintln!("  {:#?}", item.node);
        }
        eprintln!();
    }

    // Semantic analysis
    let sema_start = std::time::Instant::now();
    let sema_errors = turbo_sema::check(&module);
    let sema_time = sema_start.elapsed();

    if verbose {
        eprintln!("--- Sema ({} errors, {:.2?}) ---", sema_errors.len(), sema_time);
    }

    if !sema_errors.is_empty() {
        for err in &sema_errors {
            let help = sema_help(&err.message);
            report_error(
                &source,
                &filename,
                &err.message,
                &err.span,
                help.as_deref(),
            );
        }
        std::process::exit(1);
    }

    // Compile & run
    let codegen_start = std::time::Instant::now();
    match turbo_codegen_cranelift::jit_run(&module) {
        Ok(()) => {
            if verbose {
                let codegen_time = codegen_start.elapsed();
                eprintln!("\n--- Timing ---");
                eprintln!("  Lex:     {:.2?}", lex_time);
                eprintln!("  Parse:   {:.2?}", parse_time);
                eprintln!("  Sema:    {:.2?}", sema_time);
                eprintln!("  Codegen: {:.2?}", codegen_time);
                eprintln!("  Total:   {:.2?}", lex_time + parse_time + sema_time + codegen_time);
            }
        }
        Err(e) => {
            eprintln!("\x1b[1;31merror\x1b[0m: {e}");
            std::process::exit(1);
        }
    }
}

fn build_file(path: &std::path::Path, output: Option<&std::path::Path>, verbose: bool) {
    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("\x1b[1;31merror\x1b[0m: could not read file `{}`: {e}", path.display());
            std::process::exit(1);
        }
    };

    let filename = path.file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| "<unknown>".to_string());

    // Default output: filename without .tb extension
    let default_output = path.file_stem()
        .map(|s| PathBuf::from(s))
        .unwrap_or_else(|| PathBuf::from("a.out"));
    let output_path = output.unwrap_or(&default_output);

    // Lex
    let lex_start = std::time::Instant::now();
    let (tokens, lex_errors) = turbo_lexer::tokenize(&source);
    let lex_time = lex_start.elapsed();

    if !lex_errors.is_empty() {
        for span in &lex_errors {
            let snippet = &source[span.clone()];
            report_error(
                &source,
                &filename,
                &format!("unexpected character `{snippet}`"),
                span,
                Some("remove this character or check for typos"),
            );
        }
        std::process::exit(1);
    }

    if verbose {
        eprintln!("--- Tokens ({} total, {:.2?}) ---", tokens.len(), lex_time);
    }

    // Parse
    let parse_start = std::time::Instant::now();
    let (module, parse_errors) = turbo_parser::parse(tokens);
    let parse_time = parse_start.elapsed();

    if !parse_errors.is_empty() {
        for err in &parse_errors {
            report_error(
                &source,
                &filename,
                &err.message,
                &err.span,
                None,
            );
        }
        std::process::exit(1);
    }

    if module.items.is_empty() {
        report_error(
            &source,
            &filename,
            "no functions defined",
            &(0..source.len().min(1)),
            Some("add a `fn main() { ... }` function to get started"),
        );
        std::process::exit(1);
    }

    if verbose {
        eprintln!("--- AST ({} items, {:.2?}) ---", module.items.len(), parse_time);
    }

    // Semantic analysis
    let sema_start = std::time::Instant::now();
    let sema_errors = turbo_sema::check(&module);
    let sema_time = sema_start.elapsed();

    if !sema_errors.is_empty() {
        for err in &sema_errors {
            let help = sema_help(&err.message);
            report_error(
                &source,
                &filename,
                &err.message,
                &err.span,
                help.as_deref(),
            );
        }
        std::process::exit(1);
    }

    // Compile to native binary
    let codegen_start = std::time::Instant::now();
    match turbo_codegen_cranelift::aot_compile(&module, output_path, true) {
        Ok(()) => {
            let codegen_time = codegen_start.elapsed();
            eprintln!("\x1b[32m\u{2713}\x1b[0m Compiled to {}", output_path.display());
            if verbose {
                eprintln!("\n--- Timing ---");
                eprintln!("  Lex:     {:.2?}", lex_time);
                eprintln!("  Parse:   {:.2?}", parse_time);
                eprintln!("  Sema:    {:.2?}", sema_time);
                eprintln!("  Codegen: {:.2?}", codegen_time);
                eprintln!("  Total:   {:.2?}", lex_time + parse_time + sema_time + codegen_time);
            }
        }
        Err(e) => {
            eprintln!("\x1b[1;31merror\x1b[0m: {e}");
            std::process::exit(1);
        }
    }
}

/// Generate contextual help text for common sema error patterns.
fn sema_help(message: &str) -> Option<String> {
    if message.contains("undefined variable") {
        // Extract variable name from backticks
        if let Some(name) = extract_backtick_name(message) {
            return Some(format!("did you mean to declare `{name}` with `let {name} = ...`?"));
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
        return Some("both sides of an arithmetic operation must have the same numeric type".to_string());
    }
    if message.contains("cannot perform arithmetic on") {
        return Some("arithmetic operators (`+`, `-`, `*`, `/`, `%`) only work on numeric types (`i32`, `i64`, `f32`, `f64`)".to_string());
    }
    if message.contains("type annotation") && message.contains("doesn't match") {
        return Some("either change the type annotation or the assigned value".to_string());
    }
    if message.contains("should return") && message.contains("but body returns") {
        return Some("make sure the last expression in the function body matches the declared return type".to_string());
    }
    if message.contains("if/else branches have different types") {
        return Some("both branches of an if/else expression must produce the same type".to_string());
    }
    if message.contains("if condition must be `bool`") || message.contains("while condition must be `bool`") {
        return Some("conditions must be `bool`; use a comparison like `x > 0` instead".to_string());
    }
    if message.contains("expects") && message.contains("argument(s) but") {
        return Some("check the function signature for the correct number of arguments".to_string());
    }
    if message.contains("argument") && message.contains("expects") && message.contains("found") {
        return Some("the argument type doesn't match the parameter type in the function signature".to_string());
    }
    None
}

/// Extract the first name enclosed in backticks from a message.
fn extract_backtick_name(message: &str) -> Option<&str> {
    let start = message.find('`')? + 1;
    let end = message[start..].find('`')? + start;
    Some(&message[start..end])
}
