#!/usr/bin/env bash
# Local cold-start proxy: process start → first successful HTTP response,
# for equivalent hello-JSON servers in Turbo (AOT binary), Node, and Python.
#
# This is NOT a Lambda measurement (no sandbox provisioning) — it's the
# runnable-anywhere sanity check for the shape of the comparison. For the
# real thing, see lambda/bench.sh.
#
# Usage: ./local_cold_start.sh [rounds]   (requires: turbolang, node, python3, curl)
set -euo pipefail
cd "$(dirname "$0")"

N="${1:-10}"
PORT=18990
WORK=$(mktemp -d -t turbo-coldstart)
trap 'rm -rf "$WORK"' EXIT

# --- equivalent servers ------------------------------------------------------

cat > "$WORK/server.tb" <<'EOF'
fn main() {
    let app = http_server(18990)
    route(app, "GET", "/", |req: str| -> str { respond_json(200, "\{\"ok\":true\}") })
    http_listen(app)
}
EOF

cat > "$WORK/server.mjs" <<'EOF'
import { createServer } from "node:http";
createServer((req, res) => {
  res.setHeader("content-type", "application/json");
  res.end('{"ok":true}');
}).listen(18990, "127.0.0.1");
EOF

cat > "$WORK/server.py" <<'EOF'
from http.server import BaseHTTPRequestHandler, HTTPServer

class H(BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def do_GET(self):
        body = b'{"ok":true}'
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

HTTPServer(("127.0.0.1", 18990), H).serve_forever()
EOF

echo "compiling Turbo server (AOT)..."
turbolang build "$WORK/server.tb" -o "$WORK/server-turbo" >/dev/null

# --- measurement -------------------------------------------------------------

# Start the command, poll / until it answers, print elapsed ms, kill it.
measure_once() { # cmd...
    local start end pid
    start=$(python3 -c 'import time; print(time.time_ns())')
    "$@" >/dev/null 2>&1 &
    pid=$!
    while ! curl -s -o /dev/null --max-time 0.2 "http://127.0.0.1:$PORT/"; do
        if ! kill -0 "$pid" 2>/dev/null; then echo "server died" >&2; return 1; fi
        sleep 0.005
    done
    end=$(python3 -c 'import time; print(time.time_ns())')
    kill "$pid" 2>/dev/null; wait "$pid" 2>/dev/null || true
    # wait for the port to free up before the next round
    while curl -s -o /dev/null --max-time 0.2 "http://127.0.0.1:$PORT/"; do sleep 0.02; done
    echo $(( (end - start) / 1000000 ))
}

run_series() { # label cmd...
    local label="$1"; shift
    local times=()
    for _ in $(seq 1 "$N"); do
        times+=("$(measure_once "$@")")
    done
    printf '%s' "${times[@]/#/ }" | awk -v l="$label" '{
        n = split($0, a, " ")
        asort_min = a[1]; sum = 0
        for (i = 1; i <= n; i++) { if (a[i] < asort_min) asort_min = a[i]; sum += a[i] }
        # median via sort
        for (i = 1; i <= n; i++) for (j = i+1; j <= n; j++) if (a[j] < a[i]) { t=a[i]; a[i]=a[j]; a[j]=t }
        med = (n % 2) ? a[(n+1)/2] : (a[n/2] + a[n/2+1]) / 2
        printf "%-8s min %4d ms   median %6.1f ms   (n=%d)\n", l, asort_min, med, n
    }'
}

echo "rounds per runtime: $N"
run_series turbo  "$WORK/server-turbo"
run_series node   node "$WORK/server.mjs"
run_series python python3 "$WORK/server.py"
