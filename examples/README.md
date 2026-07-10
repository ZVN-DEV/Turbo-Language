# Turbo Examples

Runnable example projects demonstrating real-world Turbo code. Each can be run directly with `turbolang run`.

## Start here

If you only try one example, make it [`web-dashboard`](./web-dashboard/). It is the clearest end-to-end demo of what ships today: one Turbo file serving a browser UI plus JSON benchmark endpoints.

```bash
turbolang run examples/web-dashboard/main.tb
# then open http://localhost:3000
```

## Examples

| Example | Description | Key Features |
|---------|-------------|--------------|
| [web-dashboard](./web-dashboard/) | **Flagship demo: Interactive Web Dashboard** | HTML UI, multiple API endpoints, real-time benchmarks |
| [todo-cli](./todo-cli/) | Task Manager | Structs, file I/O, string ops, for-in, if/else |
| [data-pipeline](./data-pipeline/) | Log Analysis Engine | File I/O, hashmaps, string parsing, closures |
| [game-of-life](./game-of-life/) | Conway's Game of Life | String-as-grid, char_at, nested loops, algorithms |
| [simple-script](./simple-script/) | Text Statistics Analyzer | Strings, HashMaps, arrays, pipes, string interpolation |
| [speed-server](./speed-server/) | REST API Benchmark Server | HTTP server, JSON responses, fibonacci, primes, sorting |
| [http-sqlite-api](./http-sqlite-api/) | Todo API on built-in SQLite | HTTP server, embedded SQLite (`sqlite_*`), prepared statements, JSON — one self-contained binary |
| [stateful-counter](./stateful-counter/) | Persistent Hit Counter | HTTP server, persistent in-memory state across requests (a startup hashmap survives the per-request arena), bounded memory |
| [file-analyzer](./file-analyzer/) | Source Code Analyzer | File I/O, line parsing, statistics, progress bars |
| [libturbo-c-host](./libturbo-c-host/) | C Host Embedding Demo | `libturbo`, JIT eval, host callbacks, typed `i64`/`str` exchange |
| [deploy/lambda](./deploy/lambda/) | AWS Lambda function (custom runtime) | `turbo-lambda` adapter, cross-compiled zip artifact, local mock-runtime test |
| [deploy/cloud-run](./deploy/cloud-run/) | Cloud Run service | Multi-stage Docker build, `PORT` env, `0.0.0.0` bind, health checks |
| [deploy/fly](./deploy/fly/) | Fly.io service | Same container shape + `fly.toml` with health checks and scale-to-zero |

### web-dashboard

The flagship runnable demo. It serves a styled browser dashboard on port 3000, exposes five benchmark endpoints, and lets you trigger them live from the UI. It is the best first stop if you want a quick “Turbo is real” moment.

```bash
turbolang run examples/web-dashboard/main.tb
# then open http://localhost:3000
```

Quick checks:

- Click **Run All Benchmarks**
- Visit `http://localhost:3000/api/info` in a second tab
- Stop the server with `Ctrl+C`

### todo-cli

A task manager that creates tasks with priorities, marks them complete, filters views, and persists to disk using pipe-delimited file I/O. Demonstrates structs, string interpolation, and file operations.

```bash
turbolang run examples/todo-cli/main.tb
```

### data-pipeline

A log analysis engine that generates sample server logs, writes them to disk, then parses and analyzes them — producing level distributions, HTTP method breakdowns, status code analysis, endpoint frequency via hashmaps, and a health summary.

```bash
turbolang run examples/data-pipeline/main.tb
```

### game-of-life

Conway's Game of Life using strings as the grid representation. Places a Glider, Blinker, and Block pattern, then simulates 8 generations. Demonstrates char_at, nested loops, constants, and pure-functional grid updates.

```bash
turbolang run examples/game-of-life/main.tb
```

### simple-script

A text statistics analyzer that counts words, calculates frequencies, and ranks results using pipes, HashMaps, and string interpolation. A great first example to understand Turbo's data processing capabilities.

```bash
turbolang run examples/simple-script/main.tb
```

### file-analyzer

A source code analyzer that reads its own source file, classifies each line (code, comment, blank), computes statistics (avg/max line length), and displays results with ASCII progress bars. Demonstrates file I/O, string inspection, and formatted output.

```bash
turbolang run examples/file-analyzer/main.tb
```

### speed-server

An HTTP server on port 8080 with endpoints for fibonacci, prime counting, sorting benchmarks, and health checks. Returns JSON responses. Demonstrates Turbo's HTTP primitives.

```bash
turbolang run examples/speed-server/main.tb
# curl http://localhost:8080/api/fib
```

### http-sqlite-api

A tiny todo API backed by a real, embedded SQLite database — written entirely in Turbo. SQLite is vendored into the compiler and statically linked, so there is no external database server and no `libsqlite3` dependency: `turbolang build` produces a single self-contained binary. Demonstrates `sqlite_open`, prepared statements with bound parameters, row-by-row reads, and `Result`-based error handling behind an HTTP + JSON API.

```bash
turbolang run examples/http-sqlite-api/main.tb
# curl -s http://127.0.0.1:8080/todos
```

### libturbo-c-host

A C host embedding demo that registers native callbacks, evaluates Turbo source through `libturbo`, then calls Turbo functions returning `i64` and `str`. Build and run it with the commands in [`examples/libturbo-c-host/README.md`](./libturbo-c-host/README.md).

---

## Running Examples

```bash
turbolang run examples/web-dashboard/main.tb
turbolang run examples/todo-cli/main.tb
turbolang run examples/data-pipeline/main.tb
turbolang run examples/game-of-life/main.tb
turbolang run examples/simple-script/main.tb
turbolang run examples/speed-server/main.tb
turbolang run examples/http-sqlite-api/main.tb
turbolang run examples/stateful-counter/main.tb
turbolang run examples/file-analyzer/main.tb
```

---

## TurboServo (External)

For a full-featured web framework example, see [TurboServo](https://github.com/ZVN-DEV/turboservo) — TurboLang's HTTP server framework with SSR HTML, CRUD persistence, compute benchmarks, and a live browser dashboard.

---

## Roadmap Examples

The [`roadmap/`](./roadmap/) directory contains example projects that demonstrate features currently in development. These use syntax that is not yet implemented in the compiler (`from` imports, `Shared<T>`, regions). They are design documents showing where Turbo is headed — not runnable code.

| Example | Description | Status |
|---------|-------------|--------|
| [web-api](./roadmap/web-api/) | Production Bookmarking API | Planned — uses `from` imports, `Shared<T>` |
| [realtime-system](./roadmap/realtime-system/) | Trading Order Matching Engine | Planned — uses regions, `?.` |
| [edge-wasm](./roadmap/edge-wasm/) | Edge Image Processing | Planned — uses `from` imports, WASM target |
