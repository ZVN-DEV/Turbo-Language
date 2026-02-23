# TurboNotes: Desktop Application Briefing

## What This Example Demonstrates

TurboNotes is a native desktop markdown editor with AI-powered writing assistance. It showcases Turbo as a language for building desktop applications that would traditionally require Electron, Tauri, or native platform SDKs. This example covers the full scope of a real-world desktop app: file I/O, text editing, search, AI integration, keyboard shortcuts, and export.

## Key Demonstrations

### 1. Event-Driven Architecture

The entire application is driven by a single `AppEvent` algebraic data type. Every user interaction, file system change, and AI response flows through one unified event loop:

```
type AppEvent {
  TextInput(text: str)
  SaveNote
  AiSummarize
  FileChanged(event: FileEvent)
  ...
}
```

This is the same pattern used by Elm, Redux, and SwiftUI's state management -- but expressed as a first-class language construct. The compiler exhaustively checks that every event variant is handled. No event can be silently dropped.

The `dispatch_event()` function uses pattern matching to route each event to its handler. Adding a new feature means adding a new variant to `AppEvent` and the compiler tells you everywhere it needs to be handled.

### 2. Deterministic Memory -- No GC Pauses During Typing

This is the single most important advantage for a desktop text editor. When a user is typing at speed, even a 16ms GC pause causes a visible hitch. Electron apps (VS Code, Obsidian, Notion) all suffer from this -- the garbage collector runs in the middle of keystroke processing, causing dropped frames and input lag.

Turbo's compile-time memory management (CTRC) means:

- **Every keystroke is processed in constant time.** No stop-the-world pause can interrupt the editing loop.
- **Undo history does not cause memory spikes.** The undo stack holds immutable action objects that are freed deterministically when they fall off the stack.
- **File watching and auto-save run in background tasks** with their own memory scopes. A large file scan never pauses the editor.

The `perf_test.tb` file includes explicit tests for this: `test_editor_typing_throughput` inserts 10,000 characters and asserts completion within 500ms with under 20MB of memory.

### 3. `const fn` for Compile-Time Parsing Tables

The markdown parser (`markdown.tb`) builds two lookup tables at compile time:

```
const fn build_inline_trigger_table() -> [bool; 256] { ... }
const INLINE_TRIGGERS: [bool; 256] = build_inline_trigger_table()
```

This technique (borrowed from Zig) means the parser's hot path -- scanning for `*`, `` ` ``, `[`, etc. -- is a single array lookup instead of a chain of `if` statements. The table is baked into the binary. Zero runtime cost.

Similarly, the keyboard shortcut system builds its default binding table at compile time:

```
const fn build_default_shortcuts() -> [ShortcutBinding] { ... }
const DEFAULT_SHORTCUTS: [ShortcutBinding] = build_default_shortcuts()
```

### 4. AI Agent Integration in a Desktop Context

The `agent.tb` module demonstrates Turbo's first-class `agent` and `tool fn` keywords in a desktop application -- not a web API, but a locally-running AI assistant that has direct access to the editor state.

Key design choices:
- **Tools access shared state directly.** The `get_current_note_context()` tool reads the editor's `Shared<Editor>` to get the current selection, word count, and cursor position. No HTTP roundtrip.
- **Streaming responses** use `async gen` and `for await` to pipe AI output into the editor in real time.
- **The agent runs in a background task.** AI operations are spawned with `spawn async`, so the UI remains responsive while the model generates a response.

### 5. Conflict Detection and Auto-Save

The storage module (`storage.tb`) implements a real conflict detection system:

- Every saved file's SHA-256 hash is stored in memory.
- Before saving, the current disk hash is compared against the stored hash.
- If they differ, a `ConflictState.ExternalChange` is raised with both versions.
- The user can choose to keep their version or accept the external version.

Auto-save uses a debounced queue: rapid keystrokes do not trigger 60 saves per second. Instead, changes are buffered and flushed at a configurable interval (default: 3 seconds).

### 6. Full-Text Search with TF-IDF Ranking

The search module (`search.tb`) builds an inverted index with term frequency scoring:

- Tokenization with stop word removal and punctuation stripping.
- TF-IDF scoring with inverse document frequency weighting.
- Title match boosting (matches in the title score 2.5x higher).
- Prefix search for the command palette.

The performance tests assert that searching 5,000 documents completes in under 50ms.

