//! Turbo source formatter — a real AST pretty-printer.
//!
//! Unlike a line-based tidier, this formatter parses the source into the
//! [`turbo_ast::Module`] and re-emits it with canonical layout: 4-space
//! indentation, spaces around binary operators, `: ` after names in type
//! annotations, ` -> ` around return arrows, `, ` after separators, expanded
//! `if`/`while`/`for`/`fn`/`match` bodies, and exactly one blank line between
//! top-level items.
//!
//! # Safety contract
//!
//! Formatting must never change what a program does. The formatter therefore
//! drives layout entirely from the parsed AST and re-verifies its own output
//! before returning it:
//!
//! 1. **Refuses unparseable input.** If the source does not lex/parse cleanly,
//!    the original text is returned unchanged (gofmt-style).
//! 2. **Semantics self-check.** After printing, the candidate output is
//!    re-parsed and compared structurally (ignoring spans) against the input
//!    AST. If they differ for *any* reason, the original text is returned
//!    unchanged. Since the compiler only ever observes the AST, AST-equality is
//!    exactly behaviour-equality.
//! 3. **Comment self-check.** Line comments are reattached by source position;
//!    the multiset of comment texts in the output must equal the input's, or
//!    the original text is returned unchanged. Block comments (`/* */`) cannot
//!    be round-tripped faithfully, so any file containing one is left untouched.
//!
//! The net effect is that the formatter either produces canonical, idempotent
//! output that preserves behaviour and every comment, or it returns the input
//! byte-for-byte. It can never corrupt a program.

use std::path::Path;
use turbo_ast::{
    BinOp, EnumDef, Expr, ExternBlock, FnDef, ImplBlock, Item, MatchArm, Module, Param, Pattern,
    Span, Spanned, Stmt, StructDef, TraitDef, TypeExpr, UnaryOp,
};
use turbo_lexer::{tokenize, Spanned as LexSpanned, Token};

/// Builtins that the parser's COW pass rewrites from statement-position method
/// calls (`xs.push(4)`) into self-assignments (`xs = push(xs, 4)`). The
/// formatter reverses that rewrite for display so it can re-emit the idiomatic
/// method-call form.
const COW_BUILTINS: &[&str] = &[
    "push", "map", "filter", "replace", "upper", "lower", "trim", "repeat", "split", "sort",
    "reverse",
];

/// Format a Turbo source file in place (or check formatting).
pub fn format_file(path: &Path, check: bool) {
    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: could not read file `{}`: {e}", path.display());
            std::process::exit(1);
        }
    };

    // Refuse to reformat source that doesn't lex/parse. A formatter that
    // silently reindents broken code can mask or compound the underlying error;
    // gofmt/rustfmt both decline unparseable input. Leave the file untouched.
    let (tokens, lex_errors) = tokenize(&source);
    if !lex_errors.is_empty() {
        eprintln!(
            "error: {} has syntax errors and was not formatted (run `turbolang check {}`)",
            path.display(),
            path.display()
        );
        std::process::exit(1);
    }
    let (_, parse_errors) = turbo_parser::parse(tokens);
    if !parse_errors.is_empty() {
        eprintln!(
            "error: {} has syntax errors and was not formatted (run `turbolang check {}`)",
            path.display(),
            path.display()
        );
        std::process::exit(1);
    }

    let formatted = format_source(&source);

    if check {
        if source != formatted {
            eprintln!(
                "error: {} is not formatted (run `turbolang fmt {}` to fix)",
                path.display(),
                path.display()
            );
            std::process::exit(1);
        }
    } else if source != formatted {
        if let Err(e) = std::fs::write(path, &formatted) {
            eprintln!("error: could not write file `{}`: {e}", path.display());
            std::process::exit(1);
        }
        eprintln!("\x1b[32m\u{2713}\x1b[0m Formatted {}", path.display());
    } else {
        eprintln!("{} already formatted", path.display());
    }
}

/// Format Turbo source code, returning the canonically-formatted text.
///
/// If the input cannot be parsed, or the formatted output would not be
/// behaviour-identical / comment-preserving, the original `source` is returned
/// unchanged. See the module docs for the full safety contract.
pub fn format_source(source: &str) -> String {
    let (tokens, lex_errors) = tokenize(source);
    if !lex_errors.is_empty() {
        return source.to_string();
    }
    let (module, parse_errors) = turbo_parser::parse(tokens.clone());
    if !parse_errors.is_empty() {
        return source.to_string();
    }

    // Gather comments string-safely from the gaps between real tokens. Returns
    // `None` if a block comment is present (cannot be round-tripped).
    let comments = match extract_comments(source, &tokens) {
        Some(c) => c,
        None => return source.to_string(),
    };

    let mut printer = Printer::new(source, comments);
    printer.print_module(&module);
    let output = printer.finish();

    // Safety net A: behaviour preservation. The compiler only observes the AST,
    // so structural AST equality (modulo spans) is behaviour equality.
    if !ast_equivalent(source, &output) {
        return source.to_string();
    }
    // Safety net B: no comment lost, moved-out, or duplicated.
    if !comments_preserved(source, &output) {
        return source.to_string();
    }

    output
}

// ===========================================================================
// Comments
// ===========================================================================

/// A `//` line comment extracted from the source.
#[derive(Clone)]
struct Comment {
    /// Byte offset of the comment's first `/`.
    start: usize,
    /// The exact comment lexeme (`// ...`), trailing whitespace trimmed.
    text: String,
}

