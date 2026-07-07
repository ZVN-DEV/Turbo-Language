//! Import resolution: reading, lexing, and parsing imported `.tb` files and
//! inlining the requested items (with transitive closure and cycle checks).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use turbo_ast::{Expr, InterpolPart, Item, Module, Stmt, TypeExpr};

/// Resolve the file path for an import.
/// Resolution order:
/// 1. Relative path from `base_dir` (existing behavior for `./foo` paths)
/// 2. `turbo_modules/{module_name}/src/lib.tb` (package entry point)
/// 3. `turbo_modules/{module_name}/src/{module_name}.tb` (named entry point)
pub(crate) fn resolve_import_path(base_dir: &Path, import_path: &str) -> PathBuf {
    // If the path starts with "./" or "../", resolve relative to base_dir
    if import_path.starts_with("./") || import_path.starts_with("../") {
        let mut path = base_dir.join(import_path);
        if path.extension().is_none() {
            path.set_extension("tb");
        }
        return path;
    }

    // First try the old relative behavior (for backwards compatibility)
    let mut relative_path = base_dir.join(import_path);
    if relative_path.extension().is_none() {
        relative_path.set_extension("tb");
    }
    if relative_path.exists() {
        return relative_path;
    }

    // Try turbo_modules/{module_name}/src/lib.tb
    // Walk up from base_dir to find the project root (where turbo_modules lives)
    let mut search_dir = base_dir.to_path_buf();
    loop {
        let modules_dir = search_dir.join("turbo_modules");
        if modules_dir.is_dir() {
            let lib_path = modules_dir.join(import_path).join("src/lib.tb");
            if lib_path.exists() {
                return lib_path;
            }
            let named_path = modules_dir
                .join(import_path)
                .join("src")
                .join(format!("{}.tb", import_path));
            if named_path.exists() {
                return named_path;
            }
            // Module dir exists but no source found -- fall through to return the lib.tb
            // path so we get a clear error message
            if modules_dir.join(import_path).is_dir() {
                return lib_path;
            }
        }
        if !search_dir.pop() {
            break;
        }
    }

    // Fallback: return the relative path (will produce an error downstream)
    relative_path
}

