# turbo-lambda

AWS Lambda custom-runtime adapter, in pure Turbo. Your function is one safe
`fn(str) -> str` — event JSON in, response JSON out — and `lambda_run(handler)`
is the entire runtime loop.

```turbo
import { lambda_run } from "turbo-lambda"

fn handler(event: str) -> str {
    let name = json_get(event, "name")
    "\{\"greeting\":\"Hello, " + name + "!\"\}"
}

@unsafe fn main() {
    lambda_run(handler)
}
```

`main` is `@unsafe` because the adapter's transport shells out to `curl`
(see *How it works*). The handler itself stays safe.

## Deploying

Lambda custom runtimes are a zip with a `bootstrap` executable at the root.
Full walkthrough (build, zip, create-function, local testing with a mock
runtime API): [`examples/deploy/lambda`](../../examples/deploy/lambda).

The bootstrap is two lines — it must export the SSRF-guard opt-out so the
adapter may POST to the runtime API's loopback/link-local address:

```sh
#!/bin/sh
export TURBO_ALLOW_PRIVATE_HOSTS=1
exec ./handler
```

## API

| Function | Purpose |
|----------|---------|
| `lambda_run(handler)` | Production entry point: poll + dispatch forever. `@unsafe`. |
| `lambda_run_n(handler, n) -> i64` | Handle at most `n` invocations; returns how many succeeded. For tests/drain workers. `@unsafe`. |
| `lambda_handle_once(base, handler) -> bool` | One poll-dispatch-respond cycle. `@unsafe`. |
| `lambda_post_error(base, id, type, msg)` | Report a failed invocation to the runtime API. |
| `lambda_api_base() -> str` | Runtime API base URL from `AWS_LAMBDA_RUNTIME_API`. |
| `lambda_header(raw, name) -> str` | Case-insensitive header lookup in a raw `curl -si` transcript (pure, tested). |
| `lambda_body(raw) -> str` | Body of a raw transcript (pure, tested). |
| `lambda_parse_invocation(raw) -> [str]` | `[request_id, event_body]`, `["", ""]` on malformed input (pure, tested). |

## How it works (and honest constraints)

The adapter speaks the [Lambda Runtime API](https://docs.aws.amazon.com/lambda/latest/dg/runtimes-api.html)
(2018-06-01):

- **`GET /invocation/next`** is fetched via `exec("curl -si ...")`, because the
  request id arrives in the `Lambda-Runtime-Aws-Request-Id` **response
  header** and Turbo's built-in `http_get` returns only the body. `curl` is
  present in AWS's `provided.al2023` image (AWS's own bash custom-runtime
  tutorial uses it the same way). `exec` is why the loop functions are
  `@unsafe`.
- **Responses are POSTed with `http_post`** (a JSON body must travel as one
  argument; `exec` tokenizes on whitespace). The built-in SSRF guard blocks
  loopback/link-local targets by default, which is why the bootstrap exports
  `TURBO_ALLOW_PRIVATE_HOSTS=1`.
- **`/invocation/next` long-polls by design** — between invocations Lambda
  freezes the process while curl blocks on the socket. That is the protocol,
  not a hang, so no timeout is passed on that call.

## Tests

```bash
turbolang test packages/turbo-lambda/tests
```

17 unit tests cover the pure parsing core (header extraction incl.
case-insensitivity and colon-bearing values, body extraction incl. CRLF/LF
and inner blank lines, invocation parsing, JSON escaping, error payloads).
The loop itself is exercised end-to-end against a mock runtime API — see
[`examples/deploy/lambda`](../../examples/deploy/lambda).
