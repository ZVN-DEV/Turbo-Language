//! JSON, concurrency, hashmap, and reference builtin signatures. Part of the
//! [`super`] expression-checking implementation.

use turbo_ast::*;

use crate::{Checker, HandleKind, Ty};

impl Checker {
    pub(crate) fn check_builtin_json(
        &mut self,
        name: &str,
        args: &[Spanned<Expr>],
        callee: &Spanned<Expr>,
    ) -> Option<Ty> {
        if name == "json_get" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0513,
                    format!("json_get() takes exactly 2 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let json_ty = self.check_expr(&args[0]);
            let key_ty = self.check_expr(&args[1]);
            if !json_ty.is_error() && json_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("json_get() first argument must be str, found `{json_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !key_ty.is_error() && key_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("json_get() second argument must be str, found `{key_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::Str);
        }
        // json_stringify(key: str, value: str) -> str
        if name == "json_stringify" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0513,
                    format!(
                        "json_stringify() takes exactly 2 arguments, got {}",
                        args.len()
                    ),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let key_ty = self.check_expr(&args[0]);
            let value_ty = self.check_expr(&args[1]);
            if !key_ty.is_error() && key_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("json_stringify() first argument must be str, found `{key_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !value_ty.is_error() && value_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("json_stringify() second argument must be str, found `{value_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::Str);
        }
        // json_build(pairs: str) -> str
        if name == "json_build" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0513,
                    format!("json_build() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let pairs_ty = self.check_expr(&args[0]);
            if !pairs_ty.is_error() && pairs_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("json_build() argument must be str, found `{pairs_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Str);
        }
        // to_json(val: any) -> str
        if name == "to_json" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0100,
                    format!("to_json() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let _val_ty = self.check_expr(&args[0]);
            return Some(Ty::Str);
        }
        // to_json_array(arr: [any]) -> str
        if name == "to_json_array" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0100,
                    format!(
                        "to_json_array() takes exactly 1 argument, got {}",
                        args.len()
                    ),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let val_ty = self.check_expr(&args[0]);
            if !val_ty.is_error() && !matches!(val_ty, Ty::Array(_)) {
                self.error(
                    ErrorCode::E0100,
                    format!("to_json_array() argument must be an array, found `{val_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Str);
        }

        // ── Channel builtins ───────────────────────────────
        // channel() -> i64 (channel pointer)
        None
    }

    pub(crate) fn check_builtin_concurrency(
        &mut self,
        name: &str,
        args: &[Spanned<Expr>],
        callee: &Spanned<Expr>,
    ) -> Option<Ty> {
        if name == "channel" {
            if !args.is_empty() {
                self.error(
                    ErrorCode::E0100,
                    format!("channel() takes no arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            return Some(Ty::I64);
        }
        // send(ch: i64, value: i64) -> ()
        if name == "send" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0513,
                    format!("send() takes exactly 2 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let ch_ty = self.check_expr(&args[0]);
            let val_ty = self.check_expr(&args[1]);
            if !ch_ty.is_error() && !ch_ty.is_integer() {
                self.error(
                    ErrorCode::E0133,
                    format!("send() first argument must be a channel (integer), found `{ch_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !val_ty.is_error() && !val_ty.is_integer() {
                self.error(
                    ErrorCode::E0133,
                    format!("send() second argument must be integer, found `{val_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::Unit);
        }
        // recv(ch: i64) -> i64
        if name == "recv" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0513,
                    format!("recv() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let ch_ty = self.check_expr(&args[0]);
            if !ch_ty.is_error() && !ch_ty.is_integer() {
                self.error(
                    ErrorCode::E0133,
                    format!("recv() argument must be a channel (integer), found `{ch_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::I64);
        }

        // ── Mutex builtins ────────────────────────────────
        // mutex(value: i64) -> i64 (mutex pointer)
        if name == "mutex" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0100,
                    format!("mutex() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let val_ty = self.check_expr(&args[0]);
            if !val_ty.is_error() && !val_ty.is_integer() {
                self.error(
                    ErrorCode::E0100,
                    format!("mutex() argument must be integer, found `{val_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Handle(HandleKind::Mutex));
        }
        // mutex_get(m: i64) -> i64
        if name == "mutex_get" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0513,
                    format!("mutex_get() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let m_ty = self.check_expr(&args[0]);
            if !m_ty.is_error() && !m_ty.is_handle_or_int(HandleKind::Mutex) {
                self.error(
                    ErrorCode::E0133,
                    format!("mutex_get() argument must be a mutex (integer), found `{m_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::I64);
        }
        // mutex_set(m: i64, value: i64) -> ()
        if name == "mutex_set" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0513,
                    format!("mutex_set() takes exactly 2 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let m_ty = self.check_expr(&args[0]);
            let val_ty = self.check_expr(&args[1]);
            if !m_ty.is_error() && !m_ty.is_handle_or_int(HandleKind::Mutex) {
                self.error(
                    ErrorCode::E0133,
                    format!("mutex_set() first argument must be a mutex (integer), found `{m_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !val_ty.is_error() && !val_ty.is_integer() {
                self.error(
                    ErrorCode::E0133,
                    format!("mutex_set() second argument must be integer, found `{val_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::Unit);
        }
        // mutex_update(m: i64, f: fn(i64) -> i64) -> i64
        // Runs `f(old)` atomically under the lock and stores the
        // result — the only way to express a correct read-modify-write
        // (e.g. a shared counter) over a mutex.
        if name == "mutex_update" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0513,
                    format!(
                        "mutex_update() takes exactly 2 arguments, got {}",
                        args.len()
                    ),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let m_ty = self.check_expr(&args[0]);
            if !m_ty.is_error() && !m_ty.is_handle_or_int(HandleKind::Mutex) {
                self.error(
                    ErrorCode::E0133,
                    format!(
                        "mutex_update() first argument must be a mutex (integer), found `{m_ty}`"
                    ),
                    args[0].span.clone(),
                );
            }
            // Hint the closure's parameter type so `|x| ...` infers `x: i64`.
            self.closure_param_hint = Some(vec![Ty::I64]);
            let fn_ty = self.check_expr(&args[1]);
            match &fn_ty {
                Ty::Fn(params, ret) => {
                    if params.len() != 1 {
                        self.error(
                            ErrorCode::E0133,
                            format!(
                                "mutex_update() callback must take 1 parameter, takes {}",
                                params.len()
                            ),
                            args[1].span.clone(),
                        );
                    } else if !params[0].is_error() && !params[0].is_integer() {
                        self.error(
                            ErrorCode::E0133,
                            format!(
                                "mutex_update() callback parameter must be integer, found `{}`",
                                params[0]
                            ),
                            args[1].span.clone(),
                        );
                    }
                    if !ret.is_error() && !ret.is_integer() {
                        self.error(
                            ErrorCode::E0133,
                            format!("mutex_update() callback must return integer, returns `{ret}`"),
                            args[1].span.clone(),
                        );
                    }
                }
                _ if fn_ty.is_error() => {}
                _ => {
                    self.error(
                                    ErrorCode::E0133,
                                    format!("mutex_update() second argument must be a function `fn(int) -> int`, found `{fn_ty}`"),
                                    args[1].span.clone(),
                                );
                }
            }
            return Some(Ty::I64);
        }

        // clone(val) -> T (requires @derive(Clone))
        if name == "clone" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0513,
                    format!("clone() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let arg_ty = self.check_expr(&args[0]);
            if let Ty::Struct(ref struct_name) = arg_ty {
                if let Some(info) = self.structs.get(struct_name) {
                    if !info.derives.contains(&"Clone".to_string()) {
                        self.error(
                            ErrorCode::E0100,
                            format!("cannot clone struct `{struct_name}` without `@derive(Clone)`"),
                            callee.span.clone(),
                        );
                        return Some(Ty::Error);
                    }
                }
            } else if !arg_ty.is_error() {
                self.error(
                    ErrorCode::E0100,
                    format!("clone() expects a struct argument, found `{arg_ty}`"),
                    args[0].span.clone(),
                );
                return Some(Ty::Error);
            }
            return Some(arg_ty);
        }

        // ── HashMap builtins ────────────────────────────────
        // hashmap() -> i64 (opaque pointer)
        None
    }

    pub(crate) fn check_builtin_hashmap(
        &mut self,
        name: &str,
        args: &[Spanned<Expr>],
        callee: &Spanned<Expr>,
    ) -> Option<Ty> {
        if name == "hashmap" {
            if !args.is_empty() {
                self.error(
                    ErrorCode::E0100,
                    format!("hashmap() takes no arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            return Some(Ty::Handle(HandleKind::HashMap));
        }
        // hashmap_set(map: i64, key: str, value: str) -> ()
        if name == "hashmap_set" {
            if args.len() != 3 {
                self.error(
                    ErrorCode::E0513,
                    format!(
                        "hashmap_set() takes exactly 3 arguments, got {}",
                        args.len()
                    ),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let map_ty = self.check_expr(&args[0]);
            let key_ty = self.check_expr(&args[1]);
            let val_ty = self.check_expr(&args[2]);
            if !map_ty.is_error() && !map_ty.is_handle_or_int(HandleKind::HashMap) {
                self.error(ErrorCode::E0133, format!("hashmap_set() first argument must be a hashmap (integer), found `{map_ty}`"), args[0].span.clone());
            }
            if !key_ty.is_error() && key_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("hashmap_set() second argument must be str, found `{key_ty}`"),
                    args[1].span.clone(),
                );
            }
            if !val_ty.is_error() && val_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("hashmap_set() third argument must be str, found `{val_ty}`"),
                    args[2].span.clone(),
                );
            }
            return Some(Ty::Unit);
        }
        // hashmap_get(map: i64, key: str) -> str
        if name == "hashmap_get" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0513,
                    format!(
                        "hashmap_get() takes exactly 2 arguments, got {}",
                        args.len()
                    ),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let map_ty = self.check_expr(&args[0]);
            let key_ty = self.check_expr(&args[1]);
            if !map_ty.is_error() && !map_ty.is_handle_or_int(HandleKind::HashMap) {
                self.error(ErrorCode::E0133, format!("hashmap_get() first argument must be a hashmap (integer), found `{map_ty}`"), args[0].span.clone());
            }
            if !key_ty.is_error() && key_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("hashmap_get() second argument must be str, found `{key_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::Str);
        }
        // hashmap_has(map: i64, key: str) -> bool
        if name == "hashmap_has" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0513,
                    format!(
                        "hashmap_has() takes exactly 2 arguments, got {}",
                        args.len()
                    ),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let map_ty = self.check_expr(&args[0]);
            let key_ty = self.check_expr(&args[1]);
            if !map_ty.is_error() && !map_ty.is_handle_or_int(HandleKind::HashMap) {
                self.error(ErrorCode::E0133, format!("hashmap_has() first argument must be a hashmap (integer), found `{map_ty}`"), args[0].span.clone());
            }
            if !key_ty.is_error() && key_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("hashmap_has() second argument must be str, found `{key_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::Bool);
        }
        // hashmap_len / hashmap_size(map: i64) -> i64
        if name == "hashmap_len" || name == "hashmap_size" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0513,
                    format!("hashmap_len() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let map_ty = self.check_expr(&args[0]);
            if !map_ty.is_error() && !map_ty.is_handle_or_int(HandleKind::HashMap) {
                self.error(
                    ErrorCode::E0133,
                    format!("hashmap_len() argument must be a hashmap (integer), found `{map_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::I64);
        }
        // hashmap_keys(map: i64) -> [str]
        if name == "hashmap_keys" {
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0513,
                    format!(
                        "hashmap_keys() takes exactly 1 argument, got {}",
                        args.len()
                    ),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let map_ty = self.check_expr(&args[0]);
            if !map_ty.is_error() && !map_ty.is_handle_or_int(HandleKind::HashMap) {
                self.error(
                    ErrorCode::E0133,
                    format!(
                        "hashmap_keys() argument must be a hashmap (integer), found `{map_ty}`"
                    ),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::Array(Box::new(Ty::Str)));
        }
        // hashmap_remove(map: i64, key: str) -> ()
        if name == "hashmap_remove" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0513,
                    format!(
                        "hashmap_remove() takes exactly 2 arguments, got {}",
                        args.len()
                    ),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let map_ty = self.check_expr(&args[0]);
            let key_ty = self.check_expr(&args[1]);
            if !map_ty.is_error() && !map_ty.is_handle_or_int(HandleKind::HashMap) {
                self.error(ErrorCode::E0133, format!("hashmap_remove() first argument must be a hashmap (integer), found `{map_ty}`"), args[0].span.clone());
            }
            if !key_ty.is_error() && key_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("hashmap_remove() second argument must be str, found `{key_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::Unit);
        }
        // hashmap_set_int(map: i64, key: str, value: int) -> hashmap (i64)
        // v0.8.0 "Safe Core" str→int variant. Returns the map so it can be
        // used in `m = hashmap_set_int(m, k, v)` style.
        if name == "hashmap_set_int" {
            if args.len() != 3 {
                self.error(
                    ErrorCode::E0513,
                    format!(
                        "hashmap_set_int() takes exactly 3 arguments, got {}",
                        args.len()
                    ),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let map_ty = self.check_expr(&args[0]);
            let key_ty = self.check_expr(&args[1]);
            let val_ty = self.check_expr(&args[2]);
            if !map_ty.is_error() && !map_ty.is_handle_or_int(HandleKind::HashMap) {
                self.error(ErrorCode::E0133, format!("hashmap_set_int() first argument must be a hashmap (integer), found `{map_ty}`"), args[0].span.clone());
            }
            if !key_ty.is_error() && key_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("hashmap_set_int() second argument must be str, found `{key_ty}`"),
                    args[1].span.clone(),
                );
            }
            if !val_ty.is_error() && !val_ty.is_integer() {
                self.error(
                    ErrorCode::E0133,
                    format!("hashmap_set_int() third argument must be int, found `{val_ty}`"),
                    args[2].span.clone(),
                );
            }
            // Returns the map handle so `m = hashmap_set_int(m, k, v)` keeps `m`
            // typed as a hashmap handle (not a plain int).
            return Some(Ty::Handle(HandleKind::HashMap));
        }
        // hashmap_get_int(map: i64, key: str) -> int
        // Returns 0 on miss — guard with hashmap_has() if you need to
        // distinguish missing from a stored 0.
        if name == "hashmap_get_int" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0513,
                    format!(
                        "hashmap_get_int() takes exactly 2 arguments, got {}",
                        args.len()
                    ),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let map_ty = self.check_expr(&args[0]);
            let key_ty = self.check_expr(&args[1]);
            if !map_ty.is_error() && !map_ty.is_handle_or_int(HandleKind::HashMap) {
                self.error(ErrorCode::E0133, format!("hashmap_get_int() first argument must be a hashmap (integer), found `{map_ty}`"), args[0].span.clone());
            }
            if !key_ty.is_error() && key_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("hashmap_get_int() second argument must be str, found `{key_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::I64);
        }
        // hashmap_inc(map: i64, key: str[, delta: int]) -> int
        // Fused str→int increment: adds `delta` (default 1) to the
        // value at `key` (missing key counts as 0) and returns the
        // new value. Single hash + single probe — the fast path for
        // word-count style counters.
        if name == "hashmap_inc" {
            if args.len() != 2 && args.len() != 3 {
                self.error(
                    ErrorCode::E0513,
                    format!("hashmap_inc() takes 2 or 3 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let map_ty = self.check_expr(&args[0]);
            let key_ty = self.check_expr(&args[1]);
            if !map_ty.is_error() && !map_ty.is_handle_or_int(HandleKind::HashMap) {
                self.error(ErrorCode::E0133, format!("hashmap_inc() first argument must be a hashmap (integer), found `{map_ty}`"), args[0].span.clone());
            }
            if !key_ty.is_error() && key_ty != Ty::Str {
                self.error(
                    ErrorCode::E0133,
                    format!("hashmap_inc() second argument must be str, found `{key_ty}`"),
                    args[1].span.clone(),
                );
            }
            if args.len() == 3 {
                let delta_ty = self.check_expr(&args[2]);
                if !delta_ty.is_error() && !delta_ty.is_integer() {
                    self.error(
                        ErrorCode::E0133,
                        format!("hashmap_inc() third argument must be int, found `{delta_ty}`"),
                        args[2].span.clone(),
                    );
                }
            }
            return Some(Ty::I64);
        }

        // ── Unsafe builtins ────────────────────────────────
        // deref(addr: i64) -> i64 — raw memory load (unsafe only)
        None
    }

    pub(crate) fn check_builtin_refs(
        &mut self,
        name: &str,
        args: &[Spanned<Expr>],
        callee: &Spanned<Expr>,
    ) -> Option<Ty> {
        if name == "deref" {
            if !self.in_unsafe_context {
                self.error(
                    ErrorCode::E0100,
                    "`deref()` can only be called inside an `@unsafe` function".to_string(),
                    callee.span.clone(),
                );
            }
            if args.len() != 1 {
                self.error(
                    ErrorCode::E0100,
                    format!("deref() takes exactly 1 argument, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let addr_ty = self.check_expr(&args[0]);
            if !addr_ty.is_error() && !addr_ty.is_integer() {
                self.error(
                    ErrorCode::E0100,
                    format!("deref() argument must be i64, found `{addr_ty}`"),
                    args[0].span.clone(),
                );
            }
            return Some(Ty::I64);
        }
        // store(addr: i64, value: i64) — raw memory store (unsafe only)
        if name == "store" {
            if !self.in_unsafe_context {
                self.error(
                    ErrorCode::E0100,
                    "`store()` can only be called inside an `@unsafe` function".to_string(),
                    callee.span.clone(),
                );
            }
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0100,
                    format!("store() takes exactly 2 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let addr_ty = self.check_expr(&args[0]);
            let val_ty = self.check_expr(&args[1]);
            if !addr_ty.is_error() && !addr_ty.is_integer() {
                self.error(
                    ErrorCode::E0100,
                    format!("store() first argument must be i64, found `{addr_ty}`"),
                    args[0].span.clone(),
                );
            }
            if !val_ty.is_error() && !val_ty.is_integer() {
                self.error(
                    ErrorCode::E0100,
                    format!("store() second argument must be i64, found `{val_ty}`"),
                    args[1].span.clone(),
                );
            }
            return Some(Ty::Unit);
        }

        // map(arr, fn) -> [U]
        if name == "map" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0513,
                    format!("map() takes exactly 2 arguments, got {}", args.len()),
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
                        format!("map() first argument must be an array, found `{arr_ty}`"),
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
                                "map() callback must take 1 parameter, takes {}",
                                params.len()
                            ),
                            args[1].span.clone(),
                        );
                    } else if !elem_ty.is_error() && !params[0].is_error() && elem_ty != params[0] {
                        self.error(ErrorCode::E0100,
                                        format!("map() callback parameter type `{}` doesn't match array element type `{}`", params[0], elem_ty),
                                        args[1].span.clone(),
                                    );
                    }
                    return Some(Ty::Array(ret.clone()));
                }
                _ if fn_ty.is_error() => return Some(Ty::Error),
                _ => {
                    self.error(
                        ErrorCode::E0133,
                        format!("map() second argument must be a function, found `{fn_ty}`"),
                        args[1].span.clone(),
                    );
                    return Some(Ty::Error);
                }
            }
        }

        // filter(arr, fn) -> [T]
        if name == "filter" {
            if args.len() != 2 {
                self.error(
                    ErrorCode::E0513,
                    format!("filter() takes exactly 2 arguments, got {}", args.len()),
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
                        format!("filter() first argument must be an array, found `{arr_ty}`"),
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
                                "filter() callback must take 1 parameter, takes {}",
                                params.len()
                            ),
                            args[1].span.clone(),
                        );
                    } else if !elem_ty.is_error() && !params[0].is_error() && elem_ty != params[0] {
                        self.error(ErrorCode::E0100,
                                        format!("filter() callback parameter type `{}` doesn't match array element type `{}`", params[0], elem_ty),
                                        args[1].span.clone(),
                                    );
                    }
                    if **ret != Ty::Bool && !ret.is_error() {
                        self.error(
                            ErrorCode::E0133,
                            format!("filter() callback must return `bool`, returns `{}`", ret),
                            args[1].span.clone(),
                        );
                    }
                    return Some(arr_ty);
                }
                _ if fn_ty.is_error() => return Some(Ty::Error),
                _ => {
                    self.error(
                        ErrorCode::E0133,
                        format!("filter() second argument must be a function, found `{fn_ty}`"),
                        args[1].span.clone(),
                    );
                    return Some(Ty::Error);
                }
            }
        }

        // reduce(arr, init, fn) -> U
        if name == "reduce" {
            if args.len() != 3 {
                self.error(
                    ErrorCode::E0513,
                    format!("reduce() takes exactly 3 arguments, got {}", args.len()),
                    callee.span.clone(),
                );
                return Some(Ty::Error);
            }
            let arr_ty = self.check_expr(&args[0]);
            let init_ty = self.check_expr(&args[1]);
            {
                let elem = match &arr_ty {
                    Ty::Array(inner) => *inner.clone(),
                    _ => Ty::Error,
                };
                self.closure_param_hint = Some(vec![init_ty.clone(), elem]);
            }
            let fn_ty = self.check_expr(&args[2]);
            let elem_ty = match &arr_ty {
                Ty::Array(inner) => *inner.clone(),
                _ if arr_ty.is_error() => Ty::Error,
                _ => {
                    self.error(
                        ErrorCode::E0133,
                        format!("reduce() first argument must be an array, found `{arr_ty}`"),
                        args[0].span.clone(),
                    );
                    return Some(Ty::Error);
                }
            };
            match &fn_ty {
                Ty::Fn(params, ret) => {
                    if params.len() != 2 {
                        self.error(
                            ErrorCode::E0133,
                            format!(
                                "reduce() callback must take 2 parameters, takes {}",
                                params.len()
                            ),
                            args[2].span.clone(),
                        );
                    } else {
                        if !init_ty.is_error() && !params[0].is_error() && init_ty != params[0] {
                            self.error(ErrorCode::E0133,
                                            format!("reduce() callback first parameter type `{}` doesn't match initial value type `{}`", params[0], init_ty),
                                            args[2].span.clone(),
                                        );
                        }
                        if !elem_ty.is_error() && !params[1].is_error() && elem_ty != params[1] {
                            self.error(ErrorCode::E0133,
                                            format!("reduce() callback second parameter type `{}` doesn't match array element type `{}`", params[1], elem_ty),
                                            args[2].span.clone(),
                                        );
                        }
                    }
                    return Some(*ret.clone());
                }
                _ if fn_ty.is_error() => return Some(Ty::Error),
                _ => {
                    self.error(
                        ErrorCode::E0133,
                        format!("reduce() third argument must be a function, found `{fn_ty}`"),
                        args[2].span.clone(),
                    );
                    return Some(Ty::Error);
                }
            }
        }
        None
    }
}
