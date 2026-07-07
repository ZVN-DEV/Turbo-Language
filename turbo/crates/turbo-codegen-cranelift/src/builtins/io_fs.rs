//! IO and filesystem built-ins: stdin, file, path, and process-exec helpers.

use cranelift::prelude::*;
use cranelift_module::Module;
use turbo_ast::*;

use crate::turbo_types::{CodegenError, MaybeTyped, TurboTy};
use crate::{compile_expr, Ctx};

/// read_line() -> str
pub(crate) fn compile_stdlib_read_line<M: Module>(
    cx: &mut Ctx<'_, M>,
) -> Result<MaybeTyped, CodegenError> {
    let fid = cx.rt_fns["rt_read_line"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Str)))
}

/// read_file(path) -> str
pub(crate) fn compile_stdlib_read_file<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (path_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_stdlib_read_file: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let fid = cx.rt_fns["rt_read_file"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[path_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Str)))
}

/// write_file(path, content) -> ()
pub(crate) fn compile_stdlib_write_file<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (path_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_stdlib_write_file: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let (content_val, _) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_stdlib_write_file: `&args[1]` produced no value during code generation"
            .to_string(),
    })?;
    let fid = cx.rt_fns["rt_write_file"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    cx.builder.ins().call(fref, &[path_val, content_val]);
    Ok(None)
}

/// try_read_file(path) -> str ! str
///
/// Fallible counterpart to `read_file`. Returns `ok(contents)` on success
/// or `err(message)` on any I/O failure — never panics.
pub(crate) fn compile_stdlib_try_read_file<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (path_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message:
            "compile_stdlib_try_read_file: `&args[0]` produced no value during code generation"
                .to_string(),
    })?;
    let fid = cx.rt_fns["rt_try_read_file"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[path_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((
        result,
        TurboTy::Result(Box::new(TurboTy::Str), Box::new(TurboTy::Str)),
    )))
}

/// try_write_file(path, content) -> bool ! str
pub(crate) fn compile_stdlib_try_write_file<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (path_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message:
            "compile_stdlib_try_write_file: `&args[0]` produced no value during code generation"
                .to_string(),
    })?;
    let (content_val, _) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message:
            "compile_stdlib_try_write_file: `&args[1]` produced no value during code generation"
                .to_string(),
    })?;
    let fid = cx.rt_fns["rt_try_write_file"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[path_val, content_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((
        result,
        TurboTy::Result(Box::new(TurboTy::Bool), Box::new(TurboTy::Str)),
    )))
}

/// shell_exec(cmd) -> str
pub(crate) fn compile_stdlib_exec<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (cmd_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_stdlib_exec: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let fid = cx.rt_fns["rt_exec"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[cmd_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Str)))
}

/// env_get(name) -> str
pub(crate) fn compile_stdlib_env_get<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (name_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_stdlib_env_get: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let fid = cx.rt_fns["rt_env_get"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[name_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Str)))
}

/// file_exists(path) -> bool
pub(crate) fn compile_file_exists<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (path_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_file_exists: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let fid = cx.rt_fns["rt_file_exists"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[path_val]);
    let result = cx.builder.inst_results(call)[0];
    // Convert i64 to i8 bool
    let bool_val = cx.builder.ins().ireduce(types::I8, result);
    Ok(Some((bool_val, TurboTy::Bool)))
}

/// delete_file(path) -> bool
pub(crate) fn compile_delete_file<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (path_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_delete_file: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let fid = cx.rt_fns["rt_delete_file"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[path_val]);
    let result = cx.builder.inst_results(call)[0];
    let bool_val = cx.builder.ins().ireduce(types::I8, result);
    Ok(Some((bool_val, TurboTy::Bool)))
}

/// list_dir(path) -> [str]
pub(crate) fn compile_list_dir<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (path_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_list_dir: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let fid = cx.rt_fns["rt_list_dir"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[path_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Array(Box::new(TurboTy::Str)))))
}

/// mkdir(path) -> bool
pub(crate) fn compile_mkdir<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (path_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_mkdir: `&args[0]` produced no value during code generation".to_string(),
    })?;
    let fid = cx.rt_fns["rt_mkdir"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[path_val]);
    let result = cx.builder.inst_results(call)[0];
    let bool_val = cx.builder.ins().ireduce(types::I8, result);
    Ok(Some((bool_val, TurboTy::Bool)))
}

/// path_join(a, b) -> str
pub(crate) fn compile_path_join<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (a_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_path_join: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let (b_val, _) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_path_join: `&args[1]` produced no value during code generation"
            .to_string(),
    })?;
    let fid = cx.rt_fns["rt_path_join"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[a_val, b_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Str)))
}

/// path_dir(path) -> str
pub(crate) fn compile_path_str1<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
    rt_name: &str,
) -> Result<MaybeTyped, CodegenError> {
    let (path_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_path_str1: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let fid = cx.rt_fns[rt_name];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[path_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Str)))
}

// ── Collection builtins ──────────────────────────────────────────────
