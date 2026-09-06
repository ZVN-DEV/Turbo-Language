# G2.1 initial diagnostic baseline — 2026-09-06

Measured with evaluator revision `75f27e5e293c7c7f89e64ca06621287ab24600b3`
on Apple M5 Max / macOS26.5.1. This is an **incomplete diagnostic subset**,
not G2.1 completion, a Rust-parity claim, or a performance regression comparison.

Reproduce from that revision with a release Turbo compiler:

```sh
python3 benchmarks/evaluator.py --output benchmarks/results/a-new-run-name
```

Protocol: three batches, three warmup pairs per case/batch,20 measured pairs
per case/batch, randomized execution order,2000 hierarchical bootstrap draws.
Both cases have60 measured pairs. The [raw log](samples.jsonl) has281 process
records, including builds, input generation, warmups and measurements. Every
runtime output matched its independent oracle with empty stderr.

| Case | Category | Turbo median wall | Rust median wall | Median paired elapsed ratio | Median peak RSS Turbo / Rust |
|---|---|---:|---:|---:|---:|
| Recursive fib(40) | CPU |233.31ms |161.09ms |1.444× |1.453 /1.562MiB |
| Existing5MiB word count | Application |88.62ms |22.64ms |3.871× |52.031 /6.719MiB |

The ratio estimator uses within-pair ratios; it need not equal the quotient of
the separately reported medians. Bootstrap95% elapsed-ratio intervals are
[1.4325,1.4618] for Fibonacci and [3.7976,3.9549] for word count. Complete precision,
flags, tool versions, input/fixture/binary hashes and samples are in [report.json](report.json).

Important limits:

- Peak RSS is not allocation count, live payload memory or a process-tree sum.
  Allocation/RC counters have not been implemented; the report preserves nulls.
- Word count is deliberately excluded from the CPU geometric mean: Turbo uses
  repeated top20 selection and owned token operations, while Rust sorts all
  entries and has a different token/storage path. Output equivalence is proven;
  identical implementations are not claimed.
- Samples below200ms are flagged; in-program batching is still needed for
  qualification. Host load/power settings, compiler source provenance and other
  target hosts are not independently attested.
- This collector uses a different timing method, paired estimator and explicit
  Rust native-CPU flags from older exploratory scripts. These numbers must not
  be described as an optimization gain or regression from the older tables.
- Nine fixtures and controlled-memory profiles remain pending. The CPU subset
  misses its target; overall qualification is **incomplete**, not pass.

Next evidence needs: allocation/live-byte/RC instrumentation, the remaining
current-capability workloads, sufficiently long in-program samples and host /
compiler provenance. See [evaluator contract](../../EVALUATOR.md) and the master
acceptance specification. The earlier pre-lock trial was kept in `/tmp` as an
exploratory diagnostic and is not used as this baseline.
