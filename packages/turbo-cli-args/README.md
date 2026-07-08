# turbo-cli-args

Flag and positional argument parsing over a `[str]` argv. Pass the builtin
`args()` (which already drops the program name) into these pure functions.

```toml
[dependencies]
turbo-cli-args = "0.1"
```

## Example

```turbo
import { has_flag, flag_str, flag_int, positional_at } from "turbo-cli-args"

fn main() {
    let argv = args()                                  // e.g. ["serve", "--port=9090", "--verbose"]
    let cmd = positional_at(argv, 0, "help")
    let port = flag_int(argv, "port", 8080)
    let verbose = has_flag(argv, "verbose")
    print("cmd={cmd} port={port} verbose={verbose}")
}
```

```
$ turbolang run app.tb -- serve --port=9090 --verbose
cmd=serve port=9090 verbose=true
```

## API

| Function | Purpose |
|----------|---------|
| `has_flag(argv, name) -> bool` | Is `--name`, `-name`, or `--name=...` present? |
| `flag_str(argv, name, fallback) -> str` | Value of `--name=value` or `--name value`, else `fallback`. |
| `flag_int(argv, name, fallback) -> i64` | Same, parsed as an integer (invalid → `fallback`). |
| `positionals(argv) -> [str]` | Every token that does not start with `-`. |
| `positional_at(argv, index, fallback) -> str` | The Nth positional, or `fallback`. |

## Supported forms

- `--name=value` and `--name value`
- `--name` / `-name` boolean switches
- positionals — any token not starting with `-`

## Honest limitation

Without a declared schema, the parser cannot know whether a bare `--name` is a
boolean switch or expects a following value. `flag_str` treats the next token as
the value for `--name value`, and `positionals` counts every non-`-` token — so
a value passed as `--name value` is also seen as a positional. Prefer the
unambiguous `--name=value` form when a flag has a value.
