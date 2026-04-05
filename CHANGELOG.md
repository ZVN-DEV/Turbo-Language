# Changelog

All notable changes to the Turbo compiler are documented here.
Format based on [Keep a Changelog](https://keepachangelog.com/).

## [Unreleased]

## [0.3.2] - 2026-04-05

### Added
- Trailing comma support in arrays and function calls
- Optional values now print as `some(42)` / `none` instead of `[optional]`
- Unused variable warnings (E0515) for `let` bindings
- Enhanced `turbolang explain` with code examples for 12 error codes
- Doc comment extraction (`///`) attached to functions, structs, and enums
- REPL readline support with history (via rustyline)
- `--quiet` flag for `turbolang bench`
- Complete standard library reference (`docs/stdlib.md`) covering all 64 built-in functions
- Comprehensive release checklist (`docs/RELEASE.md`)

### Changed
- Formatter now adds spaces around `=` in assignments
- `turbolang init` template includes struct methods, enum pattern matching, and Result examples
- README Result type example uses lowercase `ok(T)`/`err(str)` matching actual syntax
- SYNTAX.md arrow functions marked `[Implemented]`
- Showcase docs page updated to show actual flat built-in functions (not fictional namespaced modules)

### Fixed
- Unused variable false positives on match bindings, callback params, and if-let patterns
- Test runner stderr handling for warning compatibility

### Tests
- 266 unit tests, 155 integration tests (417 total, all passing)

## [0.3.1] - 2026-04-04

### Added
- Generic impl blocks (`impl Pair<A, B> { ... }`)
- Generic type arguments in return types (`-> Pair<B, A>`)
- `hashmap_size()` builtin (alias for `hashmap_len`)
- HTTP request context builtins (`request_method`, `request_path`, `request_query`, `request_header`)

### Fixed
- `.push()` method syntax now auto-reassigns (no more silent no-op)
- `f32` float literal coercion from `f64` annotations
- README type system and enum constructor examples

### Changed
- Multi-threaded HTTP server with keep-alive (5x throughput improvement)

### Tests
- 151 integration tests passing (up from 148)

## [0.3.0] - 2026-04-03

### Added
- **Arrow closures**: TypeScript-style `(x: int) => x * 2` syntax alongside existing `|x| x * 2`
- **Typed const declarations**: `const MAX: int = 100` with type annotation validation
- **`push` builtin**: array push as a proper builtin function with type checking
- **`if let` expressions**: pattern matching on `Optional` and `Result` values inline
- **Optional chaining**: `user?.name` syntax for safe field access on optional structs
- **Struct destructuring**: `let { x, y } = point` for extracting struct fields
- **Map literals**: `{"key": value}` sugar for hashmap creation
- **String interpolation nesting**: fixed lexer to handle nested quotes inside `{...}` blocks
- **Formatter improvements**: operator spacing (`==`, `!=`, `<=`, `>=`, `&&`, `||`) and brace spacing (`else {`)
- 8 new integration test files covering all new features
- 259 unit tests, 151 integration tests all passing

### Fixed
- String interpolation lexer now correctly handles `"hello {get_name("world")}"` with nested quotes
- Expression-body closures with typed parameters now correctly return their value
- Optional-returning functions no longer lose inner type info when inlined
- Formatter no longer mangles generic type parameters when adding operator spacing

## [0.2.2] - 2026-04-03

### Added
- LLVM 18 AOT backend now ships with Homebrew install (`turbolang build --llvm`)
- REPL auto-prints expression results (no need to wrap in `print()`)
- `turbolang doc` properly extracts fields from single-line struct definitions

### Fixed
- Suppress `<error>` type leak in optional/result diagnostics (recursive `contains_error()`)
- Website examples updated to use string interpolation instead of deprecated `to_str()` concatenation
- Variables & Types docs now document string interpolation nesting limitation

### Changed
- Release CI builds include LLVM backend for all 3 targets (ARM macOS, Intel macOS, Linux x86_64)

## [0.2.1] - 2026-04-03

### Added
- Production error codes (E0001-E0513) for all compiler diagnostics, Rust E0308-style
  - `ErrorCode` enum in `turbo-ast/src/errors.rs` with `as_str()`, `description()`, `Display`
  - All `SemaError`, `ParseError`, and `CodegenError` types now carry an `ErrorCode`
  - CLI displays errors as `error[E0100]: message` via ariadne
  - New `turbolang explain <code>` CLI subcommand prints the error description
  - Error code reference: `docs/errors.md`
- Gold standard audit: CI workflow, SECURITY.md, CLAUDE.md, allocation safety, strip binary
- CONTRIBUTING.md and CHANGELOG.md documentation
- `turbo-lsp` binary now included in Homebrew formula and release artifacts

### Fixed
- Replace last `panic!` in sema with proper error diagnostic
- Security audit: safety fixes, tracked artifact cleanup, .gitignore hardening
- `turbolang test <dir>` now discovers `@test` functions in all `.tb` files, not just `test_*`/`*_test` named files

### Removed
- Implicit `int + str` / `str + int` coercion in the `+` operator — use `to_str()` or string interpolation instead

## [0.1.0] - 2026-03-10

Initial public release of the Turbo compiler.

### Language Features
- Core types: `i32`, `i64`, `u32`, `u64`, `f32`, `f64`, `bool`, `str`, `()`, arrays `[T]`
- `let` / `let mut` variable bindings with type inference
- `if`/`else` and `while` as expressions (return values)
- `for..in` loops over arrays
- `break` and `continue` in loops
- `match` expressions with pattern matching
- Match guards (`n if n > 0 => ...`)
- Enum types with data-carrying variants (tagged union representation)
- Match exhaustiveness checking
- Struct types with field access and field assignment
- Generic functions with type parameter inference
- Generic structs and enums with type parameters
- Multi-parameter generics with trait bounds
- Basic trait system with built-in `Display` trait
- Default trait methods
- `@derive(Eq, Clone, Display)` attribute for auto-generating trait implementations
- Optional types (`T?`)
- `Result`/`Error` types with `ok`, `err` constructors and pattern matching
- `?` operator for error propagation
- Closures with variable capture (heap-allocated environments)
- Closure parameter type inference and single-expression bodies
- Higher-order functions (function types as parameters)
- `async fn`, `spawn`, and `await` with thread-based concurrency
- `sleep` builtin for async timing
- Channels and mutex builtins for thread communication
- `agent` keyword with instantiation and field access
- `tool fn` keyword for AI agent primitives
- `const` declarations
- `defer` statements
- `import` system for cross-file code sharing
- String interpolation with `{expr}` syntax
- String concatenation (`+`) and equality (`==`, `!=`) operators
- Multiline strings
- String coercion (automatic `to_string` in print contexts)
- Array index assignment and struct field assignment
- Copy-on-write semantics for shared arrays
- ARC refcount header on all heap allocations
- `@unsafe` functions and raw pointer operations
- `@test` attribute on functions with `assert_eq`/`assert_ne` builtins

### Built-in Functions
- `print` -- polymorphic output for all types
- `assert`, `assert_eq`, `assert_ne` -- runtime assertions
- `abs`, `min`, `max` -- math operations
- `to_str` -- value to string conversion
- `to_json`, `to_json_array` -- struct serialization to JSON
- `len`, `push` -- array operations
- `map`, `filter`, `reduce` -- higher-order collection operations
- `str_*` -- string manipulation builtins
- `hashmap`, `hashmap_get`, `hashmap_set`, `hashmap_keys` -- key-value storage
- `channel`, `mutex` -- concurrency primitives
- HTTP client builtins -- `http_get`, `http_post`, JSON parsing
- Math builtins -- `math_sqrt`, `math_floor`, etc.
- I/O builtins -- `read_file`, `write_file`, `read_line`

### Compiler Infrastructure
- Five-stage pipeline: Lexer (logos) -> Parser (recursive descent) -> Sema -> Codegen -> Link
- Cranelift JIT backend (`turbolang run`) for rapid development
- Cranelift AOT backend (`turbolang build`) for native binary production
- C runtime (`turbo_rt.c`) linked into AOT binaries for print, allocation, strings, arrays, async
- Multi-error parser recovery (no bail-on-first-error)
- `Ty::Error` poison type in sema to prevent cascading diagnostics
- Colorized error reporting with `ariadne` and contextual help messages
- Function inlining optimization in codegen
- Benchmark suite comparing against C (gcc -O2)
- Comprehensive test suite: integration, regression, and adversarial tests
- Builtin shadowing rejection and argument count validation in sema

### Toolchain
- `turbolang run <file>` -- JIT compile and execute
- `turbolang build <file>` -- AOT compile to native binary
- `turbolang test <file>` -- run `@test` functions with subprocess-based runner
- `turbolang bench <file>` -- benchmark with timing
- `turbolang fmt <file>` -- source code formatter
- `turbolang doc <file>` -- documentation generator
- `turbolang init <name>` -- project scaffolding (with `.gitignore`)
- `turbolang install` -- dependency resolution from `turbo.toml` / `turbo_modules`
- `turbolang update` -- GitHub package registry support
- `turbolang repl` -- interactive REPL
- `turbolang lsp` -- Language Server Protocol server (diagnostics, hover, go-to-definition)
- VS Code extension (`zvndev.turbo-lang`) -- syntax highlighting, snippets, LSP client
- Built-in HTTP server framework with socket-based runtime

### Distribution
- Install script (`install.sh`) for curl-based installation
- Homebrew formula (`brew tap ZVN-DEV/turbo && brew install turbo-lang`)
- Dockerfile for containerized builds
- CI release workflow (`.github/workflows/release.yml`) -- cross-platform builds, release on tag

### Fixed
- Nested block comment parsing
- Semicolons accepted as statement terminators
- Unsigned literal coercion in call arguments
- Variable shadowing scope leak
- Array indexing element type loss
- Function inlining skips generic and Result-returning functions
- Critical codegen safety bugs (bounds checking, null guards)
- All critical and high-severity security issues for public release

### Security
- Checked allocation in C runtime (exits on OOM)
- SECURITY.md with vulnerability reporting policy
- `.gitignore` hardened to exclude build artifacts and sensitive files
- `@unsafe` block enforcement for raw pointer operations
