//! Core built-ins: print, assert, len, numeric, conversion, and process helpers.

use cranelift::prelude::*;
use cranelift_module::Module;
use turbo_ast::*;

use crate::turbo_types::{CodegenError, MaybeTyped, TurboTy};
use crate::{
    compile_expr, expr_produces_owned_rc_temp, release_expr_temp_if_needed, release_if_needed,
    retain_if_needed, Ctx,
};

use super::*;

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
        match &tty {
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
            TurboTy::Enum(enum_name) => {
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
                release_if_needed(cx, s, &TurboTy::Str);
            }
            TurboTy::Fn(_, _) => {
                let ptr = cx.create_string("[function]")?;
                cx.rt_call("rt_print_str", &[ptr]);
            }
            TurboTy::Future(_) => {
                let ptr = cx.create_string("[future]")?;
                cx.rt_call("rt_print_str", &[ptr]);
            }
            TurboTy::HashMap(_, _) => {
                let ptr = cx.create_string("[hashmap]")?;
                cx.rt_call("rt_print_str", &[ptr]);
            }
        }
        release_expr_temp_if_needed(cx, v, &tty, &args[0]);
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
    release_expr_temp_if_needed(cx, left_val, &left_tty, &args[0]);
    release_expr_temp_if_needed(cx, right_val, &right_tty, &args[1]);

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
        release_expr_temp_if_needed(cx, val, &tty, &args[0]);
        Ok(Some((result, TurboTy::Int)))
    } else {
        let len_fid = cx.rt_fns["rt_array_len"];
        let len_ref = cx.module.declare_func_in_func(len_fid, cx.builder.func);
        let call = cx.builder.ins().call(len_ref, &[val]);
        let result = cx.builder.inst_results(call)[0];
        release_expr_temp_if_needed(cx, val, &tty, &args[0]);
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
    if tty == TurboTy::Str {
        if !expr_produces_owned_rc_temp(&args[0]) {
            retain_if_needed(cx, str_val, &TurboTy::Str);
        }
    } else {
        release_expr_temp_if_needed(cx, val, &tty, &args[0]);
    }
    Ok(Some((str_val, TurboTy::Str)))
}

// ── Stdlib builtins ─────────────────────────────────────────────────

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
    let type_name = if let Some((value, ref tty)) = result {
        let name = match tty {
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
            TurboTy::HashMap(_, _) => "hashmap",
        };
        release_expr_temp_if_needed(cx, value, tty, &args[0]);
        name.to_string()
    } else {
        "unit".to_string()
    };
    let ptr = cx.create_string(&type_name)?;
    Ok(Some((ptr, TurboTy::Str)))
}

// ── String parsing builtins ────────────────────────────────────────

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
