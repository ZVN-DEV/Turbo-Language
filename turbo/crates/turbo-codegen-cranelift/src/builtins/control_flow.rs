//! Control-flow lowering: if, while, for, match, and optional chaining.

use cranelift::prelude::*;
use cranelift_module::Module;
use std::collections::HashMap;
use turbo_ast::*;

use crate::turbo_types::{CodegenError, MaybeTyped, TurboTy};
use crate::{compile_expr, retain_if_needed, Ctx};

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
