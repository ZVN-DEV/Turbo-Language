//! Closure and spawn-site extraction.
//!
//! Before codegen starts compiling function bodies, it needs to know about
//! every closure and every `spawn` expression in the module. This module
//! walks the AST to collect that information:
//!
//! * **Capture analysis** — `collect_free_vars` / `find_captures` determine
//!   which variables a closure references from its enclosing scope.
//! * **Closure extraction** — `extract_all_closures` walks the entire module
//!   and returns a flat list of `ExtractedClosure` descriptors.
//! * **Spawn-site extraction** — `extract_all_spawn_sites` does the same for
//!   `spawn fn_call(args…)` expressions, producing `SpawnSite` descriptors
//!   that the compiler uses to generate thunk functions.

use turbo_ast::*;

use crate::turbo_types::TurboTy;

// ── Closure capture analysis ────────────────────────────────────────

/// Collect all free variable references in an expression.
/// `bound` contains names defined locally (parameters, let bindings).
/// Any Ident not in `bound` is a free variable (capture candidate).
pub(crate) fn collect_free_vars(expr: &Expr, bound: &mut Vec<String>, free: &mut Vec<String>) {
    match expr {
        Expr::Ident(name) => {
            if !bound.contains(name) && !free.contains(name) {
                free.push(name.clone());
            }
        }
        Expr::Block { stmts, tail_expr } => {
            let orig_len = bound.len();
            for stmt in stmts {
                match &stmt.node {
                    Stmt::Let { name, value, .. } => {
                        collect_free_vars(&value.node, bound, free);
                        bound.push(name.clone());
                    }
                    Stmt::LetDestructure { fields, value, .. } => {
                        collect_free_vars(&value.node, bound, free);
                        for field_name in fields {
                            bound.push(field_name.clone());
                        }
                    }
                    Stmt::Expr(e) => collect_free_vars(&e.node, bound, free),
                    Stmt::Return(Some(e)) => collect_free_vars(&e.node, bound, free),
                    Stmt::Return(None) => {}
                    Stmt::Defer(e) => collect_free_vars(&e.node, bound, free),
                }
            }
            if let Some(tail) = tail_expr {
                collect_free_vars(&tail.node, bound, free);
            }
            bound.truncate(orig_len);
        }
        Expr::BinaryOp { left, right, .. } => {
            collect_free_vars(&left.node, bound, free);
            collect_free_vars(&right.node, bound, free);
        }
        Expr::UnaryOp { expr: e, .. } => {
            collect_free_vars(&e.node, bound, free);
        }
        Expr::Call { callee, args } => {
            collect_free_vars(&callee.node, bound, free);
            for arg in args {
                collect_free_vars(&arg.node, bound, free);
            }
        }
        Expr::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_free_vars(&condition.node, bound, free);
            collect_free_vars(&then_branch.node, bound, free);
            if let Some(e) = else_branch {
                collect_free_vars(&e.node, bound, free);
            }
        }
        Expr::IfLet {
            pattern,
            value,
            then_branch,
            else_branch,
        } => {
            collect_free_vars(&value.node, bound, free);
            let orig_len = bound.len();
            // Bind pattern variable so it's not seen as free
            match &pattern.node {
                Pattern::Some(binding) | Pattern::Ok(binding) | Pattern::Err(binding) => {
                    bound.push(binding.clone());
                }
                _ => {}
            }
            collect_free_vars(&then_branch.node, bound, free);
            bound.truncate(orig_len);
            if let Some(e) = else_branch {
                collect_free_vars(&e.node, bound, free);
            }
        }
        Expr::While { condition, body } => {
            collect_free_vars(&condition.node, bound, free);
            collect_free_vars(&body.node, bound, free);
        }
        Expr::ForIn {
            var_name,
            iterable,
            body,
        } => {
            collect_free_vars(&iterable.node, bound, free);
            let orig_len = bound.len();
            bound.push(var_name.clone());
            collect_free_vars(&body.node, bound, free);
            bound.truncate(orig_len);
        }
        Expr::Assign { target, value } => {
            if !bound.contains(target) && !free.contains(target) {
                free.push(target.clone());
            }
            collect_free_vars(&value.node, bound, free);
        }
        Expr::CompoundAssign { target, value, .. } => {
            if !bound.contains(target) && !free.contains(target) {
                free.push(target.clone());
            }
            collect_free_vars(&value.node, bound, free);
        }
        Expr::FieldAssign { object, value, .. } => {
            collect_free_vars(&object.node, bound, free);
            collect_free_vars(&value.node, bound, free);
        }
        Expr::IndexAssign {
            object,
            index,
            value,
        } => {
            collect_free_vars(&object.node, bound, free);
            collect_free_vars(&index.node, bound, free);
            collect_free_vars(&value.node, bound, free);
        }
        Expr::ArrayLit(elems) => {
            for e in elems {
                collect_free_vars(&e.node, bound, free);
            }
        }
        Expr::Index { object, index } => {
            collect_free_vars(&object.node, bound, free);
            collect_free_vars(&index.node, bound, free);
        }
        Expr::StructLit { fields, .. } => {
            for (_, v) in fields {
                collect_free_vars(&v.node, bound, free);
            }
        }
        Expr::FieldAccess { object, .. } | Expr::OptionalChain { object, .. } => {
            collect_free_vars(&object.node, bound, free);
        }
        Expr::Match { subject, arms } => {
            collect_free_vars(&subject.node, bound, free);
            for arm in arms {
                let orig_len = bound.len();
                match &arm.pattern.node {
                    Pattern::Ok(name) | Pattern::Err(name) | Pattern::Some(name) => {
                        bound.push(name.clone());
                    }
                    Pattern::Ident(name) if name != "_" => {
                        bound.push(name.clone());
                    }
                    _ => {}
                }
                if let Some(ref guard) = arm.guard {
                    collect_free_vars(&guard.node, bound, free);
                }
                collect_free_vars(&arm.body.node, bound, free);
                bound.truncate(orig_len);
            }
        }
        Expr::Interpolation(parts) => {
            for part in parts {
                if let InterpolPart::Expr(e) = part {
                    collect_free_vars(&e.node, bound, free);
                }
            }
        }
        Expr::Closure { params, body, .. } => {
            let orig_len = bound.len();
            for p in params {
                bound.push(p.name.clone());
            }
            collect_free_vars(&body.node, bound, free);
            bound.truncate(orig_len);
        }
        Expr::Range { start, end } => {
            collect_free_vars(&start.node, bound, free);
            collect_free_vars(&end.node, bound, free);
        }
        Expr::OkExpr(e) | Expr::ErrExpr(e) | Expr::SomeExpr(e) => {
            collect_free_vars(&e.node, bound, free);
        }
        Expr::NullCoalesce { value, default } => {
            collect_free_vars(&value.node, bound, free);
            collect_free_vars(&default.node, bound, free);
        }
        Expr::Await(inner) | Expr::Spawn(inner) | Expr::Try(inner) => {
            collect_free_vars(&inner.node, bound, free);
        }
        Expr::MapLit(entries) => {
            for (k, v) in entries {
                collect_free_vars(&k.node, bound, free);
                collect_free_vars(&v.node, bound, free);
            }
        }
        Expr::EnumVariant { .. }
        | Expr::IntLit(_)
        | Expr::FloatLit(_)
        | Expr::StringLit(_)
        | Expr::BoolLit(_)
        | Expr::Unit
        | Expr::NoneExpr
        | Expr::Break
        | Expr::Continue => {}
    }
}

