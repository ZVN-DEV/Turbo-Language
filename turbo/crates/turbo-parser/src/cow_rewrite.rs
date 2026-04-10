//! Post-parse AST pass that rewrites COW (copy-on-write) builtin calls in
//! statement position into self-assignments.
//!
//! # Background
//!
//! Turbo has a set of "COW builtins" — `push`, `replace`, `upper`, `lower`,
//! `trim`, `repeat`, `split`, `map`, `filter` — that return a NEW value
//! rather than mutating their first argument in place. Idiomatic user code
//! expects statements like `arr.push(4)` or `s.replace("a", "b")` to mutate
//! `arr` / `s`, so the compiler rewrites those calls in statement position
//! into `arr = push(arr, 4)` / `s = replace(s, "a", "b")`.
//!
//! Deciding whether a given call is in "statement position" (value
//! discarded) or "expression position" (value consumed) requires full
//! top-down context — the parser can produce the raw AST correctly, but
//! can't know for each nested block whether its tail value is used by the
//! enclosing construct. For example:
//!
//! ```turbo
//! fn f(s: str) -> str {
//!     if cond { replace(s, "a", "b") } else { ... }   // value position
//! }
//!
//! fn g() {
//!     if cond { a.push(4) }                           // statement position
//! }
//! ```
//!
//! In both cases the parser sees `{ <cow call> }` as an if-branch. Whether
//! to rewrite that call depends on whether the enclosing `if` itself is in
//! value or statement position — knowable only from the parent context.
//!
//! This pass walks the module after parsing with a `value_ctx` flag
//! threaded top-down from each function's body (whose context is set by
//! its declared return type). At each `Block`, it:
//!
//!   * Rewrites every `Stmt::Expr(cow_call)` in `stmts` to a self-assign
//!     (those are always statement-position).
//!   * If the block's value is discarded by its parent AND the tail
//!     expression is a COW call, moves the tail into `stmts` (rewriting it).
//!   * Leaves tail COW calls alone when the block's value is consumed.
//!
//! See `turbo/tests/phase1/cow_tail_expression.tb` and
//! `turbo/tests/phase1/cow_tail_rvalue_contexts.tb` for the end-to-end
//! regression tests this pass is designed to pass.

use turbo_ast::*;

/// COW builtins that return a new value instead of mutating in-place.
const COW_BUILTINS: &[&str] = &[
    "push", "map", "filter", "replace", "upper", "lower", "trim", "repeat", "split",
];

/// Apply COW rewrites across the entire module.
pub fn apply_cow_rewrites(module: &mut Module) {
    for item in &mut module.items {
        rewrite_item(&mut item.node);
    }
}

fn rewrite_item(item: &mut Item) {
    match item {
        Item::Function(f) => rewrite_fn(f),
        Item::Impl(block) => {
            for m in &mut block.methods {
                rewrite_fn(&mut m.node);
            }
        }
        Item::Trait(t) => {
            for m in &mut t.methods {
                if let Some(body) = &mut m.default_body {
                    let value_ctx = return_type_requires_value(&m.return_type);
                    rewrite_expr(&mut body.node, value_ctx, value_ctx);
                }
            }
        }
        Item::Const(c) => {
            // A constant initializer is always value position. There is no
            // enclosing function, so `return` is meaningless here; use
            // `true` as a neutral default for `fn_value_ctx`.
            rewrite_expr(&mut c.value.node, true, true);
        }
        Item::Struct(_)
        | Item::Enum(_)
        | Item::Import { .. }
        | Item::Extern(_) => {}
    }
}

fn rewrite_fn(f: &mut FnDef) {
    let value_ctx = return_type_requires_value(&f.return_type);
    // The enclosing function's return-type context is threaded untouched
    // down through the body so `Stmt::Return(Some(v))` can use it.
    rewrite_expr(&mut f.body.node, value_ctx, value_ctx);
}

/// Returns true if the given declared return type means the function body's
/// tail expression must produce a value (i.e. anything that is not `Unit`
/// or absent). This mirrors the rule the parser used in iter-1.
fn return_type_requires_value(return_type: &Option<Spanned<TypeExpr>>) -> bool {
    match return_type {
        None => false,
        Some(t) => !matches!(t.node, TypeExpr::Unit),
    }
}

