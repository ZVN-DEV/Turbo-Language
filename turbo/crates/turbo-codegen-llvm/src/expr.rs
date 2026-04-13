//! Expression compilation: compile_expr, compile_binop, control flow,
//! and value conversion helpers.

use inkwell::types::{BasicType, BasicTypeEnum};
use inkwell::values::{BasicValueEnum, IntValue, PointerValue};
use inkwell::{AddressSpace, FloatPredicate, IntPredicate};
use std::collections::HashMap;
use turbo_ast::*;

use crate::builtins::compile_call;
use crate::ctx::Ctx;
use crate::helpers::{collect_free_vars_llvm, lookup_variant_tag};
use crate::stmt::compile_stmt;
use crate::types::{
    turbo_ty_from_type_expr, turbo_ty_to_llvm, turbo_ty_to_llvm_ctx, MaybeTyped, TurboTy,
};
use crate::CodegenError;

// ── Expression compilation ──────────────────────────────────────────

pub(crate) fn compile_expr<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    expr: &Spanned<Expr>,
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    match &expr.node {
        Expr::IntLit(n) => {
            let val = cx.context.i64_type().const_int(*n as u64, true);
            Ok(Some((val.into(), TurboTy::Int)))
        }

        Expr::FloatLit(f) => {
            let val = cx.context.f64_type().const_float(*f);
            Ok(Some((val.into(), TurboTy::Float)))
        }

        Expr::BoolLit(b) => {
            let val = cx.context.i8_type().const_int(*b as u64, false);
            Ok(Some((val.into(), TurboTy::Bool)))
        }

        Expr::StringLit(s) => {
            let ptr = cx.create_string(s)?;
            Ok(Some((ptr.into(), TurboTy::Str)))
        }

        Expr::Unit => Ok(None),

        Expr::Ident(name) => {
            // Check constants first
            if let Some(const_expr) = cx.constants.get(name.as_str()) {
                let const_expr = const_expr.clone();
                return compile_expr(cx, &const_expr);
            }
            let (alloca, turbo_ty) = cx.vars.get(name).ok_or_else(|| CodegenError {
                code: ErrorCode::E0401,
                message: format!("undefined variable: {name}"),
            })?;
            let turbo_ty = turbo_ty.clone();
            let llvm_ty = turbo_ty_to_llvm_ctx(&turbo_ty, cx.context, cx.enum_max_slots);
            let val = cx
                .builder
                .build_load(llvm_ty, *alloca, name)
                .expect("build_load failed");
            Ok(Some((val, turbo_ty)))
        }

        Expr::BinaryOp { left, op, right } => {
            // Short-circuit for && and ||
            if *op == BinOp::And || *op == BinOp::Or {
                return compile_short_circuit(cx, left, *op, right);
            }

            let (lhs, lhs_tty) = compile_expr(cx, left)?.unwrap();
            let (rhs, rhs_tty) = compile_expr(cx, right)?.unwrap();

            // String operations
            if lhs_tty == TurboTy::Str && rhs_tty == TurboTy::Str {
                match op {
                    BinOp::Add => {
                        let result = cx
                            .rt_call("rt_str_concat", &[lhs.into(), rhs.into()])
                            .unwrap();
                        return Ok(Some((result, TurboTy::Str)));
                    }
                    BinOp::Eq | BinOp::NotEq => {
                        let result = cx.rt_call("rt_str_eq", &[lhs.into(), rhs.into()]).unwrap();
                        let result_int = result.into_int_value();
                        if *op == BinOp::NotEq {
                            let one = cx.context.i8_type().const_int(1, false);
                            let flipped = cx
                                .builder
                                .build_xor(result_int, one, "neq")
                                .expect("build_xor failed");
                            return Ok(Some((flipped.into(), TurboTy::Bool)));
                        }
                        return Ok(Some((result, TurboTy::Bool)));
                    }
                    _ => {}
                }
            }

            // Struct equality: use derived __eq method if available
            if let TurboTy::Struct(ref sname) = lhs_tty {
                if *op == BinOp::Eq || *op == BinOp::NotEq {
                    let eq_fn_name = format!("{sname}__eq");
                    if let Some(&eq_fn) = cx.user_fns.get(&eq_fn_name) {
                        let result = cx
                            .builder
                            .build_direct_call(eq_fn, &[lhs.into(), rhs.into()], "struct_eq")
                            .expect("build_direct_call")
                            .try_as_basic_value()
                            .left()
                            .unwrap();
                        if *op == BinOp::NotEq {
                            let one = cx.context.i8_type().const_int(1, false);
                            let flipped = cx
                                .builder
                                .build_xor(result.into_int_value(), one, "neq")
                                .expect("xor");
                            return Ok(Some((flipped.into(), TurboTy::Bool)));
                        }
                        return Ok(Some((result, TurboTy::Bool)));
                    }
                    // Fallback: pointer comparison
                    let lp = cx
                        .builder
                        .build_ptr_to_int(lhs.into_pointer_value(), cx.context.i64_type(), "lp")
                        .expect("p2i");
                    let rp = cx
                        .builder
                        .build_ptr_to_int(rhs.into_pointer_value(), cx.context.i64_type(), "rp")
                        .expect("p2i");
                    let pred = if *op == BinOp::Eq {
                        IntPredicate::EQ
                    } else {
                        IntPredicate::NE
                    };
                    let cmp = cx
                        .builder
                        .build_int_compare(pred, lp, rp, "ptr_eq")
                        .expect("cmp");
                    return Ok(Some((cmp.into(), TurboTy::Bool)));
                }
            }

            // String coercion: str + non-str or non-str + str
            if *op == BinOp::Add {
                if lhs_tty == TurboTy::Str && rhs_tty != TurboTy::Str {
                    let rhs_str = convert_to_str(cx, rhs, &rhs_tty)?;
                    let result = cx
                        .rt_call("rt_str_concat", &[lhs.into(), rhs_str.into()])
                        .unwrap();
                    return Ok(Some((result, TurboTy::Str)));
                }
                if rhs_tty == TurboTy::Str && lhs_tty != TurboTy::Str {
                    let lhs_str = convert_to_str(cx, lhs, &lhs_tty)?;
                    let result = cx
                        .rt_call("rt_str_concat", &[lhs_str.into(), rhs.into()])
                        .unwrap();
                    return Ok(Some((result, TurboTy::Str)));
                }
            }

            let result = compile_binop(cx, lhs, *op, rhs)?;
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
                UnaryOp::Neg => match val {
                    BasicValueEnum::FloatValue(fv) => cx
                        .builder
                        .build_float_neg(fv, "fneg")
                        .expect("build_float_neg failed")
                        .into(),
                    BasicValueEnum::IntValue(iv) => cx
                        .builder
                        .build_int_neg(iv, "ineg")
                        .expect("build_int_neg failed")
                        .into(),
                    _ => {
                        return Err(CodegenError {
                            code: ErrorCode::E0403,
                            message: "cannot negate this type".to_string(),
                        })
                    }
                },
                UnaryOp::Not => {
                    let iv = val.into_int_value();
                    let one = cx.context.i8_type().const_int(1, false);
                    cx.builder
                        .build_xor(iv, one, "not")
                        .expect("build_xor failed")
                        .into()
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

        Expr::IfLet { .. } => {
            // TODO: if-let not yet implemented for LLVM backend
            Err(CodegenError {
                code: ErrorCode::E0400,
                message: "if-let is not yet supported in the LLVM backend".to_string(),
            })
        }

        Expr::Block { stmts, tail_expr } => {
            let saved_vars = cx.vars.clone();

            let mut deferred: Vec<&Spanned<Expr>> = Vec::new();
            for stmt in stmts {
                if let Stmt::Defer(ref defer_expr) = stmt.node {
                    deferred.push(defer_expr);
                }
                compile_stmt(cx, stmt)?;
            }
            let result = if let Some(tail) = tail_expr {
                compile_expr(cx, tail)
            } else {
                Ok(None)
            };

            // Emit deferred expressions in LIFO order
            for defer_expr in deferred.iter().rev() {
                let block = cx.builder.get_insert_block().unwrap();
                if block.get_terminator().is_none() {
                    compile_expr(cx, defer_expr)?;
                }
            }

            cx.vars = saved_vars;
            result
        }

        Expr::Assign { target, value } => {
            let (val, tty) = compile_expr(cx, value)?.unwrap();
            let (alloca, _) = cx.vars.get(target).ok_or_else(|| CodegenError {
                code: ErrorCode::E0401,
                message: format!("undefined variable: {target}"),
            })?;
            let alloca = *alloca;
            cx.builder
                .build_store(alloca, val)
                .expect("build_store failed");
            // Update type
            if let Some(entry) = cx.vars.get_mut(target) {
                entry.1 = tty;
            }
            Ok(None)
        }

        Expr::CompoundAssign { target, op, value } => {
            let (rhs, _) = compile_expr(cx, value)?.unwrap();
            let (alloca, turbo_ty) = cx.vars.get(target).ok_or_else(|| CodegenError {
                code: ErrorCode::E0401,
                message: format!("undefined variable: {target}"),
            })?;
            let alloca = *alloca;
            let turbo_ty = turbo_ty.clone();
            let llvm_ty = turbo_ty_to_llvm_ctx(&turbo_ty, cx.context, cx.enum_max_slots);
            let lhs = cx
                .builder
                .build_load(llvm_ty, alloca, target)
                .expect("build_load failed");
            let result = compile_binop(cx, lhs, *op, rhs)?;
            cx.builder
                .build_store(alloca, result)
                .expect("build_store failed");
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

            let offset = field_index as u64 * 8;
            let obj_ptr_val = obj_ptr.into_pointer_value();

            // GEP to field offset
            let field_ptr = unsafe {
                cx.builder
                    .build_gep(
                        cx.context.i8_type(),
                        obj_ptr_val,
                        &[cx.context.i64_type().const_int(offset, false)],
                        "field_ptr",
                    )
                    .expect("build_gep failed")
            };

            // Widen to i64 for uniform storage
            let store_val = widen_for_storage(cx, val);
            cx.builder
                .build_store(field_ptr, store_val)
                .expect("build_store failed");
            Ok(None)
        }

        Expr::IndexAssign {
            object,
            index,
            value,
        } => {
            let (arr, _) = compile_expr(cx, object)?.unwrap();
            let (idx, _) = compile_expr(cx, index)?.unwrap();
            let (val, _) = compile_expr(cx, value)?.unwrap();

            let store_val = widen_for_storage(cx, val);
            let new_arr = cx
                .rt_call("rt_array_set", &[arr.into(), idx.into(), store_val.into()])
                .unwrap();

            // Update the variable to point to the (possibly new) array
            if let Expr::Ident(name) = &object.node {
                if let Some((alloca, _)) = cx.vars.get(name) {
                    cx.builder
                        .build_store(*alloca, new_arr)
                        .expect("build_store failed");
                }
            }

            Ok(None)
        }

        Expr::While { condition, body } => compile_while(cx, condition, body),

        Expr::ForIn {
            var_name,
            iterable,
            body,
        } => compile_for_in(cx, var_name, iterable, body),

        Expr::ArrayLit(elems) => {
            let len = elems.len() as u64;
            let len_val = cx.context.i64_type().const_int(len, false);
            let arr = cx.rt_call("rt_array_alloc", &[len_val.into()]).unwrap();

            let mut elem_tty = TurboTy::Int;
            for (i, elem) in elems.iter().enumerate() {
                let (val, tty) = compile_expr(cx, elem)?.unwrap();
                if i == 0 {
                    elem_tty = tty;
                }
                let idx = cx.context.i64_type().const_int(i as u64, false);
                let store_val = widen_for_storage(cx, val);
                cx.rt_call("rt_array_set", &[arr.into(), idx.into(), store_val.into()]);
            }

            Ok(Some((arr, TurboTy::Array(Box::new(elem_tty)))))
        }

        Expr::Index { object, index } => {
            let (obj, obj_tty) = compile_expr(cx, object)?.unwrap();
            let (idx, _) = compile_expr(cx, index)?.unwrap();

            let elem_tty = match &obj_tty {
                TurboTy::Array(inner) => *inner.clone(),
                _ => TurboTy::Int,
            };

            let raw = cx
                .rt_call("rt_array_get", &[obj.into(), idx.into()])
                .unwrap();

            // Narrow the result back from i64 to the element type
            let result = narrow_from_storage(cx, raw, &elem_tty);
            Ok(Some((result, elem_tty)))
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

            let num_fields = struct_layout.len() as u64;
            let num_fields_val = cx.context.i64_type().const_int(num_fields, false);
            let ptr = cx
                .rt_call("rt_struct_alloc", &[num_fields_val.into()])
                .unwrap()
                .into_pointer_value();

            let mut concrete_fields: Vec<(String, TurboTy)> = Vec::new();
            for (field_name, field_expr) in fields {
                let (val, val_tty) = compile_expr(cx, field_expr)?.unwrap();
                concrete_fields.push((field_name.clone(), val_tty));
                let field_index = struct_layout
                    .iter()
                    .position(|(n, _)| n == field_name)
                    .ok_or_else(|| CodegenError {
                        code: ErrorCode::E0400,
                        message: format!("struct `{name}` has no field `{field_name}`"),
                    })?;

                let offset = field_index as u64 * 8;
                let field_ptr = unsafe {
                    cx.builder
                        .build_gep(
                            cx.context.i8_type(),
                            ptr,
                            &[cx.context.i64_type().const_int(offset, false)],
                            "field_ptr",
                        )
                        .expect("build_gep failed")
                };

                let store_val = widen_for_storage(cx, val);
                cx.builder
                    .build_store(field_ptr, store_val)
                    .expect("build_store failed");
            }

            let result_tty = TurboTy::Struct(name.clone());
            // Store concrete field types for generic struct tracking
            // Use a temp key "__last_struct_lit" that Let binding will pick up
            if !concrete_fields.is_empty() {
                cx.concrete_struct_fields
                    .insert("__last_struct_lit".to_string(), concrete_fields);
            }
            Ok(Some((ptr.into(), result_tty)))
        }

        Expr::FieldAccess { object, field } => {
            // Check if this is an enum variant access: EnumName.VariantName
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

                    if let Some(&max_slots) = cx.enum_max_slots.get(name.as_str()) {
                        // Data-carrying enum: allocate tagged union
                        let total_slots = 1 + max_slots;
                        let num_fields_val =
                            cx.context.i64_type().const_int(total_slots as u64, false);
                        let ptr = cx
                            .rt_call("rt_struct_alloc", &[num_fields_val.into()])
                            .unwrap()
                            .into_pointer_value();
                        let tag_val = cx.context.i64_type().const_int(index as u64, false);
                        cx.builder
                            .build_store(ptr, tag_val)
                            .expect("build_store failed");
                        return Ok(Some((ptr.into(), TurboTy::Enum(name.clone()))));
                    } else {
                        let val = cx.context.i64_type().const_int(index as u64, false);
                        return Ok(Some((val.into(), TurboTy::Enum(name.clone()))));
                    }
                }
            }

            let (obj, obj_tty) = compile_expr(cx, object)?.unwrap();

            let struct_name = match &obj_tty {
                TurboTy::Struct(name) => name.clone(),
                _ => {
                    return Err(CodegenError {
                        code: ErrorCode::E0400,
                        message: format!("field access on non-struct type: {field}"),
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

            let (field_index, (_, field_tty)) = struct_layout
                .iter()
                .enumerate()
                .find(|(_, (n, _))| n == field)
                .ok_or_else(|| CodegenError {
                    code: ErrorCode::E0400,
                    message: format!("struct `{struct_name}` has no field `{field}`"),
                })?;

            // Check if we have concrete field types (from generic struct instantiation)
            let concrete_tty = if let Expr::Ident(ref var_name) = object.node {
                cx.concrete_struct_fields.get(var_name).and_then(|fields| {
                    fields
                        .iter()
                        .find(|(n, _)| n == field)
                        .map(|(_, t)| t.clone())
                })
            } else {
                None
            };
            let field_tty = concrete_tty.unwrap_or_else(|| field_tty.clone());

            let offset = field_index as u64 * 8;
            let obj_ptr = obj.into_pointer_value();

            let field_ptr = unsafe {
                cx.builder
                    .build_gep(
                        cx.context.i8_type(),
                        obj_ptr,
                        &[cx.context.i64_type().const_int(offset, false)],
                        "field_ptr",
                    )
                    .expect("build_gep failed")
            };

            // Load as i64 then narrow to the field type
            let raw = cx
                .builder
                .build_load(cx.context.i64_type(), field_ptr, field)
                .expect("build_load failed");
            let result = narrow_from_storage(cx, raw, &field_tty);
            Ok(Some((result, field_tty)))
        }

        Expr::EnumVariant { enum_name, variant } => {
            let variants = cx
                .enum_variants
                .get(enum_name)
                .ok_or_else(|| CodegenError {
                    code: ErrorCode::E0400,
                    message: format!("undefined enum: {enum_name}"),
                })?;
            let variant_index =
                variants
                    .iter()
                    .position(|v| v == variant)
                    .ok_or_else(|| CodegenError {
                        code: ErrorCode::E0400,
                        message: format!("enum `{enum_name}` has no variant `{variant}`"),
                    })?;
            let val = cx.context.i64_type().const_int(variant_index as u64, false);
            Ok(Some((val.into(), TurboTy::Enum(enum_name.clone()))))
        }

        Expr::Match { subject, arms } => compile_match(cx, subject, arms),

        Expr::Interpolation(parts) => {
            let empty_str = cx.create_string("")?;
            let mut result: BasicValueEnum<'ctx> = empty_str.into();

            for part in parts {
                match part {
                    InterpolPart::Lit(s) => {
                        let lit_ptr = cx.create_string(s)?;
                        result = cx
                            .rt_call("rt_str_concat", &[result.into(), lit_ptr.into()])
                            .unwrap();
                    }
                    InterpolPart::Expr(e) => {
                        let (val, tty) = compile_expr(cx, e)?.unwrap();
                        let str_val = convert_to_str(cx, val, &tty)?;
                        result = cx
                            .rt_call("rt_str_concat", &[result.into(), str_val.into()])
                            .unwrap();
                    }
                }
            }

            Ok(Some((result, TurboTy::Str)))
        }

        Expr::Range { start, end } => {
            // Ranges are only used inside for-in, but if used standalone, return a tuple-like thing
            let (start_val, _) = compile_expr(cx, start)?.unwrap();
            let (end_val, _) = compile_expr(cx, end)?.unwrap();
            // Store as array [start, end]
            let len_val = cx.context.i64_type().const_int(2, false);
            let arr = cx.rt_call("rt_array_alloc", &[len_val.into()]).unwrap();
            let idx0 = cx.context.i64_type().const_int(0, false);
            let idx1 = cx.context.i64_type().const_int(1, false);
            cx.rt_call("rt_array_set", &[arr.into(), idx0.into(), start_val.into()]);
            cx.rt_call("rt_array_set", &[arr.into(), idx1.into(), end_val.into()]);
            Ok(Some((arr, TurboTy::Array(Box::new(TurboTy::Int)))))
        }

        Expr::OkExpr(inner) => {
            let (val, _) = compile_expr(cx, inner)?.unwrap();
            let val_i64 = widen_for_storage(cx, val);
            let result = cx.rt_call("rt_result_ok", &[val_i64.into()]).unwrap();
            Ok(Some((
                result,
                TurboTy::Result(Box::new(TurboTy::Int), Box::new(TurboTy::Str)),
            )))
        }

        Expr::ErrExpr(inner) => {
            let (val, _) = compile_expr(cx, inner)?.unwrap();
            let val_i64 = widen_for_storage(cx, val);
            let result = cx.rt_call("rt_result_err", &[val_i64.into()]).unwrap();
            Ok(Some((
                result,
                TurboTy::Result(Box::new(TurboTy::Int), Box::new(TurboTy::Str)),
            )))
        }

        Expr::SomeExpr(inner) => {
            let (val, _) = compile_expr(cx, inner)?.unwrap();
            let val_i64 = widen_for_storage(cx, val);
            let result = cx.rt_call("rt_option_some", &[val_i64.into()]).unwrap();
            Ok(Some((result, TurboTy::Optional(Box::new(TurboTy::Int)))))
        }

        Expr::NoneExpr => {
            let result = cx.rt_call("rt_option_none", &[]).unwrap();
            Ok(Some((result, TurboTy::Optional(Box::new(TurboTy::Int)))))
        }

        Expr::NullCoalesce { value, default } => {
            let (val, val_tty) = compile_expr(cx, value)?.unwrap();
            // Get the tag
            let tag = cx
                .rt_call("rt_option_tag", &[val.into()])
                .unwrap()
                .into_int_value();
            let zero = cx.context.i64_type().const_int(0, false);
            let is_none = cx
                .builder
                .build_int_compare(IntPredicate::EQ, tag, zero, "is_none")
                .expect("build_int_compare failed");

            let then_block = cx
                .context
                .append_basic_block(cx.current_fn, "coalesce_none");
            let else_block = cx
                .context
                .append_basic_block(cx.current_fn, "coalesce_some");
            let merge_block = cx
                .context
                .append_basic_block(cx.current_fn, "coalesce_merge");

            cx.builder
                .build_conditional_branch(is_none, then_block, else_block)
                .expect("build_conditional_branch failed");

            // None case: use default
            cx.builder.position_at_end(then_block);
            let (default_val, default_tty) = compile_expr(cx, default)?.unwrap();
            let then_end_block = cx.builder.get_insert_block().unwrap();
            cx.builder
                .build_unconditional_branch(merge_block)
                .expect("build_unconditional_branch failed");

            // Some case: unwrap
            cx.builder.position_at_end(else_block);
            let unwrapped = cx.rt_call("rt_option_value", &[val.into()]).unwrap();
            let inner_tty = match &val_tty {
                TurboTy::Optional(inner) => *inner.clone(),
                _ => TurboTy::Int,
            };
            let unwrapped = narrow_from_storage(cx, unwrapped, &inner_tty);
            let else_end_block = cx.builder.get_insert_block().unwrap();
            cx.builder
                .build_unconditional_branch(merge_block)
                .expect("build_unconditional_branch failed");

            cx.builder.position_at_end(merge_block);
            let phi = cx
                .builder
                .build_phi(default_val.get_type(), "coalesce")
                .expect("build_phi failed");
            phi.add_incoming(&[(&default_val, then_end_block), (&unwrapped, else_end_block)]);

            Ok(Some((phi.as_basic_value(), default_tty)))
        }

        Expr::OptionalChain { .. } => {
            // TODO: implement optional chaining in LLVM backend
            Err(CodegenError {
                code: ErrorCode::E0400,
                message: "optional chaining `?.` is not yet supported in the LLVM backend"
                    .to_string(),
            })
        }

        Expr::Closure { params, .. } => {
            // Look up the pre-extracted closure function by span start
            let span_start = expr.span.start;
            let (closure_name, closure_ty, free_vars) = cx
                .closure_fns
                .get(&span_start)
                .ok_or_else(|| CodegenError {
                    code: ErrorCode::E0400,
                    message: "internal error: closure not found in pre-compiled map".to_string(),
                })?
                .clone();

            let func = *cx
                .user_fns
                .get(closure_name.as_str())
                .ok_or_else(|| CodegenError {
                    code: ErrorCode::E0400,
                    message: format!(
                        "internal error: closure function {} not found",
                        closure_name
                    ),
                })?;

            // Get the function pointer as an i64 (pointer-sized integer)
            let fn_ptr = func.as_global_value().as_pointer_value();

            // Determine captures: free variables that actually exist in scope
            let mut bound_params: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
            let mut all_free: Vec<String> = Vec::new();
            collect_free_vars_llvm(&expr.node, &mut bound_params, &mut all_free);
            let capture_names: Vec<String> = free_vars
                .iter()
                .filter(|n| cx.vars.contains_key(*n))
                .cloned()
                .collect();

            let ptr_type = cx.context.ptr_type(AddressSpace::default());
            let i8_type = cx.context.i8_type();
            let i64_type = cx.context.i64_type();

            // Allocate environment struct for captured variables
            let env_ptr = if !capture_names.is_empty() {
                let num_captures = i64_type.const_int(capture_names.len() as u64, false);
                let env_ptr = cx
                    .rt_call("rt_struct_alloc", &[num_captures.into()])
                    .unwrap()
                    .into_pointer_value();

                // Store each captured variable into the env struct
                for (cap_idx, cap_name) in capture_names.iter().enumerate() {
                    let (alloca, cap_tty) = cx
                        .vars
                        .get(cap_name)
                        .ok_or_else(|| CodegenError {
                            code: ErrorCode::E0400,
                            message: format!("capture variable {} not found", cap_name),
                        })?
                        .clone();
                    let val = cx
                        .builder
                        .build_load(
                            turbo_ty_to_llvm_ctx(&cap_tty, cx.context, cx.enum_max_slots),
                            alloca,
                            cap_name,
                        )
                        .expect("build_load failed");
                    let val_i64 = widen_for_storage(cx, val);
                    let offset = (cap_idx as u64) * 8;
                    let field_ptr = unsafe {
                        cx.builder
                            .build_gep(
                                i8_type,
                                env_ptr,
                                &[i64_type.const_int(offset, false)],
                                "cap_ptr",
                            )
                            .expect("build_gep failed")
                    };
                    cx.builder
                        .build_store(field_ptr, val_i64)
                        .expect("build_store failed");
                }
                env_ptr.into()
            } else {
                // No captures: null pointer
                ptr_type.const_null().into()
            };

            // Allocate closure pair: [fn_ptr_as_i64, env_ptr_as_i64]
            let two = i64_type.const_int(2, false);
            let closure_ptr = cx
                .rt_call("rt_struct_alloc", &[two.into()])
                .unwrap()
                .into_pointer_value();

            // Store fn_ptr at slot 0 (as i64)
            let fn_ptr_i64 = cx
                .builder
                .build_ptr_to_int(fn_ptr, i64_type, "fn_ptr_i64")
                .expect("build_ptr_to_int failed");
            cx.builder
                .build_store(closure_ptr, fn_ptr_i64)
                .expect("build_store failed");

            // Store env_ptr at slot 1 (offset 8)
            let env_slot = unsafe {
                cx.builder
                    .build_gep(
                        i8_type,
                        closure_ptr,
                        &[i64_type.const_int(8, false)],
                        "env_slot",
                    )
                    .expect("build_gep failed")
            };
            let env_i64: BasicValueEnum = match env_ptr {
                BasicValueEnum::PointerValue(pv) => cx
                    .builder
                    .build_ptr_to_int(pv, i64_type, "env_i64")
                    .expect("pti")
                    .into(),
                other => other,
            };
            cx.builder
                .build_store(env_slot, env_i64)
                .expect("build_store failed");

            Ok(Some((closure_ptr.into(), closure_ty)))
        }

        Expr::Await(inner) => {
            let result = compile_expr(cx, inner)?;
            if let Some((val, tty)) = result {
                match tty {
                    TurboTy::Future(inner_tty) => {
                        let joined = cx.rt_call("rt_await_handle", &[val.into()]).unwrap();
                        let narrowed = narrow_from_storage(cx, joined, &inner_tty);
                        Ok(Some((narrowed, *inner_tty)))
                    }
                    _ => Ok(Some((val, tty))),
                }
            } else {
                Ok(None)
            }
        }

        Expr::Spawn(inner) => {
            let span_start = expr.span.start;
            if let Some(thunk_name) = cx.spawn_thunks.get(&span_start).cloned() {
                if let Expr::Call { callee, args } = &inner.node {
                    if let Expr::Ident(callee_name) = &callee.node {
                        let inner_ret_tty = cx
                            .fn_ret_types
                            .get(callee_name.as_str())
                            .cloned()
                            .unwrap_or(TurboTy::Unit);

                        let target_func =
                            *cx.user_fns
                                .get(callee_name.as_str())
                                .ok_or_else(|| CodegenError {
                                    code: ErrorCode::E0402,
                                    message: format!("spawn: unknown function `{}`", callee_name),
                                })?;
                        let target_fn_ptr = target_func.as_global_value().as_pointer_value();

                        // Compile all arguments
                        let mut arg_vals: Vec<BasicValueEnum> = Vec::new();
                        for arg in args {
                            if let Some((val, _tty)) = compile_expr(cx, arg)? {
                                let val_i64 = widen_for_storage(cx, val);
                                arg_vals.push(val_i64.into());
                            }
                        }

                        let i8_type = cx.context.i8_type();
                        let i64_type = cx.context.i64_type();
                        let ptr_type = cx.context.ptr_type(AddressSpace::default());

                        // Allocate args struct: [fn_ptr, arg0, arg1, ...]
                        let num_slots = i64_type.const_int((1 + arg_vals.len()) as u64, false);
                        let args_ptr = cx
                            .rt_call("rt_struct_alloc", &[num_slots.into()])
                            .unwrap()
                            .into_pointer_value();

                        // Store fn_ptr at offset 0
                        let fn_ptr_i64 = cx
                            .builder
                            .build_ptr_to_int(target_fn_ptr, i64_type, "spawn_fn_i64")
                            .expect("pti");
                        cx.builder.build_store(args_ptr, fn_ptr_i64).expect("store");

                        // Store args at offsets 8, 16, 24, ...
                        for (i, val) in arg_vals.iter().enumerate() {
                            let offset = ((i + 1) * 8) as u64;
                            let slot = unsafe {
                                cx.builder
                                    .build_gep(
                                        i8_type,
                                        args_ptr,
                                        &[i64_type.const_int(offset, false)],
                                        "arg_slot",
                                    )
                                    .expect("gep")
                            };
                            cx.builder.build_store(slot, *val).expect("store");
                        }

                        // Get the thunk function address
                        let thunk_func =
                            *cx.user_fns
                                .get(thunk_name.as_str())
                                .ok_or_else(|| CodegenError {
                                    code: ErrorCode::E0405,
                                    message: format!("spawn: thunk `{}` not found", thunk_name),
                                })?;
                        let thunk_fn_ptr = thunk_func.as_global_value().as_pointer_value();

                        // rt_spawn_with_args(thunk_ptr: ptr, args_ptr: ptr) -> ptr (handle)
                        let handle = cx
                            .rt_call(
                                "rt_spawn_with_args",
                                &[thunk_fn_ptr.into(), args_ptr.into()],
                            )
                            .unwrap();

                        return Ok(Some((handle, TurboTy::Future(Box::new(inner_ret_tty)))));
                    }
                }
            }
            // Fallback: compile inner expression synchronously
            compile_expr(cx, inner)
        }

        Expr::Try(inner) => {
            let (val, val_tty) = compile_expr(cx, inner)?.unwrap();
            // Check tag: 0 = Ok, 1 = Err
            let tag = cx
                .rt_call("rt_result_tag", &[val.into()])
                .unwrap()
                .into_int_value();
            let one = cx.context.i64_type().const_int(1, false);
            let is_err = cx
                .builder
                .build_int_compare(IntPredicate::EQ, tag, one, "is_err")
                .expect("build_int_compare failed");

            let err_block = cx.context.append_basic_block(cx.current_fn, "try_err");
            let ok_block = cx.context.append_basic_block(cx.current_fn, "try_ok");

            cx.builder
                .build_conditional_branch(is_err, err_block, ok_block)
                .expect("build_conditional_branch failed");

            // Error path: propagate
            cx.builder.position_at_end(err_block);
            let err_val = cx.rt_call("rt_result_value", &[val.into()]).unwrap();
            let err_result = cx.rt_call("rt_result_err", &[err_val.into()]).unwrap();
            cx.builder
                .build_return(Some(&err_result))
                .expect("build_return failed");

            // Ok path: unwrap
            cx.builder.position_at_end(ok_block);
            let ok_val = cx.rt_call("rt_result_value", &[val.into()]).unwrap();
            let inner_tty = match &val_tty {
                TurboTy::Result(ok, _) => *ok.clone(),
                _ => TurboTy::Int,
            };
            let narrowed = narrow_from_storage(cx, ok_val, &inner_tty);
            Ok(Some((narrowed, inner_tty)))
        }

        Expr::Break => {
            if let Some((_, exit_block)) = cx.loop_stack.last() {
                cx.builder
                    .build_unconditional_branch(*exit_block)
                    .expect("build_unconditional_branch failed");
                // Create unreachable block for subsequent code
                let dead_block = cx.context.append_basic_block(cx.current_fn, "after_break");
                cx.builder.position_at_end(dead_block);
            }
            Ok(None)
        }

        Expr::Continue => {
            if let Some((header_block, _)) = cx.loop_stack.last() {
                cx.builder
                    .build_unconditional_branch(*header_block)
                    .expect("build_unconditional_branch failed");
                let dead_block = cx
                    .context
                    .append_basic_block(cx.current_fn, "after_continue");
                cx.builder.position_at_end(dead_block);
            }
            Ok(None)
        }

        Expr::MapLit(entries) => {
            let map = cx.rt_call("rt_hashmap_new", &[]).unwrap();
            for (key, value) in entries {
                let (k, _) = compile_expr(cx, key)?.unwrap();
                let (v, _) = compile_expr(cx, value)?.unwrap();
                cx.rt_call("rt_hashmap_set", &[map.into(), k.into(), v.into()]);
            }
            Ok(Some((map, TurboTy::Int)))
        }
    }
}

// ── Binary operations ───────────────────────────────────────────────

pub(crate) fn compile_binop<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    lhs: BasicValueEnum<'ctx>,
    op: BinOp,
    rhs: BasicValueEnum<'ctx>,
) -> Result<BasicValueEnum<'ctx>, CodegenError> {
    // Float operations
    if let (BasicValueEnum::FloatValue(lf), BasicValueEnum::FloatValue(rf)) = (lhs, rhs) {
        let result = match op {
            BinOp::Add => cx
                .builder
                .build_float_add(lf, rf, "fadd")
                .expect("build_float_add failed")
                .into(),
            BinOp::Sub => cx
                .builder
                .build_float_sub(lf, rf, "fsub")
                .expect("build_float_sub failed")
                .into(),
            BinOp::Mul => cx
                .builder
                .build_float_mul(lf, rf, "fmul")
                .expect("build_float_mul failed")
                .into(),
            BinOp::Div => cx
                .builder
                .build_float_div(lf, rf, "fdiv")
                .expect("build_float_div failed")
                .into(),
            BinOp::Mod => cx
                .builder
                .build_float_rem(lf, rf, "fmod")
                .expect("build_float_rem failed")
                .into(),
            BinOp::Eq => cx
                .builder
                .build_float_compare(FloatPredicate::OEQ, lf, rf, "feq")
                .expect("build_float_compare failed")
                .into(),
            BinOp::NotEq => cx
                .builder
                .build_float_compare(FloatPredicate::ONE, lf, rf, "fneq")
                .expect("build_float_compare failed")
                .into(),
            BinOp::Less => cx
                .builder
                .build_float_compare(FloatPredicate::OLT, lf, rf, "flt")
                .expect("build_float_compare failed")
                .into(),
            BinOp::LessEq => cx
                .builder
                .build_float_compare(FloatPredicate::OLE, lf, rf, "fle")
                .expect("build_float_compare failed")
                .into(),
            BinOp::Greater => cx
                .builder
                .build_float_compare(FloatPredicate::OGT, lf, rf, "fgt")
                .expect("build_float_compare failed")
                .into(),
            BinOp::GreaterEq => cx
                .builder
                .build_float_compare(FloatPredicate::OGE, lf, rf, "fge")
                .expect("build_float_compare failed")
                .into(),
            _ => {
                return Err(CodegenError {
                    code: ErrorCode::E0403,
                    message: format!("unsupported float op: {op:?}"),
                })
            }
        };
        // Widen i1 comparison results to i8 for consistent Bool representation
        let result = widen_i1_to_i8(cx, result);
        return Ok(result);
    }

    // Integer operations
    let li = lhs.into_int_value();
    let ri = rhs.into_int_value();

    // Widen mismatched widths
    let (li, ri) = if li.get_type().get_bit_width() != ri.get_type().get_bit_width() {
        let target_bits = li
            .get_type()
            .get_bit_width()
            .max(ri.get_type().get_bit_width());
        let target_type = cx.context.custom_width_int_type(target_bits);
        let li = if li.get_type().get_bit_width() < target_bits {
            cx.builder
                .build_int_s_extend(li, target_type, "sext")
                .expect("build_int_s_extend failed")
        } else {
            li
        };
        let ri = if ri.get_type().get_bit_width() < target_bits {
            cx.builder
                .build_int_s_extend(ri, target_type, "sext")
                .expect("build_int_s_extend failed")
        } else {
            ri
        };
        (li, ri)
    } else {
        (li, ri)
    };

    let result: BasicValueEnum = match op {
        BinOp::Add => cx
            .builder
            .build_int_add(li, ri, "iadd")
            .expect("build_int_add failed")
            .into(),
        BinOp::Sub => cx
            .builder
            .build_int_sub(li, ri, "isub")
            .expect("build_int_sub failed")
            .into(),
        BinOp::Mul => cx
            .builder
            .build_int_mul(li, ri, "imul")
            .expect("build_int_mul failed")
            .into(),
        BinOp::Div => {
            emit_div_zero_check(cx, ri);
            cx.builder
                .build_int_signed_div(li, ri, "sdiv")
                .expect("build_int_signed_div failed")
                .into()
        }
        BinOp::Mod => {
            emit_div_zero_check(cx, ri);
            cx.builder
                .build_int_signed_rem(li, ri, "srem")
                .expect("build_int_signed_rem failed")
                .into()
        }
        BinOp::Eq => cx
            .builder
            .build_int_compare(IntPredicate::EQ, li, ri, "ieq")
            .expect("build_int_compare failed")
            .into(),
        BinOp::NotEq => cx
            .builder
            .build_int_compare(IntPredicate::NE, li, ri, "ineq")
            .expect("build_int_compare failed")
            .into(),
        BinOp::Less => cx
            .builder
            .build_int_compare(IntPredicate::SLT, li, ri, "ilt")
            .expect("build_int_compare failed")
            .into(),
        BinOp::LessEq => cx
            .builder
            .build_int_compare(IntPredicate::SLE, li, ri, "ile")
            .expect("build_int_compare failed")
            .into(),
        BinOp::Greater => cx
            .builder
            .build_int_compare(IntPredicate::SGT, li, ri, "igt")
            .expect("build_int_compare failed")
            .into(),
        BinOp::GreaterEq => cx
            .builder
            .build_int_compare(IntPredicate::SGE, li, ri, "ige")
            .expect("build_int_compare failed")
            .into(),
        BinOp::And => cx
            .builder
            .build_and(li, ri, "and")
            .expect("build_and failed")
            .into(),
        BinOp::Or => cx
            .builder
            .build_or(li, ri, "or")
            .expect("build_or failed")
            .into(),
    };
    // Widen i1 comparison results to i8 for consistent Bool representation
    let result = widen_i1_to_i8(cx, result);
    Ok(result)
}

/// If a value is i1 (LLVM comparison result), widen it to i8 (Turbo Bool type).
pub(crate) fn widen_i1_to_i8<'a, 'ctx>(
    cx: &Ctx<'a, 'ctx>,
    val: BasicValueEnum<'ctx>,
) -> BasicValueEnum<'ctx> {
    if let BasicValueEnum::IntValue(iv) = val {
        if iv.get_type().get_bit_width() == 1 {
            return cx
                .builder
                .build_int_z_extend(iv, cx.context.i8_type(), "i1_to_i8")
                .expect("build_int_z_extend failed")
                .into();
        }
    }
    val
}

