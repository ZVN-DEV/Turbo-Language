//! Type resolution and coercion for the Cranelift codegen backend.
//!
//! Contains `turbo_ty_to_cl_type`, `coerce_value`, and `resolve_cl_type*`
//! helpers that translate Turbo-level types into Cranelift IR types and
//! insert narrowing/widening instructions when types need coercion.

use cranelift::prelude::*;
use cranelift_module::Module;
use std::collections::HashMap;
use turbo_ast::*;

use crate::turbo_types::*;
use crate::Ctx;

// ── TurboTy → Cranelift type conversion ───────────────────────────

/// Convert a TurboTy to a Cranelift types::Type
pub(crate) fn turbo_ty_to_cl_type(tty: &TurboTy, ptr_type: types::Type) -> types::Type {
    match tty {
        TurboTy::I8 | TurboTy::U8 => types::I8,
        TurboTy::I16 | TurboTy::U16 => types::I16,
        TurboTy::Int => types::I64,
        TurboTy::Float => types::F64,
        TurboTy::Bool => types::I8,
        TurboTy::Str => ptr_type,
        TurboTy::Unit => types::I64,   // should not happen, but fallback
        TurboTy::Fn(_, _) => ptr_type, // function pointers are pointers
        TurboTy::Array(_) => ptr_type,
        TurboTy::Struct(_) => ptr_type,
        TurboTy::Enum(_) => types::I64, // both unit (tag) and data (ptr) enums fit in I64
        TurboTy::Result(_, _) => ptr_type,
        TurboTy::Optional(_) => ptr_type,
        TurboTy::Future(_) => ptr_type, // thread handle pointer
    }
}

/// Coerce a value from one TurboTy to another, inserting narrowing or widening
/// instructions as needed. Returns the coerced value and the target type.
pub(crate) fn coerce_value<M: Module>(
    cx: &mut Ctx<'_, M>,
    val: Value,
    from: &TurboTy,
    to: &TurboTy,
) -> (Value, TurboTy) {
    if from == to {
        return (val, from.clone());
    }
    match (from, to) {
        // Narrowing from Int (i64) to smaller types
        (TurboTy::Int, TurboTy::I8) | (TurboTy::Int, TurboTy::U8) => {
            (cx.builder.ins().ireduce(types::I8, val), to.clone())
        }
        (TurboTy::Int, TurboTy::I16) | (TurboTy::Int, TurboTy::U16) => {
            (cx.builder.ins().ireduce(types::I16, val), to.clone())
        }
        // Widening from smaller types to Int (i64) - sign-extend for signed
        (TurboTy::I8, TurboTy::Int) | (TurboTy::I16, TurboTy::Int) => {
            (cx.builder.ins().sextend(types::I64, val), TurboTy::Int)
        }
        // Widening from smaller types to Int (i64) - zero-extend for unsigned
        (TurboTy::U8, TurboTy::Int) | (TurboTy::U16, TurboTy::Int) => {
            (cx.builder.ins().uextend(types::I64, val), TurboTy::Int)
        }
        // Array-to-Array: runtime representation is identical (pointer to a
        // length-prefixed heap block of 8-byte slots). Trust the declared
        // element type from the annotation. In practice this path only fires
        // when the value is an empty `ArrayLit([])` whose element type
        // codegen couldn't infer — sema catches any real element-type
        // mismatches before we get here. See ISSUES.md Issue #1.
        (TurboTy::Array(_), TurboTy::Array(_)) => (val, to.clone()),
        // No coercion available / same size
        _ => (val, from.clone()),
    }
}

/// Whether a `TurboTy` is one of the scalar numeric tags an `as` cast operates
/// on. (`Bool` is *not* numeric here — sema rejects `bool as int`.)
pub(crate) fn is_numeric_tty(tty: &TurboTy) -> bool {
    matches!(
        tty,
        TurboTy::I8 | TurboTy::I16 | TurboTy::U8 | TurboTy::U16 | TurboTy::Int | TurboTy::Float
    )
}

