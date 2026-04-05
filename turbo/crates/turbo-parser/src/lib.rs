use turbo_ast::*;
use turbo_lexer::{Spanned as LexSpanned, Token};

/// Maximum nesting depth for recursive constructs (blocks, if, while, for,
/// match, closures, parenthesised expressions, etc.). Exceeding this limit
/// produces a parse error instead of a stack overflow.
const MAX_NESTING_DEPTH: usize = 256;

/// A parse error with location info
#[derive(Debug, Clone)]
pub struct ParseError {
    pub code: ErrorCode,
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
    /// Current nesting depth; incremented on entry to recursive constructs.
    depth: usize,
}

impl Parser {
    fn new(tokens: Vec<LexSpanned<Token>>) -> Self {
        // Filter out newlines — Turbo doesn't use them for syntax
        let tokens: Vec<_> = tokens
            .into_iter()
            .filter(|t| !matches!(t.value, Token::Newline | Token::Semi))
            .collect();
        Self {
            tokens,
            pos: 0,
            errors: Vec::new(),
            depth: 0,
        }
    }

    /// Increment the nesting depth, returning an error if the limit is exceeded.
    fn enter_nesting(&mut self) -> Result<(), ParseError> {
        self.depth += 1;
        if self.depth > MAX_NESTING_DEPTH {
            let span = self.peek_span();
            return Err(ParseError {
                code: ErrorCode::E0003,
                message: format!("maximum nesting depth ({MAX_NESTING_DEPTH}) exceeded"),
                span,
            });
        }
        Ok(())
    }

