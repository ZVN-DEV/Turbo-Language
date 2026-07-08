# turbo-dotenv

Load a `.env` file into a `HashMap<str, str>`. Built on the `try_read_file`
builtin, so a missing file is recoverable, not a crash.

```toml
[dependencies]
turbo-dotenv = "0.1"
```

## Example

```turbo
import { dotenv_load, dotenv_get } from "turbo-dotenv"

fn main() {
    let env = dotenv_load(".env")
    let host = dotenv_get(env, "HOST", "localhost")
    let port = dotenv_get(env, "PORT", "8080")
    print("serving on {host}:{port}")
}
```

Given a `.env` of:

```
# app config
HOST=0.0.0.0
export PORT=9090
TOKEN="secret value"
```

this prints `serving on 0.0.0.0:9090`.

## API

| Function | Purpose |
|----------|---------|
| `dotenv_load(path) -> HashMap<str,str>` | Read + parse a file; missing/unreadable file → empty map. |
| `dotenv_parse(content) -> HashMap<str,str>` | Parse `.env` text directly (never fails). |
| `dotenv_get(env, key, fallback) -> str` | Read a key, or `fallback` if absent. |
| `dotenv_has(env, key) -> bool` | Is the key present? |

## What the parser accepts

- `KEY=value` lines, and `export KEY=value` (the `export ` is stripped).
- Blank lines and full-line `#` comments (ignored).
- Surrounding single or double quotes on the value (stripped).
- Whitespace around the key and value (trimmed).
- Only the **first** `=` splits key from value, so URLs and base64 survive.

## Honest limitations

- **No inline comments.** `KEY=value  # note` keeps `# note` as part of the
  value — quote the value or put the comment on its own line.
- **No escape sequences, multi-line values, or `${VAR}` expansion.**
