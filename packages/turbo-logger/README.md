# turbo-logger

Leveled logging with level filtering and optional timestamps. A logger is a
small stateful handle (an `i64`) you create once and pass around.

```toml
[dependencies]
turbo-logger = "0.1"
```

## Example

```turbo
import { logger_new, log_debug, log_info, log_warn, log_error, level_warn } from "turbo-logger"

fn main() {
    let lg = logger_new(level_warn(), false)   // min level WARN, no timestamps
    log_debug(lg, "starting up")               // dropped (below WARN)
    log_info(lg, "config loaded")              // dropped
    log_warn(lg, "disk almost full")           // [WARN] disk almost full
    log_error(lg, "connection lost")           // [ERROR] connection lost
}
```

With timestamps enabled (`logger_new(level_info(), true)`), lines are prefixed
`[2026-07-07 12:00:00] [INFO] ...` using `time_now` / `format_time`.

## API

| Function | Purpose |
|----------|---------|
| `logger_new(level, timestamps) -> i64` | Create a logger with a min level and timestamp flag. |
| `logger_default() -> i64` | A logger that prints everything, no timestamps. |
| `log_debug / log_info / log_warn / log_error(lg, msg)` | Emit at that level (if it passes the threshold). |
| `log_at(lg, msg_level, msg)` | Emit at an explicit level. |
| `logger_set_level(lg, level)` | Change the min level at runtime (keeps the timestamp flag). |
| `logger_level(lg) -> i64` / `logger_has_timestamps(lg) -> bool` | Inspect config. |
| `logger_should_log(lg, msg_level) -> bool` | Would this level be emitted? |
| `logger_format(lg, msg_level, msg) -> str` | Build the line without emitting it (unit-testable). |
| `level_debug / level_info / level_warn / level_error() -> i64` | Level constants. |
| `level_label(level) -> str` | `"DEBUG"` / `"INFO"` / `"WARN"` / `"ERROR"`. |

Levels: `DEBUG(0) < INFO(1) < WARN(2) < ERROR(3)`. A message prints only when its
level is `>=` the logger's minimum. Output goes to stdout via `print`.

## Notes

- The level and timestamp flag are packed into one `mutex`-backed int, so the
  handle is a plain `i64` and `logger_set_level` is safe to call at runtime.
- Timestamps use the machine's local time zone (`format_time` semantics).