/// Determine which variables a closure captures from its enclosing scope.
pub(crate) fn find_captures(
    closure_params: &[Param],
    body: &Expr,
    outer_vars: &[String],
) -> Vec<String> {
    let mut bound: Vec<String> = closure_params.iter().map(|p| p.name.clone()).collect();
    let mut free = Vec::new();
    collect_free_vars(body, &mut bound, &mut free);
    free.retain(|name| outer_vars.contains(name));
    free
}

/// Info about a closure's captures, determined at creation site during compilation
#[derive(Debug, Clone)]
pub(crate) struct CaptureInfo {
    pub(crate) captures: Vec<(String, TurboTy)>,
}

// ── Closure extraction ──────────────────────────────────────────────

/// A pre-extracted closure with its metadata
pub(crate) struct ExtractedClosure<'a> {
    /// Byte offset of the `|` token in source -- used as a unique key
    pub(crate) span_start: usize,
    /// Synthetic function name (e.g. `__closure_0`)
    pub(crate) name: String,
    /// Closure parameters
    pub(crate) params: &'a [Param],
    /// Declared return type (if any)
    pub(crate) return_type: &'a Option<Spanned<TypeExpr>>,
    /// Closure body
    pub(crate) body: &'a Spanned<Expr>,
    /// Free variable names referenced in the body (potential captures)
    pub(crate) free_vars: Vec<String>,
}

