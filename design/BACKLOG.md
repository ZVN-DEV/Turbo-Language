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

- [ ] **BL-1 — Finish retiring the `?.unwrap()` panic class in codegen.** _Status: IN PROGRESS (Claude, 2026-06-28)._
  The 0.9.2 sprint fixed the 2 reachable panics but ~152 `compile_expr(...)?.unwrap()` remain in
  `turbo/crates/turbo-codegen-cranelift/src/builtins.rs` (+5 in `src/expr.rs`: ~1595/1620/1646/1851/1890).
  All are sema-guarded today, but a future sema gap panics the compiler (exit 101) instead of diagnosing.
  Convert them to `?.ok_or_else(|| CodegenError { code: <fitting ErrorCode, e.g. E0400/E0403>, message })?`.
  **AC:** no `?.unwrap()` on a `compile_expr` result remains in those two files; add a fuzz/regression
  pass (extend `turbo/fuzz/` or add adversarial `.tb`s) that throws malformed-but-parseable input at
  codegen and asserts a clean diagnostic, never a panic.

- [ ] **BL-2 — Add a real-world benchmark (CEO's top "next").** The suite is fib40-only; the positioning
  is "fast enough on real work" with zero real-work evidence. Add ONE end-to-end workload to
  `turbo/benchmarks/` — e.g. the built-in HTTP server doing JSON request/response throughput, or a
  text-processing pass over a real input file — with honest C/Rust/Go baselines, warmup, and
  output-equality enforcement (mirror `run_comparison.sh`). Publish the real-workload row next to fib40
  in README/website with methodology. **AC:** reproducible via a committed script; numbers are measured,
  not hardcoded; README/website updated truthfully.

## P2 — language & tooling depth

- [ ] **BL-3 — f32 across the spawn/generic/closure ABI boundary.** 0.9.2 made f32 params *fail loud*
  (sema E0100) because they previously panicked Cranelift / miscompiled. Do it properly: thread real
  `F32` ABI handling (bitcast/register-class) through `compile.rs` spawn thunks, `expr.rs` generic +
  inferred-closure calls, and `type_conv.rs` so `f32` works across those boundaries — then remove the
  sema reject and add JIT↔AOT parity tests. (If a full fix is out of reach, formally document f32 as a
  restricted type instead.) **AC:** an f32 spawn arg / generic type-arg / closure param round-trips
  correctly on JIT and AOT, parity-tested.

- [ ] **BL-4 — LSP scope/binding resolution.** Hover, go-to-def, references, and rename are token-text +
  top-level only (`turbo-lsp/src`); locals/fields/methods aren't navigable and shadowed names over-rename.
  Add real binding/scope resolution. **AC:** rename of a local only touches its scope; go-to-def resolves
  a local/param/field to its declaration; tests assert exact edit sets.

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

---
_When all boxes are checked, STOP and ask for the next priorities — do not invent hygiene work._
