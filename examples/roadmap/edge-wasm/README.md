# edge-wasm (TurboEdge) — Roadmap Spec

> **Status: aspirational.** This example is a design document, not runnable
> code. It uses syntax and toolchain features that are not yet implemented
> in the current Turbo compiler. See `BRIEFING.md` for the full design
> write-up. Tracked under the P3 backlog.

An image processing service that compiles to WebAssembly and runs on CDN edge
nodes (Cloudflare Workers, Vercel Edge Functions, Deno Deploy, Fastly
Compute, any WASI-compatible runtime). Used as the canary for "Turbo as a
serious WASM target with cold-start times measured in microseconds and
binary sizes measured in single-digit MB."

## What this example would demonstrate

- A single Turbo codebase producing both a native binary (for local
  development) and a `wasm32-wasi` binary (for production edge deployment)
- `@wasm_export` decorators marking functions as entry points callable from
  the host edge runtime
- Streaming image transform pipelines: resize, sharpen, format conversion
- SIMD intrinsics enabled via `simd = true` in `turbo.toml` for the inner
  convolution loops
- A `from`-style import system that does not yet exist in the parser
- Edge-cache integration via the host runtime's cache API
- Sandboxed execution with hard memory caps

## Run

This example does not currently run. The WASM backend is not yet wired into
the CLI for this manifest format, and the source uses syntax the parser
does not accept.

Once the language and toolchain reach the milestones described in
`BRIEFING.md`, the intended workflows will be:

```bash
# Local development (native target)
turbolang run examples/roadmap/edge-wasm/src/main.tb

# Production build (wasm32-wasi target)
turbolang build --target wasm32-wasi examples/roadmap/edge-wasm
```

## Expected output

A `.wasm` binary deployable to any WASI-compatible edge runtime, plus a
local HTTP server for development that responds to URLs like:

```
GET /image/resize/800x600/sharpen/0.5/format/webp?url=https://cdn.example.com/photo.jpg
```

## Caveats

- **Aspirational.** Do not edit this code expecting it to compile. The
  syntax intentionally runs ahead of the compiler.
- **For P3 follow-up.** When the WASM backend, `from` imports, SIMD
  intrinsics, and cache/HTTP host bindings are all in place, this example
  should be revisited and brought back into the runnable set.
- The Turbo codegen-cranelift crate already has a `wasm_codegen.rs`
  module, but it does not yet expose the surface area this example
  assumes.
