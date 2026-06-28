//! `textDocument/rename` and `textDocument/prepareRename` handlers.
//!
//! This implements **intra-file**, scope-aware rename for top-level functions,
//! structs, enums, traits, consts, and *local* identifiers (parameters, `let`
//! bindings, closure params, `for` binders, and match/`if let` pattern
//! bindings).
//!
//! The occurrence set comes from [`crate::symbol_occurrences`], which walks the
//! parsed AST with a lexical scope stack:
//! * renaming a local edits only that binding's occurrences, so a shadowed
//!   inner `x` is untouched when renaming an outer `x` (and vice-versa);
//! * renaming a top-level item edits every textual occurrence of the name
//!   *except* the spans that bind to a same-named local, so a global rename
//!   never sweeps up a shadowing local.
//!
//! When the buffer does not parse, it falls back to a pure textual match (the
//! original first-pass behaviour) so broken files still get a best-effort edit.
//! Cross-file renames remain P3 work.

use std::collections::HashMap;

use lsp_types::{
    OneOf, Position, PrepareRenameResponse, Range, RenameParams, TextDocumentPositionParams,
    TextEdit, Uri, WorkspaceEdit,
};

use crate::{identifier_at_position, span_to_range, symbol_occurrences};

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

    let (target_span, target_name) = crate::ident_token_at(source, pos)?;
    if target_name == new_name {
        return None;
    }

    let spans = symbol_occurrences(source, &target_span, &target_name);
    if spans.occurrences.is_empty() {
        return None;
    }

    let edits: Vec<TextEdit> = spans
        .occurrences
        .iter()
        .map(|span| TextEdit {
            range: span_to_range(source, span),
            new_text: new_name.to_string(),
        })
        .collect();

    let uri = params.text_document_position.text_document.uri.clone();

    #[allow(clippy::mutable_key_type)] // Uri uses interior mutability for caching
    let mut changes: HashMap<Uri, Vec<TextEdit>> = HashMap::new();
    changes.insert(uri, edits);

    Some(WorkspaceEdit {
        changes: Some(changes),
        document_changes: None,
        change_annotations: None,
    })
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

    /// Sorted byte-offsets of every edit's start position.
    fn edit_offsets(src: &str, edit: &WorkspaceEdit) -> Vec<usize> {
        #[allow(clippy::mutable_key_type)]
        let changes = edit.changes.as_ref().unwrap();
        let edits = changes.values().next().unwrap();
        let mut offsets: Vec<usize> = edits
            .iter()
            .map(|e| crate::position_to_offset(src, e.range.start).unwrap())
            .collect();
        offsets.sort_unstable();
        offsets
    }

    #[test]
    fn rename_inner_shadowed_local_does_not_touch_outer() {
        // Two `let x` in the same block; the second shadows the first.
        let src = "fn main() {\n    let x = 1\n    print(x)\n    let x = 2\n    print(x)\n}";
        // Cursor on the SECOND `let x` (idx 2 of four `x`s).
        let x2 = src.match_indices('x').nth(2).unwrap().0;
        let pos = crate::offset_to_position(src, x2);
        let params = RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: "file:///t.tb".parse::<Uri>().unwrap(),
                },
                position: pos,
            },
            new_name: "z".to_string(),
            work_done_progress_params: Default::default(),
        };
        let edit = compute_rename(src, &params).expect("rename produces edits");
        let got = edit_offsets(src, &edit);

        let decl2 = src.match_indices('x').nth(2).unwrap().0;
        let use2 = src.match_indices('x').nth(3).unwrap().0;
        // EXACT edit set: only the inner binding + its single use.
        assert_eq!(got, vec![decl2, use2]);
    }

    #[test]
    fn rename_outer_shadowed_local_does_not_touch_inner() {
        let src = "fn main() {\n    let x = 1\n    print(x)\n    let x = 2\n    print(x)\n}";
        // Cursor on the FIRST `let x` (idx 0).
        let x0 = src.match_indices('x').next().unwrap().0;
        let pos = crate::offset_to_position(src, x0);
        let params = RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: "file:///t.tb".parse::<Uri>().unwrap(),
                },
                position: pos,
            },
            new_name: "z".to_string(),
            work_done_progress_params: Default::default(),
        };
        let edit = compute_rename(src, &params).expect("rename produces edits");
        let got = edit_offsets(src, &edit);

        let decl1 = src.match_indices('x').next().unwrap().0;
        let use1 = src.match_indices('x').nth(1).unwrap().0;
        assert_eq!(got, vec![decl1, use1]);
    }

    #[test]
    fn rename_top_level_fn_skips_a_shadowing_local() {
        // Global `fn value`, called from `other` (unshadowed), plus a local
        // `value` in `main` that shadows it. Renaming the global rewrites the
        // global decl + the unshadowed call only; the local stays put.
        let src = "fn value() {}\nfn other() {\n    value()\n}\nfn main() {\n    let value = 1\n    print(value)\n}";
        // Cursor on the global declaration `fn value`.
        let decl = src.match_indices("value").next().unwrap().0;
        let pos = crate::offset_to_position(src, decl);
        let params = RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: "file:///t.tb".parse::<Uri>().unwrap(),
                },
                position: pos,
            },
            new_name: "compute".to_string(),
            work_done_progress_params: Default::default(),
        };
        let edit = compute_rename(src, &params).expect("rename produces edits");
        let got = edit_offsets(src, &edit);

        let call = src.match_indices("value").nth(1).unwrap().0; // value() in other
                                                                 // Only the global decl + its call; the local decl/use are excluded.
        assert_eq!(got, vec![decl, call]);
    }

    #[test]
    fn rename_local_param_edits_only_that_function() {
        // Same name `n` declared independently in two functions.
        let src = "fn a(n: int) {\n    print(n)\n}\nfn b(n: int) {\n    print(n)\n}";
        // Cursor on `a`'s parameter `n` (first `(n` occurrence).
        let a_param = src.match_indices("(n").next().unwrap().0 + 1;
        let pos = crate::offset_to_position(src, a_param);
        let params = RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: "file:///t.tb".parse::<Uri>().unwrap(),
                },
                position: pos,
            },
            new_name: "m".to_string(),
            work_done_progress_params: Default::default(),
        };
        let edit = compute_rename(src, &params).expect("rename produces edits");
        let got = edit_offsets(src, &edit);

        // a's param decl + a's single use; b's `n`s are untouched.
        let a_use = src.match_indices("(n)").next().unwrap().0 + 1;
        assert_eq!(got, vec![a_param, a_use]);
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
