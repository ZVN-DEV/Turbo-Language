//! Built-in function dispatch (compile_call) and all built-in
//! function implementations (print, assert, len, map, filter, etc.).

use inkwell::types::{BasicMetadataTypeEnum, BasicType, BasicTypeEnum};
use inkwell::values::{BasicMetadataValueEnum, BasicValueEnum};
use inkwell::{AddressSpace, FloatPredicate, IntPredicate};
use turbo_ast::*;

use crate::ctx::Ctx;
use crate::expr::{
    coerce_arg, compile_expr, convert_to_str, int_to_ptr_if_needed, widen_for_storage,
};
use crate::types::{
    turbo_ty_from_type_expr, turbo_ty_to_llvm, turbo_ty_to_llvm_ctx, MaybeTyped, TurboTy,
};
use crate::CodegenError;

// ── Function calls ──────────────────────────────────────────────────

pub(crate) fn compile_call<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    callee: &Spanned<Expr>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    // Method calls: expr.method(args)
    if let Expr::FieldAccess {
        ref object,
        ref field,
    } = callee.node
    {
        let (obj_val, obj_tty) = compile_expr(cx, object)?.unwrap();
        if let TurboTy::Struct(ref type_name) = obj_tty {
            let mangled = format!("{}__{}", type_name, field);
            if let Some(&func) = cx.user_fns.get(&mangled) {
                let mut arg_vals: Vec<BasicMetadataValueEnum> = vec![obj_val.into()];
                for arg in args {
                    if let Some((v, _)) = compile_expr(cx, arg)? {
                        arg_vals.push(v.into());
                    }
                }
                let call = cx
                    .builder
                    .build_direct_call(func, &arg_vals, "")
                    .expect("build_direct_call failed");
                let ret_tty = cx
                    .fn_ret_types
                    .get(&mangled)
                    .cloned()
                    .unwrap_or(TurboTy::Unit);
                return match call.try_as_basic_value().left() {
                    Some(val) => Ok(Some((val, ret_tty))),
                    None => Ok(None),
                };
            }
        }
        // String/array method calls
        return compile_method_call(cx, obj_val, &obj_tty, field, args);
    }

    // Check if callee is a closure variable (TurboTy::Fn stored in vars)
    if let Expr::Ident(callee_name) = &callee.node {
        if let Some((alloca, TurboTy::Fn(ref param_tys, ref ret_ty))) =
            cx.vars.get(callee_name.as_str()).cloned()
        {
            let param_tys = param_tys.clone();
            let ret_ty = *ret_ty.clone();
            let ptr_type = cx.context.ptr_type(AddressSpace::default());
            let i8_type = cx.context.i8_type();
            let i64_type = cx.context.i64_type();

            // Load the closure pair struct pointer
            let closure_ptr = cx
                .builder
                .build_load(ptr_type, alloca, "closure_ptr")
                .expect("load")
                .into_pointer_value();

            // Load fn_ptr (as i64) from offset 0, convert to pointer
            let fn_ptr_i64 = cx
                .builder
                .build_load(i64_type, closure_ptr, "fn_ptr_i64")
                .expect("load")
                .into_int_value();
            let fn_ptr = cx
                .builder
                .build_int_to_ptr(fn_ptr_i64, ptr_type, "fn_ptr")
                .expect("itp");

            // Load env_ptr (as i64) from offset 8, convert to pointer
            let env_slot = unsafe {
                cx.builder
                    .build_gep(
                        i8_type,
                        closure_ptr,
                        &[i64_type.const_int(8, false)],
                        "env_slot",
                    )
                    .expect("gep")
            };
            let env_ptr_i64 = cx
                .builder
                .build_load(i64_type, env_slot, "env_ptr_i64")
                .expect("load")
                .into_int_value();
            let env_ptr = cx
                .builder
                .build_int_to_ptr(env_ptr_i64, ptr_type, "env_ptr")
                .expect("itp");

            // Build LLVM function type: (ptr, ...params) -> ret
            let mut llvm_param_types: Vec<BasicMetadataTypeEnum<'ctx>> = vec![ptr_type.into()]; // env_ptr
            for pt in &param_tys {
                llvm_param_types
                    .push(turbo_ty_to_llvm_ctx(pt, cx.context, cx.enum_max_slots).into());
            }
            let fn_type = if ret_ty == TurboTy::Unit {
                cx.context.void_type().fn_type(&llvm_param_types, false)
            } else {
                let ret_llvm = turbo_ty_to_llvm_ctx(&ret_ty, cx.context, cx.enum_max_slots);
                ret_llvm.fn_type(&llvm_param_types, false)
            };

            // Compile arguments
            let mut arg_vals: Vec<BasicMetadataValueEnum<'ctx>> = vec![env_ptr.into()];
            for (i, arg) in args.iter().enumerate() {
                if let Some((val, _)) = compile_expr(cx, arg)? {
                    if i < param_tys.len() {
                        let expected =
                            turbo_ty_to_llvm_ctx(&param_tys[i], cx.context, cx.enum_max_slots);
                        let val = coerce_arg(cx, val, expected);
                        arg_vals.push(val.into());
                    } else {
                        arg_vals.push(val.into());
                    }
                }
            }

            let call = cx
                .builder
                .build_indirect_call(fn_type, fn_ptr, &arg_vals, "closure_call")
                .expect("indirect call failed");

            return match call.try_as_basic_value().left() {
                Some(val) => Ok(Some((val, ret_ty))),
                None => Ok(None),
            };
        }
    }

    let Expr::Ident(name) = &callee.node else {
        return Err(CodegenError {
            code: ErrorCode::E0400,
            message: "indirect function calls not yet supported in LLVM backend".to_string(),
        });
    };

    match name.as_str() {
        "print" => compile_print(cx, args),
        "panic" => compile_panic(cx, args),
        "assert" => compile_assert(cx, args),
        "assert_eq" => compile_assert_eq(cx, args, false),
        "assert_ne" => compile_assert_eq(cx, args, true),
        "len" => compile_len(cx, args),
        "abs" => compile_abs(cx, args),
        "min" => compile_min_max(cx, args, true),
        "max" => compile_min_max(cx, args, false),
        "to_str" => compile_to_str_builtin(cx, args),
        // Stdlib
        "split" => compile_stdlib_2arg_rt(
            cx,
            args,
            "rt_str_split",
            TurboTy::Array(Box::new(TurboTy::Str)),
        ),
        "trim" => compile_stdlib_1arg_rt(cx, args, "rt_str_trim", TurboTy::Str),
        "upper" => compile_stdlib_1arg_rt(cx, args, "rt_str_upper", TurboTy::Str),
        "lower" => compile_stdlib_1arg_rt(cx, args, "rt_str_lower", TurboTy::Str),
        "starts_with" => compile_stdlib_2arg_rt(cx, args, "rt_str_starts_with", TurboTy::Bool),
        "ends_with" => compile_stdlib_2arg_rt(cx, args, "rt_str_ends_with", TurboTy::Bool),
        "contains" => compile_stdlib_2arg_rt(cx, args, "rt_str_contains", TurboTy::Bool),
        "index_of" => compile_stdlib_2arg_rt(cx, args, "rt_str_index_of", TurboTy::Int),
        "replace" => {
            if args.len() >= 3 {
                let (a, _) = compile_expr(cx, &args[0])?.unwrap();
                let (b, _) = compile_expr(cx, &args[1])?.unwrap();
                let (c, _) = compile_expr(cx, &args[2])?.unwrap();
                let result = cx
                    .rt_call("rt_str_replace", &[a.into(), b.into(), c.into()])
                    .unwrap();
                Ok(Some((result, TurboTy::Str)))
            } else {
                Ok(None)
            }
        }
        "char_at" => compile_stdlib_2arg_rt(cx, args, "rt_str_char_at", TurboTy::Str),
        "join" => compile_stdlib_2arg_rt(cx, args, "rt_str_join", TurboTy::Str),
        "repeat" => compile_stdlib_2arg_rt(cx, args, "rt_str_repeat", TurboTy::Str),
        "read_line" => {
            let result = cx.rt_call("rt_read_line", &[]).unwrap();
            Ok(Some((result, TurboTy::Str)))
        }
        "read_file" => compile_stdlib_1arg_rt(cx, args, "rt_read_file", TurboTy::Str),
        "write_file" => {
            if args.len() >= 2 {
                let (a, _) = compile_expr(cx, &args[0])?.unwrap();
                let (b, _) = compile_expr(cx, &args[1])?.unwrap();
                cx.rt_call("rt_write_file", &[a.into(), b.into()]);
            }
            Ok(None)
        }
        "pow" => {
            if args.len() >= 2 {
                let (a, _) = compile_expr(cx, &args[0])?.unwrap();
                let (b, _) = compile_expr(cx, &args[1])?.unwrap();
                let result = cx.rt_call("rt_pow", &[a.into(), b.into()]).unwrap();
                Ok(Some((result, TurboTy::Int)))
            } else {
                Ok(None)
            }
        }
        "sqrt" => {
            if !args.is_empty() {
                let (a, _) = compile_expr(cx, &args[0])?.unwrap();
                let result = cx.rt_call("rt_sqrt", &[a.into()]).unwrap();
                Ok(Some((result, TurboTy::Float)))
            } else {
                Ok(None)
            }
        }
        "sleep" => {
            if !args.is_empty() {
                let (a, _) = compile_expr(cx, &args[0])?.unwrap();
                cx.rt_call("rt_sleep_ms", &[a.into()]);
            }
            Ok(None)
        }
        "http_get" => compile_stdlib_1arg_rt(cx, args, "rt_http_get", TurboTy::Str),
        "http_post" => compile_stdlib_2arg_rt(cx, args, "rt_http_post", TurboTy::Str),
        "json_get" => compile_stdlib_2arg_rt(cx, args, "rt_json_get", TurboTy::Str),
        "channel" => {
            let result = cx.rt_call("rt_channel_create", &[]).unwrap();
            Ok(Some((result, TurboTy::Struct("Channel".to_string()))))
        }
        "send" => {
            if args.len() >= 2 {
                let (ch, _) = compile_expr(cx, &args[0])?.unwrap();
                let (val, _) = compile_expr(cx, &args[1])?.unwrap();
                let val_i64 = widen_for_storage(cx, val);
                let ch_ptr = int_to_ptr_if_needed(cx, ch);
                cx.rt_call("rt_channel_send", &[ch_ptr.into(), val_i64.into()]);
            }
            Ok(None)
        }
        "recv" => {
            if !args.is_empty() {
                let (ch, _) = compile_expr(cx, &args[0])?.unwrap();
                let ch_ptr = int_to_ptr_if_needed(cx, ch);
                let result = cx.rt_call("rt_channel_recv", &[ch_ptr.into()]).unwrap();
                Ok(Some((result, TurboTy::Int)))
            } else {
                Ok(None)
            }
        }
        "mutex" => {
            if !args.is_empty() {
                let (val, _) = compile_expr(cx, &args[0])?.unwrap();
                let val_i64 = widen_for_storage(cx, val);
                let result = cx.rt_call("rt_mutex_create", &[val_i64.into()]).unwrap();
                Ok(Some((result, TurboTy::Struct("Mutex".to_string()))))
            } else {
                Ok(None)
            }
        }
        "mutex_get" => {
            if !args.is_empty() {
                let (m, _) = compile_expr(cx, &args[0])?.unwrap();
                let m_ptr = int_to_ptr_if_needed(cx, m);
                let result = cx.rt_call("rt_mutex_get", &[m_ptr.into()]).unwrap();
                Ok(Some((result, TurboTy::Int)))
            } else {
                Ok(None)
            }
        }
        "mutex_set" => {
            if args.len() >= 2 {
                let (m, _) = compile_expr(cx, &args[0])?.unwrap();
                let (val, _) = compile_expr(cx, &args[1])?.unwrap();
                let m_ptr = int_to_ptr_if_needed(cx, m);
                let val_i64 = widen_for_storage(cx, val);
                cx.rt_call("rt_mutex_set", &[m_ptr.into(), val_i64.into()]);
            }
            Ok(None)
        }
        "hashmap" => {
            let result = cx.rt_call("rt_hashmap_new", &[]).unwrap();
            Ok(Some((result, TurboTy::Struct("HashMap".to_string()))))
        }
        "hashmap_set" => {
            if args.len() >= 3 {
                let (m, _) = compile_expr(cx, &args[0])?.unwrap();
                let (k, _) = compile_expr(cx, &args[1])?.unwrap();
                let (v, _) = compile_expr(cx, &args[2])?.unwrap();
                cx.rt_call("rt_hashmap_set", &[m.into(), k.into(), v.into()]);
            }
            Ok(None)
        }
        "hashmap_get" => compile_stdlib_2arg_rt(cx, args, "rt_hashmap_get", TurboTy::Str),
        "hashmap_has" => compile_stdlib_2arg_rt(cx, args, "rt_hashmap_has", TurboTy::Bool),
        "hashmap_len" => compile_stdlib_1arg_rt(cx, args, "rt_hashmap_len", TurboTy::Int),
        "hashmap_keys" => compile_stdlib_1arg_rt(
            cx,
            args,
            "rt_hashmap_keys",
            TurboTy::Array(Box::new(TurboTy::Str)),
        ),
        "hashmap_remove" => {
            if args.len() >= 2 {
                let (m, _) = compile_expr(cx, &args[0])?.unwrap();
                let (k, _) = compile_expr(cx, &args[1])?.unwrap();
                cx.rt_call("rt_hashmap_remove", &[m.into(), k.into()]);
            }
            Ok(None)
        }
        "map" => compile_builtin_map_llvm(cx, args),
        "filter" => compile_builtin_filter_llvm(cx, args),
        "reduce" => compile_builtin_reduce_llvm(cx, args),
        "clone" => {
            if !args.is_empty() {
                let (val, tty) = compile_expr(cx, &args[0])?.unwrap();
                // For structs with clone derive, call StructName__clone
                if let TurboTy::Struct(ref sname) = tty {
                    let clone_fn_name = format!("{sname}__clone");
                    if let Some(&clone_fn) = cx.user_fns.get(&clone_fn_name) {
                        let call = cx
                            .builder
                            .build_direct_call(clone_fn, &[val.into()], "")
                            .expect("build_direct_call");
                        return match call.try_as_basic_value().left() {
                            Some(v) => Ok(Some((v, tty))),
                            None => Ok(None),
                        };
                    }
                }
                // Otherwise, shallow clone (return same value for primitives)
                Ok(Some((val, tty)))
            } else {
                Ok(None)
            }
        }
        "deref" => {
            if !args.is_empty() {
                let (val, _) = compile_expr(cx, &args[0])?.unwrap();
                // deref: load i64 from pointer
                let i64_type = cx.context.i64_type();
                let ptr = cx
                    .builder
                    .build_int_to_ptr(
                        val.into_int_value(),
                        cx.context.ptr_type(AddressSpace::default()),
                        "deref_ptr",
                    )
                    .expect("itp");
                let loaded = cx
                    .builder
                    .build_load(i64_type, ptr, "deref_val")
                    .expect("load");
                Ok(Some((loaded, TurboTy::Int)))
            } else {
                Ok(None)
            }
        }
        "store" => {
            if args.len() >= 2 {
                let (ptr_val, _) = compile_expr(cx, &args[0])?.unwrap();
                let (val, _) = compile_expr(cx, &args[1])?.unwrap();
                let ptr = cx
                    .builder
                    .build_int_to_ptr(
                        ptr_val.into_int_value(),
                        cx.context.ptr_type(AddressSpace::default()),
                        "store_ptr",
                    )
                    .expect("itp");
                cx.builder.build_store(ptr, val).expect("store");
            }
            Ok(None)
        }
        "json_stringify" => {
            // rt_json_stringify(key_str, value_str) -> json_str
            if args.len() >= 2 {
                let (a, _) = compile_expr(cx, &args[0])?.unwrap();
                let (b, _) = compile_expr(cx, &args[1])?.unwrap();
                let result = cx
                    .rt_call("rt_json_stringify", &[a.into(), b.into()])
                    .unwrap();
                Ok(Some((result, TurboTy::Str)))
            } else if args.len() == 1 {
                let (a, _) = compile_expr(cx, &args[0])?.unwrap();
                let null_ptr = cx.context.ptr_type(AddressSpace::default()).const_null();
                let result = cx
                    .rt_call("rt_json_stringify", &[a.into(), null_ptr.into()])
                    .unwrap();
                Ok(Some((result, TurboTy::Str)))
            } else {
                Ok(None)
            }
        }
        "to_json" => {
            if !args.is_empty() {
                let (val, tty) = compile_expr(cx, &args[0])?.unwrap();
                if let TurboTy::Struct(ref sname) = tty {
                    compile_struct_to_json_llvm(cx, val, sname)
                } else {
                    let str_val = convert_to_str(cx, val, &tty)?;
                    Ok(Some((str_val, TurboTy::Str)))
                }
            } else {
                Ok(None)
            }
        }
        "to_json_array" => {
            if !args.is_empty() {
                let (arr_val, arr_tty) = compile_expr(cx, &args[0])?.unwrap();
                let elem_sname = match &arr_tty {
                    TurboTy::Array(inner) => match inner.as_ref() {
                        TurboTy::Struct(s) => Some(s.clone()),
                        _ => None,
                    },
                    _ => None,
                };
                if let Some(sname) = elem_sname {
                    compile_array_to_json_llvm(cx, arr_val, &sname)
                } else {
                    let str_val = convert_to_str(cx, arr_val, &arr_tty)?;
                    Ok(Some((str_val, TurboTy::Str)))
                }
            } else {
                Ok(None)
            }
        }
        "http_server" | "route" | "http_listen" | "respond" | "request_body" => {
            // Stub: these are complex HTTP server operations; emit a no-op for now
            for arg in args {
                compile_expr(cx, arg)?;
            }
            Ok(None)
        }
        _ => {
            // Check enum variant construction
            if !args.is_empty() {
                if let Expr::Ident(ref first_name) = args[0].node {
                    if let Some(variants) = cx.enum_variants.get(first_name.as_str()) {
                        if let Some(variant_index) = variants.iter().position(|v| v == name) {
                            let data_args = &args[1..];
                            let enum_name = first_name;

                            if let Some(&max_slots) = cx.enum_max_slots.get(enum_name.as_str()) {
                                let total_slots = 1 + max_slots;
                                let num_fields_val =
                                    cx.context.i64_type().const_int(total_slots as u64, false);
                                let ptr = cx
                                    .rt_call("rt_struct_alloc", &[num_fields_val.into()])
                                    .unwrap()
                                    .into_pointer_value();

                                // Store tag at offset 0
                                let tag_val =
                                    cx.context.i64_type().const_int(variant_index as u64, false);
                                cx.builder
                                    .build_store(ptr, tag_val)
                                    .expect("build_store failed");

                                // Store fields at offsets 8, 16, ...
                                for (j, arg) in data_args.iter().enumerate() {
                                    let (val, _) = compile_expr(cx, arg)?.unwrap();
                                    let offset = ((j + 1) * 8) as u64;
                                    let field_ptr = unsafe {
                                        cx.builder
                                            .build_gep(
                                                cx.context.i8_type(),
                                                ptr,
                                                &[cx.context.i64_type().const_int(offset, false)],
                                                "var_field_ptr",
                                            )
                                            .expect("build_gep failed")
                                    };
                                    let store_val = widen_for_storage(cx, val);
                                    cx.builder
                                        .build_store(field_ptr, store_val)
                                        .expect("build_store failed");
                                }

                                return Ok(Some((ptr.into(), TurboTy::Enum(enum_name.clone()))));
                            } else {
                                let val =
                                    cx.context.i64_type().const_int(variant_index as u64, false);
                                return Ok(Some((val.into(), TurboTy::Enum(enum_name.clone()))));
                            }
                        }
                    }
                }
            }

            // UFCS method call: parser rewrites obj.method(args) -> method(obj, args)
            if cx.user_fns.get(name.as_str()).is_none() && !args.is_empty() {
                let (first_val, first_tty) = compile_expr(cx, &args[0])?.unwrap();
                if let TurboTy::Struct(ref type_name) = first_tty {
                    let mangled = format!("{}__{}", type_name, name);
                    if let Some(&func) = cx.user_fns.get(&mangled) {
                        let mut arg_vals: Vec<BasicMetadataValueEnum> = vec![first_val.into()];
                        for arg in &args[1..] {
                            if let Some((v, _)) = compile_expr(cx, arg)? {
                                arg_vals.push(v.into());
                            }
                        }
                        let call = cx
                            .builder
                            .build_direct_call(func, &arg_vals, "")
                            .expect("build_direct_call failed");
                        let ret_tty = cx
                            .fn_ret_types
                            .get(&mangled)
                            .cloned()
                            .unwrap_or(TurboTy::Unit);
                        return match call.try_as_basic_value().left() {
                            Some(val) => Ok(Some((val, ret_tty))),
                            None => Ok(None),
                        };
                    }
                }
            }

            // Regular user function call
            let func = *cx.user_fns.get(name.as_str()).ok_or_else(|| CodegenError {
                code: ErrorCode::E0402,
                message: format!("undefined function: {name}"),
            })?;

            let ret_tty = cx
                .fn_ret_types
                .get(name.as_str())
                .cloned()
                .unwrap_or(TurboTy::Unit);

            let type_params = cx
                .fn_type_params
                .get(name.as_str())
                .cloned()
                .unwrap_or_default();

            let mut arg_vals: Vec<BasicMetadataValueEnum> = Vec::new();
            let mut arg_ttys: Vec<TurboTy> = Vec::new();
            for (i, arg) in args.iter().enumerate() {
                if let Some((val, tty)) = compile_expr(cx, arg)? {
                    // Type coercion: match parameter types
                    let expected_type = func.get_type().get_param_types();
                    if i < expected_type.len() {
                        let val = coerce_arg(cx, val, expected_type[i]);
                        arg_vals.push(val.into());
                    } else {
                        arg_vals.push(val.into());
                    }
                    arg_ttys.push(tty);
                }
            }

            // For generic functions, infer the actual return TurboTy from args.
            let actual_ret_tty = if !type_params.is_empty() {
                if let Some(f_def) = cx.fn_asts.get(name.as_str()) {
                    if let Some(ret_ty) = &f_def.return_type {
                        if let TypeExpr::Named(ref ret_name) = ret_ty.node {
                            if type_params.contains(ret_name) {
                                let mut inferred = None;
                                for (i, param) in f_def.params.iter().enumerate() {
                                    if let TypeExpr::Named(ref pname) = param.ty.node {
                                        if pname == ret_name {
                                            if i < arg_ttys.len() {
                                                inferred = Some(arg_ttys[i].clone());
                                            }
                                            break;
                                        }
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

            let call = cx
                .builder
                .build_direct_call(func, &arg_vals, "")
                .expect("build_direct_call failed");

            match call.try_as_basic_value().left() {
                Some(val) => Ok(Some((val, actual_ret_tty))),
                None => Ok(None),
            }
        }
    }
}

// ── Closure-based builtins (map, filter, reduce) ────────────────────

/// compile_builtin_map_llvm: map(arr, closure) -> [T]
fn compile_builtin_map_llvm<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    if args.len() < 2 {
        return Ok(None);
    }
    let (arr_ptr, _arr_tty) = compile_expr(cx, &args[0])?.unwrap();
    let (closure_ptr_val, fn_tty) = compile_expr(cx, &args[1])?.unwrap();

    let (param_tty, ret_tty) = match &fn_tty {
        TurboTy::Fn(params, ret) => (params[0].clone(), *ret.clone()),
        _ => (TurboTy::Int, TurboTy::Int),
    };

    let ptr_type = cx.context.ptr_type(AddressSpace::default());
    let i8_type = cx.context.i8_type();
    let i64_type = cx.context.i64_type();

    let closure_ptr = closure_ptr_val.into_pointer_value();

    // Load fn_ptr (as i64) from offset 0
    let fn_ptr_i64 = cx
        .builder
        .build_load(i64_type, closure_ptr, "map_fn_i64")
        .expect("load")
        .into_int_value();
    let fn_ptr = cx
        .builder
        .build_int_to_ptr(fn_ptr_i64, ptr_type, "map_fn_ptr")
        .expect("itp");

    // Load env_ptr (as i64) from offset 8
    let env_slot = unsafe {
        cx.builder
            .build_gep(
                i8_type,
                closure_ptr,
                &[i64_type.const_int(8, false)],
                "env_slot",
            )
            .expect("gep")
    };
    let env_ptr_i64 = cx
        .builder
        .build_load(i64_type, env_slot, "map_env_i64")
        .expect("load")
        .into_int_value();
    let env_ptr = cx
        .builder
        .build_int_to_ptr(env_ptr_i64, ptr_type, "map_env_ptr")
        .expect("itp");

    // Get array length
    let arr_len = cx
        .rt_call("rt_array_len", &[arr_ptr.into()])
        .unwrap()
        .into_int_value();

    // Allocate result array
    let result_ptr = cx
        .rt_call("rt_array_alloc", &[arr_len.into()])
        .unwrap()
        .into_pointer_value();

    // Build function type for indirect call: (ptr, elem) -> ret
    let param_llvm = turbo_ty_to_llvm_ctx(&param_tty, cx.context, cx.enum_max_slots);
    let fn_type = if ret_tty == TurboTy::Unit {
        cx.context
            .void_type()
            .fn_type(&[ptr_type.into(), param_llvm.into()], false)
    } else {
        let ret_llvm = turbo_ty_to_llvm_ctx(&ret_tty, cx.context, cx.enum_max_slots);
        ret_llvm.fn_type(&[ptr_type.into(), param_llvm.into()], false)
    };

    // Loop: for i in 0..arr_len
    let current_fn = cx.current_fn;
    let header_block = cx.context.append_basic_block(current_fn, "map_header");
    let body_block = cx.context.append_basic_block(current_fn, "map_body");
    let exit_block = cx.context.append_basic_block(current_fn, "map_exit");

    // Allocate loop index on the stack
    let idx_alloca = cx
        .builder
        .build_alloca(i64_type, "map_idx")
        .expect("alloca");
    cx.builder
        .build_store(idx_alloca, i64_type.const_int(0, false))
        .expect("store");

    cx.builder
        .build_unconditional_branch(header_block)
        .expect("br");
    cx.builder.position_at_end(header_block);

    let idx = cx
        .builder
        .build_load(i64_type, idx_alloca, "idx")
        .expect("load")
        .into_int_value();
    let cond = cx
        .builder
        .build_int_compare(IntPredicate::SLT, idx, arr_len, "cond")
        .expect("cmp");
    cx.builder
        .build_conditional_branch(cond, body_block, exit_block)
        .expect("br");

    cx.builder.position_at_end(body_block);
    let idx2 = cx
        .builder
        .build_load(i64_type, idx_alloca, "idx2")
        .expect("load")
        .into_int_value();

    // Get element (as i64)
    let raw_elem = cx
        .rt_call("rt_array_get", &[arr_ptr.into(), idx2.into()])
        .unwrap()
        .into_int_value();
    // Narrow to param type
    let typed_elem: BasicValueEnum = match &param_tty {
        TurboTy::Bool => cx
            .builder
            .build_int_truncate(raw_elem, cx.context.bool_type(), "trunc")
            .expect("trunc")
            .into(),
        TurboTy::Float => cx
            .builder
            .build_bit_cast(raw_elem, cx.context.f64_type(), "f2i")
            .expect("bc")
            .into(),
        _ => raw_elem.into(),
    };

    // Call closure
    let mapped_val = cx
        .builder
        .build_indirect_call(
            fn_type,
            fn_ptr,
            &[env_ptr.into(), typed_elem.into()],
            "mapped",
        )
        .expect("indirect_call");

    // Store result
    if let Some(mapped_basic) = mapped_val.try_as_basic_value().left() {
        let store_val = widen_for_storage(cx, mapped_basic);
        let idx3 = cx
            .builder
            .build_load(i64_type, idx_alloca, "idx3")
            .expect("load")
            .into_int_value();
        cx.rt_call(
            "rt_array_set",
            &[result_ptr.into(), idx3.into(), store_val.into()],
        );
    }

    let idx4 = cx
        .builder
        .build_load(i64_type, idx_alloca, "idx4")
        .expect("load")
        .into_int_value();
    let one = i64_type.const_int(1, false);
    let next_idx = cx
        .builder
        .build_int_add(idx4, one, "next_idx")
        .expect("add");
    cx.builder.build_store(idx_alloca, next_idx).expect("store");
    cx.builder
        .build_unconditional_branch(header_block)
        .expect("br");

    cx.builder.position_at_end(exit_block);

    Ok(Some((result_ptr.into(), TurboTy::Array(Box::new(ret_tty)))))
}

/// compile_builtin_filter_llvm: filter(arr, closure) -> [T]
fn compile_builtin_filter_llvm<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    if args.len() < 2 {
        return Ok(None);
    }
    let (arr_ptr, arr_tty) = compile_expr(cx, &args[0])?.unwrap();
    let (closure_ptr_val, fn_tty) = compile_expr(cx, &args[1])?.unwrap();

    let elem_tty = match &arr_tty {
        TurboTy::Array(inner) => *inner.clone(),
        _ => TurboTy::Int,
    };
    let param_tty = match &fn_tty {
        TurboTy::Fn(params, _) => params[0].clone(),
        _ => TurboTy::Int,
    };

    let ptr_type = cx.context.ptr_type(AddressSpace::default());
    let i8_type = cx.context.i8_type();
    let i64_type = cx.context.i64_type();

    let closure_ptr = closure_ptr_val.into_pointer_value();
    let fn_ptr_i64 = cx
        .builder
        .build_load(i64_type, closure_ptr, "filt_fn_i64")
        .expect("load")
        .into_int_value();
    let fn_ptr = cx
        .builder
        .build_int_to_ptr(fn_ptr_i64, ptr_type, "filt_fn_ptr")
        .expect("itp");
    let env_slot = unsafe {
        cx.builder
            .build_gep(
                i8_type,
                closure_ptr,
                &[i64_type.const_int(8, false)],
                "env_slot",
            )
            .expect("gep")
    };
    let env_ptr_i64 = cx
        .builder
        .build_load(i64_type, env_slot, "filt_env_i64")
        .expect("load")
        .into_int_value();
    let env_ptr = cx
        .builder
        .build_int_to_ptr(env_ptr_i64, ptr_type, "filt_env_ptr")
        .expect("itp");

    let arr_len = cx
        .rt_call("rt_array_len", &[arr_ptr.into()])
        .unwrap()
        .into_int_value();
    let result_ptr = cx
        .rt_call("rt_array_alloc", &[arr_len.into()])
        .unwrap()
        .into_pointer_value();

    let param_llvm = turbo_ty_to_llvm_ctx(&param_tty, cx.context, cx.enum_max_slots);
    let bool_type = cx.context.bool_type();
    let fn_type = bool_type.fn_type(&[ptr_type.into(), param_llvm.into()], false);

    let current_fn = cx.current_fn;
    let header_block = cx.context.append_basic_block(current_fn, "filt_header");
    let body_block = cx.context.append_basic_block(current_fn, "filt_body");
    let store_block = cx.context.append_basic_block(current_fn, "filt_store");
    let inc_block = cx.context.append_basic_block(current_fn, "filt_inc");
    let exit_block = cx.context.append_basic_block(current_fn, "filt_exit");

    let idx_alloca = cx
        .builder
        .build_alloca(i64_type, "filt_idx")
        .expect("alloca");
    let out_idx_alloca = cx
        .builder
        .build_alloca(i64_type, "filt_out_idx")
        .expect("alloca");
    cx.builder
        .build_store(idx_alloca, i64_type.const_int(0, false))
        .expect("store");
    cx.builder
        .build_store(out_idx_alloca, i64_type.const_int(0, false))
        .expect("store");

    cx.builder
        .build_unconditional_branch(header_block)
        .expect("br");
    cx.builder.position_at_end(header_block);
    let idx = cx
        .builder
        .build_load(i64_type, idx_alloca, "idx")
        .expect("load")
        .into_int_value();
    let cond = cx
        .builder
        .build_int_compare(IntPredicate::SLT, idx, arr_len, "cond")
        .expect("cmp");
    cx.builder
        .build_conditional_branch(cond, body_block, exit_block)
        .expect("br");

    cx.builder.position_at_end(body_block);
    let idx2 = cx
        .builder
        .build_load(i64_type, idx_alloca, "idx2")
        .expect("load")
        .into_int_value();
    let raw_elem = cx
        .rt_call("rt_array_get", &[arr_ptr.into(), idx2.into()])
        .unwrap()
        .into_int_value();
    let typed_elem: BasicValueEnum = match &param_tty {
        TurboTy::Bool => cx
            .builder
            .build_int_truncate(raw_elem, bool_type, "trunc")
            .expect("trunc")
            .into(),
        TurboTy::Float => cx
            .builder
            .build_bit_cast(raw_elem, cx.context.f64_type(), "bc")
            .expect("bc")
            .into(),
        _ => raw_elem.into(),
    };
    let pred_val = cx
        .builder
        .build_indirect_call(
            fn_type,
            fn_ptr,
            &[env_ptr.into(), typed_elem.into()],
            "pred",
        )
        .expect("indirect_call");
    let keep = match pred_val.try_as_basic_value().left() {
        Some(BasicValueEnum::IntValue(v)) => v,
        _ => i64_type.const_int(0, false),
    };
    let zero8 = cx.context.bool_type().const_int(0, false);
    let keep_bool = cx
        .builder
        .build_int_compare(IntPredicate::NE, keep, zero8, "keep")
        .expect("cmp");
    cx.builder
        .build_conditional_branch(keep_bool, store_block, inc_block)
        .expect("br");

    cx.builder.position_at_end(store_block);
    let raw_elem2 = cx
        .rt_call("rt_array_get", &[arr_ptr.into(), idx2.into()])
        .unwrap()
        .into_int_value();
    let out_idx = cx
        .builder
        .build_load(i64_type, out_idx_alloca, "out_idx")
        .expect("load")
        .into_int_value();
    cx.rt_call(
        "rt_array_set",
        &[result_ptr.into(), out_idx.into(), raw_elem2.into()],
    );
    let one = i64_type.const_int(1, false);
    let next_out = cx
        .builder
        .build_int_add(out_idx, one, "next_out")
        .expect("add");
    cx.builder
        .build_store(out_idx_alloca, next_out)
        .expect("store");
    cx.builder
        .build_unconditional_branch(inc_block)
        .expect("br");

    cx.builder.position_at_end(inc_block);
    let idx3 = cx
        .builder
        .build_load(i64_type, idx_alloca, "idx3")
        .expect("load")
        .into_int_value();
    let one2 = i64_type.const_int(1, false);
    let next_idx = cx
        .builder
        .build_int_add(idx3, one2, "next_idx")
        .expect("add");
    cx.builder.build_store(idx_alloca, next_idx).expect("store");
    cx.builder
        .build_unconditional_branch(header_block)
        .expect("br");

    cx.builder.position_at_end(exit_block);
    // Update result array length to actual number of kept elements
    let final_out_idx = cx
        .builder
        .build_load(i64_type, out_idx_alloca, "final_out")
        .expect("load");
    cx.builder
        .build_store(result_ptr, final_out_idx)
        .expect("store");

    Ok(Some((result_ptr.into(), arr_tty)))
}

/// compile_builtin_reduce_llvm: reduce(arr, init, closure) -> T
fn compile_builtin_reduce_llvm<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    if args.len() < 3 {
        return Ok(None);
    }
    let (arr_ptr, arr_tty) = compile_expr(cx, &args[0])?.unwrap();
    let (init_val, init_tty) = compile_expr(cx, &args[1])?.unwrap();
    let (closure_ptr_val, _fn_tty) = compile_expr(cx, &args[2])?.unwrap();

    let elem_tty = match &arr_tty {
        TurboTy::Array(inner) => *inner.clone(),
        _ => TurboTy::Int,
    };

    let ptr_type = cx.context.ptr_type(AddressSpace::default());
    let i8_type = cx.context.i8_type();
    let i64_type = cx.context.i64_type();

    let closure_ptr = closure_ptr_val.into_pointer_value();
    let fn_ptr_i64 = cx
        .builder
        .build_load(i64_type, closure_ptr, "red_fn_i64")
        .expect("load")
        .into_int_value();
    let fn_ptr = cx
        .builder
        .build_int_to_ptr(fn_ptr_i64, ptr_type, "red_fn_ptr")
        .expect("itp");
    let env_slot = unsafe {
        cx.builder
            .build_gep(
                i8_type,
                closure_ptr,
                &[i64_type.const_int(8, false)],
                "env_slot",
            )
            .expect("gep")
    };
    let env_ptr_i64 = cx
        .builder
        .build_load(i64_type, env_slot, "red_env_i64")
        .expect("load")
        .into_int_value();
    let env_ptr = cx
        .builder
        .build_int_to_ptr(env_ptr_i64, ptr_type, "red_env_ptr")
        .expect("itp");

    let arr_len = cx
        .rt_call("rt_array_len", &[arr_ptr.into()])
        .unwrap()
        .into_int_value();

    // Accumulator stored on stack (as i64)
    let acc_alloca = cx.builder.build_alloca(i64_type, "acc").expect("alloca");
    let init_i64 = widen_for_storage(cx, init_val);
    cx.builder.build_store(acc_alloca, init_i64).expect("store");

    let elem_llvm = turbo_ty_to_llvm_ctx(&elem_tty, cx.context, cx.enum_max_slots);
    let acc_llvm = turbo_ty_to_llvm_ctx(&init_tty, cx.context, cx.enum_max_slots);
    // closure: (env, acc, elem) -> acc
    let fn_type = acc_llvm.fn_type(&[ptr_type.into(), acc_llvm.into(), elem_llvm.into()], false);

    let current_fn = cx.current_fn;
    let header_block = cx.context.append_basic_block(current_fn, "red_header");
    let body_block = cx.context.append_basic_block(current_fn, "red_body");
    let exit_block = cx.context.append_basic_block(current_fn, "red_exit");

    let idx_alloca = cx
        .builder
        .build_alloca(i64_type, "red_idx")
        .expect("alloca");
    cx.builder
        .build_store(idx_alloca, i64_type.const_int(0, false))
        .expect("store");

    cx.builder
        .build_unconditional_branch(header_block)
        .expect("br");
    cx.builder.position_at_end(header_block);
    let idx = cx
        .builder
        .build_load(i64_type, idx_alloca, "idx")
        .expect("load")
        .into_int_value();
    let cond = cx
        .builder
        .build_int_compare(IntPredicate::SLT, idx, arr_len, "cond")
        .expect("cmp");
    cx.builder
        .build_conditional_branch(cond, body_block, exit_block)
        .expect("br");

    cx.builder.position_at_end(body_block);
    let idx2 = cx
        .builder
        .build_load(i64_type, idx_alloca, "idx2")
        .expect("load")
        .into_int_value();
    let raw_elem = cx
        .rt_call("rt_array_get", &[arr_ptr.into(), idx2.into()])
        .unwrap()
        .into_int_value();
    let acc_i64 = cx
        .builder
        .build_load(i64_type, acc_alloca, "acc_i64")
        .expect("load")
        .into_int_value();

    // Narrow both for the call
    let typed_elem: BasicValueEnum = match &elem_tty {
        TurboTy::Float => cx
            .builder
            .build_bit_cast(raw_elem, cx.context.f64_type(), "bc")
            .expect("bc")
            .into(),
        _ => raw_elem.into(),
    };
    let typed_acc: BasicValueEnum = match &init_tty {
        TurboTy::Float => cx
            .builder
            .build_bit_cast(acc_i64, cx.context.f64_type(), "bc_acc")
            .expect("bc")
            .into(),
        _ => acc_i64.into(),
    };

    let new_acc = cx
        .builder
        .build_indirect_call(
            fn_type,
            fn_ptr,
            &[env_ptr.into(), typed_acc.into(), typed_elem.into()],
            "new_acc",
        )
        .expect("indirect_call");

    if let Some(new_acc_val) = new_acc.try_as_basic_value().left() {
        let new_acc_i64 = widen_for_storage(cx, new_acc_val);
        cx.builder
            .build_store(acc_alloca, new_acc_i64)
            .expect("store");
    }

    let one = i64_type.const_int(1, false);
    let idx3 = cx
        .builder
        .build_load(i64_type, idx_alloca, "idx3")
        .expect("load")
        .into_int_value();
    let next_idx = cx
        .builder
        .build_int_add(idx3, one, "next_idx")
        .expect("add");
    cx.builder.build_store(idx_alloca, next_idx).expect("store");
    cx.builder
        .build_unconditional_branch(header_block)
        .expect("br");

    cx.builder.position_at_end(exit_block);
    let final_acc_i64 = cx
        .builder
        .build_load(i64_type, acc_alloca, "final_acc")
        .expect("load")
        .into_int_value();
    let final_acc: BasicValueEnum = match &init_tty {
        TurboTy::Float => cx
            .builder
            .build_bit_cast(final_acc_i64, cx.context.f64_type(), "bc_final")
            .expect("bc")
            .into(),
        _ => final_acc_i64.into(),
    };

    Ok(Some((final_acc, init_tty)))
}

// ── Built-in functions ──────────────────────────────────────────────

/// Serialize a struct to JSON: {"field1":val1,"field2":val2}
fn compile_struct_to_json_llvm<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    struct_ptr: BasicValueEnum<'ctx>,
    struct_name: &str,
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    let struct_layout = cx
        .struct_fields
        .get(struct_name)
        .ok_or_else(|| CodegenError {
            code: ErrorCode::E0400,
            message: format!("undefined struct: {struct_name}"),
        })?
        .clone();

    let i8_type = cx.context.i8_type();
    let i64_type = cx.context.i64_type();
    let ptr = struct_ptr.into_pointer_value();

    let mut result: BasicValueEnum = cx.create_string("{")?.into();

    for (i, (field_name, field_ty)) in struct_layout.iter().enumerate() {
        // Add key prefix
        let prefix = if i > 0 {
            format!(",\"{}\":", field_name)
        } else {
            format!("\"{}\":", field_name)
        };
        let prefix_ptr = cx.create_string(&prefix)?;
        result = cx
            .rt_call("rt_str_concat", &[result.into(), prefix_ptr.into()])
            .unwrap();

        // Load field value
        let offset = (i * 8) as u64;
        let field_ptr = if offset == 0 {
            ptr
        } else {
            unsafe {
                cx.builder
                    .build_gep(
                        i8_type,
                        ptr,
                        &[i64_type.const_int(offset, false)],
                        "json_field_ptr",
                    )
                    .expect("gep")
            }
        };
        let raw_val = cx
            .builder
            .build_load(i64_type, field_ptr, "json_field_val")
            .expect("load");

        // Convert field to JSON string representation
        let field_str = match field_ty {
            TurboTy::Str => {
                let str_ptr = cx
                    .builder
                    .build_int_to_ptr(
                        raw_val.into_int_value(),
                        cx.context.ptr_type(AddressSpace::default()),
                        "str_ptr",
                    )
                    .expect("itp");
                let quote = cx.create_string("\"")?;
                let tmp = cx
                    .rt_call("rt_str_concat", &[quote.into(), str_ptr.into()])
                    .unwrap();
                let quote2 = cx.create_string("\"")?;
                cx.rt_call("rt_str_concat", &[tmp.into(), quote2.into()])
                    .unwrap()
            }
            TurboTy::Int => cx.rt_call("rt_i64_to_str", &[raw_val.into()]).unwrap(),
            TurboTy::Bool => {
                let bool_val = cx
                    .builder
                    .build_int_truncate(raw_val.into_int_value(), cx.context.i8_type(), "trunc")
                    .expect("trunc");
                cx.rt_call("rt_bool_to_str", &[bool_val.into()]).unwrap()
            }
            TurboTy::Float => {
                let fval = cx
                    .builder
                    .build_bit_cast(raw_val.into_int_value(), cx.context.f64_type(), "i2f")
                    .expect("bc");
                cx.rt_call("rt_f64_to_str", &[fval.into()]).unwrap()
            }
            _ => cx.rt_call("rt_i64_to_str", &[raw_val.into()]).unwrap(),
        };

        result = cx
            .rt_call("rt_str_concat", &[result.into(), field_str.into()])
            .unwrap();
    }

    let suffix = cx.create_string("}")?;
    result = cx
        .rt_call("rt_str_concat", &[result.into(), suffix.into()])
        .unwrap();

    Ok(Some((result, TurboTy::Str)))
}

/// Serialize an array of structs to JSON: [item1,item2,...]
fn compile_array_to_json_llvm<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    arr_val: BasicValueEnum<'ctx>,
    struct_name: &str,
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    let i64_type = cx.context.i64_type();
    let arr_len = cx
        .rt_call("rt_array_len", &[arr_val.into()])
        .unwrap()
        .into_int_value();

    let mut result: BasicValueEnum = cx.create_string("[")?.into();

    let current_fn = cx.current_fn;
    let header = cx.context.append_basic_block(current_fn, "json_arr_header");
    let body = cx.context.append_basic_block(current_fn, "json_arr_body");
    let exit = cx.context.append_basic_block(current_fn, "json_arr_exit");

    let idx_alloca = cx
        .builder
        .build_alloca(i64_type, "json_idx")
        .expect("alloca");
    let result_alloca = cx
        .builder
        .build_alloca(cx.context.ptr_type(AddressSpace::default()), "json_result")
        .expect("alloca");
    cx.builder
        .build_store(idx_alloca, i64_type.const_int(0, false))
        .expect("store");
    cx.builder
        .build_store(result_alloca, result.into_pointer_value())
        .expect("store");
    cx.builder.build_unconditional_branch(header).expect("br");

    cx.builder.position_at_end(header);
    let idx = cx
        .builder
        .build_load(i64_type, idx_alloca, "idx")
        .expect("load")
        .into_int_value();
    let cond = cx
        .builder
        .build_int_compare(IntPredicate::SLT, idx, arr_len, "cond")
        .expect("cmp");
    cx.builder
        .build_conditional_branch(cond, body, exit)
        .expect("br");

    cx.builder.position_at_end(body);
    let idx2 = cx
        .builder
        .build_load(i64_type, idx_alloca, "idx2")
        .expect("load")
        .into_int_value();
    let cur_result = cx
        .builder
        .build_load(
            cx.context.ptr_type(AddressSpace::default()),
            result_alloca,
            "cur",
        )
        .expect("load");

    // Add comma if not first
    let zero = i64_type.const_int(0, false);
    let is_first = cx
        .builder
        .build_int_compare(IntPredicate::EQ, idx2, zero, "is_first")
        .expect("cmp");
    let comma_ptr = cx.create_string(",")?;
    let empty_ptr = cx.create_string("")?;
    let sep = cx
        .builder
        .build_select(is_first, empty_ptr, comma_ptr, "sep")
        .expect("select");
    let with_sep = cx
        .rt_call("rt_str_concat", &[cur_result.into(), sep.into()])
        .unwrap();

    // Get element and serialize
    let elem = cx
        .rt_call("rt_array_get", &[arr_val.into(), idx2.into()])
        .unwrap();
    let elem_ptr = cx
        .builder
        .build_int_to_ptr(
            elem.into_int_value(),
            cx.context.ptr_type(AddressSpace::default()),
            "elem_ptr",
        )
        .expect("itp");
    let sname = struct_name.to_string();
    let (elem_json, _) = compile_struct_to_json_llvm(cx, elem_ptr.into(), &sname)?.unwrap();
    let new_result = cx
        .rt_call("rt_str_concat", &[with_sep.into(), elem_json.into()])
        .unwrap();
    cx.builder
        .build_store(result_alloca, new_result.into_pointer_value())
        .expect("store");

    let one = i64_type.const_int(1, false);
    let next = cx.builder.build_int_add(idx2, one, "next").expect("add");
    cx.builder.build_store(idx_alloca, next).expect("store");
    cx.builder.build_unconditional_branch(header).expect("br");

    cx.builder.position_at_end(exit);
    let final_result = cx
        .builder
        .build_load(
            cx.context.ptr_type(AddressSpace::default()),
            result_alloca,
            "final",
        )
        .expect("load");
    let suffix = cx.create_string("]")?;
    let done = cx
        .rt_call("rt_str_concat", &[final_result.into(), suffix.into()])
        .unwrap();

    Ok(Some((done, TurboTy::Str)))
}

fn compile_print<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    if args.is_empty() {
        let ptr = cx.create_string("")?;
        cx.rt_call("rt_print_str", &[ptr.into()]);
        return Ok(None);
    }

    let result = compile_expr(cx, &args[0])?;
    if let Some((v, tty)) = result {
        match tty {
            TurboTy::Str => {
                // Generic functions return i64 even for Str; convert to ptr if needed
                let ptr_val: BasicValueEnum = match v {
                    BasicValueEnum::PointerValue(_) => v,
                    BasicValueEnum::IntValue(iv) => cx
                        .builder
                        .build_int_to_ptr(
                            iv,
                            cx.context.ptr_type(AddressSpace::default()),
                            "str_ptr",
                        )
                        .expect("itp")
                        .into(),
                    _ => v,
                };
                cx.rt_call("rt_print_str", &[ptr_val.into()]);
            }
            TurboTy::Float => {
                cx.rt_call("rt_print_f64", &[v.into()]);
            }
            TurboTy::Bool => {
                let iv = v.into_int_value();
                let iv = if iv.get_type().get_bit_width() > 8 {
                    cx.builder
                        .build_int_truncate(iv, cx.context.i8_type(), "tobool")
                        .expect("build_int_truncate failed")
                } else {
                    iv
                };
                cx.rt_call("rt_print_bool", &[iv.into()]);
            }
            TurboTy::Int => {
                let iv = v.into_int_value();
                let iv = if iv.get_type().get_bit_width() < 64 {
                    cx.builder
                        .build_int_s_extend(iv, cx.context.i64_type(), "ext")
                        .expect("build_int_s_extend failed")
                } else {
                    iv
                };
                cx.rt_call("rt_print_i64", &[iv.into()]);
            }
            TurboTy::Unit => {
                let ptr = cx.create_string("()")?;
                cx.rt_call("rt_print_str", &[ptr.into()]);
            }
            TurboTy::Struct(ref sname) => {
                // If Display is derived, call StructName__to_string
                let sname = sname.clone();
                let to_str_fn = format!("{sname}__to_string");
                if let Some(&ts_fn) = cx.user_fns.get(&to_str_fn) {
                    let s = cx
                        .builder
                        .build_direct_call(ts_fn, &[v.into()], "to_str")
                        .expect("call")
                        .try_as_basic_value()
                        .left()
                        .unwrap();
                    cx.rt_call("rt_print_str", &[s.into()]);
                } else {
                    // No Display, print struct name as placeholder
                    let ptr = cx.create_string(&format!("<{sname}>"))?;
                    cx.rt_call("rt_print_str", &[ptr.into()]);
                }
            }
            TurboTy::Array(_) => {
                // Print array as a bracketed list via rt_array_print_str if available
                let ptr = cx.create_string("<array>")?;
                cx.rt_call("rt_print_str", &[ptr.into()]);
            }
            TurboTy::Enum(_) => {
                // Enums: print the integer tag value
                let iv = match v {
                    BasicValueEnum::IntValue(i) => {
                        if i.get_type().get_bit_width() < 64 {
                            cx.builder
                                .build_int_s_extend(i, cx.context.i64_type(), "ext")
                                .expect("extend")
                        } else {
                            i
                        }
                    }
                    BasicValueEnum::PointerValue(p) => cx
                        .builder
                        .build_ptr_to_int(p, cx.context.i64_type(), "p2i")
                        .expect("p2i"),
                    _ => cx.context.i64_type().const_int(0, false),
                };
                cx.rt_call("rt_print_i64", &[iv.into()]);
            }
            TurboTy::Result(_, _) | TurboTy::Optional(_) => {
                let ptr = cx.rt_call("rt_result_to_str", &[v.into()]);
                if let Some(s) = ptr {
                    cx.rt_call("rt_print_str", &[s.into()]);
                } else {
                    let ptr = cx.create_string("<result>")?;
                    cx.rt_call("rt_print_str", &[ptr.into()]);
                }
            }
            _ => {
                // For other types, print as integer (best-effort)
                let iv = match v {
                    BasicValueEnum::IntValue(i) => i,
                    BasicValueEnum::PointerValue(p) => cx
                        .builder
                        .build_ptr_to_int(p, cx.context.i64_type(), "p2i")
                        .expect("p2i"),
                    _ => cx.context.i64_type().const_int(0, false),
                };
                cx.rt_call("rt_print_i64", &[iv.into()]);
            }
        }
    }
    Ok(None)
}

