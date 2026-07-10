# turbo-lambda

AWS Lambda custom-runtime adapter, in pure Turbo. Your function is one safe
`fn(str) -> str` — event JSON in, response JSON out — and `lambda_run(handler)`
is the entire runtime loop. No `@unsafe` anywhere.

```turbo
import { lambda_run } from "turbo-lambda"

fn handler(event: str) -> str {
    let name = json_get(event, "name")
    "\{\"greeting\":\"Hello, " + name + "!\"\}"
}

fn main() {
    lambda_run(handler)
}
```

## Deploying

Lambda custom runtimes are a zip with a `bootstrap` executable at the root.
Full walkthrough (build, zip, create-function, local testing with a mock
runtime API): [`examples/deploy/lambda`](../../examples/deploy/lambda).

The bootstrap is two lines — it must export the SSRF-guard opt-out so the
HTTP client may talk to the runtime API's loopback/link-local address:

```sh
#!/bin/sh
export TURBO_ALLOW_PRIVATE_HOSTS=1
exec ./handler
```

## API

| Function | Purpose |
|----------|---------|
| `lambda_run(handler)` | Production entry point: poll + dispatch forever. |
| `lambda_run_n(handler, n) -> i64` | Handle at most `n` invocations; returns how many succeeded. For tests/drain workers. |
| `lambda_handle_once(base, handler) -> bool` | One poll-dispatch-respond cycle. |
| `lambda_post_error(base, id, type, msg)` | Report a failed invocation to the runtime API. |
| `lambda_api_base() -> str` | Runtime API base URL from `AWS_LAMBDA_RUNTIME_API`. |
| `lambda_header(raw, name) -> str` | Case-insensitive header lookup in a raw HTTP response (pure, tested). |
| `lambda_body(raw) -> str` | Body of a raw HTTP response (pure, tested). |
| `lambda_parse_invocation(raw) -> [str]` | `[request_id, event_body]`, `["", ""]` on malformed input (pure, tested). |

## How it works

The adapter speaks the [Lambda Runtime API](https://docs.aws.amazon.com/lambda/latest/dg/runtimes-api.html)
(2018-06-01):

- **`GET /invocation/next`** uses the `http_get_raw` builtin, which returns
  the full response — status line, headers, body — because the request id
  arrives in the `Lambda-Runtime-Aws-Request-Id` **response header** (the
  body-only `http_get` can't see it).
- **Responses are POSTed with `http_post`** to
  `/invocation/{request_id}/response` (errors to `.../error`).
- **The SSRF guard** in Turbo's HTTP client blocks loopback/link-local
  targets by default, which is why the bootstrap exports
  `TURBO_ALLOW_PRIVATE_HOSTS=1` — scoped to the Lambda sandbox, where the
  runtime API is the loopback service you *want* to reach.
- **`/invocation/next` long-polls** — between invocations Lambda freezes the
  process mid-poll. The client's standard 30-second time bound only matters
  outside real Lambda (e.g. a local mock): an idle poll returns empty and the
  loop re-polls.

## Tests

```bash
turbolang test packages/turbo-lambda/tests
```

17 unit tests cover the pure parsing core (header extraction incl.
case-insensitivity and colon-bearing values, body extraction incl. CRLF/LF
and inner blank lines, invocation parsing, JSON escaping, error payloads).
The loop itself is exercised end-to-end against a mock runtime API — see
[`examples/deploy/lambda`](../../examples/deploy/lambda).
