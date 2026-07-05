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

- [x] **BL-6 — Split the two god-functions.** _Done 2026-06-28 (sema commit `65a5ada`, codegen commit `d08fe36`,
  merged to master)._ `check_expr_inner` (~4,540 lines) → a thin dispatcher delegating to 35 per-variant
  `check_<variant>` handlers (the ~2,800-line builtin section fanned out to 13 category helpers via
  `check_builtin_call`); longest fn now `check_match` at 370 lines. The `compile_call` `_` fallback arm (~311 lines,
  six concerns) → `compile_call` 577→202 lines via `compile_enum_variant_ctor` / `compile_ufcs_method_call` /
  `compile_closure_call` / `compile_plain_fn_call` (+ `compile_fn_call_args` / `infer_generic_ret_tty` /
  `try_inline_fn_call`), all ≤84 lines. Zero behavior change — verified by identical ErrorCode/builtin-name/return
  multisets (sema) and JIT≡AOT parity (codegen); full suite green (267 integration / 28 parity). Holdout (out of
  scope, named): the pre-existing `compile_expr_inner` Expr-dispatcher (1,347 lines) — BL-6 codegen was scoped to
  `compile_call`'s `_` arm; a future split could target it.

## P3 — coverage & ops

- [x] **BL-7 — WASM backend feature gaps (closures).** _Done (commit `bedac76`, merged to master)._ AC met:
  closures now compile to WASM and run under `wasmtime`, parity-matched to native. A closure is a heap
  `{fn_ptr, env_ptr}` pair; the body is lifted to a top-level C `fn(env, params)` with captures in an env
  struct; clang lowers the fn pointers to `call_indirect`. **Works:** direct closures, captures
  (int/bool/str/f64 via bit-pun), and `map`/`filter` closures. **Fails loud (clean E0403, no miscompile):**
  closures as user higher-order-fn params, and float-array `map`/`filter`. +5 WASM exec tests (the runner now
  does a 3-way `wasm == native == .expected` check) +6 codegen unit tests; no native regression. **Remaining
  WASM gaps (future):** async, some match patterns, higher-order closure params, returning/storing closures,
  and the BL-27 Part B struct/array COW.

- [x] **BL-8 — Homebrew formula sha sync.** _Done (commit `f5f0bf5`, merged to master)._ The real checksums
  are only known post-build and `release.yml` already writes the authoritative formula to the
  `ZVN-DEV/homebrew-turbo` tap, so the in-repo copy can't carry trustworthy shas. Marked it a
  non-authoritative template and extended `scripts/check_release_consistency.py` (already run in CI via
  ci.yml + nightly.yml) to FAIL if placeholder all-zero shas appear WITHOUT the template marker (or if a
  marked template carries real-looking shas) — so the repo can never silently present fake-but-real-looking
  checksums. Network-free; `--self-test` covers all three states + a mutation check.

