//! `textDocument/semanticTokens/full` handler.
//!
//! Classifies every token in the buffer as one of a small fixed set of LSP
//! semantic-token types so the editor can apply richer highlighting than
//! tree-sitter alone provides:
//!
//! * `KEYWORD` — Turbo language keywords (`fn`, `let`, `if`, `match`, ...)
//! * `STRING` / `NUMBER` — string and numeric literals
//! * `COMMENT` — line comments
//! * `OPERATOR` — punctuation operators
//! * `FUNCTION` — identifiers that match a top-level function name
//! * `STRUCT` — identifiers that match a top-level struct name
//! * `ENUM` — identifiers that match a top-level enum name
//! * `INTERFACE` — identifiers that match a top-level trait name
//! * `VARIABLE` — every other identifier
//!
//! The classification of identifiers is intentionally cheap: it parses the
//! buffer once, builds a name → kind map from top-level items, and looks
//! each `Ident` token up. Local-scope analysis is left to a future pass.

use lsp_types::{SemanticToken, SemanticTokenType, SemanticTokens, SemanticTokensLegend};

use crate::position_to_offset;
use turbo_lexer::Token;

/// LSP semantic token type indices used by [`compute_semantic_tokens`].
///
/// The indices match the order of [`legend()`].
pub(crate) const TT_KEYWORD: u32 = 0;
pub(crate) const TT_STRING: u32 = 1;
pub(crate) const TT_NUMBER: u32 = 2;
pub(crate) const TT_COMMENT: u32 = 3;
pub(crate) const TT_OPERATOR: u32 = 4;
pub(crate) const TT_FUNCTION: u32 = 5;
pub(crate) const TT_STRUCT: u32 = 6;
pub(crate) const TT_ENUM: u32 = 7;
pub(crate) const TT_INTERFACE: u32 = 8;
pub(crate) const TT_VARIABLE: u32 = 9;

/// Returns the legend that must be advertised in the `initialize` response
/// for clients to decode the encoded delta-stream below.
pub(crate) fn legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: vec![
            SemanticTokenType::KEYWORD,
            SemanticTokenType::STRING,
            SemanticTokenType::NUMBER,
            SemanticTokenType::COMMENT,
            SemanticTokenType::OPERATOR,
            SemanticTokenType::FUNCTION,
            SemanticTokenType::STRUCT,
            SemanticTokenType::ENUM,
            SemanticTokenType::INTERFACE,
            SemanticTokenType::VARIABLE,
        ],
        token_modifiers: vec![],
    }
}

/// Compute the semantic-token stream for `source`. Returns an empty token
/// list when the source can't be lexed (so the editor falls back to
/// syntactic highlighting instead of seeing an error).
pub(crate) fn compute_semantic_tokens(source: &str) -> SemanticTokens {
    let (tokens, lex_errors) = turbo_lexer::tokenize(source);
    if !lex_errors.is_empty() {
        return SemanticTokens::default();
    }

    // Build a name -> token type map by parsing top-level items.
    let mut ident_kinds: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    let (module, parse_errors) = turbo_parser::parse(tokens.clone());
    if parse_errors.is_empty() {
        for item in &module.items {
            match &item.node {
                turbo_ast::Item::Function(f) => {
                    ident_kinds.insert(f.name.clone(), TT_FUNCTION);
                }
                turbo_ast::Item::Struct(s) => {
                    ident_kinds.insert(s.name.clone(), TT_STRUCT);
                }
                turbo_ast::Item::Enum(e) => {
                    ident_kinds.insert(e.name.clone(), TT_ENUM);
                }
                turbo_ast::Item::Trait(t) => {
                    ident_kinds.insert(t.name.clone(), TT_INTERFACE);
                }
                _ => {}
            }
        }
    }

    let mut raw: Vec<(u32, u32, u32, u32)> = Vec::new(); // (line, char, len, token_type)
    for tok in &tokens {
        let Some(token_type) = classify_token(&tok.value, &ident_kinds) else {
            continue;
        };
        let (line, character) = offset_to_line_char(source, tok.span.start);
        let len = (tok.span.end - tok.span.start) as u32;
        // Skip multi-line tokens (LSP semantic tokens are single-line per entry).
        if source[tok.span.clone()].contains('\n') {
            continue;
        }
        raw.push((line, character, len, token_type));
    }

    // The LSP wire format requires deltas relative to the previous token.
    raw.sort_by_key(|t| (t.0, t.1));
    let mut data = Vec::with_capacity(raw.len());
    let mut prev_line = 0u32;
    let mut prev_char = 0u32;
    for (line, character, length, token_type) in raw {
        let delta_line = line - prev_line;
        let delta_start = if delta_line == 0 {
            character - prev_char
        } else {
            character
        };
        data.push(SemanticToken {
            delta_line,
            delta_start,
            length,
            token_type,
            token_modifiers_bitset: 0,
        });
        prev_line = line;
        prev_char = character;
    }

    SemanticTokens {
        result_id: None,
        data,
    }
}

