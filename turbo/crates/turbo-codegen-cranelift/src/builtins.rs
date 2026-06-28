//! Built-in function compilation helpers.
//!
//! Each `compile_*` function handles a specific built-in function call
//! (print, assert, len, string ops, math, IO, JSON, HTTP, channels, etc.).

use cranelift::prelude::isa::CallConv;
use cranelift::prelude::*;
use cranelift_module::Module;
use std::collections::HashMap;
use turbo_ast::*;

use crate::turbo_types::{CodegenError, MaybeTyped, TurboTy};
use crate::{compile_expr, retain_if_needed, turbo_ty_to_cl_type, Ctx};

pub(crate) fn compile_print<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    if args.is_empty() {
        let ptr = cx.create_string("")?;
        cx.rt_call("rt_print_str", &[ptr]);
        return Ok(None);
    }

    let result = compile_expr(cx, &args[0])?;

    if let Some((v, tty)) = result {
        match tty {
            TurboTy::Str => cx.rt_call("rt_print_str", &[v]),
            TurboTy::Float => cx.rt_call("rt_print_f64", &[v]),
            TurboTy::Bool => {
                let ty = cx.builder.func.dfg.value_type(v);
                let v = if ty.bits() > 8 {
                    cx.builder.ins().ireduce(types::I8, v)
                } else {
                    v
                };
                cx.rt_call("rt_print_bool", &[v]);
            }
            TurboTy::I8 | TurboTy::I16 => {
                // Sign-extend to i64 for printing. Guard on the IR width: a
                // narrow-tagged value that already rides a full i64 slot (e.g.
                // a cast result) must not be re-extended, or Cranelift panics.
                let ty = cx.builder.func.dfg.value_type(v);
                let v = if ty.bits() < 64 {
                    cx.builder.ins().sextend(types::I64, v)
                } else {
                    v
                };
                cx.rt_call("rt_print_i64", &[v]);
            }
            TurboTy::U8 | TurboTy::U16 => {
                // Zero-extend to i64 for printing (see note above on the guard).
                let ty = cx.builder.func.dfg.value_type(v);
                let v = if ty.bits() < 64 {
                    cx.builder.ins().uextend(types::I64, v)
                } else {
                    v
                };
                cx.rt_call("rt_print_i64", &[v]);
            }
            TurboTy::Int => {
                let ty = cx.builder.func.dfg.value_type(v);
                let v = if ty.bits() < 64 {
                    cx.builder.ins().sextend(types::I64, v)
                } else {
                    v
                };
                cx.rt_call("rt_print_i64", &[v]);
            }
            TurboTy::Unit => {
                let ptr = cx.create_string("()")?;
                cx.rt_call("rt_print_str", &[ptr]);
            }
            TurboTy::Enum(ref enum_name) => {
                // For data enums, extract the tag from the tagged union pointer; for unit enums, use the value directly
                let tag_val = if cx.enum_max_slots.contains_key(enum_name.as_str()) {
                    // Data enum: load tag from ptr[0]
                    cx.builder.ins().load(types::I64, MemFlags::new(), v, 0)
                } else {
                    let v = if cx.builder.func.dfg.value_type(v).bits() < 64 {
                        cx.builder.ins().sextend(types::I64, v)
                    } else {
                        v
                    };
                    v
                };
                cx.rt_call("rt_print_i64", &[tag_val]);
            }
            // Compound values (arrays, structs, results, optionals) all share
            // the recursive renderer in `convert_to_str`, which knows the
            // element/field/payload static types. Delegating here keeps
            // `print(x)` and `to_str(x)` / `"{x}"` byte-identical and is what
            // makes nested rendering (e.g. an array of structs) work.
            TurboTy::Array(_)
            | TurboTy::Struct(_)
            | TurboTy::Result(_, _)
            | TurboTy::Optional(_) => {
                let s = convert_to_str(cx, v, &tty)?;
                cx.rt_call("rt_print_str", &[s]);
            }
            TurboTy::Fn(_, _) => {
                let ptr = cx.create_string("[function]")?;
                cx.rt_call("rt_print_str", &[ptr]);
            }
            TurboTy::Future(_) => {
                let ptr = cx.create_string("[future]")?;
                cx.rt_call("rt_print_str", &[ptr]);
            }
        }
    } else {
        let ptr = cx.create_string("()")?;
        cx.rt_call("rt_print_str", &[ptr]);
    }

    Ok(None)
}

pub(crate) fn compile_panic<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let msg = if !args.is_empty() {
        compile_expr(cx, &args[0])?
            .ok_or_else(|| CodegenError {
                code: ErrorCode::E0400,
                message: "compile_panic: `&args[0]` produced no value during code generation"
                    .to_string(),
            })?
            .0
    } else {
        cx.create_string("explicit panic")?
    };

    cx.rt_call("rt_panic", &[msg]);
    cx.builder.ins().trap(TrapCode::unwrap_user(1));

    let new_block = cx.builder.create_block();
    cx.builder.switch_to_block(new_block);
    cx.builder.seal_block(new_block);

    Ok(None)
}

pub(crate) fn compile_assert<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    if args.is_empty() {
        return Err(CodegenError {
            code: ErrorCode::E0400,
            message: "assert() requires at least one argument".to_string(),
        });
    }

    let (cond, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_assert: `&args[0]` produced no value during code generation".to_string(),
    })?;
    let cond_bool = cx.to_bool(cond);

    let fail_block = cx.builder.create_block();
    let ok_block = cx.builder.create_block();

    cx.builder
        .ins()
        .brif(cond_bool, ok_block, &[], fail_block, &[]);

    cx.builder.switch_to_block(fail_block);
    cx.builder.seal_block(fail_block);

    let msg = if args.len() > 1 {
        compile_expr(cx, &args[1])?
            .ok_or_else(|| CodegenError {
                code: ErrorCode::E0400,
                message: "compile_assert: `&args[1]` produced no value during code generation"
                    .to_string(),
            })?
            .0
    } else {
        cx.create_string("assertion failed")?
    };

    cx.rt_call("rt_assert_fail", &[msg]);
    cx.builder.ins().trap(TrapCode::unwrap_user(1));

    cx.builder.switch_to_block(ok_block);
    cx.builder.seal_block(ok_block);

    Ok(None)
}

pub(crate) fn compile_assert_eq<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
    is_ne: bool,
) -> Result<MaybeTyped, CodegenError> {
    if args.len() != 2 {
        let name = if is_ne { "assert_ne" } else { "assert_eq" };
        return Err(CodegenError {
            code: ErrorCode::E0400,
            message: format!("{name}() requires exactly 2 arguments"),
        });
    }

    let (left_val, left_tty) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_assert_eq: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let (right_val, right_tty) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_assert_eq: `&args[1]` produced no value during code generation"
            .to_string(),
    })?;

    // Compare based on type
    let cond = match &left_tty {
        TurboTy::Str => {
            let fid = cx.rt_fns["rt_str_eq"];
            let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
            let call = cx.builder.ins().call(fref, &[left_val, right_val]);
            cx.builder.inst_results(call)[0]
        }
        TurboTy::Float => cx.builder.ins().fcmp(FloatCC::Equal, left_val, right_val),
        TurboTy::Bool => cx.builder.ins().icmp(IntCC::Equal, left_val, right_val),
        _ => {
            // For Int, Enum (unit), etc: i64 comparison
            let lv = {
                let ty = cx.builder.func.dfg.value_type(left_val);
                if ty.bits() < 64 {
                    cx.builder.ins().sextend(types::I64, left_val)
                } else {
                    left_val
                }
            };
            let rv = {
                let ty = cx.builder.func.dfg.value_type(right_val);
                if ty.bits() < 64 {
                    cx.builder.ins().sextend(types::I64, right_val)
                } else {
                    right_val
                }
            };
            cx.builder.ins().icmp(IntCC::Equal, lv, rv)
        }
    };

    let fail_block = cx.builder.create_block();
    let ok_block = cx.builder.create_block();

    if is_ne {
        // assert_ne: fail if equal (cond == true)
        cx.builder.ins().brif(cond, fail_block, &[], ok_block, &[]);
    } else {
        // assert_eq: fail if not equal (cond == false)
        cx.builder.ins().brif(cond, ok_block, &[], fail_block, &[]);
    }

    cx.builder.switch_to_block(fail_block);
    cx.builder.seal_block(fail_block);

    // Convert both values to string for error message
    let left_str = convert_to_str(cx, left_val, &left_tty)?;
    let right_str = convert_to_str(cx, right_val, &right_tty)?;

    let kind_val = cx
        .builder
        .ins()
        .iconst(types::I64, if is_ne { 1 } else { 0 });

    let fid = cx.rt_fns["rt_assert_eq_fail"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    cx.builder
        .ins()
        .call(fref, &[kind_val, left_str, right_str]);
    cx.builder.ins().trap(TrapCode::unwrap_user(1));

    cx.builder.switch_to_block(ok_block);
    cx.builder.seal_block(ok_block);

    Ok(None)
}

pub(crate) fn compile_len<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    if args.is_empty() {
        return Err(CodegenError {
            code: ErrorCode::E0400,
            message: "len() requires exactly 1 argument".to_string(),
        });
    }
    let (val, tty) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_len: `&args[0]` produced no value during code generation".to_string(),
    })?;
    if tty == TurboTy::Str {
        let len_fid = cx.rt_fns["rt_str_len"];
        let len_ref = cx.module.declare_func_in_func(len_fid, cx.builder.func);
        let call = cx.builder.ins().call(len_ref, &[val]);
        let result = cx.builder.inst_results(call)[0];
        Ok(Some((result, TurboTy::Int)))
    } else {
        let len_fid = cx.rt_fns["rt_array_len"];
        let len_ref = cx.module.declare_func_in_func(len_fid, cx.builder.func);
        let call = cx.builder.ins().call(len_ref, &[val]);
        let result = cx.builder.inst_results(call)[0];
        Ok(Some((result, TurboTy::Int)))
    }
}

// ── abs/min/max/to_str builtins ─────────────────────────────────────

pub(crate) fn compile_abs<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (val, tty) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_abs: `&args[0]` produced no value during code generation".to_string(),
    })?;
    if tty == TurboTy::Float {
        let result = cx.builder.ins().fabs(val);
        Ok(Some((result, TurboTy::Float)))
    } else {
        let zero = cx.builder.ins().iconst(types::I64, 0);
        let is_neg = cx.builder.ins().icmp(IntCC::SignedLessThan, val, zero);
        let neg_val = cx.builder.ins().ineg(val);
        let result = cx.builder.ins().select(is_neg, neg_val, val);
        Ok(Some((result, TurboTy::Int)))
    }
}

pub(crate) fn compile_min<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (a, a_tty) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_min: `&args[0]` produced no value during code generation".to_string(),
    })?;
    let (b, _) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_min: `&args[1]` produced no value during code generation".to_string(),
    })?;
    if a_tty == TurboTy::Float {
        let result = cx.builder.ins().fmin(a, b);
        Ok(Some((result, TurboTy::Float)))
    } else {
        let cc = if a_tty == TurboTy::U8 || a_tty == TurboTy::U16 {
            IntCC::UnsignedLessThan
        } else {
            IntCC::SignedLessThan
        };
        let cmp = cx.builder.ins().icmp(cc, a, b);
        let result = cx.builder.ins().select(cmp, a, b);
        Ok(Some((result, a_tty)))
    }
}

pub(crate) fn compile_max<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (a, a_tty) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_max: `&args[0]` produced no value during code generation".to_string(),
    })?;
    let (b, _) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_max: `&args[1]` produced no value during code generation".to_string(),
    })?;
    if a_tty == TurboTy::Float {
        let result = cx.builder.ins().fmax(a, b);
        Ok(Some((result, TurboTy::Float)))
    } else {
        let cc = if a_tty == TurboTy::U8 || a_tty == TurboTy::U16 {
            IntCC::UnsignedGreaterThan
        } else {
            IntCC::SignedGreaterThan
        };
        let cmp = cx.builder.ins().icmp(cc, a, b);
        let result = cx.builder.ins().select(cmp, a, b);
        Ok(Some((result, a_tty)))
    }
}

/// float_to_int(f64) -> i64 — truncate float to signed integer (saturating)
pub(crate) fn compile_builtin_float_to_int<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message:
            "compile_builtin_float_to_int: `&args[0]` produced no value during code generation"
                .to_string(),
    })?;
    let result = cx.builder.ins().fcvt_to_sint_sat(types::I64, val);
    Ok(Some((result, TurboTy::Int)))
}

/// int_to_float(i64) -> f64 — convert signed integer to float
pub(crate) fn compile_builtin_int_to_float<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message:
            "compile_builtin_int_to_float: `&args[0]` produced no value during code generation"
                .to_string(),
    })?;
    let result = cx.builder.ins().fcvt_from_sint(types::F64, val);
    Ok(Some((result, TurboTy::Float)))
}

/// str_from_char(i64) -> str — convert ASCII code to single-character string
pub(crate) fn compile_builtin_str_from_char<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message:
            "compile_builtin_str_from_char: `&args[0]` produced no value during code generation"
                .to_string(),
    })?;
    let fid = cx.rt_fns["rt_str_from_char"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Str)))
}

pub(crate) fn compile_to_str_builtin<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (val, tty) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_to_str_builtin: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let str_val = convert_to_str(cx, val, &tty)?;
    Ok(Some((str_val, TurboTy::Str)))
}

// ── Stdlib builtins ─────────────────────────────────────────────────

/// split(s, sep) -> [str] — calls rt_str_split, returns Array(Str)
pub(crate) fn compile_stdlib_split<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (s_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_stdlib_split: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let (sep_val, _) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_stdlib_split: `&args[1]` produced no value during code generation"
            .to_string(),
    })?;
    let fid = cx.rt_fns["rt_str_split"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[s_val, sep_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Array(Box::new(TurboTy::Str)))))
}

/// Generic helper for str->str builtins (trim, upper, lower)
pub(crate) fn compile_stdlib_str1<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
    rt_name: &str,
) -> Result<MaybeTyped, CodegenError> {
    let (s_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_stdlib_str1: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let fid = cx.rt_fns[rt_name];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[s_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Str)))
}

/// Generic helper for (str, str)->bool builtins (starts_with, ends_with)
pub(crate) fn compile_stdlib_str_bool2<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
    rt_name: &str,
) -> Result<MaybeTyped, CodegenError> {
    let (s_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_stdlib_str_bool2: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let (other_val, _) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_stdlib_str_bool2: `&args[1]` produced no value during code generation"
            .to_string(),
    })?;
    let fid = cx.rt_fns[rt_name];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[s_val, other_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Bool)))
}

/// replace(s, from, to) -> str
pub(crate) fn compile_stdlib_replace<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (s_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_stdlib_replace: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let (from_val, _) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_stdlib_replace: `&args[1]` produced no value during code generation"
            .to_string(),
    })?;
    let (to_val, _) = compile_expr(cx, &args[2])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_stdlib_replace: `&args[2]` produced no value during code generation"
            .to_string(),
    })?;
    let fid = cx.rt_fns["rt_str_replace"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[s_val, from_val, to_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Str)))
}

