# Turbo Standard Library Reference

> All functions that take a value as their first argument support both free-function and method syntax via UFCS (Uniform Function Call Syntax):
> `trim(s)` and `s.trim()` are equivalent.

---

## I/O

### print

```turbo
print("hello")
print(42)
print(true)
```

Prints a value to stdout followed by a newline. Accepts any type.

### read_line

```turbo
let name = read_line()
print("Hello, {name}!")
```

Reads a line of text from stdin. Returns the input as a string (without trailing newline).

### read_file

```turbo
let contents = read_file("data.txt")
print(contents)
```

Reads the entire contents of a file and returns it as a string. The path is relative to the current working directory.

### write_file

```turbo
write_file("output.txt", "Hello, file!")
let data = "line1\nline2\nline3"
write_file("lines.txt", data)
```

Writes a string to a file, creating the file if it does not exist or overwriting it if it does.

### Fallible I/O (`try_read_file` / `try_write_file`)

The plain `read_file` / `write_file` builtins above **panic** (abort the process) on any I/O failure — missing file, permission denied, bad path, disk full, etc. That's convenient for scripts and prototypes, but in production code you almost always want to handle errors explicitly.

For that, use the `try_` variants, which return a `Result`:

- `try_read_file(path: str) -> str ! str` — `ok(contents)` on success, `err(message)` on any failure.
- `try_write_file(path: str, content: str) -> bool ! str` — `ok(true)` on success, `err(message)` on any failure.

Pattern match to handle both cases:

```turbo
fn main() {
    let r = try_read_file("config.toml")
    match r {
        ok(s)  => print("loaded {len(s)} bytes")
        err(e) => print("could not read config: {e}")
    }
}
```

**Rule of thumb:** use `read_file` / `write_file` when a missing file is a bug you want to crash on. Use `try_read_file` / `try_write_file` when a missing file is a situation you want to recover from — that is, almost always, in real software.

---

## System

### exec

```turbo
let output = exec("ls -la")
print(output)
```

Executes a system command and returns its stdout as a string. Also available as `shell_exec`. As of v0.8.0, commands containing shell metacharacters (`;`, `|`, `&`, `$`, etc.) are rejected at runtime to prevent shell injection. The command is tokenized on whitespace and executed directly via `execvp` -- no shell is involved.

### env_get

```turbo
let home = env_get("HOME")
print("Home directory: {home}")
```

Returns the value of an environment variable as a string. Returns an empty string if the variable is not set.

### args

```turbo
fn main() {
    let a = args()
    print(len(a))
    let mut i = 0
    while i < len(a) {
        print(a[i])
        i = i + 1
    }
}
```

Returns the program's command-line arguments as a `[str]`. The argument list
**excludes** the program name / binary path: `args()[0]` is the *first* user
argument (the convention is `argv[1..]`). When the program is invoked with no
arguments, `args()` returns an empty array.

The same convention applies whether the program is run through the JIT or built
to a native binary, so `args()` returns identical values either way:

```bash
# JIT — pass arguments after the source file. Use `--` to forward
# hyphen-leading arguments to the program instead of to turbolang:
turbolang run app.tb -- alpha beta
turbolang run app.tb -- --name alice

# Native binary — arguments are passed directly:
turbolang build app.tb -o app
./app alpha beta
```

Both forms above make `args()` return `["alpha", "beta"]`.

### exit

```turbo
exit(0)    // terminate successfully
exit(1)    // terminate with a failure code
```

Immediately terminates the process with the given integer exit code. This call never returns — any code after it, including the rest of `main`, does not run.

### type_of

```turbo
let a = type_of(42)         // "i64"
let b = type_of(3.14)       // "f64"
let c = type_of("hello")    // "str"
let d = type_of(true)       // "bool"
let e = type_of([1, 2, 3])  // "array"
```

Returns the name of a value's type as a string. The type is resolved at compile time. Structs and enums return their declared name; arrays return `"array"`.

---

## Filesystem

> Paths are resolved relative to the current working directory unless they are absolute.

### file_exists

```turbo
if file_exists("config.toml") {
    print("found config")
}
```

Returns `true` if a file or directory exists at the given path, `false` otherwise.

### mkdir

```turbo
let ok = mkdir("build/cache")    // creates parents as needed
print(ok)                        // true
```

Creates a directory at the given path, including any missing parent directories (like `mkdir -p`). Returns `true` if the directory exists afterwards.

### delete_file

```turbo
let removed = delete_file("build/cache/old.txt")    // true
```

