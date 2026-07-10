# First-party Turbo packages

Seven first-party packages, each a self-contained Turbo library with a
`turbo.toml`, a `src/lib.tb` entry point, a `README.md`, and a passing `tests/`
suite runnable with `turbolang test`.

| Package | What it does |
|---------|--------------|
| [turbo-http-router](turbo-http-router/) | Path + method routing for the built-in HTTP server via a `HashMap<str, fn(str)->str>` dispatch table, with `:param` / wildcard matching helpers. |
| [turbo-lambda](turbo-lambda/) | AWS Lambda custom-runtime adapter: a pure-Turbo runtime-API loop dispatching events to a `fn(str) -> str` handler. |
| [turbo-sqlite](turbo-sqlite/) | Ergonomic, thin wrapper over the raw `sqlite_*` builtins: exec-with-params, scalar/column queries, counts, migrations. |
| [turbo-dotenv](turbo-dotenv/) | Load a `.env` file into a `HashMap<str, str>`. |
| [turbo-cli-args](turbo-cli-args/) | Flag and positional argument parsing over `args()`. |
| [turbo-logger](turbo-logger/) | Leveled logging (debug/info/warn/error) with filtering and optional timestamps. |
| [turbo-test-assertions](turbo-test-assertions/) | Expressive assertion helpers for `@test` functions with self-describing failures. |

## Using a package

Each package is listed in the [registry index](../registry/index.json) and
appears at [turbolang.dev/packages](https://turbolang.dev/packages). Declare a
dependency in your `turbo.toml` and run `turbolang install`:

```toml
[dependencies]
turbo-http-router = { path = "../TurboLang/packages/turbo-http-router" }
```

Then import by name — the resolver looks in `turbo_modules/<name>/src/lib.tb`:

```turbo
import { router_new, router_get, router_mount } from "turbo-http-router"
```

See [docs/packages.md](../docs/packages.md) for the full dependency-source and
registry-resolution reference.

## Running the tests

```bash
turbolang test packages/turbo-http-router/tests
# ...or point it at any package's tests/ directory
```