fn emit_div_zero_check<'a, 'ctx>(cx: &mut Ctx<'a, 'ctx>, divisor: IntValue<'ctx>) {
    let zero = divisor.get_type().const_int(0, false);
    let is_zero = cx
        .builder
        .build_int_compare(IntPredicate::EQ, divisor, zero, "divzero")
        .expect("build_int_compare failed");

    let trap_block = cx.context.append_basic_block(cx.current_fn, "div_trap");
    let ok_block = cx.context.append_basic_block(cx.current_fn, "div_ok");

    cx.builder
        .build_conditional_branch(is_zero, trap_block, ok_block)
        .expect("build_conditional_branch failed");

    cx.builder.position_at_end(trap_block);
    cx.rt_call("rt_div_by_zero", &[]);
    cx.builder
        .build_unreachable()
        .expect("build_unreachable failed");

    cx.builder.position_at_end(ok_block);
}

// ── Short-circuit && / || ───────────────────────────────────────────

fn compile_short_circuit<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    left: &Spanned<Expr>,
    op: BinOp,
    right: &Spanned<Expr>,
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    let (lhs, _) = compile_expr(cx, left)?.unwrap();
    let lhs_bool = cx.to_bool(lhs);

    let eval_rhs_block = cx.context.append_basic_block(cx.current_fn, "sc_rhs");
    let merge_block = cx.context.append_basic_block(cx.current_fn, "sc_merge");

    let current_block = cx.builder.get_insert_block().unwrap();

    match op {
        BinOp::And => {
            cx.builder
                .build_conditional_branch(lhs_bool, eval_rhs_block, merge_block)
                .expect("build_conditional_branch failed");
        }
        BinOp::Or => {
            cx.builder
                .build_conditional_branch(lhs_bool, merge_block, eval_rhs_block)
                .expect("build_conditional_branch failed");
        }
        _ => unreachable!(),
    }

    cx.builder.position_at_end(eval_rhs_block);
    let (rhs, _) = compile_expr(cx, right)?.unwrap();
    let rhs_bool = cx.to_bool(rhs);
    let rhs_end_block = cx.builder.get_insert_block().unwrap();
    cx.builder
        .build_unconditional_branch(merge_block)
        .expect("build_unconditional_branch failed");

    cx.builder.position_at_end(merge_block);
    let phi = cx
        .builder
        .build_phi(cx.context.bool_type(), "sc_result")
        .expect("build_phi failed");

    match op {
        BinOp::And => {
            let false_val = cx.context.bool_type().const_int(0, false);
            phi.add_incoming(&[(&false_val, current_block), (&rhs_bool, rhs_end_block)]);
        }
        BinOp::Or => {
            let true_val = cx.context.bool_type().const_int(1, false);
            phi.add_incoming(&[(&true_val, current_block), (&rhs_bool, rhs_end_block)]);
        }
        _ => unreachable!(),
    }

    // Widen i1 back to i8 for consistent Bool representation
    let result = cx
        .builder
        .build_int_z_extend(
            phi.as_basic_value().into_int_value(),
            cx.context.i8_type(),
            "sc_zext",
        )
        .expect("build_int_z_extend failed");
    Ok(Some((result.into(), TurboTy::Bool)))
}

