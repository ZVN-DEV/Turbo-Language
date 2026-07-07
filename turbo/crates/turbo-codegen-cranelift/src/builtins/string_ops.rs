//! String built-ins: slicing, formatting, comparison, and value rendering.

use cranelift::prelude::*;
use cranelift_module::Module;
use turbo_ast::*;

use crate::turbo_types::{CodegenError, MaybeTyped, TurboTy};
use crate::{compile_expr, release_expr_temp_if_needed, Ctx};

/// split(s, sep) -> [str] — calls rt_str_split, returns Array(Str)
pub(crate) fn compile_stdlib_split<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (s_val, s_tty) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_stdlib_split: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let (sep_val, sep_tty) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_stdlib_split: `&args[1]` produced no value during code generation"
            .to_string(),
    })?;
    let fid = cx.rt_fns["rt_str_split"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[s_val, sep_val]);
    let result = cx.builder.inst_results(call)[0];
    release_expr_temp_if_needed(cx, s_val, &s_tty, &args[0]);
    release_expr_temp_if_needed(cx, sep_val, &sep_tty, &args[1]);
    Ok(Some((result, TurboTy::Array(Box::new(TurboTy::Str)))))
}

/// Generic helper for str->str builtins (trim, upper, lower)
pub(crate) fn compile_stdlib_str1<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
    rt_name: &str,
) -> Result<MaybeTyped, CodegenError> {
    let (s_val, s_tty) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_stdlib_str1: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let fid = cx.rt_fns[rt_name];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[s_val]);
    let result = cx.builder.inst_results(call)[0];
    release_expr_temp_if_needed(cx, s_val, &s_tty, &args[0]);
    Ok(Some((result, TurboTy::Str)))
}

/// Generic helper for (str, str)->bool builtins (starts_with, ends_with)
pub(crate) fn compile_stdlib_str_bool2<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
    rt_name: &str,
) -> Result<MaybeTyped, CodegenError> {
    let (s_val, s_tty) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_stdlib_str_bool2: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let (other_val, other_tty) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_stdlib_str_bool2: `&args[1]` produced no value during code generation"
            .to_string(),
    })?;
    let fid = cx.rt_fns[rt_name];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[s_val, other_val]);
    let result = cx.builder.inst_results(call)[0];
    release_expr_temp_if_needed(cx, s_val, &s_tty, &args[0]);
    release_expr_temp_if_needed(cx, other_val, &other_tty, &args[1]);
    Ok(Some((result, TurboTy::Bool)))
}

/// replace(s, from, to) -> str
pub(crate) fn compile_stdlib_replace<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (s_val, s_tty) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_stdlib_replace: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let (from_val, from_tty) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_stdlib_replace: `&args[1]` produced no value during code generation"
            .to_string(),
    })?;
    let (to_val, to_tty) = compile_expr(cx, &args[2])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_stdlib_replace: `&args[2]` produced no value during code generation"
            .to_string(),
    })?;
    let fid = cx.rt_fns["rt_str_replace"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[s_val, from_val, to_val]);
    let result = cx.builder.inst_results(call)[0];
    release_expr_temp_if_needed(cx, s_val, &s_tty, &args[0]);
    release_expr_temp_if_needed(cx, from_val, &from_tty, &args[1]);
    release_expr_temp_if_needed(cx, to_val, &to_tty, &args[2]);
    Ok(Some((result, TurboTy::Str)))
}

/// char_at(s, index) -> str
pub(crate) fn compile_stdlib_char_at<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (s_val, s_tty) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_stdlib_char_at: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let (idx_val, _) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_stdlib_char_at: `&args[1]` produced no value during code generation"
            .to_string(),
    })?;
    // Ensure index is i64
    let idx_ty = cx.builder.func.dfg.value_type(idx_val);
    let idx_val = if idx_ty.bits() < 64 {
        cx.builder.ins().sextend(types::I64, idx_val)
    } else {
        idx_val
    };
    let fid = cx.rt_fns["rt_str_char_at"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[s_val, idx_val]);
    let result = cx.builder.inst_results(call)[0];
    release_expr_temp_if_needed(cx, s_val, &s_tty, &args[0]);
    Ok(Some((result, TurboTy::Str)))
}

