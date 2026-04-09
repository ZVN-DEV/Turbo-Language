# web-dashboard

An interactive HTML benchmark dashboard served entirely from a single Turbo
file. Visiting `http://localhost:3000` returns a styled dark-mode UI with
five "Run" cards (fibonacci, prime counter, bubble sort, sieve of
Eratosthenes, and a zero-cost language-info endpoint). Clicking any card —
or "Run All Benchmarks" — `fetch`es the matching `/api/*` route and prints
the JSON response and elapsed time.

## Turbo features shown

- `http_server(3000)` + `route(...)` + `http_listen` for serving HTML and
  JSON from the same process
- A pure-Turbo HTML+CSS+JS builder (`build_css`, `build_js`, `build_card`,
  `build_html`) using string concatenation and `\{` / `\}` escapes inside
  string literals to emit literal `{` / `}` for JS/CSS
- Closure handlers per route: `|req: str| -> str { respond(200, body) }`
- Recursive `fib`, trial-division `is_prime` / `count_primes`, hashmap-based
  `sieve_count`, and `bubble_sort` for the benchmark workloads
- Mutable arrays (`let mut arr = arr`) with index assignment
- `hashmap()` + `hashmap_set` / `hashmap_has` + `to_str` for the sieve

## Run

```bash
turbolang run examples/web-dashboard/main.tb
# then open http://localhost:3000 in a browser
```

You can also hit the JSON endpoints directly:

```bash
curl http://localhost:3000/api/fib
curl http://localhost:3000/api/primes
curl http://localhost:3000/api/sort
curl http://localhost:3000/api/sieve
curl http://localhost:3000/api/info
```

## Expected output

The server prints a banner and then blocks on `http_listen`:

```
==============================================
  Turbo Interactive Benchmark Dashboard
  http://localhost:3000
==============================================

Routes:
  GET /            - Dashboard UI
  GET /api/fib     - Fibonacci(35)
  GET /api/primes  - Count primes < 10,000
  GET /api/sort    - Bubble sort 500 integers
  GET /api/sieve   - Sieve of Eratosthenes < 100,000
  GET /api/info    - Language info

Dashboard ready. Press Ctrl+C to stop.
```

Sample response from `GET /api/sieve`:

```json
{"benchmark": "sieve_eratosthenes", "limit": 100000, "prime_count": 9592}
```

## Caveats

- The bubble sort over 500 reverse-sorted integers is intentionally slow.
  It's the slowest button on the dashboard by design.
- Inline CSS / JS uses `\{` and `\}` because braces inside string literals
  would otherwise look like Turbo string interpolation. This is the
  current escape syntax — expect a cleaner option later.
- Stop with `Ctrl+C`.
