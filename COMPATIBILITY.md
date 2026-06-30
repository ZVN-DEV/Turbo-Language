# Compatibility and Stability

Turbo is pre-1.0. This document describes what today's `0.10.x` releases
guarantee, what is still fluid, and what the `1.0` stability contract
will mean when we cut it. It is a contract, not a marketing pitch — if
you are shipping production code against a pre-1.0 Turbo you should
read this in full.

## Current Status: Pre-1.0

> Until `1.0`, expect breaking changes across minor releases.
> Pin `turbolang --version` and vendor your toolchain for production.

We are intentionally dwelling in the `0.x` series for a long while,
cutting many point releases, rather than racing to `1.0`. That means:

- **Point releases (`0.10.0` → `0.10.1`)** — additive, bug fixes, no
  syntactic breaking changes. Safe to auto-update in CI if you re-run
  your test suite.
- **Minor releases (`0.10.x` → `0.11.0`)** — may break syntax, remove
  deprecated builtins, reshape the stdlib, or renumber error codes in
  the `E05xx` range. Read the CHANGELOG before upgrading.
- **Major release (`1.0`)** — the stability contract below kicks in.

## What `1.0` Will Guarantee

These become load-bearing promises the moment `1.0` ships. They are
not promises today.

- **Syntactic compatibility for the documented core.** Programs that
  compile against `1.0.0` will compile on every `1.x.y` release
  without source edits. "Documented core" means syntax covered in
  `design/SYNTAX.md` and surfaced in the reference docs — experimental
  flags and unstable attributes are excluded.
- **SemVer on the standard library.** Public stdlib APIs follow
  semantic versioning. Additions in minor releases, breaking changes
  only at the next major. Items explicitly marked `#[unstable]` are
  exempt.
- **No silent ABI churn for FFI.** The C ABI Turbo binaries expose to
  `extern "C"` callers, and the layout of `extern`-declared structs,
  is stable within a major version. Changes require a major bump and
  a migration note.
- **Error codes below `E0200` are stable.** Numbers do not get reused,
  do not get deleted, and do not change meaning. Explanation text may
  be rewritten for clarity.
- **Tier-1 platform support.** Every `1.x` release builds and passes
  the integration suite on all tier-1 platforms (see below).

## What Is Explicitly Fluid in `0.10.x`

Do not build load-bearing production code against these unless you
are prepared to update it on every minor release.

- **The set of builtin functions.** `push`, `map`, `str_*`,
  `hashmap_*`, `math_*`, etc. may be renamed, grouped under modules,
  or replaced with method syntax. The end state is a proper stdlib;
  today's names are provisional.
- **Error codes at `E0500` and above.** These may be renumbered,
  split, or merged. Codes below `E0200` are already treated as stable
  in practice and will lock in at `1.0`.
- **Stdlib organization.** Expect module boundaries and namespacing
  to change as the stdlib takes shape. Function behavior tends to be
  stable; import paths do not.
- **Runtime binary layout.** Anything internal to `turbo_rt.c` — the
  string representation, array header, hashmap layout, async task
  block — is an implementation detail. Linking against it from
  non-Turbo code is not supported.
- **REPL and playground APIs.** The command set, embedding surface,
  and serialization formats of `turbolang repl` and
  `turbolang playground` are unstable and may be redesigned.
- **LSP wire behavior.** Diagnostics, hover contents, and completion
  items may gain or lose fields between minor releases. The editor
  extensions are updated in lockstep.
- **The experimental WASM target.** The `--target wasm` backend is
  flagged experimental in `CHANGELOG.md`; it may be reshaped between
  minor releases.

## Platform Support Tiers

| Tier | Platforms | Guarantee |
|------|-----------|-----------|
| **Tier 1** | macOS `arm64`, macOS `x86_64`, Linux `x86_64` | Built and tested on every release. Release artifacts shipped. Bugs block release. |
| **Tier 2** | Linux `arm64` | Cross-compile target only — `--target linux-arm64` emits a valid ARM64 ELF, but it is not yet runtime-validated and no release artifact is shipped. Bugs tracked but do not block release. |
| **Experimental** | WASM, Windows (`x86_64`) | Best effort. No release artifacts yet. Breakage between releases is allowed. |

Tier assignments are reviewed each minor release. A platform moves up
a tier only after it has been stable across at least two consecutive
minor releases.

## Deprecation Policy (Post-1.0)

Once `1.0` ships, removing or renaming a stable API follows a
three-step cycle:

1. **Deprecate.** The item is marked `#[deprecated]` in a minor
   release. Using it emits a compile-time warning with a pointer to
   the replacement. The item continues to work identically.
2. **Warn loudly.** In the next minor release, the warning escalates
   (louder phrasing, linked migration note). The item still works.
3. **Remove.** The item is removed in the next major release (never
   earlier). No stable API is ever removed without going through
   steps 1 and 2.

Security-sensitive removals (cryptographic APIs with known flaws, for
example) are the one documented exception. Those follow the
disclosure policy in `SECURITY.md`.

## What This Means in Practice

- **If you are experimenting.** Track the latest `0.10.x`. Re-run your
  tests on every bump. File issues.
- **If you are shipping something today.** Pin an exact Turbo version
  (`turbolang --version` in CI). Vendor the install script or the
  Homebrew formula so your build is reproducible. Budget time on
  every minor release to re-read the CHANGELOG and migrate.
- **If you are waiting for stability.** `1.0` is the right signal to
  start. Until then, anything here can move.

See also: `SECURITY.md` (release verification + supported versions),
`CHANGELOG.md` (per-release breaking changes), `design/ROADMAP.md`
(feature roadmap), `docs/errors/` (error code reference).