/// index_of(s, sub) -> i64
pub(crate) fn compile_stdlib_index_of<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (s_val, s_tty) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_stdlib_index_of: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let (sub_val, sub_tty) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_stdlib_index_of: `&args[1]` produced no value during code generation"
            .to_string(),
    })?;
    let fid = cx.rt_fns["rt_str_index_of"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[s_val, sub_val]);
    let result = cx.builder.inst_results(call)[0];
    release_expr_temp_if_needed(cx, s_val, &s_tty, &args[0]);
    release_expr_temp_if_needed(cx, sub_val, &sub_tty, &args[1]);
    Ok(Some((result, TurboTy::Int)))
}

/// join(arr, sep) -> str
pub(crate) fn compile_stdlib_join<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (arr_val, arr_tty) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_stdlib_join: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let (sep_val, sep_tty) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_stdlib_join: `&args[1]` produced no value during code generation"
            .to_string(),
    })?;
    let fid = cx.rt_fns["rt_str_join"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[arr_val, sep_val]);
    let result = cx.builder.inst_results(call)[0];
    release_expr_temp_if_needed(cx, arr_val, &arr_tty, &args[0]);
    release_expr_temp_if_needed(cx, sep_val, &sep_tty, &args[1]);
    Ok(Some((result, TurboTy::Str)))
}

/// repeat(s, n) -> str
pub(crate) fn compile_stdlib_repeat<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (s_val, s_tty) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_stdlib_repeat: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let (n_val, _) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_stdlib_repeat: `&args[1]` produced no value during code generation"
            .to_string(),
    })?;
    // Ensure n is i64
    let n_ty = cx.builder.func.dfg.value_type(n_val);
    let n_val = if n_ty.bits() < 64 {
        cx.builder.ins().sextend(types::I64, n_val)
    } else {
        n_val
    };
    let fid = cx.rt_fns["rt_str_repeat"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[s_val, n_val]);
    let result = cx.builder.inst_results(call)[0];
    release_expr_temp_if_needed(cx, s_val, &s_tty, &args[0]);
    Ok(Some((result, TurboTy::Str)))
}

/// substring(s, start, end) -> str
pub(crate) fn compile_substring<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (s_val, s_tty) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_substring: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let (start_val, _) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_substring: `&args[1]` produced no value during code generation"
            .to_string(),
    })?;
    let (end_val, _) = compile_expr(cx, &args[2])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_substring: `&args[2]` produced no value during code generation"
            .to_string(),
    })?;
    let fid = cx.rt_fns["rt_substring"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[s_val, start_val, end_val]);
    let result = cx.builder.inst_results(call)[0];
    release_expr_temp_if_needed(cx, s_val, &s_tty, &args[0]);
    Ok(Some((result, TurboTy::Str)))
}

/// pad_left(s, width, char) -> str
pub(crate) fn compile_pad_left<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (s_val, s_tty) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_pad_left: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let (width_val, _) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_pad_left: `&args[1]` produced no value during code generation"
            .to_string(),
    })?;
    let (char_val, char_tty) = compile_expr(cx, &args[2])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_pad_left: `&args[2]` produced no value during code generation"
            .to_string(),
    })?;
    let fid = cx.rt_fns["rt_pad_left"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[s_val, width_val, char_val]);
    let result = cx.builder.inst_results(call)[0];
    release_expr_temp_if_needed(cx, s_val, &s_tty, &args[0]);
    release_expr_temp_if_needed(cx, char_val, &char_tty, &args[2]);
    Ok(Some((result, TurboTy::Str)))
}

/// pad_right(s, width, char) -> str
pub(crate) fn compile_pad_right<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (s_val, s_tty) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_pad_right: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let (width_val, _) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_pad_right: `&args[1]` produced no value during code generation"
            .to_string(),
    })?;
    let (char_val, char_tty) = compile_expr(cx, &args[2])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_pad_right: `&args[2]` produced no value during code generation"
            .to_string(),
    })?;
    let fid = cx.rt_fns["rt_pad_right"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[s_val, width_val, char_val]);
    let result = cx.builder.inst_results(call)[0];
    release_expr_temp_if_needed(cx, s_val, &s_tty, &args[0]);
    release_expr_temp_if_needed(cx, char_val, &char_tty, &args[2]);
    Ok(Some((result, TurboTy::Str)))
}

