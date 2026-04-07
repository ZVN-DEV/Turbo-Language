# Sprint Plan — Turbo v0.5.x Hardening

Generated: 2026-04-06
Based on: AUDIT-REPORT.md (gold-standard-os + product-review, 2026-04-06)
Execution: Single dev agent, sequential, isolated worktree

## Sprint Goal

Take Turbo from "impressive prototype with confirmed live exploits" to "trustworthy v0.5.1 with hardened runtime, gating CI, and clean public-facing surface" — without touching the 6,000-LOC codegen god files or shipping a full ARC (those are P2/separate).

## Success Criteria

- [ ] All 8 P0 items resolved and verified
- [ ] P1 items 10, 11, 12, 13, 15 resolved
- [ ] P1 #9 (full ARC): documented with prominent warning + tracked as follow-up (not shipped this sprint)
- [ ] P1 #14 (Homebrew SHA automation): skipped (cross-repo, out of worktree scope)
- [ ] `cargo test --workspace --exclude turbo-codegen-llvm` still passes
- [ ] `cd turbo && ./tests/run_tests.sh` still passes
- [ ] New CI workflow is valid YAML
- [ ] No regressions in the 19 `phase1` integration tests

## Out of Scope (explicit)

- Splitting the 6,000-LOC god files (P2)
- Shipping real ARC (P1 #9 — too big; document only, plan a follow-up)
- Homebrew SHA automation (cross-repo)
- Closure param inference (P2 — touches sema god file)

## Dev Tracks

### Track 1: Full Hardening — Single Sequential Dev Agent

All tasks below flow through one agent in one isolated worktree, executed in order. This matches the user's explicit instruction: "1 DEV AGENT ONLY going sequentially".

**Files touched:**
- `AUDIT-REPORT.md` (read-only reference)
- `SPRINT-PLAN.md` (read-only reference)
- `CHANGELOG.md` (new)
- `SECURITY.md` (new)
- `.github/workflows/ci.yml`
- `turbo/crates/turbo-codegen-cranelift/runtime/turbo_rt.c`
- `turbo/crates/turbo-codegen-cranelift/runtime/tests/` (new C tests)
- `turbo/crates/turbo-codegen-cranelift/runtime/tests.sh` (new test runner)
- `turbo/fuzz/src/codegen_fuzz.rs` (new codegen fuzz target)
- `turbo/fuzz/src/main.rs` (add codegen mode hook — leave existing modes alone)
- `turbo/crates/turbo-sema/src/suggest.rs` (new, did-you-mean helper)
- `turbo/crates/turbo-sema/src/lib.rs` (minimal: wire did-you-mean into unresolved-name errors only)
- `showcase/getting-started.html` (fix "design phase" string)
- `turbo/crates/turbo-cli/src/playground.html` (fix "Design Phase" footer)
- `README.md` (add prominent runtime-leak warning pointing at the ARC follow-up)

**Files the agent MUST NOT touch:**
- Any file with "LLVM" in its path (that backend is excluded from default-members and unrelated to this sprint)
- `turbo/crates/turbo-codegen-cranelift/src/lib.rs` (god file, out of scope)
- `turbo/crates/turbo-sema/src/lib.rs` — except for the minimal did-you-mean wiring noted above
- `turbo/tests/phase1/**` (these are the regression suite)
- `Cargo.lock` (unless adding a dep is unavoidable — prefer zero new deps)
- `design/**` (language spec — separate concern)
- Memory system at `/Users/macbookpro-kirby/.claude/**`

---

## Task List (execute in order)

### STALE-DOC CLEANUP

**TASK-01 (P0): Remove stale internal planning docs from repo**
- Delete `findings.md` (50 KB, internal Claude audit at v0.3.1)
- Delete `turbo/cow-bug-audit.md` (bugs confirmed fixed)
- Delete `PLAN-2025-03-31.md`
- Delete `INDEX.md`
- Add the patterns to `.gitignore` so they can't accidentally come back:
  ```
  /findings.md
  /PLAN-*.md
  /INDEX.md
  /turbo/cow-bug-audit.md
  ```

**TASK-02 (P0): Fix "design phase" string leaks**
- `showcase/getting-started.html` line ~310: replace "Turbo is currently in design phase — compiler coming soon" with an accurate v0.5.0 status sentence linking to the install instructions
- `turbo/crates/turbo-cli/src/playground.html` line ~453: replace footer "Turbo Language — Design Phase" with "Turbo Language v0.5.0"

### CI HARDENING

**TASK-03 (P0): Add integration tests + concurrency + clippy to CI**
- Edit `.github/workflows/ci.yml`:
  - Add top-level `concurrency: { group: ${{ github.workflow }}-${{ github.ref }}, cancel-in-progress: true }`
  - Add a new `integration` job that:
    - Runs on `ubuntu-latest` and `macos-latest` in a matrix
    - Builds release: `cargo build --release --manifest-path turbo/Cargo.toml`
    - Runs `cd turbo && ./tests/run_tests.sh`
  - Add a new `clippy` job: `cargo clippy --manifest-path turbo/Cargo.toml --workspace --exclude turbo-codegen-llvm --all-targets -- -D warnings`
  - Add a new `fmt` job: `cargo fmt --manifest-path turbo/Cargo.toml --all -- --check`
  - Add a new `c-runtime-tests` job that compiles and runs `turbo/crates/turbo-codegen-cranelift/runtime/tests/` (see TASK-10)
  - Add `actions/cache@v4` for `~/.cargo/registry` and `target` keyed on `hashFiles('turbo/Cargo.lock')` (no `restore-keys`)
- Do NOT add the LLVM backend to CI in this sprint (separate follow-up)

### RUNTIME SECURITY FIXES (turbo_rt.c)

All line numbers below are **approximate** — the agent MUST open the file, grep/search for the function name, and confirm the location before editing. Read the whole function before modifying it.

**TASK-04 (P0): Harden `rt_http_get` / `rt_http_post` against SSRF + argument injection**
- File: `turbo/crates/turbo-codegen-cranelift/runtime/turbo_rt.c` (`rt_http_get` around line 589, and the corresponding `rt_http_post`)
- Before calling `execlp`:
  1. Validate the URL string is non-NULL and non-empty
  2. Reject URLs that do not start with `http://` or `https://` (case-insensitive prefix match). On reject: return an empty string and print an error to stderr with an error prefix like `[rt_http] blocked non-http(s) URL:` — do NOT crash
  3. Pass `--` to curl before the URL argument so `--flag`-looking URLs can't be re-interpreted as flags
  4. Add `--proto`, `=http,https`, `--max-time`, `30`, `--max-redirs`, `5` to the curl argv
- The final `execlp` for GET should look something like:
  `execlp("curl", "curl", "-s", "-L", "--proto", "=http,https", "--max-time", "30", "--max-redirs", "5", "--", url, NULL);`
- Apply the same three changes to `rt_http_post` (preserving its POST-specific flags)
- Do not introduce any new includes beyond what's already in the file unless strictly necessary

**TASK-05 (P0): Fix Content-Length parsing DoS**
- File: same `turbo_rt.c`, inside the HTTP server request parser (grep for `Content-Length` — around line 1126-1168)
- Replace the `atoi(cl + i + 15)` call with `strtoll`:
  - Parse with `strtoll(cl + i + 15, &endptr, 10)`
  - Reject if `errno == ERANGE`
  - Reject if the value is `< 0`
  - Reject if the value is greater than a sane max (define `#define RT_HTTP_MAX_BODY (32 * 1024 * 1024)` near the top of the HTTP section, use that)
  - On any reject: write back `HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n`, close the connection, and continue the accept loop. Do NOT pass a bad length into `memcpy`.
- Include `<errno.h>` and `<limits.h>` if they're not already included

**TASK-06 (P0): Bind HTTP server to 127.0.0.1 by default + add explicit public opt-in**
- File: same `turbo_rt.c`, `rt_http_server` around line 1253 (currently binds `INADDR_ANY`)
- Change `rt_http_server` to bind to `127.0.0.1` (`inet_addr("127.0.0.1")`) by default
- Add a new function `rt_http_server_public(int port)` that binds `INADDR_ANY` — for users who explicitly need external access
- Also:
  - Register `rt_http_server_public` in the JIT symbol table (search for where `rt_http_server` is registered in `turbo/crates/turbo-codegen-cranelift/src/lib.rs` — **only touch the registration/builtin dispatch spot for `rt_http_server_public`, do NOT refactor anything else in that file**)
  - Add it to the sema built-in environment in `turbo/crates/turbo-sema/src/lib.rs` with signature `http_server_public(i64) -> i64` (minimal addition only)
- If wiring the new builtin through sema/codegen turns out to be non-trivial (>30 lines of change in either god file), STOP and instead just change the default bind address. Leave the `_public` helper as a C function only and document the limitation in CHANGELOG.md.

**TASK-07 (P0): Fix `rt_str_repeat` integer overflow**
- File: same `turbo_rt.c`, `rt_str_repeat` around line 421-435
- Before `malloc(len * count + 1)`: check `count < 0`, `len < 0`, and `count != 0 && len > (SIZE_MAX - 1) / count`
- On overflow: return an empty string (consistent with other runtime error paths in this file — confirm by reading nearby functions) and print an error prefix to stderr

**TASK-08 (P0): Add SECURITY.md at repo root**
- New file `SECURITY.md` with:
  - Disclosure channel: ask the user what email they want (default to `security@turbolang.org` as placeholder with a clear TODO comment saying "update this before publishing this file")
  - Supported versions table (v0.5.x: current)
  - Explicit scope: "The Turbo C runtime (`turbo_rt.c`) is in scope. Experimental features flagged in CHANGELOG are out of scope."
  - 48-hour ack SLA, 7-day critical fix target
  - Explicit note: "Turbo's HTTP server primitives are experimental. They are not hardened for untrusted network exposure. Use behind a reverse proxy."

### P1 — FUZZ + C TESTS + CHANGELOG + DYM

**TASK-09 (P1): Extend fuzz coverage to codegen (JIT path)**
- New file `turbo/fuzz/src/codegen_fuzz.rs` that:
  - Imports the existing random-program generator from `turbo/fuzz/src/main.rs` (move it to a shared `mod gen;` if needed — otherwise add a `pub` marker; keep the existing modes working)
  - For N iterations (default 200, override via `TURBO_FUZZ_ITERS`):
    - Generate a random program
    - Lex → parse → sema → if sema-clean, JIT-compile via the Cranelift backend
    - Assert no panic/ICE; timeouts via a wall-clock bound are OK to skip this sprint
  - `panic::catch_unwind` around the compile call so a single crash doesn't kill the corpus
- Add a `fuzz-smoke` job to `.github/workflows/ci.yml` that runs `TURBO_FUZZ_ITERS=50 cargo run --manifest-path turbo/fuzz/Cargo.toml --bin turbo-fuzz -- codegen` (or whatever binary naming matches existing fuzz). If the existing fuzz layout makes adding a second binary hard, add the codegen mode as a subcommand to the existing binary instead — mirror whatever pattern is there.
- Read `turbo/fuzz/src/main.rs` first and follow its conventions

**TASK-10 (P1): C runtime tests**
- New directory `turbo/crates/turbo-codegen-cranelift/runtime/tests/`
- New file `runtime/tests/test_rt.c` — a minimal self-contained test harness (no external test framework; just `assert.h` + a main with numbered cases). Include:
  - `test_str_repeat_overflow`: call `rt_str_repeat` with huge count, assert it returns empty string
  - `test_str_repeat_normal`: `rt_str_repeat("ab", 3) == "ababab"`
  - `test_http_get_rejects_file_scheme`: `rt_http_get("file:///etc/hosts")` must return empty
  - `test_http_get_rejects_flag_injection`: `rt_http_get("--help")` must return empty
  - `test_content_length_negative`: feed `parse_request` a request with `Content-Length: -1`, assert it's rejected (if the parser isn't exposed, fall back to a socket-based integration test — or mark as integration-only with a TODO and ship the overflow + scheme tests)
  - `test_str_concat_basic`: `rt_str_concat("foo", "bar") == "foobar"`
