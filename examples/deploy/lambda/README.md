# Deploy Turbo to AWS Lambda

A Lambda function written in Turbo, running as a **custom runtime** via the
[`turbo-lambda`](../../../packages/turbo-lambda) adapter. The deployed
artifact is a zip containing exactly two files: a native `handler` binary and
a two-line `bootstrap` script. No language runtime ships in the package — cold
starts are just process start.

## Files

| File | Purpose |
|------|---------|
| `main.tb` | The function: a safe `fn(str) -> str` handler + `lambda_run`. |
| `bootstrap` | Lambda's entry point: exports `TURBO_ALLOW_PRIVATE_HOSTS=1` and execs `./handler`. |
| `mock_runtime.py` | Local mock of the Lambda Runtime API (correct headers included). |
| `test_local.sh` | End-to-end test against the mock — no AWS account needed. |

## Test locally first (no AWS account)

```bash
./test_local.sh
# ... PASS: 2 invocations handled end-to-end
```

This runs the real function loop against a faithful mock of the runtime API
(`mock_runtime.py` sends the same `Lambda-Runtime-Aws-Request-Id` headers the
real one does).

## Build the deployment package

Lambda's `provided.al2023` runtime on `x86_64` matches Turbo's `linux-x86`
cross-compile target, so you can build the artifact from macOS or Linux:

```bash
turbolang build main.tb --target linux-x86 -o handler
zip function.zip bootstrap handler
```

(`bootstrap` must be executable — `chmod +x bootstrap` if your checkout lost
the bit.)

## Create the function

```bash
aws iam create-role --role-name turbo-lambda-demo \
  --assume-role-policy-document '{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":{"Service":"lambda.amazonaws.com"},"Action":"sts:AssumeRole"}]}'
aws iam attach-role-policy --role-name turbo-lambda-demo \
  --policy-arn arn:aws:iam::aws:policy/service-role/AWSLambdaBasicExecutionRole

aws lambda create-function \
  --function-name turbo-hello \
  --runtime provided.al2023 \
  --architectures x86_64 \
  --handler unused \
  --zip-file fileb://function.zip \
  --role arn:aws:iam::<ACCOUNT_ID>:role/turbo-lambda-demo
```

## Invoke it

```bash
aws lambda invoke --function-name turbo-hello \
  --payload '{"name": "Turbo"}' \
  --cli-binary-format raw-in-base64-out /dev/stdout
# {"greeting":"Hello, Turbo!"}
```

## Notes

- **Cold start:** the artifact contains no language runtime to boot. Measure
  it yourself with the reproducible scripts in
  [`benchmarks/serverless`](../../../benchmarks/serverless) — this repo does
  not publish numbers that scripts didn't generate.
- **Why `TURBO_ALLOW_PRIVATE_HOSTS=1`:** the adapter POSTs results to the
  runtime API's loopback/link-local address, which Turbo's SSRF guard blocks
  by default. The opt-out is scoped to the Lambda sandbox by living in
  `bootstrap`, not in your shell.
- **arm64:** Lambda also offers `arm64`, but Turbo's `linux-arm64` target is
  Tier 2 (cross-compiles, not yet runtime-validated) — use `x86_64` until
  that graduates. See [`COMPATIBILITY.md`](../../../COMPATIBILITY.md).
- **How the adapter works** (`http_get_raw` for `/invocation/next` because
  the request id arrives in a response header, `http_post` for responses):
  see the [package README](../../../packages/turbo-lambda/README.md).