/// Extract every `//` line comment from `source`, in order. Returns `None` if a
/// `/* */` block comment is encountered (those are not round-tripped).
///
/// Comments only ever live in the *gaps* between real tokens (the lexer keeps
/// string literals and newlines as tokens), so scanning the gaps is inherently
/// string-safe: a `//` inside a string literal is part of that literal's token
/// and never appears in a gap.
fn extract_comments(source: &str, tokens: &[LexSpanned<Token>]) -> Option<Vec<Comment>> {
    let bytes = source.as_bytes();
    let mut comments = Vec::new();
    let mut prev_end = 0usize;

    fn scan_gap(
        source: &str,
        bytes: &[u8],
        start: usize,
        end: usize,
        out: &mut Vec<Comment>,
    ) -> Option<()> {
        let mut i = start;
        while i + 1 < end {
            if bytes[i] == b'/' && bytes[i + 1] == b'*' {
                return None; // block comment — bail
            }
            if bytes[i] == b'/' && bytes[i + 1] == b'/' {
                // Line comment runs to the next newline (or the gap end).
                let mut j = i;
                while j < end && bytes[j] != b'\n' {
                    j += 1;
                }
                let text = source[i..j].trim_end().to_string();
                out.push(Comment { start: i, text });
                i = j;
            } else {
                i += 1;
            }
        }
        Some(())
    }

    for tok in tokens {
        scan_gap(source, bytes, prev_end, tok.span.start, &mut comments)?;
        prev_end = tok.span.end;
    }
    scan_gap(source, bytes, prev_end, source.len(), &mut comments)?;

    Some(comments)
}

/// True if the line immediately above byte `pos` is blank (whitespace only).
fn blank_line_above(bytes: &[u8], pos: usize) -> bool {
    let mut i = pos;
    // Walk back over the current line's leading whitespace to its newline.
    while i > 0 && (bytes[i - 1] == b' ' || bytes[i - 1] == b'\t' || bytes[i - 1] == b'\r') {
        i -= 1;
    }
    if i == 0 || bytes[i - 1] != b'\n' {
        return false;
    }
    i -= 1; // step over the newline → end of previous line
    if i > 0 && bytes[i - 1] == b'\r' {
        i -= 1;
    }
    while i > 0 && (bytes[i - 1] == b' ' || bytes[i - 1] == b'\t') {
        i -= 1;
    }
    i > 0 && bytes[i - 1] == b'\n'
}

// ===========================================================================
// Printer
// ===========================================================================

struct Printer<'a> {
    src: &'a str,
    sb: &'a [u8],
    comments: Vec<Comment>,
    /// Cursor into `comments`: the next comment not yet emitted.
    ci: usize,
    out: String,
    indent: usize,
}

impl<'a> Printer<'a> {
    fn new(src: &'a str, comments: Vec<Comment>) -> Self {
        Printer {
            src,
            sb: src.as_bytes(),
            comments,
            ci: 0,
            out: String::new(),
            indent: 0,
        }
    }

    fn finish(mut self) -> String {
        // Flush any comments that trail the last item (EOF comments).
        self.flush_leading(self.src.len(), false);
        while self.out.ends_with("\n\n") {
            self.out.pop();
        }
        if !self.out.is_empty() && !self.out.ends_with('\n') {
            self.out.push('\n');
        }
        self.out
    }

    // --- low-level emission -------------------------------------------------

    fn slice(&self, span: &Span) -> String {
        self.src.get(span.start..span.end).unwrap_or("").to_string()
    }

    /// Emit a content line at the current indentation.
    fn line(&mut self, content: String) {
        for _ in 0..self.indent {
            self.out.push_str("    ");
        }
        self.out.push_str(&content);
        self.out.push('\n');
    }

    /// Emit a blank line, unless we're at the very start or already on a blank.
    fn blank(&mut self) {
        if self.out.is_empty() || self.out.ends_with("\n\n") {
            return;
        }
        self.out.push('\n');
    }

    /// Append a trailing `// comment` to the just-emitted line.
    fn attach_trailing(&mut self, text: &str) {
        if self.out.ends_with('\n') {
            self.out.pop();
        }
        self.out.push(' ');
        self.out.push_str(text);
        self.out.push('\n');
    }

    // --- comment interleaving ----------------------------------------------

    /// Emit every pending standalone comment whose start is before `upto`.
    /// Returns whether anything was emitted. `first` suppresses a leading blank
    /// line for the first element of a container.
    fn flush_leading(&mut self, upto: usize, first: bool) -> bool {
        let mut emitted = false;
        while self.ci < self.comments.len() && self.comments[self.ci].start < upto {
            let start = self.comments[self.ci].start;
            let text = self.comments[self.ci].text.clone();
            let suppress_blank = first && !emitted;
            if !suppress_blank && blank_line_above(self.sb, start) {
                self.blank();
            }
            self.line(text);
            emitted = true;
            self.ci += 1;
        }
        emitted
    }

    /// Emit leading comments and an optional preserved blank line before a node.
    fn before_node(&mut self, start: usize, first: bool) {
        let emitted = self.flush_leading(start, first);
        let node_first = first && !emitted;
        if !node_first && blank_line_above(self.sb, start) {
            self.blank();
        }
    }

    /// Claim any comment that trails the node ending at byte `end` (on the same
    /// physical line) and append it to the last emitted line.
    fn after_node(&mut self, end: usize) {
        while self.ci < self.comments.len() {
            let start = self.comments[self.ci].start;
            if start < end {
                break; // interior comment — leave for the next leading flush
            }
            let between = self.src.get(end..start).unwrap_or("");
            if between.contains('\n') {
                break; // on a later line — not a trailing comment
            }
            let text = self.comments[self.ci].text.clone();
            self.attach_trailing(&text);
            self.ci += 1;
        }
    }

    // --- module / items -----------------------------------------------------

    fn print_module(&mut self, module: &Module) {
        for (idx, item) in module.items.iter().enumerate() {
            if idx > 0 {
                self.blank();
            }
            self.before_node(item.span.start, idx == 0);
            self.print_item(&item.node, item.span.end);
            self.after_node(item.span.end);
        }
    }