// ── If/else ─────────────────────────────────────────────────────────

fn compile_if<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    condition: &Spanned<Expr>,
    then_branch: &Spanned<Expr>,
    else_branch: Option<&Spanned<Expr>>,
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    let (cond, _) = compile_expr(cx, condition)?.unwrap();
    let cond_bool = cx.to_bool(cond);

    let then_block = cx.context.append_basic_block(cx.current_fn, "then");
    let else_block = cx.context.append_basic_block(cx.current_fn, "else");
    let merge_block = cx.context.append_basic_block(cx.current_fn, "ifmerge");

    cx.builder
        .build_conditional_branch(cond_bool, then_block, else_block)
        .expect("build_conditional_branch failed");

    // Then branch
    cx.builder.position_at_end(then_block);
    let then_result = compile_expr(cx, then_branch)?;
    let then_end_block = cx.builder.get_insert_block().unwrap();
    let then_needs_jump = then_end_block.get_terminator().is_none();
    if then_needs_jump {
        cx.builder
            .build_unconditional_branch(merge_block)
            .expect("build_unconditional_branch failed");
    }

    // Else branch
    cx.builder.position_at_end(else_block);
    let else_result = if let Some(else_expr) = else_branch {
        compile_expr(cx, else_expr)?
    } else {
        None
    };
    let else_end_block = cx.builder.get_insert_block().unwrap();
    let else_needs_jump = else_end_block.get_terminator().is_none();
    if else_needs_jump {
        cx.builder
            .build_unconditional_branch(merge_block)
            .expect("build_unconditional_branch failed");
    }

    // Merge block
    cx.builder.position_at_end(merge_block);

    if let (Some((then_val, then_tty)), Some((else_val, _))) = (then_result, else_result) {
        if then_needs_jump && else_needs_jump {
            let phi = cx
                .builder
                .build_phi(then_val.get_type(), "ifphi")
                .expect("build_phi failed");
            phi.add_incoming(&[(&then_val, then_end_block), (&else_val, else_end_block)]);
            Ok(Some((phi.as_basic_value(), then_tty)))
        } else if then_needs_jump {
            // Only then branch reaches merge
            Ok(Some((then_val, then_tty)))
        } else if else_needs_jump {
            // Only else branch reaches merge
            Ok(Some((else_val, then_tty)))
        } else {
            // Neither branch reaches merge (both return/break)
            Ok(None)
        }
    } else {
        Ok(None)
    }
}