- [x] **BL-9 — Runtime hashmap/string-key performance.** _Surfaced by BL-2. Done 2026-06-28._ The str→int
  hashmap path was re-stringifying (`snprintf`), re-parsing (`strtoll`), and re-allocating (`strdup`/`free`)
  the value on every increment — a `get_int` then a separate `set_int`, each hashing the key. **Fix:** int
  values are now stored inline in the hashmap entry (tagged `is_int` + `ivalue`) in both runtimes
  (`turbo_rt.c` AOT + `runtime.rs` JIT), so `hashmap_get_int`/`hashmap_set_int` do a single hash + single
  probe with no per-update allocation or stringification; str→str semantics are unchanged (`hashmap_get`
  stringifies an int entry on demand). Also added a fused `hashmap_inc(map, key[, delta]) -> int` primitive
  (the str→int counterpart of C's `table[k]++`; single lookup-or-insert) wired through sema + JIT/AOT
  codegen. **Measured (best of 5, ~5 MB, Apple M5 Max / macOS 26.5.1, `run_wordcount.sh`, output-equal):**
  Turbo AOT **~150 ms vs C ~108 ms → ~1.4x** (was ~240 ms / ~2.2x) — clears the ≤1.5x AC. The fused
  `hashmap_inc` is within run-to-run noise of the optimized `get_int`/`set_int` on word-count (the win was
  the inline storage, not the second hash), so `wordcount.tb` is left UNCHANGED on the idiomatic
  `get_int`/`set_int` pattern — the published number needs no benchmark rewrite. JIT still round-trips
  strings for `get_int`/`set_int` (~205 ms / ~1.9x, ~unchanged). Microbenchmark: `bench_hashmap_inc.tb`
  (~5M increments, ~46 ms AOT). Tests: `tests/phase1/hashmap_inc.{tb,expected}` + parity
  `tests/parity/programs/hashmap_inc.tb` (JIT≡AOT).

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
  _Progress 2026-07-04 (branch `codex/bl14-browser-playground`): added a static `/play` route, homepage
  secondary CTA, persistent nav/footer links, sitemap coverage, and regression tests. `/play` now server-renders the
  default playground UI without a client-side rendering bailout while still hydrating shared `?code=` links. Added a same-origin
  `/api/playground/run` proxy contract that refuses local shell execution, rejects non-JSON content types, caps
  request JSON before parsing, validates source size/shape, and only forwards to `TURBO_PLAYGROUND_RUNNER_URL`
  when a separate sandbox runner is configured. Added a dependency-free
  `website/playground-runner/` service + Dockerfile that runs `turbolang` through `execFile` with a fixed minimal
  child environment inside the documented container hardening boundary; startup now refuses missing/blank
  `TURBO_PLAYGROUND_RUNNER_TOKEN` unless explicitly opted into tokenless isolated-local testing, configured tokens are trimmed, blank tokens are never accepted as bearer secrets, and startup no longer logs token-derived fingerprints. The runner now rejects excess concurrent executions with HTTP 429. Runner tests cover auth, HTTP `/run`, non-JSON content-type
  rejection, malformed UTF-8 request rejection, concurrency backpressure, failed execution stderr, SIGKILL-enforced timeout, oversized-output failure, JSON failure envelopes, and source-policy
  rejection for compile-time imports, unsafe/FFI features, plus host-access filesystem/process/network builtins including `args`
  and `http_post_with_headers` while allowing pure path string helpers; bundled examples are now regression-checked
  against that public-runner policy, and the collections example was smoke-run through
  `turbo/target/release/turbolang` (`Total: 29`, `Count: 4`). The website proxy now returns explicit no-store /
  nosniff JSON, passes through safe runner validation/backpressure errors (400/413/429) instead of flattening them into a generic
  502, while still hiding upstream server/auth failures behind proxy-owned errors. Added scriptable runner and
  public-site smoke probes (`npm run smoke:playground-runner`, `npm run smoke:playground`) that check `/healthz`,
  `/play`, safe execution, and `exec` rejection before deployment. Local public-site smoke against `next start`
  verified `/play` contains the real playground UI, has no CSR bailout marker, safely executes code through the
  runner, and rejects `exec`. Local smoke against
  `turbo/target/release/turbolang` returned stdout and enforced token auth. A
  configured `next start` smoke
  (`TURBO_PLAYGROUND_RUNNER_URL=http://localhost:8790/run`) successfully proxied `/api/playground/run` through the
  runner and returned Turbo stdout. Browser QA verified desktop and 320px mobile `/play` rendering, Run, example
  selection, copy/share fallback behavior, and fixed the cramped mobile header by hiding the external GitHub nav
  link below `sm`. Full local gates passed: `cargo fmt --check` from `turbo/`, `cargo test --workspace --manifest-path turbo/Cargo.toml`,
  `cargo build --release --manifest-path turbo/Cargo.toml`, `cd turbo && ./tests/run_tests.sh` (272 passed, 0 failed, 10 skipped),
  `cd turbo && ./tests/parity/run_parity.sh` (29 passed), and `cargo clippy --workspace --manifest-path turbo/Cargo.toml -- -D warnings`.
  Local Docker image verification is blocked by an OrbStack data-image permission issue on this Mac, but PR CI now builds
  `website/playground-runner/Dockerfile`, starts the resulting image, waits for `/healthz`, and runs
  `npm run smoke:playground-runner` against the containerized service. That CI pass exposed and fixed a stale
  `rust:1.86-slim` builder image pin that no longer satisfies the current lockfile (`home@0.5.12` requires
  Rust 1.88), then exposed and fixed the missing `docs/errors` public-docs tree that `turbo-cli/build.rs`
  requires during the runner image build.
  The public page deliberately does **not** proxy arbitrary source to a
  shell-running `/api/run`; BL-14 remains open until the runner is deployed behind the website and smoke-tested at
  `turbolang.dev/play`._

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

- [x] **BL-20 — No CLI args (`args()` is a stub).** _[e2e] Done (commit `6b08cad`, merged to master)._ AOT
  `main(argc, argv)` → `rt_set_args` → `rt_args()` builds `[str]` from `argv[1..]`; JIT uses a `PROGRAM_ARGS`
  global set by the CLI before `jit_run`; clap `Run` gained a `trailing_var_arg` so `turbolang run f.tb -- a b c`
  (and trailing args) forward to the program. Convention: `args()[0]` is the first user arg (binary/source path
  excluded); JIT≡AOT (`run f.tb -- a b` and `./bin a b` produce identical output). +dedicated args shell test
  (`tests/args/`, JIT==AOT, hyphen-leading + empty cases) +parity. Docs updated. _WASM `args()` untouched._

- [x] **BL-21 — `bench` reports "0/N passed" on every valid benchmark.** _[designer#8, e2e] Done (commit
  `13b3850`, merged to master)._ Root cause was the **parser** rejecting `@bench` (`unknown attribute`), so
  BOTH the JIT and AOT bench paths failed. Made `@bench` a first-class marker attribute (`FnDef.is_bench`)
  accepted on every backend and round-tripped by `fmt`. `bench` reporting now leads with the timing, makes
  AOT-vs-JIT parity a separate non-fatal line (`AOT parity: ok/…`), labels by function name, and never prints
  `0/N` for a run that produced a valid timing (headline is `N/M benchmarks completed`). +`@bench` fixture +CLI
  integration test +`TOOLCHAIN.md` docs. _`@bench` is a marker only (no `Bencher`/`b.iter()` API yet) — noted
  aspirational in the doc._

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

- [x] **BL-25 — Long-running-server memory model.** _Done 2026-06-28 (code commit `8b3dcab` + docs, merged to master;
  Kirby chose "truthful fix + go deep")._ The investigation found the README's leak warning was **backwards**: the
  per-request bump arena already bounds **AOT** servers (~1.8 MB RSS plateau), while the unbounded leak was on **JIT**
  (`turbolang run` — `handle_http_connection` never reset `STRING_ARENA`, which only drains when `main()` returns) —
  and the flagship demo instructed exactly that JIT path. It also surfaced an UNDISCLOSED **use-after-free**: a
  stateful AOT server (a startup hashmap mutated in a handler) had its entries freed by `rt_arena_end()`.
  **A1** — the JIT HTTP handler records a per-request arena high-water mark and truncates back to it after each
  response: RSS ~62 MB/1k-req leak → flat ~11 MB plateau (stateless `web-dashboard` byte-identical across requests;
  non-server `run` unchanged). **A2** — persistent hashmaps allocate entry storage with real `malloc`/`free`
  (scope-following: request-local maps stay arena-backed → no residual leak), so server state survives
  `rt_arena_end()`: ASan heap-use-after-free in `rt_hashmap_inc` → clean, counter `1,2,1,3…` → `1..10`. JIT hashmaps
  needed no A2 (they own keys/values as Rust `String`s). +2 ASan-gated C-runtime regression tests + an
  `examples/stateful-counter/` demo; JIT≡AOT parity preserved (28/28). **Docs:** corrected the backwards leak claim
  in `README.md` + `docs/SAFETY.md` (servers are bounded on both backends; residual narrowed to non-server infinite
  loops) and listed the stateful-counter example. **Also fixed a latent CI red:** BL-26's whole-float change made
  `rt_f64_to_str(-0.0)` → `"0.0"` but the C-runtime `test_rt.c` expectation (run by `tests.sh`, which the standard
  local gate omits) still asserted `"0"` (commit `28b0dfb`) — `tests.sh` + the ASan `test_rt.c` build are now in the
  local pre-push routine. _A2 follow-on: the AOT `channel_queue` uses `turbo_calloc`, so a channel created INSIDE a
  handler is arena-scoped (a startup channel is malloc-backed and fine) — not the BL-25 bug, left untouched; revisit
  for BL-16's embedded VM if in-handler channel persistence is ever needed._

- [ ] **BL-27 — COW parity gaps surfaced by BL-10.** _[backend, follow-on]_ Two parts: native arrays (Part A,
  **DONE**) and the WASM backend (Part B, **still open**).
  - **Part A — native arrays (DONE, this commit).** Confirmed that arrays had the same aliasing at two of three
    sites: passing an array to a `mut` param and `let row = grid[0]; row[0]=…` still aliased the source (only
    `let b = a` array copy was correct). The array COW machinery already existed (`rt_array_set`'s
    `refcount>1 → private copy` + the `IndexAssign` binding repoint); only the refcount was dishonest because
    BL-10's retains were gated to `TurboTy::Struct(_)`. Fix: widened both retain-site gates to
    `TurboTy::Struct(_) | TurboTy::Array(_)` — the `mut`-param arg retain in `compile_call` (`src/expr.rs`) and
    the array-element extraction retain in `let` (`src/stmt.rs`). No runtime change (rt_array_set self-sizes its
    copy). +2 phase1 tests (`array_cow_mut_param`, `array_cow_nested_element`) + 1 JIT/AOT parity test
    (`array_value_semantics`); JIT≡AOT verified on both reproductions.
  - **Part B — WASM backend struct + array value semantics (STILL OPEN, deferred).** The WASM C-transpiler
    (`wasm_codegen.rs` / `turbo_rt_wasm.c`) aliases on EVERY binding site for BOTH structs and arrays, and is a
    genuinely larger fix than Part A — deferred rather than shipped half-done. Confirmed under `wasmtime`:
    `let b = a; b.x=99` (struct) and `let b = a; b[0]=99` (array) and array-`mut`-param all print the mutated
    value (e.g. `99 99` where native prints `1 99`); a struct passed to a fn does not even compile
    (`void bump(long long p)` vs a `void*` arg → clang `-Wint-conversion`), and nested-array element extraction
    (`let row = grid[0]`) likewise fails to compile. **Root cause (why it is bigger than Part A):** (1) the WASM
    transpiler never emits `rt_retain` at ANY binding site, so the refcount is always 1 → `rt_array_set`'s COW
    never fires and there is no struct COW at all; (2) there is no `rt_struct_cow` in `turbo_rt_wasm.c` and
    `FieldAssign` does a direct store; (3) the stmt-context `IndexAssign` discards `rt_array_set`'s return, so
    even with honest refcounts the COW copy's write would be lost rather than repointed; (4) the coarse
    string type-tags (`var_types`) don't track struct *names* (needed to size `rt_struct_cow`'s field count) or
    array *element types* (needed to safely gate a retain on `grid[0]` vs a scalar `ints[0]`); (5) the
    struct/array param ABI is typed `long long` while values are `void*`. **Exact remaining scope:** (a) fix the
    struct/array param ABI typing; (b) enrich `var_types`/inference to carry struct name + array element type;
    (c) port `rt_struct_cow` to `turbo_rt_wasm.c` (8-byte refcount header, no cap field) and call it in
    `FieldAssign` with a binding repoint; (d) emit `rt_retain` at the three binding sites (`let b = a`,
    `mut`-param arg, array/struct element extraction), gated on the enriched types; (e) make the stmt-context
    `IndexAssign` repoint `obj = rt_array_set(obj, idx, val)` when `obj` is an lvalue; (f) add
    `turbo/tests/wasm/` value-semantics coverage + a WASM↔native parity case. **Why deferred:** a partial WASM
    change is either a no-op (no retains ⇒ COW never triggers) or actively unsafe (a mis-gated `rt_retain` on a
    scalar treats an integer as `addr-8` and corrupts memory — strictly worse than the current honest
    aliasing). Per the "don't make it worse" bar, the WASM gap is recorded here rather than half-fixed.
  - **Known bounded leak (unchanged):** BL-10 left a small bounded leak — a named struct (now also array) passed
    to a *read-only* callee is retained without a matching release (callee never COWs); the Part A array widening
    inherits the same shape but does not worsen it. Tighten if the embedded/long-running memory model
    (BL-25/BL-16) needs it.

