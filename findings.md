# Turbo Language -- Full Audit Findings (v0.3.0)
Date: 2026-04-04

## Executive Summary

Turbo v0.3.0 is a remarkably capable single-developer language project. The core compiler pipeline (lexer, parser, sema, codegen) is solid: 259 unit tests and 146 integration tests all pass, both Cranelift JIT and AOT produce correct binaries, LLVM backend works, and fib(40) performance (~0.27s) is competitive with C -O2 (~0.29s). Since the v0.2.2 audit, significant features have landed: arrow closures, if-let, optional chaining, struct destructuring, map literals, typed const declarations, and push(). However, several issues remain: `push()` silently no-ops without reassignment, `.push()` method syntax silently fails, `impl` blocks for generic types don't parse, `f32` is unusable with float literals, and the README/SYNTAX.md still contain some inaccuracies. All 14 CLI commands are functional. The project is in better shape than most languages at this stage.

---

## Version Audit

| Artifact | Version | Consistent? |
|----------|---------|-------------|
| `turbolang --version` | 0.3.0 | Baseline |
| Homebrew formula | 0.3.0 | YES |
| CHANGELOG.md latest | 0.3.0 | YES |
| All Cargo.toml crates | 0.3.0 | YES |
| VS Code extension | 0.2.0 | **NO -- 2 minor versions behind** |

All 8 crates (turbo-cli, turbo-ast, turbo-lexer, turbo-parser, turbo-sema, turbo-codegen-cranelift, turbo-codegen-llvm, turbo-lsp) are at 0.3.0. This is much improved from the last audit where internal crates were at 0.1.0.

---

## CLI Commands Audit

| Command | Status | Notes |
|---------|--------|-------|
| `turbolang run <file>` | WORKS | Cranelift JIT, correct output, ~0.26s for fib(40) |
| `turbolang build <file>` | WORKS | Cranelift AOT, produces native Mach-O arm64 binary, 56KB |
| `turbolang build --llvm <file>` | WORKS | LLVM 18 backend, produces 55KB binary, ~0.28s fib(40) |
| `turbolang check <file>` | WORKS | Type-checks without compiling, beautiful ariadne diagnostics |
| `turbolang fmt <file>` | WORKS | Basic formatting; reports "already formatted" for clean files |
| `turbolang init <name>` | WORKS | Creates project with src/main.tb, tests/, turbo.toml, .gitignore |
| `turbolang test <file>` | WORKS | Discovers @test fns, pretty PASS/FAIL output with summary |
| `turbolang bench <file>` | WORKS | Runs JIT+AOT, compares outputs, reports median timing |
| `turbolang doc <file>` | WORKS | Generates markdown docs with functions, structs, fields, methods |
| `turbolang repl` | WORKS | Interactive with auto-print, :help, :quit commands |
| `turbolang playground` | WORKS | Built-in HTTP server at localhost:3000 with code editor |
| `turbolang lsp` | WORKS | Starts LSP server (needs proper client like VS Code) |
| `turbolang explain <code>` | WORKS | `explain E0100` -> "type mismatch", covers E0001-E0513 |
| `turbolang install` | WORKS | Reads turbo.toml dependencies (reports "none found" if empty) |
| `turbolang update` | WORKS | Updates GitHub dependencies (reports "none found" if empty) |

All 15 advertised CLI commands work. This is a clean sweep.

---

## Language Features Audit

### Core Language -- Tested and Working

