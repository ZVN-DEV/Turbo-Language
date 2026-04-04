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
use crate::{compile_expr, turbo_ty_to_cl_type, Ctx};

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
            TurboTy::Array(_) => {
                let ptr = cx.create_string("[array]")?;
                cx.rt_call("rt_print_str", &[ptr]);
            }
            TurboTy::Struct(ref name) => {
                // Check if struct implements Display trait
                let has_display = cx
                    .trait_impls
                    .get(name)
                    .is_some_and(|traits| traits.contains(&"Display".to_string()));
                if has_display {
                    let mangled = format!("{name}__to_string");
                    if let Some(&fid) = cx.user_fns.get(&mangled) {
                        let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
                        let call = cx.builder.ins().call(fref, &[v]);
                        let str_val = cx.builder.inst_results(call)[0];
                        cx.rt_call("rt_print_str", &[str_val]);
                    } else {
                        let ptr = cx.create_string(&format!("[struct {}]", name))?;
                        cx.rt_call("rt_print_str", &[ptr]);
                    }
                } else {
                    let ptr = cx.create_string(&format!("[struct {}]", name))?;
                    cx.rt_call("rt_print_str", &[ptr]);
                }
            }
            TurboTy::Fn(_, _) => {
                let ptr = cx.create_string("[function]")?;
                cx.rt_call("rt_print_str", &[ptr]);
            }
            TurboTy::Result(_, _) => {
                let ptr = cx.create_string("[result]")?;
                cx.rt_call("rt_print_str", &[ptr]);
            }
            TurboTy::Optional(_) => {
                let ptr = cx.create_string("[optional]")?;
                cx.rt_call("rt_print_str", &[ptr]);
            }
            TurboTy::Agent(ref name) => {
                let ptr = cx.create_string(&format!("[agent {}]", name))?;
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
        compile_expr(cx, &args[0])?.unwrap().0
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

    let (cond, _) = compile_expr(cx, &args[0])?.unwrap();
    let cond_bool = cx.to_bool(cond);

    let fail_block = cx.builder.create_block();
    let ok_block = cx.builder.create_block();

    cx.builder
        .ins()
        .brif(cond_bool, ok_block, &[], fail_block, &[]);

    cx.builder.switch_to_block(fail_block);
    cx.builder.seal_block(fail_block);

    let msg = if args.len() > 1 {
        compile_expr(cx, &args[1])?.unwrap().0
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

    let (left_val, left_tty) = compile_expr(cx, &args[0])?.unwrap();
    let (right_val, right_tty) = compile_expr(cx, &args[1])?.unwrap();

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
    let (val, tty) = compile_expr(cx, &args[0])?.unwrap();
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
    let (val, _) = compile_expr(cx, &args[0])?.unwrap();
    let zero = cx.builder.ins().iconst(types::I64, 0);
    let is_neg = cx.builder.ins().icmp(IntCC::SignedLessThan, val, zero);
    let neg_val = cx.builder.ins().ineg(val);
    let result = cx.builder.ins().select(is_neg, neg_val, val);
    Ok(Some((result, TurboTy::Int)))
}

pub(crate) fn compile_min<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (a, _) = compile_expr(cx, &args[0])?.unwrap();
    let (b, _) = compile_expr(cx, &args[1])?.unwrap();
    let cmp = cx.builder.ins().icmp(IntCC::SignedLessThan, a, b);
    let result = cx.builder.ins().select(cmp, a, b);
    Ok(Some((result, TurboTy::Int)))
}

pub(crate) fn compile_max<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (a, _) = compile_expr(cx, &args[0])?.unwrap();
    let (b, _) = compile_expr(cx, &args[1])?.unwrap();
    let cmp = cx.builder.ins().icmp(IntCC::SignedGreaterThan, a, b);
    let result = cx.builder.ins().select(cmp, a, b);
    Ok(Some((result, TurboTy::Int)))
}

pub(crate) fn compile_to_str_builtin<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (val, tty) = compile_expr(cx, &args[0])?.unwrap();
    let str_val = convert_to_str(cx, val, &tty)?;
    Ok(Some((str_val, TurboTy::Str)))
}

// ── Stdlib builtins ─────────────────────────────────────────────────

