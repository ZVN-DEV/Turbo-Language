//! Turbo-level type definitions used across the codegen crate.
//!
//! Contains `TurboTy`, `CodegenError`, type aliases, and conversion functions.

use cranelift::prelude::Value;
use std::collections::HashMap;
use turbo_ast::*;

#[derive(Debug)]
pub struct CodegenError {
    pub code: ErrorCode,
    pub message: String,
}

impl std::fmt::Display for CodegenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "codegen error: {}", self.message)
    }
}

impl std::error::Error for CodegenError {}

/// Turbo-level type tag — needed because on ARM64 ptr_type == I64,
/// so Cranelift IR types alone can't distinguish strings from ints.
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub(crate) enum TurboTy {
    I8,
    I16,
    Int,
    U8,
    U16,
    Float,
    Bool,
    Str,
    Unit,
    Array(Box<TurboTy>),
    Struct(String),
    /// Enum type: name of the enum. Unit-only enums are i64 tags; data enums are heap pointers.
    Enum(String),
    /// Function pointer: param types and return type
    Fn(Vec<TurboTy>, Box<TurboTy>),
    /// Result type (heap-allocated tagged union): ok_type, err_type
    Result(Box<TurboTy>, Box<TurboTy>),
    /// Optional type (heap-allocated tagged union): inner_type
    Optional(Box<TurboTy>),
    /// Future type: a spawned thread handle (pointer to JoinHandle)
    Future(Box<TurboTy>),
    /// Typed hash map `HashMap<K, V>` — an opaque runtime handle (pointer/i64)
    /// carrying its key and value types so codegen can pick key-kind hashing
    /// and retain/release the values.
    HashMap(Box<TurboTy>, Box<TurboTy>),
}

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
            // Type parameters use Int representation (I64/ptr sized)
            if type_params.contains(name) {
                return TurboTy::Int;
            }
            match name.as_str() {
                "i8" => TurboTy::I8,
                "i16" => TurboTy::I16,
                "u8" => TurboTy::U8,
                "u16" => TurboTy::U16,
                "int" | "i32" | "i64" | "u32" | "u64" | "usize" => TurboTy::Int,
                "float" | "f32" | "f64" => TurboTy::Float,
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
        TypeExpr::Array(inner) => {
            // Thread type_params so the element of a generic array like `[T]`
            // resolves to `Int` (the uniform type-param representation) rather
            // than being misread as `Struct("T")`. Getting this wrong makes
            // indexing return an integer typed as a struct pointer, which the
            // refcount/retain path then dereferences — a segfault.
            let inner_tty =
                turbo_ty_from_type_expr_with_params(&inner.node, enum_variants, type_params);
            TurboTy::Array(Box::new(inner_tty))
        }
        TypeExpr::FnType { params, ret } => {
            let param_tys: Vec<TurboTy> = params
                .iter()
                .map(|p| turbo_ty_from_type_expr_with_params(&p.node, enum_variants, type_params))
                .collect();
            let ret_ty = turbo_ty_from_type_expr_with_params(&ret.node, enum_variants, type_params);
            TurboTy::Fn(param_tys, Box::new(ret_ty))
        }
        TypeExpr::Result { ok_type, err_type } => {
            let ok_tty =
                turbo_ty_from_type_expr_with_params(&ok_type.node, enum_variants, type_params);
            let err_tty =
                turbo_ty_from_type_expr_with_params(&err_type.node, enum_variants, type_params);
            TurboTy::Result(Box::new(ok_tty), Box::new(err_tty))
        }
        TypeExpr::Optional(inner) => {
            let inner_tty =
                turbo_ty_from_type_expr_with_params(&inner.node, enum_variants, type_params);
            TurboTy::Optional(Box::new(inner_tty))
        }
        // Future<T> is a thread handle pointer (underlying value is i64/ptr)
        TypeExpr::Future(inner) => {
            let inner_tty =
                turbo_ty_from_type_expr_with_params(&inner.node, enum_variants, type_params);
            TurboTy::Future(Box::new(inner_tty))
        }
        TypeExpr::HashMap(k, v) => {
            let k_tty = turbo_ty_from_type_expr_with_params(&k.node, enum_variants, type_params);
            let v_tty = turbo_ty_from_type_expr_with_params(&v.node, enum_variants, type_params);
            TurboTy::HashMap(Box::new(k_tty), Box::new(v_tty))
        }
        #[allow(unreachable_patterns)]
        _ => TurboTy::Int,
    }
}

/// Compiled value with its Turbo type.
pub(crate) type Typed = (Value, TurboTy);
/// Optional compiled value (None = unit).
pub(crate) type MaybeTyped = Option<Typed>;