/// char_at(s, index) -> str
pub(crate) fn compile_stdlib_char_at<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (s_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
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
    Ok(Some((result, TurboTy::Str)))
}

/// index_of(s, sub) -> i64
pub(crate) fn compile_stdlib_index_of<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (s_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_stdlib_index_of: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let (sub_val, _) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_stdlib_index_of: `&args[1]` produced no value during code generation"
            .to_string(),
    })?;
    let fid = cx.rt_fns["rt_str_index_of"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[s_val, sub_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Int)))
}

/// join(arr, sep) -> str
pub(crate) fn compile_stdlib_join<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (arr_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_stdlib_join: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let (sep_val, _) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_stdlib_join: `&args[1]` produced no value during code generation"
            .to_string(),
    })?;
    let fid = cx.rt_fns["rt_str_join"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[arr_val, sep_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Str)))
}

/// repeat(s, n) -> str
pub(crate) fn compile_stdlib_repeat<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (s_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
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
    Ok(Some((result, TurboTy::Str)))
}

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

/// random() -> f64
pub(crate) fn compile_random<M: Module>(cx: &mut Ctx<'_, M>) -> Result<MaybeTyped, CodegenError> {
    let fid = cx.rt_fns["rt_random"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Float)))
}

/// random_range(min, max) -> i64
pub(crate) fn compile_random_range<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (min_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_random_range: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let (max_val, _) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_random_range: `&args[1]` produced no value during code generation"
            .to_string(),
    })?;
    let fid = cx.rt_fns["rt_random_range"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[min_val, max_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Int)))
}

// ── System builtins ────────────────────────────────────────────────

/// exit(code) -> ()
pub(crate) fn compile_exit<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (code_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_exit: `&args[0]` produced no value during code generation".to_string(),
    })?;
    let code_ty = cx.builder.func.dfg.value_type(code_val);
    let code_val = if code_ty.bits() < 64 {
        cx.builder.ins().sextend(types::I64, code_val)
    } else {
        code_val
    };
    let fid = cx.rt_fns["rt_exit"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    cx.builder.ins().call(fref, &[code_val]);
    // rt_exit never returns (it calls process::exit), but Cranelift can't know
    // that from the call alone. Emit a trap and move to a fresh sealed,
    // predecessor-less block — exactly as `compile_panic` does — so that
    // `is_unreachable()` reports true afterwards. Otherwise `exit` in one arm
    // of an `if`/`match` leaves the arm looking reachable-but-valueless,
    // producing a merge-block arg/param arity mismatch (malformed SSA).
    cx.builder.ins().trap(TrapCode::unwrap_user(1));
    let new_block = cx.builder.create_block();
    cx.builder.switch_to_block(new_block);
    cx.builder.seal_block(new_block);
    Ok(None)
}

/// args() -> [str]
pub(crate) fn compile_args<M: Module>(cx: &mut Ctx<'_, M>) -> Result<MaybeTyped, CodegenError> {
    let fid = cx.rt_fns["rt_args"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Array(Box::new(TurboTy::Str)))))
}

/// type_of(val) -> str — compiler intrinsic, emits type name as string constant
pub(crate) fn compile_type_of<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    // Compile the argument to get its type, then discard the value
    let result = compile_expr(cx, &args[0])?;
    let type_name = if let Some((_, ref tty)) = result {
        match tty {
            TurboTy::I8 => "i8",
            TurboTy::I16 => "i16",
            TurboTy::Int => "i64",
            TurboTy::U8 => "u8",
            TurboTy::U16 => "u16",
            TurboTy::Float => "f64",
            TurboTy::Bool => "bool",
            TurboTy::Str => "str",
            TurboTy::Unit => "unit",
            TurboTy::Array(_) => "array",
            TurboTy::Struct(name) => name.as_str(),
            TurboTy::Enum(name) => name.as_str(),
            TurboTy::Fn(_, _) => "fn",
            TurboTy::Result(_, _) => "result",
            TurboTy::Optional(_) => "optional",
            TurboTy::Future(_) => "future",
        }
    } else {
        "unit"
    };
    let ptr = cx.create_string(type_name)?;
    Ok(Some((ptr, TurboTy::Str)))
}

// ── String parsing builtins ────────────────────────────────────────

/// substring(s, start, end) -> str
pub(crate) fn compile_substring<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (s_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
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
    Ok(Some((result, TurboTy::Str)))
}

/// pad_left(s, width, char) -> str
pub(crate) fn compile_pad_left<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (s_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_pad_left: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let (width_val, _) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_pad_left: `&args[1]` produced no value during code generation"
            .to_string(),
    })?;
    let (char_val, _) = compile_expr(cx, &args[2])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_pad_left: `&args[2]` produced no value during code generation"
            .to_string(),
    })?;
    let fid = cx.rt_fns["rt_pad_left"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[s_val, width_val, char_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Str)))
}

/// pad_right(s, width, char) -> str
pub(crate) fn compile_pad_right<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (s_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_pad_right: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let (width_val, _) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_pad_right: `&args[1]` produced no value during code generation"
            .to_string(),
    })?;
    let (char_val, _) = compile_expr(cx, &args[2])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_pad_right: `&args[2]` produced no value during code generation"
            .to_string(),
    })?;
    let fid = cx.rt_fns["rt_pad_right"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[s_val, width_val, char_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Str)))
}

/// str_to_int(s) -> i64 ! str
pub(crate) fn compile_str_to_int<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (s_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_str_to_int: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let fid = cx.rt_fns["rt_str_to_int"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[s_val]);
    let result = cx.builder.inst_results(call)[0];
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
    let (s_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_str_to_float: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let fid = cx.rt_fns["rt_str_to_float"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[s_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((
        result,
        TurboTy::Result(Box::new(TurboTy::Float), Box::new(TurboTy::Str)),
    )))
}

/// sleep(ms) -> () — sleep the current thread for ms milliseconds
pub(crate) fn compile_builtin_sleep<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (ms_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_builtin_sleep: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    // Ensure it's i64
    let ms_ty = cx.builder.func.dfg.value_type(ms_val);
    let ms_val = if ms_ty.bits() < 64 {
        cx.builder.ins().sextend(types::I64, ms_val)
    } else {
        ms_val
    };
    let fid = cx.rt_fns["rt_sleep_ms"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    cx.builder.ins().call(fref, &[ms_val]);
    Ok(None)
}

// ── HTTP + JSON builtins ────────────────────────────────────────────

/// http_get(url) -> str
pub(crate) fn compile_builtin_http_get<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (url_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_builtin_http_get: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let fid = cx.rt_fns["rt_http_get"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[url_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Str)))
}

/// http_post(url, body) -> str
pub(crate) fn compile_builtin_http_post<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (url_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_builtin_http_post: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let (body_val, _) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_builtin_http_post: `&args[1]` produced no value during code generation"
            .to_string(),
    })?;
    let fid = cx.rt_fns["rt_http_post"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[url_val, body_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Str)))
}

/// http_post_with_headers(url, body, headers) -> str
pub(crate) fn compile_builtin_http_post_with_headers<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (url_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError { code: ErrorCode::E0400, message: "compile_builtin_http_post_with_headers: `&args[0]` produced no value during code generation".to_string() })?;
    let (body_val, _) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError { code: ErrorCode::E0400, message: "compile_builtin_http_post_with_headers: `&args[1]` produced no value during code generation".to_string() })?;
    let (headers_val, _) = compile_expr(cx, &args[2])?.ok_or_else(|| CodegenError { code: ErrorCode::E0400, message: "compile_builtin_http_post_with_headers: `&args[2]` produced no value during code generation".to_string() })?;
    let fid = cx.rt_fns["rt_http_post_with_headers"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx
        .builder
        .ins()
        .call(fref, &[url_val, body_val, headers_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Str)))
}

/// json_get(json_str, key) -> str
pub(crate) fn compile_builtin_json_get<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (json_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_builtin_json_get: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let (key_val, _) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_builtin_json_get: `&args[1]` produced no value during code generation"
            .to_string(),
    })?;
    let fid = cx.rt_fns["rt_json_get"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[json_val, key_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Str)))
}

/// json_stringify(key, value) -> str
pub(crate) fn compile_builtin_json_stringify<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (key_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message:
            "compile_builtin_json_stringify: `&args[0]` produced no value during code generation"
                .to_string(),
    })?;
    let (value_val, _) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message:
            "compile_builtin_json_stringify: `&args[1]` produced no value during code generation"
                .to_string(),
    })?;
    let fid = cx.rt_fns["rt_json_stringify"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[key_val, value_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Str)))
}

/// json_build(pairs_str) -> str
pub(crate) fn compile_builtin_json_build<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (pairs_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_builtin_json_build: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let fid = cx.rt_fns["rt_json_build"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[pairs_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Str)))
}

// ── HTTP server builtins ────────────────────────────────────────────

/// http_server(port) -> i64 (server id). Binds to 127.0.0.1 by default —
/// use http_server_public to listen on all interfaces.
pub(crate) fn compile_builtin_http_server<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (port_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_builtin_http_server: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let fid = cx.rt_fns["rt_http_server"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[port_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Int)))
}

/// http_server_public(port) -> i64 (server id). Opt-in public bind to
/// INADDR_ANY. Callers are expected to front the server with a proxy.
pub(crate) fn compile_builtin_http_server_public<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (port_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError { code: ErrorCode::E0400, message: "compile_builtin_http_server_public: `&args[0]` produced no value during code generation".to_string() })?;
    let fid = cx.rt_fns["rt_http_server_public"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[port_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Int)))
}

/// route(server_id, method, path, handler_closure)
/// Extracts fn_ptr and env_ptr from the closure pair and passes to rt_http_route.
pub(crate) fn compile_builtin_route<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (server_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_builtin_route: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let (method_val, _) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_builtin_route: `&args[1]` produced no value during code generation"
            .to_string(),
    })?;
    let (path_val, _) = compile_expr(cx, &args[2])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_builtin_route: `&args[2]` produced no value during code generation"
            .to_string(),
    })?;
    let (closure_ptr, _) = compile_expr(cx, &args[3])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_builtin_route: `&args[3]` produced no value during code generation"
            .to_string(),
    })?;

    // Extract fn_ptr and env_ptr from the closure pair struct (offset 0 = fn_ptr, offset 8 = env_ptr)
    let fn_ptr = cx
        .builder
        .ins()
        .load(cx.ptr_type, MemFlags::new(), closure_ptr, 0);
    let env_ptr = cx
        .builder
        .ins()
        .load(cx.ptr_type, MemFlags::new(), closure_ptr, 8);

    let fid = cx.rt_fns["rt_http_route"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    cx.builder
        .ins()
        .call(fref, &[server_val, method_val, path_val, fn_ptr, env_ptr]);
    Ok(None)
}

/// http_listen(server_id) -> () — starts the server, blocks forever
pub(crate) fn compile_builtin_http_listen<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (server_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_builtin_http_listen: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let fid = cx.rt_fns["rt_http_listen"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    cx.builder.ins().call(fref, &[server_val]);
    Ok(None)
}

fn compile_builtin_respond_with_type<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
    content_type: &str,
) -> Result<MaybeTyped, CodegenError> {
    let (status_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message:
            "compile_builtin_respond_with_type: `&args[0]` produced no value during code generation"
                .to_string(),
    })?;
    let (body_val, _) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message:
            "compile_builtin_respond_with_type: `&args[1]` produced no value during code generation"
                .to_string(),
    })?;
    let content_type_val = cx.create_string(content_type)?;
    let fid = cx.rt_fns["rt_respond_typed"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx
        .builder
        .ins()
        .call(fref, &[status_val, content_type_val, body_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Str)))
}

/// respond(status, body) -> str — builds a text/plain response
pub(crate) fn compile_builtin_respond_text<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    compile_builtin_respond_with_type(cx, args, "text/plain")
}

/// respond_html(status, body) -> str — builds a text/html response
pub(crate) fn compile_builtin_respond_html<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    compile_builtin_respond_with_type(cx, args, "text/html")
}

/// respond_json(status, body) -> str — builds an application/json response
pub(crate) fn compile_builtin_respond_json<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    compile_builtin_respond_with_type(cx, args, "application/json")
}

/// request_body(req) -> str — extracts body from request (identity for now)
pub(crate) fn compile_builtin_request_body<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (req_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message:
            "compile_builtin_request_body: `&args[0]` produced no value during code generation"
                .to_string(),
    })?;
    let fid = cx.rt_fns["rt_request_body"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[req_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Str)))
}

/// Generic single-arg request builtin: request_method(req), request_path(req)
pub(crate) fn compile_builtin_request_simple<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
    rt_fn_name: &str,
) -> Result<MaybeTyped, CodegenError> {
    let (req_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message:
            "compile_builtin_request_simple: `&args[0]` produced no value during code generation"
                .to_string(),
    })?;
    let fid = cx.rt_fns[rt_fn_name];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[req_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Str)))
}

/// Generic two-arg request builtin: request_query(req, key), request_header(req, name)
pub(crate) fn compile_builtin_request_two_arg<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
    rt_fn_name: &str,
) -> Result<MaybeTyped, CodegenError> {
    let (req_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message:
            "compile_builtin_request_two_arg: `&args[0]` produced no value during code generation"
                .to_string(),
    })?;
    let (key_val, _) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message:
            "compile_builtin_request_two_arg: `&args[1]` produced no value during code generation"
                .to_string(),
    })?;
    let fid = cx.rt_fns[rt_fn_name];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[req_val, key_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Str)))
}

// ── to_json / to_json_array builtins ────────────────────────────────

/// to_json(val) -> str — serialize a struct to a JSON string at codegen time
/// Uses struct field layout to generate field-by-field concatenation.
pub(crate) fn compile_builtin_to_json<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (val, tty) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_builtin_to_json: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;

    if let TurboTy::Struct(ref struct_name) = tty {
        compile_struct_to_json(cx, val, struct_name)
    } else {
        // For non-structs, just convert to string
        let str_val = convert_to_str(cx, val, &tty)?;
        Ok(Some((str_val, TurboTy::Str)))
    }
}