/// Walk an expression tree and collect all closure nodes.
fn extract_closures_from_expr<'a>(
    expr: &'a Spanned<Expr>,
    out: &mut Vec<ExtractedClosure<'a>>,
    counter: &mut usize,
) {
    match &expr.node {
        Expr::Closure {
            params,
            return_type,
            body,
        } => {
            let name = format!("__closure_{}", *counter);
            *counter += 1;
            let mut bound: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
            let mut free_vars = Vec::new();
            collect_free_vars(&body.node, &mut bound, &mut free_vars);
            out.push(ExtractedClosure {
                span_start: expr.span.start,
                name,
                params,
                return_type,
                body,
                free_vars,
            });
            // Also scan the closure body for nested closures
            extract_closures_from_expr(body, out, counter);
        }
        Expr::Block { stmts, tail_expr } => {
            for stmt in stmts {
                match &stmt.node {
                    Stmt::Let { value, .. } | Stmt::LetDestructure { value, .. } => {
                        extract_closures_from_expr(value, out, counter)
                    }
                    Stmt::Expr(e) => extract_closures_from_expr(e, out, counter),
                    Stmt::Return(Some(e)) => extract_closures_from_expr(e, out, counter),
                    Stmt::Return(None) => {}
                    Stmt::Defer(e) => extract_closures_from_expr(e, out, counter),
                }
            }
            if let Some(tail) = tail_expr {
                extract_closures_from_expr(tail, out, counter);
            }
        }
        Expr::If {
            condition,
            then_branch,
            else_branch,
        } => {
            extract_closures_from_expr(condition, out, counter);
            extract_closures_from_expr(then_branch, out, counter);
            if let Some(e) = else_branch {
                extract_closures_from_expr(e, out, counter);
            }
        }
        Expr::IfLet {
            value,
            then_branch,
            else_branch,
            ..
        } => {
            extract_closures_from_expr(value, out, counter);
            extract_closures_from_expr(then_branch, out, counter);
            if let Some(e) = else_branch {
                extract_closures_from_expr(e, out, counter);
            }
        }
        Expr::While { condition, body } => {
            extract_closures_from_expr(condition, out, counter);
            extract_closures_from_expr(body, out, counter);
        }
        Expr::ForIn { iterable, body, .. } => {
            extract_closures_from_expr(iterable, out, counter);
            extract_closures_from_expr(body, out, counter);
        }
        Expr::BinaryOp { left, right, .. } => {
            extract_closures_from_expr(left, out, counter);
            extract_closures_from_expr(right, out, counter);
        }
        Expr::UnaryOp { expr, .. } => {
            extract_closures_from_expr(expr, out, counter);
        }
        Expr::Call { callee, args } => {
            extract_closures_from_expr(callee, out, counter);
            for arg in args {
                extract_closures_from_expr(arg, out, counter);
            }
        }
        Expr::Assign { value, .. } | Expr::CompoundAssign { value, .. } => {
            extract_closures_from_expr(value, out, counter);
        }
        Expr::FieldAssign { object, value, .. } => {
            extract_closures_from_expr(object, out, counter);
            extract_closures_from_expr(value, out, counter);
        }
        Expr::IndexAssign {
            object,
            index,
            value,
        } => {
            extract_closures_from_expr(object, out, counter);
            extract_closures_from_expr(index, out, counter);
            extract_closures_from_expr(value, out, counter);
        }
        Expr::OkExpr(value) | Expr::ErrExpr(value) | Expr::SomeExpr(value) => {
            extract_closures_from_expr(value, out, counter);
        }
        Expr::NullCoalesce { value, default } => {
            extract_closures_from_expr(value, out, counter);
            extract_closures_from_expr(default, out, counter);
        }
        Expr::Await(inner) | Expr::Spawn(inner) | Expr::Try(inner) => {
            extract_closures_from_expr(inner, out, counter);
        }
        Expr::OptionalChain { object, .. } => {
            extract_closures_from_expr(object, out, counter);
        }
        Expr::MapLit(entries) => {
            for (k, v) in entries {
                extract_closures_from_expr(k, out, counter);
                extract_closures_from_expr(v, out, counter);
            }
        }
        _ => {} // Literals, Ident, Unit, NoneExpr, etc. -- no sub-expressions with closures
    }
}