| Feature | Status | Test Result |
|---------|--------|-------------|
| Variables (let, let mut) | WORKS | `let x = 42`, `let mut counter = 0` |
| Type annotations (i32, i64, u32, u64, f64, bool, str) | WORKS | All types tested |
| Functions with return types | WORKS | `fn add(a: i64, b: i64) -> i64` |
| Recursion | WORKS | fib(40) = 102334155 |
| If/else as expressions | WORKS | `let status = if age > 18 { "adult" } else { "minor" }` |
| Else-if chains | WORKS | `if ... else if ... else ...` |
| While loops | WORKS | Standard while with condition |
| For-in loops over arrays | WORKS | `for item in items { ... }` |
| For-in over ranges | WORKS | `for x in 0..3 { ... }` |
| Break and continue | WORKS | Both work inside while loops |
| String interpolation | WORKS | `"Hello, {name}!"` including nested function calls |
| Nested quote interpolation | WORKS | `"result: {get_name("world")}"` works (fixed in v0.3.0) |
| Print with interpolation | WORKS | Integers, floats, bools, strings all print correctly |
| Constants | WORKS | `const MAX: i64 = 100` with type annotations (new in v0.3.0) |
| Struct types | WORKS | Field access, mutation, passing to functions |
| Impl blocks with methods | WORKS | `self` parameter, method calls via dot syntax |
| Enum types with data variants | WORKS | `type Shape { Circle(f64), Rect(f64, f64) }` |
| Enum constructors | WORKS | Requires qualified syntax: `Shape.Circle(5.0)` |
| Match expressions | WORKS | Enum patterns, integer patterns, wildcard `_` |
| Match guards | WORKS | `n if n > 0 => "positive"` |
| Optional types (T?) | WORKS | `some(v)`, `none`, match patterns |
| Result types (T ! E) | WORKS | `ok(v)`, `err(e)`, match patterns |
| ? operator | WORKS | Error propagation in Result-returning functions |
| If-let expressions | WORKS | `if let some(v) = maybe_value() { ... }` (new in v0.3.0) |
| Closures (pipe syntax) | WORKS | `\|x: i64\| -> i64 { x * 2 }` with capture |
| Arrow closures | WORKS | `(x: i64) => x * 2` (new in v0.3.0) |
| Higher-order functions | WORKS | Functions as parameters and return values |
| .map() and .filter() on arrays | WORKS | Method chaining works |
| reduce() | WORKS | `reduce(arr, init, closure)` |
| Pipe operator (\|>) | WORKS | `text \|> trim \|> lower` |
| Generic functions | WORKS | `fn identity<T>(x: T) -> T` |
| Generic structs | WORKS | `struct Pair<A, B> { first: A, second: B }` |
| Traits | WORKS | `trait Describable { fn describe(self) -> str }` |
| Trait impl | WORKS | `impl Describable for Dog { ... }` |
| @derive(Eq, Clone, Display) | WORKS | Struct equality, display formatting |
| @test functions | WORKS | `@test fn test_add() { assert_eq(...) }` |
| @unsafe functions | WORKS | `@unsafe fn` declared, sema enforces safe-call restriction |
| Defer statements | WORKS | `defer { print("cleanup") }` runs at scope exit |
| Copy-on-write arrays | WORKS | `let mut b = a; b[0] = 99` -- a unchanged |
| Imports | WORKS | `import { square } from "./math_lib"` resolves .tb files |
| Struct destructuring | WORKS | `let { x, y } = point` (new in v0.3.0) |
| Optional chaining | WORKS | `u?.name` returns optional (new in v0.3.0) |
| Map literals | WORKS | `{"key": "value"}` creates hashmap (new in v0.3.0) |
| Async/await | WORKS | `async fn`, `spawn`, `await` with thread-based concurrency |
| Channels | WORKS | Per channels.tb test |
| Mutex | WORKS | Per mutex_basic.tb test |
| Hashmap operations | WORKS | `hashmap()`, `hashmap_set()`, `hashmap_get()` |
| String builtins | WORKS | upper, lower, trim, replace, split, starts_with, ends_with, char_at |
| Math builtins | WORKS | Per stdlib_math.tb, stdlib_math_ext.tb tests |
| JSON serialization | WORKS | Per json_*.tb tests |
| HTTP server | WORKS | `http_server()`, `route()`, `http_listen()` -- verified with curl |
| Agent/tool declarations | WORKS | `agent`, `tool fn` syntax, field access on agent instances |

### Issues Found

