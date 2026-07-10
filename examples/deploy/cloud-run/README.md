# Deploy Turbo to Google Cloud Run

A Turbo HTTP service in a container: multi-stage Docker build that installs
the released toolchain, AOT-compiles `main.tb`, and ships a slim image
containing only the native binary. Cloud Run terminates TLS and injects
`PORT`; the app reads it and binds `0.0.0.0` — exactly the behind-a-proxy
deployment model Turbo's built-in server is designed for.

## Deploy

The build context is this directory (self-contained — no repo checkout needed
in the image):

```bash
cd examples/deploy/cloud-run
gcloud run deploy turbo-hello --source . --region us-central1 --allow-unauthenticated
```

Cloud Build runs the Dockerfile remotely, so you don't need Docker locally.

## Verify

```bash
URL=$(gcloud run services describe turbo-hello --region us-central1 --format='value(status.url)')
curl "$URL/"
# {"message":"Hello from Turbo on Cloud Run","language":"turbo"}
curl "$URL/healthz"
# ok
curl "$URL/count"
# {"hits":1}  — increments per request while the instance is warm
```

## Test locally without GCP

```bash
PORT=8080 turbolang run main.tb
# or the full container:
docker build -t turbo-cloudrun . && docker run --rm -p 8080:8080 turbo-cloudrun
curl http://127.0.0.1:8080/healthz
```

## Notes

- **Cold start:** the image's entry point is one native binary — no language
  runtime boots before the first request. Measure it with the reproducible
  scripts in [`benchmarks/serverless`](../../../benchmarks/serverless) — this
  repo publishes no numbers a script didn't generate.
- **State:** the `/count` hashmap lives per instance and resets on scale-down;
  that's Cloud Run semantics, not a Turbo limitation. For durable state, pair
  with SQLite on a mounted volume or an external store (see
  [`examples/http-sqlite-api`](../../http-sqlite-api)).
- **Toolchain pinning:** the Dockerfile installs the latest release; pin a
  version with `bash -s -- --version X.Y.Z` on the install line for
  reproducible image builds.
- **Concurrency:** the built-in server is thread-per-connection. Cloud Run's
  default `--concurrency 80` per instance is comfortably inside that model;
  don't crank per-instance concurrency into the thousands.
