use clap::{Parser, Subcommand};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use turbo_ast::{Item, Module};

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
    let (mut module, parse_errors) = turbo_parser::parse(tokens);
    let parse_time = parse_start.elapsed();

    if !parse_errors.is_empty() {
        for err in &parse_errors {
            let (line, col) = line_col(&source, err.span.start);
            eprintln!("error: {} at {filename}:{line}:{col}", err.message);
        }
        std::process::exit(1);
    }

    // Resolve imports
    let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let canonical_self = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let mut loading = HashSet::new();
    loading.insert(canonical_self);
    if let Err(e) = resolve_imports(&mut module, base_dir, &mut loading) {
        eprintln!("error: {e}");
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
    let (mut module, parse_errors) = turbo_parser::parse(tokens);
    let parse_time = parse_start.elapsed();

    if !parse_errors.is_empty() {
        for err in &parse_errors {
            let (line, col) = line_col(&source, err.span.start);
            eprintln!("error: {} at {filename}:{line}:{col}", err.message);
        }
        std::process::exit(1);
    }

    // Resolve imports
    let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let canonical_self = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let mut loading = HashSet::new();
    loading.insert(canonical_self);
    if let Err(e) = resolve_imports(&mut module, base_dir, &mut loading) {
        eprintln!("error: {e}");
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

/// Resolve the file path for an import.
/// The `.tb` extension is appended if not already present.
/// The path is resolved relative to `base_dir`.
fn resolve_import_path(base_dir: &Path, import_path: &str) -> PathBuf {
    let mut path = base_dir.join(import_path);
    if path.extension().is_none() {
        path.set_extension("tb");
    }
    path
}

/// Resolve all `import` items in the module by reading, lexing, and parsing
/// the imported files and inlining the requested items.
/// `loading` tracks files currently being loaded (for circular import detection).
fn resolve_imports(
    module: &mut Module,
    base_dir: &Path,
    loading: &mut HashSet<PathBuf>,
) -> Result<(), String> {
    let mut import_items = Vec::new();

    for item in &module.items {
        if let Item::Import { names, path } = &item.node {
            let resolved_path = resolve_import_path(base_dir, path);
            let canonical = resolved_path.canonicalize().map_err(|e| {
                format!("could not resolve import path `{}`: {e}", resolved_path.display())
            })?;

            // Circular import detection
            if loading.contains(&canonical) {
                return Err(format!(
                    "circular import detected: `{}`",
                    resolved_path.display()
                ));
            }

            loading.insert(canonical.clone());

            let source = std::fs::read_to_string(&resolved_path).map_err(|e| {
                format!("could not read imported file `{}`: {e}", resolved_path.display())
            })?;

            let (tokens, lex_errors) = turbo_lexer::tokenize(&source);
            if !lex_errors.is_empty() {
                return Err(format!(
                    "lex errors in imported file `{}`",
                    resolved_path.display()
                ));
            }

            let (mut imported_module, parse_errors) = turbo_parser::parse(tokens);
            if !parse_errors.is_empty() {
                return Err(format!(
                    "parse errors in imported file `{}`: {}",
                    resolved_path.display(),
                    parse_errors[0].message
                ));
            }

            // Recursively resolve imports in the imported file
            let imported_dir = resolved_path.parent().unwrap_or(base_dir);
            resolve_imports(&mut imported_module, imported_dir, loading)?;

            loading.remove(&canonical);

            // Extract the requested items
            for imported_item in imported_module.items {
                match &imported_item.node {
                    Item::Function(f) if names.contains(&f.name) => {
                        import_items.push(imported_item);
                    }
                    Item::Struct(s) if names.contains(&s.name) => {
                        import_items.push(imported_item);
                    }
                    Item::Enum(e) if names.contains(&e.name) => {
                        import_items.push(imported_item);
                    }
                    Item::Impl(imp) if names.contains(&imp.type_name) => {
                        import_items.push(imported_item);
                    }
                    _ => {}
                }
            }

            // Check that all requested names were found
            for name in names {
                let found = import_items.iter().any(|item| match &item.node {
                    Item::Function(f) => &f.name == name,
                    Item::Struct(s) => &s.name == name,
                    Item::Enum(e) => &e.name == name,
                    Item::Impl(imp) => &imp.type_name == name,
                    _ => false,
                });
                if !found {
                    return Err(format!(
                        "name `{name}` not found in `{}`",
                        resolved_path.display()
                    ));
                }
            }
        }
    }

    // Remove import items and prepend imported items
    module.items.retain(|item| !matches!(&item.node, Item::Import { .. }));
    let mut new_items = import_items;
    new_items.append(&mut module.items);
    module.items = new_items;

    Ok(())
}

fn line_col(source: &str, offset: usize) -> (usize, usize) {
    let line = source[..offset].matches('\n').count() + 1;
    let col = offset - source[..offset].rfind('\n').map(|p| p + 1).unwrap_or(0) + 1;
    (line, col)
}