/// Extract all closures from the entire module
pub(crate) fn extract_all_closures(ast_module: &turbo_ast::Module) -> Vec<ExtractedClosure<'_>> {
    let mut closures = Vec::new();
    let mut counter = 0;
    for item in &ast_module.items {
        match &item.node {
            Item::Function(f) => {
                extract_closures_from_expr(&f.body, &mut closures, &mut counter);
            }
            Item::Impl(imp) => {
                for method in &imp.methods {
                    extract_closures_from_expr(&method.node.body, &mut closures, &mut counter);
                }
            }
            _ => {}
        }
    }
    closures
}

// ── Spawn site extraction ───────────────────────────────────────────

/// A pre-extracted spawn site: `spawn fn_call(args...)`
pub(crate) struct SpawnSite {
    pub(crate) span_start: usize,
    pub(crate) thunk_name: String,
    pub(crate) callee_name: String,
    pub(crate) num_args: usize,
}

fn extract_spawn_sites_from_expr(
    expr: &Spanned<Expr>,
    out: &mut Vec<SpawnSite>,
    counter: &mut usize,
) {
    match &expr.node {
        Expr::Spawn(inner) => {
            if let Expr::Call { callee, args } = &inner.node {
                if let Expr::Ident(name) = &callee.node {
                    out.push(SpawnSite {
                        span_start: expr.span.start,
                        thunk_name: format!("__spawn_thunk_{}", *counter),
                        callee_name: name.clone(),
                        num_args: args.len(),
                    });
                    *counter += 1;
                    for arg in args {
                        extract_spawn_sites_from_expr(arg, out, counter);
                    }
                    return;
                }
            }
            extract_spawn_sites_from_expr(inner, out, counter);
        }
        Expr::Block { stmts, tail_expr } => {
            for stmt in stmts {
                match &stmt.node {
                    Stmt::Let { value, .. } | Stmt::LetDestructure { value, .. } => {
                        extract_spawn_sites_from_expr(value, out, counter)
                    }
                    Stmt::Expr(e) => extract_spawn_sites_from_expr(e, out, counter),
                    Stmt::Return(Some(e)) => extract_spawn_sites_from_expr(e, out, counter),
                    Stmt::Return(None) => {}
                    Stmt::Defer(e) => extract_spawn_sites_from_expr(e, out, counter),
                }
            }
            if let Some(tail) = tail_expr {
                extract_spawn_sites_from_expr(tail, out, counter);
            }
        }
        Expr::If {
            condition,
            then_branch,
            else_branch,
        } => {
            extract_spawn_sites_from_expr(condition, out, counter);
            extract_spawn_sites_from_expr(then_branch, out, counter);
            if let Some(e) = else_branch {
                extract_spawn_sites_from_expr(e, out, counter);
            }
        }
        Expr::IfLet {
            value,
            then_branch,
            else_branch,
            ..
        } => {
            extract_spawn_sites_from_expr(value, out, counter);
            extract_spawn_sites_from_expr(then_branch, out, counter);
            if let Some(e) = else_branch {
                extract_spawn_sites_from_expr(e, out, counter);
            }
        }
        Expr::While { condition, body } => {
            extract_spawn_sites_from_expr(condition, out, counter);
            extract_spawn_sites_from_expr(body, out, counter);
        }
        Expr::ForIn { iterable, body, .. } => {
            extract_spawn_sites_from_expr(iterable, out, counter);
            extract_spawn_sites_from_expr(body, out, counter);
        }
        Expr::BinaryOp { left, right, .. } => {
            extract_spawn_sites_from_expr(left, out, counter);
            extract_spawn_sites_from_expr(right, out, counter);
        }
        Expr::UnaryOp { expr, .. } => {
            extract_spawn_sites_from_expr(expr, out, counter);
        }
        Expr::Call { callee, args } => {
            extract_spawn_sites_from_expr(callee, out, counter);
            for arg in args {
                extract_spawn_sites_from_expr(arg, out, counter);
            }
        }
        Expr::Assign { value, .. } | Expr::CompoundAssign { value, .. } => {
            extract_spawn_sites_from_expr(value, out, counter);
        }
        Expr::FieldAssign { object, value, .. } => {
            extract_spawn_sites_from_expr(object, out, counter);
            extract_spawn_sites_from_expr(value, out, counter);
        }
        Expr::IndexAssign {
            object,
            index,
            value,
        } => {
            extract_spawn_sites_from_expr(object, out, counter);
            extract_spawn_sites_from_expr(index, out, counter);
            extract_spawn_sites_from_expr(value, out, counter);
        }
        Expr::Index { object, index } => {
            extract_spawn_sites_from_expr(object, out, counter);
            extract_spawn_sites_from_expr(index, out, counter);
        }
        Expr::Range { start, end } => {
            extract_spawn_sites_from_expr(start, out, counter);
            extract_spawn_sites_from_expr(end, out, counter);
        }
        Expr::FieldAccess { object, .. } | Expr::OptionalChain { object, .. } => {
            extract_spawn_sites_from_expr(object, out, counter);
        }
        Expr::ArrayLit(elems) => {
            for e in elems {
                extract_spawn_sites_from_expr(e, out, counter);
            }
        }
        Expr::StructLit { fields, .. } => {
            for (_, e) in fields {
                extract_spawn_sites_from_expr(e, out, counter);
            }
        }
        Expr::Match { subject, arms } => {
            extract_spawn_sites_from_expr(subject, out, counter);
            for arm in arms {
                if let Some(ref guard) = arm.guard {
                    extract_spawn_sites_from_expr(guard, out, counter);
                }
                extract_spawn_sites_from_expr(&arm.body, out, counter);
            }
        }
        Expr::Closure { body, .. } => {
            extract_spawn_sites_from_expr(body, out, counter);
        }
        Expr::OkExpr(e) | Expr::ErrExpr(e) | Expr::SomeExpr(e) | Expr::Await(e) | Expr::Try(e) => {
            extract_spawn_sites_from_expr(e, out, counter);
        }
        Expr::NullCoalesce { value, default } => {
            extract_spawn_sites_from_expr(value, out, counter);
            extract_spawn_sites_from_expr(default, out, counter);
        }
        Expr::Interpolation(parts) => {
            for part in parts {
                if let InterpolPart::Expr(e) = part {
                    extract_spawn_sites_from_expr(e, out, counter);
                }
            }
        }
        Expr::MapLit(entries) => {
            for (k, v) in entries {
                extract_spawn_sites_from_expr(k, out, counter);
                extract_spawn_sites_from_expr(v, out, counter);
            }
        }
        _ => {}
    }
}

