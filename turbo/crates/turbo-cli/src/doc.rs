//! `turbolang doc`: render Turbo doc comments (`///`) to Markdown.

use std::collections::HashMap;

use crate::diagnostics::report_file_error;

/// Extract doc comments (lines starting with `///`) from source text.
/// Returns a map from line number (0-indexed) to the collected doc comment lines
/// for the item that starts at that line.
pub(crate) fn extract_doc_comments(source: &str) -> HashMap<usize, Vec<String>> {
    let mut docs: HashMap<usize, Vec<String>> = HashMap::new();
    let lines: Vec<&str> = source.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].trim().starts_with("///") {
            let mut comments = Vec::new();
            while i < lines.len() && lines[i].trim().starts_with("///") {
                comments.push(lines[i].trim().trim_start_matches("///").trim().to_string());
                i += 1;
            }
            // Skip any decorator lines (@derive, etc.) between doc comment and item
            while i < lines.len() && lines[i].trim().starts_with('@') {
                i += 1;
            }
            // i now points to the item after the doc comments
            if i < lines.len() {
                docs.insert(i, comments);
            }
        }
        i += 1;
    }
    docs
}

/// A documentation item extracted from source text scanning.
#[derive(Debug)]
pub(crate) enum DocItem {
    Function {
        signature: String,
        doc: Vec<String>,
    },
    Struct {
        name: String,
        fields: Vec<String>,
        doc: Vec<String>,
    },
    Enum {
        name: String,
        variants: Vec<String>,
        doc: Vec<String>,
    },
    Trait {
        name: String,
        methods: Vec<String>,
        doc: Vec<String>,
    },
    Impl {
        target: String,
        methods: Vec<String>,
    },
}

/// Format a TypeExpr to a human-readable string.
pub(crate) fn format_type_expr(ty: &turbo_ast::TypeExpr) -> String {
    match ty {
        turbo_ast::TypeExpr::Named(n) => n.clone(),
        turbo_ast::TypeExpr::Unit => "()".to_string(),
        turbo_ast::TypeExpr::Array(inner) => format!("[{}]", format_type_expr(&inner.node)),
        turbo_ast::TypeExpr::FnType { params, ret } => {
            let p: Vec<String> = params.iter().map(|p| format_type_expr(&p.node)).collect();
            format!("fn({}) -> {}", p.join(", "), format_type_expr(&ret.node))
        }
        turbo_ast::TypeExpr::Result { ok_type, err_type } => {
            format!(
                "{} ! {}",
                format_type_expr(&ok_type.node),
                format_type_expr(&err_type.node)
            )
        }
        turbo_ast::TypeExpr::Optional(inner) => format!("{}?", format_type_expr(&inner.node)),
        turbo_ast::TypeExpr::Future(inner) => format!("Future<{}>", format_type_expr(&inner.node)),
        turbo_ast::TypeExpr::Inferred => "_".to_string(),
    }
}

/// Format a function signature from an AST FnDef.
pub(crate) fn format_fn_signature(f: &turbo_ast::FnDef) -> String {
    let params: Vec<String> = f
        .params
        .iter()
        .map(|p| format!("{}: {}", p.name, format_type_expr(&p.ty.node)))
        .collect();

    let ret = match &f.return_type {
        Some(rt) => format!(" -> {}", format_type_expr(&rt.node)),
        None => String::new(),
    };

    let async_prefix = if f.is_async { "async " } else { "" };

    format!(
        "{}fn {}({}){}",
        async_prefix,
        f.name,
        params.join(", "),
        ret
    )
}

