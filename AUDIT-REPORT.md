# TurboLang Product + OSS Audit Report
Generated: 2026-05-09
Source: Combined `$product-review` + `$gold-standard-os` review in this Codex session.

## Product Verdict
TurboLang is real software: Rust compiler workspace, CLI, LSP, docs, examples, release engineering, CI, security policy, fuzzing, and runnable demos. Current rating before sprint: 7.1/10. OSS score before sprint: 39-40/50.

## P0/P1 Findings

### P0 / Critical Sprint Blockers
1. **AOT/JIT array parity failure**
   - Command: `./tests/parity/run_parity.sh`
   - Failing program: `turbo/tests/parity/programs/arrays.tb`
   - JIT prints `5,15,1,5`; AOT fails with `runtime error: array index 0 out of bounds (length 0)`.
   - Impact: core compiler correctness issue.

2. **Dependency install path traversal**
   - Files: `turbo/crates/turbo-cli/src/main.rs:503-505`, `877-889`, `934-975`, `1127-1137`.
   - Issue: dependency key is used in `turbo_modules.join(dep.name)` without validating against `..`, separators, or absolute names.
   - Impact: malicious `turbo.toml` can delete/replace files outside `turbo_modules` during install/update.

3. **Wildcard CORS in Turbo HTTP runtime**
   - Files: `turbo/crates/turbo-codegen-cranelift/src/runtime.rs:1417-1424`, `turbo/crates/turbo-codegen-cranelift/runtime/turbo_rt.c:1963-1967`.
   - Issue: typed responses emit `Access-Control-Allow-Origin: *` by default.
   - Impact: arbitrary websites can read local/public Turbo HTTP server responses.

4. **Website vulnerable dependency**
   - File: `website/package.json:12`.
   - `npm audit` reports one high Next.js advisory and one moderate PostCSS advisory. Upgrade Next to patched version.

### P1 / High Trust-Breaking Issues
5. **Formatting drift**
   - `cargo fmt --manifest-path turbo/Cargo.toml --all -- --check` fails.

6. **Parity not PR-gated**
   - CI has parity in nightly only; add PR CI parity job.

7. **Website not root-CI-gated**
   - Add `npm ci`, `npm run lint`, and `npm run build` to root CI.

8. **Version/docs drift**
   - Runtime reports `turbolang 0.8.0`; flagship demo says `v1.0` / JSON `1.0.0`.
   - `SECURITY.md` claims no request body size limits while runtime has limits.

### P2 / Important Follow-ups
9. Add coverage reporting.
10. Add CodeQL/secret scanning/dependency review.
11. Split large files over time.
