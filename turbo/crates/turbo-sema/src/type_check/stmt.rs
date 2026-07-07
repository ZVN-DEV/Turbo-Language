//! Statement-level type checking for the Turbo semantic analyzer.
//!
//! This module contains `check_stmt`, which handles `let`, `return`,
//! `defer`, expression-statements, and destructuring let bindings.

use turbo_ast::*;

use crate::scope::VarInfo;
use crate::{
    array_literal_coerces, extract_int_literal, int_literal_fits_in_type, literal_coerces_to,
    resolve_type_expr, types_compatible, Checker, Ty,
};

impl Checker {
    /// Report E0525 for any `HashMap<K, V>` reachable from `ty` whose key type
    /// `K` is not a valid hashmap key (int or str). Recurses through arrays,
    /// optionals, results, functions, and nested maps so the restriction holds
    /// wherever a map type appears in an annotation.
    pub(crate) fn report_bad_hashmap_keys(&mut self, ty: &Ty, span: &Span) {
        match ty {
            Ty::HashMap(k, v) => {
                if !k.is_valid_hashmap_key() && !k.contains_error() {
                    self.error(
                        ErrorCode::E0525,
                        format!("hashmap key type must be int or str, found `{k}`"),
                        span.clone(),
                    );
                }
                self.report_bad_hashmap_keys(k, span);
                self.report_bad_hashmap_keys(v, span);
            }
            Ty::Array(inner) | Ty::Optional(inner) | Ty::Future(inner) => {
                self.report_bad_hashmap_keys(inner, span)
            }
            Ty::Result(a, b) => {
                self.report_bad_hashmap_keys(a, span);
                self.report_bad_hashmap_keys(b, span);
            }
            Ty::Fn(params, ret) => {
                for p in params {
                    self.report_bad_hashmap_keys(p, span);
                }
                self.report_bad_hashmap_keys(ret, span);
            }
            _ => {}
        }
    }

    /// Resolve a type annotation and flag any invalid `HashMap` key inside it.
    /// A key that cannot be resolved (an unknown/generic type parameter) is left
    /// alone — it is caught, if wrong, when the generic is instantiated.
    fn validate_type_expr_hashmap_keys(&mut self, te: &Spanned<TypeExpr>) {
        if let Some(ty) = resolve_type_expr(&te.node, Some(&self.structs), Some(&self.enums)) {
            self.report_bad_hashmap_keys(&ty, &te.span);
        }
    }

    fn validate_fn_hashmap_annotations(&mut self, f: &FnDef) {
        for p in &f.params {
            self.validate_type_expr_hashmap_keys(&p.ty);
        }
        if let Some(rt) = &f.return_type {
            self.validate_type_expr_hashmap_keys(rt);
        }
    }

    /// Enforce the `HashMap` key restriction (E0525) across every signature-level
    /// type annotation in the module: fn params/returns, struct fields, enum
    /// variant payloads, trait method signatures, impl methods, and const types.
    pub(crate) fn validate_hashmap_key_annotations(&mut self, module: &Module) {
        for item in &module.items {
            match &item.node {
                Item::Function(f) => self.validate_fn_hashmap_annotations(f),
                Item::Struct(s) => {
                    for field in &s.fields {
                        self.validate_type_expr_hashmap_keys(&field.ty);
                    }
                }
                Item::Enum(e) => {
                    for variant in &e.variants {
                        for field_ty in &variant.fields {
                            self.validate_type_expr_hashmap_keys(field_ty);
                        }
                    }
                }
                Item::Trait(t) => {
                    for m in &t.methods {
                        for p in &m.params {
                            self.validate_type_expr_hashmap_keys(&p.ty);
                        }
                        if let Some(rt) = &m.return_type {
                            self.validate_type_expr_hashmap_keys(rt);
                        }
                    }
                }
                Item::Impl(imp) => {
                    for m in &imp.methods {
                        self.validate_fn_hashmap_annotations(&m.node);
                    }
                }
                Item::Const(c) => {
                    if let Some(ty) = &c.ty {
                        self.validate_type_expr_hashmap_keys(ty);
                    }
                }
                _ => {}
            }
        }
    }