## What This Would Look Like in Electron/Tauri vs. Native Turbo

### Electron (Obsidian, VS Code, Notion)

| Aspect | Electron | Turbo |
|--------|----------|-------|
| Binary size | ~200MB (bundled Chromium) | ~5MB (native binary) |
| Startup time | 1-3 seconds | <100ms |
| Memory at idle | 200-400MB (Chromium + V8 + Node) | 15-30MB |
| GC pauses during typing | Yes (V8 GC, unpredictable) | No (deterministic CTRC) |
| File I/O | Node.js async (event loop contention) | Native async (M:N scheduler) |
| AI integration | HTTP to external server or WASM | Direct in-process tool calls |
| IPC overhead | Electron IPC between main/renderer | None (single address space) |
| Cross-platform | Yes (Chromium abstraction) | Yes (native compilation per target) |

### Tauri (Rust + Web View)

| Aspect | Tauri | Turbo |
|--------|-------|-------|
| Binary size | ~10-20MB | ~5MB |
| UI layer | Web view (HTML/CSS/JS) | Native rendering |
| Business logic | Rust (manual memory management) | Turbo (auto-clone, no lifetimes) |
| IPC overhead | JSON serialization between Rust and web view | None |
| AI integration | Rust FFI or HTTP | First-class `agent` keyword |
| Learning curve | Rust + JavaScript + Tauri API | Turbo only |

### Native Swift/Kotlin/C++

| Aspect | Native (e.g., Swift) | Turbo |
|--------|---------------------|-------|
| Cross-platform | No (platform-specific) | Yes (compile per target) |
| Memory model | ARC (Swift) or manual (C++) | CTRC (auto-clone, no lifetimes) |
| AI integration | Import SDK, manual schema generation | `tool fn` + `agent` keywords |
| Package ecosystem | Mature (platform-specific) | Growing (universal) |

## Key Advantages

1. **Startup time.** Turbo compiles to a native binary. There is no runtime to initialize, no JIT to warm up, no web view to load. The window appears in under 100ms.

2. **Memory footprint.** A Turbo desktop app uses 15-30MB at idle. An equivalent Electron app uses 200-400MB. On a machine with 8GB of RAM, this is the difference between running 10 apps and running 30.

3. **No runtime dependency.** The compiled binary is self-contained. No Node.js, no Chromium, no JRE, no .NET. Ship a single file. Users double-click it and it works.

4. **Deterministic performance.** No GC pauses. No JIT compilation spikes. No V8 memory fragmentation after hours of use. The app performs the same at hour 0 and hour 8.

5. **AI as a first-class citizen.** The `tool fn` and `agent` keywords mean AI features are not bolted on through an SDK -- they are part of the language. The compiler generates tool schemas, validates agent configurations, and provides IDE autocompletion for tools. Adding an AI feature to a desktop app is as natural as adding a function.

6. **Single language.** The entire application -- editor logic, file I/O, search index, AI agent, keyboard shortcuts, export, tests -- is written in one language. No JavaScript-to-Rust IPC boundary. No HTML template layer. No CSS framework. One codebase, one build, one binary.

## File Overview

| File | Lines | Purpose |
|------|-------|---------|
| `turbo.toml` | 40 | Project config, window settings, autosave config |
| `src/main.tb` | 420 | Entry point, event loop, event dispatch |
| `src/models.tb` | 280 | All data types: Note, EditorState, UndoAction, SearchResult, AppEvent |
| `src/editor.tb` | 380 | Editor state machine: cursor, selection, undo/redo, text ops |
| `src/storage.tb` | 360 | File I/O, auto-save, conflict detection, directory watching |
| `src/markdown.tb` | 480 | Markdown parser + HTML renderer with compile-time tables |
| `src/search.tb` | 300 | Full-text search with TF-IDF ranking |
| `src/agent.tb` | 250 | AI writing assistant with 7 tools |
| `src/shortcuts.tb` | 310 | Keyboard shortcut handler with compile-time default bindings |
| `src/export.tb` | 340 | HTML and PDF export with embedded CSS |
| `tests/editor_test.tb` | 270 | Editor operations, undo/redo, cursor movement, markdown helpers |
| `tests/storage_test.tb` | 260 | File I/O, conflict detection, auto-save, concurrent access |
| `tests/perf_test.tb` | 290 | Large documents, search scaling, memory leaks, stress tests |
