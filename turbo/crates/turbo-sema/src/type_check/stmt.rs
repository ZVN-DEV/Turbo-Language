//! Statement-level type checking for the Turbo semantic analyzer.
//!
//! This module contains `check_stmt`, which handles `let`, `return`,
//! `defer`, expression-statements, and destructuring let bindings.

use turbo_ast::*;

use crate::scope::VarInfo;
use crate::{
    extract_int_literal, int_literal_fits_in_type, resolve_type_expr, types_compatible, Checker, Ty,
};

impl Checker {
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
                            if !val_ty.contains_error()
                                && !types_compatible(&t, &val_ty)
                                && t != val_ty
                            {
                                // Allow integer literal coercion: i64 literal -> narrower int types
                                let is_int_literal_coercion = val_ty == Ty::I64
                                    && matches!(
                                        t,
                                        Ty::I8
                                            | Ty::I16
                                            | Ty::I32
                                            | Ty::U8
                                            | Ty::U16
                                            | Ty::U32
                                            | Ty::U64
                                    )
                                    && extract_int_literal(&value.node)
                                        .is_some_and(|n| int_literal_fits_in_type(n, &t));
                                // Allow float literal coercion: f64 literal -> f32
                                let is_float_literal_coercion = val_ty == Ty::F64
                                    && t == Ty::F32
                                    && matches!(&value.node, Expr::FloatLit(_));
                                if !is_int_literal_coercion && !is_float_literal_coercion {
                                    self.error(ErrorCode::E0110,
                                        format!(
                                            "type annotation `{t}` doesn't match value type `{val_ty}`"
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
                    self.check_expr(val)
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