/// str_to_int(s) -> i64 ! str
pub(crate) fn compile_str_to_int<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (s_val, s_tty) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_str_to_int: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let fid = cx.rt_fns["rt_str_to_int"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[s_val]);
    let result = cx.builder.inst_results(call)[0];
    release_expr_temp_if_needed(cx, s_val, &s_tty, &args[0]);
    Ok(Some((
        result,
        TurboTy::Result(Box::new(TurboTy::Int), Box::new(TurboTy::Str)),
    )))
}

/// str_to_float(s) -> f64 ! str
pub(crate) fn compile_str_to_float<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (s_val, s_tty) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_str_to_float: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let fid = cx.rt_fns["rt_str_to_float"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[s_val]);
    let result = cx.builder.inst_results(call)[0];
    release_expr_temp_if_needed(cx, s_val, &s_tty, &args[0]);
    Ok(Some((
        result,
        TurboTy::Result(Box::new(TurboTy::Float), Box::new(TurboTy::Str)),
    )))
}

pub(crate) fn compile_str_concat<M: Module>(
    cx: &mut Ctx<'_, M>,
    lhs: Value,
    rhs: Value,
) -> Result<MaybeTyped, CodegenError> {
    let fid = cx.rt_fns["rt_str_concat"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[lhs, rhs]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Str)))
}

pub(crate) fn compile_str_compare<M: Module>(
    cx: &mut Ctx<'_, M>,
    lhs: Value,
    rhs: Value,
    op: BinOp,
) -> Result<MaybeTyped, CodegenError> {
    let fid = cx.rt_fns["rt_str_eq"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[lhs, rhs]);
    let result = cx.builder.inst_results(call)[0];
    let result = if op == BinOp::NotEq {
        let one = cx.builder.ins().iconst(types::I8, 1);
        cx.builder.ins().bxor(result, one)
    } else {
        result
    };
    Ok(Some((result, TurboTy::Bool)))
}

// ── Struct field-by-field equality (@derive(Eq)) ────────────────────

pub(crate) fn compile_struct_eq<M: Module>(
    cx: &mut Ctx<'_, M>,
    lhs_ptr: Value,
    rhs_ptr: Value,
    struct_name: &str,
    op: BinOp,
) -> Result<MaybeTyped, CodegenError> {
    let struct_layout = cx
        .struct_fields
        .get(struct_name)
        .ok_or_else(|| CodegenError {
            code: ErrorCode::E0400,
            message: format!("undefined struct: {struct_name}"),
        })?
        .clone();

    if struct_layout.is_empty() {
        // No fields: always equal
        let result = if op == BinOp::Eq {
            cx.builder.ins().iconst(types::I8, 1)
        } else {
            cx.builder.ins().iconst(types::I8, 0)
        };
        return Ok(Some((result, TurboTy::Bool)));
    }

    // Compare field by field, short-circuiting on first mismatch
    // We use a chain of basic blocks: for each field, if mismatch -> result false, else -> check next
    let merge_block = cx.builder.create_block();
    cx.builder.append_block_param(merge_block, types::I8);

    for (i, (_, field_tty)) in struct_layout.iter().enumerate() {
        let offset = (i * 8) as i32;

        let lhs_raw = cx
            .builder
            .ins()
            .load(types::I64, MemFlags::new(), lhs_ptr, offset);
        let rhs_raw = cx
            .builder
            .ins()
            .load(types::I64, MemFlags::new(), rhs_ptr, offset);

        let fields_eq = match field_tty {
            TurboTy::Str => {
                // Use rt_str_eq for string fields
                let fid = cx.rt_fns["rt_str_eq"];
                let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
                let call = cx.builder.ins().call(fref, &[lhs_raw, rhs_raw]);
                cx.builder.inst_results(call)[0]
            }
            TurboTy::Float => {
                // Bitcast back to f64 and compare
                let lhs_f = cx
                    .builder
                    .ins()
                    .bitcast(types::F64, MemFlags::new(), lhs_raw);
                let rhs_f = cx
                    .builder
                    .ins()
                    .bitcast(types::F64, MemFlags::new(), rhs_raw);
                cx.builder.ins().fcmp(FloatCC::Equal, lhs_f, rhs_f)
            }
            TurboTy::Bool => {
                // Compare the raw i64 values (booleans are stored widened to i64)
                cx.builder.ins().icmp(IntCC::Equal, lhs_raw, rhs_raw)
            }
            _ => {
                // Int, Enum, Struct (pointer equality for nested structs without derive)
                cx.builder.ins().icmp(IntCC::Equal, lhs_raw, rhs_raw)
            }
        };

        if i < struct_layout.len() - 1 {
            // Not the last field: if mismatch, jump to merge with false; else continue
            let next_block = cx.builder.create_block();
            let false_val = cx.builder.ins().iconst(types::I8, 0);
            cx.builder
                .ins()
                .brif(fields_eq, next_block, &[], merge_block, &[false_val]);
            cx.builder.switch_to_block(next_block);
            cx.builder.seal_block(next_block);
        } else {
            // Last field: jump to merge with the comparison result
            cx.builder.ins().jump(merge_block, &[fields_eq]);
        }
    }

    cx.builder.switch_to_block(merge_block);
    cx.builder.seal_block(merge_block);

    let result = cx.builder.block_params(merge_block)[0];
    let result = if op == BinOp::NotEq {
        let one = cx.builder.ins().iconst(types::I8, 1);
        cx.builder.ins().bxor(result, one)
    } else {
        result
    };
    Ok(Some((result, TurboTy::Bool)))
}