    fn print_item(&mut self, item: &Item, end: usize) {
        match item {
            Item::Function(f) => self.print_fn(f),
            Item::Struct(s) => self.print_struct(s, end),
            Item::Enum(e) => self.print_enum(e, end),
            Item::Impl(b) => self.print_impl(b, end),
            Item::Trait(t) => self.print_trait(t),
            Item::Import { names, path } => {
                self.line(format!(
                    "import {{ {} }} from {}",
                    names.join(", "),
                    encode_string(path)
                ));
            }
            Item::Const(c) => {
                let mut head = format!("const {}", c.name);
                if let Some(t) = &c.ty {
                    head.push_str(&format!(": {}", self.type_str(t)));
                }
                head.push_str(" = ");
                self.print_value(head, &c.value);
            }
            Item::Extern(b) => self.print_extern(b),
        }
    }

    fn emit_doc(&mut self, doc: &Option<String>) {
        if let Some(text) = doc {
            for line in text.split('\n') {
                if line.is_empty() {
                    self.line("///".to_string());
                } else {
                    self.line(format!("/// {line}"));
                }
            }
        }
    }

    fn print_fn(&mut self, f: &FnDef) {
        self.emit_doc(&f.doc);
        let mut head = String::new();
        if f.is_test {
            head.push_str("@test ");
        }
        if f.is_bench {
            head.push_str("@bench ");
        }
        if f.is_unsafe {
            head.push_str("@unsafe ");
        }
        if f.is_async {
            head.push_str("async ");
        }
        head.push_str("fn ");
        head.push_str(&f.name);
        head.push_str(&self.type_params_str(&f.type_params));
        head.push('(');
        head.push_str(&self.params_str(&f.params));
        head.push(')');
        if let Some(rt) = &f.return_type {
            head.push_str(&format!(" -> {}", self.type_str(rt)));
        }
        self.line(format!("{head} {{"));
        self.print_block_inner(&f.body);
        self.line("}".to_string());
    }

    fn print_struct(&mut self, s: &StructDef, end: usize) {
        self.emit_doc(&s.doc);
        if !s.derives.is_empty() {
            self.line(format!("@derive({})", s.derives.join(", ")));
        }
        self.line(format!(
            "struct {}{} {{",
            s.name,
            self.type_params_str(&s.type_params)
        ));
        self.indent += 1;
        let mut first = true;
        for field in &s.fields {
            self.before_node(field.ty.span.start, first);
            first = false;
            self.line(format!("{}: {},", field.name, self.type_str(&field.ty)));
            self.after_node(field.ty.span.end);
        }
        self.flush_leading(end, first);
        self.indent -= 1;
        self.line("}".to_string());
    }

    fn print_enum(&mut self, e: &EnumDef, end: usize) {
        self.emit_doc(&e.doc);
        self.line(format!(
            "type {}{} {{",
            e.name,
            self.type_params_str(&e.type_params)
        ));
        self.indent += 1;
        let mut first = true;
        for variant in &e.variants {
            let anchor = variant.fields.first().map(|f| f.span.start).unwrap_or(end);
            self.before_node(anchor, first);
            first = false;
            let last_end = variant.fields.last().map(|f| f.span.end).unwrap_or(anchor);
            if variant.fields.is_empty() {
                self.line(format!("{},", variant.name));
            } else {
                let tys = variant
                    .fields
                    .iter()
                    .map(|t| self.type_str(t))
                    .collect::<Vec<_>>()
                    .join(", ");
                self.line(format!("{}({}),", variant.name, tys));
            }
            self.after_node(last_end);
        }
        self.flush_leading(end, first);
        self.indent -= 1;
        self.line("}".to_string());
    }

    fn print_impl(&mut self, b: &ImplBlock, end: usize) {
        let tps = self.type_params_str(&b.type_params);
        let head = match &b.trait_name {
            Some(t) => format!("impl {} for {}{} {{", t, b.type_name, tps),
            None => format!("impl {}{} {{", b.type_name, tps),
        };
        self.line(head);
        self.indent += 1;
        for (i, m) in b.methods.iter().enumerate() {
            if i > 0 {
                self.blank();
            }
            self.before_node(m.span.start, i == 0);
            self.print_fn(&m.node);
            self.after_node(m.span.end);
        }
        self.flush_leading(end, b.methods.is_empty());
        self.indent -= 1;
        self.line("}".to_string());
    }

    fn print_trait(&mut self, t: &TraitDef) {
        self.line(format!("trait {} {{", t.name));
        self.indent += 1;
        for m in &t.methods {
            let mut head = format!("fn {}({})", m.name, self.params_str(&m.params));
            if let Some(rt) = &m.return_type {
                head.push_str(&format!(" -> {}", self.type_str(rt)));
            }
            if let Some(body) = &m.default_body {
                self.line(format!("{head} {{"));
                self.print_block_inner(body);
                self.line("}".to_string());
            } else {
                self.line(head);
            }
        }
        self.indent -= 1;
        self.line("}".to_string());
    }

    fn print_extern(&mut self, b: &ExternBlock) {
        self.line(format!("@unsafe extern {} {{", encode_string(&b.abi)));
        self.indent += 1;
        for f in &b.functions {
            let mut head = format!("fn {}({})", f.node.name, self.params_str(&f.node.params));
            if let Some(rt) = &f.node.return_type {
                head.push_str(&format!(" -> {}", self.type_str(rt)));
            }
            self.line(head);
        }
        self.indent -= 1;
        self.line("}".to_string());
    }

    // --- blocks & statements ------------------------------------------------