/// Walk an expression, applying COW rewrites to any block it contains.
///
/// `value_ctx` is `true` if the VALUE of this expression is consumed by
/// the enclosing construct (let rhs, call arg, return, etc.) — in which
/// case a tail COW call inside a block at this position should be left
/// alone so its return value flows out. `false` means this expression's
/// value is discarded, so a tail COW call becomes a mutation statement.
///
/// `fn_value_ctx` is the enclosing function's value context (derived from
/// its declared return type). It is threaded untouched through every
/// recursive call so that `Stmt::Return(Some(v))` can pick the correct
/// `value_ctx` for the returned expression. This flag does NOT change
/// during traversal — it reflects the static property "does the enclosing
/// function's signature say it returns a value?".
fn rewrite_expr(expr: &mut Expr, value_ctx: bool, fn_value_ctx: bool) {
    match expr {
        Expr::Block { stmts, tail_expr } => {
            // Every ordinary statement is in discard position.
            for stmt in stmts.iter_mut() {
                rewrite_stmt(&mut stmt.node, fn_value_ctx);
            }

            // The tail expression inherits this block's context.
            if let Some(tail) = tail_expr.as_mut() {
                rewrite_expr(&mut tail.node, value_ctx, fn_value_ctx);
            }

            // If this block is in a statement context and the tail is a
            // COW call, move it out of the tail and rewrite it as a
            // mutation. This preserves the pre-existing behavior that
            // `if cond { a.push(4) }` mutates `a` instead of throwing the
            // new array away.
            if !value_ctx {
                let should_rewrite = matches!(
                    tail_expr.as_deref(),
                    Some(Spanned { node, .. }) if is_cow_builtin_call(node)
                );
                if should_rewrite {
                    if let Some(tail) = tail_expr.take() {
                        let span = tail.span.clone();
                        let stmt = rewrite_call_to_assign(*tail);
                        stmts.push(Spanned::new(stmt, span));
                    }
                }
            }
        }
        Expr::If {
            condition,
            then_branch,
            else_branch,
        } => {
            rewrite_expr(&mut condition.node, true, fn_value_ctx);
            rewrite_expr(&mut then_branch.node, value_ctx, fn_value_ctx);
            if let Some(else_b) = else_branch {
                rewrite_expr(&mut else_b.node, value_ctx, fn_value_ctx);
            }
        }
        Expr::IfLet {
            pattern: _,
            value,
            then_branch,
            else_branch,
        } => {
            rewrite_expr(&mut value.node, true, fn_value_ctx);
            rewrite_expr(&mut then_branch.node, value_ctx, fn_value_ctx);
            if let Some(else_b) = else_branch {
                rewrite_expr(&mut else_b.node, value_ctx, fn_value_ctx);
            }
        }
        Expr::Match { subject, arms } => {
            rewrite_expr(&mut subject.node, true, fn_value_ctx);
            for arm in arms.iter_mut() {
                if let Some(g) = &mut arm.guard {
                    rewrite_expr(&mut g.node, true, fn_value_ctx);
                }
                rewrite_expr(&mut arm.body.node, value_ctx, fn_value_ctx);
            }
        }
        Expr::While { condition, body } => {
            rewrite_expr(&mut condition.node, true, fn_value_ctx);
            // Loop bodies ALWAYS discard their value.
            rewrite_expr(&mut body.node, false, fn_value_ctx);
        }
        Expr::ForIn {
            var_name: _,
            iterable,
            body,
        } => {
            rewrite_expr(&mut iterable.node, true, fn_value_ctx);
            rewrite_expr(&mut body.node, false, fn_value_ctx);
        }
        Expr::Call { callee, args } => {
            rewrite_expr(&mut callee.node, true, fn_value_ctx);
            for a in args.iter_mut() {
                rewrite_expr(&mut a.node, true, fn_value_ctx);
            }
        }
        Expr::BinaryOp { left, right, .. } => {
            rewrite_expr(&mut left.node, true, fn_value_ctx);
            rewrite_expr(&mut right.node, true, fn_value_ctx);
        }
        Expr::UnaryOp { expr: inner, .. } => {
            rewrite_expr(&mut inner.node, true, fn_value_ctx);
        }
        Expr::Assign { value, .. } | Expr::CompoundAssign { value, .. } => {
            rewrite_expr(&mut value.node, true, fn_value_ctx);
        }
        Expr::FieldAssign { object, value, .. } => {
            rewrite_expr(&mut object.node, true, fn_value_ctx);
            rewrite_expr(&mut value.node, true, fn_value_ctx);
        }
        Expr::IndexAssign {
            object,
            index,
            value,
        } => {
            rewrite_expr(&mut object.node, true, fn_value_ctx);
            rewrite_expr(&mut index.node, true, fn_value_ctx);
            rewrite_expr(&mut value.node, true, fn_value_ctx);
        }
        Expr::Range { start, end } => {
            rewrite_expr(&mut start.node, true, fn_value_ctx);
            rewrite_expr(&mut end.node, true, fn_value_ctx);
        }
        Expr::ArrayLit(elems) => {
            for e in elems.iter_mut() {
                rewrite_expr(&mut e.node, true, fn_value_ctx);
            }
        }
        Expr::Index { object, index } => {
            rewrite_expr(&mut object.node, true, fn_value_ctx);
            rewrite_expr(&mut index.node, true, fn_value_ctx);
        }
        Expr::StructLit { fields, .. } => {
            for (_, v) in fields.iter_mut() {
                rewrite_expr(&mut v.node, true, fn_value_ctx);
            }
        }
        Expr::FieldAccess { object, .. } => {
            rewrite_expr(&mut object.node, true, fn_value_ctx);
        }
        Expr::Closure {
            return_type, body, ..
        } => {
            // A closure's body value is consumed by the caller of the
            // closure. If a return type is declared and it's Unit, the
            // body's tail is discarded; otherwise the body is in value
            // position. (Inferred return type is treated as value because
            // closures in practice are almost always used for their
            // return value — e.g. `xs.map(|w| replace(w, ...))` — and
            // `for_each` style callbacks aren't in the language yet.)
            //
            // The closure introduces a NEW enclosing function for the
            // purpose of `return` semantics, so `fn_value_ctx` is
            // recomputed from the closure's own return type rather than
            // inherited from the outer function.
            let body_ctx = match return_type {
                Some(t) => !matches!(t.node, TypeExpr::Unit),
                None => true,
            };
            rewrite_expr(&mut body.node, body_ctx, body_ctx);
        }
        Expr::OkExpr(inner) | Expr::ErrExpr(inner) | Expr::SomeExpr(inner) => {
            rewrite_expr(&mut inner.node, true, fn_value_ctx);
        }
        Expr::NullCoalesce { value, default } => {
            rewrite_expr(&mut value.node, true, fn_value_ctx);
            rewrite_expr(&mut default.node, true, fn_value_ctx);
        }
        Expr::OptionalChain { object, .. } => {
            rewrite_expr(&mut object.node, true, fn_value_ctx);
        }
        Expr::Await(inner) | Expr::Spawn(inner) | Expr::Try(inner) => {
            rewrite_expr(&mut inner.node, true, fn_value_ctx);
        }
        Expr::Interpolation(parts) => {
            for p in parts.iter_mut() {
                if let InterpolPart::Expr(e) = p {
                    rewrite_expr(&mut e.node, true, fn_value_ctx);
                }
            }
        }
        Expr::MapLit(pairs) => {
            for (k, v) in pairs.iter_mut() {
                rewrite_expr(&mut k.node, true, fn_value_ctx);
                rewrite_expr(&mut v.node, true, fn_value_ctx);
            }
        }
        // Leaf nodes — no sub-expressions to walk.
        Expr::IntLit(_)
        | Expr::FloatLit(_)
        | Expr::StringLit(_)
        | Expr::BoolLit(_)
        | Expr::Unit
        | Expr::Ident(_)
        | Expr::EnumVariant { .. }
        | Expr::NoneExpr
        | Expr::Break
        | Expr::Continue => {}
    }
}

