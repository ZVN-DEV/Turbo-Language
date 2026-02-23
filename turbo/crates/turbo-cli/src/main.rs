use clap::{Parser, Subcommand};
use std::path::PathBuf;

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

fn run_file(path: &std::path::Path, verbose: bool) {
    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: could not read file `{}`: {e}", path.display());
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
            let (line, col) = line_col(&source, span.start);
            eprintln!(
                "error: unexpected character at {filename}:{line}:{col}: `{}`",
                &source[span.clone()]
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
            let (line, col) = line_col(&source, err.span.start);
            eprintln!("error: {} at {filename}:{line}:{col}", err.message);
        }
        std::process::exit(1);
    }

    if module.items.is_empty() {
        eprintln!("error: no functions defined in {filename}");
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
            let (line, col) = line_col(&source, err.span.start);
            eprintln!("error: {} at {filename}:{line}:{col}", err.message);
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
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

fn build_file(path: &std::path::Path, output: Option<&std::path::Path>, verbose: bool) {
    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: could not read file `{}`: {e}", path.display());
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
            let (line, col) = line_col(&source, span.start);
            eprintln!(
                "error: unexpected character at {filename}:{line}:{col}: `{}`",
                &source[span.clone()]
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
            let (line, col) = line_col(&source, err.span.start);
            eprintln!("error: {} at {filename}:{line}:{col}", err.message);
        }
        std::process::exit(1);
    }

    if module.items.is_empty() {
        eprintln!("error: no functions defined in {filename}");
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
            let (line, col) = line_col(&source, err.span.start);
            eprintln!("error: {} at {filename}:{line}:{col}", err.message);
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
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

fn line_col(source: &str, offset: usize) -> (usize, usize) {
    let line = source[..offset].matches('\n').count() + 1;
    let col = offset - source[..offset].rfind('\n').map(|p| p + 1).unwrap_or(0) + 1;
    (line, col)
}
