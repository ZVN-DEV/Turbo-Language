//! Expression compilation.
//!
//! Contains `compile_expr()` — the main expression compiler that handles all
//! `Expr` variants — along with binary operations, short-circuit evaluation,
//! function calls, RC heap helpers, and JSON decode support.

use super::*;

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
            let (var, _cl_ty, turbo_ty) = cx.vars.get(name).ok_or_else(|| CodegenError {
                code: ErrorCode::E0401,
                message: format!("undefined variable: {name}"),
            })?;
            let turbo_ty = turbo_ty.clone();
            let val = cx.builder.use_var(*var);
            Ok(Some((val, turbo_ty)))
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
                        return compile_str_concat(cx, lhs, rhs);
                    }
                    BinOp::Eq | BinOp::NotEq => {
                        return compile_str_compare(cx, lhs, rhs, *op);
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
                _ => lhs_tty,
            };
            Ok(Some((result, result_tty)))
        }

        Expr::UnaryOp { op, expr: inner } => {
            let (val, tty) = compile_expr(cx, inner)?.unwrap();
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
                    retain_if_needed(cx, *value, tty);
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
                        _ if is_rc_heap_type(tty) => Some((*var, tty.clone())),
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
                            let (rhs, _) = compile_expr(cx, right)?.unwrap();
                            let fid = cx.rt_fns["rt_str_concat_inplace"];
                            let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
                            let call = cx.builder.ins().call(fref, &[current, rhs]);
                            let result = cx.builder.inst_results(call)[0];
                            cx.builder.def_var(var, result);
                            return Ok(None);
                        }
                    }
                }
            }
            let rhs_ident = match &value.node {
                Expr::Ident(name) => Some(name.as_str()),
                _ => None,
            };
            let (val, tty) = compile_expr(cx, value)?.unwrap();
            let (var, _, prev_tty) = cx.vars.get(target).ok_or_else(|| CodegenError {
                code: ErrorCode::E0401,
                message: format!("undefined variable: {target}"),
            })?;
            let var = *var;
            let prev_tty = prev_tty.clone();

            if is_rc_heap_type(&prev_tty) && is_rc_heap_type(&tty) {
                // Assignment to refcounted values must handle aliasing as:
                //
                //   if old != new {
                //       retain(new); // when new is borrowed from another variable
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
                if rhs_ident.is_some() {
                    retain_if_needed(cx, val, &tty);
                }
                release_if_needed(cx, prev_val, &prev_tty);
                cx.builder.ins().jump(done_block, &[]);

                cx.builder.switch_to_block(done_block);
                cx.builder.seal_block(done_block);
            } else {
                if rhs_ident.is_some() {
                    retain_if_needed(cx, val, &tty);
                }
                if is_rc_heap_type(&prev_tty) {
                    let prev_val = cx.builder.use_var(var);
                    release_if_needed(cx, prev_val, &prev_tty);
                }
            }
            cx.builder.def_var(var, val);
            // Update the turbo type in case it changed
            if let Some(entry) = cx.vars.get_mut(target) {
                entry.2 = tty;
            }
            Ok(None)
        }

        Expr::CompoundAssign { target, op, value } => {
            let (rhs, _) = compile_expr(cx, value)?.unwrap();
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
            let (obj_ptr, obj_tty) = compile_expr(cx, object)?.unwrap();
            let (val, _) = compile_expr(cx, value)?.unwrap();

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

            if is_rc_heap_type(&field_tty) {
                retain_if_needed(cx, val, &field_tty);
                let old_val = cx
                    .builder
                    .ins()
                    .load(cx.ptr_type, MemFlags::new(), obj_ptr, offset);
                release_if_needed(cx, old_val, &field_tty);
            }

            cx.builder
                .ins()
                .store(MemFlags::new(), val, obj_ptr, offset);
            Ok(None)
        }

        Expr::IndexAssign {
            object,
            index,
            value,
        } => {
            let (arr, _arr_tty) = compile_expr(cx, object)?.unwrap();
            let (idx, _) = compile_expr(cx, index)?.unwrap();
            let idx = {
                let idx_ty = cx.builder.func.dfg.value_type(idx);
                if idx_ty.is_int() && idx_ty.bits() < 64 {
                    cx.builder.ins().uextend(types::I64, idx)
                } else {
                    idx
                }
            };
            let (val, _) = compile_expr(cx, value)?.unwrap();

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

            if cx.is_unsafe {
                // @unsafe: skip COW check and bounds check — direct store
                let data_base = cx.builder.ins().iadd_imm(arr, 8);
                let byte_offset = cx.builder.ins().ishl_imm(idx, 3);
                let elem_ptr = cx.builder.ins().iadd(data_base, byte_offset);
                cx.builder.ins().store(trusted, val, elem_ptr, 0i32);
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
                cx.builder.ins().store(trusted, val, elem_ptr, 0i32);
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
                        let result_val = cx.builder.inst_results(call)[0];
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

                        // Compile all arguments
                        let mut arg_vals = Vec::new();
                        for arg in args {
                            if let Some((val, tty)) = compile_expr(cx, arg)? {
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

                        // Call rt_spawn_with_args(thunk_ptr, args_ptr) -> handle
                        let spawn_fid = cx.rt_fns["rt_spawn_with_args"];
                        let spawn_fref = cx.module.declare_func_in_func(spawn_fid, cx.builder.func);
                        let call = cx.builder.ins().call(spawn_fref, &[thunk_fn_ptr, args_ptr]);
                        let handle = cx.builder.inst_results(call)[0];

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
            let (result_ptr, _result_tty) = compile_expr(cx, inner)?.unwrap();

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
                let (val, tty) = compile_expr(cx, elem)?.unwrap();
                if i == 0 {
                    elem_tty = tty;
                }
                let offset = cx.builder.ins().iconst(cx.ptr_type, (8 + i * 8) as i64);
                let elem_ptr = cx.builder.ins().iadd(arr_ptr, offset);
                cx.builder.ins().store(MemFlags::new(), val, elem_ptr, 0);
            }

            Ok(Some((arr_ptr, TurboTy::Array(Box::new(elem_tty)))))
        }

        Expr::Index { object, index } => {
            let (arr, arr_tty) = compile_expr(cx, object)?.unwrap();
            let (idx, _) = compile_expr(cx, index)?.unwrap();
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

                let (val, tty) = compile_expr(cx, field_value)?.unwrap();
                retain_if_needed(cx, val, &tty);
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

            let (obj_ptr, obj_tty) = compile_expr(cx, object)?.unwrap();

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
            let (val, tty) = compile_expr(cx, value)?.unwrap();
            retain_if_needed(cx, val, &tty);
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
            let (val, tty) = compile_expr(cx, value)?.unwrap();
            retain_if_needed(cx, val, &tty);
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
            let (val, tty) = compile_expr(cx, value)?.unwrap();
            retain_if_needed(cx, val, &tty);
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
            let (opt_val, _opt_tty) = compile_expr(cx, value)?.unwrap();

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
            let (def_val, def_tty) = compile_expr(cx, default)?.unwrap();
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
        TurboTy::Array(_) | TurboTy::Struct(_) | TurboTy::Result(_, _) | TurboTy::Optional(_)
    )
}

pub(crate) fn retain_if_needed<M: Module>(cx: &mut Ctx<'_, M>, value: Value, ty: &TurboTy) {
    if !is_rc_heap_type(ty) {
        return;
    }
    let retain_fid = cx.rt_fns["rt_retain"];
    let retain_ref = cx.module.declare_func_in_func(retain_fid, cx.builder.func);
    cx.builder.ins().call(retain_ref, &[value]);
}

pub(crate) fn release_if_needed<M: Module>(cx: &mut Ctx<'_, M>, value: Value, ty: &TurboTy) {
    if !is_rc_heap_type(ty) {
        return;
    }
    match ty {
        TurboTy::Struct(name) => {
            if let Some(layout) = cx.struct_fields.get(name).cloned() {
                for (index, (_field_name, field_ty)) in layout.iter().enumerate() {
                    if is_rc_heap_type(field_ty) {
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
        TurboTy::Optional(inner) if is_rc_heap_type(inner) => {
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
        TurboTy::Result(ok_tty, err_tty) if is_rc_heap_type(ok_tty) || is_rc_heap_type(err_tty) => {
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
            if is_rc_heap_type(ok_tty) {
                let val_fid = cx.rt_fns["rt_result_value"];
                let val_fref = cx.module.declare_func_in_func(val_fid, cx.builder.func);
                let val_call = cx.builder.ins().call(val_fref, &[value]);
                let inner_val = cx.builder.inst_results(val_call)[0];
                release_if_needed(cx, inner_val, ok_tty);
            }
            cx.builder.ins().jump(done_block, &[]);

            cx.builder.switch_to_block(err_block);
            cx.builder.seal_block(err_block);
            if is_rc_heap_type(err_tty) {
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
    let (lhs, _) = compile_expr(cx, left)?.unwrap();
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
    let (rhs, _) = compile_expr(cx, right)?.unwrap();

    let rhs_as_i8 = cx.to_bool(rhs);

    cx.builder.ins().jump(merge_block, &[rhs_as_i8]);

    cx.builder.switch_to_block(merge_block);
    cx.builder.seal_block(merge_block);

    let result = cx.builder.block_params(merge_block)[0];
    Ok(Some((result, TurboTy::Bool)))
}

// ── Function calls ──────────────────────────────────────────────────

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
        let (obj_val, obj_tty) = compile_expr(cx, object)?.unwrap();
        if let TurboTy::Struct(ref type_name) = obj_tty {
            let mangled = format!("{}__{}", type_name, field);
            if let Some(&fid) = cx.user_fns.get(&mangled) {
                let mut arg_vals = vec![obj_val];
                for arg in args {
                    if let Some((v, _)) = compile_expr(cx, arg)? {
                        arg_vals.push(v);
                    }
                }
                let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
                let call = cx.builder.ins().call(fref, &arg_vals);
                let results = cx.builder.inst_results(call);
                let ret_tty = cx
                    .fn_ret_types
                    .get(&mangled)
                    .cloned()
                    .unwrap_or(TurboTy::Unit);
                if results.is_empty() {
                    return Ok(None);
                } else {
                    return Ok(Some((results[0], ret_tty)));
                }
            }
        }
        return Err(CodegenError {
            code: ErrorCode::E0400,
            message: format!("no method `{field}` found"),
        });
    }

    let Expr::Ident(name) = &callee.node else {
        return Err(CodegenError {
            code: ErrorCode::E0400,
            message: "indirect function calls not yet supported".to_string(),
        });
    };

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
        // Unsafe builtins — raw pointer operations
        "deref" => compile_builtin_deref(cx, args),
        "store" => compile_builtin_store(cx, args),
        _ => {
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
                                let alloc_fref =
                                    cx.module.declare_func_in_func(alloc_fid, cx.builder.func);
                                let call = cx.builder.ins().call(alloc_fref, &[num_fields_val]);
                                let ptr = cx.builder.inst_results(call)[0];

                                // Store tag at offset 0
                                let tag_val =
                                    cx.builder.ins().iconst(types::I64, variant_index as i64);
                                cx.builder.ins().store(MemFlags::new(), tag_val, ptr, 0);

                                // Get the field types for this variant
                                let _field_tys = cx
                                    .enum_variant_fields
                                    .get(&(enum_name.clone(), name.to_string()))
                                    .cloned()
                                    .unwrap_or_default();

                                // Store each field at offset (i+1)*8
                                for (i, arg) in data_args.iter().enumerate() {
                                    let (val, _tty) = compile_expr(cx, arg)?.unwrap();
                                    let offset = ((i + 1) * 8) as i32;

                                    // Widen/convert to i64 for uniform storage
                                    let val_ty = cx.builder.func.dfg.value_type(val);
                                    let store_val = if val_ty.is_float() && val_ty.bits() == 64 {
                                        cx.builder.ins().bitcast(types::I64, MemFlags::new(), val)
                                    } else if val_ty.is_float() && val_ty.bits() == 32 {
                                        let extended = cx.builder.ins().fpromote(types::F64, val);
                                        cx.builder.ins().bitcast(
                                            types::I64,
                                            MemFlags::new(),
                                            extended,
                                        )
                                    } else if val_ty.bits() < 64 && val_ty.is_int() {
                                        cx.builder.ins().sextend(types::I64, val)
                                    } else {
                                        val
                                    };

                                    cx.builder
                                        .ins()
                                        .store(MemFlags::new(), store_val, ptr, offset);
                                }

                                return Ok(Some((ptr, TurboTy::Enum(enum_name.clone()))));
                            } else {
                                // Unit-only enum, but called with args (shouldn't happen after sema check)
                                let val = cx.builder.ins().iconst(types::I64, variant_index as i64);
                                return Ok(Some((val, TurboTy::Enum(enum_name.clone()))));
                            }
                        }
                    }
                }
            }

            // Check if this is a method call (UFCS: parser rewrites obj.method(args) -> method(obj, args))
            if cx.user_fns.get(name.as_str()).is_none() && !args.is_empty() {
                // Compile first arg to get its type, then check for method
                let (first_val, first_tty) = compile_expr(cx, &args[0])?.unwrap();
                if let TurboTy::Struct(ref type_name) = first_tty {
                    let mangled = format!("{}__{}", type_name, name);
                    if let Some(&fid) = cx.user_fns.get(&mangled) {
                        let mut arg_vals = vec![first_val];
                        for arg in &args[1..] {
                            if let Some((v, _)) = compile_expr(cx, arg)? {
                                arg_vals.push(v);
                            }
                        }
                        let fref = cx.module.declare_func_in_func(fid, cx.builder.func);
                        let call = cx.builder.ins().call(fref, &arg_vals);
                        let results = cx.builder.inst_results(call);
                        let ret_tty = cx
                            .fn_ret_types
                            .get(&mangled)
                            .cloned()
                            .unwrap_or(TurboTy::Unit);
                        if results.is_empty() {
                            return Ok(None);
                        } else {
                            return Ok(Some((results[0], ret_tty)));
                        }
                    }
                }
            }

            // Check if the callee is a variable with a function pointer type (closure)
            if let Some((var, _cl_ty, TurboTy::Fn(ref param_tys, ref ret_ty))) =
                cx.vars.get(name).cloned()
            {
                // Closure is a pair struct: [fn_ptr, env_ptr]
                let closure_ptr = cx.builder.use_var(var);
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
                for param_tty in param_tys {
                    let cl_ty = turbo_ty_to_cl_type(param_tty, cx.ptr_type);
                    sig.params.push(AbiParam::new(cl_ty));
                }
                let ret_tty = *ret_ty.clone();
                if ret_tty != TurboTy::Unit {
                    let cl_ret = turbo_ty_to_cl_type(&ret_tty, cx.ptr_type);
                    sig.returns.push(AbiParam::new(cl_ret));
                }

                let sig_ref = cx.builder.import_signature(sig);

                let mut arg_values = vec![env_ptr]; // env_ptr is first hidden arg
                for arg in args {
                    if let Some((val, _)) = compile_expr(cx, arg)? {
                        arg_values.push(val);
                    }
                }

                let call = cx.builder.ins().call_indirect(sig_ref, fn_ptr, &arg_values);
                let results = cx.builder.inst_results(call);
                if results.is_empty() {
                    return Ok(None);
                } else {
                    return Ok(Some((results[0], ret_tty)));
                }
            }

            let func_id = *cx.user_fns.get(name.as_str()).ok_or_else(|| CodegenError {
                code: ErrorCode::E0402,
                message: format!("undefined function: {name}"),
            })?;

            let ret_tty = cx
                .fn_ret_types
                .get(name.as_str())
                .cloned()
                .unwrap_or(TurboTy::Unit);
            let ret_is_result = matches!(&ret_tty, TurboTy::Result(_, _));
            let type_params = cx
                .fn_type_params
                .get(name.as_str())
                .cloned()
                .unwrap_or_default();

            let func_ref = cx.module.declare_func_in_func(func_id, cx.builder.func);
            let sig = cx.builder.func.dfg.ext_funcs[func_ref].signature;
            let param_types: Vec<types::Type> = cx.builder.func.dfg.signatures[sig]
                .params
                .iter()
                .map(|p| p.value_type)
                .collect();
            let mut arg_values = Vec::new();
            let mut arg_ttys = Vec::new();
            for (i, arg) in args.iter().enumerate() {
                if let Some((val, tty)) = compile_expr(cx, arg)? {
                    let val = if i < param_types.len() {
                        let expected = param_types[i];
                        let actual = cx.builder.func.dfg.value_type(val);
                        if actual != expected && actual.is_int() && expected.is_int() {
                            if actual.bits() > expected.bits() {
                                cx.builder.ins().ireduce(expected, val)
                            } else {
                                cx.builder.ins().sextend(expected, val)
                            }
                        } else {
                            val
                        }
                    } else {
                        val
                    };
                    arg_values.push(val);
                    arg_ttys.push(tty);
                }
            }

            // For generic functions, infer the actual return TurboTy from args.
            let actual_ret_tty = if !type_params.is_empty() {
                if let Some(f_def) = cx.fn_asts.get(name.as_str()) {
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
            };

            // For generic functions, widen bool args (I8) to I64 since
            // the generic function's parameter is compiled as I64.
            if !type_params.is_empty() {
                for val in &mut arg_values {
                    let vty = cx.builder.func.dfg.value_type(*val);
                    if vty.bits() < 64 {
                        *val = cx.builder.ins().sextend(types::I64, *val);
                    }
                }
            }

            // Try inline expansion: inline the callee body at this call site
            // if we haven't exceeded the depth limit and the function is inlineable.
            // Skip inlining for generic functions (type parameter inference needs
            // normal call path), for Result-returning functions (heap-allocated
            // tagged unions require proper call/return semantics), and for
            // Optional-returning functions (SomeExpr/NoneExpr lose inner type info).
            let ret_is_optional = matches!(&actual_ret_tty, TurboTy::Optional(_));
            if cx.inline_depth < MAX_INLINE_DEPTH
                && type_params.is_empty()
                && !ret_is_result
                && !ret_is_optional
            {
                if let Some(callee_def) = cx.fn_asts.get(name.as_str()).cloned() {
                    if !has_return(&callee_def.body.node)
                        && callee_def.params.len() == arg_values.len()
                    {
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
                                &type_params,
                            )?;
                            let turbo_ty =
                                turbo_ty_from_type_expr(&param.ty.node, cx.enum_variants);
                            let var = cx.fresh_var(cl_ty, turbo_ty.clone());
                            cx.builder.def_var(var, arg_values[i]);
                            cx.vars.insert(param.name.clone(), (var, cl_ty, turbo_ty));
                        }

                        let result = compile_expr(cx, &callee_def.body)?;

                        cx.vars = saved_vars;
                        cx.inline_depth = saved_depth;

                        return Ok(result);
                    }
                }
            }

            // Fall back to a normal call instruction.
            let call = cx.builder.ins().call(func_ref, &arg_values);
            let results = cx.builder.inst_results(call);
            if results.is_empty() {
                Ok(None)
            } else {
                Ok(Some((results[0], actual_ret_tty)))
            }
        }
    }
}