// ── While loop ──────────────────────────────────────────────────────

fn compile_while<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    condition: &Spanned<Expr>,
    body: &Spanned<Expr>,
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    let header_block = cx.context.append_basic_block(cx.current_fn, "while_header");
    let body_block = cx.context.append_basic_block(cx.current_fn, "while_body");
    let exit_block = cx.context.append_basic_block(cx.current_fn, "while_exit");

    cx.builder
        .build_unconditional_branch(header_block)
        .expect("build_unconditional_branch failed");

    cx.builder.position_at_end(header_block);
    let (cond, _) = compile_expr(cx, condition)?.unwrap();
    let cond_bool = cx.to_bool(cond);
    cx.builder
        .build_conditional_branch(cond_bool, body_block, exit_block)
        .expect("build_conditional_branch failed");

    cx.builder.position_at_end(body_block);
    cx.loop_stack.push((header_block, exit_block));
    compile_expr(cx, body)?;
    cx.loop_stack.pop();

    let body_end = cx.builder.get_insert_block().unwrap();
    if body_end.get_terminator().is_none() {
        cx.builder
            .build_unconditional_branch(header_block)
            .expect("build_unconditional_branch failed");
    }

    cx.builder.position_at_end(exit_block);
    Ok(None)
}

// ── For-in loop ─────────────────────────────────────────────────────

