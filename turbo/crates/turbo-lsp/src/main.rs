use lsp_server::{Connection, Message, Notification, Response};
use lsp_types::*;
use std::collections::HashMap;

fn main() {
    let (connection, io_threads) = Connection::stdio();

    let capabilities = ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        definition_provider: Some(OneOf::Left(true)),
        ..Default::default()
    };

    let init_result = match serde_json::to_value(InitializeResult {
        capabilities,
        ..Default::default()
    }) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("turbo-lsp: failed to serialize InitializeResult: {e}");
            return;
        }
    };

    if let Err(e) = connection.initialize(init_result) {
        eprintln!("turbo-lsp: initialization handshake failed: {e}");
        return;
    }

    #[allow(clippy::mutable_key_type)] // Uri from lsp-types uses interior mutability for caching
    let mut documents: HashMap<Uri, String> = HashMap::new();

    for msg in &connection.receiver {
        match msg {
            Message::Notification(not) => match not.method.as_str() {
                "textDocument/didOpen" => {
                    let params: DidOpenTextDocumentParams = match serde_json::from_value(not.params)
                    {
                        Ok(p) => p,
                        Err(e) => {
                            eprintln!("turbo-lsp: bad didOpen params: {e}");
                            continue;
                        }
                    };
                    let uri = params.text_document.uri.clone();
                    let text = params.text_document.text.clone();
                    documents.insert(uri.clone(), text.clone());
                    publish_diagnostics(&connection, &uri, &text);
                }
                "textDocument/didChange" => {
                    let params: DidChangeTextDocumentParams =
                        match serde_json::from_value(not.params) {
                            Ok(p) => p,
                            Err(e) => {
                                eprintln!("turbo-lsp: bad didChange params: {e}");
                                continue;
                            }
                        };
                    let uri = params.text_document.uri.clone();
                    if let Some(change) = params.content_changes.into_iter().last() {
                        documents.insert(uri.clone(), change.text.clone());
                        publish_diagnostics(&connection, &uri, &change.text);
                    }
                }
                "textDocument/didClose" => {
                    let params: DidCloseTextDocumentParams =
                        match serde_json::from_value(not.params) {
                            Ok(p) => p,
                            Err(e) => {
                                eprintln!("turbo-lsp: bad didClose params: {e}");
                                continue;
                            }
                        };
                    documents.remove(&params.text_document.uri);
                }
                _ => {}
            },
            Message::Request(req) => match req.method.as_str() {
                "textDocument/hover" => {
                    let params: HoverParams = match serde_json::from_value(req.params.clone()) {
                        Ok(p) => p,
                        Err(e) => {
                            eprintln!("turbo-lsp: bad hover params: {e}");
                            let _ = send_response(
                                &connection,
                                Response::new_ok(req.id, serde_json::Value::Null),
                            );
                            continue;
                        }
                    };
                    let uri = &params.text_document_position_params.text_document.uri;
                    let pos = params.text_document_position_params.position;
                    let hover = documents.get(uri).and_then(|text| compute_hover(text, pos));
                    let result = serde_json::to_value(hover).unwrap_or(serde_json::Value::Null);
                    let _ = send_response(&connection, Response::new_ok(req.id, result));
                }
                "textDocument/definition" => {
                    let params: GotoDefinitionParams =
                        match serde_json::from_value(req.params.clone()) {
                            Ok(p) => p,
                            Err(e) => {
                                eprintln!("turbo-lsp: bad definition params: {e}");
                                let _ = send_response(
                                    &connection,
                                    Response::new_ok(req.id, serde_json::Value::Null),
                                );
                                continue;
                            }
                        };
                    let uri = &params.text_document_position_params.text_document.uri;
                    let pos = params.text_document_position_params.position;
                    let location = documents
                        .get(uri)
                        .and_then(|text| compute_definition(text, pos, uri));
                    let result = serde_json::to_value(location.map(GotoDefinitionResponse::Scalar))
                        .unwrap_or(serde_json::Value::Null);
                    let _ = send_response(&connection, Response::new_ok(req.id, result));
                }
                "shutdown" => {
                    let _ = send_response(
                        &connection,
                        Response::new_ok(req.id, serde_json::Value::Null),
                    );
                    break;
                }
                _ => {
                    let _ = send_response(
                        &connection,
                        Response::new_ok(req.id, serde_json::Value::Null),
                    );
                }
            },
            _ => {}
        }
    }

    if let Err(e) = io_threads.join() {
        eprintln!("turbo-lsp: I/O thread error: {e}");
    }
}

