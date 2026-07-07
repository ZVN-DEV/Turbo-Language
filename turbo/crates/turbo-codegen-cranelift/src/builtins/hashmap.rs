//! Hashmap built-ins.

use cranelift::prelude::*;
use cranelift_module::Module;
use turbo_ast::*;

use crate::expr::is_rc_managed_type;
use crate::turbo_types::{CodegenError, MaybeTyped, TurboTy};
use crate::{compile_expr, Ctx};

/// A hashmap builtin sub-expression compiled to nothing (unit) — surface an
/// E0400 rather than panicking, keeping codegen within the panic budget.
fn no_value(what: &str) -> CodegenError {
    CodegenError {
        code: ErrorCode::E0400,
        message: format!("hashmap builtin: {what} produced no value during code generation"),
    }
}

/// Widen a compiled value into the uniform i64 map slot, mirroring how `Some(x)`
/// boxes a value: narrow ints sign-extend, floats move through the integer
/// register via bitcast, pointers pass through unchanged. The reverse
/// reconstruction happens when the `get` Optional is unwrapped against its
/// statically known inner type.
fn value_to_slot<M: Module>(cx: &mut Ctx<'_, M>, val: Value) -> Value {
    let vt = cx.builder.func.dfg.value_type(val);
    if vt.is_int() && vt.bits() < 64 {
        cx.builder.ins().sextend(types::I64, val)
    } else if vt.is_float() {
        cx.builder.ins().bitcast(types::I64, MemFlags::new(), val)
    } else {
        val
    }
}

/// Runtime key-kind descriptor: 0 = str (content hash), 1 = int (identity
/// hash). Sema restricts hashmap keys to int or str, so any non-`str` key type
/// here is an integer.
pub(crate) fn hashmap_key_kind(key_tty: &TurboTy) -> i64 {
    if matches!(key_tty, TurboTy::Str) {
        0
    } else {
        1
    }
}

/// hashmap() -> HashMap (opaque legacy str→str/str→int handle).
pub(crate) fn compile_builtin_hashmap<M: Module>(
    cx: &mut Ctx<'_, M>,
) -> Result<MaybeTyped, CodegenError> {
    let fid = cx.rt_fns["rt_hashmap_new"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Int)))
}

/// Emit a typed-map constructor `rt_hashmap_new_typed(key_kind, val_is_rc)`.
/// Used when a `hashmap()` call is bound to a `HashMap<K, V>` annotation.
pub(crate) fn compile_typed_hashmap_ctor<M: Module>(
    cx: &mut Ctx<'_, M>,
    key_tty: &TurboTy,
    val_tty: &TurboTy,
) -> Result<MaybeTyped, CodegenError> {
    let kk = hashmap_key_kind(key_tty);
    let rc = if is_rc_managed_type(cx, val_tty) {
        1
    } else {
        0
    };
    let key_kind = cx.builder.ins().iconst(types::I64, kk);
    let val_is_rc = cx.builder.ins().iconst(types::I64, rc);
    let fid = cx.rt_fns["rt_hashmap_new_typed"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[key_kind, val_is_rc]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((
        result,
        TurboTy::HashMap(Box::new(key_tty.clone()), Box::new(val_tty.clone())),
    )))
}

/// hashmap_set(map, key, value) -> ()
///
/// Typed `HashMap<K, V>` maps store the value in a uniform i64 slot via the
/// descriptor-aware `rt_hashmap_gset`; the legacy opaque handle keeps its
/// str→str `rt_hashmap_set`.
pub(crate) fn compile_builtin_hashmap_set<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (map_val, map_tty) =
        compile_expr(cx, &args[0])?.ok_or_else(|| no_value("hashmap_set map"))?;
    if matches!(map_tty, TurboTy::HashMap(_, _)) {
        let (key_val, _) =
            compile_expr(cx, &args[1])?.ok_or_else(|| no_value("hashmap_set key"))?;
        let key_slot = value_to_slot(cx, key_val);
        let (val_val, _) =
            compile_expr(cx, &args[2])?.ok_or_else(|| no_value("hashmap_set value"))?;
        let val_slot = value_to_slot(cx, val_val);
        let fid = cx.rt_fns["rt_hashmap_gset"];
        let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
        cx.builder.ins().call(fref, &[map_val, key_slot, val_slot]);
        return Ok(None);
    }
    let (key_val, _) = compile_expr(cx, &args[1])?.ok_or_else(|| no_value("hashmap_set key"))?;
    let (value_val, _) =
        compile_expr(cx, &args[2])?.ok_or_else(|| no_value("hashmap_set value"))?;
    let fid = cx.rt_fns["rt_hashmap_set"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    cx.builder.ins().call(fref, &[map_val, key_val, value_val]);
    Ok(None)
}

