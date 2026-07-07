# Vendored SQLite amalgamation

This directory contains the official SQLite **amalgamation** — the entire
SQLite library collapsed into a single `sqlite3.c` translation unit plus its
`sqlite3.h` header.

| | |
|---|---|
| **Version** | 3.47.2 (`SQLITE_VERSION`, release 2024-12-07) |
| **Source** | <https://www.sqlite.org/2024/sqlite-amalgamation-3470200.zip> |
| **License** | **Public domain** — SQLite is dedicated to the public domain. No copyright is claimed. See <https://www.sqlite.org/copyright.html>. |

## Why vendored?

TurboLang's SQLite builtins (`sqlite_open`, `sqlite_exec`, `sqlite_prepare`,
…) are "batteries included": a program that uses them must compile to a
single self-contained native binary with **no external `libsqlite3`
dependency**. Vendoring the amalgamation is the SQLite-recommended way to
embed the engine, and it lets Turbo pin an exact, reproducible version.

## How it is compiled

The same source file is compiled into **both** runtime paths:

* **JIT** (`turbolang run`): `build.rs` in `turbo-codegen-cranelift` compiles
  `sqlite3.c` into a static archive that is linked into the `turbolang`
  binary. The JIT SQLite builtins (Rust twins in `src/runtime.rs`) call the
  `sqlite3_*` C API directly via FFI.
* **AOT** (`turbolang build`): `src/aot.rs` links a prebuilt `sqlite3` object
  (embedded from `build.rs`, or recompiled from source when cross-compiling)
  alongside `turbo_rt.c` / `turbo_rt_sqlite.c`.

Both paths use the same compile flags (see `build.rs` and `aot.rs`):

```
-DSQLITE_THREADSAFE=1
-DSQLITE_OMIT_LOAD_EXTENSION      # no dynamic extension loading (security + size)
-DSQLITE_OMIT_DEPRECATED
-DSQLITE_DQS=0                    # no double-quoted string literals (stricter SQL)
-DSQLITE_DEFAULT_MEMSTATUS=0
-DSQLITE_OMIT_SHARED_CACHE
```

`SQLITE_THREADSAFE=1` (serialized mode) is required because the JIT HTTP
server handles requests on multiple threads.

## Updating

1. Download a newer amalgamation zip from <https://www.sqlite.org/download.html>.
2. Replace `sqlite3.c` and `sqlite3.h` here.
3. Update the version/URL in the table above.
4. Re-run the gates (unit tests, `run_parity.sh`, the C ASan `tests.sh`).

Do **not** edit `sqlite3.c` / `sqlite3.h` by hand — they are generated
artifacts and must stay byte-for-byte as shipped by upstream.
