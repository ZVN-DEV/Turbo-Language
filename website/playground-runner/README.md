# Turbo Playground Runner

This is the isolated execution target for the hosted `/play` page. The Next.js
site never shells out to `turbolang`; it forwards validated source to this
separate service through `TURBO_PLAYGROUND_RUNNER_URL`.

The runner still executes untrusted Turbo source. Run it only inside an
isolated container or equivalent sandbox boundary.

## Build

From the repository root:

```bash
docker build -t turbo-playground-runner -f website/playground-runner/Dockerfile .
```

## Run Locally

Use a random token and point the website at the mapped `/run` endpoint:

```bash
TOKEN="$(openssl rand -hex 32)"
docker run --rm \
  --name turbo-playground-runner \
  --read-only \
  --tmpfs /tmp:rw,noexec,nosuid,nodev,size=64m \
  --cap-drop=ALL \
  --security-opt no-new-privileges \
  --pids-limit=64 \
  --memory=256m \
  --cpus=1 \
  -e TURBO_PLAYGROUND_RUNNER_TOKEN="$TOKEN" \
  -e TURBO_PLAYGROUND_RUNNER_MAX_CONCURRENT=2 \
  -p 8787:8787 \
  turbo-playground-runner
```

Then start the website with:

```bash
TURBO_PLAYGROUND_RUNNER_URL=http://localhost:8787/run \
TURBO_PLAYGROUND_RUNNER_TOKEN="$TOKEN" \
npm run dev
```

Smoke the runner before wiring it to a public site:

```bash
TURBO_PLAYGROUND_RUNNER_URL=http://localhost:8787/run \
TURBO_PLAYGROUND_RUNNER_TOKEN="$TOKEN" \
npm run smoke:playground-runner
```

The smoke probe checks `/healthz`, an authenticated safe execution, and the
source-policy rejection for the forbidden `exec` process API.

`TURBO_PLAYGROUND_RUNNER_ALLOW_UNSAFE_HOST=1` is set in the Docker image so the
process can start inside the container. Do not set it for a public host process
outside an isolation boundary.

`TURBO_PLAYGROUND_RUNNER_TOKEN` is required at startup. Surrounding whitespace
is trimmed; blank tokens are treated as missing. For an isolated one-off local
experiment without auth, set `TURBO_PLAYGROUND_RUNNER_ALLOW_MISSING_TOKEN=1`;
do not use that flag on a reachable runner.

The runner listens on `PORT=8787` by default. If set, `PORT` must be an integer
from 1 to 65535.

## Contract

`POST /run`

```json
{ "source": "fn main() { print(\"hi\") }" }
```

Response:

```json
{
  "stdout": "hi\n",
  "stderr": "",
  "success": true,
  "durationMs": 12
}
```

`GET /healthz` returns `{ "ok": true }` and is wired into the Docker image
`HEALTHCHECK` so deploy targets can detect unhealthy runner instances.

Limits:

- Requests must use `Content-Type: application/json`.
- Source must be non-empty UTF-8 JSON text.
- Source is capped at 64 KiB.
- The runner is single-file. It rejects `import` declarations before invoking
  the compiler so user source cannot make compile-time file reads.
- The runner rejects unsafe/FFI features (`@unsafe`, `extern`, `deref`,
  `store`) before execution.
- The runner rejects host-access filesystem, process, environment, interactive
  input, and network/server builtins (`read_file`, `write_file`, `file_exists`,
  `delete_file`, `list_dir`, `mkdir`, `exec`, `env_get`, `args`, `read_line`,
  `http_get`, `http_post_with_headers`, `http_server`, `route`, and related
  APIs) before execution. Pure path string helpers such as `path_join`,
  `path_dir`, `path_base`, and `path_ext` remain allowed. The playground is for
  language exploration, not host access. Executable string interpolation is
  scanned by the same policy; literal string text is not.
- Combined stdout/stderr capture is capped at 128 KiB by Node's `execFile`.
- Execution times out after 5 seconds by default and uses `SIGKILL` for
  deterministic process termination. Set `TURBO_PLAYGROUND_RUNNER_TIMEOUT_MS`
  to a positive integer to tune this per deployment.
- Concurrent executions are capped at 2 per runner process by default. Set
  `TURBO_PLAYGROUND_RUNNER_MAX_CONCURRENT` to a positive integer to tune this
  per deployment. Malformed numeric config, including invalid `PORT`, fails
  startup instead of silently falling back to defaults.
- The HTTP server bounds slow clients: 5s to send headers, 10s to finish a
  request, 15s socket idle timeout, and 5s keep-alive timeout.
- The child process receives a fixed minimal environment and runs with `/tmp`
  as home.

## Production Notes

The Docker flags above are part of the security posture, not optional polish.
For public deployment, use an environment that also blocks outbound network
egress from the runner and recycles containers aggressively. The runner is a
small bridge around the Turbo CLI; the container is the sandbox. The source
policy is a defense-in-depth gate, not a replacement for container isolation.