Deletes the file (or empty directory) at the given path. Returns `true` on success, or `false` if the path could not be removed.

### list_dir

```turbo
let entries = list_dir(".")
print(entries)    // e.g. ["main.tb", "data.txt"]
```

Returns an array of the entry names in a directory, excluding `.` and `..`. Returns an empty array if the path is not a readable directory.

### path_join

```turbo
let p = path_join("logs", "app.txt")    // "logs/app.txt"
```

Joins two path segments with a `/` separator, inserting one only when it is needed.

---

## Date / Time

### time_now

```turbo
let now = time_now()    // seconds since the Unix epoch, as a float
```

Returns the current wall-clock time as the number of seconds since the Unix epoch (1970-01-01 UTC), with sub-second precision.

### time_ms

```turbo
let start = time_ms()
// ... do some work ...
let elapsed = time_ms() - start
print("took {elapsed} ms")
```

Returns the current time as an integer number of milliseconds since the Unix epoch. Useful for timing and measuring elapsed durations.

### format_time

```turbo
let label = format_time(0.0, "%Y-%m-%d")               // "1970-01-01"
let stamp = format_time(time_now(), "%Y-%m-%d %H:%M:%S")
```

Formats a Unix timestamp (seconds since the epoch, such as the value from `time_now`) into a string using a `strftime`-style format. The timestamp is interpreted in the machine's local time zone.

---

## String Operations

### len (string)

```turbo
let n = "hello".len()    // 5
let n = len("hello")     // equivalent
```

Returns the length of a string in bytes.

### trim

```turbo
let cleaned = "  hello  ".trim()    // "hello"
let cleaned = trim("  hello  ")     // equivalent
```

Strips leading and trailing whitespace from a string.

### upper

```turbo
let loud = "hello".upper()    // "HELLO"
let loud = upper("hello")     // equivalent
```

Converts all characters in a string to uppercase.

### lower

```turbo
let quiet = "HELLO".lower()    // "hello"
let quiet = lower("HELLO")     // equivalent
```

Converts all characters in a string to lowercase.

### split

```turbo
let parts = "a,b,c".split(",")    // ["a", "b", "c"]
let words = split("hello world", " ")
```

Splits a string by a separator and returns an array of substrings.

### contains

```turbo
let found = "hello world".contains("world")    // true
let found = contains("hello world", "xyz")     // false
```

Returns `true` if the string contains the given substring.

### starts_with

```turbo
let yes = "https://turbolang.dev".starts_with("https")    // true
let yes = starts_with("hello", "he")
```

Returns `true` if the string starts with the given prefix.

### ends_with

```turbo
let yes = "main.tb".ends_with(".tb")    // true
let yes = ends_with("photo.png", ".png")
```

Returns `true` if the string ends with the given suffix.

### replace

```turbo
let fixed = "hello world".replace("world", "turbo")    // "hello turbo"
let fixed = replace("aaa", "a", "b")                   // "bbb"
```

Replaces all occurrences of a substring with a replacement string.

### index_of

```turbo
let i = "hello".index_of("ll")    // 2
let i = "hello".index_of("xyz")   // -1
```

Returns the byte index of the first occurrence of a substring, or `-1` if not found.

### char_at

```turbo
let ch = "hello".char_at(0)    // "h"
let ch = "hello".char_at(4)    // "o"
```

Returns the character at the given byte index as a single-character string.

### repeat

```turbo
let stars = "*".repeat(5)        // "*****"
let wall = "ab".repeat(3)       // "ababab"
```

Repeats a string `n` times and returns the result.

### join

```turbo
let csv = join(["a", "b", "c"], ",")    // "a,b,c"
let path = join(["usr", "local", "bin"], "/")
```

Joins an array of strings with a separator between each element.

### to_str

```turbo
let s = to_str(42)       // "42"
let s = to_str(true)     // "true"
let s = to_str(3.14)     // "3.14"
```

Converts any value to its string representation.

### str_to_int

```turbo
match str_to_int("42") {
    ok(n)  => print("parsed {n}")       // parsed 42
    err(e) => print("bad number: {e}")
}
```

Parses a string into a 64-bit integer. Returns a `Result`: `ok(n)` on success, or `err(message)` if the string is not a valid integer. Pattern match to handle both cases. Together with `str_to_float`, this is the way to turn input text or file contents into numbers.

### str_to_float

```turbo
match str_to_float("3.14") {
    ok(x)  => print("parsed {x}")       // parsed 3.14
    err(e) => print("bad float: {e}")
}
```

