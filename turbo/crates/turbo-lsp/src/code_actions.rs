//! `textDocument/codeAction` handler.
//!
//! Today this provides a single, focused quick-fix:
//!
//! * **"Apply suggestion"** — when a sema diagnostic message contains a
//!   ``did you mean `name`?`` hint (which the suggester emits for typo'd
//!   identifiers), surface it as a `QuickFix` code action that rewrites
//!   the offending range to the suggested identifier.
//!
//! Additional quick-fixes (auto-import, missing-field stubs, etc.) can
//! plug into [`compute_code_actions`] without changing the LSP wiring.

use std::collections::HashMap;

use lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, CodeActionParams, CodeActionResponse,
    TextEdit, Uri, WorkspaceEdit,
};

/// Compute the list of code actions for a given range. Returns an empty
/// vector when no actions apply — the LSP spec says this is preferable to
/// returning a JSON `null`.
pub(crate) fn compute_code_actions(params: &CodeActionParams) -> CodeActionResponse {
    let uri = params.text_document.uri.clone();
    let mut actions: Vec<CodeActionOrCommand> = Vec::new();

    for diag in &params.context.diagnostics {
        if let Some(suggestion) = extract_did_you_mean(&diag.message) {
            let edit = TextEdit {
                range: diag.range,
                new_text: suggestion.clone(),
            };
            #[allow(clippy::mutable_key_type)] // Uri uses interior mutability for caching
            let mut changes: HashMap<Uri, Vec<TextEdit>> = HashMap::new();
            changes.insert(uri.clone(), vec![edit]);

            actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: format!("Replace with `{suggestion}`"),
                kind: Some(CodeActionKind::QUICKFIX),
                diagnostics: Some(vec![diag.clone()]),
                edit: Some(WorkspaceEdit {
                    changes: Some(changes),
                    document_changes: None,
                    change_annotations: None,
                }),
                command: None,
                is_preferred: Some(true),
                disabled: None,
                data: None,
            }));
        }
    }

    actions
}

/// Extracts the suggestion from a sema "did you mean `X`?" diagnostic message.
/// Returns `None` if the message doesn't contain such a hint.
fn extract_did_you_mean(message: &str) -> Option<String> {
    let needle = "did you mean `";
    let start = message.find(needle)?;
    let after = &message[start + needle.len()..];
    let end = after.find('`')?;
    Some(after[..end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsp_types::{
        CodeActionContext, Diagnostic, DiagnosticSeverity, PartialResultParams, Position, Range,
        TextDocumentIdentifier, WorkDoneProgressParams,
    };

    fn make_params(message: &str) -> CodeActionParams {
        let diag = Diagnostic {
            range: Range {
                start: Position {
                    line: 0,
                    character: 4,
                },
                end: Position {
                    line: 0,
                    character: 7,
                },
            },
            severity: Some(DiagnosticSeverity::ERROR),
            message: message.to_string(),
            ..Default::default()
        };
        CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: "file:///t.tb".parse::<Uri>().unwrap(),
            },
            range: diag.range,
            context: CodeActionContext {
                diagnostics: vec![diag],
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        }
    }

    #[test]
    fn extract_suggestion_from_message() {
        let m = "undefined variable `foo`. did you mean `food`?";
        assert_eq!(extract_did_you_mean(m), Some("food".to_string()));
    }

    #[test]
    fn extract_returns_none_without_hint() {
        let m = "undefined variable `foo`.";
        assert_eq!(extract_did_you_mean(m), None);
    }

    #[test]
    fn code_action_quick_fix_for_did_you_mean() {
        let params = make_params("undefined variable `foo`. did you mean `food`?");
        let actions = compute_code_actions(&params);
        assert_eq!(actions.len(), 1);
        let action = match &actions[0] {
            CodeActionOrCommand::CodeAction(a) => a,
            _ => panic!("expected CodeAction"),
        };
        assert!(action.title.contains("food"));
        assert_eq!(action.kind, Some(CodeActionKind::QUICKFIX));
        let edit = action.edit.as_ref().unwrap();
        // `lsp_types::Uri` has interior mutability via cached parts, so the
        // `HashMap<Uri, _>` trips clippy::mutable_key_type. The map is
        // produced by us and never mutated through its keys, so the lint is
        // a false positive here.
        #[allow(clippy::mutable_key_type)]
        let changes = edit.changes.as_ref().unwrap();
        let edits = changes.values().next().unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "food");
    }

    #[test]
    fn code_action_returns_empty_when_no_hint() {
        let params = make_params("undefined variable `foo`.");
        let actions = compute_code_actions(&params);
        assert!(actions.is_empty());
    }
}