/// Send an LSP response, logging failures instead of panicking.
fn send_response(connection: &Connection, response: Response) -> Result<(), String> {
    connection
        .sender
        .send(Message::Response(response))
        .map_err(|e| {
            eprintln!("turbo-lsp: failed to send response: {e}");
            e.to_string()
        })
}

// ---------------------------------------------------------------------------
// Diagnostics
// ---------------------------------------------------------------------------

fn publish_diagnostics(connection: &Connection, uri: &Uri, source: &str) {
    let (tokens, lex_errors) = turbo_lexer::tokenize(source);
    let mut diagnostics = Vec::new();

    for span in &lex_errors {
        let snippet = source.get(span.clone()).unwrap_or("?");
        diagnostics.push(Diagnostic {
            range: span_to_range(source, span),
            severity: Some(DiagnosticSeverity::ERROR),
            message: format!("unexpected character `{snippet}`"),
            ..Default::default()
        });
    }

    if lex_errors.is_empty() {
        let (module, parse_errors) = turbo_parser::parse(tokens);
        for err in &parse_errors {
            diagnostics.push(Diagnostic {
                range: span_to_range(source, &err.span),
                severity: Some(DiagnosticSeverity::ERROR),
                message: err.message.clone(),
                ..Default::default()
            });
        }

        if parse_errors.is_empty() {
            let sema_errors = turbo_sema::check(&module);
            for err in &sema_errors {
                diagnostics.push(Diagnostic {
                    range: span_to_range(source, &err.span),
                    severity: Some(DiagnosticSeverity::ERROR),
                    message: err.message.clone(),
                    ..Default::default()
                });
            }
        }
    }

    let params = PublishDiagnosticsParams {
        uri: uri.clone(),
        diagnostics,
        version: None,
    };

    let value = match serde_json::to_value(params) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("turbo-lsp: failed to serialize diagnostics: {e}");
            return;
        }
    };

    if let Err(e) = connection.sender.send(Message::Notification(Notification {
        method: "textDocument/publishDiagnostics".to_string(),
        params: value,
    })) {
        eprintln!("turbo-lsp: failed to send diagnostics: {e}");
    }
}

// ---------------------------------------------------------------------------
// Hover
// ---------------------------------------------------------------------------

fn compute_hover(source: &str, pos: Position) -> Option<Hover> {
    let offset = position_to_offset(source, pos)?;
    let (tokens, _) = turbo_lexer::tokenize(source);

    for tok in &tokens {
        if tok.span.contains(&offset) {
            let content = match &tok.value {
                turbo_lexer::Token::Ident(name) => {
                    // Try to find more info from the AST
                    let extra = identifier_info(source, name);
                    if let Some(info) = extra {
                        info
                    } else {
                        format!("identifier: `{name}`")
                    }
                }
                turbo_lexer::Token::Int(n) => format!("integer literal: `{n}`"),
                turbo_lexer::Token::Float(f) => format!("float literal: `{f}`"),
                turbo_lexer::Token::String(s) => format!("string literal: `\"{s}\"`"),
                turbo_lexer::Token::Fn => "keyword: `fn` -- function definition".to_string(),
                turbo_lexer::Token::Let => "keyword: `let` -- variable binding".to_string(),
                turbo_lexer::Token::Mut => "keyword: `mut` -- mutable binding".to_string(),
                turbo_lexer::Token::Const => "keyword: `const` -- constant binding".to_string(),
                turbo_lexer::Token::If => "keyword: `if` -- conditional expression".to_string(),
                turbo_lexer::Token::Else => "keyword: `else` -- alternative branch".to_string(),
                turbo_lexer::Token::While => "keyword: `while` -- loop".to_string(),
                turbo_lexer::Token::For => "keyword: `for` -- for-in loop".to_string(),
                turbo_lexer::Token::In => "keyword: `in` -- iterator binding".to_string(),
                turbo_lexer::Token::Return => "keyword: `return` -- early return".to_string(),
                turbo_lexer::Token::Match => "keyword: `match` -- pattern matching".to_string(),
                turbo_lexer::Token::Struct => "keyword: `struct` -- struct definition".to_string(),
                turbo_lexer::Token::TypeKw => "keyword: `type` -- type alias".to_string(),
                turbo_lexer::Token::Impl => "keyword: `impl` -- implementation block".to_string(),
                turbo_lexer::Token::Trait => "keyword: `trait` -- trait definition".to_string(),
                turbo_lexer::Token::Pub => "keyword: `pub` -- public visibility".to_string(),
                turbo_lexer::Token::Import => "keyword: `import` -- import declaration".to_string(),
                turbo_lexer::Token::From => "keyword: `from` -- import source".to_string(),
                turbo_lexer::Token::Async => {
                    "keyword: `async` -- asynchronous function".to_string()
                }
                turbo_lexer::Token::Await => "keyword: `await` -- await a future".to_string(),
                turbo_lexer::Token::Spawn => {
                    "keyword: `spawn` -- spawn concurrent task".to_string()
                }
                turbo_lexer::Token::Defer => "keyword: `defer` -- deferred execution".to_string(),
                turbo_lexer::Token::Agent => "keyword: `agent` -- AI agent definition".to_string(),
                turbo_lexer::Token::Tool => {
                    "keyword: `tool` -- tool function for agents".to_string()
                }
                turbo_lexer::Token::True => "boolean literal: `true`".to_string(),
                turbo_lexer::Token::False => "boolean literal: `false`".to_string(),
                turbo_lexer::Token::None => "keyword: `none` -- absent optional value".to_string(),
                turbo_lexer::Token::Some => "keyword: `some` -- present optional value".to_string(),
                turbo_lexer::Token::Ok => "keyword: `ok` -- success result constructor".to_string(),
                turbo_lexer::Token::Err => "keyword: `err` -- error result constructor".to_string(),
                other => format!("token: `{other}`"),
            };

            return Some(Hover {
                contents: HoverContents::Scalar(MarkedString::String(content)),
                range: Some(span_to_range(source, &tok.span)),
            });
        }
    }

    None
}

