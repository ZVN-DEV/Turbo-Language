use ariadne::{Color, Label, Report, ReportKind, Source};
use clap::{Parser, Subcommand};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use turbo_ast::{ErrorCode, Item, Module};

mod formatter;
mod playground;
mod repl;

#[derive(Parser)]
#[command(
    name = "turbolang",
    version = env!("CARGO_PKG_VERSION"),
    about = "The Turbo programming language compiler"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Compile and run a Turbo source file
    Run {
        /// Path to the .tb source file (optional if turbo.toml exists)
        file: Option<PathBuf>,

        /// Show verbose output (tokens, AST, timing)
        #[arg(long, short)]
        verbose: bool,
    },
    /// Compile a Turbo source file to a native binary
    Build {
        /// Path to the .tb source file (optional if turbo.toml exists)
        file: Option<PathBuf>,

        /// Output binary path (default: filename without .tb extension)
        #[arg(long, short)]
        output: Option<PathBuf>,

        /// Show verbose output
        #[arg(long, short)]
        verbose: bool,

        /// Use LLVM backend instead of Cranelift
        #[arg(long)]
        llvm: bool,
    },
    /// Initialize a new Turbo project
    Init {
        /// Project name
        name: String,
    },
    /// Start an interactive REPL
    Repl,
    /// Launch the Turbo Playground in your browser
    Playground {
        /// Port to serve on
        #[arg(long, short, default_value = "3000")]
        port: u16,
    },
    /// Format a Turbo source file
    Fmt {
        /// Path to the .tb source file to format
        file: PathBuf,
        /// Check only, don't modify (exit 1 if unformatted)
        #[arg(long)]
        check: bool,
    },
    /// Generate documentation from a Turbo source file
    Doc {
        /// Path to the .tb source file
        file: PathBuf,
    },
    /// Install dependencies from turbo.toml
    Install,
    /// Update GitHub dependencies to latest
    Update,
    /// Start the Language Server Protocol server
    Lsp,
    /// Type-check a Turbo source file without compiling or running
    Check {
        /// Path to the .tb source file (optional if turbo.toml exists)
        file: Option<PathBuf>,
    },
    /// Run @test functions in a Turbo source file
    Test {
        /// Path to the .tb source file (or directory)
        file: Option<PathBuf>,
    },
    /// Run benchmarks and report timing
    Bench {
        /// Path to a .tb benchmark file (or directory of bench_*.tb files)
        file: Option<PathBuf>,

        /// Number of iterations to run (default: 3)
        #[arg(long, short = 'n', default_value = "3")]
        iterations: u32,

        /// Suppress program stdout, only show timing results
        #[arg(long, short)]
        quiet: bool,
    },
    /// Explain an error code (e.g. turbolang explain E0100)
    Explain {
        /// Error code to explain (e.g. E0100)
        code: String,
    },
    /// [internal] Run a single test function by name (used by test runner)
    #[command(hide = true)]
    TestRunFn {
        /// Path to the .tb source file
        file: PathBuf,
        /// Name of the function to run
        #[arg(long)]
        func: String,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Run { file, verbose } => {
            let path = resolve_entry_file(file);
            run_file(&path, verbose);
        }
        Commands::Build {
            file,
            output,
            verbose,
            llvm,
        } => {
            let path = resolve_entry_file(file);
            build_file(&path, output.as_deref(), verbose, llvm);
        }
        Commands::Init { name } => init_project(&name),
        Commands::Repl => repl::run_repl(),
        Commands::Playground { port } => playground::serve(port),
        Commands::Fmt { file, check } => formatter::format_file(&file, check),
        Commands::Doc { file } => doc_file(&file),
        Commands::Install => install_deps(),
        Commands::Update => update_deps(),
        Commands::Check { file } => {
            let path = resolve_entry_file(file);
            check_file(&path);
        }
        Commands::Lsp => start_lsp(),
        Commands::Test { file } => test_file(file),
        Commands::Bench {
            file,
            iterations,
            quiet,
        } => bench_file(file, iterations, quiet),
        Commands::Explain { code } => explain_error(&code),
        Commands::TestRunFn { file, func } => test_run_fn(&file, &func),
    }
}

fn start_lsp() {
    // The LSP server is a separate binary (turbo-lsp).
    // This command provides a convenience wrapper that exec's it.
    let exe = std::env::current_exe().unwrap_or_default();
    let lsp_bin = exe
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("turbo-lsp");

    if lsp_bin.exists() {
        use std::process::Command;
        let status = Command::new(&lsp_bin)
            .stdin(std::process::Stdio::inherit())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .status();
        match status {
            Ok(s) => std::process::exit(s.code().unwrap_or(1)),
            Err(e) => {
                eprintln!("\x1b[1;31merror\x1b[0m: failed to start LSP server: {e}");
                std::process::exit(1);
            }
        }
    } else {
        eprintln!(
            "\x1b[1;31merror\x1b[0m: turbo-lsp binary not found at `{}`",
            lsp_bin.display()
        );
        eprintln!("  Build it with: cargo build -p turbo-lsp");
        std::process::exit(1);
    }
}

/// Resolve the entry file for run/build commands.
/// If a file is explicitly provided, use it.
/// If no file is given and `turbo.toml` exists in the current directory, use `src/main.tb`.
fn resolve_entry_file(file: Option<PathBuf>) -> PathBuf {
    if let Some(f) = file {
        return f;
    }

    let manifest = Path::new("turbo.toml");
    if manifest.exists() {
        let entry = PathBuf::from("src/main.tb");
        if entry.exists() {
            return entry;
        }
        eprintln!("\x1b[1;31merror\x1b[0m: found `turbo.toml` but `src/main.tb` does not exist");
        std::process::exit(1);
    }

    eprintln!(
        "\x1b[1;31merror\x1b[0m: no file specified and no `turbo.toml` found in current directory"
    );
    eprintln!("  Usage: turbolang run <file.tb>");
    eprintln!("  Or run `turbolang init <name>` to create a new project");
    std::process::exit(1);
}

/// Initialize a new Turbo project with the given name.
fn init_project(name: &str) {
    let dir = Path::new(name);
    let pkg_name = dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| name.to_string());

    if dir.exists() {
        eprintln!("\x1b[1;31merror\x1b[0m: directory `{name}` already exists");
        std::process::exit(1);
    }

    std::fs::create_dir_all(dir.join("src")).unwrap_or_else(|e| {
        eprintln!("\x1b[1;31merror\x1b[0m: could not create directory: {e}");
        std::process::exit(1);
    });
    std::fs::create_dir_all(dir.join("tests")).unwrap_or_else(|e| {
        eprintln!("\x1b[1;31merror\x1b[0m: could not create directory: {e}");
        std::process::exit(1);
    });

    // turbo.toml
    std::fs::write(
        dir.join("turbo.toml"),
        format!(
            "[package]\nname = \"{pkg_name}\"\nversion = \"0.1.0\"\nedition = \"2026\"\n\n[dependencies]\n"
        ),
    )
    .unwrap();

    // src/main.tb
    std::fs::write(
        dir.join("src/main.tb"),
        format!(
            r#"/// A counter that tracks a value
struct Counter {{
    count: i64,
}}

impl Counter {{
    fn increment(self) -> Counter {{
        Counter {{ count: self.count + 1 }}
    }}

    fn value(self) -> i64 {{
        self.count
    }}
}}

/// Shapes with area calculation
type Shape {{
    Circle(f64),
    Rectangle(f64, f64),
}}

fn area(shape: Shape) -> f64 {{
    match shape {{
        Circle(r) => 3.14159 * r * r
        Rectangle(w, h) => w * h
    }}
}}

fn main() {{
    print("Hello from {pkg_name}!")

    // Struct with methods
    let mut c = Counter {{ count: 0 }}
    c = c.increment()
    c = c.increment()
    c = c.increment()
    print("Counter: " + to_str(c.value()))

    // Enum + pattern matching
    let circle = Shape.Circle(5.0)
    let rect = Shape.Rectangle(4.0, 6.0)
    print("Circle area: " + to_str(area(circle)))
    print("Rectangle area: " + to_str(area(rect)))
}}
"#
        ),
    )
    .unwrap();

    // tests/main_test.tb
    std::fs::write(
        dir.join("tests/main_test.tb"),
        r#"struct Counter {
    count: i64,
}

