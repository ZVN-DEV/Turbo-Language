//! Free helper functions: closure extraction, spawn-site extraction,
//! variant tag lookup.

use std::collections::HashMap;
use turbo_ast::*;

use crate::types::TurboTy;

// ── Closure extraction ──────────────────────────────────────────────

pub(crate) struct ExtractedClosure<'a> {
    pub(crate) span_start: usize,
    pub(crate) name: String,
    pub(crate) params: &'a [Param],
    pub(crate) return_type: &'a Option<Spanned<TypeExpr>>,
    pub(crate) body: &'a Spanned<Expr>,
    pub(crate) free_vars: Vec<String>,
    /// Types of captured (free) variables, inferred from enclosing scope
    pub(crate) capture_types: Vec<TurboTy>,
}

/// Infer the type of a captured variable from how it's used in the closure body.
/// Checks if the variable appears in string interpolation (-> Str) or string concat (-> Str).
pub(crate) fn infer_capture_type_from_body(body: &Expr, var_name: &str) -> TurboTy {
    match body {
        Expr::Interpolation(parts) => {
            for part in parts {
                if let InterpolPart::Expr(e) = part {
                    if let Expr::Ident(name) = &e.node {
                        if name == var_name {
                            return TurboTy::Str;
                        }
                    }
                }
            }
            // Recurse into sub-expressions
            for part in parts {
                if let InterpolPart::Expr(e) = part {
                    let t = infer_capture_type_from_body(&e.node, var_name);
                    if t != TurboTy::Int {
                        return t;
                    }
                }
            }
            TurboTy::Int
        }
        Expr::Block { stmts, tail_expr } => {
            for stmt in stmts {
                match &stmt.node {
                    Stmt::Let { value, .. } => {
                        let t = infer_capture_type_from_body(&value.node, var_name);
                        if t != TurboTy::Int {
                            return t;
                        }
                    }
                    Stmt::Expr(e) => {
                        let t = infer_capture_type_from_body(&e.node, var_name);
                        if t != TurboTy::Int {
                            return t;
                        }
                    }
                    Stmt::Return(Some(e)) => {
                        let t = infer_capture_type_from_body(&e.node, var_name);
                        if t != TurboTy::Int {
                            return t;
                        }
                    }
                    _ => {}
                }
            }
            if let Some(tail) = tail_expr {
                return infer_capture_type_from_body(&tail.node, var_name);
            }
            TurboTy::Int
        }
        Expr::Call { callee, args } => {
            // If passed to rt_str_concat or similar, it's a string
            for arg in args {
                let t = infer_capture_type_from_body(&arg.node, var_name);
                if t != TurboTy::Int {
                    return t;
                }
            }
            infer_capture_type_from_body(&callee.node, var_name)
        }
        Expr::BinaryOp { left, right, .. } => {
            let t = infer_capture_type_from_body(&left.node, var_name);
            if t != TurboTy::Int {
                return t;
            }
            infer_capture_type_from_body(&right.node, var_name)
        }
        Expr::If {
            condition,
            then_branch,
            else_branch,
        } => {
            let t = infer_capture_type_from_body(&condition.node, var_name);
            if t != TurboTy::Int {
                return t;
            }
            let t = infer_capture_type_from_body(&then_branch.node, var_name);
            if t != TurboTy::Int {
                return t;
            }
            if let Some(e) = else_branch {
                return infer_capture_type_from_body(&e.node, var_name);
            }
            TurboTy::Int
        }
        _ => TurboTy::Int,
    }
}