Parses a string into a 64-bit float. Returns a `Result`: `ok(x)` on success, or `err(message)` if the string is not a valid number. Pattern match to handle both cases.

### str_from_char

```turbo
let a = str_from_char(65)    // "A"
let z = str_from_char(122)   // "z"
```

Returns a one-character string for the given byte value (0–255). Only the low 8 bits of the code are used, so it is intended for ASCII characters.

### pad_left

```turbo
let n = pad_left("7", 3, "0")     // "007"
let s = pad_left("hi", 5, " ")    // "   hi"
```

Left-pads a string to the given width using the first character of the pad string. If the string is already at least `width` characters long it is returned unchanged (never truncated).

### pad_right

```turbo
let n = pad_right("7", 3, "0")    // "700"
let s = pad_right("hi", 5, ".")   // "hi..."
```

Right-pads a string to the given width using the first character of the pad string. If the string is already at least `width` characters long it is returned unchanged.

---

## Array Operations

### len (array)

```turbo
let n = [1, 2, 3].len()    // 3
let n = len([10, 20])      // 2
```

Returns the number of elements in an array.

### push

```turbo
let mut arr = [1, 2, 3]
arr = arr.push(4)          // [1, 2, 3, 4]
arr = push(arr, 5)         // [1, 2, 3, 4, 5]
```

Appends an element to the end of an array. Returns a new array (copy-on-write semantics). Reassign the result to the original variable.

### sort

```turbo
let nums = sort([3, 1, 2])                       // [1, 2, 3]
let words = sort(["banana", "apple", "cherry"])  // ["apple", "banana", "cherry"]
```

Returns a new array with the elements sorted in ascending order. Works on arrays of integers, floats, or strings. The input array is not modified.

### slice

```turbo
let mid = slice([10, 20, 30, 40, 50], 1, 3)    // [20, 30]
```

Returns a new array containing the elements from index `start` (inclusive) to `end` (exclusive). The original array is unchanged.

---

## Functional

### map

```turbo
let doubled = [1, 2, 3].map(|x: i64| -> i64 { x * 2 })    // [2, 4, 6]
let names = ["a", "b"].map(|s: str| -> str { s.upper() })
```

Transforms each element of an array by applying a function, returning a new array of the results.

### filter

```turbo
let big = [1, 2, 3, 4, 5].filter(|x: i64| -> bool { x > 3 })    // [4, 5]
let long = words.filter(|w: str| -> bool { w.len() > 3 })
```

Returns a new array containing only the elements for which the predicate returns `true`.

### reduce

```turbo
let sum = reduce([1, 2, 3, 4], 0, |acc: i64, x: i64| -> i64 { acc + x })    // 10
let product = reduce([2, 3, 4], 1, |acc: i64, x: i64| -> i64 { acc * x })
```

Folds an array into a single value by applying a function to an accumulator and each element from left to right.

---

## Math

### abs

```turbo
let a = abs(-42)    // 42
let b = abs(10)     // 10
```

Returns the absolute value of an integer.

### min

```turbo
let smallest = min(3, 7)     // 3
let smallest = min(-1, 0)    // -1
```

Returns the smaller of two integer values.

### max

```turbo
let largest = max(3, 7)     // 7
let largest = max(-1, 0)    // 0
```

Returns the larger of two integer values.

### pow

```turbo
let result = pow(2, 10)    // 1024
let cubed = pow(3, 3)      // 27
```

Raises a base to an integer exponent and returns the result.

### sqrt

```turbo
let s = sqrt(144.0)    // 12.0
let s = sqrt(2.0)      // 1.4142135623730951
```

Returns the square root of a floating-point number.

### random

```turbo
let r = random()    // a float in [0.0, 1.0)
```

Returns a pseudo-random float uniformly distributed in the range `[0.0, 1.0)`.

### random_range

```turbo
let dice = random_range(1, 6)    // an integer from 1 to 6
```

Returns a pseudo-random integer uniformly distributed between `min` and `max`, **inclusive of both bounds**.

---

## HashMap

Turbo has a fully generic, typed `HashMap<K, V>` (below), plus the original
untyped `hashmap()` handle that still backs the legacy `str → str` and
`str → int` helpers. New code should prefer the typed form.

### Typed `HashMap<K, V>`

Annotate a `hashmap()` binding with `HashMap<K, V>` to get a typed map:

```turbo
let scores: HashMap<str, int> = hashmap()
hashmap_set(scores, "alice", 10)
hashmap_set(scores, "bob", 7)

print(hashmap_get(scores, "alice") ?? 0)   // 10  — get returns V?
print(hashmap_get(scores, "carol") ?? 0)   // 0   — missing key is `none`
print(hashmap_has(scores, "bob"))          // true
print(hashmap_len(scores))                 // 2
print(hashmap_keys(scores))                // ["alice", "bob"]  — [K], sorted
hashmap_remove(scores, "bob")
```

- **Keys (`K`)** may be `int` (any integer type; hashed by identity) or `str`
  (hashed by contents). Any other key type is a compile error
  ([E0525](errors/E0525.md)).
- **Values (`V`)** may be any type: `int`, `float`, `bool`, `str`, arrays,
  structs, enums, `Result`/`Optional`, and **function values**. Reference-
  counted values (strings, arrays, structs, …) are retained on insert and
  released on overwrite/remove, so maps don't leak or alias their values.
- **`hashmap_get` returns `V?`** (an optional) for a typed map — `none` on a
  missing key. Unwrap it with `??` (default), `if let`, or `match`.
- **`hashmap_keys` returns `[K]`** — a `[str]` for string keys, `[int]` for
  int keys — sorted for deterministic iteration.

A typed map composes with first-class functions to give a dispatch table:

```turbo
fn double(x: int) -> int { x * 2 }

let ops: HashMap<str, fn(int) -> int> = hashmap()
hashmap_set(ops, "double", double)
hashmap_set(ops, "inc", |x: int| -> int { x + 1 })

let f = hashmap_get(ops, "double") ?? double
print(f(21))   // 42
```

**Type inference.** A typed map is created by annotating the `let` that binds
`hashmap()` (`let m: HashMap<K, V> = hashmap()`). The annotation is required —
Turbo does not yet infer `K`/`V` from the first insert. Function parameters and
return types spelled `HashMap<K, V>` are typed automatically; to return a typed
map from a function, build it in an annotated `let` and return that binding.

**Key restriction (v1).** Struct/array/enum/tuple/function key types are
rejected with [E0525](errors/E0525.md). Derive a stable `int` or `str` key from
a compound value if you need to key by it.

**Value-type checking (v1).** Keys and values are accepted against the declared
`K`/`V` by exact type or same numeric family (so an `int` literal fits a
`HashMap<i32, ..>` value and a float literal fits an `f32`).

**Lifetime.** Typed maps retain values on insert and release them on overwrite,
remove, and typed-map release. Flat values (`str`, or a scalar) use the fast
path: scalar values need no release, and flat rc values use normal ARC retain /
release. Aggregate values — `[str]`, structs with rc-heap fields, data enums,
and `Optional`/`Result` carrying rc data — carry a per-map release callback
generated for the concrete `V` type. That callback performs the same recursive
child release the compiler emits for ordinary scope cleanup, so aggregate value
churn is memory-flat instead of leaking nested children.

Typed map handles are also retain/release managed when they flow through typed
Turbo values. This lets nested maps compose: releasing an outer map value can
release an inner map, and the inner map uses its own stored value descriptor for
its entries. Legacy untyped maps keep their original process/request-lifetime
behavior.

**WASM.** The WASM backend supports only the legacy `str → str` map subset;
generic `HashMap<K, V>` operations are rejected there (fail-loud), not silently
mislowered.

### Legacy untyped map

The original `hashmap()` handle (no annotation) still backs the `str → str` and
`str → int` helpers. Both share one underlying map object — you pick a variant
per call, not per map. `hashmap_get` on the legacy handle returns a `str`
(empty string on a miss), and the `str → int` helpers (`hashmap_set_int`,
`hashmap_get_int`, `hashmap_inc`) are legacy-only. Prefer the typed
`HashMap<K, V>` for new code.

### hashmap

```turbo
let m = hashmap()
```

Creates and returns a new, empty hash map.

### hashmap_set

```turbo
let m = hashmap()
hashmap_set(m, "name", "Turbo")
hashmap_set(m, "version", "1.2.0")
```

Sets a string-valued key-value pair. If the key already exists, its
value is overwritten. Mutates `m` in place; the return value is unit.

### hashmap_get

```turbo
let name = hashmap_get(m, "name")    // "Turbo"
```

Returns the string value associated with the given key. The key must
exist in the map — guard with `hashmap_has()` if you're not sure.

### hashmap_set_int

