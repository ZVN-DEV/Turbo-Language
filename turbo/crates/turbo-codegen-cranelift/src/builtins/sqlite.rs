//! SQLite built-ins: `sqlite_open`, `sqlite_exec`, `sqlite_prepare`,
//! `sqlite_bind_*`, `sqlite_step`, `sqlite_column_*`, `sqlite_finalize`,
//! `sqlite_error`, `sqlite_close`.
//!
//! Each function lowers a call to the matching `rt_sqlite_*` runtime function
//! (Rust twin in `runtime.rs` for the JIT; C twin in `turbo_rt_sqlite.c` for
//! AOT) and tags the result with the right [`TurboTy`]. Argument arity/types
//! are validated in sema (`check_builtin_sqlite`), so codegen simply compiles
//! and forwards every argument.

use cranelift::prelude::*;
use cranelift_module::Module;
use turbo_ast::*;

use crate::turbo_types::{CodegenError, MaybeTyped, TurboTy};
use crate::{compile_expr, Ctx};

/// Compile every argument to a Cranelift value and emit a call to `rt_name`,
/// tagging the (optional) result with `ret_ty`. Returns `Ok(None)` when
/// `ret_ty` is `None` (a unit-returning call).
fn emit_sqlite_call<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
    rt_name: &str,
    ret_ty: Option<TurboTy>,
) -> Result<MaybeTyped, CodegenError> {
    let mut vals = Vec::with_capacity(args.len());
    for arg in args {
        let (v, _) = compile_expr(cx, arg)?.ok_or_else(|| CodegenError {
            code: ErrorCode::E0400,
            message: format!("{rt_name}: argument produced no value during code generation"),
        })?;
        vals.push(v);
    }
    let fid = cx.rt_fns[rt_name];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &vals);
    match ret_ty {
        Some(ty) => {
            let result = cx.builder.inst_results(call)[0];
            Ok(Some((result, ty)))
        }
        None => Ok(None),
    }
}

/// `sqlite_open(path: str) -> i64 ! str`
pub(crate) fn compile_sqlite_open<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    emit_sqlite_call(
        cx,
        args,
        "rt_sqlite_open",
        Some(TurboTy::Result(
            Box::new(TurboTy::Int),
            Box::new(TurboTy::Str),
        )),
    )
}

/// `sqlite_exec(h: i64, sql: str) -> unit ! str`
pub(crate) fn compile_sqlite_exec<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    emit_sqlite_call(
        cx,
        args,
        "rt_sqlite_exec",
        Some(TurboTy::Result(
            Box::new(TurboTy::Unit),
            Box::new(TurboTy::Str),
        )),
    )
}

/// `sqlite_prepare(h: i64, sql: str) -> i64 ! str`
pub(crate) fn compile_sqlite_prepare<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    emit_sqlite_call(
        cx,
        args,
        "rt_sqlite_prepare",
        Some(TurboTy::Result(
            Box::new(TurboTy::Int),
            Box::new(TurboTy::Str),
        )),
    )
}

/// `sqlite_bind_int(stmt, idx, v) -> i64`
pub(crate) fn compile_sqlite_bind_int<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    emit_sqlite_call(cx, args, "rt_sqlite_bind_int", Some(TurboTy::Int))
}

/// `sqlite_bind_str(stmt, idx, s) -> i64`
pub(crate) fn compile_sqlite_bind_str<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    emit_sqlite_call(cx, args, "rt_sqlite_bind_str", Some(TurboTy::Int))
}

/// `sqlite_bind_float(stmt, idx, f) -> i64`
pub(crate) fn compile_sqlite_bind_float<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    emit_sqlite_call(cx, args, "rt_sqlite_bind_float", Some(TurboTy::Int))
}

/// `sqlite_step(stmt) -> i64`
pub(crate) fn compile_sqlite_step<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    emit_sqlite_call(cx, args, "rt_sqlite_step", Some(TurboTy::Int))
}

/// `sqlite_column_int(stmt, i) -> i64`
pub(crate) fn compile_sqlite_column_int<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    emit_sqlite_call(cx, args, "rt_sqlite_column_int", Some(TurboTy::Int))
}

/// `sqlite_column_str(stmt, i) -> str`
pub(crate) fn compile_sqlite_column_str<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    emit_sqlite_call(cx, args, "rt_sqlite_column_str", Some(TurboTy::Str))
}

/// `sqlite_column_float(stmt, i) -> f64`
pub(crate) fn compile_sqlite_column_float<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    emit_sqlite_call(cx, args, "rt_sqlite_column_float", Some(TurboTy::Float))
}

/// `sqlite_column_count(stmt) -> i64`
pub(crate) fn compile_sqlite_column_count<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    emit_sqlite_call(cx, args, "rt_sqlite_column_count", Some(TurboTy::Int))
}

/// `sqlite_finalize(stmt) -> i64`
pub(crate) fn compile_sqlite_finalize<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    emit_sqlite_call(cx, args, "rt_sqlite_finalize", Some(TurboTy::Int))
}

/// `sqlite_error(h) -> str`
pub(crate) fn compile_sqlite_error<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    emit_sqlite_call(cx, args, "rt_sqlite_error", Some(TurboTy::Str))
}

/// `sqlite_close(h) -> i64`
pub(crate) fn compile_sqlite_close<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    emit_sqlite_call(cx, args, "rt_sqlite_close", Some(TurboTy::Int))
}
