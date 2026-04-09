# todo-cli

A self-contained task manager that builds a list of `Task` structs, prints them
as a checkbox-style report, persists them to disk in a pipe-delimited format,
reloads them, marks two tasks as done, and re-renders the result. Designed as
the "hello, structs + file I/O" example for new Turbo users.

## Turbo features shown

- `struct` definitions with mixed scalar and `bool` fields
- Array literals of structs (`[Task { ... }, Task { ... }]`)
- `for` loops with destructuring against struct fields
- String interpolation in `print` (`"  {check} {label} #{t.id}  {t.title}"`)
- File I/O via `read_file` / `write_file`
- `split` for splitting on a delimiter
- Conditional expressions (`if t.done { "[x]" } else { "[ ]" }`)
- Closures and helper functions as values

## Run

```bash
turbolang run examples/todo-cli/main.tb
```

## Expected output

```
==================================================
  Turbo Todo Manager
==================================================

  All tasks:
  [ ] [!!!] #1  Set up CI/CD pipeline
  [ ] [!!!] #2  Write unit tests
  [ ] [!! ] #3  Design landing page
  [ ] [!  ] #4  Update README
  [ ] [!! ] #5  Review pull requests
  [x] [!!!] #6  Fix login bug

  Total: 6  Done: 1  Remaining: 5  High: 3
  Saved to /tmp/turbo_todos.txt
  Loaded 6 lines from disk

  Completing tasks #1 and #2...
  ...
  Done!
==================================================
```

(Status counts in the middle reflect the second pass where #1 and #2 are
flipped to `done`.)

## Caveats

- Writes to `/tmp/turbo_todos.txt` — on Windows you'd need to adjust the path
  (Windows is not currently a supported target).
- Pure stdout; no real interactivity yet.