| Issue | Severity | Details |
|-------|----------|---------|
| `push()` requires reassignment | P1-BUG | `push(arr, 4)` returns new array but does NOT mutate `arr`. Must use `arr = push(arr, 4)`. Undocumented. |
| `.push()` method syntax silently no-ops | P0-BUG | `items.push(4)` compiles and runs without error but does nothing -- length unchanged. Silent data loss. |
| Generic impl blocks don't parse | P1-MISSING | `impl Pair<T> { ... }` fails with "expected `{`, found `<`". Only non-generic impl works. |
| f32 type unusable | P2-BUG | `let x: f32 = 1.5` fails: "type annotation f32 doesn't match value type f64". All float literals infer as f64, no f32 literal syntax. |
| `hashmap_size()` not a builtin | P2-MISSING | No way to get hashmap length. `hashmap_size` is undefined. |
| `unsafe {}` block syntax doesn't parse | P2-GAP | `unsafe { dangerous() }` fails. Sema correctly rejects unsafe calls from safe context, but there's no `unsafe` block syntax to opt in. Existing test just checks the error. |
| README type system example fails | P1-DOC | README shows `type Option<T> { some(T), none }` which fails because `some` is a reserved keyword. |
| README async example inconsistency | P2-DOC | README async example shows `fn main()` (non-async) calling `spawn` and `await`. Works, but the "Taste of Turbo" example correctly uses `async fn main()`. |
| README shows unqualified enum constructors | P1-DOC | README pattern matching shows `Circle(r)` -- should be `Shape.Circle(r)` for construction (pattern matching does use unqualified). |
| Agent syntax: trailing commas rejected | P2-DOC | Agent fields with trailing commas (`model: "claude-sonnet",`) fail to parse. Must omit trailing commas. |
| Test count badge stale | P3-DOC | README badge says "378 passing" but actual count is 405 (259 unit + 146 integration). Should be updated upward. |

---

## Build Path Audit

### JIT (turbolang run)
- **Status: WORKS**
- Cranelift backend compiles and executes in-process
- fib(40) = 102334155 in ~0.26s (includes compilation)
- All test programs produce correct output

### AOT Cranelift (turbolang build)
- **Status: WORKS**
- Produces native Mach-O arm64 binary (56KB for fib program)
- Links against turbo_rt.c runtime
- fib(40) binary runs correctly in ~0.27s

### AOT LLVM (turbolang build --llvm)
- **Status: WORKS**
- Produces native binary (55KB)
- fib(40) runs correctly in ~0.28s
- Ships with Homebrew install (no separate LLVM setup needed)

### Performance vs Claims

| Benchmark | README Claim | Measured | Verdict |
|-----------|-------------|----------|---------|
| Turbo Cranelift fib(40) | 250ms | ~270ms | APPROXIMATELY TRUE |
| Turbo LLVM fib(40) | 290ms | ~280ms | TRUE |
| C (cc -O2) fib(40) | 290ms | ~290ms | TRUE |
| Binary size | 55 KB | 55-56 KB | TRUE |

Performance claims are honest. Turbo is genuinely competitive with C on this benchmark.

---

## Test Suite Status

### Unit Tests (cargo test)
- **259 tests, 0 failures**
- Breakdown: lexer 0, ast 23, parser 106, sema 58, codegen 18, cli 13, lsp 41
- All pass cleanly in ~0.02s total

### Integration Tests (run_tests.sh)
- **146 passed, 0 failed, 4 skipped**
- Phase1: 124 test files
- Regression: 11 tests
- Import tests: 7 tests (4 skipped -- helper files without .expected)
- Adversarial: 7 tests
- All pass cleanly

### Test Coverage Assessment
The test suite is comprehensive. It covers:
- All numeric types (i32, i64, u32, u64, f64)
- All control flow (if/else, while, for, break, continue, match)
- All data types (struct, enum, array, hashmap, optional, result)
- All features (closures, generics, traits, async, agents, defer, unsafe)
- Error cases (type mismatches, undefined variables, exhaustiveness)
- Edge cases (NaN comparison, integer overflow, deep nesting, shadowing)

---

## Ecosystem Tools

### LSP Server (turbo-lsp)
- **Status: WORKS** (binary installs, starts on stdio, needs VS Code client)
- Installed at `/opt/homebrew/bin/turbo-lsp`
- Claims: diagnostics, hover, completions, go-to-definition, document symbols
- 41 unit tests pass

### Formatter (turbolang fmt)
- **Status: WORKS** but incomplete
- Handles indentation and basic spacing
- Reports "already formatted" for clean files
- Does NOT normalize all operator spacing or brace formatting