    /// Print the inside of a block (`{ ... }`) — its statements and tail — at
    /// `indent + 1`. The caller emits the opening and closing braces.
    fn print_block_inner(&mut self, block: &Spanned<Expr>) {
        if let Expr::Block { stmts, tail_expr } = &block.node {
            self.indent += 1;
            let mut first = true;
            for s in stmts {
                self.print_stmt(s, first);
                first = false;
            }
            if let Some(t) = tail_expr {
                self.before_node(t.span.start, first);
                self.emit_expr_stmt_pos(t);
                self.after_node(t.span.end);
                first = false;
            }
            self.flush_leading(block.span.end, first);
            self.indent -= 1;
        }
    }

    fn print_stmt(&mut self, s: &Spanned<Stmt>, first: bool) {
        self.before_node(s.span.start, first);
        match &s.node {
            Stmt::Let {
                mutable,
                name,
                ty,
                value,
            } => {
                let mut head = String::from("let ");
                if *mutable {
                    head.push_str("mut ");
                }
                head.push_str(name);
                if let Some(t) = ty {
                    head.push_str(&format!(": {}", self.type_str(t)));
                }
                head.push_str(" = ");
                self.print_value(head, value);
            }
            Stmt::LetDestructure {
                mutable,
                fields,
                value,
            } => {
                let mut head = String::from("let ");
                if *mutable {
                    head.push_str("mut ");
                }
                head.push_str(&format!("{{ {} }} = ", fields.join(", ")));
                self.print_value(head, value);
            }
            Stmt::Return(opt) => match opt {
                None => self.line("return".to_string()),
                Some(v) => self.print_value("return ".to_string(), v),
            },
            Stmt::Defer(e) => self.print_value("defer ".to_string(), e),
            Stmt::Expr(e) => self.emit_stmt_expr(e),
        }
        self.after_node(s.span.end);
    }

    /// A `Stmt::Expr`: handle COW reversal and assignment heads, otherwise treat
    /// like a statement-position expression.
    fn emit_stmt_expr(&mut self, e: &Spanned<Expr>) {
        if let Some(method) = self.try_reverse_cow(&e.node) {
            self.line(method);
            return;
        }
        match &e.node {
            Expr::Assign { target, value } => self.print_value(format!("{target} = "), value),
            Expr::CompoundAssign { target, op, value } => {
                self.print_value(format!("{target} {}= ", binop_sym(*op)), value)
            }
            Expr::FieldAssign {
                object,
                field,
                value,
            } => {
                let head = format!("{}.{} = ", self.paren_postfix(object), field);
                self.print_value(head, value)
            }
            Expr::IndexAssign {
                object,
                index,
                value,
            } => {
                let head = format!(
                    "{}[{}] = ",
                    self.paren_postfix(object),
                    self.inline_expr(index)
                );
                self.print_value(head, value)
            }
            _ => self.emit_expr_stmt_pos(e),
        }
    }

    /// Emit an expression in statement / tail position: block-like expressions
    /// are expanded across lines; everything else is a single inline line.
    fn emit_expr_stmt_pos(&mut self, e: &Spanned<Expr>) {
        if is_block_like(&e.node) {
            self.print_block_like(String::new(), e);
        } else {
            let s = self.inline_expr(e);
            self.line(s);
        }
    }

    /// Emit `head` followed by `value`. Block-like values are expanded with the
    /// head as the opening-line prefix (e.g. `let x = match … {`).
    fn print_value(&mut self, head: String, value: &Spanned<Expr>) {
        if is_block_like(&value.node) {
            self.print_block_like(head, value);
        } else {
            let s = self.inline_expr(value);
            self.line(format!("{head}{s}"));
        }
    }

    /// Expand a block-like expression (`if`/`while`/`for`/`match`/block) across
    /// multiple lines, prefixed by `prefix` on its opening line.
    fn print_block_like(&mut self, prefix: String, e: &Spanned<Expr>) {
        match &e.node {
            Expr::If { .. } | Expr::IfLet { .. } => self.print_if(prefix, e),
            Expr::While { condition, body } => {
                self.line(format!("{prefix}while {} {{", self.inline_expr(condition)));
                self.print_block_inner(body);
                self.line("}".to_string());
            }
            Expr::ForIn {
                var_name,
                iterable,
                body,
            } => {
                self.line(format!(
                    "{prefix}for {} in {} {{",
                    var_name,
                    self.inline_expr(iterable)
                ));
                self.print_block_inner(body);
                self.line("}".to_string());
            }
            Expr::Match { subject, arms } => self.print_match(prefix, subject, arms, e.span.end),
            Expr::Block { .. } => {
                self.line(format!("{prefix}{{"));
                self.print_block_inner(e);
                self.line("}".to_string());
            }
            _ => {
                let s = self.inline_expr(e);
                self.line(format!("{prefix}{s}"));
            }
        }
    }

    fn print_if(&mut self, prefix: String, e: &Spanned<Expr>) {
        let mut cur = e;
        let mut pre = prefix;
        loop {
            let (cond, then_b, else_b) = match &cur.node {
                Expr::If {
                    condition,
                    then_branch,
                    else_branch,
                } => (
                    format!("if {}", self.inline_expr(condition)),
                    then_branch,
                    else_branch,
                ),
                Expr::IfLet {
                    pattern,
                    value,
                    then_branch,
                    else_branch,
                } => (
                    format!(
                        "if let {} = {}",
                        self.pattern_str(pattern),
                        self.inline_expr(value)
                    ),
                    then_branch,
                    else_branch,
                ),
                _ => {
                    self.emit_expr_stmt_pos(cur);
                    return;
                }
            };
            self.line(format!("{pre}{cond} {{"));
            self.print_block_inner(then_b);
            match else_b {
                None => {
                    self.line("}".to_string());
                    return;
                }
                Some(eb) => match &eb.node {
                    Expr::If { .. } | Expr::IfLet { .. } => {
                        pre = "} else ".to_string();
                        cur = eb;
                        continue;
                    }
                    _ => {
                        self.line("} else {".to_string());
                        self.print_block_inner(eb);
                        self.line("}".to_string());
                        return;
                    }
                },
            }
        }
    }