/// Generate JSON string from a struct pointer: {"field1":val1,"field2":val2,...}
pub(crate) fn compile_struct_to_json<M: Module>(
    cx: &mut Ctx<'_, M>,
    struct_ptr: Value,
    struct_name: &str,
) -> Result<MaybeTyped, CodegenError> {
    let struct_layout = cx
        .struct_fields
        .get(struct_name)
        .ok_or_else(|| CodegenError {
            code: ErrorCode::E0400,
            message: format!("undefined struct: {struct_name}"),
        })?
        .clone();

    let concat_fid = cx.rt_fns["rt_str_concat"];

    // Start with "{"
    let mut result = cx.create_string("{")?;

    for (i, (field_name, field_ty)) in struct_layout.iter().enumerate() {
        // Add comma separator between fields (and the key)
        let prefix = if i > 0 {
            format!(",\"{}\":", field_name)
        } else {
            format!("\"{}\":", field_name)
        };
        let prefix_str = cx.create_string(&prefix)?;
        let concat_ref = cx.module.declare_func_in_func(concat_fid, cx.builder.func);
        let call = cx.builder.ins().call(concat_ref, &[result, prefix_str]);
        result = cx.builder.inst_results(call)[0];

        // Load field value from struct
        let offset = (i * 8) as i32;
        let raw_val = cx
            .builder
            .ins()
            .load(types::I64, MemFlags::new(), struct_ptr, offset);

        // For string fields, wrap the value in quotes; for numeric/bool, emit raw
        let field_json_str = match field_ty {
            TurboTy::Str => {
                let quote_str = cx.create_string("\"")?;
                let concat_ref = cx.module.declare_func_in_func(concat_fid, cx.builder.func);
                let call = cx.builder.ins().call(concat_ref, &[quote_str, raw_val]);
                let with_open_quote = cx.builder.inst_results(call)[0];
                let quote_str2 = cx.create_string("\"")?;
                let concat_ref2 = cx.module.declare_func_in_func(concat_fid, cx.builder.func);
                let call2 = cx
                    .builder
                    .ins()
                    .call(concat_ref2, &[with_open_quote, quote_str2]);
                cx.builder.inst_results(call2)[0]
            }
            TurboTy::Int => {
                let fid = cx.rt_fns["rt_i64_to_str"];
                let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
                let call = cx.builder.ins().call(fref, &[raw_val]);
                cx.builder.inst_results(call)[0]
            }
            TurboTy::Bool => {
                let bool_val = cx.builder.ins().ireduce(types::I8, raw_val);
                let fid = cx.rt_fns["rt_bool_to_str"];
                let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
                let call = cx.builder.ins().call(fref, &[bool_val]);
                cx.builder.inst_results(call)[0]
            }
            TurboTy::Float => {
                let float_val = cx
                    .builder
                    .ins()
                    .bitcast(types::F64, MemFlags::new(), raw_val);
                let fid = cx.rt_fns["rt_f64_to_str"];
                let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
                let call = cx.builder.ins().call(fref, &[float_val]);
                cx.builder.inst_results(call)[0]
            }
            _ => convert_to_str(cx, raw_val, field_ty)?,
        };

        // Concat the field value
        let concat_ref = cx.module.declare_func_in_func(concat_fid, cx.builder.func);
        let call = cx.builder.ins().call(concat_ref, &[result, field_json_str]);
        result = cx.builder.inst_results(call)[0];
    }

    // Close with "}"
    let suffix = cx.create_string("}")?;
    let concat_ref = cx.module.declare_func_in_func(concat_fid, cx.builder.func);
    let call = cx.builder.ins().call(concat_ref, &[result, suffix]);
    result = cx.builder.inst_results(call)[0];

    Ok(Some((result, TurboTy::Str)))
}

/// to_json_array(arr) -> str — serialize an array of structs to JSON array string
/// Generates [item1,item2,...] by iterating and calling to_json on each element.
pub(crate) fn compile_builtin_to_json_array<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (arr_ptr, arr_tty) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message:
            "compile_builtin_to_json_array: `&args[0]` produced no value during code generation"
                .to_string(),
    })?;

    let elem_tty = match &arr_tty {
        TurboTy::Array(inner) => *inner.clone(),
        _ => {
            return Err(CodegenError {
                code: ErrorCode::E0400,
                message: "to_json_array() argument must be an array".to_string(),
            })
        }
    };

    let struct_name = match &elem_tty {
        TurboTy::Struct(name) => name.clone(),
        _ => {
            return Err(CodegenError {
                code: ErrorCode::E0400,
                message: "to_json_array() requires an array of structs".to_string(),
            })
        }
    };

    let concat_fid = cx.rt_fns["rt_str_concat"];

    // Get array length
    let len_fid = cx.rt_fns["rt_array_len"];
    let len_ref = cx.module.declare_func_in_func(len_fid, cx.builder.func);
    let len_call = cx.builder.ins().call(len_ref, &[arr_ptr]);
    let arr_len = cx.builder.inst_results(len_call)[0];

    // Start with "["
    let open_bracket = cx.create_string("[")?;

    // result_var accumulates the JSON string; idx_var is the loop counter
    let result_var = cx.fresh_var(cx.ptr_type, TurboTy::Str);
    cx.builder.def_var(result_var, open_bracket);

    let idx_var = cx.fresh_var(types::I64, TurboTy::Int);
    let zero = cx.builder.ins().iconst(types::I64, 0);
    cx.builder.def_var(idx_var, zero);

    let header_block = cx.builder.create_block();
    let body_block = cx.builder.create_block();
    let exit_block = cx.builder.create_block();

    cx.builder.ins().jump(header_block, &[]);

    // Header: check idx < len
    cx.builder.switch_to_block(header_block);
    let idx = cx.builder.use_var(idx_var);
    let cond = cx.builder.ins().icmp(IntCC::SignedLessThan, idx, arr_len);
    cx.builder
        .ins()
        .brif(cond, body_block, &[], exit_block, &[]);

    // Body: get element, serialize, concat
    cx.builder.switch_to_block(body_block);
    cx.builder.seal_block(body_block);

    let current_idx = cx.builder.use_var(idx_var);

    // Add comma before element if idx > 0
    let needs_comma = cx
        .builder
        .ins()
        .icmp(IntCC::SignedGreaterThan, current_idx, zero);
    let comma_block = cx.builder.create_block();
    let no_comma_block = cx.builder.create_block();
    let merge_block = cx.builder.create_block();
    cx.builder.append_block_param(merge_block, cx.ptr_type);

    cx.builder
        .ins()
        .brif(needs_comma, comma_block, &[], no_comma_block, &[]);

    // comma_block: append ","
    cx.builder.switch_to_block(comma_block);
    cx.builder.seal_block(comma_block);
    let comma_str = cx.create_string(",")?;
    let concat_ref = cx.module.declare_func_in_func(concat_fid, cx.builder.func);
    let with_comma_result = cx.builder.use_var(result_var);
    let call = cx
        .builder
        .ins()
        .call(concat_ref, &[with_comma_result, comma_str]);
    let after_comma = cx.builder.inst_results(call)[0];
    cx.builder.ins().jump(merge_block, &[after_comma]);

    // no_comma_block: pass through
    cx.builder.switch_to_block(no_comma_block);
    cx.builder.seal_block(no_comma_block);
    let no_comma_result = cx.builder.use_var(result_var);
    cx.builder.ins().jump(merge_block, &[no_comma_result]);

    // merge_block
    cx.builder.switch_to_block(merge_block);
    cx.builder.seal_block(merge_block);
    let merged_result = cx.builder.block_params(merge_block)[0];

    // Get the element from the array
    let get_fid = cx.rt_fns["rt_array_get"];
    let get_ref = cx.module.declare_func_in_func(get_fid, cx.builder.func);
    let idx_val = cx.builder.use_var(idx_var);
    let get_call = cx.builder.ins().call(get_ref, &[arr_ptr, idx_val]);
    let elem_ptr = cx.builder.inst_results(get_call)[0];

    // Serialize the struct element to JSON (inline the field iteration)
    let struct_layout = cx
        .struct_fields
        .get(&struct_name)
        .ok_or_else(|| CodegenError {
            code: ErrorCode::E0400,
            message: format!("undefined struct: {struct_name}"),
        })?
        .clone();

    let inner_concat_fid = cx.rt_fns["rt_str_concat"];
    let mut elem_json = cx.create_string("{")?;

    for (fi, (fname, fty)) in struct_layout.iter().enumerate() {
        let prefix = if fi > 0 {
            format!(",\"{}\":", fname)
        } else {
            format!("\"{}\":", fname)
        };
        let prefix_str = cx.create_string(&prefix)?;
        let inner_concat_ref = cx
            .module
            .declare_func_in_func(inner_concat_fid, cx.builder.func);
        let c = cx
            .builder
            .ins()
            .call(inner_concat_ref, &[elem_json, prefix_str]);
        elem_json = cx.builder.inst_results(c)[0];

        let foffset = (fi * 8) as i32;
        let raw_val = cx
            .builder
            .ins()
            .load(types::I64, MemFlags::new(), elem_ptr, foffset);

        let field_json_str = match fty {
            TurboTy::Str => {
                let q = cx.create_string("\"")?;
                let cr = cx
                    .module
                    .declare_func_in_func(inner_concat_fid, cx.builder.func);
                let c1 = cx.builder.ins().call(cr, &[q, raw_val]);
                let wq = cx.builder.inst_results(c1)[0];
                let q2 = cx.create_string("\"")?;
                let cr2 = cx
                    .module
                    .declare_func_in_func(inner_concat_fid, cx.builder.func);
                let c2 = cx.builder.ins().call(cr2, &[wq, q2]);
                cx.builder.inst_results(c2)[0]
            }
            TurboTy::Int => {
                let fid = cx.rt_fns["rt_i64_to_str"];
                let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
                let c = cx.builder.ins().call(fref, &[raw_val]);
                cx.builder.inst_results(c)[0]
            }
            TurboTy::Bool => {
                let bool_val = cx.builder.ins().ireduce(types::I8, raw_val);
                let fid = cx.rt_fns["rt_bool_to_str"];
                let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
                let c = cx.builder.ins().call(fref, &[bool_val]);
                cx.builder.inst_results(c)[0]
            }
            TurboTy::Float => {
                let float_val = cx
                    .builder
                    .ins()
                    .bitcast(types::F64, MemFlags::new(), raw_val);
                let fid = cx.rt_fns["rt_f64_to_str"];
                let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
                let c = cx.builder.ins().call(fref, &[float_val]);
                cx.builder.inst_results(c)[0]
            }
            _ => convert_to_str(cx, raw_val, fty)?,
        };

        let cr = cx
            .module
            .declare_func_in_func(inner_concat_fid, cx.builder.func);
        let c = cx.builder.ins().call(cr, &[elem_json, field_json_str]);
        elem_json = cx.builder.inst_results(c)[0];
    }

    let close_brace = cx.create_string("}")?;
    let cr = cx
        .module
        .declare_func_in_func(inner_concat_fid, cx.builder.func);
    let c = cx.builder.ins().call(cr, &[elem_json, close_brace]);
    elem_json = cx.builder.inst_results(c)[0];

    // Concat element JSON to accumulated result
    let concat_ref2 = cx.module.declare_func_in_func(concat_fid, cx.builder.func);
    let call2 = cx
        .builder
        .ins()
        .call(concat_ref2, &[merged_result, elem_json]);
    let new_result = cx.builder.inst_results(call2)[0];
    cx.builder.def_var(result_var, new_result);

    // Increment idx
    let cur_idx = cx.builder.use_var(idx_var);
    let one = cx.builder.ins().iconst(types::I64, 1);
    let next_idx = cx.builder.ins().iadd(cur_idx, one);
    cx.builder.def_var(idx_var, next_idx);
    cx.builder.ins().jump(header_block, &[]);

    cx.builder.seal_block(header_block);

    // Exit: close with "]"
    cx.builder.switch_to_block(exit_block);
    cx.builder.seal_block(exit_block);

    let final_result = cx.builder.use_var(result_var);
    let close_bracket = cx.create_string("]")?;
    let concat_ref3 = cx.module.declare_func_in_func(concat_fid, cx.builder.func);
    let call3 = cx
        .builder
        .ins()
        .call(concat_ref3, &[final_result, close_bracket]);
    let result = cx.builder.inst_results(call3)[0];

    Ok(Some((result, TurboTy::Str)))
}

// ── map/filter/reduce builtins ──────────────────────────────────────

/// compile_builtin_map: map(arr, fn) -> [U]
/// Allocates a new array of the same length, iterates the source array,
/// calls fn_ptr on each element via call_indirect, and stores results.
pub(crate) fn compile_builtin_map<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (arr_ptr, _arr_tty) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_builtin_map: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let (closure_ptr, fn_tty) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_builtin_map: `&args[1]` produced no value during code generation"
            .to_string(),
    })?;

    let (param_tty, ret_tty) = match &fn_tty {
        TurboTy::Fn(params, ret) => (params[0].clone(), *ret.clone()),
        _ => (TurboTy::Int, TurboTy::Int),
    };

    // Extract fn_ptr and env_ptr from closure pair struct
    let fn_ptr = cx
        .builder
        .ins()
        .load(cx.ptr_type, MemFlags::new(), closure_ptr, 0);
    let env_ptr = cx
        .builder
        .ins()
        .load(cx.ptr_type, MemFlags::new(), closure_ptr, 8);

    let len_fid = cx.rt_fns["rt_array_len"];
    let len_ref = cx.module.declare_func_in_func(len_fid, cx.builder.func);
    let call = cx.builder.ins().call(len_ref, &[arr_ptr]);
    let arr_len = cx.builder.inst_results(call)[0];

    let alloc_fid = cx.rt_fns["rt_array_alloc"];
    let alloc_ref = cx.module.declare_func_in_func(alloc_fid, cx.builder.func);
    let alloc_call = cx.builder.ins().call(alloc_ref, &[arr_len]);
    let result_ptr = cx.builder.inst_results(alloc_call)[0];

    let mut sig = cx.module.make_signature();
    sig.call_conv = CallConv::Fast;
    sig.params.push(AbiParam::new(cx.ptr_type)); // env_ptr
    let param_cl_ty = turbo_ty_to_cl_type(&param_tty, cx.ptr_type);
    sig.params.push(AbiParam::new(param_cl_ty));
    if ret_tty != TurboTy::Unit {
        let ret_cl_ty = turbo_ty_to_cl_type(&ret_tty, cx.ptr_type);
        sig.returns.push(AbiParam::new(ret_cl_ty));
    }
    let sig_ref = cx.builder.import_signature(sig);

    let idx_var = cx.fresh_var(types::I64, TurboTy::Int);
    let zero = cx.builder.ins().iconst(types::I64, 0);
    cx.builder.def_var(idx_var, zero);

    let header_block = cx.builder.create_block();
    let body_block = cx.builder.create_block();
    let exit_block = cx.builder.create_block();

    cx.builder.ins().jump(header_block, &[]);

    cx.builder.switch_to_block(header_block);
    let idx = cx.builder.use_var(idx_var);
    let cond = cx.builder.ins().icmp(IntCC::SignedLessThan, idx, arr_len);
    cx.builder
        .ins()
        .brif(cond, body_block, &[], exit_block, &[]);

    cx.builder.switch_to_block(body_block);
    cx.builder.seal_block(body_block);

    let get_fid = cx.rt_fns["rt_array_get"];
    let get_ref = cx.module.declare_func_in_func(get_fid, cx.builder.func);
    let idx_val = cx.builder.use_var(idx_var);
    let get_call = cx.builder.ins().call(get_ref, &[arr_ptr, idx_val]);
    let raw_elem = cx.builder.inst_results(get_call)[0];

    let typed_elem = match &param_tty {
        TurboTy::Bool => cx.builder.ins().ireduce(types::I8, raw_elem),
        TurboTy::Float => cx
            .builder
            .ins()
            .bitcast(types::F64, MemFlags::new(), raw_elem),
        _ => raw_elem,
    };

    let indirect_call = cx
        .builder
        .ins()
        .call_indirect(sig_ref, fn_ptr, &[env_ptr, typed_elem]);
    let mapped_val = cx.builder.inst_results(indirect_call)[0];

    let store_val = match &ret_tty {
        TurboTy::Bool => cx.builder.ins().sextend(types::I64, mapped_val),
        TurboTy::Float => cx
            .builder
            .ins()
            .bitcast(types::I64, MemFlags::new(), mapped_val),
        _ => mapped_val,
    };

    let set_fid = cx.rt_fns["rt_array_set"];
    let set_ref = cx.module.declare_func_in_func(set_fid, cx.builder.func);
    let idx_val2 = cx.builder.use_var(idx_var);
    cx.builder
        .ins()
        .call(set_ref, &[result_ptr, idx_val2, store_val]);

    let current_idx = cx.builder.use_var(idx_var);
    let one = cx.builder.ins().iconst(types::I64, 1);
    let next_idx = cx.builder.ins().iadd(current_idx, one);
    cx.builder.def_var(idx_var, next_idx);
    cx.builder.ins().jump(header_block, &[]);

    cx.builder.seal_block(header_block);

    cx.builder.switch_to_block(exit_block);
    cx.builder.seal_block(exit_block);

    let result_elem_tty = ret_tty;
    Ok(Some((
        result_ptr,
        TurboTy::Array(Box::new(result_elem_tty)),
    )))
}

