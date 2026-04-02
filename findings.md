# Turbo Language -- Full DX Audit

**Date**: 2026-04-01
**Auditor**: Claude (automated)
**Compiler Version**: turbo 0.1.0
**Build**: Release, Cranelift backend (LLVM unavailable without LLVM 18)

## Executive Summary

Turbo is a surprisingly capable compiler with solid core functionality -- 116 phase 1 integration tests, 11 regression tests, 7 adversarial tests, and 224 unit tests all pass. The language features (async/await, structs, enums, generics, traits, closures, pattern matching, agents) work as designed. However, the developer experience is undermined by **broken installation paths** (Homebrew, install script, and from-source instructions all fail out of the box), **inconsistent documentation** (README and website show different benchmark numbers, code examples that don't compile), and **phantom features** (`turbo check` is documented but doesn't exist).

## Installation (Homebrew)

- **Status**: BROKEN
- `brew tap ZVN-DEV/turbo` succeeds (taps 1 formula)
- `brew install turbo-lang` fails: formula is HEAD-only, and the HEAD URL points to `https://github.com/ZVN-DEV/turbo.git` which does not exist (404)
- The actual repo is at `https://github.com/ZVN-DEV/Turbo-Language.git`
- Even if the URL were correct, the formula runs `cargo build --release` which builds the entire workspace including `turbo-codegen-llvm`, which requires LLVM 18 -- a dependency not declared in the formula
- Formula also tries to install `turbo-lsp` binary separately, but `turbo lsp` is a subcommand of the main `turbo` binary (the separate `turbo-lsp` crate binary may or may not exist)

### Install Script

- **Status**: BROKEN
- README references `curl -fsSL https://raw.githubusercontent.com/ZVN-DEV/Turbo-Language/master/install.sh | sh`
- The file does not exist at the repo root; it's at `distribution/install.sh`
- The script itself references `ZVN-DEV/turbo` (wrong repo name) for the GitHub API call on line 32
- No GitHub releases exist, so the script would fail even with the correct paths

### From Source (README)

- **Status**: BROKEN
- Instructions say `cd Turbo-Language/turbo && cargo build --release`
- This builds the entire workspace including `turbo-codegen-llvm`, which fails without LLVM 18 installed
- Should use `cargo build --release -p turbo-cli` instead

### From Source (Website)

- **Status**: BROKEN
- Website says `cd turbo-lang` after `git clone https://github.com/ZVN-DEV/Turbo-Language.git`
- Git clone creates directory `Turbo-Language`, not `turbo-lang`
- Also uses `cargo build --release --manifest-path turbo/Cargo.toml` which has the same LLVM workspace build failure

### Docker

- **Status**: BROKEN (same `cargo build --release` workspace issue in Dockerfile)

## CLI Commands Audit

| Command | Documented (README)? | Documented (Website)? | Actually Exists? | Works? | Notes |
|---------|---------------------|----------------------|-----------------|--------|-------|
| `run` | Yes | Yes | Yes | Yes | Works perfectly. JIT via Cranelift. |
| `build` | Yes | Yes | Yes | Yes | AOT compilation works. Produces ~55 KB binaries. |
| `build --llvm` | Yes | Yes | Yes (flag exists) | No (without LLVM) | Correct error message: "LLVM backend not available" |
| `check` | Yes | No | **NO** | N/A | **Documented in README but does not exist.** `turbo check` returns "unrecognized subcommand". |
| `test` | Yes | Yes | Yes | Yes | Works. Runs `@test` functions. |
| `fmt` | Yes | Yes | Yes | Yes | Works. `--check` flag works. |
| `init` | Yes | Yes | Yes | Yes | Scaffolds project with `src/main.tb`, `tests/main_test.tb`, `turbo.toml`, `.gitignore`. |
| `repl` | Yes | Yes | Yes | Yes | Launches interactive REPL. |
| `playground` | Yes | No | Yes | Yes | Launches Next.js dev server on port 3000. |
| `lsp` | Yes | Yes | Yes | Yes | Starts LSP server (stdio transport). |
| `bench` | Yes | No | Yes | Yes | Runs JIT and AOT, compares outputs, reports timing. |
| `doc` | Yes | No | Yes | Yes | Generates basic markdown documentation. |
| `explain` | Yes | Yes | Yes | Partial | Works but output is one-liner only. Website claims multi-line descriptions. |
| `install` | No (README) | Yes (website) | Yes | Yes | Reads `turbo.toml` dependencies. |
| `update` | No (README) | Yes (website) | Yes | Yes | Updates GitHub dependencies from `turbo.toml`. |