/// hashmap_get(map, key) -> V?  (typed) / str (legacy)
///
/// A typed map returns `Optional<V>` — `none` for a missing key — from
/// `rt_hashmap_gget`, which already retains any rc-heap value into the result.
/// The legacy handle keeps its raw str return (null → empty on a miss).
pub(crate) fn compile_builtin_hashmap_get<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (map_val, map_tty) =
        compile_expr(cx, &args[0])?.ok_or_else(|| no_value("hashmap_get map"))?;
    if let TurboTy::HashMap(_, v) = &map_tty {
        let val_tty = (**v).clone();
        let (key_val, _) =
            compile_expr(cx, &args[1])?.ok_or_else(|| no_value("hashmap_get key"))?;
        let key_slot = value_to_slot(cx, key_val);
        let fid = cx.rt_fns["rt_hashmap_gget"];
        let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
        let call = cx.builder.ins().call(fref, &[map_val, key_slot]);
        let result = cx.builder.inst_results(call)[0];
        return Ok(Some((result, TurboTy::Optional(Box::new(val_tty)))));
    }
    let (key_val, _) = compile_expr(cx, &args[1])?.ok_or_else(|| no_value("hashmap_get key"))?;
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
    let (map_val, map_tty) =
        compile_expr(cx, &args[0])?.ok_or_else(|| no_value("hashmap_has map"))?;
    let generic = matches!(map_tty, TurboTy::HashMap(_, _));
    let (key_val, _) = compile_expr(cx, &args[1])?.ok_or_else(|| no_value("hashmap_has key"))?;
    let (fname, key_arg) = if generic {
        ("rt_hashmap_ghas", value_to_slot(cx, key_val))
    } else {
        ("rt_hashmap_has", key_val)
    };
    let fid = cx.rt_fns[fname];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[map_val, key_arg]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Bool)))
}

/// hashmap_len(map) -> i64
pub(crate) fn compile_builtin_hashmap_len<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (map_val, map_tty) =
        compile_expr(cx, &args[0])?.ok_or_else(|| no_value("hashmap_len map"))?;
    // Typed maps are a distinct JIT pointee (GMap) from the legacy
    // HashMap<String,String>, so they need the descriptor-aware `glen`.
    let fname = if matches!(map_tty, TurboTy::HashMap(_, _)) {
        "rt_hashmap_glen"
    } else {
        "rt_hashmap_len"
    };
    let fid = cx.rt_fns[fname];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[map_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Int)))
}

/// hashmap_keys(map) -> [K]  (typed) / [str] (legacy)
pub(crate) fn compile_builtin_hashmap_keys<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (map_val, map_tty) =
        compile_expr(cx, &args[0])?.ok_or_else(|| no_value("hashmap_keys map"))?;
    let (fname, key_tty) = if let TurboTy::HashMap(k, _) = &map_tty {
        ("rt_hashmap_gkeys", (**k).clone())
    } else {
        ("rt_hashmap_keys", TurboTy::Str)
    };
    let fid = cx.rt_fns[fname];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[map_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Array(Box::new(key_tty)))))
}

/// hashmap_remove(map, key) -> ()
pub(crate) fn compile_builtin_hashmap_remove<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (map_val, map_tty) =
        compile_expr(cx, &args[0])?.ok_or_else(|| no_value("hashmap_remove map"))?;
    let generic = matches!(map_tty, TurboTy::HashMap(_, _));
    let (key_val, _) = compile_expr(cx, &args[1])?.ok_or_else(|| no_value("hashmap_remove key"))?;
    let (fname, key_arg) = if generic {
        ("rt_hashmap_gremove", value_to_slot(cx, key_val))
    } else {
        ("rt_hashmap_remove", key_val)
    };
    let fid = cx.rt_fns[fname];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    cx.builder.ins().call(fref, &[map_val, key_arg]);
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