/// compile_builtin_filter: filter(arr, fn) -> [T]
/// Allocates same-size array, filters elements by predicate, patches length.
pub(crate) fn compile_builtin_filter<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (arr_ptr, arr_tty) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_builtin_filter: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let (closure_ptr, fn_tty) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_builtin_filter: `&args[1]` produced no value during code generation"
            .to_string(),
    })?;

    let elem_tty = match &arr_tty {
        TurboTy::Array(inner) => *inner.clone(),
        _ => TurboTy::Int,
    };

    let param_tty = match &fn_tty {
        TurboTy::Fn(params, _) => params[0].clone(),
        _ => TurboTy::Int,
    };
    // The predicate's return ABI must match the closure's actual signature.
    // Inferred/expression-body closures are declared with an `int` (i64) return
    // (see closure declaration in compile.rs); explicit `-> bool` closures use
    // i8. Derive it from the closure's `fn_tty` exactly as `map`/`reduce` do —
    // hardcoding the return type disagreed with the closure's real signature
    // and produced a Cranelift verifier error. `brif` treats either width as
    // truthy (non-zero), so no narrowing of the result is needed.
    let pred_ret_tty = match &fn_tty {
        TurboTy::Fn(_, ret) => *ret.clone(),
        _ => TurboTy::Int,
    };

    // Extract fn_ptr and env_ptr from closure pair struct
    let fn_ptr = cx
        .builder
        .ins()
        .load(cx.ptr_type, MemFlags::new(), closure_ptr, 0);
    let env_ptr = cx
        .builder
        .ins()
        .load(cx.ptr_type, MemFlags::new(), closure_ptr, 8);

    let len_fid = cx.rt_fns["rt_array_len"];
    let len_ref = cx.module.declare_func_in_func(len_fid, cx.builder.func);
    let call = cx.builder.ins().call(len_ref, &[arr_ptr]);
    let arr_len = cx.builder.inst_results(call)[0];

    let alloc_fid = cx.rt_fns["rt_array_alloc"];
    let alloc_ref = cx.module.declare_func_in_func(alloc_fid, cx.builder.func);
    let alloc_call = cx.builder.ins().call(alloc_ref, &[arr_len]);
    let result_ptr = cx.builder.inst_results(alloc_call)[0];

    let mut sig = cx.module.make_signature();
    sig.call_conv = CallConv::Fast;
    sig.params.push(AbiParam::new(cx.ptr_type)); // env_ptr
    let param_cl_ty = turbo_ty_to_cl_type(&param_tty, cx.ptr_type);
    sig.params.push(AbiParam::new(param_cl_ty));
    let pred_ret_cl_ty = turbo_ty_to_cl_type(&pred_ret_tty, cx.ptr_type);
    sig.returns.push(AbiParam::new(pred_ret_cl_ty));
    let sig_ref = cx.builder.import_signature(sig);

    let idx_var = cx.fresh_var(types::I64, TurboTy::Int);
    let zero = cx.builder.ins().iconst(types::I64, 0);
    cx.builder.def_var(idx_var, zero);

    let out_var = cx.fresh_var(types::I64, TurboTy::Int);
    let zero2 = cx.builder.ins().iconst(types::I64, 0);
    cx.builder.def_var(out_var, zero2);

    let header_block = cx.builder.create_block();
    let body_block = cx.builder.create_block();
    let store_block = cx.builder.create_block();
    let inc_block = cx.builder.create_block();
    let exit_block = cx.builder.create_block();

    cx.builder.ins().jump(header_block, &[]);

    cx.builder.switch_to_block(header_block);
    let idx = cx.builder.use_var(idx_var);
    let cond = cx.builder.ins().icmp(IntCC::SignedLessThan, idx, arr_len);
    cx.builder
        .ins()
        .brif(cond, body_block, &[], exit_block, &[]);

    cx.builder.switch_to_block(body_block);
    cx.builder.seal_block(body_block);

    let get_fid = cx.rt_fns["rt_array_get"];
    let get_ref = cx.module.declare_func_in_func(get_fid, cx.builder.func);
    let idx_val = cx.builder.use_var(idx_var);
    let get_call = cx.builder.ins().call(get_ref, &[arr_ptr, idx_val]);
    let raw_elem = cx.builder.inst_results(get_call)[0];

    let typed_elem = match &param_tty {
        TurboTy::Bool => cx.builder.ins().ireduce(types::I8, raw_elem),
        TurboTy::Float => cx
            .builder
            .ins()
            .bitcast(types::F64, MemFlags::new(), raw_elem),
        _ => raw_elem,
    };

    let indirect_call = cx
        .builder
        .ins()
        .call_indirect(sig_ref, fn_ptr, &[env_ptr, typed_elem]);
    let pred_result = cx.builder.inst_results(indirect_call)[0];

    cx.builder
        .ins()
        .brif(pred_result, store_block, &[], inc_block, &[]);

    cx.builder.switch_to_block(store_block);
    cx.builder.seal_block(store_block);

    let set_fid = cx.rt_fns["rt_array_set"];
    let set_ref = cx.module.declare_func_in_func(set_fid, cx.builder.func);
    let out_idx = cx.builder.use_var(out_var);
    cx.builder
        .ins()
        .call(set_ref, &[result_ptr, out_idx, raw_elem]);

    let one = cx.builder.ins().iconst(types::I64, 1);
    let next_out = cx.builder.ins().iadd(out_idx, one);
    cx.builder.def_var(out_var, next_out);

    cx.builder.ins().jump(inc_block, &[]);

    cx.builder.switch_to_block(inc_block);
    cx.builder.seal_block(inc_block);

    let current_idx = cx.builder.use_var(idx_var);
    let one2 = cx.builder.ins().iconst(types::I64, 1);
    let next_idx = cx.builder.ins().iadd(current_idx, one2);
    cx.builder.def_var(idx_var, next_idx);
    cx.builder.ins().jump(header_block, &[]);

    cx.builder.seal_block(header_block);

    cx.builder.switch_to_block(exit_block);
    cx.builder.seal_block(exit_block);

    let final_count = cx.builder.use_var(out_var);
    cx.builder
        .ins()
        .store(MemFlags::new(), final_count, result_ptr, 0);

    Ok(Some((result_ptr, TurboTy::Array(Box::new(elem_tty)))))
}

/// compile_builtin_reduce: reduce(arr, init, fn) -> U
/// Folds through the array calling fn(acc, elem) for each element.
pub(crate) fn compile_builtin_reduce<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (arr_ptr, _arr_tty) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_builtin_reduce: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let (init_val, init_tty) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_builtin_reduce: `&args[1]` produced no value during code generation"
            .to_string(),
    })?;
    let (closure_ptr, fn_tty) = compile_expr(cx, &args[2])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_builtin_reduce: `&args[2]` produced no value during code generation"
            .to_string(),
    })?;

    let (acc_tty, elem_tty, ret_tty) = match &fn_tty {
        TurboTy::Fn(params, ret) => (params[0].clone(), params[1].clone(), *ret.clone()),
        _ => (TurboTy::Int, TurboTy::Int, TurboTy::Int),
    };

    // Extract fn_ptr and env_ptr from closure pair struct
    let fn_ptr = cx
        .builder
        .ins()
        .load(cx.ptr_type, MemFlags::new(), closure_ptr, 0);
    let env_ptr = cx
        .builder
        .ins()
        .load(cx.ptr_type, MemFlags::new(), closure_ptr, 8);

    let acc_cl_ty = turbo_ty_to_cl_type(&acc_tty, cx.ptr_type);
    let elem_cl_ty = turbo_ty_to_cl_type(&elem_tty, cx.ptr_type);
    let ret_cl_ty = turbo_ty_to_cl_type(&ret_tty, cx.ptr_type);

    let len_fid = cx.rt_fns["rt_array_len"];
    let len_ref = cx.module.declare_func_in_func(len_fid, cx.builder.func);
    let call = cx.builder.ins().call(len_ref, &[arr_ptr]);
    let arr_len = cx.builder.inst_results(call)[0];

    let mut sig = cx.module.make_signature();
    sig.call_conv = CallConv::Fast;
    sig.params.push(AbiParam::new(cx.ptr_type)); // env_ptr
    sig.params.push(AbiParam::new(acc_cl_ty));
    sig.params.push(AbiParam::new(elem_cl_ty));
    sig.returns.push(AbiParam::new(ret_cl_ty));
    let sig_ref = cx.builder.import_signature(sig);

    let acc_var = cx.fresh_var(acc_cl_ty, acc_tty.clone());
    cx.builder.def_var(acc_var, init_val);

    let idx_var = cx.fresh_var(types::I64, TurboTy::Int);
    let zero = cx.builder.ins().iconst(types::I64, 0);
    cx.builder.def_var(idx_var, zero);

    let header_block = cx.builder.create_block();
    let body_block = cx.builder.create_block();
    let exit_block = cx.builder.create_block();

    cx.builder.ins().jump(header_block, &[]);

    cx.builder.switch_to_block(header_block);
    let idx = cx.builder.use_var(idx_var);
    let cond = cx.builder.ins().icmp(IntCC::SignedLessThan, idx, arr_len);
    cx.builder
        .ins()
        .brif(cond, body_block, &[], exit_block, &[]);

    cx.builder.switch_to_block(body_block);
    cx.builder.seal_block(body_block);

    let get_fid = cx.rt_fns["rt_array_get"];
    let get_ref = cx.module.declare_func_in_func(get_fid, cx.builder.func);
    let idx_val = cx.builder.use_var(idx_var);
    let get_call = cx.builder.ins().call(get_ref, &[arr_ptr, idx_val]);
    let raw_elem = cx.builder.inst_results(get_call)[0];

    let typed_elem = match &elem_tty {
        TurboTy::Bool => cx.builder.ins().ireduce(types::I8, raw_elem),
        TurboTy::Float => cx
            .builder
            .ins()
            .bitcast(types::F64, MemFlags::new(), raw_elem),
        _ => raw_elem,
    };

    let current_acc = cx.builder.use_var(acc_var);
    let indirect_call =
        cx.builder
            .ins()
            .call_indirect(sig_ref, fn_ptr, &[env_ptr, current_acc, typed_elem]);
    let new_acc = cx.builder.inst_results(indirect_call)[0];
    cx.builder.def_var(acc_var, new_acc);

    let current_idx = cx.builder.use_var(idx_var);
    let one = cx.builder.ins().iconst(types::I64, 1);
    let next_idx = cx.builder.ins().iadd(current_idx, one);
    cx.builder.def_var(idx_var, next_idx);
    cx.builder.ins().jump(header_block, &[]);

    cx.builder.seal_block(header_block);

    cx.builder.switch_to_block(exit_block);
    cx.builder.seal_block(exit_block);

    let final_acc = cx.builder.use_var(acc_var);
    Ok(Some((final_acc, init_tty)))
}

// ── If expression ───────────────────────────────────────────────────

fn extract_single_assign(branch: &Spanned<Expr>) -> Option<(&str, &Spanned<Expr>)> {
    match &branch.node {
        Expr::Block { stmts, tail_expr } if stmts.len() == 1 && tail_expr.is_none() => {
            if let Stmt::Expr(ref inner) = stmts[0].node {
                if let Expr::Assign { target, value } = &inner.node {
                    return Some((target.as_str(), value));
                }
            }
            None
        }
        Expr::Assign { target, value } => Some((target.as_str(), value)),
        _ => None,
    }
}

fn is_pure_expr(expr: &Expr) -> bool {
    match expr {
        Expr::IntLit(_) | Expr::FloatLit(_) | Expr::BoolLit(_) | Expr::Ident(_) => true,
        Expr::BinaryOp { left, op, right } => {
            // Division/modulo by a non-literal (or literal zero) can trap, so
            // it isn't pure.
            if matches!(op, BinOp::Div | BinOp::Mod)
                && !matches!(right.node, Expr::IntLit(n) if n != 0)
            {
                return false;
            }
            is_pure_expr(&left.node) && is_pure_expr(&right.node)
        }
        Expr::UnaryOp { expr, .. } => is_pure_expr(&expr.node),
        _ => false,
    }
}