pub(crate) fn extract_all_spawn_sites(ast_module: &turbo_ast::Module) -> Vec<SpawnSite> {
    let mut sites = Vec::new();
    let mut counter = 0;
    for item in &ast_module.items {
        match &item.node {
            Item::Function(f) => extract_spawn_sites_from_expr(&f.body, &mut sites, &mut counter),
            Item::Impl(imp) => {
                for method in &imp.methods {
                    extract_spawn_sites_from_expr(&method.node.body, &mut sites, &mut counter);
                }
            }
            _ => {}
        }
    }
    sites
}

// ── Inlining helpers ────────────────────────────────────────────────

/// Returns true if an expression subtree contains any return statement.
/// Functions with returns can't be safely inlined (would need merge blocks).
pub(crate) fn has_return(expr: &Expr) -> bool {
    match expr {
        Expr::Block { stmts, tail_expr } => {
            for stmt in stmts {
                match &stmt.node {
                    Stmt::Return(_) => return true,
                    Stmt::Let { value, .. } | Stmt::LetDestructure { value, .. } => {
                        if has_return(&value.node) {
                            return true;
                        }
                    }
                    Stmt::Expr(e) => {
                        if has_return(&e.node) {
                            return true;
                        }
                    }
                    Stmt::Defer(e) => {
                        if has_return(&e.node) {
                            return true;
                        }
                    }
                }
            }
            tail_expr.as_ref().is_some_and(|t| has_return(&t.node))
        }
        Expr::If {
            condition,
            then_branch,
            else_branch,
        } => {
            has_return(&condition.node)
                || has_return(&then_branch.node)
                || else_branch.as_ref().is_some_and(|e| has_return(&e.node))
        }
        Expr::IfLet {
            value,
            then_branch,
            else_branch,
            ..
        } => {
            has_return(&value.node)
                || has_return(&then_branch.node)
                || else_branch.as_ref().is_some_and(|e| has_return(&e.node))
        }
        Expr::While { condition, body } => has_return(&condition.node) || has_return(&body.node),
        Expr::ForIn { iterable, body, .. } => has_return(&iterable.node) || has_return(&body.node),
        Expr::BinaryOp { left, right, .. } => has_return(&left.node) || has_return(&right.node),
        Expr::UnaryOp { expr, .. } => has_return(&expr.node),
        Expr::Call { callee, args } => {
            has_return(&callee.node) || args.iter().any(|a| has_return(&a.node))
        }
        Expr::Assign { value, .. } | Expr::CompoundAssign { value, .. } => has_return(&value.node),
        Expr::Await(inner) | Expr::Spawn(inner) | Expr::Try(inner) => has_return(&inner.node),
        Expr::FieldAssign { object, value, .. } => {
            has_return(&object.node) || has_return(&value.node)
        }
        Expr::IndexAssign {
            object,
            index,
            value,
        } => has_return(&object.node) || has_return(&index.node) || has_return(&value.node),
        Expr::Index { object, index } => has_return(&object.node) || has_return(&index.node),
        Expr::Closure { body, .. } => has_return(&body.node),
        Expr::Match { subject, arms } => {
            has_return(&subject.node) || arms.iter().any(|a| has_return(&a.body.node))
        }
        Expr::OkExpr(e) | Expr::ErrExpr(e) | Expr::SomeExpr(e) => has_return(&e.node),
        Expr::NoneExpr => false,
        Expr::OptionalChain { object, .. } => has_return(&object.node),
        Expr::NullCoalesce { value, default } => {
            has_return(&value.node) || has_return(&default.node)
        }
        Expr::Interpolation(parts) => parts.iter().any(|p| {
            if let InterpolPart::Expr(e) = p {
                has_return(&e.node)
            } else {
                false
            }
        }),
        Expr::MapLit(entries) => entries
            .iter()
            .any(|(k, v)| has_return(&k.node) || has_return(&v.node)),
        _ => false,
    }
}
