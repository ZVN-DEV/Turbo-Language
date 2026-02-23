# task-agent

A complete Task Management API with an AI Agent assistant, built in Turbo.

This example demonstrates how Turbo combines the ergonomics of JavaScript/TypeScript with the performance and safety of Rust, plus first-class AI agent primitives that no other language offers.

## What This Project Demonstrates

### 1. Web Server with REST API (`src/routes.tb`)
- Route definitions with typed request/response handling
- JSON parsing with `req.json<T>()`
- Query parameter extraction
- Status codes and error responses
- Middleware (CORS, rate limiting, request logging)

### 2. Data Types and Pattern Matching (`src/models.tb`)
- Algebraic data types with `type TaskStatus { Todo, InProgress, Done, Blocked(reason: str) }`
- Structs with default field values: `priority: u8 = 3`
- `T?` optionals: `assignee: str?`
- Pattern matching with `match` for exhaustive case handling
- `@derive(Debug, Eq, Clone, Schema, Serialize)` for automatic trait implementations

### 3. Error Handling with `T ! E` (`src/models.tb`, `src/store.tb`)
- Custom error types: `type TaskError: Error { NotFound(id: u64), ... }`
- `?` operator for error propagation
- `match` on `ok()`/`err()` for explicit handling
- `guard` statements for early validation returns

### 4. Async/Await and Concurrency (`src/main.tb`, `src/store.tb`)
- Top-level `await` in `main()`
- `async fn` with `await` calls
- `Shared<T>` for thread-safe shared state (read/write locks)
- `all()` for concurrent execution (like `Promise.all()`)

### 5. AI Agent Integration (`src/agent.tb`)
- `tool fn` keyword for LLM-callable functions with auto-generated JSON schemas
- `agent TaskAssistant { ... }` declaration with model, tools, memory, and system prompt
- `agent.ask()` for single responses
- `agent.stream()` for token-by-token streaming
- Agent routing and context binding

### 6. Testing (`tests/`)
- **Unit tests** (`store_test.tb`): CRUD, validation, filtering, concurrent access
- **Integration tests** (`api_test.tb`): HTTP API with `MockServer` and `TestClient`
- **Performance tests** (`perf_test.tb`): `@perf` decorator with memory/time budgets, `@stress` for load testing
- **Agent tests** (`agent_test.tb`): `@mock` decorator for LLM responses, tool function unit tests

### 7. Structured Logging (`src/main.tb`, `src/routes.tb`)
- `log.init(level, format)` for configuration
- `log.info()`, `log.debug()`, `log.error()` with structured fields
- JSON-formatted log output

### 8. Configuration Management (`src/main.tb`)
- Environment variable loading with `env.get()`
- Default values with `??` null coalescing
- Typed config struct

## Project Structure

```
task-agent/
  turbo.toml          # Project configuration
  src/
    main.tb           # Entry point, config, server startup
    models.tb         # Data types, error types, DTOs
    store.tb          # In-memory data store with Shared<T>
    routes.tb         # HTTP route handlers
    agent.tb          # AI agent with tool functions
  tests/
    store_test.tb     # Unit tests for the store
    api_test.tb       # Integration tests for the API
    perf_test.tb      # Performance and stress tests
    agent_test.tb     # Agent tests with mocked LLM
```

## Running

```bash
# Build and run
turbo run

# Run all tests
turbo test

# Run only unit tests
turbo test tests/store_test.tb

# Run performance tests
turbo test tests/perf_test.tb

# Run with custom config
TASK_PORT=8080 TASK_LOG_LEVEL=debug turbo run
```

## API Endpoints

| Method | Path               | Description                          |
|--------|--------------------|--------------------------------------|
| GET    | /tasks             | List tasks (with optional filters)   |
| GET    | /tasks/:id         | Get a task by ID                     |
| GET    | /tasks/stats       | Get task statistics                  |
| POST   | /tasks             | Create a new task                    |
| PATCH  | /tasks/:id         | Update an existing task              |
| DELETE | /tasks/:id         | Delete a task                        |
| POST   | /tasks/organize    | Ask the AI agent to organize tasks   |
| POST   | /tasks/chat        | Stream a conversation with the agent |

## Turbo Features Used

| Feature | Syntax | File |
|---------|--------|------|
| Algebraic data types | `type TaskStatus { Todo, InProgress, ... }` | models.tb |
| Optionals | `T?`, `none`, `some()`, `??` | models.tb, store.tb |
| Result types | `T ! E`, `ok()`, `err()`, `?` | store.tb, routes.tb |
| Pattern matching | `match value { ... }` | All files |
| Decorators | `@derive()`, `@test`, `@perf`, `@mock` | All files |
| Shared state | `Shared<T>`, `.read()`, `.write()` | store.tb, routes.tb |
| Arrow functions | `(x) => x.value` | store.tb, routes.tb |
| String interpolation | `"Hello {name}"` | All files |
| Guard statements | `guard condition else { ... }` | store.tb, agent.tb |
| Pipe operator | `data \|> filter \|> sort` | agent.tb |
| Destructuring | `let { name, age } = user` | api_test.tb |
| Tool functions | `tool fn name() { ... }` | agent.tb |
| Agent declaration | `agent Name { model, tools, ... }` | agent.tb |
| Async/await | `async fn`, `await`, `all()` | All files |
| Streaming | `for await token in stream { }` | agent.tb |
| Structured logging | `log.info("msg", { key: val })` | main.tb, routes.tb |
| Defer | `defer { cleanup() }` | main.tb |