### CLI Issues Found

1. **`turbo check` does not exist** -- documented in README CLI table but not implemented
2. **`turbo explain` output mismatch** -- website shows `E0100: Type mismatch\n  The compiler expected one type but found another.` but actual output is just `E0100: type mismatch` (one line, no description)
3. **`turbo build --output`** flag is `--output` per website CLI docs but actually `-o` / `--output` per help text (minor: both work)
4. **`turbo test`** with `assert_test.tb` returns `0 passed, 0 failed` because the file uses `assert()` not `@test` functions -- the test command only finds `@test`-annotated functions, which is correct behavior but confusing for the test file name
5. **`turbo playground`** and **`turbo bench`** are not documented on the website CLI page

## Feature Claims vs Reality

| Feature | Claimed Where | Actually Works? | Notes |
|---------|--------------|-----------------|-------|
| JIT compilation (Cranelift) | README, Website | Yes | Works perfectly |
| AOT compilation (Cranelift) | README, Website | Yes | Produces working native binaries |
| AOT compilation (LLVM) | README, Website | Untestable | Requires LLVM 18; not available in default build |
| Type inference | README, Website | Yes | Works for let bindings |
| Generics | README, Website | Yes | `struct Point<T>`, `fn identity<T>` work |
| Traits | README, Website | Yes | Trait definitions and implementations work |
| Pattern matching | README, Website | Yes | Match with guards, enum destructuring work |
| Algebraic data types | README, Website | Yes | `type Shape { Circle(f64) }` works |
| Async/await | README, Website | Yes | `async fn`, `spawn`, `await` work |
| Channels | Website | Yes | `channel()`, `send()`, `recv()` work |
| Mutex | Website | Yes | `mutex()`, `mutex_get()`, `mutex_set()` work |
| AI Agent primitives | README, Website | Yes | `agent` and `tool fn` keywords work |
| Closures | README, Website | Yes | Including captures and higher-order functions |
| Pipe operator | README, Website | Yes | `|>` works with builtins |
| String interpolation | README, Website | Yes | `"Hello, {name}!"` works |
| Copy-on-write arrays | README | Yes | Works (but README example is wrong) |
| Derive attributes | README, Website | Yes | `@derive(Eq, Clone, Display)` work |
| HTTP server | README | Yes | `http_server()`, `route()`, `http_listen()` work |
| Result type | README, Website | Yes | `Result<T, E>` with `try` operator works |
| Optional type | README, Website | Yes | `T?` syntax works |
| Defer | Website | Yes | `defer` statement works |
| Unsafe | Tests | Yes | `unsafe` blocks work |
| Import system | Tests | Partial | Basic imports work; circular import detection works but test shows edge case |
| Package manager | Website | Partial | `turbo install` and `turbo update` exist but no package registry |
| `turbo check` | README, Design docs | **NO** | Command does not exist |

## Website / Landing Page Issues

### Broken Code Examples

1. **Pattern matching example on landing page** -- Uses trailing commas after match arms (`Circle(r) => 3.14159 * r * r,`) which causes parse error. Turbo does not support commas as match arm separators.

2. **Type system example in README** -- Defines `trait Display` which conflicts with the built-in `Display` trait. Compile error: `trait 'Display' is already defined`.

3. **Pattern matching example in README** -- Uses `Shape::Circle(3.0)` (Rust-style double colon). Turbo uses dot syntax: `Shape.Circle(3.0)`. The `::` causes a parse error.

4. **Copy-on-write example in README** -- Uses `let b = a` then `b[0] = 99` without `let mut`. Compile error: `cannot assign to index of immutable variable 'b'`. Should be `let mut b = a`.

5. **Hello-world docs "bigger example"** -- Defines `fn abs(x: i64)` which conflicts with built-in `abs` function. Compile error: `cannot redefine builtin function 'abs'`.

6. **AI Agent example on landing page** -- Puts `tool fn` definitions inside the `agent` block (lines 65-78 of page.tsx). The actual working syntax has `tool fn` at module level, outside the agent.

### Benchmark Inconsistencies

The README and website show **completely different benchmark numbers** for the same test (fib(40) on Apple Silicon):

