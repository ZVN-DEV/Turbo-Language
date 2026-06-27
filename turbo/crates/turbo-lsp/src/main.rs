//! Turbo Language Server (LSP) binary.
//!
//! Implements the editor-side half of the Turbo developer experience. The
//! server runs over stdio and consumes the same `lexer` → `parser` → `sema`
//! pipeline as the compiler, then translates compiler diagnostics and AST
//! information into LSP notifications and responses.
//!
//! Capabilities advertised on initialize:
//! * `textDocument/publishDiagnostics` (push, on every change)
//! * `textDocument/hover`
//! * `textDocument/definition`
//! * `textDocument/completion`
//! * `textDocument/references`
//! * `textDocument/documentSymbol`
//! * `textDocument/rename` + `textDocument/prepareRename`
//! * `textDocument/codeAction`
//! * `textDocument/formatting`
//! * `textDocument/semanticTokens/full`
//!
//! ```text
//! # Run from the workspace root:
//! cargo run -p turbo-lsp
//!
//! # Then point any LSP client at the resulting binary; the VS Code
//! # extension at editors/vscode/ does this out of the box.
//! ```

use lsp_server::{Connection, ErrorCode, Message, Notification, Request, Response};
use lsp_types::*;
use std::collections::{HashMap, HashSet};

/// JSON-RPC 2.0 standard error codes we surface to clients. Kept as local
/// constants so handler sites read as "return `invalid_params(...)`" rather
/// than sprinkling magic numbers.
const INVALID_PARAMS: i32 = ErrorCode::InvalidParams as i32;
const INTERNAL_ERROR: i32 = ErrorCode::InternalError as i32;

mod code_actions;
mod rename;
mod semantic_tokens;

fn main() {
    let (connection, io_threads) = Connection::stdio();

    let capabilities = ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        definition_provider: Some(OneOf::Left(true)),
        completion_provider: Some(CompletionOptions::default()),
        references_provider: Some(OneOf::Left(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
        rename_provider: rename::rename_capability(),
        code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
        document_formatting_provider: Some(OneOf::Left(true)),
        semantic_tokens_provider: Some(SemanticTokensServerCapabilities::SemanticTokensOptions(
            SemanticTokensOptions {
                work_done_progress_options: Default::default(),
                legend: semantic_tokens::legend(),
                range: Some(false),
                full: Some(SemanticTokensFullOptions::Bool(true)),
            },
        )),
        ..Default::default()
    };

    let init_result = match serde_json::to_value(capabilities) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("turbo-lsp: failed to serialize ServerCapabilities: {e}");
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
            Message::Request(req) => {
                if req.method == "shutdown" {
                    let _ = send_response(
                        &connection,
                        Response::new_ok(req.id, serde_json::Value::Null),
                    );
                    break;
                }
                let response = dispatch_request(req, &documents);
                let _ = send_response(&connection, response);
            }
            _ => {}
        }
    }

    drop(connection);
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
// Request dispatch
//
// Every handler below is fallible: on malformed client input it returns a
// proper JSON-RPC `Response::new_err` with the standard `InvalidParams` code
// (-32602), and on an internal bug it returns `InternalError` (-32603). The
// server must stay alive regardless of what a misbehaving editor sends, so
// panics and unwraps on client-derived data are forbidden in this section.
// ---------------------------------------------------------------------------

#[allow(clippy::mutable_key_type)] // Uri uses interior mutability for caching
fn dispatch_request(req: Request, documents: &HashMap<Uri, String>) -> Response {
    let id = req.id.clone();
    let result: Result<serde_json::Value, (i32, String)> = match req.method.as_str() {
        "textDocument/hover" => handle_hover(req.params, documents),
        "textDocument/definition" => handle_definition(req.params, documents),
        "textDocument/completion" => handle_completion(req.params, documents),
        "textDocument/references" => handle_references(req.params, documents),
        "textDocument/documentSymbol" => handle_document_symbol(req.params, documents),
        "textDocument/prepareRename" => handle_prepare_rename(req.params, documents),
        "textDocument/rename" => handle_rename(req.params, documents),
        "textDocument/codeAction" => handle_code_action(req.params, documents),
        "textDocument/formatting" => handle_formatting(req.params, documents),
        "textDocument/semanticTokens/full" => handle_semantic_tokens(req.params, documents),
        // Unknown methods get an empty success response: lsp-server already
        // logs them, and some clients probe for optional capabilities this way.
        _ => Ok(serde_json::Value::Null),
    };

    match result {
        Ok(value) => Response::new_ok(id, value),
        Err((code, message)) => {
            eprintln!("turbo-lsp: {} ({code}): {message}", req.method);
            Response::new_err(id, code, message)
        }
    }
}

