use std::io::{self, BufRead, Write};

pub fn run_repl() {
    println!("Turbo v0.1.0 \u{2014} Interactive Mode");
    println!("Type expressions or statements. Use :quit to exit.\n");

    let stdin = io::stdin();
    let mut accumulated_items: Vec<String> = Vec::new(); // fn definitions, structs, etc.
    let mut accumulated_vars: Vec<String> = Vec::new(); // let bindings

    loop {
        print!(">>> ");
        io::stdout().flush().unwrap();

        let mut line = String::new();
        if stdin.lock().read_line(&mut line).unwrap() == 0 {
            break; // EOF
        }
        let line = line.trim();

        if line.is_empty() {
            continue;
        }

        // REPL commands
        if line == ":quit" || line == ":q" || line == ":exit" {
            println!("Goodbye!");
            break;
        }
        if line == ":help" || line == ":h" {
            println!("Commands:");
            println!("  :quit    Exit the REPL");
            println!("  :clear   Clear accumulated state");
            println!("  :help    Show this help");
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
                print!("... ");
                io::stdout().flush().unwrap();
                let mut more = String::new();
                if stdin.lock().read_line(&mut more).unwrap() == 0 {
                    break;
                }
                let more = more.trim_end();
                brace_count +=
                    more.matches('{').count() as i32 - more.matches('}').count() as i32;
                full.push('\n');
                full.push_str(more);
            }
            accumulated_items.push(full);
            continue; // silently accept item definitions
        }

        let is_let = line.starts_with("let ");
        if is_let {
            accumulated_vars.push(line.to_string());
        }

        // Build a complete program wrapping everything in main()
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
            // This is an expression/statement -- add it
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
        let sema_errors = turbo_sema::check(&module);
        if !sema_errors.is_empty() {
            for e in &sema_errors {
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
}