    fn print_match(
        &mut self,
        prefix: String,
        subject: &Spanned<Expr>,
        arms: &[MatchArm],
        end: usize,
    ) {
        self.line(format!("{prefix}match {} {{", self.inline_expr(subject)));
        self.indent += 1;
        let mut first = true;
        for arm in arms {
            self.before_node(arm.pattern.span.start, first);
            first = false;
            let mut head = self.pattern_str(&arm.pattern);
            if let Some(g) = &arm.guard {
                head.push_str(&format!(" if {}", self.inline_expr(g)));
            }
            if matches!(arm.body.node, Expr::Block { .. }) {
                self.line(format!("{head} => {{"));
                self.print_block_inner(&arm.body);
                self.line("}".to_string());
            } else if is_block_like(&arm.body.node) {
                self.print_block_like(format!("{head} => "), &arm.body);
            } else {
                let b = self.inline_expr(&arm.body);
                self.line(format!("{head} => {b}"));
            }
            self.after_node(arm.body.span.end);
        }
        self.flush_leading(end, first);
        self.indent -= 1;
        self.line("}".to_string());
    }

    /// Detect a COW self-assignment (`xs = push(xs, …)`, `s = upper(trim(s))`)
    /// produced by the parser's COW rewrite of a statement-position COW call,
    /// and return the bare call rendering (`xs.push(…)`, `s.trim().upper()`).
    ///
    /// Emitting the bare call — rather than the self-assignment — is what keeps
    /// the round-trip exact: re-parsing the bare call re-applies the very same
    /// COW rewrite (including moving a statement-context tail back into `stmts`),
    /// so the AST is reproduced identically. Only fires in statement position.
    fn try_reverse_cow(&self, node: &Expr) -> Option<String> {
        let (target_str, value): (String, &Spanned<Expr>) = match node {
            Expr::Assign { target, value } => (target.clone(), value),
            Expr::FieldAssign {
                object,
                field,
                value,
            } => (format!("{}.{}", self.paren_postfix(object), field), value),
            Expr::IndexAssign {
                object,
                index,
                value,
            } => (
                format!(
                    "{}[{}]",
                    self.paren_postfix(object),
                    self.inline_expr(index)
                ),
                value,
            ),
            _ => return None,
        };
        if self.cow_chain_root(value).as_deref() == Some(target_str.as_str()) {
            return Some(self.inline_expr(value));
        }
        None
    }

    /// Follow a (possibly chained) COW-builtin call down its receiver chain and
    /// render the lvalue at its root, e.g. `upper(trim(s))` → `s`. Returns
    /// `None` if `value` is not a COW call rooted in a simple lvalue.
    fn cow_chain_root(&self, value: &Spanned<Expr>) -> Option<String> {
        if let Expr::Call { callee, args } = &value.node {
            if let Expr::Ident(name) = &callee.node {
                if COW_BUILTINS.contains(&name.as_str()) && !args.is_empty() {
                    if let Some(inner) = self.cow_chain_root(&args[0]) {
                        return Some(inner);
                    }
                    return self.render_lvalue(&args[0].node);
                }
            }
        }
        None
    }

    fn render_lvalue(&self, e: &Expr) -> Option<String> {
        match e {
            Expr::Ident(n) => Some(n.clone()),
            Expr::FieldAccess { object, field } => {
                Some(format!("{}.{}", self.paren_postfix(object), field))
            }
            Expr::Index { object, index } => Some(format!(
                "{}[{}]",
                self.paren_postfix(object),
                self.inline_expr(index)
            )),
            _ => None,
        }
    }

    // --- inline expression printing ----------------------------------------