/// Scan source lines for struct definitions and their fields.
pub(crate) fn scan_structs(
    lines: &[&str],
    doc_comments: &HashMap<usize, Vec<String>>,
) -> Vec<DocItem> {
    let mut items = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        let is_struct = trimmed.starts_with("struct ") || trimmed.starts_with("pub struct ");
        if is_struct && trimmed.contains('{') {
            let name = trimmed
                .trim_start_matches("pub ")
                .trim_start_matches("struct ")
                .split('{')
                .next()
                .unwrap_or("")
                .trim()
                .to_string();

            let doc = doc_comments.get(&i).cloned().unwrap_or_default();
            let mut fields = Vec::new();

            // Check if this is a single-line struct: `struct Foo { x: i64, y: i64 }`
            if trimmed.contains('}') {
                // Extract fields from between { and }
                if let Some(start) = trimmed.find('{') {
                    if let Some(end) = trimmed.rfind('}') {
                        let body = trimmed[start + 1..end].trim();
                        if !body.is_empty() {
                            for field_str in body.split(',') {
                                let f = field_str.trim();
                                if !f.is_empty() && !f.starts_with("//") {
                                    fields.push(f.to_string());
                                }
                            }
                        }
                    }
                }
            } else {
                // Multi-line struct: scan subsequent lines for fields
                i += 1;
                while i < lines.len() {
                    let field_line = lines[i].trim();
                    if field_line == "}" || field_line.starts_with('}') {
                        break;
                    }
                    if !field_line.is_empty() && !field_line.starts_with("//") {
                        fields.push(field_line.trim_end_matches(',').to_string());
                    }
                    i += 1;
                }
            }

            items.push(DocItem::Struct { name, fields, doc });
        }
        i += 1;
    }
    items
}

/// Scan source lines for enum definitions (using `type Name {` syntax).
pub(crate) fn scan_enums(
    lines: &[&str],
    doc_comments: &HashMap<usize, Vec<String>>,
) -> Vec<DocItem> {
    let mut items = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        let is_enum = (trimmed.starts_with("type ") || trimmed.starts_with("pub type "))
            && trimmed.contains('{')
            && !trimmed.contains("fn ")
            && !trimmed.contains("let ");
        if is_enum {
            let after_type = trimmed
                .trim_start_matches("pub ")
                .trim_start_matches("type ");
            let name = after_type
                .split('{')
                .next()
                .unwrap_or("")
                .split(':')
                .next()
                .unwrap_or("")
                .trim()
                .to_string();

            let doc = doc_comments.get(&i).cloned().unwrap_or_default();
            let mut variants = Vec::new();
            i += 1;
            while i < lines.len() {
                let variant_line = lines[i].trim();
                if variant_line == "}" || variant_line.starts_with('}') {
                    break;
                }
                if !variant_line.is_empty() && !variant_line.starts_with("//") {
                    variants.push(variant_line.trim_end_matches(',').to_string());
                }
                i += 1;
            }

            items.push(DocItem::Enum {
                name,
                variants,
                doc,
            });
        }
        i += 1;
    }
    items
}

/// Scan source lines for trait definitions.
pub(crate) fn scan_traits(
    lines: &[&str],
    doc_comments: &HashMap<usize, Vec<String>>,
) -> Vec<DocItem> {
    let mut items = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        let is_trait = (trimmed.starts_with("trait ") || trimmed.starts_with("pub trait "))
            && trimmed.contains('{');
        if is_trait {
            let name = trimmed
                .trim_start_matches("pub ")
                .trim_start_matches("trait ")
                .split('{')
                .next()
                .unwrap_or("")
                .trim()
                .to_string();

            let doc = doc_comments.get(&i).cloned().unwrap_or_default();
            let mut methods = Vec::new();
            i += 1;
            let mut brace_depth = 1;
            while i < lines.len() && brace_depth > 0 {
                let method_line = lines[i].trim();
                brace_depth += method_line.matches('{').count();
                brace_depth -= method_line.matches('}').count();
                if (method_line.starts_with("fn ") || method_line.starts_with("pub fn "))
                    && method_line.contains('(')
                {
                    let sig = method_line.split('{').next().unwrap_or(method_line).trim();
                    methods.push(sig.to_string());
                }
                i += 1;
            }

            items.push(DocItem::Trait { name, methods, doc });
        }
        i += 1;
    }
    items
}