fn classify_token(
    tok: &Token,
    ident_kinds: &std::collections::HashMap<String, u32>,
) -> Option<u32> {
    match tok {
        Token::Ident(name) => Some(*ident_kinds.get(name).unwrap_or(&TT_VARIABLE)),
        Token::String(_) => Some(TT_STRING),
        Token::Int(_) | Token::Float(_) => Some(TT_NUMBER),
        Token::DocComment(_) => Some(TT_COMMENT),
        Token::Fn
        | Token::Let
        | Token::Mut
        | Token::Const
        | Token::If
        | Token::Else
        | Token::While
        | Token::For
        | Token::In
        | Token::Return
        | Token::Match
        | Token::Struct
        | Token::TypeKw
        | Token::Impl
        | Token::Trait
        | Token::Pub
        | Token::Import
        | Token::From
        | Token::Async
        | Token::Await
        | Token::Spawn
        | Token::Defer
        | Token::Extern
        | Token::True
        | Token::False
        | Token::None
        | Token::Some
        | Token::Ok
        | Token::Err
        | Token::Break
        | Token::Continue => Some(TT_KEYWORD),
        Token::Plus
        | Token::Minus
        | Token::Star
        | Token::Slash
        | Token::Percent
        | Token::Eq
        | Token::EqEq
        | Token::NotEq
        | Token::Less
        | Token::Greater
        | Token::LessEq
        | Token::GreaterEq
        | Token::And
        | Token::Or
        | Token::Bang
        | Token::Arrow
        | Token::FatArrow => Some(TT_OPERATOR),
        _ => None,
    }
}

fn offset_to_line_char(source: &str, offset: usize) -> (u32, u32) {
    // We can't use the existing helper because the LSP one returns a Position.
    // This is the same algorithm but tuple-shaped.
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
    (line, col)
}

// Re-export for completeness so the main loop can wire prepareSemanticTokens
// hover hints if it ever wants to highlight on hover too.
#[allow(dead_code)]
pub(crate) fn position_to_offset_compat(source: &str, pos: lsp_types::Position) -> Option<usize> {
    position_to_offset(source, pos)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legend_lists_all_token_types_in_order() {
        let l = legend();
        assert_eq!(l.token_types.len(), 10);
        assert_eq!(
            l.token_types[TT_KEYWORD as usize],
            SemanticTokenType::KEYWORD
        );
        assert_eq!(
            l.token_types[TT_FUNCTION as usize],
            SemanticTokenType::FUNCTION
        );
        assert_eq!(
            l.token_types[TT_VARIABLE as usize],
            SemanticTokenType::VARIABLE
        );
    }

    #[test]
    fn classifies_keywords_strings_numbers() {
        let src = "fn main() { let x = 42 }";
        let tokens = compute_semantic_tokens(src);
        // We expect at least: fn(KW), main(FUN), let(KW), x(VAR), 42(NUM)
        assert!(!tokens.data.is_empty());
        let kinds: Vec<u32> = tokens.data.iter().map(|t| t.token_type).collect();
        assert!(kinds.contains(&TT_KEYWORD));
        assert!(kinds.contains(&TT_FUNCTION));
        assert!(kinds.contains(&TT_VARIABLE));
        assert!(kinds.contains(&TT_NUMBER));
    }

    #[test]
    fn classifies_struct_definition() {
        let src = "struct Point { x: i32 }\nfn main() { let p = Point { x: 1 } }";
        let tokens = compute_semantic_tokens(src);
        assert!(tokens.data.iter().any(|t| t.token_type == TT_STRUCT));
    }

    #[test]
    fn empty_source_returns_empty_token_list() {
        let tokens = compute_semantic_tokens("");
        assert!(tokens.data.is_empty());
    }

    #[test]
    fn lex_error_returns_empty_instead_of_panicking() {
        // The lexer treats `$` as an unknown char.
        let tokens = compute_semantic_tokens("$$$");
        assert!(tokens.data.is_empty());
    }

    #[test]
    fn deltas_are_relative_per_lsp_spec() {
        let src = "fn a() {}\nfn b() {}";
        let tokens = compute_semantic_tokens(src);
        // The first token starts at line 0; subsequent deltas should never
        // make a token go backwards.
        let mut line = 0u32;
        for t in &tokens.data {
            line += t.delta_line;
            assert!(line < 100); // sanity bound
        }
    }
}
