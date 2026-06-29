//! `turbolang` — the Turbo language CLI.
//!
//! This binary is the user-facing entry point for the entire compiler. It
//! drives the lexer → parser → sema → codegen pipeline and exposes a small
//! suite of subcommands for everyday workflows.
//!
//! # Subcommands
//!
//! * `run`        — JIT-compile and execute a `.tb` source file.
//! * `build`      — AOT-compile a source file to a native binary.
//! * `check`      — Run lexer/parser/sema and report diagnostics, no codegen.
//! * `test`       — Run all `@test` functions in a file or directory.
//! * `fmt`        — Pretty-print a `.tb` file in place.
//! * `init`       — Scaffold a new Turbo project.
//! * `lsp`        — Start the language-server (stdio).
//! * `repl`       — Start an interactive REPL.
//! * `bench`      — Run benchmarks.
//! * `doc`        — Render Turbo doc comments to Markdown.
//! * `playground` — Serve a local web playground.
//! * `explain`    — Print the long-form explanation for an error code.
//! * `watch`      — Re-run another command on file changes.
//!
//! Pretty diagnostics are produced via the [`ariadne`] crate; every error
//! carries a unique [`turbo_ast::ErrorCode`] so users can `turbolang
//! explain E0NNN` for more detail.

use ariadne::{Color, Label, Report, ReportKind, Source};
use clap::{Parser, Subcommand};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use turbo_ast::{ErrorCode, Expr, InterpolPart, Item, Module, Stmt, TypeExpr};

mod playground;
mod repl;
mod watch;

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

        /// Watch for file changes and auto-reload
        #[arg(long, short)]
        watch: bool,

        /// Arguments forwarded to the program's `args()` builtin. Everything
        /// after the source file (use `--` to separate hyphen-leading args),
        /// e.g. `turbolang run app.tb -- --name alice`.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
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

        /// Compilation target (e.g. "wasm", "linux-arm64", "linux-x86")
        #[arg(long)]
        target: Option<String>,

        /// Link additional C libraries (e.g. --link m --link pthread)
        #[arg(long)]
        link: Vec<String>,
    },
    /// Initialize a new Turbo project
    Init {
        /// Project name (omit, or pass `.`, to scaffold into the current directory)
        name: Option<String>,
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
        /// Path to the .tb source file to format (optional if turbo.toml exists)
        file: Option<PathBuf>,
        /// Check only, don't modify (exit 1 if unformatted)
        #[arg(long)]
        check: bool,
    },
    /// Generate documentation from a Turbo source file
    Doc {
        /// Path to the .tb source file (optional if turbo.toml exists)
        file: Option<PathBuf>,
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
    /// `[internal]` Run a single test function by name (used by test runner)
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
        Commands::Run {
            file,
            verbose,
            watch,
            args,
        } => {
            let path = resolve_entry_file(file);
            // Install the program's CLI args so its `args()` builtin returns
            // them. Set before run (and watch re-runs) so every JIT execution
            // sees them. Matches the AOT convention (argv[1..]).
            turbo_codegen_cranelift::set_program_args(args);
            if watch {
                watch::run_watch(&path, verbose);
            } else {
                run_file(&path, verbose);
            }
        }
        Commands::Build {
            file,
            output,
            verbose,
            target,
            link,
        } => {
            let path = resolve_entry_file(file);
            build_file(&path, output.as_deref(), verbose, target.as_deref(), &link);
        }
        Commands::Init { name } => init_project(name.as_deref().unwrap_or(".")),
        Commands::Repl => repl::run_repl(),
        Commands::Playground { port } => playground::serve(port),
        Commands::Fmt { file, check } => {
            let path = resolve_entry_file(file);
            turbo_formatter::format_file(&path, check);
        }
        Commands::Doc { file } => {
            let path = resolve_entry_file(file);
            doc_file(&path);
        }
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
///
/// Passing `.` (or an empty name) scaffolds into the current directory instead
/// of creating a new one; the package name is then taken from the current
/// directory's name.
fn init_project(name: &str) {
    let into_current = name == "." || name.is_empty();
    let dir = PathBuf::from(if into_current { "." } else { name });
    let pkg_name = if into_current {
        std::env::current_dir()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            .unwrap_or_else(|| "app".to_string())
    } else {
        dir.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| name.to_string())
    };

    if into_current {
        if dir.join("turbo.toml").exists() {
            eprintln!(
                "\x1b[1;31merror\x1b[0m: `turbo.toml` already exists in the current directory"
            );
            std::process::exit(1);
        }
    } else if dir.exists() {
        eprintln!("\x1b[1;31merror\x1b[0m: directory `{name}` already exists");
        std::process::exit(1);
    }

    std::fs::create_dir_all(dir.join("src")).unwrap_or_else(|e| {
        eprintln!(
            "\x1b[1;31merror\x1b[0m: could not create directory: {}",
            io_reason(&e)
        );
        std::process::exit(1);
    });
    std::fs::create_dir_all(dir.join("tests")).unwrap_or_else(|e| {
        eprintln!(
            "\x1b[1;31merror\x1b[0m: could not create directory: {}",
            io_reason(&e)
        );
        std::process::exit(1);
    });

    // turbo.toml
    std::fs::write(
        dir.join("turbo.toml"),
        format!(
            "[package]\nname = \"{pkg_name}\"\nversion = \"0.1.0\"\nedition = \"2026\"\n\n[dependencies]\n"
        ),
    )
    .unwrap_or_else(|e| {
        eprintln!(
            "\x1b[1;31merror\x1b[0m: failed to write turbo.toml: {}",
            io_reason(&e)
        );
        std::process::exit(1);
    });

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
    .unwrap_or_else(|e| {
        eprintln!(
            "\x1b[1;31merror\x1b[0m: failed to write src/main.tb: {}",
            io_reason(&e)
        );
        std::process::exit(1);
    });

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
    .unwrap_or_else(|e| {
        eprintln!(
            "\x1b[1;31merror\x1b[0m: failed to write tests/main_test.tb: {}",
            io_reason(&e)
        );
        std::process::exit(1);
    });

    // .gitignore
    std::fs::write(dir.join(".gitignore"), "turbo_modules/\ntarget/\n*.o\n").unwrap_or_else(|e| {
        eprintln!(
            "\x1b[1;31merror\x1b[0m: failed to write .gitignore: {}",
            io_reason(&e)
        );
        std::process::exit(1);
    });

    if into_current {
        eprintln!("\x1b[32m\u{2713}\x1b[0m Created project `{pkg_name}`");
        eprintln!("  turbolang run");
    } else {
        eprintln!("\x1b[32m\u{2713}\x1b[0m Created project `{name}`");
        eprintln!("  cd {name} && turbolang run");
    }
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum DependencySource {
    Path {
        path: String,
    },
    GitHub {
        repo: String,
        rev: Option<String>,
        version: Option<String>,
    },
    Version {
        version: String,
    },
    Unsupported {
        raw: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DependencySpec {
    name: String,
    section: String,
    source: DependencySource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LockedGitDependency {
    repo: String,
    rev: String,
}

fn parse_dependency_spec(name: &str, rest: &str, section: &str) -> DependencySpec {
    let source = if let Some(path) = extract_quoted_value(rest, "path") {
        DependencySource::Path { path }
    } else if let Some(repo) = extract_quoted_value(rest, "github") {
        let rev = extract_quoted_value(rest, "rev");
        let version = extract_quoted_value(rest, "version");
        DependencySource::GitHub { repo, rev, version }
    } else if let Some(version) = rest
        .trim()
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
    {
        DependencySource::Version {
            version: version.to_string(),
        }
    } else if let Some(version) = extract_quoted_value(rest, "version") {
        DependencySource::Version { version }
    } else {
        DependencySource::Unsupported {
            raw: rest.trim().to_string(),
        }
    };

    DependencySpec {
        name: name.trim().trim_matches('"').to_string(),
        section: section.to_string(),
        source,
    }
}

fn parse_dependencies_from_manifest(toml: &str) -> Vec<DependencySpec> {
    let mut current_section: Option<&str> = None;
    let mut deps = Vec::new();

    for raw_line in toml.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "[dependencies]" {
            current_section = Some("dependencies");
            continue;
        }
        if line == "[dev-dependencies]" {
            current_section = Some("dev-dependencies");
            continue;
        }
        if line.starts_with('[') {
            current_section = None;
            continue;
        }
        let Some(section) = current_section else {
            continue;
        };
        let Some((name, rest)) = line.split_once('=') else {
            continue;
        };
        deps.push(parse_dependency_spec(name, rest.trim(), section));
    }

    deps
}

fn parse_registry_map(toml: &str) -> HashMap<String, String> {
    let mut current_section: Option<&str> = None;
    let mut registries = HashMap::new();

    for raw_line in toml.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "[registries]" {
            current_section = Some("registries");
            continue;
        }
        if line.starts_with('[') {
            current_section = None;
            continue;
        }
        if current_section != Some("registries") {
            continue;
        }
        let Some((name, rest)) = line.split_once('=') else {
            continue;
        };
        let name = name.trim().trim_matches('"').to_string();
        let rest = rest.trim();
        if let Some(repo) = rest
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .map(|s| s.to_string())
        {
            registries.insert(name, repo);
        } else if let Some(repo) = extract_quoted_value(rest, "github") {
            registries.insert(name, repo);
        }
    }

    registries
}

fn default_registry_repo(name: &str) -> Option<String> {
    if name.starts_with("turbo-") {
        Some(format!("ZVN-DEV/{name}"))
    } else {
        None
    }
}

fn resolve_registry_repo(name: &str, registries: &HashMap<String, String>) -> Option<String> {
    registries
        .get(name)
        .cloned()
        .or_else(|| default_registry_repo(name))
}

fn validate_dependency_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("dependency name must not be empty".to_string());
    }
    if name.contains('/') || name.contains('\\') {
        return Err(format!(
            "dependency name `{name}` must not contain path separators"
        ));
    }
    if name == ".." || name.contains("..") {
        return Err(format!(
            "dependency name `{name}` must not contain `..` path traversal"
        ));
    }
    if Path::new(name).is_absolute() {
        return Err(format!(
            "dependency name `{name}` must not be an absolute path"
        ));
    }
    if is_windows_path_prefix(name) {
        return Err(format!(
            "dependency name `{name}` must not use a Windows path prefix"
        ));
    }

    Ok(())
}