### Playground (turbolang playground)
- **Status: WORKS**
- Built-in HTTP server at localhost:3000
- Includes code editor and benchmarks page
- Self-contained HTML app embedded in binary

### REPL (turbolang repl)
- **Status: WORKS**
- Auto-prints expression results
- Supports let bindings across lines
- :help and :quit commands work

### VS Code Extension
- **Status: INSTALLED** at v0.2.0 (behind compiler at v0.3.0)
- Provides syntax highlighting and 23 snippets

---

## README/Docs Accuracy

### Claims That Are TRUE
- Homebrew install works
- JIT and AOT compilation work
- LLVM backend works
- Performance benchmarks are honest
- All listed types work (i32, i64, u32, u64, f64, bool, str, T?, T ! E)
- Pattern matching with guards works
- Async/await with spawn works
- Closures with capture work
- .map(), .filter() on arrays work
- Pipe operator works
- String interpolation works (including nested quotes, fixed in v0.3.0)
- Copy-on-write arrays work
- Agents and tool fn syntax work (as struct-like declarations)
- HTTP server built-in works
- All 3 example projects run correctly
- Error codes E0001-E0513 all documented

### Claims That Are INACCURATE
1. **Type system example** in README uses `some(T)` as enum variant name -- `some` is reserved
2. **Pattern matching example** uses unqualified constructors `Circle(r)` for construction -- needs `Shape.Circle(r)`
3. **Test badge** says "378 passing" -- actual count is 405
4. **README shows `.push(4)` method syntax** -- this silently fails (does nothing)
5. **SYNTAX.md marks some features `[Implemented]` that have caveats** -- e.g. destructuring works for structs but not for arrays, optional chaining works but README doesn't show the required `if let` unwrap

### Links Check
- turbolang.dev: LIVE (HTTP 200)
- turbolang.dev/docs: LIVE (HTTP 200)
- All design/ docs referenced in README exist
- CONTRIBUTING.md exists
- LICENSE exists
- docs/errors.md exists
- distribution/Dockerfile exists
- distribution/install.sh exists

---

## DX Issues

1. **Silent `.push()` failure is dangerous** -- `items.push(4)` compiles, runs, and does nothing. No error, no warning. The array length doesn't change. This is the most dangerous bug found because it causes silent data loss. The correct usage is `items = push(items, 4)` but nothing tells the user this.

2. **Float literal inflexibility** -- All float literals infer as f64. The f32 type exists in the type system but there's no way to create an f32 value from a literal. `let x: f32 = 1.5` fails with a type mismatch.

3. **No `unsafe` block syntax** -- @unsafe functions can be declared and sema correctly rejects calls from safe contexts, but there's no `unsafe { ... }` block to actually call them. The feature is half-implemented.

4. **Error messages are excellent** -- Ariadne-formatted, with error codes, source highlighting, span pointing, and actionable help hints. This is production-quality DX and better than many mature languages.

5. **Agent system is structural only** -- `agent` and `tool fn` parse and create data structures, but there's no actual LLM integration. They're essentially fancy structs. This is fine for v0.3.0 but should be made clearer.

---

## Recommendations (Prioritized)

### P0 -- Fix Before Anyone Evaluates
1. ~~**Fix `.push()` method syntax**~~ -- **FIXED in v0.3.1**: Compiler auto-reassigns push() result in statement position
2. ~~**Fix README type system example**~~ -- **FIXED in v0.3.1**: Changed to `Result<T>` example
3. ~~**Fix README enum constructor examples**~~ -- **FIXED in v0.3.1**: Added qualified syntax

### P1 -- Fix Soon
4. ~~**Add generic impl blocks**~~ -- **FIXED in v0.3.1**: Parser handles `impl Pair<A, B> { ... }`
5. ~~**Add hashmap_size() builtin**~~ -- **FIXED in v0.3.1**: Added as alias for hashmap_len
6. ~~**Update test count badge**~~ -- **FIXED in v0.3.1**: Updated to current count
7. **Sync VS Code extension to 0.3.1** -- Extension is behind compiler
8. ~~**Document push() semantics**~~ -- **FIXED in v0.3.1**: push() now works as method syntax