/// Scan source lines for impl blocks and their methods.
pub(crate) fn scan_impls(lines: &[&str]) -> Vec<DocItem> {
    let mut items = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        if trimmed.starts_with("impl ") && trimmed.contains('{') {
            let target = trimmed
                .trim_start_matches("impl ")
                .split('{')
                .next()
                .unwrap_or("")
                .trim()
                .to_string();

            let mut methods = Vec::new();
            i += 1;
            let mut brace_depth = 1;
            while i < lines.len() && brace_depth > 0 {
                let method_line = lines[i].trim();
                brace_depth += method_line.matches('{').count();
                brace_depth -= method_line.matches('}').count();
                if (method_line.starts_with("fn ")
                    || method_line.starts_with("pub fn ")
                    || method_line.starts_with("async fn ")
                    || method_line.starts_with("pub async fn "))
                    && method_line.contains('(')
                {
                    let sig = method_line.split('{').next().unwrap_or(method_line).trim();
                    methods.push(sig.to_string());
                }
                i += 1;
            }

            if !methods.is_empty() {
                items.push(DocItem::Impl { target, methods });
            }
        }
        i += 1;
    }
    items
}

/// Scan for top-level `fn` and `async fn` definitions in source text.
pub(crate) fn scan_functions(
    lines: &[&str],
    doc_comments: &HashMap<usize, Vec<String>>,
) -> Vec<DocItem> {
    let mut items = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        let is_fn = (trimmed.starts_with("fn ")
            || trimmed.starts_with("pub fn ")
            || trimmed.starts_with("async fn ")
            || trimmed.starts_with("pub async fn "))
            && trimmed.contains('(');

        if is_fn {
            let sig = trimmed
                .split('{')
                .next()
                .unwrap_or(trimmed)
                .trim()
                .to_string();
            let doc = doc_comments.get(&i).cloned().unwrap_or_default();
            items.push(DocItem::Function {
                signature: sig,
                doc,
            });
        }
        i += 1;
    }
    items
}