/// Try to get richer info for an identifier by parsing the source and inspecting
/// top-level definitions for a matching name.
fn identifier_info(source: &str, name: &str) -> Option<String> {
    let (tokens, lex_errors) = turbo_lexer::tokenize(source);
    if !lex_errors.is_empty() {
        return None;
    }
    let (module, parse_errors) = turbo_parser::parse(tokens);
    if !parse_errors.is_empty() {
        return None;
    }

    for item in &module.items {
        match &item.node {
            turbo_ast::Item::Function(f) if f.name == name => {
                let params: Vec<String> = f
                    .params
                    .iter()
                    .map(|p| format!("{}: {}", p.name, format_type(&p.ty.node)))
                    .collect();
                let ret = f
                    .return_type
                    .as_ref()
                    .map(|t| format!(" -> {}", format_type(&t.node)))
                    .unwrap_or_default();
                let async_prefix = if f.is_async { "async " } else { "" };
                let tool_prefix = if f.is_tool { "tool " } else { "" };
                return Some(format!(
                    "{}{}fn {}({}){ret}",
                    tool_prefix,
                    async_prefix,
                    f.name,
                    params.join(", ")
                ));
            }
            turbo_ast::Item::Struct(s) if s.name == name => {
                let fields: Vec<String> = s
                    .fields
                    .iter()
                    .map(|fd| format!("    {}: {}", fd.name, format_type(&fd.ty.node)))
                    .collect();
                let type_params = if s.type_params.is_empty() {
                    String::new()
                } else {
                    let names: Vec<&str> =
                        s.type_params.iter().map(|tp| tp.name.as_str()).collect();
                    format!("<{}>", names.join(", "))
                };
                return Some(format!(
                    "struct {}{} {{\n{}\n}}",
                    s.name,
                    type_params,
                    fields.join(",\n")
                ));
            }
            turbo_ast::Item::Enum(e) if e.name == name => {
                let variants: Vec<String> = e
                    .variants
                    .iter()
                    .map(|v| {
                        if v.fields.is_empty() {
                            format!("    {}", v.name)
                        } else {
                            let tys: Vec<String> =
                                v.fields.iter().map(|f| format_type(&f.node)).collect();
                            format!("    {}({})", v.name, tys.join(", "))
                        }
                    })
                    .collect();
                let type_params = if e.type_params.is_empty() {
                    String::new()
                } else {
                    let names: Vec<&str> =
                        e.type_params.iter().map(|tp| tp.name.as_str()).collect();
                    format!("<{}>", names.join(", "))
                };
                return Some(format!(
                    "enum {}{} {{\n{}\n}}",
                    e.name,
                    type_params,
                    variants.join(",\n")
                ));
            }
            turbo_ast::Item::Agent(a) if a.name == name => {
                return Some(format!(
                    "agent {} {{ model: \"{}\", tools: [{}] }}",
                    a.name,
                    a.model,
                    a.tools.join(", ")
                ));
            }
            _ => {}
        }
    }

    None
}

