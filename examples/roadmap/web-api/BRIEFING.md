# BookmarkAPI: Turbo Web API Example

## What This App Demonstrates

BookmarkAPI is a production-quality REST API for a social bookmarking service (similar to Pinboard or Raindrop.io), built entirely in Turbo. It demonstrates that Turbo can express a complete, real-world web service with authentication, real-time sync, full-text search, and observability -- all with less code and more safety than the Node.js/Express equivalent.

This is not a toy example. It covers the patterns a production API actually needs: JWT auth with token revocation, rate limiting, pagination, ownership checks, input validation, search indexing, WebSocket broadcasting, structured logging, and metrics. Every feature is tested with unit, integration, and performance tests.

---

## Key Design Patterns

### 1. Middleware Stack

```
request_logger() -> cors() -> rate_limiter() -> metrics_collector() -> auth_required() -> handler
```

Turbo middleware composes as a list of `Middleware` functions passed to `Server.new()`. Each middleware is an `async (req, next) -> Response` function that can inspect/modify the request, call `next`, and then inspect/modify the response. This is identical to the Express/Koa mental model but with static typing -- the middleware function signature is checked at compile time.

**Key Turbo features shown:** Higher-order async functions, `Shared<T>` for per-middleware state (rate limit buckets, metrics counters), `Atomic<T>` for lock-free counters.

### 2. Authentication Flow

The auth system demonstrates a complete JWT lifecycle:
- **Registration:** Validate input -> hash password (bcrypt) -> store user -> sign JWT -> return token
- **Login:** Lookup user -> verify password -> sign JWT -> return token
- **Middleware:** Extract Bearer token -> check blacklist -> verify JWT -> inject claims into `req.state`
- **Logout:** Add token to blacklist -> revoked tokens rejected by middleware
- **Ownership:** `require_ownership()` guard checks `req.state.user_id` against the resource's owner

**Key Turbo features shown:** `const fn` for configuration constants, `T ! E` error propagation with `?` operator, `guard` statements for validation, pattern matching on `ok()`/`err()` for HTTP response mapping, `Shared<T>` for the token blacklist.

### 3. WebSocket Real-Time Sync

When a user creates, updates, or deletes a bookmark via HTTP, the change is broadcast to all of that user's other connected devices via WebSocket. The `SyncBroadcaster` maintains a registry of `{device_id: WsClient}` connections, protected by `Shared<T>`.

