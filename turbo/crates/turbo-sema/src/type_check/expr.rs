//! Expression type checking for the Turbo semantic analyzer.
//!
//! This module contains `check_expr_inner`, the large match on every
//! [`turbo_ast::Expr`] variant. It is the workhorse of expression-level
//! type inference and validation.

use std::collections::HashMap;

use turbo_ast::*;

use crate::scope::VarInfo;
use crate::suggest;
use crate::{
    extract_int_literal, int_literal_fits_in_type, literal_coerces_to, resolve_type_expr,
    types_compatible, Checker, HandleKind, Ty,
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

    fn check_ident(&mut self, expr: &Spanned<Expr>) -> Ty {
        let Expr::Ident(name) = &expr.node else {
            unreachable!()
        };
        if let Some(info) = self.lookup_var(name) {
            info.ty.clone()
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

    fn check_binary_op(&mut self, expr: &Spanned<Expr>) -> Ty {
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

    fn check_unary_op(&mut self, expr: &Spanned<Expr>) -> Ty {
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

    fn check_cast(&mut self, expr: &Spanned<Expr>) -> Ty {
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

    fn check_call(&mut self, expr: &Spanned<Expr>) -> Ty {
        let Expr::Call { callee, args } = &expr.node else {
            unreachable!()
        };
        if let Expr::Ident(name) = &callee.node {
            // Built-in functions
            if let Some(builtin_ty) = self.check_builtin_call(name, args, callee) {
                return builtin_ty;
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
                        let arg_ty = self.check_expr(arg);
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
                    let arg_ty = self.check_expr(arg);
                    arg_types.push(arg_ty.clone());
                    let (_, ref param_ty) = &sig.params[i];

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
            self.error(
                ErrorCode::E0512,
                "only named function calls are supported".to_string(),
                callee.span.clone(),
            );
            Ty::Error
        }
    }

    fn check_if(&mut self, expr: &Spanned<Expr>) -> Ty {
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

    fn check_if_let(&mut self, expr: &Spanned<Expr>) -> Ty {
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

    fn check_block(&mut self, expr: &Spanned<Expr>) -> Ty {
        let Expr::Block { stmts, tail_expr } = &expr.node else {
            unreachable!()
        };
        self.push_scope();

        for stmt in stmts {
            self.check_stmt(stmt);
        }

        let ty = if let Some(tail) = tail_expr {
            self.check_expr(tail)
        } else {
            Ty::Unit
        };

        self.pop_scope();
        ty
    }

    fn check_assign(&mut self, expr: &Spanned<Expr>) -> Ty {
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

    fn check_compound_assign(&mut self, expr: &Spanned<Expr>) -> Ty {
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

    fn check_field_assign(&mut self, expr: &Spanned<Expr>) -> Ty {
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

    fn check_index_assign(&mut self, expr: &Spanned<Expr>) -> Ty {
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

    fn check_while(&mut self, expr: &Spanned<Expr>) -> Ty {
        let Expr::While { condition, body } = &expr.node else {
            unreachable!()
        };
        let cond_ty = self.check_expr(condition);
        if !cond_ty.is_error() && cond_ty != Ty::Bool && !cond_ty.is_integer() {
            self.error(
                ErrorCode::E0117,
                format!("while condition must be `bool`, found `{cond_ty}`"),
                condition.span.clone(),
            );
        }
        self.loop_depth += 1;
        self.check_expr(body);
        self.loop_depth -= 1;
        Ty::Unit
    }

    fn check_await(&mut self, expr: &Spanned<Expr>) -> Ty {
        let Expr::Await(inner) = &expr.node else {
            unreachable!()
        };
        let ty = self.check_expr(inner);
        // In Sprint 9, await on a Future<T> yields T.
        // Await on a non-future type just passes through (sync await).
        match ty {
            Ty::Future(inner_ty) => *inner_ty,
            other => other,
        }
    }

    fn check_spawn(&mut self, expr: &Spanned<Expr>) -> Ty {
        let Expr::Spawn(inner) = &expr.node else {
            unreachable!()
        };
        let ty = self.check_expr(inner);
        // spawn wraps the result in Future<T>
        if ty.is_error() {
            Ty::Error
        } else {
            Ty::Future(Box::new(ty))
        }
    }

    fn check_try(&mut self, expr: &Spanned<Expr>) -> Ty {
        let Expr::Try(inner) = &expr.node else {
            unreachable!()
        };
        let inner_ty = self.check_expr(inner);
        if inner_ty.is_error() {
            return Ty::Error;
        }
        match &inner_ty {
            Ty::Result(ok_ty, _err_ty) => {
                // The enclosing function must also return a Result type
                match &self.current_return_type {
                    Ty::Result(_, _) => {}
                    _ => {
                        self.error(ErrorCode::E0121,
                                    "`?` operator can only be used in a function that returns a Result type".to_string(),
                                    expr.span.clone(),
                                );
                    }
                }
                *ok_ty.clone()
            }
            _ => {
                self.error(
                    ErrorCode::E0120,
                    format!("`?` operator requires a Result type, found `{inner_ty}`"),
                    inner.span.clone(),
                );
                Ty::Error
            }
        }
    }

    fn check_range(&mut self, expr: &Spanned<Expr>) -> Ty {
        let Expr::Range { start, end } = &expr.node else {
            unreachable!()
        };
        let start_ty = self.check_expr(start);
        let end_ty = self.check_expr(end);
        if !start_ty.is_error() && !start_ty.is_integer() {
            self.error(
                ErrorCode::E0122,
                format!("range start must be an integer, found `{start_ty}`"),
                start.span.clone(),
            );
        }
        if !end_ty.is_error() && !end_ty.is_integer() {
            self.error(
                ErrorCode::E0122,
                format!("range end must be an integer, found `{end_ty}`"),
                end.span.clone(),
            );
        }
        Ty::Unit // Range type (treated as unit for now)
    }

    fn check_for_in(&mut self, expr: &Spanned<Expr>) -> Ty {
        let Expr::ForIn {
            var_name,
            iterable,
            body,
        } = &expr.node
        else {
            unreachable!()
        };
        // Check the iterable expression
        let iter_ty = self.check_expr(iterable);

        // Infer element type from the iterable
        let elem_ty = match &iter_ty {
            Ty::Array(inner) => *inner.clone(),
            _ if !iter_ty.is_error() => {
                // Range or unknown -- default to I64 for ranges
                Ty::I64
            }
            _ => Ty::Error,
        };

        self.push_scope();
        self.define_var(
            var_name,
            VarInfo {
                ty: elem_ty,
                mutable: false,
                span: 0..0,
                from_let: false,
            },
            &expr.span,
        );
        self.loop_depth += 1;
        self.check_expr(body);
        self.loop_depth -= 1;
        self.pop_scope();
        Ty::Unit
    }

    fn check_array_lit(&mut self, expr: &Spanned<Expr>) -> Ty {
        let Expr::ArrayLit(elements) = &expr.node else {
            unreachable!()
        };
        if elements.is_empty() {
            self.error(
                ErrorCode::E0115,
                "cannot infer type of empty array".to_string(),
                expr.span.clone(),
            );
            return Ty::Error;
        }
        let mut first_ty = self.check_expr(&elements[0]);
        if first_ty == Ty::Unit {
            self.error(
                ErrorCode::E0100,
                "cannot use unit value `()` as an array element".to_string(),
                elements[0].span.clone(),
            );
            // Poison to suppress cascading element-mismatch errors.
            first_ty = Ty::Error;
        }
        for elem in &elements[1..] {
            let elem_ty = self.check_expr(elem);
            if elem_ty == Ty::Unit {
                self.error(
                    ErrorCode::E0100,
                    "cannot use unit value `()` as an array element".to_string(),
                    elem.span.clone(),
                );
            } else if !elem_ty.is_error() && !first_ty.is_error() && elem_ty != first_ty {
                self.error(ErrorCode::E0114,
                            format!("array elements must all have the same type, expected `{first_ty}` but found `{elem_ty}`"),
                            elem.span.clone(),
                        );
            }
        }
        Ty::Array(Box::new(first_ty))
    }

    fn check_index(&mut self, expr: &Spanned<Expr>) -> Ty {
        let Expr::Index { object, index } = &expr.node else {
            unreachable!()
        };
        let obj_ty = self.check_expr(object);
        let idx_ty = self.check_expr(index);
        if !idx_ty.is_error() && !idx_ty.is_integer() {
            self.error(
                ErrorCode::E0123,
                format!("array index must be an integer, found `{idx_ty}`"),
                index.span.clone(),
            );
        }
        match &obj_ty {
            Ty::Array(inner) => *inner.clone(),
            Ty::Error => Ty::Error,
            _ => {
                self.error(
                    ErrorCode::E0124,
                    format!("cannot index into `{obj_ty}`"),
                    object.span.clone(),
                );
                Ty::Error
            }
        }
    }

    fn check_struct_lit(&mut self, expr: &Spanned<Expr>) -> Ty {
        let Expr::StructLit { name, fields } = &expr.node else {
            unreachable!()
        };
        let Some(struct_info) = self.structs.get(name).cloned() else {
            self.error(
                ErrorCode::E0302,
                format!("undefined struct `{name}`"),
                expr.span.clone(),
            );
            // Still check field value expressions
            for (_, value) in fields {
                self.check_expr(value);
            }
            return Ty::Error;
        };

        // Check that all fields are provided and types match
        let expected_fields: HashMap<&str, &Ty> = struct_info
            .fields
            .iter()
            .map(|(n, t)| (n.as_str(), t))
            .collect();

        // Track type parameter inference for generic structs
        let mut tp_inferred: HashMap<String, Ty> = HashMap::new();

        let mut provided = std::collections::HashSet::new();
        for (field_name, value) in fields {
            let val_ty = self.check_expr(value);
            if let Some(expected_ty) = expected_fields.get(field_name.as_str()) {
                if let Ty::TypeParam(ref tp_name) = expected_ty {
                    // Generic field: infer or check consistency
                    if !val_ty.is_error() {
                        if let Some(prev) = tp_inferred.get(tp_name) {
                            if prev != &val_ty {
                                self.error(ErrorCode::E0131,
                                            format!(
                                                "type parameter `{tp_name}` in struct `{name}` inferred as `{prev}` but field `{field_name}` has type `{val_ty}`"
                                            ),
                                            value.span.clone(),
                                        );
                            }
                        } else {
                            tp_inferred.insert(tp_name.clone(), val_ty.clone());
                        }
                    }
                } else if !val_ty.is_error()
                            && !expected_ty.is_error()
                            && &val_ty != *expected_ty
                            // An untyped numeric literal coerces into the
                            // field's sized numeric type, e.g. `U { age: 30 }`
                            // where `age: u32`.
                            && !literal_coerces_to(&value.node, expected_ty)
                {
                    self.error(ErrorCode::E0100,
                                format!(
                                    "field `{field_name}` of struct `{name}` expects `{}`, found `{val_ty}`",
                                    expected_ty
                                ),
                                value.span.clone(),
                            );
                }
                provided.insert(field_name.as_str());
            } else {
                self.error(
                    ErrorCode::E0315,
                    crate::no_such_field_message(name, field_name, &struct_info.fields),
                    value.span.clone(),
                );
            }
        }

        // Check for missing fields
        for (field_name, _) in &struct_info.fields {
            if !provided.contains(field_name.as_str()) {
                self.error(
                    ErrorCode::E0318,
                    format!("missing field `{field_name}` in struct `{name}`"),
                    expr.span.clone(),
                );
            }
        }

        Ty::Struct(name.clone())
    }

    fn check_field_access(&mut self, expr: &Spanned<Expr>) -> Ty {
        let Expr::FieldAccess { object, field } = &expr.node else {
            unreachable!()
        };
        // Check if this is actually an enum variant access: EnumName.VariantName
        if let Expr::Ident(ref name) = object.node {
            if let Some(info) = self.enums.get(name).cloned() {
                if !info.has_variant(field) {
                    self.error(
                        ErrorCode::E0316,
                        format!("enum `{name}` has no variant `{field}`"),
                        expr.span.clone(),
                    );
                } else if let Some(fields) = info.variant_fields(field) {
                    if !fields.is_empty() {
                        self.error(
                            ErrorCode::E0100,
                            format!(
                                "variant `{field}` of enum `{name}` requires {} argument(s)",
                                fields.len()
                            ),
                            expr.span.clone(),
                        );
                    }
                }
                return Ty::Enum(name.clone());
            }
        }

        let obj_ty = self.check_expr(object);
        match &obj_ty {
            Ty::Struct(struct_name) => {
                if let Some(struct_info) = self.structs.get(struct_name).cloned() {
                    if let Some((_, field_ty)) = struct_info.fields.iter().find(|(n, _)| n == field)
                    {
                        field_ty.clone()
                    } else {
                        self.error(
                            ErrorCode::E0315,
                            crate::no_such_field_message(struct_name, field, &struct_info.fields),
                            expr.span.clone(),
                        );
                        Ty::Error
                    }
                } else {
                    self.error(
                        ErrorCode::E0302,
                        format!("undefined struct `{struct_name}`"),
                        expr.span.clone(),
                    );
                    Ty::Error
                }
            }
            Ty::Error => Ty::Error,
            _ => {
                self.error(
                    ErrorCode::E0135,
                    format!("cannot access field `{field}` on type `{obj_ty}`"),
                    object.span.clone(),
                );
                Ty::Error
            }
        }
    }

    fn check_optional_chain(&mut self, expr: &Spanned<Expr>) -> Ty {
        let Expr::OptionalChain { object, field } = &expr.node else {
            unreachable!()
        };
        let obj_ty = self.check_expr(object);
        match &obj_ty {
            Ty::Optional(inner) => match inner.as_ref() {
                Ty::Struct(struct_name) => {
                    if let Some(struct_info) = self.structs.get(struct_name).cloned() {
                        if let Some((_, field_ty)) =
                            struct_info.fields.iter().find(|(n, _)| n == field)
                        {
                            Ty::Optional(Box::new(field_ty.clone()))
                        } else {
                            self.error(
                                ErrorCode::E0315,
                                crate::no_such_field_message(
                                    struct_name,
                                    field,
                                    &struct_info.fields,
                                ),
                                expr.span.clone(),
                            );
                            Ty::Error
                        }
                    } else {
                        self.error(
                            ErrorCode::E0302,
                            format!("undefined struct `{struct_name}`"),
                            expr.span.clone(),
                        );
                        Ty::Error
                    }
                }
                Ty::Error => Ty::Error,
                other => {
                    self.error(
                                ErrorCode::E0135,
                                format!(
                                    "optional chaining `?.` requires an optional struct type, found `{other}?`"
                                ),
                                expr.span.clone(),
                            );
                    Ty::Error
                }
            },
            Ty::Error => Ty::Error,
            _ => {
                self.error(
                    ErrorCode::E0135,
                    format!("optional chaining `?.` requires an optional type, found `{obj_ty}`"),
                    object.span.clone(),
                );
                Ty::Error
            }
        }
    }

    fn check_enum_variant(&mut self, expr: &Spanned<Expr>) -> Ty {
        let Expr::EnumVariant { enum_name, variant } = &expr.node else {
            unreachable!()
        };
        if let Some(info) = self.enums.get(enum_name) {
            if !info.has_variant(variant) {
                self.error(
                    ErrorCode::E0316,
                    format!("enum `{enum_name}` has no variant `{variant}`"),
                    expr.span.clone(),
                );
            }
            Ty::Enum(enum_name.clone())
        } else {
            self.error(
                ErrorCode::E0303,
                format!("undefined enum `{enum_name}`"),
                expr.span.clone(),
            );
            Ty::Error
        }
    }

    fn check_match(&mut self, expr: &Spanned<Expr>) -> Ty {
        let Expr::Match { subject, arms } = &expr.node else {
            unreachable!()
        };
        let subject_ty = self.check_expr(subject);

        if arms.is_empty() {
            self.error(
                ErrorCode::E0201,
                "match expression has no arms".to_string(),
                expr.span.clone(),
            );
            return Ty::Error;
        }

        // Check each arm's pattern and body
        let mut result_ty: Option<Ty> = None;
        let mut has_wildcard = false;
        let mut covered_variants: Vec<String> = Vec::new();

        for arm in arms {
            // Validate pattern against subject type
            self.check_pattern(&arm.pattern, &subject_ty);

            // Track coverage for exhaustiveness
            match &arm.pattern.node {
                Pattern::Wildcard => {
                    has_wildcard = true;
                }
                Pattern::Ident(name) => {
                    // For enums, ident patterns are variant names
                    if let Ty::Enum(_) = &subject_ty {
                        covered_variants.push(name.clone());
                    } else {
                        // For non-enum types, an ident pattern is a
                        // variable binding which acts as a wildcard
                        has_wildcard = true;
                    }
                }
                Pattern::BoolLit(b) => {
                    covered_variants.push(b.to_string());
                }
                Pattern::Ok(_) => {
                    covered_variants.push("ok".to_string());
                }
                Pattern::Err(_) => {
                    covered_variants.push("err".to_string());
                }
                Pattern::Some(_) => {
                    covered_variants.push("some".to_string());
                }
                Pattern::None => {
                    covered_variants.push("none".to_string());
                }
                Pattern::VariantDestructure { variant, .. } => {
                    covered_variants.push(variant.clone());
                }
                _ => {} // IntLit and StringLit don't cover the full domain
            }

            // For patterns with bindings, push a scope so both guards and bodies
            // can reference destructured variables.
            let body_ty = match &arm.pattern.node {
                Pattern::Ok(binding) => {
                    self.push_scope();
                    let ok_ty = if let Ty::Result(ref ok, _) = subject_ty {
                        *ok.clone()
                    } else {
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
                        &arm.pattern.span,
                    );
                    if let Some(ref guard) = arm.guard {
                        let guard_ty = self.check_expr(guard);
                        if guard_ty != Ty::Bool && !guard_ty.is_error() {
                            self.error(
                                ErrorCode::E0202,
                                format!("match guard must be bool, got `{guard_ty}`"),
                                guard.span.clone(),
                            );
                        }
                    }
                    let ty = self.check_expr(&arm.body);
                    self.pop_scope();
                    ty
                }
                Pattern::Err(binding) => {
                    self.push_scope();
                    let err_ty = if let Ty::Result(_, ref err) = subject_ty {
                        *err.clone()
                    } else {
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
                        &arm.pattern.span,
                    );
                    if let Some(ref guard) = arm.guard {
                        let guard_ty = self.check_expr(guard);
                        if guard_ty != Ty::Bool && !guard_ty.is_error() {
                            self.error(
                                ErrorCode::E0202,
                                format!("match guard must be bool, got `{guard_ty}`"),
                                guard.span.clone(),
                            );
                        }
                    }
                    let ty = self.check_expr(&arm.body);
                    self.pop_scope();
                    ty
                }
                Pattern::Some(binding) => {
                    self.push_scope();
                    let inner_ty = if let Ty::Optional(ref inner) = subject_ty {
                        *inner.clone()
                    } else {
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
                        &arm.pattern.span,
                    );
                    if let Some(ref guard) = arm.guard {
                        let guard_ty = self.check_expr(guard);
                        if guard_ty != Ty::Bool && !guard_ty.is_error() {
                            self.error(
                                ErrorCode::E0202,
                                format!("match guard must be bool, got `{guard_ty}`"),
                                guard.span.clone(),
                            );
                        }
                    }
                    let ty = self.check_expr(&arm.body);
                    self.pop_scope();
                    ty
                }
                Pattern::None => {
                    if let Some(ref guard) = arm.guard {
                        let guard_ty = self.check_expr(guard);
                        if guard_ty != Ty::Bool && !guard_ty.is_error() {
                            self.error(
                                ErrorCode::E0202,
                                format!("match guard must be bool, got `{guard_ty}`"),
                                guard.span.clone(),
                            );
                        }
                    }
                    self.check_expr(&arm.body)
                }
                Pattern::VariantDestructure { variant, bindings } => {
                    self.push_scope();
                    if let Ty::Enum(ref enum_name) = subject_ty {
                        if let Some(info) = self.enums.get(enum_name).cloned() {
                            if let Some(field_tys) = info.variant_fields(variant) {
                                for (i, binding) in bindings.iter().enumerate() {
                                    let ty = if i < field_tys.len() {
                                        field_tys[i].clone()
                                    } else {
                                        Ty::Error
                                    };
                                    self.define_var(
                                        binding,
                                        VarInfo {
                                            ty,
                                            mutable: false,
                                            span: 0..0,
                                            from_let: false,
                                        },
                                        &arm.pattern.span,
                                    );
                                }
                            }
                        }
                    }
                    if let Some(ref guard) = arm.guard {
                        let guard_ty = self.check_expr(guard);
                        if guard_ty != Ty::Bool && !guard_ty.is_error() {
                            self.error(
                                ErrorCode::E0202,
                                format!("match guard must be bool, got `{guard_ty}`"),
                                guard.span.clone(),
                            );
                        }
                    }
                    let ty = self.check_expr(&arm.body);
                    self.pop_scope();
                    ty
                }
                Pattern::Ident(name) if !matches!(subject_ty, Ty::Enum(_)) => {
                    // Non-enum ident pattern acts as a variable binding
                    self.push_scope();
                    self.define_var(
                        name,
                        VarInfo {
                            ty: subject_ty.clone(),
                            mutable: false,
                            span: 0..0,
                            from_let: false,
                        },
                        &arm.pattern.span,
                    );
                    if let Some(ref guard) = arm.guard {
                        let guard_ty = self.check_expr(guard);
                        if guard_ty != Ty::Bool && !guard_ty.is_error() {
                            self.error(
                                ErrorCode::E0202,
                                format!("match guard must be bool, got `{guard_ty}`"),
                                guard.span.clone(),
                            );
                        }
                    }
                    let ty = self.check_expr(&arm.body);
                    self.pop_scope();
                    ty
                }
                _ => {
                    // Enum ident patterns, wildcard, literal patterns
                    if let Some(ref guard) = arm.guard {
                        let guard_ty = self.check_expr(guard);
                        if guard_ty != Ty::Bool && !guard_ty.is_error() {
                            self.error(
                                ErrorCode::E0202,
                                format!("match guard must be bool, got `{guard_ty}`"),
                                guard.span.clone(),
                            );
                        }
                    }
                    self.check_expr(&arm.body)
                }
            };

            if let Some(ref expected) = result_ty {
                if !body_ty.is_error() && !expected.is_error() && body_ty != *expected {
                    self.error(
                        ErrorCode::E0108,
                        format!("match arms have different types: `{expected}` and `{body_ty}`"),
                        arm.body.span.clone(),
                    );
                }
            } else {
                result_ty = Some(body_ty);
            }
        }

        // Exhaustiveness check
        // TODO(P3): hoist this inline missing-variants logic into
        // `exhaustiveness::Checker::check_match_exhaustiveness` so all
        // pattern/exhaustiveness analysis lives in one module. Blocked
        // on plumbing partial `Checker` borrows for the bool / Result
        // / Optional branches below.
        if !has_wildcard && !subject_ty.is_error() {
            match &subject_ty {
                Ty::Enum(enum_name) => {
                    if let Some(info) = self.enums.get(enum_name).cloned() {
                        let variant_names = info.variant_names();
                        let missing: Vec<&String> = variant_names
                            .iter()
                            .filter(|v| !covered_variants.contains(v))
                            .collect();
                        if !missing.is_empty() {
                            let missing_str: Vec<&str> =
                                missing.iter().map(|s| s.as_str()).collect();
                            self.error(
                                ErrorCode::E0200,
                                format!(
                                    "match is not exhaustive; missing variants: {}",
                                    missing_str.join(", ")
                                ),
                                expr.span.clone(),
                            );
                        }
                    }
                }
                Ty::Bool => {
                    let has_true = covered_variants.contains(&"true".to_string());
                    let has_false = covered_variants.contains(&"false".to_string());
                    if !has_true || !has_false {
                        let mut missing = Vec::new();
                        if !has_true {
                            missing.push("true");
                        }
                        if !has_false {
                            missing.push("false");
                        }
                        self.error(
                            ErrorCode::E0200,
                            format!(
                                "match is not exhaustive; missing variants: {}",
                                missing.join(", ")
                            ),
                            expr.span.clone(),
                        );
                    }
                }
                Ty::Result(_, _) => {
                    let has_ok = covered_variants.contains(&"ok".to_string());
                    let has_err = covered_variants.contains(&"err".to_string());
                    if !has_ok || !has_err {
                        let mut missing = Vec::new();
                        if !has_ok {
                            missing.push("ok");
                        }
                        if !has_err {
                            missing.push("err");
                        }
                        self.error(
                            ErrorCode::E0200,
                            format!(
                                "match is not exhaustive; missing variants: {}",
                                missing.join(", ")
                            ),
                            expr.span.clone(),
                        );
                    }
                }
                Ty::Optional(_) => {
                    let has_some = covered_variants.contains(&"some".to_string());
                    let has_none = covered_variants.contains(&"none".to_string());
                    if !has_some || !has_none {
                        let mut missing = Vec::new();
                        if !has_some {
                            missing.push("some");
                        }
                        if !has_none {
                            missing.push("none");
                        }
                        self.error(
                            ErrorCode::E0200,
                            format!(
                                "match is not exhaustive; missing variants: {}",
                                missing.join(", ")
                            ),
                            expr.span.clone(),
                        );
                    }
                }
                _ => {
                    // For integers, strings, etc., a wildcard is required
                    self.error(
                        ErrorCode::E0200,
                        "match is not exhaustive; consider adding a wildcard `_` arm".to_string(),
                        expr.span.clone(),
                    );
                }
            }
        }

        result_ty.unwrap_or(Ty::Unit)
    }

    fn check_interpolation(&mut self, expr: &Spanned<Expr>) -> Ty {
        let Expr::Interpolation(parts) = &expr.node else {
            unreachable!()
        };
        for part in parts {
            if let InterpolPart::Expr(expr) = part {
                let part_ty = self.check_expr(expr);
                if part_ty == Ty::Unit {
                    self.error(
                        ErrorCode::E0100,
                        "cannot interpolate unit value `()`: expression produces no value"
                            .to_string(),
                        expr.span.clone(),
                    );
                }
            }
        }
        Ty::Str
    }

    fn check_closure(&mut self, expr: &Spanned<Expr>) -> Ty {
        let Expr::Closure {
            params,
            return_type,
            body,
        } = &expr.node
        else {
            unreachable!()
        };
        let mut param_types = Vec::new();
        let param_hint = self.closure_param_hint.take();
        self.push_scope();
        for (i, param) in params.iter().enumerate() {
            let ty = if matches!(param.ty.node, TypeExpr::Inferred) {
                if let Some(ref hints) = param_hint {
                    if i < hints.len() {
                        hints[i].clone()
                    } else {
                        self.error(
                            ErrorCode::E0126,
                            format!("cannot infer type of closure parameter `{}`", param.name),
                            param.ty.span.clone(),
                        );
                        Ty::Error
                    }
                } else {
                    self.error(
                        ErrorCode::E0126,
                        format!(
                            "cannot infer type of closure parameter `{}` -- add a type annotation",
                            param.name
                        ),
                        param.ty.span.clone(),
                    );
                    Ty::Error
                }
            } else {
                match resolve_type_expr(&param.ty.node, Some(&self.structs), Some(&self.enums)) {
                    Some(ty) => ty,
                    None => {
                        self.error(
                            ErrorCode::E0305,
                            format!("unknown type in closure parameter `{}`", param.name),
                            param.ty.span.clone(),
                        );
                        Ty::Error
                    }
                }
            };
            self.define_var(
                &param.name,
                VarInfo {
                    ty: ty.clone(),
                    mutable: false,
                    span: 0..0,
                    from_let: false,
                },
                &param.span,
            );
            param_types.push(ty);
        }
        let body_ty = self.check_expr(body);
        self.pop_scope();

        let ret_ty = if let Some(rt) = return_type {
            match resolve_type_expr(&rt.node, Some(&self.structs), Some(&self.enums)) {
                Some(ty) => {
                    if !body_ty.is_error() && !ty.is_error() && body_ty != ty {
                        self.error(
                            ErrorCode::E0125,
                            format!("closure body returns `{body_ty}` but return type is `{ty}`"),
                            body.span.clone(),
                        );
                    }
                    ty
                }
                None => {
                    self.error(
                        ErrorCode::E0127,
                        "unknown closure return type".to_string(),
                        rt.span.clone(),
                    );
                    body_ty
                }
            }
        } else {
            body_ty
        };

        Ty::Fn(param_types, Box::new(ret_ty))
    }

    fn check_ok_expr(&mut self, expr: &Spanned<Expr>) -> Ty {
        let Expr::OkExpr(value) = &expr.node else {
            unreachable!()
        };
        let val_ty = self.check_expr(value);
        // Return a partial result type -- the error type is unknown without context
        Ty::Result(Box::new(val_ty), Box::new(Ty::Error))
    }

    fn check_err_expr(&mut self, expr: &Spanned<Expr>) -> Ty {
        let Expr::ErrExpr(value) = &expr.node else {
            unreachable!()
        };
        let val_ty = self.check_expr(value);
        // Return a partial result type -- the ok type is unknown without context
        Ty::Result(Box::new(Ty::Error), Box::new(val_ty))
    }

    fn check_some_expr(&mut self, expr: &Spanned<Expr>) -> Ty {
        let Expr::SomeExpr(value) = &expr.node else {
            unreachable!()
        };
        let val_ty = self.check_expr(value);
        Ty::Optional(Box::new(val_ty))
    }

    fn check_break(&mut self, expr: &Spanned<Expr>) -> Ty {
        if self.loop_depth == 0 {
            self.error(
                ErrorCode::E0507,
                "`break` can only be used inside a loop".to_string(),
                expr.span.clone(),
            );
        }
        Ty::Unit
    }

    fn check_continue(&mut self, expr: &Spanned<Expr>) -> Ty {
        if self.loop_depth == 0 {
            self.error(
                ErrorCode::E0508,
                "`continue` can only be used inside a loop".to_string(),
                expr.span.clone(),
            );
        }
        Ty::Unit
    }

    fn check_map_lit(&mut self, expr: &Spanned<Expr>) -> Ty {
        let Expr::MapLit(entries) = &expr.node else {
            unreachable!()
        };
        for (key, value) in entries {
            let key_ty = self.check_expr(key);
            let val_ty = self.check_expr(value);
            if !key_ty.is_error() && key_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("map literal keys must be strings, found `{key_ty}`"),
                    key.span.clone(),
                );
            }
            if !val_ty.is_error() && val_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("map literal values must be strings, found `{val_ty}`"),
                    value.span.clone(),
                );
            }
        }
        Ty::Handle(HandleKind::HashMap) // a map literal is a hashmap handle
    }

    fn check_null_coalesce(&mut self, expr: &Spanned<Expr>) -> Ty {
        let Expr::NullCoalesce { value, default } = &expr.node else {
            unreachable!()
        };
        let val_ty = self.check_expr(value);
        let def_ty = self.check_expr(default);

        if val_ty.is_error() || def_ty.is_error() {
            return if def_ty.is_error() { Ty::Error } else { def_ty };
        }

        match &val_ty {
            Ty::Optional(inner) => {
                if !inner.is_error() && !def_ty.is_error() && **inner != def_ty {
                    self.error(ErrorCode::E0118,
                                format!(
                                    "`??` operator: optional inner type `{}` doesn't match default type `{def_ty}`",
                                    inner
                                ),
                                default.span.clone(),
                            );
                }
                if inner.is_error() {
                    def_ty
                } else {
                    *inner.clone()
                }
            }
            _ => {
                self.error(
                    ErrorCode::E0119,
                    format!(
                        "`??` operator requires an optional type on the left, found `{val_ty}`"
                    ),
                    value.span.clone(),
                );
                def_ty
            }
        }
    }

    fn check_builtin_call(
        &mut self,
        name: &str,
        args: &[Spanned<Expr>],
        callee: &Spanned<Expr>,
    ) -> Option<Ty> {
        if let Some(t) = self.check_builtin_core(name, args, callee) {
            return Some(t);
        }
        if let Some(t) = self.check_builtin_string(name, args, callee) {
            return Some(t);
        }
        if let Some(t) = self.check_builtin_io_env(name, args, callee) {
            return Some(t);
        }
        if let Some(t) = self.check_builtin_math(name, args, callee) {
            return Some(t);
        }
        if let Some(t) = self.check_builtin_convert(name, args, callee) {
            return Some(t);
        }
        if let Some(t) = self.check_builtin_fs_path(name, args, callee) {
            return Some(t);
        }
        if let Some(t) = self.check_builtin_array(name, args, callee) {
            return Some(t);
        }
        if let Some(t) = self.check_builtin_time(name, args, callee) {
            return Some(t);
        }
        if let Some(t) = self.check_builtin_http(name, args, callee) {
            return Some(t);
        }
        if let Some(t) = self.check_builtin_json(name, args, callee) {
            return Some(t);
        }
        if let Some(t) = self.check_builtin_concurrency(name, args, callee) {
            return Some(t);
        }
        if let Some(t) = self.check_builtin_hashmap(name, args, callee) {
            return Some(t);
        }
        if let Some(t) = self.check_builtin_refs(name, args, callee) {
            return Some(t);
        }
        None
    }

    fn check_builtin_core(
        &mut self,
        name: &str,
        args: &[Spanned<Expr>],
        callee: &Spanned<Expr>,
    ) -> Option<Ty> {
        if name == "print" {
            if args.len() > 1 {
                self.error(
                    ErrorCode::E0513,
                    format!("print() takes at most 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
            }
            for arg in args {
                self.check_expr(arg);
            }
            return Some(Ty::Unit);
        }
        if name == "panic" {
            if args.len() > 1 {
                self.error(
                    ErrorCode::E0513,
                    format!("panic() takes at most 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
            }
            for arg in args {
                self.check_expr(arg);
            }
            return Some(Ty::Unit);
        }
        if name == "assert" {
            if args.is_empty() {
                self.error(
                    ErrorCode::E0513,
                    "assert() requires at least one argument".to_string(),
                    callee.span.clone(),
                );
            } else if args.len() > 2 {
                self.error(
                    ErrorCode::E0513,
                    format!("assert() takes at most 2 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
            }
            if !args.is_empty() {
                let cond_ty = self.check_expr(&args[0]);
                if !cond_ty.is_error() && cond_ty != Ty::Bool {
                    self.error(
                        ErrorCode::E0133,
                        format!("assert() condition must be `bool`, found `{cond_ty}`"),
                        args[0].span.clone(),
                    );
                }
                // Optional message argument
                if args.len() > 1 {
                    self.check_expr(&args[1]);
                }
                // Type-check remaining args even if too many
                for arg in args.iter().skip(2) {
                    self.check_expr(arg);
                }
            }
            return Some(Ty::Unit);
        }
        if name == "assert_eq" || name == "assert_ne" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0100,
                    format!("{name}() takes exactly 2 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
            }
            if args.len() >= 2 {
                let left_ty = self.check_expr(&args[0]);
                let right_ty = self.check_expr(&args[1]);
                if !left_ty.is_error()
                    && !right_ty.is_error()
                    && !types_compatible(&left_ty, &right_ty)
                    && left_ty != right_ty
                {
                    self.error(ErrorCode::E0100,
                                    format!("{name}() arguments must be the same type: left is `{left_ty}`, right is `{right_ty}`"),
                                    callee.span.clone(),
                                );
                }
            }
            for arg in args {
                self.check_expr(arg);
            }
            return Some(Ty::Unit);
        }
        if name == "len" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0513,
                    format!("len() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let arg_ty = self.check_expr(&args[0]);
            match &arg_ty {
                Ty::Array(_) => return Some(Ty::I64),
                Ty::Str => return Some(Ty::I64),
                _ if arg_ty.is_error() => return Some(Ty::Error),
                _ => {
                    self.error(
                        ErrorCode::E0133,
                        format!("len() expects array or string, found `{arg_ty}`"),
                        args[0].span.clone(),
                    );
                    return Some(Ty::Error);
                }
            }
        }

        if name == "push" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0513,
                    format!("push() takes exactly 2 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let arr_ty = self.check_expr(&args[0]);
            let elem_ty = self.check_expr(&args[1]);
            match &arr_ty {
                Ty::Array(inner) => {
                    if !elem_ty.is_error() && !inner.is_error() && **inner != elem_ty {
                        self.error(
                                        ErrorCode::E0133,
                                        format!(
                                            "push() element type `{elem_ty}` does not match array element type `{inner}`"
                                        ),
                                        args[1].span.clone(),
                                    );
                    }
                    return Some(arr_ty);
                }
                _ if arr_ty.is_error() => return Some(Ty::Error),
                _ => {
                    self.error(
                        ErrorCode::E0133,
                        format!("push() expects array as first argument, found `{arr_ty}`"),
                        args[0].span.clone(),
                    );
                    return Some(Ty::Error);
                }
            }
        }

        if name == "abs" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0513,
                    format!("abs() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let arg_ty = self.check_expr(&args[0]);
            if !arg_ty.is_error() && !arg_ty.is_numeric() {
                self.error(
                    ErrorCode::E0133,
                    format!("abs() expects numeric type, found `{arg_ty}`"),
                    args[0].span.clone(),
                );
            }
            if arg_ty.is_float() {
                return Some(Ty::F64);
            }
            return Some(Ty::I64);
        }
        if name == "min" || name == "max" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0100,
                    format!("{}() takes exactly 2 arguments, got {}", name, args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let a_ty = self.check_expr(&args[0]);
            let b_ty = self.check_expr(&args[1]);
            if !a_ty.is_error() && !a_ty.is_numeric() {
                self.error(
                    ErrorCode::E0100,
                    format!("{}() expects numeric types, found `{a_ty}`", name),
                    args[0].span.clone(),
                );
            }
            if !b_ty.is_error() && !b_ty.is_numeric() {
                self.error(
                    ErrorCode::E0100,
                    format!("{}() expects numeric types, found `{b_ty}`", name),
                    args[1].span.clone(),
                );
            }
            if a_ty.is_float() || b_ty.is_float() {
                return Some(Ty::F64);
            }
            return Some(Ty::I64);
        }
        None
    }

    fn check_builtin_string(
        &mut self,
        name: &str,
        args: &[Spanned<Expr>],
        callee: &Spanned<Expr>,
    ) -> Option<Ty> {
        if name == "to_str" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0513,
                    format!("to_str() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            self.check_expr(&args[0]);
            return Some(Ty::Str);
        }

        // ── Stdlib string functions ──────────────────────
        if name == "split" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0513,
                    format!("split() takes exactly 2 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let s_ty = self.check_expr(&args[0]);
            let sep_ty = self.check_expr(&args[1]);
            if !s_ty.is_error() && s_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("split() first argument must be str, found `{s_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !sep_ty.is_error() && sep_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("split() second argument must be str, found `{sep_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::Array(Box::new(Ty::Str)));
        }
        if name == "trim" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0513,
                    format!("trim() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let s_ty = self.check_expr(&args[0]);
            if !s_ty.is_error() && s_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("trim() expects str, found `{s_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Str);
        }
        if name == "upper" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0100,
                    format!("upper() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let s_ty = self.check_expr(&args[0]);
            if !s_ty.is_error() && s_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("upper() expects str, found `{s_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Str);
        }
        if name == "lower" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0100,
                    format!("lower() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let s_ty = self.check_expr(&args[0]);
            if !s_ty.is_error() && s_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("lower() expects str, found `{s_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Str);
        }
        if name == "starts_with" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0513,
                    format!(
                        "starts_with() takes exactly 2 arguments, got {}",
                        args.len()
                    ),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let s_ty = self.check_expr(&args[0]);
            let prefix_ty = self.check_expr(&args[1]);
            if !s_ty.is_error() && s_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("starts_with() first argument must be str, found `{s_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !prefix_ty.is_error() && prefix_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("starts_with() second argument must be str, found `{prefix_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::Bool);
        }
        if name == "ends_with" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0513,
                    format!("ends_with() takes exactly 2 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let s_ty = self.check_expr(&args[0]);
            let suffix_ty = self.check_expr(&args[1]);
            if !s_ty.is_error() && s_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("ends_with() first argument must be str, found `{s_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !suffix_ty.is_error() && suffix_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("ends_with() second argument must be str, found `{suffix_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::Bool);
        }
        if name == "replace" {
            if args.len() != 3 {
                self.error(
                    ErrorCode::E0513,
                    format!("replace() takes exactly 3 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let s_ty = self.check_expr(&args[0]);
            let from_ty = self.check_expr(&args[1]);
            let to_ty = self.check_expr(&args[2]);
            if !s_ty.is_error() && s_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("replace() first argument must be str, found `{s_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !from_ty.is_error() && from_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("replace() second argument must be str, found `{from_ty}`"),
                    args[1].span.clone(),
                );
            }
            if !to_ty.is_error() && to_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("replace() third argument must be str, found `{to_ty}`"),
                    args[2].span.clone(),
                );
            }
            return Some(Ty::Str);
        }
        if name == "char_at" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0513,
                    format!("char_at() takes exactly 2 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let s_ty = self.check_expr(&args[0]);
            let idx_ty = self.check_expr(&args[1]);
            if !s_ty.is_error() && s_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("char_at() first argument must be str, found `{s_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !idx_ty.is_error() && !idx_ty.is_integer() {
                self.error(
                    ErrorCode::E0133,
                    format!("char_at() second argument must be integer, found `{idx_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::Str);
        }

        if name == "contains" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0513,
                    format!("contains() takes exactly 2 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let s_ty = self.check_expr(&args[0]);
            let sub_ty = self.check_expr(&args[1]);
            if !s_ty.is_error() && s_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("contains() first argument must be str, found `{s_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !sub_ty.is_error() && sub_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("contains() second argument must be str, found `{sub_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::Bool);
        }
        if name == "index_of" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0100,
                    format!("index_of() takes exactly 2 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let s_ty = self.check_expr(&args[0]);
            let sub_ty = self.check_expr(&args[1]);
            if !s_ty.is_error() && s_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("index_of() first argument must be str, found `{s_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !sub_ty.is_error() && sub_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("index_of() second argument must be str, found `{sub_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::I64);
        }
        if name == "join" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0513,
                    format!("join() takes exactly 2 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let arr_ty = self.check_expr(&args[0]);
            let sep_ty = self.check_expr(&args[1]);
            if !arr_ty.is_error() && arr_ty != Ty::Array(Box::new(Ty::Str)) {
                self.error(
                    ErrorCode::E0133,
                    format!("join() first argument must be [str], found `{arr_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !sep_ty.is_error() && sep_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("join() second argument must be str, found `{sep_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::Str);
        }
        if name == "repeat" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0100,
                    format!("repeat() takes exactly 2 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let s_ty = self.check_expr(&args[0]);
            let n_ty = self.check_expr(&args[1]);
            if !s_ty.is_error() && s_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("repeat() first argument must be str, found `{s_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !n_ty.is_error() && !n_ty.is_integer() {
                self.error(
                    ErrorCode::E0100,
                    format!("repeat() second argument must be integer, found `{n_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::Str);
        }

        // ── Stdlib I/O functions ─────────────────────────
        None
    }

    fn check_builtin_io_env(
        &mut self,
        name: &str,
        args: &[Spanned<Expr>],
        callee: &Spanned<Expr>,
    ) -> Option<Ty> {
        if name == "read_line" {
            if !args.is_empty() {
                self.error(
                    ErrorCode::E0100,
                    format!("read_line() takes 0 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            return Some(Ty::Str);
        }
        if name == "read_file" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0513,
                    format!("read_file() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let path_ty = self.check_expr(&args[0]);
            if !path_ty.is_error() && path_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("read_file() expects str, found `{path_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Str);
        }
        if name == "write_file" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0513,
                    format!("write_file() takes exactly 2 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let path_ty = self.check_expr(&args[0]);
            let content_ty = self.check_expr(&args[1]);
            if !path_ty.is_error() && path_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("write_file() first argument must be str, found `{path_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !content_ty.is_error() && content_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("write_file() second argument must be str, found `{content_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::Unit);
        }
        if name == "try_read_file" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0513,
                    format!(
                        "try_read_file() takes exactly 1 argument, got {}",
                        args.len()
                    ),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let path_ty = self.check_expr(&args[0]);
            if !path_ty.is_error() && path_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("try_read_file() expects str, found `{path_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Result(Box::new(Ty::Str), Box::new(Ty::Str)));
        }
        if name == "try_write_file" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0513,
                    format!(
                        "try_write_file() takes exactly 2 arguments, got {}",
                        args.len()
                    ),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let path_ty = self.check_expr(&args[0]);
            let content_ty = self.check_expr(&args[1]);
            if !path_ty.is_error() && path_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("try_write_file() first argument must be str, found `{path_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !content_ty.is_error() && content_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("try_write_file() second argument must be str, found `{content_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::Result(Box::new(Ty::Bool), Box::new(Ty::Str)));
        }

        // ── shell_exec / exec / env_get ──────────────────────────────
        if name == "shell_exec" || name == "exec" {
            if !self.in_unsafe_context {
                self.error(
                    ErrorCode::E0100,
                    format!("`{name}()` can only be called inside an `@unsafe` function")
                        .to_string(),
                    callee.span.clone(),
                );
            }
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0513,
                    format!("{name}() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let cmd_ty = self.check_expr(&args[0]);
            if !cmd_ty.is_error() && cmd_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("{name}() expects str, found `{cmd_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Str);
        }
        if name == "env_get" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0513,
                    format!("env_get() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let name_ty = self.check_expr(&args[0]);
            if !name_ty.is_error() && name_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("env_get() expects str, found `{name_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Str);
        }

        // ── Stdlib math functions ────────────────────────
        None
    }

    fn check_builtin_math(
        &mut self,
        name: &str,
        args: &[Spanned<Expr>],
        callee: &Spanned<Expr>,
    ) -> Option<Ty> {
        if name == "pow" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0100,
                    format!("pow() takes exactly 2 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let base_ty = self.check_expr(&args[0]);
            let exp_ty = self.check_expr(&args[1]);
            if !base_ty.is_error() && !base_ty.is_integer() {
                self.error(
                    ErrorCode::E0100,
                    format!("pow() first argument must be integer, found `{base_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !exp_ty.is_error() && !exp_ty.is_integer() {
                self.error(
                    ErrorCode::E0100,
                    format!("pow() second argument must be integer, found `{exp_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::I64);
        }
        if name == "sqrt" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0100,
                    format!("sqrt() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let x_ty = self.check_expr(&args[0]);
            if !x_ty.is_error() && x_ty != Ty::F64 && x_ty != Ty::F32 {
                self.error(
                    ErrorCode::E0100,
                    format!("sqrt() expects float, found `{x_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::F64);
        }

        // ── Math builtins (Tier 1) ───────────────────
        // floor/ceil/round: (f64) -> i64
        if name == "floor" || name == "ceil" || name == "round" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0100,
                    format!("{name}() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let x_ty = self.check_expr(&args[0]);
            if !x_ty.is_error() && x_ty != Ty::F64 && x_ty != Ty::F32 {
                self.error(
                    ErrorCode::E0100,
                    format!("{name}() expects float, found `{x_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::I64);
        }
        // sin/cos/tan/log/log2/log10/exp: (f64) -> f64
        if name == "sin"
            || name == "cos"
            || name == "tan"
            || name == "log"
            || name == "log2"
            || name == "log10"
            || name == "exp"
        {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0100,
                    format!("{name}() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let x_ty = self.check_expr(&args[0]);
            if !x_ty.is_error() && x_ty != Ty::F64 && x_ty != Ty::F32 {
                self.error(
                    ErrorCode::E0100,
                    format!("{name}() expects float, found `{x_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::F64);
        }
        // random() -> f64
        if name == "random" {
            if !args.is_empty() {
                self.error(
                    ErrorCode::E0100,
                    format!("random() takes 0 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            return Some(Ty::F64);
        }
        // random_range(min, max) -> i64
        if name == "random_range" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0100,
                    format!(
                        "random_range() takes exactly 2 arguments, got {}",
                        args.len()
                    ),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let min_ty = self.check_expr(&args[0]);
            let max_ty = self.check_expr(&args[1]);
            if !min_ty.is_error() && !min_ty.is_integer() {
                self.error(
                    ErrorCode::E0100,
                    format!("random_range() first argument must be integer, found `{min_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !max_ty.is_error() && !max_ty.is_integer() {
                self.error(
                    ErrorCode::E0100,
                    format!("random_range() second argument must be integer, found `{max_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::I64);
        }

        // ── System builtins (Tier 1) ────────────────────
        // args() -> [str]
        None
    }

    fn check_builtin_convert(
        &mut self,
        name: &str,
        args: &[Spanned<Expr>],
        callee: &Spanned<Expr>,
    ) -> Option<Ty> {
        if name == "args" {
            if !args.is_empty() {
                self.error(
                    ErrorCode::E0100,
                    format!("args() takes 0 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            return Some(Ty::Array(Box::new(Ty::Str)));
        }
        // exit(code: i64) -> unit
        if name == "exit" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0100,
                    format!("exit() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let code_ty = self.check_expr(&args[0]);
            if !code_ty.is_error() && !code_ty.is_integer() {
                self.error(
                    ErrorCode::E0100,
                    format!("exit() expects integer, found `{code_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Unit);
        }
        // type_of(val) -> str
        if name == "type_of" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0100,
                    format!("type_of() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            // Accept any type
            self.check_expr(&args[0]);
            return Some(Ty::Str);
        }

        // ── String parsing builtins (Tier 1) ────────────
        // substring(s, start, end) -> str
        if name == "substring" {
            if args.len() != 3 {
                self.error(
                    ErrorCode::E0100,
                    format!("substring() takes exactly 3 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let s_ty = self.check_expr(&args[0]);
            let start_ty = self.check_expr(&args[1]);
            let end_ty = self.check_expr(&args[2]);
            if !s_ty.is_error() && s_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("substring() first argument must be str, found `{s_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !start_ty.is_error() && !start_ty.is_integer() {
                self.error(
                    ErrorCode::E0100,
                    format!("substring() second argument must be integer, found `{start_ty}`"),
                    args[1].span.clone(),
                );
            }
            if !end_ty.is_error() && !end_ty.is_integer() {
                self.error(
                    ErrorCode::E0100,
                    format!("substring() third argument must be integer, found `{end_ty}`"),
                    args[2].span.clone(),
                );
            }
            return Some(Ty::Str);
        }
        // pad_left(s, width, char) -> str
        if name == "pad_left" {
            if args.len() != 3 {
                self.error(
                    ErrorCode::E0100,
                    format!("pad_left() takes exactly 3 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let s_ty = self.check_expr(&args[0]);
            let width_ty = self.check_expr(&args[1]);
            let char_ty = self.check_expr(&args[2]);
            if !s_ty.is_error() && s_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("pad_left() first argument must be str, found `{s_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !width_ty.is_error() && !width_ty.is_integer() {
                self.error(
                    ErrorCode::E0100,
                    format!("pad_left() second argument must be integer, found `{width_ty}`"),
                    args[1].span.clone(),
                );
            }
            if !char_ty.is_error() && char_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("pad_left() third argument must be str, found `{char_ty}`"),
                    args[2].span.clone(),
                );
            }
            return Some(Ty::Str);
        }
        // pad_right(s, width, char) -> str
        if name == "pad_right" {
            if args.len() != 3 {
                self.error(
                    ErrorCode::E0100,
                    format!("pad_right() takes exactly 3 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let s_ty = self.check_expr(&args[0]);
            let width_ty = self.check_expr(&args[1]);
            let char_ty = self.check_expr(&args[2]);
            if !s_ty.is_error() && s_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("pad_right() first argument must be str, found `{s_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !width_ty.is_error() && !width_ty.is_integer() {
                self.error(
                    ErrorCode::E0100,
                    format!("pad_right() second argument must be integer, found `{width_ty}`"),
                    args[1].span.clone(),
                );
            }
            if !char_ty.is_error() && char_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("pad_right() third argument must be str, found `{char_ty}`"),
                    args[2].span.clone(),
                );
            }
            return Some(Ty::Str);
        }
        // str_to_int(s) -> i64 ! str
        if name == "str_to_int" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0100,
                    format!("str_to_int() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let s_ty = self.check_expr(&args[0]);
            if !s_ty.is_error() && s_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("str_to_int() expects str, found `{s_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Result(Box::new(Ty::I64), Box::new(Ty::Str)));
        }
        // str_to_float(s) -> f64 ! str
        if name == "str_to_float" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0100,
                    format!(
                        "str_to_float() takes exactly 1 argument, got {}",
                        args.len()
                    ),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let s_ty = self.check_expr(&args[0]);
            if !s_ty.is_error() && s_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("str_to_float() expects str, found `{s_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Result(Box::new(Ty::F64), Box::new(Ty::Str)));
        }

        // float_to_int(f: float) -> int
        if name == "float_to_int" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0513,
                    format!(
                        "float_to_int() takes exactly 1 argument, got {}",
                        args.len()
                    ),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let arg_ty = self.check_expr(&args[0]);
            if !arg_ty.is_error() && arg_ty != Ty::F64 {
                self.error(
                    ErrorCode::E0133,
                    format!("float_to_int() argument must be float, found `{arg_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::I64);
        }

        // int_to_float(i: int) -> float
        if name == "int_to_float" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0513,
                    format!(
                        "int_to_float() takes exactly 1 argument, got {}",
                        args.len()
                    ),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let arg_ty = self.check_expr(&args[0]);
            if !arg_ty.is_error() && arg_ty != Ty::I64 {
                self.error(
                    ErrorCode::E0133,
                    format!("int_to_float() argument must be int, found `{arg_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::F64);
        }

        // str_from_char(code: int) -> str
        if name == "str_from_char" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0513,
                    format!(
                        "str_from_char() takes exactly 1 argument, got {}",
                        args.len()
                    ),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let arg_ty = self.check_expr(&args[0]);
            if !arg_ty.is_error() && !arg_ty.is_integer() {
                self.error(
                    ErrorCode::E0133,
                    format!("str_from_char() argument must be int, found `{arg_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Str);
        }

        // ── Filesystem builtins ─────────────────────────
        // file_exists(path: str) -> bool
        None
    }

    fn check_builtin_fs_path(
        &mut self,
        name: &str,
        args: &[Spanned<Expr>],
        callee: &Spanned<Expr>,
    ) -> Option<Ty> {
        if name == "file_exists" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0100,
                    format!("file_exists() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let path_ty = self.check_expr(&args[0]);
            if !path_ty.is_error() && path_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("file_exists() expects str, found `{path_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Bool);
        }
        // delete_file(path: str) -> bool
        if name == "delete_file" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0100,
                    format!("delete_file() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let path_ty = self.check_expr(&args[0]);
            if !path_ty.is_error() && path_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("delete_file() expects str, found `{path_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Bool);
        }
        // list_dir(path: str) -> [str]
        if name == "list_dir" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0100,
                    format!("list_dir() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let path_ty = self.check_expr(&args[0]);
            if !path_ty.is_error() && path_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("list_dir() expects str, found `{path_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Array(Box::new(Ty::Str)));
        }
        // mkdir(path: str) -> bool
        if name == "mkdir" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0100,
                    format!("mkdir() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let path_ty = self.check_expr(&args[0]);
            if !path_ty.is_error() && path_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("mkdir() expects str, found `{path_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Bool);
        }
        // path_join(a: str, b: str) -> str
        if name == "path_join" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0100,
                    format!("path_join() takes exactly 2 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let a_ty = self.check_expr(&args[0]);
            let b_ty = self.check_expr(&args[1]);
            if !a_ty.is_error() && a_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("path_join() first argument must be str, found `{a_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !b_ty.is_error() && b_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("path_join() second argument must be str, found `{b_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::Str);
        }
        // path_dir/path_base/path_ext: (str) -> str
        if name == "path_dir" || name == "path_base" || name == "path_ext" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0100,
                    format!("{name}() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let path_ty = self.check_expr(&args[0]);
            if !path_ty.is_error() && path_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("{name}() expects str, found `{path_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Str);
        }

        // ── Collection builtins ────────────────────────
        // sort(arr) -> [T]
        None
    }

    fn check_builtin_array(
        &mut self,
        name: &str,
        args: &[Spanned<Expr>],
        callee: &Spanned<Expr>,
    ) -> Option<Ty> {
        if name == "sort" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0100,
                    format!("sort() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let arr_ty = self.check_expr(&args[0]);
            match &arr_ty {
                Ty::Array(_) => return Some(arr_ty),
                _ if arr_ty.is_error() => return Some(Ty::Error),
                _ => {
                    self.error(
                        ErrorCode::E0133,
                        format!("sort() expects an array, found `{arr_ty}`"),
                        args[0].span.clone(),
                    );
                    return Some(Ty::Error);
                }
            }
        }
        // reverse(arr) -> [T]
        if name == "reverse" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0100,
                    format!("reverse() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let arr_ty = self.check_expr(&args[0]);
            match &arr_ty {
                Ty::Array(_) => return Some(arr_ty),
                _ if arr_ty.is_error() => return Some(Ty::Error),
                _ => {
                    self.error(
                        ErrorCode::E0133,
                        format!("reverse() expects an array, found `{arr_ty}`"),
                        args[0].span.clone(),
                    );
                    return Some(Ty::Error);
                }
            }
        }
        // array_contains(arr, val) -> bool
        if name == "array_contains" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0100,
                    format!(
                        "array_contains() takes exactly 2 arguments, got {}",
                        args.len()
                    ),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let arr_ty = self.check_expr(&args[0]);
            let val_ty = self.check_expr(&args[1]);
            let elem_ty = match &arr_ty {
                Ty::Array(inner) => *inner.clone(),
                _ if arr_ty.is_error() => Ty::Error,
                _ => {
                    self.error(
                        ErrorCode::E0133,
                        format!(
                            "array_contains() first argument must be an array, found `{arr_ty}`"
                        ),
                        args[0].span.clone(),
                    );
                    return Some(Ty::Error);
                }
            };
            if !elem_ty.is_error() && !val_ty.is_error() && elem_ty != val_ty {
                self.error(
                                ErrorCode::E0100,
                                format!("array_contains() value type `{val_ty}` doesn't match array element type `{elem_ty}`"),
                                args[1].span.clone(),
                            );
            }
            return Some(Ty::Bool);
        }
        // slice(arr, start, end) -> [T]
        if name == "slice" {
            if args.len() != 3 {
                self.error(
                    ErrorCode::E0100,
                    format!("slice() takes exactly 3 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let arr_ty = self.check_expr(&args[0]);
            let start_ty = self.check_expr(&args[1]);
            let end_ty = self.check_expr(&args[2]);
            if !start_ty.is_error() && !start_ty.is_integer() {
                self.error(
                    ErrorCode::E0100,
                    format!("slice() start must be integer, found `{start_ty}`"),
                    args[1].span.clone(),
                );
            }
            if !end_ty.is_error() && !end_ty.is_integer() {
                self.error(
                    ErrorCode::E0100,
                    format!("slice() end must be integer, found `{end_ty}`"),
                    args[2].span.clone(),
                );
            }
            match &arr_ty {
                Ty::Array(_) => return Some(arr_ty),
                _ if arr_ty.is_error() => return Some(Ty::Error),
                _ => {
                    self.error(
                        ErrorCode::E0133,
                        format!("slice() first argument must be an array, found `{arr_ty}`"),
                        args[0].span.clone(),
                    );
                    return Some(Ty::Error);
                }
            }
        }
        // any(arr, fn) -> bool
        if name == "any" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0513,
                    format!("any() takes exactly 2 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let arr_ty = self.check_expr(&args[0]);
            if let Ty::Array(ref inner) = arr_ty {
                self.closure_param_hint = Some(vec![*inner.clone()]);
            }
            let fn_ty = self.check_expr(&args[1]);
            let elem_ty = match &arr_ty {
                Ty::Array(inner) => *inner.clone(),
                _ if arr_ty.is_error() => Ty::Error,
                _ => {
                    self.error(
                        ErrorCode::E0133,
                        format!("any() first argument must be an array, found `{arr_ty}`"),
                        args[0].span.clone(),
                    );
                    return Some(Ty::Error);
                }
            };
            match &fn_ty {
                Ty::Fn(params, ret) => {
                    if params.len() != 1 {
                        self.error(
                            ErrorCode::E0133,
                            format!(
                                "any() callback must take 1 parameter, takes {}",
                                params.len()
                            ),
                            args[1].span.clone(),
                        );
                    } else if !elem_ty.is_error() && !params[0].is_error() && elem_ty != params[0] {
                        self.error(
                                        ErrorCode::E0100,
                                        format!("any() callback parameter type `{}` doesn't match array element type `{}`", params[0], elem_ty),
                                        args[1].span.clone(),
                                    );
                    }
                    if **ret != Ty::Bool && !ret.is_error() {
                        self.error(
                            ErrorCode::E0133,
                            format!("any() callback must return `bool`, returns `{}`", ret),
                            args[1].span.clone(),
                        );
                    }
                    return Some(Ty::Bool);
                }
                _ if fn_ty.is_error() => return Some(Ty::Error),
                _ => {
                    self.error(
                        ErrorCode::E0133,
                        format!("any() second argument must be a function, found `{fn_ty}`"),
                        args[1].span.clone(),
                    );
                    return Some(Ty::Error);
                }
            }
        }
        // all(arr, fn) -> bool
        if name == "all" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0513,
                    format!("all() takes exactly 2 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let arr_ty = self.check_expr(&args[0]);
            if let Ty::Array(ref inner) = arr_ty {
                self.closure_param_hint = Some(vec![*inner.clone()]);
            }
            let fn_ty = self.check_expr(&args[1]);
            let elem_ty = match &arr_ty {
                Ty::Array(inner) => *inner.clone(),
                _ if arr_ty.is_error() => Ty::Error,
                _ => {
                    self.error(
                        ErrorCode::E0133,
                        format!("all() first argument must be an array, found `{arr_ty}`"),
                        args[0].span.clone(),
                    );
                    return Some(Ty::Error);
                }
            };
            match &fn_ty {
                Ty::Fn(params, ret) => {
                    if params.len() != 1 {
                        self.error(
                            ErrorCode::E0133,
                            format!(
                                "all() callback must take 1 parameter, takes {}",
                                params.len()
                            ),
                            args[1].span.clone(),
                        );
                    } else if !elem_ty.is_error() && !params[0].is_error() && elem_ty != params[0] {
                        self.error(
                                        ErrorCode::E0100,
                                        format!("all() callback parameter type `{}` doesn't match array element type `{}`", params[0], elem_ty),
                                        args[1].span.clone(),
                                    );
                    }
                    if **ret != Ty::Bool && !ret.is_error() {
                        self.error(
                            ErrorCode::E0133,
                            format!("all() callback must return `bool`, returns `{}`", ret),
                            args[1].span.clone(),
                        );
                    }
                    return Some(Ty::Bool);
                }
                _ if fn_ty.is_error() => return Some(Ty::Error),
                _ => {
                    self.error(
                        ErrorCode::E0133,
                        format!("all() second argument must be a function, found `{fn_ty}`"),
                        args[1].span.clone(),
                    );
                    return Some(Ty::Error);
                }
            }
        }

        // ── Date/Time builtins ─────────────────────────
        // time_now() -> f64
        None
    }

    fn check_builtin_time(
        &mut self,
        name: &str,
        args: &[Spanned<Expr>],
        callee: &Spanned<Expr>,
    ) -> Option<Ty> {
        if name == "time_now" {
            if !args.is_empty() {
                self.error(
                    ErrorCode::E0100,
                    format!("time_now() takes 0 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            return Some(Ty::F64);
        }
        // time_ms() -> i64
        if name == "time_ms" {
            if !args.is_empty() {
                self.error(
                    ErrorCode::E0100,
                    format!("time_ms() takes 0 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            return Some(Ty::I64);
        }
        // format_time(timestamp: f64, format: str) -> str
        if name == "format_time" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0100,
                    format!(
                        "format_time() takes exactly 2 arguments, got {}",
                        args.len()
                    ),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let ts_ty = self.check_expr(&args[0]);
            let fmt_ty = self.check_expr(&args[1]);
            if !ts_ty.is_error() && ts_ty != Ty::F64 && ts_ty != Ty::F32 {
                self.error(
                    ErrorCode::E0100,
                    format!("format_time() first argument must be float, found `{ts_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !fmt_ty.is_error() && fmt_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("format_time() second argument must be str, found `{fmt_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::Str);
        }

        // sleep(ms: i64) -> () — sleep the current thread
        if name == "sleep" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0100,
                    format!("sleep() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let ms_ty = self.check_expr(&args[0]);
            if !ms_ty.is_error() && !ms_ty.is_integer() {
                self.error(
                    ErrorCode::E0100,
                    format!("sleep() expects integer (milliseconds), found `{ms_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Unit);
        }

        // ── HTTP builtins ───────────────────────────────
        // http_get(url: str) -> str
        None
    }

    fn check_builtin_http(
        &mut self,
        name: &str,
        args: &[Spanned<Expr>],
        callee: &Spanned<Expr>,
    ) -> Option<Ty> {
        if name == "http_get" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0513,
                    format!("http_get() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let url_ty = self.check_expr(&args[0]);
            if !url_ty.is_error() && url_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("http_get() expects str, found `{url_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Str);
        }
        // http_post(url: str, body: str) -> str
        if name == "http_post" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0513,
                    format!("http_post() takes exactly 2 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let url_ty = self.check_expr(&args[0]);
            let body_ty = self.check_expr(&args[1]);
            if !url_ty.is_error() && url_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("http_post() first argument must be str, found `{url_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !body_ty.is_error() && body_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("http_post() second argument must be str, found `{body_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::Str);
        }
        // http_post_with_headers(url: str, body: str, headers: str) -> str
        if name == "http_post_with_headers" {
            if args.len() != 3 {
                self.error(
                    ErrorCode::E0513,
                    format!(
                        "http_post_with_headers() takes exactly 3 arguments, got {}",
                        args.len()
                    ),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let url_ty = self.check_expr(&args[0]);
            let body_ty = self.check_expr(&args[1]);
            let headers_ty = self.check_expr(&args[2]);
            if !url_ty.is_error() && url_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!(
                        "http_post_with_headers() first argument must be str, found `{url_ty}`"
                    ),
                    args[0].span.clone(),
                );
            }
            if !body_ty.is_error() && body_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!(
                        "http_post_with_headers() second argument must be str, found `{body_ty}`"
                    ),
                    args[1].span.clone(),
                );
            }
            if !headers_ty.is_error() && headers_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!(
                        "http_post_with_headers() third argument must be str, found `{headers_ty}`"
                    ),
                    args[2].span.clone(),
                );
            }
            return Some(Ty::Str);
        }

        // ── HTTP server builtins ──────────────────────────
        // http_server(port: i64) -> i64          (binds 127.0.0.1)
        // http_server_public(port: i64) -> i64   (binds 0.0.0.0)
        if name == "http_server" || name == "http_server_public" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0100,
                    format!("{name}() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let port_ty = self.check_expr(&args[0]);
            if !port_ty.is_error() && !port_ty.is_integer() {
                self.error(
                    ErrorCode::E0100,
                    format!("{name}() expects integer port, found `{port_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Handle(HandleKind::HttpServer));
        }
        // route(server: i64, method: str, path: str, handler: fn(str) -> str)
        if name == "route" {
            if args.len() != 4 {
                self.error(
                    ErrorCode::E0513,
                    format!("route() takes exactly 4 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let server_ty = self.check_expr(&args[0]);
            let method_ty = self.check_expr(&args[1]);
            let path_ty = self.check_expr(&args[2]);
            // Set hint so closure param types can be inferred
            self.closure_param_hint = Some(vec![Ty::Str]);
            let handler_ty = self.check_expr(&args[3]);
            if !server_ty.is_error() && !server_ty.is_handle_or_int(HandleKind::HttpServer) {
                self.error(
                    ErrorCode::E0133,
                    format!("route() first argument must be server id (i64), found `{server_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !method_ty.is_error() && method_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!(
                        "route() second argument must be str (HTTP method), found `{method_ty}`"
                    ),
                    args[1].span.clone(),
                );
            }
            if !path_ty.is_error() && path_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("route() third argument must be str (path), found `{path_ty}`"),
                    args[2].span.clone(),
                );
            }
            match &handler_ty {
                Ty::Fn(params, ret) => {
                    if params.len() != 1 {
                        self.error(
                            ErrorCode::E0133,
                            format!(
                                "route() handler must take 1 parameter (request), takes {}",
                                params.len()
                            ),
                            args[3].span.clone(),
                        );
                    } else if !params[0].is_error() && params[0] != Ty::Str {
                        self.error(
                            ErrorCode::E0133,
                            format!(
                                "route() handler parameter must be str, found `{}`",
                                params[0]
                            ),
                            args[3].span.clone(),
                        );
                    }
                    if !ret.is_error() && **ret != Ty::Str {
                        self.error(
                            ErrorCode::E0100,
                            format!("route() handler must return str, returns `{}`", ret),
                            args[3].span.clone(),
                        );
                    }
                }
                _ if handler_ty.is_error() => {}
                _ => {
                    self.error(
                        ErrorCode::E0133,
                        format!("route() fourth argument must be a function, found `{handler_ty}`"),
                        args[3].span.clone(),
                    );
                }
            }
            return Some(Ty::Unit);
        }
        // http_listen(server: i64) -> ()
        if name == "http_listen" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0513,
                    format!("http_listen() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let server_ty = self.check_expr(&args[0]);
            if !server_ty.is_error() && !server_ty.is_handle_or_int(HandleKind::HttpServer) {
                self.error(
                    ErrorCode::E0100,
                    format!("http_listen() expects server id (i64), found `{server_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Unit);
        }
        // respond*(status: i64, body: str) -> str
        if matches!(
            name,
            "respond" | "respond_text" | "respond_html" | "respond_json"
        ) {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0513,
                    format!("{name}() takes exactly 2 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let status_ty = self.check_expr(&args[0]);
            let body_ty = self.check_expr(&args[1]);
            if !status_ty.is_error() && !status_ty.is_integer() {
                self.error(
                    ErrorCode::E0133,
                    format!(
                        "{name}() first argument must be integer status code, found `{status_ty}`"
                    ),
                    args[0].span.clone(),
                );
            }
            if !body_ty.is_error() && body_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("{name}() second argument must be str, found `{body_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::Str);
        }
        // request_body(req: str) -> str
        if name == "request_body" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0100,
                    format!(
                        "request_body() takes exactly 1 argument, got {}",
                        args.len()
                    ),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let req_ty = self.check_expr(&args[0]);
            if !req_ty.is_error() && req_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("request_body() expects str, found `{req_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Str);
        }

        // request_method(req: str) -> str, request_path(req: str) -> str
        if name == "request_method" || name == "request_path" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0100,
                    format!("{name}() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            self.check_expr(&args[0]);
            return Some(Ty::Str);
        }
        // request_query(req: str, key: str) -> str
        // request_header(req: str, name: str) -> str
        if name == "request_query" || name == "request_header" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0100,
                    format!("{name}() takes exactly 2 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            self.check_expr(&args[0]);
            self.check_expr(&args[1]);
            return Some(Ty::Str);
        }

        // ── JSON builtins ───────────────────────────────
        // json_get(json: str, key: str) -> str
        None
    }

    fn check_builtin_json(
        &mut self,
        name: &str,
        args: &[Spanned<Expr>],
        callee: &Spanned<Expr>,
    ) -> Option<Ty> {
        if name == "json_get" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0513,
                    format!("json_get() takes exactly 2 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let json_ty = self.check_expr(&args[0]);
            let key_ty = self.check_expr(&args[1]);
            if !json_ty.is_error() && json_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("json_get() first argument must be str, found `{json_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !key_ty.is_error() && key_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("json_get() second argument must be str, found `{key_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::Str);
        }
        // json_stringify(key: str, value: str) -> str
        if name == "json_stringify" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0513,
                    format!(
                        "json_stringify() takes exactly 2 arguments, got {}",
                        args.len()
                    ),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let key_ty = self.check_expr(&args[0]);
            let value_ty = self.check_expr(&args[1]);
            if !key_ty.is_error() && key_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("json_stringify() first argument must be str, found `{key_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !value_ty.is_error() && value_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("json_stringify() second argument must be str, found `{value_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::Str);
        }
        // json_build(pairs: str) -> str
        if name == "json_build" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0513,
                    format!("json_build() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let pairs_ty = self.check_expr(&args[0]);
            if !pairs_ty.is_error() && pairs_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("json_build() argument must be str, found `{pairs_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Str);
        }
        // to_json(val: any) -> str
        if name == "to_json" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0100,
                    format!("to_json() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let _val_ty = self.check_expr(&args[0]);
            return Some(Ty::Str);
        }
        // to_json_array(arr: [any]) -> str
        if name == "to_json_array" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0100,
                    format!(
                        "to_json_array() takes exactly 1 argument, got {}",
                        args.len()
                    ),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let val_ty = self.check_expr(&args[0]);
            if !val_ty.is_error() && !matches!(val_ty, Ty::Array(_)) {
                self.error(
                    ErrorCode::E0100,
                    format!("to_json_array() argument must be an array, found `{val_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Str);
        }

        // ── Channel builtins ───────────────────────────────
        // channel() -> i64 (channel pointer)
        None
    }

    fn check_builtin_concurrency(
        &mut self,
        name: &str,
        args: &[Spanned<Expr>],
        callee: &Spanned<Expr>,
    ) -> Option<Ty> {
        if name == "channel" {
            if !args.is_empty() {
                self.error(
                    ErrorCode::E0100,
                    format!("channel() takes no arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            return Some(Ty::I64);
        }
        // send(ch: i64, value: i64) -> ()
        if name == "send" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0513,
                    format!("send() takes exactly 2 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let ch_ty = self.check_expr(&args[0]);
            let val_ty = self.check_expr(&args[1]);
            if !ch_ty.is_error() && !ch_ty.is_integer() {
                self.error(
                    ErrorCode::E0133,
                    format!("send() first argument must be a channel (integer), found `{ch_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !val_ty.is_error() && !val_ty.is_integer() {
                self.error(
                    ErrorCode::E0133,
                    format!("send() second argument must be integer, found `{val_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::Unit);
        }
        // recv(ch: i64) -> i64
        if name == "recv" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0513,
                    format!("recv() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let ch_ty = self.check_expr(&args[0]);
            if !ch_ty.is_error() && !ch_ty.is_integer() {
                self.error(
                    ErrorCode::E0133,
                    format!("recv() argument must be a channel (integer), found `{ch_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::I64);
        }

        // ── Mutex builtins ────────────────────────────────
        // mutex(value: i64) -> i64 (mutex pointer)
        if name == "mutex" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0100,
                    format!("mutex() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let val_ty = self.check_expr(&args[0]);
            if !val_ty.is_error() && !val_ty.is_integer() {
                self.error(
                    ErrorCode::E0100,
                    format!("mutex() argument must be integer, found `{val_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Handle(HandleKind::Mutex));
        }
        // mutex_get(m: i64) -> i64
        if name == "mutex_get" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0513,
                    format!("mutex_get() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let m_ty = self.check_expr(&args[0]);
            if !m_ty.is_error() && !m_ty.is_handle_or_int(HandleKind::Mutex) {
                self.error(
                    ErrorCode::E0133,
                    format!("mutex_get() argument must be a mutex (integer), found `{m_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::I64);
        }
        // mutex_set(m: i64, value: i64) -> ()
        if name == "mutex_set" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0513,
                    format!("mutex_set() takes exactly 2 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let m_ty = self.check_expr(&args[0]);
            let val_ty = self.check_expr(&args[1]);
            if !m_ty.is_error() && !m_ty.is_handle_or_int(HandleKind::Mutex) {
                self.error(
                    ErrorCode::E0133,
                    format!("mutex_set() first argument must be a mutex (integer), found `{m_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !val_ty.is_error() && !val_ty.is_integer() {
                self.error(
                    ErrorCode::E0133,
                    format!("mutex_set() second argument must be integer, found `{val_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::Unit);
        }
        // mutex_update(m: i64, f: fn(i64) -> i64) -> i64
        // Runs `f(old)` atomically under the lock and stores the
        // result — the only way to express a correct read-modify-write
        // (e.g. a shared counter) over a mutex.
        if name == "mutex_update" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0513,
                    format!(
                        "mutex_update() takes exactly 2 arguments, got {}",
                        args.len()
                    ),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let m_ty = self.check_expr(&args[0]);
            if !m_ty.is_error() && !m_ty.is_handle_or_int(HandleKind::Mutex) {
                self.error(
                    ErrorCode::E0133,
                    format!(
                        "mutex_update() first argument must be a mutex (integer), found `{m_ty}`"
                    ),
                    args[0].span.clone(),
                );
            }
            // Hint the closure's parameter type so `|x| ...` infers `x: i64`.
            self.closure_param_hint = Some(vec![Ty::I64]);
            let fn_ty = self.check_expr(&args[1]);
            match &fn_ty {
                Ty::Fn(params, ret) => {
                    if params.len() != 1 {
                        self.error(
                            ErrorCode::E0133,
                            format!(
                                "mutex_update() callback must take 1 parameter, takes {}",
                                params.len()
                            ),
                            args[1].span.clone(),
                        );
                    } else if !params[0].is_error() && !params[0].is_integer() {
                        self.error(
                            ErrorCode::E0133,
                            format!(
                                "mutex_update() callback parameter must be integer, found `{}`",
                                params[0]
                            ),
                            args[1].span.clone(),
                        );
                    }
                    if !ret.is_error() && !ret.is_integer() {
                        self.error(
                            ErrorCode::E0133,
                            format!("mutex_update() callback must return integer, returns `{ret}`"),
                            args[1].span.clone(),
                        );
                    }
                }
                _ if fn_ty.is_error() => {}
                _ => {
                    self.error(
                                    ErrorCode::E0133,
                                    format!("mutex_update() second argument must be a function `fn(int) -> int`, found `{fn_ty}`"),
                                    args[1].span.clone(),
                                );
                }
            }
            return Some(Ty::I64);
        }

        // clone(val) -> T (requires @derive(Clone))
        if name == "clone" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0513,
                    format!("clone() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let arg_ty = self.check_expr(&args[0]);
            if let Ty::Struct(ref struct_name) = arg_ty {
                if let Some(info) = self.structs.get(struct_name) {
                    if !info.derives.contains(&"Clone".to_string()) {
                        self.error(
                            ErrorCode::E0100,
                            format!("cannot clone struct `{struct_name}` without `@derive(Clone)`"),
                            callee.span.clone(),
                        );
                        return Some(Ty::Error);
                    }
                }
            } else if !arg_ty.is_error() {
                self.error(
                    ErrorCode::E0100,
                    format!("clone() expects a struct argument, found `{arg_ty}`"),
                    args[0].span.clone(),
                );
                return Some(Ty::Error);
            }
            return Some(arg_ty);
        }

        // ── HashMap builtins ────────────────────────────────
        // hashmap() -> i64 (opaque pointer)
        None
    }

    fn check_builtin_hashmap(
        &mut self,
        name: &str,
        args: &[Spanned<Expr>],
        callee: &Spanned<Expr>,
    ) -> Option<Ty> {
        if name == "hashmap" {
            if !args.is_empty() {
                self.error(
                    ErrorCode::E0100,
                    format!("hashmap() takes no arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            return Some(Ty::Handle(HandleKind::HashMap));
        }
        // hashmap_set(map: i64, key: str, value: str) -> ()
        if name == "hashmap_set" {
            if args.len() != 3 {
                self.error(
                    ErrorCode::E0513,
                    format!(
                        "hashmap_set() takes exactly 3 arguments, got {}",
                        args.len()
                    ),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let map_ty = self.check_expr(&args[0]);
            let key_ty = self.check_expr(&args[1]);
            let val_ty = self.check_expr(&args[2]);
            if !map_ty.is_error() && !map_ty.is_handle_or_int(HandleKind::HashMap) {
                self.error(ErrorCode::E0133, format!("hashmap_set() first argument must be a hashmap (integer), found `{map_ty}`"), args[0].span.clone());
            }
            if !key_ty.is_error() && key_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("hashmap_set() second argument must be str, found `{key_ty}`"),
                    args[1].span.clone(),
                );
            }
            if !val_ty.is_error() && val_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("hashmap_set() third argument must be str, found `{val_ty}`"),
                    args[2].span.clone(),
                );
            }
            return Some(Ty::Unit);
        }
        // hashmap_get(map: i64, key: str) -> str
        if name == "hashmap_get" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0513,
                    format!(
                        "hashmap_get() takes exactly 2 arguments, got {}",
                        args.len()
                    ),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let map_ty = self.check_expr(&args[0]);
            let key_ty = self.check_expr(&args[1]);
            if !map_ty.is_error() && !map_ty.is_handle_or_int(HandleKind::HashMap) {
                self.error(ErrorCode::E0133, format!("hashmap_get() first argument must be a hashmap (integer), found `{map_ty}`"), args[0].span.clone());
            }
            if !key_ty.is_error() && key_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("hashmap_get() second argument must be str, found `{key_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::Str);
        }
        // hashmap_has(map: i64, key: str) -> bool
        if name == "hashmap_has" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0513,
                    format!(
                        "hashmap_has() takes exactly 2 arguments, got {}",
                        args.len()
                    ),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let map_ty = self.check_expr(&args[0]);
            let key_ty = self.check_expr(&args[1]);
            if !map_ty.is_error() && !map_ty.is_handle_or_int(HandleKind::HashMap) {
                self.error(ErrorCode::E0133, format!("hashmap_has() first argument must be a hashmap (integer), found `{map_ty}`"), args[0].span.clone());
            }
            if !key_ty.is_error() && key_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("hashmap_has() second argument must be str, found `{key_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::Bool);
        }
        // hashmap_len / hashmap_size(map: i64) -> i64
        if name == "hashmap_len" || name == "hashmap_size" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0513,
                    format!("hashmap_len() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let map_ty = self.check_expr(&args[0]);
            if !map_ty.is_error() && !map_ty.is_handle_or_int(HandleKind::HashMap) {
                self.error(
                    ErrorCode::E0133,
                    format!("hashmap_len() argument must be a hashmap (integer), found `{map_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::I64);
        }
        // hashmap_keys(map: i64) -> [str]
        if name == "hashmap_keys" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0513,
                    format!(
                        "hashmap_keys() takes exactly 1 argument, got {}",
                        args.len()
                    ),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let map_ty = self.check_expr(&args[0]);
            if !map_ty.is_error() && !map_ty.is_handle_or_int(HandleKind::HashMap) {
                self.error(
                    ErrorCode::E0133,
                    format!(
                        "hashmap_keys() argument must be a hashmap (integer), found `{map_ty}`"
                    ),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Array(Box::new(Ty::Str)));
        }
        // hashmap_remove(map: i64, key: str) -> ()
        if name == "hashmap_remove" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0513,
                    format!(
                        "hashmap_remove() takes exactly 2 arguments, got {}",
                        args.len()
                    ),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let map_ty = self.check_expr(&args[0]);
            let key_ty = self.check_expr(&args[1]);
            if !map_ty.is_error() && !map_ty.is_handle_or_int(HandleKind::HashMap) {
                self.error(ErrorCode::E0133, format!("hashmap_remove() first argument must be a hashmap (integer), found `{map_ty}`"), args[0].span.clone());
            }
            if !key_ty.is_error() && key_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("hashmap_remove() second argument must be str, found `{key_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::Unit);
        }
        // hashmap_set_int(map: i64, key: str, value: int) -> hashmap (i64)
        // v0.8.0 "Safe Core" str→int variant. Returns the map so it can be
        // used in `m = hashmap_set_int(m, k, v)` style.
        if name == "hashmap_set_int" {
            if args.len() != 3 {
                self.error(
                    ErrorCode::E0513,
                    format!(
                        "hashmap_set_int() takes exactly 3 arguments, got {}",
                        args.len()
                    ),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let map_ty = self.check_expr(&args[0]);
            let key_ty = self.check_expr(&args[1]);
            let val_ty = self.check_expr(&args[2]);
            if !map_ty.is_error() && !map_ty.is_handle_or_int(HandleKind::HashMap) {
                self.error(ErrorCode::E0133, format!("hashmap_set_int() first argument must be a hashmap (integer), found `{map_ty}`"), args[0].span.clone());
            }
            if !key_ty.is_error() && key_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("hashmap_set_int() second argument must be str, found `{key_ty}`"),
                    args[1].span.clone(),
                );
            }
            if !val_ty.is_error() && !val_ty.is_integer() {
                self.error(
                    ErrorCode::E0133,
                    format!("hashmap_set_int() third argument must be int, found `{val_ty}`"),
                    args[2].span.clone(),
                );
            }
            // Returns the map handle so `m = hashmap_set_int(m, k, v)` keeps `m`
            // typed as a hashmap handle (not a plain int).
            return Some(Ty::Handle(HandleKind::HashMap));
        }
        // hashmap_get_int(map: i64, key: str) -> int
        // Returns 0 on miss — guard with hashmap_has() if you need to
        // distinguish missing from a stored 0.
        if name == "hashmap_get_int" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0513,
                    format!(
                        "hashmap_get_int() takes exactly 2 arguments, got {}",
                        args.len()
                    ),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let map_ty = self.check_expr(&args[0]);
            let key_ty = self.check_expr(&args[1]);
            if !map_ty.is_error() && !map_ty.is_handle_or_int(HandleKind::HashMap) {
                self.error(ErrorCode::E0133, format!("hashmap_get_int() first argument must be a hashmap (integer), found `{map_ty}`"), args[0].span.clone());
            }
            if !key_ty.is_error() && key_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("hashmap_get_int() second argument must be str, found `{key_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::I64);
        }
        // hashmap_inc(map: i64, key: str[, delta: int]) -> int
        // Fused str→int increment: adds `delta` (default 1) to the
        // value at `key` (missing key counts as 0) and returns the
        // new value. Single hash + single probe — the fast path for
        // word-count style counters.
        if name == "hashmap_inc" {
            if args.len() != 2 && args.len() != 3 {
                self.error(
                    ErrorCode::E0513,
                    format!("hashmap_inc() takes 2 or 3 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let map_ty = self.check_expr(&args[0]);
            let key_ty = self.check_expr(&args[1]);
            if !map_ty.is_error() && !map_ty.is_handle_or_int(HandleKind::HashMap) {
                self.error(ErrorCode::E0133, format!("hashmap_inc() first argument must be a hashmap (integer), found `{map_ty}`"), args[0].span.clone());
            }
            if !key_ty.is_error() && key_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("hashmap_inc() second argument must be str, found `{key_ty}`"),
                    args[1].span.clone(),
                );
            }
            if args.len() == 3 {
                let delta_ty = self.check_expr(&args[2]);
                if !delta_ty.is_error() && !delta_ty.is_integer() {
                    self.error(
                        ErrorCode::E0133,
                        format!("hashmap_inc() third argument must be int, found `{delta_ty}`"),
                        args[2].span.clone(),
                    );
                }
            }
            return Some(Ty::I64);
        }

        // ── Unsafe builtins ────────────────────────────────
        // deref(addr: i64) -> i64 — raw memory load (unsafe only)
        None
    }

    fn check_builtin_refs(
        &mut self,
        name: &str,
        args: &[Spanned<Expr>],
        callee: &Spanned<Expr>,
    ) -> Option<Ty> {
        if name == "deref" {
            if !self.in_unsafe_context {
                self.error(
                    ErrorCode::E0100,
                    "`deref()` can only be called inside an `@unsafe` function".to_string(),
                    callee.span.clone(),
                );
            }
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0100,
                    format!("deref() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let addr_ty = self.check_expr(&args[0]);
            if !addr_ty.is_error() && !addr_ty.is_integer() {
                self.error(
                    ErrorCode::E0100,
                    format!("deref() argument must be i64, found `{addr_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::I64);
        }
        // store(addr: i64, value: i64) — raw memory store (unsafe only)
        if name == "store" {
            if !self.in_unsafe_context {
                self.error(
                    ErrorCode::E0100,
                    "`store()` can only be called inside an `@unsafe` function".to_string(),
                    callee.span.clone(),
                );
            }
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0100,
                    format!("store() takes exactly 2 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let addr_ty = self.check_expr(&args[0]);
            let val_ty = self.check_expr(&args[1]);
            if !addr_ty.is_error() && !addr_ty.is_integer() {
                self.error(
                    ErrorCode::E0100,
                    format!("store() first argument must be i64, found `{addr_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !val_ty.is_error() && !val_ty.is_integer() {
                self.error(
                    ErrorCode::E0100,
                    format!("store() second argument must be i64, found `{val_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::Unit);
        }

        // map(arr, fn) -> [U]
        if name == "map" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0513,
                    format!("map() takes exactly 2 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let arr_ty = self.check_expr(&args[0]);
            if let Ty::Array(ref inner) = arr_ty {
                self.closure_param_hint = Some(vec![*inner.clone()]);
            }
            let fn_ty = self.check_expr(&args[1]);
            let elem_ty = match &arr_ty {
                Ty::Array(inner) => *inner.clone(),
                _ if arr_ty.is_error() => Ty::Error,
                _ => {
                    self.error(
                        ErrorCode::E0133,
                        format!("map() first argument must be an array, found `{arr_ty}`"),
                        args[0].span.clone(),
                    );
                    return Some(Ty::Error);
                }
            };
            match &fn_ty {
                Ty::Fn(params, ret) => {
                    if params.len() != 1 {
                        self.error(
                            ErrorCode::E0133,
                            format!(
                                "map() callback must take 1 parameter, takes {}",
                                params.len()
                            ),
                            args[1].span.clone(),
                        );
                    } else if !elem_ty.is_error() && !params[0].is_error() && elem_ty != params[0] {
                        self.error(ErrorCode::E0100,
                                        format!("map() callback parameter type `{}` doesn't match array element type `{}`", params[0], elem_ty),
                                        args[1].span.clone(),
                                    );
                    }
                    return Some(Ty::Array(ret.clone()));
                }
                _ if fn_ty.is_error() => return Some(Ty::Error),
                _ => {
                    self.error(
                        ErrorCode::E0133,
                        format!("map() second argument must be a function, found `{fn_ty}`"),
                        args[1].span.clone(),
                    );
                    return Some(Ty::Error);
                }
            }
        }

        // filter(arr, fn) -> [T]
        if name == "filter" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0513,
                    format!("filter() takes exactly 2 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let arr_ty = self.check_expr(&args[0]);
            if let Ty::Array(ref inner) = arr_ty {
                self.closure_param_hint = Some(vec![*inner.clone()]);
            }
            let fn_ty = self.check_expr(&args[1]);
            let elem_ty = match &arr_ty {
                Ty::Array(inner) => *inner.clone(),
                _ if arr_ty.is_error() => Ty::Error,
                _ => {
                    self.error(
                        ErrorCode::E0133,
                        format!("filter() first argument must be an array, found `{arr_ty}`"),
                        args[0].span.clone(),
                    );
                    return Some(Ty::Error);
                }
            };
            match &fn_ty {
                Ty::Fn(params, ret) => {
                    if params.len() != 1 {
                        self.error(
                            ErrorCode::E0133,
                            format!(
                                "filter() callback must take 1 parameter, takes {}",
                                params.len()
                            ),
                            args[1].span.clone(),
                        );
                    } else if !elem_ty.is_error() && !params[0].is_error() && elem_ty != params[0] {
                        self.error(ErrorCode::E0100,
                                        format!("filter() callback parameter type `{}` doesn't match array element type `{}`", params[0], elem_ty),
                                        args[1].span.clone(),
                                    );
                    }
                    if **ret != Ty::Bool && !ret.is_error() {
                        self.error(
                            ErrorCode::E0133,
                            format!("filter() callback must return `bool`, returns `{}`", ret),
                            args[1].span.clone(),
                        );
                    }
                    return Some(arr_ty);
                }
                _ if fn_ty.is_error() => return Some(Ty::Error),
                _ => {
                    self.error(
                        ErrorCode::E0133,
                        format!("filter() second argument must be a function, found `{fn_ty}`"),
                        args[1].span.clone(),
                    );
                    return Some(Ty::Error);
                }
            }
        }

        // reduce(arr, init, fn) -> U
        if name == "reduce" {
            if args.len() != 3 {
                self.error(
                    ErrorCode::E0513,
                    format!("reduce() takes exactly 3 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let arr_ty = self.check_expr(&args[0]);
            let init_ty = self.check_expr(&args[1]);
            {
                let elem = match &arr_ty {
                    Ty::Array(inner) => *inner.clone(),
                    _ => Ty::Error,
                };
                self.closure_param_hint = Some(vec![init_ty.clone(), elem]);
            }
            let fn_ty = self.check_expr(&args[2]);
            let elem_ty = match &arr_ty {
                Ty::Array(inner) => *inner.clone(),
                _ if arr_ty.is_error() => Ty::Error,
                _ => {
                    self.error(
                        ErrorCode::E0133,
                        format!("reduce() first argument must be an array, found `{arr_ty}`"),
                        args[0].span.clone(),
                    );
                    return Some(Ty::Error);
                }
            };
            match &fn_ty {
                Ty::Fn(params, ret) => {
                    if params.len() != 2 {
                        self.error(
                            ErrorCode::E0133,
                            format!(
                                "reduce() callback must take 2 parameters, takes {}",
                                params.len()
                            ),
                            args[2].span.clone(),
                        );
                    } else {
                        if !init_ty.is_error() && !params[0].is_error() && init_ty != params[0] {
                            self.error(ErrorCode::E0133,
                                            format!("reduce() callback first parameter type `{}` doesn't match initial value type `{}`", params[0], init_ty),
                                            args[2].span.clone(),
                                        );
                        }
                        if !elem_ty.is_error() && !params[1].is_error() && elem_ty != params[1] {
                            self.error(ErrorCode::E0133,
                                            format!("reduce() callback second parameter type `{}` doesn't match array element type `{}`", params[1], elem_ty),
                                            args[2].span.clone(),
                                        );
                        }
                    }
                    return Some(*ret.clone());
                }
                _ if fn_ty.is_error() => return Some(Ty::Error),
                _ => {
                    self.error(
                        ErrorCode::E0133,
                        format!("reduce() third argument must be a function, found `{fn_ty}`"),
                        args[2].span.clone(),
                    );
                    return Some(Ty::Error);
                }
            }
        }
        None
    }
}