- New file `runtime/tests.sh`:
  - Compiles `test_rt.c` + `turbo_rt.c` with `cc -std=c11 -Wall -Werror`
  - Runs the binary
  - Exits nonzero on failure
- Wire the `c-runtime-tests` CI job from TASK-03 to call this script

**TASK-11 (P1): CHANGELOG.md**
- New file `CHANGELOG.md` using Keep-a-Changelog format
- Backfill entries from recent commits (`git log --oneline | head -30`) covering v0.3, v0.4, v0.5
- Add an `[Unreleased]` section at the top listing everything this sprint adds:
  - Security: hardened `rt_http_get`/`rt_http_post`, fixed Content-Length DoS, default 127.0.0.1 bind, fixed `rt_str_repeat` overflow
  - Added: SECURITY.md, C runtime tests, codegen fuzz target, did-you-mean suggestions, clippy CI gate, integration tests in CI
  - Removed: stale internal planning docs from repo
  - Known issues: **runtime memory leak — `rt_release` is currently a no-op; long-running servers will grow. Real reference counting is planned for v0.6.**

**TASK-12 (P1): Did-you-mean suggestions for unresolved names**
- New file `turbo/crates/turbo-sema/src/suggest.rs`:
  - Implement plain Levenshtein (no crate) with an early-exit cap at distance 3
  - Export `fn closest<'a>(needle: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<&'a str>`
