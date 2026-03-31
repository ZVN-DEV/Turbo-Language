# Changelog

All notable changes to the Turbo compiler are documented here.
Format based on [Keep a Changelog](https://keepachangelog.com/).

## [Unreleased]

### Added
- Gold standard audit: CI workflow, SECURITY.md, CLAUDE.md, allocation safety, strip binary
- CONTRIBUTING.md and CHANGELOG.md documentation
- Replace last `panic!` in sema with proper error diagnostic
- Security audit: safety fixes, tracked artifact cleanup, .gitignore hardening

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
- Cranelift JIT backend (`turbo run`) for rapid development
- Cranelift AOT backend (`turbo build`) for native binary production
- C runtime (`turbo_rt.c`) linked into AOT binaries for print, allocation, strings, arrays, async
- Multi-error parser recovery (no bail-on-first-error)
- `Ty::Error` poison type in sema to prevent cascading diagnostics
- Colorized error reporting with `ariadne` and contextual help messages
- Function inlining optimization in codegen
- Benchmark suite comparing against C (gcc -O2)
- Comprehensive test suite: integration, regression, and adversarial tests
- Builtin shadowing rejection and argument count validation in sema

### Toolchain
- `turbo run <file>` -- JIT compile and execute
- `turbo build <file>` -- AOT compile to native binary
- `turbo test <file>` -- run `@test` functions with subprocess-based runner
- `turbo bench <file>` -- benchmark with timing
- `turbo fmt <file>` -- source code formatter
- `turbo doc <file>` -- documentation generator
- `turbo init <name>` -- project scaffolding (with `.gitignore`)
- `turbo install` -- dependency resolution from `turbo.toml` / `turbo_modules`
- `turbo update` -- GitHub package registry support
- `turbo repl` -- interactive REPL
- `turbo lsp` -- Language Server Protocol server (diagnostics, hover, go-to-definition)
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