fn compile_panic<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    if args.is_empty() {
        let ptr = cx.create_string("explicit panic")?;
        cx.rt_call("rt_panic", &[ptr.into()]);
    } else {
        let (val, _) = compile_expr(cx, &args[0])?.unwrap();
        cx.rt_call("rt_panic", &[val.into()]);
    }
    cx.builder
        .build_unreachable()
        .expect("build_unreachable failed");
    let dead = cx.context.append_basic_block(cx.current_fn, "after_panic");
    cx.builder.position_at_end(dead);
    Ok(None)
}

fn compile_assert<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    if args.is_empty() {
        return Ok(None);
    }
    let (cond, _) = compile_expr(cx, &args[0])?.unwrap();
    let cond_bool = cx.to_bool(cond);

    let fail_block = cx.context.append_basic_block(cx.current_fn, "assert_fail");
    let ok_block = cx.context.append_basic_block(cx.current_fn, "assert_ok");

    cx.builder
        .build_conditional_branch(cond_bool, ok_block, fail_block)
        .expect("build_conditional_branch failed");

    cx.builder.position_at_end(fail_block);
    if args.len() > 1 {
        let (msg, _) = compile_expr(cx, &args[1])?.unwrap();
        cx.rt_call("rt_assert_fail", &[msg.into()]);
    } else {
        let ptr = cx.create_string("assertion failed")?;
        cx.rt_call("rt_assert_fail", &[ptr.into()]);
    }
    cx.builder
        .build_unreachable()
        .expect("build_unreachable failed");

    cx.builder.position_at_end(ok_block);
    Ok(None)
}