impl Counter {
    fn increment(self) -> Counter {
        Counter { count: self.count + 1 }
    }

    fn value(self) -> i64 {
        self.count
    }
}

type Shape {
    Circle(f64),
    Rectangle(f64, f64),
}

fn area(shape: Shape) -> f64 {
    match shape {
        Circle(r) => 3.14159 * r * r
        Rectangle(w, h) => w * h
    }
}

@test fn test_counter() {
    let c = Counter { count: 0 }
    assert(c.value() == 0, "new counter starts at 0")
    let c2 = c.increment()
    assert(c2.value() == 1, "increment adds 1")
}

@test fn test_area() {
    let rect = Shape.Rectangle(3.0, 4.0)
    assert(area(rect) == 12.0, "rectangle area is w * h")
}

@test fn test_math() {
    assert(1 + 1 == 2, "basic math works")
}
"#,
    )
    .unwrap();

    // .gitignore
    std::fs::write(dir.join(".gitignore"), "turbo_modules/\ntarget/\n*.o\n").unwrap();

    eprintln!("\x1b[32m\u{2713}\x1b[0m Created project `{name}`");
    eprintln!("  cd {name} && turbolang run");
}

/// Read the project name from `turbo.toml` in the current directory, if it exists.
fn read_project_name() -> Option<PathBuf> {
    let toml = std::fs::read_to_string("turbo.toml").ok()?;
    for line in toml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("name") {
            if let Some(val) = trimmed.split('=').nth(1) {
                let name = val.trim().trim_matches('"').trim_matches('\'');
                if !name.is_empty() {
                    return Some(PathBuf::from(name));
                }
            }
        }
    }
    None
}

/// Extract a quoted value for a given key from a TOML inline table string.
/// e.g. for `{ path = "../utils" }` and key `"path"`, returns `Some("../utils")`.
fn extract_quoted_value(s: &str, key: &str) -> Option<String> {
    let key_pos = s.find(key)?;
    let after_key = &s[key_pos + key.len()..];
    // Skip whitespace and '='
    let after_eq = after_key.trim_start().strip_prefix('=')?.trim_start();
    // Extract quoted string
    let quote_char = after_eq.chars().next()?;
    if quote_char != '"' && quote_char != '\'' {
        return None;
    }
    let inner = &after_eq[1..];
    let end = inner.find(quote_char)?;
    Some(inner[..end].to_string())
}

/// Install dependencies listed in `turbo.toml` by symlinking path dependencies
/// into a local `turbo_modules/` directory.
fn install_deps() {
    let toml = std::fs::read_to_string("turbo.toml").unwrap_or_else(|_| {
        eprintln!("\x1b[1;31merror\x1b[0m: no turbo.toml found in current directory");
        std::process::exit(1);
    });

    std::fs::create_dir_all("turbo_modules").ok();

    let mut in_deps = false;
    let mut count = 0u32;

    for line in toml.lines() {
        let line = line.trim();
        if line == "[dependencies]" {
            in_deps = true;
            continue;
        }
        if line.starts_with('[') {
            in_deps = false;
            continue;
        }
        if !in_deps || line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Parse: name = { path = "../utils" }
        if let Some((name, rest)) = line.split_once('=') {
            let name = name.trim().trim_matches('"');
            let rest = rest.trim();
            if let Some(path) = extract_quoted_value(rest, "path") {
                let source_path = std::path::Path::new(&path);
                let canonical = match std::fs::canonicalize(source_path) {
                    Ok(p) => p,
                    Err(e) => {
                        eprintln!(
                            "\x1b[1;31merror\x1b[0m: could not resolve dependency path `{}`: {e}",
                            path
                        );
                        std::process::exit(1);
                    }
                };

                let target = std::path::Path::new("turbo_modules").join(name);
                if target.exists() {
                    // Remove existing symlink or directory
                    if target.is_dir() {
                        std::fs::remove_dir_all(&target).ok();
                    } else {
                        std::fs::remove_file(&target).ok();
                    }
                }

                #[cfg(unix)]
                {
                    std::os::unix::fs::symlink(&canonical, &target).unwrap_or_else(|e| {
                        eprintln!(
                            "\x1b[1;31merror\x1b[0m: could not create symlink for `{}`: {e}",
                            name
                        );
                        std::process::exit(1);
                    });
                }

                #[cfg(not(unix))]
                {
                    // On non-Unix platforms, fall back to copying
                    fn copy_dir_recursive(
                        src: &std::path::Path,
                        dst: &std::path::Path,
                    ) -> std::io::Result<()> {
                        std::fs::create_dir_all(dst)?;
                        for entry in std::fs::read_dir(src)? {
                            let entry = entry?;
                            let ty = entry.file_type()?;
                            let dest = dst.join(entry.file_name());
                            if ty.is_dir() {
                                copy_dir_recursive(&entry.path(), &dest)?;
                            } else {
                                std::fs::copy(entry.path(), dest)?;
                            }
                        }
                        Ok(())
                    }
                    copy_dir_recursive(&canonical, &target).unwrap_or_else(|e| {
                        eprintln!(
                            "\x1b[1;31merror\x1b[0m: could not copy dependency `{}`: {e}",
                            name
                        );
                        std::process::exit(1);
                    });
                }

                eprintln!("  \x1b[32m\u{2713}\x1b[0m Installed {} -> {}", name, path);
                count += 1;
            } else if let Some(github_repo) = extract_quoted_value(rest, "github") {
                // Clone from GitHub
                let target = Path::new("turbo_modules").join(name);
                if target.exists() {
                    eprintln!("  \x1b[32m\u{2713}\x1b[0m {} (already installed)", name);
                    count += 1;
                    continue;
                }
                let url = format!("https://github.com/{}.git", github_repo);
                eprintln!(
                    "  \x1b[36m\u{2193}\x1b[0m Cloning {} from github:{}...",
                    name, github_repo
                );
                let output = std::process::Command::new("git")
                    .arg("clone")
                    .arg("--depth=1")
                    .arg(&url)
                    .arg(&target)
                    .output();
                match output {
                    Ok(o) if o.status.success() => {
                        eprintln!(
                            "  \x1b[32m\u{2713}\x1b[0m Installed {} from github:{}",
                            name, github_repo
                        );
                        count += 1;
                    }
                    Ok(o) => {
                        let stderr = String::from_utf8_lossy(&o.stderr);
                        eprintln!(
                            "  \x1b[31m\u{2717}\x1b[0m Failed to install {}: {}",
                            name,
                            stderr.trim()
                        );
                    }
                    Err(e) => {
                        eprintln!("  \x1b[31m\u{2717}\x1b[0m Failed to clone {}: {}", name, e);
                    }
                }
            }
        }
    }

    if count == 0 {
        eprintln!("No dependencies found in turbo.toml");
    } else {
        eprintln!(
            "Installed {} dependenc{}.",
            count,
            if count == 1 { "y" } else { "ies" }
        );
    }
}