/// Wrap a serde_json deserialize error into an `InvalidParams` response.
fn invalid_params(method: &str, err: impl std::fmt::Display) -> (i32, String) {
    (INVALID_PARAMS, format!("invalid {method} params: {err}"))
}

/// Wrap a serde_json serialize error into an `InternalError` response. This
/// should effectively never fire (we only serialize types we control), but
/// panicking here would still kill the server.
fn serialize_error(err: impl std::fmt::Display) -> (i32, String) {
    (
        INTERNAL_ERROR,
        format!("failed to serialize response: {err}"),
    )
}

/// Validate that a position lies inside the given document. Returns an
/// `InvalidParams` error for positions past the end of file so a malformed
/// rename request surfaces as a proper LSP error instead of a silent null.
fn validate_position(source: &str, pos: Position, method: &str) -> Result<(), (i32, String)> {
    if position_to_offset(source, pos).is_none() {
        return Err((
            INVALID_PARAMS,
            format!(
                "{method}: position {line}:{char} is past end of document",
                line = pos.line,
                char = pos.character
            ),
        ));
    }
    Ok(())
}

/// Validate that an LSP range is well-formed (start <= end). The LSP spec
/// allows an empty range (start == end) to mean "cursor", so we only reject
/// ranges where the end precedes the start.
fn validate_range(range: Range, method: &str) -> Result<(), (i32, String)> {
    let start = (range.start.line, range.start.character);
    let end = (range.end.line, range.end.character);
    if end < start {
        return Err((
            INVALID_PARAMS,
            format!("{method}: range end precedes range start"),
        ));
    }
    Ok(())
}

#[allow(clippy::mutable_key_type)] // Uri uses interior mutability for caching
fn handle_hover(
    params: serde_json::Value,
    documents: &HashMap<Uri, String>,
) -> Result<serde_json::Value, (i32, String)> {
    let params: HoverParams =
        serde_json::from_value(params).map_err(|e| invalid_params("textDocument/hover", e))?;
    let uri = &params.text_document_position_params.text_document.uri;
    let pos = params.text_document_position_params.position;
    let hover = match documents.get(uri) {
        Some(text) => compute_hover(text, pos),
        None => None,
    };
    serde_json::to_value(hover).map_err(serialize_error)
}

#[allow(clippy::mutable_key_type)] // Uri uses interior mutability for caching
fn handle_definition(
    params: serde_json::Value,
    documents: &HashMap<Uri, String>,
) -> Result<serde_json::Value, (i32, String)> {
    let params: GotoDefinitionParams =
        serde_json::from_value(params).map_err(|e| invalid_params("textDocument/definition", e))?;
    let uri = &params.text_document_position_params.text_document.uri;
    let pos = params.text_document_position_params.position;
    let location = match documents.get(uri) {
        Some(text) => compute_definition(text, pos, uri),
        None => None,
    };
    serde_json::to_value(location.map(GotoDefinitionResponse::Scalar)).map_err(serialize_error)
}

#[allow(clippy::mutable_key_type)] // Uri uses interior mutability for caching
fn handle_completion(
    params: serde_json::Value,
    documents: &HashMap<Uri, String>,
) -> Result<serde_json::Value, (i32, String)> {
    let params: CompletionParams =
        serde_json::from_value(params).map_err(|e| invalid_params("textDocument/completion", e))?;
    let uri = &params.text_document_position.text_document.uri;
    let pos = params.text_document_position.position;
    let result = documents
        .get(uri)
        .map(|text| CompletionResponse::Array(compute_completion_items(text, pos)));
    serde_json::to_value(result).map_err(serialize_error)
}

#[allow(clippy::mutable_key_type)] // Uri uses interior mutability for caching
fn handle_references(
    params: serde_json::Value,
    documents: &HashMap<Uri, String>,
) -> Result<serde_json::Value, (i32, String)> {
    let params: ReferenceParams =
        serde_json::from_value(params).map_err(|e| invalid_params("textDocument/references", e))?;
    let uri = &params.text_document_position.text_document.uri;
    let pos = params.text_document_position.position;
    let refs = documents
        .get(uri)
        .map(|text| compute_references(text, pos, uri, params.context.include_declaration));
    serde_json::to_value(refs).map_err(serialize_error)
}

#[allow(clippy::mutable_key_type)] // Uri uses interior mutability for caching
fn handle_document_symbol(
    params: serde_json::Value,
    documents: &HashMap<Uri, String>,
) -> Result<serde_json::Value, (i32, String)> {
    let params: DocumentSymbolParams = serde_json::from_value(params)
        .map_err(|e| invalid_params("textDocument/documentSymbol", e))?;
    let uri = &params.text_document.uri;
    let result = documents
        .get(uri)
        .map(|text| DocumentSymbolResponse::Nested(compute_document_symbols(text)));
    serde_json::to_value(result).map_err(serialize_error)
}

