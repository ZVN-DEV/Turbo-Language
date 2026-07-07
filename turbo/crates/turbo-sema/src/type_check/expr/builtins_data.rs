//! Filesystem/path, array, time, and HTTP builtin signatures. Part of the
//! [`super`] expression-checking implementation.

use turbo_ast::*;

use crate::{Checker, HandleKind, Ty};

impl Checker {
    pub(crate) fn check_builtin_fs_path(
        &mut self,
        name: &str,
        args: &[Spanned<Expr>],
        callee: &Spanned<Expr>,
    ) -> Option<Ty> {
        if name == "file_exists" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0100,
                    format!("file_exists() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let path_ty = self.check_expr(&args[0]);
            if !path_ty.is_error() && path_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("file_exists() expects str, found `{path_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Bool);
        }
        // delete_file(path: str) -> bool
        if name == "delete_file" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0100,
                    format!("delete_file() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let path_ty = self.check_expr(&args[0]);
            if !path_ty.is_error() && path_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("delete_file() expects str, found `{path_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Bool);
        }
        // list_dir(path: str) -> [str]
        if name == "list_dir" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0100,
                    format!("list_dir() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let path_ty = self.check_expr(&args[0]);
            if !path_ty.is_error() && path_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("list_dir() expects str, found `{path_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Array(Box::new(Ty::Str)));
        }
        // mkdir(path: str) -> bool
        if name == "mkdir" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0100,
                    format!("mkdir() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let path_ty = self.check_expr(&args[0]);
            if !path_ty.is_error() && path_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("mkdir() expects str, found `{path_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Bool);
        }
        // path_join(a: str, b: str) -> str
        if name == "path_join" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0100,
                    format!("path_join() takes exactly 2 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let a_ty = self.check_expr(&args[0]);
            let b_ty = self.check_expr(&args[1]);
            if !a_ty.is_error() && a_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("path_join() first argument must be str, found `{a_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !b_ty.is_error() && b_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("path_join() second argument must be str, found `{b_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::Str);
        }
        // path_dir/path_base/path_ext: (str) -> str
        if name == "path_dir" || name == "path_base" || name == "path_ext" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0100,
                    format!("{name}() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let path_ty = self.check_expr(&args[0]);
            if !path_ty.is_error() && path_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("{name}() expects str, found `{path_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Str);
        }

        // ── Collection builtins ────────────────────────
        // sort(arr) -> [T]
        None
    }

    pub(crate) fn check_builtin_array(
        &mut self,
        name: &str,
        args: &[Spanned<Expr>],
        callee: &Spanned<Expr>,
    ) -> Option<Ty> {
        if name == "sort" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0100,
                    format!("sort() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let arr_ty = self.check_expr(&args[0]);
            match &arr_ty {
                Ty::Array(_) => return Some(arr_ty),
                _ if arr_ty.is_error() => return Some(Ty::Error),
                _ => {
                    self.error(
                        ErrorCode::E0133,
                        format!("sort() expects an array, found `{arr_ty}`"),
                        args[0].span.clone(),
                    );
                    return Some(Ty::Error);
                }
            }
        }
        // reverse(arr) -> [T]
        if name == "reverse" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0100,
                    format!("reverse() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let arr_ty = self.check_expr(&args[0]);
            match &arr_ty {
                Ty::Array(_) => return Some(arr_ty),
                _ if arr_ty.is_error() => return Some(Ty::Error),
                _ => {
                    self.error(
                        ErrorCode::E0133,
                        format!("reverse() expects an array, found `{arr_ty}`"),
                        args[0].span.clone(),
                    );
                    return Some(Ty::Error);
                }
            }
        }
        // array_contains(arr, val) -> bool
        if name == "array_contains" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0100,
                    format!(
                        "array_contains() takes exactly 2 arguments, got {}",
                        args.len()
                    ),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let arr_ty = self.check_expr(&args[0]);
            let val_ty = self.check_expr(&args[1]);
            let elem_ty = match &arr_ty {
                Ty::Array(inner) => *inner.clone(),
                _ if arr_ty.is_error() => Ty::Error,
                _ => {
                    self.error(
                        ErrorCode::E0133,
                        format!(
                            "array_contains() first argument must be an array, found `{arr_ty}`"
                        ),
                        args[0].span.clone(),
                    );
                    return Some(Ty::Error);
                }
            };
            if !elem_ty.is_error() && !val_ty.is_error() && elem_ty != val_ty {
                self.error(
                                ErrorCode::E0100,
                                format!("array_contains() value type `{val_ty}` doesn't match array element type `{elem_ty}`"),
                                args[1].span.clone(),
                            );
            }
            return Some(Ty::Bool);
        }
        // slice(arr, start, end) -> [T]
        if name == "slice" {
            if args.len() != 3 {
                self.error(
                    ErrorCode::E0100,
                    format!("slice() takes exactly 3 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let arr_ty = self.check_expr(&args[0]);
            let start_ty = self.check_expr(&args[1]);
            let end_ty = self.check_expr(&args[2]);
            if !start_ty.is_error() && !start_ty.is_integer() {
                self.error(
                    ErrorCode::E0100,
                    format!("slice() start must be integer, found `{start_ty}`"),
                    args[1].span.clone(),
                );
            }
            if !end_ty.is_error() && !end_ty.is_integer() {
                self.error(
                    ErrorCode::E0100,
                    format!("slice() end must be integer, found `{end_ty}`"),
                    args[2].span.clone(),
                );
            }
            match &arr_ty {
                Ty::Array(_) => return Some(arr_ty),
                _ if arr_ty.is_error() => return Some(Ty::Error),
                _ => {
                    self.error(
                        ErrorCode::E0133,
                        format!("slice() first argument must be an array, found `{arr_ty}`"),
                        args[0].span.clone(),
                    );
                    return Some(Ty::Error);
                }
            }
        }
        // any(arr, fn) -> bool
        if name == "any" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0513,
                    format!("any() takes exactly 2 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let arr_ty = self.check_expr(&args[0]);
            if let Ty::Array(ref inner) = arr_ty {
                self.closure_param_hint = Some(vec![*inner.clone()]);
            }
            let fn_ty = self.check_expr(&args[1]);
            let elem_ty = match &arr_ty {
                Ty::Array(inner) => *inner.clone(),
                _ if arr_ty.is_error() => Ty::Error,
                _ => {
                    self.error(
                        ErrorCode::E0133,
                        format!("any() first argument must be an array, found `{arr_ty}`"),
                        args[0].span.clone(),
                    );
                    return Some(Ty::Error);
                }
            };
            match &fn_ty {
                Ty::Fn(params, ret) => {
                    if params.len() != 1 {
                        self.error(
                            ErrorCode::E0133,
                            format!(
                                "any() callback must take 1 parameter, takes {}",
                                params.len()
                            ),
                            args[1].span.clone(),
                        );
                    } else if !elem_ty.is_error() && !params[0].is_error() && elem_ty != params[0] {
                        self.error(
                                        ErrorCode::E0100,
                                        format!("any() callback parameter type `{}` doesn't match array element type `{}`", params[0], elem_ty),
                                        args[1].span.clone(),
                                    );
                    }
                    if **ret != Ty::Bool && !ret.is_error() {
                        self.error(
                            ErrorCode::E0133,
                            format!("any() callback must return `bool`, returns `{}`", ret),
                            args[1].span.clone(),
                        );
                    }
                    return Some(Ty::Bool);
                }
                _ if fn_ty.is_error() => return Some(Ty::Error),
                _ => {
                    self.error(
                        ErrorCode::E0133,
                        format!("any() second argument must be a function, found `{fn_ty}`"),
                        args[1].span.clone(),
                    );
                    return Some(Ty::Error);
                }
            }
        }
        // all(arr, fn) -> bool
        if name == "all" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0513,
                    format!("all() takes exactly 2 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let arr_ty = self.check_expr(&args[0]);
            if let Ty::Array(ref inner) = arr_ty {
                self.closure_param_hint = Some(vec![*inner.clone()]);
            }
            let fn_ty = self.check_expr(&args[1]);
            let elem_ty = match &arr_ty {
                Ty::Array(inner) => *inner.clone(),
                _ if arr_ty.is_error() => Ty::Error,
                _ => {
                    self.error(
                        ErrorCode::E0133,
                        format!("all() first argument must be an array, found `{arr_ty}`"),
                        args[0].span.clone(),
                    );
                    return Some(Ty::Error);
                }
            };
            match &fn_ty {
                Ty::Fn(params, ret) => {
                    if params.len() != 1 {
                        self.error(
                            ErrorCode::E0133,
                            format!(
                                "all() callback must take 1 parameter, takes {}",
                                params.len()
                            ),
                            args[1].span.clone(),
                        );
                    } else if !elem_ty.is_error() && !params[0].is_error() && elem_ty != params[0] {
                        self.error(
                                        ErrorCode::E0100,
                                        format!("all() callback parameter type `{}` doesn't match array element type `{}`", params[0], elem_ty),
                                        args[1].span.clone(),
                                    );
                    }
                    if **ret != Ty::Bool && !ret.is_error() {
                        self.error(
                            ErrorCode::E0133,
                            format!("all() callback must return `bool`, returns `{}`", ret),
                            args[1].span.clone(),
                        );
                    }
                    return Some(Ty::Bool);
                }
                _ if fn_ty.is_error() => return Some(Ty::Error),
                _ => {
                    self.error(
                        ErrorCode::E0133,
                        format!("all() second argument must be a function, found `{fn_ty}`"),
                        args[1].span.clone(),
                    );
                    return Some(Ty::Error);
                }
            }
        }

        // ── Date/Time builtins ─────────────────────────
        // time_now() -> f64
        None
    }

    pub(crate) fn check_builtin_time(
        &mut self,
        name: &str,
        args: &[Spanned<Expr>],
        callee: &Spanned<Expr>,
    ) -> Option<Ty> {
        if name == "time_now" {
            if !args.is_empty() {
                self.error(
                    ErrorCode::E0100,
                    format!("time_now() takes 0 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            return Some(Ty::F64);
        }
        // time_ms() -> i64
        if name == "time_ms" {
            if !args.is_empty() {
                self.error(
                    ErrorCode::E0100,
                    format!("time_ms() takes 0 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            return Some(Ty::I64);
        }
        // format_time(timestamp: f64, format: str) -> str
        if name == "format_time" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0100,
                    format!(
                        "format_time() takes exactly 2 arguments, got {}",
                        args.len()
                    ),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let ts_ty = self.check_expr(&args[0]);
            let fmt_ty = self.check_expr(&args[1]);
            if !ts_ty.is_error() && ts_ty != Ty::F64 && ts_ty != Ty::F32 {
                self.error(
                    ErrorCode::E0100,
                    format!("format_time() first argument must be float, found `{ts_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !fmt_ty.is_error() && fmt_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("format_time() second argument must be str, found `{fmt_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::Str);
        }

        // sleep(ms: i64) -> () — sleep the current thread
        if name == "sleep" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0100,
                    format!("sleep() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let ms_ty = self.check_expr(&args[0]);
            if !ms_ty.is_error() && !ms_ty.is_integer() {
                self.error(
                    ErrorCode::E0100,
                    format!("sleep() expects integer (milliseconds), found `{ms_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Unit);
        }

        // ── HTTP builtins ───────────────────────────────
        // http_get(url: str) -> str
        None
    }

    pub(crate) fn check_builtin_http(
        &mut self,
        name: &str,
        args: &[Spanned<Expr>],
        callee: &Spanned<Expr>,
    ) -> Option<Ty> {
        if name == "http_get" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0513,
                    format!("http_get() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let url_ty = self.check_expr(&args[0]);
            if !url_ty.is_error() && url_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("http_get() expects str, found `{url_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Str);
        }
        // http_post(url: str, body: str) -> str
        if name == "http_post" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0513,
                    format!("http_post() takes exactly 2 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let url_ty = self.check_expr(&args[0]);
            let body_ty = self.check_expr(&args[1]);
            if !url_ty.is_error() && url_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("http_post() first argument must be str, found `{url_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !body_ty.is_error() && body_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("http_post() second argument must be str, found `{body_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::Str);
        }
        // http_post_with_headers(url: str, body: str, headers: str) -> str
        if name == "http_post_with_headers" {
            if args.len() != 3 {
                self.error(
                    ErrorCode::E0513,
                    format!(
                        "http_post_with_headers() takes exactly 3 arguments, got {}",
                        args.len()
                    ),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let url_ty = self.check_expr(&args[0]);
            let body_ty = self.check_expr(&args[1]);
            let headers_ty = self.check_expr(&args[2]);
            if !url_ty.is_error() && url_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!(
                        "http_post_with_headers() first argument must be str, found `{url_ty}`"
                    ),
                    args[0].span.clone(),
                );
            }
            if !body_ty.is_error() && body_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!(
                        "http_post_with_headers() second argument must be str, found `{body_ty}`"
                    ),
                    args[1].span.clone(),
                );
            }
            if !headers_ty.is_error() && headers_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!(
                        "http_post_with_headers() third argument must be str, found `{headers_ty}`"
                    ),
                    args[2].span.clone(),
                );
            }
            return Some(Ty::Str);
        }

        // ── HTTP server builtins ──────────────────────────
        // http_server(port: i64) -> i64          (binds 127.0.0.1)
        // http_server_public(port: i64) -> i64   (binds 0.0.0.0)
        if name == "http_server" || name == "http_server_public" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0100,
                    format!("{name}() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let port_ty = self.check_expr(&args[0]);
            if !port_ty.is_error() && !port_ty.is_integer() {
                self.error(
                    ErrorCode::E0100,
                    format!("{name}() expects integer port, found `{port_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Handle(HandleKind::HttpServer));
        }
        // route(server: i64, method: str, path: str, handler: fn(str) -> str)
        if name == "route" {
            if args.len() != 4 {
                self.error(
                    ErrorCode::E0513,
                    format!("route() takes exactly 4 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let server_ty = self.check_expr(&args[0]);
            let method_ty = self.check_expr(&args[1]);
            let path_ty = self.check_expr(&args[2]);
            // Set hint so closure param types can be inferred
            self.closure_param_hint = Some(vec![Ty::Str]);
            let handler_ty = self.check_expr(&args[3]);
            if !server_ty.is_error() && !server_ty.is_handle_or_int(HandleKind::HttpServer) {
                self.error(
                    ErrorCode::E0133,
                    format!("route() first argument must be server id (i64), found `{server_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !method_ty.is_error() && method_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!(
                        "route() second argument must be str (HTTP method), found `{method_ty}`"
                    ),
                    args[1].span.clone(),
                );
            }
            if !path_ty.is_error() && path_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("route() third argument must be str (path), found `{path_ty}`"),
                    args[2].span.clone(),
                );
            }
            match &handler_ty {
                Ty::Fn(params, ret) => {
                    if params.len() != 1 {
                        self.error(
                            ErrorCode::E0133,
                            format!(
                                "route() handler must take 1 parameter (request), takes {}",
                                params.len()
                            ),
                            args[3].span.clone(),
                        );
                    } else if !params[0].is_error() && params[0] != Ty::Str {
                        self.error(
                            ErrorCode::E0133,
                            format!(
                                "route() handler parameter must be str, found `{}`",
                                params[0]
                            ),
                            args[3].span.clone(),
                        );
                    }
                    if !ret.is_error() && **ret != Ty::Str {
                        self.error(
                            ErrorCode::E0100,
                            format!("route() handler must return str, returns `{}`", ret),
                            args[3].span.clone(),
                        );
                    }
                }
                _ if handler_ty.is_error() => {}
                _ => {
                    self.error(
                        ErrorCode::E0133,
                        format!("route() fourth argument must be a function, found `{handler_ty}`"),
                        args[3].span.clone(),
                    );
                }
            }
            return Some(Ty::Unit);
        }
        // http_listen(server: i64) -> ()
        if name == "http_listen" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0513,
                    format!("http_listen() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let server_ty = self.check_expr(&args[0]);
            if !server_ty.is_error() && !server_ty.is_handle_or_int(HandleKind::HttpServer) {
                self.error(
                    ErrorCode::E0100,
                    format!("http_listen() expects server id (i64), found `{server_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Unit);
        }
        // respond*(status: i64, body: str) -> str
        if matches!(
            name,
            "respond" | "respond_text" | "respond_html" | "respond_json"
        ) {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0513,
                    format!("{name}() takes exactly 2 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let status_ty = self.check_expr(&args[0]);
            let body_ty = self.check_expr(&args[1]);
            if !status_ty.is_error() && !status_ty.is_integer() {
                self.error(
                    ErrorCode::E0133,
                    format!(
                        "{name}() first argument must be integer status code, found `{status_ty}`"
                    ),
                    args[0].span.clone(),
                );
            }
            if !body_ty.is_error() && body_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("{name}() second argument must be str, found `{body_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::Str);
        }
        // request_body(req: str) -> str
        if name == "request_body" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0100,
                    format!(
                        "request_body() takes exactly 1 argument, got {}",
                        args.len()
                    ),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let req_ty = self.check_expr(&args[0]);
            if !req_ty.is_error() && req_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("request_body() expects str, found `{req_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Str);
        }

        // request_method(req: str) -> str, request_path(req: str) -> str
        if name == "request_method" || name == "request_path" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0100,
                    format!("{name}() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            self.check_expr(&args[0]);
            return Some(Ty::Str);
        }
        // request_query(req: str, key: str) -> str
        // request_header(req: str, name: str) -> str
        if name == "request_query" || name == "request_header" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0100,
                    format!("{name}() takes exactly 2 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            self.check_expr(&args[0]);
            self.check_expr(&args[1]);
            return Some(Ty::Str);
        }

        // ── JSON builtins ───────────────────────────────
        // json_get(json: str, key: str) -> str
        None
    }
}