// ── Struct clone (@derive(Clone)) ───────────────────────────────────

pub(crate) fn compile_clone<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (src_ptr, src_tty) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_clone: `&args[0]` produced no value during code generation".to_string(),
    })?;

    let struct_name = match &src_tty {
        TurboTy::Struct(name) => name.clone(),
        _ => {
            return Err(CodegenError {
                code: ErrorCode::E0400,
                message: "clone() expects a struct argument".to_string(),
            })
        }
    };

    let struct_layout = cx
        .struct_fields
        .get(&struct_name)
        .ok_or_else(|| CodegenError {
            code: ErrorCode::E0400,
            message: format!("undefined struct: {struct_name}"),
        })?
        .clone();

    let num_fields = struct_layout.len() as i64;
    let num_fields_val = cx.builder.ins().iconst(types::I64, num_fields);

    // Allocate a new struct
    let alloc_fid = cx.rt_fns["rt_struct_alloc"];
    let alloc_fref = cx.module.declare_func_in_func(alloc_fid, cx.builder.func);
    let call = cx.builder.ins().call(alloc_fref, &[num_fields_val]);
    let new_ptr = cx.builder.inst_results(call)[0];

    // Copy each field from source to destination
    for (i, (_field_name, _field_tty)) in struct_layout.iter().enumerate() {
        let offset = (i * 8) as i32;
        let val = cx
            .builder
            .ins()
            .load(types::I64, MemFlags::new(), src_ptr, offset);
        cx.builder
            .ins()
            .store(MemFlags::new(), val, new_ptr, offset);
    }

    Ok(Some((new_ptr, TurboTy::Struct(struct_name))))
}

// ── String interpolation ────────────────────────────────────────────

pub(crate) fn compile_interpolation<M: Module>(
    cx: &mut Ctx<'_, M>,
    parts: &[turbo_ast::InterpolPart],
) -> Result<MaybeTyped, CodegenError> {
    let mut result: Option<Value> = None;

    for part in parts {
        let part_str = match part {
            turbo_ast::InterpolPart::Lit(s) => cx.create_string(s)?,
            turbo_ast::InterpolPart::Expr(expr) => {
                let (val, tty) = compile_expr(cx, expr)?.ok_or_else(|| CodegenError {
                    code: ErrorCode::E0400,
                    message: "cannot interpolate a unit value: expression produces no value"
                        .to_string(),
                })?;
                convert_to_str(cx, val, &tty)?
            }
        };

        result = Some(match result {
            None => part_str,
            Some(acc) => {
                let fid = cx.rt_fns["rt_str_concat"];
                let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
                let call = cx.builder.ins().call(fref, &[acc, part_str]);
                cx.builder.inst_results(call)[0]
            }
        });
    }

    match result {
        Some(val) => Ok(Some((val, TurboTy::Str))),
        None => {
            let ptr = cx.create_string("")?;
            Ok(Some((ptr, TurboTy::Str)))
        }
    }
}

