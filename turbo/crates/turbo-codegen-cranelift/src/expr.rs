//! Expression compilation.
//!
//! Contains `compile_expr()` — the main expression compiler that handles all
//! `Expr` variants — along with binary operations, short-circuit evaluation,
//! function calls, RC heap helpers, and JSON decode support.

use super::*;
use std::collections::HashMap;

// ── Expression compilation ──────────────────────────────────────────

pub(crate) fn compile_expr<M: Module>(
    cx: &mut Ctx<'_, M>,
    expr: &Spanned<Expr>,
) -> Result<MaybeTyped, CodegenError> {
    // Reject pathologically deep ASTs before they overflow the native stack.
    // Matches the parser's `MAX_PARSER_DEPTH` so anything the parser accepts
    // is either lowered successfully or rejected here with a diagnostic.
    cx.expr_depth += 1;
    if cx.expr_depth > crate::MAX_CODEGEN_DEPTH {
        cx.expr_depth -= 1;
        return Err(CodegenError {
            code: ErrorCode::E0516,
            message: format!(
                "expression nesting exceeds {} levels (compiler recursion limit)",
                crate::MAX_CODEGEN_DEPTH
            ),
        });
    }
    let result = compile_expr_inner(cx, expr);
    cx.expr_depth -= 1;
    result
}