fn compile_for_in<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    var_name: &str,
    iterable: &Spanned<Expr>,
    body: &Spanned<Expr>,
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    match &iterable.node {
        Expr::Range { start, end } => compile_for_in_range(cx, var_name, start, end, body),
        _ => compile_for_in_array(cx, var_name, iterable, body),
    }
}

fn compile_for_in_range<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    var_name: &str,
    start: &Spanned<Expr>,
    end: &Spanned<Expr>,
    body: &Spanned<Expr>,
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    let (range_start, _) = compile_expr(cx, start)?.unwrap();
    let (range_end, _) = compile_expr(cx, end)?.unwrap();

    let alloca = cx.create_entry_block_alloca(cx.context.i64_type().into(), var_name);
    cx.builder
        .build_store(alloca, range_start)
        .expect("build_store failed");
    cx.vars.insert(var_name.to_string(), (alloca, TurboTy::Int));

    let header_block = cx.context.append_basic_block(cx.current_fn, "forin_header");
    let body_block = cx.context.append_basic_block(cx.current_fn, "forin_body");
    let continue_block = cx
        .context
        .append_basic_block(cx.current_fn, "forin_continue");
    let exit_block = cx.context.append_basic_block(cx.current_fn, "forin_exit");

    cx.builder
        .build_unconditional_branch(header_block)
        .expect("build_unconditional_branch failed");

    cx.builder.position_at_end(header_block);
    let current_i = cx
        .builder
        .build_load(cx.context.i64_type(), alloca, "i")
        .expect("build_load failed")
        .into_int_value();
    let range_end_i = range_end.into_int_value();
    let cond = cx
        .builder
        .build_int_compare(IntPredicate::SLT, current_i, range_end_i, "forin_cond")
        .expect("build_int_compare failed");
    cx.builder
        .build_conditional_branch(cond, body_block, exit_block)
        .expect("build_conditional_branch failed");

    cx.builder.position_at_end(body_block);
    cx.loop_stack.push((continue_block, exit_block));
    compile_expr(cx, body)?;
    cx.loop_stack.pop();

    let body_end = cx.builder.get_insert_block().unwrap();
    if body_end.get_terminator().is_none() {
        cx.builder
            .build_unconditional_branch(continue_block)
            .expect("build_unconditional_branch failed");
    }

    cx.builder.position_at_end(continue_block);
    let updated_i = cx
        .builder
        .build_load(cx.context.i64_type(), alloca, "i_cur")
        .expect("build_load failed")
        .into_int_value();
    let one = cx.context.i64_type().const_int(1, false);
    let next_i = cx
        .builder
        .build_int_add(updated_i, one, "next_i")
        .expect("build_int_add failed");
    cx.builder
        .build_store(alloca, next_i)
        .expect("build_store failed");
    cx.builder
        .build_unconditional_branch(header_block)
        .expect("build_unconditional_branch failed");

    cx.builder.position_at_end(exit_block);
    Ok(None)
}

