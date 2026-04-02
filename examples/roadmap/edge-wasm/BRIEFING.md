# TurboEdge: Image Processing at the Edge via WASM

## What This Is

TurboEdge is an image processing service that compiles to WebAssembly and runs on CDN edge nodes. A single Turbo codebase produces a WASM binary that deploys to Cloudflare Workers, Vercel Edge Functions, or Deno Deploy -- the same code also runs natively for local development and testing.

Users request image transformations via URL:
```
GET /image/resize/800x600/sharpen/0.5/format/webp?url=https://cdn.example.com/photo.jpg
```

The service fetches the source image, applies transforms as a streaming pipeline, caches the result at the edge, and returns the processed image -- all within milliseconds, running in a WASM sandbox mere kilometers from the user.

---

## Why WASM for Edge Computing

**Deploy anywhere.** WASM is a universal compilation target. The same `.wasm` binary runs on Cloudflare's network (330+ cities), Vercel's edge (dozens of regions), Deno Deploy, Fastly Compute, or any WASI-compatible runtime. No vendor lock-in.

**Sandboxed by design.** Each WASM instance gets its own linear memory with hard limits. A malicious or buggy image can't escape the sandbox, can't access the filesystem, can't make unauthorized network calls. The edge runtime enforces memory caps (typically 128-256MB) and execution timeouts.

**Near-native performance.** WASM executes at 80-95% of native speed. With SIMD instructions enabled (`simd = true` in `turbo.toml`), image convolutions approach hardware limits. The startup time is sub-millisecond -- orders of magnitude faster than spinning up a container or a V8 isolate running JavaScript.

**Tiny binaries.** A Turbo WASM binary for TurboEdge is typically 2-4MB stripped. Compare this to a Go WASM binary (10-30MB) or a full Node.js runtime. Smaller binaries mean faster cold starts and less memory pressure.

---

## How Turbo Compiles to WASM

Turbo treats WASM as a first-class compilation target. The same source code compiles to both native (for development) and `wasm32-wasi` (for production edge deployment):

```toml
[build]
target = "wasm32-wasi"
opt-level = "release"
strip = true
lto = true
```

The `@wasm_export` decorator marks functions as entry points callable by the edge runtime:

```
@wasm_export("handle_request")
pub async fn wasm_handle_request() -> () {
  let raw = host_get_request()
  let request = Request.from_raw(raw)
  // ... process and respond
}
```

