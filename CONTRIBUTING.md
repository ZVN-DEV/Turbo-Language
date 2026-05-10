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
cargo test --workspace --exclude turbo-codegen-llvm --manifest-path turbo/Cargo.toml

# Run a source file to sanity-check
cargo run --manifest-path turbo/Cargo.toml -- run turbo/tests/phase1/hello.tb
```

## Project Structure

The compiler is a Cargo workspace under `turbo/` with seven crates:

| Crate | Purpose |
|-------|---------|
| `turbo-lexer` | Logos-based tokenizer. Produces `Spanned<Token>` stream. |
| `turbo-ast` | Shared AST types: `Module`, `Item`, `Expr`, `Stmt`, `TypeExpr`, `Pattern`. |
| `turbo-parser` | Recursive descent parser with multi-error recovery; also runs a post-parse COW rewrite pass (`cow_rewrite.rs`). |
| `turbo-sema` | Semantic analysis: type checking, scope resolution, exhaustiveness. |
| `turbo-codegen-cranelift` | Cranelift JIT + AOT backend. Also contains `runtime/turbo_rt.c`. |
| `turbo-cli` | CLI frontend: run, build, test, fmt, init, lsp, repl, bench, doc. |
| `turbo-lsp` | Language Server Protocol server (diagnostics, hover, go-to-def). |

**Compiler pipeline:** Lexer -> Parser (+ COW rewrite pass) -> Sema -> Codegen

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
cargo test --workspace --exclude turbo-codegen-llvm --manifest-path turbo/Cargo.toml

# Integration tests (requires release build)
cargo build --release -p turbo-cli --manifest-path turbo/Cargo.toml
cd turbo && ./tests/run_tests.sh
```

CI enforces all four of these checks. A PR that fails any of them will not be merged.

### Install the pre-commit hook

The repo ships a pre-commit hook at `turbo/.git-hooks/pre-commit` that runs
`cargo fmt --check` and `cargo clippy -D warnings` before every commit. Install
it once with:

```bash
git config core.hooksPath turbo/.git-hooks
```

The hook is fast (fmt first; clippy only runs if fmt passes) and prints the
command to re-run if any check fails.

## Adding Features

### New built-in function

1. **Sema** (`turbo-sema/src/lib.rs`) -- add the function signature to the built-in type environment.
2. **Codegen** (`turbo-codegen-cranelift/src/builtins.rs`) -- add a compile function, and add the dispatch branch in `compile_call()` in `src/expr.rs`.
3. **JIT setup** (`turbo-codegen-cranelift/src/jit.rs`) -- register the function pointer in the JIT symbol table.
4. **C runtime** (`turbo-codegen-cranelift/runtime/turbo_rt.c`) -- implement the C function for AOT builds.
5. **Test** -- add a `.tb`/`.expected` pair in `turbo/tests/phase1/`.
6. **COW registration** -- if the builtin returns a new value instead of mutating its first argument in place (like `push`, `map`, `trim`), add its name to the `COW_BUILTINS` list in `turbo-parser/src/cow_rewrite.rs` so that statement-position calls get rewritten into self-assigns (`arr.push(4)` becomes `arr = push(arr, 4)`).

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

## Error Codes and Documentation

Every diagnostic in Turbo carries a unique `ErrorCode` (e.g. `E0100`).
The long-form explanation lives in **one** place — never duplicate it.

- **Source of truth:** `turbo/crates/turbo-cli/src/errors/E0NNN.md`. The CLI
  embeds these via `include_str!` so `turbolang explain E0NNN` keeps working
  in a release binary with no filesystem dependency.
- **Public docs path:** `docs/errors/E0NNN.md` at the repo root. These are
  symlinks pointing back at the source-of-truth files. Both paths must
  resolve to the same content; `turbo-cli/build.rs` fails the build if any
  variant of `ErrorCode` is missing a `docs/errors/` entry.
- **URL convention:** every rendered error ends with a `more info:`
  footer pointing at the GitHub blob URL
  `https://github.com/ZVN-DEV/Turbo-Language/blob/master/docs/errors/E0NNN.md`.
  Once `turbolang.dev/errors/E0NNN` has a real redirect to the same
  content the CLI will be flipped back to the short form (see the
  `TODO(P3)` in `error_code_url()` in `turbo-cli/src/main.rs`).

### Adding a new error code

1. Add the variant to `turbo-ast::ErrorCode` (`turbo/crates/turbo-ast/src/errors.rs`).
2. Update the `as_str`, `description`, and `all` impls so the new variant
   is recognized everywhere.
3. Create the long-form explanation at
   `turbo/crates/turbo-cli/src/errors/E0NNN.md`.
4. Create the public symlink:
   ```bash
   cd docs/errors && ln -s ../../turbo/crates/turbo-cli/src/errors/E0NNN.md E0NNN.md
   ```
5. Add the `include_str!` line in `detailed_explanation()` inside
   `turbo-cli/src/main.rs`.
6. The build will fail until steps 3, 4, and 5 are all in place — that is
   the intended drift-prevention behavior, not a bug.

## Release Verification

Release artifacts ship with a signed `checksums.txt`. To verify a download
before installing:

```bash
# 1. Import the public release-signing key (one-time setup).
curl -sSL https://turbolang.dev/keys/release.asc | gpg --import

# 2. Verify the signature on the manifest.
gpg --verify checksums.txt.sig checksums.txt

# 3. Verify the tarball matches the signed checksum.
sha256sum --check --ignore-missing checksums.txt
```

If `gpg --verify` reports `Good signature`, the manifest is trusted; the
`sha256sum --check` step then proves the tarball you downloaded matches
the manifest entry.

## Code Style

- **Naming:** `snake_case` for functions and variables, `CamelCase` for types and enums.
- **Error types:** `SemaError`, `ParseError`, and `CodegenError` all carry a `Span` for diagnostic rendering. Use `ariadne` for pretty error output.
- **Comments:** Explain *why*, not *what*. The code should be clear enough to explain itself.
- **No `unwrap()` in production code.** Use `?` or `match` in the CLI, LSP, and codegen crates. `unwrap()` is acceptable only in unit tests.
- **Error propagation:** The parser collects errors into `Vec<ParseError>` and keeps going. Sema uses `Ty::Error` as a poison type to prevent cascading errors.
- **Built-in functions** are recognized by name in `compile_call()`, not in the AST. Follow the existing pattern when adding new ones.

## Security

If you discover a security vulnerability, **do not open a public issue.**
Use one of the private channels described in [`SECURITY.md`](SECURITY.md):

- **GitHub:** [Private vulnerability reporting](https://github.com/ZVN-DEV/Turbo-Language/security/advisories/new)

For details on the security model, threat boundaries, and what is in scope,
see [`SECURITY.md`](SECURITY.md).

## Questions?

Open an issue or start a discussion on GitHub. We are happy to help you find the right place to make your change.