/// Whether a target type expression names an unsigned integer. Used to pick the
/// signed vs unsigned float→int conversion: `i32`/`i64` and the `Int` tag default
/// to signed, while `u8…u64`/`usize` use the unsigned conversion. This matters
/// only for `float as u32`/`u64`, since those widths ride the uniform 64-bit
/// `Int` slot and so can't be told apart by `TurboTy` alone.
pub(crate) fn type_expr_is_unsigned(te: &TypeExpr) -> bool {
    matches!(
        te,
        TypeExpr::Named(name)
            if matches!(name.as_str(), "u8" | "u16" | "u32" | "u64" | "usize")
    )
}

/// Lower a numeric `as` cast value from `from` to `to`. Both are expected to be
/// numeric `TurboTy`s (sema rejects everything else). Semantics:
///
/// * **int → int**: normalise the source to a 64-bit value (sign/zero-extending
///   per the source's signedness) then narrow to the target. Narrowing to
///   `i8`/`i16`/`u8`/`u16` wraps via `ireduce` (two's-complement truncation, so
///   `300 as u8 == 44`). `i32`/`u32`/`u64`/`i64` all share one 64-bit slot
///   internally (`TurboTy::Int`), so casts among them are identity at the IR
///   level — only the 8/16-bit types have a distinct width that can wrap.
/// * **int → float**: widen to i64 then `fcvt_from_sint` (unsigned narrow
///   sources are zero-extended first, so they convert correctly; only `u64`
///   values ≥ 2⁶³ would differ, an accepted edge of the uniform-i64 model).
/// * **float → int**: saturating `fcvt_to_{s,u}int_sat` (NaN → 0, out-of-range
///   clamps) then narrow to the target width. Truncates toward zero, so
///   `3.9 as i64 == 3`.
/// * **float → float**: identity — `f32` and `f64` share one F64 slot
///   internally (BL-3), so `as f32` is a no-op except at the C FFI boundary.
pub(crate) fn numeric_cast<M: Module>(
    cx: &mut Ctx<'_, M>,
    val: Value,
    from: &TurboTy,
    to: &TurboTy,
    to_unsigned: bool,
) -> (Value, TurboTy) {
    let from_is_float = matches!(from, TurboTy::Float);
    let to_is_float = matches!(to, TurboTy::Float);
    match (from_is_float, to_is_float) {
        (false, false) => {
            // int -> int: widen to i64, then narrow to the target width.
            let (i64v, _) = coerce_value(cx, val, from, &TurboTy::Int);
            coerce_value(cx, i64v, &TurboTy::Int, to)
        }
        (false, true) => {
            // int -> float
            let (i64v, _) = coerce_value(cx, val, from, &TurboTy::Int);
            let f = cx.builder.ins().fcvt_from_sint(types::F64, i64v);
            (f, TurboTy::Float)
        }
        (true, false) => {
            // float -> int (saturating), then narrow to the target width.
            let i64v = if to_unsigned {
                cx.builder.ins().fcvt_to_uint_sat(types::I64, val)
            } else {
                cx.builder.ins().fcvt_to_sint_sat(types::I64, val)
            };
            coerce_value(cx, i64v, &TurboTy::Int, to)
        }
        (true, true) => (val, TurboTy::Float),
    }
}

// ── TypeExpr → Cranelift type resolution ──────────────────────────

