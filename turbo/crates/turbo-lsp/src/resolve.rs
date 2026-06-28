//! Scope / binding resolution over the Turbo AST.
//!
//! The earlier LSP implementation answered go-to-definition, find-references,
//! and rename purely by **token-text matching**: every identifier spelled `x`
//! was treated as the same symbol. That is wrong in two directions —
//!
//! * locals, parameters, and pattern bindings were never navigable (only
//!   top-level items were), and
//! * renaming an outer `x` also renamed an unrelated/shadowed inner `x`.
//!
//! This module walks the parsed [`turbo_ast::Module`] with a lexical scope
//! stack and maps each **local** identifier *occurrence* (by span) to its
//! *declaration* (by span). It deliberately models only the bindings whose
//! lexical scope is unambiguous from the AST alone:
//!
//! * function parameters and `mut` params,
//! * `let` / `let mut` bindings (including shadowing within a block),
//! * `let { a, b } = ...` destructuring fields,
//! * closure parameters,
//! * `for <x> in ...` loop binders,
//! * `match` / `if let` bindings (`ok(v)`, `err(v)`, `some(v)`,
//!   `Variant(a, b)`, and bare-ident bindings that are not enum variants).
//!
//! Everything else — top-level functions/structs/enums/traits/consts, type
//! references, field accesses, struct-literal keys, enum-variant qualifiers —
//! is intentionally *not* resolved here. Those keep the existing top-level /
//! textual behaviour in `main.rs`, which the caller layers on top by treating
//! "not a local" as the fall-back case. This keeps the pass correct over
//! complete: a construct we cannot resolve precisely falls back rather than
//! returning a wrong answer.
//!
//! ## Span recovery
//!
//! The parser keeps precise spans for identifier *uses* (`Expr::Ident` carries
//! the ident token span) but discards the span of *binder names* (`let x`,
//! `fn f(x: T)`, `for x in`, …) — only the enclosing construct's span is
//! retained. We recover a binder's exact name span by scanning the token
//! stream for the first matching `Ident` token inside a narrowed sub-range
//! (e.g. between `let` and the `=` for a `let` binding). Every recorded span
//! therefore lines up exactly with a real ident token, so a cursor hit can be
//! matched by span equality.

use std::collections::HashMap;
use std::ops::Range;

use turbo_ast::{Expr, Item, MatchArm, Module, Param, Pattern, Spanned, Stmt, TypeExpr};
use turbo_lexer::Token;

type Span = Range<usize>;
type LexToken = turbo_lexer::Spanned<Token>;

/// What kind of binding a local declaration is — used for hover labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LocalKind {
    Param,
    Let,
    ClosureParam,
    ForBinder,
    PatternBinding,
}

impl LocalKind {
    /// Human-readable noun used in hover text.
    pub(crate) fn label(self) -> &'static str {
        match self {
            LocalKind::Param => "parameter",
            LocalKind::Let => "local variable",
            LocalKind::ClosureParam => "closure parameter",
            LocalKind::ForBinder => "loop variable",
            LocalKind::PatternBinding => "pattern binding",
        }
    }
}

/// A resolved local declaration and every occurrence (including the
/// declaration site itself) that binds to it.
#[derive(Debug, Clone)]
pub(crate) struct LocalDecl {
    pub(crate) name: String,
    pub(crate) decl_span: Span,
    pub(crate) kind: LocalKind,
    /// Declared type, when the source spelled one out (param annotation or
    /// `let x: T`). `None` when the type is left to inference — we never
    /// fabricate a type we cannot read off the AST.
    pub(crate) ty: Option<String>,
    /// All occurrence spans that resolve to this declaration, in source order,
    /// including `decl_span`.
    pub(crate) occurrences: Vec<Span>,
}

/// The result of resolving a whole module: a flat list of local declarations
/// plus an index from any occurrence span to its declaration.
#[derive(Debug, Default)]
pub(crate) struct Resolution {
    decls: Vec<LocalDecl>,
    /// occurrence span -> index into `decls`.
    span_to_decl: HashMap<Span, usize>,
}

impl Resolution {
    /// Look up the local declaration an occurrence span resolves to, if any.
    /// `span` must be the exact span of an identifier token (which is how the
    /// callers obtain it).
    pub(crate) fn local_at(&self, span: &Span) -> Option<&LocalDecl> {
        self.span_to_decl.get(span).map(|&i| &self.decls[i])
    }

