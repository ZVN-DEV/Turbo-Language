#!/bin/bash
# Master Benchmark Runner
# Runs all language benchmarks and collects results into a unified JSON report
# Usage: ./run_all.sh [language1 language2 ...] or ./run_all.sh (runs all)

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RESULTS_FILE="$SCRIPT_DIR/results.json"
RESULTS_CSV="$SCRIPT_DIR/results.csv"
TIMESTAMP=$(date -u +"%Y-%m-%dT%H:%M:%SZ")

# All available languages
ALL_LANGUAGES=(rust go cpp swift zig typescript python java kotlin csharp dart scala ruby elixir julia)

# Use specified languages or all
if [ $# -gt 0 ]; then
  LANGUAGES=("$@")
else
  LANGUAGES=("${ALL_LANGUAGES[@]}")
fi

echo "============================================"
echo "  Multi-Language Benchmark Suite"
echo "  $(date)"
echo "============================================"
echo ""
echo "Languages to benchmark: ${LANGUAGES[*]}"
echo "Results will be saved to: $RESULTS_FILE"
echo ""

# Collect all JSON results
ALL_RESULTS=()
FAILED_LANGUAGES=()
SKIPPED_LANGUAGES=()

for lang in "${LANGUAGES[@]}"; do
  LANG_DIR="$SCRIPT_DIR/$lang"
  RUN_SCRIPT="$LANG_DIR/run.sh"

  echo "--------------------------------------------"
  echo "  Running: $lang"
  echo "--------------------------------------------"

  if [ ! -d "$LANG_DIR" ]; then
    echo "  SKIP: Directory $LANG_DIR not found"
    SKIPPED_LANGUAGES+=("$lang")
    continue
  fi

  if [ ! -f "$RUN_SCRIPT" ]; then
    echo "  SKIP: No run.sh found in $LANG_DIR"
    SKIPPED_LANGUAGES+=("$lang")
    continue
  fi

  # Make run.sh executable
  chmod +x "$RUN_SCRIPT"

  # Run the benchmark and capture JSON output lines
  cd "$LANG_DIR"
  if OUTPUT=$(bash run.sh 2>&1); then
    # Extract JSON lines (lines starting with {)
    JSON_LINES=$(echo "$OUTPUT" | grep '^\s*{' || true)
    if [ -n "$JSON_LINES" ]; then
      while IFS= read -r line; do
        ALL_RESULTS+=("$line")
        # Print inline
        echo "  $line"
      done <<< "$JSON_LINES"
    else
      echo "  WARNING: No JSON output captured"
      echo "  Raw output:"
      echo "$OUTPUT" | head -20
      FAILED_LANGUAGES+=("$lang")
    fi
  else
    echo "  FAILED: Benchmark for $lang failed"
    echo "  Output:"
    echo "$OUTPUT" | tail -20
    FAILED_LANGUAGES+=("$lang")
  fi
  cd "$SCRIPT_DIR"
  echo ""
done

# Write unified JSON results
echo "============================================"
echo "  Writing Results"
echo "============================================"

# Create JSON array
{
  echo "{"
  echo "  \"timestamp\": \"$TIMESTAMP\","
  echo "  \"platform\": \"$(uname -s) $(uname -m)\","
  echo "  \"results\": ["

  for i in "${!ALL_RESULTS[@]}"; do
    if [ $i -lt $((${#ALL_RESULTS[@]} - 1)) ]; then
      echo "    ${ALL_RESULTS[$i]},"
    else
      echo "    ${ALL_RESULTS[$i]}"
    fi
  done

  echo "  ],"
  echo "  \"failed\": [$(printf '"%s",' "${FAILED_LANGUAGES[@]}" | sed 's/,$//')],"
  echo "  \"skipped\": [$(printf '"%s",' "${SKIPPED_LANGUAGES[@]}" | sed 's/,$//')]"
  echo "}"
} > "$RESULTS_FILE"

echo "JSON results saved to: $RESULTS_FILE"

# Create CSV for easy spreadsheet import
{
  echo "language,benchmark,time_ms,result"
  for result in "${ALL_RESULTS[@]}"; do
    # Parse JSON fields (basic parsing with sed/awk)
    lang=$(echo "$result" | sed 's/.*"language":"\([^"]*\)".*/\1/')
    bench=$(echo "$result" | sed 's/.*"benchmark":"\([^"]*\)".*/\1/')
    time_ms=$(echo "$result" | sed 's/.*"time_ms":\([0-9.]*\).*/\1/')
    res=$(echo "$result" | sed 's/.*"result":"\([^"]*\)".*/\1/')
    echo "$lang,$bench,$time_ms,$res"
  done
} > "$RESULTS_CSV"

echo "CSV results saved to: $RESULTS_CSV"

# Print summary table
echo ""
echo "============================================"
echo "  Summary"
echo "============================================"
echo ""
printf "%-14s %-12s %12s\n" "LANGUAGE" "BENCHMARK" "TIME (ms)"
printf "%-14s %-12s %12s\n" "----------" "----------" "----------"

for result in "${ALL_RESULTS[@]}"; do
  lang=$(echo "$result" | sed 's/.*"language":"\([^"]*\)".*/\1/')
  bench=$(echo "$result" | sed 's/.*"benchmark":"\([^"]*\)".*/\1/')
  time_ms=$(echo "$result" | sed 's/.*"time_ms":\([0-9.]*\).*/\1/')
  printf "%-14s %-12s %12s\n" "$lang" "$bench" "$time_ms"
done

echo ""
if [ ${#FAILED_LANGUAGES[@]} -gt 0 ]; then
  echo "FAILED: ${FAILED_LANGUAGES[*]}"
fi
if [ ${#SKIPPED_LANGUAGES[@]} -gt 0 ]; then
  echo "SKIPPED: ${SKIPPED_LANGUAGES[*]}"
fi

echo ""
echo "Total benchmarks run: ${#ALL_RESULTS[@]}"
echo "Done."