pub(crate) fn convert_to_str<M: Module>(
    cx: &mut Ctx<'_, M>,
    val: Value,
    tty: &TurboTy,
) -> Result<Value, CodegenError> {
    match tty {
        TurboTy::Str => Ok(val),
        TurboTy::I8 | TurboTy::I16 => {
            // Sign-extend to i64 for string conversion
            let val = cx.builder.ins().sextend(types::I64, val);
            let fid = cx.rt_fns["rt_i64_to_str"];
            let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
            let call = cx.builder.ins().call(fref, &[val]);
            Ok(cx.builder.inst_results(call)[0])
        }
        TurboTy::U8 | TurboTy::U16 => {
            // Zero-extend to i64 for string conversion
            let val = cx.builder.ins().uextend(types::I64, val);
            let fid = cx.rt_fns["rt_i64_to_str"];
            let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
            let call = cx.builder.ins().call(fref, &[val]);
            Ok(cx.builder.inst_results(call)[0])
        }
        TurboTy::Int => {
            let ty = cx.builder.func.dfg.value_type(val);
            let val = if ty.bits() < 64 {
                cx.builder.ins().sextend(types::I64, val)
            } else {
                val
            };
            let fid = cx.rt_fns["rt_i64_to_str"];
            let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
            let call = cx.builder.ins().call(fref, &[val]);
            Ok(cx.builder.inst_results(call)[0])
        }
        TurboTy::Float => {
            let fid = cx.rt_fns["rt_f64_to_str"];
            let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
            let call = cx.builder.ins().call(fref, &[val]);
            Ok(cx.builder.inst_results(call)[0])
        }
        TurboTy::Bool => {
            let fid = cx.rt_fns["rt_bool_to_str"];
            let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
            let call = cx.builder.ins().call(fref, &[val]);
            Ok(cx.builder.inst_results(call)[0])
        }
        TurboTy::Unit => cx.create_string("()"),
        TurboTy::Enum(ref enum_name) => {
            let tag_val = if cx.enum_max_slots.contains_key(enum_name.as_str()) {
                cx.builder.ins().load(types::I64, MemFlags::new(), val, 0)
            } else {
                let val = if cx.builder.func.dfg.value_type(val).bits() < 64 {
                    cx.builder.ins().sextend(types::I64, val)
                } else {
                    val
                };
                val
            };
            let fid = cx.rt_fns["rt_i64_to_str"];
            let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
            let call = cx.builder.ins().call(fref, &[tag_val]);
            Ok(cx.builder.inst_results(call)[0])
        }
        TurboTy::Array(ref elem) => render_array(cx, val, elem),
        TurboTy::Struct(ref name) => render_struct(cx, val, name),
        TurboTy::Fn(_, _) => cx.create_string("[function]"),
        TurboTy::Result(ref ok_ty, ref err_ty) => render_result(cx, val, ok_ty, err_ty),
        TurboTy::Optional(ref inner) => {
            // Return "some(<value>)" or "none" string based on the tag
            let tag_fid = cx.rt_fns["rt_option_tag"];
            let tag_fref = cx.module.declare_func_in_func(tag_fid, cx.builder.func);
            let tag_call = cx.builder.ins().call(tag_fref, &[val]);
            let tag = cx.builder.inst_results(tag_call)[0];

            let one = cx.builder.ins().iconst(types::I64, 1);
            let is_some = cx.builder.ins().icmp(IntCC::Equal, tag, one);

            let some_block = cx.builder.create_block();
            let none_block = cx.builder.create_block();
            let merge_block = cx.builder.create_block();

            cx.builder
                .ins()
                .brif(is_some, some_block, &[], none_block, &[]);

            // Some path
            cx.builder.switch_to_block(some_block);
            cx.builder.seal_block(some_block);
            let val_fid = cx.rt_fns["rt_option_value"];
            let val_fref = cx.module.declare_func_in_func(val_fid, cx.builder.func);
            let val_call = cx.builder.ins().call(val_fref, &[val]);
            let inner_val = cx.builder.inst_results(val_call)[0];
            // The payload arrives as a uniform i64 slot; `render_slot`
            // reinterprets it (float bits / bool byte / etc.) before rendering.
            let inner_str = render_slot(cx, inner_val, inner)?;
            let prefix = cx.create_string("some(")?;
            let suffix = cx.create_string(")")?;
            let concat_fid = cx.rt_fns["rt_str_concat"];
            let concat_fref = cx.module.declare_func_in_func(concat_fid, cx.builder.func);
            let call1 = cx.builder.ins().call(concat_fref, &[prefix, inner_str]);
            let partial = cx.builder.inst_results(call1)[0];
            let call2 = cx.builder.ins().call(concat_fref, &[partial, suffix]);
            let some_str = cx.builder.inst_results(call2)[0];
            cx.builder.ins().jump(merge_block, &[some_str]);

            // None path
            cx.builder.switch_to_block(none_block);
            cx.builder.seal_block(none_block);
            let none_str = cx.create_string("none")?;
            cx.builder.ins().jump(merge_block, &[none_str]);

            // Merge
            cx.builder.append_block_param(merge_block, cx.ptr_type);
            cx.builder.switch_to_block(merge_block);
            cx.builder.seal_block(merge_block);
            Ok(cx.builder.block_params(merge_block)[0])
        }
        TurboTy::Future(_) => cx.create_string("[future]"),
    }
}