    /// `true` if the span is a recorded occurrence of *some* local binding.
    /// Used by the top-level/textual fall-back to subtract spans that belong to
    /// a (possibly shadowing) local so they are never swept up by a textual
    /// rename of a same-named global.
    pub(crate) fn is_local_occurrence(&self, span: &Span) -> bool {
        self.span_to_decl.contains_key(span)
    }

    /// All occurrence spans bound to the same local declaration as `span`,
    /// in source order. Empty if `span` is not a local occurrence.
    pub(crate) fn occurrences_at(&self, span: &Span) -> Vec<Span> {
        match self.local_at(span) {
            Some(decl) => decl.occurrences.clone(),
            None => Vec::new(),
        }
    }
}

/// Resolve a module against its token stream. Returns an empty resolution when
/// nothing local is found (callers then use their textual fall-back).
pub(crate) fn resolve_module(module: &Module, tokens: &[LexToken]) -> Resolution {
    // Names of every enum variant in the file. A bare-ident match pattern that
    // matches one of these is a *variant* test, not a binding (mirroring sema,
    // which only treats non-enum ident patterns as bindings).
    let mut enum_variants: Vec<String> = Vec::new();
    for item in &module.items {
        if let Item::Enum(e) = &item.node {
            for v in &e.variants {
                enum_variants.push(v.name.clone());
            }
        }
    }

    let mut builder = Builder {
        tokens,
        enum_variants,
        decls: Vec::new(),
        span_to_decl: HashMap::new(),
        scopes: Vec::new(),
    };

    for item in &module.items {
        builder.walk_item(&item.node);
    }

    // Sort each declaration's occurrences by source position for stable,
    // predictable rename/reference output.
    for decl in &mut builder.decls {
        decl.occurrences.sort_by_key(|s| s.start);
        decl.occurrences.dedup();
    }

    Resolution {
        decls: builder.decls,
        span_to_decl: builder.span_to_decl,
    }
}

struct Builder<'a> {
    tokens: &'a [LexToken],
    enum_variants: Vec<String>,
    decls: Vec<LocalDecl>,
    span_to_decl: HashMap<Span, usize>,
    /// Stack of lexical scopes; each holds indices into `decls`.
    scopes: Vec<Vec<usize>>,
}

