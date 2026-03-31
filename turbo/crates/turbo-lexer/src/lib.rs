use logos::Logos;
use std::fmt;

/// A span in source code: byte offset range [start, end)
pub type Span = std::ops::Range<usize>;

/// A token with its span in the source
#[derive(Debug, Clone, PartialEq)]
pub struct Spanned<T> {
    pub value: T,
    pub span: Span,
}

impl<T> Spanned<T> {
    pub fn new(value: T, span: Span) -> Self {
        Self { value, span }
    }
}

#[derive(Logos, Debug, Clone, PartialEq, Eq, Hash)]
#[logos(skip r"[ \t\r]+")]
pub enum Token {
    // --- Keywords ---
    #[token("fn")]
    Fn,
    #[token("let")]
    Let,
    #[token("mut")]
    Mut,
    #[token("const")]
    Const,
    #[token("if")]
    If,
    #[token("else")]
    Else,
    #[token("while")]
    While,
    #[token("for")]
    For,
    #[token("in")]
    In,
    #[token("return")]
    Return,
    #[token("true")]
    True,
    #[token("false")]
    False,
    #[token("match")]
    Match,
    #[token("struct")]
    Struct,
    #[token("type")]
    TypeKw,
    #[token("impl")]
    Impl,
    #[token("trait")]
    Trait,
    #[token("pub")]
    Pub,
    #[token("import")]
    Import,
    #[token("from")]
    From,
    #[token("async")]
    Async,
    #[token("await")]
    Await,
    #[token("spawn")]
    Spawn,
    #[token("defer")]
    Defer,
    #[token("tool")]
    Tool,
    #[token("agent")]
    Agent,
    #[token("none")]
    None,
    #[token("some")]
    Some,
    #[token("ok")]
    Ok,
    #[token("err")]
    Err,
    #[token("break")]
    Break,
    #[token("continue")]
    Continue,

    // --- Literals ---
    #[regex(r"[0-9][0-9_]*\.[0-9][0-9_]*", |lex| Some(lex.slice().replace('_', "")))]
    Float(std::string::String),

    #[regex(r"[0-9][0-9_]*", |lex| lex.slice().replace('_', "").parse::<i64>().ok(), priority = 2)]
    Int(i64),

    #[token(r#"""""#, lex_triple_quote_string, priority = 10)]
    #[regex(r#""([^"\\]|\\.)*""#, parse_string)]
    String(std::string::String),

    // --- Identifiers ---
    #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*", |lex| lex.slice().to_string(), priority = 1)]
    Ident(std::string::String),

    // --- Operators (multi-char first for priority) ---
    #[token("==")]
    EqEq,
    #[token("!=")]
    NotEq,
    #[token("<=")]
    LessEq,
    #[token(">=")]
    GreaterEq,
    #[token("&&")]
    And,
    #[token("||")]
    Or,
    #[token("->")]
    Arrow,
    #[token("=>")]
    FatArrow,
    #[token("|>")]
    Pipe,
    #[token("..")]
    DotDot,
    #[token("?.")]
    QuestionDot,
    #[token("??")]
    QuestionQuestion,
    #[token("+=")]
    PlusEq,
    #[token("-=")]
    MinusEq,
    #[token("*=")]
    StarEq,
    #[token("/=")]
    SlashEq,

    // --- Single pipe (for closures) ---
    #[token("|")]
    Bar,

    // --- Single-char operators ---
    #[token("+")]
    Plus,
    #[token("-")]
    Minus,
    #[token("*")]
    Star,
    #[token("/")]
    Slash,
    #[token("%")]
    Percent,
    #[token("=")]
    Eq,
    #[token("<")]
    Less,
    #[token(">")]
    Greater,
    #[token("!")]
    Bang,
    #[token("?")]
    Question,
    #[token(".")]
    Dot,

    // --- Delimiters ---
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,

    // --- Punctuation ---
    #[token(",")]
    Comma,
    #[token(":")]
    Colon,
    #[token(";")]
    Semi,
    #[token("@")]
    At,

    // --- Comments & whitespace ---
    #[regex(r"//[^\n]*", logos::skip)]
    LineComment,

    #[token("/*", lex_block_comment)]
    BlockComment,

    #[token("\n")]
    Newline,
}

fn lex_block_comment(lex: &mut logos::Lexer<Token>) -> logos::FilterResult<(), ()> {
    let remainder = lex.remainder();
    let mut depth: u32 = 1;
    let mut chars = remainder.char_indices();
    while let Some((_i, c)) = chars.next() {
        match c {
            '/' => {
                if let Some((_, '*')) = chars.clone().next() {
                    chars.next();
                    depth += 1;
                }
            }
            '*' => {
                if let Some((j, '/')) = chars.clone().next() {
                    chars.next();
                    depth -= 1;
                    if depth == 0 {
                        lex.bump(j + 1);
                        return logos::FilterResult::Skip;
                    }
                }
            }
            _ => {}
        }
    }
    // Unclosed comment - bump to end
    lex.bump(remainder.len());
    logos::FilterResult::Skip
}