pub(crate) fn collect_free_vars_llvm(expr: &Expr, bound: &mut Vec<String>, free: &mut Vec<String>) {
    match expr {
        Expr::Ident(name) => {
            if !bound.contains(name) && !free.contains(name) {
                free.push(name.clone());
            }
        }
        // Other nodes don't bind names; handled by sub-expression walk below
        _ => {}
    }
    // Walk sub-expressions
    match expr {
        Expr::BinaryOp { left, right, .. } => {
            collect_free_vars_llvm(&left.node, bound, free);
            collect_free_vars_llvm(&right.node, bound, free);
        }
        Expr::UnaryOp { expr: e, .. } => collect_free_vars_llvm(&e.node, bound, free),
        Expr::Call { callee, args } => {
            collect_free_vars_llvm(&callee.node, bound, free);
            for arg in args {
                collect_free_vars_llvm(&arg.node, bound, free);
            }
        }
        Expr::Block { stmts, tail_expr } => {
            let prev_len = bound.len();
            for stmt in stmts {
                match &stmt.node {
                    Stmt::Let { name, value, .. } => {
                        collect_free_vars_llvm(&value.node, bound, free);
                        bound.push(name.clone());
                    }
                    Stmt::LetDestructure { fields, value, .. } => {
                        collect_free_vars_llvm(&value.node, bound, free);
                        for field_name in fields {
                            bound.push(field_name.clone());
                        }
                    }
                    Stmt::Expr(e) | Stmt::Return(Some(e)) | Stmt::Defer(e) => {
                        collect_free_vars_llvm(&e.node, bound, free);
                    }
                    Stmt::Return(None) => {}
                }
            }
            if let Some(tail) = tail_expr {
                collect_free_vars_llvm(&tail.node, bound, free);
            }
            bound.truncate(prev_len);
        }
        Expr::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_free_vars_llvm(&condition.node, bound, free);
            collect_free_vars_llvm(&then_branch.node, bound, free);
            if let Some(e) = else_branch {
                collect_free_vars_llvm(&e.node, bound, free);
            }
        }
        Expr::While { condition, body } => {
            collect_free_vars_llvm(&condition.node, bound, free);
            collect_free_vars_llvm(&body.node, bound, free);
        }
        Expr::ForIn {
            var_name,
            iterable,
            body,
        } => {
            collect_free_vars_llvm(&iterable.node, bound, free);
            let prev = bound.len();
            bound.push(var_name.clone());
            collect_free_vars_llvm(&body.node, bound, free);
            bound.truncate(prev);
        }
        Expr::Assign { target, value } | Expr::CompoundAssign { target, value, .. } => {
            if !bound.contains(target) && !free.contains(target) {
                free.push(target.clone());
            }
            collect_free_vars_llvm(&value.node, bound, free);
        }
        Expr::FieldAssign { object, value, .. } => {
            collect_free_vars_llvm(&object.node, bound, free);
            collect_free_vars_llvm(&value.node, bound, free);
        }
        Expr::IndexAssign {
            object,
            index,
            value,
        } => {
            collect_free_vars_llvm(&object.node, bound, free);
            collect_free_vars_llvm(&index.node, bound, free);
            collect_free_vars_llvm(&value.node, bound, free);
        }
        Expr::FieldAccess { object, .. } | Expr::OptionalChain { object, .. } => {
            collect_free_vars_llvm(&object.node, bound, free)
        }
        Expr::Index { object, index } => {
            collect_free_vars_llvm(&object.node, bound, free);
            collect_free_vars_llvm(&index.node, bound, free);
        }
        Expr::ArrayLit(elems) => {
            for e in elems {
                collect_free_vars_llvm(&e.node, bound, free);
            }
        }
        Expr::StructLit { fields, .. } => {
            for (_, e) in fields {
                collect_free_vars_llvm(&e.node, bound, free);
            }
        }
        Expr::MapLit(entries) => {
            for (k, v) in entries {
                collect_free_vars_llvm(&k.node, bound, free);
                collect_free_vars_llvm(&v.node, bound, free);
            }
        }
        Expr::Match { subject, arms } => {
            collect_free_vars_llvm(&subject.node, bound, free);
            for arm in arms {
                if let Some(ref g) = arm.guard {
                    collect_free_vars_llvm(&g.node, bound, free);
                }
                collect_free_vars_llvm(&arm.body.node, bound, free);
            }
        }
        Expr::Closure { params, body, .. } => {
            let prev = bound.len();
            for p in params {
                bound.push(p.name.clone());
            }
            collect_free_vars_llvm(&body.node, bound, free);
            bound.truncate(prev);
        }
        Expr::OkExpr(v)
        | Expr::ErrExpr(v)
        | Expr::SomeExpr(v)
        | Expr::Await(v)
        | Expr::Spawn(v)
        | Expr::Try(v) => {
            collect_free_vars_llvm(&v.node, bound, free);
        }
        Expr::NullCoalesce { value, default } => {
            collect_free_vars_llvm(&value.node, bound, free);
            collect_free_vars_llvm(&default.node, bound, free);
        }
        Expr::Interpolation(parts) => {
            for part in parts {
                if let InterpolPart::Expr(e) = part {
                    collect_free_vars_llvm(&e.node, bound, free);
                }
            }
        }
        Expr::Range { start, end } => {
            collect_free_vars_llvm(&start.node, bound, free);
            collect_free_vars_llvm(&end.node, bound, free);
        }
        _ => {}
    }
}

