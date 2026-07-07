//! Concurrency built-ins: channels and mutexes.

use cranelift::prelude::*;
use cranelift_module::Module;
use turbo_ast::*;

use crate::turbo_types::{CodegenError, MaybeTyped, TurboTy};
use crate::{compile_expr, Ctx};

/// channel() -> Channel (pointer)
pub(crate) fn compile_builtin_channel<M: Module>(
    cx: &mut Ctx<'_, M>,
) -> Result<MaybeTyped, CodegenError> {
    let fid = cx.rt_fns["rt_channel_create"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Int)))
}

/// send(ch, value) -> ()
pub(crate) fn compile_builtin_send<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (ch_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_builtin_send: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let (value_val, _) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_builtin_send: `&args[1]` produced no value during code generation"
            .to_string(),
    })?;
    // Ensure value is i64
    let val_ty = cx.builder.func.dfg.value_type(value_val);
    let value_val = if val_ty.bits() < 64 {
        cx.builder.ins().sextend(types::I64, value_val)
    } else {
        value_val
    };
    let fid = cx.rt_fns["rt_channel_send"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    cx.builder.ins().call(fref, &[ch_val, value_val]);
    Ok(None)
}

/// recv(ch) -> i64
pub(crate) fn compile_builtin_recv<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (ch_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_builtin_recv: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let fid = cx.rt_fns["rt_channel_recv"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[ch_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Int)))
}

// ── Mutex builtins ──────────────────────────────────────────────────

/// mutex(value) -> Mutex (pointer)
pub(crate) fn compile_builtin_mutex<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (value_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_builtin_mutex: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    // Ensure value is i64
    let val_ty = cx.builder.func.dfg.value_type(value_val);
    let value_val = if val_ty.bits() < 64 {
        cx.builder.ins().sextend(types::I64, value_val)
    } else {
        value_val
    };
    let fid = cx.rt_fns["rt_mutex_create"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[value_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Int)))
}

/// mutex_get(m) -> i64
pub(crate) fn compile_builtin_mutex_get<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (m_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_builtin_mutex_get: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let fid = cx.rt_fns["rt_mutex_get"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[m_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Int)))
}

/// mutex_set(m, value) -> ()
pub(crate) fn compile_builtin_mutex_set<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (m_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_builtin_mutex_set: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let (value_val, _) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_builtin_mutex_set: `&args[1]` produced no value during code generation"
            .to_string(),
    })?;
    // Ensure value is i64
    let val_ty = cx.builder.func.dfg.value_type(value_val);
    let value_val = if val_ty.bits() < 64 {
        cx.builder.ins().sextend(types::I64, value_val)
    } else {
        value_val
    };
    let fid = cx.rt_fns["rt_mutex_set"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    cx.builder.ins().call(fref, &[m_val, value_val]);
    Ok(None)
}

/// mutex_update(m, closure) -> i64
/// Runs `closure(old)` under the lock, stores the result, and returns the new
/// value. The closure is `(int) -> int`; it executes inside `rt_mutex_update`
/// while the lock is held, so a read-modify-write is one atomic critical
/// section. We pass the closure exactly like `map`/`route` do: extract the
/// `fn_ptr` (offset 0) and `env_ptr` (offset 8) from the closure pair and hand
/// both to the runtime, which calls back into the closure.
pub(crate) fn compile_builtin_mutex_update<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (m_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message:
            "compile_builtin_mutex_update: `&args[0]` produced no value during code generation"
                .to_string(),
    })?;
    let (closure_ptr, _fn_tty) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message:
            "compile_builtin_mutex_update: `&args[1]` produced no value during code generation"
                .to_string(),
    })?;
    // Extract fn_ptr and env_ptr from the closure pair struct
    // (offset 0 = fn_ptr, offset 8 = env_ptr).
    let fn_ptr = cx
        .builder
        .ins()
        .load(cx.ptr_type, MemFlags::new(), closure_ptr, 0);
    let env_ptr = cx
        .builder
        .ins()
        .load(cx.ptr_type, MemFlags::new(), closure_ptr, 8);
    let fid = cx.rt_fns["rt_mutex_update"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[m_val, fn_ptr, env_ptr]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Int)))
}

// ── HashMap builtins ────────────────────────────────────────────────