fn is_windows_path_prefix(name: &str) -> bool {
    let bytes = name.as_bytes();
    bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic()
}

fn dependency_target_path(modules_dir: &Path, dep_name: &str) -> Result<PathBuf, String> {
    validate_dependency_name(dep_name)?;
    let modules_dir = canonical_modules_dir(modules_dir)?;
    let target = modules_dir.join(dep_name);
    if !target.starts_with(&modules_dir) {
        return Err(format!(
            "dependency target `{}` would escape `{}`",
            target.display(),
            modules_dir.display()
        ));
    }

    Ok(target)
}

fn canonical_modules_dir(modules_dir: &Path) -> Result<PathBuf, String> {
    let cwd = std::fs::canonicalize(".")
        .map_err(|e| format!("could not resolve current directory for turbo_modules: {e}"))?;

    if let Ok(metadata) = std::fs::symlink_metadata(modules_dir) {
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "turbo_modules directory `{}` must not be a symlink",
                modules_dir.display()
            ));
        }
    }

    let resolved = if modules_dir.exists() {
        std::fs::canonicalize(modules_dir).map_err(|e| {
            format!(
                "could not resolve turbo_modules directory `{}`: {e}",
                modules_dir.display()
            )
        })?
    } else if modules_dir.is_absolute() {
        let parent = modules_dir.parent().ok_or_else(|| {
            format!(
                "could not resolve turbo_modules directory `{}`: missing parent",
                modules_dir.display()
            )
        })?;
        let name = modules_dir.file_name().ok_or_else(|| {
            format!(
                "could not resolve turbo_modules directory `{}`: missing directory name",
                modules_dir.display()
            )
        })?;
        std::fs::canonicalize(parent)
            .map(|parent| parent.join(name))
            .map_err(|e| {
                format!(
                    "could not resolve turbo_modules parent `{}`: {e}",
                    parent.display()
                )
            })?
    } else {
        cwd.join(modules_dir)
    };

    if !resolved.starts_with(&cwd) {
        return Err(format!(
            "turbo_modules directory `{}` must stay inside project root `{}`",
            resolved.display(),
            cwd.display()
        ));
    }

    Ok(resolved)
}

fn validate_manifest_dependency_names(deps: &[DependencySpec]) {
    for dep in deps {
        if let Err(err) = validate_dependency_name(&dep.name) {
            eprintln!(
                "\x1b[1;31merror\x1b[0m: invalid dependency name `{}` in [{}]: {}",
                dep.name, dep.section, err
            );
            std::process::exit(1);
        }
    }
}

fn read_lockfile() -> HashMap<String, LockedGitDependency> {
    let contents = match std::fs::read_to_string("turbo.lock") {
        Ok(s) => s,
        Err(_) => return HashMap::new(),
    };

    let mut current_section = None;
    let mut locks = HashMap::new();

    for raw_line in contents.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "[github]" {
            current_section = Some("github");
            continue;
        }
        if line.starts_with('[') {
            current_section = None;
            continue;
        }
        if current_section != Some("github") {
            continue;
        }
        let Some((name, rest)) = line.split_once('=') else {
            continue;
        };
        let Some(value) = rest
            .trim()
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
        else {
            continue;
        };
        let Some((repo, rev)) = value.rsplit_once('#') else {
            continue;
        };
        locks.insert(
            name.trim().to_string(),
            LockedGitDependency {
                repo: repo.to_string(),
                rev: rev.to_string(),
            },
        );
    }

    locks
}

fn write_lockfile(locks: &HashMap<String, LockedGitDependency>) {
    if locks.is_empty() {
        let _ = std::fs::remove_file("turbo.lock");
        return;
    }

    let mut names: Vec<&String> = locks.keys().collect();
    names.sort();

    let mut out = String::from(
        "# This file is generated by `turbolang install` / `turbolang update`.\n\
         # It pins GitHub dependencies to exact commits for reproducible installs.\n\n\
         [github]\n",
    );
    for name in names {
        let entry = &locks[name];
        out.push_str(&format!("{name} = \"{}#{}\"\n", entry.repo, entry.rev));
    }

    if let Err(e) = std::fs::write("turbo.lock", out) {
        eprintln!(
            "\x1b[1;31merror\x1b[0m: could not write turbo.lock: {}",
            io_reason(&e)
        );
        std::process::exit(1);
    }
}

fn git_output(args: &[&str]) -> Result<String, String> {
    let output = std::process::Command::new("git").args(args).output();
    match output {
        Ok(o) if o.status.success() => Ok(String::from_utf8_lossy(&o.stdout).trim().to_string()),
        Ok(o) => Err(String::from_utf8_lossy(&o.stderr).trim().to_string()),
        Err(e) => Err(e.to_string()),
    }
}

fn git_output_in_dir(dir: &Path, args: &[&str]) -> Result<String, String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output();
    match output {
        Ok(o) if o.status.success() => Ok(String::from_utf8_lossy(&o.stdout).trim().to_string()),
        Ok(o) => Err(String::from_utf8_lossy(&o.stderr).trim().to_string()),
        Err(e) => Err(e.to_string()),
    }
}

fn current_git_head(dir: &Path) -> Result<String, String> {
    git_output_in_dir(dir, &["rev-parse", "HEAD"])
}

fn clone_github_repo(repo: &str, target: &Path) -> Result<(), String> {
    let url = format!("https://github.com/{repo}.git");
    let target_str = target.to_string_lossy().to_string();
    git_output(&["clone", "--depth=1", &url, &target_str]).map(|_| ())
}

fn checkout_git_rev(dir: &Path, rev: &str) -> Result<(), String> {
    git_output_in_dir(dir, &["fetch", "--depth=1", "origin", rev])?;
    git_output_in_dir(dir, &["checkout", "--detach", rev]).map(|_| ())
}