fn compile_assert_eq<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    args: &[Spanned<Expr>],
    negate: bool,
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    if args.len() < 2 {
        return Ok(None);
    }
    let (a, a_tty) = compile_expr(cx, &args[0])?.unwrap();
    let (b, _) = compile_expr(cx, &args[1])?.unwrap();

    let eq = if matches!(a_tty, TurboTy::Str) || a.is_pointer_value() && b.is_pointer_value() {
        // String comparison via rt_str_eq
        let eq_val = cx
            .rt_call("rt_str_eq", &[a.into(), b.into()])
            .unwrap()
            .into_int_value();
        let zero = cx.context.i8_type().const_int(0, false);
        let cmp = cx
            .builder
            .build_int_compare(IntPredicate::NE, eq_val, zero, "str_eq")
            .expect("icmp");
        if negate {
            cx.builder.build_not(cmp, "not_eq").expect("not")
        } else {
            cmp
        }
    } else {
        let ai = a.into_int_value();
        let bi = b.into_int_value();
        let pred = if negate {
            IntPredicate::NE
        } else {
            IntPredicate::EQ
        };
        cx.builder
            .build_int_compare(pred, ai, bi, "assert_eq")
            .expect("build_int_compare failed")
    };

    let fail_block = cx
        .context
        .append_basic_block(cx.current_fn, "assert_eq_fail");
    let ok_block = cx.context.append_basic_block(cx.current_fn, "assert_eq_ok");

    cx.builder
        .build_conditional_branch(eq, ok_block, fail_block)
        .expect("build_conditional_branch failed");

    cx.builder.position_at_end(fail_block);
    // Simple assert fail for now
    let ptr = cx.create_string("assertion failed: values not equal")?;
    cx.rt_call("rt_assert_fail", &[ptr.into()]);
    cx.builder
        .build_unreachable()
        .expect("build_unreachable failed");

    cx.builder.position_at_end(ok_block);
    Ok(None)
}

