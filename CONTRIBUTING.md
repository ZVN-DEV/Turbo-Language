# Contributing to Turbo

Thanks for your interest in contributing to the Turbo compiler. This guide covers everything you need to get started.

## Development Setup

### Prerequisites

- **Rust** (stable toolchain) -- install via [rustup](https://rustup.rs/)
- **C compiler** (`cc`) -- required for linking the C runtime into AOT binaries
- **Git**

### Build and Verify

```bash
git clone https://github.com/ZVN-DEV/Turbo-Language.git
cd Turbo-Language

# Build (debug)
cargo build --manifest-path turbo/Cargo.toml

# Run all unit tests
cargo test --all --manifest-path turbo/Cargo.toml

# Run a source file to sanity-check
cargo run --manifest-path turbo/Cargo.toml -- run turbo/tests/phase1/hello.tb
```

## Project Structure

The compiler is a Cargo workspace under `turbo/` with seven crates:

| Crate | Purpose |
|-------|---------|
| `turbo-lexer` | Logos-based tokenizer. Produces `Spanned<Token>` stream. |
| `turbo-ast` | Shared AST types: `Module`, `Item`, `Expr`, `Stmt`, `TypeExpr`, `Pattern`. |
| `turbo-parser` | Recursive descent parser with multi-error recovery. |
| `turbo-sema` | Semantic analysis: type checking, scope resolution, exhaustiveness. |
| `turbo-codegen-cranelift` | Cranelift JIT + AOT backend. Also contains `runtime/turbo_rt.c`. |
| `turbo-cli` | CLI frontend: run, build, test, fmt, init, lsp, repl, bench, doc. |
| `turbo-lsp` | Language Server Protocol server (diagnostics, hover, go-to-def). |

**Compiler pipeline:** Lexer -> Parser -> Sema -> Codegen

See `CLAUDE.md` for detailed architecture, key types, and common task walkthroughs.

### Test Locations

- **Integration tests:** `turbo/tests/phase1/` -- `.tb` source files with matching `.expected` output files.
- **Unit tests:** `#[cfg(test)]` modules inside each crate.
- **Test runner:** `turbo/tests/run_tests.sh` runs all integration tests against a release build.

## Development Workflow

Before opening a PR, make sure all checks pass locally:

```bash
# Format -- must produce no changes
cargo fmt --all --manifest-path turbo/Cargo.toml

# Lint -- must produce zero warnings
cargo clippy --all --manifest-path turbo/Cargo.toml -- -D warnings

# Unit tests
cargo test --all --manifest-path turbo/Cargo.toml

# Integration tests (requires release build)
cargo build --release --manifest-path turbo/Cargo.toml
cd turbo && ./tests/run_tests.sh
```

CI enforces all four of these checks. A PR that fails any of them will not be merged.

## Adding Features

### New built-in function

1. **Sema** (`turbo-sema/src/lib.rs`) -- add the function signature to the built-in type environment.
2. **Codegen** (`turbo-codegen-cranelift/src/lib.rs`) -- add a branch in `compile_call()` to emit the Cranelift IR call.
3. **JIT setup** (same file) -- register the function pointer in the JIT symbol table.
4. **C runtime** (`turbo-codegen-cranelift/runtime/turbo_rt.c`) -- implement the C function for AOT builds.
5. **Test** -- add a `.tb`/`.expected` pair in `turbo/tests/phase1/`.

### New syntax

1. **Lexer** (`turbo-lexer/src/lib.rs`) -- add any new tokens or keywords.
2. **AST** (`turbo-ast/src/lib.rs`) -- add the new variant to `Expr`, `Stmt`, or `Item`.
3. **Parser** (`turbo-parser/src/lib.rs`) -- parse the new syntax into the AST node.
4. **Sema** (`turbo-sema/src/lib.rs`) -- add type-checking logic.
5. **Codegen** (`turbo-codegen-cranelift/src/lib.rs`) -- add code generation.
6. **Formatter** (`turbo-cli/src/formatter.rs`) -- handle pretty-printing if applicable.
7. **Test** -- add a `.tb`/`.expected` pair in `turbo/tests/phase1/`.

### New integration test

1. Create `turbo/tests/phase1/my_feature.tb` with a `fn main()` that prints output.
2. Create `turbo/tests/phase1/my_feature.expected` with the exact expected stdout.
3. For error tests, put `ERROR:<pattern>` as the first line of `.expected`.

## Pull Request Process

1. **Fork** the repository and create a feature branch from `master`.
2. **Implement** your change, following the patterns above.
3. **Test** locally -- all four checks (fmt, clippy, tests, integration) must pass.
4. **Open a PR** against `master` with a clear description of what and why.
5. **Keep PRs focused** -- one feature or one fix per PR. Split large changes into smaller PRs.

CI runs automatically on every PR. All checks must pass before merge.

## Code Style

- **Naming:** `snake_case` for functions and variables, `CamelCase` for types and enums.
- **Error types:** `SemaError`, `ParseError`, and `CodegenError` all carry a `Span` for diagnostic rendering. Use `ariadne` for pretty error output.
- **Comments:** Explain *why*, not *what*. The code should be clear enough to explain itself.
- **No `unwrap()` in production code.** Use `?` or `match` in the CLI, LSP, and codegen crates. `unwrap()` is acceptable only in unit tests.
- **Error propagation:** The parser collects errors into `Vec<ParseError>` and keeps going. Sema uses `Ty::Error` as a poison type to prevent cascading errors.
- **Built-in functions** are recognized by name in `compile_call()`, not in the AST. Follow the existing pattern when adding new ones.

## Questions?

Open an issue or start a discussion on GitHub. We are happy to help you find the right place to make your change.