pub(crate) fn extract_closures_from_expr_llvm<'a>(
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
            collect_free_vars_llvm(&body.node, &mut bound, &mut free_vars);
            // Infer capture types from body usage: scan for string interpolation/concat
            let capture_types: Vec<TurboTy> = free_vars
                .iter()
                .map(|var_name| infer_capture_type_from_body(&body.node, var_name))
                .collect();
            out.push(ExtractedClosure {
                span_start: expr.span.start,
                name,
                params,
                return_type,
                body,
                free_vars,
                capture_types,
            });
            extract_closures_from_expr_llvm(body, out, counter);
        }
        Expr::Block { stmts, tail_expr } => {
            for stmt in stmts {
                match &stmt.node {
                    Stmt::Let { value, .. }
                    | Stmt::LetDestructure { value, .. }
                    | Stmt::Expr(value) => {
                        extract_closures_from_expr_llvm(value, out, counter);
                    }
                    Stmt::Return(Some(e)) | Stmt::Defer(e) => {
                        extract_closures_from_expr_llvm(e, out, counter);
                    }
                    _ => {}
                }
            }
            if let Some(tail) = tail_expr {
                extract_closures_from_expr_llvm(tail, out, counter);
            }
        }
        Expr::If {
            condition,
            then_branch,
            else_branch,
        } => {
            extract_closures_from_expr_llvm(condition, out, counter);
            extract_closures_from_expr_llvm(then_branch, out, counter);
            if let Some(e) = else_branch {
                extract_closures_from_expr_llvm(e, out, counter);
            }
        }
        Expr::While { condition, body }
        | Expr::ForIn {
            iterable: condition,
            body,
            ..
        } => {
            extract_closures_from_expr_llvm(condition, out, counter);
            extract_closures_from_expr_llvm(body, out, counter);
        }
        Expr::BinaryOp { left, right, .. } => {
            extract_closures_from_expr_llvm(left, out, counter);
            extract_closures_from_expr_llvm(right, out, counter);
        }
        Expr::UnaryOp { expr: e, .. } => extract_closures_from_expr_llvm(e, out, counter),
        Expr::Call { callee, args } => {
            extract_closures_from_expr_llvm(callee, out, counter);
            for arg in args {
                extract_closures_from_expr_llvm(arg, out, counter);
            }
        }
        Expr::Assign { value, .. } | Expr::CompoundAssign { value, .. } => {
            extract_closures_from_expr_llvm(value, out, counter);
        }
        Expr::FieldAssign { object, value, .. } => {
            extract_closures_from_expr_llvm(object, out, counter);
            extract_closures_from_expr_llvm(value, out, counter);
        }
        Expr::IndexAssign {
            object,
            index,
            value,
        } => {
            extract_closures_from_expr_llvm(object, out, counter);
            extract_closures_from_expr_llvm(index, out, counter);
            extract_closures_from_expr_llvm(value, out, counter);
        }
        Expr::OkExpr(v)
        | Expr::ErrExpr(v)
        | Expr::SomeExpr(v)
        | Expr::Await(v)
        | Expr::Spawn(v)
        | Expr::Try(v) => {
            extract_closures_from_expr_llvm(v, out, counter);
        }
        Expr::NullCoalesce { value, default } => {
            extract_closures_from_expr_llvm(value, out, counter);
            extract_closures_from_expr_llvm(default, out, counter);
        }
        Expr::OptionalChain { object, .. } => {
            extract_closures_from_expr_llvm(object, out, counter);
        }
        Expr::Interpolation(parts) => {
            for part in parts {
                if let InterpolPart::Expr(e) = part {
                    extract_closures_from_expr_llvm(e, out, counter);
                }
            }
        }
        Expr::ArrayLit(elems) => {
            for e in elems {
                extract_closures_from_expr_llvm(e, out, counter);
            }
        }
        Expr::StructLit { fields, .. } => {
            for (_, e) in fields {
                extract_closures_from_expr_llvm(e, out, counter);
            }
        }
        Expr::MapLit(entries) => {
            for (k, v) in entries {
                extract_closures_from_expr_llvm(k, out, counter);
                extract_closures_from_expr_llvm(v, out, counter);
            }
        }
        Expr::Match { subject, arms } => {
            extract_closures_from_expr_llvm(subject, out, counter);
            for arm in arms {
                if let Some(ref g) = arm.guard {
                    extract_closures_from_expr_llvm(g, out, counter);
                }
                extract_closures_from_expr_llvm(&arm.body, out, counter);
            }
        }
        _ => {}
    }
}

pub(crate) fn extract_all_closures_llvm(
    ast_module: &turbo_ast::Module,
) -> Vec<ExtractedClosure<'_>> {
    let mut closures = Vec::new();
    let mut counter = 0;
    for item in &ast_module.items {
        match &item.node {
            Item::Function(f) => {
                extract_closures_from_expr_llvm(&f.body, &mut closures, &mut counter)
            }
            Item::Impl(imp) => {
                for method in &imp.methods {
                    extract_closures_from_expr_llvm(&method.node.body, &mut closures, &mut counter);
                }
            }
            _ => {}
        }
    }
    closures
}