fn compile_len<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    if args.is_empty() {
        return Ok(None);
    }
    let (val, tty) = compile_expr(cx, &args[0])?.unwrap();
    let result = match tty {
        TurboTy::Str => cx.rt_call("rt_str_len", &[val.into()]).unwrap(),
        TurboTy::Array(_) => cx.rt_call("rt_array_len", &[val.into()]).unwrap(),
        _ => cx.context.i64_type().const_int(0, false).into(),
    };
    Ok(Some((result, TurboTy::Int)))
}

fn compile_abs<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    if args.is_empty() {
        return Ok(None);
    }
    let (val, tty) = compile_expr(cx, &args[0])?.unwrap();
    match tty {
        TurboTy::Int => {
            let iv = val.into_int_value();
            let zero = cx.context.i64_type().const_int(0, false);
            let neg = cx
                .builder
                .build_int_neg(iv, "neg")
                .expect("build_int_neg failed");
            let is_neg = cx
                .builder
                .build_int_compare(IntPredicate::SLT, iv, zero, "is_neg")
                .expect("build_int_compare failed");
            let result: BasicValueEnum = cx
                .builder
                .build_select(
                    is_neg,
                    BasicValueEnum::IntValue(neg),
                    BasicValueEnum::IntValue(iv),
                    "abs",
                )
                .expect("build_select failed");
            Ok(Some((result, TurboTy::Int)))
        }
        TurboTy::Float => {
            // Use llvm.fabs intrinsic via negation + select
            let fv = val.into_float_value();
            let zero = cx.context.f64_type().const_float(0.0);
            let neg = cx
                .builder
                .build_float_neg(fv, "fneg")
                .expect("build_float_neg failed");
            let is_neg = cx
                .builder
                .build_float_compare(FloatPredicate::OLT, fv, zero, "is_neg")
                .expect("build_float_compare failed");
            let result: BasicValueEnum = cx
                .builder
                .build_select(
                    is_neg,
                    BasicValueEnum::FloatValue(neg),
                    BasicValueEnum::FloatValue(fv),
                    "fabs",
                )
                .expect("build_select failed");
            Ok(Some((result, TurboTy::Float)))
        }
        _ => Ok(Some((val, tty))),
    }
}