/// Emit a `rt_str_concat(a, b)` call and return the resulting string value.
fn str_concat<M: Module>(cx: &mut Ctx<'_, M>, a: Value, b: Value) -> Value {
    let fid = cx.rt_fns["rt_str_concat"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[a, b]);
    cx.builder.inst_results(call)[0]
}

/// Render one value held in a uniform 8-byte slot (array element, struct
/// field, or result/optional payload) to a string.
///
/// Compound containers store every element as a raw i64 (pointers and ints
/// directly, floats and bools as bit patterns). This mirrors the exact
/// reinterpretation `Expr::Index` / `Expr::FieldAccess` perform on read so the
/// rendered value matches what the program would observe, then defers to the
/// recursive `convert_to_str`.
fn render_slot<M: Module>(
    cx: &mut Ctx<'_, M>,
    raw: Value,
    tty: &TurboTy,
) -> Result<Value, CodegenError> {
    let (val, render_tty) = match tty {
        TurboTy::Bool => (cx.builder.ins().ireduce(types::I8, raw), TurboTy::Bool),
        TurboTy::Float => (
            cx.builder.ins().bitcast(types::F64, MemFlags::new(), raw),
            TurboTy::Float,
        ),
        // Narrow ints are sign/zero-extended into the full i64 slot on store,
        // so render them as a plain integer (avoids `convert_to_str`'s
        // sextend, which would reject the already-64-bit value).
        TurboTy::I8 | TurboTy::I16 | TurboTy::U8 | TurboTy::U16 => (raw, TurboTy::Int),
        other => (raw, other.clone()),
    };
    convert_to_str(cx, val, &render_tty)
}