/// Format a TypeExpr into a human-readable string.
fn format_type(ty: &turbo_ast::TypeExpr) -> String {
    match ty {
        turbo_ast::TypeExpr::Named(n) => n.clone(),
        turbo_ast::TypeExpr::Unit => "()".to_string(),
        turbo_ast::TypeExpr::Array(inner) => format!("[{}]", format_type(&inner.node)),
        turbo_ast::TypeExpr::FnType { params, ret } => {
            let ps: Vec<String> = params.iter().map(|p| format_type(&p.node)).collect();
            format!("fn({}) -> {}", ps.join(", "), format_type(&ret.node))
        }
        turbo_ast::TypeExpr::Result { ok_type, err_type } => {
            format!(
                "{} ! {}",
                format_type(&ok_type.node),
                format_type(&err_type.node)
            )
        }
        turbo_ast::TypeExpr::Optional(inner) => {
            format!("{}?", format_type(&inner.node))
        }
        turbo_ast::TypeExpr::Future(inner) => {
            format!("Future<{}>", format_type(&inner.node))
        }
        turbo_ast::TypeExpr::Inferred => "_".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Go to definition
// ---------------------------------------------------------------------------

fn compute_definition(source: &str, pos: Position, uri: &Uri) -> Option<Location> {
    let offset = position_to_offset(source, pos)?;
    let (tokens, lex_errors) = turbo_lexer::tokenize(source);
    if !lex_errors.is_empty() {
        return None;
    }

    // Find which identifier is at the cursor
    let target_name = tokens.iter().find_map(|tok| {
        if tok.span.contains(&offset) {
            if let turbo_lexer::Token::Ident(name) = &tok.value {
                return Some(name.clone());
            }
        }
        None
    })?;

    // Parse to find definitions
    let (module, parse_errors) = turbo_parser::parse(tokens);
    if !parse_errors.is_empty() {
        return None;
    }

    for item in &module.items {
        match &item.node {
            turbo_ast::Item::Function(f) if f.name == target_name => {
                return Some(Location {
                    uri: uri.clone(),
                    range: span_to_range(source, &item.span),
                });
            }
            turbo_ast::Item::Struct(s) if s.name == target_name => {
                return Some(Location {
                    uri: uri.clone(),
                    range: span_to_range(source, &item.span),
                });
            }
            turbo_ast::Item::Enum(e) if e.name == target_name => {
                return Some(Location {
                    uri: uri.clone(),
                    range: span_to_range(source, &item.span),
                });
            }
            turbo_ast::Item::Trait(t) if t.name == target_name => {
                return Some(Location {
                    uri: uri.clone(),
                    range: span_to_range(source, &item.span),
                });
            }
            turbo_ast::Item::Agent(a) if a.name == target_name => {
                return Some(Location {
                    uri: uri.clone(),
                    range: span_to_range(source, &item.span),
                });
            }
            _ => {}
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Coordinate helpers
// ---------------------------------------------------------------------------

fn span_to_range(source: &str, span: &std::ops::Range<usize>) -> Range {
    let start = offset_to_position(source, span.start);
    let end = offset_to_position(source, span.end);
    Range { start, end }
}

fn offset_to_position(source: &str, offset: usize) -> Position {
    let mut line = 0u32;
    let mut col = 0u32;
    for (i, c) in source.char_indices() {
        if i >= offset {
            break;
        }
        if c == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    Position {
        line,
        character: col,
    }
}

fn position_to_offset(source: &str, pos: Position) -> Option<usize> {
    let mut line = 0u32;
    let mut col = 0u32;
    for (i, c) in source.char_indices() {
        if line == pos.line && col == pos.character {
            return Some(i);
        }
        if c == '\n' {
            // If we're on the right line but past the character, return end of line
            if line == pos.line {
                return Some(i);
            }
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    // Handle position at end of file
    if line == pos.line {
        return Some(source.len());
    }
    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_offset_to_position_first_char() {
        let src = "fn main() {}";
        let pos = offset_to_position(src, 0);
        assert_eq!(pos.line, 0);
        assert_eq!(pos.character, 0);
    }

    #[test]
    fn test_offset_to_position_second_line() {
        let src = "line one\nline two";
        // 'l' of "line two" is at offset 9
        let pos = offset_to_position(src, 9);
        assert_eq!(pos.line, 1);
        assert_eq!(pos.character, 0);
    }

    #[test]
    fn test_position_to_offset_roundtrip() {
        let src = "fn main() {\n    print(\"hello\")\n}";
        for offset in 0..src.len() {
            let pos = offset_to_position(src, offset);
            if let Some(back) = position_to_offset(src, pos) {
                assert_eq!(back, offset, "roundtrip failed for offset {offset}");
            }
        }
    }

    #[test]
    fn test_span_to_range() {
        let src = "let x = 42";
        // "x" is at offset 4..5
        let range = span_to_range(src, &(4..5));
        assert_eq!(range.start.line, 0);
        assert_eq!(range.start.character, 4);
        assert_eq!(range.end.line, 0);
        assert_eq!(range.end.character, 5);
    }

    #[test]
    fn test_compute_hover_keyword() {
        let src = "fn main() {}";
        let hover = compute_hover(
            src,
            Position {
                line: 0,
                character: 0,
            },
        );
        assert!(hover.is_some());
        let content = match hover.unwrap().contents {
            HoverContents::Scalar(MarkedString::String(s)) => s,
            _ => panic!("unexpected hover contents"),
        };
        assert!(content.contains("fn"), "hover should mention fn keyword");
    }

    #[test]
    fn test_compute_hover_identifier() {
        let src = "fn main() {}";
        // "main" starts at character 3
        let hover = compute_hover(
            src,
            Position {
                line: 0,
                character: 3,
            },
        );
        assert!(hover.is_some());
        let content = match hover.unwrap().contents {
            HoverContents::Scalar(MarkedString::String(s)) => s,
            _ => panic!("unexpected hover contents"),
        };
        // Should show function signature from AST
        assert!(
            content.contains("fn main()"),
            "expected function signature in hover, got: {content}"
        );
    }

    #[test]
    fn test_compute_hover_integer() {
        let src = "let x = 42";
        // "42" starts at character 8
        let hover = compute_hover(
            src,
            Position {
                line: 0,
                character: 8,
            },
        );
        assert!(hover.is_some());
        let content = match hover.unwrap().contents {
            HoverContents::Scalar(MarkedString::String(s)) => s,
            _ => panic!("unexpected hover contents"),
        };
        assert!(content.contains("42"));
    }

    #[test]
    fn test_compute_hover_no_token() {
        let src = "fn main() {}";
        // Position way past end of line
        let hover = compute_hover(
            src,
            Position {
                line: 5,
                character: 0,
            },
        );
        assert!(hover.is_none());
    }

    #[test]
    fn test_compute_definition_function() {
        let src = "fn greet() {\n    print(\"hi\")\n}\n\nfn main() {\n    greet()\n}";
        let uri = "file:///test.tb".parse::<Uri>().unwrap();
        // "greet" in the call on line 5 (0-indexed), character 4
        let loc = compute_definition(
            src,
            Position {
                line: 5,
                character: 4,
            },
            &uri,
        );
        assert!(loc.is_some(), "should find definition of greet");
        let loc = loc.unwrap();
        // The definition should point to line 0 (where `fn greet` is)
        assert_eq!(loc.range.start.line, 0);
    }

    #[test]
    fn test_compute_definition_struct() {
        let src = "struct Point {\n    x: i32,\n    y: i32,\n}\n\nfn main() {\n    let p = Point { x: 1, y: 2 }\n}";
        let uri = "file:///test.tb".parse::<Uri>().unwrap();
        // "Point" in the struct literal on line 6
        let loc = compute_definition(
            src,
            Position {
                line: 6,
                character: 12,
            },
            &uri,
        );
        assert!(loc.is_some(), "should find definition of Point");
        let loc = loc.unwrap();
        assert_eq!(loc.range.start.line, 0);
    }

    #[test]
    fn test_format_type_named() {
        assert_eq!(
            format_type(&turbo_ast::TypeExpr::Named("i32".to_string())),
            "i32"
        );
    }

    #[test]
    fn test_format_type_optional() {
        let inner = turbo_ast::Spanned::new(turbo_ast::TypeExpr::Named("str".to_string()), 0..3);
        assert_eq!(
            format_type(&turbo_ast::TypeExpr::Optional(Box::new(inner))),
            "str?"
        );
    }

    #[test]
    fn test_format_type_array() {
        let inner = turbo_ast::Spanned::new(turbo_ast::TypeExpr::Named("i32".to_string()), 0..3);
        assert_eq!(
            format_type(&turbo_ast::TypeExpr::Array(Box::new(inner))),
            "[i32]"
        );
    }
}