#[allow(clippy::mutable_key_type)] // Uri uses interior mutability for caching
fn handle_prepare_rename(
    params: serde_json::Value,
    documents: &HashMap<Uri, String>,
) -> Result<serde_json::Value, (i32, String)> {
    let params: TextDocumentPositionParams = serde_json::from_value(params)
        .map_err(|e| invalid_params("textDocument/prepareRename", e))?;
    let uri = &params.text_document.uri;
    if let Some(text) = documents.get(uri) {
        validate_position(text, params.position, "textDocument/prepareRename")?;
    }
    let resp = documents
        .get(uri)
        .and_then(|text| rename::compute_prepare_rename(text, &params));
    serde_json::to_value(resp).map_err(serialize_error)
}

#[allow(clippy::mutable_key_type)] // Uri uses interior mutability for caching
fn handle_rename(
    params: serde_json::Value,
    documents: &HashMap<Uri, String>,
) -> Result<serde_json::Value, (i32, String)> {
    let params: RenameParams =
        serde_json::from_value(params).map_err(|e| invalid_params("textDocument/rename", e))?;
    let uri = &params.text_document_position.text_document.uri;
    if let Some(text) = documents.get(uri) {
        validate_position(
            text,
            params.text_document_position.position,
            "textDocument/rename",
        )?;
    }
    let edit = documents
        .get(uri)
        .and_then(|text| rename::compute_rename(text, &params));
    serde_json::to_value(edit).map_err(serialize_error)
}

#[allow(clippy::mutable_key_type)] // Uri uses interior mutability for caching
fn handle_code_action(
    params: serde_json::Value,
    _documents: &HashMap<Uri, String>,
) -> Result<serde_json::Value, (i32, String)> {
    let params: CodeActionParams =
        serde_json::from_value(params).map_err(|e| invalid_params("textDocument/codeAction", e))?;
    validate_range(params.range, "textDocument/codeAction")?;
    let actions = code_actions::compute_code_actions(&params);
    serde_json::to_value(actions).map_err(serialize_error)
}

#[allow(clippy::mutable_key_type)] // Uri uses interior mutability for caching
fn handle_formatting(
    params: serde_json::Value,
    documents: &HashMap<Uri, String>,
) -> Result<serde_json::Value, (i32, String)> {
    let params: DocumentFormattingParams =
        serde_json::from_value(params).map_err(|e| invalid_params("textDocument/formatting", e))?;
    let uri = &params.text_document.uri;
    let edits = documents
        .get(uri)
        .and_then(|source| compute_formatting_edits(source));
    serde_json::to_value(edits).map_err(serialize_error)
}

fn compute_formatting_edits(source: &str) -> Option<Vec<TextEdit>> {
    if has_syntax_errors(source) {
        return None;
    }

    let formatted = turbo_formatter::format_source(source);
    if formatted == source {
        return Some(Vec::new());
    }

    Some(vec![TextEdit {
        range: full_document_range(source),
        new_text: formatted,
    }])
}

fn has_syntax_errors(source: &str) -> bool {
    let (tokens, lex_errors) = turbo_lexer::tokenize(source);
    if !lex_errors.is_empty() {
        return true;
    }
    let (_, parse_errors) = turbo_parser::parse(tokens);
    !parse_errors.is_empty()
}

fn full_document_range(source: &str) -> Range {
    Range {
        start: Position {
            line: 0,
            character: 0,
        },
        end: offset_to_position(source, source.len()),
    }
}

