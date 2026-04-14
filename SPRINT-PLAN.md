# Sprint Plan — Turbo
Generated: 2026-04-13
Based on: product review findings from this session

## Sprint Goal
Make Turbo materially more trustworthy by removing misleading shipped-feature claims, fixing high-signal runtime/security issues, and tightening demo/docs consistency.

## Success Criteria
- [ ] All P0/P1 trust-breaking issues resolved
- [ ] AI/agent product messaging matches actual compiler behavior
- [ ] `exec()` no longer functions as a safe-context shell escape
- [ ] HTTP demo surface returns sane content types for browser-facing flows
- [ ] Insecure roadmap auth defaults are removed or clearly hardened
- [ ] Version/docs/example inconsistencies reduced
- [ ] Build + core verification commands pass after integration

## Priority Triage

### P0 - CRITICAL
- None from the inherited review

### P1 - HIGH
- TRACK-01 / TASK-01: Remove or reframe shipped AI-agent claims that overstate compiler support
- TRACK-02 / TASK-02: Gate `exec()` behind `@unsafe` semantics instead of allowing shell execution from safe code
- TRACK-02 / TASK-03: Fix HTTP response content-type behavior that makes browser demos render incorrectly
- TRACK-03 / TASK-04: Remove insecure fallback JWT secret and public-facing unsafe defaults in roadmap API

### P2 - MEDIUM
- TRACK-03 / TASK-05: Stop passing WebSocket auth tokens in query strings in the roadmap example/docs
- TRACK-04 / TASK-06: Fix stale version / docs drift that undermines trust
- TRACK-04 / TASK-07: Clean up deprecated error-code docs that no longer match source of truth

### P3 - LOW
- Leave broad compiler simplification / architecture refactors out of this sprint
- Leave LLVM-path expansion out of this sprint

## Dev Tracks

### Track 1: Trust Surface & Product Honesty — executor
**Files touched:** `README.md`, `website/src/app/page.tsx`, `website/src/app/docs/page.tsx`, `website/src/app/docs/agents/page.tsx`, `website/src/app/docs/examples/page.tsx`

**Tasks:**
- [ ] TASK-01: Remove or soften claims that Turbo already ships first-class AI agents/tool-calling when the compiler does not parse `agent` / `tool fn`
- [ ] TASK-08: Make docs/examples language accurately distinguish runnable examples vs roadmap/planned examples
- [ ] TASK-09: Keep differentiation honest: compiler/tooling integration is shipped; AI integration is planned

### Track 2: Runtime Safety & Demo Correctness — executor
**Files touched:** `turbo/crates/turbo-sema/src/type_check/expr.rs`, `turbo/crates/turbo-codegen-cranelift/src/runtime.rs`, `turbo/crates/turbo-codegen-cranelift/runtime/turbo_rt.c`, `turbo/tests/phase1/exec_env_get.tb`, `turbo/tests/phase1/http_server.tb`

**Tasks:**
- [ ] TASK-02: Make `exec()` require `@unsafe` context, consistent with other dangerous builtins
- [ ] TASK-03: Fix HTTP runtime response typing so HTML/browser-facing flows do not get mislabeled as JSON
- [ ] TASK-10: Update regression coverage so unsafe exec and HTTP response behavior are tested

### Track 3: Roadmap Example Hardening — executor
**Files touched:** `examples/roadmap/web-api/src/auth.tb`, `examples/roadmap/web-api/src/main.tb`, `examples/roadmap/web-api/src/routes/ws.tb`, `examples/roadmap/web-api/README.md`

**Tasks:**
- [ ] TASK-04: Remove known default JWT secret fallback and make configuration expectations explicit
- [ ] TASK-05: Replace query-string WebSocket token guidance with a safer pattern or explicit warning/placeholder flow
- [ ] TASK-11: Align roadmap example README with hardened/non-runnable status so readers do not cargo-cult insecure defaults

### Track 4: Trust Cleanup & Consistency — executor
**Files touched:** `docs/errors.md`, `turbo/crates/turbo-cli/src/playground.html`

**Tasks:**
- [ ] TASK-06: Update playground footer/version so shipped tooling reflects current release version
- [ ] TASK-07: Remove stale deprecated agent error-code references from docs that no longer exist in source

## Intentionally Skipped This Sprint
- Full AI-agent language implementation (`agent` / `tool fn`) — too large for a trust/hardening sprint
- Broad runtime/server security redesign (thread-pool limits, TLS, auth framework)
- Large-scale architecture cleanup across compiler crates

## Manual Follow-Ups After Sprint
- Consider rotating any secrets copied from old roadmap docs/examples
- Consider pinning GitHub Actions by commit SHA in release-bearing workflows
- Decide whether `exec()` should remain in the language long-term or be replaced by a more explicit process API

---

## Follow-on Sprint (Iteration 2)

### Goal
Turn the first trust sprint into a stronger shipping story by hardening the release/install path, replacing response content sniffing with explicit APIs, making shell execution more explicit, tightening server runtime safety, and elevating the dashboard into a true flagship demo.

### Tracks

#### Track 5: Supply-chain hardening
**Files touched:** `distribution/install.sh`, `.github/workflows/ci.yml`, `.github/workflows/release.yml`, `.github/workflows/nightly.yml`, `SECURITY.md`
- [ ] Pin GitHub Actions to immutable SHAs
- [ ] Tighten installer verification flow and documentation

#### Track 6: Runtime API & server hardening
**Files touched:** `turbo/crates/turbo-sema/src/type_check/expr.rs`, `turbo/crates/turbo-sema/src/type_check/mod.rs`, `turbo/crates/turbo-codegen-cranelift/src/expr.rs`, `turbo/crates/turbo-codegen-cranelift/src/builtins.rs`, `turbo/crates/turbo-codegen-cranelift/src/lib.rs`, `turbo/crates/turbo-codegen-cranelift/src/runtime.rs`, `turbo/crates/turbo-codegen-cranelift/runtime/turbo_rt.c`, `turbo/crates/turbo-codegen-cranelift/runtime/turbo_rt_wasm.c`, `turbo/tests/phase1/http_server.tb`, `turbo/tests/phase1/exec_env_get.tb`
- [ ] Add explicit HTTP response helpers instead of relying on content sniffing
- [ ] Add server-id bounds checks and active-connection limits
- [ ] Make shell execution more explicit than plain `exec`

#### Track 7: Flagship demo elevation
**Files touched:** `README.md`, `examples/README.md`, `examples/web-dashboard/README.md`, `examples/web-dashboard/main.tb`, `website/src/app/page.tsx`, `website/src/app/docs/examples/page.tsx`
- [ ] Make `web-dashboard` the clear flagship runnable demo
- [ ] Improve demo guidance and public positioning around it