/// Render an array value as `[e0, e1, …]` (empty arrays render as `[]`).
///
/// Walks the array at runtime — the length is dynamic — building the string
/// with a Cranelift loop over `[len][elem0]…`. Each element is rendered via
/// `render_slot`, so element types (including nested arrays/structs) recurse
/// through `convert_to_str`. Strings are rendered unquoted, matching how
/// optionals render `some("hi")` as `some(hi)`.
fn render_array<M: Module>(
    cx: &mut Ctx<'_, M>,
    arr: Value,
    elem_tty: &TurboTy,
) -> Result<Value, CodegenError> {
    // Length lives in the first i64 slot of the array data.
    let len = cx
        .builder
        .ins()
        .load(types::I64, MemFlags::trusted(), arr, 0i32);

    // Accumulator string variable, seeded with the opening bracket.
    let acc_var = Variable::new(cx.next_var);
    cx.next_var += 1;
    cx.builder.declare_var(acc_var, cx.ptr_type);
    let open = cx.create_string("[")?;
    cx.builder.def_var(acc_var, open);

    // Index counter.
    let idx_var = Variable::new(cx.next_var);
    cx.next_var += 1;
    cx.builder.declare_var(idx_var, types::I64);
    let zero = cx.builder.ins().iconst(types::I64, 0);
    cx.builder.def_var(idx_var, zero);

    let header = cx.builder.create_block();
    let body = cx.builder.create_block();
    let sep_block = cx.builder.create_block();
    let elem_block = cx.builder.create_block();
    let cont = cx.builder.create_block();
    let exit = cx.builder.create_block();

    cx.builder.ins().jump(header, &[]);

    // Header: while idx < len. Not sealed yet (back-edge from `cont`).
    cx.builder.switch_to_block(header);
    let i = cx.builder.use_var(idx_var);
    let cond = cx.builder.ins().icmp(IntCC::SignedLessThan, i, len);
    cx.builder.ins().brif(cond, body, &[], exit, &[]);

    // Body: prepend ", " for every element after the first.
    cx.builder.switch_to_block(body);
    cx.builder.seal_block(body);
    let i = cx.builder.use_var(idx_var);
    let is_first = cx.builder.ins().icmp(IntCC::Equal, i, zero);
    cx.builder
        .ins()
        .brif(is_first, elem_block, &[], sep_block, &[]);

    // Separator path.
    cx.builder.switch_to_block(sep_block);
    cx.builder.seal_block(sep_block);
    let acc = cx.builder.use_var(acc_var);
    let sep = cx.create_string(", ")?;
    let acc = str_concat(cx, acc, sep);
    cx.builder.def_var(acc_var, acc);
    cx.builder.ins().jump(elem_block, &[]);

    // Element path: load slot, render, append.
    cx.builder.switch_to_block(elem_block);
    cx.builder.seal_block(elem_block);
    let i = cx.builder.use_var(idx_var);
    let data_base = cx.builder.ins().iadd_imm(arr, 8);
    let byte_off = cx.builder.ins().ishl_imm(i, 3);
    let elem_ptr = cx.builder.ins().iadd(data_base, byte_off);
    let raw = cx
        .builder
        .ins()
        .load(types::I64, MemFlags::trusted(), elem_ptr, 0i32);
    let elem_str = render_slot(cx, raw, elem_tty)?;
    // `render_slot` may have created and switched blocks (nested compounds);
    // re-read the accumulator through its variable and continue here.
    let acc = cx.builder.use_var(acc_var);
    let acc = str_concat(cx, acc, elem_str);
    cx.builder.def_var(acc_var, acc);
    cx.builder.ins().jump(cont, &[]);

    // Continue: idx += 1, back to header.
    cx.builder.switch_to_block(cont);
    cx.builder.seal_block(cont);
    let i = cx.builder.use_var(idx_var);
    let one = cx.builder.ins().iconst(types::I64, 1);
    let next = cx.builder.ins().iadd(i, one);
    cx.builder.def_var(idx_var, next);
    cx.builder.ins().jump(header, &[]);

    // All predecessors of `header` are now known.
    cx.builder.seal_block(header);

    // Exit: append the closing bracket.
    cx.builder.switch_to_block(exit);
    cx.builder.seal_block(exit);
    let acc = cx.builder.use_var(acc_var);
    let close = cx.create_string("]")?;
    Ok(str_concat(cx, acc, close))
}