#[allow(clippy::mutable_key_type)] // Uri uses interior mutability for caching
fn handle_semantic_tokens(
    params: serde_json::Value,
    documents: &HashMap<Uri, String>,
) -> Result<serde_json::Value, (i32, String)> {
    let params: SemanticTokensParams = serde_json::from_value(params)
        .map_err(|e| invalid_params("textDocument/semanticTokens/full", e))?;
    let uri = &params.text_document.uri;
    let tokens = documents
        .get(uri)
        .map(|text| semantic_tokens::compute_semantic_tokens(text))
        .map(SemanticTokensResult::Tokens);
    serde_json::to_value(tokens).map_err(serialize_error)
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
                code: Some(NumberOrString::String(err.code.as_str().to_string())),
                message: err.message.clone(),
                ..Default::default()
            });
        }

        if parse_errors.is_empty() {
            let sema_result = turbo_sema::check(&module);
            for err in &sema_result.errors {
                diagnostics.push(Diagnostic {
                    range: span_to_range(source, &err.span),
                    severity: Some(DiagnosticSeverity::ERROR),
                    code: Some(NumberOrString::String(err.code.as_str().to_string())),
                    message: err.message.clone(),
                    ..Default::default()
                });
            }
            for w in &sema_result.warnings {
                diagnostics.push(Diagnostic {
                    range: span_to_range(source, &w.span),
                    severity: Some(DiagnosticSeverity::WARNING),
                    code: Some(NumberOrString::String(w.code.as_str().to_string())),
                    message: w.message.clone(),
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
                let declaration_prefix = if f.is_async {
                    "async fn ".to_string()
                } else {
                    "fn ".to_string()
                };
                return Some(format!(
                    "{}{}({}){ret}",
                    declaration_prefix,
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
            _ => {}
        }
    }

    None
}

fn identifier_at_position(source: &str, pos: Position) -> Option<String> {
    let offset = position_to_offset(source, pos)?;
    let (tokens, lex_errors) = turbo_lexer::tokenize(source);
    if !lex_errors.is_empty() {
        return None;
    }
    tokens.iter().find_map(|tok| {
        if tok.span.contains(&offset) {
            if let turbo_lexer::Token::Ident(name) = &tok.value {
                return Some(name.clone());
            }
        }
        None
    })
}

fn compute_references(
    source: &str,
    pos: Position,
    uri: &Uri,
    include_declaration: bool,
) -> Vec<Location> {
    let Some(target_name) = identifier_at_position(source, pos) else {
        return Vec::new();
    };

    let declaration_span = find_top_level_declaration_span(source, &target_name);
    let (tokens, lex_errors) = turbo_lexer::tokenize(source);
    if !lex_errors.is_empty() {
        return Vec::new();
    }

    let mut refs = Vec::new();
    for tok in &tokens {
        if let turbo_lexer::Token::Ident(name) = &tok.value {
            if name == &target_name {
                let is_decl = declaration_span.as_ref() == Some(&tok.span);
                if is_decl && !include_declaration {
                    continue;
                }
                refs.push(Location {
                    uri: uri.clone(),
                    range: span_to_range(source, &tok.span),
                });
            }
        }
    }
    refs
}

fn compute_completion_items(source: &str, pos: Position) -> Vec<CompletionItem> {
    let mut seen = HashSet::new();
    let prefix = completion_prefix(source, pos).unwrap_or_default();
    let mut items = Vec::new();

    let keywords = [
        ("fn", CompletionItemKind::KEYWORD),
        ("let", CompletionItemKind::KEYWORD),
        ("mut", CompletionItemKind::KEYWORD),
        ("const", CompletionItemKind::KEYWORD),
        ("if", CompletionItemKind::KEYWORD),
        ("else", CompletionItemKind::KEYWORD),
        ("while", CompletionItemKind::KEYWORD),
        ("for", CompletionItemKind::KEYWORD),
        ("in", CompletionItemKind::KEYWORD),
        ("return", CompletionItemKind::KEYWORD),
        ("match", CompletionItemKind::KEYWORD),
        ("struct", CompletionItemKind::KEYWORD),
        ("type", CompletionItemKind::KEYWORD),
        ("trait", CompletionItemKind::KEYWORD),
        ("impl", CompletionItemKind::KEYWORD),
        ("async", CompletionItemKind::KEYWORD),
        ("await", CompletionItemKind::KEYWORD),
        ("spawn", CompletionItemKind::KEYWORD),
        ("defer", CompletionItemKind::KEYWORD),
        ("import", CompletionItemKind::KEYWORD),
        ("true", CompletionItemKind::VALUE),
        ("false", CompletionItemKind::VALUE),
        ("none", CompletionItemKind::VALUE),
        ("some", CompletionItemKind::VALUE),
        ("ok", CompletionItemKind::VALUE),
        ("err", CompletionItemKind::VALUE),
    ];

    for (label, kind) in keywords {
        push_completion_item(&mut items, &mut seen, &prefix, label, kind, None);
    }

    let (tokens, lex_errors) = turbo_lexer::tokenize(source);
    if lex_errors.is_empty() {
        let (module, parse_errors) = turbo_parser::parse(tokens);
        if parse_errors.is_empty() {
            for item in &module.items {
                match &item.node {
                    turbo_ast::Item::Function(f) => push_completion_item(
                        &mut items,
                        &mut seen,
                        &prefix,
                        &f.name,
                        CompletionItemKind::FUNCTION,
                        Some("function"),
                    ),
                    turbo_ast::Item::Struct(s) => push_completion_item(
                        &mut items,
                        &mut seen,
                        &prefix,
                        &s.name,
                        CompletionItemKind::STRUCT,
                        Some("struct"),
                    ),
                    turbo_ast::Item::Enum(e) => push_completion_item(
                        &mut items,
                        &mut seen,
                        &prefix,
                        &e.name,
                        CompletionItemKind::ENUM,
                        Some("enum"),
                    ),
                    turbo_ast::Item::Trait(t) => push_completion_item(
                        &mut items,
                        &mut seen,
                        &prefix,
                        &t.name,
                        CompletionItemKind::INTERFACE,
                        Some("trait"),
                    ),
                    _ => {}
                }
            }
        }
    }

    items
}

fn push_completion_item(
    items: &mut Vec<CompletionItem>,
    seen: &mut HashSet<String>,
    prefix: &str,
    label: &str,
    kind: CompletionItemKind,
    detail: Option<&str>,
) {
    if !prefix.is_empty() && !label.starts_with(prefix) {
        return;
    }
    if !seen.insert(label.to_string()) {
        return;
    }
    items.push(CompletionItem {
        label: label.to_string(),
        kind: Some(kind),
        detail: detail.map(|s| s.to_string()),
        ..Default::default()
    });
}

fn completion_prefix(source: &str, pos: Position) -> Option<String> {
    let offset = position_to_offset(source, pos)?;
    let prefix_chars: String = source[..offset]
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    Some(prefix_chars.chars().rev().collect())
}

#[allow(deprecated)]
fn compute_document_symbols(source: &str) -> Vec<DocumentSymbol> {
    let (tokens, lex_errors) = turbo_lexer::tokenize(source);
    if !lex_errors.is_empty() {
        return Vec::new();
    }
    let (module, parse_errors) = turbo_parser::parse(tokens);
    if !parse_errors.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    for item in &module.items {
        match &item.node {
            turbo_ast::Item::Function(f) => out.push(DocumentSymbol {
                name: f.name.clone(),
                detail: Some(if f.is_async {
                    "async fn".to_string()
                } else {
                    "fn".to_string()
                }),
                kind: SymbolKind::FUNCTION,
                tags: None,
                deprecated: None,
                range: span_to_range(source, &item.span),
                selection_range: span_to_range(source, &item.span),
                children: None,
            }),
            turbo_ast::Item::Struct(s) => {
                let children = s
                    .fields
                    .iter()
                    .map(|field| DocumentSymbol {
                        name: field.name.clone(),
                        detail: Some(format_type(&field.ty.node)),
                        kind: SymbolKind::FIELD,
                        tags: None,
                        deprecated: None,
                        range: span_to_range(source, &field.ty.span),
                        selection_range: span_to_range(source, &field.ty.span),
                        children: None,
                    })
                    .collect();
                out.push(DocumentSymbol {
                    name: s.name.clone(),
                    detail: Some("struct".to_string()),
                    kind: SymbolKind::STRUCT,
                    tags: None,
                    deprecated: None,
                    range: span_to_range(source, &item.span),
                    selection_range: span_to_range(source, &item.span),
                    children: Some(children),
                });
            }
            turbo_ast::Item::Enum(e) => {
                let children = e
                    .variants
                    .iter()
                    .map(|variant| DocumentSymbol {
                        name: variant.name.clone(),
                        detail: Some("variant".to_string()),
                        kind: SymbolKind::ENUM_MEMBER,
                        tags: None,
                        deprecated: None,
                        range: span_to_range(source, &item.span),
                        selection_range: span_to_range(source, &item.span),
                        children: None,
                    })
                    .collect();
                out.push(DocumentSymbol {
                    name: e.name.clone(),
                    detail: Some("enum".to_string()),
                    kind: SymbolKind::ENUM,
                    tags: None,
                    deprecated: None,
                    range: span_to_range(source, &item.span),
                    selection_range: span_to_range(source, &item.span),
                    children: Some(children),
                });
            }
            turbo_ast::Item::Trait(t) => out.push(DocumentSymbol {
                name: t.name.clone(),
                detail: Some("trait".to_string()),
                kind: SymbolKind::INTERFACE,
                tags: None,
                deprecated: None,
                range: span_to_range(source, &item.span),
                selection_range: span_to_range(source, &item.span),
                children: None,
            }),
            _ => {}
        }
    }

    out
}

fn find_top_level_declaration_span(source: &str, name: &str) -> Option<std::ops::Range<usize>> {
    let (tokens, lex_errors) = turbo_lexer::tokenize(source);
    if !lex_errors.is_empty() {
        return None;
    }
    let (module, parse_errors) = turbo_parser::parse(tokens.clone());
    if !parse_errors.is_empty() {
        return None;
    }

    for item in &module.items {
        let matches = match &item.node {
            turbo_ast::Item::Function(f) => f.name == name,
            turbo_ast::Item::Struct(s) => s.name == name,
            turbo_ast::Item::Enum(e) => e.name == name,
            turbo_ast::Item::Trait(t) => t.name == name,
            _ => false,
        };
        if matches {
            for tok in &tokens {
                if tok.span.start < item.span.start || tok.span.end > item.span.end {
                    continue;
                }
                if let turbo_lexer::Token::Ident(found) = &tok.value {
                    if found == name {
                        return Some(tok.span.clone());
                    }
                }
            }
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
            col += c.len_utf16() as u32;
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
            let next_col = col + c.len_utf16() as u32;
            if line == pos.line && pos.character < next_col {
                return Some(i);
            }
            col = next_col;
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
    fn unicode_positions_use_lsp_utf16_units() {
        let src = "let icon = \"🙂\"\nlet x = 1";
        let emoji_offset = src.find('🙂').unwrap();
        let after_emoji = emoji_offset + "🙂".len();

        let emoji_pos = offset_to_position(src, emoji_offset);
        assert_eq!(emoji_pos.line, 0);
        assert_eq!(emoji_pos.character, 12);

        let after_emoji_pos = offset_to_position(src, after_emoji);
        assert_eq!(after_emoji_pos.line, 0);
        assert_eq!(after_emoji_pos.character, 14);
        assert_eq!(position_to_offset(src, after_emoji_pos), Some(after_emoji));
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
    fn test_compute_references_function() {
        let src = "fn greet() {}\nfn main() {\n  greet()\n  greet()\n}";
        let uri = "file:///test.tb".parse::<Uri>().unwrap();
        let refs = compute_references(
            src,
            Position {
                line: 2,
                character: 2,
            },
            &uri,
            false,
        );
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].range.start.line, 2);
        assert_eq!(refs[1].range.start.line, 3);
    }

    #[test]
    fn test_compute_completion_items_keyword_prefix() {
        let src = "fn main() {\n  sp\n}";
        let items = compute_completion_items(
            src,
            Position {
                line: 1,
                character: 4,
            },
        );
        assert!(items.iter().any(|item| item.label == "spawn"));
    }

    #[test]
    fn test_compute_document_symbols() {
        let src = "struct Point { x: i32, y: i32 }\nfn main() {}";
        let symbols = compute_document_symbols(src);
        assert_eq!(symbols.len(), 2);
        assert_eq!(symbols[0].name, "Point");
        assert_eq!(symbols[1].name, "main");
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

    // -----------------------------------------------------------------------
    // Robustness tests: malformed client input must never crash the server.
    // These drive the request dispatch layer with adversarial JSON to make
    // sure every public LSP method returns a JSON-RPC error response
    // (InvalidParams / InternalError) rather than panicking the event loop.
    // -----------------------------------------------------------------------

    use serde_json::json;

    // `lsp_types::Uri` has interior mutability (cached parts), so any
    // `HashMap<Uri, _>` trips `clippy::mutable_key_type`. The maps here are
    // built by the tests and never mutated through their keys — it is a
    // false positive in every one of these fixtures.
    #[allow(clippy::mutable_key_type)]
    fn empty_docs() -> HashMap<Uri, String> {
        HashMap::new()
    }

    #[allow(clippy::mutable_key_type)]
    fn docs_with(uri: &str, text: &str) -> HashMap<Uri, String> {
        let mut docs = HashMap::new();
        docs.insert(uri.parse::<Uri>().unwrap(), text.to_string());
        docs
    }

    fn make_request(method: &str, params: serde_json::Value) -> Request {
        Request {
            id: RequestId::from(1),
            method: method.to_string(),
            params,
        }
    }

    // A RequestId constructor is only reachable via the trait impl, so
    // re-export it here instead of at the top of the file (it is unused
    // outside of tests).
    use lsp_server::RequestId;

    #[test]
    fn rename_missing_position_field_returns_invalid_params() {
        // `textDocument/rename` requires a `position` field. Drop it.
        let bad = json!({
            "textDocument": { "uri": "file:///t.tb" },
            "newName": "renamed",
        });
        let resp = dispatch_request(make_request("textDocument/rename", bad), &empty_docs());
        let err = resp
            .error
            .expect("expected JSON-RPC error for missing position");
        assert_eq!(err.code, INVALID_PARAMS);
    }

    #[test]
    #[allow(clippy::mutable_key_type)]
    fn rename_position_past_eof_returns_invalid_params() {
        let docs = docs_with("file:///t.tb", "fn main() {}");
        let params = json!({
            "textDocument": { "uri": "file:///t.tb" },
            "position": { "line": 9999, "character": 9999 },
            "newName": "renamed",
        });
        let resp = dispatch_request(make_request("textDocument/rename", params), &docs);
        let err = resp
            .error
            .expect("expected error for out-of-range position");
        assert_eq!(err.code, INVALID_PARAMS);
        assert!(err.message.contains("past end"));
    }

    #[test]
    #[allow(clippy::mutable_key_type)]
    fn prepare_rename_position_past_eof_returns_invalid_params() {
        let docs = docs_with("file:///t.tb", "fn main() {}");
        let params = json!({
            "textDocument": { "uri": "file:///t.tb" },
            "position": { "line": 9999, "character": 9999 },
        });
        let resp = dispatch_request(make_request("textDocument/prepareRename", params), &docs);
        let err = resp
            .error
            .expect("expected error for out-of-range prepareRename");
        assert_eq!(err.code, INVALID_PARAMS);
    }

    #[test]
    fn code_action_inverted_range_returns_invalid_params() {
        // Range where `end` precedes `start` — an empty range (start == end)
        // is legal per the LSP spec, so we only reject truly malformed ones.
        let params = json!({
            "textDocument": { "uri": "file:///t.tb" },
            "range": {
                "start": { "line": 5, "character": 10 },
                "end":   { "line": 1, "character": 0 },
            },
            "context": { "diagnostics": [] },
        });
        let resp = dispatch_request(
            make_request("textDocument/codeAction", params),
            &empty_docs(),
        );
        let err = resp.error.expect("expected error for inverted range");
        assert_eq!(err.code, INVALID_PARAMS);
    }

    #[test]
    fn code_action_empty_range_is_ok() {
        // An empty (zero-width) range is a valid LSP "cursor position" signal.
        // It must produce a success response, not an error.
        let params = json!({
            "textDocument": { "uri": "file:///t.tb" },
            "range": {
                "start": { "line": 0, "character": 0 },
                "end":   { "line": 0, "character": 0 },
            },
            "context": { "diagnostics": [] },
        });
        let resp = dispatch_request(
            make_request("textDocument/codeAction", params),
            &empty_docs(),
        );
        assert!(resp.error.is_none(), "empty range should be accepted");
    }

    #[test]
    #[allow(clippy::mutable_key_type)]
    fn formatting_returns_whole_document_edit_for_unformatted_source() {
        let docs = docs_with("file:///t.tb", "fn main(){\nlet x=1\n}\n");
        let params = json!({
            "textDocument": { "uri": "file:///t.tb" },
            "options": { "tabSize": 4, "insertSpaces": true },
        });
        let resp = dispatch_request(make_request("textDocument/formatting", params), &docs);
        assert!(resp.error.is_none(), "formatting should succeed");
        let edits: Option<Vec<TextEdit>> =
            serde_json::from_value(resp.result.expect("expected formatting result")).unwrap();
        let edits = edits.expect("open document should return formatting edits");
        assert_eq!(edits.len(), 1, "unformatted source should get one edit");
        assert_eq!(edits[0].range.start.line, 0);
        assert_eq!(edits[0].range.start.character, 0);
        assert_eq!(edits[0].range.end.line, 3);
        assert_eq!(edits[0].range.end.character, 0);
        assert!(
            edits[0].new_text.contains("fn main() {"),
            "formatter should normalize function spacing: {:?}",
            edits[0].new_text
        );
        assert!(
            edits[0].new_text.contains("    let x = 1"),
            "formatter should normalize indentation and assignment spacing: {:?}",
            edits[0].new_text
        );
    }

    #[test]
    #[allow(clippy::mutable_key_type)]
    fn formatting_range_covers_non_bmp_final_line_without_trailing_newline() {
        let source = "fn main(){print(\"🙂\")}";
        let docs = docs_with("file:///emoji.tb", source);
        let params = json!({
            "textDocument": { "uri": "file:///emoji.tb" },
            "options": { "tabSize": 4, "insertSpaces": true },
        });
        let resp = dispatch_request(make_request("textDocument/formatting", params), &docs);
        assert!(resp.error.is_none(), "formatting should succeed");
        let edits: Option<Vec<TextEdit>> =
            serde_json::from_value(resp.result.expect("expected formatting result")).unwrap();
        let edits = edits.expect("open document should return formatting edits");
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].range, full_document_range(source));
        assert_eq!(edits[0].range.end.line, 0);
        assert_eq!(edits[0].range.end.character, 22);
        assert!(
            edits[0].new_text.ends_with('\n'),
            "formatter should still normalize trailing newline"
        );
    }

    #[test]
    #[allow(clippy::mutable_key_type)]
    fn formatting_returns_empty_edits_for_formatted_source() {
        let docs = docs_with("file:///t.tb", "fn main() {\n    let x = 1\n}\n");
        let params = json!({
            "textDocument": { "uri": "file:///t.tb" },
            "options": { "tabSize": 4, "insertSpaces": true },
        });
        let resp = dispatch_request(make_request("textDocument/formatting", params), &docs);
        assert!(resp.error.is_none(), "formatting should succeed");
        let edits: Option<Vec<TextEdit>> =
            serde_json::from_value(resp.result.expect("expected formatting result")).unwrap();
        assert_eq!(edits.expect("open document should return edits").len(), 0);
    }

    #[test]
    #[allow(clippy::mutable_key_type)]
    fn formatting_leaves_syntax_errors_untouched() {
        let docs = docs_with("file:///bad.tb", "fn main( {\n");
        let params = json!({
            "textDocument": { "uri": "file:///bad.tb" },
            "options": { "tabSize": 4, "insertSpaces": true },
        });
        let resp = dispatch_request(make_request("textDocument/formatting", params), &docs);
        assert!(
            resp.error.is_none(),
            "syntax errors should not make formatting fail"
        );
        assert_eq!(resp.result, Some(serde_json::Value::Null));
    }

    #[test]
    #[allow(clippy::mutable_key_type)]
    fn formatting_returns_null_for_unopened_document() {
        let params = json!({
            "textDocument": { "uri": "file:///missing.tb" },
            "options": { "tabSize": 4, "insertSpaces": true },
        });
        let resp = dispatch_request(
            make_request("textDocument/formatting", params),
            &empty_docs(),
        );
        assert!(resp.error.is_none(), "missing documents should not error");
        assert_eq!(resp.result, Some(serde_json::Value::Null));
    }

    #[test]
    #[allow(clippy::mutable_key_type)]
    fn formatting_leaves_lex_errors_untouched() {
        let docs = docs_with("file:///bad.tb", "fn main() {\n    §\n}\n");
        let params = json!({
            "textDocument": { "uri": "file:///bad.tb" },
            "options": { "tabSize": 4, "insertSpaces": true },
        });
        let resp = dispatch_request(make_request("textDocument/formatting", params), &docs);
        assert!(
            resp.error.is_none(),
            "lex errors should not make formatting fail"
        );
        assert_eq!(resp.result, Some(serde_json::Value::Null));
    }

    #[test]
    fn full_document_range_uses_utf16_end_character() {
        let range = full_document_range("fn main() { print(\"🙂\") }");
        assert_eq!(range.end.line, 0);
        assert_eq!(range.end.character, 25);
    }

    #[test]
    fn formatting_with_malformed_params_returns_invalid_params() {
        let resp = dispatch_request(
            make_request("textDocument/formatting", json!({"bogus": true})),
            &empty_docs(),
        );
        let err = resp
            .error
            .expect("expected error for malformed formatting params");
        assert_eq!(err.code, INVALID_PARAMS);
    }

    #[test]
    #[allow(clippy::mutable_key_type)]
    fn hover_on_whitespace_returns_null_not_error() {
        // Per the LSP spec hovering on a non-token location should succeed
        // (usually with `null`), never panic. The lexer treats newline as a
        // token so the exact payload depends on cursor placement, but the
        // contract we care about here is "no error response".
        let docs = docs_with("file:///t.tb", "fn main() {}   \n");
        let params = json!({
            "textDocument": { "uri": "file:///t.tb" },
            // Sitting on the trailing spaces (char 13) — no token covers
            // that offset.
            "position": { "line": 0, "character": 13 },
        });
        let resp = dispatch_request(make_request("textDocument/hover", params), &docs);
        assert!(resp.error.is_none(), "hover on whitespace must succeed");
        assert_eq!(resp.result, Some(serde_json::Value::Null));
    }

    #[test]
    fn hover_with_malformed_params_returns_invalid_params() {
        let resp = dispatch_request(
            make_request("textDocument/hover", json!({"bogus": true})),
            &empty_docs(),
        );
        let err = resp
            .error
            .expect("expected error for malformed hover params");
        assert_eq!(err.code, INVALID_PARAMS);
    }

    #[test]
    fn unknown_method_is_ignored_not_crashed() {
        // Unknown request methods must not crash or error — lsp-server's own
        // fallback is a null success response, and we match that behaviour
        // so that client capability probes work.
        let resp = dispatch_request(
            make_request("textDocument/nonexistent", json!({})),
            &empty_docs(),
        );
        assert!(resp.error.is_none());
        assert_eq!(resp.result, Some(serde_json::Value::Null));
    }

    #[test]
    fn every_request_method_handles_malformed_params() {
        // Smoke-test: send `null` as params to every method we implement
        // and assert the dispatcher never panics and always returns an
        // InvalidParams response (except `shutdown`, which is short-circuited
        // by the main loop before dispatch_request).
        let methods = [
            "textDocument/hover",
            "textDocument/definition",
            "textDocument/completion",
            "textDocument/references",
            "textDocument/documentSymbol",
            "textDocument/prepareRename",
            "textDocument/rename",
            "textDocument/codeAction",
            "textDocument/formatting",
            "textDocument/semanticTokens/full",
        ];
        for method in methods {
            let resp = dispatch_request(make_request(method, json!(null)), &empty_docs());
            let err = resp
                .error
                .unwrap_or_else(|| panic!("{method}: expected error for null params"));
            assert_eq!(err.code, INVALID_PARAMS, "{method}: wrong error code");
        }
    }
}