/// Walk an expression and collect every identifier / type name / struct name
/// it references. Used by `resolve_imports()` to pull in transitively
/// referenced top-level items from the same imported module (so users don't
/// have to name every helper in their `import { ... }` clause).
pub(crate) fn collect_names_in_expr(e: &Expr, out: &mut HashSet<String>) {
    match e {
        Expr::Ident(name) => {
            out.insert(name.clone());
        }
        Expr::StructLit { name, fields } => {
            out.insert(name.clone());
            for (_, v) in fields {
                collect_names_in_expr(&v.node, out);
            }
        }
        Expr::EnumVariant { enum_name, .. } => {
            out.insert(enum_name.clone());
        }
        Expr::Call { callee, args } => {
            collect_names_in_expr(&callee.node, out);
            for a in args {
                collect_names_in_expr(&a.node, out);
            }
        }
        Expr::BinaryOp { left, right, .. } => {
            collect_names_in_expr(&left.node, out);
            collect_names_in_expr(&right.node, out);
        }
        Expr::UnaryOp { expr, .. } => collect_names_in_expr(&expr.node, out),
        Expr::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_names_in_expr(&condition.node, out);
            collect_names_in_expr(&then_branch.node, out);
            if let Some(e) = else_branch {
                collect_names_in_expr(&e.node, out);
            }
        }
        Expr::Block { stmts, tail_expr } => {
            for s in stmts {
                collect_names_in_stmt(&s.node, out);
            }
            if let Some(t) = tail_expr {
                collect_names_in_expr(&t.node, out);
            }
        }
        Expr::Assign { value, .. } => collect_names_in_expr(&value.node, out),
        Expr::CompoundAssign { value, .. } => collect_names_in_expr(&value.node, out),
        Expr::FieldAssign { object, value, .. } => {
            collect_names_in_expr(&object.node, out);
            collect_names_in_expr(&value.node, out);
        }
        Expr::IndexAssign {
            object,
            index,
            value,
        } => {
            collect_names_in_expr(&object.node, out);
            collect_names_in_expr(&index.node, out);
            collect_names_in_expr(&value.node, out);
        }
        Expr::While { condition, body } => {
            collect_names_in_expr(&condition.node, out);
            collect_names_in_expr(&body.node, out);
        }
        Expr::ForIn { iterable, body, .. } => {
            collect_names_in_expr(&iterable.node, out);
            collect_names_in_expr(&body.node, out);
        }
        Expr::Range { start, end } => {
            collect_names_in_expr(&start.node, out);
            collect_names_in_expr(&end.node, out);
        }
        Expr::ArrayLit(elements) => {
            for el in elements {
                collect_names_in_expr(&el.node, out);
            }
        }
        Expr::Index { object, index } => {
            collect_names_in_expr(&object.node, out);
            collect_names_in_expr(&index.node, out);
        }
        Expr::FieldAccess { object, .. } => {
            collect_names_in_expr(&object.node, out);
        }
        Expr::Match { subject, arms } => {
            collect_names_in_expr(&subject.node, out);
            for arm in arms {
                if let Some(g) = &arm.guard {
                    collect_names_in_expr(&g.node, out);
                }
                collect_names_in_expr(&arm.body.node, out);
            }
        }
        Expr::Interpolation(parts) => {
            for p in parts {
                if let InterpolPart::Expr(e) = p {
                    collect_names_in_expr(&e.node, out);
                }
            }
        }
        Expr::Closure {
            params,
            return_type,
            body,
        } => {
            for p in params {
                collect_names_in_type(&p.ty.node, out);
            }
            if let Some(rt) = return_type {
                collect_names_in_type(&rt.node, out);
            }
            collect_names_in_expr(&body.node, out);
        }
        Expr::OkExpr(e)
        | Expr::ErrExpr(e)
        | Expr::SomeExpr(e)
        | Expr::Await(e)
        | Expr::Spawn(e)
        | Expr::Try(e) => {
            collect_names_in_expr(&e.node, out);
        }
        Expr::Cast { expr, ty } => {
            collect_names_in_expr(&expr.node, out);
            collect_names_in_type(&ty.node, out);
        }
        Expr::NullCoalesce { value, default } => {
            collect_names_in_expr(&value.node, out);
            collect_names_in_expr(&default.node, out);
        }
        Expr::OptionalChain { object, .. } => {
            collect_names_in_expr(&object.node, out);
        }
        Expr::IfLet {
            value,
            then_branch,
            else_branch,
            ..
        } => {
            collect_names_in_expr(&value.node, out);
            collect_names_in_expr(&then_branch.node, out);
            if let Some(e) = else_branch {
                collect_names_in_expr(&e.node, out);
            }
        }
        Expr::MapLit(pairs) => {
            for (k, v) in pairs {
                collect_names_in_expr(&k.node, out);
                collect_names_in_expr(&v.node, out);
            }
        }
        // Leaves — no names to collect
        Expr::IntLit(_)
        | Expr::FloatLit(_)
        | Expr::StringLit(_)
        | Expr::BoolLit(_)
        | Expr::Unit
        | Expr::NoneExpr
        | Expr::Break
        | Expr::Continue => {}
    }
}

pub(crate) fn collect_names_in_stmt(s: &Stmt, out: &mut HashSet<String>) {
    match s {
        Stmt::Let { ty, value, .. } => {
            if let Some(t) = ty {
                collect_names_in_type(&t.node, out);
            }
            collect_names_in_expr(&value.node, out);
        }
        Stmt::Expr(e) => collect_names_in_expr(&e.node, out),
        Stmt::Return(e) => {
            if let Some(e) = e {
                collect_names_in_expr(&e.node, out);
            }
        }
        Stmt::Defer(e) => collect_names_in_expr(&e.node, out),
        Stmt::LetDestructure { value, .. } => collect_names_in_expr(&value.node, out),
    }
}

