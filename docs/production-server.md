# Running the Turbo HTTP server in production

Turbo ships a small built-in HTTP server (`http_server`, `route`,
`http_listen`, `respond_*`, `request_*`). It is designed for **API and service
backends that sit behind a reverse proxy** — not as an internet-facing edge
server. This guide covers the concurrency model, the tunable limits, graceful
shutdown, and a sample nginx deployment.

## Concurrency model: thread-per-connection

The server is **thread-per-connection**. A single accept loop runs on the
thread that called `http_listen`; every accepted connection is handled on its
own OS thread, with HTTP/1.1 keep-alive and request pipelining. There is no
async event loop.

Practical consequences:

- Throughput is bounded by the OS thread scheduler and the connection cap
  (default 256 concurrent connections; see `max_connections`). This is ample
  for typical API workloads behind a proxy, not for tens of thousands of
  idle-but-open connections.
- A blocking handler blocks only its own connection's thread.
- Per-request temporaries are allocated in a per-request arena and reclaimed in
  bulk when the response is written; server state you keep in a hashmap created
  *before* the request persists correctly across requests.

## Run it behind nginx / Caddy

The server intentionally does **not** implement TLS, HTTP/2, virtual hosts,
compression, static-file serving, rate limiting, or request filtering. Put it
behind nginx, Caddy, or a cloud load balancer, which provide those and shield
the Turbo process from hostile traffic.

Bind the Turbo server to loopback (the default `http_server(port)`) and let the
proxy be the only public listener. Use `http_server_public(port)` only when you
deliberately need to bind all interfaces (e.g. inside a container network where
the proxy reaches it by address).

### Sample nginx location block

```nginx
# Terminate TLS and HTTP/2 at nginx; proxy to the loopback Turbo server.
upstream turbo_app {
    server 127.0.0.1:8080;
    keepalive 32;
}

server {
    listen 443 ssl http2;
    server_name api.example.com;

    ssl_certificate     /etc/ssl/certs/api.example.com.crt;
    ssl_certificate_key /etc/ssl/private/api.example.com.key;

    # Let nginx enforce the public-facing body limit; keep it <= the Turbo
    # max_body_bytes so oversized uploads are rejected at the edge.
    client_max_body_size 32m;

    location / {
        proxy_pass         http://turbo_app;
        proxy_http_version 1.1;
        proxy_set_header   Connection "";           # enable upstream keep-alive
        proxy_set_header   Host              $host;
        proxy_set_header   X-Real-IP         $remote_addr;
        proxy_set_header   X-Forwarded-For   $proxy_add_x_forwarded_for;
        proxy_set_header   X-Forwarded-Proto $scheme;

        proxy_connect_timeout 5s;
        proxy_read_timeout    30s;
        proxy_send_timeout    30s;
    }
}
```

Caddy is equivalent — a `reverse_proxy 127.0.0.1:8080` directive with automatic
HTTPS gives you the same shape with less configuration.

## Tuning limits with `http_config`

Call `http_config(key, value)` **before `http_listen`** to override a default.
It returns `1` on success and `0` for an unknown key or an invalid value (the
error is printed to stderr; the program never panics). Every value must be
`>= 1`.

| Key | Default | Meaning |
|-----|---------|---------|
| `max_body_bytes` | `33554432` (32 MiB) | Max request body. A larger `Content-Length` is rejected with `413 Payload Too Large`; a malformed/negative one with `400 Bad Request`. |
| `max_header_bytes` | `16384` (16 KiB) | Max total request-header bytes. Larger headers get `431 Request Header Fields Too Large`. Allowed range: 256 … 16 MiB. |
| `max_connections` | `256` | Max concurrent connections. Beyond this the server replies `503 Service Unavailable` and closes. |
| `read_timeout_ms` | `10000` | Socket read timeout while a request is being received. A stalled sender is dropped. |
| `write_timeout_ms` | `10000` | Socket write timeout for responses. A client that stops reading (slowloris-on-write) is dropped instead of pinning a thread. |
| `keepalive_max_requests` | `1000` | Requests served on a single keep-alive connection before the server sends `Connection: close`. |
| `idle_timeout_ms` | `10000` | How long to wait for the next request on an idle keep-alive connection before closing it. |

```turbo
fn main() {
    // A JSON API: small bodies, snappy timeouts, generous concurrency.
    http_config("max_body_bytes", 1048576)   // 1 MiB
    http_config("read_timeout_ms", 5000)
    http_config("write_timeout_ms", 5000)
    http_config("idle_timeout_ms", 15000)
    http_config("max_connections", 512)

    let app = http_server(8080)
    route(app, "GET", "/health", |req: str| -> str { respond_text(200, "ok") })
    http_listen(app)
}
```

Note the two layers of body limits: keep nginx's `client_max_body_size` at or
below `max_body_bytes` so oversized uploads are rejected at the edge, with the
Turbo cap as defense in depth.

## Graceful shutdown

On `SIGTERM` or `SIGINT` the server shuts down gracefully:

1. The accept loop stops accepting new connections.
2. In-flight requests are allowed to finish; idle keep-alive connections close
   on their next idle-timeout wakeup or the request cap.
3. After a bounded drain window (10 seconds), the process exits `0`.

This makes the server safe to run under a process supervisor (systemd, Docker,
Kubernetes) that sends `SIGTERM` on stop or rolling deploy — in-flight requests
complete rather than being cut off, and the clean exit code signals a healthy
stop. Signals are handled on the accept thread; worker threads block them, so a
signal never interrupts a response mid-write.

### systemd unit sketch

```ini
[Service]
ExecStart=/usr/local/bin/my-turbo-app
Restart=on-failure
# systemd sends SIGTERM on stop; the server drains and exits 0.
TimeoutStopSec=15
```

## Robustness details

- **Partial writes** on the response path loop until the whole response is
  sent. A dead peer (`EPIPE`/`ECONNRESET`) or a write timeout tears the
  connection down and frees its resources instead of looping on a broken
  socket. `SIGPIPE` is ignored process-wide so a disconnected client cannot
  kill the server.
- **Keep-alive** connections are bounded by both `keepalive_max_requests` and
  `idle_timeout_ms`, so a client cannot hold a connection (and its thread) open
  indefinitely.
- **Header and body caps** are enforced before the request is dispatched to a
  handler, so oversized requests never reach user code.

## Passing request data to spawned threads

If a handler `spawn`s a thread and passes it request data (for example, a
request body or header value that arrives as a `str`), the runtime **deep-copies
that string** before it crosses the thread boundary. Request strings live in the
per-request arena, which is reclaimed when the handler returns; the copy ensures
the spawned thread reads valid memory even after the request completes. You do
not need to copy the string yourself.

This copy currently applies to `str` arguments. If you need to hand a whole
array or struct of request data to a long-lived thread, copy it into
process-lifetime storage (e.g. a hashmap created at startup) rather than relying
on the request-scoped allocation surviving.

## What this server is not

- Not a TLS endpoint — terminate TLS at the proxy.
- Not an HTTP/2 or HTTP/3 server — the proxy speaks those to clients and HTTP/1.1
  to Turbo.
- Not a static-file or CDN server.
- Not a substitute for a WAF, rate limiter, or authentication gateway.

Keep those concerns in the proxy layer and let the Turbo server focus on your
application logic.