### P2 -- Nice to Have
9. ~~**Add f32 literal coercion**~~ -- **FIXED in v0.3.1**: Sema coerces f64 literals to f32 when annotated
10. **Add `unsafe {}` block syntax** -- Complete the unsafe feature
11. **Improve formatter** -- Add operator spacing and brace normalization
12. **Add `hashmap_keys()` and `hashmap_values()` builtins** -- Common operations missing

### P3 -- Roadmap
13. List comprehensions, guard let, with blocks -- documented as planned in SYNTAX.md
14. Standard library modules (proper `import std/...`)
15. Package manager with lockfiles
16. Actual LLM integration for agents

---

## TurboServo Stress Test Findings (v0.3.1)

Building TurboServo (a full HTTP server framework) as a real-world stress test of the language surfaced several issues and drove fixes:

### Issues Found & Fixed

| Issue | Severity | Status | Fix |
|-------|----------|--------|-----|
| `.push()` method syntax silently no-ops | P0-BUG | **FIXED** | Compiler auto-reassigns push() result in statement position |
| Generic impl blocks don't parse | P1-MISSING | **FIXED** | Parser now handles `impl Pair<A, B> { ... }` |
| f32 literals fail type check | P2-BUG | **FIXED** | Sema now coerces f64 float literals to f32 when annotation is f32 |
| `hashmap_size()` missing | P2-MISSING | **FIXED** | Added as alias for `hashmap_len()` in sema + codegen |
| README type system example wrong | P1-DOC | **FIXED** | Changed `Option<T>` example to `Result<T>` |
| README unqualified enum constructors | P1-DOC | **FIXED** | Added `Shape.Circle(3.14)` qualified syntax |
| Test badge stale (378) | P3-DOC | **FIXED** | Updated to current count |
| Generic type args in return types fail | P1-MISSING | **FIXED** | Parser now handles `-> Pair<B, A>` return types |
| HTTP handlers only receive body | P1-MISSING | **FIXED** | New structured request format with method/path/query/headers/body |
| Single-threaded HTTP accept loop | P1-PERF | **FIXED** | Thread-per-connection + HTTP/1.1 keep-alive in both runtimes |

### New Builtins Added
- `request_method(req)` -- extract HTTP method from request
- `request_path(req)` -- extract URL path
- `request_query(req, key)` -- extract query parameter by key
- `request_header(req, key)` -- extract header value by key

### Dual Runtime Lesson
**CRITICAL**: Every runtime change must be made in BOTH `turbo_rt.c` (C for AOT) AND `runtime.rs` (Rust for JIT). The JIT uses the Rust runtime at development time; AOT links the C runtime for production. Missing one causes behavioral divergence. This was discovered when HTTP request context worked in AOT but not JIT.

### Benchmark Results (post-fix)

| Framework | GET /json (req/s) | GET /user?id=1 (req/s) | Avg Latency | Memory |
|---|---|---|---|---|
| **TurboServo (Turbo)** | **165,253** | **166,982** | **~595μs** | **5 MB** |
| Hono (Bun) | 114,961 | 118,173 | ~857μs | ~35 MB |
| Go net/http | 94,697 | 95,428 | ~1.03ms | ~15 MB |

TurboServo is 1.4x faster than Hono/Bun and 1.7x faster than Go net/http with 7x less memory than Bun. The multi-threaded HTTP + keep-alive fix was a 5.3x improvement from the initial single-threaded 31K req/s.

---

## What's Genuinely Impressive

- **259 unit + 146 integration tests, all passing** -- Excellent coverage for a young language
- **Both JIT and AOT work** -- Cranelift AND LLVM, both producing correct binaries
- **Performance is honest** -- fib(40) benchmarks match claims, competitive with C
- **15 working CLI commands** -- More tooling than languages 10x its age
- **6 working example projects** -- Real programs (Game of Life, HTTP servers, data pipelines)
- **Error diagnostics** -- Production-quality with error codes, source highlighting, help hints
- **Built-in playground and REPL** -- Shipped in the binary
- **HTTP server built-in** -- Actually works, verified with curl
- **Significant progress since v0.2.2** -- Arrow closures, if-let, optional chaining, destructuring, map literals all landed
- **The language design** -- T?, T ! E, match, closures, pipes, agents -- cohesive and elegant