```turbo
let mut m = hashmap()
m = hashmap_set_int(m, "count", 1)
m = hashmap_set_int(m, "count", hashmap_get_int(m, "count") + 1)
```

Stores an integer value under a string key. Returns
the same map so you can chain it idiomatically as
`m = hashmap_set_int(m, k, v)`. Internally shares storage with the
`str → str` variant — do not mix `hashmap_set` and `hashmap_set_int` on
the same key.

### hashmap_get_int

```turbo
let n = hashmap_get_int(m, "count")    // 1
let missing = hashmap_get_int(m, "nope") // 0
```

Returns the integer value associated with the given
key, or `0` if the key is not present. If you need to distinguish a
missing key from a stored `0`, guard with `hashmap_has()` first.

### hashmap_inc

```turbo
let m = hashmap()
let a = hashmap_inc(m, "hits")        // 1 — a missing key counts as 0
let b = hashmap_inc(m, "hits")        // 2
let c = hashmap_inc(m, "hits", 10)    // 12 — optional delta (defaults to 1)
```

Fused `str → int` increment: adds `delta` (default `1`) to the integer value
at `key`, treating a missing key as `0`, then stores and returns the new value.
Internally a single hash + single probe, so it is the fast path for
word-count-style counters — equivalent to
`hashmap_set_int(m, k, hashmap_get_int(m, k) + delta)` but with one lookup
instead of two. Shares storage with the other `str → int` entries; mutates `m`
in place.

### hashmap_has

```turbo
let exists = hashmap_has(m, "name")    // true
let missing = hashmap_has(m, "foo")    // false
```

Returns `true` if the hash map contains the given key. Works for both
`hashmap_set` and `hashmap_set_int` entries.

### hashmap_len

```turbo
let count = hashmap_len(m)    // 2
```

Returns the number of key-value pairs in the hash map. Also available as `hashmap_size`.

### hashmap_keys

```turbo
let keys = hashmap_keys(m)    // ["name", "version"]
```

Returns an array of all keys in the hash map.

### hashmap_remove

```turbo
hashmap_remove(m, "version")
```

Removes the key-value pair with the given key from the hash map.

---

## JSON

### json_get

```turbo
let json = "{\"name\":\"Turbo\",\"version\":\"0.3\"}"
let name = json_get(json, "name")    // "Turbo"
```

Extracts a value from a JSON string by key. Returns the value as a string.

### json_stringify

```turbo
let json = json_stringify("name", "Turbo")    // "{\"name\":\"Turbo\"}"
```

Creates a JSON string containing a single key-value pair.

### to_json

```turbo
@derive(Display)
struct Point { x: i64, y: i64 }

let p = Point { x: 1, y: 2 }
let json = to_json(p)    // "{\"x\":1,\"y\":2}"
```

Serializes a struct to a JSON string.

### to_json_array

```turbo
let points = [Point { x: 1, y: 2 }, Point { x: 3, y: 4 }]
let json = to_json_array(points)    // "[{\"x\":1,\"y\":2},{\"x\":3,\"y\":4}]"
```

Serializes an array of structs to a JSON array string.

---

## Database (SQLite)

Turbo embeds SQLite directly in the compiler — the engine is vendored and
statically linked, so `turbolang build` produces a single self-contained binary
with no `libsqlite3` dependency and no external database server. Database
handles and prepared statements are opaque `i64` values.

Fallible functions return a `Result` (mirroring `try_read_file`); the failure
carries the SQLite error message as the `str` error payload. Use `?` to
propagate, or `match` to handle both cases — nothing panics.

Because `?` requires the enclosing function to return a `Result`, put the
fallible work in a helper that returns one (for example `i64 ! str`) and call
it from `main`:

```turbo
fn setup(db: i64) -> i64 ! str {
    sqlite_exec(db, "CREATE TABLE IF NOT EXISTS todos (id INTEGER PRIMARY KEY, title TEXT, done INTEGER)")?
    let ins = sqlite_prepare(db, "INSERT INTO todos (title, done) VALUES (?, 0)")?  // ? = a bound parameter (1-based)
    sqlite_bind_str(ins, 1, "learn turbo")
    sqlite_step(ins)
    sqlite_finalize(ins)
    ok(0)
}

fn run() -> i64 ! str {
    let db = sqlite_open("app.db")?
    setup(db)?

    let q = sqlite_prepare(db, "SELECT id, title, done FROM todos ORDER BY id")?
    while sqlite_step(q) == 1 {                 // 1 = row, 0 = done, -1 = error
        let id    = sqlite_column_int(q, 0)
        let title = sqlite_column_str(q, 1)
        let done  = sqlite_column_int(q, 2)
        print("{id}: {title} ({done})")
    }
    sqlite_finalize(q)
    sqlite_close(db)
    ok(0)
}

fn main() {
    match run() {
        ok(_)  => print("done")
        err(e) => print("sqlite error: {e}")
    }
}
```

