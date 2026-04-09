# realtime-system (TurboExchange) — Roadmap Spec

> **Status: aspirational.** This example is a design document, not runnable
> code. It uses syntax and language features that are not yet implemented
> in the current Turbo compiler. See `BRIEFING.md` for the full design
> write-up. Tracked under the P3 backlog.

A real-time financial order matching engine, used as the canary for "Turbo
can hit the strictest latency and correctness requirements of any software
system" — sub-microsecond order matching, deterministic memory, actor
isolation, and zero-allocation hot paths.

## What this example would demonstrate

- The full memory ladder Turbo is targeting:
  - **Level 0 (auto-clone)** for the REST API, config loader, and metrics
    dashboard — reads like TypeScript
  - **Level 1 (`Shared<T>` / `Atomic<T>`)** for the cross-actor state
    that the matching core publishes
  - **Level 2 (regions / arenas / `unsafe` zero-alloc paths)** for the
    matching engine's hot loop
- Pattern matching on order types and side, exhaustively checked
- Actor-style isolation between the matching core, the gateway, and the
  market-data publisher
- A REST + WebSocket API layered over the matching core
- Hard real-time guarantees: every operation must complete inside a fixed
  deadline, with no GC pauses, ever
- A test suite that asserts latency percentiles, not just correctness

## Run

This example does not currently run. The source uses optional chaining
(`?.`), region annotations, and the `Shared<T>` / `Atomic<T>` types,
none of which are in the present grammar.

Once the language reaches the milestones described in `BRIEFING.md`, the
intended entry point will be:

```bash
turbolang run examples/roadmap/realtime-system/src/main.tb
```

## Expected output

The matching engine starts, exposes its REST + WebSocket API, and prints a
banner. Live activity (orders accepted, trades printed, latency
percentiles) streams to stdout.

## Caveats

- **Aspirational.** Do not edit this code expecting it to compile. The
  syntax intentionally runs ahead of the compiler.
- **For P3 follow-up.** Bringing this example back to runnable will
  require shipping the memory ladder, region/arena annotations, the
  actor runtime, and a real-time scheduler — i.e. it represents the
  most ambitious slice of the Turbo roadmap.
- Don't take any of the latency numbers in `BRIEFING.md` as benchmarks
  of the current compiler.
