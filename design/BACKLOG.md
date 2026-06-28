# TurboLang — Engineering Backlog (autonomous-agent queue)

This is the **work queue for autonomous agents** (Codex loop / Claude). It exists so automation
works on real, verifiable correctness and feature work instead of converging on packaging/hygiene
busywork. It was seeded from the 2026-06-28 product review + 0.9.2 hardening sprint.

## Rules for autonomous runs
1. **Pull the highest-priority unchecked item** (`[ ]`) whose status is not `IN PROGRESS`. Work ONE item per branch.
2. **Definition of done (every item):** the fix/feature is implemented, **new tests are added that would fail without the change**, and the full suite is green — `cargo test --workspace --manifest-path turbo/Cargo.toml`, `cd turbo && ./tests/run_tests.sh`, `turbo/tests/parity/run_parity.sh`, and `cargo clippy ... -D warnings` + `cargo fmt --check`. Update this file: check the box and note the commit.
3. **No hygiene busywork.** Do NOT open PRs whose substance is "keep X reproducible", "guard Y", "prevent drift", README/metadata polish, dependency bumps, or re-formatting — unless an item below explicitly calls for it. If you cannot make real progress on a backlog item, **stop and leave a note here** rather than inventing hygiene work.
4. **Honesty bar:** never add a performance/benchmark/marketing claim that isn't backed by a committed, reproducible measurement (see the 0.9.2 benchmark correction). Turbo AOT is ~1.25–1.3× slower than C/Rust on fib40 — keep claims consistent with that.
5. Keep changes surgical and matched to existing style. One concern per branch; respect the file-ownership discipline if multiple agents run in parallel.

---

## P1 — correctness & credibility

- [x] **BL-1 — Finish retiring the `?.unwrap()` panic class in codegen.** _Status: DONE (Claude, 2026-06-28, commit `47bef1ce68adbc2be4640fc5ad8f012c06e81a6e`)._
  The 0.9.2 sprint fixed the 2 reachable panics but ~152 `compile_expr(...)?.unwrap()` remain in
  `turbo/crates/turbo-codegen-cranelift/src/builtins.rs` (+5 in `src/expr.rs`: ~1595/1620/1646/1851/1890).
  All are sema-guarded today, but a future sema gap panics the compiler (exit 101) instead of diagnosing.
  Convert them to `?.ok_or_else(|| CodegenError { code: <fitting ErrorCode, e.g. E0400/E0403>, message })?`.
  **AC:** no `?.unwrap()` on a `compile_expr` result remains in those two files; add a fuzz/regression
  pass (extend `turbo/fuzz/` or add adversarial `.tb`s) that throws malformed-but-parseable input at
  codegen and asserts a clean diagnostic, never a panic.
  **Done:** converted all 152 sites in `builtins.rs` and all 5 in `expr.rs` to
  `?.ok_or_else(|| CodegenError { code: ErrorCode::E0400, message: "<fn>: `<arg>` produced no value during
  code generation" })?` (context-specific messages). `grep -c '?.unwrap()'` is now `0` in both files.
  Coverage: 15 in-crate regression tests (`builtins.rs` `mod panic_class_regression`, run under
  `cargo test`) that drive the backend with sema bypassed and assert a graceful `E0400` for every
  expr.rs site + representative builtins.rs sites; a 55-program deterministic sema-bypass corpus in
  `turbo/fuzz/src/codegen_fuzz.rs` (`run_robustness_corpus`, runs in CI); and 3 adversarial `.tb`s
  (`tests/adversarial/bl1_unit_in_*`) asserting a clean non-101 diagnostic end-to-end. Residual: 2
  non-`compile_expr` `.unwrap()`s in `builtins.rs` (`lookup_variant_tag_static(...).unwrap()`, lines
  ~3351/3422) were intentionally left — they are infallible by construction (the variant tag was just
  resolved), not unwraps of a compiled subexpression.