    fn inline_expr(&self, e: &Spanned<Expr>) -> String {
        match &e.node {
            // Literals are sliced verbatim from source so escapes, underscores,
            // float spellings, and interpolation text round-trip exactly.
            Expr::IntLit(_) | Expr::FloatLit(_) | Expr::StringLit(_) | Expr::Interpolation(_) => {
                self.slice(&e.span)
            }
            Expr::BoolLit(b) => b.to_string(),
            Expr::Unit => "()".to_string(),
            Expr::Ident(s) => s.clone(),
            Expr::BinaryOp { left, op, right } => {
                let p = op.precedence();
                let ls = self.paren_binop_left(left, p);
                let rs = self.paren_binop_right(right, p);
                format!("{ls} {} {rs}", binop_sym(*op))
            }
            Expr::UnaryOp { op, expr } => {
                format!("{}{}", unary_sym(*op), self.paren_atom(expr))
            }
            Expr::Call { callee, args } => self.inline_call(callee, args),
            Expr::Assign { target, value } => format!("{} = {}", target, self.inline_expr(value)),
            Expr::CompoundAssign { target, op, value } => {
                format!("{} {}= {}", target, binop_sym(*op), self.inline_expr(value))
            }
            Expr::FieldAssign {
                object,
                field,
                value,
            } => format!(
                "{}.{} = {}",
                self.paren_postfix(object),
                field,
                self.inline_expr(value)
            ),
            Expr::IndexAssign {
                object,
                index,
                value,
            } => format!(
                "{}[{}] = {}",
                self.paren_postfix(object),
                self.inline_expr(index),
                self.inline_expr(value)
            ),
            Expr::Range { start, end } => {
                format!("{}..{}", self.paren_atom(start), self.paren_atom(end))
            }
            Expr::ArrayLit(elems) => format!("[{}]", self.inline_list(elems)),
            Expr::Index { object, index } => {
                format!(
                    "{}[{}]",
                    self.paren_postfix(object),
                    self.inline_expr(index)
                )
            }
            Expr::StructLit { name, fields } => {
                if fields.is_empty() {
                    format!("{name} {{}}")
                } else {
                    let inner = fields
                        .iter()
                        .map(|(k, v)| format!("{k}: {}", self.inline_expr(v)))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("{name} {{ {inner} }}")
                }
            }
            Expr::FieldAccess { object, field } => {
                format!("{}.{}", self.paren_postfix(object), field)
            }
            Expr::EnumVariant { enum_name, variant } => format!("{enum_name}.{variant}"),
            Expr::Match { .. }
            | Expr::If { .. }
            | Expr::IfLet { .. }
            | Expr::While { .. }
            | Expr::ForIn { .. }
            | Expr::Block { .. } => self.inline_block_like(e),
            Expr::Closure {
                params,
                return_type,
                body,
            } => self.inline_closure(e.span.start, params, return_type, body),
            Expr::OkExpr(v) => format!("ok({})", self.inline_expr(v)),
            Expr::ErrExpr(v) => format!("err({})", self.inline_expr(v)),
            Expr::SomeExpr(v) => format!("some({})", self.inline_expr(v)),
            Expr::NoneExpr => "none".to_string(),
            Expr::NullCoalesce { value, default } => {
                format!(
                    "{} ?? {}",
                    self.inline_expr(value),
                    self.inline_expr(default)
                )
            }
            Expr::OptionalChain { object, field } => {
                format!("{}?.{}", self.paren_postfix(object), field)
            }
            Expr::Await(v) => format!("await {}", self.paren_atom(v)),
            Expr::Spawn(v) => format!("spawn {}", self.paren_atom(v)),
            Expr::Try(v) => format!("{}?", self.paren_postfix(v)),
            // `as` binds looser than any binary operator, so parenthesise a
            // binary/range/assign operand (`(a + b) as u8`); atoms, unary and
            // postfix forms need no parens.
            Expr::Cast { expr, ty } => {
                format!("{} as {}", self.paren_atom(expr), self.type_str(ty))
            }
            Expr::Break => "break".to_string(),
            Expr::Continue => "continue".to_string(),
            Expr::MapLit(entries) => {
                if entries.is_empty() {
                    "{}".to_string()
                } else {
                    let inner = entries
                        .iter()
                        .map(|(k, v)| format!("{}: {}", self.inline_expr(k), self.inline_expr(v)))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("{{{inner}}}")
                }
            }
        }
    }

    fn inline_list(&self, items: &[Spanned<Expr>]) -> String {
        items
            .iter()
            .map(|a| self.inline_expr(a))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Render a call, reconstructing method-call syntax (`recv.method(args)`)
    /// when the callee name appears after the first argument in the source —
    /// which is exactly how the parser desugars `recv.method(args)`.
    fn inline_call(&self, callee: &Spanned<Expr>, args: &[Spanned<Expr>]) -> String {
        if let Expr::Ident(name) = &callee.node {
            if !args.is_empty() && callee.span.start > args[0].span.start {
                let recv = self.paren_postfix(&args[0]);
                let rest = self.inline_list(&args[1..]);
                return format!("{recv}.{name}({rest})");
            }
        }
        format!("{}({})", self.paren_postfix(callee), self.inline_list(args))
    }

    fn inline_closure(
        &self,
        cstart: usize,
        params: &[Param],
        return_type: &Option<Spanned<TypeExpr>>,
        body: &Spanned<Expr>,
    ) -> String {
        let is_pipe = self.sb.get(cstart) == Some(&b'|');
        let params_s = params
            .iter()
            .map(|p| self.closure_param(p))
            .collect::<Vec<_>>()
            .join(", ");
        let mut s = if is_pipe {
            format!("|{params_s}|")
        } else {
            format!("({params_s})")
        };
        if let Some(rt) = return_type {
            s.push_str(&format!(" -> {}", self.type_str(rt)));
        }
        let braced = self.sb.get(body.span.start) == Some(&b'{');
        if braced {
            let inner = self.inline_block_contents(body);
            if is_pipe {
                if inner.is_empty() {
                    s.push_str(" {}");
                } else {
                    s.push_str(&format!(" {{ {inner} }}"));
                }
            } else if inner.is_empty() {
                s.push_str(" => {}");
            } else {
                s.push_str(&format!(" => {{ {inner} }}"));
            }
        } else {
            // Unbraced body: a single tail expression.
            let inner = if let Expr::Block {
                tail_expr: Some(t), ..
            } = &body.node
            {
                self.inline_expr(t)
            } else {
                self.inline_expr(body)
            };
            if is_pipe {
                s.push_str(&format!(" {inner}"));
            } else {
                s.push_str(&format!(" => {inner}"));
            }
        }
        s
    }

    fn closure_param(&self, p: &Param) -> String {
        if matches!(p.ty.node, TypeExpr::Inferred) {
            p.name.clone()
        } else {
            format!("{}: {}", p.name, self.type_str(&p.ty))
        }
    }

    /// Inline (single-line) rendering of a block-like expression, used when one
    /// appears nested inside a larger inline expression.
    fn inline_block_like(&self, e: &Spanned<Expr>) -> String {
        match &e.node {
            Expr::Block { .. } => self.inline_block(e),
            Expr::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let mut s = format!(
                    "if {} {}",
                    self.inline_expr(condition),
                    self.inline_block(then_branch)
                );
                if let Some(eb) = else_branch {
                    s.push_str(&format!(" else {}", self.inline_block_like(eb)));
                }
                s
            }
            Expr::IfLet {
                pattern,
                value,
                then_branch,
                else_branch,
            } => {
                let mut s = format!(
                    "if let {} = {} {}",
                    self.pattern_str(pattern),
                    self.inline_expr(value),
                    self.inline_block(then_branch)
                );
                if let Some(eb) = else_branch {
                    s.push_str(&format!(" else {}", self.inline_block_like(eb)));
                }
                s
            }
            Expr::While { condition, body } => format!(
                "while {} {}",
                self.inline_expr(condition),
                self.inline_block(body)
            ),
            Expr::ForIn {
                var_name,
                iterable,
                body,
            } => format!(
                "for {} in {} {}",
                var_name,
                self.inline_expr(iterable),
                self.inline_block(body)
            ),
            Expr::Match { subject, arms } => {
                let body = arms
                    .iter()
                    .map(|arm| self.inline_arm(arm))
                    .collect::<Vec<_>>()
                    .join(" ");
                if body.is_empty() {
                    format!("match {} {{}}", self.inline_expr(subject))
                } else {
                    format!("match {} {{ {body} }}", self.inline_expr(subject))
                }
            }
            _ => self.inline_expr(e),
        }
    }