fn rewrite_stmt(stmt: &mut Stmt, fn_value_ctx: bool) {
    match stmt {
        Stmt::Let { value, .. } | Stmt::LetDestructure { value, .. } => {
            rewrite_expr(&mut value.node, true, fn_value_ctx);
        }
        Stmt::Return(Some(v)) => {
            // The return expression's value context matches the enclosing
            // function's declared return type: a unit-return fn discards
            // the value (so a tail COW call should rewrite to a self-
            // assign), while a value-returning fn consumes it (so the
            // tail COW call must flow out).
            rewrite_expr(&mut v.node, fn_value_ctx, fn_value_ctx);

            // Special case: in a unit-return fn, `return push(arr, x)`
            // (where the returned expression is DIRECTLY a top-level COW
            // call) would otherwise silently discard the new array. We
            // rewrite the returned expression in-place into an assignment
            // `arr = push(arr, x)`, which evaluates to unit — the
            // mutation is preserved and the return still type-checks as
            // a unit-producing tail.
            if !fn_value_ctx && is_cow_builtin_call(&v.node) {
                let span = v.span.clone();
                let taken = std::mem::replace(v, Spanned::new(Expr::Unit, span.clone()));
                let as_stmt = rewrite_call_to_assign(taken);
                if let Stmt::Expr(rewritten) = as_stmt {
                    *v = rewritten;
                } else {
                    unreachable!("rewrite_call_to_assign returned non-Expr");
                }
            }
        }
        Stmt::Return(None) => {}
        Stmt::Defer(e) => {
            // `defer` runs its expression at block exit for side effects.
            // The value is discarded.
            rewrite_expr(&mut e.node, false, fn_value_ctx);
        }
        Stmt::Expr(e) => {
            // First descend into the expression with value_ctx=false so
            // any sub-blocks (if/match/for/while/bare blocks) correctly
            // see themselves as statement position.
            rewrite_expr(&mut e.node, false, fn_value_ctx);

            // If the statement expression is itself a top-level COW call
            // (e.g. `push(items, 4)` written as a free-standing statement
            // or via UFCS), rewrite it in place to a self-assign. This
            // handles the `arr.push(x)` case where the parser produced
            // a bare `Stmt::Expr(call)` node.
            if is_cow_builtin_call(&e.node) {
                let span = e.span.clone();
                let replaced = std::mem::replace(e, Spanned::new(Expr::Unit, span.clone()));
                let new_stmt_inner = rewrite_call_to_assign(replaced);
                // `rewrite_call_to_assign` always returns a `Stmt::Expr`
                // wrapping the rewritten call; pull it back out.
                if let Stmt::Expr(new_expr) = new_stmt_inner {
                    *e = new_expr;
                } else {
                    unreachable!("rewrite_call_to_assign returned non-Expr");
                }
            }
        }
    }
}

