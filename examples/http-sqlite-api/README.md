# http-sqlite-api

**HTTP in, SQLite under, JSON out, one binary.**

A tiny todo API backed by a real, embedded SQLite database — written entirely
in Turbo. There is no external database server and no `libsqlite3` dependency:
the SQLite engine is vendored into the compiler and linked straight into the
binary, so `turbolang build` produces a single self-contained executable.

## Run it

Straight from source with the JIT:

```bash
turbolang run examples/http-sqlite-api/main.tb
```

Or build the self-contained native binary:

```bash
turbolang build examples/http-sqlite-api/main.tb -o todo-api
./todo-api
```

Either way it opens (creating on first run) a `todos.db` file in the current
directory, seeds two rows, and listens on <http://127.0.0.1:8080>.

## Routes

| Method | Path      | Description                                   |
|--------|-----------|-----------------------------------------------|
| GET    | `/todos`  | Returns all todos as a JSON array.            |
| POST   | `/todos`  | Body is the todo title (plain text). Inserts it and returns the created row as JSON. |

## Try it with curl

List the seeded todos:

```bash
curl -s http://127.0.0.1:8080/todos
# [{"id":1,"title":"learn turbo","done":1},{"id":2,"title":"ship sqlite builtin","done":0}]
```

Add a todo (the request body is the title):

```bash
curl -s -X POST -d 'write the README' http://127.0.0.1:8080/todos
# {"ok":true,"title":"write the README"}
```

List again — the new row has been persisted to SQLite:

```bash
curl -s http://127.0.0.1:8080/todos
# [ ... ,{"id":3,"title":"write the README","done":0}]
```

Because the data lives in `todos.db`, it survives restarts. Delete the file to
start fresh:

```bash
rm todos.db
```

## How it works

The relevant SQLite builtins:

```turbo
let db = sqlite_open("todos.db")?              // i64 ! str  (Result)
sqlite_exec(db, "CREATE TABLE ...")?           // unit ! str
let stmt = sqlite_prepare(db, "SELECT ...")?   // i64 ! str
while sqlite_step(stmt) == 1 {                 // 1 = row, 0 = done, -1 = error
    let id    = sqlite_column_int(stmt, 0)
    let title = sqlite_column_str(stmt, 1)
    let done  = sqlite_column_int(stmt, 2)
}
sqlite_finalize(stmt)

let ins = sqlite_prepare(db, "INSERT INTO todos (title, done) VALUES (?, 0)")?
sqlite_bind_str(ins, 1, title)                 // 1-based parameter index
sqlite_step(ins)
sqlite_finalize(ins)
```

Failures surface as `Result` errors carrying the SQLite error message, so
nothing panics — the handlers turn any error into a JSON `{"error": ...}`
response.
