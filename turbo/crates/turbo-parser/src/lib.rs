use turbo_ast::*;
use turbo_lexer::{Token, Spanned as LexSpanned};

/// A parse error with location info
#[derive(Debug, Clone)]
pub struct ParseError {
    pub message: String,
    pub span: Span,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "parse error at {:?}: {}", self.span, self.message)
    }
}

/// Recursive descent parser for Turbo
struct Parser {
    tokens: Vec<LexSpanned<Token>>,
    pos: usize,
    errors: Vec<ParseError>,
}

impl Parser {
    fn new(tokens: Vec<LexSpanned<Token>>) -> Self {
        // Filter out newlines — Turbo doesn't use them for syntax
        let tokens: Vec<_> = tokens
            .into_iter()
            .filter(|t| !matches!(t.value, Token::Newline))
            .collect();
        Self {
            tokens,
            pos: 0,
            errors: Vec::new(),
        }
    }

    // === Token access ===

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos).map(|t| &t.value)
    }

    fn peek_span(&self) -> Span {
        self.tokens
            .get(self.pos)
            .map(|t| t.span.clone())
            .unwrap_or_else(|| {
                let end = self.tokens.last().map(|t| t.span.end).unwrap_or(0);
                end..end
            })
    }

    fn advance(&mut self) -> &LexSpanned<Token> {
        let tok = &self.tokens[self.pos];
        self.pos += 1;
        tok
    }

    fn at_end(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    fn expect(&mut self, expected: &Token) -> Result<Span, ParseError> {
        if let Some(tok) = self.peek() {
            if tok == expected {
                let span = self.tokens[self.pos].span.clone();
                self.pos += 1;
                return Ok(span);
            }
            let span = self.peek_span();
            Err(ParseError {
                message: format!("expected `{expected}`, found `{tok}`"),
                span,
            })
        } else {
            let span = self.peek_span();
            Err(ParseError {
                message: format!("expected `{expected}`, found end of file"),
                span,
            })
        }
    }

    fn expect_ident(&mut self) -> Result<(String, Span), ParseError> {
        if let Some(Token::Ident(_)) = self.peek() {
            let tok = self.advance();
            if let Token::Ident(name) = &tok.value {
                return Ok((name.clone(), tok.span.clone()));
            }
        }
        let span = self.peek_span();
        let found = self.peek().map(|t| format!("`{t}`")).unwrap_or("end of file".to_string());
        Err(ParseError {
            message: format!("expected identifier, found {found}"),
            span,
        })
    }

    // === Top-level parsing ===

    fn parse_module(&mut self) -> Module {
        let mut items = Vec::new();
        while !self.at_end() {
            match self.parse_item() {
                Ok(item) => items.push(item),
                Err(e) => {
                    self.errors.push(e);
                    // Skip to next `fn` to recover
                    self.pos += 1;
                    while !self.at_end() && !matches!(self.peek(), Some(Token::Fn)) {
                        self.pos += 1;
                    }
                }
            }
        }
        Module { items }
    }

    fn parse_item(&mut self) -> Result<Spanned<Item>, ParseError> {
        let start = self.peek_span().start;
        match self.peek() {
            Some(Token::Fn) => {
                let f = self.parse_fn_def()?;
                let end = f.body.span.end;
                Ok(Spanned::new(Item::Function(f), start..end))
            }
            _ => {
                let span = self.peek_span();
                let found = self.peek().map(|t| format!("`{t}`")).unwrap_or("end of file".to_string());
                Err(ParseError {
                    message: format!("expected `fn`, found {found}"),
                    span,
                })
            }
        }
    }

    fn parse_fn_def(&mut self) -> Result<FnDef, ParseError> {
        self.expect(&Token::Fn)?;
        let (name, _) = self.expect_ident()?;

        // Parse optional type parameters: <T> or <T, U, ...>
        let type_params = if matches!(self.peek(), Some(Token::Less)) {
            self.advance(); // consume <
            let mut params = Vec::new();
            loop {
                let (tp_name, _) = self.expect_ident()?;
                params.push(tp_name);
                if matches!(self.peek(), Some(Token::Comma)) {
                    self.advance();
                } else {
                    break;
                }
            }
            self.expect(&Token::Greater)?;
            params
        } else {
            Vec::new()
        };

        self.expect(&Token::LParen)?;
        let params = self.parse_params()?;
        self.expect(&Token::RParen)?;

        let return_type = if matches!(self.peek(), Some(Token::Arrow)) {
            self.advance();
            Some(self.parse_type()?)
        } else {
            None
        };

        let body = self.parse_block()?;

        Ok(FnDef {
            name,
            type_params,
            params,
            return_type,
            body,
        })
    }

    fn parse_params(&mut self) -> Result<Vec<Param>, ParseError> {
        let mut params = Vec::new();
        if matches!(self.peek(), Some(Token::RParen)) {
            return Ok(params);
        }

        loop {
            let start = self.peek_span().start;
            let (name, _) = self.expect_ident()?;
            self.expect(&Token::Colon)?;
            let ty = self.parse_type()?;
            let end = ty.span.end;
            params.push(Param {
                name,
                ty,
                span: start..end,
            });

            if matches!(self.peek(), Some(Token::Comma)) {
                self.advance();
            } else {
                break;
            }
        }

        Ok(params)
    }

    fn parse_type(&mut self) -> Result<Spanned<TypeExpr>, ParseError> {
        let start = self.peek_span().start;

        // Unit type ()
        if matches!(self.peek(), Some(Token::LParen)) {
            let lp_pos = self.pos;
            self.advance();
            if matches!(self.peek(), Some(Token::RParen)) {
                let end = self.peek_span().end;
                self.advance();
                return Ok(Spanned::new(TypeExpr::Unit, start..end));
            }
            // Not a unit type, backtrack
            self.pos = lp_pos;
        }

        // Named type
        let (name, span) = self.expect_ident()?;
        Ok(Spanned::new(TypeExpr::Named(name), span))
    }

    // === Block ===

    fn parse_block(&mut self) -> Result<Spanned<Expr>, ParseError> {
        let start = self.peek_span().start;
        self.expect(&Token::LBrace)?;

        let mut stmts = Vec::new();
        let mut tail_expr = None;

        while !matches!(self.peek(), Some(Token::RBrace) | None) {
            // Try to parse a statement (let, return)
            if matches!(self.peek(), Some(Token::Let)) {
                let stmt = self.parse_let_stmt()?;
                stmts.push(stmt);
            } else if matches!(self.peek(), Some(Token::Return)) {
                let stmt = self.parse_return_stmt()?;
                stmts.push(stmt);
            } else {
                // Parse an expression
                let expr = self.parse_expr()?;

                // If followed by RBrace, this is the tail expression
                if matches!(self.peek(), Some(Token::RBrace)) {
                    tail_expr = Some(Box::new(expr));
                } else {
                    // It's a statement expression
                    let span = expr.span.clone();
                    stmts.push(Spanned::new(Stmt::Expr(expr), span));
                }
            }
        }

        let end = self.peek_span().end;
        self.expect(&Token::RBrace)?;

        Ok(Spanned::new(
            Expr::Block { stmts, tail_expr },
            start..end,
        ))
    }

    fn parse_let_stmt(&mut self) -> Result<Spanned<Stmt>, ParseError> {
        let start = self.peek_span().start;
        self.expect(&Token::Let)?;

        let mutable = if matches!(self.peek(), Some(Token::Mut)) {
            self.advance();
            true
        } else {
            false
        };

        let (name, _) = self.expect_ident()?;

        let ty = if matches!(self.peek(), Some(Token::Colon)) {
            self.advance();
            Some(self.parse_type()?)
        } else {
            None
        };

        self.expect(&Token::Eq)?;
        let value = self.parse_expr()?;
        let end = value.span.end;

        Ok(Spanned::new(
            Stmt::Let {
                mutable,
                name,
                ty,
                value,
            },
            start..end,
        ))
    }

    fn parse_return_stmt(&mut self) -> Result<Spanned<Stmt>, ParseError> {
        let start = self.peek_span().start;
        self.expect(&Token::Return)?;

        let value = if matches!(self.peek(), Some(Token::RBrace) | None) {
            None
        } else {
            Some(self.parse_expr()?)
        };

        let end = value.as_ref().map(|v| v.span.end).unwrap_or(start + 6);
        Ok(Spanned::new(Stmt::Return(value), start..end))
    }

    // === Expression parsing (Pratt precedence) ===

    fn parse_expr(&mut self) -> Result<Spanned<Expr>, ParseError> {
        let lhs = self.parse_unary()?;

        // Check for assignment
        if let Expr::Ident(ref name) = lhs.node {
            if matches!(self.peek(), Some(Token::Eq)) {
                let name = name.clone();
                self.advance();
                let value = self.parse_expr()?;
                let span = lhs.span.start..value.span.end;
                return Ok(Spanned::new(
                    Expr::Assign {
                        target: name,
                        value: Box::new(value),
                    },
                    span,
                ));
            }
            if let Some(op) = self.peek_compound_assign() {
                let name = name.clone();
                self.advance();
                let value = self.parse_expr()?;
                let span = lhs.span.start..value.span.end;
                return Ok(Spanned::new(
                    Expr::CompoundAssign {
                        target: name,
                        op,
                        value: Box::new(value),
                    },
                    span,
                ));
            }
        }

        self.parse_binary(lhs, 0)
    }

    fn peek_compound_assign(&self) -> Option<BinOp> {
        match self.peek() {
            Some(Token::PlusEq) => Some(BinOp::Add),
            Some(Token::MinusEq) => Some(BinOp::Sub),
            Some(Token::StarEq) => Some(BinOp::Mul),
            Some(Token::SlashEq) => Some(BinOp::Div),
            _ => None,
        }
    }

    fn parse_binary(&mut self, mut lhs: Spanned<Expr>, min_prec: u8) -> Result<Spanned<Expr>, ParseError> {
        while let Some(op) = self.peek_binop() {
            let prec = op.precedence();
            if prec < min_prec {
                break;
            }
            self.advance(); // consume operator
            let mut rhs = self.parse_unary()?;

            while let Some(next_op) = self.peek_binop() {
                let next_prec = next_op.precedence();
                if next_prec <= prec {
                    break;
                }
                rhs = self.parse_binary(rhs, next_prec)?;
            }

            let span = lhs.span.start..rhs.span.end;
            lhs = Spanned::new(
                Expr::BinaryOp {
                    left: Box::new(lhs),
                    op,
                    right: Box::new(rhs),
                },
                span,
            );
        }
        Ok(lhs)
    }

    fn peek_binop(&self) -> Option<BinOp> {
        match self.peek() {
            Some(Token::Plus) => Some(BinOp::Add),
            Some(Token::Minus) => Some(BinOp::Sub),
            Some(Token::Star) => Some(BinOp::Mul),
            Some(Token::Slash) => Some(BinOp::Div),
            Some(Token::Percent) => Some(BinOp::Mod),
            Some(Token::EqEq) => Some(BinOp::Eq),
            Some(Token::NotEq) => Some(BinOp::NotEq),
            Some(Token::Less) => Some(BinOp::Less),
            Some(Token::LessEq) => Some(BinOp::LessEq),
            Some(Token::Greater) => Some(BinOp::Greater),
            Some(Token::GreaterEq) => Some(BinOp::GreaterEq),
            Some(Token::And) => Some(BinOp::And),
            Some(Token::Or) => Some(BinOp::Or),
            _ => None,
        }
    }

    fn parse_unary(&mut self) -> Result<Spanned<Expr>, ParseError> {
        let start = self.peek_span().start;

        if matches!(self.peek(), Some(Token::Minus)) {
            self.advance();
            let expr = self.parse_unary()?;
            let end = expr.span.end;
            return Ok(Spanned::new(
                Expr::UnaryOp {
                    op: UnaryOp::Neg,
                    expr: Box::new(expr),
                },
                start..end,
            ));
        }

        if matches!(self.peek(), Some(Token::Bang)) {
            self.advance();
            let expr = self.parse_unary()?;
            let end = expr.span.end;
            return Ok(Spanned::new(
                Expr::UnaryOp {
                    op: UnaryOp::Not,
                    expr: Box::new(expr),
                },
                start..end,
            ));
        }

        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<Spanned<Expr>, ParseError> {
        let mut expr = self.parse_atom()?;

        loop {
            if matches!(self.peek(), Some(Token::LParen)) {
                // Function call
                self.advance();
                let args = self.parse_call_args()?;
                let end = self.peek_span().end;
                self.expect(&Token::RParen)?;
                let span = expr.span.start..end;
                expr = Spanned::new(
                    Expr::Call {
                        callee: Box::new(expr),
                        args,
                    },
                    span,
                );
            } else {
                break;
            }
        }

        Ok(expr)
    }

    fn parse_call_args(&mut self) -> Result<Vec<Spanned<Expr>>, ParseError> {
        let mut args = Vec::new();
        if matches!(self.peek(), Some(Token::RParen)) {
            return Ok(args);
        }

        loop {
            args.push(self.parse_expr()?);
            if matches!(self.peek(), Some(Token::Comma)) {
                self.advance();
            } else {
                break;
            }
        }

        Ok(args)
    }

    fn parse_atom(&mut self) -> Result<Spanned<Expr>, ParseError> {
        let start = self.peek_span().start;

        match self.peek() {
            Some(Token::Int(_)) => {
                let tok = self.advance();
                if let Token::Int(n) = &tok.value {
                    let n = *n;
                    let span = tok.span.clone();
                    Ok(Spanned::new(Expr::IntLit(n), span))
                } else {
                    unreachable!()
                }
            }
            Some(Token::Float(_)) => {
                let tok = self.advance();
                if let Token::Float(s) = &tok.value {
                    let f: f64 = s.parse().unwrap_or(0.0);
                    let span = tok.span.clone();
                    Ok(Spanned::new(Expr::FloatLit(f), span))
                } else {
                    unreachable!()
                }
            }
            Some(Token::String(_)) => {
                let tok = self.advance();
                if let Token::String(s) = &tok.value {
                    let s = s.clone();
                    let span = tok.span.clone();
                    Ok(Spanned::new(Expr::StringLit(s), span))
                } else {
                    unreachable!()
                }
            }
            Some(Token::True) => {
                let span = self.advance().span.clone();
                Ok(Spanned::new(Expr::BoolLit(true), span))
            }
            Some(Token::False) => {
                let span = self.advance().span.clone();
                Ok(Spanned::new(Expr::BoolLit(false), span))
            }
            Some(Token::Ident(_)) => {
                let tok = self.advance();
                if let Token::Ident(name) = &tok.value {
                    let name = name.clone();
                    let span = tok.span.clone();
                    Ok(Spanned::new(Expr::Ident(name), span))
                } else {
                    unreachable!()
                }
            }
            Some(Token::LParen) => {
                // Parenthesized expression or unit
                self.advance();
                if matches!(self.peek(), Some(Token::RParen)) {
                    let end = self.peek_span().end;
                    self.advance();
                    return Ok(Spanned::new(Expr::Unit, start..end));
                }
                let expr = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                Ok(expr)
            }
            Some(Token::If) => self.parse_if_expr(),
            Some(Token::While) => self.parse_while_expr(),
            Some(Token::LBrace) => self.parse_block(),
            _ => {
                let span = self.peek_span();
                let found = self.peek().map(|t| format!("`{t}`")).unwrap_or("end of file".to_string());
                Err(ParseError {
                    message: format!("expected expression, found {found}"),
                    span,
                })
            }
        }
    }

    fn parse_if_expr(&mut self) -> Result<Spanned<Expr>, ParseError> {
        let start = self.peek_span().start;
        self.expect(&Token::If)?;

        let condition = self.parse_expr()?;
        let then_branch = self.parse_block()?;

        let else_branch = if matches!(self.peek(), Some(Token::Else)) {
            self.advance();
            if matches!(self.peek(), Some(Token::If)) {
                // else if chain
                Some(self.parse_if_expr()?)
            } else {
                Some(self.parse_block()?)
            }
        } else {
            None
        };

        let end = else_branch
            .as_ref()
            .map(|e| e.span.end)
            .unwrap_or(then_branch.span.end);

        Ok(Spanned::new(
            Expr::If {
                condition: Box::new(condition),
                then_branch: Box::new(then_branch),
                else_branch: else_branch.map(Box::new),
            },
            start..end,
        ))
    }

    fn parse_while_expr(&mut self) -> Result<Spanned<Expr>, ParseError> {
        let start = self.peek_span().start;
        self.expect(&Token::While)?;

        let condition = self.parse_expr()?;
        let body = self.parse_block()?;
        let end = body.span.end;

        Ok(Spanned::new(
            Expr::While {
                condition: Box::new(condition),
                body: Box::new(body),
            },
            start..end,
        ))
    }
}

/// Parse a token stream into a Module.
/// Returns the module and any parse errors.
pub fn parse(tokens: Vec<LexSpanned<Token>>) -> (Module, Vec<ParseError>) {
    let mut parser = Parser::new(tokens);
    let module = parser.parse_module();
    (module, parser.errors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use turbo_lexer::tokenize;

    fn parse_source(source: &str) -> Module {
        let (tokens, lex_errors) = tokenize(source);
        assert!(lex_errors.is_empty(), "Lex errors: {:?}", lex_errors);
        let (module, parse_errors) = parse(tokens);
        assert!(parse_errors.is_empty(), "Parse errors: {:?}", parse_errors);
        module
    }

    #[test]
    fn test_empty_main() {
        let module = parse_source("fn main() { }");
        assert_eq!(module.items.len(), 1);
        if let Item::Function(f) = &module.items[0].node {
            assert_eq!(f.name, "main");
            assert!(f.params.is_empty());
            assert!(f.return_type.is_none());
        } else {
            panic!("Expected function");
        }
    }

    #[test]
    fn test_hello_world() {
        let source = r#"fn main() {
            print("Hello, world!")
        }"#;
        let module = parse_source(source);
        assert_eq!(module.items.len(), 1);
    }

    #[test]
    fn test_let_binding() {
        let source = "fn main() { let x = 42 }";
        let module = parse_source(source);
        if let Item::Function(f) = &module.items[0].node {
            if let Expr::Block { stmts, .. } = &f.body.node {
                assert_eq!(stmts.len(), 1);
                if let Stmt::Let { name, mutable, .. } = &stmts[0].node {
                    assert_eq!(name, "x");
                    assert!(!mutable);
                } else {
                    panic!("Expected let statement");
                }
            } else {
                panic!("Expected block");
            }
        }
    }

    #[test]
    fn test_let_mut() {
        let source = "fn main() { let mut x = 0 }";
        let module = parse_source(source);
        if let Item::Function(f) = &module.items[0].node {
            if let Expr::Block { stmts, .. } = &f.body.node {
                if let Stmt::Let { name, mutable, .. } = &stmts[0].node {
                    assert_eq!(name, "x");
                    assert!(mutable);
                }
            }
        }
    }

    #[test]
    fn test_binary_precedence() {
        let source = "fn main() { 1 + 2 * 3 }";
        let module = parse_source(source);
        if let Item::Function(f) = &module.items[0].node {
            if let Expr::Block { tail_expr: Some(tail), .. } = &f.body.node {
                // Should be Add(1, Mul(2, 3)) due to precedence
                if let Expr::BinaryOp { op, left, right } = &tail.node {
                    assert_eq!(*op, BinOp::Add);
                    assert!(matches!(left.node, Expr::IntLit(1)));
                    if let Expr::BinaryOp { op: inner_op, .. } = &right.node {
                        assert_eq!(*inner_op, BinOp::Mul);
                    } else {
                        panic!("Expected Mul on RHS, got {:?}", right.node);
                    }
                } else {
                    panic!("Expected BinaryOp, got {:?}", tail.node);
                }
            } else {
                panic!("Expected tail expr");
            }
        }
    }

    #[test]
    fn test_if_else() {
        let source = "fn main() { if true { 1 } else { 2 } }";
        let module = parse_source(source);
        if let Item::Function(f) = &module.items[0].node {
            if let Expr::Block { tail_expr: Some(tail), .. } = &f.body.node {
                assert!(matches!(tail.node, Expr::If { .. }));
            } else {
                panic!("Expected tail expr");
            }
        }
    }

    #[test]
    fn test_function_with_params() {
        let source = "fn add(a: i32, b: i32) -> i32 { a + b }";
        let module = parse_source(source);
        if let Item::Function(f) = &module.items[0].node {
            assert_eq!(f.name, "add");
            assert_eq!(f.params.len(), 2);
            assert_eq!(f.params[0].name, "a");
            assert_eq!(f.params[1].name, "b");
            assert!(f.return_type.is_some());
        }
    }

    #[test]
    fn test_function_call() {
        let source = r#"fn main() { print("hello") }"#;
        let module = parse_source(source);
        if let Item::Function(f) = &module.items[0].node {
            if let Expr::Block { tail_expr: Some(tail), .. } = &f.body.node {
                assert!(matches!(tail.node, Expr::Call { .. }));
            }
        }
    }

    #[test]
    fn test_multiple_functions() {
        let source = r#"
            fn foo() -> i32 { 42 }
            fn main() { foo() }
        "#;
        let module = parse_source(source);
        assert_eq!(module.items.len(), 2);
    }

    #[test]
    fn test_nested_if() {
        let source = "fn main() { if true { if false { 1 } else { 2 } } else { 3 } }";
        let module = parse_source(source);
        assert_eq!(module.items.len(), 1);
    }

    #[test]
    fn test_unary_neg() {
        let source = "fn main() { -42 }";
        let module = parse_source(source);
        if let Item::Function(f) = &module.items[0].node {
            if let Expr::Block { tail_expr: Some(tail), .. } = &f.body.node {
                assert!(matches!(tail.node, Expr::UnaryOp { op: UnaryOp::Neg, .. }));
            }
        }
    }

    #[test]
    fn test_comparison() {
        let source = "fn main() { x > 25 }";
        let module = parse_source(source);
        if let Item::Function(f) = &module.items[0].node {
            if let Expr::Block { tail_expr: Some(tail), .. } = &f.body.node {
                if let Expr::BinaryOp { op, .. } = &tail.node {
                    assert_eq!(*op, BinOp::Greater);
                }
            }
        }
    }

    #[test]
    fn test_logical_operators() {
        let source = "fn main() { a && b || c }";
        let module = parse_source(source);
        if let Item::Function(f) = &module.items[0].node {
            if let Expr::Block { tail_expr: Some(tail), .. } = &f.body.node {
                // Should be Or(And(a, b), c) since && binds tighter
                if let Expr::BinaryOp { op, .. } = &tail.node {
                    assert_eq!(*op, BinOp::Or);
                }
            }
        }
    }

    #[test]
    fn test_stmt_then_tail() {
        let source = "fn main() { let x = 1\n x + 2 }";
        let module = parse_source(source);
        if let Item::Function(f) = &module.items[0].node {
            if let Expr::Block { stmts, tail_expr } = &f.body.node {
                assert_eq!(stmts.len(), 1);
                assert!(tail_expr.is_some());
            }
        }
    }

    #[test]
    fn test_return_statement() {
        let source = "fn foo() -> i32 { return 42 }";
        let module = parse_source(source);
        if let Item::Function(f) = &module.items[0].node {
            if let Expr::Block { stmts, .. } = &f.body.node {
                assert_eq!(stmts.len(), 1);
                assert!(matches!(&stmts[0].node, Stmt::Return(Some(_))));
            }
        }
    }

    #[test]
    fn test_let_with_type() {
        let source = "fn main() { let x: i32 = 42 }";
        let module = parse_source(source);
        if let Item::Function(f) = &module.items[0].node {
            if let Expr::Block { stmts, .. } = &f.body.node {
                if let Stmt::Let { ty: Some(ty), .. } = &stmts[0].node {
                    assert!(matches!(&ty.node, TypeExpr::Named(n) if n == "i32"));
                }
            }
        }
    }

    #[test]
    fn test_hello_world_program() {
        let source = r#"fn main() {
            let x = 10
            let y = 20
            if x + y > 25 {
                print("Big!")
            } else {
                print("Small!")
            }
        }"#;
        let module = parse_source(source);
        assert_eq!(module.items.len(), 1);
        if let Item::Function(f) = &module.items[0].node {
            if let Expr::Block { stmts, tail_expr } = &f.body.node {
                assert_eq!(stmts.len(), 2); // two let bindings
                assert!(tail_expr.is_some()); // if-else is tail expr
            }
        }
    }

    #[test]
    fn test_assignment() {
        let source = "fn main() { let mut x = 1\n x = 2 }";
        let module = parse_source(source);
        if let Item::Function(f) = &module.items[0].node {
            if let Expr::Block { stmts, tail_expr } = &f.body.node {
                assert_eq!(stmts.len(), 1); // let
                if let Some(tail) = tail_expr {
                    assert!(matches!(tail.node, Expr::Assign { .. }));
                }
            }
        }
    }

    #[test]
    fn test_compound_assignment() {
        let source = "fn main() { let mut x = 1\n x += 2 }";
        let module = parse_source(source);
        if let Item::Function(f) = &module.items[0].node {
            if let Expr::Block { stmts, tail_expr } = &f.body.node {
                assert_eq!(stmts.len(), 1);
                if let Some(tail) = tail_expr {
                    if let Expr::CompoundAssign { op, .. } = &tail.node {
                        assert_eq!(*op, BinOp::Add);
                    } else {
                        panic!("Expected CompoundAssign");
                    }
                }
            }
        }
    }

    #[test]
    fn test_generic_function() {
        let source = "fn identity<T>(x: T) -> T { x }";
        let module = parse_source(source);
        assert_eq!(module.items.len(), 1);
        if let Item::Function(f) = &module.items[0].node {
            assert_eq!(f.name, "identity");
            assert_eq!(f.type_params, vec!["T".to_string()]);
            assert_eq!(f.params.len(), 1);
            assert_eq!(f.params[0].name, "x");
            assert!(matches!(&f.params[0].ty.node, TypeExpr::Named(n) if n == "T"));
            assert!(matches!(&f.return_type.as_ref().unwrap().node, TypeExpr::Named(n) if n == "T"));
        }
    }

    #[test]
    fn test_non_generic_function_has_empty_type_params() {
        let source = "fn add(a: i64, b: i64) -> i64 { a + b }";
        let module = parse_source(source);
        if let Item::Function(f) = &module.items[0].node {
            assert!(f.type_params.is_empty());
        }
    }

    #[test]
    fn test_generic_function_with_program() {
        let source = r#"
            fn identity<T>(x: T) -> T { x }
            fn main() { print(identity(42)) }
        "#;
        let module = parse_source(source);
        assert_eq!(module.items.len(), 2);
    }
}