// ── Spawn extraction ────────────────────────────────────────────────

pub(crate) struct SpawnSite {
    pub(crate) span_start: usize,
    pub(crate) thunk_name: String,
    pub(crate) callee_name: String,
    pub(crate) num_args: usize,
}

fn extract_spawn_sites_from_expr_llvm(
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
                        extract_spawn_sites_from_expr_llvm(arg, out, counter);
                    }
                    return;
                }
            }
            extract_spawn_sites_from_expr_llvm(inner, out, counter);
        }
        Expr::Block { stmts, tail_expr } => {
            for stmt in stmts {
                match &stmt.node {
                    Stmt::Let { value, .. }
                    | Stmt::LetDestructure { value, .. }
                    | Stmt::Expr(value) => {
                        extract_spawn_sites_from_expr_llvm(value, out, counter);
                    }
                    Stmt::Return(Some(e)) | Stmt::Defer(e) => {
                        extract_spawn_sites_from_expr_llvm(e, out, counter);
                    }
                    _ => {}
                }
            }
            if let Some(tail) = tail_expr {
                extract_spawn_sites_from_expr_llvm(tail, out, counter);
            }
        }
        Expr::If {
            condition,
            then_branch,
            else_branch,
        } => {
            extract_spawn_sites_from_expr_llvm(condition, out, counter);
            extract_spawn_sites_from_expr_llvm(then_branch, out, counter);
            if let Some(e) = else_branch {
                extract_spawn_sites_from_expr_llvm(e, out, counter);
            }
        }
        Expr::While { condition, body }
        | Expr::ForIn {
            iterable: condition,
            body,
            ..
        } => {
            extract_spawn_sites_from_expr_llvm(condition, out, counter);
            extract_spawn_sites_from_expr_llvm(body, out, counter);
        }
        Expr::BinaryOp { left, right, .. } => {
            extract_spawn_sites_from_expr_llvm(left, out, counter);
            extract_spawn_sites_from_expr_llvm(right, out, counter);
        }
        Expr::UnaryOp { expr: e, .. } => extract_spawn_sites_from_expr_llvm(e, out, counter),
        Expr::Call { callee, args } => {
            extract_spawn_sites_from_expr_llvm(callee, out, counter);
            for arg in args {
                extract_spawn_sites_from_expr_llvm(arg, out, counter);
            }
        }
        Expr::Assign { value, .. } | Expr::CompoundAssign { value, .. } => {
            extract_spawn_sites_from_expr_llvm(value, out, counter);
        }
        Expr::OkExpr(v) | Expr::ErrExpr(v) | Expr::SomeExpr(v) | Expr::Await(v) | Expr::Try(v) => {
            extract_spawn_sites_from_expr_llvm(v, out, counter);
        }
        Expr::NullCoalesce { value, default } => {
            extract_spawn_sites_from_expr_llvm(value, out, counter);
            extract_spawn_sites_from_expr_llvm(default, out, counter);
        }
        Expr::OptionalChain { object, .. } => {
            extract_spawn_sites_from_expr_llvm(object, out, counter);
        }
        Expr::ArrayLit(elems) => {
            for e in elems {
                extract_spawn_sites_from_expr_llvm(e, out, counter);
            }
        }
        Expr::Match { subject, arms } => {
            extract_spawn_sites_from_expr_llvm(subject, out, counter);
            for arm in arms {
                extract_spawn_sites_from_expr_llvm(&arm.body, out, counter);
            }
        }
        Expr::MapLit(entries) => {
            for (k, v) in entries {
                extract_spawn_sites_from_expr_llvm(k, out, counter);
                extract_spawn_sites_from_expr_llvm(v, out, counter);
            }
        }
        _ => {}
    }
}

pub(crate) fn extract_all_spawn_sites_llvm(ast_module: &turbo_ast::Module) -> Vec<SpawnSite> {
    let mut sites = Vec::new();
    let mut counter = 0;
    for item in &ast_module.items {
        match &item.node {
            Item::Function(f) => {
                extract_spawn_sites_from_expr_llvm(&f.body, &mut sites, &mut counter)
            }
            Item::Impl(imp) => {
                for method in &imp.methods {
                    extract_spawn_sites_from_expr_llvm(&method.node.body, &mut sites, &mut counter);
                }
            }
            _ => {}
        }
    }
    sites
}

// ── Variant tag lookup ──────────────────────────────────────────────

/// Look up a variant name across all enums and return its tag index.
pub(crate) fn lookup_variant_tag(
    enum_variants: &HashMap<String, Vec<String>>,
    name: &str,
) -> Option<usize> {
    for variants in enum_variants.values() {
        if let Some(idx) = variants.iter().position(|v| v == name) {
            return Some(idx);
        }
    }
    None
}