/// split(s, sep) -> [str] — calls rt_str_split, returns Array(Str)
pub(crate) fn compile_stdlib_split<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (s_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let (sep_val, _) = compile_expr(cx, &args[1])?.unwrap();
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
    let (s_val, _) = compile_expr(cx, &args[0])?.unwrap();
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
    let (s_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let (other_val, _) = compile_expr(cx, &args[1])?.unwrap();
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
    let (s_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let (from_val, _) = compile_expr(cx, &args[1])?.unwrap();
    let (to_val, _) = compile_expr(cx, &args[2])?.unwrap();
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
    let (s_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let (idx_val, _) = compile_expr(cx, &args[1])?.unwrap();
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
    let (s_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let (sub_val, _) = compile_expr(cx, &args[1])?.unwrap();
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
    let (arr_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let (sep_val, _) = compile_expr(cx, &args[1])?.unwrap();
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
    let (s_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let (n_val, _) = compile_expr(cx, &args[1])?.unwrap();
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
    let (path_val, _) = compile_expr(cx, &args[0])?.unwrap();
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
    let (path_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let (content_val, _) = compile_expr(cx, &args[1])?.unwrap();
    let fid = cx.rt_fns["rt_write_file"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    cx.builder.ins().call(fref, &[path_val, content_val]);
    Ok(None)
}

/// pow(base, exp) -> i64
pub(crate) fn compile_stdlib_pow<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (base_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let (exp_val, _) = compile_expr(cx, &args[1])?.unwrap();
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
    let (x_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let fid = cx.rt_fns["rt_sqrt"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[x_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Float)))
}

/// sleep(ms) -> () — sleep the current thread for ms milliseconds
pub(crate) fn compile_builtin_sleep<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (ms_val, _) = compile_expr(cx, &args[0])?.unwrap();
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
    let (url_val, _) = compile_expr(cx, &args[0])?.unwrap();
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
    let (url_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let (body_val, _) = compile_expr(cx, &args[1])?.unwrap();
    let fid = cx.rt_fns["rt_http_post"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[url_val, body_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Str)))
}

/// json_get(json_str, key) -> str
pub(crate) fn compile_builtin_json_get<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (json_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let (key_val, _) = compile_expr(cx, &args[1])?.unwrap();
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
    let (key_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let (value_val, _) = compile_expr(cx, &args[1])?.unwrap();
    let fid = cx.rt_fns["rt_json_stringify"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[key_val, value_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Str)))
}

// ── HTTP server builtins ────────────────────────────────────────────

/// http_server(port) -> i64 (server id)
pub(crate) fn compile_builtin_http_server<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (port_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let fid = cx.rt_fns["rt_http_server"];
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
    let (server_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let (method_val, _) = compile_expr(cx, &args[1])?.unwrap();
    let (path_val, _) = compile_expr(cx, &args[2])?.unwrap();
    let (closure_ptr, _) = compile_expr(cx, &args[3])?.unwrap();

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
    let (server_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let fid = cx.rt_fns["rt_http_listen"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    cx.builder.ins().call(fref, &[server_val]);
    Ok(None)
}

/// respond(status, body) -> str — builds "STATUS:BODY" format response
pub(crate) fn compile_builtin_respond<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (status_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let (body_val, _) = compile_expr(cx, &args[1])?.unwrap();
    let fid = cx.rt_fns["rt_respond"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[status_val, body_val]);
    let result = cx.builder.inst_results(call)[0];
    Ok(Some((result, TurboTy::Str)))
}

/// request_body(req) -> str — extracts body from request (identity for now)
pub(crate) fn compile_builtin_request_body<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (req_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let fid = cx.rt_fns["rt_request_body"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    let call = cx.builder.ins().call(fref, &[req_val]);
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
    let (val, tty) = compile_expr(cx, &args[0])?.unwrap();

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
    let (arr_ptr, arr_tty) = compile_expr(cx, &args[0])?.unwrap();

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
    let (arr_ptr, _arr_tty) = compile_expr(cx, &args[0])?.unwrap();
    let (closure_ptr, fn_tty) = compile_expr(cx, &args[1])?.unwrap();

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
    let (arr_ptr, arr_tty) = compile_expr(cx, &args[0])?.unwrap();
    let (closure_ptr, fn_tty) = compile_expr(cx, &args[1])?.unwrap();

    let elem_tty = match &arr_tty {
        TurboTy::Array(inner) => *inner.clone(),
        _ => TurboTy::Int,
    };

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

    let alloc_fid = cx.rt_fns["rt_array_alloc"];
    let alloc_ref = cx.module.declare_func_in_func(alloc_fid, cx.builder.func);
    let alloc_call = cx.builder.ins().call(alloc_ref, &[arr_len]);
    let result_ptr = cx.builder.inst_results(alloc_call)[0];

    let mut sig = cx.module.make_signature();
    sig.call_conv = CallConv::Fast;
    sig.params.push(AbiParam::new(cx.ptr_type)); // env_ptr
    let param_cl_ty = turbo_ty_to_cl_type(&param_tty, cx.ptr_type);
    sig.params.push(AbiParam::new(param_cl_ty));
    sig.returns.push(AbiParam::new(types::I8));
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
    let (arr_ptr, _arr_tty) = compile_expr(cx, &args[0])?.unwrap();
    let (init_val, init_tty) = compile_expr(cx, &args[1])?.unwrap();
    let (closure_ptr, fn_tty) = compile_expr(cx, &args[2])?.unwrap();

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

pub(crate) fn compile_if<M: Module>(
    cx: &mut Ctx<'_, M>,
    condition: &Spanned<Expr>,
    then_branch: &Spanned<Expr>,
    else_branch: Option<&Spanned<Expr>>,
) -> Result<MaybeTyped, CodegenError> {
    let (cond, _) = compile_expr(cx, condition)?.unwrap();
    let cond_bool = cx.to_bool(cond);

    let then_block = cx.builder.create_block();
    let else_block = cx.builder.create_block();
    let merge_block = cx.builder.create_block();

    cx.builder
        .ins()
        .brif(cond_bool, then_block, &[], else_block, &[]);

    // Then
    cx.builder.switch_to_block(then_block);
    cx.builder.seal_block(then_block);
    let then_result = compile_expr(cx, then_branch)?;
    let then_needs_jump = !cx.builder.is_unreachable();
    if then_needs_jump {
        if let Some((v, _)) = then_result {
            cx.builder.ins().jump(merge_block, &[v]);
        } else {
            cx.builder.ins().jump(merge_block, &[]);
        }
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

    if let (Some((then_val, then_tty)), Some(_)) = (then_result, else_result) {
        let ty = cx.builder.func.dfg.value_type(then_val);
        cx.builder.append_block_param(merge_block, ty);
        let param = cx.builder.block_params(merge_block)[0];
        Ok(Some((param, then_tty)))
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
    let (val, val_tty) = compile_expr(cx, value)?.unwrap();

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

            let var = Variable::new(cx.next_var);
            cx.next_var += 1;
            cx.builder.declare_var(var, types::I64);
            cx.builder.def_var(var, raw_val);

            let turbo_ty = match &val_tty {
                TurboTy::Optional(inner_tty) => *inner_tty.clone(),
                _ => TurboTy::Int,
            };
            cx.vars.insert(binding.clone(), (var, types::I64, turbo_ty));
        }
        Pattern::Ok(binding) | Pattern::Err(binding) => {
            let val_fid = cx.rt_fns["rt_result_value"];
            let val_fref = cx.module.declare_func_in_func(val_fid, cx.builder.func);
            let val_call = cx.builder.ins().call(val_fref, &[val]);
            let raw_val = cx.builder.inst_results(val_call)[0];

            let var = Variable::new(cx.next_var);
            cx.next_var += 1;
            cx.builder.declare_var(var, types::I64);
            cx.builder.def_var(var, raw_val);

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
            cx.vars.insert(binding.clone(), (var, types::I64, turbo_ty));
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

    // Merge block
    cx.builder.switch_to_block(merge_block);
    cx.builder.seal_block(merge_block);

    if let (Some((then_val, then_tty)), Some(_)) = (then_result, else_result) {
        let ty = cx.builder.func.dfg.value_type(then_val);
        cx.builder.append_block_param(merge_block, ty);
        let param = cx.builder.block_params(merge_block)[0];
        Ok(Some((param, then_tty)))
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
    let (src_ptr, src_tty) = compile_expr(cx, &args[0])?.unwrap();

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
                let (val, tty) = compile_expr(cx, expr)?.unwrap();
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
        TurboTy::Array(_) => cx.create_string("[array]"),
        TurboTy::Struct(ref name) => {
            // Check if struct implements Display trait
            let has_display = cx
                .trait_impls
                .get(name)
                .is_some_and(|traits| traits.contains(&"Display".to_string()));
            if has_display {
                let mangled = format!("{name}__to_string");
                if let Some(&fid) = cx.user_fns.get(&mangled) {
                    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
                    let call = cx.builder.ins().call(fref, &[val]);
                    Ok(cx.builder.inst_results(call)[0])
                } else {
                    cx.create_string(&format!("[struct {}]", name))
                }
            } else {
                cx.create_string(&format!("[struct {}]", name))
            }
        }
        TurboTy::Fn(_, _) => cx.create_string("[function]"),
        TurboTy::Result(_, _) => cx.create_string("[result]"),
        TurboTy::Optional(_) => cx.create_string("[optional]"),
        TurboTy::Agent(ref name) => cx.create_string(&format!("[agent {}]", name)),
        TurboTy::Future(_) => cx.create_string("[future]"),
    }
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
    let (cond, _) = compile_expr(cx, condition)?.unwrap();
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
    let (range_start, _) = compile_expr(cx, start)?.unwrap();
    let (range_end, _) = compile_expr(cx, end)?.unwrap();

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
    let (arr_ptr, arr_tty) = compile_expr(cx, iterable)?.unwrap();

    // Get array length via rt_array_len
    let len_fid = cx.rt_fns["rt_array_len"];
    let len_ref = cx.module.declare_func_in_func(len_fid, cx.builder.func);
    let call = cx.builder.ins().call(len_ref, &[arr_ptr]);
    let arr_len = cx.builder.inst_results(call)[0];

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

    // Load element: arr[idx] via rt_array_get
    let get_fid = cx.rt_fns["rt_array_get"];
    let get_ref = cx.module.declare_func_in_func(get_fid, cx.builder.func);
    let idx_val = cx.builder.use_var(idx_var);
    let get_call = cx.builder.ins().call(get_ref, &[arr_ptr, idx_val]);
    let raw_elem = cx.builder.inst_results(get_call)[0];

    // rt_array_get returns raw i64 bits; convert to the correct type
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

    Ok(None)
}

// ── Match expression ────────────────────────────────────────────────

pub(crate) fn compile_match<M: Module>(
    cx: &mut Ctx<'_, M>,
    subject: &Spanned<Expr>,
    arms: &[MatchArm],
) -> Result<MaybeTyped, CodegenError> {
    let (subj_val, subj_tty) = compile_expr(cx, subject)?.unwrap();

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

                let var = Variable::new(cx.next_var);
                cx.next_var += 1;
                cx.builder.declare_var(var, types::I64);
                cx.builder.def_var(var, raw_val);

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
                cx.vars.insert(binding.clone(), (var, types::I64, turbo_ty));
            }
            Pattern::Some(binding) => {
                let val_fid = cx.rt_fns["rt_option_value"];
                let val_fref = cx.module.declare_func_in_func(val_fid, cx.builder.func);
                let val_call = cx.builder.ins().call(val_fref, &[subj_val]);
                let raw_val = cx.builder.inst_results(val_call)[0];

                let var = Variable::new(cx.next_var);
                cx.next_var += 1;
                cx.builder.declare_var(var, types::I64);
                cx.builder.def_var(var, raw_val);

                let turbo_ty = match &subj_tty {
                    TurboTy::Optional(inner_tty) => *inner_tty.clone(),
                    _ => TurboTy::Int,
                };
                cx.vars.insert(binding.clone(), (var, types::I64, turbo_ty));
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
    let (ch_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let (value_val, _) = compile_expr(cx, &args[1])?.unwrap();
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
    let (ch_val, _) = compile_expr(cx, &args[0])?.unwrap();
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
    let (value_val, _) = compile_expr(cx, &args[0])?.unwrap();
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
    let (m_val, _) = compile_expr(cx, &args[0])?.unwrap();
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
    let (m_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let (value_val, _) = compile_expr(cx, &args[1])?.unwrap();
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
    let (map_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let (key_val, _) = compile_expr(cx, &args[1])?.unwrap();
    let (value_val, _) = compile_expr(cx, &args[2])?.unwrap();
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
    let (map_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let (key_val, _) = compile_expr(cx, &args[1])?.unwrap();
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
    let (map_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let (key_val, _) = compile_expr(cx, &args[1])?.unwrap();
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
    let (map_val, _) = compile_expr(cx, &args[0])?.unwrap();
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
    let (map_val, _) = compile_expr(cx, &args[0])?.unwrap();
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
    let (map_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let (key_val, _) = compile_expr(cx, &args[1])?.unwrap();
    let fid = cx.rt_fns["rt_hashmap_remove"];
    let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
    cx.builder.ins().call(fref, &[map_val, key_val]);
    Ok(None)
}

// ── Unsafe builtins — raw pointer operations ────────────────────────

/// deref(addr: i64) -> i64 — load an i64 from the given memory address
pub(crate) fn compile_builtin_deref<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let (addr_val, _) = compile_expr(cx, &args[0])?.unwrap();
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
    let (arr_val, arr_tty) = compile_expr(cx, &args[0])?.unwrap();
    let (elem_val, _) = compile_expr(cx, &args[1])?.unwrap();
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
    let (addr_val, _) = compile_expr(cx, &args[0])?.unwrap();
    let (val, _) = compile_expr(cx, &args[1])?.unwrap();
    cx.builder.ins().store(MemFlags::new(), val, addr_val, 0);
    Ok(None)
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
    let (opt_val, val_tty) = compile_expr(cx, object)?.unwrap();

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
    let fields = cx.struct_fields.get(&struct_name).ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: format!("undefined struct: {struct_name}"),
    })?.clone();

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
    cx.builder
        .append_block_param(merge_block, cx.ptr_type);
    cx.builder.switch_to_block(merge_block);
    cx.builder.seal_block(merge_block);
    let result = cx.builder.block_params(merge_block)[0];

    Ok(Some((result, TurboTy::Optional(Box::new(field_tty)))))
}