/// Update all GitHub dependencies to their latest versions by pulling in each cloned repo.
fn update_deps() {
    let toml = std::fs::read_to_string("turbo.toml").unwrap_or_else(|_| {
        eprintln!("\x1b[1;31merror\x1b[0m: no turbo.toml found in current directory");
        std::process::exit(1);
    });

    let mut in_deps = false;
    let mut count = 0u32;

    for line in toml.lines() {
        let line = line.trim();
        if line == "[dependencies]" {
            in_deps = true;
            continue;
        }
        if line.starts_with('[') {
            in_deps = false;
            continue;
        }
        if !in_deps || line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some((name, rest)) = line.split_once('=') {
            let name = name.trim().trim_matches('"');
            let rest = rest.trim();
            if let Some(github_repo) = extract_quoted_value(rest, "github") {
                let target = Path::new("turbo_modules").join(name);
                if !target.exists() {
                    eprintln!(
                        "  \x1b[33m!\x1b[0m {} not installed — run `turbolang install` first",
                        name
                    );
                    continue;
                }
                eprintln!(
                    "  \x1b[36m\u{2193}\x1b[0m Updating {} from github:{}...",
                    name, github_repo
                );
                let output = std::process::Command::new("git")
                    .arg("-C")
                    .arg(&target)
                    .arg("pull")
                    .arg("--ff-only")
                    .output();
                match output {
                    Ok(o) if o.status.success() => {
                        let stdout = String::from_utf8_lossy(&o.stdout);
                        if stdout.contains("Already up to date") {
                            eprintln!("  \x1b[32m\u{2713}\x1b[0m {} already up to date", name);
                        } else {
                            eprintln!("  \x1b[32m\u{2713}\x1b[0m Updated {}", name);
                        }
                        count += 1;
                    }
                    Ok(o) => {
                        let stderr = String::from_utf8_lossy(&o.stderr);
                        eprintln!(
                            "  \x1b[31m\u{2717}\x1b[0m Failed to update {}: {}",
                            name,
                            stderr.trim()
                        );
                    }
                    Err(e) => {
                        eprintln!("  \x1b[31m\u{2717}\x1b[0m Failed to update {}: {}", name, e);
                    }
                }
            }
        }
    }

    if count == 0 {
        eprintln!("No GitHub dependencies found to update.");
    } else {
        eprintln!(
            "Updated {} dependenc{}.",
            count,
            if count == 1 { "y" } else { "ies" }
        );
    }
}

/// Print a rich error diagnostic using ariadne.
fn report_error(
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

    if let Some(help_text) = help {
        builder = builder.with_help(help_text);
    }

    builder
        .finish()
        .eprint((filename, Source::from(source)))
        .unwrap();
}

fn report_warning(
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

/// Maximum source file size: 50 MB. Files larger than this are rejected
/// to prevent denial-of-service via memory exhaustion.
const MAX_SOURCE_FILE_SIZE: u64 = 50 * 1024 * 1024;

/// Check that a source file does not exceed the maximum allowed size.
/// Prints an error and exits if the file is too large.
fn check_file_size(path: &std::path::Path) {
    match std::fs::metadata(path) {
        Ok(meta) => {
            if meta.len() > MAX_SOURCE_FILE_SIZE {
                eprintln!(
                    "\x1b[1;31merror\x1b[0m: file `{}` is too large ({:.1} MB, max {} MB)",
                    path.display(),
                    meta.len() as f64 / (1024.0 * 1024.0),
                    MAX_SOURCE_FILE_SIZE / (1024 * 1024),
                );
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!(
                "\x1b[1;31merror\x1b[0m: could not stat file `{}`: {e}",
                path.display()
            );
            std::process::exit(1);
        }
    }
}

fn run_file(path: &std::path::Path, verbose: bool) {
    check_file_size(path);

    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "\x1b[1;31merror\x1b[0m: could not read file `{}`: {e}",
                path.display()
            );
            std::process::exit(1);
        }
    };

    let filename = path
        .file_name()
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
                None,
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
            report_error(
                &source,
                &filename,
                &err.message,
                &err.span,
                None,
                Some(err.code),
            );
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
        report_error(
            &source,
            &filename,
            "no functions defined",
            &(0..source.len().min(1)),
            Some("add a `fn main() { ... }` function to get started"),
            None,
        );
        std::process::exit(1);
    }

    if verbose {
        eprintln!(
            "--- AST ({} items, {:.2?}) ---",
            module.items.len(),
            parse_time
        );
        for item in &module.items {
            eprintln!("  {:#?}", item.node);
        }
        eprintln!();
    }

    // Semantic analysis
    let sema_start = std::time::Instant::now();
    let sema_result = turbo_sema::check(&module);
    let sema_time = sema_start.elapsed();

    if verbose {
        eprintln!(
            "--- Sema ({} errors, {} warnings, {:.2?}) ---",
            sema_result.errors.len(),
            sema_result.warnings.len(),
            sema_time
        );
    }

    // Display warnings (non-fatal)
    for w in &sema_result.warnings {
        report_warning(&source, &filename, &w.message, &w.span, Some(w.code));
    }

    if !sema_result.errors.is_empty() {
        for err in &sema_result.errors {
            let help = sema_help(&err.message);
            report_error(
                &source,
                &filename,
                &err.message,
                &err.span,
                help.as_deref(),
                Some(err.code),
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
                eprintln!(
                    "  Total:   {:.2?}",
                    lex_time + parse_time + sema_time + codegen_time
                );
            }
        }
        Err(e) => {
            eprintln!("\x1b[1;31merror\x1b[0m: {e}");
            std::process::exit(1);
        }
    }
}

