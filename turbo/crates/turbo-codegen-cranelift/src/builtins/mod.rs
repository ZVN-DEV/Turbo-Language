//! Built-in function compilation helpers.
//!
//! Each `compile_*` function handles a specific built-in function call
//! (print, assert, len, string ops, math, IO, JSON, HTTP, channels, etc.).

mod array_ops;
mod concurrency;
mod control_flow;
mod core_ops;
mod hashmap;
mod io_fs;
mod math_time;
mod net_json;
mod sqlite;
mod string_ops;

pub(crate) use array_ops::*;
pub(crate) use concurrency::*;
pub(crate) use control_flow::*;
pub(crate) use core_ops::*;
pub(crate) use hashmap::*;
pub(crate) use io_fs::*;
pub(crate) use math_time::*;
pub(crate) use net_json::*;
pub(crate) use sqlite::*;
pub(crate) use string_ops::*;