fn lex_triple_quote_string(lex: &mut logos::Lexer<Token>) -> Option<std::string::String> {
    let remainder = lex.remainder();
    // Find closing """
    if let Some(end_idx) = remainder.find(r#"""""#) {
        let content = &remainder[..end_idx];
        lex.bump(end_idx + 3); // skip content + closing """

        // Strip leading newline if present (the newline right after opening """)
        let content = if let Some(stripped) = content.strip_prefix('\n') {
            stripped
        } else if let Some(stripped) = content.strip_prefix("\r\n") {
            stripped
        } else {
            content
        };

        // Strip trailing whitespace-only line + newline before closing """
        let content = content.trim_end();

        // Find the minimum indentation (non-empty lines only)
        let min_indent = content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| line.len() - line.trim_start().len())
            .min()
            .unwrap_or(0);

        // Strip common leading whitespace
        let dedented: std::string::String = content
            .lines()
            .map(|line| {
                if line.trim().is_empty() {
                    ""
                } else if line.len() >= min_indent {
                    &line[min_indent..]
                } else {
                    line
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        Some(dedented)
    } else {
        // Unclosed triple-quote string — consume to end
        lex.bump(remainder.len());
        Some(remainder.to_string())
    }
}

fn parse_string(lex: &mut logos::Lexer<Token>) -> Option<std::string::String> {
    let slice = lex.slice();
    // Strip surrounding quotes
    let inner = &slice[1..slice.len() - 1];
    // Process escape sequences
    let mut result = std::string::String::new();
    let mut chars = inner.chars();
    while let std::option::Option::Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                std::option::Option::Some('n') => result.push('\n'),
                std::option::Option::Some('t') => result.push('\t'),
                std::option::Option::Some('r') => result.push('\r'),
                std::option::Option::Some('\\') => result.push('\\'),
                std::option::Option::Some('"') => result.push('"'),
                std::option::Option::Some('{') => {
                    result.push('\\');
                    result.push('{');
                }
                std::option::Option::Some('}') => {
                    result.push('\\');
                    result.push('}');
                }
                std::option::Option::Some(other) => {
                    result.push('\\');
                    result.push(other);
                }
                std::option::Option::None => result.push('\\'),
            }
        } else {
            result.push(c);
        }
    }
    std::option::Option::Some(result)
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Token::Fn => write!(f, "fn"),
            Token::Let => write!(f, "let"),
            Token::Mut => write!(f, "mut"),
            Token::Const => write!(f, "const"),
            Token::If => write!(f, "if"),
            Token::Else => write!(f, "else"),
            Token::While => write!(f, "while"),
            Token::For => write!(f, "for"),
            Token::In => write!(f, "in"),
            Token::Return => write!(f, "return"),
            Token::True => write!(f, "true"),
            Token::False => write!(f, "false"),
            Token::Match => write!(f, "match"),
            Token::Struct => write!(f, "struct"),
            Token::TypeKw => write!(f, "type"),
            Token::Impl => write!(f, "impl"),
            Token::Trait => write!(f, "trait"),
            Token::Pub => write!(f, "pub"),
            Token::Import => write!(f, "import"),
            Token::From => write!(f, "from"),
            Token::Async => write!(f, "async"),
            Token::Await => write!(f, "await"),
            Token::Spawn => write!(f, "spawn"),
            Token::Defer => write!(f, "defer"),
            Token::Tool => write!(f, "tool"),
            Token::Agent => write!(f, "agent"),
            Token::None => write!(f, "none"),
            Token::Some => write!(f, "some"),
            Token::Ok => write!(f, "ok"),
            Token::Err => write!(f, "err"),
            Token::Break => write!(f, "break"),
            Token::Continue => write!(f, "continue"),
            Token::Float(v) => write!(f, "{v}"),
            Token::Int(v) => write!(f, "{v}"),
            Token::String(s) => write!(f, "\"{s}\""),
            Token::Ident(s) => write!(f, "{s}"),
            Token::EqEq => write!(f, "=="),
            Token::NotEq => write!(f, "!="),
            Token::LessEq => write!(f, "<="),
            Token::GreaterEq => write!(f, ">="),
            Token::And => write!(f, "&&"),
            Token::Or => write!(f, "||"),
            Token::Arrow => write!(f, "->"),
            Token::FatArrow => write!(f, "=>"),
            Token::Pipe => write!(f, "|>"),
            Token::DotDot => write!(f, ".."),
            Token::QuestionDot => write!(f, "?."),
            Token::QuestionQuestion => write!(f, "??"),
            Token::PlusEq => write!(f, "+="),
            Token::MinusEq => write!(f, "-="),
            Token::StarEq => write!(f, "*="),
            Token::SlashEq => write!(f, "/="),
            Token::Bar => write!(f, "|"),
            Token::Plus => write!(f, "+"),
            Token::Minus => write!(f, "-"),
            Token::Star => write!(f, "*"),
            Token::Slash => write!(f, "/"),
            Token::Percent => write!(f, "%"),
            Token::Eq => write!(f, "="),
            Token::Less => write!(f, "<"),
            Token::Greater => write!(f, ">"),
            Token::Bang => write!(f, "!"),
            Token::Question => write!(f, "?"),
            Token::Dot => write!(f, "."),
            Token::LParen => write!(f, "("),
            Token::RParen => write!(f, ")"),
            Token::LBrace => write!(f, "{{"),
            Token::RBrace => write!(f, "}}"),
            Token::LBracket => write!(f, "["),
            Token::RBracket => write!(f, "]"),
            Token::Comma => write!(f, ","),
            Token::Colon => write!(f, ":"),
            Token::Semi => write!(f, ";"),
            Token::At => write!(f, "@"),
            Token::LineComment => write!(f, "// comment"),
            Token::BlockComment => write!(f, "/* comment */"),
            Token::Newline => write!(f, "\\n"),
        }
    }
}

