# speed-server

A small HTTP API that exposes a handful of CPU benchmarks (recursive
fibonacci, prime trial division, bubble sort) plus an info and a health
endpoint, all returning JSON. Acts as the "Turbo can serve real HTTP
traffic" demo and as the backend the `web-dashboard` example talks to in
spirit.

## Turbo features shown

- `http_server(port)` + `route(app, METHOD, PATH, handler)` + `http_listen`
- Closure handlers: `|req: str| -> str { respond(200, body) }`
- `struct` definitions used as JSON response shapes (`ServerInfo`,
  `FibResult`, `PrimeResult`, `HealthStatus`)
- `to_json(value)` for struct -> JSON serialization
- `respond(status, body)` for HTTP responses
- Recursive functions (`fib`), `while` loops (`is_prime`, `count_primes`)
- Mutable arrays + index assignment in `bubble_sort`

## Run

```bash
turbolang run examples/speed-server/main.tb
# in another terminal:
curl http://localhost:8080/
curl http://localhost:8080/api/fib
curl http://localhost:8080/api/primes
curl http://localhost:8080/api/sort
curl http://localhost:8080/api/health
```

## Expected output

The server prints a banner and then blocks on `http_listen`:

```
===========================================
  Turbo Speed Server v1.0
  Listening on http://localhost:8080
===========================================

Routes:
  GET /           - Server info
  GET /api/fib    - Fibonacci benchmark
  GET /api/primes - Prime sieve
  GET /api/sort   - Sorting benchmark
  GET /api/health - Health check

Server ready. Press Ctrl+C to stop.
```

Sample response from `GET /api/fib`:

```json
{"n": 35, "result": 9227465}
```

## Caveats

- The HTTP server is part of Turbo's bundled runtime — no external crate
  install needed, but the API is intentionally minimal (no middleware,
  no path params, no streaming bodies).
- Stop with `Ctrl+C`.
- `bubble_sort` is O(n^2) by design. Don't benchmark this against real
  sort implementations.
