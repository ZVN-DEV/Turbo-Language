# Turbo Hardening Audit Archive

Historical note: this file summarizes the 2026-04-06 hardening audit that
fed the v0.5.1 cleanup and security release. It is not the current health
report for the repository.

## Current status

The issues called out in the original audit were used to drive the v0.5.1
release work. The repo now contains the main remediations the audit asked for:

- `CHANGELOG.md` documents the v0.5.1 security backport work.
- `SECURITY.md` defines disclosure scope and response expectations.
- `.github/workflows/ci.yml` now runs fmt, clippy, unit tests, integration
  tests, C runtime tests, and fuzz smoke coverage.
- `turbo/crates/turbo-codegen-cranelift/runtime/tests/test_rt.c` and
  `turbo/crates/turbo-codegen-cranelift/runtime/tests.sh` cover the hardened C
  runtime cases.
- The HTTP runtime now rejects non-HTTP(S) URLs, rejects flag-shaped curl
  inputs, parses `Content-Length` safely, and defaults `http_server(port)` to
  loopback binding.

If you want the current product state, use `README.md`, `CHANGELOG.md`,
`SECURITY.md`, and the live CI workflows rather than the original audit draft.

## What remains true

Some concerns from the audit are still relevant:

- `rt_release` is still effectively non-freeing, so long-running allocation-
  heavy programs remain a known limitation for v0.5.x.
- The Playground executes user code locally and should still be treated as a
  developer convenience, not a hardened multi-tenant sandbox.
- The largest compiler crates are still monolithic and remain a maintenance
  risk even though they are no longer an immediate release blocker.

## Original audit intent

The original audit was a pre-release forcing function, not a permanent root
document. Its purpose was to identify runtime security holes, CI gaps, stale
public artifacts, and launch blockers before the v0.5.1 release. That work is
complete enough that the original "confirmed live exploits" framing is now
misleading if left unqualified.
