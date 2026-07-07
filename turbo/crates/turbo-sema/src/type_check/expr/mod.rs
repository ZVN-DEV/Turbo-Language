//! Expression type checking for the Turbo semantic analyzer.
//!
//! This module contains `check_expr_inner`, the large match on every
//! [`turbo_ast::Expr`] variant. It is the workhorse of expression-level
//! type inference and validation.
//!
//! The implementation is split across several files, each contributing
//! `impl Checker` blocks:
//!
//! * **`mod.rs`** (this file) — the `check_expr_inner` dispatcher plus the
//!   core expression forms: identifiers, operators, casts, calls,
//!   conditionals, blocks, and assignments.
//! * **[`collections_control`]** — loops, ranges, collection/struct/field
//!   expressions, pattern matching, closures, and the small leaf forms.
//! * **[`builtins_core`]** — the builtin-call dispatcher plus core, string,
//!   IO/env, math, and conversion builtins.
//! * **[`builtins_data`]** — filesystem/path, array, time, and HTTP builtins.
//! * **[`builtins_net`]** — JSON, concurrency, hashmap, and reference builtins.

mod builtins_core;
mod builtins_data;
mod builtins_net;
mod builtins_sqlite;
mod collections_control;

use std::collections::HashMap;

use turbo_ast::*;

use crate::scope::VarInfo;
use crate::suggest;
use crate::{
    extract_int_literal, int_literal_fits_in_type, literal_coerces_to, resolve_type_expr,
    types_compatible, Checker, Ty,
};

impl Checker {
    pub(crate) fn check_expr_inner(&mut self, expr: &Spanned<Expr>) -> Ty {
        match &expr.node {
            Expr::IntLit(_) => Ty::I64,
            Expr::FloatLit(_) => Ty::F64,
            Expr::BoolLit(_) => Ty::Bool,
            Expr::StringLit(_) => Ty::Str,
            Expr::Unit => Ty::Unit,
            Expr::Ident(_) => self.check_ident(expr),
            Expr::BinaryOp { .. } => self.check_binary_op(expr),
            Expr::UnaryOp { .. } => self.check_unary_op(expr),
            Expr::Cast { .. } => self.check_cast(expr),
            Expr::Call { .. } => self.check_call(expr),
            Expr::If { .. } => self.check_if(expr),
            Expr::IfLet { .. } => self.check_if_let(expr),
            Expr::Block { .. } => self.check_block(expr),
            Expr::Assign { .. } => self.check_assign(expr),
            Expr::CompoundAssign { .. } => self.check_compound_assign(expr),
            Expr::FieldAssign { .. } => self.check_field_assign(expr),
            Expr::IndexAssign { .. } => self.check_index_assign(expr),
            Expr::While { .. } => self.check_while(expr),
            Expr::Await(_) => self.check_await(expr),
            Expr::Spawn(_) => self.check_spawn(expr),
            Expr::Try(_) => self.check_try(expr),
            Expr::Range { .. } => self.check_range(expr),
            Expr::ForIn { .. } => self.check_for_in(expr),
            Expr::ArrayLit(_) => self.check_array_lit(expr),
            Expr::Index { .. } => self.check_index(expr),
            Expr::StructLit { .. } => self.check_struct_lit(expr),
            Expr::FieldAccess { .. } => self.check_field_access(expr),
            Expr::OptionalChain { .. } => self.check_optional_chain(expr),
            Expr::EnumVariant { .. } => self.check_enum_variant(expr),
            Expr::Match { .. } => self.check_match(expr),
            Expr::Interpolation(_) => self.check_interpolation(expr),
            Expr::Closure { .. } => self.check_closure(expr),
            Expr::OkExpr(_) => self.check_ok_expr(expr),
            Expr::ErrExpr(_) => self.check_err_expr(expr),
            Expr::SomeExpr(_) => self.check_some_expr(expr),
            Expr::NoneExpr => {
                // Return a partial optional type -- the inner type is unknown without context
                Ty::Optional(Box::new(Ty::Error))
            }
            Expr::Break => self.check_break(expr),
            Expr::Continue => self.check_continue(expr),
            Expr::MapLit(_) => self.check_map_lit(expr),
            Expr::NullCoalesce { .. } => self.check_null_coalesce(expr),
        }
    }

