//! SQLite builtin signatures.
//!
//! Turbo surface (handles are opaque `i64` values):
//!
//! ```text
//! sqlite_open(path: str)           -> i64 ! str
//! sqlite_close(h: i64)             -> i64
//! sqlite_exec(h: i64, sql: str)    -> unit ! str
//! sqlite_error(h: i64)             -> str
//! sqlite_prepare(h: i64, sql: str) -> i64 ! str
//! sqlite_bind_int(s, idx, v)       -> i64
//! sqlite_bind_str(s, idx, v: str)  -> i64
//! sqlite_bind_float(s, idx, v: f64)-> i64
//! sqlite_step(s: i64)              -> i64
//! sqlite_column_int(s, i)          -> i64
//! sqlite_column_str(s, i)          -> str
//! sqlite_column_float(s, i)        -> f64
//! sqlite_column_count(s: i64)      -> i64
//! sqlite_finalize(s: i64)          -> i64
//! ```
//!
//! Fallible functions return a `Result` (mirroring `try_read_file`); failures
//! carry the SQLite error message as the `str` error payload.

use turbo_ast::*;

use crate::{Checker, Ty};

impl Checker {
    pub(crate) fn check_builtin_sqlite(
        &mut self,
        name: &str,
        args: &[Spanned<Expr>],
        callee: &Spanned<Expr>,
    ) -> Option<Ty> {
        // (param types, return type) for each builtin. `int`/handles are I64.
        let spec: (&[Ty], Ty) = match name {
            "sqlite_open" => (&[Ty::Str], Ty::Result(Box::new(Ty::I64), Box::new(Ty::Str))),
            "sqlite_close" => (&[Ty::I64], Ty::I64),
            "sqlite_exec" => (
                &[Ty::I64, Ty::Str],
                Ty::Result(Box::new(Ty::Unit), Box::new(Ty::Str)),
            ),
            "sqlite_error" => (&[Ty::I64], Ty::Str),
            "sqlite_prepare" => (
                &[Ty::I64, Ty::Str],
                Ty::Result(Box::new(Ty::I64), Box::new(Ty::Str)),
            ),
            "sqlite_bind_int" => (&[Ty::I64, Ty::I64, Ty::I64], Ty::I64),
            "sqlite_bind_str" => (&[Ty::I64, Ty::I64, Ty::Str], Ty::I64),
            "sqlite_bind_float" => (&[Ty::I64, Ty::I64, Ty::F64], Ty::I64),
            "sqlite_step" => (&[Ty::I64], Ty::I64),
            "sqlite_column_int" => (&[Ty::I64, Ty::I64], Ty::I64),
            "sqlite_column_str" => (&[Ty::I64, Ty::I64], Ty::Str),
            "sqlite_column_float" => (&[Ty::I64, Ty::I64], Ty::F64),
            "sqlite_column_count" => (&[Ty::I64], Ty::I64),
            "sqlite_finalize" => (&[Ty::I64], Ty::I64),
            _ => return None,
        };
        let (params, ret) = spec;

        if args.len() != params.len() {
            self.error(
                ErrorCode::E0513,
                format!(
                    "{name}() takes exactly {} argument{}, got {}",
                    params.len(),
                    if params.len() == 1 { "" } else { "s" },
                    args.len()
                ),
                callee.span.clone(),
            );
            return Some(ret);
        }

        for (i, expected) in params.iter().enumerate() {
            let actual = self.check_expr(&args[i]);
            if actual.is_error() {
                continue;
            }
            let ok = match expected {
                // Any integer width is acceptable where an i64 handle/index is
                // expected (literals default to I64, but be permissive).
                Ty::I64 => actual.is_integer(),
                Ty::F64 => actual == Ty::F64 || actual == Ty::F32,
                other => &actual == other,
            };
            if !ok {
                self.error(
                    ErrorCode::E0100,
                    format!(
                        "{name}() argument {} expects `{expected}`, found `{actual}`",
                        i + 1
                    ),
                    args[i].span.clone(),
                );
            }
        }

        Some(ret)
    }
}