fn compile_min_max<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    args: &[Spanned<Expr>],
    is_min: bool,
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    if args.len() < 2 {
        return Ok(None);
    }
    let (a, tty) = compile_expr(cx, &args[0])?.unwrap();
    let (b, _) = compile_expr(cx, &args[1])?.unwrap();

    let cmp_pred = if is_min {
        IntPredicate::SLT
    } else {
        IntPredicate::SGT
    };

    let ai = a.into_int_value();
    let bi = b.into_int_value();
    let cond = cx
        .builder
        .build_int_compare(cmp_pred, ai, bi, "cmp")
        .expect("build_int_compare failed");
    let result: BasicValueEnum = cx
        .builder
        .build_select(
            cond,
            BasicValueEnum::IntValue(ai),
            BasicValueEnum::IntValue(bi),
            if is_min { "min" } else { "max" },
        )
        .expect("build_select failed");
    Ok(Some((result, tty)))
}

fn compile_to_str_builtin<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    if args.is_empty() {
        return Ok(None);
    }
    let (val, tty) = compile_expr(cx, &args[0])?.unwrap();
    let str_val = convert_to_str(cx, val, &tty)?;
    Ok(Some((str_val, TurboTy::Str)))
}

fn compile_stdlib_1arg_rt<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    args: &[Spanned<Expr>],
    rt_name: &str,
    ret_tty: TurboTy,
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    if args.is_empty() {
        return Ok(None);
    }
    let (a, _) = compile_expr(cx, &args[0])?.unwrap();
    let result = cx.rt_call(rt_name, &[a.into()]).unwrap();
    Ok(Some((result, ret_tty)))
}

fn compile_stdlib_2arg_rt<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    args: &[Spanned<Expr>],
    rt_name: &str,
    ret_tty: TurboTy,
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    if args.len() < 2 {
        return Ok(None);
    }
    let (a, _) = compile_expr(cx, &args[0])?.unwrap();
    let (b, _) = compile_expr(cx, &args[1])?.unwrap();
    let result = cx.rt_call(rt_name, &[a.into(), b.into()]).unwrap();
    Ok(Some((result, ret_tty)))
}

#[allow(unused_variables)]
fn compile_method_call<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    obj: BasicValueEnum<'ctx>,
    obj_tty: &TurboTy,
    field: &str,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    Err(CodegenError {
        code: ErrorCode::E0400,
        message: format!("LLVM: method call `{field}` not yet implemented"),
    })
}