pub(crate) fn compile_if<M: Module>(
    cx: &mut Ctx<'_, M>,
    condition: &Spanned<Expr>,
    then_branch: &Spanned<Expr>,
    else_branch: Option<&Spanned<Expr>>,
) -> Result<MaybeTyped, CodegenError> {
    // Select optimization: if cond { x = a } else { x = b }
    // → compute both a and b, then x = select(cond, a, b)
    if let Some(else_br) = else_branch {
        if let (Some((then_target, then_val)), Some((else_target, else_val))) = (
            extract_single_assign(then_branch),
            extract_single_assign(else_br),
        ) {
            if then_target == else_target
                && is_pure_expr(&then_val.node)
                && is_pure_expr(&else_val.node)
            {
                if let Some((var, _, _)) = cx.vars.get(then_target) {
                    let var = *var;
                    let (then_v, _) = compile_expr(cx, then_val)?.ok_or_else(|| CodegenError {
                        code: ErrorCode::E0400,
                        message: "compile_if: `then_val` produced no value during code generation"
                            .to_string(),
                    })?;
                    let (else_v, _) = compile_expr(cx, else_val)?.ok_or_else(|| CodegenError {
                        code: ErrorCode::E0400,
                        message: "compile_if: `else_val` produced no value during code generation"
                            .to_string(),
                    })?;
                    let (cond, _) = compile_expr(cx, condition)?.ok_or_else(|| CodegenError {
                        code: ErrorCode::E0400,
                        message: "compile_if: `condition` produced no value during code generation"
                            .to_string(),
                    })?;
                    let cond_bool = cx.to_bool(cond);
                    let result = cx.builder.ins().select(cond_bool, then_v, else_v);
                    cx.builder.def_var(var, result);
                    return Ok(None);
                }
            }
        }
    }

    let (cond, _) = compile_expr(cx, condition)?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_if: `condition` produced no value during code generation".to_string(),
    })?;
    let cond_bool = cx.to_bool(cond);

    let then_block = cx.builder.create_block();
    let else_block = cx.builder.create_block();
    let merge_block = cx.builder.create_block();

    cx.builder
        .ins()
        .brif(cond_bool, then_block, &[], else_block, &[]);

    // An `if` yields a value only when it has an `else` and both arms produce
    // one (checked below). Without an `else` it is a statement, so the
    // then-branch's value must be discarded — passing it into the merge block
    // (which then has zero params) produces malformed SSA that crashes
    // Cranelift's remove_constant_phis pass. This bit gates the then-jump's
    // argument so its count always matches the merge block's param count.
    let can_yield_value = else_branch.is_some();

    // Then
    cx.builder.switch_to_block(then_block);
    cx.builder.seal_block(then_block);
    let then_result = compile_expr(cx, then_branch)?;
    let then_needs_jump = !cx.builder.is_unreachable();
    if then_needs_jump {
        match then_result {
            Some((v, _)) if can_yield_value => cx.builder.ins().jump(merge_block, &[v]),
            _ => cx.builder.ins().jump(merge_block, &[]),
        };
    }

    // Else
    cx.builder.switch_to_block(else_block);
    cx.builder.seal_block(else_block);
    let else_result = if let Some(else_expr) = else_branch {
        compile_expr(cx, else_expr)?
    } else {
        None
    };
    let else_needs_jump = !cx.builder.is_unreachable();
    if else_needs_jump {
        if let Some((v, _)) = else_result {
            cx.builder.ins().jump(merge_block, &[v]);
        } else {
            cx.builder.ins().jump(merge_block, &[]);
        }
    }

    // Merge
    cx.builder.switch_to_block(merge_block);
    cx.builder.seal_block(merge_block);

    // A branch contributes a value-carrying edge to the merge block iff it is
    // reachable (`*_needs_jump`), the `if` can yield a value (`can_yield_value`),
    // and the branch actually produced one. The merge block needs exactly one
    // param iff at least one such edge exists. Crucially we must NOT require
    // *both* arms to yield: when one arm diverges (`exit`/`panic`/`return`) it
    // emits no jump, so the surviving arm alone defines the value and the merge
    // block has a single value-carrying predecessor. Requiring both-`Some` here
    // appended zero params while the live arm still jumped with an argument —
    // malformed SSA that crashed Cranelift's remove_constant_phis pass.
    let then_edge = then_needs_jump && can_yield_value;
    let else_edge = else_needs_jump && can_yield_value;
    let then_yielded = then_edge && then_result.is_some();
    let else_yielded = else_edge && else_result.is_some();

    if then_yielded || else_yielded {
        let (val_for_ty, tty) = match (&then_result, &else_result) {
            (Some((v, t)), _) if then_yielded => (*v, t.clone()),
            (_, Some((v, t))) => (*v, t.clone()),
            _ => unreachable!("then_yielded || else_yielded guarantees one Some"),
        };
        let ty = cx.builder.func.dfg.value_type(val_for_ty);
        cx.builder.append_block_param(merge_block, ty);
        let param = cx.builder.block_params(merge_block)[0];
        Ok(Some((param, tty)))
    } else {
        Ok(None)
    }
}

// ── If-let pattern matching ────────────────────────────────────────

pub(crate) fn compile_if_let<M: Module>(
    cx: &mut Ctx<'_, M>,
    pattern: &Spanned<Pattern>,
    value: &Spanned<Expr>,
    then_branch: &Spanned<Expr>,
    else_branch: Option<&Spanned<Expr>>,
) -> Result<MaybeTyped, CodegenError> {
    // Compile the value expression
    let (val, val_tty) = compile_expr(cx, value)?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_if_let: `value` produced no value during code generation".to_string(),
    })?;

    // Determine the tag check based on the pattern
    let matches_cond = match &pattern.node {
        Pattern::Some(_) => {
            let tag_fid = cx.rt_fns["rt_option_tag"];
            let tag_fref = cx.module.declare_func_in_func(tag_fid, cx.builder.func);
            let tag_call = cx.builder.ins().call(tag_fref, &[val]);
            let tag = cx.builder.inst_results(tag_call)[0];
            let one = cx.builder.ins().iconst(types::I64, 1);
            cx.builder.ins().icmp(IntCC::Equal, tag, one)
        }
        Pattern::None => {
            let tag_fid = cx.rt_fns["rt_option_tag"];
            let tag_fref = cx.module.declare_func_in_func(tag_fid, cx.builder.func);
            let tag_call = cx.builder.ins().call(tag_fref, &[val]);
            let tag = cx.builder.inst_results(tag_call)[0];
            let zero = cx.builder.ins().iconst(types::I64, 0);
            cx.builder.ins().icmp(IntCC::Equal, tag, zero)
        }
        Pattern::Ok(_) => {
            let tag_fid = cx.rt_fns["rt_result_tag"];
            let tag_fref = cx.module.declare_func_in_func(tag_fid, cx.builder.func);
            let tag_call = cx.builder.ins().call(tag_fref, &[val]);
            let tag = cx.builder.inst_results(tag_call)[0];
            let zero = cx.builder.ins().iconst(types::I64, 0);
            cx.builder.ins().icmp(IntCC::Equal, tag, zero)
        }
        Pattern::Err(_) => {
            let tag_fid = cx.rt_fns["rt_result_tag"];
            let tag_fref = cx.module.declare_func_in_func(tag_fid, cx.builder.func);
            let tag_call = cx.builder.ins().call(tag_fref, &[val]);
            let tag = cx.builder.inst_results(tag_call)[0];
            let one = cx.builder.ins().iconst(types::I64, 1);
            cx.builder.ins().icmp(IntCC::Equal, tag, one)
        }
        _ => {
            return Err(CodegenError {
                code: ErrorCode::E0400,
                message: "unsupported pattern in `if let`".to_string(),
            });
        }
    };

    let then_block = cx.builder.create_block();
    let else_block = cx.builder.create_block();
    let merge_block = cx.builder.create_block();

    cx.builder
        .ins()
        .brif(matches_cond, then_block, &[], else_block, &[]);

    // Then block: bind the pattern variable and compile the then branch
    cx.builder.switch_to_block(then_block);
    cx.builder.seal_block(then_block);

    let saved_vars = cx.vars.clone();

    // Bind the extracted value to the pattern variable
    match &pattern.node {
        Pattern::Some(binding) => {
            let val_fid = cx.rt_fns["rt_option_value"];
            let val_fref = cx.module.declare_func_in_func(val_fid, cx.builder.func);
            let val_call = cx.builder.ins().call(val_fref, &[val]);
            let raw_val = cx.builder.inst_results(val_call)[0];

            let turbo_ty = match &val_tty {
                TurboTy::Optional(inner_tty) => *inner_tty.clone(),
                _ => TurboTy::Int,
            };

            // A `float?` stores its payload as raw i64 bits; bind it as F64 so
            // later float arithmetic doesn't fadd an i64-classed register
            // (backend register-class panic). Mirrors the Ok/Err arm below.
            let (cl_ty, bind_val) = if matches!(turbo_ty, TurboTy::Float) {
                (
                    types::F64,
                    cx.builder
                        .ins()
                        .bitcast(types::F64, MemFlags::new(), raw_val),
                )
            } else {
                (types::I64, raw_val)
            };

            let var = Variable::new(cx.next_var);
            cx.next_var += 1;
            cx.builder.declare_var(var, cl_ty);
            cx.builder.def_var(var, bind_val);
            cx.vars.insert(binding.clone(), (var, cl_ty, turbo_ty));
        }
        Pattern::Ok(binding) | Pattern::Err(binding) => {
            let val_fid = cx.rt_fns["rt_result_value"];
            let val_fref = cx.module.declare_func_in_func(val_fid, cx.builder.func);
            let val_call = cx.builder.ins().call(val_fref, &[val]);
            let raw_val = cx.builder.inst_results(val_call)[0];

            let turbo_ty = match &val_tty {
                TurboTy::Result(ok_tty, err_tty) => {
                    if matches!(&pattern.node, Pattern::Ok(_)) {
                        *ok_tty.clone()
                    } else {
                        *err_tty.clone()
                    }
                }
                _ => TurboTy::Int,
            };

            // If the turbo type is Float, bitcast the i64 bits to f64
            let (cl_ty, bind_val) = if matches!(turbo_ty, TurboTy::Float) {
                (
                    types::F64,
                    cx.builder
                        .ins()
                        .bitcast(types::F64, MemFlags::new(), raw_val),
                )
            } else {
                (types::I64, raw_val)
            };

            let var = Variable::new(cx.next_var);
            cx.next_var += 1;
            cx.builder.declare_var(var, cl_ty);
            cx.builder.def_var(var, bind_val);
            cx.vars.insert(binding.clone(), (var, cl_ty, turbo_ty));
        }
        Pattern::None => {
            // No binding for none pattern
        }
        _ => {}
    }

    let then_result = compile_expr(cx, then_branch)?;
    let then_needs_jump = !cx.builder.is_unreachable();
    if then_needs_jump {
        if let Some((v, _)) = then_result {
            cx.builder.ins().jump(merge_block, &[v]);
        } else {
            cx.builder.ins().jump(merge_block, &[]);
        }
    }

    // Restore variables
    cx.vars = saved_vars;

    // Else block
    cx.builder.switch_to_block(else_block);
    cx.builder.seal_block(else_block);
    let else_result = if let Some(else_expr) = else_branch {
        compile_expr(cx, else_expr)?
    } else {
        None
    };
    let else_needs_jump = !cx.builder.is_unreachable();
    if else_needs_jump {
        if let Some((v, _)) = else_result {
            cx.builder.ins().jump(merge_block, &[v]);
        } else {
            cx.builder.ins().jump(merge_block, &[]);
        }
    }

    // Merge block. As in `compile_if`, append the param iff at least one
    // reachable arm yields a value — requiring both-`Some` would emit zero
    // params while a live arm jumps with an argument when the other arm
    // diverges (`exit`/`panic`/`return`), crashing remove_constant_phis.
    cx.builder.switch_to_block(merge_block);
    cx.builder.seal_block(merge_block);

    let then_yielded = then_needs_jump && then_result.is_some();
    let else_yielded = else_needs_jump && else_result.is_some();

    if then_yielded || else_yielded {
        let (val_for_ty, tty) = match (&then_result, &else_result) {
            (Some((v, t)), _) if then_yielded => (*v, t.clone()),
            (_, Some((v, t))) => (*v, t.clone()),
            _ => unreachable!("then_yielded || else_yielded guarantees one Some"),
        };
        let ty = cx.builder.func.dfg.value_type(val_for_ty);
        cx.builder.append_block_param(merge_block, ty);
        let param = cx.builder.block_params(merge_block)[0];
        Ok(Some((param, tty)))
    } else {
        Ok(None)
    }
}

// ── String operations ───────────────────────────────────────────────

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

pub(crate) fn compile_while<M: Module>(
    cx: &mut Ctx<'_, M>,
    condition: &Spanned<Expr>,
    body: &Spanned<Expr>,
) -> Result<MaybeTyped, CodegenError> {
    let header_block = cx.builder.create_block();
    let body_block = cx.builder.create_block();
    let exit_block = cx.builder.create_block();

    cx.builder.ins().jump(header_block, &[]);

    // Header: evaluate condition
    cx.builder.switch_to_block(header_block);
    let (cond, _) = compile_expr(cx, condition)?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_while: `condition` produced no value during code generation".to_string(),
    })?;
    let cond_bool = cx.to_bool(cond);

    cx.builder
        .ins()
        .brif(cond_bool, body_block, &[], exit_block, &[]);

    // Body
    cx.builder.switch_to_block(body_block);
    cx.builder.seal_block(body_block);
    cx.loop_stack.push((header_block, exit_block));
    compile_expr(cx, body)?;
    cx.loop_stack.pop();

    if !cx.builder.is_unreachable() {
        cx.builder.ins().jump(header_block, &[]);
    }

    cx.builder.seal_block(header_block);

    cx.builder.switch_to_block(exit_block);
    cx.builder.seal_block(exit_block);

    Ok(None)
}

// ── For-in loop ─────────────────────────────────────────────────────

pub(crate) fn compile_for_in<M: Module>(
    cx: &mut Ctx<'_, M>,
    var_name: &str,
    iterable: &Spanned<Expr>,
    body: &Spanned<Expr>,
) -> Result<MaybeTyped, CodegenError> {
    match &iterable.node {
        Expr::Range { start, end } => compile_for_in_range(cx, var_name, start, end, body),
        _ => compile_for_in_array(cx, var_name, iterable, body),
    }
}

pub(crate) fn compile_for_in_range<M: Module>(
    cx: &mut Ctx<'_, M>,
    var_name: &str,
    start: &Spanned<Expr>,
    end: &Spanned<Expr>,
    body: &Spanned<Expr>,
) -> Result<MaybeTyped, CodegenError> {
    let (range_start, _) = compile_expr(cx, start)?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_for_in_range: `start` produced no value during code generation"
            .to_string(),
    })?;
    let (range_end, _) = compile_expr(cx, end)?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_for_in_range: `end` produced no value during code generation".to_string(),
    })?;

    // Create loop variable
    let var = Variable::new(cx.next_var);
    cx.next_var += 1;
    cx.builder.declare_var(var, types::I64);
    cx.builder.def_var(var, range_start);
    cx.vars
        .insert(var_name.to_string(), (var, types::I64, TurboTy::Int));

    // Create blocks: header, body, continue (increment), exit
    let header_block = cx.builder.create_block();
    let body_block = cx.builder.create_block();
    let continue_block = cx.builder.create_block();
    let exit_block = cx.builder.create_block();

    cx.builder.ins().jump(header_block, &[]);

    // Header: check i < end
    // Do NOT seal header yet -- it has predecessors (entry + continue back edge + possible continue jumps)
    cx.builder.switch_to_block(header_block);

    let current_i = cx.builder.use_var(var);
    let cond = cx
        .builder
        .ins()
        .icmp(IntCC::SignedLessThan, current_i, range_end);
    cx.builder
        .ins()
        .brif(cond, body_block, &[], exit_block, &[]);

    // Body
    cx.builder.switch_to_block(body_block);
    cx.builder.seal_block(body_block);

    cx.loop_stack.push((continue_block, exit_block));
    compile_expr(cx, body)?;
    cx.loop_stack.pop();

    // Fall through to continue block
    if !cx.builder.is_unreachable() {
        cx.builder.ins().jump(continue_block, &[]);
    }

    // Continue block: increment i = i + 1, then jump to header
    cx.builder.switch_to_block(continue_block);
    // Don't seal continue_block yet -- it can have predecessors from body fallthrough + continue jumps
    let current_i = cx.builder.use_var(var);
    let one = cx.builder.ins().iconst(types::I64, 1);
    let next_i = cx.builder.ins().iadd(current_i, one);
    cx.builder.def_var(var, next_i);
    cx.builder.ins().jump(header_block, &[]);

    // NOW seal the header and continue block (all predecessors are known)
    cx.builder.seal_block(continue_block);
    cx.builder.seal_block(header_block);

    // Exit
    cx.builder.switch_to_block(exit_block);
    cx.builder.seal_block(exit_block);

    Ok(None)
}