- In `turbo/crates/turbo-sema/src/lib.rs`: find the **one** place where unresolved-identifier errors are produced (grep for the existing E-code — likely E0201 or E0202 "unknown identifier" / "undefined variable"). Modify only that message-construction site to append `. Did you mean '{suggestion}'?` when a close match exists in the current scope.
- DO NOT refactor surrounding code. DO NOT rename anything. The diff to `sema/lib.rs` should be under 20 lines.
- Add one integration test at `turbo/tests/phase1/did_you_mean.tb` + `.expected` with `ERROR:Did you mean`

### DOCS

**TASK-13 (P1): README runtime-leak warning + ARC follow-up tracker**
- Edit `README.md` (read it first — if it doesn't already have a "Known Limitations" or "Status" section, add one near the top, right after the install instructions)
- Add a prominent callout:
  > **⚠️ Known limitation (v0.5.x):** The runtime does not yet perform reference counting — `rt_release` is a no-op. Long-running servers and hot loops that allocate repeatedly will leak memory (~2.5 KB/request on the example HTTP server). Real ARC is planned for v0.6. For short-running CLI programs this is not a problem.
- If a `docs/` or `TODO.md` tracker exists, add an entry there. Otherwise do not create a new tracker file.

---

## Verification Steps (agent must run these before committing)

1. `cargo build --manifest-path turbo/Cargo.toml` — must succeed
2. `cargo test --workspace --exclude turbo-codegen-llvm --manifest-path turbo/Cargo.toml` — must pass
3. `cargo build --release --manifest-path turbo/Cargo.toml` — must succeed
4. `cd turbo && ./tests/run_tests.sh` — all 19+ tests must pass (new did-you-mean test is the 20th)
5. `cd turbo/crates/turbo-codegen-cranelift/runtime && bash tests.sh` — must pass
6. Quick manual repro of the old exploits — agent should build and confirm:
   - `http_get("file:///etc/hosts")` now returns empty
   - `http_get("--help")` now returns empty
   - `http_server(8080)` now binds 127.0.0.1 only (check with `lsof` or `ss`)
7. `git status` before commit — no accidental edits to LLVM backend, codegen god file beyond minimal additions, design docs, or memory system

## Commit Strategy

One commit per logical group, in this order (so history is bisectable):
1. `chore: remove stale internal planning docs`
2. `docs: fix "design phase" strings + add runtime leak warning`
3. `ci: add integration tests, clippy, fmt, concurrency control`
4. `security: harden rt_http_get/post against SSRF + arg injection`
5. `security: fix Content-Length DoS in http server`
6. `security: default bind 127.0.0.1 + rt_str_repeat overflow fix`
7. `docs: add SECURITY.md + CHANGELOG.md`
8. `test: C runtime tests`
9. `test: codegen fuzz target`
10. `feat(sema): did-you-mean suggestions for unknown identifiers`

Each commit message should reference the TASK-NN numbers it closes.

## Manual Actions for the User (outside this sprint)

- Replace the placeholder `security@turbolang.org` in `SECURITY.md` with a real disclosure address
- Review the CHANGELOG `[Unreleased]` section before cutting v0.5.1
- Follow up in a separate sprint: real ARC (P1 #9), splitting god files (P2), LLVM backend in CI (P2), Homebrew SHA automation (P2)
