//! Builtin-call dispatch plus the core, string, IO/env, math, and
//! conversion builtin signatures. Part of the [`super`] expression-checking
//! implementation.

use turbo_ast::*;

use crate::{types_compatible, Checker, Ty};

impl Checker {
    pub(crate) fn check_builtin_call(
        &mut self,
        name: &str,
        args: &[Spanned<Expr>],
        callee: &Spanned<Expr>,
    ) -> Option<Ty> {
        if let Some(t) = self.check_builtin_core(name, args, callee) {
            return Some(t);
        }
        if let Some(t) = self.check_builtin_string(name, args, callee) {
            return Some(t);
        }
        if let Some(t) = self.check_builtin_io_env(name, args, callee) {
            return Some(t);
        }
        if let Some(t) = self.check_builtin_math(name, args, callee) {
            return Some(t);
        }
        if let Some(t) = self.check_builtin_convert(name, args, callee) {
            return Some(t);
        }
        if let Some(t) = self.check_builtin_fs_path(name, args, callee) {
            return Some(t);
        }
        if let Some(t) = self.check_builtin_array(name, args, callee) {
            return Some(t);
        }
        if let Some(t) = self.check_builtin_time(name, args, callee) {
            return Some(t);
        }
        if let Some(t) = self.check_builtin_http(name, args, callee) {
            return Some(t);
        }
        if let Some(t) = self.check_builtin_json(name, args, callee) {
            return Some(t);
        }
        if let Some(t) = self.check_builtin_concurrency(name, args, callee) {
            return Some(t);
        }
        if let Some(t) = self.check_builtin_hashmap(name, args, callee) {
            return Some(t);
        }
        if let Some(t) = self.check_builtin_refs(name, args, callee) {
            return Some(t);
        }
        None
    }

