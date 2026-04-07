# Turbo v0.5.0 — Audit Report

Source: Combined product-review + gold-standard-os audit (2026-04-06)
Score: 25/50 (gold-standard-os) / 6.5/10 (product-review)

## Confirmed live exploits (hands-on via speed-server)

| # | Finding | File:Line | Repro |
|---|---------|-----------|-------|
| S1 | SSRF via `file://` scheme | `turbo/crates/turbo-codegen-cranelift/runtime/turbo_rt.c:589` (`rt_http_get`) | `http_get("file:///etc/hosts")` returned hosts contents |
| S2 | Curl argument injection | same | `http_get("--help")` returned 13,430 bytes of curl help text — any flag injectable |
| S3 | Content-Length DoS (negative) | `turbo_rt.c:~1126-1168` — `atoi(cl + i + 15)` then `memcpy` with SIZE_MAX | `printf 'POST / HTTP/1.1\r\nHost: x\r\nContent-Length: -1\r\n\r\n' \| nc 127.0.0.1 8080` killed PID instantly |
| S4 | `INADDR_ANY` default bind | `turbo_rt.c:1253` — `http_server(port)` | `lsof -iTCP:8080` → `*:http-alt (LISTEN)` |
| S5 | Heap leak per request | `turbo_rt.c:1399-1408` — `rt_release` no-op TODO | `leaks` reported 3,405 ROOT LEAKs / 252,960 bytes after 100 /api/sort requests (~2.5 KB/req) |
| S6 | Integer overflow in `rt_str_repeat` | `turbo_rt.c:421-435` — unchecked `len * count` | Large counts wraparound malloc |
| S7 | Playground no sandbox | `turbo/crates/turbo-cli/src/playground.rs:67-126` | Fixed `/tmp/playground.tb`, CORS `*`, no timeout |

## CI/CD gaps

- `.github/workflows/ci.yml` runs `cargo test --workspace --exclude turbo-codegen-llvm` only
- **Integration tests (`tests/run_tests.sh`) are NEVER executed in CI**
- No `concurrency: cancel-in-progress` → stale runs consume minutes
- No clippy gate (despite one-time cleanup in `b4204df`)
- LLVM backend completely unexecuted → can silently rot
- No build caching → cold compiles on every run
- No fuzz smoke test
- No matrix across macOS + Linux for C runtime

## Testing gaps

- Fuzz harness (`turbo/fuzz/src/main.rs`, 618 LOC) covers lexer/parser/sema but **not codegen** — the layer most likely to ICE
- `turbo_rt.c` (1,415 LOC) has **zero automated tests**
- 341 combined `.unwrap()`s across the two codegen backends → latent ICE surface

## Tech debt / stale artifacts in repo

- `findings.md` (50 KB) — internal Claude-authored DX audit from 2026-04-04 at v0.3.1, leaking into public repo
- `turbo/cow-bug-audit.md` — lists 20 P0 bugs that **no longer reproduce** (verified: push inside if/for both return correct len)
- `PLAN-2025-03-31.md` — stale planning doc
- `INDEX.md` — stale index
- `showcase/getting-started.html:310` — still reads "Turbo is currently in design phase — compiler coming soon" (project is at v0.5.0)
- `turbo/crates/turbo-cli/src/playground.html:453` — footer "Turbo Language — Design Phase"

## DX gaps

- Closure param inference fails: `xs.reduce((a, b) => a + b, 0)`
- No "did you mean" suggestions: `lenght(x)` doesn't suggest `len`
- No CHANGELOG.md despite clean version history
- Memory leaks make long-running servers impractical

## Code organization

- `turbo/crates/turbo-codegen-cranelift/src/lib.rs` — 6,549 LOC, 135 unwraps
- `turbo/crates/turbo-codegen-llvm/src/lib.rs` — 6,464 LOC, 206 unwraps
- `turbo/crates/turbo-sema/src/lib.rs` — 5,976 LOC
- (Splitting deferred to P2; not in this sprint)

## Documentation

- Strong: `CLAUDE.md`, `docs/errors.md`, design docs
- Missing: `SECURITY.md` with disclosure process, `CHANGELOG.md`

## What's verified GOOD (don't regress these)

- E-code system E0001-E0521 with ariadne rendering and `turbolang explain`
- 19/19 `phase1` integration tests pass via `turbolang run`
- Clean signal handling (SIGTERM), SO_REUSEADDR set, clean rebind
- COW array bugs from old `cow-bug-audit.md` are fixed
- Release profile has lto+strip+codegen-units=1
- Homebrew tap + install.sh + release workflow work
