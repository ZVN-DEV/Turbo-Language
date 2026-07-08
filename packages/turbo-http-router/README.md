# turbo-http-router

Path + method routing for Turbo's built-in HTTP server, built on a
`HashMap<str, fn(str) -> str>` dispatch table.

```toml
[dependencies]
turbo-http-router = "0.1"
```

## Example

```turbo
import { router_new, router_get, router_mount } from "turbo-http-router"

fn home(req: str) -> str   { respond_text(200, "hello") }
fn health(req: str) -> str { respond_text(200, "ok") }

fn main() {
    let r = router_new()
    router_get(r, "/", home)
    router_get(r, "/health", health)

    let app = http_server(8080)
    let mounted = router_mount(app, r)   // registers the static routes
    print("mounted {mounted} routes")
    http_listen(app)
}
```

```
$ curl localhost:8080/         -> hello
$ curl localhost:8080/health   -> ok
```

## API

| Function | Purpose |
|----------|---------|
| `router_new()` | Create an empty router (a typed dispatch table). |
| `router_get / router_post / router_put / router_delete / router_patch(r, path, handler)` | Register a handler for that method. |
| `router_add(r, method, path, handler)` | Register with an arbitrary method. |
| `router_mount(app, r) -> i64` | Register every **static** route on a real server; returns the count. |
| `router_dispatch(r, req) -> str` | Match a live request through the table (exact then `:param`), returns the handler's response or a 404. |
| `router_handler_for(r, method, path) -> fn(str)->str` | Resolve a (method, path) to its handler (or the 404 handler). |
| `route_matches(pattern, path) -> bool` | Does `/todos/:id` match `/todos/42`? |
| `path_param(pattern, path, name) -> str` | Extract a named `:param` from a concrete path. |

Handlers are plain `fn(str) -> str` — exactly what the built-in `route` requires.
Build the response with `respond_text` / `respond_html` / `respond_json`.

## Path params

`route_matches` and `path_param` handle `:name` params and single-segment `*`
wildcards, in pure Turbo string ops:

```turbo
route_matches("/users/:id/posts/:pid", "/users/7/posts/3")   // true
path_param("/users/:id", "/users/7", "id")                    // "7"
```

## Honest limitations

Turbo's built-in server (`http_server` + `route`) matches request paths by an
**exact** string compare and has no wildcard/catch-all hook — an unmatched path
gets a hardcoded 404 inside the runtime. So:

- **`router_mount` registers only static routes.** A param route like
  `/todos/:id` cannot be auto-dispatched, because the server would never deliver
  `/todos/42` to a handler registered under the literal path `/todos/:id`. Mount
  skips param/wildcard routes rather than registering them where they'd never
  fire.
- **To serve a param route,** register one static handler and pull the id out of
  `request_path(req)` with `path_param`, or drive `router_dispatch` yourself.
  The matcher is fully usable (and tested) for that; the server just can't
  expand an unbounded `:id` segment on its own.

Static routing is fully server-native and ergonomic; param routing is a tested,
pure-Turbo matching layer you drive explicitly.