### P3 — polish (batch; do NOT spawn busywork PRs — fold opportunistically)

- [x] **BL-26 — Error/CLI/runtime polish cluster.** _FULLY DONE 2026-06-28 (3 rounds)._ Weak/missing `Help:` that should echo the actual signature/field
  list/missing variant (E0100/E0200/E0315); messages rename the user's type via aliases (`i64`→`int`) instead of
  echoing source spelling (E0110); import error doesn't teach `import { x } from "./m.tb"`; `explain` rejects `100`/
  `e0100` (normalize input); whole-number floats print as ints (ambiguous type); raw `(os error 2)` jargon in file
  errors; `test` summary ordering/color/total-time; empty-RHS `let x =` emits a misleading double-error; REPL
  spurious `unused variable` across lines; consider raw strings (`r"…"`) so JSON/`{`-strings paste verbatim; empty
  `[]` can't infer. Backend P3s: `rt_spawn_with_args` ignores `pthread_create` return (joins uninit `pthread_t` on
  thread exhaustion); `rt_format_time` uses non-reentrant `localtime()` (use `localtime_r`); JIT hashmap `&mut *ptr`
  is a data race under concurrent `spawn` writes. **Hashmap-handle type confusion (pre-existing, found during
  BL-9):** a `hashmap` is an opaque i64 handle, so sema does NOT reject assigning an int to a hashmap-typed var
  (e.g. `m = hashmap_get_int(m, k)` or `m = hashmap_inc(m, k)`) — the next `hashmap_*(m, …)` then dereferences an
  integer and **segfaults** instead of producing a type error. Reproduces with pre-existing builtins too; the fix
  is a distinct opaque-handle type for hashmaps (and likely mutex/http handles) so int↔handle assignment is a
  clean sema error. _(The `fmt` `:`/`->`/operator-spacing gap here is already covered by BL-5.)_

  **DONE 2026-06-28 (contained high-value subset):** E0100 arity `Help:` now echoes the full signature + what was
  passed (`'add' takes 2 args (a: int, b: int); you passed 1`); E0200 `Help:` names the missing variant(s)
  (`add an arm 'Blue => ...' or a catch-all '_ => ...'`); E0315 `Help:` lists the struct's fields with a did-you-mean
  (`'Point' has fields 'x', 'y' — did you mean 'width'?`); E0110 now echoes the source spelling (`i64`, not `int`);
  malformed imports now teach the syntax (`imports look like \`import { sqrt, pi } from "./math.tb"\``); `explain`
  normalizes `100`/`e0100`/`E100` → `E0100`; `rt_format_time` switched to `localtime_r`; `rt_spawn_with_args` now
  checks the `pthread_create` return and fails cleanly instead of joining an uninitialized `pthread_t`.

  **DONE 2026-06-28 (Round 2 — 4 parallel branches, all merged to master, full suite green: 267 integration / 28
  parity / workspace, clippy + fmt clean):** raw strings `r"…"` (lexer-only; reuses the existing string node so
  sema/codegen are untouched, brace re-encoding keeps interpolation literal — commit `b526e8a`); whole-number floats
  now print with a trailing `.0` (`2.0` not `2`, so the type is unambiguous; one shared JIT+AOT float helper +
  14 `.expected` updated; JIT≡AOT byte-identical — commit `9bcf269`); **hashmap/mutex/http opaque-handle typing** —
  int↔handle mixing (`m = hashmap_get_int(m,k)` / `m = hashmap_inc(m,k)`) is now a clean compile-time `E0111` instead
  of a runtime SEGFAULT (exit 139→1), via a distinct `Ty::Handle(HandleKind{HashMap,Mutex,HttpServer})` with one-way
  `int→Handle` coercion that preserves the legit "pass a handle to an `i64` param" idiom (commit `ed4d266`); CLI polish
  — remaining `(os error N)` IO leaks translated to plain language, `test` summary gains TTY-gated color + total-time,
  and the REPL no longer flags a later-used binding as `unused` (commit `87fd7fb`).

  **DONE 2026-06-28 (Round 3 — 4 parallel branches, all merged to master, full suite green: 271 integration / 29
  parity / 7 WASM / C-runtime tests.sh, clippy + fmt clean):** **empty-`[]` inference** — a bare `[]` now adopts its
  element type from an annotated let/param/struct-field/return context (concrete-type guard keeps genuinely
  uninferrable `let xs = []` / generic `[T]` a clean `E0115`; commit `d93be28`); **empty-RHS `let x =`** now emits one
  clear diagnostic anchored on the `=` ("expected an expression after `=` in let binding") — the literal double-error
  was already gone, the message was just misdirected at `}`/EOF (commit `aabf0f0`); **WASM whole-float drift** —
  `turbo_rt_wasm.c`'s `rt_format_f64` ported byte-identical from the AOT rule, verified `wasm == native == expected`
  under wasmtime 43 (commit `a5386eb`); **JIT hashmap data race** — the JIT hashmap is now `Mutex<HashMap>` behind the
  same i64 handle (one `lock_hashmap` helper for all 10 ops, no reentrancy/deadlock, no escaping borrows); an
  8-thread × 50k-inc stress repro went from **30/30 crashes → 30/30 correct** (commit `bc0170c`). **BL-26 is fully
  resolved.** _New finding while aligning WASM floats → tracked as **BL-28**._

