# TurboServo Update — v0.5.1 to v0.7.1 + Performance Showcase

**Date:** 2026-04-09
**Status:** Approved
**Repo:** github.com/ZVN-DEV/turboservo (separate repo)
**Local:** /Users/macbookpro-kirby/Desktop/Coding/ZVN/turboservo

---

## 1. Overview

TurboServo is a lightweight HTTP server framework for Turbo Lang. Currently at v0.5.1, it needs to be updated to work with the Turbo compiler v0.7.1. Additionally, new performance showcase endpoints will demonstrate Turbo's speed on real data processing workloads.

## 2. Update Tasks

### Compatibility Update (v0.5.1 → v0.7.1)

1. Update `turbo.toml` version to `0.7.1`
2. Test all existing modules against v0.7.1 compiler:
   - `src/servo.tb` — server creation, route registration, listen
   - `src/request.tb` — request helpers
   - `src/response.tb` — response builders
   - `src/router.tb` — path parameter extraction
3. Fix any breaking changes from compiler updates (COW rewrite, type checking, etc.)
4. Verify all existing examples still work
5. Run the benchmark suite and update numbers

### Identify Breaking Changes

Turbo v0.5.1 → v0.7.1 changes that may affect TurboServo:
- COW rewrite pass (v0.7.0) — statement-position calls to push/map/filter now auto-rewrite
- Enhanced type checking — stricter sema may flag previously-allowed patterns
- New error codes (91 total) — may surface new warnings
- ARC memory management (v0.6.0) — rt_release is no longer a no-op

## 3. Performance Showcase Endpoints

New endpoints that process large datasets with complex operations to demonstrate Turbo's native speed. These should be impressive — the kind of workload where Python/Node take seconds and Turbo finishes in milliseconds.

### Endpoint: `POST /transform`
**Large dataset transformation pipeline.**
- Accepts JSON array of 10K-100K records
- Applies multi-stage transformation: parse → validate → normalize → aggregate → sort → paginate
- Returns transformed + paginated result
- Target: < 50ms for 10K records, < 500ms for 100K records

### Endpoint: `POST /analyze`
**Statistical analysis on numerical datasets.**
- Accepts JSON array of numbers (up to 1M values)
- Computes: mean, median, mode, stddev, percentiles (p50/p95/p99), histogram bins, outlier detection
- Returns full statistical summary as JSON
- Target: < 100ms for 1M values

### Endpoint: `POST /search`
**Full-text search across in-memory dataset.**
- On startup, loads a dataset into memory (e.g., 50K product records)
- Accepts search query with filters (fuzzy match, field filters, sort, pagination)
- Returns ranked results with relevance scores
- Target: < 10ms for fuzzy search across 50K records

### Endpoint: `GET /matrix`
**Matrix computation showcase.**
- Accepts matrix dimensions and operation (multiply, transpose, determinant, invert)
- Computes result using tight loops (no external libraries)
- Returns result matrix as JSON
- Target: 1000x1000 matrix multiply < 200ms

### Endpoint: `POST /pipeline`
**Chained data processing pipeline.**
- Accepts a pipeline definition: array of stages (filter, map, reduce, sort, group_by, join)
- Each stage operates on the output of the previous
- Demonstrates Turbo's closure and higher-order function performance
- Returns final result + per-stage timing breakdown
- Target: 5-stage pipeline on 50K records < 100ms

## 4. Benchmark Harness Update

Update `benchmarks/run_benchmarks.sh` to:
1. Include the new endpoints in the benchmark suite
2. Compare against equivalent Go and Bun/Hono implementations
3. Report latency percentiles (p50, p95, p99) not just averages
4. Test with increasing data sizes to show scaling behavior

## 5. Updated Project Structure

```
turboservo/
├── src/
│   ├── servo.tb              # Core server API (existing)
│   ├── request.tb            # Request helpers (existing)
│   ├── response.tb           # Response builders (existing)
│   ├── router.tb             # Path params (existing)
│   └── showcase/
│       ├── transform.tb      # /transform endpoint
│       ├── analyze.tb        # /analyze endpoint
│       ├── search.tb         # /search endpoint
│       ├── matrix.tb         # /matrix endpoint
│       └── pipeline.tb       # /pipeline endpoint
├── examples/
│   ├── hello/main.tb         # Simple hello world (existing)
│   └── showcase/main.tb      # Performance showcase server (NEW)
├── benchmarks/
│   ├── run_benchmarks.sh     # Updated benchmark runner
│   ├── api_bench.tb          # TurboServo benchmark server
│   ├── go_bench.go           # Go comparison
│   ├── hono_bench.ts         # Bun/Hono comparison
│   └── data/
│       ├── records_10k.json  # Test dataset (10K records)
│       ├── records_100k.json # Test dataset (100K records)
│       └── numbers_1m.json   # Test dataset (1M numbers)
├── turbo.toml                # Updated to v0.7.1
└── README.md                 # Updated with showcase docs
```

## 6. Success Criteria

- All existing TurboServo functionality works on v0.7.1
- Showcase endpoints handle specified data sizes within target latencies
- Benchmark suite shows Turbo competitive with or faster than Go on data processing
- No memory leaks in showcase endpoints (verify with arena scoping)
- README updated with new benchmark numbers
