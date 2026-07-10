# Serverless cold-start benchmarks

Reproducible scripts measuring cold-start behavior of Turbo vs Node.js vs
Python on AWS Lambda, plus a local process-start proxy metric that needs no
cloud account.

**Rule: this repo publishes no benchmark numbers that these scripts did not
generate.** Run them yourself; your account, region, and the day's hardware
all move the numbers. If you paste results into a doc or the website, link
the script and the run date.

## What is measured

| Script | Metric | Needs |
|--------|--------|-------|
| `lambda/bench.sh` | Lambda **Init Duration** (the platform-reported cold-start cost) and billed duration over N forced cold starts per runtime | AWS CLI, an account |
| `local_cold_start.sh` | Process start → first HTTP response, locally | turbolang, node, python3 |

The local metric is a *proxy* — it excludes Lambda's sandbox provisioning and
image pull, which affect all runtimes roughly equally. It exists so anyone
can sanity-check the shape of the comparison in seconds.

## Lambda benchmark

```bash
cd benchmarks/serverless/lambda
./deploy.sh <role-arn>     # creates turbo-bench, node-bench, python-bench
./bench.sh 10              # 10 forced cold starts each; writes results.csv
./teardown.sh              # deletes the three functions
```

`bench.sh` forces a cold start per iteration by flipping an env var
(`BENCH_EPOCH`), which makes Lambda provision a fresh sandbox, then parses
`Init Duration` and `Billed Duration` from the invocation's `REPORT` log tail.
Results land in `results.csv` as
`runtime,iteration,init_ms,duration_ms,billed_ms,memory_max_mb`.

All three functions are configured identically: 128MB, `x86_64`,
same trivial JSON-echo handler.

## Local proxy benchmark

```bash
cd benchmarks/serverless
./local_cold_start.sh 10   # 10 rounds; prints per-runtime min/median
```

Starts each of three equivalent HTTP servers (Turbo AOT binary, `node`,
plain `python3` script), polls until the first successful response, records
the elapsed time, kills the process, repeats.

Absolute values include a fixed measurement floor (curl polling at 5ms
intervals, process-spawn overhead) that is identical across runtimes — the
*differences between runtimes* are the signal, not the absolute numbers.
