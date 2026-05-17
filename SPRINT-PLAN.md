# Sprint Plan — TurboLang v0.9.1 (MEDIUM Fixes + P1 Features)

Generated: 2026-05-17
Based on: FINDINGS-REPORT.md (issues #8-13) + strategic gaps

## Sprint Goal

Fix all 6 remaining MEDIUM-severity bugs and add 3 P1 builtins (http_post_with_headers, json_build, float_to_int/int_to_float) to unblock turbo-agent and harden the runtime.

## Success Criteria

- [ ] All 6 MEDIUM issues resolved
- [ ] http_post_with_headers builtin working end-to-end (C runtime + JIT + codegen + sema)
- [ ] json_build builtin working end-to-end
- [ ] float_to_int / int_to_float builtins working end-to-end
- [ ] All existing tests still pass
- [ ] New integration tests for each fix and feature

## Dev Tracks

### Track 1: C Runtime Fixes + New Runtime Functions
**Agent:** C Runtime Specialist
**Files touched:** `turbo/crates/turbo-codegen-cranelift/runtime/turbo_rt.c`
**Tasks:**
- [ ] TASK-02: Fix `rt_json_get` naive substring search for nested JSON (line ~1375)
- [ ] TASK-03: Fix `rt_mutex_clone` to use refcounted wrapper (line ~1599)
- [ ] TASK-06: Fix `rt_list_dir` to use malloc/realloc instead of turbo_alloc/turbo_realloc (line ~2579)
- [ ] TASK-07a: Implement `rt_http_post_with_headers(url, body, headers)` in C runtime
- [ ] TASK-08a: Implement `rt_json_build(pairs_csv)` in C runtime
- [ ] TASK-09a: Implement `rt_float_to_int(f64) -> i64` and `rt_int_to_float(i64) -> f64` in C runtime

### Track 2: Codegen Fixes + New Builtin Compilation
**Agent:** Codegen Specialist
**Files touched:** `turbo/crates/turbo-codegen-cranelift/src/builtins.rs`, `turbo/crates/turbo-codegen-cranelift/src/expr.rs`
**Tasks:**
- [ ] TASK-01: Replace `.unwrap()` on binary op compile (expr.rs:78) with proper CodegenError
- [ ] TASK-04: Fix `compile_abs` to handle f64 via fabs
- [ ] TASK-05: Fix `compile_min`/`compile_max` to use unsigned comparison for unsigned types
- [ ] TASK-07b: Add `http_post_with_headers` codegen in builtins.rs
- [ ] TASK-08b: Add `json_build` codegen in builtins.rs
- [ ] TASK-09b: Add `float_to_int` / `int_to_float` codegen in builtins.rs

### Track 3: JIT Runtime + Sema Type Signatures
**Agent:** Type System Specialist
**Files touched:** `turbo/crates/turbo-codegen-cranelift/src/jit.rs`, `turbo/crates/turbo-codegen-cranelift/src/runtime.rs`, `turbo/crates/turbo-sema/src/lib.rs`
**Tasks:**
- [ ] TASK-07c: Register `rt_http_post_with_headers` in JIT symbol table + add JIT wrapper in runtime.rs
- [ ] TASK-08c: Register `rt_json_build` in JIT symbol table + add JIT wrapper in runtime.rs
- [ ] TASK-09c: Register `rt_float_to_int` / `rt_int_to_float` in JIT symbol table + add JIT wrappers
- [ ] TASK-07d: Add `http_post_with_headers` type signature in sema
- [ ] TASK-08d: Add `json_build` type signature in sema
- [ ] TASK-09d: Add `float_to_int` / `int_to_float` type signatures in sema

### Track 4: Integration Tests
**Agent:** Test Engineer
**Files touched:** `turbo/tests/phase1/` (new .tb + .expected files only)
**Tasks:**
- [ ] TEST-01: `json_nested.tb` — test rt_json_get with nested JSON objects
- [ ] TEST-02: `list_dir.tb` — test rt_list_dir basic functionality
- [ ] TEST-03: `http_post_headers.tb` — test http_post_with_headers
- [ ] TEST-04: `json_build.tb` — test json_build with multiple key-value pairs
- [ ] TEST-05: `type_conversions.tb` — test float_to_int and int_to_float
- [ ] TEST-06: `abs_float.tb` — test abs() with float values
- [ ] TEST-07: `min_max_types.tb` — test min/max with various numeric types

## File Conflict Analysis
- Track 1: turbo_rt.c (exclusive)
- Track 2: builtins.rs, expr.rs (exclusive)
- Track 3: jit.rs, runtime.rs, sema/lib.rs (exclusive)
- Track 4: tests/phase1/*.tb (exclusive, new files only)
- **No conflicts between tracks.**