| Function | Signature | Description |
|----------|-----------|-------------|
| `sqlite_open` | `(path: str) -> i64 ! str` | Opens (creating if absent) a database file; returns a handle. |
| `sqlite_close` | `(db: i64) -> i64` | Closes a database handle. |
| `sqlite_exec` | `(db: i64, sql: str) -> unit ! str` | Runs one or more SQL statements with no result rows. |
| `sqlite_error` | `(db: i64) -> str` | Returns the most recent error message for a handle. |
| `sqlite_prepare` | `(db: i64, sql: str) -> i64 ! str` | Compiles a SQL statement; returns a statement handle. |
| `sqlite_bind_int` | `(stmt: i64, idx: i64, v: i64) -> i64` | Binds an integer to a `?` parameter (1-based `idx`). |
| `sqlite_bind_str` | `(stmt: i64, idx: i64, v: str) -> i64` | Binds a string to a `?` parameter (1-based `idx`). |
| `sqlite_bind_float` | `(stmt: i64, idx: i64, v: f64) -> i64` | Binds a float to a `?` parameter (1-based `idx`). |
| `sqlite_step` | `(stmt: i64) -> i64` | Advances a statement: `1` = a row is ready, `0` = done, `-1` = error. |
| `sqlite_column_int` | `(stmt: i64, col: i64) -> i64` | Reads the current row's column as an integer (0-based `col`). |
| `sqlite_column_str` | `(stmt: i64, col: i64) -> str` | Reads the current row's column as a string. |
| `sqlite_column_float` | `(stmt: i64, col: i64) -> f64` | Reads the current row's column as a float. |
| `sqlite_column_count` | `(stmt: i64) -> i64` | Returns the number of columns in the current result row. |
| `sqlite_finalize` | `(stmt: i64) -> i64` | Releases a prepared statement. Call it when you're done with each statement. |

> **Note — filesystem access.** `sqlite_open` reads and writes a real file on
> disk, so it is disabled in the browser playground sandbox and unavailable on
> the WASM target. A runnable end-to-end example lives in
> [`examples/http-sqlite-api`](../examples/http-sqlite-api).

---

## HTTP Client

### http_get

```turbo
let body = http_get("https://api.example.com/data")
print(body)
```

Performs an HTTP GET request and returns the response body as a string.

### http_post

```turbo
let response = http_post("https://api.example.com/items", "{\"name\":\"turbo\"}")
print(response)
```

Performs an HTTP POST request with the given body string and returns the response body as a string.

### http_post_with_headers

```turbo
let headers = "Content-Type: application/json\nAuthorization: Bearer sk-xxx"
let response = http_post_with_headers("https://api.example.com/items", "{\"name\":\"turbo\"}", headers)
print(response)
```

Performs an HTTP POST request with custom request headers and returns the response body as a string. The `headers` argument is a single string with one header per line, separated by `\n` (for example `"Content-Type: application/json\nAuthorization: Bearer sk-xxx"`); each line becomes a separate header on the outgoing request.

---

## HTTP Server

### http_server

```turbo
let app = http_server(8080)
```

Creates a new HTTP server bound to the given port on `127.0.0.1` (localhost only). The server does not start listening until `http_listen` is called.

### http_server_public

```turbo
let app = http_server_public(8080)
```

Creates a new HTTP server bound to the given port on `0.0.0.0` (all interfaces). Use this only when you intentionally want the server accessible from other machines. **For production, always put a reverse proxy (nginx, Caddy) in front.** See [SECURITY.md](../SECURITY.md).

### route

```turbo
route(app, "GET", "/", |req: str| -> str {
    respond_text(200, "Hello, Turbo!")
})

route(app, "POST", "/api/echo", |req: str| -> str {
    let body = request_body(req)
    respond_text(200, body)
})
```

Registers a route handler for the given HTTP method and path. The handler receives the request as a string and must return a response string (use `respond_text`, `respond_html`, or `respond_json` to construct it).

### http_listen

```turbo
let app = http_server(3000)
route(app, "GET", "/", |req: str| -> str { respond_text(200, "ok") })
http_listen(app)
```