- [x] **BL-2 — Add a real-world benchmark (CEO's top "next").** _Done (commit 79129ef, merged to master)._
  Added a word-count workload (`turbo/benchmarks/wordcount.tb` + C/Rust/Go baselines + deterministic input
  generator + `run_wordcount.sh` runner with warmup, best-of-5, and byte-for-byte output-equality
  enforcement). Measured on M5 Max: Turbo AOT ~240 ms vs C ~110 / Rust ~125 / Go ~130 — **~2.2x slower than
  C** (further behind than fib40's ~1.3x). Published honestly in README + website with a reproduce command.
  Root-caused the gap to runtime hashmap/string handling → see BL-9.

## P2 — language & tooling depth

- [x] **BL-3 — f32 across the spawn/generic/closure ABI boundary.** _Done (commit `7e4a6d5`, merged to
  master)._ Root cause was a single ABI disagreement: `resolve_cl_type` mapped `f32 → F32` for internal
  function signatures while every float value flows as F64 (f32 locals/arithmetic/returns already
  round-trip as f64), so the param var was declared F32 but fed an F64 value → Cranelift panic / closure +
  spawn miscompiles. Fix: promote f32 → F64 uniformly across the **internal** Turbo ABI (`type_conv.rs`),
  and preserve a real 32-bit `F32` only at the **C FFI** (`extern`) boundary via a new `resolve_cl_type_ffi`
  threaded through `compile.rs`. Removed the 0.9.2 sema rejects (`reject_f32_params` + closure-param
  reject). New tests: 4 JIT↔AOT parity programs (`f32_abi_{param,closure,spawn,generic}`) + 5 phase1
  value-assertion tests; removed the obsolete `f32_{spawn,closure}_reject` ERROR tests. Verified: 503 unit
  / 238 integration / 18 parity, clippy + fmt clean. No new semantic loss (the language never promised
  32-bit rounding for f32).

- [x] **BL-4 — LSP scope/binding resolution.** _Done (commit 4ff0476, merged to master)._ New
  `turbo-lsp/src/resolve.rs` walks the AST with a lexical scope stack mapping each ident occurrence (by
  span) to its declaration, respecting shadowing; rewired go-to-def, references, rename, hover, and a new
  document-highlight to be scope-precise (shadowed inner `x` no longer renamed with an outer `x`).
  Correct-over-complete: field-access-to-struct-field resolution still falls back (needs type inference).
  LSP tests 50 → 70. **Follow-on (future):** field/method resolution once a span→type map exists.

- [x] **BL-5 — AST-based formatter.** _Done (commit `677d243`, merged to master)._ Replaced the line-based
  tidier with a real lex→parse→print pretty-printer (canonical operator/`:`/`->` spacing, block expansion,
  4-space indent, trailing commas). Safety-gated: re-parses its own output and compares ASTs (behaviour-
  equality) + a comment multiset check; on any mismatch / unparseable input / block comments it returns the
  source **byte-for-byte**. Idempotent + semantics-preserving across all 204 phase1 + regression +
  adversarial programs; `init` scaffold passes `fmt --check`. turbo-formatter tests 8 → 36.

- [ ] **BL-6 — Split the two god-functions.** `turbo-sema/src/type_check/expr.rs::check_expr_inner`
  (~4,540 lines) and the `compile_call` `_` fallback arm in `turbo-codegen-cranelift/src/expr.rs`
  (~311 lines, six concerns). Decompose into per-`Expr`-variant handlers with no behavior change.
  **AC:** full suite stays green; no function over ~400 lines remains in those two hotspots.

## P3 — coverage & ops

- [ ] **BL-7 — WASM backend feature gaps.** The WASM C-transpiler (`src/wasm_codegen.rs`) now fails loud on
  closures/async/some match patterns instead of emitting `0`. Implement real support (start with closures),
  and expand `turbo/tests/wasm/` execution coverage + add WASM↔native parity. **AC:** a closure program
  compiles and runs correctly under `wasmtime`, parity-tested.

- [ ] **BL-8 — Homebrew formula sha sync.** The in-repo `distribution/homebrew/turbo-lang.rb` ships
  placeholder `000…` sha256 (real ones go to the `ZVN-DEV/homebrew-turbo` tap via `release.yml`). Either
  teach the release pipeline to write the real checksums back into the in-repo mirror, or add a post-release
  check that the mirror matches the published artifacts. **AC:** the in-repo formula is never stale/fake
  after a release, enforced by a check.

- [ ] **BL-9 — Runtime hashmap/string-key performance.** _Surfaced by BL-2._ The word-count benchmark is
  ~2.2x slower than C, root-caused to the str→int hashmap path re-stringifying the key on every increment
  (and broader runtime string/hashmap handling). Profile the hashmap increment path (`turbo_rt.c`
  `rt_hashmap_*` + the codegen lowering in `builtins.rs`) and cut the redundant allocation/hashing per
  update (e.g. single lookup-or-insert, avoid re-stringify). **AC:** word-count Turbo-AOT/C ratio improves
  measurably (target ≤1.5x) with the BL-2 runner still output-equal; add a microbenchmark for hashmap
  increment throughput. Honest numbers only.

---

## Product-Team Cycle 2 (2026-06-28) — new findings

Seeded from a full `/product-team` cycle (strategist + e2e + frontend + backend + product-designer),
scoped to NEW/forward-looking issues (excludes the already-fixed 0.9.2 items and BL-1..BL-9). Each item
notes its source lane. Same rules apply (real tests, full green suite, no hygiene busywork, honest claims).

### P1 — correctness, trust, and the wedge

- [x] **BL-10 — Struct field-assignment aliases storage (no COW) → silent data corruption.** _[backend]
  Done (commit `40ad9ef`, merged to master)._ Structs already carried the same `[cap][refcount]` header as
  arrays but never consulted it on write. New `rt_struct_cow` (in both `runtime.rs` JIT and `turbo_rt.c` AOT)
  mirrors `rt_array_set`'s `refcount>1 → private copy → drop shared ref` dance; `FieldAssign` calls it before
  the store and repoints the binding, with retains added at the `mut`-param and array-element binding sites so
  the refcount is honest. Restores the `docs/SAFETY.md` CoW value-semantics guarantee for structs. +3 phase1
  tests + 1 JIT/AOT parity test (`struct_value_semantics`); 241 integration / 19 parity green. _Surfaced
  BL-27._

- [x] **BL-11 — Printing a compound value yields an opaque placeholder.** _[e2e]_ Done (branch
  `fix/bl11-compound-printing`). `print(arr)` / interpolating an array → `[array]`; structs → `[struct P]`;
  results → `[result]` (only scalars + Optionals rendered). The official Collections tutorial visibly emitted
  `[array]`. Biggest day-1 debugging wall. **Fix:** extended the codegen-side recursive renderer
  (`convert_to_str` in `turbo-codegen-cranelift/src/builtins.rs`, the shared `to_str`/interpolation path) to
  walk arrays (`[1, 2, 3]`, `[]`, nested), structs (`Point { x: 9, y: 2 }` — distinct from `to_json`'s
  `{"x":9}`), and results (`ok(7)` / `err(reason)`), mirroring the existing Optional `some(x)`/`none`
  mechanism; `print()` now delegates all compound types to it. Strings render unquoted to match
  `some("hi")` → `some(hi)`. Also fixed a latent bug where float/bool optional payloads rendered garbage.
  +4 phase1 tests + 1 JIT/AOT parity test (`compound_printing`); 245 integration / 20 parity green.

- [x] **BL-12 — Docs lie about syntax (`explain` examples + SYNTAX.md `[Implemented]`).** _[designer#1, e2e]
  Done (commit `57bab40`, merged to master)._ Corrected the `enum`/`::` snippets in `explain` docs
  E0200/E0303/E0307/E0316 to `type`/`.` syntax (each compile-verified via `turbolang run`); relabeled five
  `design/SYNTAX.md` `[Implemented]` section headers to `[Partial]` with per-construct `[Planned]` tags
  (array/slice patterns, default params, named args, inferred-arrow, array/nested destructuring, tuples,
  type aliases, turbofish, `@derive(Debug/Serialize)` — each status verified against the release binary),
  plus a legend pinning tags to "works on the current release binary." `docs/errors.md` had no code
  snippets (no change). _Pre-existing quirk noted: a `Color.Red` to an undefined enum routes to E0300, not
  E0303 — left out of scope._

- [x] **BL-13 — Runtime/operational errors are second-class.** _[designer#2,#3] DONE._ **BL-13a** (commit
  `57bab40`): renderer no longer glues `more info:` onto an empty `Help:` label. **BL-13b** (commit `64991cc`,
  merged to master): added an `E0601-E0611` runtime/operational code range (fully documented; `explain`
  works). Runtime traps (div-by-zero E0601, index OOB E0602, overflow E0603) print a styled `runtime
  error[E06xx]:` + `Help:` + `more info:` footer, **byte-identical on JIT and AOT** (twin helpers, TTY-gated
  color). Operational errors get the full colored `error[E06xx]:` envelope — file-not-found E0611 + import
  E0610 drop the raw `(os error N)` jargon. _WASM runtime traps left naked (separate backend) — minor follow-up._

- [ ] **BL-14 — No in-browser "Try it"; playground gated behind install.** _[designer#4, strategist theme 2]_
  `turbolang playground` serves a working browser playground — but only after a CLI install. For a *language*,
  "run code in 5s" is the highest-converting action. **AC:** host the existing playground at `turbolang.dev/play`
  and add it as the hero's primary secondary-CTA ("Try in browser →") + a persistent nav item.

- [x] **BL-15 — Website SEO: duplicate metadata, no sitemap/robots/OG card.** _[frontend#1,#3] Done (commit
  `4482ae9`, merged to master)._ Root layout sets `metadataBase` (turbolang.dev) + a `title` template; all 15
  docs routes now carry unique titles + content-derived descriptions; added `app/sitemap.ts` (reads the docs
  dir from disk so it can't drift), `app/robots.ts`, and a generated `next/og` `opengraph-image` + Twitter
  `summary_large_image` card. All 21 routes stay static SSG; lint + tsc clean. Per-page `og:title`/`og:desc`
  left inheriting the brand default (noted as a one-helper follow-up).

- [ ] **BL-16 — Commit the embeddable-typed-scripting wedge: `libturbo` C API spike.** _[strategist] STRATEGIC._
  The biggest gap is positioning, not code: as a "general-purpose compiled language" Turbo loses to Go/Rust on
  every axis. The one differentiated asset is a real Cranelift JIT. There is **zero `cdylib`/`staticlib`** in the
  repo. **AC (spike):** ship the codegen crate as a `cdylib`/`staticlib`; minimal `libturbo` C API
  (`turbo_vm_new`, `turbo_eval(src)`, register one host fn callable from Turbo, call one Turbo fn from C, marshal
  int + string both ways); a runnable "C host calls Turbo, exchanges typed values, calls a host fn back" demo +
  docs; one line in `design/VISION.md` committing the north star. Target **trusted/first-party scripts first** (no
  sandbox). Separately time-box a **sandbox-feasibility** investigation (can the JIT be stripped of file/net/
  syscall in a quarter?) → go/no-go, NOT a finished sandbox. Do this BEFORE any LLVM/perf or domain-sidecar work.

### P2 — robustness, language depth, conversion

- [x] **BL-17 — AOT HTTP correctness (JIT divergences).** _[backend F2,F3] Done (commit `f929328`, merged to
  master)._ F2: `rt_parse_response` colon-fallback now uses a strict `rt_parse_status_u16` (mirrors the JIT's
  `parse::<u16>()`) — a non-numeric prefix is sent as a 200 body instead of `HTTP/1.1 0 OK` + colon-truncated
  body. F3: request bodies that don't fit the 16KB stack buffer are read into a `content_length`-sized heap
  buffer (capped by `RT_HTTP_MAX_BODY`), mirroring the JIT's `read_exact` — no more spurious `431` on >16KB
  POSTs; all existing guards preserved. +2 JIT/AOT parity programs (`http_colon_response`, `http_body_limits`);
  fail-before-fix proven by stashing the C change.

- [x] **BL-18 — Mutex can't express atomic read-modify-write.** _[backend F4] Done (commit `5a6b36b`, merged to
  master)._ Added `mutex_update(m, closure)` — the closure (`fn(int)->int`) runs UNDER the lock (lock → new =
  closure(old) → store → unlock), reusing the existing runtime-calls-closure pattern from the HTTP route handler
  (JIT `MutexGuard` RAII; AOT `pthread_mutex_lock`/`unlock`). A shared counter is now correct under contention
  (4×25k → exactly 100000, 6/6 runs on JIT and AOT; get-then-set lost ~57%). Fixed `docs/stdlib.md` +
  `CONCURRENCY.md` to stop implying bare get/set suffice for concurrent read-modify-write. +phase1 +parity +2
  codegen unit tests. _Mutex values remain int-only (existing design); generic-T mutex out of scope._

- [x] **BL-19 — Sized integer types are an unusable island.** _[e2e] Done (commit `634a57a`, merged to
  master)._ Added an `as` cast operator end-to-end (lexer keyword → `Expr::Cast` → sema → Cranelift JIT+AOT;
  Rust-style precedence between unary and binary; numeric↔numeric only, `str as i32` rejected via E0100;
  narrowing wraps two's-complement, float→int saturates toward zero). Extended untyped-literal coercion into
  annotated sized types for array literals (`[u8] = [104,105]`), struct-field init, and literal-operand
  arithmetic (`n:i32; n+1`). +3 phase1 tests +1 parity (`int_casts`); 248 integration / 23 parity. Deferred:
  `to_i32`-style builtins (superseded by `as`); literal coercion in comparisons + into float types; narrow
  array-element assignment.

- [ ] **BL-20 — No CLI args (`args()` is a stub).** _[e2e]_ `rt_args()` returns an empty array ("not yet wired");
  `turbolang run f.tb hello` is rejected by clap; native binaries see `argc=0`. Can't write an arg-driven CLI —
  a stated target use case. **AC:** wire argc/argv into `rt_args` for AOT; `turbolang run <file> -- <args>` forwards
  trailing args; test asserts a native binary sees its argv.

- [ ] **BL-21 — `bench` reports "0/N passed" on every valid benchmark.** _[designer#8, e2e]_ Pass criterion is
  "JIT and AOT output match," but the build path rejects `@bench` (`unknown attribute '@bench'`) so AOT always
  fails → headline reads as total failure on correct input. `@bench` is also undocumented; the label prints the
  filename not the fn. **AC:** AOT/build path accepts `@bench`; lead with timings; AOT-match is a separate
  non-fatal line; label by function name; add a `@bench` doc + fixture.

- [x] **BL-22 — Website conversion friction.** _[frontend#2, designer#9] Done (commit `533501e`, merged to
  master)._ New `CopyButton` client component copies a raw prompt-free command via a string prop (never
  DOM-scraped, so `$ ` markers can't leak); homepage install block stores clean commands with the `$` as a
  `select-none aria-hidden` affordance. Installation leads with Homebrew; hero reduced to one primary CTA +
  an icon GitHub link; "Run the flagship demo" → "See" (links to static docs — no playground route exists
  yet; that's BL-14). All 21 routes stay static SSG; lint clean. _Docs-page `<pre>` blocks hand-roll markup
  (no shared component) so copy buttons there were out of cheap scope._

- [x] **BL-23 — `stdlib.md` is missing ~18 working builtins.** _[e2e] Done (commit `a5d74fc`, merged to
  master)._ Documented all 19 (`str_to_int`/`str_to_float` with their `Result` returns, `sort`, `slice`,
  `random`, `random_range`, `pad_left/right`, `list_dir`, `mkdir`, `delete_file`, `file_exists`, `path_join`,
  `time_now`, `time_ms`, `format_time`, `exit`, `str_from_char`, `type_of`) with compile-verified signatures
  read from the sema built-in env; added Filesystem + Date/Time sections. Count reconciliation: the brittle/
  false "all 104 built-in functions" claims in `README.md` + `GETTING-STARTED.md` were softened to "100+"
  (drops the inaccurate exact count AND the false "all", since the docs still aren't exhaustive). _stdlib.md
  still omits some builtins (substring, args, reverse, array_contains, any, all, int/float casts, math family)
  — a future doc pass, not blocking._

- [x] **BL-24 — Website a11y defects.** _[frontend#4-#7] Done (commit `6f105e1`, merged to master)._ Single
  `<main>` per docs route (inner → `<div>`); sub-AA `text-gray-500/600` small text → AA-passing `gray-400`
  (footer, benchmark labels, sidebar headers, captions; the only `gray-500` left is the decorative aria-hidden
  `$` prompt); global `:focus-visible` ring using the real accent token; `aria-label` on all 3 navs + a
  sr-only skip-to-content link; `prefers-reduced-motion` guard in globals.css. All 21 routes stay static SSG;
  lint clean.

- [ ] **BL-25 — Long-running-server memory model.** _[strategist theme 3]_ The flagship demo is an HTTP server, but
  the string arena is freed per-run and the README warns AOT servers leak — a credibility landmine the moment a
  skeptic runs it for real (also blocks a persistent embedded VM for BL-16). **AC:** fix the long-lived-process
  memory model, OR demote "server" from flagship to a terminating demo and stop leading with it. (Investigate first.)

- [ ] **BL-27 — COW parity gaps surfaced by BL-10.** _[backend, follow-on]_ While fixing struct COW it was
  confirmed that **arrays have the same aliasing at two of three sites**: passing an array to a `mut` param and
  `let s = arr2d[0]; s[0]=…` still alias the source (only `let b = a` array copy was correct). BL-10's struct
  retains are gated to `TurboTy::Struct(_)`, so this array gap is pre-existing and untouched. Separately, the
  **WASM backend** (`wasm_codegen.rs` / `turbo_rt_wasm.c`) has the analogous struct-store aliasing (direct store
  while its `IndexAssign` uses `rt_array_set`). **AC:** extend the COW retain/`rt_*_cow` discipline to arrays at
  the mut-param + array-element sites, and to WASM struct stores; parity tests for both (native + WASM). Also:
  BL-10 left a small bounded leak — a named struct passed to a *read-only* callee is retained without a matching
  release (callee never COWs); tighten if the embedded/long-running memory model (BL-25/BL-16) needs it.

### P3 — polish (batch; do NOT spawn busywork PRs — fold opportunistically)

- [ ] **BL-26 — Error/CLI/runtime polish cluster.** Weak/missing `Help:` that should echo the actual signature/field
  list/missing variant (E0100/E0200/E0315); messages rename the user's type via aliases (`i64`→`int`) instead of
  echoing source spelling (E0110); import error doesn't teach `import { x } from "./m.tb"`; `explain` rejects `100`/
  `e0100` (normalize input); whole-number floats print as ints (ambiguous type); raw `(os error 2)` jargon in file
  errors; `test` summary ordering/color/total-time; empty-RHS `let x =` emits a misleading double-error; REPL
  spurious `unused variable` across lines; consider raw strings (`r"…"`) so JSON/`{`-strings paste verbatim; empty
  `[]` can't infer. Backend P3s: `rt_spawn_with_args` ignores `pthread_create` return (joins uninit `pthread_t` on
  thread exhaustion); `rt_format_time` uses non-reentrant `localtime()` (use `localtime_r`); JIT hashmap `&mut *ptr`
  is a data race under concurrent `spawn` writes. _(The `fmt` `:`/`->`/operator-spacing gap here is already covered
  by BL-5.)_

---
_When all boxes are checked, STOP and ask for the next priorities — do not invent hygiene work._
