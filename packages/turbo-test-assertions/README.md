# turbo-test-assertions

Expressive assertion helpers for `@test` functions, layered on Turbo's four
assertion builtins (`assert`, `assert_eq`, `assert_ne`, `panic`). A failing
helper aborts with a message that says *why*, instead of a bare
"assertion failed".

```toml
[dev-dependencies]
turbo-test-assertions = "0.1"
```

## Example

```turbo
import { assert_str_eq, assert_contains, assert_gt } from "turbo-test-assertions"

@test fn test_greeting() {
    let msg = "hello turbo"
    assert_contains(msg, "turbo")
    assert_str_eq("a" + "b", "ab")
    assert_gt(len(msg), 3)
}
```

Run with `turbolang test` (auto-discovers a `tests/` directory, or takes a file).

## API

- **Booleans:** `assert_true(cond, what)`, `assert_false(cond, what)`
- **Integers:** `assert_int_eq(actual, expected)`, `assert_gt/assert_ge/assert_lt/assert_le(a, b)`
- **Strings:** `assert_str_eq(actual, expected)`, `assert_contains(haystack, needle)`,
  `assert_not_contains(...)`, `assert_starts_with(s, prefix)`, `assert_ends_with(s, suffix)`,
  `assert_empty(s)`, `assert_not_empty(s)`
- **Results (`i64 ! str`):** `assert_ok(r)`, `assert_err(r)`

## How failure works

Turbo's assertion builtins abort the process (exit 1) on failure — which is
exactly how `turbolang test` decides a test failed. So these helpers verify the
**passing** path of your code; a failed assertion is the abort itself. The
`assert_ok` / `assert_err` helpers target the `i64 ! str` Result shape returned
by the `sqlite_*` and file builtins.
