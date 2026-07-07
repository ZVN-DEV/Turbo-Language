//! Hashmap built-ins.

use cranelift::prelude::*;
use cranelift_module::Module;
use turbo_ast::*;

use crate::turbo_types::{CodegenError, MaybeTyped, TurboTy};
use crate::{compile_expr, Ctx};

/// hashmap() -> HashMap (opaque pointer)
pub(crate) fn compile_builtin_hashmap<M: Module>(
    cx: &mut Ctx<'_, M>,
) -> Result<MaybeTyped, CodegenError> {
    let fid = cx.rt_fns["rt_hashmap_new"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Int)))
}

/// hashmap_set(map, key, value) -> ()
pub(crate) fn compile_builtin_hashmap_set<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (map_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_builtin_hashmap_set: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let (key_val, _) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_builtin_hashmap_set: `&args[1]` produced no value during code generation"
            .to_string(),
    })?;
    let (value_val, _) = compile_expr(cx, &args[2])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_builtin_hashmap_set: `&args[2]` produced no value during code generation"
            .to_string(),
    })?;
    let fid = cx.rt_fns["rt_hashmap_set"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    cx.builder.ins().call(fref, &[map_val, key_val, value_val]);
    Ok(None)
}

/// hashmap_get(map, key) -> str
pub(crate) fn compile_builtin_hashmap_get<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (map_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_builtin_hashmap_get: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let (key_val, _) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_builtin_hashmap_get: `&args[1]` produced no value during code generation"
            .to_string(),
    })?;
    let fid = cx.rt_fns["rt_hashmap_get"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[map_val, key_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Str)))
}

/// hashmap_has(map, key) -> bool
pub(crate) fn compile_builtin_hashmap_has<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (map_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_builtin_hashmap_has: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let (key_val, _) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_builtin_hashmap_has: `&args[1]` produced no value during code generation"
            .to_string(),
    })?;
    let fid = cx.rt_fns["rt_hashmap_has"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[map_val, key_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Bool)))
}

/// hashmap_len(map) -> i64
pub(crate) fn compile_builtin_hashmap_len<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (map_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_builtin_hashmap_len: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let fid = cx.rt_fns["rt_hashmap_len"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[map_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Int)))
}

/// hashmap_keys(map) -> [str]
pub(crate) fn compile_builtin_hashmap_keys<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (map_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message:
            "compile_builtin_hashmap_keys: `&args[0]` produced no value during code generation"
                .to_string(),
    })?;
    let fid = cx.rt_fns["rt_hashmap_keys"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[map_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Array(Box::new(TurboTy::Str)))))
}

/// hashmap_remove(map, key) -> ()
pub(crate) fn compile_builtin_hashmap_remove<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (map_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message:
            "compile_builtin_hashmap_remove: `&args[0]` produced no value during code generation"
                .to_string(),
    })?;
    let (key_val, _) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message:
            "compile_builtin_hashmap_remove: `&args[1]` produced no value during code generation"
                .to_string(),
    })?;
    let fid = cx.rt_fns["rt_hashmap_remove"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    cx.builder.ins().call(fref, &[map_val, key_val]);
    Ok(None)
}

/// hashmap_set_int(map, key, value: int) -> hashmap
///
/// v0.8.0 "Safe Core" variant: stores an int value under a string key.
/// Returns the same map pointer so callers can write
/// `m = hashmap_set_int(m, k, v)` without a separate mutation rule.
pub(crate) fn compile_builtin_hashmap_set_int<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (map_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message:
            "compile_builtin_hashmap_set_int: `&args[0]` produced no value during code generation"
                .to_string(),
    })?;
    let (key_val, _) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message:
            "compile_builtin_hashmap_set_int: `&args[1]` produced no value during code generation"
                .to_string(),
    })?;
    let (value_val, _) = compile_expr(cx, &args[2])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message:
            "compile_builtin_hashmap_set_int: `&args[2]` produced no value during code generation"
                .to_string(),
    })?;
    let fid = cx.rt_fns["rt_hashmap_set_int"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[map_val, key_val, value_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Int)))
}

/// hashmap_get_int(map, key) -> int
///
/// Returns 0 on miss. If you need to distinguish missing from a stored
/// 0, guard with `hashmap_has(m, k)` first.
pub(crate) fn compile_builtin_hashmap_get_int<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (map_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message:
            "compile_builtin_hashmap_get_int: `&args[0]` produced no value during code generation"
                .to_string(),
    })?;
    let (key_val, _) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message:
            "compile_builtin_hashmap_get_int: `&args[1]` produced no value during code generation"
                .to_string(),
    })?;
    let fid = cx.rt_fns["rt_hashmap_get_int"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[map_val, key_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Int)))
}

/// hashmap_inc(map, key) -> int  /  hashmap_inc(map, key, delta) -> int
///
/// Fused str→int increment: a single hash + single probe that adds `delta`
/// (default 1) to the value at `key`, treating a missing key as 0, and returns
/// the new value. This is the fast path for word-count style counters, lowering
/// `count = hashmap_get_int(m, k); hashmap_set_int(m, k, count + 1)` (two
/// lookups) into one — the str→int counterpart of C's idiomatic `table[k]++`.
pub(crate) fn compile_builtin_hashmap_inc<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (map_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_builtin_hashmap_inc: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let (key_val, _) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_builtin_hashmap_inc: `&args[1]` produced no value during code generation"
            .to_string(),
    })?;
    // delta defaults to 1 when only (map, key) are supplied.
    let delta_val = if args.len() >= 3 {
        compile_expr(cx, &args[2])?
            .ok_or_else(|| CodegenError {
                code: ErrorCode::E0400,
                message:
                    "compile_builtin_hashmap_inc: `&args[2]` produced no value during code generation"
                        .to_string(),
            })?
            .0
    } else {
        cx.builder.ins().iconst(types::I64, 1)
    };
    let fid = cx.rt_fns["rt_hashmap_inc"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[map_val, key_val, delta_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Int)))
}

// ── Map literal compilation ────────────────────────────────────────
