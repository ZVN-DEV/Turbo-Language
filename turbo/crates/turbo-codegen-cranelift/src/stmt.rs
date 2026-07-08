//! Statement compilation.
//!
//! Contains `compile_stmt()` which handles `Let`, `Expr`, `Return`,
//! `Defer`, and `LetDestructure` statement variants.

use super::*;

// ── Statement compilation ───────────────────────────────────────────

/// Whether `e` is a bare, zero-argument `hashmap()` call — the constructor
/// that a `HashMap<K, V>` annotation turns into a typed map.
fn is_bare_hashmap_call(e: &Expr) -> bool {
    if let Expr::Call { callee, args } = e {
        args.is_empty() && matches!(&callee.node, Expr::Ident(n) if n == "hashmap")
    } else {
        false
    }
}

pub(crate) fn compile_stmt<M: Module>(
    cx: &mut Ctx<'_, M>,
    stmt: &Spanned<Stmt>,
) -> Result<(), CodegenError> {
    match &stmt.node {
        Stmt::Let {
            name, value, ty, ..
        } => {
            // Clear any stale concrete fields from a previous struct lit
            cx.last_struct_lit_concrete_fields = None;
            // Check if the RHS is a variable reference (for COW retain)
            let rhs_is_ident = matches!(&value.node, Expr::Ident(_));
            // A `hashmap()` bound to a `HashMap<K, V>` annotation constructs a
            // typed, descriptor-carrying map instead of the legacy str→str
            // handle, so its key hashing and rc value retain/release are set up.
            let typed_map_ctor = ty.as_ref().and_then(|t| {
                match turbo_ty_from_type_expr(&t.node, cx.enum_variants) {
                    TurboTy::HashMap(k, v) if is_bare_hashmap_call(&value.node) => Some((k, v)),
                    _ => None,
                }
            });
            let result = match typed_map_ctor {
                Some((k, v)) => crate::builtins::compile_typed_hashmap_ctor(cx, &k, &v)?,
                None => compile_expr(cx, value)?,
            };
            // If the value was a struct literal, capture concrete field types for generic structs
            if let Some(concrete_fields) = cx.last_struct_lit_concrete_fields.take() {
                cx.generic_struct_field_overrides
                    .insert(name.clone(), concrete_fields);
            }
            let (cl_ty, turbo_ty, val) = if let Some((v, tty)) = result {
                // If there's a type annotation, check if we need to coerce (e.g., int -> u8)
                if let Some(ty_ann) = ty {
                    let target_tty = turbo_ty_from_type_expr(&ty_ann.node, cx.enum_variants);
                    let (coerced_val, coerced_tty) = coerce_value(cx, v, &tty, &target_tty);
                    let coerced_cl = cx.builder.func.dfg.value_type(coerced_val);
                    (coerced_cl, coerced_tty, Some(coerced_val))
                } else {
                    (cx.builder.func.dfg.value_type(v), tty, Some(v))
                }
            } else {
                (types::I64, TurboTy::Unit, None)
            };
            // COW: retain when the RHS aliases an existing heap allocation so
            // the shared object's refcount reflects both bindings (which is
            // what triggers copy-on-write on a later mutation):
            //   - any heap value bound from another variable (`let b = a`)
            //   - a struct or inner array copied out of an array element
            //     (`let s = arr[0]` / `let row = grid[0]`): the outer array
            //     keeps its reference to that element, so the new binding must
            //     be counted too — otherwise a later `s.x = ..` / `row[0] = ..`
            //     sees refcount 1, mutates in place, and silently aliases the
            //     array element (structs: BL-10; nested arrays: BL-27 Part A).
            //     Strings follow the same rule now that they are ARC-managed.
            //     Gated to refcounted element types so scalar indexing (e.g.
            //     `let x = ints[0]`) is left byte-for-byte unchanged.
            let rhs_retains = rhs_is_ident
                || (matches!(&value.node, Expr::Index { .. }) && is_rc_managed_type(cx, &turbo_ty))
                || (matches!(&value.node, Expr::FieldAccess { .. })
                    && is_rc_managed_type(cx, &turbo_ty));
            if rhs_retains {
                if let Some(v) = val {
                    retain_if_needed(cx, v, &turbo_ty);
                }
            }
            let var = Variable::new(cx.next_var);
            cx.next_var += 1;
            cx.builder.declare_var(var, cl_ty);
            if let Some(v) = val {
                cx.builder.def_var(var, v);
            }
            cx.vars.insert(name.clone(), (var, cl_ty, turbo_ty));
            Ok(())
        }
        Stmt::Expr(e) => {
            // COW builtin auto-reassign is handled in the parser now
            // (rewrites `push(items, 4)` → `items = push(items, 4)`)
            if let Some((value, tty)) = compile_expr(cx, e)? {
                release_expr_temp_if_needed(cx, value, &tty, e);
            }
            Ok(())
        }
        Stmt::Return(value) => {
            if let Some(val_expr) = value {
                let result = compile_expr(cx, val_expr)?;
                if let Some((v, tty)) = result {
                    if let Expr::Ident(name) = &val_expr.node {
                        if let Some((var, _, _)) = cx.vars.get(name) {
                            if cx.borrowed_param_vars.contains(var) && is_rc_managed_type(cx, &tty)
                            {
                                retain_if_needed(cx, v, &tty);
                            }
                        }
                    }
                    release_mutable_param_vars(cx);
                    cx.builder.ins().return_(&[v]);
                } else {
                    release_mutable_param_vars(cx);
                    cx.builder.ins().return_(&[]);
                }
            } else {
                release_mutable_param_vars(cx);
                cx.builder.ins().return_(&[]);
            }
            let new_block = cx.builder.create_block();
            cx.builder.switch_to_block(new_block);
            cx.builder.seal_block(new_block);
            Ok(())
        }
        Stmt::Defer(_) => {
            // Defer statements are handled at the block level (compile_expr for Block)
            // — they are collected and emitted in reverse order at the end of the block.
            Ok(())
        }
        Stmt::LetDestructure { fields, value, .. } => {
            // Compile the value expression (should produce a struct pointer)
            let (struct_ptr, struct_tty) =
                compile_expr(cx, value)?.ok_or_else(|| CodegenError {
                    code: ErrorCode::E0400,
                    message: "destructured value produced no result".to_string(),
                })?;

            let struct_name = match &struct_tty {
                TurboTy::Struct(name) => name.clone(),
                _ => {
                    return Err(CodegenError {
                        code: ErrorCode::E0400,
                        message: "cannot destructure non-struct type".to_string(),
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

            for field_name in fields {
                let field_index = struct_layout
                    .iter()
                    .position(|(n, _)| n == field_name)
                    .ok_or_else(|| CodegenError {
                        code: ErrorCode::E0400,
                        message: format!("struct `{struct_name}` has no field `{field_name}`"),
                    })?;

                let field_tty = struct_layout[field_index].1.clone();
                let offset = (field_index * 8) as i32;

                let val = cx
                    .builder
                    .ins()
                    .load(types::I64, MemFlags::new(), struct_ptr, offset);

                let var = Variable::new(cx.next_var);
                cx.next_var += 1;
                cx.builder.declare_var(var, types::I64);
                cx.builder.def_var(var, val);
                cx.vars
                    .insert(field_name.clone(), (var, types::I64, field_tty));
            }
            Ok(())
        }
    }
}
