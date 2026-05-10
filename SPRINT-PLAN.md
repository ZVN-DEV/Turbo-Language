# Sprint Plan — TurboLang v0.8.1 Hardening Sprint
Generated: 2026-05-09
Based on: Product review findings (in-conversation audit, 5 parallel research agents)

## Sprint Goal
Fix all P0/P1 security and memory-safety bugs in the C and Rust runtimes, document the security model honestly, write a getting-started tutorial, and update all marketing/docs to match reality.

## Success Criteria
- [ ] All P0 issues resolved (arena/malloc mismatch, overflow checks, JIT memory leak)
- [ ] All P1 issues resolved (strdup bypass, header injection, request size limits, sema panic)
- [ ] Security model documented honestly (sandbox limitations, safety narrative)
- [ ] Getting-started tutorial exists
- [ ] README and design docs updated for accuracy
- [ ] All existing tests still pass
- [ ] New regression tests added for every fix

## Intentionally Deferred
- **Codegen lib.rs split** (P2) — 4327-line file needs decomposition but the refactor is risky mid-sprint
- **Cross-file LSP go-to-definition** (P3) — important but separate initiative
- **AST-aware formatter** (P3) — line-based formatter is adequate for now
- **Closure type inference** (P3) — verbose but functional
- **Windows support** (P3) — significant effort, separate initiative

---

## Dev Tracks

### Track 1: C Runtime Memory Safety — Security Hardening Agent
**Priority:** P0 + P1
**Files touched:** `turbo/crates/turbo-codegen-cranelift/runtime/turbo_rt.c`, `turbo/crates/turbo-codegen-cranelift/runtime/tests/test_rt.c`

**Tasks:**
- [ ] TASK-01 (P0): Implement `turbo_free()` — arena-aware free that checks if pointer is within arena before calling `free()`.
- [ ] TASK-02 (P0): Implement `turbo_strdup()` — strdup replacement that routes through `turbo_alloc()`.
- [ ] TASK-03 (P0): Replace all `free()` on arena-backed memory with `turbo_free()`: rt_json_get, hashmap_resize, rt_hashmap_set, rt_hashmap_remove, rt_await_handle.
- [ ] TASK-04 (P0): Replace all `strdup()` with `turbo_strdup()`: ~13 call sites.
- [ ] TASK-05 (P0): Add overflow check to `turbo_calloc()` using `__builtin_mul_overflow`.
- [ ] TASK-06 (P0): Add negative/overflow check to `rt_struct_alloc()`.
- [ ] TASK-07 (P1): Fix HTTP response header injection in `rt_respond_typed()` — reject `\r\n` in content_type.
- [ ] TASK-08 (P1): Add HTTP request size limit — cap header line length and total header size.
- [ ] TASK-09 (P1): Add regression tests in test_rt.c for all fixes.

### Track 2: Rust Runtime & Compiler Fixes — Compiler Hardening Agent
**Priority:** P0 + P1
**Files touched:** `turbo/crates/turbo-codegen-cranelift/src/runtime.rs`, `turbo/crates/turbo-codegen-cranelift/src/jit.rs`, `turbo/crates/turbo-sema/src/type_check/mod.rs`

**Tasks:**
- [ ] TASK-10 (P0): Fix `rt_release` — implement actual deallocation when refcount reaches zero.
- [ ] TASK-11 (P1): Fix sema `.unwrap()` panic on unregistered function lookup.
- [ ] TASK-12 (P1): Enable Cranelift IR verifier in debug builds.
- [ ] TASK-13 (P1): Fix HTTP response header injection on Rust side — sanitize content_type.
- [ ] TASK-14 (P1): Add HTTP request size limit on Rust side — cap read_line().
- [ ] TASK-15 (P1): Fix `rt_str_char_at` negative index — add explicit check before usize cast.

### Track 3: Documentation, Safety Narrative & Marketing — Docs & Marketing Agent
**Priority:** P1 + P2
**Files touched:** `README.md`, `docs/*`, `design/*`, `CONTRIBUTING.md`, `SECURITY.md`

**Tasks:**
- [ ] TASK-16 (P1): Create `docs/SAFETY.md` — comprehensive safety narrative.
- [ ] TASK-17 (P1): Create/update `SECURITY.md` — honest security model documentation.
- [ ] TASK-18 (P2): Create `docs/GETTING-STARTED.md` — 15-minute tutorial.
- [ ] TASK-19 (P2): Review and update `README.md` — verify claims, add links, update benchmarks.
- [ ] TASK-20 (P2): Review and update all design docs for accuracy.
- [ ] TASK-21 (P2): Update `docs/stdlib.md` and `docs/errors.md` for completeness.
- [ ] TASK-22 (P2): Update `CONTRIBUTING.md` — verify instructions, add security reporting guidance.

### Track 4: Test Coverage & Build Quality — Test Coverage Agent
**Priority:** P2
**Files touched:** `turbo/crates/turbo-cli/src/` (tests), `turbo/crates/turbo-ast/src/lib.rs` (doc-tests), `turbo/crates/turbo-parser/src/lib.rs` (doc-tests), `turbo/tests/phase1/`, `turbo/crates/turbo-cli/build.rs`

**Tasks:**
- [ ] TASK-23 (P2): Fix build.rs stale path causing silent exhaustiveness check skip.
- [ ] TASK-24 (P2): Add 8+ CLI unit tests covering init, explain, formatter, file validation.
- [ ] TASK-25 (P2): Add doc-tests to key public types in turbo-ast and turbo-parser.
- [ ] TASK-26 (P2): Add 3-5 new integration tests (.tb/.expected pairs).
