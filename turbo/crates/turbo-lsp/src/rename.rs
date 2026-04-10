//! `textDocument/rename` and `textDocument/prepareRename` handlers.
//!
//! This implements **intra-file** rename for top-level functions, structs,
//! enums, traits, and *local* identifiers (variables, fields, params)
//! that share a name with no other binding in the same file.
//!
//! Cross-file renames are P3 work. The strategy here is purposely simple:
//! 1. Lex the buffer.
//! 2. Find the identifier under the cursor.
//! 3. Replace every other occurrence of that identifier in the buffer
//!    with the new name.
//!
//! Rename is currently token-text based, so in files with shadowed names
//! (e.g. a local `x` shadowing a top-level `fn x`) it may over-rename — every
//! token spelled `x` will be rewritten regardless of its binding. Scope-aware
//! rename driven by sema binding spans is a P3 follow-up. For the common
//! refactor case ("rename this top-level function") this v1 strategy is
//! correct and matches what most LSP clients ship as a first-pass rename.

use std::collections::HashMap;

use lsp_types::{
    OneOf, Position, PrepareRenameResponse, Range, RenameParams, TextDocumentPositionParams,
    TextEdit, Uri, WorkspaceEdit,
};

use crate::{identifier_at_position, span_to_range};

/// Compute the prepare-rename response: returns the range of the identifier
/// under the cursor (so the editor can validate the name and pre-fill the
/// rename popup), or `None` when the cursor is not on an identifier.
pub(crate) fn compute_prepare_rename(
    source: &str,
    params: &TextDocumentPositionParams,
) -> Option<PrepareRenameResponse> {
    let pos = params.position;
    let name = identifier_at_position(source, pos)?;
    let range = identifier_range_at(source, pos, &name)?;
    Some(PrepareRenameResponse::RangeWithPlaceholder {
        range,
        placeholder: name,
    })
}

/// Compute the rename workspace edit. Returns `None` when the cursor is not
/// on an identifier or the new name is empty / equals the old name.
pub(crate) fn compute_rename(source: &str, params: &RenameParams) -> Option<WorkspaceEdit> {
    let pos = params.text_document_position.position;
    let new_name = params.new_name.trim();
    if new_name.is_empty() {
        return None;
    }
    if !is_valid_identifier(new_name) {
        return None;
    }

    let target = identifier_at_position(source, pos)?;
    if target == new_name {
        return None;
    }

    let uri = params.text_document_position.text_document.uri.clone();
    let edits = collect_rename_edits(source, &target, new_name);
    if edits.is_empty() {
        return None;
    }

    #[allow(clippy::mutable_key_type)] // Uri uses interior mutability for caching
    let mut changes: HashMap<Uri, Vec<TextEdit>> = HashMap::new();
    changes.insert(uri, edits);

    Some(WorkspaceEdit {
        changes: Some(changes),
        document_changes: None,
        change_annotations: None,
    })
}

fn collect_rename_edits(source: &str, target: &str, new_name: &str) -> Vec<TextEdit> {
    let (tokens, lex_errors) = turbo_lexer::tokenize(source);
    if !lex_errors.is_empty() {
        return Vec::new();
    }

    let mut edits = Vec::new();
    for tok in &tokens {
        if let turbo_lexer::Token::Ident(name) = &tok.value {
            if name == target {
                edits.push(TextEdit {
                    range: span_to_range(source, &tok.span),
                    new_text: new_name.to_string(),
                });
            }
        }
    }
    edits
}

fn identifier_range_at(source: &str, pos: Position, target: &str) -> Option<Range> {
    let offset = crate::position_to_offset(source, pos)?;
    let (tokens, lex_errors) = turbo_lexer::tokenize(source);
    if !lex_errors.is_empty() {
        return None;
    }
    for tok in &tokens {
        if tok.span.contains(&offset) {
            if let turbo_lexer::Token::Ident(name) = &tok.value {
                if name == target {
                    return Some(span_to_range(source, &tok.span));
                }
            }
        }
    }
    None
}

fn is_valid_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

// Tiny adapter so that the public capability registration in main.rs can
// flag rename as supported with a prepare-step.
pub(crate) fn rename_capability() -> Option<OneOf<bool, lsp_types::RenameOptions>> {
    Some(OneOf::Right(lsp_types::RenameOptions {
        prepare_provider: Some(true),
        work_done_progress_options: Default::default(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsp_types::TextDocumentIdentifier;

    fn rename_params(uri: &str, line: u32, character: u32, new_name: &str) -> RenameParams {
        RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: uri.parse::<Uri>().unwrap(),
                },
                position: Position { line, character },
            },
            new_name: new_name.to_string(),
            work_done_progress_params: Default::default(),
        }
    }

    #[test]
    fn rename_function_replaces_all_occurrences() {
        let src = "fn greet() {}\nfn main() {\n  greet()\n  greet()\n}";
        let params = rename_params("file:///t.tb", 0, 3, "hello");
        let edit = compute_rename(src, &params).expect("rename should produce an edit");
        // `lsp_types::Uri` has interior mutability via cached parts, so the
        // `HashMap<Uri, _>` trips clippy::mutable_key_type. The map is
        // produced by us and never mutated through its keys, so the lint is
        // a false positive here.
        #[allow(clippy::mutable_key_type)]
        let changes = edit.changes.unwrap();
        let edits = changes.values().next().unwrap();
        // 3 occurrences: definition + 2 call sites
        assert_eq!(edits.len(), 3);
        for e in edits {
            assert_eq!(e.new_text, "hello");
        }
    }

    #[test]
    fn rename_returns_none_for_empty_new_name() {
        let src = "fn greet() {}";
        let params = rename_params("file:///t.tb", 0, 3, "");
        assert!(compute_rename(src, &params).is_none());
    }

    #[test]
    fn rename_returns_none_when_new_name_equals_old() {
        let src = "fn greet() {}";
        let params = rename_params("file:///t.tb", 0, 3, "greet");
        assert!(compute_rename(src, &params).is_none());
    }

    #[test]
    fn rename_rejects_invalid_identifiers() {
        let src = "fn greet() {}";
        let params = rename_params("file:///t.tb", 0, 3, "1bad");
        assert!(compute_rename(src, &params).is_none());
        let params = rename_params("file:///t.tb", 0, 3, "has space");
        assert!(compute_rename(src, &params).is_none());
    }

    #[test]
    fn prepare_rename_returns_range_for_identifier() {
        let src = "fn greet() {}";
        let params = TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: "file:///t.tb".parse::<Uri>().unwrap(),
            },
            position: Position {
                line: 0,
                character: 3,
            },
        };
        let resp = compute_prepare_rename(src, &params).unwrap();
        match resp {
            PrepareRenameResponse::RangeWithPlaceholder { placeholder, .. } => {
                assert_eq!(placeholder, "greet");
            }
            _ => panic!("expected RangeWithPlaceholder"),
        }
    }

    #[test]
    fn prepare_rename_returns_none_off_identifier() {
        let src = "fn greet() {}";
        let params = TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: "file:///t.tb".parse::<Uri>().unwrap(),
            },
            // way past the end of the file
            position: Position {
                line: 5,
                character: 0,
            },
        };
        assert!(compute_prepare_rename(src, &params).is_none());
    }
}