pub(crate) fn collect_names_in_type(t: &TypeExpr, out: &mut HashSet<String>) {
    match t {
        TypeExpr::Named(name) => {
            out.insert(name.clone());
        }
        TypeExpr::Array(inner) => collect_names_in_type(&inner.node, out),
        TypeExpr::FnType { params, ret } => {
            for p in params {
                collect_names_in_type(&p.node, out);
            }
            collect_names_in_type(&ret.node, out);
        }
        TypeExpr::Result { ok_type, err_type } => {
            collect_names_in_type(&ok_type.node, out);
            collect_names_in_type(&err_type.node, out);
        }
        TypeExpr::Optional(inner) => collect_names_in_type(&inner.node, out),
        TypeExpr::Future(inner) => collect_names_in_type(&inner.node, out),
        TypeExpr::HashMap(k, v) => {
            collect_names_in_type(&k.node, out);
            collect_names_in_type(&v.node, out);
        }
        TypeExpr::Unit | TypeExpr::Inferred => {}
    }
}

/// Collect every name referenced in a top-level item's signature and body.
/// This lets `resolve_imports()` do a fixed-point expansion pulling in any
/// sibling items the requested items transitively depend on.
pub(crate) fn collect_names_in_item(item: &Item, out: &mut HashSet<String>) {
    match item {
        Item::Function(f) => {
            for p in &f.params {
                collect_names_in_type(&p.ty.node, out);
            }
            if let Some(rt) = &f.return_type {
                collect_names_in_type(&rt.node, out);
            }
            collect_names_in_expr(&f.body.node, out);
        }
        Item::Struct(s) => {
            for field in &s.fields {
                collect_names_in_type(&field.ty.node, out);
            }
        }
        Item::Enum(e) => {
            for variant in &e.variants {
                for f in &variant.fields {
                    collect_names_in_type(&f.node, out);
                }
            }
        }
        Item::Impl(imp) => {
            out.insert(imp.type_name.clone());
            for m in &imp.methods {
                for p in &m.node.params {
                    collect_names_in_type(&p.ty.node, out);
                }
                if let Some(rt) = &m.node.return_type {
                    collect_names_in_type(&rt.node, out);
                }
                collect_names_in_expr(&m.node.body.node, out);
            }
        }
        Item::Const(c) => {
            if let Some(t) = &c.ty {
                collect_names_in_type(&t.node, out);
            }
            collect_names_in_expr(&c.value.node, out);
        }
        Item::Trait(_) | Item::Import { .. } | Item::Extern(_) => {}
    }
}

/// Return the defining name of a top-level item, if it has one.
/// Used by `resolve_imports()` to match referenced names against items
/// available in an imported module.
pub(crate) fn item_def_name(item: &Item) -> Option<&str> {
    match item {
        Item::Function(f) => Some(&f.name),
        Item::Struct(s) => Some(&s.name),
        Item::Enum(e) => Some(&e.name),
        Item::Impl(imp) => Some(&imp.type_name),
        Item::Const(c) => Some(&c.name),
        Item::Trait(t) => Some(&t.name),
        Item::Import { .. } | Item::Extern(_) => None,
    }
}

/// An imported file after parse + recursive-resolve, held in memory
/// while the cross-module walker runs. The walker needs every imported
/// module simultaneously so that a reference in file A to something
/// defined in file B can be traced across the boundary.
pub(crate) struct ImportedFile {
    resolved_path: PathBuf,
    module: Module,
    explicit_names: Vec<String>,
}