pub(crate) fn compile_for_in_array<M: Module>(
    cx: &mut Ctx<'_, M>,
    var_name: &str,
    iterable: &Spanned<Expr>,
    body: &Spanned<Expr>,
) -> Result<MaybeTyped, CodegenError> {
    // Compile the array expression
    let (arr_ptr, arr_tty) = compile_expr(cx, iterable)?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_for_in_array: `iterable` produced no value during code generation"
            .to_string(),
    })?;

    // Inline array length load (first i64 at arr_ptr)
    let arr_len = cx
        .builder
        .ins()
        .load(types::I64, MemFlags::trusted(), arr_ptr, 0i32);

    // Determine element TurboTy from the array type
    let elem_tty = match arr_tty {
        TurboTy::Array(ref inner) => *inner.clone(),
        _ => TurboTy::Int, // fallback
    };

    // Determine Cranelift type for the element
    let elem_cl_ty = match &elem_tty {
        TurboTy::Float => types::F64,
        TurboTy::Bool => types::I8,
        _ => types::I64, // Int, Str are all i64/ptr-sized
    };

    // Create index counter variable
    let idx_var = Variable::new(cx.next_var);
    cx.next_var += 1;
    cx.builder.declare_var(idx_var, types::I64);
    let zero = cx.builder.ins().iconst(types::I64, 0);
    cx.builder.def_var(idx_var, zero);

    // Create element variable for the loop body
    let elem_var = Variable::new(cx.next_var);
    cx.next_var += 1;
    cx.builder.declare_var(elem_var, elem_cl_ty);
    let default_val = match &elem_tty {
        TurboTy::Float => cx.builder.ins().f64const(0.0),
        _ => cx.builder.ins().iconst(elem_cl_ty, 0),
    };
    cx.builder.def_var(elem_var, default_val);
    // The loop variable is a *borrow* of an array element, not an owned value:
    // the loop never retains it (no rt_retain on the element). It must therefore
    // never be released either, or the same element struct/array gets freed once
    // per iterating loop — a double-free when several loops walk the same array
    // (e.g. count_done/count_high/save_tasks all iterating `tasks`). We stash any
    // prior binding for this name and restore it after the loop so the enclosing
    // block's cleanup pass never sees the loop var as an owned local to release.
    let prev_binding = cx.vars.get(var_name).cloned();
    cx.vars.insert(
        var_name.to_string(),
        (elem_var, elem_cl_ty, elem_tty.clone()),
    );

    // Loop blocks
    let header_block = cx.builder.create_block();
    let body_block = cx.builder.create_block();
    let continue_block = cx.builder.create_block();
    let exit_block = cx.builder.create_block();

    cx.builder.ins().jump(header_block, &[]);

    // Header: check idx < len
    // Do NOT seal header yet -- it has predecessors (entry + continue back edge)
    cx.builder.switch_to_block(header_block);
    let idx = cx.builder.use_var(idx_var);
    let cond = cx.builder.ins().icmp(IntCC::SignedLessThan, idx, arr_len);
    cx.builder
        .ins()
        .brif(cond, body_block, &[], exit_block, &[]);

    // Body: load element, execute body
    cx.builder.switch_to_block(body_block);
    cx.builder.seal_block(body_block);

    // Inline array element load (bounds already checked by idx < len in header)
    let idx_val = cx.builder.use_var(idx_var);
    let data_base = cx.builder.ins().iadd_imm(arr_ptr, 8);
    let byte_offset = cx.builder.ins().ishl_imm(idx_val, 3);
    let elem_ptr = cx.builder.ins().iadd(data_base, byte_offset);
    let raw_elem = cx
        .builder
        .ins()
        .load(types::I64, MemFlags::trusted(), elem_ptr, 0i32);

    // Raw i64 bits; convert to the correct type
    let typed_elem = match &elem_tty {
        TurboTy::Bool => cx.builder.ins().ireduce(types::I8, raw_elem),
        TurboTy::Float => cx
            .builder
            .ins()
            .bitcast(types::F64, MemFlags::new(), raw_elem),
        _ => raw_elem,
    };
    cx.builder.def_var(elem_var, typed_elem);

    // Compile loop body
    cx.loop_stack.push((continue_block, exit_block));
    compile_expr(cx, body)?;
    cx.loop_stack.pop();

    // Fall through to continue block
    if !cx.builder.is_unreachable() {
        cx.builder.ins().jump(continue_block, &[]);
    }

    // Continue block: increment index, then jump to header
    cx.builder.switch_to_block(continue_block);
    let current_idx = cx.builder.use_var(idx_var);
    let one = cx.builder.ins().iconst(types::I64, 1);
    let next_idx = cx.builder.ins().iadd(current_idx, one);
    cx.builder.def_var(idx_var, next_idx);
    cx.builder.ins().jump(header_block, &[]);

    // NOW seal header and continue block (all predecessors are known)
    cx.builder.seal_block(continue_block);
    cx.builder.seal_block(header_block);

    // Exit
    cx.builder.switch_to_block(exit_block);
    cx.builder.seal_block(exit_block);

    // Restore the prior binding for the loop variable name (or remove it). This
    // takes the loop var out of scope so the enclosing block won't release it —
    // it was a borrow of an array element, never retained.
    match prev_binding {
        Some(binding) => {
            cx.vars.insert(var_name.to_string(), binding);
        }
        None => {
            cx.vars.remove(var_name);
        }
    }

    Ok(None)
}

// ── Match expression ────────────────────────────────────────────────

pub(crate) fn compile_match<M: Module>(
    cx: &mut Ctx<'_, M>,
    subject: &Spanned<Expr>,
    arms: &[MatchArm],
) -> Result<MaybeTyped, CodegenError> {
    let (subj_val, subj_tty) = compile_expr(cx, subject)?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_match: `subject` produced no value during code generation".to_string(),
    })?;

    if arms.is_empty() {
        return Ok(None);
    }

    let merge_block = cx.builder.create_block();
    let mut has_result = false;
    let mut result_turbo_ty = TurboTy::Unit;
    let mut hit_catchall = false;

    for (i, arm) in arms.iter().enumerate() {
        let is_last = i == arms.len() - 1;
        let has_guard = arm.guard.is_some();
        // A catch-all pattern is only truly unconditional when it has no guard.
        let is_catchall_pattern = matches!(&arm.pattern.node, Pattern::Wildcard)
            || matches!(&arm.pattern.node, Pattern::Ident(name)
                if lookup_variant_tag_static(cx.enum_variants, name).is_none());
        let is_catchall = is_catchall_pattern && !has_guard;

        if is_catchall {
            // Unconditional arm -- bind variable if ident pattern, compile body
            let saved_vars = cx.vars.clone();
            if let Pattern::Ident(name) = &arm.pattern.node {
                let cl_ty = cx.builder.func.dfg.value_type(subj_val);
                let var = Variable::new(cx.next_var);
                cx.next_var += 1;
                cx.builder.declare_var(var, cl_ty);
                cx.builder.def_var(var, subj_val);
                cx.vars.insert(name.clone(), (var, cl_ty, subj_tty.clone()));
            }
            let body_result = compile_expr(cx, &arm.body)?;
            cx.vars = saved_vars;
            emit_match_arm_jump(
                cx,
                merge_block,
                body_result,
                &mut has_result,
                &mut result_turbo_ty,
            );
            hit_catchall = true;

            if !is_last {
                let dead_block = cx.builder.create_block();
                cx.builder.switch_to_block(dead_block);
                cx.builder.seal_block(dead_block);
            }
            break;
        }

        // For catch-all patterns with a guard, skip pattern condition check.
        let next_block = cx.builder.create_block();

        if is_catchall_pattern && has_guard {
            let match_block = cx.builder.create_block();
            cx.builder.ins().jump(match_block, &[]);
            cx.builder.switch_to_block(match_block);
            cx.builder.seal_block(match_block);
        } else {
            // Conditional arm: compute whether the pattern matches
            let matches_cond = match &arm.pattern.node {
                Pattern::Ident(name) => {
                    let tag_val = lookup_variant_tag_static(cx.enum_variants, name).unwrap();
                    let pat_val = cx.builder.ins().iconst(types::I64, tag_val as i64);
                    let actual_tag = if let TurboTy::Enum(ref enum_name) = subj_tty {
                        if cx.enum_max_slots.contains_key(enum_name.as_str()) {
                            cx.builder
                                .ins()
                                .load(types::I64, MemFlags::new(), subj_val, 0)
                        } else {
                            subj_val
                        }
                    } else {
                        subj_val
                    };
                    cx.builder.ins().icmp(IntCC::Equal, actual_tag, pat_val)
                }
                Pattern::IntLit(n) => {
                    let pat_val = cx.builder.ins().iconst(types::I64, *n);
                    cx.builder.ins().icmp(IntCC::Equal, subj_val, pat_val)
                }
                Pattern::BoolLit(b) => {
                    let pat_val = cx.builder.ins().iconst(types::I8, *b as i64);
                    let subj_narrowed = if cx.builder.func.dfg.value_type(subj_val) != types::I8 {
                        cx.builder.ins().ireduce(types::I8, subj_val)
                    } else {
                        subj_val
                    };
                    cx.builder.ins().icmp(IntCC::Equal, subj_narrowed, pat_val)
                }
                Pattern::StringLit(s) => {
                    let pat_ptr = cx.create_string(s)?;
                    let fid = cx.rt_fns["rt_str_eq"];
                    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
                    let call = cx.builder.ins().call(fref, &[subj_val, pat_ptr]);
                    let eq_result = cx.builder.inst_results(call)[0];
                    let zero = cx.builder.ins().iconst(types::I8, 0);
                    cx.builder.ins().icmp(IntCC::NotEqual, eq_result, zero)
                }
                Pattern::Wildcard => unreachable!(),
                Pattern::Ok(_binding) => {
                    let tag_fid = cx.rt_fns["rt_result_tag"];
                    let tag_fref = cx.module.declare_func_in_func(tag_fid, cx.builder.func);
                    let tag_call = cx.builder.ins().call(tag_fref, &[subj_val]);
                    let tag = cx.builder.inst_results(tag_call)[0];
                    let zero = cx.builder.ins().iconst(types::I64, 0);
                    cx.builder.ins().icmp(IntCC::Equal, tag, zero)
                }
                Pattern::Err(_binding) => {
                    let tag_fid = cx.rt_fns["rt_result_tag"];
                    let tag_fref = cx.module.declare_func_in_func(tag_fid, cx.builder.func);
                    let tag_call = cx.builder.ins().call(tag_fref, &[subj_val]);
                    let tag = cx.builder.inst_results(tag_call)[0];
                    let one = cx.builder.ins().iconst(types::I64, 1);
                    cx.builder.ins().icmp(IntCC::Equal, tag, one)
                }
                Pattern::Some(_binding) => {
                    let tag_fid = cx.rt_fns["rt_option_tag"];
                    let tag_fref = cx.module.declare_func_in_func(tag_fid, cx.builder.func);
                    let tag_call = cx.builder.ins().call(tag_fref, &[subj_val]);
                    let tag = cx.builder.inst_results(tag_call)[0];
                    let one = cx.builder.ins().iconst(types::I64, 1);
                    cx.builder.ins().icmp(IntCC::Equal, tag, one)
                }
                Pattern::None => {
                    let tag_fid = cx.rt_fns["rt_option_tag"];
                    let tag_fref = cx.module.declare_func_in_func(tag_fid, cx.builder.func);
                    let tag_call = cx.builder.ins().call(tag_fref, &[subj_val]);
                    let tag = cx.builder.inst_results(tag_call)[0];
                    let zero = cx.builder.ins().iconst(types::I64, 0);
                    cx.builder.ins().icmp(IntCC::Equal, tag, zero)
                }
                Pattern::VariantDestructure { variant, .. } => {
                    let tag_val = lookup_variant_tag_static(cx.enum_variants, variant).unwrap();
                    let pat_val = cx.builder.ins().iconst(types::I64, tag_val as i64);
                    let actual_tag =
                        cx.builder
                            .ins()
                            .load(types::I64, MemFlags::new(), subj_val, 0);
                    cx.builder.ins().icmp(IntCC::Equal, actual_tag, pat_val)
                }
            };

            let match_block = cx.builder.create_block();
            cx.builder
                .ins()
                .brif(matches_cond, match_block, &[], next_block, &[]);
            cx.builder.switch_to_block(match_block);
            cx.builder.seal_block(match_block);
        }

        // Bind pattern variables in the match block
        let saved_vars = cx.vars.clone();
        match &arm.pattern.node {
            Pattern::Ok(binding) | Pattern::Err(binding) => {
                let val_fid = cx.rt_fns["rt_result_value"];
                let val_fref = cx.module.declare_func_in_func(val_fid, cx.builder.func);
                let val_call = cx.builder.ins().call(val_fref, &[subj_val]);
                let raw_val = cx.builder.inst_results(val_call)[0];

                let turbo_ty = match &subj_tty {
                    TurboTy::Result(ok_tty, err_tty) => {
                        if matches!(&arm.pattern.node, Pattern::Ok(_)) {
                            *ok_tty.clone()
                        } else {
                            *err_tty.clone()
                        }
                    }
                    _ => TurboTy::Int,
                };

                // If the turbo type is Float, bitcast the i64 bits to f64
                let (cl_ty, val) = if matches!(turbo_ty, TurboTy::Float) {
                    (
                        types::F64,
                        cx.builder
                            .ins()
                            .bitcast(types::F64, MemFlags::new(), raw_val),
                    )
                } else {
                    (types::I64, raw_val)
                };

                let var = Variable::new(cx.next_var);
                cx.next_var += 1;
                cx.builder.declare_var(var, cl_ty);
                cx.builder.def_var(var, val);
                cx.vars.insert(binding.clone(), (var, cl_ty, turbo_ty));
            }
            Pattern::Some(binding) => {
                let val_fid = cx.rt_fns["rt_option_value"];
                let val_fref = cx.module.declare_func_in_func(val_fid, cx.builder.func);
                let val_call = cx.builder.ins().call(val_fref, &[subj_val]);
                let raw_val = cx.builder.inst_results(val_call)[0];

                let turbo_ty = match &subj_tty {
                    TurboTy::Optional(inner_tty) => *inner_tty.clone(),
                    _ => TurboTy::Int,
                };

                // A `float?` payload is stored as raw i64 bits; bind it as F64
                // so float arithmetic on the binding is well-typed. Mirrors the
                // Ok/Err arm above (the Some arm previously forgot to bitcast).
                let (cl_ty, bind_val) = if matches!(turbo_ty, TurboTy::Float) {
                    (
                        types::F64,
                        cx.builder
                            .ins()
                            .bitcast(types::F64, MemFlags::new(), raw_val),
                    )
                } else {
                    (types::I64, raw_val)
                };

                let var = Variable::new(cx.next_var);
                cx.next_var += 1;
                cx.builder.declare_var(var, cl_ty);
                cx.builder.def_var(var, bind_val);
                cx.vars.insert(binding.clone(), (var, cl_ty, turbo_ty));
            }
            Pattern::VariantDestructure { variant, bindings } => {
                let field_tys = if let TurboTy::Enum(ref enum_name) = subj_tty {
                    cx.enum_variant_fields
                        .get(&(enum_name.clone(), variant.clone()))
                        .cloned()
                        .unwrap_or_default()
                } else {
                    Vec::new()
                };

                for (i, binding) in bindings.iter().enumerate() {
                    let offset = ((i + 1) * 8) as i32;
                    let raw_val =
                        cx.builder
                            .ins()
                            .load(types::I64, MemFlags::new(), subj_val, offset);

                    let field_tty = if i < field_tys.len() {
                        field_tys[i].clone()
                    } else {
                        TurboTy::Int
                    };

                    let (val, cl_ty) = match &field_tty {
                        TurboTy::Float => {
                            let f = cx
                                .builder
                                .ins()
                                .bitcast(types::F64, MemFlags::new(), raw_val);
                            (f, types::F64)
                        }
                        TurboTy::Bool => {
                            let b = cx.builder.ins().ireduce(types::I8, raw_val);
                            (b, types::I8)
                        }
                        _ => (raw_val, types::I64),
                    };

                    let var = Variable::new(cx.next_var);
                    cx.next_var += 1;
                    cx.builder.declare_var(var, cl_ty);
                    cx.builder.def_var(var, val);
                    cx.vars.insert(binding.clone(), (var, cl_ty, field_tty));
                }
            }
            Pattern::Ident(name) if is_catchall_pattern => {
                // Catch-all ident with guard: bind subject value as variable
                let cl_ty = cx.builder.func.dfg.value_type(subj_val);
                let var = Variable::new(cx.next_var);
                cx.next_var += 1;
                cx.builder.declare_var(var, cl_ty);
                cx.builder.def_var(var, subj_val);
                cx.vars.insert(name.clone(), (var, cl_ty, subj_tty.clone()));
            }
            _ => {}
        }

        // If there is a guard, evaluate it: true -> body, false -> next arm
        if let Some(ref guard) = arm.guard {
            let guard_result = compile_expr(cx, guard)?;
            if let Some((guard_val, _)) = guard_result {
                let body_block = cx.builder.create_block();
                cx.builder
                    .ins()
                    .brif(guard_val, body_block, &[], next_block, &[]);
                cx.builder.switch_to_block(body_block);
                cx.builder.seal_block(body_block);
            }
        }

        let body_result = compile_expr(cx, &arm.body)?;
        cx.vars = saved_vars;
        emit_match_arm_jump(
            cx,
            merge_block,
            body_result,
            &mut has_result,
            &mut result_turbo_ty,
        );

        // Continue to next arm's check
        cx.builder.switch_to_block(next_block);
        cx.builder.seal_block(next_block);
    }

    // If no catchall was reached, we're in the fallthrough block (no arm matched).
    if !hit_catchall && !cx.builder.is_unreachable() {
        // Call rt_panic with a clear message instead of a bare trap
        let msg = cx.create_string("non-exhaustive match")?;
        cx.rt_call("rt_panic", &[msg]);
        cx.builder.ins().trap(TrapCode::unwrap_user(1));
    }

    cx.builder.switch_to_block(merge_block);
    cx.builder.seal_block(merge_block);

    if has_result {
        let param = cx.builder.block_params(merge_block)[0];
        Ok(Some((param, result_turbo_ty)))
    } else {
        Ok(None)
    }
}

