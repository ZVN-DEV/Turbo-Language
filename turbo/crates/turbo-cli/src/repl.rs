use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;

pub fn run_repl() {
    let version = env!("CARGO_PKG_VERSION");
    println!("\x1b[1;36mTurbo v{version}\x1b[0m \x1b[90m\u{2014} Interactive Mode\x1b[0m");
    println!(
        "\x1b[90mType expressions or statements. Use :help for commands, :quit to exit.\x1b[0m"
    );
    println!();

    let mut rl = DefaultEditor::new().unwrap_or_else(|e| {
        eprintln!("warning: could not initialize line editor: {e}");
        // Fall back to a basic editor
        DefaultEditor::new().expect("failed to create editor")
    });

    // Load history from ~/.turbo_history
    let history_path = dirs_path();
    let _ = rl.load_history(&history_path);

    let mut accumulated_items: Vec<String> = Vec::new(); // fn definitions, structs, etc.
    let mut accumulated_vars: Vec<String> = Vec::new(); // let bindings

    loop {
        let readline = rl.readline(">>> ");
        let line = match readline {
            Ok(line) => line,
            Err(ReadlineError::Interrupted) => {
                // Ctrl-C: clear current line, continue
                continue;
            }
            Err(ReadlineError::Eof) => {
                // Ctrl-D: exit
                break;
            }
            Err(err) => {
                eprintln!("error: {err}");
                break;
            }
        };
        let line = line.trim();

        if line.is_empty() {
            continue;
        }

        // Add to history
        let _ = rl.add_history_entry(line);

        // REPL commands
        if line == ":quit" || line == ":q" || line == ":exit" || line == "exit" || line == "quit" {
            println!("Goodbye!");
            break;
        }
        if line == ":help" || line == ":h" {
            println!("\x1b[1mREPL Commands:\x1b[0m");
            println!("  \x1b[33m:help\x1b[0m    Show this help");
            println!("  \x1b[33m:clear\x1b[0m   Clear accumulated state");
            println!("  \x1b[33m:quit\x1b[0m    Exit the REPL");
            println!();
            println!("\x1b[1mUsage:\x1b[0m");
            println!("  Define functions, structs, enums, and other items directly.");
            println!("  Evaluate expressions by typing them at the prompt.");
            println!("  Use \x1b[36mlet x = ...\x1b[0m to bind variables across lines.");
            continue;
        }
        if line == ":clear" {
            accumulated_items.clear();
            accumulated_vars.clear();
            println!("State cleared.");
            continue;
        }

        // Determine what kind of input this is
        let is_item = line.starts_with("fn ")
            || line.starts_with("struct ")
            || line.starts_with("type ")
            || line.starts_with("impl ")
            || line.starts_with("trait ")
            || line.starts_with("tool ")
            || line.starts_with("agent ")
            || line.starts_with("async fn ")
            || line.starts_with("enum ")
            || line.starts_with("import ");

        if is_item {
            // Multi-line: read until braces balance (for items that use braces)
            let mut full = line.to_string();
            let mut brace_count: i32 =
                full.matches('{').count() as i32 - full.matches('}').count() as i32;
            while brace_count > 0 {
                match rl.readline("... ") {
                    Ok(more) => {
                        let more = more.trim_end();
                        brace_count +=
                            more.matches('{').count() as i32 - more.matches('}').count() as i32;
                        full.push('\n');
                        full.push_str(more);
                    }
                    Err(_) => break,
                }
            }
            accumulated_items.push(full);
            continue; // silently accept item definitions
        }

        let is_let = line.starts_with("let ");
        if is_let {
            accumulated_vars.push(line.to_string());
        }

        // For bare expressions (not let bindings), try auto-printing the result
        let skip_auto_print = line.starts_with("print(")
            || line.starts_with("assert(")
            || line.starts_with("assert_eq(");

        if !is_let && !skip_auto_print {
            let mut print_prog = String::new();
            for item in &accumulated_items {
                print_prog.push_str(item);
                print_prog.push('\n');
            }
            print_prog.push_str("fn main() {\n");
            for var in &accumulated_vars {
                print_prog.push_str("    ");
                print_prog.push_str(var);
                print_prog.push('\n');
            }
            print_prog.push_str("    print(");
            print_prog.push_str(line);
            print_prog.push_str(")\n");
            print_prog.push_str("}\n");

            let (tokens, lex_errs) = turbo_lexer::tokenize(&print_prog);
            if lex_errs.is_empty() {
                let (module, parse_errs) = turbo_parser::parse(tokens);
                if parse_errs.is_empty() {
                    let sema_result = turbo_sema::check(&module);
                    if sema_result.errors.is_empty() {
                        if turbo_codegen_cranelift::jit_run(&module).is_ok() {
                            continue;
                        }
                    }
                }
            }
        }

        // Build the bare program (for let bindings, or when auto-print fails)
        let mut program = String::new();
        for item in &accumulated_items {
            program.push_str(item);
            program.push('\n');
        }
        program.push_str("fn main() {\n");
        for var in &accumulated_vars {
            program.push_str("    ");
            program.push_str(var);
            program.push('\n');
        }
        if !is_let {
            program.push_str("    ");
            program.push_str(line);
            program.push('\n');
        }
        program.push_str("}\n");

        // Lex
        let (tokens, lex_errors) = turbo_lexer::tokenize(&program);
        if !lex_errors.is_empty() {
            for span in &lex_errors {
                let snippet = &program[span.clone()];
                eprintln!("error: unexpected character `{snippet}`");
            }
            if is_let {
                accumulated_vars.pop();
            }
            continue;
        }

        // Parse
        let (module, parse_errors) = turbo_parser::parse(tokens);
        if !parse_errors.is_empty() {
            for e in &parse_errors {
                eprintln!("error: {}", e.message);
            }
            if is_let {
                accumulated_vars.pop();
            }
            continue;
        }

        // Semantic analysis
        let sema_result = turbo_sema::check(&module);
        for w in &sema_result.warnings {
            eprintln!("warning: {}", w.message);
        }
        if !sema_result.errors.is_empty() {
            for e in &sema_result.errors {
                eprintln!("error: {}", e.message);
            }
            if is_let {
                accumulated_vars.pop();
            }
            continue;
        }

        // Compile & run via JIT
        match turbo_codegen_cranelift::jit_run(&module) {
            Ok(()) => {}
            Err(e) => {
                eprintln!("error: {}", e.message);
                if is_let {
                    accumulated_vars.pop();
                }
            }
        }
    }

    // Save history
    let _ = rl.save_history(&history_path);
}

/// Get the path to the REPL history file (~/.turbo_history).
fn dirs_path() -> String {
    if let Some(home) = std::env::var_os("HOME") {
        let mut path = std::path::PathBuf::from(home);
        path.push(".turbo_history");
        path.to_string_lossy().to_string()
    } else {
        ".turbo_history".to_string()
    }
}
