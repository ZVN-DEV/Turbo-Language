# Turbo on serverless platforms

Turbo's deployment artifact is a native binary with no language runtime to
boot — which is exactly the property serverless platforms price: cold starts
are process start, and ARC keeps the memory floor flat while an instance
stays warm. This page ties together the deploy examples, the Lambda adapter,
and the benchmark methodology.

## Where to start

| Platform | What you deploy | Walkthrough |
|----------|-----------------|-------------|
| **AWS Lambda** | zip: native `handler` binary + 2-line `bootstrap` (custom runtime, `provided.al2023`) | [`examples/deploy/lambda`](../examples/deploy/lambda) |
| **Google Cloud Run** | slim Docker image containing only the binary | [`examples/deploy/cloud-run`](../examples/deploy/cloud-run) |
| **Fly.io** | same image shape + `fly.toml` (health checks, scale-to-zero) | [`examples/deploy/fly`](../examples/deploy/fly) |

All three examples are self-contained and tested locally without a cloud
account (`test_local.sh` for Lambda against a mock runtime API; `docker build
&& docker run` for the container platforms).

## The Lambda adapter

[`turbo-lambda`](../packages/turbo-lambda) is a pure-Turbo custom-runtime
adapter: your function is one safe `fn(str) -> str` (event JSON in, response
JSON out) and `lambda_run(handler)` is the entire loop. Install it by name:

```bash
turbolang install turbo-lambda
```

See the package README for how it speaks the runtime API (`http_get_raw` for
`/invocation/next`, since the request id arrives in a response header) and
why the bootstrap exports `TURBO_ALLOW_PRIVATE_HOSTS=1`.

## Why the model fits

- **Cold start = process start.** The artifact contains no interpreter or VM;
  Lambda's `Init Duration` for a Turbo function is dominated by the platform,
  not the language.
- **Flat warm memory.** Strings, arrays, structs, and containers are
  reference-counted and freed during execution, so a warm instance's RSS
  doesn't creep toward the memory limit.
- **TLS is the platform's job.** Every platform here terminates TLS at its
  edge and forwards plain HTTP to your process — the deployment model Turbo's
  built-in server is designed for (`http_server_public` + `PORT` env). No
  reverse-proxy setup of your own is needed.
- **Concurrency fits the platform's slicing.** Serverless platforms cap
  per-instance concurrency (Lambda: 1 invocation per sandbox; Cloud Run:
  configurable, default 80) — comfortably inside the thread-per-connection
  server model. See the [main README](../README.md)'s honest caveats for why
  a single self-hosted process is a different story.

## Benchmarks: run them, don't quote them

[`benchmarks/serverless`](../benchmarks/serverless) has reproducible scripts:
`lambda/bench.sh` measures platform-reported Init Duration over N forced cold
starts for identical Turbo/Node/Python functions, and `local_cold_start.sh`
is a no-cloud-account proxy. **This repo publishes no benchmark numbers a
script didn't generate** — run them in your own account/region and cite the
script + date if you share results.

## Current limits (honest list)

- **`x86_64` only for Lambda today** — Turbo's `linux-arm64` target
  cross-compiles but is not yet runtime-validated (Tier 2), so skip Lambda's
  arm64/Graviton option for now. See [COMPATIBILITY.md](COMPATIBILITY.md).
- **The HTTP client executes `curl` under the hood** (all of `http_get`,
  `http_get_raw`, `http_post`), so the deploy image needs curl present —
  true of `provided.al2023` and the example Dockerfiles. A native-socket
  client is [Reach roadmap](../design/REACH-ROADMAP.md) Phase 1 material.
- **Edge WASM platforms (Cloudflare Workers, Fastly) are not supported yet** —
  they deploy WASM, not native binaries, and Turbo's WASM target is still
  experimental. That's Phase 4 of the Reach roadmap.
