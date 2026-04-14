# web-api (BookmarkAPI) — Roadmap Spec

> **Status: aspirational.** This example is a design document, not runnable
> code. It uses syntax and language features that are not yet implemented
> in the current Turbo compiler. See `BRIEFING.md` for the full design
> write-up. Tracked under the P3 backlog.
>
> **Security note:** this folder is roadmap material, not production auth
> guidance. Do **not** cargo-cult its JWT, CORS, bind-host, or WebSocket auth
> sketches into a real service without a full security review.

A production-quality REST API for a social bookmarking service (think
Pinboard or Raindrop.io) built entirely in Turbo. Used as the canary for
"Turbo can express a complete, real-world web service with auth, real-time
sync, full-text search, and observability — with less code and more safety
than the Node.js / Express equivalent."

## What this example would demonstrate

- A composable middleware stack: `request_logger -> cors -> rate_limiter ->
  metrics_collector -> auth_required -> handler`, statically typed end to end
- A complete JWT lifecycle: registration, login, middleware extraction,
  logout / blacklist, and `require_ownership` guards on per-resource routes
- `Shared<T>` for per-middleware state (rate limit buckets, metrics
  counters) and `Atomic<T>` for lock-free counters
- WebSocket broadcasting for real-time bookmark sync
- Full-text search indexing
- Structured logging and Prometheus-style metrics
- Higher-order async functions, including the `(req, next) -> Response`
  middleware shape, fully type-checked
- Unit, integration, and performance test layers

## Run

This example does not currently run. The source uses `from`-style imports,
`Shared<T>` / `Atomic<T>` generics, optional chaining (`?.`), and a number
of standard-library types that the parser and sema do not yet recognize.

Once the language reaches the milestones described in `BRIEFING.md`, the
intended entry point will be:

```bash
turbolang run examples/roadmap/web-api/src/main.tb
```

## Expected output

An HTTP server on the configured port, plus a banner listing the registered
routes. (Sibling tests are designed to exercise the API end to end.)

## Caveats

- **Aspirational.** Do not edit this code expecting it to compile. The
  syntax intentionally runs ahead of the compiler.
- **Not production auth guidance.** The auth and WebSocket flows here are
  future-facing sketches. Real deployments should require an explicitly
  provisioned JWT secret, bind deliberately, scope CORS origins narrowly,
  and use secure cookies or short-lived upgrade tickets instead of putting
  bearer tokens in a WebSocket URL.
- **For P3 follow-up.** Bringing this example back to runnable will
  require shipping the import system, the `Shared<T>` / `Atomic<T>`
  primitives, optional chaining, and a much larger standard library
  surface (JWT, bcrypt, fts, websocket, prometheus).
- The simpler `examples/speed-server` example shows what is currently
  possible with Turbo's bundled HTTP runtime today.
