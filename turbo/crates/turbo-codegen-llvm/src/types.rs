//! Turbo-level type tags and LLVM type conversion helpers.

use inkwell::context::Context;
use inkwell::types::BasicTypeEnum;
use inkwell::AddressSpace;
use std::collections::HashMap;
use turbo_ast::*;

// ── Turbo-level type tag ────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub(crate) enum TurboTy {
    Int,
    Float,
    Bool,
    Str,
    Unit,
    Array(Box<TurboTy>),
    Struct(String),
    Enum(String),
    Fn(Vec<TurboTy>, Box<TurboTy>),
    Result(Box<TurboTy>, Box<TurboTy>),
    Optional(Box<TurboTy>),
    Future(Box<TurboTy>),
}

pub(crate) type Typed<'ctx> = (inkwell::values::BasicValueEnum<'ctx>, TurboTy);
pub(crate) type MaybeTyped<'ctx> = Option<Typed<'ctx>>;

// ── Type conversion helpers ─────────────────────────────────────────

pub(crate) fn turbo_ty_from_type_expr(
    te: &TypeExpr,
    enum_variants: &HashMap<String, Vec<String>>,
) -> TurboTy {
    turbo_ty_from_type_expr_with_params(te, enum_variants, &[])
}

pub(crate) fn turbo_ty_from_type_expr_with_params(
    te: &TypeExpr,
    enum_variants: &HashMap<String, Vec<String>>,
    type_params: &[String],
) -> TurboTy {
    match te {
        TypeExpr::Named(name) => {
            if type_params.contains(name) {
                return TurboTy::Int;
            }
            match name.as_str() {
                "i32" | "i64" | "u32" | "u64" => TurboTy::Int,
                "f32" | "f64" => TurboTy::Float,
                "bool" => TurboTy::Bool,
                "str" => TurboTy::Str,
                _ => {
                    if enum_variants.contains_key(name.as_str()) {
                        TurboTy::Enum(name.clone())
                    } else {
                        TurboTy::Struct(name.clone())
                    }
                }
            }
        }
        TypeExpr::Unit => TurboTy::Unit,
        TypeExpr::Array(inner) => TurboTy::Array(Box::new(turbo_ty_from_type_expr(
            &inner.node,
            enum_variants,
        ))),
        TypeExpr::FnType { params, ret } => {
            let param_tys: Vec<TurboTy> = params
                .iter()
                .map(|p| turbo_ty_from_type_expr(&p.node, enum_variants))
                .collect();
            let ret_ty = turbo_ty_from_type_expr(&ret.node, enum_variants);
            TurboTy::Fn(param_tys, Box::new(ret_ty))
        }
        TypeExpr::Result { ok_type, err_type } => {
            let ok_tty = turbo_ty_from_type_expr(&ok_type.node, enum_variants);
            let err_tty = turbo_ty_from_type_expr(&err_type.node, enum_variants);
            TurboTy::Result(Box::new(ok_tty), Box::new(err_tty))
        }
        TypeExpr::Optional(inner) => TurboTy::Optional(Box::new(turbo_ty_from_type_expr(
            &inner.node,
            enum_variants,
        ))),
        TypeExpr::Future(inner) => TurboTy::Future(Box::new(turbo_ty_from_type_expr_with_params(
            &inner.node,
            enum_variants,
            type_params,
        ))),
        _ => TurboTy::Int,
    }
}

/// Convert a TurboTy to an LLVM BasicTypeEnum.
pub(crate) fn turbo_ty_to_llvm<'ctx>(tty: &TurboTy, context: &'ctx Context) -> BasicTypeEnum<'ctx> {
    match tty {
        TurboTy::Int => context.i64_type().into(),
        TurboTy::Float => context.f64_type().into(),
        TurboTy::Bool => context.i8_type().into(),
        TurboTy::Str => context.ptr_type(AddressSpace::default()).into(),
        TurboTy::Unit => context.i64_type().into(),
        TurboTy::Fn(_, _) => context.ptr_type(AddressSpace::default()).into(),
        TurboTy::Array(_) => context.ptr_type(AddressSpace::default()).into(),
        TurboTy::Struct(_) => context.ptr_type(AddressSpace::default()).into(),
        // NOTE: Enum types are i64 for unit-only enums. For data-carrying enums,
        // use turbo_ty_to_llvm_with_enums() which checks enum_max_slots.
        TurboTy::Enum(_) => context.i64_type().into(),
        TurboTy::Result(_, _) => context.ptr_type(AddressSpace::default()).into(),
        TurboTy::Optional(_) => context.ptr_type(AddressSpace::default()).into(),
        TurboTy::Future(_) => context.ptr_type(AddressSpace::default()).into(),
    }
}

/// Resolve a TypeExpr to a TurboTy, then to an LLVM type.
#[allow(dead_code)]
pub(crate) fn resolve_llvm_type<'ctx>(
    ty: &TypeExpr,
    context: &'ctx Context,
    enum_variants: &HashMap<String, Vec<String>>,
    type_params: &[String],
) -> BasicTypeEnum<'ctx> {
    let tty = turbo_ty_from_type_expr_with_params(ty, enum_variants, type_params);
    turbo_ty_to_llvm(&tty, context)
}

/// Like turbo_ty_to_llvm but correctly handles data-carrying enums as pointers.
pub(crate) fn turbo_ty_to_llvm_ctx<'ctx>(
    tty: &TurboTy,
    context: &'ctx Context,
    enum_max_slots: &HashMap<String, usize>,
) -> BasicTypeEnum<'ctx> {
    if let TurboTy::Enum(ref name) = tty {
        if enum_max_slots.contains_key(name) {
            return context.ptr_type(AddressSpace::default()).into();
        }
    }
    turbo_ty_to_llvm(tty, context)
}

/// Resolve a TypeExpr to an LLVM type, data-enum-aware.
pub(crate) fn resolve_llvm_type_ctx<'ctx>(
    ty: &TypeExpr,
    context: &'ctx Context,
    enum_variants: &HashMap<String, Vec<String>>,
    enum_max_slots: &HashMap<String, usize>,
    type_params: &[String],
) -> BasicTypeEnum<'ctx> {
    let tty = turbo_ty_from_type_expr_with_params(ty, enum_variants, type_params);
    turbo_ty_to_llvm_ctx(&tty, context, enum_max_slots)
}