    pub(crate) fn check_ident(&mut self, expr: &Spanned<Expr>) -> Ty {
        let Expr::Ident(name) = &expr.node else {
            unreachable!()
        };
        if let Some(info) = self.lookup_var(name) {
            info.ty.clone()
        } else if let Some(fn_ty) = self.named_fn_value_ty(name) {
            // A bare function name used as a value (not called): it becomes a
            // first-class function value with the function's signature. Only
            // non-generic, non-async, non-`@unsafe`, non-FFI functions can be
            // used this way.
            fn_ty
        } else if let Some(sig) = self.functions.get(name).filter(|s| s.is_unsafe).cloned() {
            // An `@unsafe` function cannot be used as a first-class value:
            // invoking it through a value bypasses the per-call unsafe-context
            // check, letting an unsafe call run from safe code. Reject it here
            // regardless of context. Still yield the function's type so a
            // subsequent call doesn't cascade into a spurious second error.
            self.error(
                ErrorCode::E0530,
                format!(
                    "cannot use `@unsafe` function `{name}` as a value — call it directly inside an `@unsafe` context"
                ),
                expr.span.clone(),
            );
            let param_tys = sig.params.iter().map(|(_, ty)| ty.clone()).collect();
            Ty::Fn(param_tys, Box::new(sig.ret))
        } else {
            let in_scope: Vec<&str> = self
                .scopes
                .iter()
                .flat_map(|s| s.vars.keys().map(String::as_str))
                .collect();
            let msg = match suggest::suggest_for(name, in_scope.iter().copied()) {
                Some(hit) => format!("undefined variable `{name}`. did you mean `{hit}`?"),
                None => format!("undefined variable `{name}`"),
            };
            self.error(ErrorCode::E0300, msg, expr.span.clone());
            Ty::Error
        }
    }

    /// If `name` is a top-level user function usable as a first-class value,
    /// return its `fn(params) -> ret` type. Generic, async, `@unsafe`, and FFI
    /// functions are excluded: the first three because their calling convention
    /// isn't a plain uniform fat pair, and `@unsafe` because a value form would
    /// let an unsafe call escape the unsafe-context check. This list must match
    /// codegen's adapter-generation filter in `compile.rs`.
    pub(crate) fn named_fn_value_ty(&self, name: &str) -> Option<Ty> {
        if name == "main" || self.extern_fns.contains(name) {
            return None;
        }
        let sig = self.functions.get(name)?;
        if !sig.type_params.is_empty() || sig.is_async || sig.is_unsafe {
            return None;
        }
        let param_tys = sig.params.iter().map(|(_, ty)| ty.clone()).collect();
        Some(Ty::Fn(param_tys, Box::new(sig.ret.clone())))
    }

    /// Type-check a call made through a first-class function value whose type is
    /// `Ty::Fn(param_tys, ret)`. Emits E0530 on arity or argument-type mismatch
    /// and returns the function value's return type.
    pub(crate) fn check_fn_value_call(
        &mut self,
        param_tys: &[Ty],
        ret_ty: &Ty,
        args: &[Spanned<Expr>],
        call_span: turbo_ast::Span,
    ) -> Ty {
        if args.len() != param_tys.len() {
            self.error(
                ErrorCode::E0530,
                format!(
                    "function value expects {} argument(s) but {} were given",
                    param_tys.len(),
                    args.len()
                ),
                call_span,
            );
            return ret_ty.clone();
        }
        for (i, arg) in args.iter().enumerate() {
            let arg_ty = self.check_expr_expecting(arg, &param_tys[i]);
            if !arg_ty.contains_error()
                && !param_tys[i].contains_error()
                && !types_compatible(&param_tys[i], &arg_ty)
                && arg_ty != param_tys[i]
            {
                self.error(
                    ErrorCode::E0530,
                    format!(
                        "argument {} expects `{}`, found `{arg_ty}`",
                        i + 1,
                        param_tys[i]
                    ),
                    arg.span.clone(),
                );
            }
        }
        ret_ty.clone()
    }

