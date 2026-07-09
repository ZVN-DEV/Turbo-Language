# Reach Roadmap — Serverless, WASM, and Interop

> **Status: Planned (sequencing doc, 2026-07-09).** This turns the strategy in
> `POLYGLOT.md` and the serverless positioning discussion into a concrete,
> ordered plan. "Reach" means two things: where a Turbo program can *run*
> (serverless, edge, browser) and what it can *call* (C, Rust, and the hosts
> that embed it). Version targets assume the current cadence continues from
> v0.14.0; they are intents, not promises.

## Guiding decisions (already made — do not relitigate)

1. **No npm/PyPI package compat layer.** Running Node or Python packages
   requires embedding V8/CPython (kills the no-runtime identity) or
   reimplementing their semantics (infeasible; see `POLYGLOT.md` "Why Not Full
   Transpilation"). The inversion ships instead: Turbo callable *from* Node and
   Python via `libturbo` wrappers.
2. **C FFI is the one compat layer that works.** Every ecosystem unlock in this
   plan routes through the C ABI — including Rust (no stable ABI of its own).
3. **TLS/HTTP2 are deliberately last.** Serverless platforms and reverse
   proxies terminate TLS; the built-in server speaking plain HTTP/1.1 on
   localhost is the industry-standard contract. TLS ships post-1.0 via rustls,
   never hand-rolled. HTTP/2 only after TLS, and only if gRPC/direct-edge
   demand materializes.
4. **WASM completion is the single biggest strategic bet.** It serves three
   goals at once: edge serverless (Workers/Fastly deploy WASM, not native
   binaries), sandboxed execution of untrusted `.tb` code (fixes the
   "treat `.tb` like executables" caveat and lets the playground run
   client-side), and the Component Model as the long-term cross-language
   interop story.
5. **No benchmark ships without a reproducible script in-repo.** Cold-start and
   throughput claims are central to this positioning; every number published
   must be regenerable by `benchmarks/` tooling.

---

## Phase 0 — Serverless on-ramps (v0.14.x, docs-and-glue, ~days)

Cheapest possible test of the serverless market claim. No compiler changes.

| # | Deliverable | Notes |
|---|-------------|-------|
| 0.1 | `examples/deploy/cloud-run/` | Multi-stage Dockerfile building `examples/http-sqlite-api` (or cross-compiled `--target linux-x86` artifact), `PORT` env handling, README with `gcloud run deploy` walkthrough. |
| 0.2 | `examples/deploy/fly/` | `fly.toml` + Dockerfile + README for the same app. |
| 0.3 | `turbo-lambda` package | Pure-Turbo AWS Lambda custom-runtime adapter: a `bootstrap` loop polling the Lambda Runtime API with the existing `http_get`/`http_post` client, dispatching to a user handler `fn(str) -> str`. Ships in `packages/` with an example function + SAM/Terraform snippet. |
| 0.4 | Cold-start benchmark | Reproducible script comparing Turbo vs Node vs Python on Lambda and Cloud Run (init duration, memory floor). Publish only script-generated numbers. |
| 0.5 | `docs/serverless.md` | Ties 0.1–0.4 together; linked from README "What Turbo is for". |
| 0.6 | `POLYGLOT.md` update | Add explicit "npm/PyPI package compat: refused" entry with the two-paths reasoning, so the refusal is recorded, not just folklore. |

**Acceptance:** each example deploys from a fresh clone by following its README
verbatim; benchmark numbers regenerate from the script.

**Why first:** validates the market thesis for everything below at near-zero
cost, and gives later phases (WASM edge, event loop) real usage data to
prioritize against.

## Phase 1 — HTTP/1.1 completeness (v0.15)

The platform contract work that actually matters for "serverless Node-type"
workloads (serverless platforms' local proxies exercise these paths hard).
Explicitly *not* TLS/HTTP2.

- Keep-alive correctness audit: connection reuse, `Connection: close`
  honoring, half-close handling, FD-leak soak test.
- Chunked transfer-encoding: decode on requests, encode on responses;
  streaming/large-body handling without full buffering where feasible.
- Edge-case hardening: `Content-Length` vs `Transfer-Encoding` conflicts
  (request-smuggling class), header casing, `Expect: 100-continue`.
- Conformance harness in CI: soak the server behind a real proxy config
  (nginx/Caddy from `docs/production-server.md`) plus a load tool run.

**Acceptance:** conformance harness green in CI; documented behavior matrix in
`docs/production-server.md`; no FD/memory growth over a multi-hour soak.

## Phase 2 — C FFI ergonomics + bindgen, Rust recipe (v0.15–v0.16)

The ecosystem unlock. Goal: Zig-grade "just call the C library" experience.

- **Audit + spec** current `extern`/FFI support (what exists today vs
  `POLYGLOT.md` Tier 1 sketch); write the gap list before building.
- **`turbolang bindgen <header.h>`**: generates Turbo `extern` declarations
  from a C header. Scope honestly: an allowlist/denylist filter for symbols is
  acceptable; full arbitrary-header fidelity is not required to be useful.
- **Three flagship binding packages** in `packages/`: `turbo-postgres`
  (libpq), `turbo-zlib`, `turbo-openssl-crypto` (hashing/HMAC only — *not*
  TLS). Each proves bindgen on a real header and fills a real stdlib gap.
- **`docs/ffi.md`**: memory-ownership rules across the boundary (who frees
  what, ARC interaction), safety guidance, bindgen usage.
- **`docs/rust-interop.md`**: the recipe — crate → `cdylib` +
  `#[no_mangle] extern "C"` shims → cbindgen header → `turbolang bindgen` →
  Turbo package. One worked example wrapping a small, high-value crate.

**Acceptance:** all three flagship bindings build and pass integration tests on
Tier 1 platforms via `turbolang install <name>`; the Rust guide's example works
from a fresh clone.

## Phase 3 — Turbo as an extension language (v0.16)

The inversion of the compat-layer idea: bring Turbo *to* the Node/Python
ecosystems via `libturbo`. This is how Rust actually infiltrated both (napi-rs,
PyO3) — "write your hot path in Turbo, keep your ecosystem."

- **`@turbolang/node`**: napi module wrapping `libturbo` — load a `.tb`
  module, call exported functions from JS. Marshal `int`/`float`/`bool`/`str`
  directly; arrays/structs cross as JSON initially (upgrade path: typed
  marshaling).
- **`turbolang-py`**: CPython extension module with the same surface,
  pip-installable (maturin or plain C-extension build).
- **Demo + benchmark each**: a hot-loop function (e.g. text scoring or numeric
  crunch) called from Node/Python, with the reproducible-script rule applied.
- Document both in `docs/libturbo.md`.

**Acceptance:** `npm install` / `pip install` from the examples works on Tier 1
platforms; demos show the FFI round-trip cost honestly (including where staying
in JS/Python wins).

## Phase 4 — WASM completion + WASI (v0.17–v0.18, the big bet)

Promote WASM from Experimental toward Tier 2. Two workstreams, runtime first.

1. **Decision spike (timeboxed ~1 week):** compile `turbo_rt.c` via wasi-libc
   vs implement a WASM-side runtime. Deliverable is a written decision in
   `design/` with a proof-of-concept for strings + arrays, since everything
   else stacks on this.
2. **Runtime port** per the spike decision: strings, arrays, hashmap, ARC
   release paths, JSON.
3. **Close the expression gaps** currently hard-erroring in
   `wasm_codegen.rs`: generic `HashMap<K,V>`, function values inside
   arrays/maps, remaining closure-argument forms, catch-all expr kinds.
   Driver: run the full `tests/phase1/` suite against the wasm target with a
   parity harness; the failing list *is* the backlog.
4. **WASI Preview 2** target + `wasmtime` as the CI test runner; documented
   builtin-availability matrix (OS-only builtins like `exec` stay native-only).
5. **JS interop layer** (`POLYGLOT.md` Tier 1): `extern "js"` imports,
   `@wasm_export`, generated `.d.ts`, `turbolang build --target wasm --npm`.
6. **Client-side playground:** the website playground executes user code in
   the browser via the WASM build — removes the server-side runner
   infrastructure (and its sandboxing/prlimit burden) entirely.
7. **Edge deploy examples:** Cloudflare Workers + Fastly Compute templates,
   closing the loop with Phase 0's serverless positioning.

**Acceptance:** phase1 suite green under wasmtime (documented exclusions
only); playground runs client-side in production; one Workers example deployed
and linked from `docs/serverless.md`; WASM tier promotion criteria per
`COMPATIBILITY.md` (stable across two consecutive minor releases) begins.

**Component Model note:** WIT/component interop (`POLYGLOT.md` Tier 1 item)
follows *after* core WASI stability — it's the long-term cross-language story,
not a prerequisite for edge deploys.

## Phase 5 — Concurrency ceiling: async I/O (v0.19+)

Unlocks the self-hosted "real web server" segment (thousands of concurrent
connections). Deliberately after WASM because serverless platforms cap
per-instance concurrency anyway — thread-per-connection is not the bottleneck
for the Phase 0–4 market.

- Design doc first (extend `design/CONCURRENCY.md`): kqueue/epoll-backed
  accept + I/O loop for the HTTP server, preserving today's `spawn`/channel
  semantics for user code. Decide explicitly whether this is a server-internal
  change (cheap, likely first) or a general async runtime (expensive, maybe
  never — write down which).
- Implement the server-internal variant; keep thread-per-`spawn` for user
  tasks.
- Soak/benchmark: 10k idle connections, sustained RPS, flat RSS — reproducible
  scripts in `benchmarks/`.

**Acceptance:** benchmark targets hit on Tier 1 platforms; README's "honest
caveats" concurrency paragraph rewritten to the new truth.

## Phase 6 — TLS, then HTTP/2 (post-1.0)

- **TLS via rustls** behind a small `extern "C"` shim crate (we already build
  Rust), exposed as opt-in `http_config` settings. Serves the "single binary,
  zero infra" story (the Caddy appeal). Never hand-rolled crypto; SECURITY.md
  threat model updated.
- **HTTP/2** only after TLS, and only if gRPC or direct-edge-exposure demand
  shows up in real usage. Otherwise it stays parked.

## Sequencing at a glance

| Phase | Target | Size | Depends on | Primary payoff |
|-------|--------|------|------------|----------------|
| 0. Serverless on-ramps | v0.14.x | S (days) | — | Tests the market thesis; deployable today |
| 1. HTTP/1.1 completeness | v0.15 | M | — | Platform-proxy correctness for serverless |
| 2. C FFI + bindgen + Rust recipe | v0.15–0.16 | M–L | — | Ecosystem unlock (C + Rust) |
| 3. libturbo Node/Python wrappers | v0.16 | M | libturbo (shipped) | Adoption wedge into JS/Python worlds |
| 4. WASM + WASI completion | v0.17–0.18 | XL | Spike decision | Edge serverless, sandboxing, browser playground |
| 5. Async I/O event loop | v0.19+ | L–XL | Design doc | Self-hosted high-concurrency servers |
| 6. TLS (rustls), then HTTP/2 | post-1.0 | M / L | Phase 1 | Zero-infra direct exposure; gRPC (maybe) |

Phases 0–3 are independent of each other and parallelizable; Phase 4 is the
long pole and can start (spike + runtime decision) while 1–3 land; 5 and 6 are
strictly later.

## Explicit non-goals

- npm/PyPI package compatibility or transpilation (see Guiding decision 1).
- Embedding V8, Node, or CPython into Turbo binaries.
- A general-purpose async runtime for user code in Phase 5 unless the design
  doc argues for it — the server-internal loop is the default plan.
- Hand-rolled TLS, ever.
- GUI toolkits, JVM interop, and everything in `POLYGLOT.md` Tier 3.