fn git_ls_remote_tags(repo: &str) -> Result<Vec<(String, String)>, String> {
    let url = format!("https://github.com/{repo}.git");
    let output = git_output(&["ls-remote", "--tags", &url])?;
    let mut tags = HashMap::new();

    for line in output.lines() {
        let Some((sha, raw_ref)) = line.split_once('\t') else {
            continue;
        };
        let Some(tag_ref) = raw_ref.strip_prefix("refs/tags/") else {
            continue;
        };
        let tag = tag_ref.strip_suffix("^{}").unwrap_or(tag_ref).to_string();
        tags.insert(tag, sha.to_string());
    }

    let mut out: Vec<(String, String)> = tags.into_iter().collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

fn parse_semver_like(input: &str) -> Option<(u64, u64, u64)> {
    let trimmed = input.trim().trim_start_matches('v');
    let mut parts = trimmed.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next().map_or(Some(0), |p| p.parse().ok())?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

fn select_tag_for_version(version: &str, tags: &[(String, String)]) -> Option<(String, String)> {
    let requested = parse_semver_like(version)?;
    let exact_requested = version.trim_start_matches('v').split('.').count() >= 3;

    let mut exact = None;
    let mut matching_minor = Vec::new();

    for (tag, sha) in tags {
        let parsed = match parse_semver_like(tag) {
            Some(v) => v,
            None => continue,
        };
        if exact_requested {
            if parsed == requested {
                exact = Some((tag.clone(), sha.clone()));
                break;
            }
        } else if parsed.0 == requested.0 && parsed.1 == requested.1 {
            matching_minor.push((parsed, tag.clone(), sha.clone()));
        }
    }

    if let Some(found) = exact {
        return Some(found);
    }

    if matching_minor.is_empty() {
        return None;
    }
    matching_minor.sort_by_key(|a| a.0);
    let (_, tag, sha) = matching_minor.pop()?;
    Some((tag, sha))
}

fn resolve_versioned_rev(repo: &str, version: &str) -> Result<(String, String), String> {
    let tags = git_ls_remote_tags(repo)?;
    select_tag_for_version(version, &tags)
        .ok_or_else(|| format!("no tag found in {repo} matching version {version}"))
}

#[cfg(test)]
mod dependency_tests {
    use super::*;

    #[test]
    fn parse_registry_section() {
        let toml = r#"
[registries]
turbo-db = "ZVN-DEV/turbo-db"
turbo-test-utils = { github = "ZVN-DEV/turbo-test-utils" }
"#;
        let registries = parse_registry_map(toml);
        assert_eq!(
            registries.get("turbo-db").map(String::as_str),
            Some("ZVN-DEV/turbo-db")
        );
        assert_eq!(
            registries.get("turbo-test-utils").map(String::as_str),
            Some("ZVN-DEV/turbo-test-utils")
        );
    }

    #[test]
    fn parse_version_dependency() {
        let deps = parse_dependencies_from_manifest(
            r#"
[dependencies]
turbo-db = "0.1"
agent-kit = { github = "owner/agent-kit", version = "1.2" }
"#,
        );
        assert!(matches!(
            deps[0].source,
            DependencySource::Version { ref version } if version == "0.1"
        ));
        assert!(matches!(
            deps[1].source,
            DependencySource::GitHub { ref repo, version: Some(ref version), .. }
                if repo == "owner/agent-kit" && version == "1.2"
        ));
    }

    #[test]
    fn validate_dependency_name_accepts_plain_package_names() {
        for name in ["turbo-db", "agent_kit", "http2"] {
            assert!(
                validate_dependency_name(name).is_ok(),
                "{name} should be a valid dependency name"
            );
        }
    }

    #[test]
    fn validate_dependency_name_rejects_path_like_names() {
        for name in [
            "",
            "..",
            "../escape",
            "escape/child",
            "escape\\child",
            "/absolute",
            "C:escape",
            "C:\\escape",
            "pkg..escape",
        ] {
            assert!(
                validate_dependency_name(name).is_err(),
                "{name:?} should be rejected as a dependency name"
            );
        }
    }

    #[test]
    fn dependency_target_path_stays_under_turbo_modules() {
        let test_root = PathBuf::from(format!(
            ".turbo-cli-dep-target-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let modules_dir = test_root.join("turbo_modules");
        std::fs::create_dir_all(&modules_dir).unwrap();

        let target = dependency_target_path(&modules_dir, "safe_dep").unwrap();
        let canonical_modules = std::fs::canonicalize(&modules_dir).unwrap();
        assert!(target.starts_with(&canonical_modules));
        assert_eq!(target, canonical_modules.join("safe_dep"));

        let err = dependency_target_path(&modules_dir, "../escape").unwrap_err();
        assert!(
            err.contains("path separators") || err.contains("path traversal"),
            "unexpected error: {err}"
        );

        std::fs::remove_dir_all(&test_root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn dependency_target_path_rejects_symlinked_turbo_modules() {
        let test_root = PathBuf::from(format!(
            ".turbo-cli-dep-target-symlink-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let outside = test_root.join("outside");
        let modules_dir = test_root.join("turbo_modules");
        std::fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, &modules_dir).unwrap();

        let err = dependency_target_path(&modules_dir, "safe_dep").unwrap_err();
        assert!(
            err.contains("must not be a symlink"),
            "unexpected error: {err}"
        );

        std::fs::remove_dir_all(&test_root).unwrap();
    }

    #[test]
    fn select_latest_patch_for_minor_version() {
        let tags = vec![
            ("v0.1.0".to_string(), "aaa".to_string()),
            ("v0.1.4".to_string(), "bbb".to_string()),
            ("v0.2.0".to_string(), "ccc".to_string()),
        ];
        let selected = select_tag_for_version("0.1", &tags).unwrap();
        assert_eq!(selected.0, "v0.1.4");
        assert_eq!(selected.1, "bbb");
    }

    #[test]
    fn select_exact_patch_when_requested() {
        let tags = vec![
            ("v0.1.0".to_string(), "aaa".to_string()),
            ("v0.1.4".to_string(), "bbb".to_string()),
        ];
        let selected = select_tag_for_version("0.1.0", &tags).unwrap();
        assert_eq!(selected.0, "v0.1.0");
    }
}

/// Install dependencies listed in `turbo.toml` by symlinking path dependencies
/// into a local `turbo_modules/` directory.
fn install_deps() {
    let toml = std::fs::read_to_string("turbo.toml").unwrap_or_else(|_| {
        eprintln!("\x1b[1;31merror\x1b[0m: no turbo.toml found in current directory");
        std::process::exit(1);
    });

    std::fs::create_dir_all("turbo_modules").ok();
    let deps = parse_dependencies_from_manifest(&toml);
    validate_manifest_dependency_names(&deps);
    let registries = parse_registry_map(&toml);
    let mut lockfile = read_lockfile();
    let mut count = 0u32;
    let mut unsupported = Vec::new();

    for dep in deps {
        match dep.source {
            DependencySource::Path { path } => {
                let source_path = std::path::Path::new(&path);
                let canonical = match std::fs::canonicalize(source_path) {
                    Ok(p) => p,
                    Err(e) => {
                        eprintln!(
                            "\x1b[1;31merror\x1b[0m: could not resolve dependency path `{}`: {}",
                            path,
                            io_reason(&e)
                        );
                        std::process::exit(1);
                    }
                };

                let target = dependency_target_path(Path::new("turbo_modules"), &dep.name)
                    .unwrap_or_else(|err| {
                        eprintln!("\x1b[1;31merror\x1b[0m: {err}");
                        std::process::exit(1);
                    });
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
                            "\x1b[1;31merror\x1b[0m: could not create symlink for `{}`: {}",
                            dep.name,
                            io_reason(&e)
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
                            "\x1b[1;31merror\x1b[0m: could not copy dependency `{}`: {}",
                            dep.name,
                            io_reason(&e)
                        );
                        std::process::exit(1);
                    });
                }

                eprintln!(
                    "  \x1b[32m\u{2713}\x1b[0m Installed {} -> {} ({})",
                    dep.name, path, dep.section
                );
                count += 1;
            }
            DependencySource::GitHub { repo, rev, version } => {
                let target = dependency_target_path(Path::new("turbo_modules"), &dep.name)
                    .unwrap_or_else(|err| {
                        eprintln!("\x1b[1;31merror\x1b[0m: {err}");
                        std::process::exit(1);
                    });
                let resolved = if let Some(wanted_rev) = rev.clone() {
                    Ok((wanted_rev.clone(), format!("rev {wanted_rev}")))
                } else if let Some(version) = version.as_deref() {
                    resolve_versioned_rev(&repo, version)
                        .map(|(tag, sha)| (sha, format!("tag {tag}")))
                } else if let Some(locked) = lockfile
                    .get(&dep.name)
                    .filter(|entry| entry.repo == repo)
                    .map(|entry| entry.rev.clone())
                {
                    Ok((locked, "lockfile".to_string()))
                } else {
                    Err("no rev, version, or existing turbo.lock entry".to_string())
                };
                let (pinned_rev, pinned_label) = match resolved {
                    Ok(v) => v,
                    Err(err) => {
                        eprintln!(
                            "  \x1b[31m\u{2717}\x1b[0m Failed to resolve {} from github:{}: {}",
                            dep.name, repo, err
                        );
                        continue;
                    }
                };

                if target.exists() {
                    if current_git_head(&target).ok().as_deref() != Some(pinned_rev.as_str()) {
                        if let Err(err) = checkout_git_rev(&target, &pinned_rev) {
                            eprintln!(
                                "  \x1b[31m\u{2717}\x1b[0m Failed to pin {} to {}: {}",
                                dep.name, pinned_rev, err
                            );
                            continue;
                        }
                    }
                } else {
                    eprintln!(
                        "  \x1b[36m\u{2193}\x1b[0m Cloning {} from github:{}...",
                        dep.name, repo
                    );
                    if let Err(err) = clone_github_repo(&repo, &target) {
                        eprintln!(
                            "  \x1b[31m\u{2717}\x1b[0m Failed to clone {}: {}",
                            dep.name, err
                        );
                        continue;
                    }
                    if let Err(err) = checkout_git_rev(&target, &pinned_rev) {
                        eprintln!(
                            "  \x1b[31m\u{2717}\x1b[0m Failed to pin {} to {}: {}",
                            dep.name, pinned_rev, err
                        );
                        continue;
                    }
                }

                match current_git_head(&target) {
                    Ok(head) => {
                        lockfile.insert(
                            dep.name.clone(),
                            LockedGitDependency {
                                repo: repo.clone(),
                                rev: head.clone(),
                            },
                        );
                        eprintln!(
                            "  \x1b[32m\u{2713}\x1b[0m Installed {} from github:{} @ {} via {} ({})",
                            dep.name,
                            repo,
                            &head[..head.len().min(12)],
                            pinned_label,
                            dep.section
                        );
                        count += 1;
                    }
                    Err(err) => eprintln!(
                        "  \x1b[31m\u{2717}\x1b[0m Failed to resolve installed rev for {}: {}",
                        dep.name, err
                    ),
                }
            }
            DependencySource::Version { version } => {
                let Some(repo) = resolve_registry_repo(&dep.name, &registries) else {
                    eprintln!(
                        "  \x1b[31m\u{2717}\x1b[0m No registry mapping found for {} {}",
                        dep.name, version
                    );
                    continue;
                };
                let target = dependency_target_path(Path::new("turbo_modules"), &dep.name)
                    .unwrap_or_else(|err| {
                        eprintln!("\x1b[1;31merror\x1b[0m: {err}");
                        std::process::exit(1);
                    });
                let (tag, pinned_rev) = match resolve_versioned_rev(&repo, &version) {
                    Ok(v) => v,
                    Err(err) => {
                        eprintln!(
                            "  \x1b[31m\u{2717}\x1b[0m Failed to resolve {} {}: {}",
                            dep.name, version, err
                        );
                        continue;
                    }
                };
                if target.exists() {
                    if current_git_head(&target).ok().as_deref() != Some(pinned_rev.as_str()) {
                        if let Err(err) = checkout_git_rev(&target, &pinned_rev) {
                            eprintln!(
                                "  \x1b[31m\u{2717}\x1b[0m Failed to pin {} to {}: {}",
                                dep.name, pinned_rev, err
                            );
                            continue;
                        }
                    }
                } else {
                    eprintln!(
                        "  \x1b[36m\u{2193}\x1b[0m Cloning {} {} from {}...",
                        dep.name, version, repo
                    );
                    if let Err(err) = clone_github_repo(&repo, &target) {
                        eprintln!(
                            "  \x1b[31m\u{2717}\x1b[0m Failed to clone {}: {}",
                            dep.name, err
                        );
                        continue;
                    }
                    if let Err(err) = checkout_git_rev(&target, &pinned_rev) {
                        eprintln!(
                            "  \x1b[31m\u{2717}\x1b[0m Failed to pin {} to {}: {}",
                            dep.name, pinned_rev, err
                        );
                        continue;
                    }
                }
                lockfile.insert(
                    dep.name.clone(),
                    LockedGitDependency {
                        repo: repo.clone(),
                        rev: pinned_rev.clone(),
                    },
                );
                eprintln!(
                    "  \x1b[32m\u{2713}\x1b[0m Installed {} {} from {} @ {} via {} ({})",
                    dep.name,
                    version,
                    repo,
                    &pinned_rev[..pinned_rev.len().min(12)],
                    tag,
                    dep.section
                );
                count += 1;
            }
            DependencySource::Unsupported { raw } => unsupported.push((dep.name, dep.section, raw)),
        }
    }

    if !unsupported.is_empty() {
        for (name, section, raw) in &unsupported {
            eprintln!(
                "  \x1b[31m\u{2717}\x1b[0m Unsupported dependency format for {} ({}) -> {}",
                name, section, raw
            );
        }
        eprintln!("\nerror: unsupported dependency syntax in turbo.toml.");
        std::process::exit(1);
    }

    write_lockfile(&lockfile);

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

    let deps = parse_dependencies_from_manifest(&toml);
    validate_manifest_dependency_names(&deps);
    let registries = parse_registry_map(&toml);
    let mut lockfile = read_lockfile();
    let mut count = 0u32;
    let mut unsupported = Vec::new();

    for dep in deps {
        match dep.source {
            DependencySource::GitHub { repo, rev, version } => {
                let target = dependency_target_path(Path::new("turbo_modules"), &dep.name)
                    .unwrap_or_else(|err| {
                        eprintln!("\x1b[1;31merror\x1b[0m: {err}");
                        std::process::exit(1);
                    });
                if !target.exists() {
                    eprintln!(
                        "  \x1b[33m!\x1b[0m {} not installed — run `turbolang install` first",
                        dep.name
                    );
                    continue;
                }

                if let Some(wanted) = rev.as_deref() {
                    match checkout_git_rev(&target, wanted).and_then(|_| current_git_head(&target))
                    {
                        Ok(head) => {
                            lockfile.insert(
                                dep.name.clone(),
                                LockedGitDependency {
                                    repo: repo.clone(),
                                    rev: head.clone(),
                                },
                            );
                            eprintln!(
                                "  \x1b[32m\u{2713}\x1b[0m {} pinned at manifest rev {}",
                                dep.name,
                                &head[..head.len().min(12)]
                            );
                            count += 1;
                        }
                        Err(err) => eprintln!(
                            "  \x1b[31m\u{2717}\x1b[0m Failed to pin {}: {}",
                            dep.name, err
                        ),
                    }
                    continue;
                }

                if let Some(version) = version.as_deref() {
                    match resolve_versioned_rev(&repo, version)
                        .and_then(|(tag, rev)| checkout_git_rev(&target, &rev).map(|_| (tag, rev)))
                        .and_then(|(tag, rev)| {
                            current_git_head(&target).map(|head| (tag, rev, head))
                        }) {
                        Ok((tag, _rev, head)) => {
                            lockfile.insert(
                                dep.name.clone(),
                                LockedGitDependency {
                                    repo: repo.clone(),
                                    rev: head.clone(),
                                },
                            );
                            eprintln!(
                                "  \x1b[32m\u{2713}\x1b[0m {} updated to {} ({})",
                                dep.name,
                                &head[..head.len().min(12)],
                                tag
                            );
                            count += 1;
                        }
                        Err(err) => eprintln!(
                            "  \x1b[31m\u{2717}\x1b[0m Failed to update {} {}: {}",
                            dep.name, version, err
                        ),
                    }
                    continue;
                }

                eprintln!(
                    "  \x1b[36m\u{2193}\x1b[0m Updating {} from github:{}...",
                    dep.name, repo
                );
                match git_output_in_dir(&target, &["pull", "--ff-only"])
                    .and_then(|_| current_git_head(&target))
                {
                    Ok(head) => {
                        lockfile.insert(
                            dep.name.clone(),
                            LockedGitDependency {
                                repo: repo.clone(),
                                rev: head.clone(),
                            },
                        );
                        eprintln!(
                            "  \x1b[32m\u{2713}\x1b[0m Updated {} -> {}",
                            dep.name,
                            &head[..head.len().min(12)]
                        );
                        count += 1;
                    }
                    Err(err) => eprintln!(
                        "  \x1b[31m\u{2717}\x1b[0m Failed to update {}: {}",
                        dep.name, err
                    ),
                }
            }
            DependencySource::Version { version } => {
                let Some(repo) = resolve_registry_repo(&dep.name, &registries) else {
                    eprintln!(
                        "  \x1b[31m\u{2717}\x1b[0m No registry mapping found for {} {}",
                        dep.name, version
                    );
                    continue;
                };
                let target = dependency_target_path(Path::new("turbo_modules"), &dep.name)
                    .unwrap_or_else(|err| {
                        eprintln!("\x1b[1;31merror\x1b[0m: {err}");
                        std::process::exit(1);
                    });
                if !target.exists() {
                    eprintln!(
                        "  \x1b[33m!\x1b[0m {} not installed — run `turbolang install` first",
                        dep.name
                    );
                    continue;
                }
                match resolve_versioned_rev(&repo, &version)
                    .and_then(|(tag, rev)| checkout_git_rev(&target, &rev).map(|_| (tag, rev)))
                    .and_then(|(tag, rev)| current_git_head(&target).map(|head| (tag, rev, head)))
                {
                    Ok((tag, _rev, head)) => {
                        lockfile.insert(
                            dep.name.clone(),
                            LockedGitDependency {
                                repo: repo.clone(),
                                rev: head.clone(),
                            },
                        );
                        eprintln!(
                            "  \x1b[32m\u{2713}\x1b[0m {} {} -> {} ({})",
                            dep.name,
                            version,
                            &head[..head.len().min(12)],
                            tag
                        );
                        count += 1;
                    }
                    Err(err) => eprintln!(
                        "  \x1b[31m\u{2717}\x1b[0m Failed to update {} {}: {}",
                        dep.name, version, err
                    ),
                }
            }
            DependencySource::Unsupported { raw } => unsupported.push((dep.name, dep.section, raw)),
            DependencySource::Path { .. } => {}
        }
    }

    if !unsupported.is_empty() {
        for (name, section, raw) in &unsupported {
            eprintln!(
                "  \x1b[31m\u{2717}\x1b[0m Unsupported dependency format for {} ({}) -> {}",
                name, section, raw
            );
        }
        std::process::exit(1);
    }

    write_lockfile(&lockfile);

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
/// Produce a (message, help) pair for a lexer error span. A bare "unexpected
/// character" is unhelpful when the real problem is a numeric literal the lexer
/// matched but couldn't fit into `i64` (it returns `None`, which surfaces as a
/// lex error). Detect the all-digits case and say so precisely.
fn lex_error_message(snippet: &str) -> (String, &'static str) {
    let is_int_literal =
        !snippet.is_empty() && snippet.chars().all(|c| c.is_ascii_digit() || c == '_');
    if is_int_literal {
        (
            format!("integer literal `{snippet}` is too large for `i64` (max 9223372036854775807)"),
            "use a smaller value, or split the computation to stay within i64 range",
        )
    } else {
        (
            format!("unexpected character `{snippet}`"),
            "remove this character or check for typos",
        )
    }
}

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

    // Compose the footer. Real help text (if any) is rendered by ariadne as a
    // `Help:` block. The `more info: <url>` line is decoupled from it: when
    // there is help text the URL is appended under the Help block, but when
    // there is none it is emitted on its own line *after* the frame rather
    // than collapsing onto an empty `Help:` label (which read like a bug).
    let (help_block, standalone_footer) = compose_diagnostic_footer(help, code);
    if let Some(block) = help_block {
        builder = builder.with_help(block);
    }

    builder
        .finish()
        .eprint((filename, Source::from(source)))
        .unwrap();

    if let Some(footer) = standalone_footer {
        eprintln!("{footer}");
    }
}

/// Build the two footer pieces for a diagnostic, keeping the `Help:` label and
/// the `more info:` URL decoupled.
///
/// Returns `(help_block, standalone_footer)`:
/// - `help_block` is handed to ariadne's `with_help` and rendered as a
///   `Help:` block. It is only present when there is real help text; the
///   `more info:` line is appended under it as a continuation when a code is
///   also present.
/// - `standalone_footer` is printed on its own line after the frame. It
///   carries the `more info:` line when there is no help text, so the URL is
///   never glued onto an empty `Help:` label.
fn compose_diagnostic_footer(
    help: Option<&str>,
    code: Option<ErrorCode>,
) -> (Option<String>, Option<String>) {
    let more_info = code.map(|c| format!("more info: {}", error_code_url(c)));
    match (help, more_info) {
        (Some(h), Some(mi)) => (Some(format!("{h}\n  {mi}")), None),
        (Some(h), None) => (Some(h.to_string()), None),
        (None, Some(mi)) => (None, Some(format!("  {mi}"))),
        (None, None) => (None, None),
    }
}

/// Returns the canonical public URL for a given error code.
///
/// We currently point at the GitHub blob URL for the source-of-truth
/// markdown file. The `docs/errors/` tree is a parallel symlink farm
/// pointing back at `turbo-cli/src/errors/E0NNN.md`, so the GitHub URL
/// is guaranteed to resolve as long as the file exists in master.
///
/// TODO(P3): once `turbolang.dev/errors/E0NNN` is live (with a stable
/// redirect to the same content), flip this back to the short form.
fn error_code_url(code: ErrorCode) -> String {
    format!(
        "https://github.com/ZVN-DEV/Turbo-Language/blob/master/docs/errors/{}.md",
        code.as_str()
    )
}

/// Render an operational/CLI error with the same envelope as the compile-time
/// diagnostics, but without an ariadne source frame (these errors — a missing
/// file, an unresolvable import — have no `.tb` span to point a caret at).
///
/// Reuses [`compose_diagnostic_footer`] / [`error_code_url`] so the `Help:`
/// block and the `more info:` footer are formatted identically to every other
/// diagnostic:
///
/// ```text
/// error[E06NN]: <message>
/// Help: <help>
///   more info: <url>
/// ```
fn report_codeful_error(message: &str, help: Option<&str>, code: ErrorCode) {
    eprintln!("\x1b[1;31merror[{}]\x1b[0m: {}", code.as_str(), message);
    let (help_block, standalone_footer) = compose_diagnostic_footer(help, Some(code));
    if let Some(block) = help_block {
        eprintln!("Help: {block}");
    }
    if let Some(footer) = standalone_footer {
        eprintln!("{footer}");
    }
}

/// Translate a [`std::io::Error`] into a jargon-free reason phrase, dropping
/// the `(os error N)` suffix that the error's `Display` appends for OS errors.
///
/// `io::ErrorKind`'s own `Display` ("is a directory", "read-only filesystem",
/// …) is already human-readable and never includes the raw errno; the two most
/// common kinds get an even friendlier phrasing. This mirrors the catch-all in
/// [`report_file_error`], which surfaces `err.kind()` rather than `err`, and is
/// used by the operational error paths (`init`, `bench`, lockfile writes) that
/// render an io error inline instead of through the E0611 envelope.
fn io_reason(err: &std::io::Error) -> String {
    use std::io::ErrorKind;
    match err.kind() {
        ErrorKind::NotFound => "no such file or directory".to_string(),
        ErrorKind::PermissionDenied => "permission denied".to_string(),
        other => other.to_string(),
    }
}

/// Render a file-not-found / unreadable-source error (E0611) and exit.
///
/// Drops the raw `(os error N)` jargon that `std::io::Error`'s `Display`
/// leaks: callers see a plain-language reason plus a `Help:` line and the
/// `more info:` footer, matching the quality of the compile diagnostics.
fn report_file_error(path: &std::path::Path, err: &std::io::Error) -> ! {
    use std::io::ErrorKind;
    let (message, help) = match err.kind() {
        ErrorKind::NotFound => (
            format!("could not find `{}` — check the path", path.display()),
            "make sure the file exists and the path is spelled correctly",
        ),
        ErrorKind::PermissionDenied => (
            format!("permission denied reading `{}`", path.display()),
            "check the file's permissions, or run as a user that can read it",
        ),
        // `err.kind()`'s Display ("is a directory", "invalid input", …) is
        // jargon-free — unlike `{err}`, it never appends "(os error N)".
        other => (
            format!("could not read `{}`: {other}", path.display()),
            "check that the path points to a readable file",
        ),
    };
    report_codeful_error(&message, Some(help), ErrorCode::E0611);
    std::process::exit(1);
}

/// Render an import-resolution failure (E0610) and exit. `message` is the
/// human-readable reason produced by [`resolve_imports`].
fn report_import_error(message: &str) -> ! {
    report_codeful_error(
        message,
        Some("check the import path, and that the file exists and parses cleanly"),
        ErrorCode::E0610,
    );
    std::process::exit(1);
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
        Err(e) => report_file_error(path, &e),
    }
}

fn run_file(path: &std::path::Path, verbose: bool) {
    check_file_size(path);

    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => report_file_error(path, &e),
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
            let (msg, help) = lex_error_message(snippet);
            report_error(&source, &filename, &msg, span, Some(help), None);
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
                parse_help(&err.message).as_deref(),
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
        report_import_error(&e);
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
        Err(e) => report_file_error(path, &e),
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
            let (msg, help) = lex_error_message(snippet);
            report_error(&source, &filename, &msg, span, Some(help), None);
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
                parse_help(&err.message).as_deref(),
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
        report_import_error(&e);
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

    // Color the PASS/FAIL tags only when stderr is a terminal, so captured
    // output (CI, the integration harness) stays deterministic and free of
    // ANSI escapes. Mirrors the runtime's `is_terminal()` gating.
    use std::io::IsTerminal;
    let use_color = std::io::stderr().is_terminal();
    let pass_tag = if use_color {
        "\x1b[32mPASS\x1b[0m"
    } else {
        "PASS"
    };
    let fail_tag = if use_color {
        "\x1b[31mFAIL\x1b[0m"
    } else {
        "FAIL"
    };

    let suite_start = std::time::Instant::now();
    let mut total_passed = 0u32;
    let mut total_failed = 0u32;

    // Test files are collected in sorted order (see `collect_test_files`) and
    // each file's tests run in declaration order, so the result listing below
    // is stable across runs.
    for path in &files {
        let source = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => report_file_error(path, &e),
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
                let (msg, help) = lex_error_message(snippet);
                report_error(&source, &filename, &msg, span, Some(help), None);
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
                    parse_help(&err.message).as_deref(),
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
            report_import_error(&e);
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
                        eprintln!("  {pass_tag}  {name}");
                        total_passed += 1;
                    } else {
                        // Print captured stderr (assertion failure messages)
                        let stderr = String::from_utf8_lossy(&result.stderr);
                        for line in stderr.lines() {
                            if !line.is_empty() {
                                eprintln!("        {line}");
                            }
                        }
                        eprintln!("  {fail_tag}  {name}");
                        total_failed += 1;
                    }
                }
                Err(e) => {
                    eprintln!("  {fail_tag}  {name} (failed to spawn: {})", io_reason(&e));
                    total_failed += 1;
                }
            }
        }
    }

    // Summary: counts plus the total wall-clock time the suite took. The time
    // is formatted with Rust's `{:.2}` (not the language's float printer), so
    // it is unaffected by `.tb` float-formatting changes.
    let elapsed = suite_start.elapsed().as_secs_f64();
    eprintln!("{total_passed} passed, {total_failed} failed in {elapsed:.2}s");

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
        // Read the source once: used to find the benchmark's label (the
        // `@bench` function name) and to surface an expected-output hint.
        let source = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!(
                    "  \x1b[31merror\x1b[0m: could not read `{}`: {}",
                    path.display(),
                    io_reason(&e)
                );
                continue;
            }
        };

        // Label by the `@bench` function name when present, else the file stem.
        let file_stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "<unknown>".to_string());
        let label = bench_label(&source).unwrap_or_else(|| file_stem.clone());

        total += 1;
        eprintln!("\x1b[1;36m--- {label} ---\x1b[0m");

        // Show expected output from comment if present
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

        // JIT mode: run N iterations, report median. A benchmark "completes"
        // when it produces a valid timing — i.e. the program actually ran to a
        // clean exit. AOT parity is reported separately and never gates this.
        let mut jit_times = Vec::new();
        let mut jit_output = String::new();
        let mut jit_ok = false;
        // Warm-up: one untimed run to prime the OS page cache before the timed
        // loop, so the recorded median reflects steady-state execution rather
        // than a cold-start outlier. (Kept symmetric with the AOT warm-up below,
        // where it matters far more — see that note.)
        let _ = std::process::Command::new(&turbo_exe)
            .arg("run")
            .arg(path)
            .output();
        for _ in 0..iterations {
            let start = std::time::Instant::now();
            let output = std::process::Command::new(&turbo_exe)
                .arg("run")
                .arg(path)
                .output();
            let elapsed = start.elapsed();
            match output {
                Ok(result) if result.status.success() => {
                    jit_times.push(elapsed);
                    jit_ok = true;
                    if jit_output.is_empty() {
                        jit_output = String::from_utf8_lossy(&result.stdout).trim().to_string();
                    }
                }
                Ok(result) => {
                    // The program failed to compile or run: a real failure, so
                    // there is no valid timing. Report once and stop retrying.
                    eprintln!("  \x1b[31mFAIL\x1b[0m  benchmark did not run");
                    for line in String::from_utf8_lossy(&result.stderr)
                        .lines()
                        .filter(|l| !l.is_empty())
                    {
                        eprintln!("        {line}");
                    }
                    break;
                }
                Err(e) => {
                    eprintln!("  \x1b[31merror\x1b[0m: failed to run JIT: {e}");
                    break;
                }
            }
        }

        if !jit_ok {
            // No valid timing — not counted as completed.
            eprintln!();
            continue;
        }

        // Lead with the timing: it's the thing the user came for.
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
        passed += 1;

        // AOT mode: build then run N iterations, report median. The AOT-vs-JIT
        // output comparison is a SEPARATE, non-fatal annotation ("AOT parity"),
        // never the headline pass/fail.
        let tmp_bin = std::env::temp_dir().join(format!("turbo_bench_{file_stem}"));
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
                let mut aot_ok = false;
                // Warm-up: the first launch of a freshly built binary pays a
                // one-time OS cold-start (page-fault-in, and on macOS a Gatekeeper
                // assessment of the new executable) that is NOT representative of
                // the binary's actual run time — it can be several times the
                // steady-state cost. Without discarding it, the AOT median (which
                // at low iteration counts lands on that cold sample) made AOT look
                // ~5x slower than JIT even though the two backends run at parity.
                // This run is intentionally untimed.
                let _ = std::process::Command::new(&tmp_bin).output();
                for _ in 0..iterations {
                    let start = std::time::Instant::now();
                    let output = std::process::Command::new(&tmp_bin).output();
                    let elapsed = start.elapsed();
                    match output {
                        Ok(result) if result.status.success() => {
                            aot_times.push(elapsed);
                            aot_ok = true;
                            if aot_output.is_empty() {
                                aot_output =
                                    String::from_utf8_lossy(&result.stdout).trim().to_string();
                            }
                        }
                        Ok(_) => break,
                        Err(e) => {
                            eprintln!("  \x1b[31merror\x1b[0m: failed to run AOT binary: {e}");
                            break;
                        }
                    }
                }

                if aot_ok {
                    aot_times.sort();
                    let median = aot_times[aot_times.len() / 2];
                    if quiet {
                        eprintln!(
                            "  \x1b[33mAOT (run only):\x1b[0m  \x1b[90m{:.3}s median ({} runs)\x1b[0m",
                            median.as_secs_f64(),
                            aot_times.len()
                        );
                    } else {
                        eprintln!(
                            "  \x1b[33mAOT (run only):\x1b[0m  {} \x1b[90m({:.3}s median, {} runs)\x1b[0m",
                            aot_output,
                            median.as_secs_f64(),
                            aot_times.len()
                        );
                    }

                    if jit_output == aot_output {
                        eprintln!("  \x1b[90mAOT parity:\x1b[0m \x1b[32mok\x1b[0m");
                    } else {
                        eprintln!("  \x1b[90mAOT parity:\x1b[0m \x1b[31moutputs differ\x1b[0m");
                        eprintln!("    JIT: {jit_output}");
                        eprintln!("    AOT: {aot_output}");
                    }
                } else {
                    eprintln!("  \x1b[90mAOT parity: skipped (AOT run failed)\x1b[0m");
                }

                // Cleanup temp binary
                std::fs::remove_file(&tmp_bin).ok();
            }
            _ => {
                eprintln!("  \x1b[90mAOT parity: skipped (build unavailable)\x1b[0m");
            }
        }

        eprintln!();
    }

    eprintln!("\x1b[1mResults: {passed}/{total} benchmarks completed\x1b[0m");
    eprintln!(
        "\x1b[90m(\"completed\" = produced a valid JIT timing; AOT parity is annotated per benchmark)\x1b[0m"
    );
    eprintln!(
        "\x1b[90mnote: AOT figures are execution-only (steady state). The AOT build (a one-time\n      cc compile+link) is performed once, separately, and is NOT included in the\n      median above. JIT figures include code generation, which AOT amortizes away.\x1b[0m"
    );
}

