//! Array built-ins: map/filter/reduce, push, sort, slice, and predicates.

use cranelift::prelude::isa::CallConv;
use cranelift::prelude::*;
use cranelift_module::Module;
use turbo_ast::*;

use crate::turbo_types::{CodegenError, MaybeTyped, TurboTy};
use crate::{
    compile_expr, retain_array_elements_if_needed, retain_array_prefix_if_needed, retain_if_needed,
    turbo_ty_to_cl_type, Ctx,
};

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
    retain_if_needed(cx, raw_elem, &elem_tty);

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
    let elem_tty = match &arr_tty {
        TurboTy::Array(inner) => *inner.clone(),
        _ => TurboTy::Int,
    };
    let old_len = cx
        .builder
        .ins()
        .load(types::I64, MemFlags::new(), arr_val, 0);
    let (elem_val, elem_val_tty) = compile_expr(cx, &args[1])?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_builtin_push: `&args[1]` produced no value during code generation"
            .to_string(),
    })?;
    if matches!(
        &args[1].node,
        Expr::Ident(_) | Expr::Index { .. } | Expr::FieldAccess { .. }
    ) {
        retain_if_needed(cx, elem_val, &elem_val_tty);
    }
    let push_fid = cx.rt_fns["rt_array_push"];
    let push_ref = cx.module.declare_func_in_func(push_fid, cx.builder.func);
    let call = cx.builder.ins().call(push_ref, &[arr_val, elem_val]);
    let result = cx.builder.inst_results(call)[0];
    let same_ptr = cx.builder.ins().icmp(IntCC::Equal, arr_val, result);
    let copied_block = cx.builder.create_block();
    let done_block = cx.builder.create_block();
    cx.builder
        .ins()
        .brif(same_ptr, done_block, &[], copied_block, &[]);

    cx.builder.switch_to_block(copied_block);
    cx.builder.seal_block(copied_block);
    retain_array_prefix_if_needed(cx, result, &elem_tty, old_len);
    cx.builder.ins().jump(done_block, &[]);

    cx.builder.switch_to_block(done_block);
    cx.builder.seal_block(done_block);
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
    retain_array_elements_if_needed(cx, result, &elem_tty);
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
    retain_array_elements_if_needed(cx, result, &elem_tty);
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
    retain_array_elements_if_needed(cx, result, &elem_tty);
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