The WebSocket handler:
1. Authenticates via query parameter (since WS can't use custom headers easily)
2. Registers the connection
3. Spawns a heartbeat coroutine with `spawn async { loop { ... } }`
4. Processes incoming messages with `for await msg in socket.messages()`
5. Cleans up with `defer { broadcaster.remove_client(device_id) }`

**Key Turbo features shown:** `for await` on async streams, `spawn` for background tasks, `defer` for cleanup, `WsMessage` pattern matching, `Shared<T>` for concurrent connection registry.

### 4. Pagination with Generics

`PaginatedResponse<T>` is a generic struct that wraps any list of items with pagination metadata. The `from_slice()` constructor takes a full list plus page/per_page parameters and returns the correct slice with total count and page info. This single generic type is reused for bookmark lists, search results, and could serve any future paginated endpoint.

**Key Turbo features shown:** Generic structs (`PaginatedResponse<T>`), `impl<T>` blocks, computed fields, integer/float casting.

### 5. Full-Text Search with TF-IDF

The search engine maintains an inverted index (`{str: [Posting]}`) that maps terms to the bookmarks containing them. Each posting carries a term frequency, field name, and field boost. Search queries are tokenized, stop-words are filtered, and results are scored with TF-IDF plus field boosting (tags 4x, titles 3x, descriptions 1.5x, URLs 0.5x). Prefix matching provides partial-query support.

**Key Turbo features shown:** Pipe operator (`|>`) for data transformation chains, `group_by`, `map_values`, `unique`, arrow functions in `filter`/`map`/`reduce`, math operations, `Shared<T>` for the index.

---

## Turbo Feature -> Real-World Need Mapping

| Turbo Feature | Real-World Need |
|---|---|
| `T ! E` (typed errors) | Every API operation can fail; the type system forces you to handle `NotFound`, `Unauthorized`, `BadRequest`, etc. You cannot forget an error case. |
| `T?` (optionals) | Update DTOs (`UpdateBookmark`) have all-optional fields. `none` means "don't change this field". No nulls sneaking into required fields. |
| `Shared<T>` | Thread-safe state for user store, bookmark store, rate limit buckets, token blacklist, WebSocket registry, metrics. No data races, no manual locks. |
| `Atomic<T>` | Lock-free counters for total requests, active requests, active WS connections. High-performance metrics without contention. |
| `@derive(Schema, Serialize)` | Auto-generated JSON serialization and OpenAPI schema from struct definitions. Zero boilerplate. |
| `guard` statements | Input validation reads naturally: `guard !url.is_empty() else { return err(...) }`. Fail early and clearly. |
| `match` on `ok()`/`err()` | Map domain errors to HTTP status codes directly at the route handler level. Each error variant gets its own status code and message. |
| `async`/`await` + `spawn` | Non-blocking I/O for HTTP handlers, background cleanup tasks (blacklist GC), heartbeat coroutines. |
| `for await` | Process WebSocket message streams and search result streams lazily. |
| `defer` | Graceful shutdown (stop server, cancel background tasks) and WebSocket cleanup (remove client on disconnect). |
| Pipe operator (`\|>`) | Data transformation chains for tag cloud aggregation, search result scoring, filter pipelines. |
| Arrow functions | Concise callbacks in `filter`, `map`, `sort_by`, `group_by` -- used throughout the codebase. |
| `const fn` | Compile-time configuration constants (JWT secret, bcrypt cost, token expiry). |
| `@test`, `@perf`, `@stress` | Comprehensive testing: unit tests, integration tests with `MockServer`, performance benchmarks with memory/time limits, stress tests with configurable concurrency. |
| `"text {var}"` interpolation | Structured log messages, error messages, URL construction, header values. |

---

## Performance Characteristics

### What the Tests Verify

**Bulk operations (`@perf` tests):**
- 10,000 bookmark creations in under 2 seconds, under 100 MB
- 10,000 search index insertions in under 3 seconds, under 200 MB
- Paginated reads at 1,000 bookmarks x 100 iterations in under 500ms
- Full reindex of 2,000 documents in under 200ms

**Memory safety:**
- Create-and-delete cycles leave no leaked memory (within 50 KB tolerance)
- Search index add-and-remove cycles leave no leaked memory
- Update-in-place frees old data properly

**Concurrent access (`@stress` tests):**
- 50 concurrent workers performing full CRUD cycles for 10 seconds with zero errors and P99 < 20ms
- 30 concurrent workers mixing 70% search reads, 20% list reads, and 10% writes for 5 seconds with zero errors and P99 < 50ms
- 500 concurrent HTTP requests to the list endpoint all succeed

**Scaling characteristics:**
- Bookmark insertion time does not degrade more than 3x as the store grows from 1,000 to 10,000 entries (no O(n^2) behavior)
- Search query time at 5,000 documents is no more than 5x slower than at 500 documents (sublinear scaling)
- Pagination to the last page is no more than 3x slower than the first page

---

## Turbo vs Node.js/Express: A Brief Comparison

### The same app in Express would require:

```
express               - framework
jsonwebtoken          - JWT sign/verify
bcryptjs              - password hashing
express-rate-limit    - rate limiting
cors                  - CORS middleware
ws                    - WebSocket
morgan/pino           - logging
prom-client           - metrics
joi/zod               - input validation
jest/mocha            - testing
supertest             - HTTP test client
artillery             - load testing
```

That is 12+ npm packages, each with its own API surface, versioning, and security posture. In Turbo, everything is built in or covered by the standard library (`turbo/http`, `turbo/ws`, `turbo/crypto`, `turbo/sync`, `turbo/log`, `turbo/test`).

### Key differences:

| Concern | Express/Node.js | Turbo |
|---|---|---|
| **Error handling** | Uncaught exceptions crash the process. You must remember try/catch everywhere, or use error-handling middleware. Nothing forces you to handle specific error types. | `T ! E` forces exhaustive error handling at compile time. The `?` operator propagates errors. `match` on error variants maps to HTTP status codes. |
| **Null safety** | `undefined`, `null`, optional chaining (`?.`), and runtime crashes. No compile-time guarantee a field exists. | `T?` optionals. `none` is a first-class value. `guard let some(x) = ...` for safe unwrapping. No null reference exceptions. |
| **Concurrency** | Single-threaded event loop. Shared state requires careful discipline (or Redis). Race conditions are easy to introduce. | `Shared<T>` provides compile-time-checked thread-safe access. `Atomic<T>` for lock-free counters. The type system prevents data races. |
| **Type safety** | TypeScript helps but is optional and can be bypassed. Runtime type errors in production. | Full static typing. `@derive(Schema)` generates validation from types. No runtime type mismatches. |
| **Testing** | Separate packages for HTTP testing, mocking, load testing. Configuration varies per tool. | `@test`, `@perf(max_time, max_memory)`, `@stress(duration, concurrency)` are built-in decorators. `MockServer` and `TestClient` are standard library. |
| **WebSocket** | Requires `ws` package, manual connection tracking, no built-in broadcast pattern. | `router.ws()` for upgrade, `for await msg in socket.messages()` for processing, pattern matching on message types. |
| **Performance** | V8 JIT is fast for I/O, but GC pauses and single-thread bottlenecks limit throughput. | Native compilation. No GC pauses. True parallelism via lightweight threads. `Shared<T>` read/write locks allow concurrent readers. |

### Lines of code (approximate):

| Component | Express + TypeScript | Turbo |
|---|---|---|
| Models + validation | ~200 (types) + ~150 (Zod schemas) | ~250 (structs with `@derive`) |
| Auth (JWT + bcrypt + middleware) | ~180 | ~160 |
| Routes (bookmarks + users) | ~350 | ~400 |
| Middleware (rate limit, CORS, logging, metrics) | ~250 | ~270 |
| Search engine | ~200 | ~200 |
| WebSocket | ~150 | ~170 |
| Tests | ~400 | ~600 (includes perf + stress) |
| **Total** | **~1,880** | **~2,050** |

The line counts are comparable, but the Turbo version includes performance tests, stress tests, memory leak detection, and compile-time safety guarantees that the Express version does not.

---

## File Overview

```
web-api/
  turbo.toml                  # Project configuration
  BRIEFING.md                 # This document
  src/
    main.tb                   # Server entry point, config, middleware stack, routing
    models.tb                 # All data types, DTOs, errors, generic pagination
    auth.tb                   # JWT auth, password hashing, auth middleware, token blacklist
    middleware.tb              # Rate limiter, CORS, request logger, metrics collector
    search.tb                 # Full-text search engine with TF-IDF scoring
    routes/
      bookmarks.tb            # Bookmark CRUD, search, tag cloud, pagination
      users.tb                # Registration, login, logout, profile, settings
      ws.tb                   # WebSocket sync handler, connection registry, broadcasting
  tests/
    auth_test.tb              # JWT, password hashing, middleware unit tests
    api_test.tb               # Full HTTP integration tests for all endpoints
    perf_test.tb              # Bulk ops, memory leaks, concurrency stress, scaling
```