    fn inline_arm(&self, arm: &MatchArm) -> String {
        let mut head = self.pattern_str(&arm.pattern);
        if let Some(g) = &arm.guard {
            head.push_str(&format!(" if {}", self.inline_expr(g)));
        }
        let body = if matches!(arm.body.node, Expr::Block { .. }) {
            self.inline_block(&arm.body)
        } else if is_block_like(&arm.body.node) {
            self.inline_block_like(&arm.body)
        } else {
            self.inline_expr(&arm.body)
        };
        format!("{head} => {body}")
    }

    fn inline_block(&self, b: &Spanned<Expr>) -> String {
        let inner = self.inline_block_contents(b);
        if inner.is_empty() {
            "{}".to_string()
        } else {
            format!("{{ {inner} }}")
        }
    }

    fn inline_block_contents(&self, b: &Spanned<Expr>) -> String {
        if let Expr::Block { stmts, tail_expr } = &b.node {
            let mut parts = Vec::new();
            for s in stmts {
                parts.push(self.inline_stmt(s));
            }
            if let Some(t) = tail_expr {
                parts.push(self.inline_expr(t));
            }
            parts.join(" ")
        } else {
            String::new()
        }
    }

    fn inline_stmt(&self, s: &Spanned<Stmt>) -> String {
        match &s.node {
            Stmt::Let {
                mutable,
                name,
                ty,
                value,
            } => {
                let mut head = String::from("let ");
                if *mutable {
                    head.push_str("mut ");
                }
                head.push_str(name);
                if let Some(t) = ty {
                    head.push_str(&format!(": {}", self.type_str(t)));
                }
                format!("{head} = {}", self.inline_expr(value))
            }
            Stmt::LetDestructure {
                mutable,
                fields,
                value,
            } => {
                let m = if *mutable { "mut " } else { "" };
                format!(
                    "let {m}{{ {} }} = {}",
                    fields.join(", "),
                    self.inline_expr(value)
                )
            }
            Stmt::Return(None) => "return".to_string(),
            Stmt::Return(Some(v)) => format!("return {}", self.inline_expr(v)),
            Stmt::Defer(e) => format!("defer {}", self.inline_expr(e)),
            Stmt::Expr(e) => {
                if let Some(method) = self.try_reverse_cow(&e.node) {
                    method
                } else {
                    self.inline_expr(e)
                }
            }
        }
    }

    // --- parenthesisation ---------------------------------------------------

    fn paren_binop_left(&self, e: &Spanned<Expr>, parent: u8) -> String {
        let s = self.inline_expr(e);
        if prec_of(&e.node) < parent {
            format!("({s})")
        } else {
            s
        }
    }

    fn paren_binop_right(&self, e: &Spanned<Expr>, parent: u8) -> String {
        let s = self.inline_expr(e);
        if prec_of(&e.node) <= parent {
            format!("({s})")
        } else {
            s
        }
    }

    /// Parenthesise an operand of a prefix/`..`/`await`/`spawn` form if it is a
    /// lower-precedence (binary/range/assignment) expression.
    fn paren_atom(&self, e: &Spanned<Expr>) -> String {
        let s = self.inline_expr(e);
        if prec_of(&e.node) < 100 {
            format!("({s})")
        } else {
            s
        }
    }

    /// Parenthesise the receiver/object of a postfix form (call, index, field,
    /// `?`) unless it is already a primary/postfix expression.
    fn paren_postfix(&self, e: &Spanned<Expr>) -> String {
        let s = self.inline_expr(e);
        if is_postfix_primary(&e.node) {
            s
        } else {
            format!("({s})")
        }
    }

    // --- patterns & types ---------------------------------------------------

    fn pattern_str(&self, p: &Spanned<Pattern>) -> String {
        match &p.node {
            Pattern::Ident(s) => s.clone(),
            Pattern::Wildcard => "_".to_string(),
            Pattern::IntLit(_) | Pattern::StringLit(_) => self.slice(&p.span),
            Pattern::BoolLit(b) => b.to_string(),
            Pattern::Ok(b) => format!("ok({b})"),
            Pattern::Err(b) => format!("err({b})"),
            Pattern::Some(b) => format!("some({b})"),
            Pattern::None => "none".to_string(),
            Pattern::VariantDestructure { variant, bindings } => {
                format!("{}({})", variant, bindings.join(", "))
            }
        }
    }

    /// Render a type by normalising whitespace in its source slice. The parser
    /// discards generic type arguments from the AST, so slicing source is the
    /// only faithful way to reproduce `Vec<i64>`, `HashMap<str, Vec<i64>>`, etc.
    fn type_str(&self, t: &Spanned<TypeExpr>) -> String {
        let raw = self.slice(&t.span);
        let normalized = raw.split_whitespace().collect::<Vec<_>>().join(" ");
        if normalized.is_empty() {
            // Fall back to a structural rendering if the span is unavailable.
            match &t.node {
                TypeExpr::Named(n) => n.clone(),
                TypeExpr::Unit => "()".to_string(),
                _ => normalized,
            }
        } else {
            normalized
        }
    }

