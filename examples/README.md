# Turbo Examples

Runnable example projects demonstrating real-world Turbo code. Each can be run directly with `turbolang run`.

## Examples

| Example | Description | Key Features |
|---------|-------------|--------------|
| [todo-cli](./todo-cli/) | Task Manager | Structs, file I/O, string ops, for-in, if/else |
| [data-pipeline](./data-pipeline/) | Log Analysis Engine | File I/O, hashmaps, string parsing, closures |
| [game-of-life](./game-of-life/) | Conway's Game of Life | String-as-grid, char_at, nested loops, algorithms |
| [simple-script](./simple-script/) | Text Statistics Analyzer | Strings, HashMaps, arrays, pipes, string interpolation |
| [speed-server](./speed-server/) | REST API Benchmark Server | HTTP server, JSON responses, fibonacci, primes, sorting |
| [web-dashboard](./web-dashboard/) | Interactive Web Dashboard | HTML UI, multiple API endpoints, real-time benchmarks |

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
turbolang run examples/todo-cli/main.tb
turbolang run examples/data-pipeline/main.tb
turbolang run examples/game-of-life/main.tb
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
