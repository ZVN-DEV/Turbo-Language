# Changelog

All notable changes to the Turbo compiler are documented here.
Format based on [Keep a Changelog](https://keepachangelog.com/).

## [0.12.0] - 2026-07-07

A feature release that turns Turbo into a language you can build a real service
in: a typed generic `HashMap<K,V>`, SQLite compiled into the binary, and an HTTP
server hardened for life behind a proxy. Together they land the flagship
`examples/http-sqlite-api` — HTTP in, SQLite under, JSON out, one static binary.

### Added
- **Generic `HashMap<K,V>`.** Maps are now typed: keys are `int` or `str`
  (anything else is rejected at every annotation site with `E0525`, not at
  runtime), and values can be any type — including first-class functions, so a
  `HashMap<str, fn(i64) -> i64>` dispatch table works. `hashmap_get` returns
  `V?` and `keys()` returns `[K]`, both correctly typed. Reference-counted
  values are retained on insert and released on overwrite/drop, so a loop that
  overwrites keys 2M times holds flat RSS instead of leaking. Legacy untyped
  hashmaps still work unchanged (benches within noise), and a legacy handle
  passed to a typed boundary is now rejected rather than segfaulting; unsigned
  narrow keys and values round-trip correctly. Survived a GPT-5.5 adversarial
  review round (7 findings fixed before merge). Known limit (documented, tracked
  in #60): aggregate map values release only one level deep. (#61)
- **SQLite, built in.** SQLite 3.47.2 is vendored and statically linked — no
  system dependency, identical behavior across JIT and AOT. `sqlite_open`,
  `sqlite_exec`, and `sqlite_prepare` return a `Result` with real error
  messages, and the bind/step/column/finalize/close surface is exposed for
  prepared-statement work. String returns are ARC-compliant (a raw-malloc heap
  corruption bug was caught and fixed in review). Costs ~3.1MB of binary. Ships
  with a new flagship example, `examples/http-sqlite-api`: an HTTP service that
  reads and writes SQLite and returns JSON, as one binary. (#62)

### Changed
- **HTTP server hardened for behind-a-proxy production.** The server now closes
  slowloris-on-write with a write/send timeout, drains gracefully on SIGTERM and
  SIGINT, and caps keep-alive connections with a request limit and an idle
  timeout. A new `http_config(key, value)` builtin tunes the limits —
  `max_body_bytes`, `max_header_bytes`, `max_connections`, `read_timeout_ms`,
  `write_timeout_ms`, `keepalive_max_requests`, and `idle_timeout_ms`. Oversized
  bodies now get a `413` instead of being read into memory, partial writes are
  handled robustly, and `spawn` deep-copies arena-backed string arguments so a
  spawned handler no longer reads freed request memory (fixes #56). New
  `docs/production-server.md` documents running behind nginx. (#59)

### Known issues
- Owned string temporaries passed as arguments to *user-defined* functions, and
  their `Result` wrappers, still accumulate across a loop (#54).
- A string bound in a block-tail `match` arm can be over-retained (#55).
- Aggregate `HashMap` values release only one level deep on drop (#60).

## [0.11.0] - 2026-07-07

The headline of this release is real string memory management: Turbo strings are
now reference-counted and free during execution instead of only at program exit,
so long-running programs stay memory-bounded. Also lands WASM match support for
enum/Result/Optional, the first package-registry infrastructure, and an honesty
pass over the design docs.

### Added
- **String memory is reference-counted and freed during execution.** Previously
  every string a program allocated lived until the process exited — fine for a
  short compile-and-run, a slow leak for anything long-lived. Strings now carry
  an atomic refcount and are released at the point they're consumed (passed to
  `print`, `len`, `type_of`, or used as a `match` scrutinee), so a loop that
  churns strings no longer grows without bound. The numbers: a 1M-iteration
  string-churn loop went from 252MB to 1.6MB peak RSS; a 2M-iteration
  print-interpolation loop from 124MB to 1.5MB. String-heavy benchmarks got
  slightly *faster* — freeing as you go reduces allocator pressure. String
  literals are immortal (never freed), and the per-request server arenas are
  unchanged (the `RT_RC_ARENA` sentinel opts them out). Verified across two
  adversarial ASan review rounds. The old README caveat that "string memory
  frees only at program exit" is gone because it's no longer true. (#53)
- **WASM backend: `match` on enums, `Result`, and `Optional` in statement
  position.** The WASM target now compiles statement-position matches with
  payload destructuring, guard clauses, and wildcard arms, plus enum
  construction — producing byte-identical output to the native backend
  (confirmed by a host-compile parity test). The codegen crate's panic budget
  was ratcheted 282 → 271 in the process. (#52)
- **Package registry infrastructure.** A curated static index
  (`registry/index.json`, published at `turbolang.dev/registry/index.json`)
  now backs a `/packages` page, a `turbolang search <query>` command, and
  index-aware resolution in `turbolang install`. First step toward a real
  package ecosystem. (#51)
- **First-class function values.** Functions — named or closures — can now be
  stored and called from anywhere: struct fields, array dispatch tables
  (`handlers[i](x)`), immediately-invoked results (`make_adder(3)(4)`), and
  plain bindings (`let g = my_fn`). Named functions and closures share one
  calling ABI, so a field typed `fn(i64) -> i64` accepts either. `@unsafe`
  functions cannot be used as values (new diagnostic `E0530` — the value path
  would have bypassed the unsafe-call check). The WASM backend rejects
  function values loudly rather than emitting invalid C. (#57)

### Changed
- **Design docs honesty pass.** The `design/` documents now clearly mark
  not-yet-implemented features — green threads, actors, the memory ladder, and
  the LLVM backend references — as *Planned*, with status banners, so the spec
  no longer reads as if everything already ships. (#50)

### Known issues
- Owned string temporaries passed as arguments to *user-defined* functions are
  still not freed at the call site (pre-existing behavior; tracked in #54).
- A string bound in a block-tail `match` arm can be over-retained (#55).
- A string that escapes a per-request arena via `spawn` is constrained by the
  arena lifetime (#56).

## [0.10.3] - 2026-07-07

A follow-up patch that hardens the safety machinery introduced in 0.10.x. A
multi-agent review of the hardening release caught real gaps in the new gates
themselves; this closes them. No language or API changes.

### Security
- **Playground sandbox: two allowlist-evasion holes closed.** The hosted
  playground refuses any builtin outside a known-safe allowlist. Two lexical
  tricks could sneak an *unknown* (potentially future-dangerous) builtin call
  past that check: splitting the call across a newline (`socket_open\n(...)` —
  the parser ignores newlines, so it still calls), and shadowing the check with
  a same-named struct field or scalar parameter (`socket_open: int`). Both are
  now rejected; a call is only permitted when the name is genuinely user-defined
  (a function, a `let`/`const` binding, or a function-typed parameter).

### Fixed
- **Differential fuzzer could abort before recording a finding.** Its output
  truncation sliced UTF-8 at an arbitrary byte offset and could panic on
  multi-byte program output — losing the very divergence it had just caught. It
  now truncates on a character boundary, includes captured stderr in finding
  reports, and no longer masks a JIT crash when the AOT build also fails.
- **CI perf-smoke gate could seed a bogus baseline.** The `--update-baseline`
  path wrote a new baseline before the correctness check, so a miscompile could
  bake a meaningless ratio into the gate; it now refuses to update on any output
  mismatch. Build failures and crashing benchmarks are also reported clearly
  instead of aborting the run silently.
- **Docs-vs-compiler parity check tightened.** It now compares against
  error-code *enum declarations* only, so a code left behind in a comment can no
  longer mask a genuinely undocumented one.

## [0.10.2] - 2026-06-29

A trust-and-polish patch from a full public smoke audit. Fixes one real
correctness bug (C-FFI float returns), two CLI rough edges, and a broad
honesty pass across the docs, marketing site, and repository metadata so every
public surface matches what actually ships (Cranelift-only — no LLVM backend,
no agent keywords).

### Fixed
- **C-FFI float returns were typed `int`.** An `@unsafe extern "C"` function
  declared `-> f64`/`-> f32` whose name collided with a native math builtin
  (`floor`/`ceil`/`round`) was silently shadowed by the builtin, so its result
  came back as an `int` — `let x: f64 = floor(3.7)` failed to compile (E0110)
  and `print(floor(3.7))` printed `3` instead of `3.0`. Extern-declared names
  now resolve to their FFI signature in both sema and codegen; plain
  (non-extern) builtin calls are unchanged. Regression test added.
- **`turbo-lsp --version` failed.** It ignored the flag and tried to start an
  LSP session, printing a handshake error. It now prints `turbo-lsp <version>`
  (and `--help`) and exits cleanly.
- **`turbolang bench` made AOT look slower than JIT.** The AOT figure folded in
  the one-time `cc` compile+link. AOT timing is now execution-only and clearly
  labeled, with the build step reported separately.

### Changed
- **Documentation honesty pass.** `docs/SAFETY.md`: the 256-level recursion cap
  is a compile-time codegen-nesting guard (E0516), not a runtime guarantee —
  deep runtime recursion is bounded only by the OS stack; corrected the HTTP
  header cap to a single 16 KB total; dropped the unbacked "measured flat RSS"
  wording. `SECURITY.md`: supported-versions table refreshed to the 0.10.x
  series, removed a phantom "LLVM backend" from experimental features, dropped a
  dead `TODO.md` reference. `examples/README.md`: stopped listing shipped
  features (`?.`, result types, WASM) as unimplemented. `README.md`: tagline
  now "JavaScript's soul. Rust's speed." (matches the site); corrected the AOT
  binary-size figure (~113 KB), `pow`'s integer-only contract, and the
  `linux-arm64` status. `COMPATIBILITY.md` realigned to 0.10.x. `docs/stdlib.md`
  documents `hashmap_inc` and `http_post_with_headers` and is de-pinned from
  0.8.0.
- **Public metadata corrected.** GitHub description and turbolang.dev no longer
  advertise an LLVM backend or agent primitives that don't ship; the marketing
  site was redeployed; `turbolang.dev/errors/E0NNN` now resolves to the
  canonical error doc; VS Code publisher normalized to `zvndev`; the Homebrew
  tap README's repo link fixed.

## [0.10.1] - 2026-06-29

A correctness and polish patch. Closes two silent server-runtime bugs (a
use-after-free and a data race), bounds long-running JIT server memory, turns a
runtime segfault into a clean compile error, and clears the remaining diagnostics
and language-ergonomics backlog. All changes are JIT/AOT parity-tested; CI now
also gates the C-runtime tests and sanitizers on every change.

### Fixed
- **Stateful HTTP server use-after-free (critical).** A server holding state in a
  hashmap created at startup and mutated inside a request handler had its entries
  freed by the per-request memory arena, dangling on the next request. Persistent
  hashmaps now allocate their entry storage with `malloc`/`free` and survive the
  request boundary (request-local maps stay arena-backed — no leak). Confirmed
  clean under AddressSanitizer.
- **JIT HTTP server unbounded memory growth.** `turbolang run` servers never
  reclaimed per-request string memory (the arena only drained when `main()`
  returned, which a server never does), so RSS grew without bound. The request
  handler now resets the arena to a per-request high-water mark — RSS stays flat
  over thousands of requests. AOT servers were already bounded; the previous
  README/SAFETY note warned about the wrong path and has been corrected.
- **JIT hashmap data race under concurrent `spawn`.** Two threads mutating a
  shared hashmap raced (undefined behavior / crashes). The JIT hashmap is now
  mutex-guarded; an 8-thread × 50k-increment stress test went from crashing every
  run to deterministically correct.
- **Opaque-handle type confusion → segfault.** Assigning an integer into a
  hashmap/mutex/HTTP-handle-typed variable (e.g. `m = hashmap_inc(m, k)`, which
  returns an `int`) silently clobbered the handle and segfaulted on the next use.
  Handles now have a distinct type, so this is a clean compile-time error
  (`E0111`) instead of a runtime crash.
- **Whole-number floats print ambiguously.** A `float` with no fractional part
  printed without a decimal point (`2.0` → `2`), indistinguishable from an `int`.
  Whole-valued floats now render with a trailing `.0` consistently across JIT,
  AOT, and WASM.
- **Misleading `let x =` diagnostic.** An empty right-hand side now produces one
  clear error anchored on the `=` ("expected an expression after `=` in let
  binding") instead of a message pointing at the next token or end of file.

### Added
- **Raw string literals** `r"..."` — backslashes and braces are taken literally,
  so JSON, regexes, and Windows paths paste in verbatim.
- **Empty-array element-type inference.** A bare `[]` now infers its element type
  from an annotated `let`, parameter, struct field, or return type; genuinely
  uninferrable cases still give a single clear diagnostic.
- **`examples/stateful-counter/`** — a correct persistent-state HTTP server demo.

### Changed
- Decomposed the two largest internal functions (semantic analyzer's
  `check_expr_inner`, code generator's `compile_call` fallback) into focused
  handlers — no behavior change.
- Diagnostics/CLI polish: cleaner IO error messages, a TTY-gated `test` summary
  with total time, and no spurious REPL "unused variable" warnings.

## [0.10.0] - 2026-06-28

A large correctness, language-features, and DX release driven by a full product
review. Closes two silent data-corruption classes, lands several real language
features, measurably improves runtime performance (honestly re-benchmarked), and
sweeps the diagnostics, docs, and website. All changes are JIT/AOT parity-tested.

### Fixed
- **Copy-on-write value semantics (critical).** Structs and arrays no longer
  alias their source's heap storage when copied — `let b = a; b.x = 99` (and the
  same for passing a value to a `mut` parameter or extracting an element from a
  parent collection) now correctly leaves the original unchanged, on both JIT and
  AOT. Previously this silently corrupted data, contradicting the documented CoW
  guarantee.
- **AOT HTTP runtime (production path).** Plain-string handler responses
  containing a colon no longer emit a malformed `HTTP/1.1 0 OK` with a truncated
  body, and request bodies larger than ~16 KB are no longer rejected with a
  spurious `431` — the AOT server now matches the JIT server (bodies up to 32 MB).
- **Concurrency.** Added `mutex_update(m, closure)` (an atomic read-modify-write
  critical section); a shared counter under contention is now exact instead of
  losing ~57% of updates. The docs no longer imply bare `mutex_get`/`mutex_set`
  are sufficient for concurrent mutation.
- **f32 ABI.** `f32` values now round-trip correctly across spawn / generic /
  closure boundaries (previously a Cranelift panic or miscompile); the 0.9.2 sema
  reject is removed.
- **`bench` command.** No longer reports `0/N passed` for a valid benchmark —
  `@bench` is now a recognized attribute on every backend; results lead with the
  timing and report AOT/JIT parity separately.
- **Diagnostics.** Runtime and operational errors now carry error codes
  (`E0601`–`E0611`), color, and `Help:` lines; arity / missing-variant / unknown-
  field / type-mismatch errors echo the real signature, variant, field list, and
  the user's source type spelling; raw `(os error N)` jargon dropped.

### Added
- **`as` cast operator + sized-integer usability.** Numeric `as` casts
  (width/sign/float conversions) plus integer-literal coercion into annotated
  sized types in array literals, struct-field init, and arithmetic — `i8`..`u64`
  are no longer an unusable island.
- **Compound-value printing.** `print`/`to_str`/interpolation of arrays, structs,
  and results now render their contents recursively (`[1, 2, 3]`, `P { x: 9 }`,
  `ok(7)`) instead of opaque `[array]`/`[struct]`/`[result]` placeholders.
- **CLI arguments.** `args()` returns the program's real command-line arguments
  (`turbolang run f.tb -- a b c` and built binaries alike); previously a stub.
- **WASM closures.** Closures (direct, capturing, and in `map`/`filter`) now
  compile and run under `wasmtime`, parity-matched to native; unsupported closure
  shapes fail loud instead of miscompiling.
- **`hashmap_inc`** fused increment builtin.

### Changed
- **Performance — word-count ~2.2x → ~1.4x slower than C.** The str→int hashmap
  path stored integers as strings and re-hashed/re-allocated on every increment;
  it now uses inline-int entry storage (single hash, zero alloc). Re-measured with
  `run_wordcount.sh` and published consistently across README/website/benchmarks.
- **Formatter.** Replaced the line-based tidier with a real AST pretty-printer
  (canonical spacing, idempotent, semantics-preserving with a byte-for-byte
  fallback).
- **Docs & website.** Corrected `explain`/SYNTAX.md examples that didn't compile,
  documented 19 missing stdlib builtins, softened the inaccurate "104 built-in
  functions" claim to "100+", and added per-route SEO metadata, a sitemap/robots/
  OG card, copy buttons, and accessibility fixes to the site.

## [0.9.2] - 2026-06-28

A correctness, hardening, and honesty pass. Fixes another round of codegen and
runtime bugs, tightens the WASM and HTTP paths, and aligns the public-facing
docs, website, and benchmarks with what actually ships.

### Fixed
- **Codegen — unit values.** Fixed compiler panics when unit (`()`) values flow
  through expression positions.
- **Codegen — float ABI.** Corrected a float-ABI miscompile in the calling
  convention.
- **WASM runtime.** Added overflow guards to the WASM runtime to stop integer
  overflow from corrupting execution.
- **Playground.** Fixed error rendering so compile/runtime diagnostics display
  correctly instead of being swallowed.

### Changed
- **Honest benchmarks.** Replaced the fabricated published performance numbers
  with real `fib(40)` measurements (Turbo runs ~1.25–1.3x slower than C/Rust on
  this microbenchmark, not faster), clearly labeled with machine, date, and
  methodology, across the README and website. Repaired the corrupted benchmark
  results dataset and added a Turbo row.
- **Corrected marketing claims.** Removed unsupported "AI age" / "competitive on
  real workloads" / "edges out cc -O2" framing; reconciled the async/concurrency
  story to honest OS-thread `spawn`/`await` (no event loop); fixed broken website
  links, the documented install pipe (`| bash`), and the unshipped
  `--target linux-arm64` claim.

### Added
- **WASM execution test** validating compiled WASM output actually runs.
- **SSRF host blocking** for the HTTP client to reject requests to disallowed
  hosts.

## [0.9.1] - 2026-06-08 — Hardening, Security & Correctness

A correctness-and-safety release. A full hardening audit fixed every known
miscompilation and memory-safety bug across codegen, the C runtime, and the
frontend; earlier sprints hardened the runtime against malformed input and
closed remaining stdlib gaps. Validated AddressSanitizer-clean across 200
programs, with 222 integration tests and the full unit suite passing.

### Fixed
- **Codegen — control flow & SSA.** Corrected block/merge parameter handling for
  diverging `else` branches in `if` and `if let`, added missing float bitcasts in
  `some(v)` and `??`, fixed the closure-return and `exit` calling conventions, and
  stopped a crash on a value-returning builtin in an else-less `if`. Dead code
  after a diverging call no longer crashes compilation.
- **Codegen — memory safety.** Stopped double-freeing struct-array elements across
  `for` loops.
- **C runtime.** Closed a use-after-free in copy-on-write `push`, fixed string-alias
  corruption in in-place concat, and fixed a `substring` abort on multi-byte
  (UTF-8) input. Earlier hardening: depth-aware `json_get`, JSON escape handling,
  padding validation, HTTP parsing bounds, mutex refcounting, and `list_dir`
  allocation checks.
- **Generics.** Generic function bodies are now type-checked, and `[T]` indexing no
  longer segfaults.
- **Frontend.** Deeply-nested types no longer overflow the stack (now a clean
  `E0516`); duplicate struct fields and duplicate function parameters are rejected;
  parser soft-keyword double-peek fixed.
- **DX.** Clearer range-bounds and integer-overflow diagnostics, a safer formatter,
  corrected LSP error codes, and more robust CLI/`init` error handling.

### Added
- New builtins and stdlib gap-closing: `http_post_with_headers`, `json_build`,
  `str_from_char`, unsigned `min`/`max`, saturating `float_to_int`, `mutex_drop`,
  and float support for `abs`/`min`/`max` in sema.
- Performance: amortized O(1) array `push`, `select` optimization, and `@unsafe`
  array access.
- VS Code: wired-up LSP client and editor snippets.

### Removed
- **The experimental LLVM backend (`turbo-codegen-llvm`, `turbolang build --llvm`).**
  Cranelift is now the single code-generation backend. The LLVM path was a
  partial second implementation (roughly half the builtins), did not build
  cleanly from the documented steps, and offered no measured speedup over
  Cranelift on the fib benchmark. Removing it
  eliminates a large duplicated codegen surface and the cost of keeping two
  backends at parity. A future optimizing backend, if pursued, should lower
  through a shared mid-level IR rather than re-walking the AST a second time.
  (Work continues on the `llvm-backend` branch; it will be reintroduced only when
  it reaches parity, builds cleanly, and shows a measured speedup.)

## [0.9.0] - 2026-05-16 — Batteries-Included Stdlib

Major stdlib expansion: 37 new builtins bringing TurboLang from 67 to 104 built-in functions. Inspired by the batteries-included philosophy — every dependency is liability, the stdlib must be complete enough that developers never need a package manager.

### Added — System Essentials
- `exit(code)` — exit process with status code
- `type_of(val)` — get type name as string (compiler intrinsic)
- `args()` — CLI argument array

### Added — Math (12 functions)
- `floor(x)`, `ceil(x)`, `round(x)` — float rounding to i64
- `sin(x)`, `cos(x)`, `tan(x)` — trigonometric functions
- `log(x)`, `log2(x)`, `log10(x)` — logarithms
- `exp(x)` — exponential (e^x)
- `random()` — random f64 in [0.0, 1.0)
- `random_range(min, max)` — random i64 in [min, max]

### Added — String Parsing
- `substring(s, start, end)` — extract substring by index
- `pad_left(s, width, char)`, `pad_right(s, width, char)` — string padding
- `str_to_int(s)`, `str_to_float(s)` — parse strings to numbers (returns Result)

### Added — Filesystem (8 functions)
- `file_exists(path)` — check if file exists
- `delete_file(path)` — delete a file
- `list_dir(path)` — list directory contents as string array
- `mkdir(path)` — create directory recursively
- `path_join(a, b)` — join path segments
- `path_dir(path)`, `path_base(path)`, `path_ext(path)` — path component extraction

### Added — Collections (5 functions)
- `sort(arr)` — sort array (COW, returns new array)
- `reverse(arr)` — reverse array (COW)
- `array_contains(arr, val)` — check if value in array
- `slice(arr, start, end)` — sub-array extraction

### Added — Date/Time (3 functions)
- `time_now()` — current Unix timestamp in seconds (f64)
- `time_ms()` — current Unix timestamp in milliseconds (i64)
- `format_time(timestamp, format)` — format timestamp to string

### Added — Examples
- `examples/file-analyzer/` — 211-line CLI tool showcasing filesystem, string parsing, and struct features; compiles to 95KB AOT binary

### Internal
- Decomposed codegen `lib.rs` (4,389 lines) into focused modules: `type_conv.rs`, `closures.rs`, `compile.rs`, `tests_codegen.rs`
- Fixed f64 Result extraction bitcast bug in Cranelift codegen
- Added `STDLIB-ROADMAP.md` tracking 74 planned builtins across 3 tiers

## [0.8.2] - 2026-05-10 — Product Hardening Sprint

Follow-up hardening release closing product-review and gold-standard OSS findings around compiler parity, dependency installation safety, HTTP response defaults, CI coverage, and website dependency hygiene.

### Security
- **Dependency install/update path traversal blocked.** Manifest dependency names now reject empty names, `..`, path separators, absolute paths, and Windows path prefixes before writing under `turbo_modules`.
- **Symlinked `turbo_modules` rejected.** Install/update now resolve the modules directory against the project root and refuse symlinked `turbo_modules`, closing a canonicalization bypass.
- **Wildcard CORS removed from typed HTTP responses.** Rust and C runtimes no longer add `Access-Control-Allow-Origin: *` by default.
- **Next.js upgraded to 16.2.6** and website CI now gates high-severity `npm audit` findings.

### Fixed
- **AOT/JIT array parity restored.** Fixed RC heap assignment aliasing so in-place array mutations such as `push` preserve the updated pointer/length in AOT output.
- **Exclusive AOT/LLVM temp directories.** Build temp directories now use exclusive creation instead of predictable shared paths.
- **C runtime CORS regression test hardened** with loopback ephemeral-port selection and full response reads.

### CI / Docs
- PR CI now runs website lint/build, high-severity npm audit, and parity tests.
- Updated stale v1.0 demo copy to the current 0.8.x status.
- Updated `SECURITY.md` HTTP request-limit text and README test-status wording.
- Captured sprint artifacts in `AUDIT-REPORT.md` and `SPRINT-PLAN.md`.

## [0.8.1] - 2026-05-09 — Hardening Sprint

Hardening sprint addressing all P0/P1 findings from a comprehensive product review. Fixes systemic memory safety bugs in both runtimes, hardens the HTTP server, documents the security model honestly, and adds a getting-started tutorial.

### Security
- **Arena/malloc mismatch eliminated.** `turbo_free()` now checks whether a pointer is arena-backed before calling `free()`, preventing heap corruption in HTTP handlers using JSON or hashmaps. `turbo_strdup()` routes all string duplication through the arena allocator. 22 call sites across the C runtime were audited and corrected.
- **Integer overflow checks added** to `turbo_calloc()` and `rt_struct_alloc()` using `__builtin_mul_overflow`, matching the pattern from `rt_pow`.
- **HTTP response header injection blocked.** Both C and Rust runtimes now strip `\r\n` from content-type before interpolation into response headers.
- **HTTP request size limits enforced.** 8 KB per header line, 64 KB total headers, 32 MB request body, 256 max concurrent connections. Exceeding limits returns HTTP 431/413.
- **JIT memory leak fixed.** `rt_release` now tracks allocations via `ALLOC_REGISTRY` and calls `dealloc` when refcount reaches zero, instead of silently leaking.
- **Cranelift IR verifier** enabled in debug builds (`cfg!(debug_assertions)`), catching codegen bugs before they produce miscompiled code.
- **Sema panic eliminated.** Replaced `.unwrap()` on function lookups with safe fallbacks, preventing compiler crashes on edge-case AST states.

### Added
- **`docs/SAFETY.md`** (328 lines) — Safety narrative explaining what errors are impossible, caught at compile time, caught at runtime, and the programmer's responsibility. Includes comparison tables vs C, Go, and Rust.
- **`docs/GETTING-STARTED.md`** (538 lines) — 10-section tutorial from installation through building a text-stats analyzer project.
- **`SECURITY.md`** updated with 7-section security model: JIT execution, playground, HTTP server, AOT binaries, FFI, file I/O, shell execution.
- 5 new C runtime regression tests (arena-aware free, strdup routing, overflow detection, header injection).
- 6 new Rust runtime/sema unit tests (deallocation, unwrap safety).
- 14 new CLI/formatter unit tests.
- 7 new doc-tests across turbo-ast and turbo-parser.
- 4 new integration tests: `cow_method_chain`, `result_chain`, `nested_match`, `optional_coalesce`.

### Changed
- `rt_str_char_at` now returns a clear error message for negative indices instead of wrapping to `usize::MAX`.
- `build.rs` uses runtime `CARGO_MANIFEST_DIR` instead of compile-time macro, fixing silent error-code exhaustiveness check skip on non-standard paths.
- `README.md` updated: test counts, version references, security model section, stdlib table, fixed dead links.
- `CONTRIBUTING.md` fixed: correct test commands, updated file paths, added security reporting section.
- `docs/stdlib.md` updated with `try_read_file`/`try_write_file`, `hashmap_set_int`/`hashmap_get_int`, `exec`, `env_get`, `http_server_public`.

### Notes
- Sprint tracked in `SPRINT-PLAN.md` with 26 tasks across 4 parallel dev tracks. 5-agent review cycle (PM, CEO, Security, Code Quality, UX/DX) validated all changes before ship.
- Known limitation: `ALLOC_REGISTRY` is thread-local; cross-thread refcounted objects leak silently rather than causing UB. Tracked for future improvement.
- Codegen `lib.rs` split (4,327 lines) deferred to a future sprint.

---

## [0.8.0] - 2026-04-14 — The Safe Core

Safety-focused release. Four parallel tracks close the highest-severity findings from the 0.7.7 product review: rt_exec shell injection, unchecked integer arithmetic in pow, silently-swallowed I/O errors on `read_file`, generic hashmaps limited to string→string, a realloc corruption window in `read_fd_to_string`, and unbounded linker flags on AOT builds.

### Security
- **rt_exec no longer shells out.** Both the AOT C runtime (`turbo_rt.c`) and the JIT twin (`crates/turbo-codegen-cranelift/src/runtime.rs`) now reject any command containing shell metacharacters (`; | & $ \` ( ) < > \n \\`), tokenize on whitespace, and `execvp` the argv directly with a 64-argument cap. Previously `shell_exec("echo hi; echo pwned")` happily ran both halves; now it is refused with a runtime error. The JIT divergence was caught by code-quality review and fixed before ship — no `Command::new("sh").arg("-c", ...)` remains on either path. New adversarial test: `turbo/tests/adversarial/exec_injection.tb`.
- **AOT linker flags are now allowlisted.** `aot.rs` validates every `-l<lib>` name against `[A-Za-z0-9_.+-]+` before invoking `cc`, returning `error[E0404]` on rejection. Closes a vector where a malicious dependency could smuggle `-Wl,...` arguments through the build. Unit tests cover the allowlist edges.

### Changed
- **`rt_pow` is now overflow-checked.** The C runtime uses `__builtin_mul_overflow`; the JIT runtime uses `i64::checked_mul`. Both exit with `runtime error: integer overflow in pow` instead of silently wrapping. Negative exponents now abort with a clear error rather than returning 1.
- **`read_fd_to_string` no longer corrupts memory on large reads.** The read loop grows a libc `malloc`/`realloc` buffer, then performs a single `turbo_alloc` + `memcpy` at the end, instead of reallocating arena-owned memory mid-read (which was a latent use-after-realloc window on files larger than the initial chunk).

### Added
- **`try_read_file` / `try_write_file`** return `str ! str` and integrate with `match ok/err`, replacing the old behavior of `read_file` swallowing I/O errors and returning an empty string. `read_file` still exists for the common happy-path case. See `docs/stdlib.md` § Fallible I/O. Tests: `turbo/tests/phase1/try_read_file.tb`, `try_read_file_ok.tb`.
- **`hashmap_set_int` / `hashmap_get_int`** provide a str→int hashmap variant — the first step toward generic hashmaps. Implemented as a stringify-and-reuse wrapper on top of `rt_hashmap_set` / `rt_hashmap_get`, so existing str→str usage is unchanged. Test: `turbo/tests/phase1/hashmap_int.tb`.

### Fixed
- `turbo/tests/phase1/exec_env_get.tb` dropped its `2>&1` suffix (rt_exec already merges stderr into the returned string, and `&` is now a rejected metachar).

### Notes
- Adversarial suite now exercises shell-injection payloads end-to-end. Previously the test only checked that `shell_exec` returned at all; it now asserts that the post-`;` payload did **not** execute.
- This release is the "Safe Core" milestone called out in the 0.7.7 product review's Path Forward. Remaining deferred items (full parametric generics for hashmap, bigint pow variant, UTF-8 normalization in `str` ordering) are tracked for 0.9.x.

---

## [0.7.7] - 2026-04-14

### Added
- LLVM backend now supports struct destructuring in `let` patterns, `if let` binding, and optional chaining (`x?.y`). Previously these emitted a "not yet implemented" error. The LLVM CI job is now a hard-required check.
- Added COMPATIBILITY.md documenting the 1.0 stability contract and what remains fluid in 0.7.x and 0.8.x.

### Security
- Install script now pins the GPG release key against a local copy in-repo; mismatched remote key aborts the install.
- Playground: token generation now uses 128 bits of OS randomness via `getrandom` instead of PID+timestamp, closing a localhost CSRF foot-gun.
- Playground: source files are written via `tempfile::NamedTempFile` (exclusive-create with a random suffix), closing a TOCTOU race on the previously predictable temp filename.
- Runtime: replace unbounded strcat in rt_str_join with length-tracked memcpy to eliminate a latent heap-overflow foot-gun and an accidental O(n²) append loop.
- Runtime: guard rt_array_push against size_t overflow on adversarial lengths.
- Parser and codegen now enforce a 256-level recursion depth limit (E0516), so adversarial deeply-nested input errors out with a diagnostic instead of overflowing the compiler's stack.

### Changed
- Documentation: removed `agent`, `tool fn`, and "first AI-native language" claims from core-facing copy. Agentic features will ship in a separate `turbo-agent` sidecar library after the core language hits 1.0 stability, not inside the compiler.
- Runtime: rt_array_push now doubles capacity on growth, giving amortized O(1) pushes instead of O(n) per call. The shared refcount allocation header grew from 8 bytes to 16 bytes to carry the capacity slot; callers continue to see the same data pointer offsets.
- Runtime: COW refcount reads in rt_array_push and rt_array_set now use `__atomic_load_n(..., __ATOMIC_ACQUIRE)` to match the rest of the ARC surface (`__sync_fetch_and_{add,sub}` retain/release).
- Docs: VISION.md and ROADMAP.md v1.2+ sketches now explicitly note that GPU, mobile-UI, and distributed-actor features will ship as sidecar libraries -- not compiler keywords -- per the 2026-04-09 "sidecar, not syntax" decision.
- README: dropped the stale `E0001--E0521` range claim; error-code coverage is communicated without a (sparse, misleading) numeric range.
- CI: fuzz-smoke now runs on every push (cap 60s/target) instead of nightly-only.
- CI: new ASAN+UBSAN job rebuilds the C runtime tests with clang sanitizers.

### Fixed
- LSP: malformed `textDocument/rename`, `codeAction`, and `hover` requests now return proper JSON-RPC error responses instead of crashing the server.
- WASM codegen: `defer` statements are now lowered correctly (LIFO at scope exit). Previously they were silently dropped, producing wrong semantics when targeting WASM.

### Notes
- Error-code docs audit: `ErrorCode` variants, `turbo-cli/src/errors/` entries, and `docs/errors/` entries are all in sync at 88/88/88 (after Stream E added E0516). The previously flagged "94 variants vs 87 docs" gap does not exist.

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