    fn params_str(&self, params: &[Param]) -> String {
        params
            .iter()
            .map(|p| self.fn_param(p))
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn fn_param(&self, p: &Param) -> String {
        // Bare `self` carries a synthetic `Self` type placeholder; re-emit it
        // without an annotation. (`self: Self` produces the same AST, so this
        // stays behaviour-identical.)
        if p.name == "self" && matches!(&p.ty.node, TypeExpr::Named(n) if n == "Self") {
            return "self".to_string();
        }
        let mut s = String::new();
        if p.mutable {
            s.push_str("mut ");
        }
        s.push_str(&p.name);
        s.push_str(&format!(": {}", self.type_str(&p.ty)));
        s
    }

    fn type_params_str(&self, tps: &[turbo_ast::TypeParam]) -> String {
        if tps.is_empty() {
            return String::new();
        }
        let inner = tps
            .iter()
            .map(|tp| {
                if tp.bounds.is_empty() {
                    tp.name.clone()
                } else {
                    format!("{}: {}", tp.name, tp.bounds.join(" + "))
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!("<{inner}>")
    }
}

// ===========================================================================
// Free helpers
// ===========================================================================

fn is_block_like(e: &Expr) -> bool {
    matches!(
        e,
        Expr::If { .. }
            | Expr::IfLet { .. }
            | Expr::While { .. }
            | Expr::ForIn { .. }
            | Expr::Match { .. }
            | Expr::Block { .. }
    )
}

/// Precedence used for operand parenthesisation. Binary operators use the
/// language's own precedence (1..=6); assignment/range/null-coalesce bind
/// looser than any binary operator; everything else is atomic.
fn prec_of(e: &Expr) -> u8 {
    match e {
        Expr::BinaryOp { op, .. } => op.precedence(),
        Expr::Range { .. }
        | Expr::NullCoalesce { .. }
        | Expr::Assign { .. }
        | Expr::CompoundAssign { .. }
        | Expr::FieldAssign { .. }
        | Expr::IndexAssign { .. } => 0,
        _ => 100,
    }
}

/// Whether an expression can directly carry a postfix (`.f`, `(…)`, `[…]`, `?`)
/// without needing surrounding parentheses.
fn is_postfix_primary(e: &Expr) -> bool {
    matches!(
        e,
        Expr::IntLit(_)
            | Expr::FloatLit(_)
            | Expr::StringLit(_)
            | Expr::BoolLit(_)
            | Expr::Unit
            | Expr::Ident(_)
            | Expr::Call { .. }
            | Expr::Index { .. }
            | Expr::FieldAccess { .. }
            | Expr::OptionalChain { .. }
            | Expr::Try { .. }
            | Expr::StructLit { .. }
            | Expr::ArrayLit(_)
            | Expr::MapLit(_)
            | Expr::EnumVariant { .. }
            | Expr::Interpolation(_)
            | Expr::OkExpr(_)
            | Expr::ErrExpr(_)
            | Expr::SomeExpr(_)
            | Expr::NoneExpr
            | Expr::Break
            | Expr::Continue
    )
}

fn binop_sym(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Mod => "%",
        BinOp::Eq => "==",
        BinOp::NotEq => "!=",
        BinOp::Less => "<",
        BinOp::LessEq => "<=",
        BinOp::Greater => ">",
        BinOp::GreaterEq => ">=",
        BinOp::And => "&&",
        BinOp::Or => "||",
    }
}

fn unary_sym(op: UnaryOp) -> &'static str {
    match op {
        UnaryOp::Neg => "-",
        UnaryOp::Not => "!",
    }
}

/// Encode a string value as a Turbo string literal. Used for the few strings
/// the AST stores without a span (import paths, extern ABIs).
fn encode_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '{' => out.push_str("\\{"),
            '}' => out.push_str("\\}"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

// ===========================================================================
// Self-checks
// ===========================================================================

/// Parse `src` into a `Module`, returning `None` on any lex/parse error.
fn parse_module(src: &str) -> Option<Module> {
    let (tokens, lex_errors) = tokenize(src);
    if !lex_errors.is_empty() {
        return None;
    }
    let (module, parse_errors) = turbo_parser::parse(tokens);
    if !parse_errors.is_empty() {
        return None;
    }
    Some(module)
}

/// True if `a` and `b` parse to structurally-identical ASTs (ignoring spans).
/// The compiler only ever observes the AST, so this is behaviour equality.
fn ast_equivalent(a: &str, b: &str) -> bool {
    match (parse_module(a), parse_module(b)) {
        (Some(ma), Some(mb)) => strip_spans(&format!("{ma:?}")) == strip_spans(&format!("{mb:?}")),
        _ => false,
    }
}

/// Remove span byte-ranges from a `Debug`-formatted AST so two ASTs that differ
/// only in source positions compare equal. Spans render as `span: START..END`.
fn strip_spans(debug: &str) -> String {
    let needle = "span: ";
    let bytes = debug.as_bytes();
    let mut out = String::with_capacity(debug.len());
    let mut i = 0;
    while i < debug.len() {
        if debug[i..].starts_with(needle) {
            out.push_str("span: 0..0");
            i += needle.len();
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                i += 1;
            }
        } else {
            let ch = debug[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

/// True if the formatted output preserves exactly the same multiset of line
/// comments as the input (none lost, moved out of the file, or duplicated).
fn comments_preserved(source: &str, output: &str) -> bool {
    let texts = |s: &str| -> Option<Vec<String>> {
        let (tokens, lex_errors) = tokenize(s);
        if !lex_errors.is_empty() {
            return None;
        }
        let mut v: Vec<String> = extract_comments(s, &tokens)?
            .into_iter()
            .map(|c| c.text)
            .collect();
        v.sort();
        Some(v)
    };
    match (texts(source), texts(output)) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

#[cfg(test)]
mod tests;