fn check_file(path: &std::path::Path) {
    check_file_size(path);

    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "\x1b[1;31merror\x1b[0m: could not read file `{}`: {e}",
                path.display()
            );
            std::process::exit(1);
        }
    };

    let filename = path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| "<unknown>".to_string());

    // Lex
    let (tokens, lex_errors) = turbo_lexer::tokenize(&source);

    if !lex_errors.is_empty() {
        for span in &lex_errors {
            let snippet = &source[span.clone()];
            report_error(
                &source,
                &filename,
                &format!("unexpected character `{snippet}`"),
                span,
                Some("remove this character or check for typos"),
                None,
            );
        }
        std::process::exit(1);
    }

    // Parse
    let (mut module, parse_errors) = turbo_parser::parse(tokens);

    if !parse_errors.is_empty() {
        for err in &parse_errors {
            report_error(
                &source,
                &filename,
                &err.message,
                &err.span,
                None,
                Some(err.code),
            );
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

    // Semantic analysis
    let sema_result = turbo_sema::check(&module);

    for w in &sema_result.warnings {
        report_warning(&source, &filename, &w.message, &w.span, Some(w.code));
    }

    if !sema_result.errors.is_empty() {
        for err in &sema_result.errors {
            let help = sema_help(&err.message);
            report_error(
                &source,
                &filename,
                &err.message,
                &err.span,
                help.as_deref(),
                Some(err.code),
            );
        }
        std::process::exit(1);
    }

    eprintln!("\x1b[32m\u{2713}\x1b[0m No errors in `{}`", filename);
}

fn test_file(file: Option<PathBuf>) {
    // Collect test files: either a specific file, or all *_test.tb / test_*.tb in tests/
    let files: Vec<PathBuf> = if let Some(f) = file {
        if f.is_dir() {
            collect_test_files(&f)
        } else {
            vec![f]
        }
    } else {
        // Look for tests/ directory
        let tests_dir = Path::new("tests");
        if tests_dir.is_dir() {
            collect_test_files(tests_dir)
        } else {
            eprintln!("\x1b[1;31merror\x1b[0m: no file specified and no `tests/` directory found");
            eprintln!("  Usage: turbolang test <file.tb>");
            std::process::exit(1);
        }
    };

    if files.is_empty() {
        eprintln!("No test files found.");
        std::process::exit(0);
    }

    let turbo_exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("turbolang"));

    let mut total_passed = 0u32;
    let mut total_failed = 0u32;

    for path in &files {
        let source = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!(
                    "\x1b[1;31merror\x1b[0m: could not read file `{}`: {e}",
                    path.display()
                );
                std::process::exit(1);
            }
        };

        let filename = path
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| "<unknown>".to_string());

        // Lex
        let (tokens, lex_errors) = turbo_lexer::tokenize(&source);
        if !lex_errors.is_empty() {
            for span in &lex_errors {
                let snippet = &source[span.clone()];
                report_error(
                    &source,
                    &filename,
                    &format!("unexpected character `{snippet}`"),
                    span,
                    Some("remove this character or check for typos"),
                    None,
                );
            }
            std::process::exit(1);
        }

        // Parse
        let (mut module, parse_errors) = turbo_parser::parse(tokens);
        if !parse_errors.is_empty() {
            for err in &parse_errors {
                report_error(
                    &source,
                    &filename,
                    &err.message,
                    &err.span,
                    None,
                    Some(err.code),
                );
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

        // Collect @test function names
        let test_names: Vec<String> = module
            .items
            .iter()
            .filter_map(|item| {
                if let Item::Function(f) = &item.node {
                    if f.is_test {
                        Some(f.name.clone())
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect();

        if test_names.is_empty() {
            continue; // No tests in this file
        }

        // Semantic analysis in test mode (main not required)
        let sema_result = turbo_sema::check_test(&module);
        for w in &sema_result.warnings {
            report_warning(&source, &filename, &w.message, &w.span, Some(w.code));
        }
        if !sema_result.errors.is_empty() {
            for err in &sema_result.errors {
                let help = sema_help(&err.message);
                report_error(
                    &source,
                    &filename,
                    &err.message,
                    &err.span,
                    help.as_deref(),
                    Some(err.code),
                );
            }
            std::process::exit(1);
        }

        // Run each test in its own subprocess
        let canonical_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        for name in &test_names {
            let output = std::process::Command::new(&turbo_exe)
                .arg("test-run-fn")
                .arg(&canonical_path)
                .arg("--func")
                .arg(name)
                .output();

            match output {
                Ok(result) => {
                    if result.status.success() {
                        eprintln!("  \x1b[32mPASS\x1b[0m  {name}");
                        total_passed += 1;
                    } else {
                        // Print captured stderr (assertion failure messages)
                        let stderr = String::from_utf8_lossy(&result.stderr);
                        for line in stderr.lines() {
                            if !line.is_empty() {
                                eprintln!("        {line}");
                            }
                        }
                        eprintln!("  \x1b[31mFAIL\x1b[0m  {name}");
                        total_failed += 1;
                    }
                }
                Err(e) => {
                    eprintln!("  \x1b[31mFAIL\x1b[0m  {name} (failed to spawn: {e})");
                    total_failed += 1;
                }
            }
        }
    }

    // Print summary
    eprintln!("{total_passed} passed, {total_failed} failed");

    if total_failed > 0 {
        std::process::exit(1);
    }
}

/// Run benchmark files and report timing.
/// If a file is given, benchmark that file. Otherwise, look for bench_*.tb
/// files in a `benchmarks/` directory.
fn bench_file(file: Option<PathBuf>, iterations: u32, quiet: bool) {
    let files: Vec<PathBuf> = if let Some(f) = file {
        if f.is_dir() {
            collect_bench_files(&f)
        } else {
            vec![f]
        }
    } else {
        let bench_dir = Path::new("benchmarks");
        if bench_dir.is_dir() {
            collect_bench_files(bench_dir)
        } else {
            eprintln!(
                "\x1b[1;31merror\x1b[0m: no file specified and no `benchmarks/` directory found"
            );
            eprintln!("  Usage: turbolang bench <file.tb>");
            std::process::exit(1);
        }
    };

    if files.is_empty() {
        eprintln!("No benchmark files found.");
        std::process::exit(0);
    }

    let turbo_exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("turbolang"));

    eprintln!("\x1b[1mTurbo Benchmark Suite\x1b[0m");
    eprintln!("\x1b[90m=====================\x1b[0m");
    eprintln!("\x1b[90mIterations: {iterations}\x1b[0m");
    eprintln!();

    let mut total = 0u32;
    let mut passed = 0u32;

    for path in &files {
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "<unknown>".to_string());

        total += 1;
        eprintln!("\x1b[1;36m--- {name} ---\x1b[0m");

        // Show expected output from comment if present
        let source = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!(
                    "  \x1b[31merror\x1b[0m: could not read `{}`: {e}",
                    path.display()
                );
                continue;
            }
        };
        let expected: Option<String> = source
            .lines()
            .find(|l| l.starts_with("// Expected output:"))
            .map(|l| {
                l.trim_start_matches("// Expected output:")
                    .trim()
                    .to_string()
            });
        if let Some(ref exp) = expected {
            eprintln!("  \x1b[90mexpected: {exp}\x1b[0m");
        }

        // JIT mode: run N iterations, report median
        let mut jit_times = Vec::new();
        let mut jit_output = String::new();
        for _ in 0..iterations {
            let start = std::time::Instant::now();
            let output = std::process::Command::new(&turbo_exe)
                .arg("run")
                .arg(path)
                .output();
            let elapsed = start.elapsed();
            match output {
                Ok(result) => {
                    jit_times.push(elapsed);
                    if jit_output.is_empty() {
                        jit_output = String::from_utf8_lossy(&result.stdout).trim().to_string();
                    }
                }
                Err(e) => {
                    eprintln!("  \x1b[31merror\x1b[0m: failed to run JIT: {e}");
                    break;
                }
            }
        }

        if !jit_times.is_empty() {
            jit_times.sort();
            let median = jit_times[jit_times.len() / 2];
            if quiet {
                eprintln!(
                    "  \x1b[33mJIT:\x1b[0m  \x1b[90m{:.3}s median ({} runs)\x1b[0m",
                    median.as_secs_f64(),
                    jit_times.len()
                );
            } else {
                eprintln!(
                    "  \x1b[33mJIT:\x1b[0m  {} \x1b[90m({:.3}s median, {} runs)\x1b[0m",
                    jit_output,
                    median.as_secs_f64(),
                    jit_times.len()
                );
            }
        }

        // AOT mode: build then run N iterations, report median
        let tmp_bin = std::env::temp_dir().join(format!("turbo_bench_{name}"));
        let build_result = std::process::Command::new(&turbo_exe)
            .arg("build")
            .arg(path)
            .arg("-o")
            .arg(&tmp_bin)
            .output();

        match build_result {
            Ok(result) if result.status.success() => {
                let mut aot_times = Vec::new();
                let mut aot_output = String::new();
                for _ in 0..iterations {
                    let start = std::time::Instant::now();
                    let output = std::process::Command::new(&tmp_bin).output();
                    let elapsed = start.elapsed();
                    match output {
                        Ok(result) => {
                            aot_times.push(elapsed);
                            if aot_output.is_empty() {
                                aot_output =
                                    String::from_utf8_lossy(&result.stdout).trim().to_string();
                            }
                        }
                        Err(e) => {
                            eprintln!("  \x1b[31merror\x1b[0m: failed to run AOT binary: {e}");
                            break;
                        }
                    }
                }

                if !aot_times.is_empty() {
                    aot_times.sort();
                    let median = aot_times[aot_times.len() / 2];
                    if quiet {
                        eprintln!(
                            "  \x1b[33mAOT:\x1b[0m  \x1b[90m{:.3}s median ({} runs)\x1b[0m",
                            median.as_secs_f64(),
                            aot_times.len()
                        );
                    } else {
                        eprintln!(
                            "  \x1b[33mAOT:\x1b[0m  {} \x1b[90m({:.3}s median, {} runs)\x1b[0m",
                            aot_output,
                            median.as_secs_f64(),
                            aot_times.len()
                        );
                    }
                }

                // Check if outputs match
                if !jit_output.is_empty() && !aot_output.is_empty() {
                    if jit_output == aot_output {
                        eprintln!("  \x1b[32moutputs match\x1b[0m");
                        passed += 1;
                    } else {
                        eprintln!("  \x1b[31moutputs differ!\x1b[0m");
                        eprintln!("    JIT: {jit_output}");
                        eprintln!("    AOT: {aot_output}");
                    }
                } else if !jit_output.is_empty() {
                    passed += 1;
                }

                // Cleanup temp binary
                std::fs::remove_file(&tmp_bin).ok();
            }
            _ => {
                eprintln!("  \x1b[90m(AOT build failed, skipping)\x1b[0m");
                if !jit_output.is_empty() {
                    passed += 1;
                }
            }
        }

        eprintln!();
    }

    eprintln!("\x1b[1mResults: {passed}/{total} benchmarks passed (JIT/AOT output match)\x1b[0m");
}

