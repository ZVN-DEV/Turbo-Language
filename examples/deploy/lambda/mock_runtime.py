#!/usr/bin/env python3
"""Minimal mock of the AWS Lambda Runtime API for local testing.

Serves N invocation events on GET /2018-06-01/runtime/invocation/next
(with the same headers the real API sends), records the function's
POSTed responses, writes them to a JSON file, then shuts down.

Usage: mock_runtime.py <port> <out-file> [n-events]
"""
import json
import sys
import threading
from http.server import BaseHTTPRequestHandler, HTTPServer

PORT = int(sys.argv[1])
OUT = sys.argv[2]
N = int(sys.argv[3]) if len(sys.argv) > 3 else 2

events = [{"name": f"invocation-{i + 1}"} for i in range(N)]
received = []
served = 0


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_GET(self):
        global served
        if self.path == "/2018-06-01/runtime/invocation/next" and served < N:
            body = json.dumps(events[served]).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Lambda-Runtime-Aws-Request-Id", f"req-{served + 1}")
            self.send_header("Lambda-Runtime-Deadline-Ms", "9999999999999")
            self.send_header(
                "Lambda-Runtime-Invoked-Function-Arn",
                "arn:aws:lambda:local:000000000000:function:mock",
            )
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            served += 1
        else:
            self.send_response(404)
            self.send_header("Content-Length", "0")
            self.end_headers()

    def do_POST(self):
        length = int(self.headers.get("Content-Length", 0))
        received.append({"path": self.path, "body": self.rfile.read(length).decode()})
        self.send_response(202)
        self.send_header("Content-Length", "2")
        self.end_headers()
        self.wfile.write(b"{}")
        if len(received) >= N:
            with open(OUT, "w") as f:
                json.dump(received, f, indent=2)
            threading.Thread(target=server.shutdown).start()


server = HTTPServer(("127.0.0.1", PORT), Handler)
print(f"mock runtime API on 127.0.0.1:{PORT}, serving {N} events", flush=True)
server.serve_forever()
