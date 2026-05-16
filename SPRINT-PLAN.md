# Sprint Plan — TurboLang v0.9.1 Hardening + TurboServo v0.9.0

Generated: 2026-05-16
Based on: Product review conversation findings (5-agent audit)

## Sprint Goal

Fix all security vulnerabilities and engineering quality issues from the product review, then update TurboServo to v0.9.0 as the flagship killer app showcase.

## Success Criteria

- [ ] All P0 (CRITICAL/HIGH security) issues resolved
- [ ] All P1 (MEDIUM security + engineering quality) issues resolved
- [ ] C runtime hardened: JSON, padding, HTTP parsing
- [ ] Parser panic on user input eliminated
- [ ] CLI unwraps replaced with proper error handling
- [ ] Codegen CString helper extracted (29 duplicates removed)
- [ ] TurboServo updated to TurboLang v0.9.0
- [ ] Build passes, all tests pass

## Dev Tracks

### Track 1: C Runtime Security Hardening
**Files touched:** `turbo/crates/turbo-codegen-cranelift/runtime/turbo_rt.c`
**Tasks:**
- [P0] TASK-01: Fix JSON escape parser in rt_json_root() — add \t \b \f handling, bounds tracking
- [P0] TASK-02: Validate padding width in rt_pad_left/rt_pad_right — reject negative width
- [P1] TASK-03: Check sscanf return value in HTTP request parsing
- [P1] TASK-04: Add bounds checking to rt_json_get pointer arithmetic

### Track 2: Parser Safety
**Files touched:** `turbo/crates/turbo-parser/src/lib.rs`
**Tasks:**
- [P0] TASK-05: Fix parser panic on soft-keyword EOF — replace .unwrap() at line 1565

### Track 3: CLI Robustness + Build Config
**Files touched:** `turbo/crates/turbo-cli/src/main.rs`, `turbo/Cargo.toml`
**Tasks:**
- [P1] TASK-06: Replace .unwrap() on fs ops in turbolang init with proper error messages
- [P1] TASK-07: Add default-run = "turbolang" to workspace Cargo.toml

### Track 4: Codegen Code Quality
**Files touched:** `turbo/crates/turbo-codegen-cranelift/src/runtime.rs`
**Tasks:**
- [P2] TASK-08: Extract cstring_or_empty() helper, replace 29 duplicate patterns

### Track 5: Integration Tests for Security Fixes
**Files touched:** `turbo/tests/phase1/` (new files only), `turbo/tests/adversarial/` (new files only)
**Tasks:**
- [P1] TASK-09: Add test for JSON escape sequences
- [P1] TASK-10: Add test for padding edge cases
- [P1] TASK-11: Add test for soft-keyword EOF (expected error)

### Track 6: TurboServo v0.9.0 Update (separate repo)
**Files touched:** `/Users/macbookpro-kirby/Desktop/Coding/ZVN/turboservo/`
**Tasks:**
- [P1] TASK-12: Update TurboServo to compile with TurboLang v0.9.0
- [P1] TASK-13: Update turbo.toml version to 0.9.0
- [P1] TASK-14: Update TURBO-LANG-AUDIT.md
- [P2] TASK-15: Migrate hand-rolled parse_i64 to str_to_int builtin
