//! Match-arm pattern checking and exhaustiveness helpers.
//!
//! This module is intentionally small and focused:
//! * `Checker::check_pattern` — validates that a single pattern is well-typed
//!   against its match subject. Catches E0132 (pattern type mismatch) and
//!   E0316 (unknown enum variant) among others.
//!
//! The bulk of the exhaustiveness / usefulness analysis for a whole match
//! expression still lives inline in `type_check.rs` because it is tightly
//! interleaved with binding introduction and branch-type unification for the
//! `Expr::Match` arm of `check_expr_inner`. Splitting it further would
//! require non-trivial plumbing of partial `Checker` state; that refactor is
//! deferred (see `// TODO(P3)` markers in `type_check.rs`).

use turbo_ast::{ErrorCode, Pattern, Spanned};

use crate::{Checker, Ty};

impl Checker {
    pub(crate) fn check_pattern(&mut self, pattern: &Spanned<Pattern>, subject_ty: &Ty) {
        match &pattern.node {
            Pattern::Wildcard => {
                // Wildcard matches anything
            }
            Pattern::Ident(name) => {
                // If subject is an enum, check that name is a valid variant
                if let Ty::Enum(enum_name) = subject_ty {
                    if let Some(info) = self.enums.get(enum_name) {
                        if !info.has_variant(name) {
                            self.error(
                                ErrorCode::E0316,
                                format!("enum `{enum_name}` has no variant `{name}`"),
                                pattern.span.clone(),
                            );
                        }
                    }
                }
                // For non-enum types, Ident pattern is treated as a variable binding
            }
            Pattern::IntLit(_) => {
                if !subject_ty.is_error() && !subject_ty.is_integer() {
                    self.error(
                        ErrorCode::E0132,
                        format!("integer pattern cannot match `{subject_ty}`"),
                        pattern.span.clone(),
                    );
                }
            }
            Pattern::BoolLit(_) => {
                if !subject_ty.is_error() && *subject_ty != Ty::Bool {
                    self.error(
                        ErrorCode::E0132,
                        format!("boolean pattern cannot match `{subject_ty}`"),
                        pattern.span.clone(),
                    );
                }
            }
            Pattern::StringLit(_) => {
                if !subject_ty.is_error() && *subject_ty != Ty::Str {
                    self.error(
                        ErrorCode::E0132,
                        format!("string pattern cannot match `{subject_ty}`"),
                        pattern.span.clone(),
                    );
                }
            }
            Pattern::Ok(_) => {
                if !subject_ty.is_error() && !matches!(subject_ty, Ty::Result(_, _)) {
                    self.error(
                        ErrorCode::E0132,
                        format!("ok pattern cannot match `{subject_ty}`"),
                        pattern.span.clone(),
                    );
                }
            }
            Pattern::Err(_) => {
                if !subject_ty.is_error() && !matches!(subject_ty, Ty::Result(_, _)) {
                    self.error(
                        ErrorCode::E0132,
                        format!("err pattern cannot match `{subject_ty}`"),
                        pattern.span.clone(),
                    );
                }
            }
            Pattern::Some(_) => {
                if !subject_ty.is_error() && !matches!(subject_ty, Ty::Optional(_)) {
                    self.error(
                        ErrorCode::E0132,
                        format!("some pattern cannot match `{subject_ty}`"),
                        pattern.span.clone(),
                    );
                }
            }
            Pattern::None => {
                if !subject_ty.is_error() && !matches!(subject_ty, Ty::Optional(_)) {
                    self.error(
                        ErrorCode::E0132,
                        format!("none pattern cannot match `{subject_ty}`"),
                        pattern.span.clone(),
                    );
                }
            }
            Pattern::VariantDestructure { variant, bindings } => {
                if let Ty::Enum(enum_name) = subject_ty {
                    if let Some(info) = self.enums.get(enum_name) {
                        if let Some(field_tys) = info.variant_fields(variant) {
                            if bindings.len() != field_tys.len() {
                                self.error(ErrorCode::E0100,
                                    format!(
                                        "variant `{variant}` has {} field(s) but pattern has {} binding(s)",
                                        field_tys.len(), bindings.len()
                                    ),
                                    pattern.span.clone(),
                                );
                            }
                        } else {
                            self.error(
                                ErrorCode::E0316,
                                format!("enum `{enum_name}` has no variant `{variant}`"),
                                pattern.span.clone(),
                            );
                        }
                    }
                } else if !subject_ty.is_error() {
                    self.error(
                        ErrorCode::E0132,
                        format!("variant destructure pattern cannot match `{subject_ty}`"),
                        pattern.span.clone(),
                    );
                }
            }
        }
    }
}