fn compile_for_in_array<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    var_name: &str,
    iterable: &Spanned<Expr>,
    body: &Spanned<Expr>,
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    let (arr, arr_tty) = compile_expr(cx, iterable)?.unwrap();
    let elem_tty = match &arr_tty {
        TurboTy::Array(inner) => *inner.clone(),
        _ => TurboTy::Int,
    };

    let arr_len = cx
        .rt_call("rt_array_len", &[arr.into()])
        .unwrap()
        .into_int_value();

    // Index counter
    let idx_alloca = cx.create_entry_block_alloca(cx.context.i64_type().into(), "__forin_idx");
    cx.builder
        .build_store(idx_alloca, cx.context.i64_type().const_int(0, false))
        .expect("build_store failed");

    // Loop variable
    let elem_llvm_ty = turbo_ty_to_llvm_ctx(&elem_tty, cx.context, cx.enum_max_slots);
    let var_alloca = cx.create_entry_block_alloca(elem_llvm_ty, var_name);
    cx.vars
        .insert(var_name.to_string(), (var_alloca, elem_tty.clone()));

    let header_block = cx
        .context
        .append_basic_block(cx.current_fn, "forin_arr_header");
    let body_block = cx
        .context
        .append_basic_block(cx.current_fn, "forin_arr_body");
    let continue_block = cx
        .context
        .append_basic_block(cx.current_fn, "forin_arr_continue");
    let exit_block = cx
        .context
        .append_basic_block(cx.current_fn, "forin_arr_exit");

    cx.builder
        .build_unconditional_branch(header_block)
        .expect("build_unconditional_branch failed");

    cx.builder.position_at_end(header_block);
    let idx = cx
        .builder
        .build_load(cx.context.i64_type(), idx_alloca, "idx")
        .expect("build_load failed")
        .into_int_value();
    let cond = cx
        .builder
        .build_int_compare(IntPredicate::SLT, idx, arr_len, "forin_arr_cond")
        .expect("build_int_compare failed");
    cx.builder
        .build_conditional_branch(cond, body_block, exit_block)
        .expect("build_conditional_branch failed");

    cx.builder.position_at_end(body_block);
    // Load element
    let idx2 = cx
        .builder
        .build_load(cx.context.i64_type(), idx_alloca, "idx")
        .expect("build_load failed");
    let raw_elem = cx
        .rt_call("rt_array_get", &[arr.into(), idx2.into()])
        .unwrap();
    let elem = narrow_from_storage(cx, raw_elem, &elem_tty);
    cx.builder
        .build_store(var_alloca, elem)
        .expect("build_store failed");

    cx.loop_stack.push((continue_block, exit_block));
    compile_expr(cx, body)?;
    cx.loop_stack.pop();

    let body_end = cx.builder.get_insert_block().unwrap();
    if body_end.get_terminator().is_none() {
        cx.builder
            .build_unconditional_branch(continue_block)
            .expect("build_unconditional_branch failed");
    }

    cx.builder.position_at_end(continue_block);
    let idx3 = cx
        .builder
        .build_load(cx.context.i64_type(), idx_alloca, "idx")
        .expect("build_load failed")
        .into_int_value();
    let one = cx.context.i64_type().const_int(1, false);
    let next = cx
        .builder
        .build_int_add(idx3, one, "next_idx")
        .expect("build_int_add failed");
    cx.builder
        .build_store(idx_alloca, next)
        .expect("build_store failed");
    cx.builder
        .build_unconditional_branch(header_block)
        .expect("build_unconditional_branch failed");

    cx.builder.position_at_end(exit_block);
    Ok(None)
}

// ── Match expression ────────────────────────────────────────────────

