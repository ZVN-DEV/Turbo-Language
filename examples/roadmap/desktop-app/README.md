# desktop-app (TurboNotes) — Roadmap Spec

> **Status: aspirational.** This example is a design document, not runnable
> code. It uses syntax and language features that are not yet implemented in
> the current Turbo compiler. See `BRIEFING.md` for the full design write-up.
> Tracked under the P3 backlog.

A native desktop markdown editor with AI-powered writing assistance, used as
the canary for "Turbo as a serious alternative to Electron / Tauri / SwiftUI"
for desktop apps.

## What this example would demonstrate

- Event-driven architecture built on an `AppEvent` algebraic data type with
  exhaustive `match` dispatch
- Compile-time-deterministic memory (CTRC), so the editor's typing loop is
  free of GC pauses even with a large undo history and background file
  watchers running
- `const fn` evaluated at compile time to bake parser tables and default
  keyboard-shortcut tables into the binary
- First-class `agent` and `tool fn` keywords integrated into a desktop
  context, with the AI assistant given typed, scoped access to editor
  state
- File I/O, file watching, full-text search, markdown rendering, and export
  paths all written in pure Turbo
- Optional chaining (`?.`), `from` imports, `Shared<T>`, and other syntax
  not yet present in the parser

## Run

This example does not currently run. Attempting `turbolang run
examples/roadmap/desktop-app/src/main.tb` will produce parse errors against
the present Turbo grammar.

Once the language reaches the milestones described in `BRIEFING.md`, the
intended entry point will be:

```bash
turbolang run examples/roadmap/desktop-app/src/main.tb
```

## Expected output

A native desktop window hosting the markdown editor. (No expected stdout —
the example is GUI-driven.)

## Caveats

- **Aspirational.** Do not edit this code expecting it to compile. The
  syntax intentionally runs ahead of the compiler.
- **For P3 follow-up.** When the language ships the missing features
  (CTRC ladder, `const fn`, optional chaining, `agent`/`tool fn` desktop
  bindings, GUI runtime), this example should be revisited and brought
  back into the runnable set.
- **No GUI runtime exists yet.** Even with all language features in
  place, a desktop windowing/event runtime would need to be added on
  the C-runtime side.