| Language | README Claims | Website Claims | Measured (This Audit) |
|----------|-------------|---------------|----------------------|
| Turbo (LLVM) | 160ms, 35 KB | 290ms, 55 KB | N/A (LLVM unavailable) |
| C (cc -O2) | 170ms, 33 KB | 290ms, 33 KB | N/A |
| Rust (rustc -O) | 180ms, 441 KB | 180ms, 441 KB | N/A |
| Turbo (Cranelift) | 220ms, 35 KB | 250ms, 55 KB | ~250ms, 55 KB |
| Node.js | 580ms | 580ms | N/A |
| Python | 13.1s | 13.1s | N/A |

The README claims 35 KB binaries; actual binaries are 55 KB. The README puts Turbo (LLVM) as the fastest at 160ms beating C; the website puts it at 290ms tied with C.

### Wrong Installation Instructions

- Website "From Source" says `cd turbo-lang` but the git clone creates `Turbo-Language`
- Homebrew instructions fail (formula points to nonexistent repo)
- From-source `cargo build --release` fails without LLVM 18

### Missing/Incorrect Claims

- README says "237 tests across unit and integration suites" -- actual count is 366+ (224 unit + 142 integration)
- Website's `turbo explain` documentation shows multi-line output with descriptions, but actual output is a single line
- Website CLI page does not document `turbo bench`, `turbo playground`, `turbo install`, or `turbo update`
- README's install script URL points to repo root but file is at `distribution/install.sh`
- Docs page claim "~55 KB for a hello world" -- this matches reality (55,600 bytes measured)

### Link Issues

- Homebrew formula URL: `https://github.com/ZVN-DEV/turbo.git` -- 404 (should be `Turbo-Language`)
- Install script GitHub API URL: `ZVN-DEV/turbo` -- should be `ZVN-DEV/Turbo-Language`
- README install script curl URL: `install.sh` at root -- file is at `distribution/install.sh`
- Website links to `https://github.com/ZVN-DEV/Turbo-Language` -- correct
- Footer links to `turbo-vscode` and `tree-sitter-turbo` repos -- not verified if these exist

## Code Examples Audit

| Example | Source | Compiles? | Runs Correctly? | Issue |
|---------|--------|-----------|-----------------|-------|
| Hello World (README) | README | Yes | Yes | -- |
| "Taste of Turbo" (README) | README | Yes | Yes | -- |
| Type System (README) | README | **No** | -- | `trait Display` conflicts with built-in |
| Pattern Matching (README) | README | **No** | -- | `Shape::Circle` syntax wrong (should be `Shape.Circle`) |
| Async/Await (README) | README | Yes | Yes | -- |
| AI Agent (README) | README | Yes | Yes | -- |
| Closures (README) | README | Yes | Yes | -- |
| Pipes & Collections (README) | README | Yes | Yes | -- |
| HTTP Server (README) | README | Yes | Yes | Server starts and responds to requests |
| Derive/Testing (README) | README | Yes | Yes | -- |
| Copy-on-Write (README) | README | **No** | -- | Missing `let mut` on `b` |
| Hero fib (Website) | Landing page | Yes | Yes | -- |
| Pattern Matching (Website) | Landing page | **No** | -- | Trailing commas in match arms not supported |
| Async (Website) | Landing page | Likely | -- | Uses `http_get` which may not exist as builtin |
| Agent (Website) | Landing page | **No** | -- | `tool fn` inside agent block is wrong syntax |
| Hello bigger example (Docs) | Docs/hello-world | **No** | -- | `fn abs` conflicts with builtin |
| simple-script example | `examples/` | Yes | Yes | Runs perfectly |
| speed-server example | `examples/` | Yes | Yes | Server starts, responds to HTTP requests |

**5 out of 17 tested code examples fail to compile.**

## Ecosystem

### VS Code Extension

- Referenced as `zvndev.turbo-lang` in README and installation docs
- Footer links to `https://github.com/ZVN-DEV/turbo-vscode`
- Local `editors/vscode/turbo-lang/` directory exists
- Install instructions given: `code --install-extension zvndev.turbo-lang`
- Not verified if the extension is actually published to VS Code Marketplace

### Tree-sitter Grammar

- Referenced in README and footer as `https://github.com/ZVN-DEV/tree-sitter-turbo`
- Not verified if the repo exists

### LSP Server

- `turbo lsp` starts and runs (stdio-based LSP server)
- Website claims: diagnostics, hover, go-to-definition, code completions, document symbols
- Not tested with an actual editor connection

### Homebrew Formula

- **BROKEN**: Points to wrong GitHub repo URL
- Even if fixed, `cargo build --release` would fail without LLVM 18
- Formula doesn't declare LLVM as a dependency

### Docker