fn compile_expr_inner<M: Module>(
    cx: &mut Ctx<'_, M>,
    expr: &Spanned<Expr>,
) -> Result<MaybeTyped, CodegenError> {
    match &expr.node {
        Expr::IntLit(n) => {
            let val = cx.builder.ins().iconst(types::I64, *n);
            Ok(Some((val, TurboTy::Int)))
        }
        Expr::FloatLit(f) => {
            let val = cx.builder.ins().f64const(*f);
            Ok(Some((val, TurboTy::Float)))
        }
        Expr::BoolLit(b) => {
            let val = cx.builder.ins().iconst(types::I8, *b as i64);
            Ok(Some((val, TurboTy::Bool)))
        }
        Expr::StringLit(s) => {
            let ptr = cx.create_string(s)?;
            Ok(Some((ptr, TurboTy::Str)))
        }
        Expr::Unit => Ok(None),

        Expr::Ident(name) => {
            // Check if this is a module-level constant — inline the value
            if let Some(const_expr) = cx.constants.get(name.as_str()) {
                let const_expr = const_expr.clone();
                return compile_expr(cx, &const_expr);
            }
            if let Some((var, _cl_ty, turbo_ty)) = cx.vars.get(name) {
                let turbo_ty = turbo_ty.clone();
                let val = cx.builder.use_var(*var);
                if let Some(origin) = cx.generic_var_origins.get(name).cloned() {
                    mark_generic_value_origin(cx, val, origin);
                }
                return Ok(Some((val, turbo_ty)));
            }
            // A bare function name used as a value becomes a first-class
            // function value: a `[adapter_ptr, null_env]` pair (see
            // `compile_named_fn_value`).
            if let Some(result) = compile_named_fn_value(cx, name)? {
                return Ok(result);
            }
            Err(CodegenError {
                code: ErrorCode::E0401,
                message: format!("undefined variable: {name}"),
            })
        }

        Expr::BinaryOp { left, op, right } => {
            // Short-circuit for && and ||
            if *op == BinOp::And || *op == BinOp::Or {
                return compile_short_circuit(cx, left, *op, right);
            }

            let (lhs, lhs_tty) = compile_expr(cx, left)?.ok_or_else(|| CodegenError {
                code: ErrorCode::E0100,
                message: "binary operation on unit type".to_string(),
            })?;
            let (rhs, rhs_tty) = compile_expr(cx, right)?.ok_or_else(|| CodegenError {
                code: ErrorCode::E0100,
                message: "binary operation on unit type".to_string(),
            })?;

            // String operations
            if lhs_tty == TurboTy::Str && rhs_tty == TurboTy::Str {
                match op {
                    BinOp::Add => {
                        let result = compile_str_concat(cx, lhs, rhs)?;
                        release_expr_temp_if_needed(cx, lhs, &lhs_tty, left);
                        release_expr_temp_if_needed(cx, rhs, &rhs_tty, right);
                        return Ok(result);
                    }
                    BinOp::Eq | BinOp::NotEq => {
                        let result = compile_str_compare(cx, lhs, rhs, *op)?;
                        release_expr_temp_if_needed(cx, lhs, &lhs_tty, left);
                        release_expr_temp_if_needed(cx, rhs, &rhs_tty, right);
                        return Ok(result);
                    }
                    _ => {}
                }
            }

            // Mixed str + non-str is rejected by sema — no implicit coercion

            // Struct field-by-field equality comparison (@derive(Eq))
            if let TurboTy::Struct(ref struct_name) = lhs_tty {
                if matches!(op, BinOp::Eq | BinOp::NotEq) {
                    return compile_struct_eq(cx, lhs, rhs, struct_name, *op);
                }
            }

            // Unify mismatched integer operand tags for arithmetic. Sema only
            // permits a mix when an untyped int literal coerces into a sized
            // operand (`n + 1` where `n: i8`), and the result is that sized
            // type. Coerce both operands to the narrower tag so the value's IR
            // width matches its TurboTy — otherwise a narrow-tagged i64 result
            // would later hit `sextend(i64, i64)` in the print/convert path and
            // panic Cranelift.
            let (lhs, rhs, arith_tty) = if matches!(
                op,
                BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod
            ) && lhs_tty != rhs_tty
            {
                match unify_int_tty(&lhs_tty, &rhs_tty) {
                    Some(common) => {
                        let (l, _) = coerce_value(cx, lhs, &lhs_tty, &common);
                        let (r, _) = coerce_value(cx, rhs, &rhs_tty, &common);
                        (l, r, common)
                    }
                    None => (lhs, rhs, lhs_tty.clone()),
                }
            } else {
                (lhs, rhs, lhs_tty.clone())
            };

            let result = compile_binop(cx, lhs, *op, rhs)?;

            // Comparison/logical ops produce Bool, arithmetic preserves input type
            let result_tty = match op {
                BinOp::Eq
                | BinOp::NotEq
                | BinOp::Less
                | BinOp::LessEq
                | BinOp::Greater
                | BinOp::GreaterEq
                | BinOp::And
                | BinOp::Or => TurboTy::Bool,
                _ => arith_tty,
            };
            Ok(Some((result, result_tty)))
        }

        Expr::UnaryOp { op, expr: inner } => {
            let (val, tty) = compile_expr(cx, inner)?.ok_or_else(|| CodegenError {
                code: ErrorCode::E0400,
                message: "expected a value, but sub-expression has unit type".to_string(),
            })?;
            let result = match op {
                UnaryOp::Neg => {
                    let ty = cx.builder.func.dfg.value_type(val);
                    if ty.is_float() {
                        cx.builder.ins().fneg(val)
                    } else {
                        cx.builder.ins().ineg(val)
                    }
                }
                UnaryOp::Not => {
                    let one = cx.builder.ins().iconst(types::I8, 1);
                    cx.builder.ins().bxor(val, one)
                }
            };
            let result_tty = match op {
                UnaryOp::Not => TurboTy::Bool,
                UnaryOp::Neg => tty,
            };
            Ok(Some((result, result_tty)))
        }

        Expr::Cast { expr: inner, ty } => {
            let (val, from_tty) = compile_expr(cx, inner)?.ok_or_else(|| CodegenError {
                code: ErrorCode::E0400,
                message: "cannot cast a unit value".to_string(),
            })?;
            let to_tty = turbo_ty_from_type_expr(&ty.node, cx.enum_variants);
            // Sema guarantees numeric ↔ numeric (or an identity cast). For the
            // numeric case lower the real conversion; an identity / non-numeric
            // cast is a defensive no-op retag.
            if is_numeric_tty(&from_tty) && is_numeric_tty(&to_tty) {
                let to_unsigned = type_expr_is_unsigned(&ty.node);
                let cast = numeric_cast(cx, val, &from_tty, &to_tty, to_unsigned);
                Ok(Some(cast))
            } else {
                Ok(Some((val, to_tty)))
            }
        }

        Expr::Call { callee, args } => compile_call(cx, callee, args),

        Expr::If {
            condition,
            then_branch,
            else_branch,
        } => compile_if(cx, condition, then_branch, else_branch.as_deref()),

        Expr::IfLet {
            pattern,
            value,
            then_branch,
            else_branch,
        } => compile_if_let(cx, pattern, value, then_branch, else_branch.as_deref()),

        Expr::Block { stmts, tail_expr } => {
            let saved_vars = cx.vars.clone();
            let saved_generic_var_origins = cx.generic_var_origins.clone();

            // Collect defer expressions while compiling statements. Stop once a
            // statement diverges (`exit`/`panic`/`return`) — everything after it
            // is dead code, and emitting it into the now-unreachable block would
            // leave that block unterminated (Cranelift finalize panic).
            let mut deferred: Vec<&Spanned<Expr>> = Vec::new();
            for stmt in stmts {
                if cx.builder.is_unreachable() {
                    break;
                }
                if let Stmt::Defer(ref defer_expr) = stmt.node {
                    deferred.push(defer_expr);
                }
                compile_stmt(cx, stmt)?;
            }
            let result = if cx.builder.is_unreachable() {
                Ok(None)
            } else if let Some(tail) = tail_expr {
                let result = compile_expr(cx, tail)?;
                if let Some((value, tty)) = result.as_ref() {
                    if !expr_produces_owned_rc_temp(tail) {
                        retain_if_needed(cx, *value, tty);
                    }
                }
                Ok(result)
            } else {
                Ok(None)
            };

            // Emit deferred expressions in LIFO order (reverse)
            for defer_expr in deferred.iter().rev() {
                if !cx.builder.is_unreachable() {
                    compile_expr(cx, defer_expr)?;
                }
            }

            if !cx.builder.is_unreachable() {
                let locals_to_release: Vec<(Variable, TurboTy)> = cx
                    .vars
                    .iter()
                    .filter_map(|(name, (var, _, tty))| match saved_vars.get(name) {
                        Some((saved_var, _, _)) if saved_var == var => None,
                        _ if is_rc_managed_type(cx, tty) => Some((*var, tty.clone())),
                        _ => None,
                    })
                    .collect();
                for (var, tty) in locals_to_release {
                    let value = cx.builder.use_var(var);
                    release_if_needed(cx, value, &tty);
                }
            }

            // Restore variable scope: this ensures inner `let` bindings
            // that shadow outer names don't leak out of the block.
            // Actual SSA values in Cranelift variables are unaffected —
            // only the name-to-Variable mapping is restored.
            cx.vars = saved_vars;
            cx.generic_var_origins = saved_generic_var_origins;

            result
        }

        Expr::Assign { target, value } => {
            // Optimize s = s + expr → rt_str_concat_inplace(s, expr)
            if let Expr::BinaryOp {
                left,
                op: BinOp::Add,
                right,
            } = &value.node
            {
                if let Expr::Ident(name) = &left.node {
                    if name == target {
                        if let Some((var, _, TurboTy::Str)) = cx.vars.get(target) {
                            let var = *var;
                            let current = cx.builder.use_var(var);
                            let (rhs, rhs_tty) =
                                compile_expr(cx, right)?.ok_or_else(|| CodegenError {
                                    code: ErrorCode::E0400,
                                    message: "expected a value, but sub-expression has unit type"
                                        .to_string(),
                                })?;
                            let fid = cx.rt_fns["rt_str_concat_inplace"];
                            let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
                            let call = cx.builder.ins().call(fref, &[current, rhs]);
                            let result = cx.builder.inst_results(call)[0];
                            release_expr_temp_if_needed(cx, rhs, &rhs_tty, right);
                            let same_ptr = cx.builder.ins().icmp(IntCC::Equal, current, result);
                            let release_block = cx.builder.create_block();
                            let done_block = cx.builder.create_block();
                            cx.builder
                                .ins()
                                .brif(same_ptr, done_block, &[], release_block, &[]);

                            cx.builder.switch_to_block(release_block);
                            cx.builder.seal_block(release_block);
                            release_if_needed(cx, current, &TurboTy::Str);
                            cx.builder.ins().jump(done_block, &[]);

                            cx.builder.switch_to_block(done_block);
                            cx.builder.seal_block(done_block);
                            cx.builder.def_var(var, result);
                            return Ok(None);
                        }
                    }
                }
            }
            let rhs_borrows_existing = expr_result_borrows_existing_rc(value);
            let (val, tty) = compile_expr(cx, value)?.ok_or_else(|| CodegenError {
                code: ErrorCode::E0400,
                message: "expected a value, but sub-expression has unit type".to_string(),
            })?;
            let (var, _, prev_tty) = cx.vars.get(target).ok_or_else(|| CodegenError {
                code: ErrorCode::E0401,
                message: format!("undefined variable: {target}"),
            })?;
            let var = *var;
            let prev_tty = prev_tty.clone();

            if is_rc_managed_type(cx, &prev_tty) && is_rc_managed_type(cx, &tty) {
                // Assignment to refcounted values must handle aliasing as:
                //
                //   if old != new {
                //       retain(new); // when new is borrowed from existing storage
                //       release(old);
                //   }
                //
                // This matters for AOT arrays because the C runtime's
                // rt_array_push may grow an unshared array in place and return
                // the same pointer. Unconditionally releasing `old` after
                // `xs = push(xs, value)` would release the still-live `xs`.
                let prev_val = cx.builder.use_var(var);
                let same_ptr = cx.builder.ins().icmp(IntCC::Equal, prev_val, val);
                let changed_block = cx.builder.create_block();
                let done_block = cx.builder.create_block();
                cx.builder
                    .ins()
                    .brif(same_ptr, done_block, &[], changed_block, &[]);

                cx.builder.switch_to_block(changed_block);
                cx.builder.seal_block(changed_block);
                if rhs_borrows_existing {
                    retain_if_needed(cx, val, &tty);
                }
                release_if_needed(cx, prev_val, &prev_tty);
                cx.builder.ins().jump(done_block, &[]);

                cx.builder.switch_to_block(done_block);
                cx.builder.seal_block(done_block);
            } else {
                if rhs_borrows_existing {
                    retain_if_needed(cx, val, &tty);
                }
                if is_rc_managed_type(cx, &prev_tty) {
                    let prev_val = cx.builder.use_var(var);
                    release_if_needed(cx, prev_val, &prev_tty);
                }
            }
            cx.builder.def_var(var, val);
            if let Some(origin) = generic_origin_for_value(cx, val) {
                cx.generic_var_origins.insert(target.clone(), origin);
            } else {
                cx.generic_var_origins.remove(target);
            }
            // Update the turbo type in case it changed
            if let Some(entry) = cx.vars.get_mut(target) {
                entry.2 = tty;
            }
            Ok(None)
        }

        Expr::CompoundAssign { target, op, value } => {
            let (rhs, _) = compile_expr(cx, value)?.ok_or_else(|| CodegenError {
                code: ErrorCode::E0400,
                message: "expected a value, but sub-expression has unit type".to_string(),
            })?;
            let (var, _, _) = cx.vars.get(target).ok_or_else(|| CodegenError {
                code: ErrorCode::E0401,
                message: format!("undefined variable: {target}"),
            })?;
            let var = *var;
            let lhs = cx.builder.use_var(var);
            let result = compile_binop(cx, lhs, *op, rhs)?;
            cx.builder.def_var(var, result);
            Ok(None)
        }

        Expr::FieldAssign {
            object,
            field,
            value,
        } => {
            let (obj_ptr, obj_tty) = compile_expr(cx, object)?.ok_or_else(|| CodegenError {
                code: ErrorCode::E0400,
                message: "expected a value, but sub-expression has unit type".to_string(),
            })?;
            let (val, _) = compile_expr(cx, value)?.ok_or_else(|| CodegenError {
                code: ErrorCode::E0400,
                message: "expected a value, but sub-expression has unit type".to_string(),
            })?;

            let struct_name = match &obj_tty {
                TurboTy::Struct(name) => name.clone(),
                _ => {
                    return Err(CodegenError {
                        code: ErrorCode::E0400,
                        message: "field assignment on non-struct type".to_string(),
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

            let field_index = struct_layout
                .iter()
                .position(|(n, _)| n == field)
                .ok_or_else(|| CodegenError {
                    code: ErrorCode::E0400,
                    message: format!("struct `{struct_name}` has no field `{field}`"),
                })?;
            let field_tty = struct_layout[field_index].1.clone();

            let offset = (field_index * 8) as i32;

            // Copy-on-write: structs carry the same refcount header as arrays,
            // so a `let b = a` / `mut`-param / array-element copy can leave two
            // live bindings aliasing one allocation. Before mutating a field,
            // make a private copy if the allocation is shared (refcount > 1),
            // exactly as `IndexAssign` does for arrays via `rt_array_set`. When
            // the struct is the sole owner the original pointer is returned and
            // the store happens in place.
            let num_fields = cx
                .builder
                .ins()
                .iconst(types::I64, struct_layout.len() as i64);
            let cow_fid = cx.rt_fns["rt_struct_cow"];
            let cow_ref = cx.module.declare_func_in_func(cow_fid, cx.builder.func);
            let cow_call = cx.builder.ins().call(cow_ref, &[obj_ptr, num_fields]);
            let target = cx.builder.inst_results(cow_call)[0];
            let same_struct = cx.builder.ins().icmp(IntCC::Equal, obj_ptr, target);
            let copied_block = cx.builder.create_block();
            let cow_done_block = cx.builder.create_block();
            cx.builder
                .ins()
                .brif(same_struct, cow_done_block, &[], copied_block, &[]);

            cx.builder.switch_to_block(copied_block);
            cx.builder.seal_block(copied_block);
            for (index, (_name, copied_field_ty)) in struct_layout.iter().enumerate() {
                if is_rc_managed_type(cx, copied_field_ty) {
                    let copied_field = cx.builder.ins().load(
                        cx.ptr_type,
                        MemFlags::new(),
                        target,
                        (index * 8) as i32,
                    );
                    retain_if_needed(cx, copied_field, copied_field_ty);
                }
            }
            cx.builder.ins().jump(cow_done_block, &[]);

            cx.builder.switch_to_block(cow_done_block);
            cx.builder.seal_block(cow_done_block);

            // If we mutated through a named binding, repoint it at the (possibly
            // new) copy so subsequent reads see the mutation. Mirrors the
            // variable rebind in `IndexAssign`.
            if let Expr::Ident(name) = &object.node {
                if let Some((var, _cl_ty, _tty)) = cx.vars.get(name) {
                    let var = *var;
                    cx.builder.def_var(var, target);
                }
            }

            // Widen smaller types to 64-bit for uniform storage
            let val_ty = cx.builder.func.dfg.value_type(val);
            let val = if val_ty.bits() < 64 && val_ty.is_int() {
                cx.builder.ins().sextend(types::I64, val)
            } else if val_ty.is_float() && val_ty.bits() == 64 {
                cx.builder.ins().bitcast(types::I64, MemFlags::new(), val)
            } else if val_ty.is_float() && val_ty.bits() == 32 {
                let extended = cx.builder.ins().fpromote(types::F64, val);
                cx.builder
                    .ins()
                    .bitcast(types::I64, MemFlags::new(), extended)
            } else {
                val
            };

            if is_rc_managed_type(cx, &field_tty) {
                if expr_result_borrows_existing_rc(value) {
                    retain_if_needed(cx, val, &field_tty);
                }
                let old_val = cx
                    .builder
                    .ins()
                    .load(cx.ptr_type, MemFlags::new(), target, offset);
                release_if_needed(cx, old_val, &field_tty);
            }

            cx.builder.ins().store(MemFlags::new(), val, target, offset);
            Ok(None)
        }

        Expr::IndexAssign {
            object,
            index,
            value,
        } => {
            let (arr, arr_tty) = compile_expr(cx, object)?.ok_or_else(|| CodegenError {
                code: ErrorCode::E0400,
                message: "expected a value, but sub-expression has unit type".to_string(),
            })?;
            let elem_tty = match &arr_tty {
                TurboTy::Array(inner) => *inner.clone(),
                _ => TurboTy::Int,
            };
            let (idx, _) = compile_expr(cx, index)?.ok_or_else(|| CodegenError {
                code: ErrorCode::E0400,
                message: "expected a value, but sub-expression has unit type".to_string(),
            })?;
            let idx = {
                let idx_ty = cx.builder.func.dfg.value_type(idx);
                if idx_ty.is_int() && idx_ty.bits() < 64 {
                    cx.builder.ins().uextend(types::I64, idx)
                } else {
                    idx
                }
            };
            let (val, _) = compile_expr(cx, value)?.ok_or_else(|| CodegenError {
                code: ErrorCode::E0400,
                message: "expected a value, but sub-expression has unit type".to_string(),
            })?;

            let val_ty = cx.builder.func.dfg.value_type(val);
            let val = if val_ty.bits() < 64 && val_ty.is_int() {
                cx.builder.ins().sextend(types::I64, val)
            } else if val_ty.is_float() && val_ty.bits() == 64 {
                cx.builder.ins().bitcast(types::I64, MemFlags::new(), val)
            } else if val_ty.is_float() && val_ty.bits() == 32 {
                let extended = cx.builder.ins().fpromote(types::F64, val);
                cx.builder
                    .ins()
                    .bitcast(types::I64, MemFlags::new(), extended)
            } else {
                val
            };

            let trusted = MemFlags::trusted();
            let elem_is_rc = is_rc_managed_type(cx, &elem_tty);
            let value_borrows_existing = expr_result_borrows_existing_rc(value);
            if elem_is_rc && value_borrows_existing {
                retain_if_needed(cx, val, &elem_tty);
            }

            if cx.is_unsafe {
                // @unsafe: skip COW check and bounds check — direct store
                let data_base = cx.builder.ins().iadd_imm(arr, 8);
                let byte_offset = cx.builder.ins().ishl_imm(idx, 3);
                let elem_ptr = cx.builder.ins().iadd(data_base, byte_offset);
                let old_val = if elem_is_rc {
                    Some(cx.builder.ins().load(cx.ptr_type, trusted, elem_ptr, 0i32))
                } else {
                    None
                };
                cx.builder.ins().store(trusted, val, elem_ptr, 0i32);
                if let Some(old_val) = old_val {
                    release_if_needed(cx, old_val, &elem_tty);
                }
            } else {
                // COW check: if refcount > 1, call rt_array_set (slow/copy path)
                let rc = cx
                    .builder
                    .ins()
                    .load(types::I64, MemFlags::new(), arr, -8i32);
                let shared = cx.builder.ins().icmp_imm(IntCC::SignedGreaterThan, rc, 1);

                let slow_block = cx.builder.create_block();
                let fast_block = cx.builder.create_block();
                let merge_block = cx.builder.create_block();
                cx.builder.append_block_param(merge_block, types::I64);

                cx.builder
                    .ins()
                    .brif(shared, slow_block, &[], fast_block, &[]);

                // Slow path: call rt_array_set (handles COW copy)
                cx.builder.switch_to_block(slow_block);
                cx.builder.seal_block(slow_block);
                let set_fid = cx.rt_fns["rt_array_set"];
                let set_ref = cx.module.declare_func_in_func(set_fid, cx.builder.func);
                let call = cx.builder.ins().call(set_ref, &[arr, idx, val]);
                let slow_result = cx.builder.inst_results(call)[0];
                retain_array_elements_except_index_if_needed(cx, slow_result, &elem_tty, idx);
                cx.builder.ins().jump(merge_block, &[slow_result]);

                // Fast path: inline bounds check + store
                cx.builder.switch_to_block(fast_block);
                cx.builder.seal_block(fast_block);
                let len = cx.builder.ins().load(types::I64, trusted, arr, 0i32);
                let oob = cx
                    .builder
                    .ins()
                    .icmp(IntCC::UnsignedGreaterThanOrEqual, idx, len);

                let oob_block = cx.builder.create_block();
                let store_block = cx.builder.create_block();
                cx.builder.ins().brif(oob, oob_block, &[], store_block, &[]);

                cx.builder.switch_to_block(oob_block);
                cx.builder.seal_block(oob_block);
                let oob_fid = cx.rt_fns["rt_array_oob_exit"];
                let oob_ref = cx.module.declare_func_in_func(oob_fid, cx.builder.func);
                cx.builder.ins().call(oob_ref, &[idx, len]);
                cx.builder.ins().trap(TrapCode::unwrap_user(1));

                cx.builder.switch_to_block(store_block);
                cx.builder.seal_block(store_block);
                let data_base = cx.builder.ins().iadd_imm(arr, 8);
                let byte_offset = cx.builder.ins().ishl_imm(idx, 3);
                let elem_ptr = cx.builder.ins().iadd(data_base, byte_offset);
                let old_val = if elem_is_rc {
                    Some(cx.builder.ins().load(cx.ptr_type, trusted, elem_ptr, 0i32))
                } else {
                    None
                };
                cx.builder.ins().store(trusted, val, elem_ptr, 0i32);
                if let Some(old_val) = old_val {
                    release_if_needed(cx, old_val, &elem_tty);
                }
                cx.builder.ins().jump(merge_block, &[arr]);

                cx.builder.switch_to_block(merge_block);
                cx.builder.seal_block(merge_block);
                let new_arr = cx.builder.block_params(merge_block)[0];

                if let Expr::Ident(name) = &object.node {
                    if let Some((var, _cl_ty, _tty)) = cx.vars.get(name) {
                        let var = *var;
                        cx.builder.def_var(var, new_arr);
                    }
                }
            }

            Ok(None)
        }

        Expr::While { condition, body } => compile_while(cx, condition, body),

        // Await: if the inner value is a Future (thread handle), join it.
        // Otherwise (direct function call), just pass through.
        Expr::Await(inner) => {
            let result = compile_expr(cx, inner)?;
            if let Some((val, tty)) = result {
                match tty {
                    TurboTy::Future(inner_tty) => {
                        // This is a spawned thread handle — join it
                        let fid = cx.rt_fns["rt_await_handle"];
                        let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
                        let call = cx.builder.ins().call(fref, &[val]);
                        let mut result_val = cx.builder.inst_results(call)[0];
                        // rt_await_handle returns the result as raw i64 bits; if the
                        // future's value is a float, reinterpret those bits as F64 so
                        // the Cranelift value type matches its TurboTy.
                        if matches!(*inner_tty, TurboTy::Float) {
                            result_val =
                                cx.builder
                                    .ins()
                                    .bitcast(types::F64, MemFlags::new(), result_val);
                        }
                        Ok(Some((result_val, *inner_tty)))
                    }
                    _ => {
                        // Not a future — pass through (sync await)
                        Ok(Some((val, tty)))
                    }
                }
            } else {
                Ok(None)
            }
        }

        // Spawn: create an args struct, get thunk fn_ptr, call rt_spawn_with_args
        Expr::Spawn(inner) => {
            let span_start = expr.span.start;
            if let Some(thunk_name) = cx.spawn_thunks.get(&span_start).cloned() {
                if let Expr::Call { callee, args } = &inner.node {
                    if let Expr::Ident(callee_name) = &callee.node {
                        // Determine the return type of the spawned function
                        let inner_ret_tty = cx
                            .fn_ret_types
                            .get(callee_name.as_str())
                            .cloned()
                            .unwrap_or(TurboTy::Unit);

                        // Get the target function's address
                        let target_fid =
                            *cx.user_fns
                                .get(callee_name.as_str())
                                .ok_or_else(|| CodegenError {
                                    code: ErrorCode::E0402,
                                    message: format!("spawn: unknown function `{}`", callee_name),
                                })?;
                        let target_fref =
                            cx.module.declare_func_in_func(target_fid, cx.builder.func);
                        let target_fn_ptr = cx.builder.ins().func_addr(cx.ptr_type, target_fref);

                        // Compile all arguments. Alongside each value we track
                        // whether the argument is a heap string, building a
                        // pointer mask so rt_spawn_with_args can deep-copy
                        // arena-backed string args before they cross the thread
                        // boundary (arena-escape fix, issue #56).
                        let mut arg_vals = Vec::new();
                        let mut owned_string_arg_temps = Vec::new();
                        let mut ptr_mask: i64 = 0;
                        for arg in args {
                            if let Some((val, tty)) = compile_expr(cx, arg)? {
                                if matches!(tty, TurboTy::Str) && arg_vals.len() < 64 {
                                    ptr_mask |= 1i64 << arg_vals.len();
                                }
                                if matches!(tty, TurboTy::Str) && expr_produces_owned_rc_temp(arg) {
                                    owned_string_arg_temps.push((val, tty.clone()));
                                }
                                let val = match tty {
                                    TurboTy::Bool => cx.builder.ins().sextend(types::I64, val),
                                    TurboTy::Float => {
                                        cx.builder.ins().bitcast(types::I64, MemFlags::new(), val)
                                    }
                                    _ => val,
                                };
                                arg_vals.push(val);
                            }
                        }

                        // Allocate args struct: [fn_ptr, arg0, arg1, ...]
                        let num_slots = (1 + arg_vals.len()) as i64;
                        let num_slots_val = cx.builder.ins().iconst(types::I64, num_slots);
                        let alloc_fid = cx.rt_fns["rt_struct_alloc"];
                        let alloc_fref = cx.module.declare_func_in_func(alloc_fid, cx.builder.func);
                        let call = cx.builder.ins().call(alloc_fref, &[num_slots_val]);
                        let args_ptr = cx.builder.inst_results(call)[0];

                        // Store fn_ptr at offset 0
                        cx.builder
                            .ins()
                            .store(MemFlags::new(), target_fn_ptr, args_ptr, 0);
                        // Store args at offsets 8, 16, 24, ...
                        for (i, val) in arg_vals.iter().enumerate() {
                            let offset = ((i + 1) * 8) as i32;
                            cx.builder
                                .ins()
                                .store(MemFlags::new(), *val, args_ptr, offset);
                        }

                        // Get the thunk function address
                        let thunk_fid =
                            *cx.user_fns
                                .get(thunk_name.as_str())
                                .ok_or_else(|| CodegenError {
                                    code: ErrorCode::E0405,
                                    message: format!("spawn: thunk `{}` not found", thunk_name),
                                })?;
                        let thunk_fref = cx.module.declare_func_in_func(thunk_fid, cx.builder.func);
                        let thunk_fn_ptr = cx.builder.ins().func_addr(cx.ptr_type, thunk_fref);

                        // Call rt_spawn_with_args(thunk_ptr, args_ptr, ptr_mask,
                        // num_args) -> handle
                        let spawn_fid = cx.rt_fns["rt_spawn_with_args"];
                        let spawn_fref = cx.module.declare_func_in_func(spawn_fid, cx.builder.func);
                        let ptr_mask_val = cx.builder.ins().iconst(types::I64, ptr_mask);
                        let num_args_val =
                            cx.builder.ins().iconst(types::I64, arg_vals.len() as i64);
                        let call = cx.builder.ins().call(
                            spawn_fref,
                            &[thunk_fn_ptr, args_ptr, ptr_mask_val, num_args_val],
                        );
                        let handle = cx.builder.inst_results(call)[0];
                        for (value, tty) in owned_string_arg_temps {
                            release_if_needed(cx, value, &tty);
                        }
                        let release_fid = cx.rt_fns["rt_release"];
                        let release_ref =
                            cx.module.declare_func_in_func(release_fid, cx.builder.func);
                        cx.builder.ins().call(release_ref, &[args_ptr]);

                        return Ok(Some((handle, TurboTy::Future(Box::new(inner_ret_tty)))));
                    }
                }
            }
            // Fallback: synchronous execution (backward compat)
            compile_expr(cx, inner)
        }

        // Try operator: expr? — unwrap Ok, propagate Err (simplified: just evaluate inner)
        // Try operator: expr? — unwrap Ok, propagate Err
        Expr::Try(inner) => {
            // Compile the inner expression (must produce a Result pointer)
            let (result_ptr, _result_tty) =
                compile_expr(cx, inner)?.ok_or_else(|| CodegenError {
                    code: ErrorCode::E0400,
                    message: "expected a value, but sub-expression has unit type".to_string(),
                })?;

            // Get the tag: 0 = ok, 1 = err
            let tag_fid = cx.rt_fns["rt_result_tag"];
            let tag_fref = cx.module.declare_func_in_func(tag_fid, cx.builder.func);
            let tag_call = cx.builder.ins().call(tag_fref, &[result_ptr]);
            let tag = cx.builder.inst_results(tag_call)[0];

            // Branch: if tag == 0 (ok), continue; else return the Result as-is
            let zero = cx.builder.ins().iconst(types::I64, 0);
            let is_ok = cx.builder.ins().icmp(IntCC::Equal, tag, zero);

            let ok_block = cx.builder.create_block();
            let err_block = cx.builder.create_block();

            cx.builder.ins().brif(is_ok, ok_block, &[], err_block, &[]);

            // err_block: propagate the error by returning the Result pointer
            cx.builder.switch_to_block(err_block);
            cx.builder.seal_block(err_block);
            cx.builder.ins().return_(&[result_ptr]);

            // ok_block: extract the ok value and continue
            cx.builder.switch_to_block(ok_block);
            cx.builder.seal_block(ok_block);
            let val_fid = cx.rt_fns["rt_result_value"];
            let val_fref = cx.module.declare_func_in_func(val_fid, cx.builder.func);
            let val_call = cx.builder.ins().call(val_fref, &[result_ptr]);
            let ok_value = cx.builder.inst_results(val_call)[0];

            let ok_tty = match _result_tty {
                TurboTy::Result(ref ok, _) => *ok.clone(),
                _ => TurboTy::Int,
            };
            Ok(Some((ok_value, ok_tty)))
        }

        Expr::ForIn {
            var_name,
            iterable,
            body,
        } => compile_for_in(cx, var_name, iterable, body),

        Expr::Range { .. } => Err(CodegenError {
            code: ErrorCode::E0400,
            message: "range expressions can only be used in for-in loops".to_string(),
        }),

        Expr::Break => {
            if let Some(&(_header, exit)) = cx.loop_stack.last() {
                cx.builder.ins().jump(exit, &[]);
                // Create an unreachable block so subsequent code has somewhere to go
                let dead_block = cx.builder.create_block();
                cx.builder.switch_to_block(dead_block);
                cx.builder.seal_block(dead_block);
            }
            Ok(None)
        }

        Expr::Continue => {
            if let Some(&(header, _exit)) = cx.loop_stack.last() {
                cx.builder.ins().jump(header, &[]);
                // Create an unreachable block so subsequent code has somewhere to go
                let dead_block = cx.builder.create_block();
                cx.builder.switch_to_block(dead_block);
                cx.builder.seal_block(dead_block);
            }
            Ok(None)
        }

        Expr::ArrayLit(elements) => {
            let len = elements.len() as i64;
            let len_val = cx.builder.ins().iconst(types::I64, len);

            let alloc_fid = cx.rt_fns["rt_array_alloc"];
            let alloc_ref = cx.module.declare_func_in_func(alloc_fid, cx.builder.func);
            let call = cx.builder.ins().call(alloc_ref, &[len_val]);
            let arr_ptr = cx.builder.inst_results(call)[0];

            let mut elem_tty = TurboTy::Int; // default; overridden by first element
            for (i, elem) in elements.iter().enumerate() {
                let (val, tty) = compile_expr(cx, elem)?.ok_or_else(|| CodegenError {
                    code: ErrorCode::E0400,
                    message: "expected a value, but sub-expression has unit type".to_string(),
                })?;
                if i == 0 {
                    elem_tty = tty;
                }
                if is_rc_managed_type(cx, &elem_tty) && expr_result_borrows_existing_rc(elem) {
                    retain_if_needed(cx, val, &elem_tty);
                }
                let offset = cx.builder.ins().iconst(cx.ptr_type, (8 + i * 8) as i64);
                let elem_ptr = cx.builder.ins().iadd(arr_ptr, offset);
                cx.builder.ins().store(MemFlags::new(), val, elem_ptr, 0);
            }

            Ok(Some((arr_ptr, TurboTy::Array(Box::new(elem_tty)))))
        }

        Expr::Index { object, index } => {
            let (arr, arr_tty) = compile_expr(cx, object)?.ok_or_else(|| CodegenError {
                code: ErrorCode::E0400,
                message: "expected a value, but sub-expression has unit type".to_string(),
            })?;
            let (idx, _) = compile_expr(cx, index)?.ok_or_else(|| CodegenError {
                code: ErrorCode::E0400,
                message: "expected a value, but sub-expression has unit type".to_string(),
            })?;
            let idx = {
                let idx_ty = cx.builder.func.dfg.value_type(idx);
                if idx_ty.is_int() && idx_ty.bits() < 64 {
                    cx.builder.ins().uextend(types::I64, idx)
                } else {
                    idx
                }
            };

            let trusted = MemFlags::trusted();

            if !cx.is_unsafe {
                // Bounds check: load length, compare, branch to OOB handler
                let len = cx.builder.ins().load(types::I64, trusted, arr, 0i32);
                let oob = cx
                    .builder
                    .ins()
                    .icmp(IntCC::UnsignedGreaterThanOrEqual, idx, len);

                let oob_block = cx.builder.create_block();
                let ok_block = cx.builder.create_block();
                cx.builder.ins().brif(oob, oob_block, &[], ok_block, &[]);

                cx.builder.switch_to_block(oob_block);
                cx.builder.seal_block(oob_block);
                let oob_fid = cx.rt_fns["rt_array_oob_exit"];
                let oob_ref = cx.module.declare_func_in_func(oob_fid, cx.builder.func);
                cx.builder.ins().call(oob_ref, &[idx, len]);
                cx.builder.ins().trap(TrapCode::unwrap_user(1));

                cx.builder.switch_to_block(ok_block);
                cx.builder.seal_block(ok_block);
            }

            // data starts at arr+8 (skip length field)
            let data_base = cx.builder.ins().iadd_imm(arr, 8);
            let byte_offset = cx.builder.ins().ishl_imm(idx, 3);
            let elem_ptr = cx.builder.ins().iadd(data_base, byte_offset);
            let raw = cx.builder.ins().load(types::I64, trusted, elem_ptr, 0i32);

            let elem_tty = match arr_tty {
                TurboTy::Array(inner) => *inner,
                _ => TurboTy::Int,
            };

            let (result, result_tty) = match &elem_tty {
                TurboTy::Bool => {
                    let truncated = cx.builder.ins().ireduce(types::I8, raw);
                    (truncated, elem_tty)
                }
                // Narrow integer elements are stored in the array's 8-byte slots
                // but flow as their own width elsewhere. Truncate the loaded i64
                // back to the element width so its IR type matches its tag
                // (e.g. `let b: [u8] = [104, 105]; b[0]` yields an i8, not an
                // i64 mislabelled `u8`, which the print/convert path would
                // double-extend and crash on).
                TurboTy::I8 | TurboTy::U8 => {
                    let truncated = cx.builder.ins().ireduce(types::I8, raw);
                    (truncated, elem_tty)
                }
                TurboTy::I16 | TurboTy::U16 => {
                    let truncated = cx.builder.ins().ireduce(types::I16, raw);
                    (truncated, elem_tty)
                }
                TurboTy::Float => {
                    let f = cx.builder.ins().bitcast(types::F64, MemFlags::new(), raw);
                    (f, elem_tty)
                }
                _ => (raw, elem_tty),
            };
            Ok(Some((result, result_tty)))
        }

        Expr::StructLit { name, fields } => {
            let struct_layout = cx
                .struct_fields
                .get(name)
                .ok_or_else(|| CodegenError {
                    code: ErrorCode::E0400,
                    message: format!("undefined struct: {name}"),
                })?
                .clone();

            let num_fields = struct_layout.len() as i64;
            let num_fields_val = cx.builder.ins().iconst(types::I64, num_fields);

            // Call rt_struct_alloc to allocate memory
            let alloc_fid = cx.rt_fns["rt_struct_alloc"];
            let alloc_fref = cx.module.declare_func_in_func(alloc_fid, cx.builder.func);
            let call = cx.builder.ins().call(alloc_fref, &[num_fields_val]);
            let ptr = cx.builder.inst_results(call)[0];

            // Track concrete field types for generic structs
            let mut concrete_fields: Vec<(String, TurboTy)> = Vec::new();

            // Store each field at its offset
            for (field_name, field_value) in fields {
                let field_index = struct_layout
                    .iter()
                    .position(|(n, _)| n == field_name)
                    .ok_or_else(|| CodegenError {
                        code: ErrorCode::E0400,
                        message: format!("struct `{name}` has no field `{field_name}`"),
                    })?;

                let (val, tty) = compile_expr(cx, field_value)?.ok_or_else(|| CodegenError {
                    code: ErrorCode::E0400,
                    message: "expected a value, but sub-expression has unit type".to_string(),
                })?;
                if expr_result_borrows_existing_rc(field_value) {
                    retain_if_needed(cx, val, &tty);
                }
                concrete_fields.push((field_name.clone(), tty));
                let offset = (field_index * 8) as i32;

                // Widen smaller types to 64-bit for uniform storage
                let val_ty = cx.builder.func.dfg.value_type(val);
                let val = if val_ty.bits() < 64 && val_ty.is_int() {
                    cx.builder.ins().sextend(types::I64, val)
                } else if val_ty.is_float() && val_ty.bits() == 64 {
                    cx.builder.ins().bitcast(types::I64, MemFlags::new(), val)
                } else if val_ty.is_float() && val_ty.bits() == 32 {
                    let extended = cx.builder.ins().fpromote(types::F64, val);
                    cx.builder
                        .ins()
                        .bitcast(types::I64, MemFlags::new(), extended)
                } else {
                    val
                };

                cx.builder.ins().store(MemFlags::new(), val, ptr, offset);
            }

            // Store concrete field types for generic struct instances
            cx.last_struct_lit_concrete_fields = Some(concrete_fields);

            Ok(Some((ptr, TurboTy::Struct(name.clone()))))
        }

        Expr::FieldAccess { object, field } => {
            // Check if this is actually an enum variant access: EnumName.VariantName
            if let Expr::Ident(ref name) = object.node {
                if let Some(variants) = cx.enum_variants.get(name.as_str()) {
                    let index =
                        variants
                            .iter()
                            .position(|v| v == field)
                            .ok_or_else(|| CodegenError {
                                code: ErrorCode::E0400,
                                message: format!("enum `{name}` has no variant `{field}`"),
                            })?;

                    // Check if this is a data-carrying enum
                    if let Some(&max_slots) = cx.enum_max_slots.get(name.as_str()) {
                        // Allocate tagged union: [tag][slot0][slot1]...[slotN]
                        let total_slots = 1 + max_slots; // tag + payload
                        let num_fields_val =
                            cx.builder.ins().iconst(types::I64, total_slots as i64);
                        let alloc_fid = cx.rt_fns["rt_struct_alloc"];
                        let alloc_fref = cx.module.declare_func_in_func(alloc_fid, cx.builder.func);
                        let call = cx.builder.ins().call(alloc_fref, &[num_fields_val]);
                        let ptr = cx.builder.inst_results(call)[0];
                        // Store tag
                        let tag_val = cx.builder.ins().iconst(types::I64, index as i64);
                        cx.builder.ins().store(MemFlags::new(), tag_val, ptr, 0);
                        return Ok(Some((ptr, TurboTy::Enum(name.clone()))));
                    } else {
                        let val = cx.builder.ins().iconst(types::I64, index as i64);
                        return Ok(Some((val, TurboTy::Enum(name.clone()))));
                    }
                }
            }

            let (obj_ptr, obj_tty) = compile_expr(cx, object)?.ok_or_else(|| CodegenError {
                code: ErrorCode::E0400,
                message: "expected a value, but sub-expression has unit type".to_string(),
            })?;

            let struct_name = match &obj_tty {
                TurboTy::Struct(name) => name.clone(),
                _ => {
                    return Err(CodegenError {
                        code: ErrorCode::E0400,
                        message: "field access on non-struct type".to_string(),
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

            let field_index = struct_layout
                .iter()
                .position(|(n, _)| n == field)
                .ok_or_else(|| CodegenError {
                    code: ErrorCode::E0400,
                    message: format!("struct `{struct_name}` has no field `{field}`"),
                })?;

            let mut field_tty = struct_layout[field_index].1.clone();

            // For generic structs, check if we have concrete field type overrides
            if let Expr::Ident(ref var_name) = object.node {
                if let Some(concrete_fields) = cx.generic_struct_field_overrides.get(var_name) {
                    if let Some((_, concrete_tty)) =
                        concrete_fields.iter().find(|(n, _)| n == field)
                    {
                        field_tty = concrete_tty.clone();
                    }
                }
            }

            let offset = (field_index * 8) as i32;

            // Load from the struct pointer
            let raw_val = cx
                .builder
                .ins()
                .load(types::I64, MemFlags::new(), obj_ptr, offset);

            // Convert back to the appropriate type
            let (val, tty) = match &field_tty {
                TurboTy::Int => (raw_val, TurboTy::Int),
                TurboTy::Bool => {
                    let truncated = cx.builder.ins().ireduce(types::I8, raw_val);
                    (truncated, TurboTy::Bool)
                }
                TurboTy::Float => {
                    let f = cx
                        .builder
                        .ins()
                        .bitcast(types::F64, MemFlags::new(), raw_val);
                    (f, TurboTy::Float)
                }
                TurboTy::Str => (raw_val, TurboTy::Str),
                TurboTy::Struct(name) => (raw_val, TurboTy::Struct(name.clone())),
                _ => (raw_val, field_tty),
            };

            Ok(Some((val, tty)))
        }

        Expr::EnumVariant { enum_name, variant } => {
            let variants =
                cx.enum_variants
                    .get(enum_name.as_str())
                    .ok_or_else(|| CodegenError {
                        code: ErrorCode::E0400,
                        message: format!("undefined enum: {enum_name}"),
                    })?;
            let index = variants
                .iter()
                .position(|v| v == variant)
                .ok_or_else(|| CodegenError {
                    code: ErrorCode::E0400,
                    message: format!("enum `{enum_name}` has no variant `{variant}`"),
                })?;

            // Check if this is a data-carrying enum
            if let Some(&max_slots) = cx.enum_max_slots.get(enum_name.as_str()) {
                // Allocate tagged union: [tag][slot0][slot1]...[slotN]
                let total_slots = 1 + max_slots;
                let num_fields_val = cx.builder.ins().iconst(types::I64, total_slots as i64);
                let alloc_fid = cx.rt_fns["rt_struct_alloc"];
                let alloc_fref = cx.module.declare_func_in_func(alloc_fid, cx.builder.func);
                let call = cx.builder.ins().call(alloc_fref, &[num_fields_val]);
                let ptr = cx.builder.inst_results(call)[0];
                let tag_val = cx.builder.ins().iconst(types::I64, index as i64);
                cx.builder.ins().store(MemFlags::new(), tag_val, ptr, 0);
                Ok(Some((ptr, TurboTy::Enum(enum_name.clone()))))
            } else {
                let val = cx.builder.ins().iconst(types::I64, index as i64);
                Ok(Some((val, TurboTy::Enum(enum_name.clone()))))
            }
        }

        Expr::Match { subject, arms } => compile_match(cx, subject, arms),

        Expr::Interpolation(parts) => compile_interpolation(cx, parts),

        Expr::Closure { params, .. } => {
            // Look up the pre-declared closure function by span start
            let span_start = expr.span.start;
            let (closure_name, closure_ty, _free_vars) = cx
                .closure_fns
                .get(&span_start)
                .ok_or_else(|| CodegenError {
                    code: ErrorCode::E0400,
                    message: "internal error: closure not found in pre-compiled map".to_string(),
                })?;
            let closure_ty = closure_ty.clone();
            let closure_name = closure_name.clone();
            let func_id = *cx
                .user_fns
                .get(closure_name.as_str())
                .ok_or_else(|| CodegenError {
                    code: ErrorCode::E0400,
                    message: format!(
                        "internal error: closure function {} not found",
                        closure_name
                    ),
                })?;
            let func_ref = cx.module.declare_func_in_func(func_id, cx.builder.func);
            let fn_ptr = cx.builder.ins().func_addr(cx.ptr_type, func_ref);

            // Determine captures: free variables that exist in the current scope
            let outer_var_names: Vec<String> = cx.vars.keys().cloned().collect();
            let capture_names = find_captures(params, &expr.node, &outer_var_names);

            // Build capture info with types
            let mut captures: Vec<(String, TurboTy)> = Vec::new();
            for cap_name in &capture_names {
                if let Some((_var, _cl_ty, turbo_ty)) = cx.vars.get(cap_name) {
                    captures.push((cap_name.clone(), turbo_ty.clone()));
                }
            }

            // Store capture info for the closure body compiler
            cx.closure_captures.insert(
                span_start,
                CaptureInfo {
                    captures: captures.clone(),
                },
            );

            // Allocate environment struct for captured variables
            let num_captures = captures.len() as i64;
            let env_ptr = if num_captures > 0 {
                let num_fields_val = cx.builder.ins().iconst(types::I64, num_captures);
                let alloc_fid = cx.rt_fns["rt_struct_alloc"];
                let alloc_fref = cx.module.declare_func_in_func(alloc_fid, cx.builder.func);
                let call = cx.builder.ins().call(alloc_fref, &[num_fields_val]);
                let env_ptr = cx.builder.inst_results(call)[0];

                // Store each captured variable into the env struct
                for (cap_idx, (cap_name, _cap_tty)) in captures.iter().enumerate() {
                    let (var, _cl_ty, _turbo_ty) =
                        cx.vars.get(cap_name).ok_or_else(|| CodegenError {
                            code: ErrorCode::E0400,
                            message: format!(
                                "internal error: capture variable {} not found",
                                cap_name
                            ),
                        })?;
                    let val = cx.builder.use_var(*var);
                    let offset = (cap_idx * 8) as i32;

                    // Widen to i64 for uniform storage
                    let val_ty = cx.builder.func.dfg.value_type(val);
                    let val = if val_ty.bits() < 64 && val_ty.is_int() {
                        cx.builder.ins().sextend(types::I64, val)
                    } else if val_ty.is_float() && val_ty.bits() == 64 {
                        cx.builder.ins().bitcast(types::I64, MemFlags::new(), val)
                    } else if val_ty.is_float() && val_ty.bits() == 32 {
                        let extended = cx.builder.ins().fpromote(types::F64, val);
                        cx.builder
                            .ins()
                            .bitcast(types::I64, MemFlags::new(), extended)
                    } else {
                        val
                    };
                    cx.builder
                        .ins()
                        .store(MemFlags::new(), val, env_ptr, offset);
                }
                env_ptr
            } else {
                // No captures: null env pointer
                cx.builder.ins().iconst(cx.ptr_type, 0)
            };

            // Allocate closure pair struct: [fn_ptr, env_ptr]
            let two = cx.builder.ins().iconst(types::I64, 2);
            let alloc_fid = cx.rt_fns["rt_struct_alloc"];
            let alloc_fref = cx.module.declare_func_in_func(alloc_fid, cx.builder.func);
            let call = cx.builder.ins().call(alloc_fref, &[two]);
            let closure_ptr = cx.builder.inst_results(call)[0];

            // Store fn_ptr at offset 0
            cx.builder
                .ins()
                .store(MemFlags::new(), fn_ptr, closure_ptr, 0);
            // Store env_ptr at offset 8
            cx.builder
                .ins()
                .store(MemFlags::new(), env_ptr, closure_ptr, 8);

            Ok(Some((closure_ptr, closure_ty)))
        }

        Expr::OkExpr(value) => {
            let (val, tty) = compile_expr(cx, value)?.ok_or_else(|| CodegenError {
                code: ErrorCode::E0400,
                message: "expected a value, but sub-expression has unit type".to_string(),
            })?;
            if expr_result_borrows_existing_rc(value) {
                retain_if_needed(cx, val, &tty);
            }
            // Widen to i64 if needed (bools, etc.)
            let val_ty = cx.builder.func.dfg.value_type(val);
            let val = if val_ty.is_int() && val_ty.bits() < 64 {
                cx.builder.ins().sextend(types::I64, val)
            } else if val_ty.is_float() {
                cx.builder.ins().bitcast(types::I64, MemFlags::new(), val)
            } else {
                val
            };
            let fid = cx.rt_fns["rt_result_ok"];
            let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
            let call = cx.builder.ins().call(fref, &[val]);
            let ptr = cx.builder.inst_results(call)[0];
            Ok(Some((
                ptr,
                TurboTy::Result(Box::new(tty), Box::new(TurboTy::Int)),
            )))
        }

        Expr::ErrExpr(value) => {
            let (val, tty) = compile_expr(cx, value)?.ok_or_else(|| CodegenError {
                code: ErrorCode::E0400,
                message: "expected a value, but sub-expression has unit type".to_string(),
            })?;
            if expr_result_borrows_existing_rc(value) {
                retain_if_needed(cx, val, &tty);
            }
            // Widen to i64 if needed
            let val_ty = cx.builder.func.dfg.value_type(val);
            let val = if val_ty.is_int() && val_ty.bits() < 64 {
                cx.builder.ins().sextend(types::I64, val)
            } else if val_ty.is_float() {
                cx.builder.ins().bitcast(types::I64, MemFlags::new(), val)
            } else {
                val
            };
            let fid = cx.rt_fns["rt_result_err"];
            let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
            let call = cx.builder.ins().call(fref, &[val]);
            let ptr = cx.builder.inst_results(call)[0];
            Ok(Some((
                ptr,
                TurboTy::Result(Box::new(TurboTy::Int), Box::new(tty)),
            )))
        }

        Expr::SomeExpr(value) => {
            let (val, tty) = compile_expr(cx, value)?.ok_or_else(|| CodegenError {
                code: ErrorCode::E0400,
                message: "expected a value, but sub-expression has unit type".to_string(),
            })?;
            if expr_result_borrows_existing_rc(value) {
                retain_if_needed(cx, val, &tty);
            }
            // Widen to i64 if needed (bools, etc.)
            let val_ty = cx.builder.func.dfg.value_type(val);
            let val = if val_ty.is_int() && val_ty.bits() < 64 {
                cx.builder.ins().sextend(types::I64, val)
            } else if val_ty.is_float() {
                cx.builder.ins().bitcast(types::I64, MemFlags::new(), val)
            } else {
                val
            };
            let fid = cx.rt_fns["rt_option_some"];
            let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
            let call = cx.builder.ins().call(fref, &[val]);
            let ptr = cx.builder.inst_results(call)[0];
            Ok(Some((ptr, TurboTy::Optional(Box::new(tty)))))
        }

        Expr::NoneExpr => {
            let fid = cx.rt_fns["rt_option_none"];
            let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
            let call = cx.builder.ins().call(fref, &[]);
            let ptr = cx.builder.inst_results(call)[0];
            Ok(Some((ptr, TurboTy::Optional(Box::new(TurboTy::Int)))))
        }

        Expr::NullCoalesce { value, default } => {
            // Compile the optional value
            let (opt_val, _opt_tty) = compile_expr(cx, value)?.ok_or_else(|| CodegenError {
                code: ErrorCode::E0400,
                message: "expected a value, but sub-expression has unit type".to_string(),
            })?;

            // Extract tag
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

            // Some path: extract value
            cx.builder.switch_to_block(some_block);
            cx.builder.seal_block(some_block);
            let val_fid = cx.rt_fns["rt_option_value"];
            let val_fref = cx.module.declare_func_in_func(val_fid, cx.builder.func);
            let val_call = cx.builder.ins().call(val_fref, &[opt_val]);
            let some_val = cx.builder.inst_results(val_call)[0];
            cx.builder.ins().jump(merge_block, &[some_val]);

            // None path: compile default
            cx.builder.switch_to_block(none_block);
            cx.builder.seal_block(none_block);
            let (def_val, def_tty) = compile_expr(cx, default)?.ok_or_else(|| CodegenError {
                code: ErrorCode::E0400,
                message: "expected a value, but sub-expression has unit type".to_string(),
            })?;
            // Widen default to i64 if needed for consistency
            let def_ty = cx.builder.func.dfg.value_type(def_val);
            let def_val = if def_ty.is_int() && def_ty.bits() < 64 {
                cx.builder.ins().sextend(types::I64, def_val)
            } else if def_ty.is_float() {
                cx.builder
                    .ins()
                    .bitcast(types::I64, MemFlags::new(), def_val)
            } else {
                def_val
            };
            cx.builder.ins().jump(merge_block, &[def_val]);

            // Merge block. Both edges carry the value as raw i64 bits (the
            // some-path payload and the float-bitcast default), so the param is
            // i64. If the result type is Float, bitcast back to F64 before
            // returning — otherwise the caller would fadd an i64-classed value
            // (wrong answer + backend register-class panic).
            cx.builder.append_block_param(merge_block, types::I64);
            cx.builder.switch_to_block(merge_block);
            cx.builder.seal_block(merge_block);
            let result = cx.builder.block_params(merge_block)[0];

            let result = if matches!(def_tty, TurboTy::Float) {
                cx.builder
                    .ins()
                    .bitcast(types::F64, MemFlags::new(), result)
            } else {
                result
            };

            Ok(Some((result, def_tty)))
        }

        Expr::OptionalChain { object, field } => compile_optional_chain(cx, object, field),

        Expr::MapLit(entries) => compile_map_lit(cx, entries),
    }
}

// ── RC heap type helpers ────────────────────────────────────────────

pub(crate) fn is_rc_heap_type(ty: &TurboTy) -> bool {
    matches!(
        ty,
        TurboTy::Str
            | TurboTy::Array(_)
            | TurboTy::Struct(_)
            | TurboTy::Result(_, _)
            | TurboTy::Optional(_)
    )
}

pub(crate) fn is_rc_managed_type_with_layouts(
    ty: &TurboTy,
    enum_max_slots: &HashMap<String, usize>,
) -> bool {
    is_rc_heap_type(ty)
        || matches!(ty, TurboTy::HashMap(_, _))
        || matches!(ty, TurboTy::Enum(name) if enum_max_slots.contains_key(name.as_str()))
}

pub(crate) fn is_rc_managed_type<M: Module>(cx: &Ctx<'_, M>, ty: &TurboTy) -> bool {
    is_rc_managed_type_with_layouts(ty, cx.enum_max_slots)
}

pub(crate) fn retain_if_needed<M: Module>(cx: &mut Ctx<'_, M>, value: Value, ty: &TurboTy) {
    if matches!(ty, TurboTy::HashMap(_, _)) {
        let retain_fid = cx.rt_fns["rt_hashmap_gretain"];
        let retain_ref = cx.module.declare_func_in_func(retain_fid, cx.builder.func);
        cx.builder.ins().call(retain_ref, &[value]);
        return;
    }
    if !is_rc_managed_type(cx, ty) {
        return;
    }
    let retain_fid = cx.rt_fns["rt_retain"];
    let retain_ref = cx.module.declare_func_in_func(retain_fid, cx.builder.func);
    cx.builder.ins().call(retain_ref, &[value]);
}

pub(crate) fn retain_array_prefix_if_needed<M: Module>(
    cx: &mut Ctx<'_, M>,
    array: Value,
    elem_ty: &TurboTy,
    len: Value,
) {
    if !is_rc_managed_type(cx, elem_ty) {
        return;
    }
    let idx_var = cx.fresh_var(types::I64, TurboTy::Int);
    let zero = cx.builder.ins().iconst(types::I64, 0);
    cx.builder.def_var(idx_var, zero);

    let header_block = cx.builder.create_block();
    let body_block = cx.builder.create_block();
    let done_block = cx.builder.create_block();

    cx.builder.ins().jump(header_block, &[]);

    cx.builder.switch_to_block(header_block);
    let idx = cx.builder.use_var(idx_var);
    let keep_going = cx.builder.ins().icmp(IntCC::SignedLessThan, idx, len);
    cx.builder
        .ins()
        .brif(keep_going, body_block, &[], done_block, &[]);

    cx.builder.switch_to_block(body_block);
    cx.builder.seal_block(body_block);
    let idx = cx.builder.use_var(idx_var);
    let data_base = cx.builder.ins().iadd_imm(array, 8);
    let byte_offset = cx.builder.ins().ishl_imm(idx, 3);
    let elem_ptr = cx.builder.ins().iadd(data_base, byte_offset);
    let elem_val = cx
        .builder
        .ins()
        .load(cx.ptr_type, MemFlags::new(), elem_ptr, 0);
    retain_if_needed(cx, elem_val, elem_ty);
    let one = cx.builder.ins().iconst(types::I64, 1);
    let next_idx = cx.builder.ins().iadd(idx, one);
    cx.builder.def_var(idx_var, next_idx);
    cx.builder.ins().jump(header_block, &[]);

    cx.builder.seal_block(header_block);
    cx.builder.switch_to_block(done_block);
    cx.builder.seal_block(done_block);
}

fn retain_array_elements_except_index_if_needed<M: Module>(
    cx: &mut Ctx<'_, M>,
    array: Value,
    elem_ty: &TurboTy,
    skip_idx: Value,
) {
    if !is_rc_managed_type(cx, elem_ty) {
        return;
    }
    let len = cx.builder.ins().load(types::I64, MemFlags::new(), array, 0);
    let idx_var = cx.fresh_var(types::I64, TurboTy::Int);
    let zero = cx.builder.ins().iconst(types::I64, 0);
    cx.builder.def_var(idx_var, zero);

    let header_block = cx.builder.create_block();
    let body_block = cx.builder.create_block();
    let retain_block = cx.builder.create_block();
    let inc_block = cx.builder.create_block();
    let done_block = cx.builder.create_block();

    cx.builder.ins().jump(header_block, &[]);

    cx.builder.switch_to_block(header_block);
    let idx = cx.builder.use_var(idx_var);
    let keep_going = cx.builder.ins().icmp(IntCC::SignedLessThan, idx, len);
    cx.builder
        .ins()
        .brif(keep_going, body_block, &[], done_block, &[]);

    cx.builder.switch_to_block(body_block);
    cx.builder.seal_block(body_block);
    let idx = cx.builder.use_var(idx_var);
    let is_skip = cx.builder.ins().icmp(IntCC::Equal, idx, skip_idx);
    cx.builder
        .ins()
        .brif(is_skip, inc_block, &[], retain_block, &[]);

    cx.builder.switch_to_block(retain_block);
    cx.builder.seal_block(retain_block);
    let idx = cx.builder.use_var(idx_var);
    let data_base = cx.builder.ins().iadd_imm(array, 8);
    let byte_offset = cx.builder.ins().ishl_imm(idx, 3);
    let elem_ptr = cx.builder.ins().iadd(data_base, byte_offset);
    let elem_val = cx
        .builder
        .ins()
        .load(cx.ptr_type, MemFlags::new(), elem_ptr, 0);
    retain_if_needed(cx, elem_val, elem_ty);
    cx.builder.ins().jump(inc_block, &[]);

    cx.builder.switch_to_block(inc_block);
    cx.builder.seal_block(inc_block);
    let idx = cx.builder.use_var(idx_var);
    let one = cx.builder.ins().iconst(types::I64, 1);
    let next_idx = cx.builder.ins().iadd(idx, one);
    cx.builder.def_var(idx_var, next_idx);
    cx.builder.ins().jump(header_block, &[]);

    cx.builder.seal_block(header_block);
    cx.builder.switch_to_block(done_block);
    cx.builder.seal_block(done_block);
}

pub(crate) fn retain_array_elements_if_needed<M: Module>(
    cx: &mut Ctx<'_, M>,
    array: Value,
    elem_ty: &TurboTy,
) {
    if !is_rc_managed_type(cx, elem_ty) {
        return;
    }
    let len = cx.builder.ins().load(types::I64, MemFlags::new(), array, 0);
    retain_array_prefix_if_needed(cx, array, elem_ty, len);
}

fn pattern_binds_name(pattern: &Pattern, name: &str) -> bool {
    match pattern {
        Pattern::Ident(binding)
        | Pattern::Ok(binding)
        | Pattern::Err(binding)
        | Pattern::Some(binding) => binding == name,
        Pattern::VariantDestructure { bindings, .. } => bindings.iter().any(|b| b == name),
        _ => false,
    }
}

fn match_arm_yields_subject_binding(arm: &MatchArm) -> bool {
    matches!(&arm.body.node, Expr::Ident(name) if pattern_binds_name(&arm.pattern.node, name))
}

fn match_arm_yields_owned_or_static_rc(arm: &MatchArm) -> bool {
    expr_produces_owned_rc_temp(&arm.body)
        || matches!(arm.body.node, Expr::StringLit(_))
        || match_arm_yields_subject_binding(arm)
}

pub(crate) fn expr_result_borrows_existing_rc(expr: &Spanned<Expr>) -> bool {
    match &expr.node {
        Expr::Ident(_) | Expr::Index { .. } | Expr::FieldAccess { .. } => true,
        Expr::Match { subject, arms } => {
            !expr_produces_owned_rc_temp(subject)
                && !arms.is_empty()
                && arms.iter().any(match_arm_yields_subject_binding)
                && arms.iter().all(|arm| {
                    match_arm_yields_subject_binding(arm)
                        || matches!(arm.body.node, Expr::StringLit(_))
                })
        }
        _ => false,
    }
}

pub(crate) fn expr_produces_owned_rc_temp(expr: &Spanned<Expr>) -> bool {
    match &expr.node {
        Expr::Call { .. }
        | Expr::BinaryOp { .. }
        | Expr::Interpolation(_)
        | Expr::Block { .. }
        | Expr::ArrayLit(_)
        | Expr::StructLit { .. }
        | Expr::EnumVariant { .. }
        | Expr::OkExpr(_)
        | Expr::ErrExpr(_)
        | Expr::SomeExpr(_)
        | Expr::NoneExpr
        | Expr::OptionalChain { .. } => true,
        Expr::If {
            then_branch,
            else_branch: Some(else_branch),
            ..
        } => expr_produces_owned_rc_temp(then_branch) && expr_produces_owned_rc_temp(else_branch),
        Expr::IfLet {
            then_branch,
            else_branch: Some(else_branch),
            ..
        } => expr_produces_owned_rc_temp(then_branch) && expr_produces_owned_rc_temp(else_branch),
        Expr::Match { subject, arms } => {
            !arms.is_empty()
                && (arms
                    .iter()
                    .all(|arm| expr_produces_owned_rc_temp(&arm.body))
                    || (expr_produces_owned_rc_temp(subject)
                        && arms.iter().all(match_arm_yields_owned_or_static_rc)))
        }
        _ => false,
    }
}

pub(crate) fn release_expr_temp_if_needed<M: Module>(
    cx: &mut Ctx<'_, M>,
    value: Value,
    ty: &TurboTy,
    expr: &Spanned<Expr>,
) {
    if is_rc_managed_type(cx, ty) && expr_produces_owned_rc_temp(expr) {
        release_if_needed(cx, value, ty);
    }
}

pub(crate) fn has_nested_rc_children_with_layouts(
    ty: &TurboTy,
    struct_fields: &HashMap<String, Vec<(String, TurboTy)>>,
    enum_variant_fields: &HashMap<(String, String), Vec<TurboTy>>,
    enum_max_slots: &HashMap<String, usize>,
) -> bool {
    match ty {
        TurboTy::Array(inner) => is_rc_managed_type_with_layouts(inner, enum_max_slots),
        TurboTy::Struct(name) => struct_fields.get(name).is_some_and(|layout| {
            layout
                .iter()
                .any(|(_, field_ty)| is_rc_managed_type_with_layouts(field_ty, enum_max_slots))
        }),
        TurboTy::Enum(name) => enum_variant_fields
            .iter()
            .filter(|((enum_name, _), _)| enum_name == name)
            .any(|(_, field_tys)| {
                field_tys
                    .iter()
                    .any(|field_ty| is_rc_managed_type_with_layouts(field_ty, enum_max_slots))
            }),
        TurboTy::Optional(inner) => is_rc_managed_type_with_layouts(inner, enum_max_slots),
        TurboTy::Result(ok_tty, err_tty) => {
            is_rc_managed_type_with_layouts(ok_tty, enum_max_slots)
                || is_rc_managed_type_with_layouts(err_tty, enum_max_slots)
        }
        _ => false,
    }
}

fn has_nested_rc_children<M: Module>(cx: &Ctx<'_, M>, ty: &TurboTy) -> bool {
    has_nested_rc_children_with_layouts(
        ty,
        cx.struct_fields,
        cx.enum_variant_fields,
        cx.enum_max_slots,
    )
}

pub(crate) fn hashmap_value_needs_custom_release_with_layouts(
    ty: &TurboTy,
    struct_fields: &HashMap<String, Vec<(String, TurboTy)>>,
    enum_variant_fields: &HashMap<(String, String), Vec<TurboTy>>,
    enum_max_slots: &HashMap<String, usize>,
) -> bool {
    matches!(ty, TurboTy::HashMap(_, _))
        || has_nested_rc_children_with_layouts(
            ty,
            struct_fields,
            enum_variant_fields,
            enum_max_slots,
        )
}

pub(crate) fn hashmap_value_needs_custom_release<M: Module>(cx: &Ctx<'_, M>, ty: &TurboTy) -> bool {
    hashmap_value_needs_custom_release_with_layouts(
        ty,
        cx.struct_fields,
        cx.enum_variant_fields,
        cx.enum_max_slots,
    )
}

pub(crate) fn hashmap_value_release_thunk_key(ty: &TurboTy) -> String {
    format!("{ty:?}")
}

fn release_nested_children_if_needed<M: Module>(cx: &mut Ctx<'_, M>, value: Value, ty: &TurboTy) {
    if !has_nested_rc_children(cx, ty) {
        return;
    }

    let refcount_ptr = cx.builder.ins().iadd_imm(value, -8);
    let refcount = cx
        .builder
        .ins()
        .atomic_load(types::I64, MemFlags::new(), refcount_ptr);
    let one = cx.builder.ins().iconst(types::I64, 1);
    let is_last_ref = cx.builder.ins().icmp(IntCC::Equal, refcount, one);
    let release_children_block = cx.builder.create_block();
    let done_block = cx.builder.create_block();
    cx.builder
        .ins()
        .brif(is_last_ref, release_children_block, &[], done_block, &[]);

    cx.builder.switch_to_block(release_children_block);
    cx.builder.seal_block(release_children_block);
    release_nested_children(cx, value, ty);
    cx.builder.ins().jump(done_block, &[]);

    cx.builder.switch_to_block(done_block);
    cx.builder.seal_block(done_block);
}

fn release_nested_children<M: Module>(cx: &mut Ctx<'_, M>, value: Value, ty: &TurboTy) {
    match ty {
        TurboTy::Array(inner) if is_rc_managed_type(cx, inner) => {
            let len = cx.builder.ins().load(types::I64, MemFlags::new(), value, 0);
            let idx_var = cx.fresh_var(types::I64, TurboTy::Int);
            let zero = cx.builder.ins().iconst(types::I64, 0);
            cx.builder.def_var(idx_var, zero);

            let header_block = cx.builder.create_block();
            let body_block = cx.builder.create_block();
            let done_block = cx.builder.create_block();

            cx.builder.ins().jump(header_block, &[]);

            cx.builder.switch_to_block(header_block);
            let idx = cx.builder.use_var(idx_var);
            let keep_going = cx.builder.ins().icmp(IntCC::SignedLessThan, idx, len);
            cx.builder
                .ins()
                .brif(keep_going, body_block, &[], done_block, &[]);

            cx.builder.switch_to_block(body_block);
            cx.builder.seal_block(body_block);
            let idx = cx.builder.use_var(idx_var);
            let data_base = cx.builder.ins().iadd_imm(value, 8);
            let byte_offset = cx.builder.ins().ishl_imm(idx, 3);
            let elem_ptr = cx.builder.ins().iadd(data_base, byte_offset);
            let elem_val = cx
                .builder
                .ins()
                .load(cx.ptr_type, MemFlags::new(), elem_ptr, 0);
            release_if_needed(cx, elem_val, inner);
            let one = cx.builder.ins().iconst(types::I64, 1);
            let next_idx = cx.builder.ins().iadd(idx, one);
            cx.builder.def_var(idx_var, next_idx);
            cx.builder.ins().jump(header_block, &[]);

            cx.builder.seal_block(header_block);
            cx.builder.switch_to_block(done_block);
            cx.builder.seal_block(done_block);
        }
        TurboTy::Struct(name) => {
            if let Some(layout) = cx.struct_fields.get(name).cloned() {
                for (index, (_field_name, field_ty)) in layout.iter().enumerate() {
                    if is_rc_managed_type(cx, field_ty) {
                        let field_val = cx.builder.ins().load(
                            cx.ptr_type,
                            MemFlags::new(),
                            value,
                            (index * 8) as i32,
                        );
                        release_if_needed(cx, field_val, field_ty);
                    }
                }
            }
        }
        TurboTy::Enum(name) if cx.enum_max_slots.contains_key(name.as_str()) => {
            let variants = cx.enum_variants.get(name).cloned().unwrap_or_default();
            if variants.is_empty() {
                return;
            }

            let tag = cx.builder.ins().load(types::I64, MemFlags::new(), value, 0);
            let done_block = cx.builder.create_block();

            for (variant_index, variant) in variants.iter().enumerate() {
                let release_variant_block = cx.builder.create_block();
                let next_variant_block = cx.builder.create_block();
                let expected_tag = cx.builder.ins().iconst(types::I64, variant_index as i64);
                let is_variant = cx.builder.ins().icmp(IntCC::Equal, tag, expected_tag);
                cx.builder.ins().brif(
                    is_variant,
                    release_variant_block,
                    &[],
                    next_variant_block,
                    &[],
                );

                cx.builder.switch_to_block(release_variant_block);
                cx.builder.seal_block(release_variant_block);
                let field_tys = cx
                    .enum_variant_fields
                    .get(&(name.clone(), variant.clone()))
                    .cloned()
                    .unwrap_or_default();
                for (field_index, field_ty) in field_tys.iter().enumerate() {
                    if is_rc_managed_type(cx, field_ty) {
                        let field_val = cx.builder.ins().load(
                            cx.ptr_type,
                            MemFlags::new(),
                            value,
                            ((field_index + 1) * 8) as i32,
                        );
                        release_if_needed(cx, field_val, field_ty);
                    }
                }
                cx.builder.ins().jump(done_block, &[]);

                cx.builder.switch_to_block(next_variant_block);
                cx.builder.seal_block(next_variant_block);
            }

            cx.builder.ins().jump(done_block, &[]);
            cx.builder.switch_to_block(done_block);
            cx.builder.seal_block(done_block);
        }
        TurboTy::Optional(inner) if is_rc_managed_type(cx, inner) => {
            let tag_fid = cx.rt_fns["rt_option_tag"];
            let tag_fref = cx.module.declare_func_in_func(tag_fid, cx.builder.func);
            let tag_call = cx.builder.ins().call(tag_fref, &[value]);
            let tag = cx.builder.inst_results(tag_call)[0];
            let one = cx.builder.ins().iconst(types::I64, 1);
            let is_some = cx.builder.ins().icmp(IntCC::Equal, tag, one);
            let some_block = cx.builder.create_block();
            let done_block = cx.builder.create_block();
            cx.builder
                .ins()
                .brif(is_some, some_block, &[], done_block, &[]);
            cx.builder.switch_to_block(some_block);
            cx.builder.seal_block(some_block);
            let val_fid = cx.rt_fns["rt_option_value"];
            let val_fref = cx.module.declare_func_in_func(val_fid, cx.builder.func);
            let val_call = cx.builder.ins().call(val_fref, &[value]);
            let inner_val = cx.builder.inst_results(val_call)[0];
            release_if_needed(cx, inner_val, inner);
            cx.builder.ins().jump(done_block, &[]);
            cx.builder.switch_to_block(done_block);
            cx.builder.seal_block(done_block);
        }
        TurboTy::Result(ok_tty, err_tty)
            if is_rc_managed_type(cx, ok_tty) || is_rc_managed_type(cx, err_tty) =>
        {
            let tag_fid = cx.rt_fns["rt_result_tag"];
            let tag_fref = cx.module.declare_func_in_func(tag_fid, cx.builder.func);
            let tag_call = cx.builder.ins().call(tag_fref, &[value]);
            let tag = cx.builder.inst_results(tag_call)[0];
            let zero = cx.builder.ins().iconst(types::I64, 0);
            let is_ok = cx.builder.ins().icmp(IntCC::Equal, tag, zero);
            let ok_block = cx.builder.create_block();
            let err_block = cx.builder.create_block();
            let done_block = cx.builder.create_block();
            cx.builder.ins().brif(is_ok, ok_block, &[], err_block, &[]);

            cx.builder.switch_to_block(ok_block);
            cx.builder.seal_block(ok_block);
            if is_rc_managed_type(cx, ok_tty) {
                let val_fid = cx.rt_fns["rt_result_value"];
                let val_fref = cx.module.declare_func_in_func(val_fid, cx.builder.func);
                let val_call = cx.builder.ins().call(val_fref, &[value]);
                let inner_val = cx.builder.inst_results(val_call)[0];
                release_if_needed(cx, inner_val, ok_tty);
            }
            cx.builder.ins().jump(done_block, &[]);

            cx.builder.switch_to_block(err_block);
            cx.builder.seal_block(err_block);
            if is_rc_managed_type(cx, err_tty) {
                let val_fid = cx.rt_fns["rt_result_value"];
                let val_fref = cx.module.declare_func_in_func(val_fid, cx.builder.func);
                let val_call = cx.builder.ins().call(val_fref, &[value]);
                let inner_val = cx.builder.inst_results(val_call)[0];
                release_if_needed(cx, inner_val, err_tty);
            }
            cx.builder.ins().jump(done_block, &[]);

            cx.builder.switch_to_block(done_block);
            cx.builder.seal_block(done_block);
        }
        _ => {}
    }
}

pub(crate) fn release_if_needed<M: Module>(cx: &mut Ctx<'_, M>, value: Value, ty: &TurboTy) {
    if matches!(ty, TurboTy::HashMap(_, _)) {
        let release_fid = cx.rt_fns["rt_hashmap_grelease"];
        let release_ref = cx.module.declare_func_in_func(release_fid, cx.builder.func);
        cx.builder.ins().call(release_ref, &[value]);
        return;
    }
    if !is_rc_managed_type(cx, ty) {
        return;
    }
    release_nested_children_if_needed(cx, value, ty);
    let release_fid = cx.rt_fns["rt_release"];
    let release_ref = cx.module.declare_func_in_func(release_fid, cx.builder.func);
    cx.builder.ins().call(release_ref, &[value]);
}

// ── Binary operations ───────────────────────────────────────────────

fn try_iconst_value<M: Module>(cx: &Ctx<'_, M>, val: Value) -> Option<i64> {
    use cranelift::codegen::ir::InstructionData;
    let dfg = &cx.builder.func.dfg;
    let inst = dfg.value_def(val).inst()?;
    if let InstructionData::UnaryImm { imm, .. } = dfg.insts[inst] {
        Some(i64::from(imm))
    } else {
        None
    }
}

/// Pick the common tag for two mismatched integer operands in arithmetic. The
/// only mix sema allows is an untyped int literal (`TurboTy::Int`) against a
/// sized narrow operand (`i8`/`i16`/`u8`/`u16`); the literal coerces into the
/// sized type, so the narrow tag wins. Anything else returns `None` (no
/// unification — sema rejects e.g. `i8` + `i16` before codegen).
fn unify_int_tty(a: &TurboTy, b: &TurboTy) -> Option<TurboTy> {
    let is_narrow =
        |t: &TurboTy| matches!(t, TurboTy::I8 | TurboTy::I16 | TurboTy::U8 | TurboTy::U16);
    match (a, b) {
        (TurboTy::Int, n) | (n, TurboTy::Int) if is_narrow(n) => Some(n.clone()),
        _ => None,
    }
}

fn compile_binop<M: Module>(
    cx: &mut Ctx<'_, M>,
    lhs: Value,
    op: BinOp,
    rhs: Value,
) -> Result<Value, CodegenError> {
    let lhs_ty = cx.builder.func.dfg.value_type(lhs);

    if lhs_ty.is_float() {
        let result = match op {
            BinOp::Add => cx.builder.ins().fadd(lhs, rhs),
            BinOp::Sub => cx.builder.ins().fsub(lhs, rhs),
            BinOp::Mul => cx.builder.ins().fmul(lhs, rhs),
            BinOp::Div => cx.builder.ins().fdiv(lhs, rhs),
            BinOp::Eq => cx.builder.ins().fcmp(FloatCC::Equal, lhs, rhs),
            BinOp::NotEq => cx.builder.ins().fcmp(FloatCC::NotEqual, lhs, rhs),
            BinOp::Less => cx.builder.ins().fcmp(FloatCC::LessThan, lhs, rhs),
            BinOp::LessEq => cx.builder.ins().fcmp(FloatCC::LessThanOrEqual, lhs, rhs),
            BinOp::Greater => cx.builder.ins().fcmp(FloatCC::GreaterThan, lhs, rhs),
            BinOp::GreaterEq => cx.builder.ins().fcmp(FloatCC::GreaterThanOrEqual, lhs, rhs),
            _ => {
                return Err(CodegenError {
                    code: ErrorCode::E0403,
                    message: format!("unsupported float op: {op:?}"),
                })
            }
        };
        Ok(result)
    } else {
        // Widen mismatched integer widths
        let rhs_ty = cx.builder.func.dfg.value_type(rhs);
        let (lhs, rhs) = if lhs_ty.bits() != rhs_ty.bits() {
            let target = if lhs_ty.bits() > rhs_ty.bits() {
                lhs_ty
            } else {
                rhs_ty
            };
            let lhs = if lhs_ty.bits() < target.bits() {
                cx.builder.ins().sextend(target, lhs)
            } else {
                lhs
            };
            let rhs = if rhs_ty.bits() < target.bits() {
                cx.builder.ins().sextend(target, rhs)
            } else {
                rhs
            };
            (lhs, rhs)
        } else {
            (lhs, rhs)
        };

        let result = match op {
            BinOp::Add => cx.builder.ins().iadd(lhs, rhs),
            BinOp::Sub => cx.builder.ins().isub(lhs, rhs),
            BinOp::Mul => cx.builder.ins().imul(lhs, rhs),
            BinOp::Div | BinOp::Mod => {
                // Strength-reduce division/modulo by power-of-2 constants
                if let Some(imm) = try_iconst_value(cx, rhs) {
                    if imm > 0 && (imm & (imm - 1)) == 0 {
                        let shift = 63 - (imm as u64).leading_zeros() as i64;
                        if op == BinOp::Div {
                            // Signed division by power of 2:
                            // (x + (x >> 63 & (divisor-1))) >> shift
                            let sign_bits = cx.builder.ins().sshr_imm(lhs, 63);
                            let bias = cx.builder.ins().band_imm(sign_bits, imm - 1);
                            let biased = cx.builder.ins().iadd(lhs, bias);
                            cx.builder.ins().sshr_imm(biased, shift)
                        } else {
                            // Signed modulo by power of 2:
                            // x - ((x + (x >> 63 & (d-1))) >> shift) * d
                            // For mod 2 specifically: x & 1 doesn't work for negative
                            // Use: x - (x / d) * d  with the optimized div above
                            let sign_bits = cx.builder.ins().sshr_imm(lhs, 63);
                            let bias = cx.builder.ins().band_imm(sign_bits, imm - 1);
                            let biased = cx.builder.ins().iadd(lhs, bias);
                            let quot = cx.builder.ins().sshr_imm(biased, shift);
                            let prod = cx.builder.ins().imul_imm(quot, imm);
                            cx.builder.ins().isub(lhs, prod)
                        }
                    } else {
                        emit_div_zero_check(cx, rhs);
                        emit_int_overflow_check(cx, lhs, rhs);
                        if op == BinOp::Div {
                            cx.builder.ins().sdiv(lhs, rhs)
                        } else {
                            cx.builder.ins().srem(lhs, rhs)
                        }
                    }
                } else {
                    emit_div_zero_check(cx, rhs);
                    emit_int_overflow_check(cx, lhs, rhs);
                    if op == BinOp::Div {
                        cx.builder.ins().sdiv(lhs, rhs)
                    } else {
                        cx.builder.ins().srem(lhs, rhs)
                    }
                }
            }
            BinOp::Eq => cx.builder.ins().icmp(IntCC::Equal, lhs, rhs),
            BinOp::NotEq => cx.builder.ins().icmp(IntCC::NotEqual, lhs, rhs),
            BinOp::Less => cx.builder.ins().icmp(IntCC::SignedLessThan, lhs, rhs),
            BinOp::LessEq => cx
                .builder
                .ins()
                .icmp(IntCC::SignedLessThanOrEqual, lhs, rhs),
            BinOp::Greater => cx.builder.ins().icmp(IntCC::SignedGreaterThan, lhs, rhs),
            BinOp::GreaterEq => cx
                .builder
                .ins()
                .icmp(IntCC::SignedGreaterThanOrEqual, lhs, rhs),
            BinOp::And => cx.builder.ins().band(lhs, rhs),
            BinOp::Or => cx.builder.ins().bor(lhs, rhs),
        };
        Ok(result)
    }
}

fn emit_div_zero_check<M: Module>(cx: &mut Ctx<'_, M>, divisor: Value) {
    let ty = cx.builder.func.dfg.value_type(divisor);
    let zero = cx.builder.ins().iconst(ty, 0);
    let is_zero = cx.builder.ins().icmp(IntCC::Equal, divisor, zero);

    let trap_block = cx.builder.create_block();
    let ok_block = cx.builder.create_block();

    cx.builder
        .ins()
        .brif(is_zero, trap_block, &[], ok_block, &[]);

    cx.builder.switch_to_block(trap_block);
    cx.builder.seal_block(trap_block);
    cx.rt_call("rt_div_by_zero", &[]);
    cx.builder.ins().trap(TrapCode::unwrap_user(1));

    cx.builder.switch_to_block(ok_block);
    cx.builder.seal_block(ok_block);
}

fn emit_int_overflow_check<M: Module>(cx: &mut Ctx<'_, M>, dividend: Value, divisor: Value) {
    let ty = cx.builder.func.dfg.value_type(dividend);

    // Check if divisor is -1
    let neg_one = cx.builder.ins().iconst(ty, -1i64);
    let is_neg_one = cx.builder.ins().icmp(IntCC::Equal, divisor, neg_one);

    // Check if dividend is INT_MIN (0x8000...0)
    let int_min = cx.builder.ins().iconst(ty, i64::MIN);
    let is_int_min = cx.builder.ins().icmp(IntCC::Equal, dividend, int_min);

    // Both conditions must be true for overflow
    let is_overflow = cx.builder.ins().band(is_neg_one, is_int_min);

    let trap_block = cx.builder.create_block();
    let ok_block = cx.builder.create_block();

    cx.builder
        .ins()
        .brif(is_overflow, trap_block, &[], ok_block, &[]);

    cx.builder.switch_to_block(trap_block);
    cx.builder.seal_block(trap_block);
    cx.rt_call("rt_int_overflow", &[]);
    cx.builder.ins().trap(TrapCode::unwrap_user(1));

    cx.builder.switch_to_block(ok_block);
    cx.builder.seal_block(ok_block);
}

// ── Short-circuit && / || ───────────────────────────────────────────

fn compile_short_circuit<M: Module>(
    cx: &mut Ctx<'_, M>,
    left: &Spanned<Expr>,
    op: BinOp,
    right: &Spanned<Expr>,
) -> Result<MaybeTyped, CodegenError> {
    let (lhs, _) = compile_expr(cx, left)?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_short_circuit: `left` produced no value during code generation"
            .to_string(),
    })?;
    let lhs_bool = cx.to_bool(lhs);

    let eval_rhs_block = cx.builder.create_block();
    let merge_block = cx.builder.create_block();
    cx.builder.append_block_param(merge_block, types::I8);

    match op {
        BinOp::And => {
            let false_val = cx.builder.ins().iconst(types::I8, 0);
            cx.builder
                .ins()
                .brif(lhs_bool, eval_rhs_block, &[], merge_block, &[false_val]);
        }
        BinOp::Or => {
            let true_val = cx.builder.ins().iconst(types::I8, 1);
            cx.builder
                .ins()
                .brif(lhs_bool, merge_block, &[true_val], eval_rhs_block, &[]);
        }
        _ => unreachable!(),
    }

    cx.builder.switch_to_block(eval_rhs_block);
    cx.builder.seal_block(eval_rhs_block);
    let (rhs, _) = compile_expr(cx, right)?.ok_or_else(|| CodegenError {
        code: ErrorCode::E0400,
        message: "compile_short_circuit: `right` produced no value during code generation"
            .to_string(),
    })?;

    let rhs_as_i8 = cx.to_bool(rhs);

    cx.builder.ins().jump(merge_block, &[rhs_as_i8]);

    cx.builder.switch_to_block(merge_block);
    cx.builder.seal_block(merge_block);

    let result = cx.builder.block_params(merge_block)[0];
    Ok(Some((result, TurboTy::Bool)))
}

// ── Function calls ──────────────────────────────────────────────────

struct OwnedCallArgTemp {
    value: Value,
    tty: TurboTy,
}

type CompiledCallArgs = (Vec<Value>, Vec<TurboTy>, Vec<OwnedCallArgTemp>);

fn remember_owned_call_arg_temp<M: Module>(
    cx: &Ctx<'_, M>,
    owned_arg_temps: &mut Vec<OwnedCallArgTemp>,
    value: Value,
    tty: &TurboTy,
    arg: &Spanned<Expr>,
) {
    if is_rc_managed_type(cx, tty) && expr_produces_owned_rc_temp(arg) {
        owned_arg_temps.push(OwnedCallArgTemp {
            value,
            tty: tty.clone(),
        });
    }
}

fn retain_borrowed_call_arg_if_needed<M: Module>(
    cx: &mut Ctx<'_, M>,
    value: Value,
    tty: &TurboTy,
    arg: &Spanned<Expr>,
) {
    if matches!(
        &arg.node,
        Expr::Ident(_) | Expr::Index { .. } | Expr::FieldAccess { .. }
    ) && is_rc_managed_type(cx, tty)
    {
        retain_if_needed(cx, value, tty);
    }
}

fn retain_owned_mut_call_arg_if_needed<M: Module>(
    cx: &mut Ctx<'_, M>,
    value: Value,
    tty: &TurboTy,
    arg: &Spanned<Expr>,
    is_mut_param: bool,
) {
    if is_mut_param && is_rc_managed_type(cx, tty) && expr_produces_owned_rc_temp(arg) {
        retain_if_needed(cx, value, tty);
    }
}

fn release_owned_call_arg_temps<M: Module>(
    cx: &mut Ctx<'_, M>,
    owned_arg_temps: &[OwnedCallArgTemp],
) {
    for temp in owned_arg_temps {
        release_if_needed(cx, temp.value, &temp.tty);
    }
}

pub(crate) fn release_mutable_param_vars<M: Module>(cx: &mut Ctx<'_, M>) {
    let params = cx.mutable_param_vars.clone();
    for (var, tty) in params {
        let value = cx.builder.use_var(var);
        release_if_needed(cx, value, &tty);
    }
}

pub(crate) fn mark_generic_value_origin<M: Module>(
    cx: &mut Ctx<'_, M>,
    value: Value,
    type_param: String,
) {
    cx.generic_value_origins.insert(value, type_param);
    cx.generic_value_retain_flags.remove(&value);
}

pub(crate) fn mark_generic_value_origin_with_retain_flag<M: Module>(
    cx: &mut Ctx<'_, M>,
    value: Value,
    type_param: String,
    retain_flag: Value,
) {
    cx.generic_value_origins.insert(value, type_param);
    cx.generic_value_retain_flags.insert(value, retain_flag);
}

pub(crate) fn generic_origin_for_value<M: Module>(cx: &Ctx<'_, M>, value: Value) -> Option<String> {
    cx.generic_value_origins.get(&value).cloned()
}

pub(crate) fn generic_return_retain_flag_for_value<M: Module>(
    cx: &mut Ctx<'_, M>,
    value: Value,
) -> Value {
    if let Some(flag) = cx.generic_value_retain_flags.get(&value).copied() {
        flag
    } else if generic_origin_for_value(cx, value).is_some() {
        cx.builder.ins().iconst(types::I8, 1)
    } else {
        cx.builder.ins().iconst(types::I8, 0)
    }
}

pub(crate) fn retain_generic_return_if_needed<M: Module>(cx: &mut Ctx<'_, M>, value: Value) {
    let Some(return_type_param) = cx.return_type_param.clone() else {
        return;
    };
    if generic_origin_for_value(cx, value).as_deref() != Some(return_type_param.as_str()) {
        return;
    }
    let Some(is_rc_flag) = cx.generic_rc_flags.get(return_type_param.as_str()).copied() else {
        return;
    };

    let is_rc = cx.builder.ins().icmp_imm(IntCC::NotEqual, is_rc_flag, 0);
    let retain_condition =
        if let Some(needs_retain_flag) = cx.generic_value_retain_flags.get(&value).copied() {
            let needs_retain = cx
                .builder
                .ins()
                .icmp_imm(IntCC::NotEqual, needs_retain_flag, 0);
            cx.builder.ins().band(is_rc, needs_retain)
        } else {
            is_rc
        };
    let retain_block = cx.builder.create_block();
    let done_block = cx.builder.create_block();
    cx.builder
        .ins()
        .brif(retain_condition, retain_block, &[], done_block, &[]);

    cx.builder.switch_to_block(retain_block);
    cx.builder.seal_block(retain_block);
    let retain_fid = cx.rt_fns["rt_retain"];
    let retain_ref = cx.module.declare_func_in_func(retain_fid, cx.builder.func);
    cx.builder.ins().call(retain_ref, &[value]);
    cx.builder.ins().jump(done_block, &[]);

    cx.builder.switch_to_block(done_block);
    cx.builder.seal_block(done_block);
}

fn compile_call<M: Module>(
    cx: &mut Ctx<'_, M>,
    callee: &Spanned<Expr>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    // Handle method calls: expr.method(args)
    if let Expr::FieldAccess {
        ref object,
        ref field,
    } = callee.node
    {
        let (obj_val, obj_tty) = compile_expr(cx, object)?.ok_or_else(|| CodegenError {
            code: ErrorCode::E0400,
            message: "compile_call: `object` produced no value during code generation".to_string(),
        })?;
        if let TurboTy::Struct(ref type_name) = obj_tty {
            let mangled = format!("{}__{}", type_name, field);
            if let Some(&fid) = cx.user_fns.get(&mangled) {
                let param_mutable: Vec<bool> = cx
                    .fn_asts
                    .get(mangled.as_str())
                    .map(|f_def| f_def.params.iter().map(|param| param.mutable).collect())
                    .unwrap_or_else(|| vec![false; args.len() + 1]);
                let mut owned_arg_temps = Vec::new();
                retain_borrowed_call_arg_if_needed(cx, obj_val, &obj_tty, object);
                retain_owned_mut_call_arg_if_needed(
                    cx,
                    obj_val,
                    &obj_tty,
                    object,
                    param_mutable.first().copied().unwrap_or(false),
                );
                remember_owned_call_arg_temp(cx, &mut owned_arg_temps, obj_val, &obj_tty, object);
                let mut arg_vals = vec![obj_val];
                for (arg_index, arg) in args.iter().enumerate() {
                    if let Some((v, tty)) = compile_expr(cx, arg)? {
                        retain_borrowed_call_arg_if_needed(cx, v, &tty, arg);
                        retain_owned_mut_call_arg_if_needed(
                            cx,
                            v,
                            &tty,
                            arg,
                            param_mutable.get(arg_index + 1).copied().unwrap_or(false),
                        );
                        remember_owned_call_arg_temp(cx, &mut owned_arg_temps, v, &tty, arg);
                        arg_vals.push(v);
                    }
                }
                let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
                let call = cx.builder.ins().call(fref, &arg_vals);
                let results = cx.builder.inst_results(call).to_vec();
                let ret_tty = cx
                    .fn_ret_types
                    .get(&mangled)
                    .cloned()
                    .unwrap_or(TurboTy::Unit);
                if results.is_empty() {
                    release_owned_call_arg_temps(cx, &owned_arg_temps);
                    return Ok(None);
                } else {
                    release_owned_call_arg_temps(cx, &owned_arg_temps);
                    return Ok(Some((results[0], ret_tty)));
                }
            }
            // No method `field`: fall back to invoking a function value held in
            // a struct field, e.g. `(obj.handler)(x)`. Sema guarantees the
            // field type here is `Fn` (otherwise it reports E0530).
            if let Some((field_ptr, TurboTy::Fn(param_tys, ret_ty))) =
                load_struct_field(cx, obj_val, type_name, field)?
            {
                return compile_indirect_call_from_value(cx, field_ptr, &param_tys, &ret_ty, args);
            }
        }
        return Err(CodegenError {
            code: ErrorCode::E0400,
            message: format!("no method `{field}` found"),
        });
    }

    let Expr::Ident(name) = &callee.node else {
        // Indirect call through an arbitrary expression callee, e.g.
        // `make_adder(3)(4)` (Call callee) or `handlers[i](x)` (Index callee).
        // The callee must evaluate to a first-class function value. Sema has
        // already verified the type is `Fn`; guard here defensively.
        let (callee_val, callee_tty) = compile_expr(cx, callee)?.ok_or_else(|| CodegenError {
            code: ErrorCode::E0530,
            message: "callee expression produced no value".to_string(),
        })?;
        if let TurboTy::Fn(param_tys, ret_ty) = callee_tty {
            return compile_indirect_call_from_value(cx, callee_val, &param_tys, &ret_ty, args);
        }
        return Err(CodegenError {
            code: ErrorCode::E0530,
            message: format!("cannot call a value of type `{callee_tty:?}` — it is not a function"),
        });
    };

    // An explicit `extern "C"` declaration takes precedence over a same-named
    // native builtin: route the call through the FFI function declaration so
    // its declared `f64`/`f32` return is read from the FP register and tagged
    // correctly, rather than falling into the int-returning math builtin (e.g.
    // `floor`/`ceil`). Mirrors the same precedence in turbo-sema's check_call.
    if cx.extern_fns.contains(name.as_str()) {
        return compile_plain_fn_call(cx, name, args);
    }

    match name.as_str() {
        "print" => compile_print(cx, args),
        "panic" => compile_panic(cx, args),
        "assert" => compile_assert(cx, args),
        "assert_eq" => compile_assert_eq(cx, args, false),
        "assert_ne" => compile_assert_eq(cx, args, true),
        "len" => compile_len(cx, args),
        "push" => compile_builtin_push(cx, args),
        "abs" => compile_abs(cx, args),
        "min" => compile_min(cx, args),
        "max" => compile_max(cx, args),
        "float_to_int" => compile_builtin_float_to_int(cx, args),
        "int_to_float" => compile_builtin_int_to_float(cx, args),
        "str_from_char" => compile_builtin_str_from_char(cx, args),
        "to_str" => compile_to_str_builtin(cx, args),
        // Stdlib string builtins
        "split" => compile_stdlib_split(cx, args),
        "trim" => compile_stdlib_str1(cx, args, "rt_str_trim"),
        "upper" => compile_stdlib_str1(cx, args, "rt_str_upper"),
        "lower" => compile_stdlib_str1(cx, args, "rt_str_lower"),
        "starts_with" => compile_stdlib_str_bool2(cx, args, "rt_str_starts_with"),
        "ends_with" => compile_stdlib_str_bool2(cx, args, "rt_str_ends_with"),
        "replace" => compile_stdlib_replace(cx, args),
        "char_at" => compile_stdlib_char_at(cx, args),
        "contains" => compile_stdlib_str_bool2(cx, args, "rt_str_contains"),
        "index_of" => compile_stdlib_index_of(cx, args),
        "join" => compile_stdlib_join(cx, args),
        "repeat" => compile_stdlib_repeat(cx, args),
        // Stdlib I/O builtins
        "read_line" => compile_stdlib_read_line(cx),
        "read_file" => compile_stdlib_read_file(cx, args),
        "write_file" => compile_stdlib_write_file(cx, args),
        "try_read_file" => compile_stdlib_try_read_file(cx, args),
        "try_write_file" => compile_stdlib_try_write_file(cx, args),
        "shell_exec" | "exec" => compile_stdlib_exec(cx, args),
        "env_get" => compile_stdlib_env_get(cx, args),
        // Stdlib math builtins
        "pow" => compile_stdlib_pow(cx, args),
        "sqrt" => compile_stdlib_sqrt(cx, args),
        "floor" => compile_math_f64_to_i64(cx, args, "rt_floor"),
        "ceil" => compile_math_f64_to_i64(cx, args, "rt_ceil"),
        "round" => compile_math_f64_to_i64(cx, args, "rt_round"),
        "sin" => compile_math_f64_to_f64(cx, args, "rt_sin"),
        "cos" => compile_math_f64_to_f64(cx, args, "rt_cos"),
        "tan" => compile_math_f64_to_f64(cx, args, "rt_tan"),
        "log" => compile_math_f64_to_f64(cx, args, "rt_log_builtin"),
        "log2" => compile_math_f64_to_f64(cx, args, "rt_log2_builtin"),
        "log10" => compile_math_f64_to_f64(cx, args, "rt_log10"),
        "exp" => compile_math_f64_to_f64(cx, args, "rt_exp"),
        "random" => compile_random(cx),
        "random_range" => compile_random_range(cx, args),
        // System builtins
        "exit" => compile_exit(cx, args),
        "args" => compile_args(cx),
        "type_of" => compile_type_of(cx, args),
        // String parsing builtins
        "substring" => compile_substring(cx, args),
        "pad_left" => compile_pad_left(cx, args),
        "pad_right" => compile_pad_right(cx, args),
        "str_to_int" => compile_str_to_int(cx, args),
        "str_to_float" => compile_str_to_float(cx, args),
        // Async builtins
        "sleep" => compile_builtin_sleep(cx, args),
        "map" => compile_builtin_map(cx, args),
        "filter" => compile_builtin_filter(cx, args),
        "reduce" => compile_builtin_reduce(cx, args),
        // HTTP + JSON builtins
        "http_get" => compile_builtin_http_get(cx, args),
        "http_post" => compile_builtin_http_post(cx, args),
        "http_post_with_headers" => compile_builtin_http_post_with_headers(cx, args),
        "json_get" => compile_builtin_json_get(cx, args),
        "json_stringify" => compile_builtin_json_stringify(cx, args),
        "json_build" => compile_builtin_json_build(cx, args),
        // HTTP server builtins
        "http_server" => compile_builtin_http_server(cx, args),
        "http_server_public" => compile_builtin_http_server_public(cx, args),
        "route" => compile_builtin_route(cx, args),
        "http_listen" => compile_builtin_http_listen(cx, args),
        "http_config" => compile_builtin_http_config(cx, args),
        "respond" | "respond_text" => compile_builtin_respond_text(cx, args),
        "respond_html" => compile_builtin_respond_html(cx, args),
        "respond_json" => compile_builtin_respond_json(cx, args),
        "request_body" => compile_builtin_request_body(cx, args),
        "request_method" => compile_builtin_request_simple(cx, args, "rt_request_method"),
        "request_path" => compile_builtin_request_simple(cx, args, "rt_request_path"),
        "request_query" => compile_builtin_request_two_arg(cx, args, "rt_request_query"),
        "request_header" => compile_builtin_request_two_arg(cx, args, "rt_request_header"),
        "to_json" => compile_builtin_to_json(cx, args),
        "to_json_array" => compile_builtin_to_json_array(cx, args),
        // Channel builtins
        "channel" => compile_builtin_channel(cx),
        "send" => compile_builtin_send(cx, args),
        "recv" => compile_builtin_recv(cx, args),
        // Mutex builtins
        "mutex" => compile_builtin_mutex(cx, args),
        "mutex_get" => compile_builtin_mutex_get(cx, args),
        "mutex_set" => compile_builtin_mutex_set(cx, args),
        "mutex_update" => compile_builtin_mutex_update(cx, args),
        // Derive builtins
        "clone" => compile_clone(cx, args),
        // HashMap builtins
        "hashmap" => compile_builtin_hashmap(cx),
        "hashmap_set" => compile_builtin_hashmap_set(cx, args),
        "hashmap_get" => compile_builtin_hashmap_get(cx, args),
        "hashmap_has" => compile_builtin_hashmap_has(cx, args),
        "hashmap_len" | "hashmap_size" => compile_builtin_hashmap_len(cx, args),
        "hashmap_keys" => compile_builtin_hashmap_keys(cx, args),
        "hashmap_remove" => compile_builtin_hashmap_remove(cx, args),
        "hashmap_set_int" => compile_builtin_hashmap_set_int(cx, args),
        "hashmap_get_int" => compile_builtin_hashmap_get_int(cx, args),
        "hashmap_inc" => compile_builtin_hashmap_inc(cx, args),
        // Filesystem builtins
        "file_exists" => compile_file_exists(cx, args),
        "delete_file" => compile_delete_file(cx, args),
        "list_dir" => compile_list_dir(cx, args),
        "mkdir" => compile_mkdir(cx, args),
        "path_join" => compile_path_join(cx, args),
        "path_dir" => compile_path_str1(cx, args, "rt_path_dir"),
        "path_base" => compile_path_str1(cx, args, "rt_path_base"),
        "path_ext" => compile_path_str1(cx, args, "rt_path_ext"),
        // Collection builtins
        "sort" => compile_sort(cx, args),
        "reverse" => compile_reverse(cx, args),
        "array_contains" => compile_array_contains(cx, args),
        "slice" => compile_slice(cx, args),
        "any" => compile_builtin_any(cx, args),
        "all" => compile_builtin_all(cx, args),
        // Date/Time builtins
        "time_now" => compile_time_now(cx),
        "time_ms" => compile_time_ms(cx),
        "format_time" => compile_format_time(cx, args),
        // SQLite builtins
        "sqlite_open" => compile_sqlite_open(cx, args),
        "sqlite_close" => compile_sqlite_close(cx, args),
        "sqlite_exec" => compile_sqlite_exec(cx, args),
        "sqlite_error" => compile_sqlite_error(cx, args),
        "sqlite_prepare" => compile_sqlite_prepare(cx, args),
        "sqlite_bind_int" => compile_sqlite_bind_int(cx, args),
        "sqlite_bind_str" => compile_sqlite_bind_str(cx, args),
        "sqlite_bind_float" => compile_sqlite_bind_float(cx, args),
        "sqlite_step" => compile_sqlite_step(cx, args),
        "sqlite_column_int" => compile_sqlite_column_int(cx, args),
        "sqlite_column_str" => compile_sqlite_column_str(cx, args),
        "sqlite_column_float" => compile_sqlite_column_float(cx, args),
        "sqlite_column_count" => compile_sqlite_column_count(cx, args),
        "sqlite_finalize" => compile_sqlite_finalize(cx, args),
        // Unsafe builtins — raw pointer operations
        "deref" => compile_builtin_deref(cx, args),
        "store" => compile_builtin_store(cx, args),
        _ => {
            // User-defined call fallback. A bare-identifier call can take one
            // of several shapes; dispatch them in order, falling through to a
            // plain user-function call when none of the specialized forms match.
            if let Some(result) = compile_enum_variant_ctor(cx, name, args)? {
                return Ok(result);
            }
            if let Some(result) = compile_ufcs_method_call(cx, name, args)? {
                return Ok(result);
            }
            if let Some(result) = compile_closure_call(cx, name, args)? {
                return Ok(result);
            }
            compile_plain_fn_call(cx, name, args)
        }
    }
}

/// Enum-variant construction dispatched through the parser's UFCS rewrite, e.g.
/// `Shape.Circle(5.0)` lowered to `Call { callee: Ident("Circle"), args:
/// [Ident("Shape"), 5.0] }`. Returns `Ok(Some(..))` when the call is an enum
/// variant constructor (already lowered), or `Ok(None)` to fall through.
fn compile_enum_variant_ctor<M: Module>(
    cx: &mut Ctx<'_, M>,
    name: &str,
    args: &[Spanned<Expr>],
) -> Result<Option<MaybeTyped>, CodegenError> {
    // Check if this is an enum variant construction via UFCS rewrite:
    // Parser transforms Shape.Circle(5.0) into Call { callee: Ident("Circle"), args: [Ident("Shape"), 5.0] }
    if !args.is_empty() {
        if let Expr::Ident(ref first_name) = args[0].node {
            if let Some(variants) = cx.enum_variants.get(first_name.as_str()) {
                if let Some(variant_index) = variants.iter().position(|v| v == name) {
                    // This is an enum variant construction with data
                    let data_args = &args[1..]; // skip the enum type name
                    let enum_name = first_name;

                    if let Some(&max_slots) = cx.enum_max_slots.get(enum_name.as_str()) {
                        // Data-carrying enum: allocate tagged union
                        let total_slots = 1 + max_slots; // tag + payload
                        let num_fields_val =
                            cx.builder.ins().iconst(types::I64, total_slots as i64);
                        let alloc_fid = cx.rt_fns["rt_struct_alloc"];
                        let alloc_fref = cx.module.declare_func_in_func(alloc_fid, cx.builder.func);
                        let call = cx.builder.ins().call(alloc_fref, &[num_fields_val]);
                        let ptr = cx.builder.inst_results(call)[0];

                        // Store tag at offset 0
                        let tag_val = cx.builder.ins().iconst(types::I64, variant_index as i64);
                        cx.builder.ins().store(MemFlags::new(), tag_val, ptr, 0);

                        // Get the field types for this variant
                        let field_tys = cx
                            .enum_variant_fields
                            .get(&(enum_name.clone(), name.to_string()))
                            .cloned()
                            .unwrap_or_default();

                        // Store each field at offset (i+1)*8
                        for (i, arg) in data_args.iter().enumerate() {
                            let (val, tty) =
                                compile_expr(cx, arg)?.ok_or_else(|| CodegenError {
                                    code: ErrorCode::E0400,
                                    message: "compile_call: `arg` produced no value during code generation"
                                        .to_string(),
                                })?;
                            let offset = ((i + 1) * 8) as i32;
                            let field_tty = field_tys.get(i).unwrap_or(&tty);
                            if matches!(
                                &arg.node,
                                Expr::Ident(_) | Expr::Index { .. } | Expr::FieldAccess { .. }
                            ) {
                                retain_if_needed(cx, val, field_tty);
                            }

                            // Widen/convert to i64 for uniform storage
                            let val_ty = cx.builder.func.dfg.value_type(val);
                            let store_val = if val_ty.is_float() && val_ty.bits() == 64 {
                                cx.builder.ins().bitcast(types::I64, MemFlags::new(), val)
                            } else if val_ty.is_float() && val_ty.bits() == 32 {
                                let extended = cx.builder.ins().fpromote(types::F64, val);
                                cx.builder
                                    .ins()
                                    .bitcast(types::I64, MemFlags::new(), extended)
                            } else if val_ty.bits() < 64 && val_ty.is_int() {
                                cx.builder.ins().sextend(types::I64, val)
                            } else {
                                val
                            };

                            cx.builder
                                .ins()
                                .store(MemFlags::new(), store_val, ptr, offset);
                        }

                        return Ok(Some(Some((ptr, TurboTy::Enum(enum_name.clone())))));
                    } else {
                        // Unit-only enum, but called with args (shouldn't happen after sema check)
                        let val = cx.builder.ins().iconst(types::I64, variant_index as i64);
                        return Ok(Some(Some((val, TurboTy::Enum(enum_name.clone())))));
                    }
                }
            }
        }
    }
    Ok(None)
}

fn static_expr_turbo_ty<M: Module>(cx: &Ctx<'_, M>, expr: &Spanned<Expr>) -> Option<TurboTy> {
    match &expr.node {
        Expr::Ident(name) => cx.vars.get(name).map(|(_, _, tty)| tty.clone()),
        Expr::StructLit { name, .. } => Some(TurboTy::Struct(name.clone())),
        Expr::If {
            then_branch,
            else_branch,
            ..
        } => {
            let then_ty = static_expr_turbo_ty(cx, then_branch)?;
            let else_ty = else_branch
                .as_ref()
                .and_then(|branch| static_expr_turbo_ty(cx, branch))?;
            (then_ty == else_ty).then_some(then_ty)
        }
        Expr::IfLet {
            then_branch,
            else_branch,
            ..
        } => {
            let then_ty = static_expr_turbo_ty(cx, then_branch)?;
            let else_ty = else_branch
                .as_ref()
                .and_then(|branch| static_expr_turbo_ty(cx, branch))?;
            (then_ty == else_ty).then_some(then_ty)
        }
        Expr::Match { arms, .. } => {
            let mut arm_tys = arms.iter().map(|arm| static_expr_turbo_ty(cx, &arm.body));
            let first_ty = arm_tys.next()??;
            arm_tys
                .all(|arm_ty| arm_ty.as_ref() == Some(&first_ty))
                .then_some(first_ty)
        }
        Expr::Call { callee, args } => match &callee.node {
            Expr::Ident(name) => {
                if let Some((_, _, TurboTy::Fn(_, ret_ty))) = cx.vars.get(name) {
                    return Some((**ret_ty).clone());
                }
                if let Some(ret_tty) = cx.fn_ret_types.get(name).cloned() {
                    let type_params = cx.fn_type_params.get(name).cloned().unwrap_or_default();
                    if type_params.is_empty() {
                        return Some(ret_tty);
                    }
                    let arg_ttys = args
                        .iter()
                        .map(|arg| static_expr_turbo_ty(cx, arg))
                        .collect::<Option<Vec<_>>>();
                    return Some(arg_ttys.map_or(ret_tty.clone(), |arg_ttys| {
                        infer_generic_ret_tty(cx, name, &type_params, ret_tty, &arg_ttys)
                    }));
                }
                if !args.is_empty() {
                    let type_name = static_struct_receiver_type(cx, &args[0])?;
                    let mangled = format!("{}__{}", type_name, name);
                    return cx.fn_ret_types.get(&mangled).cloned();
                }
                None
            }
            Expr::FieldAccess { object, field } => {
                let TurboTy::Struct(type_name) = static_expr_turbo_ty(cx, object)? else {
                    return None;
                };
                let mangled = format!("{}__{}", type_name, field);
                cx.fn_ret_types.get(&mangled).cloned()
            }
            _ => None,
        },
        Expr::FieldAccess { object, field } => {
            let TurboTy::Struct(type_name) = static_expr_turbo_ty(cx, object)? else {
                return None;
            };
            cx.struct_fields
                .get(&type_name)
                .and_then(|layout| layout.iter().find(|(name, _)| name == field))
                .map(|(_, tty)| tty.clone())
        }
        Expr::Index { object, .. } => match static_expr_turbo_ty(cx, object)? {
            TurboTy::Array(inner) => Some(*inner),
            _ => None,
        },
        Expr::Block {
            tail_expr: Some(tail),
            ..
        } => static_expr_turbo_ty(cx, tail),
        _ => None,
    }
}

fn has_possible_ufcs_target<M: Module>(cx: &Ctx<'_, M>, name: &str) -> bool {
    cx.user_fns.keys().any(|candidate| {
        candidate
            .rsplit_once("__")
            .is_some_and(|(_, method)| method == name)
    }) || cx.struct_fields.values().any(|layout| {
        layout
            .iter()
            .any(|(field, tty)| field == name && matches!(tty, TurboTy::Fn(_, _)))
    })
}

fn has_ufcs_target_for_type<M: Module>(cx: &Ctx<'_, M>, type_name: &str, name: &str) -> bool {
    let mangled = format!("{}__{}", type_name, name);
    cx.user_fns.contains_key(&mangled)
        || cx
            .struct_fields
            .get(type_name)
            .and_then(|layout| layout.iter().find(|(field, _)| field == name))
            .is_some_and(|(_, tty)| matches!(tty, TurboTy::Fn(_, _)))
}

fn static_struct_receiver_type<M: Module>(cx: &Ctx<'_, M>, expr: &Spanned<Expr>) -> Option<String> {
    match static_expr_turbo_ty(cx, expr)? {
        TurboTy::Struct(name) => Some(name),
        _ => None,
    }
}

fn compile_ufcs_with_receiver<M: Module>(
    cx: &mut Ctx<'_, M>,
    type_name: &str,
    name: &str,
    first_val: Value,
    first_tty: &TurboTy,
    receiver_arg: &Spanned<Expr>,
    args: &[Spanned<Expr>],
) -> Result<Option<MaybeTyped>, CodegenError> {
    let mangled = format!("{}__{}", type_name, name);
    if let Some(&fid) = cx.user_fns.get(&mangled) {
        let param_mutable: Vec<bool> = cx
            .fn_asts
            .get(mangled.as_str())
            .map(|f_def| f_def.params.iter().map(|param| param.mutable).collect())
            .unwrap_or_else(|| vec![false; args.len()]);
        let mut owned_arg_temps = Vec::new();
        retain_borrowed_call_arg_if_needed(cx, first_val, first_tty, receiver_arg);
        retain_owned_mut_call_arg_if_needed(
            cx,
            first_val,
            first_tty,
            receiver_arg,
            param_mutable.first().copied().unwrap_or(false),
        );
        remember_owned_call_arg_temp(cx, &mut owned_arg_temps, first_val, first_tty, receiver_arg);
        let mut arg_vals = vec![first_val];
        for (arg_index, arg) in args[1..].iter().enumerate() {
            if let Some((v, tty)) = compile_expr(cx, arg)? {
                retain_borrowed_call_arg_if_needed(cx, v, &tty, arg);
                remember_owned_call_arg_temp(cx, &mut owned_arg_temps, v, &tty, arg);
                retain_owned_mut_call_arg_if_needed(
                    cx,
                    v,
                    &tty,
                    arg,
                    param_mutable.get(arg_index + 1).copied().unwrap_or(false),
                );
                arg_vals.push(v);
            }
        }
        let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
        let call = cx.builder.ins().call(fref, &arg_vals);
        let results = cx.builder.inst_results(call).to_vec();
        let ret_tty = cx
            .fn_ret_types
            .get(&mangled)
            .cloned()
            .unwrap_or(TurboTy::Unit);
        release_owned_call_arg_temps(cx, &owned_arg_temps);
        return if results.is_empty() {
            Ok(Some(None))
        } else {
            Ok(Some(Some((results[0], ret_tty))))
        };
    }

    // No method `name`: the receiver may hold a function value in a field named
    // `name`, i.e. `obj.f(x)` where `f: fn(...) -> ...`. Methods take
    // precedence (checked above); sema applies the same rule.
    if let Some((field_ptr, TurboTy::Fn(param_tys, ret_ty))) =
        load_struct_field(cx, first_val, type_name, name)?
    {
        let result =
            compile_indirect_call_from_value(cx, field_ptr, &param_tys, &ret_ty, &args[1..])?;
        release_expr_temp_if_needed(cx, first_val, first_tty, receiver_arg);
        return Ok(Some(result));
    }

    Ok(None)
}

fn static_expr_is_unit<M: Module>(cx: &Ctx<'_, M>, expr: &Spanned<Expr>) -> bool {
    match &expr.node {
        Expr::Unit => true,
        Expr::Call { callee, .. } => match &callee.node {
            Expr::Ident(name) => {
                cx.fn_ret_types
                    .get(name)
                    .is_some_and(|tty| *tty == TurboTy::Unit)
                    || matches!(
                        name.as_str(),
                        "print" | "assert" | "assert_eq" | "assert_ne"
                    )
            }
            _ => false,
        },
        Expr::Block {
            tail_expr: Some(tail),
            ..
        } => static_expr_is_unit(cx, tail),
        _ => false,
    }
}

/// Method calls lowered through UFCS (the parser rewrites `obj.method(args)` to
/// `method(obj, args)`). When `name` is not a free function, the first argument's
/// static type selects the mangled `Type__method` target before the receiver is
/// compiled, so falling through to fn-value calls cannot abandon owned temps.
/// Returns `Ok(Some(..))` when handled, `Ok(None)` to fall through.
fn compile_ufcs_method_call<M: Module>(
    cx: &mut Ctx<'_, M>,
    name: &str,
    args: &[Spanned<Expr>],
) -> Result<Option<MaybeTyped>, CodegenError> {
    // Check if this is a method call (UFCS: parser rewrites obj.method(args) -> method(obj, args))
    if cx.user_fns.get(name).is_none() && !args.is_empty() {
        if matches!(cx.vars.get(name), Some((_, _, TurboTy::Fn(_, _)))) {
            return Ok(None);
        }
        let Some(type_name) = static_struct_receiver_type(cx, &args[0]) else {
            if static_expr_is_unit(cx, &args[0]) {
                return Err(CodegenError {
                    code: ErrorCode::E0400,
                    message: "compile_call: `&args[0]` produced no value during code generation"
                        .to_string(),
                });
            }
            if !has_possible_ufcs_target(cx, name) {
                return Ok(None);
            }
            let (first_val, first_tty) =
                compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
                    code: ErrorCode::E0400,
                    message: "compile_call: `&args[0]` produced no value during code generation"
                        .to_string(),
                })?;
            if let TurboTy::Struct(type_name) = first_tty.clone() {
                if let Some(result) = compile_ufcs_with_receiver(
                    cx, &type_name, name, first_val, &first_tty, &args[0], args,
                )? {
                    return Ok(Some(result));
                }
            }
            release_expr_temp_if_needed(cx, first_val, &first_tty, &args[0]);
            return Ok(None);
        };
        if !has_ufcs_target_for_type(cx, &type_name, name) {
            return Ok(None);
        }
        let (first_val, first_tty) = compile_expr(cx, &args[0])?.ok_or_else(|| CodegenError {
            code: ErrorCode::E0400,
            message: "compile_call: `&args[0]` produced no value during code generation"
                .to_string(),
        })?;
        if let Some(result) =
            compile_ufcs_with_receiver(cx, &type_name, name, first_val, &first_tty, &args[0], args)?
        {
            return Ok(Some(result));
        }
        release_expr_temp_if_needed(cx, first_val, &first_tty, &args[0]);
    }
    Ok(None)
}

/// Indirect call through a closure value. A closure is a `[fn_ptr, env_ptr]`
/// pair; this builds the Cranelift signature (env-first), reconciles float/int
/// register classes for inferred-param slots, emits `call_indirect`, and
/// reinterprets a float return arriving through a uniform i64 slot. Returns
/// `Ok(Some(..))` when `name` is a closure-typed variable, `Ok(None)` otherwise.
fn compile_closure_call<M: Module>(
    cx: &mut Ctx<'_, M>,
    name: &str,
    args: &[Spanned<Expr>],
) -> Result<Option<MaybeTyped>, CodegenError> {
    // Check if the callee is a variable with a function pointer type (closure)
    if let Some((var, _cl_ty, TurboTy::Fn(ref param_tys, ref ret_ty))) = cx.vars.get(name).cloned()
    {
        let param_tys = param_tys.clone();
        let ret_ty = *ret_ty.clone();
        let closure_ptr = cx.builder.use_var(var);
        let result = compile_indirect_call_from_value(cx, closure_ptr, &param_tys, &ret_ty, args)?;
        return Ok(Some(result));
    }
    Ok(None)
}

/// Emit an indirect call through a first-class function value. Every function
/// value — a closure or a named function used as a value — is a heap pair
/// `[fn_ptr, env_ptr]` whose `fn_ptr` targets an env-first `CallConv::Fast`
/// entry (closures compile that way directly; named functions are wrapped in an
/// env-first adapter). This loads the pair, builds the env-first signature,
/// reconciles float/int register classes for uniform-i64 slots, emits
/// `call_indirect`, and reinterprets a float return arriving through an i64 slot.
pub(crate) fn compile_indirect_call_from_value<M: Module>(
    cx: &mut Ctx<'_, M>,
    closure_ptr: Value,
    param_tys: &[TurboTy],
    ret_ty: &TurboTy,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    // Function value is a pair struct: [fn_ptr, env_ptr]
    let fn_ptr = cx
        .builder
        .ins()
        .load(cx.ptr_type, MemFlags::new(), closure_ptr, 0);
    let env_ptr = cx
        .builder
        .ins()
        .load(cx.ptr_type, MemFlags::new(), closure_ptr, 8);

    // Build the Cranelift signature: env_ptr first, then user params
    let mut sig = cx.module.make_signature();
    sig.call_conv = CallConv::Fast;
    sig.params.push(AbiParam::new(cx.ptr_type)); // env_ptr
    let mut param_cl_tys = Vec::with_capacity(param_tys.len());
    for param_tty in param_tys {
        let cl_ty = turbo_ty_to_cl_type(param_tty, cx.ptr_type);
        sig.params.push(AbiParam::new(cl_ty));
        param_cl_tys.push(cl_ty);
    }
    let ret_tty = ret_ty.clone();
    if ret_tty != TurboTy::Unit {
        let cl_ret = turbo_ty_to_cl_type(&ret_tty, cx.ptr_type);
        sig.returns.push(AbiParam::new(cl_ret));
    }

    let sig_ref = cx.builder.import_signature(sig);

    let mut arg_values = vec![env_ptr]; // env_ptr is first hidden arg
    let mut owned_arg_temps = Vec::new();
    for (i, arg) in args.iter().enumerate() {
        if let Some((val, tty)) = compile_expr(cx, arg)? {
            // If the value's param slot is a uniform i64 but the value is a
            // float (inferred-param closure called with a float), move the
            // bits through the integer register so both sides agree on the
            // register class.
            let val = if let Some(&expected) = param_cl_tys.get(i) {
                let actual = cx.builder.func.dfg.value_type(val);
                if actual != expected
                    && actual.bits() == expected.bits()
                    && (actual.is_float() && expected.is_int()
                        || actual.is_int() && expected.is_float())
                {
                    cx.builder.ins().bitcast(expected, MemFlags::new(), val)
                } else {
                    val
                }
            } else {
                val
            };
            remember_owned_call_arg_temp(cx, &mut owned_arg_temps, val, &tty, arg);
            arg_values.push(val);
        }
    }

    let call = cx.builder.ins().call_indirect(sig_ref, fn_ptr, &arg_values);
    let results = cx.builder.inst_results(call).to_vec();
    if results.is_empty() {
        release_owned_call_arg_temps(cx, &owned_arg_temps);
        Ok(None)
    } else {
        let mut result = results[0];
        // If the declared return is float but it came back through a uniform
        // i64 slot, reinterpret the bits as F64.
        if matches!(ret_tty, TurboTy::Float) {
            let rty = cx.builder.func.dfg.value_type(result);
            if rty.is_int() && rty.bits() == 64 {
                result = cx
                    .builder
                    .ins()
                    .bitcast(types::F64, MemFlags::new(), result);
            }
        }
        release_owned_call_arg_temps(cx, &owned_arg_temps);
        Ok(Some((result, ret_tty)))
    }
}

/// Materialize a first-class function value for a bare function name used as a
/// value (e.g. `let g = dbl`). A named function is compiled with a plain
/// `(params...) -> ret` signature, but every function value must be callable
/// through the uniform env-first closure ABI. We therefore point the value's
/// `fn_ptr` at the function's env-first adapter (`__fnval$<name>`, generated in
/// `compile.rs`), which ignores the env pointer and forwards to the real
/// function. The env pointer is null. Returns `Ok(None)` when `name` is not an
/// adaptable user function.
fn compile_named_fn_value<M: Module>(
    cx: &mut Ctx<'_, M>,
    name: &str,
) -> Result<Option<MaybeTyped>, CodegenError> {
    let adapter_name = format!("__fnval${name}");
    let Some(&adapter_fid) = cx.user_fns.get(&adapter_name) else {
        return Ok(None);
    };
    // Build the value's Fn type from the function's declared signature.
    let Some(fn_def) = cx.fn_asts.get(name).copied() else {
        return Ok(None);
    };
    let param_tys: Vec<TurboTy> = fn_def
        .params
        .iter()
        .map(|p| turbo_ty_from_type_expr(&p.ty.node, cx.enum_variants))
        .collect();
    let ret_ty = cx.fn_ret_types.get(name).cloned().unwrap_or(TurboTy::Unit);

    // Address of the env-first adapter.
    let adapter_ref = cx.module.declare_func_in_func(adapter_fid, cx.builder.func);
    let fn_ptr = cx.builder.ins().func_addr(cx.ptr_type, adapter_ref);

    // Allocate the closure pair [fn_ptr, env_ptr] with a null env.
    let two = cx.builder.ins().iconst(types::I64, 2);
    let alloc_fid = cx.rt_fns["rt_struct_alloc"];
    let alloc_fref = cx.module.declare_func_in_func(alloc_fid, cx.builder.func);
    let call = cx.builder.ins().call(alloc_fref, &[two]);
    let pair_ptr = cx.builder.inst_results(call)[0];
    let null_env = cx.builder.ins().iconst(cx.ptr_type, 0);
    cx.builder.ins().store(MemFlags::new(), fn_ptr, pair_ptr, 0);
    cx.builder
        .ins()
        .store(MemFlags::new(), null_env, pair_ptr, 8);

    Ok(Some(Some((
        pair_ptr,
        TurboTy::Fn(param_tys, Box::new(ret_ty)),
    ))))
}

/// Load a struct field's raw slot value and its declared `TurboTy`. Returns
/// `Ok(None)` if the struct has no such field. Used to fetch a function value
/// stored in a struct field for invocation; the raw i64 slot IS the pair
/// pointer for `Fn`-typed fields, so no reinterpretation is needed.
fn load_struct_field<M: Module>(
    cx: &mut Ctx<'_, M>,
    struct_ptr: Value,
    struct_name: &str,
    field: &str,
) -> Result<Option<(Value, TurboTy)>, CodegenError> {
    let Some(layout) = cx.struct_fields.get(struct_name).cloned() else {
        return Ok(None);
    };
    let Some(field_index) = layout.iter().position(|(n, _)| n == field) else {
        return Ok(None);
    };
    let field_tty = layout[field_index].1.clone();
    let offset = (field_index * 8) as i32;
    let raw = cx
        .builder
        .ins()
        .load(types::I64, MemFlags::new(), struct_ptr, offset);
    Ok(Some((raw, field_tty)))
}

/// Compile each call argument, reconciling its Cranelift value type against the
/// callee's declared parameter slot (int width adjustment, and float<->int
/// register-class bitcasts for generic uniform i64 slots), and inserting COW
/// retains for aliased struct/array idents. Also records owned RC temporaries
/// that the caller must release once the callee has returned.
fn compile_fn_call_args<M: Module>(
    cx: &mut Ctx<'_, M>,
    args: &[Spanned<Expr>],
    param_types: &[types::Type],
    param_mutable: &[bool],
) -> Result<CompiledCallArgs, CodegenError> {
    let mut arg_values = Vec::new();
    let mut arg_ttys = Vec::new();
    let mut owned_arg_temps = Vec::new();
    for (i, arg) in args.iter().enumerate() {
        if let Some((val, tty)) = compile_expr(cx, arg)? {
            let val = if i < param_types.len() {
                let expected = param_types[i];
                let actual = cx.builder.func.dfg.value_type(val);
                if actual == expected {
                    val
                } else if actual.is_int() && expected.is_int() {
                    if actual.bits() > expected.bits() {
                        cx.builder.ins().ireduce(expected, val)
                    } else {
                        cx.builder.ins().sextend(expected, val)
                    }
                } else if actual.bits() == expected.bits()
                    && (actual.is_float() && expected.is_int()
                        || actual.is_int() && expected.is_float())
                {
                    // Float type-argument flowing through a generic's
                    // uniform i64 ABI slot (e.g. `fn id<T>(x: T)` called
                    // with a float): move the bits through the integer
                    // register so both sides agree on the register class.
                    cx.builder.ins().bitcast(expected, MemFlags::new(), val)
                } else {
                    val
                }
            } else {
                val
            };
            // COW/ARC: passing a borrowed refcounted value to a function
            // aliases the caller's binding — the callee receives the
            // same pointer. Retain it so the shared allocation's
            // refcount reflects both references; a `mut`-param write
            // inside the callee then sees refcount > 1 and copies
            // instead of mutating the caller's value in place
            // (`p.x = ..` via rt_struct_cow for structs, BL-10;
            // `a[i] = ..` via rt_array_set for arrays, BL-27 Part A).
            // Fresh temporaries (non-idents) are not aliased, so they
            // are left alone to avoid needless copies.
            retain_borrowed_call_arg_if_needed(cx, val, &tty, arg);
            retain_owned_mut_call_arg_if_needed(
                cx,
                val,
                &tty,
                arg,
                param_mutable.get(i).copied().unwrap_or(false),
            );
            remember_owned_call_arg_temp(cx, &mut owned_arg_temps, val, &tty, arg);
            arg_values.push(val);
            arg_ttys.push(tty);
        }
    }
    Ok((arg_values, arg_ttys, owned_arg_temps))
}

/// Reconcile the static return `TurboTy` of a generic function with the
/// concrete types of its arguments. For `fn f<T>(x: T) -> T` (or `fn f<T>(xs:
/// [T]) -> T`), the concrete `T` is recovered from the matching argument's
/// type; otherwise the declared return type is used unchanged.
fn infer_generic_ret_tty<M: Module>(
    cx: &Ctx<'_, M>,
    name: &str,
    type_params: &[String],
    ret_tty: TurboTy,
    arg_ttys: &[TurboTy],
) -> TurboTy {
    // For generic functions, infer the actual return TurboTy from args.
    if !type_params.is_empty() {
        if let Some(f_def) = cx.fn_asts.get(name) {
            if let Some(ret_ty) = &f_def.return_type {
                if let TypeExpr::Named(ref ret_name) = ret_ty.node {
                    if type_params.contains(ret_name) {
                        // Find which param carries this type parameter and
                        // recover the concrete TurboTy from the matching
                        // argument. We handle two shapes:
                        //   fn f<T>(x: T)   -> T   — infer T from the arg
                        //   fn f<T>(xs: [T]) -> T  — infer T from the arg's
                        //                            array element type
                        let mut inferred = None;
                        for (i, param) in f_def.params.iter().enumerate() {
                            if i >= arg_ttys.len() {
                                continue;
                            }
                            match &param.ty.node {
                                TypeExpr::Named(pname) if pname == ret_name => {
                                    inferred = Some(arg_ttys[i].clone());
                                    break;
                                }
                                TypeExpr::Array(elem) if matches!(&elem.node, TypeExpr::Named(en) if en == ret_name) => {
                                    if let TurboTy::Array(inner) = &arg_ttys[i] {
                                        inferred = Some((**inner).clone());
                                        break;
                                    }
                                }
                                _ => {}
                            }
                        }
                        inferred.unwrap_or(ret_tty)
                    } else {
                        ret_tty
                    }
                } else {
                    ret_tty
                }
            } else {
                ret_tty
            }
        } else {
            ret_tty
        }
    } else {
        ret_tty
    }
}

fn infer_type_param_bindings(
    declared: &TypeExpr,
    actual: &TurboTy,
    type_params: &[String],
    bindings: &mut HashMap<String, TurboTy>,
) {
    match (declared, actual) {
        (TypeExpr::Named(name), actual) if type_params.contains(name) => {
            bindings
                .entry(name.clone())
                .or_insert_with(|| actual.clone());
        }
        (TypeExpr::Array(inner), TurboTy::Array(actual_inner))
        | (TypeExpr::Optional(inner), TurboTy::Optional(actual_inner))
        | (TypeExpr::Future(inner), TurboTy::Future(actual_inner)) => {
            infer_type_param_bindings(&inner.node, actual_inner, type_params, bindings);
        }
        (TypeExpr::Result { ok_type, err_type }, TurboTy::Result(actual_ok, actual_err)) => {
            infer_type_param_bindings(&ok_type.node, actual_ok, type_params, bindings);
            infer_type_param_bindings(&err_type.node, actual_err, type_params, bindings);
        }
        (TypeExpr::FnType { params, ret }, TurboTy::Fn(actual_params, actual_ret)) => {
            for (param, actual_param) in params.iter().zip(actual_params.iter()) {
                infer_type_param_bindings(&param.node, actual_param, type_params, bindings);
            }
            infer_type_param_bindings(&ret.node, actual_ret, type_params, bindings);
        }
        (TypeExpr::HashMap(key, value), TurboTy::HashMap(actual_key, actual_value)) => {
            infer_type_param_bindings(&key.node, actual_key, type_params, bindings);
            infer_type_param_bindings(&value.node, actual_value, type_params, bindings);
        }
        _ => {}
    }
}

fn infer_generic_type_bindings(
    f_def: &FnDef,
    type_params: &[String],
    arg_ttys: &[TurboTy],
) -> HashMap<String, TurboTy> {
    let mut bindings = HashMap::new();
    for (param, actual) in f_def.params.iter().zip(arg_ttys.iter()) {
        infer_type_param_bindings(&param.ty.node, actual, type_params, &mut bindings);
    }
    bindings
}

fn type_expr_mentions_type_param(declared: &TypeExpr, type_param: &str) -> bool {
    match declared {
        TypeExpr::Named(name) => name == type_param,
        TypeExpr::Array(inner) | TypeExpr::Optional(inner) | TypeExpr::Future(inner) => {
            type_expr_mentions_type_param(&inner.node, type_param)
        }
        TypeExpr::Result { ok_type, err_type } => {
            type_expr_mentions_type_param(&ok_type.node, type_param)
                || type_expr_mentions_type_param(&err_type.node, type_param)
        }
        TypeExpr::FnType { params, ret } => {
            params
                .iter()
                .any(|param| type_expr_mentions_type_param(&param.node, type_param))
                || type_expr_mentions_type_param(&ret.node, type_param)
        }
        TypeExpr::HashMap(key, value) => {
            type_expr_mentions_type_param(&key.node, type_param)
                || type_expr_mentions_type_param(&value.node, type_param)
        }
        _ => false,
    }
}

fn infer_generic_dynamic_rc_flags<M: Module>(
    cx: &Ctx<'_, M>,
    f_def: &FnDef,
    type_params: &[String],
    arg_values: &[Value],
) -> HashMap<String, Value> {
    let mut flags = HashMap::new();
    for type_param in type_params {
        for (param, actual_value) in f_def.params.iter().zip(arg_values.iter()) {
            if !type_expr_mentions_type_param(&param.ty.node, type_param) {
                continue;
            }
            let Some(origin) = generic_origin_for_value(cx, *actual_value) else {
                continue;
            };
            let Some(flag) = cx.generic_rc_flags.get(origin.as_str()).copied() else {
                continue;
            };
            flags.insert(type_param.clone(), flag);
            break;
        }
    }
    flags
}

fn infer_generic_return_origin<M: Module>(
    cx: &Ctx<'_, M>,
    f_def: &FnDef,
    type_params: &[String],
    arg_values: &[Value],
) -> Option<String> {
    let Some(ret_ty) = &f_def.return_type else {
        return None;
    };
    let TypeExpr::Named(ret_name) = &ret_ty.node else {
        return None;
    };
    if !type_params.contains(ret_name) {
        return None;
    }
    for (param, actual_value) in f_def.params.iter().zip(arg_values.iter()) {
        if type_expr_mentions_type_param(&param.ty.node, ret_name) {
            if let Some(origin) = generic_origin_for_value(cx, *actual_value) {
                return Some(origin);
            }
        }
    }
    None
}

/// Attempt to inline the callee body at the call site. Inlining is skipped for
/// generic functions (type-parameter inference needs the normal call path),
/// Result-returning functions (heap-allocated tagged unions need real
/// call/return semantics), Optional-returning functions (Some/None lose inner
/// type info), beyond `MAX_INLINE_DEPTH`, for callees containing `return`, and
/// on arity mismatch. Returns `Ok(Some(..))` when the call was inlined,
/// `Ok(None)` to fall back to a normal call instruction.
fn try_inline_fn_call<M: Module>(
    cx: &mut Ctx<'_, M>,
    name: &str,
    type_params: &[String],
    ret_is_result: bool,
    actual_ret_tty: &TurboTy,
    arg_values: &[Value],
) -> Result<Option<MaybeTyped>, CodegenError> {
    // Try inline expansion: inline the callee body at this call site
    // if we haven't exceeded the depth limit and the function is inlineable.
    // Skip inlining for generic functions (type parameter inference needs
    // normal call path), for Result-returning functions (heap-allocated
    // tagged unions require proper call/return semantics), and for
    // Optional-returning functions (SomeExpr/NoneExpr lose inner type info).
    let ret_is_optional = matches!(actual_ret_tty, TurboTy::Optional(_));
    if cx.inline_depth < MAX_INLINE_DEPTH
        && type_params.is_empty()
        && !ret_is_result
        && !ret_is_optional
    {
        if let Some(callee_def) = cx.fn_asts.get(name).cloned() {
            if !has_return(&callee_def.body.node) && callee_def.params.len() == arg_values.len() {
                if callee_def.params.iter().any(|param| param.mutable) {
                    return Ok(None);
                }
                // Save and restore outer variable scope so inlined
                // parameter bindings don't leak out.
                let saved_vars = cx.vars.clone();
                let saved_depth = cx.inline_depth;
                cx.inline_depth += 1;

                // Bind each parameter to the already-compiled argument value.
                for (i, param) in callee_def.params.iter().enumerate() {
                    let cl_ty = resolve_cl_type(
                        &param.ty.node,
                        cx.ptr_type,
                        cx.enum_variants,
                        type_params,
                    )?;
                    let turbo_ty = turbo_ty_from_type_expr(&param.ty.node, cx.enum_variants);
                    let var = cx.fresh_var(cl_ty, turbo_ty.clone());
                    cx.builder.def_var(var, arg_values[i]);
                    cx.vars.insert(param.name.clone(), (var, cl_ty, turbo_ty));
                }

                let result = compile_expr(cx, &callee_def.body)?;

                cx.vars = saved_vars;
                cx.inline_depth = saved_depth;

                return Ok(Some(result));
            }
        }
    }
    Ok(None)
}

/// Compile a plain call to a user-defined free function: resolve the function
/// ref and its signature, compile and reconcile arguments (with COW retains),
/// infer the concrete return type for generics, attempt inline expansion, and
/// otherwise emit a normal call instruction, reinterpreting a float return that
/// arrives through a generic's uniform i64 slot.
fn compile_plain_fn_call<M: Module>(
    cx: &mut Ctx<'_, M>,
    name: &str,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let func_id = *cx.user_fns.get(name).ok_or_else(|| CodegenError {
        code: ErrorCode::E0402,
        message: format!("undefined function: {name}"),
    })?;

    let ret_tty = cx.fn_ret_types.get(name).cloned().unwrap_or(TurboTy::Unit);
    let ret_is_result = matches!(&ret_tty, TurboTy::Result(_, _));
    let type_params = cx.fn_type_params.get(name).cloned().unwrap_or_default();

    let func_ref = cx.module.declare_func_in_func(func_id, cx.builder.func);
    let sig = cx.builder.func.dfg.ext_funcs[func_ref].signature;
    let param_types: Vec<types::Type> = cx.builder.func.dfg.signatures[sig]
        .params
        .iter()
        .map(|p| p.value_type)
        .collect();

    let param_mutable: Vec<bool> = cx
        .fn_asts
        .get(name)
        .map(|f_def| f_def.params.iter().map(|param| param.mutable).collect())
        .unwrap_or_else(|| vec![false; args.len()]);
    let (mut arg_values, arg_ttys, owned_arg_temps) =
        compile_fn_call_args(cx, args, &param_types, &param_mutable)?;

    // For generic functions, infer the actual return TurboTy from args.
    let actual_ret_tty = infer_generic_ret_tty(cx, name, &type_params, ret_tty, &arg_ttys);

    // For generic functions, widen bool args (I8) to I64 since
    // the generic function's parameter is compiled as I64.
    if !type_params.is_empty() {
        let f_def = cx.fn_asts.get(name).copied();
        let type_bindings = f_def
            .map(|f_def| infer_generic_type_bindings(f_def, &type_params, &arg_ttys))
            .unwrap_or_default();
        let dynamic_rc_flags = f_def
            .map(|f_def| infer_generic_dynamic_rc_flags(cx, f_def, &type_params, &arg_values))
            .unwrap_or_default();
        for (i, val) in arg_values.iter_mut().enumerate() {
            let vty = cx.builder.func.dfg.value_type(*val);
            if param_types.get(i).copied() == Some(types::I64) && vty.bits() < 64 {
                *val = cx.builder.ins().sextend(types::I64, *val);
            }
        }
        for type_param in &type_params {
            if let Some(flag) = dynamic_rc_flags.get(type_param).copied() {
                arg_values.push(flag);
            } else {
                let is_rc = type_bindings
                    .get(type_param)
                    .is_some_and(|actual_ty| is_rc_managed_type(cx, actual_ty));
                arg_values.push(cx.builder.ins().iconst(types::I8, i64::from(is_rc)));
            }
        }
    }
    let generic_return_origin = if !type_params.is_empty() {
        cx.fn_asts
            .get(name)
            .and_then(|f_def| infer_generic_return_origin(cx, f_def, &type_params, &arg_values))
    } else {
        None
    };

    if let Some(result) = try_inline_fn_call(
        cx,
        name,
        &type_params,
        ret_is_result,
        &actual_ret_tty,
        &arg_values,
    )? {
        release_owned_call_arg_temps(cx, &owned_arg_temps);
        return Ok(result);
    }

    // Fall back to a normal call instruction.
    let call = cx.builder.ins().call(func_ref, &arg_values);
    let results = cx.builder.inst_results(call).to_vec();
    if results.is_empty() {
        release_owned_call_arg_temps(cx, &owned_arg_temps);
        Ok(None)
    } else {
        let mut result = results[0];
        // Generic functions return their type-parameter result through a
        // uniform i64 slot. If the inferred return type is float, the
        // value arrives as raw i64 bits — reinterpret them as F64 so the
        // Cranelift value type matches its TurboTy.
        if matches!(actual_ret_tty, TurboTy::Float) {
            let rty = cx.builder.func.dfg.value_type(result);
            if rty.is_int() && rty.bits() == 64 {
                result = cx
                    .builder
                    .ins()
                    .bitcast(types::F64, MemFlags::new(), result);
            }
        }
        if cx.extern_fns.contains(name) && matches!(actual_ret_tty, TurboTy::Str) {
            let fid = cx.rt_fns["rt_str_copy"];
            let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
            let call = cx.builder.ins().call(fref, &[result]);
            result = cx.builder.inst_results(call)[0];
        }
        if let Some(origin) = generic_return_origin {
            let already_owned = cx.builder.ins().iconst(types::I8, 0);
            mark_generic_value_origin_with_retain_flag(cx, result, origin, already_owned);
        }
        release_owned_call_arg_temps(cx, &owned_arg_temps);
        Ok(Some((result, actual_ret_tty)))
    }
}