/// Resolve a Turbo `TypeExpr` to the Cranelift IR type used for *internal*
/// Turbo function signatures (regular fns, methods, closures, spawn thunks).
///
/// Note on `f32`: internally TurboLang represents every float — `f32` and
/// `f64`/`float` alike — as a single 64-bit `Float` slot (`turbo_ty_to_cl_type`
/// maps the `Float` tag to `types::F64`). Resolving `f32` to `types::F64` here
/// keeps the variable-declaration type, the block-param type, and the value
/// that flows through it in agreement. Mapping `f32 -> types::F32` (a real
/// 32-bit register class) while the rest of codegen moves the value as F64 is
/// exactly what made scalar `f32` params panic Cranelift ("declared type of
/// variable doesn't match type of value") and miscompile across the
/// spawn/closure ABI. For the C FFI boundary, where `f32` genuinely means a
/// 32-bit C `float`, use [`resolve_cl_type_ffi`] instead.
pub(crate) fn resolve_cl_type(
    ty: &TypeExpr,
    ptr_type: types::Type,
    enum_variants: &HashMap<String, Vec<String>>,
    type_params: &[String],
) -> Result<types::Type, CodegenError> {
    resolve_cl_type_inner(
        ty,
        ptr_type,
        enum_variants,
        type_params,
        &HashMap::new(),
        false,
    )
}

/// Like [`resolve_cl_type`], but for the C FFI boundary (`extern` declarations),
/// where `f32` must map to a real 32-bit `types::F32` to match the platform C
/// ABI for `float`.
pub(crate) fn resolve_cl_type_ffi(
    ty: &TypeExpr,
    ptr_type: types::Type,
    enum_variants: &HashMap<String, Vec<String>>,
    type_params: &[String],
) -> Result<types::Type, CodegenError> {
    resolve_cl_type_inner(
        ty,
        ptr_type,
        enum_variants,
        type_params,
        &HashMap::new(),
        true,
    )
}

fn resolve_cl_type_inner(
    ty: &TypeExpr,
    ptr_type: types::Type,
    enum_variants: &HashMap<String, Vec<String>>,
    type_params: &[String],
    enum_max_slots: &HashMap<String, usize>,
    ffi: bool,
) -> Result<types::Type, CodegenError> {
    match ty {
        TypeExpr::Named(name) => {
            // Type parameters are represented as I64 (same size as ptr on 64-bit)
            if type_params.contains(name) {
                return Ok(types::I64);
            }
            match name.as_str() {
                "i8" => Ok(types::I8),
                "i16" => Ok(types::I16),
                "i32" => Ok(types::I32),
                "int" | "i64" => Ok(types::I64),
                "u8" => Ok(types::I8),
                "u16" => Ok(types::I16),
                "u32" => Ok(types::I32),
                "u64" | "usize" => Ok(types::I64),
                // Internally `f32` rides the uniform 64-bit float slot; only the
                // C FFI boundary needs a real 32-bit register class.
                "f32" => Ok(if ffi { types::F32 } else { types::F64 }),
                "float" | "f64" => Ok(types::F64),
                "bool" => Ok(types::I8),
                "str" => Ok(ptr_type),
                _ => {
                    if enum_variants.contains_key(name.as_str()) {
                        // Data-carrying enums are heap-allocated pointers
                        if enum_max_slots.contains_key(name.as_str()) {
                            Ok(ptr_type)
                        } else {
                            Ok(types::I64) // unit-only enums are i64 tags
                        }
                    } else {
                        Ok(ptr_type) // Struct types are represented as pointers at runtime
                    }
                }
            }
        }
        TypeExpr::Unit => Err(CodegenError {
            code: ErrorCode::E0400,
            message: "unit type has no runtime representation".to_string(),
        }),
        TypeExpr::Array(_) => Ok(ptr_type), // Arrays are represented as pointers at runtime
        TypeExpr::FnType { .. } => Ok(ptr_type), // Function pointers are pointers
        TypeExpr::Result { .. } => Ok(ptr_type), // Result types are heap-allocated tagged unions
        TypeExpr::Optional(_) => Ok(ptr_type), // Optional types are heap-allocated tagged unions
        // Sprint 9: Future<T> compiles identically to T
        TypeExpr::Future(inner) => resolve_cl_type_inner(
            &inner.node,
            ptr_type,
            enum_variants,
            type_params,
            enum_max_slots,
            ffi,
        ),
        #[allow(unreachable_patterns)]
        _ => Ok(types::I64),
    }
}
