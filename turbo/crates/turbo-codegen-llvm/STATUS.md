# LLVM Backend — Status & Roadmap

**Branch:** `llvm-backend` (this is the *only* branch where this crate lives).
**Status:** 🚧 Experimental / incomplete. **Not shipped in production.**
**Do not merge to `master` until the "Definition of done" below is fully met.**

---

## Why this lives on a separate branch

The production release line (`master`) ships a **single, fully-validated Cranelift
backend**. The LLVM backend is promising but half-built, so keeping it on `master`
would mean shipping something we can't stand behind. To avoid that, the crate was
**removed from `master`** and parked here, where it can keep being developed at its
own pace and only PR'd back when it is genuinely production-ready.

Policy, in one line: **production releases never contain anything half-built.**

## What exists today

- `aot_compile(module, output_path)` — AOT object emission via `inkwell` 0.5 (`llvm18-0`).
  There is **no JIT path** (Cranelift remains the JIT/`run` backend).
- Wired into the CLI behind an **off-by-default** cargo feature:
  - `turbo-cli/Cargo.toml`: `llvm = ["dep:turbo-codegen-llvm"]`
  - `turbolang build --llvm` (errors cleanly with "rebuild with --features llvm"
    when the feature is off).
- ~6.8K LOC across `lib.rs`, `expr.rs`, `builtins.rs`, `helpers.rs`, `stmt.rs`,
  `types.rs`, `ctx.rs`.

## What is NOT done (the gap to production)

These are the reasons it is not on `master`. Treat as the working TODO list:

1. **Builtin parity with Cranelift.** The Cranelift backend is the source of truth
   for the language surface (str_* family, math_*, hashmap_*, array ops, async
   primitives, COW builtins). The LLVM backend implements only a subset. Every
   builtin Cranelift supports must compile and behave identically here.
2. **Clean, documented build.** `inkwell`/LLVM 18 must build from a documented,
   reproducible setup (the historical zstd link failure with system LLVM 18 needs
   a pinned, written fix). Until then the crate is excluded from the default test
   command (`cargo test --workspace --exclude turbo-codegen-llvm`).
3. **End-to-end `--llvm` validation.** `turbolang build --llvm` must produce a
   correct native binary for the full `turbo/tests/phase1` corpus, byte-for-byte
   matching the Cranelift AOT output's stdout.
4. **A real test suite.** This crate has no `tests/` dir. It needs its own
   integration tests *and* must pass the shared `tests/phase1` corpus under
   `--llvm`. (The website's old "116/116 tests pass" claim was never backed by a
   suite in-tree — do not repeat unverified numbers.)
5. **Measured speedup.** The entire reason to carry a second backend is
   performance. We need a committed benchmark showing LLVM AOT beats Cranelift AOT
   on representative programs, with methodology and date. No speedup → no merge.
6. **CI green.** A CI job that builds with `--features llvm` and runs the corpus.

## Definition of done (all must be true before a PR to `master`)

- [ ] Full builtin parity with the Cranelift backend.
- [ ] Builds cleanly from documented steps on a fresh machine.
- [ ] `turbolang build --llvm` passes the entire `tests/phase1` corpus.
- [ ] Dedicated test suite + corpus run in CI, green.
- [ ] Committed benchmark proving a real speedup over Cranelift AOT.
- [ ] `STATUS.md` updated to "Ready" and this checklist removed.

## How to resume work on this branch

`master` **deleted** this crate, so `master` and `llvm-backend` have diverged on
the codegen tree. To pull the latest language core (parser/sema/runtime fixes,
new builtins) onto this branch:

```bash
git checkout llvm-backend
git merge master
# The merge will try to delete turbo-codegen-llvm and its CLI wiring (that's the
# removal that landed on master). RESOLVE BY KEEPING this crate:
#   - keep turbo/crates/turbo-codegen-llvm/**
#   - keep the "crates/turbo-codegen-llvm" line in turbo/Cargo.toml
#   - keep the llvm feature + optional dep in turbo-cli/Cargo.toml
#   - keep the --llvm wiring in turbo-cli/src/main.rs
git checkout --ours turbo/crates/turbo-codegen-llvm   # if shown as deleted-by-them
# then re-add the workspace member / feature / flag lines as needed, and rebuild.
```

After merging, re-run the Cranelift corpus to confirm the shared core still works,
then continue closing the gaps above under `--llvm`.

---

_Last updated: 2026-06-08. Parked from `master` at commit 3ea90c9 (the last commit
that still contained this crate; the next commit on the production line removed it)._
