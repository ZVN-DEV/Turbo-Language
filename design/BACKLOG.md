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

- [ ] **BL-5 — AST-based formatter.** `turbo-formatter` is a line-based brace/space tidier; it doesn't
  space arithmetic operators, `:` in params, or `->`, and won't expand inline blocks. Replace with a real
  AST pretty-printer. **AC:** idempotent (`fmt` then `fmt` is a no-op), produces canonical spacing, and the
  `init` scaffold + all `tests/phase1/*.tb` round-trip stably.

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
_When all boxes are checked, STOP and ask for the next priorities — do not invent hygiene work._