- [ ] **BL-28 — WASM backend mis-types index/field/unary float expressions as `int` (value truncation).** _[found
  during BL-26 Round 3 WASM-float work]_ On the WASM target, a `float` value reached via an **array index** (`xs[0]`),
  **struct field** (`v.x`), or **unary negation** (`-3.0`, `-0.0`) is mis-tagged as `int` by `infer_type_tag` in
  `wasm_codegen.rs` (it falls through to `"int"` for `Expr::Index` / `Expr::FieldAccess` / `Expr::UnaryOp`), so it is
  printed via `rt_print_i64` — which not only drops the `.0` but **truncates the value** (`xs[1] == 2.5` prints `2`,
  `-3.0` prints `-3`). Native (JIT/AOT) handles these correctly; this is WASM-only. Also mis-dispatches `to_str()` of
  float arrays/structs to `rt_i64_to_str`. **AC:** WASM `infer_type_tag` resolves the element/field/operand float type
  (the WASM backend needs to track array element types + struct field types it currently doesn't) so these values
  print/serialize correctly; add WASM↔native parity coverage for float-via-index/field/negation. Distinct from BL-27
  Part B (that's WASM struct/array *CoW value semantics*; this is *scalar float type inference*).

---
_When all boxes are checked, STOP and ask for the next priorities — do not invent hygiene work._