/// Collect benchmark files from a directory: files matching bench_*.tb
fn collect_bench_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "tb").unwrap_or(false) {
                let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                if stem.starts_with("bench_") {
                    files.push(path);
                }
            }
        }
    }
    files.sort();
    files
}

/// Internal: compile a file and run a single named function via JIT.
/// Used by `turbolang test` to run each @test in its own subprocess.
fn test_run_fn(path: &std::path::Path, fn_name: &str) {
    check_file_size(path);

    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "\x1b[1;31merror\x1b[0m: could not read file `{}`: {e}",
                path.display()
            );
            std::process::exit(1);
        }
    };

    let filename = path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| "<unknown>".to_string());

    // Lex
    let (tokens, lex_errors) = turbo_lexer::tokenize(&source);
    if !lex_errors.is_empty() {
        for span in &lex_errors {
            let snippet = &source[span.clone()];
            report_error(
                &source,
                &filename,
                &format!("unexpected character `{snippet}`"),
                span,
                Some("remove this character or check for typos"),
                None,
            );
        }
        std::process::exit(1);
    }

    // Parse
    let (mut module, parse_errors) = turbo_parser::parse(tokens);
    if !parse_errors.is_empty() {
        for err in &parse_errors {
            report_error(
                &source,
                &filename,
                &err.message,
                &err.span,
                None,
                Some(err.code),
            );
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

    // Semantic analysis in test mode
    let sema_result = turbo_sema::check_test(&module);
    for w in &sema_result.warnings {
        eprintln!("warning: {}", w.message);
    }
    if !sema_result.errors.is_empty() {
        for err in &sema_result.errors {
            eprintln!("error: {}", err.message);
        }
        std::process::exit(1);
    }

    // Compile and run
    match turbo_codegen_cranelift::jit_run_function(&module, fn_name) {
        Ok(()) => {}
        Err(e) => {
            eprintln!("\x1b[1;31merror\x1b[0m: {e}");
            std::process::exit(1);
        }
    }
}

/// Collect test files from a directory: files matching *_test.tb or test_*.tb
fn collect_test_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "tb").unwrap_or(false) {
                files.push(path);
            }
        }
    }
    // Also recurse into subdirectories
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                files.extend(collect_test_files(&path));
            }
        }
    }
    files.sort();
    files
}

fn build_file(
    path: &std::path::Path,
    output: Option<&std::path::Path>,
    verbose: bool,
    use_llvm: bool,
) {
    check_file_size(path);

    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "\x1b[1;31merror\x1b[0m: could not read file `{}`: {e}",
                path.display()
            );
            std::process::exit(1);
        }
    };

    let filename = path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| "<unknown>".to_string());

    // Default output: project name from turbo.toml if available, else filename without .tb
    let default_output = if output.is_none() {
        read_project_name().unwrap_or_else(|| {
            path.file_stem()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("a.out"))
        })
    } else {
        path.file_stem()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("a.out"))
    };
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
                None,
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
            report_error(
                &source,
                &filename,
                &err.message,
                &err.span,
                None,
                Some(err.code),
            );
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
        report_error(
            &source,
            &filename,
            "no functions defined",
            &(0..source.len().min(1)),
            Some("add a `fn main() { ... }` function to get started"),
            None,
        );
        std::process::exit(1);
    }

    if verbose {
        eprintln!(
            "--- AST ({} items, {:.2?}) ---",
            module.items.len(),
            parse_time
        );
    }

    // Semantic analysis
    let sema_start = std::time::Instant::now();
    let sema_result = turbo_sema::check(&module);
    let sema_time = sema_start.elapsed();

    for w in &sema_result.warnings {
        report_warning(&source, &filename, &w.message, &w.span, Some(w.code));
    }

    if !sema_result.errors.is_empty() {
        for err in &sema_result.errors {
            let help = sema_help(&err.message);
            report_error(
                &source,
                &filename,
                &err.message,
                &err.span,
                help.as_deref(),
                Some(err.code),
            );
        }
        std::process::exit(1);
    }

    // Compile to native binary
    let codegen_start = std::time::Instant::now();
    let backend_name = if use_llvm { "LLVM" } else { "Cranelift" };
    let codegen_result: Result<(), String> = if use_llvm {
        #[cfg(feature = "llvm")]
        {
            turbo_codegen_llvm::aot_compile(&module, output_path).map_err(|e| e.to_string())
        }
        #[cfg(not(feature = "llvm"))]
        {
            Err("LLVM backend not available — rebuild with --features llvm".to_string())
        }
    } else {
        turbo_codegen_cranelift::aot_compile(&module, output_path, true).map_err(|e| e.to_string())
    };
    match codegen_result {
        Ok(()) => {
            let codegen_time = codegen_start.elapsed();
            eprintln!(
                "\x1b[32m\u{2713}\x1b[0m Compiled to {} ({})",
                output_path.display(),
                backend_name
            );
            if verbose {
                eprintln!("\n--- Timing ---");
                eprintln!("  Lex:     {:.2?}", lex_time);
                eprintln!("  Parse:   {:.2?}", parse_time);
                eprintln!("  Sema:    {:.2?}", sema_time);
                eprintln!("  Codegen: {:.2?}", codegen_time);
                eprintln!(
                    "  Total:   {:.2?}",
                    lex_time + parse_time + sema_time + codegen_time
                );
            }
        }
        Err(e) => {
            eprintln!("\x1b[1;31merror\x1b[0m: {e}");
            std::process::exit(1);
        }
    }
}

