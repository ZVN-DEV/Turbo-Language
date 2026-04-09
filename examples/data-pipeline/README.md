# data-pipeline

A log analysis pipeline that generates a sample server log, writes it to disk,
reloads it, and produces a multi-section report: log level distribution, error
detail listing, HTTP method breakdown, status code histogram, endpoint
frequency via a `hashmap`, warning timeline, and a final health summary.

## Turbo features shown

- `read_file` / `write_file` for round-tripping data through `/tmp`
- `split` / `join` / `contains` for line-oriented parsing
- `hashmap()` + `hashmap_set` for endpoint frequency counts
- `for ... in` loops over arrays of strings
- String interpolation in `print` (`"  INFO:  {info_count}"`)
- `if` expressions used as values (`let health = if error_count == 0 { ... }`)
- `repeat` for ASCII bar charts and section dividers

## Run

```bash
turbolang run examples/data-pipeline/main.tb
```

## Expected output

```
=======================================================
  Turbo Data Pipeline -- Log Analyzer
=======================================================

  Generated 20 log entries -> /tmp/turbo_logs.txt
  Loaded 20 lines

-------------------------------------------------------
  Log Level Distribution
-------------------------------------------------------
  INFO:  13  #############
  WARN:  4    ####
  ERROR: 3    ###
  Total: 20

-------------------------------------------------------
  Error Details
-------------------------------------------------------
  1. [08:03:15] ERROR POST /api/orders 500 230ms
  2. [08:05:15] ERROR GET /api/reports 503 5000ms
  3. [08:05:16] ERROR Upstream timeout analytics-svc
  ...
-------------------------------------------------------
  Summary
-------------------------------------------------------
  System health:  DEGRADED
  Throughput:     11 requests in log window
  Success rate:   10/11 requests
  Action needed:  3 errors require investigation

=======================================================
  Pipeline complete!
=======================================================
```

## Caveats

- Writes to `/tmp/turbo_logs.txt` — Windows path layout would need adjusting
  (Windows is not currently a supported target).
- Counts are derived from substring matches, not a real log parser, so the
  "WARN" total includes the literal `WARN ` token only.