pub(crate) fn doc_file(path: &std::path::Path) {
    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => report_file_error(path, &e),
    };

    let filename = path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| "<unknown>".to_string());

    let doc_comments = extract_doc_comments(&source);
    let lines: Vec<&str> = source.lines().collect();

    // Try parsing with the AST for functions (works for Phase 1 files)
    let (tokens, lex_errors) = turbo_lexer::tokenize(&source);
    let ast_functions = if lex_errors.is_empty() {
        let (module, parse_errors) = turbo_parser::parse(tokens);
        if parse_errors.is_empty() {
            Some(module)
        } else {
            None
        }
    } else {
        None
    };

    // Collect all items via source scanning
    let scanned_functions = scan_functions(&lines, &doc_comments);
    let structs = scan_structs(&lines, &doc_comments);
    let enums = scan_enums(&lines, &doc_comments);
    let traits = scan_traits(&lines, &doc_comments);
    let impls = scan_impls(&lines);
    // --- Generate markdown ---
    let mut out = String::new();
    out.push_str(&format!("# Documentation for {}\n", filename));

    // Functions section
    let has_functions = ast_functions.is_some() || !scanned_functions.is_empty();
    if has_functions {
        out.push_str("\n## Functions\n");

        if let Some(ref module) = ast_functions {
            // Use AST for accurate signatures and doc comments
            for item in &module.items {
                if let turbo_ast::Item::Function(f) = &item.node {
                    let sig = format_fn_signature(f);
                    // Prefer AST doc field, fall back to source-scanned doc comments
                    let doc = if let Some(ref d) = f.doc {
                        vec![d.clone()]
                    } else {
                        let fn_line = source[..item.span.start].matches('\n').count();
                        doc_comments.get(&fn_line).cloned().unwrap_or_default()
                    };

                    out.push_str(&format!("\n### `{}`\n", sig));
                    if !doc.is_empty() {
                        out.push_str(&format!("{}\n", doc.join("\n")));
                    }
                }
            }
        } else {
            // Fallback: use scanned functions
            for item in &scanned_functions {
                if let DocItem::Function { signature, doc } = item {
                    out.push_str(&format!("\n### `{}`\n", signature));
                    if !doc.is_empty() {
                        out.push_str(&format!("{}\n", doc.join("\n")));
                    }
                }
            }
        }
    }

    // Structs section — prefer AST for accurate field types, fall back to scanner
    let has_ast_structs = ast_functions.as_ref().is_some_and(|module| {
        module
            .items
            .iter()
            .any(|item| matches!(&item.node, turbo_ast::Item::Struct(_)))
    });

    if let Some(module) = ast_functions.as_ref().filter(|_| has_ast_structs) {
        out.push_str("\n## Structs\n");
        for item in &module.items {
            if let turbo_ast::Item::Struct(s) = &item.node {
                out.push_str(&format!("\n### `struct {}`\n", s.name));
                let doc = if let Some(ref d) = s.doc {
                    vec![d.clone()]
                } else {
                    let struct_line = source[..item.span.start].matches('\n').count();
                    doc_comments.get(&struct_line).cloned().unwrap_or_default()
                };
                if !doc.is_empty() {
                    out.push_str(&format!("{}\n", doc.join("\n")));
                }
                if !s.fields.is_empty() {
                    out.push_str("\nFields:\n");
                    for field in &s.fields {
                        out.push_str(&format!(
                            "- `{}: {}`\n",
                            field.name,
                            format_type_expr(&field.ty.node)
                        ));
                    }
                }
            }
        }
    } else if !structs.is_empty() {
        out.push_str("\n## Structs\n");
        for item in &structs {
            if let DocItem::Struct { name, fields, doc } = item {
                out.push_str(&format!("\n### `struct {}`\n", name));
                if !doc.is_empty() {
                    out.push_str(&format!("{}\n", doc.join("\n")));
                }
                if !fields.is_empty() {
                    out.push_str("\nFields:\n");
                    for field in fields {
                        out.push_str(&format!("- `{}`\n", field));
                    }
                }
            }
        }
    }

    // Enums section
    if !enums.is_empty() {
        out.push_str("\n## Enums\n");
        for item in &enums {
            if let DocItem::Enum {
                name,
                variants,
                doc,
            } = item
            {
                out.push_str(&format!("\n### `type {}`\n", name));
                if !doc.is_empty() {
                    out.push_str(&format!("{}\n", doc.join("\n")));
                }
                if !variants.is_empty() {
                    let variant_names: Vec<&str> = variants
                        .iter()
                        .map(|v| v.split('(').next().unwrap_or(v).trim())
                        .collect();
                    out.push_str(&format!("\nVariants: {}\n", variant_names.join(", ")));
                }
            }
        }
    }

    // Traits section
    if !traits.is_empty() {
        out.push_str("\n## Traits\n");
        for item in &traits {
            if let DocItem::Trait { name, methods, doc } = item {
                out.push_str(&format!("\n### `trait {}`\n", name));
                if !doc.is_empty() {
                    out.push_str(&format!("{}\n", doc.join("\n")));
                }
                if !methods.is_empty() {
                    out.push_str("\nMethods:\n");
                    for method in methods {
                        out.push_str(&format!("- `{}`\n", method));
                    }
                }
            }
        }
    }

    // Impl blocks section
    if !impls.is_empty() {
        out.push_str("\n## Implementations\n");
        for item in &impls {
            if let DocItem::Impl { target, methods } = item {
                out.push_str(&format!("\n### `impl {}`\n", target));
                out.push_str("\nMethods:\n");
                for method in methods {
                    out.push_str(&format!("- `{}`\n", method));
                }
            }
        }
    }

    print!("{}", out);
}
