# JSON release-closeout diagnostic — 2026-09-07

This is a post-fix execution/allocation check for the v0.16.0 release line,
**not performance qualification or a before/after speed comparison**.

The full frozen input (2048 records × 256 rounds) passed the independent full
serialized-record multiset oracle in ordinary AOT and Rust, then in separately
instrumented AOT/JIT runs. Both instrumented modes reported exactly:

- 6,668,371 shared-header allocations and frees;
- zero live tracked allocations, data bytes and header bytes at entry return;
- 3298 peak live tracked allocations and 699,968 peak tracked data bytes;
- valid observer state, zero errors and zero unknown frees.

The initial four-record regression had retained 16 allocations / 259 data bytes.
The fix releases serializer concat intermediates and owned temporary inputs;
the regression covers both object and array serialization. These counters are
**shared-header ARC only**, not the whole heap, cycle collection, closure lifetime
coverage, or a general guarantee that every long-running program is bounded.

One warmup pair and one measured pair ran on the local macOS ARM64 host. The
measured pair was 3031.634 ms Turbo / 538.694 ms Rust (5.628×). One-pair intervals
are not meaningful confidence evidence. Host load was not controlled; other
verification and unrelated applications may have run. Do not use this result to
claim a speed improvement/regression or broad relative performance.

The API paths still differ: Turbo revalidates each `json_get` and performs map
get/set, while Rust parses once into `serde_json::Value` and uses map entry.
That difference is an explicit qualification blocker. All qualification remains
**incomplete**. Controlled APIs, full allocation coverage, other workloads,
sampling minimums and cross-host/source attestation are still pending.

The Rust example now builds for the selected Rust compiler's explicit host
target. The evaluator selects the executable from Cargo's source-matched JSON
artifact message and records compiler/flag/wrapper overrides, rather than
assuming an output path. Its cached workspace build time is not a clean minimal
Rust compile-time comparison.

`report.json` and `samples.jsonl` are unchanged output from the run. Their
provenance records base revision `308425e` plus the dirty release-closeout tree;
this is not a claim that the binary came from the clean base revision. Fixture
hashes, commands, binary hashes, raw outputs and all exclusions are retained.

Reproduce with separate standard and allocation-profile compilers:

```sh
python3 benchmarks/evaluator.py --cases json_transform \
  --samples 1 --batches 1 --warmups 1 --bootstrap 100 --profile-samples 1 \
  --compiler /path/to/standard/turbolang \
  --profile-compiler /path/to/profile/turbolang \
  --output /tmp/a-new-json-release-diagnostic
```

See [the evaluator contract](../../EVALUATOR.md) for supported build methods,
profile coverage and the distinction between a completed run and qualification.