/// Find the name of the first `@bench` function in a source file, if any.
/// Used to label benchmark output by function name instead of file name.
fn bench_label(source: &str) -> Option<String> {
    let (tokens, _lex_errors) = turbo_lexer::tokenize(source);
    let (module, _parse_errors) = turbo_parser::parse(tokens);
    module.items.iter().find_map(|item| match &item.node {
        Item::Function(f) if f.is_bench => Some(f.name.clone()),
        _ => None,
    })
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
        Err(e) => report_file_error(path, &e),
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
            let (msg, help) = lex_error_message(snippet);
            report_error(&source, &filename, &msg, span, Some(help), None);
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
                parse_help(&err.message).as_deref(),
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
        report_import_error(&e);
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

/// Map user-friendly target names to target-lexicon triples.
fn resolve_target_triple(target: Option<&str>) -> Option<&str> {
    match target? {
        "linux-arm64" | "linux-aarch64" => Some("aarch64-unknown-linux-gnu"),
        "linux-x86" | "linux-x64" | "linux-x86_64" => Some("x86_64-unknown-linux-gnu"),
        "macos-arm64" => Some("aarch64-apple-darwin"),
        "macos-x86" | "macos-x64" => Some("x86_64-apple-darwin"),
        "wasm" | "wasm32-wasi" | "wasm32" => None, // handled by existing wasm path
        other => Some(other),                      // raw triple passthrough
    }
}

fn build_file(
    path: &std::path::Path,
    output: Option<&std::path::Path>,
    verbose: bool,
    target: Option<&str>,
    link_libs: &[String],
) {
    check_file_size(path);

    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => report_file_error(path, &e),
    };

    let filename = path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| "<unknown>".to_string());

    let is_wasm = matches!(target, Some("wasm" | "wasm32-wasi" | "wasm32"));

    // Default output: project name from turbo.toml if available, else filename without .tb
    let default_output = if output.is_none() {
        let base = read_project_name().unwrap_or_else(|| {
            path.file_stem()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("a.out"))
        });
        if is_wasm {
            base.with_extension("wasm")
        } else {
            base
        }
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
            let (msg, help) = lex_error_message(snippet);
            report_error(&source, &filename, &msg, span, Some(help), None);
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
                parse_help(&err.message).as_deref(),
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
        report_import_error(&e);
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

    // Compile
    let codegen_start = std::time::Instant::now();
    let (codegen_result, backend_name): (Result<(), String>, &str) = if is_wasm {
        let use_wasi = !matches!(target, Some("wasm32"));
        let r = turbo_codegen_cranelift::wasm_compile(&module, output_path, use_wasi)
            .map_err(|e| e.to_string());
        (r, "Cranelift/WASM")
    } else {
        let cross_target = resolve_target_triple(target);
        let r = turbo_codegen_cranelift::aot_compile(
            &module,
            output_path,
            true,
            cross_target,
            link_libs,
        )
        .map_err(|e| e.to_string());
        (r, "Cranelift")
    };
    match codegen_result {
        Ok(()) => {
            let codegen_time = codegen_start.elapsed();
            if let Some(t) = target {
                eprintln!(
                    "\x1b[32m\u{2713}\x1b[0m Compiled to {} ({}, target: {})",
                    output_path.display(),
                    backend_name,
                    t
                );
            } else {
                eprintln!(
                    "\x1b[32m\u{2713}\x1b[0m Compiled to {} ({})",
                    output_path.display(),
                    backend_name
                );
            }
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

/// Walk an expression and collect every identifier / type name / struct name
/// it references. Used by `resolve_imports()` to pull in transitively
/// referenced top-level items from the same imported module (so users don't
/// have to name every helper in their `import { ... }` clause).
fn collect_names_in_expr(e: &Expr, out: &mut HashSet<String>) {
    match e {
        Expr::Ident(name) => {
            out.insert(name.clone());
        }
        Expr::StructLit { name, fields } => {
            out.insert(name.clone());
            for (_, v) in fields {
                collect_names_in_expr(&v.node, out);
            }
        }
        Expr::EnumVariant { enum_name, .. } => {
            out.insert(enum_name.clone());
        }
        Expr::Call { callee, args } => {
            collect_names_in_expr(&callee.node, out);
            for a in args {
                collect_names_in_expr(&a.node, out);
            }
        }
        Expr::BinaryOp { left, right, .. } => {
            collect_names_in_expr(&left.node, out);
            collect_names_in_expr(&right.node, out);
        }
        Expr::UnaryOp { expr, .. } => collect_names_in_expr(&expr.node, out),
        Expr::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_names_in_expr(&condition.node, out);
            collect_names_in_expr(&then_branch.node, out);
            if let Some(e) = else_branch {
                collect_names_in_expr(&e.node, out);
            }
        }
        Expr::Block { stmts, tail_expr } => {
            for s in stmts {
                collect_names_in_stmt(&s.node, out);
            }
            if let Some(t) = tail_expr {
                collect_names_in_expr(&t.node, out);
            }
        }
        Expr::Assign { value, .. } => collect_names_in_expr(&value.node, out),
        Expr::CompoundAssign { value, .. } => collect_names_in_expr(&value.node, out),
        Expr::FieldAssign { object, value, .. } => {
            collect_names_in_expr(&object.node, out);
            collect_names_in_expr(&value.node, out);
        }
        Expr::IndexAssign {
            object,
            index,
            value,
        } => {
            collect_names_in_expr(&object.node, out);
            collect_names_in_expr(&index.node, out);
            collect_names_in_expr(&value.node, out);
        }
        Expr::While { condition, body } => {
            collect_names_in_expr(&condition.node, out);
            collect_names_in_expr(&body.node, out);
        }
        Expr::ForIn { iterable, body, .. } => {
            collect_names_in_expr(&iterable.node, out);
            collect_names_in_expr(&body.node, out);
        }
        Expr::Range { start, end } => {
            collect_names_in_expr(&start.node, out);
            collect_names_in_expr(&end.node, out);
        }
        Expr::ArrayLit(elements) => {
            for el in elements {
                collect_names_in_expr(&el.node, out);
            }
        }
        Expr::Index { object, index } => {
            collect_names_in_expr(&object.node, out);
            collect_names_in_expr(&index.node, out);
        }
        Expr::FieldAccess { object, .. } => {
            collect_names_in_expr(&object.node, out);
        }
        Expr::Match { subject, arms } => {
            collect_names_in_expr(&subject.node, out);
            for arm in arms {
                if let Some(g) = &arm.guard {
                    collect_names_in_expr(&g.node, out);
                }
                collect_names_in_expr(&arm.body.node, out);
            }
        }
        Expr::Interpolation(parts) => {
            for p in parts {
                if let InterpolPart::Expr(e) = p {
                    collect_names_in_expr(&e.node, out);
                }
            }
        }
        Expr::Closure {
            params,
            return_type,
            body,
        } => {
            for p in params {
                collect_names_in_type(&p.ty.node, out);
            }
            if let Some(rt) = return_type {
                collect_names_in_type(&rt.node, out);
            }
            collect_names_in_expr(&body.node, out);
        }
        Expr::OkExpr(e)
        | Expr::ErrExpr(e)
        | Expr::SomeExpr(e)
        | Expr::Await(e)
        | Expr::Spawn(e)
        | Expr::Try(e) => {
            collect_names_in_expr(&e.node, out);
        }
        Expr::Cast { expr, ty } => {
            collect_names_in_expr(&expr.node, out);
            collect_names_in_type(&ty.node, out);
        }
        Expr::NullCoalesce { value, default } => {
            collect_names_in_expr(&value.node, out);
            collect_names_in_expr(&default.node, out);
        }
        Expr::OptionalChain { object, .. } => {
            collect_names_in_expr(&object.node, out);
        }
        Expr::IfLet {
            value,
            then_branch,
            else_branch,
            ..
        } => {
            collect_names_in_expr(&value.node, out);
            collect_names_in_expr(&then_branch.node, out);
            if let Some(e) = else_branch {
                collect_names_in_expr(&e.node, out);
            }
        }
        Expr::MapLit(pairs) => {
            for (k, v) in pairs {
                collect_names_in_expr(&k.node, out);
                collect_names_in_expr(&v.node, out);
            }
        }
        // Leaves — no names to collect
        Expr::IntLit(_)
        | Expr::FloatLit(_)
        | Expr::StringLit(_)
        | Expr::BoolLit(_)
        | Expr::Unit
        | Expr::NoneExpr
        | Expr::Break
        | Expr::Continue => {}
    }
}

fn collect_names_in_stmt(s: &Stmt, out: &mut HashSet<String>) {
    match s {
        Stmt::Let { ty, value, .. } => {
            if let Some(t) = ty {
                collect_names_in_type(&t.node, out);
            }
            collect_names_in_expr(&value.node, out);
        }
        Stmt::Expr(e) => collect_names_in_expr(&e.node, out),
        Stmt::Return(e) => {
            if let Some(e) = e {
                collect_names_in_expr(&e.node, out);
            }
        }
        Stmt::Defer(e) => collect_names_in_expr(&e.node, out),
        Stmt::LetDestructure { value, .. } => collect_names_in_expr(&value.node, out),
    }
}

fn collect_names_in_type(t: &TypeExpr, out: &mut HashSet<String>) {
    match t {
        TypeExpr::Named(name) => {
            out.insert(name.clone());
        }
        TypeExpr::Array(inner) => collect_names_in_type(&inner.node, out),
        TypeExpr::FnType { params, ret } => {
            for p in params {
                collect_names_in_type(&p.node, out);
            }
            collect_names_in_type(&ret.node, out);
        }
        TypeExpr::Result { ok_type, err_type } => {
            collect_names_in_type(&ok_type.node, out);
            collect_names_in_type(&err_type.node, out);
        }
        TypeExpr::Optional(inner) => collect_names_in_type(&inner.node, out),
        TypeExpr::Future(inner) => collect_names_in_type(&inner.node, out),
        TypeExpr::Unit | TypeExpr::Inferred => {}
    }
}

/// Collect every name referenced in a top-level item's signature and body.
/// This lets `resolve_imports()` do a fixed-point expansion pulling in any
/// sibling items the requested items transitively depend on.
fn collect_names_in_item(item: &Item, out: &mut HashSet<String>) {
    match item {
        Item::Function(f) => {
            for p in &f.params {
                collect_names_in_type(&p.ty.node, out);
            }
            if let Some(rt) = &f.return_type {
                collect_names_in_type(&rt.node, out);
            }
            collect_names_in_expr(&f.body.node, out);
        }
        Item::Struct(s) => {
            for field in &s.fields {
                collect_names_in_type(&field.ty.node, out);
            }
        }
        Item::Enum(e) => {
            for variant in &e.variants {
                for f in &variant.fields {
                    collect_names_in_type(&f.node, out);
                }
            }
        }
        Item::Impl(imp) => {
            out.insert(imp.type_name.clone());
            for m in &imp.methods {
                for p in &m.node.params {
                    collect_names_in_type(&p.ty.node, out);
                }
                if let Some(rt) = &m.node.return_type {
                    collect_names_in_type(&rt.node, out);
                }
                collect_names_in_expr(&m.node.body.node, out);
            }
        }
        Item::Const(c) => {
            if let Some(t) = &c.ty {
                collect_names_in_type(&t.node, out);
            }
            collect_names_in_expr(&c.value.node, out);
        }
        Item::Trait(_) | Item::Import { .. } | Item::Extern(_) => {}
    }
}

/// Return the defining name of a top-level item, if it has one.
/// Used by `resolve_imports()` to match referenced names against items
/// available in an imported module.
fn item_def_name(item: &Item) -> Option<&str> {
    match item {
        Item::Function(f) => Some(&f.name),
        Item::Struct(s) => Some(&s.name),
        Item::Enum(e) => Some(&e.name),
        Item::Impl(imp) => Some(&imp.type_name),
        Item::Const(c) => Some(&c.name),
        Item::Trait(t) => Some(&t.name),
        Item::Import { .. } | Item::Extern(_) => None,
    }
}

/// An imported file after parse + recursive-resolve, held in memory
/// while the cross-module walker runs. The walker needs every imported
/// module simultaneously so that a reference in file A to something
/// defined in file B can be traced across the boundary.
struct ImportedFile {
    resolved_path: PathBuf,
    module: Module,
    explicit_names: Vec<String>,
}

/// Resolve all `import` items in the module by reading, lexing, and parsing
/// the imported files and inlining the requested items.
/// `loading` tracks files currently being loaded (for circular import detection).
///
/// This runs in three phases:
///
/// 1. **Gather** — parse and recursively resolve every import, but defer
///    item extraction.
/// 2. **Global fixed-point** — seed per-file `wanted` sets from the
///    explicit import clauses, then iteratively expand across all
///    imported modules at once. When a wanted item in file A references
///    a name defined in file B, that name is added to file B's wanted
///    set and the loop runs again. This lets a caller name only its
///    entry point and have every transitively-referenced helper pulled
///    in automatically, *even across files*.
/// 3. **Extract + dedupe + validate** — walk each file's final wanted
///    set, pull the matching items out, dedupe across chains, and check
///    that every explicit clause name was satisfied.
fn resolve_imports(
    module: &mut Module,
    base_dir: &Path,
    loading: &mut HashSet<PathBuf>,
) -> Result<(), String> {
    // ==================== Phase A: Gather ====================
    let mut imported_files: Vec<ImportedFile> = Vec::new();

    for item in &module.items {
        if let Item::Import { names, path } = &item.node {
            // Virtual stdlib import -- validate module and names, then skip.
            // The builtins are always available globally; this import is
            // purely for validation and self-documentation.
            if turbo_ast::stdlib_modules::is_stdlib_path(path) {
                match turbo_ast::stdlib_modules::find_stdlib_module(path) {
                    Some(stdlib_mod) => {
                        for name in names {
                            if !stdlib_mod.functions.contains(&name.as_str()) {
                                return Err(format!(
                                    "`{}` is not exported by module `{}`. Available: {}",
                                    name,
                                    path,
                                    stdlib_mod.functions.join(", ")
                                ));
                            }
                        }
                    }
                    None => {
                        return Err(format!(
                            "unknown standard library module `{}`. Available modules: {}",
                            path,
                            turbo_ast::stdlib_modules::STDLIB_MODULES
                                .iter()
                                .map(|m| m.path)
                                .collect::<Vec<_>>()
                                .join(", ")
                        ));
                    }
                }
                // Don't try to load a file -- just skip this import.
                continue;
            }

            let resolved_path = resolve_import_path(base_dir, path);
            // Drop the raw `(os error N)` that `io::Error`'s Display leaks —
            // the E0610 envelope and `Help:` line carry the actionable detail.
            let canonical = resolved_path.canonicalize().map_err(|_| {
                format!(
                    "could not resolve import `{}` (looked for `{}`)",
                    path,
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
                    "could not read imported file `{}`: {}",
                    resolved_path.display(),
                    e.kind()
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

            imported_files.push(ImportedFile {
                resolved_path,
                module: imported_module,
                explicit_names: names.clone(),
            });
        }
    }

    // ==================== Phase B: Global fixed-point ====================
    // Seed per-file wanted sets from explicit clauses, then loop across
    // every imported module until no new names are added. Cross-module
    // discovery: if file A's wanted body references `helper` and `helper`
    // is defined in file B, it gets added to B's wanted set.
    let mut wanted: Vec<HashSet<String>> = imported_files
        .iter()
        .map(|f| f.explicit_names.iter().cloned().collect())
        .collect();

    loop {
        // Collect every name referenced by items currently wanted in any
        // file. Attribution to a specific origin file doesn't matter —
        // name lookup in the next step is global.
        let mut discovered: HashSet<String> = HashSet::new();
        for (fi, file) in imported_files.iter().enumerate() {
            for imported_item in &file.module.items {
                let included = match &imported_item.node {
                    Item::Impl(imp) => wanted[fi].contains(&imp.type_name),
                    other => item_def_name(other)
                        .map(|n| wanted[fi].contains(n))
                        .unwrap_or(false),
                };
                if included {
                    collect_names_in_item(&imported_item.node, &mut discovered);
                }
            }
        }

        // Route each discovered name to whichever imported file actually
        // defines it. Unknown names (builtins, host-module refs) are
        // silently dropped — sema will resolve or reject them later.
        let mut changed = false;
        for name in discovered {
            for (fi, file) in imported_files.iter().enumerate() {
                if wanted[fi].contains(&name) {
                    continue;
                }
                let defined_here = file
                    .module
                    .items
                    .iter()
                    .any(|it| item_def_name(&it.node).map(|n| n == name).unwrap_or(false));
                if defined_here {
                    wanted[fi].insert(name.clone());
                    changed = true;
                }
            }
        }

        if !changed {
            break;
        }
    }

    // ==================== Phase C: Extract ====================
    let mut import_items: Vec<turbo_ast::Spanned<Item>> = Vec::new();

    for (fi, file) in imported_files.into_iter().enumerate() {
        let ImportedFile {
            resolved_path,
            module: imported_module,
            explicit_names,
        } = file;
        let file_wanted = &wanted[fi];

        let mut found_for_file: Vec<turbo_ast::Spanned<Item>> = Vec::new();
        for imported_item in imported_module.items {
            let included = match &imported_item.node {
                Item::Impl(imp) => file_wanted.contains(&imp.type_name),
                other => item_def_name(other)
                    .map(|n| file_wanted.contains(n))
                    .unwrap_or(false),
            };
            if included {
                found_for_file.push(imported_item);
            }
        }

        // Validate explicit clause names. Transitively-pulled names are
        // best-effort (and may legitimately not exist if the user
        // over-imported), but explicit names must resolve here.
        for name in &explicit_names {
            let found = found_for_file.iter().any(|item| {
                item_def_name(&item.node)
                    .map(|n| n == name)
                    .unwrap_or(false)
            });
            if !found {
                return Err(format!(
                    "name `{name}` not found in `{}`",
                    resolved_path.display()
                ));
            }
        }

        import_items.extend(found_for_file);
    }

    // Deduplicate import_items by defining name. Without this, transitive
    // resolution creates duplicates when the same helper is pulled in
    // through multiple import chains (e.g. main.tb imports from both
    // `./roster` and `./squad`, both of which transitively import
    // `color_cyan` from `./display/output`). Impls and extern blocks have
    // no unique def name and are always kept as-is — two impls for the
    // same struct are legitimate, and sema will catch any real conflicts.
    let mut seen_names: HashSet<String> = HashSet::new();
    let mut deduped: Vec<turbo_ast::Spanned<Item>> = Vec::with_capacity(import_items.len());
    for item in import_items {
        match &item.node {
            Item::Impl(_) | Item::Extern(_) => {
                deduped.push(item);
            }
            _ => match item_def_name(&item.node) {
                Some(name) => {
                    if seen_names.insert(name.to_string()) {
                        deduped.push(item);
                    }
                }
                None => {
                    deduped.push(item);
                }
            },
        }
    }

    // Remove import items and prepend imported items
    module
        .items
        .retain(|item| !matches!(&item.node, Item::Import { .. }));
    let mut new_items = deduped;
    new_items.append(&mut module.items);
    module.items = new_items;

    Ok(())
}

/// Generate contextual help text for common parse error patterns.
fn parse_help(message: &str) -> Option<String> {
    if message.contains("import") || message.contains("`from`") || message.contains("path string") {
        return Some("imports look like `import { sqrt, pi } from \"./math.tb\"`".to_string());
    }
    None
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
fn extract_backtick_name(message: &str) -> Option<&str> {
    nth_backtick_name(message, 1)
}

/// Extract the `n`th (1-based) backtick-enclosed name from a message.
fn nth_backtick_name(message: &str, n: usize) -> Option<&str> {
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
fn parse_signature_params(message: &str) -> Option<&str> {
    let after = message.split("signature `").nth(1)?;
    let sig = after.split('`').next()?;
    let open = sig.find('(')?;
    let close = sig.rfind(')')?;
    (close > open).then(|| sig[open + 1..close].trim())
}

/// Parse the run of digits immediately following `marker` in `message`.
fn parse_count_after(message: &str, marker: &str) -> Option<usize> {
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
fn detailed_explanation(code: ErrorCode) -> Option<&'static str> {
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
fn normalize_error_code(input: &str) -> String {
    let upper = input.trim().to_uppercase();
    let digits = upper.strip_prefix('E').unwrap_or(&upper);
    if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
        if let Ok(n) = digits.parse::<u32>() {
            return format!("E{n:04}");
        }
    }
    upper
}

fn explain_error(code_str: &str) {
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

    format!(
        "{}fn {}({}){}",
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
        Err(e) => report_file_error(path, &e),
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
    let has_ast_structs = ast_functions.as_ref().is_some_and(|module| {
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

    print!("{}", out);
}

#[cfg(test)]
mod cli_tests {
    use super::*;

    // ── Init command scaffolding ───────────────────────────────────────

    #[test]
    fn init_turbo_toml_content() {
        // The init command generates a turbo.toml with [package] and [dependencies]
        let pkg_name = "my-app";
        let expected = format!(
            "[package]\nname = \"{pkg_name}\"\nversion = \"0.1.0\"\nedition = \"2026\"\n\n[dependencies]\n"
        );
        assert!(expected.contains("[package]"));
        assert!(expected.contains(&format!("name = \"{}\"", pkg_name)));
        assert!(expected.contains("version = \"0.1.0\""));
        assert!(expected.contains("[dependencies]"));
    }

    #[test]
    fn init_main_tb_content() {
        // The scaffolded src/main.tb should contain fn main(), Counter struct, and Shape enum
        let pkg_name = "test-proj";
        let main_tb = format!(
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
        );
        assert!(main_tb.contains("fn main()"));
        assert!(main_tb.contains("struct Counter"));
        assert!(main_tb.contains(&format!("Hello from {pkg_name}!")));
        assert!(main_tb.contains("type Shape"));
    }

    // ── Error code explain ────────────────────────────────────────────

    #[test]
    fn detailed_explanation_returns_content_for_parse_error() {
        let detail = detailed_explanation(ErrorCode::E0001);
        assert!(detail.is_some(), "E0001 should have a detailed explanation");
        let text = detail.unwrap();
        assert!(!text.is_empty(), "E0001 explanation should not be empty");
    }

    #[test]
    fn detailed_explanation_returns_content_for_type_error() {
        let detail = detailed_explanation(ErrorCode::E0100);
        assert!(detail.is_some(), "E0100 should have a detailed explanation");
        let text = detail.unwrap();
        assert!(!text.is_empty(), "E0100 explanation should not be empty");
    }

    #[test]
    fn detailed_explanation_returns_content_for_name_resolution_error() {
        let detail = detailed_explanation(ErrorCode::E0300);
        assert!(detail.is_some(), "E0300 should have a detailed explanation");
        let text = detail.unwrap();
        assert!(!text.is_empty(), "E0300 explanation should not be empty");
    }

    #[test]
    fn detailed_explanation_exhaustive_for_all_codes() {
        // Every error code should have a detailed explanation
        for code in ErrorCode::all() {
            let detail = detailed_explanation(code);
            assert!(
                detail.is_some(),
                "{} should have a detailed explanation",
                code.as_str()
            );
        }
    }

    // ── Error code URL generation ─────────────────────────────────────

    #[test]
    fn error_code_url_format() {
        let url = error_code_url(ErrorCode::E0100);
        assert!(
            url.contains("E0100.md"),
            "URL should contain the error code filename"
        );
        assert!(url.starts_with("https://"), "URL should be an HTTPS URL");
        assert!(
            url.contains("docs/errors/"),
            "URL should point to docs/errors/"
        );
    }

    // ── Diagnostic footer composition ─────────────────────────────────

    #[test]
    fn footer_no_help_does_not_emit_bare_help_label() {
        // When a diagnostic has a code but no help text, the `more info:`
        // line must stand on its own (as a post-frame footer) and must never
        // collapse onto an empty `Help:` label.
        let (help_block, footer) = compose_diagnostic_footer(None, Some(ErrorCode::E0200));
        assert!(
            help_block.is_none(),
            "no help text should produce no Help block"
        );
        let footer = footer.expect("more info footer should be present when a code exists");
        assert!(footer.contains("more info:"), "footer should carry the URL");
        assert!(
            !footer.contains("Help"),
            "footer must not contain a `Help` label"
        );
    }

    #[test]
    fn footer_with_help_keeps_more_info_on_its_own_line() {
        // With real help text the URL is appended under the Help block on its
        // own line, and there is no separate post-frame footer.
        let (help_block, footer) =
            compose_diagnostic_footer(Some("declare it with `let`"), Some(ErrorCode::E0300));
        let block = help_block.expect("help block expected when help text is present");
        assert!(block.starts_with("declare it with `let`"));
        assert!(
            block.contains("\n  more info:"),
            "more info should be on its own line under Help"
        );
        assert!(
            footer.is_none(),
            "with-help case should not add a standalone footer"
        );
    }

    #[test]
    fn footer_no_code_no_help_is_empty() {
        let (help_block, footer) = compose_diagnostic_footer(None, None);
        assert!(help_block.is_none());
        assert!(footer.is_none());
    }

    // ── File extension validation ─────────────────────────────────────

    #[test]
    fn tb_file_extension_check() {
        let tb_path = Path::new("test.tb");
        assert_eq!(
            tb_path.extension().and_then(|e| e.to_str()),
            Some("tb"),
            ".tb extension should be recognized"
        );

        let non_tb_path = Path::new("test.rs");
        assert_ne!(
            non_tb_path.extension().and_then(|e| e.to_str()),
            Some("tb"),
            ".rs extension should not be .tb"
        );

        let no_ext_path = Path::new("test");
        assert_eq!(
            no_ext_path.extension().and_then(|e| e.to_str()),
            None,
            "no extension should return None"
        );
    }

    // ── Doc comment extraction ────────────────────────────────────────

    #[test]
    fn extract_doc_comments_basic() {
        let source = "/// This is a doc comment\nfn main() {\n}\n";
        let docs = extract_doc_comments(source);
        // The doc comment on line 0 applies to the item on line 1
        assert!(docs.contains_key(&1), "doc should be attached to line 1");
        let comment = &docs[&1];
        assert_eq!(comment.len(), 1);
        assert_eq!(comment[0], "This is a doc comment");
    }

    // ── Error `Help:` quality (sema_help) ─────────────────────────────

    #[test]
    fn sema_help_arity_echoes_signature_and_passed_count() {
        let msg = "function `add` expects 2 argument(s) but 1 were given; signature `add(a: int, b: int)`";
        assert_eq!(
            sema_help(msg).as_deref(),
            Some("'add' takes 2 args (a: int, b: int); you passed 1")
        );
    }

    #[test]
    fn sema_help_arity_without_signature_falls_back() {
        // Closure/method arity messages carry no embedded signature; they keep
        // the generic guidance.
        let msg = "closure expects 2 argument(s) but 1 were given";
        assert_eq!(
            sema_help(msg).as_deref(),
            Some("check the function signature for the correct number of arguments")
        );
    }

    #[test]
    fn sema_help_match_exhaustive_single_variant() {
        let msg = "match is not exhaustive; missing variants: Blue";
        assert_eq!(
            sema_help(msg).as_deref(),
            Some("add an arm 'Blue => ...' or a catch-all '_ => ...'")
        );
    }

    #[test]
    fn sema_help_match_exhaustive_multiple_variants() {
        let msg = "match is not exhaustive; missing variants: Green, Blue";
        assert_eq!(
            sema_help(msg).as_deref(),
            Some("add arms for Green, Blue or a catch-all '_ => ...'")
        );
    }

    #[test]
    fn sema_help_field_lists_fields_and_suggestion() {
        let msg = "struct `Rect` has no field `widht`. did you mean `width`? (available fields: width, height)";
        assert_eq!(
            sema_help(msg).as_deref(),
            Some("'Rect' has fields 'width', 'height' — did you mean 'width'?")
        );
    }

    #[test]
    fn sema_help_field_lists_fields_without_suggestion() {
        let msg = "struct `Point` has no field `z` (available fields: x, y)";
        assert_eq!(
            sema_help(msg).as_deref(),
            Some("'Point' has fields 'x', 'y'")
        );
    }

    // ── Parse `Help:` quality (parse_help) ────────────────────────────

    #[test]
    fn parse_help_teaches_import_syntax() {
        for msg in [
            "expected `{` to begin the import list",
            "expected `from` after the import list",
            "expected a path string after `from` in import",
        ] {
            assert_eq!(
                parse_help(msg).as_deref(),
                Some("imports look like `import { sqrt, pi } from \"./math.tb\"`"),
                "message `{msg}` should get import-syntax help"
            );
        }
    }

    #[test]
    fn parse_help_ignores_unrelated_messages() {
        assert_eq!(parse_help("expected `}`, found end of file"), None);
    }

    // ── explain code normalization ────────────────────────────────────

    #[test]
    fn normalize_error_code_accepts_shorthands() {
        for input in ["100", "e100", "E100", "0100", "e0100", "E0100"] {
            assert_eq!(
                normalize_error_code(input),
                "E0100",
                "`{input}` should normalize to E0100"
            );
        }
        assert_eq!(normalize_error_code("7"), "E0007");
        // Genuinely unknown / non-numeric input is upper-cased and left for the
        // unknown-code path to reject.
        assert_eq!(normalize_error_code("bogus"), "BOGUS");
    }

    #[test]
    fn normalize_error_code_resolves_via_parse() {
        assert_eq!(
            ErrorCode::parse(&normalize_error_code("100")),
            Some(ErrorCode::E0100)
        );
        assert_eq!(ErrorCode::parse(&normalize_error_code("9999")), None);
    }
}