    pub(crate) fn check_binary_op(&mut self, expr: &Spanned<Expr>) -> Ty {
        let Expr::BinaryOp { left, op, right } = &expr.node else {
            unreachable!()
        };
        let lhs = self.check_expr(left);
        let rhs = self.check_expr(right);

        if lhs.is_error() || rhs.is_error() {
            return Ty::Error;
        }

        match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
                // String concatenation: str + str
                if *op == BinOp::Add && lhs == Ty::Str && rhs == Ty::Str {
                    return Ty::Str;
                }
                // Reject mixed str + non-str in arithmetic — use to_str() or string interpolation
                if *op == BinOp::Add {
                    if lhs == Ty::Str && rhs != Ty::Str {
                        self.error(
                                    ErrorCode::E0102,
                                    format!("cannot add `str` and `{rhs}` — use to_str() or string interpolation"),
                                    expr.span.clone(),
                                );
                        return Ty::Error;
                    }
                    if rhs == Ty::Str && lhs != Ty::Str {
                        self.error(
                                    ErrorCode::E0102,
                                    format!("cannot add `{lhs}` and `str` — use to_str() or string interpolation"),
                                    expr.span.clone(),
                                );
                        return Ty::Error;
                    }
                }
                if !lhs.is_numeric() {
                    self.error(
                        ErrorCode::E0101,
                        format!("cannot perform arithmetic on `{lhs}`"),
                        left.span.clone(),
                    );
                    return Ty::Error;
                }
                if lhs != rhs {
                    // An untyped numeric *literal* operand coerces into
                    // the other operand's sized numeric type, so idioms
                    // like `n + 1` (where `n: i32`) or `x * 2.0` (where
                    // `x: f32`) type-check to the sized type. Two
                    // differently-typed sized values still mismatch and
                    // require an explicit `as` cast.
                    if literal_coerces_to(&right.node, &lhs) {
                        return lhs;
                    }
                    if literal_coerces_to(&left.node, &rhs) {
                        return rhs;
                    }
                    self.error(
                        ErrorCode::E0102,
                        format!("mismatched types in arithmetic: `{lhs}` and `{rhs}`"),
                        expr.span.clone(),
                    );
                    return Ty::Error;
                }
                lhs
            }
            BinOp::Eq
            | BinOp::NotEq
            | BinOp::Less
            | BinOp::LessEq
            | BinOp::Greater
            | BinOp::GreaterEq => {
                if lhs != rhs {
                    self.error(
                        ErrorCode::E0103,
                        format!("cannot compare `{lhs}` with `{rhs}`"),
                        expr.span.clone(),
                    );
                    return Ty::Error;
                }
                // Struct equality requires @derive(Eq)
                if let Ty::Struct(ref struct_name) = lhs {
                    if matches!(op, BinOp::Eq | BinOp::NotEq) {
                        if let Some(info) = self.structs.get(struct_name) {
                            if !info.derives.contains(&"Eq".to_string()) {
                                self.error(ErrorCode::E0128,
                                            format!("cannot compare struct `{struct_name}` with `==`/`!=` without `@derive(Eq)`"),
                                            expr.span.clone(),
                                        );
                                return Ty::Error;
                            }
                        }
                    } else {
                        self.error(
                            ErrorCode::E0129,
                            format!("cannot use ordering comparison on struct `{struct_name}`"),
                            expr.span.clone(),
                        );
                        return Ty::Error;
                    }
                }
                Ty::Bool
            }
            BinOp::And | BinOp::Or => {
                if lhs != Ty::Bool {
                    self.error(
                        ErrorCode::E0104,
                        format!("expected `bool` in logical operation, found `{lhs}`"),
                        left.span.clone(),
                    );
                }
                if rhs != Ty::Bool {
                    self.error(
                        ErrorCode::E0104,
                        format!("expected `bool` in logical operation, found `{rhs}`"),
                        right.span.clone(),
                    );
                }
                Ty::Bool
            }
        }
    }

    pub(crate) fn check_unary_op(&mut self, expr: &Spanned<Expr>) -> Ty {
        let Expr::UnaryOp { op, expr: inner } = &expr.node else {
            unreachable!()
        };
        let ty = self.check_expr(inner);
        if ty.is_error() {
            return Ty::Error;
        }
        match op {
            UnaryOp::Neg => {
                if !ty.is_numeric() {
                    self.error(
                        ErrorCode::E0105,
                        format!("cannot negate `{ty}`"),
                        inner.span.clone(),
                    );
                    Ty::Error
                } else {
                    ty
                }
            }
            UnaryOp::Not => {
                if ty != Ty::Bool {
                    self.error(
                        ErrorCode::E0106,
                        format!("cannot apply `!` to `{ty}`, expected `bool`"),
                        inner.span.clone(),
                    );
                    Ty::Error
                } else {
                    Ty::Bool
                }
            }
        }
    }

    pub(crate) fn check_cast(&mut self, expr: &Spanned<Expr>) -> Ty {
        let Expr::Cast { expr: inner, ty } = &expr.node else {
            unreachable!()
        };
        let from = self.check_expr(inner);
        let Some(target) = resolve_type_expr(&ty.node, Some(&self.structs), Some(&self.enums))
        else {
            if let TypeExpr::Named(name) = &ty.node {
                self.error(
                    ErrorCode::E0305,
                    format!("unknown type `{name}` in cast"),
                    ty.span.clone(),
                );
            }
            return Ty::Error;
        };
        if from.is_error() {
            // Suppress cascading errors but adopt the user's intended
            // target type so downstream checks see something concrete.
            return target;
        }
        // Allowed casts: any numeric ↔ numeric conversion, plus a
        // no-op identity cast. Everything else (e.g. `str as i32`) is
        // rejected with the general type-mismatch code E0100 — there is
        // no dedicated cast error code, and E0100 reads naturally here.
        if (from.is_numeric() && target.is_numeric()) || from == target {
            target
        } else {
            self.error(
                ErrorCode::E0100,
                format!("cannot cast `{from}` to `{target}`"),
                expr.span.clone(),
            );
            Ty::Error
        }
    }

    pub(crate) fn check_call(&mut self, expr: &Spanned<Expr>) -> Ty {
        let Expr::Call { callee, args } = &expr.node else {
            unreachable!()
        };
        if let Expr::Ident(name) = &callee.node {
            // Built-in functions. An explicit `extern "C"` declaration of the
            // same name takes precedence (so e.g. `extern fn floor(x: f64) ->
            // f64` is honored as a float-returning FFI call rather than the
            // int-returning native `floor` builtin) — skip the builtin lookup
            // and fall through to the registered function signature below.
            if !self.extern_fns.contains(name) {
                if let Some(builtin_ty) = self.check_builtin_call(name, args, callee) {
                    return builtin_ty;
                }
            }

            // Check if callee is a variable with fn type (closure call)
            if let Some(info) = self.lookup_var(name) {
                if let Ty::Fn(ref param_tys, ref ret_ty) = info.ty {
                    if args.len() != param_tys.len() {
                        self.error(
                            ErrorCode::E0100,
                            format!(
                                "closure expects {} argument(s) but {} were given",
                                param_tys.len(),
                                args.len()
                            ),
                            callee.span.clone(),
                        );
                        return *ret_ty.clone();
                    }
                    for (i, arg) in args.iter().enumerate() {
                        // Hint the closure's parameter type so a bare empty
                        // array literal `[]` infers its element type (BL-26).
                        let arg_ty = self.check_expr_expecting(arg, &param_tys[i]);
                        if !arg_ty.contains_error()
                            && !param_tys[i].contains_error()
                            && !types_compatible(&param_tys[i], &arg_ty)
                            && arg_ty != param_tys[i]
                        {
                            self.error(
                                ErrorCode::E0100,
                                format!(
                                    "argument {} expects `{}`, found `{arg_ty}`",
                                    i + 1,
                                    param_tys[i]
                                ),
                                arg.span.clone(),
                            );
                        }
                    }
                    return *ret_ty.clone();
                }
                // Variable exists but is not a function type -- fall through to check named functions
            }

            // User-defined function
            if let Some(sig) = self.functions.get(name).cloned() {
                // Calling an @unsafe function from a safe context is an error
                if sig.is_unsafe && !self.in_unsafe_context {
                    self.error(
                        ErrorCode::E0100,
                        format!("cannot call `@unsafe` function `{name}` from a safe context"),
                        callee.span.clone(),
                    );
                }
                if args.len() != sig.params.len() {
                    // Echo the full signature so the diagnostic (and the
                    // CLI's `Help:` line) can name each parameter the
                    // function actually expects, not just a count.
                    let sig_params = sig
                        .params
                        .iter()
                        .map(|(pname, pty)| format!("{pname}: {pty}"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    self.error(
                                ErrorCode::E0100,
                                format!(
                                    "function `{name}` expects {} argument(s) but {} were given; signature `{name}({sig_params})`",
                                    sig.params.len(),
                                    args.len()
                                ),
                                callee.span.clone(),
                            );
                    return self.substitute_return_type(&sig, &HashMap::new());
                }

                // Check arguments and build substitution map for generic type params
                let mut substitutions: HashMap<String, Ty> = HashMap::new();
                let mut arg_types = Vec::new();
                for (i, arg) in args.iter().enumerate() {
                    let (_, ref param_ty) = &sig.params[i];
                    // Pass the declared parameter type as a hint so a bare
                    // empty array literal `[]` infers its element type from
                    // the parameter (e.g. `f([])` where `f(xs: [str])`)
                    // instead of failing with E0115 (BL-26).
                    let arg_ty = self.check_expr_expecting(arg, param_ty);
                    arg_types.push(arg_ty.clone());

                    // If param type is a type parameter, infer its concrete type
                    if let Ty::TypeParam(ref tp_name) = param_ty {
                        if let Some(existing) = substitutions.get(tp_name) {
                            // T already inferred -- check consistency
                            if !arg_ty.is_error() && !existing.is_error() && arg_ty != *existing {
                                self.error(ErrorCode::E0100,
                                            format!(
                                                "type parameter `{tp_name}` inferred as `{existing}` but argument has type `{arg_ty}`"
                                            ),
                                            arg.span.clone(),
                                        );
                            }
                        } else if !arg_ty.is_error() {
                            substitutions.insert(tp_name.clone(), arg_ty.clone());
                        }
                    }
                }

                // Now check argument types against substituted parameter types
                for (i, arg) in args.iter().enumerate() {
                    let arg_ty = &arg_types[i];
                    let (ref param_name, ref param_ty) = &sig.params[i];
                    let concrete_param_ty = self.substitute_ty(param_ty, &substitutions);
                    if !arg_ty.contains_error()
                        && !concrete_param_ty.contains_error()
                        && !matches!(concrete_param_ty, Ty::TypeParam(_))
                        && !types_compatible(&concrete_param_ty, arg_ty)
                        && *arg_ty != concrete_param_ty
                    {
                        // Allow integer literal coercion: i64 literal -> narrower int types
                        let is_int_literal_coercion = *arg_ty == Ty::I64
                            && matches!(
                                concrete_param_ty,
                                Ty::I8 | Ty::I16 | Ty::I32 | Ty::U8 | Ty::U16 | Ty::U32 | Ty::U64
                            )
                            && extract_int_literal(&arg.node)
                                .is_some_and(|n| int_literal_fits_in_type(n, &concrete_param_ty));
                        if !is_int_literal_coercion {
                            self.error(ErrorCode::E0100,
                                        format!(
                                            "argument `{param_name}` expects `{concrete_param_ty}`, found `{arg_ty}`"
                                        ),
                                        arg.span.clone(),
                                    );
                        }
                    }
                }

                // Check trait bounds for each inferred type parameter
                for (tp_name, concrete_ty) in &substitutions {
                    if let Some(bounds) = sig.type_param_bounds.get(tp_name) {
                        for bound in bounds {
                            let type_name = match concrete_ty {
                                Ty::Struct(s) => Some(s.as_str()),
                                _ => None,
                            };
                            let has_impl = type_name.is_some_and(|tn| {
                                self.trait_impls
                                    .get(tn)
                                    .is_some_and(|impls| impls.contains(bound))
                            });
                            if !has_impl && !concrete_ty.is_error() {
                                self.error(
                                    ErrorCode::E0100,
                                    format!(
                                        "type `{concrete_ty}` does not implement trait `{bound}`"
                                    ),
                                    callee.span.clone(),
                                );
                            }
                        }
                    }
                }

                self.substitute_return_type(&sig, &substitutions)
            } else {
                // Check if this is an enum variant construction via UFCS rewrite:
                // Parser transforms Shape.Circle(5.0) into Call { callee: Ident("Circle"), args: [Ident("Shape"), 5.0] }
                if !args.is_empty() {
                    if let Expr::Ident(ref first_name) = args[0].node {
                        if let Some(info) = self.enums.get(first_name).cloned() {
                            if let Some(field_tys) = info.variant_fields(name) {
                                // This is an enum variant construction
                                let expected_args = field_tys.len();
                                let actual_args = args.len() - 1; // subtract the enum type name
                                if actual_args != expected_args {
                                    self.error(ErrorCode::E0100,
                                                format!(
                                                    "variant `{name}` of enum `{first_name}` expects {} argument(s) but {} were given",
                                                    expected_args, actual_args
                                                ),
                                                callee.span.clone(),
                                            );
                                }
                                // Type-check arguments against variant field types
                                for (i, arg) in args.iter().skip(1).enumerate() {
                                    let arg_ty = self.check_expr(arg);
                                    // Skip type check for generic (TypeParam) fields
                                    if i < field_tys.len()
                                        && !matches!(&field_tys[i], Ty::TypeParam(_))
                                        && !arg_ty.is_error()
                                        && !field_tys[i].is_error()
                                        && arg_ty != field_tys[i]
                                    {
                                        self.error(ErrorCode::E0100,
                                                    format!(
                                                        "variant `{name}` field {} expects `{}`, found `{arg_ty}`",
                                                        i + 1, field_tys[i]
                                                    ),
                                                    arg.span.clone(),
                                                );
                                    }
                                }
                                return Ty::Enum(first_name.clone());
                            }
                        }
                    }
                }

                // Before reporting "undefined function", check if this is a UFCS method call.
                // The parser transforms `obj.method(args)` into `method(obj, args)`,
                // so the first arg is the receiver.
                if !args.is_empty() {
                    let first_arg_ty = self.check_expr(&args[0]);
                    if let Ty::Struct(ref type_name) = first_arg_ty {
                        if let Some(method_sig) = self
                            .methods
                            .get(type_name)
                            .and_then(|m| m.get(name))
                            .cloned()
                        {
                            // Check argument count (all args including self)
                            if args.len() != method_sig.params.len() {
                                self.error(ErrorCode::E0100,
                                            format!(
                                                "method `{name}` on `{type_name}` expects {} argument(s) but {} were given",
                                                method_sig.params.len() - 1,
                                                args.len() - 1
                                            ),
                                            callee.span.clone(),
                                        );
                                return method_sig.ret;
                            }
                            // Check argument types (skip self at index 0, already checked)
                            for (i, arg) in args.iter().skip(1).enumerate() {
                                let arg_ty = self.check_expr(arg);
                                let (ref param_name, ref param_ty) = method_sig.params[i + 1];
                                if !arg_ty.contains_error()
                                    && !param_ty.contains_error()
                                    && !types_compatible(param_ty, &arg_ty)
                                    && arg_ty != *param_ty
                                {
                                    self.error(ErrorCode::E0100,
                                                format!("argument `{param_name}` expects `{param_ty}`, found `{arg_ty}`"),
                                                arg.span.clone(),
                                            );
                                }
                            }
                            return method_sig.ret;
                        }
                        // No method `name`, but the receiver may have a field
                        // named `name` holding a function value: `obj.f(x)`
                        // where `f: fn(...) -> ...`. Methods take precedence;
                        // this is the field-value-invocation fallback.
                        if let Some(field_ty) = self
                            .structs
                            .get(type_name)
                            .and_then(|s| s.fields.iter().find(|(n, _)| n == name))
                            .map(|(_, ty)| ty.clone())
                        {
                            match field_ty {
                                Ty::Fn(ref param_tys, ref ret_ty) => {
                                    // args[0] is the receiver; the call
                                    // arguments are the remaining args.
                                    return self.check_fn_value_call(
                                        param_tys,
                                        ret_ty,
                                        &args[1..],
                                        callee.span.clone(),
                                    );
                                }
                                other => {
                                    self.error(
                                        ErrorCode::E0530,
                                        format!(
                                            "field `{name}` of `{type_name}` is `{other}`, not a function"
                                        ),
                                        callee.span.clone(),
                                    );
                                    return Ty::Error;
                                }
                            }
                        }
                    }
                }
                // Offer a "did you mean" against user-defined functions
                // and builtins — typos on common builtins like `print`
                // are the most frequent beginner error.
                let candidates: Vec<&str> = self
                    .functions
                    .keys()
                    .map(String::as_str)
                    .chain(crate::type_check::BUILTIN_FNS.iter().copied())
                    .collect();
                let msg = match suggest::suggest_for(name, candidates.iter().copied()) {
                    Some(hit) => {
                        format!("undefined function `{name}`. did you mean `{hit}`?")
                    }
                    None => format!("undefined function `{name}`"),
                };
                self.error(ErrorCode::E0301, msg, callee.span.clone());
                Ty::Error
            }
        } else if let Expr::FieldAccess { object, field } = &callee.node {
            // Method call: object.method(args) — fallback for non-UFCS path
            let obj_ty = self.check_expr(object);
            if let Ty::Struct(ref type_name) = obj_ty {
                if let Some(method_sig) = self
                    .methods
                    .get(type_name)
                    .and_then(|m| m.get(field))
                    .cloned()
                {
                    let expected_args = method_sig.params.len() - 1;
                    if args.len() != expected_args {
                        self.error(ErrorCode::E0100,
                                    format!(
                                        "method `{field}` on `{type_name}` expects {} argument(s) but {} were given",
                                        expected_args, args.len()
                                    ),
                                    callee.span.clone(),
                                );
                        return method_sig.ret;
                    }
                    for (i, arg) in args.iter().enumerate() {
                        let arg_ty = self.check_expr(arg);
                        let (ref param_name, ref param_ty) = method_sig.params[i + 1];
                        if !arg_ty.contains_error()
                            && !param_ty.contains_error()
                            && !types_compatible(param_ty, &arg_ty)
                            && arg_ty != *param_ty
                        {
                            self.error(ErrorCode::E0100,
                                        format!("argument `{param_name}` expects `{param_ty}`, found `{arg_ty}`"),
                                        arg.span.clone(),
                                    );
                        }
                    }
                    method_sig.ret
                } else if let Some(field_ty) = self
                    .structs
                    .get(type_name)
                    .and_then(|s| s.fields.iter().find(|(n, _)| n == field))
                    .map(|(_, ty)| ty.clone())
                {
                    // No method named `field`, but the struct has a field with
                    // that name. If it holds a function value, invoke it (e.g.
                    // `(obj.handler)(x)`); otherwise it is not callable.
                    match field_ty {
                        Ty::Fn(ref param_tys, ref ret_ty) => {
                            self.check_fn_value_call(param_tys, ret_ty, args, callee.span.clone())
                        }
                        other => {
                            self.error(
                                ErrorCode::E0530,
                                format!(
                                    "field `{field}` of `{type_name}` is `{other}`, not a function"
                                ),
                                callee.span.clone(),
                            );
                            Ty::Error
                        }
                    }
                } else {
                    self.error(
                        ErrorCode::E0317,
                        format!("no method `{field}` found on type `{type_name}`"),
                        callee.span.clone(),
                    );
                    Ty::Error
                }
            } else if obj_ty.is_error() {
                Ty::Error
            } else {
                self.error(
                    ErrorCode::E0134,
                    format!("cannot call method `{field}` on type `{obj_ty}`"),
                    callee.span.clone(),
                );
                Ty::Error
            }
        } else {
            // Call through an arbitrary expression callee, e.g.
            // `make_adder(3)(4)` (Call callee) or `handlers[i](x)` (Index
            // callee). The callee must evaluate to a first-class function
            // value; otherwise it is not callable.
            let callee_ty = self.check_expr(callee);
            match callee_ty {
                Ty::Fn(ref param_tys, ref ret_ty) => {
                    self.check_fn_value_call(param_tys, ret_ty, args, callee.span.clone())
                }
                Ty::Error => Ty::Error,
                other => {
                    self.error(
                        ErrorCode::E0530,
                        format!("cannot call a value of type `{other}` — it is not a function"),
                        callee.span.clone(),
                    );
                    Ty::Error
                }
            }
        }
    }

    pub(crate) fn check_if(&mut self, expr: &Spanned<Expr>) -> Ty {
        let Expr::If {
            condition,
            then_branch,
            else_branch,
        } = &expr.node
        else {
            unreachable!()
        };
        let cond_ty = self.check_expr(condition);
        if !cond_ty.is_error() && cond_ty != Ty::Bool {
            // Allow integer conditions (truthy)
            if !cond_ty.is_integer() {
                self.error(
                    ErrorCode::E0116,
                    format!("if condition must be `bool`, found `{cond_ty}`"),
                    condition.span.clone(),
                );
            }
        }

        let then_ty = self.check_expr(then_branch);

        if let Some(else_expr) = else_branch {
            let else_ty = self.check_expr(else_expr);
            // If used as expression (both branches must match)
            if !then_ty.is_error() && !else_ty.is_error() && !types_compatible(&then_ty, &else_ty) {
                // Only warn if both are non-unit (meaning it's used as an expression)
                if then_ty != Ty::Unit && else_ty != Ty::Unit {
                    self.error(
                        ErrorCode::E0107,
                        format!(
                            "if/else branches have different types: `{then_ty}` and `{else_ty}`"
                        ),
                        expr.span.clone(),
                    );
                }
            }
            then_ty
        } else {
            Ty::Unit
        }
    }

    pub(crate) fn check_if_let(&mut self, expr: &Spanned<Expr>) -> Ty {
        let Expr::IfLet {
            pattern,
            value,
            then_branch,
            else_branch,
        } = &expr.node
        else {
            unreachable!()
        };
        let val_ty = self.check_expr(value);

        // Type-check: push scope, bind variable, check then branch
        let then_ty = match &pattern.node {
            Pattern::Some(binding) => {
                self.push_scope();
                let inner_ty = if let Ty::Optional(ref inner) = val_ty {
                    *inner.clone()
                } else {
                    if !val_ty.is_error() {
                        self.error(
                            ErrorCode::E0116,
                            format!(
                                "`if let some(...)` requires an optional type, found `{val_ty}`"
                            ),
                            value.span.clone(),
                        );
                    }
                    Ty::Error
                };
                self.define_var(
                    binding,
                    VarInfo {
                        ty: inner_ty,
                        mutable: false,
                        span: 0..0,
                        from_let: false,
                    },
                    &pattern.span,
                );
                let ty = self.check_expr(then_branch);
                self.pop_scope();
                ty
            }
            Pattern::None => {
                if !val_ty.is_error() && !matches!(val_ty, Ty::Optional(_)) {
                    self.error(
                        ErrorCode::E0116,
                        format!("`if let none` requires an optional type, found `{val_ty}`"),
                        value.span.clone(),
                    );
                }
                self.check_expr(then_branch)
            }
            Pattern::Ok(binding) => {
                self.push_scope();
                let ok_ty = if let Ty::Result(ref ok, _) = val_ty {
                    *ok.clone()
                } else {
                    if !val_ty.is_error() {
                        self.error(
                            ErrorCode::E0116,
                            format!("`if let ok(...)` requires a result type, found `{val_ty}`"),
                            value.span.clone(),
                        );
                    }
                    Ty::Error
                };
                self.define_var(
                    binding,
                    VarInfo {
                        ty: ok_ty,
                        mutable: false,
                        span: 0..0,
                        from_let: false,
                    },
                    &pattern.span,
                );
                let ty = self.check_expr(then_branch);
                self.pop_scope();
                ty
            }
            Pattern::Err(binding) => {
                self.push_scope();
                let err_ty = if let Ty::Result(_, ref err) = val_ty {
                    *err.clone()
                } else {
                    if !val_ty.is_error() {
                        self.error(
                            ErrorCode::E0116,
                            format!("`if let err(...)` requires a result type, found `{val_ty}`"),
                            value.span.clone(),
                        );
                    }
                    Ty::Error
                };
                self.define_var(
                    binding,
                    VarInfo {
                        ty: err_ty,
                        mutable: false,
                        span: 0..0,
                        from_let: false,
                    },
                    &pattern.span,
                );
                let ty = self.check_expr(then_branch);
                self.pop_scope();
                ty
            }
            _ => {
                self.error(
                    ErrorCode::E0116,
                    "unsupported pattern in `if let`".to_string(),
                    pattern.span.clone(),
                );
                self.check_expr(then_branch)
            }
        };

        if let Some(else_expr) = else_branch {
            let _else_ty = self.check_expr(else_expr);
            then_ty
        } else {
            Ty::Unit
        }
    }

    pub(crate) fn check_block(&mut self, expr: &Spanned<Expr>) -> Ty {
        let Expr::Block { stmts, tail_expr } = &expr.node else {
            unreachable!()
        };
        self.push_scope();

        // Consume any function-body tail-return hint at block entry so it only
        // applies to *this* block's tail. Taking it here means nested blocks
        // checked while walking `stmts` see `None` and can't mis-borrow the
        // outer function's return type (BL-26).
        let tail_hint = self.fn_body_tail_hint.take();

        for stmt in stmts {
            self.check_stmt(stmt);
        }

        let ty = if let Some(tail) = tail_expr {
            match &tail_hint {
                Some(expected) => self.check_expr_expecting(tail, expected),
                None => self.check_expr(tail),
            }
        } else {
            Ty::Unit
        };

        self.pop_scope();
        ty
    }

    pub(crate) fn check_assign(&mut self, expr: &Spanned<Expr>) -> Ty {
        let Expr::Assign { target, value } = &expr.node else {
            unreachable!()
        };
        let val_ty = self.check_expr(value);

        if let Some(info) = self.lookup_var(target) {
            if !info.mutable {
                self.error(
                    ErrorCode::E0501,
                    format!("cannot assign to immutable variable `{target}`"),
                    expr.span.clone(),
                );
            }
            if !val_ty.contains_error()
                && !info.ty.contains_error()
                && !types_compatible(&info.ty, &val_ty)
                && val_ty != info.ty
            {
                self.error(
                    ErrorCode::E0111,
                    format!(
                        "cannot assign `{val_ty}` to variable `{target}` of type `{}`",
                        info.ty
                    ),
                    value.span.clone(),
                );
            }
        } else {
            self.error(
                ErrorCode::E0300,
                format!("undefined variable `{target}`"),
                expr.span.clone(),
            );
        }

        Ty::Unit
    }

    pub(crate) fn check_compound_assign(&mut self, expr: &Spanned<Expr>) -> Ty {
        let Expr::CompoundAssign { target, op, value } = &expr.node else {
            unreachable!()
        };
        let val_ty = self.check_expr(value);

        let op_str = match op {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Mod => "%",
            _ => {
                self.error(
                    ErrorCode::E0137,
                    "unsupported compound assignment operator".to_string(),
                    expr.span.clone(),
                );
                return Ty::Unit;
            }
        };

        if let Some(info) = self.lookup_var(target) {
            if !info.mutable {
                self.error(
                    ErrorCode::E0501,
                    format!("cannot assign to immutable variable `{target}`"),
                    expr.span.clone(),
                );
            }
            if !val_ty.contains_error()
                && !info.ty.contains_error()
                && !types_compatible(&info.ty, &val_ty)
                && val_ty != info.ty
            {
                self.error(ErrorCode::E0130,
                            format!(
                                "cannot apply `{op_str}=` with `{val_ty}` to variable `{target}` of type `{}`",
                                info.ty
                            ),
                            value.span.clone(),
                        );
            }
            if !info.ty.is_numeric() && !info.ty.is_error() {
                self.error(
                    ErrorCode::E0300,
                    format!("cannot perform arithmetic on `{}`", info.ty),
                    expr.span.clone(),
                );
            }
        } else {
            self.error(
                ErrorCode::E0300,
                format!("undefined variable `{target}`"),
                expr.span.clone(),
            );
        }

        Ty::Unit
    }

    pub(crate) fn check_field_assign(&mut self, expr: &Spanned<Expr>) -> Ty {
        let Expr::FieldAssign {
            object,
            field,
            value,
        } = &expr.node
        else {
            unreachable!()
        };
        let val_ty = self.check_expr(value);
        let obj_ty = self.check_expr(object);

        // Check mutability of the root variable
        if let Some(root_name) = Self::root_var_name(object) {
            if let Some(info) = self.lookup_var(&root_name) {
                if !info.mutable {
                    self.error(ErrorCode::E0502,
                                format!("cannot assign to field `{field}` of immutable variable `{root_name}` (declare with `let mut` to make mutable)"),
                                expr.span.clone(),
                            );
                }
            }
        }

        // Check field exists and type matches
        if let Ty::Struct(struct_name) = &obj_ty {
            if let Some(struct_info) = self.structs.get(struct_name).cloned() {
                if let Some((_, field_ty)) = struct_info.fields.iter().find(|(n, _)| n == field) {
                    // Skip type check for generic (TypeParam) fields — they accept any type
                    if !matches!(field_ty, Ty::TypeParam(_))
                        && !val_ty.is_error()
                        && !field_ty.is_error()
                        && val_ty != *field_ty
                    {
                        self.error(
                            ErrorCode::E0112,
                            format!(
                                "cannot assign `{val_ty}` to field `{field}` of type `{field_ty}`"
                            ),
                            value.span.clone(),
                        );
                    }
                } else {
                    self.error(
                        ErrorCode::E0315,
                        crate::no_such_field_message(struct_name, field, &struct_info.fields),
                        expr.span.clone(),
                    );
                }
            }
        } else if !obj_ty.is_error() {
            self.error(
                ErrorCode::E0135,
                format!("cannot assign to field `{field}` on non-struct type `{obj_ty}`"),
                object.span.clone(),
            );
        }

        Ty::Unit
    }

    pub(crate) fn check_index_assign(&mut self, expr: &Spanned<Expr>) -> Ty {
        let Expr::IndexAssign {
            object,
            index,
            value,
        } = &expr.node
        else {
            unreachable!()
        };
        let val_ty = self.check_expr(value);
        let obj_ty = self.check_expr(object);
        let idx_ty = self.check_expr(index);

        // Check mutability of the root variable
        if let Some(root_name) = Self::root_var_name(object) {
            if let Some(info) = self.lookup_var(&root_name) {
                if !info.mutable {
                    self.error(ErrorCode::E0503,
                                format!("cannot assign to index of immutable variable `{root_name}` (declare with `let mut` to make mutable)"),
                                expr.span.clone(),
                            );
                }
            }
        }

        // Check index is integer
        if !idx_ty.is_error() && !idx_ty.is_integer() {
            self.error(
                ErrorCode::E0123,
                format!("array index must be an integer, found `{idx_ty}`"),
                index.span.clone(),
            );
        }

        // Check object is array and value type matches element type
        match &obj_ty {
            Ty::Array(inner) => {
                if !val_ty.is_error() && !inner.is_error() && val_ty != **inner {
                    self.error(
                        ErrorCode::E0113,
                        format!("cannot assign `{val_ty}` to array of `{inner}`"),
                        value.span.clone(),
                    );
                }
            }
            Ty::Error => {}
            _ => {
                self.error(
                    ErrorCode::E0124,
                    format!("cannot index-assign into `{obj_ty}`"),
                    object.span.clone(),
                );
            }
        }

        Ty::Unit
    }
}
