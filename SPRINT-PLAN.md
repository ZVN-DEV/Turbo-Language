# Sprint Plan — TurboLang Product Hardening Sprint
Generated: 2026-05-09
Based on: `AUDIT-REPORT.md` from combined product-review + gold-standard-os findings.

## Sprint Goal
Close the trust-breaking correctness, security, CI, dependency, and copy drift findings so TurboLang is safer to promote as a serious pre-1.0 language project.

## Success Criteria
- [x] AOT/JIT parity suite passes, including `arrays.tb`.
- [x] Dependency names cannot escape `turbo_modules` during install/update.
- [x] Symlinked `turbo_modules` directories are rejected instead of canonicalizing outside the project root.
- [x] Turbo HTTP typed responses no longer emit wildcard CORS by default.
- [x] Website dependency audit is clean or materially reduced by upgrading Next.
- [x] Rust formatting check passes.
- [x] PR CI gates parity tests, website lint/build, and high-severity npm audit.
- [x] Stale v1.0 demo/docs claims corrected to current 0.8.x status.
- [x] Final targeted tests/builds pass.

## Dev Tracks

### Track 1: Core Runtime + Codegen Correctness — Debugger/Executor
**Files touched:**
- `turbo/crates/turbo-codegen-cranelift/src/**`
- `turbo/crates/turbo-codegen-cranelift/runtime/turbo_rt.c`
- `turbo/crates/turbo-codegen-cranelift/runtime/tests/**`
- `turbo/crates/turbo-codegen-llvm/src/lib.rs`
- `turbo/tests/parity/**`

**Tasks:**
- [x] TASK-01 (P0): Fix AOT/JIT array parity failure for `turbo/tests/parity/programs/arrays.tb`.
- [x] TASK-02 (P0): Remove default wildcard `Access-Control-Allow-Origin: *` from Rust and C typed HTTP responses.
- [x] TASK-03 (P1): Add/adjust regression coverage for CORS behavior and array parity where practical.
- [x] TASK-04 (P2): Replace predictable AOT/LLVM temp directories with exclusive temp dirs if low-risk.

### Track 2: CLI Dependency Security — Security Executor
**Files touched:**
- `turbo/crates/turbo-cli/src/main.rs`
- CLI tests in the same file/module only if needed

**Tasks:**
- [x] TASK-05 (P0): Validate dependency names parsed from `turbo.toml`.
- [x] TASK-06 (P0): Ensure install/update targets remain inside canonical `turbo_modules`.
- [x] TASK-07 (P1): Add tests for rejecting `../`, path separators, absolute paths, empty names, and accepting normal names.
- [x] TASK-07B (P0): Reject symlinked `turbo_modules` directories.

### Track 3: Website, CI, and Messaging — Product/Infra Executor
**Files touched:**
- `website/package.json`
- `website/package-lock.json`
- `.github/workflows/ci.yml`
- `README.md`
- `SECURITY.md`
- `examples/web-dashboard/main.tb`
- `examples/speed-server/main.tb`, `examples/speed-server/README.md` if needed

**Tasks:**
- [x] TASK-08 (P0): Upgrade Next.js to patched version and verify website build/audit.
- [x] TASK-09 (P1): Add PR CI jobs for website lint/build and parity tests.
- [x] TASK-09B (P1): Add high-severity `npm audit` gate to website CI.
- [x] TASK-10 (P1): Correct stale v1.0 demo/API copy to 0.8.x/current language status.
- [x] TASK-11 (P1): Correct stale `SECURITY.md` HTTP body-limit text.
- [x] TASK-12 (P1): Fix stale README test-count/status copy if needed.
- [x] TASK-13 (P2): Ignore local `.claude/` tooling artifact.

## Verification Evidence
- `cargo fmt --manifest-path turbo/Cargo.toml --all -- --check`
- `cargo clippy --manifest-path turbo/Cargo.toml --workspace --exclude turbo-codegen-llvm --all-targets -- -D warnings`
- `cargo test --workspace --exclude turbo-codegen-llvm --manifest-path turbo/Cargo.toml`
- `bash tests.sh` in `turbo/crates/turbo-codegen-cranelift/runtime`
- `./scripts/check_error_codes.sh`
- `cargo audit --deny warnings` in `turbo`
- `npm run lint && npm run build && npm audit --audit-level=high` in `website`
- `cargo build --release --workspace --exclude turbo-codegen-llvm --manifest-path Cargo.toml` in `turbo`
- `./tests/run_tests.sh` in `turbo` — 193 passed, 0 failed, 10 skipped
- `./tests/parity/run_parity.sh` in `turbo` — 11 passed, 0 failed

## Intentionally Deferred
- Coverage reporting, CodeQL/secret scanning, and large-file decomposition are important but not required to close the immediate P0/P1 trust blockers.
- Local LLVM backend validation was not rerun because this machine lacks LLVM 18; CI has a dedicated LLVM job.
- `npm audit --audit-level=high` passes after the Next upgrade; npm still reports moderate PostCSS advisories inherited through Next.