/// Resolve the file path for an import.
/// Resolution order:
/// 1. Relative path from `base_dir` (existing behavior for `./foo` paths)
/// 2. `turbo_modules/{module_name}/src/lib.tb` (package entry point)
/// 3. `turbo_modules/{module_name}/src/{module_name}.tb` (named entry point)
fn resolve_import_path(base_dir: &Path, import_path: &str) -> PathBuf {
    // If the path starts with "./" or "../", resolve relative to base_dir
    if import_path.starts_with("./") || import_path.starts_with("../") {
        let mut path = base_dir.join(import_path);
        if path.extension().is_none() {
            path.set_extension("tb");
        }
        return path;
    }

    // First try the old relative behavior (for backwards compatibility)
    let mut relative_path = base_dir.join(import_path);
    if relative_path.extension().is_none() {
        relative_path.set_extension("tb");
    }
    if relative_path.exists() {
        return relative_path;
    }

    // Try turbo_modules/{module_name}/src/lib.tb
    // Walk up from base_dir to find the project root (where turbo_modules lives)
    let mut search_dir = base_dir.to_path_buf();
    loop {
        let modules_dir = search_dir.join("turbo_modules");
        if modules_dir.is_dir() {
            let lib_path = modules_dir.join(import_path).join("src/lib.tb");
            if lib_path.exists() {
                return lib_path;
            }
            let named_path = modules_dir
                .join(import_path)
                .join("src")
                .join(format!("{}.tb", import_path));
            if named_path.exists() {
                return named_path;
            }
            // Module dir exists but no source found -- fall through to return the lib.tb
            // path so we get a clear error message
            if modules_dir.join(import_path).is_dir() {
                return lib_path;
            }
        }
        if !search_dir.pop() {
            break;
        }
    }

    // Fallback: return the relative path (will produce an error downstream)
    relative_path
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
                format!(
                    "could not resolve import path `{}`: {e}",
                    resolved_path.display()
                )
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
                format!(
                    "could not read imported file `{}`: {e}",
                    resolved_path.display()
                )
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
                    Item::Agent(a) if names.contains(&a.name) => {
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
                    Item::Agent(a) => &a.name == name,
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
    module
        .items
        .retain(|item| !matches!(&item.node, Item::Import { .. }));
    let mut new_items = import_items;
    new_items.append(&mut module.items);
    module.items = new_items;

    Ok(())
}

/// Generate contextual help text for common sema error patterns.
fn sema_help(message: &str) -> Option<String> {
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
    if message.contains("expects") && message.contains("argument(s) but") {
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
fn extract_backtick_name(message: &str) -> Option<&str> {
    let start = message.find('`')? + 1;
    let end = message[start..].find('`')? + start;
    Some(&message[start..end])
}

// =============================================================================
// turbolang explain -- Print description for an error code
// =============================================================================

/// Returns a detailed explanation with code example for well-known error codes.
fn detailed_explanation(code: ErrorCode) -> Option<&'static str> {
    match code {
        ErrorCode::E0001 => Some(
            "The parser encountered a token it did not expect at this position.\n\n\
             Example that triggers this error:\n\n\
             \x1b[90m    fn main() {\n\
             \x1b[90m        let x = +\n\
             \x1b[90m    }\x1b[0m\n\n\
             How to fix: check for typos, missing operands, or mismatched \
             delimiters near the reported location.",
        ),
        ErrorCode::E0002 => Some(
            "An identifier (variable name, function name, type name) was expected \
             but something else was found.\n\n\
             Example that triggers this error:\n\n\
             \x1b[90m    fn 123bad() { }\x1b[0m\n\n\
             How to fix: use a valid identifier. Identifiers must start with a \
             letter or underscore and contain only letters, digits, and underscores.",
        ),
        ErrorCode::E0003 => Some(
            "Expressions or blocks are nested too deeply, exceeding the parser's \
             maximum depth limit. This usually indicates accidentally recursive or \
             deeply nested code.\n\n\
             How to fix: refactor deeply nested expressions into helper functions \
             or intermediate variables.",
        ),
        ErrorCode::E0100 => Some(
            "Two types were expected to match but they differ.\n\n\
             Example that triggers this error:\n\n\
             \x1b[90m    fn greet() -> str {\n\
             \x1b[90m        42  // expected str, found i64\n\
             \x1b[90m    }\x1b[0m\n\n\
             How to fix: ensure the value's type matches the expected type. Use \
             `to_str()` for string conversions, or adjust the type annotation.",
        ),
        ErrorCode::E0101 => Some(
            "Arithmetic operators (+, -, *, /, %) require numeric operands \
             (i32, i64, u32, u64, f32, f64).\n\n\
             Example that triggers this error:\n\n\
             \x1b[90m    fn main() {\n\
             \x1b[90m        let x = true + 1  // bool is not numeric\n\
             \x1b[90m    }\x1b[0m\n\n\
             How to fix: convert the value to a number first, or use a different \
             operation appropriate for the type.",
        ),
        ErrorCode::E0102 => Some(
            "Both sides of an arithmetic operation must have the same type.\n\n\
             Example that triggers this error:\n\n\
             \x1b[90m    fn main() {\n\
             \x1b[90m        let x: i32 = 10\n\
             \x1b[90m        let y: i64 = 20\n\
             \x1b[90m        let z = x + y  // i32 + i64 mismatch\n\
             \x1b[90m    }\x1b[0m\n\n\
             How to fix: ensure both operands are the same numeric type.",
        ),
        ErrorCode::E0103 => Some(
            "Comparison operators (==, !=, <, >, <=, >=) require compatible types \
             on both sides.\n\n\
             Example that triggers this error:\n\n\
             \x1b[90m    fn main() {\n\
             \x1b[90m        let x = 42 == \"hello\"  // i64 vs str\n\
             \x1b[90m    }\x1b[0m\n\n\
             How to fix: compare values of the same type.",
        ),
        ErrorCode::E0200 => Some(
            "A `match` expression must cover all possible values of the matched \
             type. For enums, every variant must have a corresponding arm (or use \
             a wildcard `_` arm).\n\n\
             Example that triggers this error:\n\n\
             \x1b[90m    type Color { Red  Green  Blue }\n\
             \x1b[90m    fn name(c: Color) -> str {\n\
             \x1b[90m        match c {\n\
             \x1b[90m            Color::Red => \"red\"\n\
             \x1b[90m            Color::Green => \"green\"\n\
             \x1b[90m            // missing Blue!\n\
             \x1b[90m        }\n\
             \x1b[90m    }\x1b[0m\n\n\
             How to fix: add the missing variant arms, or add a `_ => ...` \
             wildcard arm to handle all remaining cases.",
        ),
        ErrorCode::E0201 => Some(
            "A `match` expression must have at least one arm.\n\n\
             Example that triggers this error:\n\n\
             \x1b[90m    fn main() {\n\
             \x1b[90m        match x { }\n\
             \x1b[90m    }\x1b[0m\n\n\
             How to fix: add at least one pattern arm to the match expression.",
        ),
        ErrorCode::E0300 => Some(
            "A variable was referenced that has not been defined in the current \
             scope or any enclosing scope.\n\n\
             Example that triggers this error:\n\n\
             \x1b[90m    fn main() {\n\
             \x1b[90m        print(x)  // x is not defined\n\
             \x1b[90m    }\x1b[0m\n\n\
             How to fix: define the variable with `let` before using it, or \
             check for typos in the variable name.",
        ),
        ErrorCode::E0400 => Some(
            "An internal error occurred during code generation. This usually \
             indicates a compiler bug.\n\n\
             How to fix: please report this issue with the source code that \
             triggered it.",
        ),
        ErrorCode::E0401 => Some(
            "A variable was referenced during code generation but was not found \
             in the compiled symbol table.\n\n\
             This can happen if semantic analysis missed an error. Please report \
             the issue if you see this.",
        ),
        _ => None,
    }
}

fn explain_error(code_str: &str) {
    if let Some(code) = ErrorCode::parse(code_str) {
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
        eprintln!("  Error codes range from E0001 to E0515.");
        eprintln!("  Example: turbolang explain E0100");
        std::process::exit(1);
    }
}

// =============================================================================
// turbolang doc -- Generate markdown documentation from source
// =============================================================================

/// Extract doc comments (lines starting with `///`) from source text.
/// Returns a map from line number (0-indexed) to the collected doc comment lines
/// for the item that starts at that line.
fn extract_doc_comments(source: &str) -> HashMap<usize, Vec<String>> {
    let mut docs: HashMap<usize, Vec<String>> = HashMap::new();
    let lines: Vec<&str> = source.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].trim().starts_with("///") {
            let mut comments = Vec::new();
            while i < lines.len() && lines[i].trim().starts_with("///") {
                comments.push(lines[i].trim().trim_start_matches("///").trim().to_string());
                i += 1;
            }
            // Skip any decorator lines (@derive, etc.) between doc comment and item
            while i < lines.len() && lines[i].trim().starts_with('@') {
                i += 1;
            }
            // i now points to the item after the doc comments
            if i < lines.len() {
                docs.insert(i, comments);
            }
        }
        i += 1;
    }
    docs
}

/// A documentation item extracted from source text scanning.
#[derive(Debug)]
enum DocItem {
    Function {
        signature: String,
        doc: Vec<String>,
    },
    Struct {
        name: String,
        fields: Vec<String>,
        doc: Vec<String>,
    },
    Enum {
        name: String,
        variants: Vec<String>,
        doc: Vec<String>,
    },
    Trait {
        name: String,
        methods: Vec<String>,
        doc: Vec<String>,
    },
    Impl {
        target: String,
        methods: Vec<String>,
    },
    Agent {
        name: String,
        model: Option<String>,
        tools: Vec<String>,
        doc: Vec<String>,
    },
}

