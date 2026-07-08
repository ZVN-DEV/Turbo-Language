# turbo-sqlite

An ergonomic, thin wrapper over Turbo's raw `sqlite_*` builtins. It keeps the
prepare/bind/step/column/finalize model but removes the boilerplate for the
common shapes.

```toml
[dependencies]
turbo-sqlite = "0.1"
```

## Example

```turbo
import { migrate, db_exec_params, query_scalar_int, query_column_str } from "turbo-sqlite"

fn run() -> i64 ! str {
    let db = sqlite_open("app.db")?
    migrate(db, [
        "CREATE TABLE IF NOT EXISTS users (id INTEGER PRIMARY KEY, name TEXT, age INTEGER)"
    ])?

    db_exec_params(db, "INSERT INTO users (name, age) VALUES (?, ?)", ["alice", "30"])?
    db_exec_params(db, "INSERT INTO users (name, age) VALUES (?, ?)", ["bob", "25"])?

    let count = query_scalar_int(db, "SELECT COUNT(*) FROM users WHERE age > 26")?
    print("adults over 26: {count}")                 // 1

    let names = query_column_str(db, "SELECT name FROM users ORDER BY name", 0)
    print("names: {names}")                          // ["alice", "bob"]

    sqlite_close(db)
    ok(0)
}

fn main() {
    match run() {
        ok(v)  => print("done")
        err(e) => print("sqlite error: {e}")
    }
}
```

## API

| Function | Returns | Purpose |
|----------|---------|---------|
| `db_exec(db, sql)` | `i64 ! str` | Run a no-param statement (DDL/DML). |
| `db_exec_params(db, sql, params)` | `i64 ! str` | Run a statement with positional string params bound to `?`. |
| `query_scalar_int(db, sql)` | `i64 ! str` | First column of the first row, as int. |
| `query_scalar_str(db, sql)` | `str ! str` | First column of the first row, as string. |
| `query_column_str(db, sql, col)` | `[str]` | Collect one string column across all rows. |
| `query_column_int(db, sql, col)` | `[i64]` | Collect one int column across all rows. |
| `db_count(db, table)` | `i64 ! str` | `SELECT COUNT(*)` for a (trusted) table name. |
| `migrate(db, statements)` | `i64 ! str` | Run each statement in order; returns the count. |

Params are bound with `sqlite_bind_str`, so use `?` placeholders for values —
never string-concatenate user input into SQL. (`db_count` interpolates the table
name, which SQLite can't bind, so pass a trusted identifier there.)

## Honest limitation

The column collectors (`query_column_str` / `query_column_int`) return the array
directly, **not** a `Result`: Turbo's parser does not currently accept an array
as a Result's ok-type (`[str] ! str` is a parse error). The tradeoff is that a
query which fails to prepare yields an **empty array**, indistinguishable from a
query that matched no rows. When you must tell "failed" from "empty" apart, probe
first with a `query_scalar_int` COUNT (which does return a `Result`).