/// True if the expression is a call to one of the COW builtins.
pub(crate) fn is_cow_builtin_call(expr: &Expr) -> bool {
    if let Expr::Call { callee, args } = expr {
        if let Expr::Ident(fn_name) = &callee.node {
            return COW_BUILTINS.contains(&fn_name.as_str()) && !args.is_empty();
        }
    }
    false
}

/// Rewrite a COW call expression into an assignment statement targeting
/// the call's first argument.
///
/// * `push(items, 4)`       → `items = push(items, 4)`
/// * `push(b.items, 4)`     → `b.items = push(b.items, 4)`
/// * `push(arr[0], 4)`      → `arr[0] = push(arr[0], 4)`
///
/// If the first argument is none of the above (e.g. a deeper expression)
/// the call is left as a plain `Stmt::Expr`.
fn rewrite_call_to_assign(expr: Spanned<Expr>) -> Stmt {
    if let Expr::Call { ref args, .. } = expr.node {
        if !args.is_empty() {
            let target_arg = args[0].clone();
            let span = expr.span.clone();
            match target_arg.node {
                Expr::Ident(var_name) => {
                    return Stmt::Expr(Spanned::new(
                        Expr::Assign {
                            target: var_name,
                            value: Box::new(expr),
                        },
                        span,
                    ));
                }
                Expr::FieldAccess { object, field } => {
                    return Stmt::Expr(Spanned::new(
                        Expr::FieldAssign {
                            object,
                            field,
                            value: Box::new(expr),
                        },
                        span,
                    ));
                }
                Expr::Index { object, index } => {
                    return Stmt::Expr(Spanned::new(
                        Expr::IndexAssign {
                            object,
                            index,
                            value: Box::new(expr),
                        },
                        span,
                    ));
                }
                _ => {}
            }
        }
    }
    Stmt::Expr(expr)
}
