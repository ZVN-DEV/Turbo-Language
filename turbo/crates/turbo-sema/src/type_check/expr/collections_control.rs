//! Loops, ranges, collection/struct/field expressions, pattern matching,
//! closures, and the small leaf expression forms. Part of the
//! [`super`] expression-checking implementation.

use std::collections::HashMap;

use turbo_ast::*;

use crate::scope::VarInfo;
use crate::{literal_coerces_to, resolve_type_expr, Checker, HandleKind, Ty};

impl Checker {
    pub(crate) fn check_while(&mut self, expr: &Spanned<Expr>) -> Ty {
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

    pub(crate) fn check_await(&mut self, expr: &Spanned<Expr>) -> Ty {
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

    pub(crate) fn check_spawn(&mut self, expr: &Spanned<Expr>) -> Ty {
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

    pub(crate) fn check_try(&mut self, expr: &Spanned<Expr>) -> Ty {
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

    pub(crate) fn check_range(&mut self, expr: &Spanned<Expr>) -> Ty {
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

    pub(crate) fn check_for_in(&mut self, expr: &Spanned<Expr>) -> Ty {
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

    pub(crate) fn check_array_lit(&mut self, expr: &Spanned<Expr>) -> Ty {
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

    pub(crate) fn check_index(&mut self, expr: &Spanned<Expr>) -> Ty {
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

    pub(crate) fn check_struct_lit(&mut self, expr: &Spanned<Expr>) -> Ty {
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
            // When the field exists, hint its declared type so a bare empty
            // array literal `[]` infers its element type from the field
            // (e.g. `Bag { tags: [] }` where `tags: [str]`) instead of
            // failing with E0115 (BL-26).
            let val_ty = match expected_fields.get(field_name.as_str()) {
                Some(expected_ty) => self.check_expr_expecting(value, expected_ty),
                None => self.check_expr(value),
            };
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

    pub(crate) fn check_field_access(&mut self, expr: &Spanned<Expr>) -> Ty {
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

    pub(crate) fn check_optional_chain(&mut self, expr: &Spanned<Expr>) -> Ty {
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

    pub(crate) fn check_enum_variant(&mut self, expr: &Spanned<Expr>) -> Ty {
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

    pub(crate) fn check_match(&mut self, expr: &Spanned<Expr>) -> Ty {
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

    pub(crate) fn check_interpolation(&mut self, expr: &Spanned<Expr>) -> Ty {
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

    pub(crate) fn check_closure(&mut self, expr: &Spanned<Expr>) -> Ty {
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

    pub(crate) fn check_ok_expr(&mut self, expr: &Spanned<Expr>) -> Ty {
        let Expr::OkExpr(value) = &expr.node else {
            unreachable!()
        };
        let val_ty = self.check_expr(value);
        // Return a partial result type -- the error type is unknown without context
        Ty::Result(Box::new(val_ty), Box::new(Ty::Error))
    }

    pub(crate) fn check_err_expr(&mut self, expr: &Spanned<Expr>) -> Ty {
        let Expr::ErrExpr(value) = &expr.node else {
            unreachable!()
        };
        let val_ty = self.check_expr(value);
        // Return a partial result type -- the ok type is unknown without context
        Ty::Result(Box::new(Ty::Error), Box::new(val_ty))
    }

    pub(crate) fn check_some_expr(&mut self, expr: &Spanned<Expr>) -> Ty {
        let Expr::SomeExpr(value) = &expr.node else {
            unreachable!()
        };
        let val_ty = self.check_expr(value);
        Ty::Optional(Box::new(val_ty))
    }

    pub(crate) fn check_break(&mut self, expr: &Spanned<Expr>) -> Ty {
        if self.loop_depth == 0 {
            self.error(
                ErrorCode::E0507,
                "`break` can only be used inside a loop".to_string(),
                expr.span.clone(),
            );
        }
        Ty::Unit
    }

    pub(crate) fn check_continue(&mut self, expr: &Spanned<Expr>) -> Ty {
        if self.loop_depth == 0 {
            self.error(
                ErrorCode::E0508,
                "`continue` can only be used inside a loop".to_string(),
                expr.span.clone(),
            );
        }
        Ty::Unit
    }

    pub(crate) fn check_map_lit(&mut self, expr: &Spanned<Expr>) -> Ty {
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

    pub(crate) fn check_null_coalesce(&mut self, expr: &Spanned<Expr>) -> Ty {
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
}