/// Format a TypeExpr to a human-readable string.
fn format_type_expr(ty: &turbo_ast::TypeExpr) -> String {
    match ty {
        turbo_ast::TypeExpr::Named(n) => n.clone(),
        turbo_ast::TypeExpr::Unit => "()".to_string(),
        turbo_ast::TypeExpr::Array(inner) => format!("[{}]", format_type_expr(&inner.node)),
        turbo_ast::TypeExpr::FnType { params, ret } => {
            let p: Vec<String> = params.iter().map(|p| format_type_expr(&p.node)).collect();
            format!("fn({}) -> {}", p.join(", "), format_type_expr(&ret.node))
        }
        turbo_ast::TypeExpr::Result { ok_type, err_type } => {
            format!(
                "{} ! {}",
                format_type_expr(&ok_type.node),
                format_type_expr(&err_type.node)
            )
        }
        turbo_ast::TypeExpr::Optional(inner) => format!("{}?", format_type_expr(&inner.node)),
        turbo_ast::TypeExpr::Future(inner) => format!("Future<{}>", format_type_expr(&inner.node)),
        turbo_ast::TypeExpr::Inferred => "_".to_string(),
    }
}

/// Format a function signature from an AST FnDef.
fn format_fn_signature(f: &turbo_ast::FnDef) -> String {
    let params: Vec<String> = f
        .params
        .iter()
        .map(|p| format!("{}: {}", p.name, format_type_expr(&p.ty.node)))
        .collect();

    let ret = match &f.return_type {
        Some(rt) => format!(" -> {}", format_type_expr(&rt.node)),
        None => String::new(),
    };

    let async_prefix = if f.is_async { "async " } else { "" };
    let tool_prefix = if f.is_tool { "tool " } else { "" };

    format!(
        "{}{}fn {}({}){}",
        tool_prefix,
        async_prefix,
        f.name,
        params.join(", "),
        ret
    )
}

/// Scan source lines for struct definitions and their fields.
fn scan_structs(lines: &[&str], doc_comments: &HashMap<usize, Vec<String>>) -> Vec<DocItem> {
    let mut items = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        let is_struct = trimmed.starts_with("struct ") || trimmed.starts_with("pub struct ");
        if is_struct && trimmed.contains('{') {
            let name = trimmed
                .trim_start_matches("pub ")
                .trim_start_matches("struct ")
                .split('{')
                .next()
                .unwrap_or("")
                .trim()
                .to_string();

            let doc = doc_comments.get(&i).cloned().unwrap_or_default();
            let mut fields = Vec::new();

            // Check if this is a single-line struct: `struct Foo { x: i64, y: i64 }`
            if trimmed.contains('}') {
                // Extract fields from between { and }
                if let Some(start) = trimmed.find('{') {
                    if let Some(end) = trimmed.rfind('}') {
                        let body = trimmed[start + 1..end].trim();
                        if !body.is_empty() {
                            for field_str in body.split(',') {
                                let f = field_str.trim();
                                if !f.is_empty() && !f.starts_with("//") {
                                    fields.push(f.to_string());
                                }
                            }
                        }
                    }
                }
            } else {
                // Multi-line struct: scan subsequent lines for fields
                i += 1;
                while i < lines.len() {
                    let field_line = lines[i].trim();
                    if field_line == "}" || field_line.starts_with('}') {
                        break;
                    }
                    if !field_line.is_empty() && !field_line.starts_with("//") {
                        fields.push(field_line.trim_end_matches(',').to_string());
                    }
                    i += 1;
                }
            }

            items.push(DocItem::Struct { name, fields, doc });
        }
        i += 1;
    }
    items
}

/// Scan source lines for enum definitions (using `type Name {` syntax).
fn scan_enums(lines: &[&str], doc_comments: &HashMap<usize, Vec<String>>) -> Vec<DocItem> {
    let mut items = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        let is_enum = (trimmed.starts_with("type ") || trimmed.starts_with("pub type "))
            && trimmed.contains('{')
            && !trimmed.contains("fn ")
            && !trimmed.contains("let ");
        if is_enum {
            let after_type = trimmed
                .trim_start_matches("pub ")
                .trim_start_matches("type ");
            let name = after_type
                .split('{')
                .next()
                .unwrap_or("")
                .split(':')
                .next()
                .unwrap_or("")
                .trim()
                .to_string();

            let doc = doc_comments.get(&i).cloned().unwrap_or_default();
            let mut variants = Vec::new();
            i += 1;
            while i < lines.len() {
                let variant_line = lines[i].trim();
                if variant_line == "}" || variant_line.starts_with('}') {
                    break;
                }
                if !variant_line.is_empty() && !variant_line.starts_with("//") {
                    variants.push(variant_line.trim_end_matches(',').to_string());
                }
                i += 1;
            }

            items.push(DocItem::Enum {
                name,
                variants,
                doc,
            });
        }
        i += 1;
    }
    items
}

/// Scan source lines for trait definitions.
fn scan_traits(lines: &[&str], doc_comments: &HashMap<usize, Vec<String>>) -> Vec<DocItem> {
    let mut items = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        let is_trait = (trimmed.starts_with("trait ") || trimmed.starts_with("pub trait "))
            && trimmed.contains('{');
        if is_trait {
            let name = trimmed
                .trim_start_matches("pub ")
                .trim_start_matches("trait ")
                .split('{')
                .next()
                .unwrap_or("")
                .trim()
                .to_string();

            let doc = doc_comments.get(&i).cloned().unwrap_or_default();
            let mut methods = Vec::new();
            i += 1;
            let mut brace_depth = 1;
            while i < lines.len() && brace_depth > 0 {
                let method_line = lines[i].trim();
                brace_depth += method_line.matches('{').count();
                brace_depth -= method_line.matches('}').count();
                if (method_line.starts_with("fn ") || method_line.starts_with("pub fn "))
                    && method_line.contains('(')
                {
                    let sig = method_line.split('{').next().unwrap_or(method_line).trim();
                    methods.push(sig.to_string());
                }
                i += 1;
            }

            items.push(DocItem::Trait { name, methods, doc });
        }
        i += 1;
    }
    items
}

/// Scan source lines for impl blocks and their methods.
fn scan_impls(lines: &[&str]) -> Vec<DocItem> {
    let mut items = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        if trimmed.starts_with("impl ") && trimmed.contains('{') {
            let target = trimmed
                .trim_start_matches("impl ")
                .split('{')
                .next()
                .unwrap_or("")
                .trim()
                .to_string();

            let mut methods = Vec::new();
            i += 1;
            let mut brace_depth = 1;
            while i < lines.len() && brace_depth > 0 {
                let method_line = lines[i].trim();
                brace_depth += method_line.matches('{').count();
                brace_depth -= method_line.matches('}').count();
                if (method_line.starts_with("fn ")
                    || method_line.starts_with("pub fn ")
                    || method_line.starts_with("async fn ")
                    || method_line.starts_with("pub async fn "))
                    && method_line.contains('(')
                {
                    let sig = method_line.split('{').next().unwrap_or(method_line).trim();
                    methods.push(sig.to_string());
                }
                i += 1;
            }

            if !methods.is_empty() {
                items.push(DocItem::Impl { target, methods });
            }
        }
        i += 1;
    }
    items
}

/// Scan source lines for agent declarations.
fn scan_agents(lines: &[&str], doc_comments: &HashMap<usize, Vec<String>>) -> Vec<DocItem> {
    let mut items = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        let is_agent = (trimmed.starts_with("agent ") || trimmed.starts_with("pub agent "))
            && trimmed.contains('{');
        if is_agent {
            let name = trimmed
                .trim_start_matches("pub ")
                .trim_start_matches("agent ")
                .split('{')
                .next()
                .unwrap_or("")
                .trim()
                .to_string();

            let doc = doc_comments.get(&i).cloned().unwrap_or_default();
            let mut model = None;
            let mut tools = Vec::new();
            i += 1;
            let mut brace_depth = 1;
            while i < lines.len() && brace_depth > 0 {
                let agent_line = lines[i].trim();
                brace_depth += agent_line.matches('{').count();
                brace_depth -= agent_line.matches('}').count();
                if agent_line.starts_with("model:") || agent_line.starts_with("model =") {
                    model = Some(
                        agent_line
                            .split([':', '='])
                            .nth(1)
                            .unwrap_or("")
                            .trim()
                            .trim_matches('"')
                            .to_string(),
                    );
                }
                if agent_line.starts_with("tool fn ") || agent_line.starts_with("pub tool fn ") {
                    let sig = agent_line.split('{').next().unwrap_or(agent_line).trim();
                    tools.push(sig.to_string());
                }
                i += 1;
            }

            items.push(DocItem::Agent {
                name,
                model,
                tools,
                doc,
            });
        }
        i += 1;
    }
    items
}