    pub(crate) fn check_builtin_core(
        &mut self,
        name: &str,
        args: &[Spanned<Expr>],
        callee: &Spanned<Expr>,
    ) -> Option<Ty> {
        if name == "print" {
            if args.len() > 1 {
                self.error(
                    ErrorCode::E0513,
                    format!("print() takes at most 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
            }
            for arg in args {
                self.check_expr(arg);
            }
            return Some(Ty::Unit);
        }
        if name == "panic" {
            if args.len() > 1 {
                self.error(
                    ErrorCode::E0513,
                    format!("panic() takes at most 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
            }
            for arg in args {
                self.check_expr(arg);
            }
            return Some(Ty::Unit);
        }
        if name == "assert" {
            if args.is_empty() {
                self.error(
                    ErrorCode::E0513,
                    "assert() requires at least one argument".to_string(),
                    callee.span.clone(),
                );
            } else if args.len() > 2 {
                self.error(
                    ErrorCode::E0513,
                    format!("assert() takes at most 2 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
            }
            if !args.is_empty() {
                let cond_ty = self.check_expr(&args[0]);
                if !cond_ty.is_error() && cond_ty != Ty::Bool {
                    self.error(
                        ErrorCode::E0133,
                        format!("assert() condition must be `bool`, found `{cond_ty}`"),
                        args[0].span.clone(),
                    );
                }
                // Optional message argument
                if args.len() > 1 {
                    self.check_expr(&args[1]);
                }
                // Type-check remaining args even if too many
                for arg in args.iter().skip(2) {
                    self.check_expr(arg);
                }
            }
            return Some(Ty::Unit);
        }
        if name == "assert_eq" || name == "assert_ne" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0100,
                    format!("{name}() takes exactly 2 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
            }
            if args.len() >= 2 {
                let left_ty = self.check_expr(&args[0]);
                let right_ty = self.check_expr(&args[1]);
                if !left_ty.is_error()
                    && !right_ty.is_error()
                    && !types_compatible(&left_ty, &right_ty)
                    && left_ty != right_ty
                {
                    self.error(ErrorCode::E0100,
                                    format!("{name}() arguments must be the same type: left is `{left_ty}`, right is `{right_ty}`"),
                                    callee.span.clone(),
                                );
                }
            }
            for arg in args {
                self.check_expr(arg);
            }
            return Some(Ty::Unit);
        }
        if name == "len" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0513,
                    format!("len() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let arg_ty = self.check_expr(&args[0]);
            match &arg_ty {
                Ty::Array(_) => return Some(Ty::I64),
                Ty::Str => return Some(Ty::I64),
                _ if arg_ty.is_error() => return Some(Ty::Error),
                _ => {
                    self.error(
                        ErrorCode::E0133,
                        format!("len() expects array or string, found `{arg_ty}`"),
                        args[0].span.clone(),
                    );
                    return Some(Ty::Error);
                }
            }
        }

        if name == "push" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0513,
                    format!("push() takes exactly 2 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let arr_ty = self.check_expr(&args[0]);
            let elem_ty = self.check_expr(&args[1]);
            match &arr_ty {
                Ty::Array(inner) => {
                    if !elem_ty.is_error() && !inner.is_error() && **inner != elem_ty {
                        self.error(
                                        ErrorCode::E0133,
                                        format!(
                                            "push() element type `{elem_ty}` does not match array element type `{inner}`"
                                        ),
                                        args[1].span.clone(),
                                    );
                    }
                    return Some(arr_ty);
                }
                _ if arr_ty.is_error() => return Some(Ty::Error),
                _ => {
                    self.error(
                        ErrorCode::E0133,
                        format!("push() expects array as first argument, found `{arr_ty}`"),
                        args[0].span.clone(),
                    );
                    return Some(Ty::Error);
                }
            }
        }

        if name == "abs" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0513,
                    format!("abs() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let arg_ty = self.check_expr(&args[0]);
            if !arg_ty.is_error() && !arg_ty.is_numeric() {
                self.error(
                    ErrorCode::E0133,
                    format!("abs() expects numeric type, found `{arg_ty}`"),
                    args[0].span.clone(),
                );
            }
            if arg_ty.is_float() {
                return Some(Ty::F64);
            }
            return Some(Ty::I64);
        }
        if name == "min" || name == "max" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0100,
                    format!("{}() takes exactly 2 arguments, got {}", name, args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let a_ty = self.check_expr(&args[0]);
            let b_ty = self.check_expr(&args[1]);
            if !a_ty.is_error() && !a_ty.is_numeric() {
                self.error(
                    ErrorCode::E0100,
                    format!("{}() expects numeric types, found `{a_ty}`", name),
                    args[0].span.clone(),
                );
            }
            if !b_ty.is_error() && !b_ty.is_numeric() {
                self.error(
                    ErrorCode::E0100,
                    format!("{}() expects numeric types, found `{b_ty}`", name),
                    args[1].span.clone(),
                );
            }
            if a_ty.is_float() || b_ty.is_float() {
                return Some(Ty::F64);
            }
            return Some(Ty::I64);
        }
        None
    }

    pub(crate) fn check_builtin_string(
        &mut self,
        name: &str,
        args: &[Spanned<Expr>],
        callee: &Spanned<Expr>,
    ) -> Option<Ty> {
        if name == "to_str" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0513,
                    format!("to_str() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            self.check_expr(&args[0]);
            return Some(Ty::Str);
        }

        // ── Stdlib string functions ──────────────────────
        if name == "split" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0513,
                    format!("split() takes exactly 2 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let s_ty = self.check_expr(&args[0]);
            let sep_ty = self.check_expr(&args[1]);
            if !s_ty.is_error() && s_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("split() first argument must be str, found `{s_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !sep_ty.is_error() && sep_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("split() second argument must be str, found `{sep_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::Array(Box::new(Ty::Str)));
        }
        if name == "trim" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0513,
                    format!("trim() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let s_ty = self.check_expr(&args[0]);
            if !s_ty.is_error() && s_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("trim() expects str, found `{s_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Str);
        }
        if name == "upper" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0100,
                    format!("upper() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let s_ty = self.check_expr(&args[0]);
            if !s_ty.is_error() && s_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("upper() expects str, found `{s_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Str);
        }
        if name == "lower" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0100,
                    format!("lower() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let s_ty = self.check_expr(&args[0]);
            if !s_ty.is_error() && s_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("lower() expects str, found `{s_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Str);
        }
        if name == "starts_with" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0513,
                    format!(
                        "starts_with() takes exactly 2 arguments, got {}",
                        args.len()
                    ),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let s_ty = self.check_expr(&args[0]);
            let prefix_ty = self.check_expr(&args[1]);
            if !s_ty.is_error() && s_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("starts_with() first argument must be str, found `{s_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !prefix_ty.is_error() && prefix_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("starts_with() second argument must be str, found `{prefix_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::Bool);
        }
        if name == "ends_with" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0513,
                    format!("ends_with() takes exactly 2 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let s_ty = self.check_expr(&args[0]);
            let suffix_ty = self.check_expr(&args[1]);
            if !s_ty.is_error() && s_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("ends_with() first argument must be str, found `{s_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !suffix_ty.is_error() && suffix_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("ends_with() second argument must be str, found `{suffix_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::Bool);
        }
        if name == "replace" {
            if args.len() != 3 {
                self.error(
                    ErrorCode::E0513,
                    format!("replace() takes exactly 3 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let s_ty = self.check_expr(&args[0]);
            let from_ty = self.check_expr(&args[1]);
            let to_ty = self.check_expr(&args[2]);
            if !s_ty.is_error() && s_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("replace() first argument must be str, found `{s_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !from_ty.is_error() && from_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("replace() second argument must be str, found `{from_ty}`"),
                    args[1].span.clone(),
                );
            }
            if !to_ty.is_error() && to_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("replace() third argument must be str, found `{to_ty}`"),
                    args[2].span.clone(),
                );
            }
            return Some(Ty::Str);
        }
        if name == "char_at" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0513,
                    format!("char_at() takes exactly 2 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let s_ty = self.check_expr(&args[0]);
            let idx_ty = self.check_expr(&args[1]);
            if !s_ty.is_error() && s_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("char_at() first argument must be str, found `{s_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !idx_ty.is_error() && !idx_ty.is_integer() {
                self.error(
                    ErrorCode::E0133,
                    format!("char_at() second argument must be integer, found `{idx_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::Str);
        }

        if name == "contains" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0513,
                    format!("contains() takes exactly 2 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let s_ty = self.check_expr(&args[0]);
            let sub_ty = self.check_expr(&args[1]);
            if !s_ty.is_error() && s_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("contains() first argument must be str, found `{s_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !sub_ty.is_error() && sub_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("contains() second argument must be str, found `{sub_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::Bool);
        }
        if name == "index_of" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0100,
                    format!("index_of() takes exactly 2 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let s_ty = self.check_expr(&args[0]);
            let sub_ty = self.check_expr(&args[1]);
            if !s_ty.is_error() && s_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("index_of() first argument must be str, found `{s_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !sub_ty.is_error() && sub_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("index_of() second argument must be str, found `{sub_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::I64);
        }
        if name == "join" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0513,
                    format!("join() takes exactly 2 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let arr_ty = self.check_expr(&args[0]);
            let sep_ty = self.check_expr(&args[1]);
            if !arr_ty.is_error() && arr_ty != Ty::Array(Box::new(Ty::Str)) {
                self.error(
                    ErrorCode::E0133,
                    format!("join() first argument must be [str], found `{arr_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !sep_ty.is_error() && sep_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("join() second argument must be str, found `{sep_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::Str);
        }
        if name == "repeat" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0100,
                    format!("repeat() takes exactly 2 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let s_ty = self.check_expr(&args[0]);
            let n_ty = self.check_expr(&args[1]);
            if !s_ty.is_error() && s_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("repeat() first argument must be str, found `{s_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !n_ty.is_error() && !n_ty.is_integer() {
                self.error(
                    ErrorCode::E0100,
                    format!("repeat() second argument must be integer, found `{n_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::Str);
        }

        // ── Stdlib I/O functions ─────────────────────────
        None
    }

    pub(crate) fn check_builtin_io_env(
        &mut self,
        name: &str,
        args: &[Spanned<Expr>],
        callee: &Spanned<Expr>,
    ) -> Option<Ty> {
        if name == "read_line" {
            if !args.is_empty() {
                self.error(
                    ErrorCode::E0100,
                    format!("read_line() takes 0 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            return Some(Ty::Str);
        }
        if name == "read_file" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0513,
                    format!("read_file() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let path_ty = self.check_expr(&args[0]);
            if !path_ty.is_error() && path_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("read_file() expects str, found `{path_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Str);
        }
        if name == "write_file" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0513,
                    format!("write_file() takes exactly 2 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let path_ty = self.check_expr(&args[0]);
            let content_ty = self.check_expr(&args[1]);
            if !path_ty.is_error() && path_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("write_file() first argument must be str, found `{path_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !content_ty.is_error() && content_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("write_file() second argument must be str, found `{content_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::Unit);
        }
        if name == "try_read_file" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0513,
                    format!(
                        "try_read_file() takes exactly 1 argument, got {}",
                        args.len()
                    ),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let path_ty = self.check_expr(&args[0]);
            if !path_ty.is_error() && path_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("try_read_file() expects str, found `{path_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Result(Box::new(Ty::Str), Box::new(Ty::Str)));
        }
        if name == "try_write_file" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0513,
                    format!(
                        "try_write_file() takes exactly 2 arguments, got {}",
                        args.len()
                    ),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let path_ty = self.check_expr(&args[0]);
            let content_ty = self.check_expr(&args[1]);
            if !path_ty.is_error() && path_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("try_write_file() first argument must be str, found `{path_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !content_ty.is_error() && content_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("try_write_file() second argument must be str, found `{content_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::Result(Box::new(Ty::Bool), Box::new(Ty::Str)));
        }

        // ── shell_exec / exec / env_get ──────────────────────────────
        if name == "shell_exec" || name == "exec" {
            if !self.in_unsafe_context {
                self.error(
                    ErrorCode::E0100,
                    format!("`{name}()` can only be called inside an `@unsafe` function")
                        .to_string(),
                    callee.span.clone(),
                );
            }
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0513,
                    format!("{name}() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let cmd_ty = self.check_expr(&args[0]);
            if !cmd_ty.is_error() && cmd_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("{name}() expects str, found `{cmd_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Str);
        }
        if name == "env_get" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0513,
                    format!("env_get() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let name_ty = self.check_expr(&args[0]);
            if !name_ty.is_error() && name_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("env_get() expects str, found `{name_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Str);
        }

        // ── Stdlib math functions ────────────────────────
        None
    }

    pub(crate) fn check_builtin_math(
        &mut self,
        name: &str,
        args: &[Spanned<Expr>],
        callee: &Spanned<Expr>,
    ) -> Option<Ty> {
        if name == "pow" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0100,
                    format!("pow() takes exactly 2 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let base_ty = self.check_expr(&args[0]);
            let exp_ty = self.check_expr(&args[1]);
            if !base_ty.is_error() && !base_ty.is_integer() {
                self.error(
                    ErrorCode::E0100,
                    format!("pow() first argument must be integer, found `{base_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !exp_ty.is_error() && !exp_ty.is_integer() {
                self.error(
                    ErrorCode::E0100,
                    format!("pow() second argument must be integer, found `{exp_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::I64);
        }
        if name == "sqrt" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0100,
                    format!("sqrt() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let x_ty = self.check_expr(&args[0]);
            if !x_ty.is_error() && x_ty != Ty::F64 && x_ty != Ty::F32 {
                self.error(
                    ErrorCode::E0100,
                    format!("sqrt() expects float, found `{x_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::F64);
        }

        // ── Math builtins (Tier 1) ───────────────────
        // floor/ceil/round: (f64) -> i64
        if name == "floor" || name == "ceil" || name == "round" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0100,
                    format!("{name}() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let x_ty = self.check_expr(&args[0]);
            if !x_ty.is_error() && x_ty != Ty::F64 && x_ty != Ty::F32 {
                self.error(
                    ErrorCode::E0100,
                    format!("{name}() expects float, found `{x_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::I64);
        }
        // sin/cos/tan/log/log2/log10/exp: (f64) -> f64
        if name == "sin"
            || name == "cos"
            || name == "tan"
            || name == "log"
            || name == "log2"
            || name == "log10"
            || name == "exp"
        {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0100,
                    format!("{name}() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let x_ty = self.check_expr(&args[0]);
            if !x_ty.is_error() && x_ty != Ty::F64 && x_ty != Ty::F32 {
                self.error(
                    ErrorCode::E0100,
                    format!("{name}() expects float, found `{x_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::F64);
        }
        // random() -> f64
        if name == "random" {
            if !args.is_empty() {
                self.error(
                    ErrorCode::E0100,
                    format!("random() takes 0 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            return Some(Ty::F64);
        }
        // random_range(min, max) -> i64
        if name == "random_range" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0100,
                    format!(
                        "random_range() takes exactly 2 arguments, got {}",
                        args.len()
                    ),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let min_ty = self.check_expr(&args[0]);
            let max_ty = self.check_expr(&args[1]);
            if !min_ty.is_error() && !min_ty.is_integer() {
                self.error(
                    ErrorCode::E0100,
                    format!("random_range() first argument must be integer, found `{min_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !max_ty.is_error() && !max_ty.is_integer() {
                self.error(
                    ErrorCode::E0100,
                    format!("random_range() second argument must be integer, found `{max_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::I64);
        }

        // ── System builtins (Tier 1) ────────────────────
        // args() -> [str]
        None
    }

    pub(crate) fn check_builtin_convert(
        &mut self,
        name: &str,
        args: &[Spanned<Expr>],
        callee: &Spanned<Expr>,
    ) -> Option<Ty> {
        if name == "args" {
            if !args.is_empty() {
                self.error(
                    ErrorCode::E0100,
                    format!("args() takes 0 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            return Some(Ty::Array(Box::new(Ty::Str)));
        }
        // exit(code: i64) -> unit
        if name == "exit" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0100,
                    format!("exit() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let code_ty = self.check_expr(&args[0]);
            if !code_ty.is_error() && !code_ty.is_integer() {
                self.error(
                    ErrorCode::E0100,
                    format!("exit() expects integer, found `{code_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Unit);
        }
        // type_of(val) -> str
        if name == "type_of" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0100,
                    format!("type_of() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            // Accept any type
            self.check_expr(&args[0]);
            return Some(Ty::Str);
        }

        // ── String parsing builtins (Tier 1) ────────────
        // substring(s, start, end) -> str
        if name == "substring" {
            if args.len() != 3 {
                self.error(
                    ErrorCode::E0100,
                    format!("substring() takes exactly 3 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let s_ty = self.check_expr(&args[0]);
            let start_ty = self.check_expr(&args[1]);
            let end_ty = self.check_expr(&args[2]);
            if !s_ty.is_error() && s_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("substring() first argument must be str, found `{s_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !start_ty.is_error() && !start_ty.is_integer() {
                self.error(
                    ErrorCode::E0100,
                    format!("substring() second argument must be integer, found `{start_ty}`"),
                    args[1].span.clone(),
                );
            }
            if !end_ty.is_error() && !end_ty.is_integer() {
                self.error(
                    ErrorCode::E0100,
                    format!("substring() third argument must be integer, found `{end_ty}`"),
                    args[2].span.clone(),
                );
            }
            return Some(Ty::Str);
        }
        // pad_left(s, width, char) -> str
        if name == "pad_left" {
            if args.len() != 3 {
                self.error(
                    ErrorCode::E0100,
                    format!("pad_left() takes exactly 3 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let s_ty = self.check_expr(&args[0]);
            let width_ty = self.check_expr(&args[1]);
            let char_ty = self.check_expr(&args[2]);
            if !s_ty.is_error() && s_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("pad_left() first argument must be str, found `{s_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !width_ty.is_error() && !width_ty.is_integer() {
                self.error(
                    ErrorCode::E0100,
                    format!("pad_left() second argument must be integer, found `{width_ty}`"),
                    args[1].span.clone(),
                );
            }
            if !char_ty.is_error() && char_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("pad_left() third argument must be str, found `{char_ty}`"),
                    args[2].span.clone(),
                );
            }
            return Some(Ty::Str);
        }
        // pad_right(s, width, char) -> str
        if name == "pad_right" {
            if args.len() != 3 {
                self.error(
                    ErrorCode::E0100,
                    format!("pad_right() takes exactly 3 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let s_ty = self.check_expr(&args[0]);
            let width_ty = self.check_expr(&args[1]);
            let char_ty = self.check_expr(&args[2]);
            if !s_ty.is_error() && s_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("pad_right() first argument must be str, found `{s_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !width_ty.is_error() && !width_ty.is_integer() {
                self.error(
                    ErrorCode::E0100,
                    format!("pad_right() second argument must be integer, found `{width_ty}`"),
                    args[1].span.clone(),
                );
            }
            if !char_ty.is_error() && char_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("pad_right() third argument must be str, found `{char_ty}`"),
                    args[2].span.clone(),
                );
            }
            return Some(Ty::Str);
        }
        // str_to_int(s) -> i64 ! str
        if name == "str_to_int" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0100,
                    format!("str_to_int() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let s_ty = self.check_expr(&args[0]);
            if !s_ty.is_error() && s_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("str_to_int() expects str, found `{s_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Result(Box::new(Ty::I64), Box::new(Ty::Str)));
        }
        // str_to_float(s) -> f64 ! str
        if name == "str_to_float" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0100,
                    format!(
                        "str_to_float() takes exactly 1 argument, got {}",
                        args.len()
                    ),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let s_ty = self.check_expr(&args[0]);
            if !s_ty.is_error() && s_ty != Ty::Str {
                self.error(
                    ErrorCode::E0100,
                    format!("str_to_float() expects str, found `{s_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Result(Box::new(Ty::F64), Box::new(Ty::Str)));
        }

        // float_to_int(f: float) -> int
        if name == "float_to_int" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0513,
                    format!(
                        "float_to_int() takes exactly 1 argument, got {}",
                        args.len()
                    ),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let arg_ty = self.check_expr(&args[0]);
            if !arg_ty.is_error() && arg_ty != Ty::F64 {
                self.error(
                    ErrorCode::E0133,
                    format!("float_to_int() argument must be float, found `{arg_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::I64);
        }

        // int_to_float(i: int) -> float
        if name == "int_to_float" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0513,
                    format!(
                        "int_to_float() takes exactly 1 argument, got {}",
                        args.len()
                    ),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let arg_ty = self.check_expr(&args[0]);
            if !arg_ty.is_error() && arg_ty != Ty::I64 {
                self.error(
                    ErrorCode::E0133,
                    format!("int_to_float() argument must be int, found `{arg_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::F64);
        }

        // str_from_char(code: int) -> str
        if name == "str_from_char" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0513,
                    format!(
                        "str_from_char() takes exactly 1 argument, got {}",
                        args.len()
                    ),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let arg_ty = self.check_expr(&args[0]);
            if !arg_ty.is_error() && !arg_ty.is_integer() {
                self.error(
                    ErrorCode::E0133,
                    format!("str_from_char() argument must be int, found `{arg_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Str);
        }

        // ── Filesystem builtins ─────────────────────────
        // file_exists(path: str) -> bool
        None
    }
}
