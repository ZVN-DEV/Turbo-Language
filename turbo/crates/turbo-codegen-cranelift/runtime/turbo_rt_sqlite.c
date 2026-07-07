/* turbo_rt_sqlite.c — SQLite builtins, AOT / C-runtime twin.
 *
 * This file is **never compiled on its own**. It is `#include`d into
 * `turbo_rt.c` (under `-DTURBO_WITH_SQLITE`) so it shares that translation
 * unit's helpers — `turbo_alloc`, `rt_result_ok`, `rt_result_err` — and the
 * arena-aware string allocator. Keeping the sqlite code in its own file keeps
 * `turbo_rt.c` itself lean.
 *
 * The JIT twins of every function below live in `src/runtime.rs` and call the
 * same `sqlite3_*` C API via FFI. The two implementations MUST stay
 * behaviourally identical (JIT ≡ AOT) — the parity harness
 * (`tests/parity/programs/sqlite_roundtrip.tb`) enforces this.
 *
 * String producers (`rt_sqlite_column_str`, `rt_sqlite_error`) return their
 * buffers via `turbo_strdup` (→ `rt_str_alloc` → `rt_rc_alloc`), exactly like
 * `rt_str_upper` and the other `rt_str_*` producers in `turbo_rt.c`, so every
 * string handed to Turbo carries the refcount header and is safely freed when
 * codegen releases the owned temporary. We NEVER hand sqlite's internal
 * pointers back to Turbo — they are always copied.
 *
 * Fallible functions (`open`, `exec`, `prepare`) return a `Result` object
 * built with `rt_result_ok` / `rt_result_err`, mirroring `rt_try_read_file`.
 * The err payload is a `turbo_alloc`d copy of the sqlite error message.
 */

#include <stdint.h>
#include <string.h>

#include "sqlite3.h"

/* Copy a C string into a fresh turbo string. NULL is treated as the empty
 * string. Uses turbo_strdup (which allocates via rt_str_alloc / rt_rc_alloc)
 * so every string handed back to Turbo carries the refcount header, exactly
 * like rt_str_upper and the other rt_str_* producers. A bare malloc'd buffer
 * would corrupt the heap when codegen releases the owned temporary. */
static char *rt_sqlite_dup(const char *s) {
    return turbo_strdup(s ? s : "");
}

/* sqlite_open(path: str) -> i64 ! str
 * Opens (creating if absent) a read/write database. `:memory:` is honoured by
 * sqlite for an in-memory database. Returns ok(handle) or err(message). */
void *rt_sqlite_open(const char *path) {
    sqlite3 *db = NULL;
    int rc = sqlite3_open_v2(path ? path : ":memory:", &db,
                             SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE, NULL);
    if (rc != SQLITE_OK) {
        const char *msg = db ? sqlite3_errmsg(db) : "unable to open database";
        char *buf = rt_sqlite_dup(msg);
        if (db) {
            sqlite3_close(db);
        }
        return rt_result_err((long long)(intptr_t)buf);
    }
    return rt_result_ok((long long)(intptr_t)db);
}

/* sqlite_exec(h: i64, sql: str) -> unit ! str
 * Runs one or more SQL statements, ignoring any result rows. */
void *rt_sqlite_exec(long long h, const char *sql) {
    sqlite3 *db = (sqlite3 *)(intptr_t)h;
    char *errmsg = NULL;
    int rc = sqlite3_exec(db, sql ? sql : "", NULL, NULL, &errmsg);
    if (rc != SQLITE_OK) {
        char *buf = rt_sqlite_dup(errmsg ? errmsg : sqlite3_errmsg(db));
        if (errmsg) {
            sqlite3_free(errmsg);
        }
        return rt_result_err((long long)(intptr_t)buf);
    }
    return rt_result_ok(0);
}

/* sqlite_prepare(h: i64, sql: str) -> i64 ! str
 * Compiles a single SQL statement into a prepared-statement handle. */
