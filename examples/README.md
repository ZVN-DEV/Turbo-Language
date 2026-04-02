# Turbo Examples

Three runnable example projects demonstrating real-world Turbo code. Each can be run directly with `turbolang run`.

## Examples

| Example | Description | Key Features |
|---------|-------------|--------------|
| [simple-script](./simple-script/) | Text Statistics Analyzer | Strings, HashMaps, arrays, pipes, string interpolation |
| [speed-server](./speed-server/) | REST API Benchmark Server | HTTP server, JSON responses, fibonacci, primes, sorting |
| [web-dashboard](./web-dashboard/) | Interactive Web Dashboard | HTML UI, multiple API endpoints, real-time benchmarks |

### simple-script

A text statistics analyzer that counts words, calculates frequencies, and ranks results using pipes, HashMaps, and string interpolation. A great first example to understand Turbo's data processing capabilities.

```bash
turbolang run examples/simple-script/main.tb
```

### speed-server

An HTTP server on port 8080 with endpoints for fibonacci, prime counting, sorting benchmarks, and health checks. Returns JSON responses. Demonstrates Turbo's async HTTP primitives.

```bash
turbolang run examples/speed-server/main.tb
# curl http://localhost:8080/api/fib
```

### web-dashboard

A full benchmark dashboard with a styled HTML UI served on port 3000. Run benchmarks from the browser and see results in real time.

```bash
turbolang run examples/web-dashboard/main.tb
# open http://localhost:3000
```

---

## Running Examples

```bash
turbolang run examples/simple-script/main.tb
turbolang run examples/speed-server/main.tb
turbolang run examples/web-dashboard/main.tb
```

---

## Roadmap Examples

The [`roadmap/`](./roadmap/) directory contains example projects that demonstrate features currently in development. These use syntax that is not yet implemented in the compiler (optional chaining `?.`, `from` imports, `Shared<T>`, result types, WASM targets, regions). They are design documents showing where Turbo is headed — not runnable code.

| Example | Description | Status |
|---------|-------------|--------|
| [task-agent](./roadmap/task-agent/) | Task Management API with AI Agent | Planned — uses `?.`, `() ! Error` |
| [web-api](./roadmap/web-api/) | Production Bookmarking API | Planned — uses `from` imports, `Shared<T>` |
| [desktop-app](./roadmap/desktop-app/) | Native Desktop Markdown Editor | Planned — uses `?.`, optional chaining |
| [realtime-system](./roadmap/realtime-system/) | Trading Order Matching Engine | Planned — uses regions, `?.` |
| [edge-wasm](./roadmap/edge-wasm/) | Edge Image Processing | Planned — uses `from` imports, WASM target |