/// Render a struct value as `Name { field0: v0, field1: v1 }`
/// (`Name {}` when it has no fields).
///
/// The field count and layout are static, so the field walk is unrolled at
/// codegen time. A `@derive(Display)` / Display-impl struct still renders via
/// its `to_string`. The braces-with-spaces form is intentionally distinct from
/// `to_json`'s `{"field":v}`. String fields are rendered unquoted, matching the
/// optional `some(hi)` convention.
fn render_struct<M: Module>(
    cx: &mut Ctx<'_, M>,
    ptr: Value,
    name: &str,
) -> Result<Value, CodegenError> {
    // Honor a Display impl (user-defined or @derive(Display)) first.
    let has_display = cx
        .trait_impls
        .get(name)
        .is_some_and(|traits| traits.contains(&"Display".to_string()));
    if has_display {
        let mangled = format!("{name}__to_string");
        if let Some(&fid) = cx.user_fns.get(&mangled) {
            let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
            let call = cx.builder.ins().call(fref, &[ptr]);
            return Ok(cx.builder.inst_results(call)[0]);
        }
    }

    let fields = match cx.struct_fields.get(name) {
        Some(f) => f.clone(),
        // Unknown struct layout: fall back to the legacy placeholder rather
        // than emitting a wrong value.
        None => return cx.create_string(&format!("[struct {name}]")),
    };

    if fields.is_empty() {
        return cx.create_string(&format!("{name} {{}}"));
    }

    let mut acc = cx.create_string(&format!("{name} {{ "))?;
    for (i, (field_name, field_tty)) in fields.iter().enumerate() {
        if i > 0 {
            let sep = cx.create_string(", ")?;
            acc = str_concat(cx, acc, sep);
        }
        let label = cx.create_string(&format!("{field_name}: "))?;
        acc = str_concat(cx, acc, label);
        let offset = (i * 8) as i32;
        let raw = cx
            .builder
            .ins()
            .load(types::I64, MemFlags::new(), ptr, offset);
        let field_str = render_slot(cx, raw, field_tty)?;
        acc = str_concat(cx, acc, field_str);
    }
    let close = cx.create_string(" }")?;
    Ok(str_concat(cx, acc, close))
}

/// Render a result value as `ok(<value>)` or `err(<value>)`, mirroring the
/// optional `some(<value>)` / `none` rendering. The runtime tag (0 = ok,
/// 1 = err) selects which payload static type to render.
fn render_result<M: Module>(
    cx: &mut Ctx<'_, M>,
    val: Value,
    ok_ty: &TurboTy,
    err_ty: &TurboTy,
) -> Result<Value, CodegenError> {
    let tag_fid = cx.rt_fns["rt_result_tag"];
    let tag_fref = cx.module.declare_func_in_func(tag_fid, cx.builder.func);
    let tag_call = cx.builder.ins().call(tag_fref, &[val]);
    let tag = cx.builder.inst_results(tag_call)[0];

    let one = cx.builder.ins().iconst(types::I64, 1);
    let is_err = cx.builder.ins().icmp(IntCC::Equal, tag, one);

    let ok_block = cx.builder.create_block();
    let err_block = cx.builder.create_block();
    let merge_block = cx.builder.create_block();

    cx.builder.ins().brif(is_err, err_block, &[], ok_block, &[]);

    // ok path: ok(<payload>)
    cx.builder.switch_to_block(ok_block);
    cx.builder.seal_block(ok_block);
    let value_fid = cx.rt_fns["rt_result_value"];
    let value_fref = cx.module.declare_func_in_func(value_fid, cx.builder.func);
    let value_call = cx.builder.ins().call(value_fref, &[val]);
    let ok_val = cx.builder.inst_results(value_call)[0];
    let ok_inner = render_slot(cx, ok_val, ok_ty)?;
    let prefix = cx.create_string("ok(")?;
    let suffix = cx.create_string(")")?;
    let ok_str = str_concat(cx, prefix, ok_inner);
    let ok_str = str_concat(cx, ok_str, suffix);
    cx.builder.ins().jump(merge_block, &[ok_str]);

    // err path: err(<payload>)
    cx.builder.switch_to_block(err_block);
    cx.builder.seal_block(err_block);
    let value_fref = cx.module.declare_func_in_func(value_fid, cx.builder.func);
    let value_call = cx.builder.ins().call(value_fref, &[val]);
    let err_val = cx.builder.inst_results(value_call)[0];
    let err_inner = render_slot(cx, err_val, err_ty)?;
    let prefix = cx.create_string("err(")?;
    let suffix = cx.create_string(")")?;
    let err_str = str_concat(cx, prefix, err_inner);
    let err_str = str_concat(cx, err_str, suffix);
    cx.builder.ins().jump(merge_block, &[err_str]);

    // merge
    cx.builder.append_block_param(merge_block, cx.ptr_type);
    cx.builder.switch_to_block(merge_block);
    cx.builder.seal_block(merge_block);
    Ok(cx.builder.block_params(merge_block)[0])
}

// ── While loop ──────────────────────────────────────────────────────