    pub(crate) fn check_stmt(&mut self, stmt: &Spanned<Stmt>) {
        match &stmt.node {
            Stmt::Let {
                mutable,
                name,
                ty,
                value,
            } => {
                // Special case: empty array literal `[]` with a declared array type.
                // Normally check_expr on an empty ArrayLit emits E0115 ("cannot infer
                // type of empty array"). When the Let binding has an explicit array
                // type annotation, use the annotation as the element-type hint instead
                // of erroring. See ISSUES.md Issue #1.
                let empty_array_with_annotation = matches!(&value.node, Expr::ArrayLit(v) if v.is_empty())
                    && ty
                        .as_ref()
                        .map(|t| matches!(&t.node, TypeExpr::Array(_)))
                        .unwrap_or(false);

                let val_ty = if empty_array_with_annotation {
                    // Resolve the declared type and use it directly.
                    let ty_expr = ty.as_ref().unwrap();
                    resolve_type_expr(&ty_expr.node, Some(&self.structs), Some(&self.enums))
                        .unwrap_or(Ty::Error)
                } else {
                    self.check_expr(value)
                };

                let declared_ty = if let Some(ty_expr) = ty {
                    match resolve_type_expr(&ty_expr.node, Some(&self.structs), Some(&self.enums)) {
                        Some(t) => {
                            self.report_bad_hashmap_keys(&t, &ty_expr.span);
                            if let Ty::HashMap(_, _) = &t {
                                // A typed map can ONLY be born from `hashmap()`
                                // bound directly to a `HashMap<K,V>` annotation
                                // (the one path codegen builds a typed descriptor
                                // for). Any other value — a legacy handle, an
                                // int, a differently-typed map — is rejected: the
                                // generic runtime would misread it and segfault.
                                let ok = crate::is_bare_hashmap_call(&value.node)
                                    || val_ty.contains_error()
                                    || val_ty == t;
                                if !ok {
                                    self.error(
                                        ErrorCode::E0110,
                                        format!(
                                            "a typed `{t}` can only be created by `hashmap()` bound to a `HashMap` annotation; `{val_ty}` is not assignable to it"
                                        ),
                                        ty_expr.span.clone(),
                                    );
                                }
                            } else if !val_ty.contains_error()
                                && !types_compatible(&t, &val_ty)
                                && t != val_ty
                            {
                                // Allow annotated-literal coercion:
                                //   * a scalar int/float literal into a sized
                                //     numeric type (`let x: u32 = 30`), and
                                //   * an array literal element-wise into a sized
                                //     array type (`let b: [u8] = [104, 105]`).
                                let is_literal_coercion = literal_coerces_to(&value.node, &t)
                                    || array_literal_coerces(&value.node, &t);
                                if !is_literal_coercion {
                                    // Echo the source spelling the user wrote
                                    // (`i64`) rather than the canonical alias
                                    // (`int`) so the message names their token.
                                    let annotation =
                                        crate::type_annotation_label(&ty_expr.node, &t);
                                    self.error(ErrorCode::E0110,
                                        format!(
                                            "type annotation `{annotation}` doesn't match value type `{val_ty}`"
                                        ),
                                        ty_expr.span.clone(),
                                    );
                                }
                            }
                            t
                        }
                        None => {
                            if let TypeExpr::Named(name) = &ty_expr.node {
                                self.error(
                                    ErrorCode::E0305,
                                    format!("unknown type `{name}`"),
                                    ty_expr.span.clone(),
                                );
                            }
                            val_ty.clone()
                        }
                    }
                } else {
                    val_ty
                };

                self.define_var(
                    name,
                    VarInfo {
                        ty: declared_ty,
                        mutable: *mutable,
                        span: 0..0,
                        from_let: true,
                    },
                    &stmt.span,
                );
            }
            Stmt::Expr(e) => {
                let ty = self.check_expr(e);
                // Warn when a pure builtin's return value is discarded in statement position
                if ty != Ty::Unit && ty != Ty::Error {
                    if let Expr::Call { ref callee, .. } = e.node {
                        if let Expr::Ident(ref fn_name) = callee.node {
                            const PURE_BUILTINS: &[&str] = &[
                                "len",
                                "abs",
                                "min",
                                "max",
                                "pow",
                                "sqrt",
                                "to_str",
                                "starts_with",
                                "ends_with",
                                "contains",
                                "char_at",
                                "index_of",
                                "join",
                                "reduce",
                                "clone",
                                "hashmap_get",
                                "hashmap_has",
                                "hashmap_len",
                                "hashmap_size",
                                "hashmap_keys",
                                "read_line",
                                "read_file",
                                "exec",
                                "env_get",
                                "json_get",
                                "json_stringify",
                                "json_build",
                                "float_to_int",
                                "int_to_float",
                                "http_post_with_headers",
                                "request_body",
                                "request_method",
                                "request_path",
                                "request_query",
                                "request_header",
                            ];
                            if PURE_BUILTINS.contains(&fn_name.as_str()) {
                                self.warn(
                                    ErrorCode::E0514,
                                    format!("return value of `{fn_name}()` is unused"),
                                    e.span.clone(),
                                );
                            }
                        }
                    }
                }
            }
            Stmt::Return(value) => {
                let ret_ty = if let Some(val) = value {
                    // Hint the function's declared return type so a bare empty
                    // array literal `return []` infers its element type from
                    // the return slot (e.g. `fn f() -> [str] { return [] }`)
                    // instead of failing with E0115 (BL-26).
                    let expected = self.current_return_type.clone();
                    self.check_expr_expecting(val, &expected)
                } else {
                    Ty::Unit
                };

                if !ret_ty.contains_error()
                    && !self.current_return_type.contains_error()
                    && self.current_return_type != Ty::Unit
                    && !types_compatible(&self.current_return_type, &ret_ty)
                    && ret_ty != self.current_return_type
                {
                    // Allow integer literal coercion for return values
                    let is_return_coercion = ret_ty == Ty::I64
                        && matches!(
                            self.current_return_type,
                            Ty::I8 | Ty::I16 | Ty::I32 | Ty::U8 | Ty::U16 | Ty::U32 | Ty::U64
                        )
                        && value
                            .as_ref()
                            .and_then(|v| extract_int_literal(&v.node))
                            .is_some_and(|n| {
                                int_literal_fits_in_type(n, &self.current_return_type)
                            });
                    if !is_return_coercion {
                        self.error(
                            ErrorCode::E0109,
                            format!(
                                "return type `{ret_ty}` doesn't match function return type `{}`",
                                self.current_return_type
                            ),
                            stmt.span.clone(),
                        );
                    }
                }
            }
            Stmt::Defer(expr) => {
                // Type-check the deferred expression (it should be a valid expression, typically a call)
                self.check_expr(expr);
            }
            Stmt::LetDestructure {
                mutable,
                fields,
                value,
            } => {
                let val_ty = self.check_expr(value);
                match &val_ty {
                    Ty::Struct(struct_name) => {
                        if let Some(info) = self.structs.get(struct_name).cloned() {
                            for field_name in fields {
                                if let Some((_, field_ty)) =
                                    info.fields.iter().find(|(n, _)| n == field_name)
                                {
                                    self.define_var(
                                        field_name,
                                        VarInfo {
                                            ty: field_ty.clone(),
                                            mutable: *mutable,
                                            span: 0..0,
                                            from_let: true,
                                        },
                                        &stmt.span,
                                    );
                                } else {
                                    self.error(
                                        ErrorCode::E0303,
                                        format!(
                                            "struct `{struct_name}` has no field `{field_name}`"
                                        ),
                                        stmt.span.clone(),
                                    );
                                }
                            }
                        } else {
                            self.error(
                                ErrorCode::E0305,
                                format!("unknown struct `{struct_name}`"),
                                stmt.span.clone(),
                            );
                        }
                    }
                    Ty::Error => {
                        // Suppress cascading errors — define all fields as error type
                        for field_name in fields {
                            self.define_var(
                                field_name,
                                VarInfo {
                                    ty: Ty::Error,
                                    mutable: *mutable,
                                    span: 0..0,
                                    from_let: true,
                                },
                                &stmt.span,
                            );
                        }
                    }
                    _ => {
                        self.error(
                            ErrorCode::E0110,
                            format!("cannot destructure non-struct type `{val_ty}`"),
                            stmt.span.clone(),
                        );
                    }
                }
            }
        }
    }
}