/// Scan for top-level `fn` and `async fn` definitions in source text.
fn scan_functions(lines: &[&str], doc_comments: &HashMap<usize, Vec<String>>) -> Vec<DocItem> {
    let mut items = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        let is_fn = (trimmed.starts_with("fn ")
            || trimmed.starts_with("pub fn ")
            || trimmed.starts_with("async fn ")
            || trimmed.starts_with("pub async fn "))
            && trimmed.contains('(');

        if is_fn {
            let sig = trimmed
                .split('{')
                .next()
                .unwrap_or(trimmed)
                .trim()
                .to_string();
            let doc = doc_comments.get(&i).cloned().unwrap_or_default();
            items.push(DocItem::Function {
                signature: sig,
                doc,
            });
        }
        i += 1;
    }
    items
}

fn doc_file(path: &std::path::Path) {
    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "\x1b[1;31merror\x1b[0m: could not read file `{}`: {e}",
                path.display()
            );
            std::process::exit(1);
        }
    };

    let filename = path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| "<unknown>".to_string());

    let doc_comments = extract_doc_comments(&source);
    let lines: Vec<&str> = source.lines().collect();

    // Try parsing with the AST for functions (works for Phase 1 files)
    let (tokens, lex_errors) = turbo_lexer::tokenize(&source);
    let ast_functions = if lex_errors.is_empty() {
        let (module, parse_errors) = turbo_parser::parse(tokens);
        if parse_errors.is_empty() {
            Some(module)
        } else {
            None
        }
    } else {
        None
    };

    // Collect all items via source scanning
    let scanned_functions = scan_functions(&lines, &doc_comments);
    let structs = scan_structs(&lines, &doc_comments);
    let enums = scan_enums(&lines, &doc_comments);
    let traits = scan_traits(&lines, &doc_comments);
    let impls = scan_impls(&lines);
    let agents = scan_agents(&lines, &doc_comments);

    // --- Generate markdown ---
    let mut out = String::new();
    out.push_str(&format!("# Documentation for {}\n", filename));

    // Functions section
    let has_functions = ast_functions.is_some() || !scanned_functions.is_empty();
    if has_functions {
        out.push_str("\n## Functions\n");

        if let Some(ref module) = ast_functions {
            // Use AST for accurate signatures and doc comments
            for item in &module.items {
                if let turbo_ast::Item::Function(f) = &item.node {
                    let sig = format_fn_signature(f);
                    // Prefer AST doc field, fall back to source-scanned doc comments
                    let doc = if let Some(ref d) = f.doc {
                        vec![d.clone()]
                    } else {
                        let fn_line = source[..item.span.start].matches('\n').count();
                        doc_comments.get(&fn_line).cloned().unwrap_or_default()
                    };

                    out.push_str(&format!("\n### `{}`\n", sig));
                    if !doc.is_empty() {
                        out.push_str(&format!("{}\n", doc.join("\n")));
                    }
                }
            }
        } else {
            // Fallback: use scanned functions
            for item in &scanned_functions {
                if let DocItem::Function { signature, doc } = item {
                    out.push_str(&format!("\n### `{}`\n", signature));
                    if !doc.is_empty() {
                        out.push_str(&format!("{}\n", doc.join("\n")));
                    }
                }
            }
        }
    }

    // Structs section — prefer AST for accurate field types, fall back to scanner
    let has_ast_structs = ast_functions.as_ref().map_or(false, |module| {
        module
            .items
            .iter()
            .any(|item| matches!(&item.node, turbo_ast::Item::Struct(_)))
    });

    if has_ast_structs {
        let module = ast_functions.as_ref().unwrap();
        out.push_str("\n## Structs\n");
        for item in &module.items {
            if let turbo_ast::Item::Struct(s) = &item.node {
                out.push_str(&format!("\n### `struct {}`\n", s.name));
                let doc = if let Some(ref d) = s.doc {
                    vec![d.clone()]
                } else {
                    let struct_line = source[..item.span.start].matches('\n').count();
                    doc_comments.get(&struct_line).cloned().unwrap_or_default()
                };
                if !doc.is_empty() {
                    out.push_str(&format!("{}\n", doc.join("\n")));
                }
                if !s.fields.is_empty() {
                    out.push_str("\nFields:\n");
                    for field in &s.fields {
                        out.push_str(&format!(
                            "- `{}: {}`\n",
                            field.name,
                            format_type_expr(&field.ty.node)
                        ));
                    }
                }
            }
        }
    } else if !structs.is_empty() {
        out.push_str("\n## Structs\n");
        for item in &structs {
            if let DocItem::Struct { name, fields, doc } = item {
                out.push_str(&format!("\n### `struct {}`\n", name));
                if !doc.is_empty() {
                    out.push_str(&format!("{}\n", doc.join("\n")));
                }
                if !fields.is_empty() {
                    out.push_str("\nFields:\n");
                    for field in fields {
                        out.push_str(&format!("- `{}`\n", field));
                    }
                }
            }
        }
    }

    // Enums section
    if !enums.is_empty() {
        out.push_str("\n## Enums\n");
        for item in &enums {
            if let DocItem::Enum {
                name,
                variants,
                doc,
            } = item
            {
                out.push_str(&format!("\n### `type {}`\n", name));
                if !doc.is_empty() {
                    out.push_str(&format!("{}\n", doc.join("\n")));
                }
                if !variants.is_empty() {
                    let variant_names: Vec<&str> = variants
                        .iter()
                        .map(|v| v.split('(').next().unwrap_or(v).trim())
                        .collect();
                    out.push_str(&format!("\nVariants: {}\n", variant_names.join(", ")));
                }
            }
        }
    }

    // Traits section
    if !traits.is_empty() {
        out.push_str("\n## Traits\n");
        for item in &traits {
            if let DocItem::Trait { name, methods, doc } = item {
                out.push_str(&format!("\n### `trait {}`\n", name));
                if !doc.is_empty() {
                    out.push_str(&format!("{}\n", doc.join("\n")));
                }
                if !methods.is_empty() {
                    out.push_str("\nMethods:\n");
                    for method in methods {
                        out.push_str(&format!("- `{}`\n", method));
                    }
                }
            }
        }
    }

    // Impl blocks section
    if !impls.is_empty() {
        out.push_str("\n## Implementations\n");
        for item in &impls {
            if let DocItem::Impl { target, methods } = item {
                out.push_str(&format!("\n### `impl {}`\n", target));
                out.push_str("\nMethods:\n");
                for method in methods {
                    out.push_str(&format!("- `{}`\n", method));
                }
            }
        }
    }

    // Agents section
    if !agents.is_empty() {
        out.push_str("\n## Agents\n");
        for item in &agents {
            if let DocItem::Agent {
                name,
                model,
                tools,
                doc,
            } = item
            {
                out.push_str(&format!("\n### `agent {}`\n", name));
                if !doc.is_empty() {
                    out.push_str(&format!("{}\n", doc.join("\n")));
                }
                if let Some(m) = model {
                    out.push_str(&format!("\nModel: `{}`\n", m));
                }
                if !tools.is_empty() {
                    out.push_str("\nTools:\n");
                    for tool in tools {
                        out.push_str(&format!("- `{}`\n", tool));
                    }
                }
            }
        }
    }

    print!("{}", out);
}
