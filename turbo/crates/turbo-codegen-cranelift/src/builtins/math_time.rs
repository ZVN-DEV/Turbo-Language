//! Math and time built-ins.

use cranelift::prelude::*;
use cranelift_module::Module;
use turbo_ast::*;

use crate::turbo_types::{CodegenError, MaybeTyped, TurboTy};
use crate::{compile_expr, Ctx};

/// pow(base, exp) -> i64
pub(crate) fn compile_stdlib_pow<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (base_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_stdlib_pow: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let (exp_val, _) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_stdlib_pow: `&args[1]` produced no value during code generation"
            .to_string(),
    })?;
    // Ensure both are i64
    let base_ty = cx.builder.func.dfg.value_type(base_val);
    let base_val = if base_ty.bits() < 64 {
        cx.builder.ins().sextend(types::I64, base_val)
    } else {
        base_val
    };
    let exp_ty = cx.builder.func.dfg.value_type(exp_val);
    let exp_val = if exp_ty.bits() < 64 {
        cx.builder.ins().sextend(types::I64, exp_val)
    } else {
        exp_val
    };
    let fid = cx.rt_fns["rt_pow"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[base_val, exp_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Int)))
}

/// sqrt(x) -> f64
pub(crate) fn compile_stdlib_sqrt<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (x_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_stdlib_sqrt: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let fid = cx.rt_fns["rt_sqrt"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[x_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Float)))
}

// ── Math builtins ──────────────────────────────────────────────────

/// Generic helper: (f64)->i64 math builtins (floor, ceil, round)
pub(crate) fn compile_math_f64_to_i64<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
    rt_name: &str,
) -> Result<MaybeTyped, CodegenError> {
    let (x_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_math_f64_to_i64: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let fid = cx.rt_fns[rt_name];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[x_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Int)))
}

/// Generic helper: (f64)->f64 math builtins (sin, cos, tan, log, exp, etc.)
pub(crate) fn compile_math_f64_to_f64<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
    rt_name: &str,
) -> Result<MaybeTyped, CodegenError> {
    let (x_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_math_f64_to_f64: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let fid = cx.rt_fns[rt_name];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[x_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Float)))
}

/// time_now() -> f64
pub(crate) fn compile_time_now<M: Module>(cx: &mut Ctx<'_, M>) -> Result<MaybeTyped, CodegenError> {
    let fid = cx.rt_fns["rt_time_now"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Float)))
}

/// time_ms() -> i64
pub(crate) fn compile_time_ms<M: Module>(cx: &mut Ctx<'_, M>) -> Result<MaybeTyped, CodegenError> {
    let fid = cx.rt_fns["rt_time_ms"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Int)))
}

/// format_time(timestamp, format) -> str
pub(crate) fn compile_format_time<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (ts_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_format_time: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let (fmt_val, _) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_format_time: `&args[1]` produced no value during code generation"
            .to_string(),
    })?;
    let fid = cx.rt_fns["rt_format_time"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[ts_val, fmt_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Str)))
}
