# web-dashboard

**The flagship runnable Turbo demo.** This example serves an interactive HTML
benchmark dashboard entirely from a single Turbo file. Visit
`http://localhost:3000` and you get a styled dark-mode UI with five benchmark
cards (fibonacci, prime counter, bubble sort, sieve of Eratosthenes, and a
zero-cost language-info endpoint). Clicking any card — or **Run All
Benchmarks** — hits the matching `/api/*` route and prints the JSON response
and elapsed time.

If you want one example that proves Turbo can compile native code, serve a web
UI, and expose JSON APIs today, this is the one to run first.

## Quickstart

```bash
turbolang run examples/web-dashboard/main.tb
```

Then:

1. Open `http://localhost:3000` in your browser
2. Click **Run All Benchmarks**
3. Open `http://localhost:3000/api/info` in another tab to see a raw JSON route
4. Stop the server with `Ctrl+C`

## Turbo features shown

- `http_server(3000)` + `route(...)` + `http_listen` for serving HTML and
  JSON from the same process
- A pure-Turbo HTML+CSS+JS builder (`build_css`, `build_js`, `build_card`,
  `build_html`) using string concatenation and `\{` / `\}` escapes inside
  string literals to emit literal `{` / `}` for JS/CSS
- Explicit response helpers: `respond_html`, `respond_json`, and `respond_text`
- Recursive `fib`, trial-division `is_prime` / `count_primes`, hashmap-based
  `sieve_count`, and `bubble_sort` for the benchmark workloads
- Mutable arrays (`let mut arr = arr`) with index assignment
- `hashmap()` + `hashmap_set` / `hashmap_has` + `to_str` for the sieve

## JSON endpoints

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
