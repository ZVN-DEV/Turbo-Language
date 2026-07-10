# Deploy Turbo to Fly.io

The same Turbo HTTP service as the [Cloud Run example](../cloud-run), shaped
for Fly.io: a multi-stage Docker build that installs the released toolchain,
AOT-compiles `main.tb`, and ships a slim image containing only the native
binary. Fly's proxy terminates TLS; the app binds `0.0.0.0:$PORT` inside the
machine — exactly the deployment model Turbo's built-in server is designed
for.

## Deploy

```bash
cd examples/deploy/fly
fly launch --copy-config --name <your-app-name> --now
```

`fly launch` reads `fly.toml` (health check on `/healthz`, 256MB
`shared-cpu-1x`, scale-to-zero) and builds the Dockerfile remotely. Subsequent
deploys are just `fly deploy`.

## Verify

```bash
curl https://<your-app-name>.fly.dev/
# {"message":"Hello from Turbo on Fly.io","language":"turbo"}
curl https://<your-app-name>.fly.dev/count
# {"hits":1}  — increments per request while the machine is warm
```

## Test locally without Fly

```bash
PORT=8080 turbolang run main.tb
# or the full container:
docker build -t turbo-fly . && docker run --rm -p 8080:8080 turbo-fly
curl http://127.0.0.1:8080/healthz
```

## Notes

- **Scale-to-zero + native cold start:** `min_machines_running = 0` stops the
  machine when idle; a native binary with no runtime to boot makes the
  wake-up path about as cheap as it gets. Measure it with the reproducible
  scripts in [`benchmarks/serverless`](../../../benchmarks/serverless) — this
  repo publishes no numbers a script didn't generate.
- **Memory floor:** Turbo's ARC keeps long-running RSS flat, which is what
  lets the 256MB VM size in `fly.toml` stay honest under sustained traffic.
- **Concurrency:** the built-in server is thread-per-connection — suited to
  the request volumes a small PaaS instance sees, not C10K on one machine.
  See the honest caveats in the [main README](../../../README.md).