/// Helper: emit the jump from a compiled match arm body to the merge block.
fn emit_match_arm_jump<M: Module>(
    cx: &mut Ctx<'_, M>,
    merge_block: cranelift::prelude::Block,
    body_result: MaybeTyped,
    has_result: &mut bool,
    result_turbo_ty: &mut TurboTy,
) {
    let needs_jump = !cx.builder.is_unreachable();
    if let Some((val, tty)) = body_result {
        if !*has_result {
            *has_result = true;
            let cl_ty = cx.builder.func.dfg.value_type(val);
            *result_turbo_ty = tty;
            cx.builder.append_block_param(merge_block, cl_ty);
        }
        if needs_jump {
            cx.builder.ins().jump(merge_block, &[val]);
        }
    } else if needs_jump {
        cx.builder.ins().jump(merge_block, &[]);
    }
}

/// Look up the integer tag for a variant name across all known enums.
fn lookup_variant_tag_static(
    enum_variants: &HashMap<String, Vec<String>>,
    variant_name: &str,
) -> Option<usize> {
    for (_enum_name, variants) in enum_variants.iter() {
        if let Some(pos) = variants.iter().position(|v| v == variant_name) {
            return Some(pos);
        }
    }
    None
}

// ── Channel builtins ────────────────────────────────────────────────

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

// ── HashMap builtins ────────────────────────────────────────────────

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

// ── Map literal compilation ────────────────────────────────────────

/// Compile a map literal: {"key": "value", ...}
/// Creates a new hashmap and inserts all entries.
pub(crate) fn compile_map_lit<M: Module>(
    cx: &mut Ctx<'_, M>,
    entries: &[(Spanned<Expr>, Spanned<Expr>)],
) -> Result<MaybeTyped, CodegenError> {
    // Call rt_hashmap_new
    let new_fid = cx.rt_fns["rt_hashmap_new"];
    let new_fref = cx.module.declare_func_in_func(new_fid, cx.builder.func);
    let new_call = cx.builder.ins().call(new_fref, &[]);
    let map_val = cx.builder.inst_results(new_call)[0];

    // For each entry, call rt_hashmap_set
    let set_fid = cx.rt_fns["rt_hashmap_set"];
    let set_fref = cx.module.declare_func_in_func(set_fid, cx.builder.func);
    for (key, value) in entries {
        let (key_val, _) = compile_expr(cx, key)?.ok_or_else(|| CodegenError {
            code: ErrorCode::E0405,
            message: "map key must return a value".to_string(),
        })?;
        let (val_val, _) = compile_expr(cx, value)?.ok_or_else(|| CodegenError {
            code: ErrorCode::E0405,
            message: "map value must return a value".to_string(),
        })?;
        cx.builder
            .ins()
            .call(set_fref, &[map_val, key_val, val_val]);
    }

    Ok(Some((map_val, TurboTy::Int)))
}

// ── Unsafe builtins — raw pointer operations ────────────────────────

/// deref(addr: i64) -> i64 — load an i64 from the given memory address
pub(crate) fn compile_builtin_deref<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (addr_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_builtin_deref: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let result = cx
        .builder
        .ins()
        .load(types::I64, MemFlags::new(), addr_val, 0);
    Ok(Some((result, TurboTy::Int)))
}

/// push(arr, elem) — returns a NEW array with `elem` appended (COW semantics)
pub(crate) fn compile_builtin_push<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    if args.len() != 2 {
        return Err(CodegenError {
            code: ErrorCode::E0400,
            message: "push() requires exactly 2 arguments".to_string(),
        });
    }
    let (arr_val, arr_tty) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_builtin_push: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let (elem_val, _) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_builtin_push: `&args[1]` produced no value during code generation"
            .to_string(),
    })?;
    let push_fid = cx.rt_fns["rt_array_push"];
    let push_ref = cx.module.declare_func_in_func(push_fid, cx.builder.func);
    let call = cx.builder.ins().call(push_ref, &[arr_val, elem_val]);
    let result = cx.builder.inst_results(call)[0];
    // Preserve the array element type from the input array
    Ok(Some((result, arr_tty)))
}

/// store(addr: i64, value: i64) — store an i64 at the given memory address
pub(crate) fn compile_builtin_store<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (addr_val, _) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_builtin_store: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let (val, _) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_builtin_store: `&args[1]` produced no value during code generation"
            .to_string(),
    })?;
    cx.builder.ins().store(MemFlags::new(), val, addr_val, 0);
    Ok(None)
}

// ── Filesystem builtins ──────────────────────────────────────────────

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

/// sort(arr) -> [T] (COW)
pub(crate) fn compile_sort<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (arr_val, arr_tty) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_sort: `&args[0]` produced no value during code generation".to_string(),
    })?;
    let elem_tty = match &arr_tty {
        TurboTy::Array(inner) => *inner.clone(),
        _ => TurboTy::Int,
    };
    let rt_name = match &elem_tty {
        TurboTy::Str => "rt_sort_str",
        _ => "rt_sort_int",
    };
    let fid = cx.rt_fns[rt_name];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[arr_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Array(Box::new(elem_tty)))))
}

/// reverse(arr) -> [T] (COW)
pub(crate) fn compile_reverse<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (arr_val, arr_tty) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_reverse: `&args[0]` produced no value during code generation".to_string(),
    })?;
    let elem_tty = match &arr_tty {
        TurboTy::Array(inner) => *inner.clone(),
        _ => TurboTy::Int,
    };
    let fid = cx.rt_fns["rt_reverse"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[arr_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Array(Box::new(elem_tty)))))
}

/// array_contains(arr, val) -> bool
pub(crate) fn compile_array_contains<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (arr_val, arr_tty) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_array_contains: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let (val, _val_tty) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_array_contains: `&args[1]` produced no value during code generation"
            .to_string(),
    })?;
    let elem_tty = match &arr_tty {
        TurboTy::Array(inner) => *inner.clone(),
        _ => TurboTy::Int,
    };
    let rt_name = match &elem_tty {
        TurboTy::Str => "rt_array_contains_str",
        _ => "rt_array_contains_int",
    };
    let fid = cx.rt_fns[rt_name];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[arr_val, val]);
    let result = cx.builder.inst_results(call)[0];
    let bool_val = cx.builder.ins().ireduce(types::I8, result);
    Ok(Some((bool_val, TurboTy::Bool)))
}

/// slice(arr, start, end) -> [T]
pub(crate) fn compile_slice<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (arr_val, arr_tty) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_slice: `&args[0]` produced no value during code generation".to_string(),
    })?;
    let (start_val, _) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_slice: `&args[1]` produced no value during code generation".to_string(),
    })?;
    let (end_val, _) = compile_expr(cx, &args[2])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_slice: `&args[2]` produced no value during code generation".to_string(),
    })?;
    let elem_tty = match &arr_tty {
        TurboTy::Array(inner) => *inner.clone(),
        _ => TurboTy::Int,
    };
    // Ensure start and end are i64
    let start_ty = cx.builder.func.dfg.value_type(start_val);
    let start_val = if start_ty.bits() < 64 {
        cx.builder.ins().sextend(types::I64, start_val)
    } else {
        start_val
    };
    let end_ty = cx.builder.func.dfg.value_type(end_val);
    let end_val = if end_ty.bits() < 64 {
        cx.builder.ins().sextend(types::I64, end_val)
    } else {
        end_val
    };
    let fid = cx.rt_fns["rt_slice"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[arr_val, start_val, end_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Array(Box::new(elem_tty)))))
}

/// any(arr, closure) -> bool
pub(crate) fn compile_builtin_any<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (arr_ptr, _arr_tty) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_builtin_any: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let (closure_ptr, fn_tty) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_builtin_any: `&args[1]` produced no value during code generation"
            .to_string(),
    })?;

    let param_tty = match &fn_tty {
        TurboTy::Fn(params, _) => params[0].clone(),
        _ => TurboTy::Int,
    };

    // Extract fn_ptr and env_ptr from closure pair struct
    let fn_ptr = cx
        .builder
        .ins()
        .load(cx.ptr_type, MemFlags::new(), closure_ptr, 0);
    let env_ptr = cx
        .builder
        .ins()
        .load(cx.ptr_type, MemFlags::new(), closure_ptr, 8);

    let len_fid = cx.rt_fns["rt_array_len"];
    let len_ref = cx.module.declare_func_in_func(len_fid, cx.builder.func);
    let call = cx.builder.ins().call(len_ref, &[arr_ptr]);
    let arr_len = cx.builder.inst_results(call)[0];

    let mut sig = cx.module.make_signature();
    sig.call_conv = CallConv::Fast;
    sig.params.push(AbiParam::new(cx.ptr_type)); // env_ptr
    let param_cl_ty = turbo_ty_to_cl_type(&param_tty, cx.ptr_type);
    sig.params.push(AbiParam::new(param_cl_ty));
    sig.returns.push(AbiParam::new(types::I8)); // bool return
    let sig_ref = cx.builder.import_signature(sig);

    // result_var: starts as false (0)
    let result_var = cx.fresh_var(types::I8, TurboTy::Bool);
    let false_val = cx.builder.ins().iconst(types::I8, 0);
    cx.builder.def_var(result_var, false_val);

    let idx_var = cx.fresh_var(types::I64, TurboTy::Int);
    let zero = cx.builder.ins().iconst(types::I64, 0);
    cx.builder.def_var(idx_var, zero);

    let header_block = cx.builder.create_block();
    let body_block = cx.builder.create_block();
    let found_block = cx.builder.create_block();
    let inc_block = cx.builder.create_block();
    let exit_block = cx.builder.create_block();

    cx.builder.ins().jump(header_block, &[]);

    cx.builder.switch_to_block(header_block);
    let idx = cx.builder.use_var(idx_var);
    let cond = cx.builder.ins().icmp(IntCC::SignedLessThan, idx, arr_len);
    cx.builder
        .ins()
        .brif(cond, body_block, &[], exit_block, &[]);

    cx.builder.switch_to_block(body_block);
    cx.builder.seal_block(body_block);

    let get_fid = cx.rt_fns["rt_array_get"];
    let get_ref = cx.module.declare_func_in_func(get_fid, cx.builder.func);
    let idx_val = cx.builder.use_var(idx_var);
    let get_call = cx.builder.ins().call(get_ref, &[arr_ptr, idx_val]);
    let raw_elem = cx.builder.inst_results(get_call)[0];

    let typed_elem = match &param_tty {
        TurboTy::Bool => cx.builder.ins().ireduce(types::I8, raw_elem),
        TurboTy::Float => cx
            .builder
            .ins()
            .bitcast(types::F64, MemFlags::new(), raw_elem),
        _ => raw_elem,
    };

    let indirect_call = cx
        .builder
        .ins()
        .call_indirect(sig_ref, fn_ptr, &[env_ptr, typed_elem]);
    let pred_result = cx.builder.inst_results(indirect_call)[0];
    cx.builder
        .ins()
        .brif(pred_result, found_block, &[], inc_block, &[]);

    // Found: set result to true and exit
    cx.builder.switch_to_block(found_block);
    cx.builder.seal_block(found_block);
    let true_val = cx.builder.ins().iconst(types::I8, 1);
    cx.builder.def_var(result_var, true_val);
    cx.builder.ins().jump(exit_block, &[]);

    // Increment and continue
    cx.builder.switch_to_block(inc_block);
    cx.builder.seal_block(inc_block);
    let current_idx = cx.builder.use_var(idx_var);
    let one = cx.builder.ins().iconst(types::I64, 1);
    let next_idx = cx.builder.ins().iadd(current_idx, one);
    cx.builder.def_var(idx_var, next_idx);
    cx.builder.ins().jump(header_block, &[]);

    cx.builder.seal_block(header_block);
    cx.builder.switch_to_block(exit_block);
    cx.builder.seal_block(exit_block);

    let result = cx.builder.use_var(result_var);
    Ok(Some((result, TurboTy::Bool)))
}