Starts the HTTP server and begins accepting connections. This call blocks until the server is stopped. On `SIGTERM`/`SIGINT` the server stops accepting new connections, drains in-flight requests (up to a 10 s deadline), and exits `0`.

### http_config

```turbo
http_config("max_body_bytes", 1048576)   // 1 MiB body cap
http_config("read_timeout_ms", 5000)
let app = http_server(8080)
http_listen(app)
```

Sets an HTTP server tunable. **Must be called before `http_listen`.** Returns `1` on success, or `0` for an unknown key or an invalid value (a runtime error is printed to stderr; the program does not panic). Keys and defaults:

| Key | Default | Meaning |
|-----|---------|---------|
| `max_body_bytes` | 33554432 (32 MiB) | Max request body; over-cap → `413` |
| `max_header_bytes` | 16384 (16 KiB) | Max total request headers; over-cap → `431` (range 256 … 16 MiB) |
| `max_connections` | 256 | Max concurrent connections; over-cap → `503` |
| `read_timeout_ms` | 10000 | Socket read timeout while reading a request |
| `write_timeout_ms` | 10000 | Socket write timeout (closes slow-write clients) |
| `keepalive_max_requests` | 1000 | Requests served per keep-alive connection before close |
| `idle_timeout_ms` | 10000 | Wait for the next request on an idle keep-alive connection |

All values must be `>= 1`. See [`docs/production-server.md`](production-server.md) for deployment guidance.

### respond

```turbo
let text = respond_text(200, "ok")
let html = respond_html(200, "<!doctype html><html><body>ok</body></html>")
let json = respond_json(200, "{\"status\":\"ok\"}")
```

Constructs an HTTP response string with the given status code, body, and an explicit content type.

### request_body

```turbo
route(app, "POST", "/data", |req: str| -> str {
    let body = request_body(req)
    respond_text(200, body)
})
```

Extracts the body from an HTTP request string.

### request_method

```turbo
route(app, "GET", "/info", |req: str| -> str {
    let method = request_method(req)    // "GET"
    respond_text(200, method)
})
```

Returns the HTTP method (GET, POST, PUT, DELETE, etc.) from a request string.

### request_path

```turbo
let path = request_path(req)    // "/api/users"
```

Returns the path portion of an HTTP request string.

### request_query

```turbo
// For a request to /search?q=turbo
let query = request_query(req, "q")    // "turbo"
```

Extracts the value of a query parameter from an HTTP request string by key.

### request_header

```turbo
let content_type = request_header(req, "Content-Type")
let auth = request_header(req, "Authorization")
```

Returns the value of a named HTTP header from a request string.

---

## Concurrency

### channel

```turbo
let ch = channel()
```

Creates an unbounded channel for sending and receiving integer values between concurrent tasks.

### send

```turbo
let ch = channel()
send(ch, 42)
```

Sends a value into a channel. Does not block (the channel is unbounded).

### recv

```turbo
let value = recv(ch)    // blocks until a value is available
print(value)
```

Receives a value from a channel. Blocks the current task until a value is available.

### mutex

```turbo
let m = mutex(0)
```

Creates a mutex wrapping an initial integer value. Read it with `mutex_get`, overwrite it with `mutex_set`, and perform an atomic read-modify-write with `mutex_update`.

### mutex_get

```turbo
let m = mutex(0)
let value = mutex_get(m)    // 0
```

Acquires the mutex lock and returns the current value. This single read is atomic.

### mutex_set

```turbo
mutex_set(m, 42)
let value = mutex_get(m)    // 42
```

Acquires the mutex lock and sets the value. This single write is atomic.

> **`mutex_get` and `mutex_set` are each individually atomic, but combining them is not.** `mutex_set(m, mutex_get(m) + 1)` performs two *separate* locked operations: another task can change the value in between, so concurrent increments lose updates. To update a value based on its current contents — a counter, an accumulator, any read-modify-write — use `mutex_update`, which runs your closure while the lock is held.

### mutex_update

```turbo
let m = mutex(0)
let new_value = mutex_update(m, |x| x + 1)   // returns 1, stores 1
```

Atomically reads the current value, calls `closure(old)` **while the lock is held**, stores the returned value, and returns it. The closure has type `fn(int) -> int`. Because the read, the closure, and the write all happen inside one critical section, this is the correct way to mutate shared state under contention.