impl Builder<'_> {
    fn push_scope(&mut self) {
        self.scopes.push(Vec::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    /// Introduce a new binding in the current (innermost) scope.
    fn declare(&mut self, name: &str, decl_span: Span, kind: LocalKind, ty: Option<String>) {
        let idx = self.decls.len();
        self.decls.push(LocalDecl {
            name: name.to_string(),
            decl_span: decl_span.clone(),
            kind,
            ty,
            occurrences: vec![decl_span.clone()],
        });
        self.span_to_decl.insert(decl_span, idx);
        if let Some(scope) = self.scopes.last_mut() {
            scope.push(idx);
        }
    }

    /// Resolve a use of `name` (an identifier occurrence at `span`). When it
    /// binds to an in-scope local, record the occurrence; otherwise leave it
    /// alone (it is a global/builtin/unknown, handled by the textual layer).
    fn resolve_use(&mut self, name: &str, span: Span) {
        if let Some(idx) = self.lookup(name) {
            self.decls[idx].occurrences.push(span.clone());
            self.span_to_decl.insert(span, idx);
        }
    }

    /// Innermost-first lookup; within a scope the most recently declared
    /// binding wins (handles same-block shadowing: `let x = 1; let x = 2`).
    fn lookup(&self, name: &str) -> Option<usize> {
        for scope in self.scopes.iter().rev() {
            for &idx in scope.iter().rev() {
                if self.decls[idx].name == name {
                    return Some(idx);
                }
            }
        }
        None
    }

    /// First `Ident(name)` token wholly inside `range`. Used to recover the
    /// exact span of a binder name the parser dropped.
    fn name_span_in(&self, range: &Span, name: &str) -> Option<Span> {
        self.tokens.iter().find_map(|t| {
            if t.span.start >= range.start && t.span.end <= range.end {
                if let Token::Ident(n) = &t.value {
                    if n == name {
                        return Some(t.span.clone());
                    }
                }
            }
            None
        })
    }

    /// All `Ident` token spans wholly inside `range`, in source order. Used to
    /// place destructure/variant-binding names that the parser stored as bare
    /// strings.
    fn ident_spans_in(&self, range: &Span) -> Vec<(String, Span)> {
        self.tokens
            .iter()
            .filter_map(|t| {
                if t.span.start >= range.start && t.span.end <= range.end {
                    if let Token::Ident(n) = &t.value {
                        return Some((n.clone(), t.span.clone()));
                    }
                }
                None
            })
            .collect()
    }

    // --- AST walk ----------------------------------------------------------

    fn walk_item(&mut self, item: &Item) {
        match item {
            Item::Function(f) => self.walk_fn(&f.params, &f.body),
            Item::Impl(block) => {
                for m in &block.methods {
                    self.walk_fn(&m.node.params, &m.node.body);
                }
            }
            Item::Trait(t) => {
                for m in &t.methods {
                    if let Some(body) = &m.default_body {
                        // A default trait method body sees its own params.
                        self.push_scope();
                        for p in &m.params {
                            self.declare_param(p, LocalKind::Param);
                        }
                        self.walk_expr(body);
                        self.pop_scope();
                    }
                }
            }
            Item::Const(c) => {
                // A const initializer has no locals of its own, but may
                // reference things; nothing local to bind, just walk.
                self.walk_expr(&c.value);
            }
            // Structs, enums, imports, extern blocks introduce no local
            // bindings — their names are top-level and handled textually.
            Item::Struct(_) | Item::Enum(_) | Item::Import { .. } | Item::Extern(_) => {}
        }
    }

    fn declare_param(&mut self, param: &Param, kind: LocalKind) {
        // The param name is the first ident inside the param span (after an
        // optional `mut` keyword, which is not an ident).
        let span = self
            .name_span_in(&param.span, &param.name)
            .unwrap_or_else(|| param.span.clone());
        let ty = type_label(&param.ty.node);
        self.declare(&param.name, span, kind, ty);
    }

    fn walk_fn(&mut self, params: &[Param], body: &Spanned<Expr>) {
        self.push_scope();
        for p in params {
            self.declare_param(p, LocalKind::Param);
        }
        self.walk_expr(body);
        self.pop_scope();
    }

    fn walk_block(&mut self, stmts: &[Spanned<Stmt>], tail: &Option<Box<Spanned<Expr>>>) {
        self.push_scope();
        for s in stmts {
            self.walk_stmt(s);
        }
        if let Some(t) = tail {
            self.walk_expr(t);
        }
        self.pop_scope();
    }

    fn walk_stmt(&mut self, stmt: &Spanned<Stmt>) {
        match &stmt.node {
            Stmt::Let {
                name, ty, value, ..
            } => {
                // Resolve the RHS *before* the binding is visible so that
                // `let x = x` refers to the outer `x`.
                self.walk_expr(value);
                let binder_range = stmt.span.start..value.span.start;
                let span = self
                    .name_span_in(&binder_range, name)
                    .unwrap_or_else(|| stmt.span.clone());
                let ty_label = ty.as_ref().and_then(|t| type_label(&t.node));
                self.declare(name, span, LocalKind::Let, ty_label);
            }
            Stmt::LetDestructure { fields, value, .. } => {
                self.walk_expr(value);
                let binder_range = stmt.span.start..value.span.start;
                let idents = self.ident_spans_in(&binder_range);
                for field in fields {
                    if let Some((_, span)) = idents.iter().find(|(n, _)| n == field) {
                        self.declare(field, span.clone(), LocalKind::Let, None);
                    }
                }
            }
            Stmt::Expr(e) => self.walk_expr(e),
            Stmt::Return(opt) => {
                if let Some(e) = opt {
                    self.walk_expr(e);
                }
            }
            Stmt::Defer(e) => self.walk_expr(e),
        }
    }

    fn walk_expr(&mut self, expr: &Spanned<Expr>) {
        match &expr.node {
            Expr::Ident(name) => self.resolve_use(name, expr.span.clone()),

            Expr::IntLit(_)
            | Expr::FloatLit(_)
            | Expr::StringLit(_)
            | Expr::BoolLit(_)
            | Expr::Unit
            | Expr::NoneExpr
            | Expr::Break
            | Expr::Continue
            | Expr::EnumVariant { .. } => {}

            Expr::BinaryOp { left, right, .. } => {
                self.walk_expr(left);
                self.walk_expr(right);
            }
            Expr::UnaryOp { expr: inner, .. } => self.walk_expr(inner),
            Expr::Call { callee, args } => {
                self.walk_expr(callee);
                for a in args {
                    self.walk_expr(a);
                }
            }
            Expr::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.walk_expr(condition);
                self.walk_expr(then_branch);
                if let Some(e) = else_branch {
                    self.walk_expr(e);
                }
            }
            Expr::Block { stmts, tail_expr } => self.walk_block(stmts, tail_expr),
            Expr::Assign { target, value } => {
                // Reassignment is a *use* of an existing binding. Recover the
                // target name span (it starts the assignment expression).
                let target_range = expr.span.start..value.span.start;
                if let Some(span) = self.name_span_in(&target_range, target) {
                    self.resolve_use(target, span);
                }
                self.walk_expr(value);
            }
            Expr::CompoundAssign { target, value, .. } => {
                let target_range = expr.span.start..value.span.start;
                if let Some(span) = self.name_span_in(&target_range, target) {
                    self.resolve_use(target, span);
                }
                self.walk_expr(value);
            }
            Expr::FieldAssign { object, value, .. } => {
                self.walk_expr(object);
                self.walk_expr(value);
            }
            Expr::IndexAssign {
                object,
                index,
                value,
            } => {
                self.walk_expr(object);
                self.walk_expr(index);
                self.walk_expr(value);
            }
            Expr::While { condition, body } => {
                self.walk_expr(condition);
                self.walk_expr(body);
            }
            Expr::ForIn {
                var_name,
                iterable,
                body,
            } => {
                self.walk_expr(iterable);
                self.push_scope();
                let binder_range = expr.span.start..iterable.span.start;
                let span = self
                    .name_span_in(&binder_range, var_name)
                    .unwrap_or_else(|| expr.span.clone());
                self.declare(var_name, span, LocalKind::ForBinder, None);
                self.walk_expr(body);
                self.pop_scope();
            }
            Expr::Range { start, end } => {
                self.walk_expr(start);
                self.walk_expr(end);
            }
            Expr::ArrayLit(items) => {
                for i in items {
                    self.walk_expr(i);
                }
            }
            Expr::Index { object, index } => {
                self.walk_expr(object);
                self.walk_expr(index);
            }
            Expr::StructLit { fields, .. } => {
                // The struct name and field keys are not locals; only the
                // field value expressions can reference bindings.
                for (_, value) in fields {
                    self.walk_expr(value);
                }
            }
            Expr::FieldAccess { object, .. } => self.walk_expr(object),
            Expr::Match { subject, arms } => {
                self.walk_expr(subject);
                for arm in arms {
                    self.walk_match_arm(arm);
                }
            }
            Expr::Interpolation(parts) => {
                for part in parts {
                    if let turbo_ast::InterpolPart::Expr(e) = part {
                        self.walk_expr(e);
                    }
                }
            }
            Expr::Closure { params, body, .. } => {
                self.push_scope();
                for p in params {
                    self.declare_param(p, LocalKind::ClosureParam);
                }
                self.walk_expr(body);
                self.pop_scope();
            }
            Expr::OkExpr(inner)
            | Expr::ErrExpr(inner)
            | Expr::SomeExpr(inner)
            | Expr::Await(inner)
            | Expr::Spawn(inner)
            | Expr::Try(inner) => self.walk_expr(inner),
            Expr::NullCoalesce { value, default } => {
                self.walk_expr(value);
                self.walk_expr(default);
            }
            Expr::OptionalChain { object, .. } => self.walk_expr(object),
            Expr::IfLet {
                pattern,
                value,
                then_branch,
                else_branch,
            } => {
                self.walk_expr(value);
                self.push_scope();
                self.declare_pattern_bindings(pattern);
                self.walk_expr(then_branch);
                self.pop_scope();
                if let Some(e) = else_branch {
                    self.walk_expr(e);
                }
            }
            Expr::MapLit(pairs) => {
                for (k, v) in pairs {
                    self.walk_expr(k);
                    self.walk_expr(v);
                }
            }
        }
    }

    fn walk_match_arm(&mut self, arm: &MatchArm) {
        self.push_scope();
        self.declare_pattern_bindings(&arm.pattern);
        if let Some(guard) = &arm.guard {
            self.walk_expr(guard);
        }
        self.walk_expr(&arm.body);
        self.pop_scope();
    }

    /// Introduce the bindings a pattern brings into scope, recovering each
    /// name's exact span from the token stream within the pattern's span.
    fn declare_pattern_bindings(&mut self, pattern: &Spanned<Pattern>) {
        match &pattern.node {
            Pattern::Ident(name) => {
                // A bare ident is a variable binding unless it names an enum
                // variant (then it is a variant test — sema's rule).
                if !self.enum_variants.iter().any(|v| v == name) {
                    self.declare(name, pattern.span.clone(), LocalKind::PatternBinding, None);
                }
            }
            Pattern::Ok(name) | Pattern::Err(name) | Pattern::Some(name) => {
                if let Some(span) = self.name_span_in(&pattern.span, name) {
                    self.declare(name, span, LocalKind::PatternBinding, None);
                }
            }
            Pattern::VariantDestructure { variant, bindings } => {
                // First ident in the pattern span is the variant name; the
                // remaining idents, in order, are the bindings.
                let idents = self.ident_spans_in(&pattern.span);
                // Skip the leading variant occurrence.
                let mut rest = idents.iter();
                if idents.first().map(|(n, _)| n == variant).unwrap_or(false) {
                    rest.next();
                }
                let binding_spans: Vec<&(String, Span)> = rest.collect();
                for (i, b) in bindings.iter().enumerate() {
                    if let Some((_, span)) = binding_spans.get(i) {
                        self.declare(b, span.clone(), LocalKind::PatternBinding, None);
                    }
                }
            }
            Pattern::Wildcard
            | Pattern::IntLit(_)
            | Pattern::BoolLit(_)
            | Pattern::StringLit(_)
            | Pattern::None => {}
        }
    }
}