/// Resolve all `import` items in the module by reading, lexing, and parsing
/// the imported files and inlining the requested items.
/// `loading` tracks files currently being loaded (for circular import detection).
///
/// This runs in three phases:
///
/// 1. **Gather** — parse and recursively resolve every import, but defer
///    item extraction.
/// 2. **Global fixed-point** — seed per-file `wanted` sets from the
///    explicit import clauses, then iteratively expand across all
///    imported modules at once. When a wanted item in file A references
///    a name defined in file B, that name is added to file B's wanted
///    set and the loop runs again. This lets a caller name only its
///    entry point and have every transitively-referenced helper pulled
///    in automatically, *even across files*.
/// 3. **Extract + dedupe + validate** — walk each file's final wanted
///    set, pull the matching items out, dedupe across chains, and check
///    that every explicit clause name was satisfied.
pub(crate) fn resolve_imports(
    module: &mut Module,
    base_dir: &Path,
    loading: &mut HashSet<PathBuf>,
) -> Result<(), String> {
    // ==================== Phase A: Gather ====================
    let mut imported_files: Vec<ImportedFile> = Vec::new();

    for item in &module.items {
        if let Item::Import { names, path } = &item.node {
            // Virtual stdlib import -- validate module and names, then skip.
            // The builtins are always available globally; this import is
            // purely for validation and self-documentation.
            if turbo_ast::stdlib_modules::is_stdlib_path(path) {
                match turbo_ast::stdlib_modules::find_stdlib_module(path) {
                    Some(stdlib_mod) => {
                        for name in names {
                            if !stdlib_mod.functions.contains(&name.as_str()) {
                                return Err(format!(
                                    "`{}` is not exported by module `{}`. Available: {}",
                                    name,
                                    path,
                                    stdlib_mod.functions.join(", ")
                                ));
                            }
                        }
                    }
                    None => {
                        return Err(format!(
                            "unknown standard library module `{}`. Available modules: {}",
                            path,
                            turbo_ast::stdlib_modules::STDLIB_MODULES
                                .iter()
                                .map(|m| m.path)
                                .collect::<Vec<_>>()
                                .join(", ")
                        ));
                    }
                }
                // Don't try to load a file -- just skip this import.
                continue;
            }

            let resolved_path = resolve_import_path(base_dir, path);
            // Drop the raw `(os error N)` that `io::Error`'s Display leaks —
            // the E0610 envelope and `Help:` line carry the actionable detail.
            let canonical = resolved_path.canonicalize().map_err(|_| {
                format!(
                    "could not resolve import `{}` (looked for `{}`)",
                    path,
                    resolved_path.display()
                )
            })?;

            // Circular import detection
            if loading.contains(&canonical) {
                return Err(format!(
                    "circular import detected: `{}`",
                    resolved_path.display()
                ));
            }

            loading.insert(canonical.clone());

            let source = std::fs::read_to_string(&resolved_path).map_err(|e| {
                format!(
                    "could not read imported file `{}`: {}",
                    resolved_path.display(),
                    e.kind()
                )
            })?;

            let (tokens, lex_errors) = turbo_lexer::tokenize(&source);
            if !lex_errors.is_empty() {
                return Err(format!(
                    "lex errors in imported file `{}`",
                    resolved_path.display()
                ));
            }

            let (mut imported_module, parse_errors) = turbo_parser::parse(tokens);
            if !parse_errors.is_empty() {
                return Err(format!(
                    "parse errors in imported file `{}`: {}",
                    resolved_path.display(),
                    parse_errors[0].message
                ));
            }

            // Recursively resolve imports in the imported file
            let imported_dir = resolved_path.parent().unwrap_or(base_dir);
            resolve_imports(&mut imported_module, imported_dir, loading)?;

            loading.remove(&canonical);

            imported_files.push(ImportedFile {
                resolved_path,
                module: imported_module,
                explicit_names: names.clone(),
            });
        }
    }

    // ==================== Phase B: Global fixed-point ====================
    // Seed per-file wanted sets from explicit clauses, then loop across
    // every imported module until no new names are added. Cross-module
    // discovery: if file A's wanted body references `helper` and `helper`
    // is defined in file B, it gets added to B's wanted set.
    let mut wanted: Vec<HashSet<String>> = imported_files
        .iter()
        .map(|f| f.explicit_names.iter().cloned().collect())
        .collect();

    loop {
        // Collect every name referenced by items currently wanted in any
        // file. Attribution to a specific origin file doesn't matter —
        // name lookup in the next step is global.
        let mut discovered: HashSet<String> = HashSet::new();
        for (fi, file) in imported_files.iter().enumerate() {
            for imported_item in &file.module.items {
                let included = match &imported_item.node {
                    Item::Impl(imp) => wanted[fi].contains(&imp.type_name),
                    other => item_def_name(other)
                        .map(|n| wanted[fi].contains(n))
                        .unwrap_or(false),
                };
                if included {
                    collect_names_in_item(&imported_item.node, &mut discovered);
                }
            }
        }

        // Route each discovered name to whichever imported file actually
        // defines it. Unknown names (builtins, host-module refs) are
        // silently dropped — sema will resolve or reject them later.
        let mut changed = false;
        for name in discovered {
            for (fi, file) in imported_files.iter().enumerate() {
                if wanted[fi].contains(&name) {
                    continue;
                }
                let defined_here = file
                    .module
                    .items
                    .iter()
                    .any(|it| item_def_name(&it.node).map(|n| n == name).unwrap_or(false));
                if defined_here {
                    wanted[fi].insert(name.clone());
                    changed = true;
                }
            }
        }

        if !changed {
            break;
        }
    }

    // ==================== Phase C: Extract ====================
    let mut import_items: Vec<turbo_ast::Spanned<Item>> = Vec::new();

    for (fi, file) in imported_files.into_iter().enumerate() {
        let ImportedFile {
            resolved_path,
            module: imported_module,
            explicit_names,
        } = file;
        let file_wanted = &wanted[fi];

        let mut found_for_file: Vec<turbo_ast::Spanned<Item>> = Vec::new();
        for imported_item in imported_module.items {
            let included = match &imported_item.node {
                Item::Impl(imp) => file_wanted.contains(&imp.type_name),
                other => item_def_name(other)
                    .map(|n| file_wanted.contains(n))
                    .unwrap_or(false),
            };
            if included {
                found_for_file.push(imported_item);
            }
        }

        // Validate explicit clause names. Transitively-pulled names are
        // best-effort (and may legitimately not exist if the user
        // over-imported), but explicit names must resolve here.
        for name in &explicit_names {
            let found = found_for_file.iter().any(|item| {
                item_def_name(&item.node)
                    .map(|n| n == name)
                    .unwrap_or(false)
            });
            if !found {
                return Err(format!(
                    "name `{name}` not found in `{}`",
                    resolved_path.display()
                ));
            }
        }

        import_items.extend(found_for_file);
    }

    // Deduplicate import_items by defining name. Without this, transitive
    // resolution creates duplicates when the same helper is pulled in
    // through multiple import chains (e.g. main.tb imports from both
    // `./roster` and `./squad`, both of which transitively import
    // `color_cyan` from `./display/output`). Impls and extern blocks have
    // no unique def name and are always kept as-is — two impls for the
    // same struct are legitimate, and sema will catch any real conflicts.
    let mut seen_names: HashSet<String> = HashSet::new();
    let mut deduped: Vec<turbo_ast::Spanned<Item>> = Vec::with_capacity(import_items.len());
    for item in import_items {
        match &item.node {
            Item::Impl(_) | Item::Extern(_) => {
                deduped.push(item);
            }
            _ => match item_def_name(&item.node) {
                Some(name) => {
                    if seen_names.insert(name.to_string()) {
                        deduped.push(item);
                    }
                }
                None => {
                    deduped.push(item);
                }
            },
        }
    }

    // Remove import items and prepend imported items
    module
        .items
        .retain(|item| !matches!(&item.node, Item::Import { .. }));
    let mut new_items = deduped;
    new_items.append(&mut module.items);
    module.items = new_items;

    Ok(())
}