```turbo
// Correct shared counter: 4 tasks x 25000 increments = exactly 100000.
fn worker(m: i64, iters: i64) -> i64 {
    let mut i = 0
    while i < iters {
        mutex_update(m, |x| x + 1)
        i = i + 1
    }
    0
}

fn main() {
    let m = mutex(0)
    let a = spawn worker(m, 25000)
    let b = spawn worker(m, 25000)
    let c = spawn worker(m, 25000)
    let d = spawn worker(m, 25000)
    await a
    await b
    await c
    await d
    print(mutex_get(m))   // 100000 — no lost updates
}
```

> The closure must not touch the **same** mutex (call `mutex_get`/`mutex_set`/`mutex_update` on it) — the lock is not reentrant and would deadlock.

### sleep

```turbo
sleep(1000)    // sleep for 1 second
sleep(100)     // sleep for 100ms
```

Suspends the current task for the given number of milliseconds.

### clone

```turbo
@derive(Clone)
struct Point { x: i64, y: i64 }

let a = Point { x: 1, y: 2 }
let b = clone(a)
```

Creates a deep copy of a struct. The struct must have `@derive(Clone)`.

---

## Testing

### assert

```turbo
assert(2 + 2 == 4)
assert(x > 0, "x must be positive")
```

Asserts that a condition is `true`. Aborts with an error if the condition is `false`. An optional message can be provided.

### assert_eq

```turbo
assert_eq(add(2, 3), 5)
assert_eq("hello".len(), 5)
```

Asserts that two values are equal. Aborts with an error showing the expected and actual values if they differ.

### assert_ne

```turbo
assert_ne(a, b)
assert_ne("hello", "world")
```

Asserts that two values are not equal. Aborts with an error if they are equal.

### panic

```turbo
panic("something went wrong")
panic()
```

Immediately aborts execution with an error message. If no message is provided, a default message is used. This function never returns.

---

## Unsafe

> These functions are only available inside `unsafe` blocks. They provide direct memory access and should be used with extreme caution.

### deref

```turbo
unsafe {
    let value = deref(addr)
}
```

Reads and returns the 64-bit integer stored at the given memory address.

### store

```turbo
unsafe {
    store(addr, 42)
}
```

Writes a 64-bit integer value to the given memory address.

---

## Quick Reference

| Category | Functions |
|----------|-----------|
| **I/O** | `print`, `read_line`, `read_file`, `write_file`, `try_read_file`, `try_write_file` |
| **Strings** | `len`, `trim`, `upper`, `lower`, `split`, `contains`, `starts_with`, `ends_with`, `replace`, `index_of`, `char_at`, `repeat`, `join`, `to_str`, `str_to_int`, `str_to_float`, `str_from_char`, `pad_left`, `pad_right` |
| **Arrays** | `len`, `push`, `sort`, `slice` |
| **Functional** | `map`, `filter`, `reduce` |
| **Math** | `abs`, `min`, `max`, `pow`, `sqrt`, `random`, `random_range` |
| **HashMap** | `hashmap`, `hashmap_set`, `hashmap_get`, `hashmap_set_int`, `hashmap_get_int`, `hashmap_inc`, `hashmap_has`, `hashmap_len`, `hashmap_keys`, `hashmap_remove` |
| **JSON** | `json_get`, `json_stringify`, `to_json`, `to_json_array` |
| **Database (SQLite)** | `sqlite_open`, `sqlite_close`, `sqlite_exec`, `sqlite_error`, `sqlite_prepare`, `sqlite_bind_int`, `sqlite_bind_str`, `sqlite_bind_float`, `sqlite_step`, `sqlite_column_int`, `sqlite_column_str`, `sqlite_column_float`, `sqlite_column_count`, `sqlite_finalize` |
| **HTTP Client** | `http_get`, `http_post`, `http_post_with_headers` |
| **HTTP Server** | `http_server`, `http_server_public`, `http_config`, `route`, `http_listen`, `respond_text`, `respond_html`, `respond_json`, `request_body`, `request_method`, `request_path`, `request_query`, `request_header` |
| **System** | `exec`, `env_get`, `exit`, `type_of` |
| **Filesystem** | `file_exists`, `mkdir`, `delete_file`, `list_dir`, `path_join` |
| **Date / Time** | `time_now`, `time_ms`, `format_time` |
| **Concurrency** | `channel`, `send`, `recv`, `mutex`, `mutex_get`, `mutex_set`, `mutex_update`, `sleep`, `clone` |
| **Testing** | `assert`, `assert_eq`, `assert_ne`, `panic` |
| **Unsafe** | `deref`, `store` |