/// Render a declared type annotation for hover. Returns `None` for inferred
/// types so we never present a type the source did not actually state.
fn type_label(ty: &TypeExpr) -> Option<String> {
    match ty {
        TypeExpr::Inferred => None,
        // `Self` is the synthetic placeholder the parser gives a bare `self`
        // parameter; it is not a user-written type, so hide it.
        TypeExpr::Named(n) if n == "Self" => None,
        other => Some(crate::format_type(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse + resolve a source buffer for testing.
    fn resolve(src: &str) -> (Vec<LexToken>, Resolution) {
        let (tokens, lex_errors) = turbo_lexer::tokenize(src);
        assert!(lex_errors.is_empty(), "lex errors in test source");
        let (module, parse_errors) = turbo_parser::parse(tokens.clone());
        assert!(
            parse_errors.is_empty(),
            "parse errors in test source: {parse_errors:?}"
        );
        let resolution = resolve_module(&module, &tokens);
        (tokens, resolution)
    }

    /// Span of the `n`-th (0-based) `Ident` token spelled `name`.
    fn nth_ident(tokens: &[LexToken], name: &str, n: usize) -> Span {
        tokens
            .iter()
            .filter(|t| matches!(&t.value, Token::Ident(x) if x == name))
            .nth(n)
            .unwrap_or_else(|| panic!("no {n}-th `{name}` ident"))
            .span
            .clone()
    }

    #[test]
    fn shadowed_let_bindings_resolve_to_distinct_declarations() {
        let src = "fn main() {\n    let x = 1\n    print(x)\n    let x = 2\n    print(x)\n}";
        let (tokens, r) = resolve(src);

        let decl1 = nth_ident(&tokens, "x", 0); // let x = 1
        let use1 = nth_ident(&tokens, "x", 1); // print(x) #1
        let decl2 = nth_ident(&tokens, "x", 2); // let x = 2
        let use2 = nth_ident(&tokens, "x", 3); // print(x) #2

        // use1 binds to the first `let`, use2 to the second (shadow).
        assert_eq!(r.local_at(&use1).unwrap().decl_span, decl1);
        assert_eq!(r.local_at(&use2).unwrap().decl_span, decl2);

        // Each declaration owns exactly its own occurrences.
        assert_eq!(r.occurrences_at(&decl1), vec![decl1.clone(), use1.clone()]);
        assert_eq!(r.occurrences_at(&decl2), vec![decl2.clone(), use2.clone()]);
    }

    #[test]
    fn parameter_resolves_with_declared_type() {
        let src = "fn f(n: int) {\n    print(n)\n}";
        let (tokens, r) = resolve(src);
        let decl = nth_ident(&tokens, "n", 0);
        let use_ = nth_ident(&tokens, "n", 1);

        let info = r.local_at(&use_).expect("use resolves to param");
        assert_eq!(info.decl_span, decl);
        assert_eq!(info.kind, LocalKind::Param);
        assert_eq!(info.ty.as_deref(), Some("int"));
        assert_eq!(r.occurrences_at(&decl), vec![decl.clone(), use_.clone()]);
    }

    #[test]
    fn for_binder_and_param_are_independent_bindings() {
        let src = "fn f(arr: [int]) {\n    for item in arr {\n        print(item)\n    }\n}";
        let (tokens, r) = resolve(src);

        let arr_decl = nth_ident(&tokens, "arr", 0);
        let arr_use = nth_ident(&tokens, "arr", 1); // for item in arr
        let item_decl = nth_ident(&tokens, "item", 0);
        let item_use = nth_ident(&tokens, "item", 1); // print(item)

        assert_eq!(r.local_at(&arr_use).unwrap().decl_span, arr_decl);
        assert_eq!(r.local_at(&item_use).unwrap().kind, LocalKind::ForBinder);
        assert_eq!(r.local_at(&item_use).unwrap().decl_span, item_decl);
        // The two bindings never cross-contaminate.
        assert_eq!(r.occurrences_at(&arr_decl), vec![arr_decl.clone(), arr_use]);
        assert_eq!(
            r.occurrences_at(&item_decl),
            vec![item_decl.clone(), item_use]
        );
    }

    #[test]
    fn closure_param_is_scoped_to_the_closure() {
        let src = "fn f() {\n    let g = |y| { print(y) }\n    print(g)\n}";
        let (tokens, r) = resolve(src);
        let y_decl = nth_ident(&tokens, "y", 0);
        let y_use = nth_ident(&tokens, "y", 1);
        let info = r.local_at(&y_use).expect("closure param use resolves");
        assert_eq!(info.kind, LocalKind::ClosureParam);
        assert_eq!(info.decl_span, y_decl);
    }

    #[test]
    fn match_binding_resolves_and_is_arm_local() {
        let src =
            "fn f(r: int ! str) {\n    match r {\n        ok(v) => print(v)\n        err(e) => print(e)\n    }\n}";
        let (tokens, r) = resolve(src);

        let v_decl = nth_ident(&tokens, "v", 0);
        let v_use = nth_ident(&tokens, "v", 1);
        let e_decl = nth_ident(&tokens, "e", 0);
        let e_use = nth_ident(&tokens, "e", 1);

        assert_eq!(r.local_at(&v_use).unwrap().decl_span, v_decl);
        assert_eq!(r.local_at(&v_use).unwrap().kind, LocalKind::PatternBinding);
        assert_eq!(r.local_at(&e_use).unwrap().decl_span, e_decl);
        // `v` and `e` are distinct bindings local to their own arms.
        assert_eq!(r.occurrences_at(&v_decl), vec![v_decl.clone(), v_use]);
        assert_eq!(r.occurrences_at(&e_decl), vec![e_decl.clone(), e_use]);
    }

    #[test]
    fn let_rhs_refers_to_outer_binding_not_the_new_one() {
        // `let x = x + 1` — the RHS `x` must bind to the *outer* `x`, and the
        // new `x` becomes a separate declaration.
        let src = "fn f(x: int) {\n    let x = x + 1\n    print(x)\n}";
        let (tokens, r) = resolve(src);

        let param_x = nth_ident(&tokens, "x", 0); // fn f(x
        let rhs_x = nth_ident(&tokens, "x", 2); // = x + 1  (idx1 is the let binder)
        let let_x = nth_ident(&tokens, "x", 1); // let x
        let print_x = nth_ident(&tokens, "x", 3); // print(x)

        // RHS use binds to the param, not the new let.
        assert_eq!(r.local_at(&rhs_x).unwrap().decl_span, param_x);
        // The later use binds to the shadowing let.
        assert_eq!(r.local_at(&print_x).unwrap().decl_span, let_x);
    }

    #[test]
    fn top_level_names_are_not_local_occurrences() {
        // A call to a top-level fn is not a local binding — it must fall
        // through to the textual/global path.
        let src = "fn helper() {}\nfn main() {\n    helper()\n}";
        let (tokens, r) = resolve(src);
        let call = nth_ident(&tokens, "helper", 1);
        assert!(r.local_at(&call).is_none());
        assert!(!r.is_local_occurrence(&call));
    }
}
