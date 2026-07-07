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

use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};

mod deps;
mod diagnostics;
mod doc;
mod explain;
mod imports;
mod pipeline;
mod playground;
mod project;
mod repl;
mod watch;

use crate::deps::{install_deps, update_deps};
use crate::doc::doc_file;
use crate::explain::explain_error;
use crate::pipeline::{bench_file, build_file, check_file, run_file, test_file, test_run_fn};
use crate::project::init_project;

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

#[cfg(test)]
mod cli_tests {
    use turbo_ast::ErrorCode;

    use crate::diagnostics::{compose_diagnostic_footer, error_code_url};
    use crate::doc::extract_doc_comments;
    use crate::explain::{detailed_explanation, normalize_error_code, parse_help, sema_help};

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