- Dockerfile exists at `distribution/Dockerfile`
- **BROKEN**: `cargo build --release` will fail (LLVM workspace member)
- Uses `rust:1.83-slim` base image (version may be outdated)

### Install Script

- Exists at `distribution/install.sh`
- README references it at repo root (wrong path)
- Script references wrong repo name (`ZVN-DEV/turbo` instead of `ZVN-DEV/Turbo-Language`)
- No GitHub releases exist, so it would fail anyway

## Integration Tests

### Phase 1 Tests
- **116 passed, 0 failed, 0 skipped**

### Regression Tests
- **11 passed, 0 failed**

### Adversarial Tests
- **7 passed, 0 failed**

### Import Tests
- **3 passed, 1 failed (circular_a -- test runner issue), 4 skipped (library files)**

### Unit Tests
- Formatter: 9 passed
- Codegen (Cranelift): 106 passed
- Lexer: 13 passed
- Parser: 41 passed
- Sema: 55 passed
- **Total unit tests: 224 passed, 0 failed**

### Test Runner Issue
- `tests/run_tests.sh` runs `cargo build --release` which fails with LLVM workspace member
- Must build with `cargo build --release -p turbo-cli` first, then run tests manually

## DX Pain Points (Ranked)

1. **No working installation path** -- Homebrew, install script, and from-source instructions all fail. A new user cannot install Turbo without debugging the build system. This is the #1 blocker.

2. **Broken code examples in docs** -- 5 of 17 examples don't compile. First impressions are terrible when the "Quick Start" code fails.

3. **Phantom `turbo check` command** -- Documented in README but doesn't exist. Users will try it and get confused.

4. **Contradictory benchmark numbers** -- README and website disagree on performance claims. Undermines credibility.

5. **LLVM backend breaks default build** -- The `turbo-codegen-llvm` workspace member causes `cargo build --release` to fail. It should be excluded from the default workspace or the build instructions should use `-p turbo-cli`.

6. **Wrong enum constructor syntax in docs** -- README uses `Shape::Circle()` (Rust-style) but actual syntax is `Shape.Circle()`. This will confuse every developer coming from Rust.

7. **Built-in function shadowing surprises** -- Code examples use `abs` and `Display` which are built-ins. No way to know what names are reserved without reading the source.

8. **Test count claim is wrong** -- README says 237, actual is 366+.

9. **`turbo explain` lacks descriptions** -- Promised multi-line explanations in docs, delivers one-line summaries.

10. **Undocumented commands** -- `bench`, `playground`, `install`, `update` exist but are not in the README CLI table.

## Recommendations (Prioritized)

### Critical (Must Fix)

1. **Fix Homebrew formula** -- Change URL from `ZVN-DEV/turbo.git` to `ZVN-DEV/Turbo-Language.git`. Change build command to `cargo build --release -p turbo-cli`.

2. **Fix from-source build instructions** -- In README, website, and Dockerfile: use `cargo build --release -p turbo-cli` instead of `cargo build --release` to avoid the LLVM dependency.

3. **Fix website `cd` path** -- Change `cd turbo-lang` to `cd Turbo-Language` in the website's from-source instructions.

4. **Fix all broken code examples** -- (a) Rename `Display` trait example to `MyDisplay`; (b) Change `Shape::Circle` to `Shape.Circle`; (c) Add `let mut` to CoW example; (d) Rename `abs` function in hello-world docs; (e) Remove trailing commas from website match arms; (f) Move `tool fn` outside agent block in website example.

5. **Either implement `turbo check` or remove it from documentation**.

### High Priority

6. **Unify benchmark numbers** -- Pick one set of verified numbers and use them everywhere. Remove the other.

7. **Fix install script** -- Move to repo root (or update README URL). Fix `ZVN-DEV/turbo` reference to `ZVN-DEV/Turbo-Language`.

8. **Update test count** -- README claims 237 but actual is 366+.

9. **Add `turbo bench`, `turbo playground`, `turbo install`, `turbo update` to README CLI table**.

10. **Enrich `turbo explain` output** -- Add the multi-line descriptions that the docs promise, or update the docs to match the actual one-line output.

### Nice to Have

11. **Document built-in function names** -- So users know not to shadow `abs`, `Display`, `len`, etc.

12. **Create GitHub releases** -- So the install script has something to download.

13. **Add `--head` note to Homebrew install instructions** -- Until proper releases exist: `brew install --HEAD turbo-lang`.

14. **Exclude `turbo-codegen-llvm` from default workspace members** -- Or gate it behind a workspace-level feature flag so plain `cargo build` works.