The `@wasm_import` decorator brings in host functions (the edge runtime's fetch, KV store, console):

```
@wasm_import("env", "fetch")
extern async fn host_fetch(url: str, options: FetchOptions) -> FetchResponse
```

This means the same Turbo code works in both environments:
- **Native:** `turbolang run` -- uses the OS network stack, filesystem, etc.
- **WASM:** `turbolang build --target wasm32-wasi` -- imports host functions from the edge runtime.

No `#[cfg(target_arch = "wasm32")]` conditional compilation needed. The compiler resolves imports at link time.

---

## The Streaming Pipeline Pattern

TurboEdge's key architectural decision: **never load the full image into memory.**

A 4000x3000 RGBA image is 48MB uncompressed. On a WASM instance with a 128MB memory budget, that's already 37% of available memory -- before any processing. Loading two copies (input + output) would exceed the budget.

Instead, TurboEdge processes images as streams of row chunks (typically 64 rows at a time, ~1MB per chunk for a 4K-wide image):

```
let result = await process_image("https://cdn.example.com/hero.jpg")
  |> resize(800, 600)
  |> sharpen(0.5)
  |> convert(.webp)
  |> execute(executor, context)
```

Each `|>` step is a stream transformer. `resize` reads chunks from upstream, interpolates rows, and emits resized chunks downstream. `sharpen` maintains a small sliding window of rows for its convolution kernel. `convert` re-encodes chunks in the target format.

The result: a 100MB source image can be processed with only 2-3MB of working memory. The chunks flow through the pipeline and are freed as soon as the next stage consumes them.

This is possible because most image transforms are either:
- **Row-local** (brightness, contrast, grayscale): each pixel depends only on itself
- **Window-local** (blur, sharpen): each pixel depends on a small neighborhood
- **Separable** (resize): horizontal and vertical passes can be done independently

Smart crop (face detection) is the exception -- it needs a global view. TurboEdge handles this by running detection on a 8x downsampled preview (consuming ~1/64 of the memory) and then replaying the full-resolution chunks through the computed crop box.

---

## Comparison to Alternatives

| | **Turbo + WASM** | **Node.js on Edge** | **Rust + WASM** | **Go + WASM** |
|---|---|---|---|---|
| Binary size | 2-4 MB | 5-20 MB (with sharp) | 1-3 MB | 10-30 MB |
| Cold start | < 1 ms | 50-200 ms | < 1 ms | 10-50 ms |
| Memory for 4K image | 2-3 MB (streaming) | 50-100 MB (buffer) | 2-3 MB (manual) | 50-100 MB (GC) |
| Throughput | ~90% native | ~20-40% native | ~95% native | ~50-60% native |
| Development speed | High (safe + ergonomic) | Highest (JS ecosystem) | Low (borrow checker) | Medium |
| Pipe operator | Yes (`\|>`) | No (method chaining) | No (combinators) | No |
| Streaming built-in | Yes (`async gen`) | Partial (Transform streams) | Manual | Manual |
| Compile-time compute | Yes (`const fn`) | No | Yes (`const fn`) | No |

**vs. Node.js on edge (Cloudflare Workers, Vercel Edge Functions):**
Node.js image processing typically requires `sharp` (a native addon that may not be available on all edge runtimes) or pure-JS libraries (slow). Memory usage is high because JavaScript's garbage collector and V8's JIT require significant overhead. Cold starts are slower due to V8 initialization.

**vs. Rust + WASM:**
Rust produces the fastest, smallest WASM binaries. But writing streaming image pipelines in Rust requires wrestling with the borrow checker, lifetimes on async streams, and manual Pin/Unpin gymnastics. Turbo's `async gen` and pipe operator make the same patterns ergonomic. Turbo's `const fn` serves the same purpose as Rust's.

**vs. Go + WASM:**
Go's WASM support produces large binaries (the entire Go runtime is compiled into the WASM module). The garbage collector's memory overhead is significant in constrained WASM environments. Go lacks streaming primitives comparable to Turbo's `async gen`.

---

## `const fn` for Compile-Time Optimization

Turbo's `const fn` allows computation to happen at compile time, with results baked directly into the WASM binary. This is critical for image processing where convolution kernels, lookup tables, and interpolation weights are mathematically fixed:

```
const fn gaussian_kernel(radius: u32, sigma: f64) -> [[f64]] {
  let size = radius * 2 + 1
  let mut kernel: [[f64]] = []
  // ... compute kernel values ...
  kernel
}

// This kernel exists in the binary as raw data -- zero runtime allocation
const BLUR_KERNEL_5: [[f64]] = gaussian_kernel(2, 1.4)
```

Other compile-time tables in TurboEdge:
- **Lanczos interpolation weights** (1024 entries per window size)
- **Gamma correction LUTs** (sRGB <-> linear conversion, 256 entries each)
- **Unsharp mask kernels** (light, medium, heavy presets)

These tables would normally be allocated and computed on first use. With `const fn`, they're free -- no allocation, no computation, no cache misses on first access. In WASM, where memory allocation is expensive (it may require growing the linear memory), this matters.

---

## The Pipe Operator

The pipe operator `|>` is what makes image processing pipelines readable. Compare:

**With pipe operator (Turbo):**
```
let result = source_stream
  |> apply_resize(target, .Lanczos(window: 3))
  |> apply_sharpen(0.5, 1.0, 0.0)
  |> apply_grayscale()
```

**Without pipe operator (typical approach):**
```
let resized = apply_resize(source_stream, target, Lanczos(3))
let sharpened = apply_sharpen(resized, 0.5, 1.0, 0.0)
let result = apply_grayscale(sharpened)
```

The pipe version reads left-to-right, top-to-bottom, matching the mental model of "data flows through transforms." Each line is a stage. Adding or removing a stage is a single-line edit. The nesting version forces you to read inside-out or introduces intermediate variables.

The builder pattern extends this to the URL-parsed pipeline:

```
let result = await process_image("https://example.com/photo.jpg")
  |> resize(800, 600)
  |> sharpen(0.5)
  |> watermark("https://brand.com/logo.png", position: .BottomRight)
  |> convert(.webp)
  |> execute(executor, context)
```

---

## Edge Deployment

Build and deploy in two commands:

```bash
# Compile to WASM
turbolang build --target wasm32-wasi

# Deploy to all edge locations
turbolang deploy --edge
```

The `turbolang deploy` command auto-detects the target platform from `turbo.toml`:

```toml
[edge]
provider = "auto"         # cloudflare, vercel, deno, or auto-detect
regions = ["all"]         # deploy to all edge locations
min-instances = 0         # scale to zero when idle
max-instances = 1000
timeout = "30s"
memory-limit = "128mb"
```

For Cloudflare Workers, it generates a `wrangler.toml` and pushes the WASM binary. For Vercel, it creates an Edge Function configuration. For Deno Deploy, it generates a `deno.json` with the WASM import.

The configuration also defines routing:

```toml
[edge.routes]
"/image/*" = "handle_image_request"
"/health" = "handle_health_check"
```

And caching behavior:

```toml
[edge.cache]
default-ttl = "1h"
max-size = "50mb"
stale-while-revalidate = "5m"
vary = ["Accept", "Accept-Encoding"]
```

---

## Architecture Overview

```
                    Client Request
                         |
                    Edge CDN Node
                         |
                 +-------+-------+
                 |  WASM Module  |
                 |  (main.tb)    |
                 +-------+-------+
                         |
              +----------+-----------+
              |          |           |
         routing.tb  cache.tb  pipeline.tb
              |          |           |
              |     LRU Cache   +---+---+
              |     (50MB)      |       |
         URL parser         transforms  stream
         geo-routing        (const fn   encoder
         content-neg         kernels)
```

**Request flow:**
1. Edge runtime calls `wasm_handle_request()` (wasm_bindings.tb)
2. Request is parsed into a `TransformPipeline` (routing.tb)
3. Geo-routing adjusts format/dimensions for the client's region (routing.tb)
4. Cache is checked for an existing result (cache.tb)
5. On miss: source image is fetched as a stream, transforms applied (pipeline.tb + transforms.tb)
6. Result is streamed back to the client and cached for future requests

**Memory model:**
- WASM linear memory: up to 256MB
- Request arena: 1MB per request (bump allocator, freed at once)
- Memory pool: reusable slabs for image chunks (16KB, 64KB, 256KB tiers)
- Edge cache: 50MB LRU for processed images
- Working set during processing: 2-3MB regardless of source image size

---

## File Structure

```
edge-wasm/
  turbo.toml              # Project config: wasm32-wasi target, edge settings
  src/
    main.tb               # Edge handler: request routing, response building
    models.tb             # Types: ImageFormat, TransformOp, Pipeline, errors
    pipeline.tb           # Streaming pipeline executor, chunk decode/encode
    transforms.tb         # Transform implementations with const fn kernels
    cache.tb              # LRU cache with TTL, ETag, conditional requests
    routing.tb            # URL parsing, geo-routing, content negotiation
    wasm_bindings.tb      # WASM exports/imports, memory management, JS interop
  tests/
    pipeline_test.tb      # Transform pipeline correctness tests
    perf_test.tb          # Latency, throughput, and memory budget tests
    cache_test.tb         # Cache hit/miss, TTL, eviction, concurrent access
```
