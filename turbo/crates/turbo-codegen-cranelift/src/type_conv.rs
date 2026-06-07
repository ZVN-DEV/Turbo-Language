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

// ── TypeExpr → Cranelift type resolution ──────────────────────────

pub(crate) fn resolve_cl_type(
    ty: &TypeExpr,
    ptr_type: types::Type,
    enum_variants: &HashMap<String, Vec<String>>,
    type_params: &[String],
) -> Result<types::Type, CodegenError> {
    resolve_cl_type_inner(ty, ptr_type, enum_variants, type_params, &HashMap::new())
}

fn resolve_cl_type_inner(
    ty: &TypeExpr,
    ptr_type: types::Type,
    enum_variants: &HashMap<String, Vec<String>>,
    type_params: &[String],
    enum_max_slots: &HashMap<String, usize>,
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
                "f32" => Ok(types::F32),
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
        ),
        #[allow(unreachable_patterns)]
        _ => Ok(types::I64),
    }
}