    /// Decrement the nesting depth.
    fn exit_nesting(&mut self) {
        self.depth = self.depth.saturating_sub(1);
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
                code: ErrorCode::E0001,
                message: format!("expected `{expected}`, found `{tok}`"),
                span,
            })
        } else {
            let span = self.peek_span();
            Err(ParseError {
                code: ErrorCode::E0001,
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
        let found = self
            .peek()
            .map(|t| format!("`{t}`"))
            .unwrap_or("end of file".to_string());
        Err(ParseError {
            code: ErrorCode::E0001,
            message: format!("expected identifier, found {found}"),
            span,
        })
    }

    // === Top-level parsing ===

    /// Collect consecutive `///` doc comment tokens and return them as a single
    /// joined string. Returns `None` if no doc comments are present.
    fn collect_doc_comments(&mut self) -> Option<String> {
        let mut lines = Vec::new();
        while let Some(Token::DocComment(text)) = self.peek() {
            lines.push(text.clone());
            self.advance();
        }
        if lines.is_empty() {
            None
        } else {
            Some(lines.join("\n"))
        }
    }

    fn parse_module(&mut self) -> Module {
        let mut items = Vec::new();
        while !self.at_end() {
            // Collect doc comments before items
            let doc = self.collect_doc_comments();
            if self.at_end() {
                break;
            }
            match self.parse_item_with_doc(doc) {
                Ok(item) => items.push(item),
                Err(e) => {
                    self.errors.push(e);
                    // Skip to next item start to recover
                    self.pos += 1;
                    while !self.at_end()
                        && !matches!(
                            self.peek(),
                            Some(Token::Fn)
                                | Some(Token::Async)
                                | Some(Token::Struct)
                                | Some(Token::TypeKw)
                                | Some(Token::Import)
                                | Some(Token::Trait)
                                | Some(Token::Impl)
                                | Some(Token::Tool)
                                | Some(Token::Agent)
                                | Some(Token::Const)
                                | Some(Token::At)
                                | Some(Token::DocComment(_))
                        )
                    {
                        self.pos += 1;
                    }
                }
            }
        }
        Module { items }
    }

    fn parse_item_with_doc(&mut self, doc: Option<String>) -> Result<Spanned<Item>, ParseError> {
        let mut item = self.parse_item()?;
        // Attach doc comments to the appropriate item type
        if doc.is_some() {
            match &mut item.node {
                Item::Function(f) => f.doc = doc,
                Item::Struct(s) => s.doc = doc,
                Item::Enum(e) => e.doc = doc,
                _ => {} // other items don't support doc comments yet
            }
        }
        Ok(item)
    }

    fn parse_item(&mut self) -> Result<Spanned<Item>, ParseError> {
        let start = self.peek_span().start;
        match self.peek() {
            Some(Token::Fn) => {
                let f = self.parse_fn_def(false)?;
                let end = f.body.span.end;
                Ok(Spanned::new(Item::Function(f), start..end))
            }
            Some(Token::Async) => {
                self.advance(); // consume `async`
                self.expect(&Token::Fn)?;
                let mut f = self.parse_fn_def_inner()?;
                f.is_async = true;
                let end = f.body.span.end;
                Ok(Spanned::new(Item::Function(f), start..end))
            }
            Some(Token::Struct) => {
                let s = self.parse_struct_def()?;
                let end = self
                    .tokens
                    .get(self.pos.saturating_sub(1))
                    .map(|t| t.span.end)
                    .unwrap_or(start);
                Ok(Spanned::new(Item::Struct(s), start..end))
            }
            Some(Token::TypeKw) => {
                let e = self.parse_enum_def()?;
                let end = self.tokens[self.pos - 1].span.end;
                Ok(Spanned::new(Item::Enum(e), start..end))
            }
            Some(Token::Impl) => {
                let imp = self.parse_impl_block()?;
                let end = self
                    .tokens
                    .get(self.pos.saturating_sub(1))
                    .map(|t| t.span.end)
                    .unwrap_or(start);
                Ok(Spanned::new(Item::Impl(imp), start..end))
            }
            Some(Token::Trait) => {
                let t = self.parse_trait_def()?;
                let end = self
                    .tokens
                    .get(self.pos.saturating_sub(1))
                    .map(|t| t.span.end)
                    .unwrap_or(start);
                Ok(Spanned::new(Item::Trait(t), start..end))
            }
            Some(Token::Tool) => {
                self.advance(); // consume 'tool'
                let mut f = self.parse_fn_def(false)?;
                f.is_tool = true;
                let end = f.body.span.end;
                Ok(Spanned::new(Item::Function(f), start..end))
            }
            Some(Token::Agent) => {
                self.advance(); // consume 'agent'
                let (name, _) = self.expect_ident()?;
                self.expect(&Token::LBrace)?;

                let mut model = String::new();
                let mut tools = Vec::new();
                let mut system = None;

                while !matches!(self.peek(), Some(Token::RBrace) | None) {
                    let (key, key_span) = self.expect_ident()?;
                    self.expect(&Token::Colon)?;
                    match key.as_str() {
                        "model" => {
                            if let Some(Token::String(s)) = self.peek() {
                                model = s.clone();
                                self.advance();
                            } else {
                                let span = self.peek_span();
                                return Err(ParseError {
                                    code: ErrorCode::E0001,
                                    message: "expected string literal for `model`".to_string(),
                                    span,
                                });
                            }
                        }
                        "tools" => {
                            self.expect(&Token::LBracket)?;
                            while !matches!(self.peek(), Some(Token::RBracket) | None) {
                                let (tool_name, _) = self.expect_ident()?;
                                tools.push(tool_name);
                                if matches!(self.peek(), Some(Token::Comma)) {
                                    self.advance();
                                }
                            }
                            self.expect(&Token::RBracket)?;
                        }
                        "system" => {
                            if let Some(Token::String(s)) = self.peek() {
                                system = Some(s.clone());
                                self.advance();
                            } else {
                                let span = self.peek_span();
                                return Err(ParseError {
                                    code: ErrorCode::E0001,
                                    message: "expected string literal for `system`".to_string(),
                                    span,
                                });
                            }
                        }
                        _ => {
                            return Err(ParseError {
                                code: ErrorCode::E0001,
                                message: format!("unknown agent field `{key}`"),
                                span: key_span,
                            });
                        }
                    }
                }

                let end = self.peek_span().end;
                self.expect(&Token::RBrace)?;

                Ok(Spanned::new(
                    Item::Agent(AgentDef {
                        name,
                        model,
                        tools,
                        system_prompt: system,
                    }),
                    start..end,
                ))
            }
            Some(Token::Import) => {
                let (names, path) = self.parse_import()?;
                let end = self
                    .tokens
                    .get(self.pos.saturating_sub(1))
                    .map(|t| t.span.end)
                    .unwrap_or(start);
                Ok(Spanned::new(Item::Import { names, path }, start..end))
            }
            Some(Token::Const) => {
                self.advance(); // consume `const`
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
                    Item::Const(ConstDef { name, ty, value }),
                    start..end,
                ))
            }
            Some(Token::At) => {
                self.advance(); // consume `@`
                let (attr_name, attr_span) = self.expect_ident()?;
                match attr_name.as_str() {
                    "derive" => {
                        self.expect(&Token::LParen)?;
                        let mut traits = Vec::new();
                        loop {
                            let (name, _) = self.expect_ident()?;
                            traits.push(name);
                            if matches!(self.peek(), Some(Token::Comma)) {
                                self.advance();
                            } else {
                                break;
                            }
                        }
                        self.expect(&Token::RParen)?;
                        let mut s = self.parse_struct_def()?;
                        s.derives = traits;
                        let end = self
                            .tokens
                            .get(self.pos.saturating_sub(1))
                            .map(|t| t.span.end)
                            .unwrap_or(start);
                        Ok(Spanned::new(Item::Struct(s), start..end))
                    }
                    "test" => {
                        let mut f = self.parse_fn_def(false)?;
                        f.is_test = true;
                        let end = f.body.span.end;
                        Ok(Spanned::new(Item::Function(f), start..end))
                    }
                    "unsafe" => {
                        let mut f = self.parse_fn_def(false)?;
                        f.is_unsafe = true;
                        let end = f.body.span.end;
                        Ok(Spanned::new(Item::Function(f), start..end))
                    }
                    _ => Err(ParseError {
                        code: ErrorCode::E0001,
                        message: format!("unknown attribute `@{attr_name}`"),
                        span: attr_span,
                    }),
                }
            }
            _ => {
                let span = self.peek_span();
                let found = self
                    .peek()
                    .map(|t| format!("`{t}`"))
                    .unwrap_or("end of file".to_string());
                Err(ParseError {
                    code: ErrorCode::E0001,
                    message: format!("expected `fn`, `tool`, `agent`, `async fn`, `struct`, `type`, `impl`, `trait`, `import`, `const`, `@derive`, `@test`, or `@unsafe`, found {found}"),
                    span,
                })
            }
        }
    }

    fn parse_struct_def(&mut self) -> Result<StructDef, ParseError> {
        self.expect(&Token::Struct)?;
        let (name, _) = self.expect_ident()?;
        let type_params = self.parse_optional_type_params()?;
        self.expect(&Token::LBrace)?;
        let mut fields = Vec::new();
        while !matches!(self.peek(), Some(Token::RBrace) | None) {
            let (field_name, _) = self.expect_ident()?;
            self.expect(&Token::Colon)?;
            let ty = self.parse_type()?;
            fields.push(FieldDef {
                name: field_name,
                ty,
            });
            // Optional comma
            if matches!(self.peek(), Some(Token::Comma)) {
                self.advance();
            }
        }
        self.expect(&Token::RBrace)?;
        Ok(StructDef {
            name,
            type_params,
            derives: Vec::new(),
            fields,
            doc: None,
        })
    }

    fn parse_enum_def(&mut self) -> Result<EnumDef, ParseError> {
        self.expect(&Token::TypeKw)?;
        let (name, _) = self.expect_ident()?;
        let type_params = self.parse_optional_type_params()?;
        self.expect(&Token::LBrace)?;
        let mut variants = Vec::new();
        while !matches!(self.peek(), Some(Token::RBrace) | None) {
            let (variant_name, _) = self.expect_ident()?;
            // Check for data-carrying variant: VariantName(Type1, Type2, ...)
            let fields = if matches!(self.peek(), Some(Token::LParen)) {
                self.advance(); // consume (
                let mut fields = Vec::new();
                if !matches!(self.peek(), Some(Token::RParen)) {
                    loop {
                        let ty = self.parse_type()?;
                        fields.push(ty);
                        if matches!(self.peek(), Some(Token::Comma)) {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                }
                self.expect(&Token::RParen)?;
                fields
            } else {
                Vec::new()
            };
            variants.push(EnumVariantDef {
                name: variant_name,
                fields,
            });
            if matches!(self.peek(), Some(Token::Comma)) {
                self.advance();
            }
        }
        self.expect(&Token::RBrace)?;
        Ok(EnumDef {
            name,
            type_params,
            variants,
            doc: None,
        })
    }

    fn parse_impl_block(&mut self) -> Result<ImplBlock, ParseError> {
        self.expect(&Token::Impl)?;
        let (first_name, _) = self.expect_ident()?;
        let first_type_params = self.parse_optional_type_params()?;

        // Check for `impl TraitName for TypeName<T> { ... }`
        let (trait_name, type_name, type_params) = if matches!(self.peek(), Some(Token::For)) {
            self.advance(); // consume `for`
            let (tn, _) = self.expect_ident()?;
            let tp = self.parse_optional_type_params()?;
            (Some(first_name), tn, tp)
        } else {
            (None, first_name, first_type_params)
        };

        self.expect(&Token::LBrace)?;
        let mut methods = Vec::new();
        while !matches!(self.peek(), Some(Token::RBrace) | None) {
            let start = self.peek_span().start;
            let f = self.parse_fn_def(false)?;
            let end = f.body.span.end;
            methods.push(Spanned::new(f, start..end));
        }
        self.expect(&Token::RBrace)?;
        Ok(ImplBlock {
            type_name,
            type_params,
            trait_name,
            methods,
        })
    }

    fn parse_trait_def(&mut self) -> Result<TraitDef, ParseError> {
        self.expect(&Token::Trait)?;
        let (name, _) = self.expect_ident()?;
        self.expect(&Token::LBrace)?;
        let mut methods = Vec::new();
        while !matches!(self.peek(), Some(Token::RBrace) | None) {
            methods.push(self.parse_trait_method_sig()?);
        }
        self.expect(&Token::RBrace)?;
        Ok(TraitDef { name, methods })
    }

    fn parse_trait_method_sig(&mut self) -> Result<TraitMethodSig, ParseError> {
        self.expect(&Token::Fn)?;
        let (name, _) = self.expect_ident()?;
        self.expect(&Token::LParen)?;
        let params = self.parse_params()?;
        self.expect(&Token::RParen)?;
        let return_type = if matches!(self.peek(), Some(Token::Arrow)) {
            self.advance();
            Some(self.parse_type()?)
        } else {
            None
        };
        // Check for optional default body
        let default_body = if matches!(self.peek(), Some(Token::LBrace)) {
            Some(self.parse_block()?)
        } else {
            None
        };
        Ok(TraitMethodSig {
            name,
            params,
            return_type,
            default_body,
        })
    }

    fn parse_import(&mut self) -> Result<(Vec<String>, String), ParseError> {
        self.expect(&Token::Import)?;
        self.expect(&Token::LBrace)?;
        let mut names = Vec::new();
        loop {
            let (name, _) = self.expect_ident()?;
            names.push(name);
            if matches!(self.peek(), Some(Token::Comma)) {
                self.advance();
            } else {
                break;
            }
        }
        self.expect(&Token::RBrace)?;
        self.expect(&Token::From)?;
        let path = match self.peek() {
            Some(Token::String(_)) => {
                let tok = self.advance();
                if let Token::String(s) = &tok.value {
                    s.clone()
                } else {
                    unreachable!()
                }
            }
            _ => {
                return Err(ParseError {
                    code: ErrorCode::E0001,
                    message: "expected path string after `from`".to_string(),
                    span: self.peek_span(),
                });
            }
        };
        Ok((names, path))
    }

    fn parse_fn_def(&mut self, is_async: bool) -> Result<FnDef, ParseError> {
        self.expect(&Token::Fn)?;
        let mut f = self.parse_fn_def_inner()?;
        f.is_async = is_async;
        Ok(f)
    }

    /// Parse optional type parameters: `<T>` or `<T, U, ...>` or `<T: Trait, U: Trait>`
    fn parse_optional_type_params(&mut self) -> Result<Vec<TypeParam>, ParseError> {
        if matches!(self.peek(), Some(Token::Less)) {
            self.advance(); // consume <
            let mut params = Vec::new();
            loop {
                let (tp_name, _) = self.expect_ident()?;
                // Check for trait bounds: T: Display
                let bounds = if matches!(self.peek(), Some(Token::Colon)) {
                    self.advance(); // consume :
                    let mut bounds = Vec::new();
                    let (bound_name, _) = self.expect_ident()?;
                    bounds.push(bound_name);
                    // Support multiple bounds with + (future extension)
                    while matches!(self.peek(), Some(Token::Plus)) {
                        self.advance();
                        let (bound_name, _) = self.expect_ident()?;
                        bounds.push(bound_name);
                    }
                    bounds
                } else {
                    vec![]
                };
                params.push(TypeParam::with_bounds(tp_name, bounds));
                if matches!(self.peek(), Some(Token::Comma)) {
                    self.advance();
                } else {
                    break;
                }
            }
            self.expect(&Token::Greater)?;
            Ok(params)
        } else {
            Ok(Vec::new())
        }
    }

    /// Parses the function definition after `fn` has already been consumed.
    fn parse_fn_def_inner(&mut self) -> Result<FnDef, ParseError> {
        self.enter_nesting()?;
        let (name, _) = self.expect_ident()?;

        let type_params = self.parse_optional_type_params()?;

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

        self.exit_nesting();
        Ok(FnDef {
            name,
            is_async: false,
            is_tool: false,
            is_test: false,
            is_unsafe: false,
            type_params,
            params,
            return_type,
            body,
            doc: None,
        })
    }

    fn parse_params(&mut self) -> Result<Vec<Param>, ParseError> {
        let mut params = Vec::new();
        if matches!(self.peek(), Some(Token::RParen)) {
            return Ok(params);
        }

        loop {
            let start = self.peek_span().start;
            let (name, name_span) = self.expect_ident()?;

            // Special case: bare `self` parameter (no type annotation)
            if name == "self" && !matches!(self.peek(), Some(Token::Colon)) {
                let end = name_span.end;
                // Use a placeholder type; sema/codegen will fill in the real struct type
                let ty = Spanned::new(TypeExpr::Named("Self".to_string()), name_span.clone());
                params.push(Param {
                    name,
                    ty,
                    span: start..end,
                });
            } else {
                self.expect(&Token::Colon)?;
                let ty = self.parse_type()?;
                let end = ty.span.end;
                params.push(Param {
                    name,
                    ty,
                    span: start..end,
                });
            }

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

        // Array type [T]
        if matches!(self.peek(), Some(Token::LBracket)) {
            self.advance();
            let elem_type = self.parse_type()?;
            let end = self.peek_span().end;
            self.expect(&Token::RBracket)?;
            return Ok(Spanned::new(
                TypeExpr::Array(Box::new(elem_type)),
                start..end,
            ));
        }

        // Function type: fn(T, T) -> T
        if matches!(self.peek(), Some(Token::Fn)) {
            self.advance();
            self.expect(&Token::LParen)?;
            let mut params = Vec::new();
            if !matches!(self.peek(), Some(Token::RParen)) {
                loop {
                    params.push(self.parse_type()?);
                    if matches!(self.peek(), Some(Token::Comma)) {
                        self.advance();
                    } else {
                        break;
                    }
                }
            }
            self.expect(&Token::RParen)?;
            self.expect(&Token::Arrow)?;
            let ret = self.parse_type()?;
            let end = ret.span.end;
            return Ok(Spanned::new(
                TypeExpr::FnType {
                    params,
                    ret: Box::new(ret),
                },
                start..end,
            ));
        }

        // Named type
        let (name, span) = self.expect_ident()?;
        let mut end = span.end;

        // Parse generic type arguments: Pair<A, B>, Result<T, E>, etc.
        if matches!(self.peek(), Some(Token::Less)) {
            self.advance(); // <
            loop {
                let _type_arg = self.parse_type()?;
                if matches!(self.peek(), Some(Token::Comma)) {
                    self.advance();
                } else {
                    break;
                }
            }
            end = self.peek_span().end;
            self.expect(&Token::Greater)?;
        }

        let mut ty = Spanned::new(TypeExpr::Named(name), start..end);

        // Check for result type: T ! E
        if matches!(self.peek(), Some(Token::Bang)) {
            self.advance();
            let err_ty = self.parse_type()?;
            let end = err_ty.span.end;
            ty = Spanned::new(
                TypeExpr::Result {
                    ok_type: Box::new(ty),
                    err_type: Box::new(err_ty),
                },
                start..end,
            );
        }

        // Check for optional type: T?
        if matches!(self.peek(), Some(Token::Question)) {
            let end = self.peek_span().end;
            self.advance();
            ty = Spanned::new(TypeExpr::Optional(Box::new(ty)), start..end);
        }

        Ok(ty)
    }

    // === Block ===

    fn parse_map_literal(&mut self) -> Result<Spanned<Expr>, ParseError> {
        let start = self.peek_span().start;
        self.expect(&Token::LBrace)?;
        let mut entries = Vec::new();
        while !matches!(self.peek(), Some(Token::RBrace) | None) {
            let key = self.parse_expr()?;
            self.expect(&Token::Colon)?;
            let value = self.parse_expr()?;
            entries.push((key, value));
            if matches!(self.peek(), Some(Token::Comma)) {
                self.advance();
            }
        }
        let end = self.peek_span().end;
        self.expect(&Token::RBrace)?;
        Ok(Spanned::new(Expr::MapLit(entries), start..end))
    }

    fn parse_block(&mut self) -> Result<Spanned<Expr>, ParseError> {
        self.enter_nesting()?;
        let start = self.peek_span().start;
        self.expect(&Token::LBrace)?;

        let mut stmts = Vec::new();
        let mut tail_expr = None;

        while !matches!(self.peek(), Some(Token::RBrace) | None) {
            // Try to parse a statement (let, return, defer)
            if matches!(self.peek(), Some(Token::Let)) {
                let stmt = self.parse_let_stmt()?;
                stmts.push(stmt);
            } else if matches!(self.peek(), Some(Token::Return)) {
                let stmt = self.parse_return_stmt()?;
                stmts.push(stmt);
            } else if matches!(self.peek(), Some(Token::Defer)) {
                let stmt = self.parse_defer_stmt()?;
                stmts.push(stmt);
            } else {
                // Parse an expression
                let expr = self.parse_expr()?;

                // If followed by RBrace, this is the tail expression
                if matches!(self.peek(), Some(Token::RBrace)) {
                    // If the tail expr is a COW builtin call, convert it to a
                    // statement with auto-reassign instead. The block returns Unit.
                    if Self::is_cow_builtin_call(&expr) {
                        let span = expr.span.clone();
                        let stmt = Self::maybe_rewrite_cow_stmt(expr);
                        stmts.push(Spanned::new(stmt, span));
                    } else {
                        tail_expr = Some(Box::new(expr));
                    }
                } else {
                    // It's a statement expression — check for COW builtin auto-reassign.
                    // Rewrites e.g. `push(items, 4)` → `items = push(items, 4)`
                    // This handles all control flow correctly because Assign is
                    // already compiled with proper SSA phi nodes.
                    let span = expr.span.clone();
                    let stmt = Self::maybe_rewrite_cow_stmt(expr);
                    stmts.push(Spanned::new(stmt, span));
                }
            }
        }

        let end = self.peek_span().end;
        self.expect(&Token::RBrace)?;

        self.exit_nesting();
        Ok(Spanned::new(Expr::Block { stmts, tail_expr }, start..end))
    }

    /// COW builtins that return a new value instead of mutating in-place.
    /// When called in statement position via UFCS (e.g. `items.push(4)`),
    /// the parser rewrites to an assignment so the result isn't silently discarded.
    const COW_BUILTINS: &'static [&'static str] = &[
        "push", "map", "filter", "replace", "upper", "lower", "trim", "repeat", "split",
    ];

    /// If `expr` is a call to a COW builtin in statement position, rewrite it
    /// to an assignment back to the source variable. Otherwise return Stmt::Expr.
    ///
    /// `push(items, 4)` → `items = push(items, 4)`  (Assign)
    /// `push(b.items, 4)` → `b.items = push(b.items, 4)` (FieldAssign)
    /// `push(arr[0], 4)` → `arr[0] = push(arr[0], 4)` (IndexAssign)
    /// Check if an expression is a call to a COW builtin.
    fn is_cow_builtin_call(expr: &Spanned<Expr>) -> bool {
        if let Expr::Call {
            ref callee,
            ref args,
        } = expr.node
        {
            if let Expr::Ident(ref fn_name) = callee.node {
                return Self::COW_BUILTINS.contains(&fn_name.as_str()) && !args.is_empty();
            }
        }
        false
    }

    fn maybe_rewrite_cow_stmt(expr: Spanned<Expr>) -> Stmt {
        if let Expr::Call {
            ref callee,
            ref args,
        } = expr.node
        {
            if let Expr::Ident(ref fn_name) = callee.node {
                if Self::COW_BUILTINS.contains(&fn_name.as_str()) && !args.is_empty() {
                    let target_arg = &args[0];
                    let span = expr.span.clone();

                    // Simple variable: push(items, 4) → items = push(items, 4)
                    if let Expr::Ident(ref var_name) = target_arg.node {
                        return Stmt::Expr(Spanned::new(
                            Expr::Assign {
                                target: var_name.clone(),
                                value: Box::new(expr),
                            },
                            span,
                        ));
                    }

                    // Field access: push(b.items, 4) → b.items = push(b.items, 4)
                    if let Expr::FieldAccess {
                        ref object,
                        ref field,
                    } = target_arg.node
                    {
                        return Stmt::Expr(Spanned::new(
                            Expr::FieldAssign {
                                object: object.clone(),
                                field: field.clone(),
                                value: Box::new(expr),
                            },
                            span,
                        ));
                    }

                    // Index access: push(arr[0], 4) → arr[0] = push(arr[0], 4)
                    if let Expr::Index {
                        ref object,
                        ref index,
                    } = target_arg.node
                    {
                        return Stmt::Expr(Spanned::new(
                            Expr::IndexAssign {
                                object: object.clone(),
                                index: index.clone(),
                                value: Box::new(expr),
                            },
                            span,
                        ));
                    }
                }
            }
        }
        Stmt::Expr(expr)
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

        // Check for struct destructuring: let { field1, field2 } = expr
        if matches!(self.peek(), Some(Token::LBrace)) {
            self.advance(); // consume '{'
            let mut fields = Vec::new();
            loop {
                if matches!(self.peek(), Some(Token::RBrace)) {
                    break;
                }
                let (field_name, _) = self.expect_ident()?;
                fields.push(field_name);
                if matches!(self.peek(), Some(Token::Comma)) {
                    self.advance();
                } else {
                    break;
                }
            }
            self.expect(&Token::RBrace)?;
            self.expect(&Token::Eq)?;
            let value = self.parse_expr()?;
            let end = value.span.end;
            return Ok(Spanned::new(
                Stmt::LetDestructure {
                    mutable,
                    fields,
                    value,
                },
                start..end,
            ));
        }

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

    fn parse_defer_stmt(&mut self) -> Result<Spanned<Stmt>, ParseError> {
        let start = self.peek_span().start;
        self.expect(&Token::Defer)?;
        let expr = self.parse_expr()?;
        let end = expr.span.end;
        Ok(Spanned::new(Stmt::Defer(expr), start..end))
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

        // Check for field assignment: expr.field = value
        if let Expr::FieldAccess { .. } = &lhs.node {
            if matches!(self.peek(), Some(Token::Eq)) {
                self.advance();
                let value = self.parse_expr()?;
                let span = lhs.span.start..value.span.end;
                if let Expr::FieldAccess { object, field } = lhs.node {
                    return Ok(Spanned::new(
                        Expr::FieldAssign {
                            object,
                            field,
                            value: Box::new(value),
                        },
                        span,
                    ));
                }
            }
        }

        // Check for index assignment: expr[index] = value
        if let Expr::Index { .. } = &lhs.node {
            if matches!(self.peek(), Some(Token::Eq)) {
                self.advance();
                let value = self.parse_expr()?;
                let span = lhs.span.start..value.span.end;
                if let Expr::Index { object, index } = lhs.node {
                    return Ok(Spanned::new(
                        Expr::IndexAssign {
                            object,
                            index,
                            value: Box::new(value),
                        },
                        span,
                    ));
                }
            }
        }

        // Check for range operator (..)
        if matches!(self.peek(), Some(Token::DotDot)) {
            self.advance();
            let rhs = self.parse_unary()?;
            let span = lhs.span.start..rhs.span.end;
            return Ok(Spanned::new(
                Expr::Range {
                    start: Box::new(lhs),
                    end: Box::new(rhs),
                },
                span,
            ));
        }

        let mut result = self.parse_binary(lhs, 0)?;

        // Null coalescing operator ?? (low precedence, left-associative)
        while matches!(self.peek(), Some(Token::QuestionQuestion)) {
            self.advance(); // consume ??
            let rhs_start = self.parse_unary()?;
            let rhs = self.parse_binary(rhs_start, 0)?;
            let span = result.span.start..rhs.span.end;
            result = Spanned::new(
                Expr::NullCoalesce {
                    value: Box::new(result),
                    default: Box::new(rhs),
                },
                span,
            );
        }

        // Pipe operator |> (lowest precedence, left-associative)
        // Desugars: `a |> f` => `f(a)`, `a |> f(b, c)` => `f(a, b, c)`
        while matches!(self.peek(), Some(Token::Pipe)) {
            self.advance(); // consume |>
                            // Parse RHS at full binary precedence (everything binds tighter than pipe)
            let rhs_start = self.parse_unary()?;
            let rhs = self.parse_binary(rhs_start, 0)?;
            // Desugar based on RHS shape
            let result_start = result.span.start;
            match rhs.node {
                Expr::Call { callee, mut args } => {
                    // a |> f(b, c) => f(a, b, c)
                    let end = args.last().map(|a| a.span.end).unwrap_or(callee.span.end);
                    args.insert(0, result);
                    let span = result_start..end;
                    result = Spanned::new(Expr::Call { callee, args }, span);
                }
                Expr::Ident(_) => {
                    // a |> f => f(a)
                    let span = result_start..rhs.span.end;
                    result = Spanned::new(
                        Expr::Call {
                            callee: Box::new(rhs),
                            args: vec![result],
                        },
                        span,
                    );
                }
                _ => {
                    return Err(ParseError {
                        code: ErrorCode::E0001,
                        message: "pipe operator `|>` requires a function name or function call on the right side".to_string(),
                        span: rhs.span,
                    });
                }
            }
        }

        Ok(result)
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

    fn parse_binary(
        &mut self,
        mut lhs: Spanned<Expr>,
        min_prec: u8,
    ) -> Result<Spanned<Expr>, ParseError> {
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

        if matches!(self.peek(), Some(Token::Await)) {
            self.advance();
            let expr = self.parse_unary()?;
            let end = expr.span.end;
            return Ok(Spanned::new(Expr::Await(Box::new(expr)), start..end));
        }

        if matches!(self.peek(), Some(Token::Spawn)) {
            self.advance();
            let expr = self.parse_unary()?;
            let end = expr.span.end;
            return Ok(Spanned::new(Expr::Spawn(Box::new(expr)), start..end));
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
            } else if matches!(self.peek(), Some(Token::LBracket)) {
                // Index expression: expr[index]
                self.advance(); // consume [
                let index = self.parse_expr()?;
                let end = self.peek_span().end;
                self.expect(&Token::RBracket)?;
                let span = expr.span.start..end;
                expr = Spanned::new(
                    Expr::Index {
                        object: Box::new(expr),
                        index: Box::new(index),
                    },
                    span,
                );
            } else if matches!(self.peek(), Some(Token::QuestionDot)) {
                // Optional chaining: expr?.field
                self.advance(); // consume `?.`
                let (field, field_span) = self.expect_ident()?;
                let span = expr.span.start..field_span.end;
                expr = Spanned::new(
                    Expr::OptionalChain {
                        object: Box::new(expr),
                        field,
                    },
                    span,
                );
            } else if matches!(self.peek(), Some(Token::Dot)) {
                // Dot access: method call, field access, or enum variant (resolved in sema)
                self.advance(); // consume .
                let (name, name_span) = self.expect_ident()?;

                if matches!(self.peek(), Some(Token::LParen)) {
                    // Method-style call: expr.method(args) => method(expr, args)
                    let expr_start = expr.span.start;
                    self.advance(); // consume (
                    let mut args = self.parse_call_args()?;
                    let end = self.peek_span().end;
                    self.expect(&Token::RParen)?;
                    args.insert(0, expr);
                    let callee = Spanned::new(Expr::Ident(name), name_span);
                    let span = expr_start..end;
                    expr = Spanned::new(
                        Expr::Call {
                            callee: Box::new(callee),
                            args,
                        },
                        span,
                    );
                } else {
                    // Field access or enum variant (existing behavior)
                    let span = expr.span.start..name_span.end;
                    expr = Spanned::new(
                        Expr::FieldAccess {
                            object: Box::new(expr),
                            field: name,
                        },
                        span,
                    );
                }
            } else if let Expr::Ident(ref name) = expr.node {
                if matches!(self.peek(), Some(Token::LBrace)) {
                    // Possible struct literal: Name { field: value, ... }
                    // Disambiguate: if after `{` we see `ident :`, it's a struct literal
                    let save_pos = self.pos;
                    self.advance(); // consume {
                    let is_struct_lit = if matches!(self.peek(), Some(Token::Ident(_))) {
                        let save_pos2 = self.pos;
                        self.advance(); // consume ident
                        let result = matches!(self.peek(), Some(Token::Colon));
                        self.pos = save_pos2;
                        result
                    } else if matches!(self.peek(), Some(Token::RBrace)) {
                        // Empty struct literal: Name {}
                        true
                    } else {
                        false
                    };
                    self.pos = save_pos; // backtrack

                    if is_struct_lit {
                        let struct_name = name.clone();
                        let start = expr.span.start;
                        expr = self.parse_struct_lit(struct_name, start)?;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            } else if matches!(self.peek(), Some(Token::Question)) {
                // Try operator: expr? — unwraps Ok, propagates Err
                let q_span = self.tokens[self.pos].span.clone();
                self.advance(); // consume ?
                let span = expr.span.start..q_span.end;
                expr = Spanned::new(Expr::Try(Box::new(expr)), span);
            } else {
                break;
            }
        }

        Ok(expr)
    }

    fn parse_struct_lit(
        &mut self,
        name: String,
        start: usize,
    ) -> Result<Spanned<Expr>, ParseError> {
        self.expect(&Token::LBrace)?;
        let mut fields = Vec::new();
        while !matches!(self.peek(), Some(Token::RBrace) | None) {
            let (field_name, _) = self.expect_ident()?;
            self.expect(&Token::Colon)?;
            let value = self.parse_expr()?;
            fields.push((field_name, value));
            if matches!(self.peek(), Some(Token::Comma)) {
                self.advance();
            }
        }
        let end = self.peek_span().end;
        self.expect(&Token::RBrace)?;
        Ok(Spanned::new(Expr::StructLit { name, fields }, start..end))
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
                // Allow trailing comma before `)`
                if matches!(self.peek(), Some(Token::RParen)) {
                    break;
                }
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
                    // Check for string interpolation: unescaped { inside the string
                    if has_unescaped_brace(&s) {
                        self.parse_interpolation(&s, &span)
                    } else {
                        // Replace escaped braces with literal braces
                        let s = unescape_braces(&s);
                        Ok(Spanned::new(Expr::StringLit(s), span))
                    }
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
                // Could be: arrow closure `(params) => body`, parenthesized expr, or unit `()`
                // Lookahead: scan to matching `)`, then check for `=>`
                let save_pos = self.pos;
                self.advance(); // consume `(`
                let mut could_be_arrow = true;
                let mut depth: usize = 1;

                // Scan forward to find matching `)`
                while depth > 0 {
                    match self.peek() {
                        Some(Token::LParen) => {
                            depth += 1;
                            self.advance();
                        }
                        Some(Token::RParen) => {
                            depth -= 1;
                            if depth > 0 {
                                self.advance();
                            }
                        }
                        None => {
                            could_be_arrow = false;
                            break;
                        }
                        _ => {
                            self.advance();
                        }
                    }
                }

                if could_be_arrow && depth == 0 {
                    self.advance(); // consume the matching `)`
                    if matches!(self.peek(), Some(Token::FatArrow)) {
                        // It's an arrow closure — backtrack and parse properly
                        self.pos = save_pos;
                        return self.parse_arrow_closure();
                    }
                }

                // Not an arrow closure — backtrack and parse as paren expr / unit
                self.pos = save_pos;
                self.enter_nesting()?;
                self.advance(); // consume `(`
                if matches!(self.peek(), Some(Token::RParen)) {
                    let end = self.peek_span().end;
                    self.advance();
                    self.exit_nesting();
                    return Ok(Spanned::new(Expr::Unit, start..end));
                }
                let expr = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                self.exit_nesting();
                Ok(expr)
            }
            Some(Token::LBracket) => {
                // Array literal: [expr, expr, ...]
                self.enter_nesting()?;
                self.advance(); // consume [
                let mut elements = Vec::new();
                if !matches!(self.peek(), Some(Token::RBracket)) {
                    loop {
                        elements.push(self.parse_expr()?);
                        if matches!(self.peek(), Some(Token::Comma)) {
                            self.advance();
                            // Allow trailing comma before `]`
                            if matches!(self.peek(), Some(Token::RBracket)) {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                }
                let end = self.peek_span().end;
                self.expect(&Token::RBracket)?;
                self.exit_nesting();
                Ok(Spanned::new(Expr::ArrayLit(elements), start..end))
            }
            Some(Token::Ok) => {
                let start = self.advance().span.start;
                self.expect(&Token::LParen)?;
                let value = self.parse_expr()?;
                let end = self.peek_span().end;
                self.expect(&Token::RParen)?;
                Ok(Spanned::new(Expr::OkExpr(Box::new(value)), start..end))
            }
            Some(Token::Err) => {
                let start = self.advance().span.start;
                self.expect(&Token::LParen)?;
                let value = self.parse_expr()?;
                let end = self.peek_span().end;
                self.expect(&Token::RParen)?;
                Ok(Spanned::new(Expr::ErrExpr(Box::new(value)), start..end))
            }
            Some(Token::Some) => {
                let start = self.advance().span.start;
                self.expect(&Token::LParen)?;
                let value = self.parse_expr()?;
                let end = self.peek_span().end;
                self.expect(&Token::RParen)?;
                Ok(Spanned::new(Expr::SomeExpr(Box::new(value)), start..end))
            }
            Some(Token::None) => {
                let span = self.advance().span.clone();
                Ok(Spanned::new(Expr::NoneExpr, span))
            }
            Some(Token::If) => self.parse_if_expr(),
            Some(Token::While) => self.parse_while_expr(),
            Some(Token::For) => self.parse_for_in(),
            Some(Token::Match) => self.parse_match_expr(),
            Some(Token::LBrace) => {
                // Disambiguate: map literal vs block
                let save_pos = self.pos;
                let start = self.peek_span().start;
                self.advance(); // consume {

                // Empty braces = empty map
                if matches!(self.peek(), Some(Token::RBrace)) {
                    let end = self.peek_span().end;
                    self.advance(); // consume }
                    return Ok(Spanned::new(Expr::MapLit(vec![]), start..end));
                }

                // Check if it's a map literal: string literal followed by ':'
                let is_map = if matches!(self.peek(), Some(Token::String(_))) {
                    let save2 = self.pos;
                    self.advance(); // consume string
                    let result = matches!(self.peek(), Some(Token::Colon));
                    self.pos = save2; // backtrack
                    result
                } else {
                    false
                };

                self.pos = save_pos; // backtrack to before {

                if is_map {
                    self.parse_map_literal()
                } else {
                    self.parse_block()
                }
            }
            Some(Token::Break) => {
                let span = self.advance().span.clone();
                Ok(Spanned::new(Expr::Break, span))
            }
            Some(Token::Continue) => {
                let span = self.advance().span.clone();
                Ok(Spanned::new(Expr::Continue, span))
            }
            Some(Token::Bar) => {
                // Closure: |params| -> ret { body }
                self.parse_closure()
            }
            Some(Token::Or) => {
                // Empty-param closure: || -> ret { body }
                // But only if followed by Arrow or LBrace (otherwise it's a binary Or)
                let save_pos = self.pos;
                self.advance(); // consume ||
                if matches!(self.peek(), Some(Token::Arrow) | Some(Token::LBrace)) {
                    // It's an empty closure
                    let return_type = if matches!(self.peek(), Some(Token::Arrow)) {
                        self.advance();
                        Some(self.parse_type()?)
                    } else {
                        None
                    };
                    let body = self.parse_block()?;
                    let end = body.span.end;
                    Ok(Spanned::new(
                        Expr::Closure {
                            params: Vec::new(),
                            return_type,
                            body: Box::new(body),
                        },
                        start..end,
                    ))
                } else {
                    // Not a closure, backtrack -- it was binary Or
                    self.pos = save_pos;
                    let span = self.peek_span();
                    let found = self
                        .peek()
                        .map(|t| format!("`{t}`"))
                        .unwrap_or("end of file".to_string());
                    Err(ParseError {
                        code: ErrorCode::E0001,
                        message: format!("expected expression, found {found}"),
                        span,
                    })
                }
            }
            _ => {
                let span = self.peek_span();
                let found = self
                    .peek()
                    .map(|t| format!("`{t}`"))
                    .unwrap_or("end of file".to_string());
                Err(ParseError {
                    code: ErrorCode::E0001,
                    message: format!("expected expression, found {found}"),
                    span,
                })
            }
        }
    }

    fn parse_if_expr(&mut self) -> Result<Spanned<Expr>, ParseError> {
        self.enter_nesting()?;
        let start = self.peek_span().start;
        self.expect(&Token::If)?;

        // Check for `if let` pattern matching
        if matches!(self.peek(), Some(Token::Let)) {
            self.advance(); // consume `let`
            let pattern = self.parse_pattern()?;
            self.expect(&Token::Eq)?;
            let value = self.parse_expr()?;
            let then_branch = self.parse_block()?;

            let else_branch = if matches!(self.peek(), Some(Token::Else)) {
                self.advance();
                if matches!(self.peek(), Some(Token::If)) {
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

            self.exit_nesting();
            return Ok(Spanned::new(
                Expr::IfLet {
                    pattern: Box::new(pattern),
                    value: Box::new(value),
                    then_branch: Box::new(then_branch),
                    else_branch: else_branch.map(Box::new),
                },
                start..end,
            ));
        }

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

        self.exit_nesting();
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
        self.enter_nesting()?;
        let start = self.peek_span().start;
        self.expect(&Token::While)?;

        let condition = self.parse_expr()?;
        let body = self.parse_block()?;
        let end = body.span.end;

        self.exit_nesting();
        Ok(Spanned::new(
            Expr::While {
                condition: Box::new(condition),
                body: Box::new(body),
            },
            start..end,
        ))
    }

    fn parse_for_in(&mut self) -> Result<Spanned<Expr>, ParseError> {
        self.enter_nesting()?;
        let start = self.peek_span().start;
        self.expect(&Token::For)?;
        let (var_name, _) = self.expect_ident()?;
        self.expect(&Token::In)?;
        let iterable = self.parse_expr()?;
        let body = self.parse_block()?;
        let end = body.span.end;

        self.exit_nesting();
        Ok(Spanned::new(
            Expr::ForIn {
                var_name,
                iterable: Box::new(iterable),
                body: Box::new(body),
            },
            start..end,
        ))
    }
    fn parse_match_expr(&mut self) -> Result<Spanned<Expr>, ParseError> {
        self.enter_nesting()?;
        let start = self.peek_span().start;
        self.expect(&Token::Match)?;
        let subject = self.parse_expr()?;
        self.expect(&Token::LBrace)?;

        let mut arms = Vec::new();
        while !matches!(self.peek(), Some(Token::RBrace) | None) {
            let pattern = self.parse_pattern()?;
            let guard = if matches!(self.peek(), Some(Token::If)) {
                self.advance();
                Some(self.parse_expr()?)
            } else {
                None
            };
            self.expect(&Token::FatArrow)?;
            let body = self.parse_expr()?;
            arms.push(MatchArm {
                pattern,
                guard,
                body,
            });
        }

        let end = self.peek_span().end;
        self.expect(&Token::RBrace)?;

        self.exit_nesting();
        Ok(Spanned::new(
            Expr::Match {
                subject: Box::new(subject),
                arms,
            },
            start..end,
        ))
    }

    fn parse_closure(&mut self) -> Result<Spanned<Expr>, ParseError> {
        self.enter_nesting()?;
        let start = self.peek_span().start;
        self.expect(&Token::Bar)?;

        // Parse parameters
        let mut params = Vec::new();
        if !matches!(self.peek(), Some(Token::Bar)) {
            loop {
                let param_start = self.peek_span().start;
                let (name, name_span) = self.expect_ident()?;

                // Type annotation is optional for closure parameters
                let ty = if matches!(self.peek(), Some(Token::Colon)) {
                    self.advance();
                    self.parse_type()?
                } else {
                    // TypeExpr::Inferred -- will be resolved by sema from context
                    let inferred_end = name_span.end;
                    Spanned::new(TypeExpr::Inferred, param_start..inferred_end)
                };

                let param_end = ty.span.end;
                params.push(Param {
                    name,
                    ty,
                    span: param_start..param_end,
                });
                if matches!(self.peek(), Some(Token::Comma)) {
                    self.advance();
                } else {
                    break;
                }
            }
        }
        self.expect(&Token::Bar)?; // closing |

        // Optional return type
        let return_type = if matches!(self.peek(), Some(Token::Arrow)) {
            self.advance();
            Some(self.parse_type()?)
        } else {
            None
        };

        // Body: either a block { ... } or a single expression
        let body = if matches!(self.peek(), Some(Token::LBrace)) {
            self.parse_block()?
        } else {
            // Single expression body (no braces needed)
            let expr = self.parse_expr()?;
            let span = expr.span.clone();
            Spanned::new(
                Expr::Block {
                    stmts: vec![],
                    tail_expr: Some(Box::new(expr)),
                },
                span,
            )
        };
        let end = body.span.end;

        self.exit_nesting();
        Ok(Spanned::new(
            Expr::Closure {
                params,
                return_type,
                body: Box::new(body),
            },
            start..end,
        ))
    }

    /// Parse an arrow closure: `(params) => body`
    ///
    /// The caller has already determined (via lookahead) that the token stream
    /// matches `( ... ) =>`, so we parse params, expect `=>`, then parse the
    /// body expression.  Produces the same `Expr::Closure` node as pipe closures.
    fn parse_arrow_closure(&mut self) -> Result<Spanned<Expr>, ParseError> {
        self.enter_nesting()?;
        let start = self.peek_span().start;
        self.expect(&Token::LParen)?;

        // Parse parameters (same format as pipe-closure / function params)
        let mut params = Vec::new();
        if !matches!(self.peek(), Some(Token::RParen)) {
            loop {
                let param_start = self.peek_span().start;
                let (name, name_span) = self.expect_ident()?;

                // Type annotation is optional for arrow closure parameters
                let ty = if matches!(self.peek(), Some(Token::Colon)) {
                    self.advance();
                    self.parse_type()?
                } else {
                    // TypeExpr::Inferred — will be resolved by sema from context
                    let inferred_end = name_span.end;
                    Spanned::new(TypeExpr::Inferred, param_start..inferred_end)
                };

                let param_end = ty.span.end;
                params.push(Param {
                    name,
                    ty,
                    span: param_start..param_end,
                });
                if matches!(self.peek(), Some(Token::Comma)) {
                    self.advance();
                } else {
                    break;
                }
            }
        }
        self.expect(&Token::RParen)?;
        self.expect(&Token::FatArrow)?;

        // Body: either a block { ... } or a single expression
        let body = if matches!(self.peek(), Some(Token::LBrace)) {
            self.parse_block()?
        } else {
            // Single expression body (no braces needed)
            let expr = self.parse_expr()?;
            let span = expr.span.clone();
            Spanned::new(
                Expr::Block {
                    stmts: vec![],
                    tail_expr: Some(Box::new(expr)),
                },
                span,
            )
        };
        let end = body.span.end;

        self.exit_nesting();
        Ok(Spanned::new(
            Expr::Closure {
                params,
                return_type: None,
                body: Box::new(body),
            },
            start..end,
        ))
    }

    fn parse_pattern(&mut self) -> Result<Spanned<Pattern>, ParseError> {
        match self.peek() {
            Some(Token::Ok) => {
                let start = self.advance().span.start;
                self.expect(&Token::LParen)?;
                let (name, _) = self.expect_ident()?;
                let end = self.peek_span().end;
                self.expect(&Token::RParen)?;
                Ok(Spanned::new(Pattern::Ok(name), start..end))
            }
            Some(Token::Err) => {
                let start = self.advance().span.start;
                self.expect(&Token::LParen)?;
                let (name, _) = self.expect_ident()?;
                let end = self.peek_span().end;
                self.expect(&Token::RParen)?;
                Ok(Spanned::new(Pattern::Err(name), start..end))
            }
            Some(Token::Some) => {
                let start = self.advance().span.start;
                self.expect(&Token::LParen)?;
                let (name, _) = self.expect_ident()?;
                let end = self.peek_span().end;
                self.expect(&Token::RParen)?;
                Ok(Spanned::new(Pattern::Some(name), start..end))
            }
            Some(Token::None) => {
                let span = self.advance().span.clone();
                Ok(Spanned::new(Pattern::None, span))
            }
            Some(Token::Ident(name)) if name == "_" => {
                let span = self.advance().span.clone();
                Ok(Spanned::new(Pattern::Wildcard, span))
            }
            Some(Token::Ident(_)) => {
                let (name, span) = self.expect_ident()?;
                if matches!(self.peek(), Some(Token::LParen)) {
                    // Variant destructure: Circle(r) or Rectangle(w, h)
                    self.advance(); // consume (
                    let mut bindings = Vec::new();
                    if !matches!(self.peek(), Some(Token::RParen)) {
                        loop {
                            let (b, _) = self.expect_ident()?;
                            bindings.push(b);
                            if matches!(self.peek(), Some(Token::Comma)) {
                                self.advance();
                            } else {
                                break;
                            }
                        }
                    }
                    let end = self.peek_span().end;
                    self.expect(&Token::RParen)?;
                    Ok(Spanned::new(
                        Pattern::VariantDestructure {
                            variant: name,
                            bindings,
                        },
                        span.start..end,
                    ))
                } else {
                    Ok(Spanned::new(Pattern::Ident(name), span))
                }
            }
            Some(Token::Int(_)) => {
                let tok = self.advance();
                if let Token::Int(n) = &tok.value {
                    let n = *n;
                    let span = tok.span.clone();
                    Ok(Spanned::new(Pattern::IntLit(n), span))
                } else {
                    unreachable!()
                }
            }
            Some(Token::True) => {
                let span = self.advance().span.clone();
                Ok(Spanned::new(Pattern::BoolLit(true), span))
            }
            Some(Token::False) => {
                let span = self.advance().span.clone();
                Ok(Spanned::new(Pattern::BoolLit(false), span))
            }
            Some(Token::String(_)) => {
                let tok = self.advance();
                if let Token::String(s) = &tok.value {
                    let s = s.clone();
                    let span = tok.span.clone();
                    Ok(Spanned::new(Pattern::StringLit(s), span))
                } else {
                    unreachable!()
                }
            }
            _ => {
                let span = self.peek_span();
                let found = self
                    .peek()
                    .map(|t| format!("`{t}`"))
                    .unwrap_or("end of file".to_string());
                Err(ParseError {
                    code: ErrorCode::E0001,
                    message: format!("expected pattern, found {found}"),
                    span,
                })
            }
        }
    }

    /// Parse a string interpolation like "Hello, {name}!"
    fn parse_interpolation(&mut self, s: &str, span: &Span) -> Result<Spanned<Expr>, ParseError> {
        let parts = split_interpolation_parts(s, span)?;
        Ok(Spanned::new(Expr::Interpolation(parts), span.clone()))
    }
}

/// Check if a string contains an unescaped `{` (interpolation marker).
fn has_unescaped_brace(s: &str) -> bool {
    let chars: Vec<char> = s.chars().collect();
    for i in 0..chars.len() {
        if chars[i] == '{' && (i == 0 || chars[i - 1] != '\\') {
            return true;
        }
    }
    false
}

/// Replace `\{` with `{` and `\}` with `}` in a string.
fn unescape_braces(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.peek() {
                Some('{') => {
                    chars.next();
                    result.push('{');
                }
                Some('}') => {
                    chars.next();
                    result.push('}');
                }
                _ => result.push(c),
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Split a string with interpolation markers into parts.
fn split_interpolation_parts(s: &str, span: &Span) -> Result<Vec<InterpolPart>, ParseError> {
    let mut parts = Vec::new();
    let mut current_lit = String::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() && (chars[i + 1] == '{' || chars[i + 1] == '}') {
            current_lit.push(chars[i + 1]);
            i += 2;
        } else if chars[i] == '{' {
            if !current_lit.is_empty() {
                parts.push(InterpolPart::Lit(current_lit.clone()));
                current_lit.clear();
            }
            i += 1;
            let mut depth = 1;
            let mut expr_str = String::new();
            while i < chars.len() && depth > 0 {
                if chars[i] == '{' {
                    depth += 1;
                    expr_str.push(chars[i]);
                } else if chars[i] == '}' {
                    depth -= 1;
                    if depth > 0 {
                        expr_str.push(chars[i]);
                    }
                } else {
                    expr_str.push(chars[i]);
                }
                i += 1;
            }
            if depth != 0 {
                return Err(ParseError {
                    code: ErrorCode::E0001,
                    message: "unterminated interpolation expression in string".to_string(),
                    span: span.clone(),
                });
            }
            let (tokens, lex_errors) = turbo_lexer::tokenize(&expr_str);
            if !lex_errors.is_empty() {
                return Err(ParseError {
                    code: ErrorCode::E0001,
                    message: format!("lex error in interpolation expression: `{}`", expr_str),
                    span: span.clone(),
                });
            }
            let mut sub_parser = Parser::new(tokens);
            let expr = sub_parser.parse_expr().map_err(|e| ParseError {
                code: ErrorCode::E0001,
                message: format!("error in interpolation expression: {}", e.message),
                span: span.clone(),
            })?;
            if !sub_parser.at_end() {
                return Err(ParseError {
                    code: ErrorCode::E0001,
                    message: format!(
                        "unexpected tokens after interpolation expression: `{}`",
                        expr_str
                    ),
                    span: span.clone(),
                });
            }
            parts.push(InterpolPart::Expr(Box::new(expr)));
        } else {
            current_lit.push(chars[i]);
            i += 1;
        }
    }

    if !current_lit.is_empty() {
        parts.push(InterpolPart::Lit(current_lit));
    }

    Ok(parts)
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
        let Item::Function(f) = &module.items[0].node else {
            panic!("Expected function")
        };
        assert_eq!(f.name, "main");
        assert!(f.params.is_empty());
        assert!(f.return_type.is_none());
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
        let Item::Function(f) = &module.items[0].node else {
            panic!("Expected function")
        };
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

    #[test]
    fn test_let_mut() {
        let source = "fn main() { let mut x = 0 }";
        let module = parse_source(source);
        let Item::Function(f) = &module.items[0].node else {
            panic!("Expected function")
        };
        if let Expr::Block { stmts, .. } = &f.body.node {
            if let Stmt::Let { name, mutable, .. } = &stmts[0].node {
                assert_eq!(name, "x");
                assert!(mutable);
            }
        }
    }

    #[test]
    fn test_binary_precedence() {
        let source = "fn main() { 1 + 2 * 3 }";
        let module = parse_source(source);
        let Item::Function(f) = &module.items[0].node else {
            panic!("Expected function")
        };
        if let Expr::Block {
            tail_expr: Some(tail),
            ..
        } = &f.body.node
        {
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

    #[test]
    fn test_if_else() {
        let source = "fn main() { if true { 1 } else { 2 } }";
        let module = parse_source(source);
        let Item::Function(f) = &module.items[0].node else {
            panic!("Expected function")
        };
        if let Expr::Block {
            tail_expr: Some(tail),
            ..
        } = &f.body.node
        {
            assert!(matches!(tail.node, Expr::If { .. }));
        } else {
            panic!("Expected tail expr");
        }
    }

    #[test]
    fn test_function_with_params() {
        let source = "fn add(a: i32, b: i32) -> i32 { a + b }";
        let module = parse_source(source);
        let Item::Function(f) = &module.items[0].node else {
            panic!("Expected function")
        };
        assert_eq!(f.name, "add");
        assert_eq!(f.params.len(), 2);
        assert_eq!(f.params[0].name, "a");
        assert_eq!(f.params[1].name, "b");
        assert!(f.return_type.is_some());
    }

    #[test]
    fn test_function_call() {
        let source = r#"fn main() { print("hello") }"#;
        let module = parse_source(source);
        let Item::Function(f) = &module.items[0].node else {
            panic!("Expected function")
        };
        if let Expr::Block {
            tail_expr: Some(tail),
            ..
        } = &f.body.node
        {
            assert!(matches!(tail.node, Expr::Call { .. }));
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
        let Item::Function(f) = &module.items[0].node else {
            panic!("Expected function")
        };
        if let Expr::Block {
            tail_expr: Some(tail),
            ..
        } = &f.body.node
        {
            assert!(matches!(
                tail.node,
                Expr::UnaryOp {
                    op: UnaryOp::Neg,
                    ..
                }
            ));
        }
    }

    #[test]
    fn test_comparison() {
        let source = "fn main() { x > 25 }";
        let module = parse_source(source);
        let Item::Function(f) = &module.items[0].node else {
            panic!("Expected function")
        };
        if let Expr::Block {
            tail_expr: Some(tail),
            ..
        } = &f.body.node
        {
            if let Expr::BinaryOp { op, .. } = &tail.node {
                assert_eq!(*op, BinOp::Greater);
            }
        }
    }

    #[test]
    fn test_logical_operators() {
        let source = "fn main() { a && b || c }";
        let module = parse_source(source);
        let Item::Function(f) = &module.items[0].node else {
            panic!("Expected function")
        };
        if let Expr::Block {
            tail_expr: Some(tail),
            ..
        } = &f.body.node
        {
            // Should be Or(And(a, b), c) since && binds tighter
            if let Expr::BinaryOp { op, .. } = &tail.node {
                assert_eq!(*op, BinOp::Or);
            }
        }
    }

    #[test]
    fn test_stmt_then_tail() {
        let source = "fn main() { let x = 1\n x + 2 }";
        let module = parse_source(source);
        let Item::Function(f) = &module.items[0].node else {
            panic!("Expected function")
        };
        if let Expr::Block { stmts, tail_expr } = &f.body.node {
            assert_eq!(stmts.len(), 1);
            assert!(tail_expr.is_some());
        }
    }

    #[test]
    fn test_return_statement() {
        let source = "fn foo() -> i32 { return 42 }";
        let module = parse_source(source);
        let Item::Function(f) = &module.items[0].node else {
            panic!("Expected function")
        };
        if let Expr::Block { stmts, .. } = &f.body.node {
            assert_eq!(stmts.len(), 1);
            assert!(matches!(&stmts[0].node, Stmt::Return(Some(_))));
        }
    }

    #[test]
    fn test_let_with_type() {
        let source = "fn main() { let x: i32 = 42 }";
        let module = parse_source(source);
        let Item::Function(f) = &module.items[0].node else {
            panic!("Expected function")
        };
        if let Expr::Block { stmts, .. } = &f.body.node {
            if let Stmt::Let { ty: Some(ty), .. } = &stmts[0].node {
                assert!(matches!(&ty.node, TypeExpr::Named(n) if n == "i32"));
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
        let Item::Function(f) = &module.items[0].node else {
            panic!("Expected function")
        };
        if let Expr::Block { stmts, tail_expr } = &f.body.node {
            assert_eq!(stmts.len(), 2); // two let bindings
            assert!(tail_expr.is_some()); // if-else is tail expr
        }
    }

    #[test]
    fn test_assignment() {
        let source = "fn main() { let mut x = 1\n x = 2 }";
        let module = parse_source(source);
        let Item::Function(f) = &module.items[0].node else {
            panic!("Expected function")
        };
        if let Expr::Block { stmts, tail_expr } = &f.body.node {
            assert_eq!(stmts.len(), 1); // let
            if let Some(tail) = tail_expr {
                assert!(matches!(tail.node, Expr::Assign { .. }));
            }
        }
    }

    #[test]
    fn test_compound_assignment() {
        let source = "fn main() { let mut x = 1\n x += 2 }";
        let module = parse_source(source);
        let Item::Function(f) = &module.items[0].node else {
            panic!("Expected function")
        };
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

    #[test]
    fn test_semicolons_accepted() {
        let source = "fn main() { let x = 5; print(x); x + 1 }";
        let module = parse_source(source);
        assert_eq!(module.items.len(), 1);
        let Item::Function(f) = &module.items[0].node else {
            panic!("Expected function")
        };
        if let Expr::Block { stmts, tail_expr } = &f.body.node {
            assert_eq!(stmts.len(), 2); // let x = 5 and print(x)
            assert!(tail_expr.is_some()); // x + 1 is the tail expr
        } else {
            panic!("Expected block");
        }
    }

    #[test]
    fn test_pipe_simple() {
        // 5 |> double desugars to double(5)
        let source = "fn double(x: i64) -> i64 { x * 2 }\nfn main() { 5 |> double }";
        let module = parse_source(source);
        let Item::Function(f) = &module.items[1].node else {
            panic!("Expected function")
        };
        if let Expr::Block {
            tail_expr: Some(tail),
            ..
        } = &f.body.node
        {
            if let Expr::Call { callee, args } = &tail.node {
                assert!(matches!(&callee.node, Expr::Ident(name) if name == "double"));
                assert_eq!(args.len(), 1);
                assert!(matches!(&args[0].node, Expr::IntLit(5)));
            } else {
                panic!("Expected Call, got {:?}", tail.node);
            }
        } else {
            panic!("Expected tail expr");
        }
    }

    #[test]
    fn test_pipe_with_args() {
        // 5 |> add(10) desugars to add(5, 10)
        let source = "fn add(a: i64, b: i64) -> i64 { a + b }\nfn main() { 5 |> add(10) }";
        let module = parse_source(source);
        let Item::Function(f) = &module.items[1].node else {
            panic!("Expected function")
        };
        if let Expr::Block {
            tail_expr: Some(tail),
            ..
        } = &f.body.node
        {
            if let Expr::Call { callee, args } = &tail.node {
                assert!(matches!(&callee.node, Expr::Ident(name) if name == "add"));
                assert_eq!(args.len(), 2);
                assert!(matches!(&args[0].node, Expr::IntLit(5)));
                assert!(matches!(&args[1].node, Expr::IntLit(10)));
            } else {
                panic!("Expected Call, got {:?}", tail.node);
            }
        } else {
            panic!("Expected tail expr");
        }
    }

    #[test]
    fn test_pipe_chained() {
        // 5 |> double |> add(10) desugars to add(double(5), 10)
        let source = "fn double(x: i64) -> i64 { x * 2 }\nfn add(a: i64, b: i64) -> i64 { a + b }\nfn main() { 5 |> double |> add(10) }";
        let module = parse_source(source);
        let Item::Function(f) = &module.items[2].node else {
            panic!("Expected function")
        };
        if let Expr::Block {
            tail_expr: Some(tail),
            ..
        } = &f.body.node
        {
            if let Expr::Call { callee, args } = &tail.node {
                assert!(matches!(&callee.node, Expr::Ident(name) if name == "add"));
                assert_eq!(args.len(), 2);
                if let Expr::Call {
                    callee: inner_callee,
                    args: inner_args,
                } = &args[0].node
                {
                    assert!(matches!(&inner_callee.node, Expr::Ident(name) if name == "double"));
                    assert_eq!(inner_args.len(), 1);
                    assert!(matches!(&inner_args[0].node, Expr::IntLit(5)));
                } else {
                    panic!("Expected inner Call, got {:?}", args[0].node);
                }
                assert!(matches!(&args[1].node, Expr::IntLit(10)));
            } else {
                panic!("Expected Call, got {:?}", tail.node);
            }
        } else {
            panic!("Expected tail expr");
        }
    }

    #[test]
    fn test_method_style_call() {
        // a.len() desugars to len(a)
        let source = "fn main() { let a = [1, 2, 3]\n a.len() }";
        let module = parse_source(source);
        let Item::Function(f) = &module.items[0].node else {
            panic!("Expected function")
        };
        if let Expr::Block {
            tail_expr: Some(tail),
            ..
        } = &f.body.node
        {
            if let Expr::Call { callee, args } = &tail.node {
                assert!(matches!(&callee.node, Expr::Ident(name) if name == "len"));
                assert_eq!(args.len(), 1);
                assert!(matches!(&args[0].node, Expr::Ident(name) if name == "a"));
            } else {
                panic!("Expected Call, got {:?}", tail.node);
            }
        } else {
            panic!("Expected tail expr");
        }
    }

    #[test]
    fn test_method_style_with_args() {
        // a.push(5) desugars to push(a, 5), then COW rewrite makes it a = push(a, 5)
        let source = "fn main() { let a = [1, 2, 3]\n a.push(5) }";
        let module = parse_source(source);
        let Item::Function(f) = &module.items[0].node else {
            panic!("Expected function")
        };
        if let Expr::Block { stmts, .. } = &f.body.node {
            // Last stmt should be the COW-rewritten assign: a = push(a, 5)
            let last = stmts.last().expect("Expected statements");
            if let Stmt::Expr(expr) = &last.node {
                if let Expr::Assign { target, value } = &expr.node {
                    assert_eq!(target, "a");
                    if let Expr::Call { callee, args } = &value.node {
                        assert!(matches!(&callee.node, Expr::Ident(name) if name == "push"));
                        assert_eq!(args.len(), 2);
                        assert!(matches!(&args[0].node, Expr::Ident(name) if name == "a"));
                        assert!(matches!(&args[1].node, Expr::IntLit(5)));
                    } else {
                        panic!("Expected Call inside Assign, got {:?}", value.node);
                    }
                } else {
                    panic!("Expected Assign, got {:?}", expr.node);
                }
            } else {
                panic!("Expected Stmt::Expr, got {:?}", last.node);
            }
        } else {
            panic!("Expected Block");
        }
    }

    #[test]
    fn test_field_access_still_works() {
        // expr.field (without parens) should still be FieldAccess
        let source = "fn main() { let p = Point { x: 1, y: 2 }\n p.x }";
        let module = parse_source(source);
        let Item::Function(f) = &module.items[0].node else {
            panic!("Expected function")
        };
        if let Expr::Block {
            tail_expr: Some(tail),
            ..
        } = &f.body.node
        {
            assert!(matches!(&tail.node, Expr::FieldAccess { field, .. } if field == "x"));
        } else {
            panic!("Expected tail expr");
        }
    }

    #[test]
    fn test_string_interpolation() {
        let source = r#"fn main() { print("Hello, {name}!") }"#;
        let module = parse_source(source);
        let Item::Function(f) = &module.items[0].node else {
            panic!("Expected function")
        };
        if let Expr::Block {
            tail_expr: Some(tail),
            ..
        } = &f.body.node
        {
            if let Expr::Call { args, .. } = &tail.node {
                assert!(
                    matches!(&args[0].node, Expr::Interpolation(_)),
                    "Expected Interpolation, got: {:?}",
                    args[0].node
                );
            } else {
                panic!("Expected Call, got: {:?}", tail.node);
            }
        } else {
            panic!("Expected Block with tail expr, got: {:?}", f.body.node);
        }
    }

    #[test]
    fn test_generic_function() {
        let source = "fn identity<T>(x: T) -> T { x }";
        let module = parse_source(source);
        assert_eq!(module.items.len(), 1);
        if let Item::Function(f) = &module.items[0].node {
            assert_eq!(f.name, "identity");
            assert_eq!(f.type_params, vec![TypeParam::new("T".to_string())]);
            assert_eq!(f.params.len(), 1);
            assert_eq!(f.params[0].name, "x");
            assert!(matches!(&f.params[0].ty.node, TypeExpr::Named(n) if n == "T"));
            assert!(
                matches!(&f.return_type.as_ref().unwrap().node, TypeExpr::Named(n) if n == "T")
            );
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

    #[test]
    fn test_async_fn() {
        let source = "async fn compute(x: i64) -> i64 { x * x }\nfn main() { }";
        let module = parse_source(source);
        assert_eq!(module.items.len(), 2);
        if let Item::Function(f) = &module.items[0].node {
            assert_eq!(f.name, "compute");
            assert!(f.is_async);
            assert_eq!(f.params.len(), 1);
        } else {
            panic!("Expected function");
        }
    }

    #[test]
    fn test_sync_fn_not_async() {
        let source = "fn main() { }";
        let module = parse_source(source);
        if let Item::Function(f) = &module.items[0].node {
            assert!(!f.is_async);
        }
    }

    #[test]
    fn test_await_expr() {
        let source = "fn main() { await foo() }";
        let module = parse_source(source);
        if let Item::Function(f) = &module.items[0].node {
            if let Expr::Block {
                tail_expr: Some(tail),
                ..
            } = &f.body.node
            {
                assert!(matches!(tail.node, Expr::Await(_)));
            } else {
                panic!("Expected tail expr");
            }
        }
    }

    #[test]
    fn test_spawn_expr() {
        let source = "fn main() { spawn foo() }";
        let module = parse_source(source);
        if let Item::Function(f) = &module.items[0].node {
            if let Expr::Block {
                tail_expr: Some(tail),
                ..
            } = &f.body.node
            {
                assert!(matches!(tail.node, Expr::Spawn(_)));
            } else {
                panic!("Expected tail expr");
            }
        }
    }

    #[test]
    fn test_await_in_let() {
        let source = "fn main() { let x = await compute(5) }";
        let module = parse_source(source);
        if let Item::Function(f) = &module.items[0].node {
            if let Expr::Block { stmts, .. } = &f.body.node {
                assert_eq!(stmts.len(), 1);
                if let Stmt::Let { value, .. } = &stmts[0].node {
                    assert!(matches!(value.node, Expr::Await(_)));
                } else {
                    panic!("Expected let statement");
                }
            }
        }
    }

    #[test]
    fn test_tool_fn() {
        let source = r#"tool fn search(query: str) -> str { "results" }
fn main() { search("hello") }"#;
        let module = parse_source(source);
        assert_eq!(module.items.len(), 2);
        if let Item::Function(f) = &module.items[0].node {
            assert_eq!(f.name, "search");
            assert!(f.is_tool);
            assert_eq!(f.params.len(), 1);
            assert_eq!(f.params[0].name, "query");
        } else {
            panic!("Expected Function");
        }
        // main should not be a tool fn
        if let Item::Function(f) = &module.items[1].node {
            assert_eq!(f.name, "main");
            assert!(!f.is_tool);
        }
    }

    #[test]
    fn test_agent_declaration() {
        let source = r#"
tool fn search(q: str) -> str { "r" }
agent Helper {
    model: "claude-sonnet"
    tools: [search]
    system: "You help."
}
fn main() { }
"#;
        let module = parse_source(source);
        assert_eq!(module.items.len(), 3);
        if let Item::Agent(agent) = &module.items[1].node {
            assert_eq!(agent.name, "Helper");
            assert_eq!(agent.model, "claude-sonnet");
            assert_eq!(agent.tools, vec!["search"]);
            assert_eq!(agent.system_prompt, Some("You help.".to_string()));
        } else {
            panic!("Expected Agent, got {:?}", module.items[1].node);
        }
    }

    #[test]
    fn test_agent_multiple_tools() {
        let source = r#"
tool fn a(x: i64) -> i64 { x }
tool fn b(x: i64) -> i64 { x }
agent Bot {
    model: "gpt-4"
    tools: [a, b]
}
fn main() { }
"#;
        let module = parse_source(source);
        if let Item::Agent(agent) = &module.items[2].node {
            assert_eq!(agent.name, "Bot");
            assert_eq!(agent.tools, vec!["a", "b"]);
            assert_eq!(agent.system_prompt, None);
        } else {
            panic!("Expected Agent");
        }
    }

    #[test]
    fn test_agent_no_system_prompt() {
        let source = r#"
tool fn t(x: i64) -> i64 { x }
agent A {
    model: "test"
    tools: [t]
}
fn main() { }
"#;
        let module = parse_source(source);
        if let Item::Agent(agent) = &module.items[1].node {
            assert_eq!(agent.system_prompt, None);
        } else {
            panic!("Expected Agent");
        }
    }

    #[test]
    fn test_nesting_depth_limit() {
        // Run in a thread with a large stack so debug-mode recursion doesn't
        // overflow the test-runner's default stack before the depth check fires.
        let handler = std::thread::Builder::new()
            .stack_size(32 * 1024 * 1024) // 32 MB
            .spawn(|| {
                // Build a chain of nested `if true { ... }` blocks that exceeds
                // MAX_NESTING_DEPTH. Each `if` adds 1 (parse_if_expr) and its
                // block adds 1 (parse_block), so ~130 nested ifs will exceed 256.
                let depth = 130;
                let mut source = String::from("fn main() {\n");
                for _ in 0..depth {
                    source.push_str("if true {\n");
                }
                source.push_str("1\n");
                for _ in 0..depth {
                    source.push_str("}\n");
                }
                source.push_str("}\n");
                let (tokens, lex_errors) = tokenize(&source);
                assert!(lex_errors.is_empty());
                let (_module, parse_errors) = parse(tokens);
                assert!(
                    !parse_errors.is_empty(),
                    "deeply nested input should produce a parse error"
                );
                assert!(
                    parse_errors[0].message.contains("nesting depth"),
                    "error should mention nesting depth, got: {}",
                    parse_errors[0].message
                );
            })
            .expect("failed to spawn test thread");
        handler.join().expect("test thread panicked");
    }

    #[test]
    fn test_fuzz_parser_no_panics() {
        // Quick fuzz: 1000 random inputs, none should panic.
        // Runs in a thread with a large stack to handle deeply nested inputs
        // in debug mode.
        let handler = std::thread::Builder::new()
            .stack_size(32 * 1024 * 1024)
            .spawn(|| {
                for seed in 0..1000u64 {
                    let input = generate_fuzz_input(seed);
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let (tokens, _) = tokenize(&input);
                        let _ = parse(tokens);
                    }));
                    // We don't care about errors, only panics
                }
            })
            .expect("failed to spawn fuzz thread");
        handler
            .join()
            .expect("fuzz thread panicked — parser panicked on fuzz input");
    }

    /// Generate a deterministic fuzz input from a seed.
    fn generate_fuzz_input(seed: u64) -> String {
        // Simple xorshift64 PRNG
        let mut state = seed.wrapping_add(1);
        let mut next = || -> u64 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };

        let keywords = &[
            "fn", "let", "mut", "const", "if", "else", "while", "for", "in", "return", "true",
            "false", "match", "struct", "type", "impl", "trait", "pub", "import", "from", "break",
            "continue",
        ];
        let operators = &[
            "==", "!=", "<=", ">=", "&&", "||", "->", "=>", "+", "-", "*", "/", "%", "=", "<", ">",
            "!", ".",
        ];
        let delimiters = &["(", ")", "{", "}", "[", "]", ",", ":", ";"];

        match seed % 5 {
            0 => {
                // Random token soup
                let count = (next() % 60 + 5) as usize;
                let mut out = String::new();
                for i in 0..count {
                    if i > 0 {
                        out.push(' ');
                    }
                    let kind = next() % 5;
                    match kind {
                        0 => out.push_str(keywords[(next() % keywords.len() as u64) as usize]),
                        1 => out.push_str(operators[(next() % operators.len() as u64) as usize]),
                        2 => out.push_str(delimiters[(next() % delimiters.len() as u64) as usize]),
                        3 => out.push_str(&format!("{}", next() % 1000)),
                        _ => {
                            let len = (next() % 10 + 1) as usize;
                            for j in 0..len {
                                let c = if j == 0 {
                                    (b'a' + (next() % 26) as u8) as char
                                } else {
                                    (b'a' + (next() % 26) as u8) as char
                                };
                                out.push(c);
                            }
                        }
                    }
                }
                out
            }
            1 => {
                // Nested parens/braces (moderate depth to avoid stack overflow in debug)
                let depth = (next() % 80 + 1) as usize;
                let mut out = String::new();
                for _ in 0..depth {
                    out.push('(');
                }
                out.push_str("42");
                for _ in 0..depth {
                    out.push(')');
                }
                out
            }
            2 => {
                // Mutated valid program
                let programs = &[
                    "fn main() { let x = 42 }",
                    "fn add(a: i64, b: i64) -> i64 { return a + b }",
                    "struct Point { x: i64, y: i64 }",
                ];
                let prog = programs[(next() % programs.len() as u64) as usize];
                let mut bytes: Vec<u8> = prog.bytes().collect();
                let mutations = (next() % 15 + 1) as usize;
                for _ in 0..mutations {
                    if bytes.is_empty() {
                        break;
                    }
                    let op = next() % 3;
                    match op {
                        0 => {
                            bytes.remove((next() % bytes.len() as u64) as usize);
                        }
                        1 => {
                            let idx = (next() % (bytes.len() as u64 + 1)) as usize;
                            bytes.insert(idx, (next() % 128) as u8);
                        }
                        _ => {
                            let idx = (next() % bytes.len() as u64) as usize;
                            bytes[idx] = (next() % 128) as u8;
                        }
                    }
                }
                String::from_utf8_lossy(&bytes).into_owned()
            }
            3 => {
                // Keyword soup
                let count = (next() % 40 + 5) as usize;
                let mut out = String::new();
                for i in 0..count {
                    if i > 0 {
                        out.push(' ');
                    }
                    out.push_str(keywords[(next() % keywords.len() as u64) as usize]);
                }
                out
            }
            _ => {
                // Boundary cases
                match next() % 4 {
                    0 => String::new(),
                    1 => "x".to_string(),
                    2 => "\0\0\0".to_string(),
                    _ => " \t\n\r".repeat(50),
                }
            }
        }
    }
}