fn compile_match<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    subject: &Spanned<Expr>,
    arms: &[MatchArm],
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    let (subject_val, subject_tty) = compile_expr(cx, subject)?.unwrap();

    let merge_block = cx.context.append_basic_block(cx.current_fn, "match_merge");

    let mut arm_blocks: Vec<inkwell::basic_block::BasicBlock<'ctx>> = Vec::new();
    for i in 0..arms.len() {
        arm_blocks.push(
            cx.context
                .append_basic_block(cx.current_fn, &format!("match_arm_{i}")),
        );
    }
    let default_block = cx
        .context
        .append_basic_block(cx.current_fn, "match_default");

    // Build chain of comparisons
    let mut phi_incoming: Vec<(BasicValueEnum<'ctx>, inkwell::basic_block::BasicBlock<'ctx>)> =
        Vec::new();
    let mut first_arm_tty: Option<TurboTy> = None;

    for (i, arm) in arms.iter().enumerate() {
        let arm_block = arm_blocks[i];
        let next_block = if i + 1 < arms.len() {
            cx.context
                .append_basic_block(cx.current_fn, &format!("match_test_{}", i + 1))
        } else {
            default_block
        };

        // Only branch from the first test block for the first arm
        if i == 0 {
            // We're still at the end of the block before the match
        }

        // Build test
        let matches = match &arm.pattern.node {
            Pattern::Wildcard => None,
            Pattern::Ident(name) => {
                // Check if this ident is an enum variant name
                let variant_tag = lookup_variant_tag(cx.enum_variants, name);
                if let Some(tag_val) = variant_tag {
                    let pat_val = cx.context.i64_type().const_int(tag_val as u64, false);
                    if let TurboTy::Enum(ref enum_name) = subject_tty {
                        if cx.enum_max_slots.contains_key(enum_name) {
                            // Data enum: load tag from ptr
                            let ptr = subject_val.into_pointer_value();
                            let tag = cx
                                .builder
                                .build_load(cx.context.i64_type(), ptr, "tag")
                                .expect("build_load failed")
                                .into_int_value();
                            Some(
                                cx.builder
                                    .build_int_compare(IntPredicate::EQ, tag, pat_val, "var_eq")
                                    .expect("build_int_compare failed"),
                            )
                        } else {
                            // Unit enum: direct tag compare
                            let tag = subject_val.into_int_value();
                            Some(
                                cx.builder
                                    .build_int_compare(IntPredicate::EQ, tag, pat_val, "var_eq")
                                    .expect("build_int_compare failed"),
                            )
                        }
                    } else {
                        // Subject is int, compare directly
                        let tag = subject_val.into_int_value();
                        Some(
                            cx.builder
                                .build_int_compare(IntPredicate::EQ, tag, pat_val, "var_eq")
                                .expect("build_int_compare failed"),
                        )
                    }
                } else {
                    None // catch-all bind
                }
            }
            Pattern::IntLit(n) => {
                let pat_val = cx.context.i64_type().const_int(*n as u64, true);
                let subject_int = subject_val.into_int_value();
                Some(
                    cx.builder
                        .build_int_compare(IntPredicate::EQ, subject_int, pat_val, "pat_eq")
                        .expect("build_int_compare failed"),
                )
            }
            Pattern::BoolLit(b) => {
                let pat_val = cx.context.i8_type().const_int(*b as u64, false);
                let subject_int = subject_val.into_int_value();
                let subject_i8 = if subject_int.get_type().get_bit_width() > 8 {
                    cx.builder
                        .build_int_truncate(subject_int, cx.context.i8_type(), "trunc")
                        .expect("build_int_truncate failed")
                } else {
                    subject_int
                };
                Some(
                    cx.builder
                        .build_int_compare(IntPredicate::EQ, subject_i8, pat_val, "pat_eq")
                        .expect("build_int_compare failed"),
                )
            }
            Pattern::StringLit(s) => {
                let pat_ptr = cx.create_string(s)?;
                let eq = cx
                    .rt_call("rt_str_eq", &[subject_val.into(), pat_ptr.into()])
                    .unwrap()
                    .into_int_value();
                let zero = cx.context.i8_type().const_int(0, false);
                Some(
                    cx.builder
                        .build_int_compare(IntPredicate::NE, eq, zero, "pat_eq")
                        .expect("build_int_compare failed"),
                )
            }
            Pattern::Ok(_) => {
                // Result tag 0 = Ok
                let tag = cx
                    .rt_call("rt_result_tag", &[subject_val.into()])
                    .unwrap()
                    .into_int_value();
                let zero = cx.context.i64_type().const_int(0, false);
                Some(
                    cx.builder
                        .build_int_compare(IntPredicate::EQ, tag, zero, "is_ok")
                        .expect("build_int_compare failed"),
                )
            }
            Pattern::Err(_) => {
                let tag = cx
                    .rt_call("rt_result_tag", &[subject_val.into()])
                    .unwrap()
                    .into_int_value();
                let one = cx.context.i64_type().const_int(1, false);
                Some(
                    cx.builder
                        .build_int_compare(IntPredicate::EQ, tag, one, "is_err")
                        .expect("build_int_compare failed"),
                )
            }
            Pattern::Some(_) => {
                let tag = cx
                    .rt_call("rt_option_tag", &[subject_val.into()])
                    .unwrap()
                    .into_int_value();
                let one = cx.context.i64_type().const_int(1, false);
                Some(
                    cx.builder
                        .build_int_compare(IntPredicate::EQ, tag, one, "is_some")
                        .expect("build_int_compare failed"),
                )
            }
            Pattern::None => {
                let tag = cx
                    .rt_call("rt_option_tag", &[subject_val.into()])
                    .unwrap()
                    .into_int_value();
                let zero = cx.context.i64_type().const_int(0, false);
                Some(
                    cx.builder
                        .build_int_compare(IntPredicate::EQ, tag, zero, "is_none")
                        .expect("build_int_compare failed"),
                )
            }
            Pattern::VariantDestructure { variant, .. } => {
                // Match on enum tag
                if let TurboTy::Enum(ref enum_name) = subject_tty {
                    if let Some(variants) = cx.enum_variants.get(enum_name) {
                        if let Some(idx) = variants.iter().position(|v| v == variant) {
                            let tag_val = cx.context.i64_type().const_int(idx as u64, false);
                            // For data enums, load tag from heap
                            if cx.enum_max_slots.contains_key(enum_name) {
                                let ptr = subject_val.into_pointer_value();
                                let tag = cx
                                    .builder
                                    .build_load(cx.context.i64_type(), ptr, "tag")
                                    .expect("build_load failed")
                                    .into_int_value();
                                Some(
                                    cx.builder
                                        .build_int_compare(IntPredicate::EQ, tag, tag_val, "var_eq")
                                        .expect("build_int_compare failed"),
                                )
                            } else {
                                let tag = subject_val.into_int_value();
                                Some(
                                    cx.builder
                                        .build_int_compare(IntPredicate::EQ, tag, tag_val, "var_eq")
                                        .expect("build_int_compare failed"),
                                )
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
        };

        let has_pattern_test = matches.is_some();
        if let Some(cond) = matches {
            if arm.guard.is_some() {
                // Pattern matched -- jump to a guard-check block
                let guard_block = cx
                    .context
                    .append_basic_block(cx.current_fn, &format!("match_guard_{i}"));
                cx.builder
                    .build_conditional_branch(cond, guard_block, next_block)
                    .expect("build_conditional_branch failed");
                cx.builder.position_at_end(guard_block);
            } else {
                cx.builder
                    .build_conditional_branch(cond, arm_block, next_block)
                    .expect("build_conditional_branch failed");
            }
        } else {
            // Wildcard or Ident: always matches
            if arm.guard.is_some() {
                // Need a guard block for wildcard + guard
                let guard_block = cx
                    .context
                    .append_basic_block(cx.current_fn, &format!("match_guard_{i}"));
                cx.builder
                    .build_unconditional_branch(guard_block)
                    .expect("br");
                cx.builder.position_at_end(guard_block);
            } else {
                cx.builder
                    .build_unconditional_branch(arm_block)
                    .expect("build_unconditional_branch failed");
            }
        }

        // If there's a guard, we're now in the guard block.
        // Bind pattern variables first (guard may reference them), then evaluate guard.
        if arm.guard.is_some() {
            // We're in the guard_block; bind vars, eval guard, branch
            // Bind variables needed by guard
            // (For simplicity, bind subject for ident patterns)
            let saved_guard_vars = cx.vars.clone();
            match &arm.pattern.node {
                Pattern::Ident(name)
                    if name != "_" && lookup_variant_tag(cx.enum_variants, name).is_none() =>
                {
                    let llvm_ty = subject_val.get_type();
                    let alloca = cx.create_entry_block_alloca(llvm_ty, name);
                    cx.builder.build_store(alloca, subject_val).expect("store");
                    cx.vars.insert(name.clone(), (alloca, subject_tty.clone()));
                }
                Pattern::VariantDestructure { variant, bindings } => {
                    if let TurboTy::Enum(ref enum_name) = subject_tty {
                        if cx.enum_max_slots.contains_key(enum_name) {
                            let ptr = subject_val.into_pointer_value();
                            for (j, bname) in bindings.iter().enumerate() {
                                if bname == "_" {
                                    continue;
                                }
                                let offset = ((j + 1) * 8) as u64;
                                let field_ptr = unsafe {
                                    cx.builder
                                        .build_gep(
                                            cx.context.i8_type(),
                                            ptr,
                                            &[cx.context.i64_type().const_int(offset, false)],
                                            "guard_bind_ptr",
                                        )
                                        .expect("gep")
                                };
                                let val = cx
                                    .builder
                                    .build_load(cx.context.i64_type(), field_ptr, "guard_bind_val")
                                    .expect("load");
                                let field_tty = cx
                                    .enum_variant_fields
                                    .get(&(enum_name.clone(), variant.clone()))
                                    .and_then(|fs| fs.get(j))
                                    .cloned()
                                    .unwrap_or(TurboTy::Int);
                                let alloca = cx.create_entry_block_alloca(val.get_type(), bname);
                                cx.builder.build_store(alloca, val).expect("store");
                                cx.vars.insert(bname.clone(), (alloca, field_tty));
                            }
                        }
                    }
                }
                _ => {}
            }
            let guard_expr = arm.guard.as_ref().unwrap();
            let (guard_val, _) = compile_expr(cx, guard_expr)?.unwrap();
            let guard_bool = guard_val.into_int_value();
            // Normalize to i1 for the branch
            let guard_cond = if guard_bool.get_type().get_bit_width() == 1 {
                guard_bool
            } else {
                let zero = guard_bool.get_type().const_int(0, false);
                cx.builder
                    .build_int_compare(IntPredicate::NE, guard_bool, zero, "guard_cond")
                    .expect("icmp")
            };
            cx.builder
                .build_conditional_branch(guard_cond, arm_block, next_block)
                .expect("cond_br");
            cx.vars = saved_guard_vars;
        }

        // Compile arm body
        cx.builder.position_at_end(arm_block);

        // Bind pattern variables
        let saved_vars = cx.vars.clone();
        match &arm.pattern.node {
            Pattern::Ident(name)
                if name != "_" && lookup_variant_tag(cx.enum_variants, name).is_none() =>
            {
                // Catch-all bind: bind subject to name
                let llvm_ty = subject_val.get_type();
                let alloca = cx.create_entry_block_alloca(llvm_ty, name);
                cx.builder
                    .build_store(alloca, subject_val)
                    .expect("build_store failed");
                cx.vars.insert(name.clone(), (alloca, subject_tty.clone()));
            }
            Pattern::Ok(name) | Pattern::Some(name) => {
                let is_ok = matches!(arm.pattern.node, Pattern::Ok(_));
                let inner_tty = match &subject_tty {
                    TurboTy::Result(ok_ty, _) if is_ok => *ok_ty.clone(),
                    TurboTy::Optional(inner_ty) if !is_ok => *inner_ty.clone(),
                    _ => TurboTy::Int,
                };
                let inner_raw = if is_ok {
                    cx.rt_call("rt_result_value", &[subject_val.into()])
                        .unwrap()
                } else {
                    cx.rt_call("rt_option_value", &[subject_val.into()])
                        .unwrap()
                };
                // inner_raw is i64; narrow to the inner type for storage
                let inner_narrowed = narrow_from_storage(cx, inner_raw, &inner_tty);
                let inner_llvm_ty = turbo_ty_to_llvm_ctx(&inner_tty, cx.context, cx.enum_max_slots);
                let alloca = cx.create_entry_block_alloca(inner_llvm_ty, name);
                cx.builder
                    .build_store(alloca, inner_narrowed)
                    .expect("build_store failed");
                cx.vars.insert(name.clone(), (alloca, inner_tty));
            }
            Pattern::Err(name) => {
                let inner_tty = match &subject_tty {
                    TurboTy::Result(_, err_ty) => *err_ty.clone(),
                    _ => TurboTy::Int,
                };
                let inner_raw = cx
                    .rt_call("rt_result_value", &[subject_val.into()])
                    .unwrap();
                let inner_narrowed = narrow_from_storage(cx, inner_raw, &inner_tty);
                let inner_llvm_ty = turbo_ty_to_llvm_ctx(&inner_tty, cx.context, cx.enum_max_slots);
                let alloca = cx.create_entry_block_alloca(inner_llvm_ty, name);
                cx.builder
                    .build_store(alloca, inner_narrowed)
                    .expect("build_store failed");
                cx.vars.insert(name.clone(), (alloca, inner_tty));
            }
            Pattern::VariantDestructure { variant, bindings } => {
                // Bind destructured fields
                if let TurboTy::Enum(ref enum_name) = subject_tty {
                    if cx.enum_max_slots.contains_key(enum_name) {
                        let ptr = subject_val.into_pointer_value();
                        for (j, binding_name) in bindings.iter().enumerate() {
                            if binding_name == "_" {
                                continue;
                            }
                            let offset = ((j + 1) * 8) as u64;
                            let field_ptr = unsafe {
                                cx.builder
                                    .build_gep(
                                        cx.context.i8_type(),
                                        ptr,
                                        &[cx.context.i64_type().const_int(offset, false)],
                                        "vf_ptr",
                                    )
                                    .expect("build_gep failed")
                            };
                            let field_val = cx
                                .builder
                                .build_load(cx.context.i64_type(), field_ptr, binding_name)
                                .expect("build_load failed");
                            // Determine field type
                            let field_tty = cx
                                .enum_variant_fields
                                .get(&(enum_name.clone(), variant.clone()))
                                .and_then(|tys| tys.get(j).cloned())
                                .unwrap_or(TurboTy::Int);
                            let field_val = narrow_from_storage(cx, field_val, &field_tty);
                            let alloca =
                                cx.create_entry_block_alloca(field_val.get_type(), binding_name);
                            cx.builder
                                .build_store(alloca, field_val)
                                .expect("build_store failed");
                            cx.vars.insert(binding_name.clone(), (alloca, field_tty));
                        }
                    }
                }
            }
            _ => {}
        }

        let arm_result = compile_expr(cx, &arm.body)?;
        cx.vars = saved_vars;

        let arm_end_block = cx.builder.get_insert_block().unwrap();
        if arm_end_block.get_terminator().is_none() {
            if let Some((val, ref tty)) = arm_result {
                if first_arm_tty.is_none() {
                    first_arm_tty = Some(tty.clone());
                }
                phi_incoming.push((val, arm_end_block));
            }
            cx.builder
                .build_unconditional_branch(merge_block)
                .expect("build_unconditional_branch failed");
        }

        // Position at next test block if needed
        if i + 1 < arms.len() {
            cx.builder.position_at_end(next_block);
        }
    }

    // Default block (unreachable)
    cx.builder.position_at_end(default_block);
    cx.builder
        .build_unreachable()
        .expect("build_unreachable failed");

    // Merge
    cx.builder.position_at_end(merge_block);

    if !phi_incoming.is_empty() && first_arm_tty.is_some() {
        let first_type = phi_incoming[0].0.get_type();
        let phi = cx
            .builder
            .build_phi(first_type, "match_result")
            .expect("build_phi failed");
        for (val, block) in &phi_incoming {
            phi.add_incoming(&[(val, *block)]);
        }
        Ok(Some((phi.as_basic_value(), first_arm_tty.unwrap())))
    } else {
        Ok(None)
    }
}

// ── Value conversion helpers ────────────────────────────────────────

pub(crate) fn convert_to_str<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    val: BasicValueEnum<'ctx>,
    tty: &TurboTy,
) -> Result<BasicValueEnum<'ctx>, CodegenError> {
    match tty {
        TurboTy::Str => Ok(val),
        TurboTy::Int => {
            let iv = val.into_int_value();
            let iv = if iv.get_type().get_bit_width() < 64 {
                cx.builder
                    .build_int_s_extend(iv, cx.context.i64_type(), "ext")
                    .expect("build_int_s_extend failed")
            } else {
                iv
            };
            Ok(cx.rt_call("rt_i64_to_str", &[iv.into()]).unwrap())
        }
        TurboTy::Float => Ok(cx.rt_call("rt_f64_to_str", &[val.into()]).unwrap()),
        TurboTy::Bool => {
            let iv = val.into_int_value();
            let iv = if iv.get_type().get_bit_width() > 8 {
                cx.builder
                    .build_int_truncate(iv, cx.context.i8_type(), "trunc")
                    .expect("build_int_truncate failed")
            } else {
                iv
            };
            Ok(cx.rt_call("rt_bool_to_str", &[iv.into()]).unwrap())
        }
        TurboTy::Struct(ref sname) => {
            let sname = sname.clone();
            let to_str_fn = format!("{sname}__to_string");
            if let Some(&ts_fn) = cx.user_fns.get(&to_str_fn) {
                let s = cx
                    .builder
                    .build_direct_call(ts_fn, &[val.into()], "to_str")
                    .expect("call")
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                Ok(s)
            } else {
                let ptr = cx.create_string(&format!("<{sname}>"))?;
                Ok(ptr.into())
            }
        }
        _ => {
            let ptr = cx.create_string("<value>")?;
            Ok(ptr.into())
        }
    }
}

/// Widen a value to i64 for uniform heap storage.
pub(crate) fn widen_for_storage<'a, 'ctx>(
    cx: &Ctx<'a, 'ctx>,
    val: BasicValueEnum<'ctx>,
) -> BasicValueEnum<'ctx> {
    match val {
        BasicValueEnum::IntValue(iv) => {
            if iv.get_type().get_bit_width() < 64 {
                cx.builder
                    .build_int_s_extend(iv, cx.context.i64_type(), "widen")
                    .expect("build_int_s_extend failed")
                    .into()
            } else {
                val
            }
        }
        BasicValueEnum::FloatValue(fv) => {
            // bitcast f64 -> i64 for storage
            cx.builder
                .build_bit_cast(fv, cx.context.i64_type(), "f2i")
                .expect("build_bitcast failed")
        }
        BasicValueEnum::PointerValue(pv) => {
            // ptr -> i64 for storage in uniform-width arrays/structs
            cx.builder
                .build_ptr_to_int(pv, cx.context.i64_type(), "ptr2i")
                .expect("build_ptr_to_int failed")
                .into()
        }
        _ => val,
    }
}

/// Convert an integer value to a pointer if it's an i64 (for channel/mutex/hashmap operations).
pub(crate) fn int_to_ptr_if_needed<'a, 'ctx>(
    cx: &Ctx<'a, 'ctx>,
    val: BasicValueEnum<'ctx>,
) -> PointerValue<'ctx> {
    match val {
        BasicValueEnum::PointerValue(pv) => pv,
        BasicValueEnum::IntValue(iv) => cx
            .builder
            .build_int_to_ptr(iv, cx.context.ptr_type(AddressSpace::default()), "i2ptr")
            .expect("int_to_ptr failed"),
        _ => cx.context.ptr_type(AddressSpace::default()).const_null(),
    }
}