void *rt_sqlite_prepare(long long h, const char *sql) {
    sqlite3 *db = (sqlite3 *)(intptr_t)h;
    sqlite3_stmt *stmt = NULL;
    int rc = sqlite3_prepare_v2(db, sql ? sql : "", -1, &stmt, NULL);
    if (rc != SQLITE_OK) {
        char *buf = rt_sqlite_dup(sqlite3_errmsg(db));
        return rt_result_err((long long)(intptr_t)buf);
    }
    return rt_result_ok((long long)(intptr_t)stmt);
}

/* sqlite_bind_int(stmt, idx, v) -> i64 (sqlite rc). idx is 1-based. */
long long rt_sqlite_bind_int(long long stmt_h, long long idx, long long v) {
    return (long long)sqlite3_bind_int64((sqlite3_stmt *)(intptr_t)stmt_h, (int)idx, v);
}

/* sqlite_bind_str(stmt, idx, s) -> i64. Uses SQLITE_TRANSIENT so sqlite copies
 * the bytes immediately — the Turbo string need not outlive the bind. */
long long rt_sqlite_bind_str(long long stmt_h, long long idx, const char *s) {
    return (long long)sqlite3_bind_text((sqlite3_stmt *)(intptr_t)stmt_h, (int)idx,
                                        s ? s : "", -1, SQLITE_TRANSIENT);
}

/* sqlite_bind_float(stmt, idx, f) -> i64. */
long long rt_sqlite_bind_float(long long stmt_h, long long idx, double f) {
    return (long long)sqlite3_bind_double((sqlite3_stmt *)(intptr_t)stmt_h, (int)idx, f);
}

/* sqlite_step(stmt) -> 1 row / 0 done / -1 error. */
long long rt_sqlite_step(long long stmt_h) {
    int rc = sqlite3_step((sqlite3_stmt *)(intptr_t)stmt_h);
    if (rc == SQLITE_ROW) return 1;
    if (rc == SQLITE_DONE) return 0;
    return -1;
}

/* sqlite_column_int(stmt, i) -> i64. Column index is 0-based. */
long long rt_sqlite_column_int(long long stmt_h, long long i) {
    return (long long)sqlite3_column_int64((sqlite3_stmt *)(intptr_t)stmt_h, (int)i);
}

/* sqlite_column_str(stmt, i) -> str. Copies out of sqlite's storage. */
const char *rt_sqlite_column_str(long long stmt_h, long long i) {
    const unsigned char *text =
        sqlite3_column_text((sqlite3_stmt *)(intptr_t)stmt_h, (int)i);
    return rt_sqlite_dup((const char *)text);
}

/* sqlite_column_float(stmt, i) -> f64. */
double rt_sqlite_column_float(long long stmt_h, long long i) {
    return sqlite3_column_double((sqlite3_stmt *)(intptr_t)stmt_h, (int)i);
}

/* sqlite_column_count(stmt) -> i64. */
long long rt_sqlite_column_count(long long stmt_h) {
    return (long long)sqlite3_column_count((sqlite3_stmt *)(intptr_t)stmt_h);
}

/* sqlite_finalize(stmt) -> i64 (sqlite rc). */
long long rt_sqlite_finalize(long long stmt_h) {
    return (long long)sqlite3_finalize((sqlite3_stmt *)(intptr_t)stmt_h);
}

/* sqlite_error(h) -> str. Last error message for the connection. */
const char *rt_sqlite_error(long long h) {
    if (h == 0) {
        return rt_sqlite_dup("invalid database handle");
    }
    return rt_sqlite_dup(sqlite3_errmsg((sqlite3 *)(intptr_t)h));
}

/* sqlite_close(h) -> i64 (sqlite rc). */
long long rt_sqlite_close(long long h) {
    return (long long)sqlite3_close((sqlite3 *)(intptr_t)h);
}