/// Tokenize source code into a vector of spanned tokens.
/// Returns tokens and any error spans.
pub fn tokenize(source: &str) -> (Vec<Spanned<Token>>, Vec<Span>) {
    let mut tokens = Vec::new();
    let mut errors = Vec::new();
    let mut lexer = Token::lexer(source);

    while let std::option::Option::Some(result) = lexer.next() {
        let span = lexer.span();
        match result {
            std::result::Result::Ok(token) => {
                tokens.push(Spanned::new(token, span));
            }
            std::result::Result::Err(()) => {
                errors.push(span);
            }
        }
    }

    (tokens, errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hello_world() {
        let source = r#"fn main() {
    print("Hello, world!")
}"#;
        let (tokens, errors) = tokenize(source);
        assert!(errors.is_empty(), "Unexpected lex errors: {:?}", errors);

        let kinds: Vec<_> = tokens.iter().map(|t| &t.value).collect();
        assert!(matches!(kinds[0], Token::Fn));
        assert!(matches!(kinds[1], Token::Ident(s) if s == "main"));
        assert!(matches!(kinds[2], Token::LParen));
        assert!(matches!(kinds[3], Token::RParen));
        assert!(matches!(kinds[4], Token::LBrace));
        assert!(matches!(kinds[5], Token::Newline));
        assert!(matches!(kinds[6], Token::Ident(s) if s == "print"));
        assert!(matches!(kinds[7], Token::LParen));
        assert!(matches!(kinds[8], Token::String(s) if s == "Hello, world!"));
        assert!(matches!(kinds[9], Token::RParen));
        assert!(matches!(kinds[10], Token::Newline));
        assert!(matches!(kinds[11], Token::RBrace));
    }

    #[test]
    fn test_operators() {
        let source = "x + y == z && a || !b";
        let (tokens, errors) = tokenize(source);
        assert!(errors.is_empty());

        let kinds: Vec<_> = tokens.iter().map(|t| &t.value).collect();
        assert!(matches!(kinds[1], Token::Plus));
        assert!(matches!(kinds[3], Token::EqEq));
        assert!(matches!(kinds[5], Token::And));
        assert!(matches!(kinds[7], Token::Or));
        assert!(matches!(kinds[8], Token::Bang));
    }

    #[test]
    fn test_let_binding() {
        let source = "let mut x = 42";
        let (tokens, errors) = tokenize(source);
        assert!(errors.is_empty());

        let kinds: Vec<_> = tokens.iter().map(|t| &t.value).collect();
        assert!(matches!(kinds[0], Token::Let));
        assert!(matches!(kinds[1], Token::Mut));
        assert!(matches!(kinds[2], Token::Ident(s) if s == "x"));
        assert!(matches!(kinds[3], Token::Eq));
        assert!(matches!(kinds[4], Token::Int(42)));
    }

    #[test]
    fn test_comments_skipped() {
        let source = "let x = 1 // this is a comment\nlet y = 2";
        let (tokens, errors) = tokenize(source);
        assert!(errors.is_empty());

        // Comments should be skipped — no comment token present
        let has_comment = tokens.iter().any(|t| matches!(t.value, Token::LineComment));
        assert!(!has_comment);
    }

    #[test]
    fn test_float_literal() {
        let source = "3.14";
        let (tokens, errors) = tokenize(source);
        assert!(errors.is_empty());
        assert!(matches!(&tokens[0].value, Token::Float(s) if s == "3.14"));
    }

    #[test]
    fn test_string_escapes() {
        let source = r#""hello\nworld""#;
        let (tokens, errors) = tokenize(source);
        assert!(errors.is_empty());
        assert!(matches!(&tokens[0].value, Token::String(s) if s == "hello\nworld"));
    }

    #[test]
    fn test_underscore_numbers() {
        let source = "1_000_000";
        let (tokens, errors) = tokenize(source);
        assert!(errors.is_empty());
        assert!(matches!(tokens[0].value, Token::Int(1_000_000)));
    }

    #[test]
    fn test_nested_block_comments() {
        let source = "/* outer /* inner */ still comment */ visible";
        let (tokens, errors) = tokenize(source);
        assert!(errors.is_empty(), "Unexpected lex errors: {:?}", errors);

        // Only the identifier "visible" should remain after the nested comment is skipped
        assert_eq!(tokens.len(), 1, "Expected 1 token, got {:?}", tokens);
        assert!(matches!(&tokens[0].value, Token::Ident(s) if s == "visible"));
    }

    #[test]
    fn test_empty_string_literal() {
        let source = r#""""#;
        let (tokens, errors) = tokenize(source);
        assert!(errors.is_empty(), "Unexpected lex errors: {:?}", errors);
        assert_eq!(tokens.len(), 1);
        assert!(matches!(&tokens[0].value, Token::String(s) if s.is_empty()));
    }

    #[test]
    fn test_all_keywords_tokenize() {
        let keywords = vec![
            ("fn", Token::Fn),
            ("let", Token::Let),
            ("mut", Token::Mut),
            ("const", Token::Const),
            ("if", Token::If),
            ("else", Token::Else),
            ("while", Token::While),
            ("for", Token::For),
            ("in", Token::In),
            ("return", Token::Return),
            ("true", Token::True),
            ("false", Token::False),
            ("match", Token::Match),
            ("struct", Token::Struct),
            ("type", Token::TypeKw),
            ("impl", Token::Impl),
            ("trait", Token::Trait),
            ("pub", Token::Pub),
            ("import", Token::Import),
            ("from", Token::From),
            ("async", Token::Async),
            ("await", Token::Await),
            ("spawn", Token::Spawn),
            ("defer", Token::Defer),
            ("tool", Token::Tool),
            ("agent", Token::Agent),
            ("none", Token::None),
            ("some", Token::Some),
            ("ok", Token::Ok),
            ("err", Token::Err),
            ("break", Token::Break),
            ("continue", Token::Continue),
        ];
        for (source, expected) in keywords {
            let (tokens, errors) = tokenize(source);
            assert!(errors.is_empty(), "Lex errors for `{source}`: {:?}", errors);
            assert_eq!(
                tokens.len(),
                1,
                "Expected 1 token for `{source}`, got {:?}",
                tokens
            );
            assert_eq!(
                tokens[0].value, expected,
                "Keyword `{source}` should tokenize as {:?}, got {:?}",
                expected, tokens[0].value
            );
            // Ensure it did NOT tokenize as an Ident
            assert!(
                !matches!(&tokens[0].value, Token::Ident(_)),
                "Keyword `{source}` was incorrectly tokenized as Ident"
            );
        }
    }

    #[test]
    fn test_adjacent_operators() {
        let source = "x+y";
        let (tokens, errors) = tokenize(source);
        assert!(errors.is_empty(), "Unexpected lex errors: {:?}", errors);
        assert_eq!(tokens.len(), 3, "Expected 3 tokens, got {:?}", tokens);
        assert!(matches!(&tokens[0].value, Token::Ident(s) if s == "x"));
        assert!(matches!(&tokens[1].value, Token::Plus));
        assert!(matches!(&tokens[2].value, Token::Ident(s) if s == "y"));
    }

    #[test]
    fn test_tool_keyword() {
        let source = "tool fn search(query: str) -> str { }";
        let (tokens, errors) = tokenize(source);
        assert!(errors.is_empty());
        let kinds: Vec<_> = tokens.iter().map(|t| &t.value).collect();
        assert!(matches!(kinds[0], Token::Tool));
        assert!(matches!(kinds[1], Token::Fn));
        assert!(matches!(kinds[2], Token::Ident(s) if s == "search"));
    }

    #[test]
    fn test_agent_keyword() {
        let source = "agent Helper { }";
        let (tokens, errors) = tokenize(source);
        assert!(errors.is_empty());
        let kinds: Vec<_> = tokens.iter().map(|t| &t.value).collect();
        assert!(matches!(kinds[0], Token::Agent));
        assert!(matches!(kinds[1], Token::Ident(s) if s == "Helper"));
        assert!(matches!(kinds[2], Token::LBrace));
        assert!(matches!(kinds[3], Token::RBrace));
    }
}
