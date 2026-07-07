//! HTTP/networking and JSON built-ins.

use cranelift::prelude::*;
use cranelift_module::Module;
use turbo_ast::*;

use crate::turbo_types::{CodegenError, MaybeTyped, TurboTy};
use crate::{compile_expr, Ctx};

use super::*;

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
