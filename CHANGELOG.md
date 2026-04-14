# Changelog

All notable changes to the Turbo compiler are documented here.
Format based on [Keep a Changelog](https://keepachangelog.com/).

## [Unreleased]

### Security
- Install script now pins the GPG release key against a local copy in-repo; mismatched remote key aborts the install.

### Changed
- Documentation: removed `agent`, `tool fn`, and "first AI-native language" claims from core-facing copy. Agentic features will ship in a separate `turbo-agent` sidecar library after the core language hits 1.0 stability, not inside the compiler.
- Added COMPATIBILITY.md documenting the 1.0 stability contract and what remains fluid in 0.7.x and 0.8.x.

### Notes
- Error-code docs audit: `ErrorCode` variants, `turbo-cli/src/errors/` entries, and `docs/errors/` entries are all in sync at 87/87/87. The previously flagged "94 variants vs 87 docs" gap does not exist.

---

## [0.7.6] - 2026-04-13

Trust and release hardening follow-up. This release replaces implicit HTTP response typing with explicit helpers, makes shell execution more explicit, adds server runtime guardrails, promotes the web dashboard as the flagship runnable demo, and hardens the release/install path.

### Changed
- Added explicit HTTP response helpers: `respond_text`, `respond_html`, and `respond_json`.
- `exec()` is now mirrored by the clearer `shell_exec()` name and remains gated behind `@unsafe`.
- Public docs and website copy now consistently distinguish shipped capabilities from roadmap work.
- `web-dashboard` is now the recommended first-run demo across README, examples, and website docs.

### Fixed
- Browser-facing Turbo demos now emit explicit content types instead of relying on fragile heuristics.
- Roadmap auth examples no longer normalize insecure JWT secret defaults or permissive bind/CORS defaults.
- Playground version, error-code docs, and metadata drift were cleaned up.

### Security
- Release workflows are pinned to immutable GitHub Action SHAs.
- `install.sh --verify` now verifies a signed release manifest before trusting tarball checksums.
- HTTP servers now reject invalid server IDs and apply a connection cap with 503 backpressure when overloaded.

---

## [0.7.5] - 2026-04-13

Pre-launch hardening sprint. Fixed critical runtime security issues,
cleaned all stale agent content, and strengthened edge-case testing.

### Security
- **Overflow-checked array allocation** — `rt_array_alloc`, `rt_array_push`,
  `rt_array_set`, `rt_str_split` now use `checked_mul`/`checked_add` to
  prevent heap overflow on large or negative lengths.
- **Thread-local string arena** — all 25 `CString::into_raw()` call sites
  now go through `arena_str()`, preventing memory leaks in long-running
  programs. Arena is freed after each JIT execution.
- **COW refcount ordering** — changed from `Ordering::Relaxed` to
  `Acquire`/`AcqRel` to prevent use-after-free in concurrent code.
- **serde_json replaces hand-rolled JSON parser** — the old string-find
  parser had no escape handling, no depth limits, and was vulnerable to
  key confusion. Now uses RFC 7159-compliant parsing.
- **Defensive unwrap audit** — 19 bare `.unwrap()` calls on `CString::new()`
  replaced with `.unwrap_or_else()` fallbacks for NUL byte safety.

### Removed
- `design/AGENTIC.md` — stale design doc for removed agent features.
- `examples/roadmap/desktop-app/` and `examples/roadmap/task-agent/` —
  used removed `agent`/`tool fn` syntax.
- Agentic features removed from `design/ROADMAP.md` v1.0 goals.

### Changed
- README tagline updated: "TypeScript's ease. Rust's speed. No GC, no
  borrow checker."
- README known limitations updated to reflect string arena.
- Error codes E0312, E0321, E0322, E0511 annotated as deprecated.

### Added
- Edge-case integration tests: `array_bounds_error`, `json_edge_cases`,
  `string_heavy`, `array_overflow`, `string_arena_basic`.
- CI job: `test-overflow` runs unit tests with `overflow-checks=on`.

---

## [0.7.4] - 2026-04-12

Gold-standard open source audit. Hardened the C runtime, split two
monolithic source files, added 45 new tests, and improved CI, release,
and install infrastructure.

### Changed
- **Split turbo-codegen-llvm/src/lib.rs** (6306 lines) into 7 modules:
  types.rs, ctx.rs, helpers.rs, expr.rs, stmt.rs, builtins.rs, and a
  reduced lib.rs (1315 lines). Follows the same structure as the
  Cranelift backend.
- **Split turbo-sema/src/type_check.rs** (4738 lines) into a directory
  module: type_check/{mod.rs, expr.rs, stmt.rs}. The 3470-line
  `check_expr_inner` match now lives in its own file.
- **Binary size reporting** added to the CI `build` job — reports size
  in GitHub step summary on every PR.
- **Canary release channel** — nightly CI now publishes a rolling
  `nightly` pre-release on GitHub with the latest binary.

### Fixed
- **rt_read_line() silent truncation** — replaced fixed 4096-byte
  `fgets` stack buffer with POSIX `getline()` for dynamic allocation.
  Long user input is no longer silently cut off.
- **cargo-audit version mismatch** — nightly.yml pinned 0.21.2 while
  ci.yml pinned 0.22.1. Both now use 0.22.1.
- **Clippy warning** in parser `soft_keyword_ident()` — removed
  unnecessary match wrapping a single `_ => None` arm.
- Removed stale `tool fn` sema tests (agent primitives removed in v0.7.3).

### Added
- **37 sema unit tests** covering type inference, function return types,
  binary ops, struct/enum checking, closures, builtins, error
  propagation (Ty::Error no-cascade), optionals, results, impl blocks,
  const declarations, compound assignment, and check_test mode.
- **8 property-based parser tests** (proptest) verifying: parser never
  panics on random token streams, valid programs always parse, and
  tokenize-then-parse never panics on arbitrary strings.
- **GPG verification** — `distribution/install.sh` now accepts
  `--verify` to download and check GPG signatures on release tarballs.

## [0.7.2] - 2026-04-09

Import ergonomics release. The transitive import walker introduced in
v0.7.0 expanded each imported file in isolation, which meant cross-file
reference chains (main imports entry from A, A's entry calls helper,
helper lives in B) couldn't be resolved automatically — the user had
to name every transitively-used helper in their explicit import clause
even when the compiler already had the defining file in hand. This
release makes the walker global.

### Fixed
- **Cross-module transitive import resolution**
  (`turbo-cli/src/main.rs::resolve_imports`). Refactored from
  per-import sequential processing into a three-phase pipeline:
  gather all imported files first, run a global fixed-point expansion
  across every imported module at once, then extract. Now a reference
  in file A to a helper defined in file B is pulled in automatically
  as long as B is in the host module's import set.
  Regression: `tests/phase1/imports/transitive_crossmod_{main,a,b}.tb`.
- **Carl Code import clause shrink.** Proof point for the walker:
  Carl's `main.tb` shrinks from 35 → 29 explicit import items because
  the compiler now traces `AgentProfile`, `SquadResult`,
  `pick_agent_names_for_task`, `resolve_squad`, `print_squad_assembling`,
  and `print_squad_complete` across the boundary between the host
  module's imports.
- **Five Carl-Code-surfaced language papercuts** (landed together in
  64a6adc): empty array literal inference, soft-keyword identifier
  parsing (`agent`/`tool`/`resource`/`prompt`), brace escape hints in
  string interpolation, tool-annotated fn imports, `mut` function
  parameters.
- **Single-module transitive import dedup.** When the same helper is
  reachable through multiple import chains (e.g. two libs both pulling
  `color_cyan` from `display/output`), resolve_imports now drops
  duplicates by defining name instead of emitting E0308.

### Added
- `exec()` and `env_get()` stdlib builtins (landed in abd5186).

## [0.7.1] - 2026-04-08

Release infrastructure patch. v0.7.0 shipped the compiler improvements
but the release pipeline itself was broken: the tagged build could not
produce artifacts because `release.yml` used the `secrets` context in
step-level `if:` expressions (which GitHub Actions does not allow), and
CI was red on an unrelated rustfmt nit plus an advisory-db parse error
in the pinned `cargo-audit 0.21.2`. This release unbreaks all of that
so v0.7.x can actually ship binaries.

### Fixed
- **Release workflow validation.** `release.yml` no longer references
  the `secrets` context directly inside `if:`. A preceding "Probe release
  secrets" step now exposes GPG-key and Homebrew-tap-token presence as
  step outputs, and the `Sign checksums.txt` and `Update Homebrew tap`
  steps gate on those outputs instead. Same fork-safe semantics,
  GitHub-Actions-legal form.
- **CI Cargo audit.** Bumped the pinned `cargo-audit` from `0.21.2` to
  `0.22.1` in `ci.yml`. `0.21.2` cannot parse CVSS 4.0 strings in newer
  RUSTSEC advisory entries (e.g. `RUSTSEC-2025-0138`), which caused
  `cargo audit` to fail before it even evaluated the workspace.
- **rustfmt compliance.** Single-lined two `std::mem::replace` calls in
  `turbo-parser/src/cow_rewrite.rs` that slipped through v0.7.0 because
  of a local rustfmt version skew. `cargo fmt --check` is green again.

## [0.7.0] - 2026-04-08

Compiler-correctness and tooling release. Fixes a class of silent-drop
bugs around copy-on-write (COW) builtins in every expression context,
introduces JIT/AOT parity test infrastructure, ships 91 uniquely
documented error codes with build-time exhaustiveness enforcement,
adds code actions / rename / semantic tokens to the LSP, and splits
the semantic analyzer into focused modules.

### Added
- **Post-parse COW rewrite pass** (`turbo-parser/src/cow_rewrite.rs`).
  The 9 COW builtins (`push`, `map`, `filter`, `replace`, `upper`,
  `lower`, `trim`, `repeat`, `split`) are now rewritten to self-assigns
  only in statement position. In r-value contexts — bare block, if/else,
  match arm, inferred-return closure body — the call's value now flows
  through correctly. Regression tests: `cow_tail_expression.tb`,
  `cow_tail_rvalue_contexts.tb`, `cow_return_unit_fn.tb`.
- **Unit-fn-aware return rewriting.** `return <cow-call>` inside a
  unit-return function is now rewritten to a self-assign instead of
  being treated as value-returning, so the mutation is actually observed.
- **JIT/AOT parity test harness** (`turbo/tests/parity/`). Runs every
  program through both backends and diffs stdout + exit code. Sibling
  files `.expected_exit` / `.expected_stderr` pin failure-mode behavior;
  new `.expect_divergence` xfail marker flags known-divergent programs
  (e.g. Unicode whitespace tokenization).
- **91 documented error codes.** Every `E0NNN` variant now has a unique
  entry in `turbo-ast::ErrorCode`, a Markdown explanation under
  `turbo-cli/src/errors/E0NNN.md` (embedded via `include_str!`, served
  by `turbolang explain E0NNN`), a public mirror at `docs/errors/E0NNN.md`,
  and a `build.rs` exhaustiveness check that fails the build if any
  variant is missing its docs file.
- **LSP: code actions** (`turbo-lsp/src/code_actions.rs`). Quick fixes
  for common diagnostics.
- **LSP: workspace-wide rename** (`turbo-lsp/src/rename.rs`).
- **LSP: semantic tokens** (`turbo-lsp/src/semantic_tokens.rs`). Proper
  syntax highlighting via the LSP semantic-tokens protocol.
- **Sema: focused modules.** Exhaustiveness checking, scope/lexical
  analysis, and type checking moved to their own files
  (`exhaustiveness.rs`, `scope.rs`, `type_check.rs`).
- **Nightly LLVM canary CI** (`.github/workflows/nightly.yml`). Builds
  the LLVM backend nightly to catch upstream LLVM drift early.
- **Lint hardening: `scripts/check_error_codes.py`.** Two-pass helper
  detection (taking-code vs not-taking-code), balanced-bracket generics
  walker, and a `--self-test` mode with 17 assertions. Replaces the
  fragile regex-based version.
- **Phase1 regression tests.** Roundtrip + limits tests pinning array
  push/pop, hashmap CRUD, integer arithmetic edges, math-function
  edges, string builtins, and UTF-8 concatenation.
- **Per-example README docs.** Every `examples/` subdirectory now has
  a README explaining what the example demonstrates.
- **Formatter/tooling configs.** `turbo/clippy.toml`, `turbo/rustfmt.toml`,
  `turbo/.cargo/` workspace config, and `turbo/.git-hooks/pre-commit`
  are now checked in.

### Changed
- **Error-code exhaustiveness is enforced at build time.**
  `turbo-cli/build.rs` parses `turbo-ast::ErrorCode` and fails the build
  if any variant is missing a `docs/errors/` entry. Matches the existing
  `include_str!` exhaustiveness on the source-of-truth side.
- **`docs/errors.md`** is now a machine-checked index of all 91 codes.

### Fixed
- **COW builtins in tail expression context silently discarded their
  value.** Pre-fix, `fn single_replace(s: str) -> str { replace(s, "a",
  "b") }` returned `()` and produced a confusing `E0109` at the
  declaration. The post-parse rewrite pass now preserves the tail value.
- **`.map(|w| { replace(w, ...) })` failed with E0501** on the immutable
  closure parameter because the parser rewrote the brace-body tail into
  `w = replace(w, ...)`. Closure bodies now compile.
- **`return <cow-call>` in a unit-return function was a silent NOP.**
  Pre-fix, `fn f() { let mut arr = [1] return push(arr, 2) }` compiled
  and ran but discarded the pushed array. Now correctly self-assigns.
- **JIT libm symbol resolution on Linux.** `extern "C"` calls to libm
  functions now resolve under the JIT on Linux.
- **C runtime POSIX/BSD feature test macros.** `turbo_rt.c` now declares
  the POSIX and BSD feature-test macros required for glibc compatibility
  (`clock_gettime`, `strdup`, BSD extensions), unblocking Linux release
  builds.

## [0.6.0] - 2026-04-07

> **Note:** Agent primitives (`agent` keyword, `tool fn`, LLM provider integration) were removed in v0.7.3. The memory management, codegen refactor, and security hardening from this release remain.

Real LLM agent integration, memory management overhaul, codegen refactor,
and runtime security hardening. This is the first release where `agent`
definitions can call live LLM APIs (OpenAI, Anthropic) — not just mocks.

### Added
- **Real LLM provider integration.** Agents can now call OpenAI (`gpt-*`
  models) and Anthropic (`claude-*` models) APIs via environment variables
  `OPENAI_API_KEY` / `ANTHROPIC_API_KEY`. Provider is auto-detected from
  the model name or explicit prefix (`openai:`, `anthropic:`, `mock:`).
- **Agent tool calling loops.** Agents with tools can execute multi-turn
  tool-calling loops (up to 4 iterations) via `__CALL_TOOL__` directives.
- **Agent structured output.** `agent.ask_structured(prompt)` appends
  JSON schema constraints and validates output against the agent's
  `output` type definition.
- **Agent streaming.** `agent.stream(prompt)` returns chunked responses
  as a string array.
- **New agent test suite.** 6 new integration tests: `agent_ask`,
  `agent_ask_structured`, `agent_ask_structured_string`,
  `agent_ask_structured_typed`, `agent_stream`, `agent_tool_loop`.
- **LLVM backend CI job.** The LLVM codegen crate is now tested in CI
  on Ubuntu with LLVM 18. Runs on path changes to `turbo-codegen-llvm/`;
  `continue-on-error: true` so failures don't block the main pipeline.

### Changed
- **Codegen refactor.** Split the monolithic `lib.rs` (7,462 lines)
  into 5 focused modules: `expr.rs` (expression compilation), `stmt.rs`
  (statement compilation), `jit.rs` (JIT entry), `aot.rs` (AOT/WASM
  entry), plus the core `lib.rs` (types, context, module compilation).
  No logic changes — purely structural.

### Fixed
- **Memory leak resolved.** `rt_release()` now performs real reference-
  counted freeing instead of being a no-op. Combined with the v0.5.1
  per-request arena allocator, long-running servers no longer leak.
- **Buffer overflow in `rt_agent_context_dup()`.** Replaced unsafe
  `strcat()` after `snprintf()` with bounds-checked `snprintf()` append.
- **Buffer overflow in `rt_join_string_array_dup()`.** Replaced O(n²)
  `strcat()` loop with `memcpy` + offset tracking.
- **Buffer overflow in `rt_str_repeat()`.** Replaced `strcat()` loop
  with `memcpy` + offset tracking.
- **Buffer overflow in `rt_json_stringify()`.** Now escapes key and
  value via `rt_json_escape_dup()` before sizing the buffer, preventing
  overflow on strings containing quotes or backslashes.
- **Mock structured output.** Mock providers (`mock:structured`,
  `mock:structured_str`) now correctly strip schema instructions from
  echoed responses, fixing `ask_structured` test failures.

### Security
- **Playground security headers.** All HTTP responses now include
  `Content-Security-Policy`, `X-Frame-Options: DENY`,
  `X-Content-Type-Options: nosniff`, and
  `Referrer-Policy: strict-origin-when-cross-origin`.
- **Eliminated all `strcat()` calls in `turbo_rt.c`.** Every instance
  replaced with bounds-checked alternatives (`snprintf`, `memcpy` with
  offset tracking). This closes the CWE-680 class of vulnerabilities
  identified in the v0.5.1 security audit.

## [0.5.1] - 2026-04-06

Security backport release. All sprint findings are landed; no breaking
API changes. Addresses 6 confirmed live exploits in the runtime.

### Security
- **`rt_http_get` / `rt_http_post` SSRF + arg-injection hardening.**
  Both functions now reject any URL whose scheme isn't `http://` or
  `https://` (case-insensitive), reject NULL/empty/`-`-prefixed input,
  and pass `--` to curl before the URL so flag-shaped values can no
  longer be re-interpreted as flags. The curl invocation is also
  pinned to `--proto =http,https`, capped at `--max-time 30`, and
  limited to `--max-redirs 5`. Closes the `http_get("file:///etc/hosts")`
  and `http_get("--help")` exploits. **Both the C AOT runtime
  (`turbo_rt.c`) and the Rust JIT runtime (`runtime.rs`) are hardened**
  — earlier patches missed the JIT path used by `turbolang run`.
- **HTTP server Content-Length DoS fix.** The request parser used
  `atoi()`, which silently returned 0 on parse failure and silently
  accepted negative values — a `Content-Length: -1` header would
  crash the server when the negative int was reinterpreted as a huge
  `size_t` and fed into `memcpy`. Now we use `strtoll` with `ERANGE`
  detection and reject anything outside `[0, RT_HTTP_MAX_BODY]`
  (32 MiB). Invalid requests get `400 Bad Request` and the connection
  closes; we never pass a bad length to `memcpy`.
- **HTTP server now binds `127.0.0.1` by default.** `http_server(port)`
  used to bind `INADDR_ANY` with no opt-out, exposing development
  servers to the network on first run. The new default is loopback
  only. A new explicit `http_server_public(port)` opt-in binds
  `0.0.0.0` for callers who deliberately need external exposure.
- **`rt_str_repeat` integer-overflow fix.** The previous implementation
  computed `len * count` without checking for wraparound. A large
  count would silently produce a tiny allocation that the subsequent
  `strcat` loop wrote past. Now we check `count > (SIZE_MAX - 1) / len`
  before the multiply and return an empty string with a stderr
  diagnostic on overflow. We also enforce a practical
  `RT_STR_REPEAT_MAX_BYTES = 256 MiB` cap so a mathematically valid
  but absurd request (e.g. `repeat("ab", 9_223_372_036_854_775_000)`)
  cannot abort the JIT process via Rust's `Vec` capacity-overflow
  panic. Mirrored in both the C and Rust runtimes.

### Added
- `SECURITY.md` with disclosure process, scope, response SLA
  (48-hour ack, 7-day critical fix target), and known hardening limits.
- `CHANGELOG.md` (this file). Backfilled entries from v0.1 to v0.5.
- C runtime test harness at
  `turbo/crates/turbo-codegen-cranelift/runtime/tests/` with a
  matching `tests.sh` runner. Covers `rt_str_repeat` overflow,
  HTTP scheme rejection, flag-injection rejection, and basic
  string ops. Wired into a new `c-runtime-tests` CI job.
- Codegen fuzz target — extends the existing fuzz harness with a
  `codegen` mode that runs lex → parse → sema → JIT-compile through
  `panic::catch_unwind`. Default 200 iterations, override via
  `TURBO_FUZZ_ITERS`.
- `rt_http_server_public(port)` builtin for opt-in public binds.
- "Did you mean?" suggestions for unresolved identifiers in sema,
  computed via a hand-rolled Levenshtein distance (no new deps).

### Changed
- CI now runs the integration test suite
  (`turbo/tests/run_tests.sh`) on both `ubuntu-latest` and
  `macos-latest`, plus separate `fmt`, `clippy`, and `c-runtime-tests`
  jobs. `actions/cache@v4` keys are pinned via `hashFiles` only —
  no `restore-keys` so stale caches can't poison a build.
- `README.md` carries a prominent **Known Limitations** callout
  about the runtime memory leak and the experimental HTTP server.

### Removed
- Stale internal planning docs deleted from the public repo:
  `findings.md`, `INDEX.md`, `PLAN-2025-03-31.md`, and
  `turbo/cow-bug-audit.md`. These patterns are now in `.gitignore`
  so they cannot accidentally come back.

### Known Issues
- **Runtime memory leak.** `rt_release` is currently a no-op, so
  long-running servers and hot loops that allocate repeatedly will
  leak memory (~2.5 KB/request on the example HTTP server). Real
  reference counting is planned for **v0.6**. Tracked in `TODO.md`.
- **HTTP server is experimental.** Use behind a reverse proxy.
  See `SECURITY.md` for the threat model.

## [0.5.0] - 2026-04-06

### Added
- **WASM compilation target**: `turbolang build --target wasm app.tb` compiles Turbo programs to WebAssembly via C transpilation (AST → C → clang → wasm-ld → .wasm), runs in wasmtime/wasmer
- **Cross-compilation**: `turbolang build --target linux-arm64` and `--target linux-x86` produce Linux ELF binaries from macOS using Zig cc as cross-linker
- **C FFI**: `@unsafe extern "C" { fn floor(x: f64) -> f64 }` declares external C functions callable from Turbo code; `--link` flag for additional libraries
- WASM-compatible runtime (`turbo_rt_wasm.c`) with stubs for unsupported platform features
- Cross-compiler discovery: env var override → Zig cc → GNU cross-compiler → helpful error
- Target aliases: `linux-arm64`, `linux-x86`, `macos-arm64`, `macos-x86`, `wasm`
- FFI type safety validation — only scalar types (i8..i64, u8..u64, f32, f64, bool, str, unit) permitted in extern signatures
- 6 new integration tests for C FFI (happy path + error cases)

### Changed
- `cranelift-codegen` now built with `features = ["all-arch"]` for cross-architecture codegen

### Tests
- 275 unit tests, 166 integration tests (441 total, all passing)

## [0.4.0] - 2026-04-05

### Added
- **Watch mode**: `turbolang run --watch` / `turbolang run -w` auto-recompiles on file changes with 300ms debounce, screen clearing, and child process management for long-running programs
- **Numeric types with progressive disclosure**: `int` and `float` aliases for `i64`/`f64`, plus narrow types `u8`, `i8`, `i16`, `u16`, and `usize` for when you need control
- **Standard library modules**: `import { trim } from "std/string"` with virtual module validation — 12 modules covering all 64 builtins
- Integer literal coercion with range checking (`let x: u8 = 255` works, `let x: u8 = 256` errors)
- Error codes E0520 (unknown std module) and E0521 (unknown name in std module)

### Changed
- `Ty::I64` displays as `int` and `Ty::F64` displays as `float` in error messages (progressive disclosure UX)
- Type names `int`, `float`, `usize` accepted as aliases in type annotations

### Tests
- 275 unit tests, 160 integration tests (435 total, all passing)

## [0.3.2] - 2026-04-05

### Added
- Trailing comma support in arrays and function calls
- Optional values now print as `some(42)` / `none` instead of `[optional]`
- Unused variable warnings (E0515) for `let` bindings
- Enhanced `turbolang explain` with code examples for 12 error codes
- Doc comment extraction (`///`) attached to functions, structs, and enums
- REPL readline support with history (via rustyline)
- `--quiet` flag for `turbolang bench`
- Complete standard library reference (`docs/stdlib.md`) covering all 64 built-in functions
- Comprehensive release checklist (`docs/RELEASE.md`)

### Changed
- Formatter now adds spaces around `=` in assignments
- `turbolang init` template includes struct methods, enum pattern matching, and Result examples
- README Result type example uses lowercase `ok(T)`/`err(str)` matching actual syntax
- SYNTAX.md arrow functions marked `[Implemented]`
- Showcase docs page updated to show actual flat built-in functions (not fictional namespaced modules)

### Fixed
- Unused variable false positives on match bindings, callback params, and if-let patterns
- Test runner stderr handling for warning compatibility

### Tests
- 266 unit tests, 155 integration tests (417 total, all passing)

## [0.3.1] - 2026-04-04

### Added
- Generic impl blocks (`impl Pair<A, B> { ... }`)
- Generic type arguments in return types (`-> Pair<B, A>`)
- `hashmap_size()` builtin (alias for `hashmap_len`)
- HTTP request context builtins (`request_method`, `request_path`, `request_query`, `request_header`)

### Fixed
- `.push()` method syntax now auto-reassigns (no more silent no-op)
- `f32` float literal coercion from `f64` annotations
- README type system and enum constructor examples

### Changed
- Multi-threaded HTTP server with keep-alive (5x throughput improvement)

### Tests
- 151 integration tests passing (up from 148)

## [0.3.0] - 2026-04-03

### Added
- **Arrow closures**: TypeScript-style `(x: int) => x * 2` syntax alongside existing `|x| x * 2`
- **Typed const declarations**: `const MAX: int = 100` with type annotation validation
- **`push` builtin**: array push as a proper builtin function with type checking
- **`if let` expressions**: pattern matching on `Optional` and `Result` values inline
- **Optional chaining**: `user?.name` syntax for safe field access on optional structs
- **Struct destructuring**: `let { x, y } = point` for extracting struct fields
- **Map literals**: `{"key": value}` sugar for hashmap creation
- **String interpolation nesting**: fixed lexer to handle nested quotes inside `{...}` blocks
- **Formatter improvements**: operator spacing (`==`, `!=`, `<=`, `>=`, `&&`, `||`) and brace spacing (`else {`)
- 8 new integration test files covering all new features
- 259 unit tests, 151 integration tests all passing

### Fixed
- String interpolation lexer now correctly handles `"hello {get_name("world")}"` with nested quotes
- Expression-body closures with typed parameters now correctly return their value
- Optional-returning functions no longer lose inner type info when inlined
- Formatter no longer mangles generic type parameters when adding operator spacing

## [0.2.2] - 2026-04-03

### Added
- LLVM 18 AOT backend now ships with Homebrew install (`turbolang build --llvm`)
- REPL auto-prints expression results (no need to wrap in `print()`)
- `turbolang doc` properly extracts fields from single-line struct definitions

### Fixed
- Suppress `<error>` type leak in optional/result diagnostics (recursive `contains_error()`)
- Website examples updated to use string interpolation instead of deprecated `to_str()` concatenation
- Variables & Types docs now document string interpolation nesting limitation

### Changed
- Release CI builds include LLVM backend for all 3 targets (ARM macOS, Intel macOS, Linux x86_64)

## [0.2.1] - 2026-04-03

### Added
- Production error codes (E0001-E0513) for all compiler diagnostics, Rust E0308-style
  - `ErrorCode` enum in `turbo-ast/src/errors.rs` with `as_str()`, `description()`, `Display`
  - All `SemaError`, `ParseError`, and `CodegenError` types now carry an `ErrorCode`
  - CLI displays errors as `error[E0100]: message` via ariadne
  - New `turbolang explain <code>` CLI subcommand prints the error description
  - Error code reference: `docs/errors.md`
- Gold standard audit: CI workflow, SECURITY.md, CLAUDE.md, allocation safety, strip binary
- CONTRIBUTING.md and CHANGELOG.md documentation
- `turbo-lsp` binary now included in Homebrew formula and release artifacts

### Fixed
- Replace last `panic!` in sema with proper error diagnostic
- Security audit: safety fixes, tracked artifact cleanup, .gitignore hardening
- `turbolang test <dir>` now discovers `@test` functions in all `.tb` files, not just `test_*`/`*_test` named files

### Removed
- Implicit `int + str` / `str + int` coercion in the `+` operator — use `to_str()` or string interpolation instead

## [0.1.0] - 2026-03-10

Initial public release of the Turbo compiler.

### Language Features
- Core types: `i32`, `i64`, `u32`, `u64`, `f32`, `f64`, `bool`, `str`, `()`, arrays `[T]`
- `let` / `let mut` variable bindings with type inference
- `if`/`else` and `while` as expressions (return values)
- `for..in` loops over arrays
- `break` and `continue` in loops
- `match` expressions with pattern matching
- Match guards (`n if n > 0 => ...`)
- Enum types with data-carrying variants (tagged union representation)
- Match exhaustiveness checking
- Struct types with field access and field assignment
- Generic functions with type parameter inference
- Generic structs and enums with type parameters
- Multi-parameter generics with trait bounds
- Basic trait system with built-in `Display` trait
- Default trait methods
- `@derive(Eq, Clone, Display)` attribute for auto-generating trait implementations
- Optional types (`T?`)
- `Result`/`Error` types with `ok`, `err` constructors and pattern matching
- `?` operator for error propagation
- Closures with variable capture (heap-allocated environments)
- Closure parameter type inference and single-expression bodies
- Higher-order functions (function types as parameters)
- `async fn`, `spawn`, and `await` with thread-based concurrency
- `sleep` builtin for async timing
- Channels and mutex builtins for thread communication
- `agent` keyword with instantiation and field access (removed in v0.7.3)
- `tool fn` keyword for AI agent primitives (removed in v0.7.3)
- `const` declarations
- `defer` statements
- `import` system for cross-file code sharing
- String interpolation with `{expr}` syntax
- String concatenation (`+`) and equality (`==`, `!=`) operators
- Multiline strings
- String coercion (automatic `to_string` in print contexts)
- Array index assignment and struct field assignment
- Copy-on-write semantics for shared arrays
- ARC refcount header on all heap allocations
- `@unsafe` functions and raw pointer operations
- `@test` attribute on functions with `assert_eq`/`assert_ne` builtins

### Built-in Functions
- `print` -- polymorphic output for all types
- `assert`, `assert_eq`, `assert_ne` -- runtime assertions
- `abs`, `min`, `max` -- math operations
- `to_str` -- value to string conversion
- `to_json`, `to_json_array` -- struct serialization to JSON
- `len`, `push` -- array operations
- `map`, `filter`, `reduce` -- higher-order collection operations
- `str_*` -- string manipulation builtins
- `hashmap`, `hashmap_get`, `hashmap_set`, `hashmap_keys` -- key-value storage
- `channel`, `mutex` -- concurrency primitives
- HTTP client builtins -- `http_get`, `http_post`, JSON parsing
- Math builtins -- `math_sqrt`, `math_floor`, etc.
- I/O builtins -- `read_file`, `write_file`, `read_line`

### Compiler Infrastructure
- Five-stage pipeline: Lexer (logos) -> Parser (recursive descent) -> Sema -> Codegen -> Link
- Cranelift JIT backend (`turbolang run`) for rapid development
- Cranelift AOT backend (`turbolang build`) for native binary production
- C runtime (`turbo_rt.c`) linked into AOT binaries for print, allocation, strings, arrays, async
- Multi-error parser recovery (no bail-on-first-error)
- `Ty::Error` poison type in sema to prevent cascading diagnostics
- Colorized error reporting with `ariadne` and contextual help messages
- Function inlining optimization in codegen
- Benchmark suite comparing against C (gcc -O2)
- Comprehensive test suite: integration, regression, and adversarial tests
- Builtin shadowing rejection and argument count validation in sema

### Toolchain
- `turbolang run <file>` -- JIT compile and execute
- `turbolang build <file>` -- AOT compile to native binary
- `turbolang test <file>` -- run `@test` functions with subprocess-based runner
- `turbolang bench <file>` -- benchmark with timing
- `turbolang fmt <file>` -- source code formatter
- `turbolang doc <file>` -- documentation generator
- `turbolang init <name>` -- project scaffolding (with `.gitignore`)
- `turbolang install` -- dependency resolution from `turbo.toml` / `turbo_modules`
- `turbolang update` -- GitHub package registry support
- `turbolang repl` -- interactive REPL
- `turbolang lsp` -- Language Server Protocol server (diagnostics, hover, go-to-definition)
- VS Code extension (`zvndev.turbo-lang`) -- syntax highlighting, snippets, LSP client
- Built-in HTTP server framework with socket-based runtime

### Distribution
- Install script (`install.sh`) for curl-based installation
- Homebrew formula (`brew tap ZVN-DEV/turbo && brew install turbo-lang`)
- Dockerfile for containerized builds
- CI release workflow (`.github/workflows/release.yml`) -- cross-platform builds, release on tag

### Fixed
- Nested block comment parsing
- Semicolons accepted as statement terminators
- Unsigned literal coercion in call arguments
- Variable shadowing scope leak
- Array indexing element type loss
- Function inlining skips generic and Result-returning functions
- Critical codegen safety bugs (bounds checking, null guards)
- All critical and high-severity security issues for public release

### Security
- Checked allocation in C runtime (exits on OOM)
- SECURITY.md with vulnerability reporting policy
- `.gitignore` hardened to exclude build artifacts and sensitive files
- `@unsafe` block enforcement for raw pointer operations