/// Narrow a value from i64 storage back to its actual type.
pub(crate) fn narrow_from_storage<'a, 'ctx>(
    cx: &Ctx<'a, 'ctx>,
    val: BasicValueEnum<'ctx>,
    tty: &TurboTy,
) -> BasicValueEnum<'ctx> {
    match tty {
        TurboTy::Bool => {
            let iv = val.into_int_value();
            cx.builder
                .build_int_truncate(iv, cx.context.i8_type(), "narrow_bool")
                .expect("build_int_truncate failed")
                .into()
        }
        TurboTy::Float => {
            let iv = val.into_int_value();
            cx.builder
                .build_bit_cast(iv, cx.context.f64_type(), "i2f")
                .expect("build_bitcast failed")
        }
        TurboTy::Str
        | TurboTy::Array(_)
        | TurboTy::Struct(_)
        | TurboTy::Result(_, _)
        | TurboTy::Optional(_)
        | TurboTy::Future(_) => {
            // i64 -> ptr via inttoptr
            let iv = val.into_int_value();
            cx.builder
                .build_int_to_ptr(iv, cx.context.ptr_type(AddressSpace::default()), "i2ptr")
                .expect("build_int_to_pointer failed")
                .into()
        }
        _ => val, // Int, Enum, etc. stay as i64
    }
}

/// Coerce argument value to match the expected LLVM type.
pub(crate) fn coerce_arg<'a, 'ctx>(
    cx: &Ctx<'a, 'ctx>,
    val: BasicValueEnum<'ctx>,
    expected: BasicTypeEnum<'ctx>,
) -> BasicValueEnum<'ctx> {
    let actual = val.get_type();
    if actual == expected {
        return val;
    }

    // Int width mismatch
    if let (BasicTypeEnum::IntType(actual_int), BasicTypeEnum::IntType(expected_int)) =
        (actual, expected)
    {
        if actual_int.get_bit_width() < expected_int.get_bit_width() {
            return cx
                .builder
                .build_int_s_extend(val.into_int_value(), expected_int, "coerce_ext")
                .expect("build_int_s_extend failed")
                .into();
        }
        if actual_int.get_bit_width() > expected_int.get_bit_width() {
            return cx
                .builder
                .build_int_truncate(val.into_int_value(), expected_int, "coerce_trunc")
                .expect("build_int_truncate failed")
                .into();
        }
    }

    // Pointer to int coercion
    if let (BasicTypeEnum::PointerType(_), BasicTypeEnum::IntType(expected_int)) =
        (actual, expected)
    {
        return cx
            .builder
            .build_ptr_to_int(val.into_pointer_value(), expected_int, "coerce_ptr2int")
            .expect("build_ptr_to_int failed")
            .into();
    }

    // Int to pointer coercion
    if let (BasicTypeEnum::IntType(_), BasicTypeEnum::PointerType(expected_ptr)) =
        (actual, expected)
    {
        return cx
            .builder
            .build_int_to_ptr(val.into_int_value(), expected_ptr, "coerce_int2ptr")
            .expect("build_int_to_ptr failed")
            .into();
    }

    val
}