/// all(arr, closure) -> bool
pub(crate) fn compile_builtin_all<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (arr_ptr, _arr_tty) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_builtin_all: `&args[0]` produced no value during code generation"
            .to_string(),
    })?;
    let (closure_ptr, fn_tty) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_builtin_all: `&args[1]` produced no value during code generation"
            .to_string(),
    })?;

    let param_tty = match &fn_tty {
        TurboTy::Fn(params, _) => params[0].clone(),
        _ => TurboTy::Int,
    };

    let fn_ptr = cx
        .builder
        .ins()
        .load(cx.ptr_type, MemFlags::new(), closure_ptr, 0);
    let env_ptr = cx
        .builder
        .ins()
        .load(cx.ptr_type, MemFlags::new(), closure_ptr, 8);

    let len_fid = cx.rt_fns["rt_array_len"];
    let len_ref = cx.module.declare_func_in_func(len_fid, cx.builder.func);
    let call = cx.builder.ins().call(len_ref, &[arr_ptr]);
    let arr_len = cx.builder.inst_results(call)[0];

    let mut sig = cx.module.make_signature();
    sig.call_conv = CallConv::Fast;
    sig.params.push(AbiParam::new(cx.ptr_type));
    let param_cl_ty = turbo_ty_to_cl_type(&param_tty, cx.ptr_type);
    sig.params.push(AbiParam::new(param_cl_ty));
    sig.returns.push(AbiParam::new(types::I8));
    let sig_ref = cx.builder.import_signature(sig);

    // result_var: starts as true (1)
    let result_var = cx.fresh_var(types::I8, TurboTy::Bool);
    let true_val = cx.builder.ins().iconst(types::I8, 1);
    cx.builder.def_var(result_var, true_val);

    let idx_var = cx.fresh_var(types::I64, TurboTy::Int);
    let zero = cx.builder.ins().iconst(types::I64, 0);
    cx.builder.def_var(idx_var, zero);

    let header_block = cx.builder.create_block();
    let body_block = cx.builder.create_block();
    let fail_block = cx.builder.create_block();
    let inc_block = cx.builder.create_block();
    let exit_block = cx.builder.create_block();

    cx.builder.ins().jump(header_block, &[]);

    cx.builder.switch_to_block(header_block);
    let idx = cx.builder.use_var(idx_var);
    let cond = cx.builder.ins().icmp(IntCC::SignedLessThan, idx, arr_len);
    cx.builder
        .ins()
        .brif(cond, body_block, &[], exit_block, &[]);

    cx.builder.switch_to_block(body_block);
    cx.builder.seal_block(body_block);

    let get_fid = cx.rt_fns["rt_array_get"];
    let get_ref = cx.module.declare_func_in_func(get_fid, cx.builder.func);
    let idx_val = cx.builder.use_var(idx_var);
    let get_call = cx.builder.ins().call(get_ref, &[arr_ptr, idx_val]);
    let raw_elem = cx.builder.inst_results(get_call)[0];

    let typed_elem = match &param_tty {
        TurboTy::Bool => cx.builder.ins().ireduce(types::I8, raw_elem),
        TurboTy::Float => cx
            .builder
            .ins()
            .bitcast(types::F64, MemFlags::new(), raw_elem),
        _ => raw_elem,
    };

    let indirect_call = cx
        .builder
        .ins()
        .call_indirect(sig_ref, fn_ptr, &[env_ptr, typed_elem]);
    let pred_result = cx.builder.inst_results(indirect_call)[0];
    cx.builder
        .ins()
        .brif(pred_result, inc_block, &[], fail_block, &[]);

    // Fail: set result to false and exit
    cx.builder.switch_to_block(fail_block);
    cx.builder.seal_block(fail_block);
    let false_val = cx.builder.ins().iconst(types::I8, 0);
    cx.builder.def_var(result_var, false_val);
    cx.builder.ins().jump(exit_block, &[]);

    // Increment and continue
    cx.builder.switch_to_block(inc_block);
    cx.builder.seal_block(inc_block);
    let current_idx = cx.builder.use_var(idx_var);
    let one = cx.builder.ins().iconst(types::I64, 1);
    let next_idx = cx.builder.ins().iadd(current_idx, one);
    cx.builder.def_var(idx_var, next_idx);
    cx.builder.ins().jump(header_block, &[]);

    cx.builder.seal_block(header_block);
    cx.builder.switch_to_block(exit_block);
    cx.builder.seal_block(exit_block);

    let result = cx.builder.use_var(result_var);
    Ok(Some((result, TurboTy::Bool)))
}

// ── Date/Time builtins ──────────────────────────────────────────────

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

/// Optional chaining: expr?.field
///
/// If expr is none, return none. If expr is some(v), unwrap v, access .field,
/// and wrap the result back in some(field_value). Result type is Optional<field_type>.
pub(crate) fn compile_optional_chain<M: Module>(
    cx: &mut Ctx<'_, M>,
    object: &Spanned<Expr>,
    field: &str,
) -> Result<MaybeTyped, CodegenError> {
    // Compile the object expression (should produce an Optional value)
    let (opt_val, val_tty) = compile_expr(cx, object)?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_optional_chain: `object` produced no value during code generation"
            .to_string(),
    })?;

    // Get the inner TurboTy from Optional
    let inner_tty = match &val_tty {
        TurboTy::Optional(inner) => inner.as_ref().clone(),
        _ => {
            return Err(CodegenError {
                code: ErrorCode::E0400,
                message: "optional chaining `?.` requires an optional type".to_string(),
            })
        }
    };

    // Get struct name from inner type
    let struct_name = match &inner_tty {
        TurboTy::Struct(name) => name.clone(),
        _ => {
            return Err(CodegenError {
                code: ErrorCode::E0400,
                message: "optional chaining `?.` requires an optional struct type".to_string(),
            })
        }
    };

    // Find field index and type
    let fields = cx
        .struct_fields
        .get(&struct_name)
        .ok_or_else(|| CodegenError {
            code: ErrorCode::E0400,
            message: format!("undefined struct: {struct_name}"),
        })?
        .clone();

    let (field_idx, field_tty) = fields
        .iter()
        .enumerate()
        .find(|(_, (name, _))| name == field)
        .map(|(idx, (_, tty))| (idx, tty.clone()))
        .ok_or_else(|| CodegenError {
            code: ErrorCode::E0400,
            message: format!("struct `{struct_name}` has no field `{field}`"),
        })?;

    let offset = (field_idx as i32) * 8;

    // Extract tag: 0 = none, 1 = some
    let tag_fid = cx.rt_fns["rt_option_tag"];
    let tag_fref = cx.module.declare_func_in_func(tag_fid, cx.builder.func);
    let tag_call = cx.builder.ins().call(tag_fref, &[opt_val]);
    let tag = cx.builder.inst_results(tag_call)[0];

    // Check if tag == 1 (some)
    let one = cx.builder.ins().iconst(types::I64, 1);
    let is_some = cx.builder.ins().icmp(IntCC::Equal, tag, one);

    let some_block = cx.builder.create_block();
    let none_block = cx.builder.create_block();
    let merge_block = cx.builder.create_block();

    cx.builder
        .ins()
        .brif(is_some, some_block, &[], none_block, &[]);

    // Some path: unwrap, access field, wrap back in some
    cx.builder.switch_to_block(some_block);
    cx.builder.seal_block(some_block);

    let val_fid = cx.rt_fns["rt_option_value"];
    let val_fref = cx.module.declare_func_in_func(val_fid, cx.builder.func);
    let val_call = cx.builder.ins().call(val_fref, &[opt_val]);
    let inner_val = cx.builder.inst_results(val_call)[0];

    // Load field from struct pointer at offset
    let field_val = cx
        .builder
        .ins()
        .load(types::I64, MemFlags::new(), inner_val, offset);

    // For float fields, bitcast to i64 before wrapping in optional
    // (rt_option_some takes i64)
    let field_val_i64 = match &field_tty {
        TurboTy::Bool => cx.builder.ins().sextend(types::I64, field_val),
        _ => field_val,
    };

    retain_if_needed(cx, field_val, &field_tty);

    // Wrap field value in some()
    let some_fid = cx.rt_fns["rt_option_some"];
    let some_fref = cx.module.declare_func_in_func(some_fid, cx.builder.func);
    let some_call = cx.builder.ins().call(some_fref, &[field_val_i64]);
    let some_result = cx.builder.inst_results(some_call)[0];
    cx.builder.ins().jump(merge_block, &[some_result]);

    // None path: return none
    cx.builder.switch_to_block(none_block);
    cx.builder.seal_block(none_block);

    let none_fid = cx.rt_fns["rt_option_none"];
    let none_fref = cx.module.declare_func_in_func(none_fid, cx.builder.func);
    let none_call = cx.builder.ins().call(none_fref, &[]);
    let none_result = cx.builder.inst_results(none_call)[0];
    cx.builder.ins().jump(merge_block, &[none_result]);

    // Merge block
    cx.builder.append_block_param(merge_block, cx.ptr_type);
    cx.builder.switch_to_block(merge_block);
    cx.builder.seal_block(merge_block);
    let result = cx.builder.block_params(merge_block)[0];

    Ok(Some((result, TurboTy::Optional(Box::new(field_tty)))))
}

// ── BL-1 regression: the codegen unwrap-on-compiled-subexpression class ─
//
// Every site that used to unwrap the `Option` returned by compiling a
// subexpression now returns a graceful `CodegenError` (E0400) instead. These
// sites are sema-guarded in the real pipeline, so to prove the panic class is
// retired we drive the backend *directly, bypassing sema*, with parseable
// programs that place a unit-typed (`print(...)` → void) value where codegen
// must read a real value. Without the fix each of these panics the process
// (exit 101); with it they diagnose cleanly.
#[cfg(test)]
mod panic_class_regression {
    use super::*;

    /// Lex → parse → JIT-compile, **skipping** sema. Returns the codegen
    /// result. A panic here (the pre-BL-1 behavior) fails the test; a clean
    /// `Err` passes. Compilation fails inside `main`'s body before the JIT is
    /// finalized, so nothing is ever executed.
    fn codegen_without_sema(source: &str) -> Result<(), CodegenError> {
        let (tokens, lex_errors) = turbo_lexer::tokenize(source);
        assert!(
            lex_errors.is_empty(),
            "unexpected lex errors: {lex_errors:?}"
        );
        let (module, parse_errors) = turbo_parser::parse(tokens);
        assert!(
            parse_errors.is_empty(),
            "program must parse so it reaches codegen: {parse_errors:?}"
        );
        crate::jit_run(&module)
    }

    /// Assert codegen returns a graceful `E0400` rather than panicking.
    fn assert_graceful(source: &str) {
        match codegen_without_sema(source) {
            Ok(()) => panic!(
                "expected a graceful codegen error for unit-in-arg program, \
                 got Ok (value path changed?): {source:?}"
            ),
            Err(e) => assert_eq!(
                e.code,
                ErrorCode::E0400,
                "expected E0400 for {source:?}, got {:?}: {}",
                e.code,
                e.message
            ),
        }
    }

    // builtins.rs sites — first-argument position.
    #[test]
    fn len_of_unit() {
        assert_graceful("fn main() { let n = len(print(0)) }");
    }

    #[test]
    fn upper_of_unit() {
        assert_graceful("fn main() { let s = upper(print(0)) }");
    }

    #[test]
    fn hashmap_get_on_unit() {
        assert_graceful("fn main() { let v = hashmap_get(print(0), \"k\") }");
    }

    // builtins.rs sites — non-first argument position.
    #[test]
    fn min_with_unit_second_arg() {
        assert_graceful("fn main() { let m = min(1, print(0)) }");
    }

    #[test]
    fn push_with_unit_element() {
        assert_graceful("fn main() { let a = push([1, 2], print(0)) }");
    }

    #[test]
    fn pow_with_unit_exponent() {
        assert_graceful("fn main() { let p = pow(2, print(0)) }");
    }

    // builtins.rs sites — control-flow heads.
    #[test]
    fn if_condition_is_unit() {
        assert_graceful("fn main() { if print(0) { print(1) } }");
    }

    #[test]
    fn while_condition_is_unit() {
        assert_graceful("fn main() { while print(0) { print(1) } }");
    }

    #[test]
    fn for_iterable_is_unit() {
        assert_graceful("fn main() { for x in print(0) { print(x) } }");
    }

    #[test]
    fn match_subject_is_unit() {
        assert_graceful("fn main() { match print(0) { _ => print(1) } }");
    }

    // expr.rs sites.
    #[test]
    fn short_circuit_and_lhs_is_unit() {
        assert_graceful("fn main() { let b = print(0) && true }");
    }

    #[test]
    fn short_circuit_or_rhs_is_unit() {
        assert_graceful("fn main() { let b = true || print(0) }");
    }

    #[test]
    fn method_call_object_is_unit() {
        assert_graceful("fn main() { let x = print(0).foo() }");
    }

    #[test]
    fn enum_variant_data_arg_is_unit() {
        assert_graceful("type Shape { Circle(i64) }\nfn main() { let s = Shape.Circle(print(0)) }");
    }

    #[test]
    fn ufcs_first_arg_is_unit() {
        assert_graceful("fn main() { let x = notafn(print(0)) }");
    }
}
