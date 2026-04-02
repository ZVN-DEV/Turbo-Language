# Turbo Compiler — Developer Guide

## Quick Start

```bash
# Build (debug)
cargo build --manifest-path turbo/Cargo.toml

# Build (release)
cargo build --release --manifest-path turbo/Cargo.toml

# Run all unit tests
cargo test --workspace --exclude turbo-codegen-llvm --manifest-path turbo/Cargo.toml

# Run a .tb source file via JIT
cargo run --manifest-path turbo/Cargo.toml -- run turbo/tests/phase1/hello.tb

# Run integration tests (requires release build)
cd turbo && ./tests/run_tests.sh
```

## Architecture

The compiler is a five-stage pipeline:

```
Source (.tb)
  │
  ▼
Lexer (turbo-lexer, logos)  →  Token stream
  │
  ▼
Parser (turbo-parser, recursive descent)  →  AST (Module)
  │
  ▼
Semantic Analysis (turbo-sema)  →  Validated AST + type errors
  │
  ▼
Codegen (turbo-codegen-cranelift)  →  JIT execution  or  AOT .o file
  │                                      │
  ▼                                      ▼
  done                           link with turbo_rt.c → native binary
```

- **Lexer** tokenizes source using the `logos` crate. Whitespace-insensitive; newlines and semicolons are filtered out by the parser.
- **Parser** is hand-written recursive descent. Collects multiple errors for better diagnostics (no bail-on-first-error).
- **Sema** walks the AST to resolve types, check scopes, validate match exhaustiveness, and enforce trait bounds. Produces `Vec<SemaError>`.
- **Codegen** translates the AST into Cranelift IR. `jit_run()` for development, `aot_compile()` for production binaries.
- **C Runtime** (`turbo_rt.c`) provides `rt_print_*`, allocation helpers, string operations, array/hashmap support, async primitives, and math functions. Linked into AOT binaries via `cc`.

## Crate Map

| Crate | Path | Purpose |
|-------|------|---------|
| `turbo-lexer` | `crates/turbo-lexer/` | Logos-based tokenizer. Defines `Token` enum and `Spanned<Token>`. |
| `turbo-ast` | `crates/turbo-ast/` | AST node definitions (`Module`, `Item`, `Expr`, `Stmt`, `TypeExpr`, `Pattern`) and `ErrorCode` enum. |
| `turbo-parser` | `crates/turbo-parser/` | Recursive descent parser. Entry: `parse(tokens) -> (Module, Vec<ParseError>)`. |
| `turbo-sema` | `crates/turbo-sema/` | Semantic analysis and type checking. Entry: `check(module) -> Vec<SemaError>`. |
| `turbo-codegen-cranelift` | `crates/turbo-codegen-cranelift/` | Cranelift JIT + AOT backend. Entries: `jit_run()`, `aot_compile()`. |
| `turbo-cli` | `crates/turbo-cli/` | CLI frontend (clap). Commands: run, build, test, fmt, init, lsp, repl, bench, doc, playground. |
| `turbo-lsp` | `crates/turbo-lsp/` | LSP server (lsp-server crate). Provides diagnostics, hover, and go-to-definition. |

## Key Types

**turbo-ast** (the shared vocabulary):
- `Span` — `Range<usize>`, byte offsets into source.
- `Spanned<T>` — wraps any node with its `Span`.
- `Module` — root AST node; contains `Vec<Spanned<Item>>`.
- `Item` — top-level: `Function(FnDef)`, `Struct(StructDef)`, `Enum(EnumDef)`, `Impl(ImplBlock)`, `Trait(TraitDef)`, `Agent(AgentDef)`, `Import`, `Const`.
- `FnDef` — function name, params, return type, body, plus flags: `is_async`, `is_tool`, `is_test`, `is_unsafe`.
- `StructDef` — name, type params, derives, fields.
- `EnumDef` — name, type params, variants (each may carry data fields).
- `Expr` — expression nodes: literals, `BinaryOp`, `Call`, `If`, `While`, `Match`, `Block`, `FieldAccess`, `MethodCall`, `Closure`, `ArrayLit`, `Index`, `Assign`, `Spawn`, `Await`, etc.
- `Stmt` — `Let { mutable, name, ty, value }`, `Expr(...)`, `Return(...)`, `Defer(...)`.
- `TypeExpr` — type syntax: `Named`, `Unit`, `Array`, `FnType`, `Result`, `Optional`, `Future`, `Inferred`.

**turbo-ast**:
- `ErrorCode` — unique error code enum (E0001-E0513). Defined in `turbo-ast/src/errors.rs`. Used by all error types. See `docs/errors.md` for the full table.

**turbo-sema**:
- `Ty` — internal type representation: `I32`, `I64`, `U32`, `U64`, `F32`, `F64`, `Bool`, `Str`, `Unit`, `Array(Box<Ty>)`, `Struct(String)`, `Enum(String)`, `Fn(Vec<Ty>, Box<Ty>)`, `Result`, `Optional`, `Future`, `TypeParam`, `Agent`, `Error`.
- `SemaError` — `{ code: ErrorCode, message: String, span: Span }`.

**turbo-codegen-cranelift**:
- `TurboTy` — codegen-level type tag (distinct from `Ty` because Cranelift IR types alone can't distinguish e.g. `str` from `i64` on ARM64).
- `CodegenError` — `{ code: ErrorCode, message: String }`.

## Patterns

### Error handling
- Every diagnostic carries an `ErrorCode` (e.g. `E0100`) for searchable, unique identification. Codes are defined in `turbo-ast/src/errors.rs`. Full reference: `docs/errors.md`.
- `turbo explain E0100` prints the description for any error code.
- All error types carry a `Span` (except `CodegenError`). The CLI uses `ariadne` to render them as pretty diagnostics with the format `error[E0100]: message`.
- The parser collects errors into `Vec<ParseError>` and continues parsing (error recovery).
- Sema uses `Ty::Error` as a poison type to avoid cascading errors — if an expression has type `Ty::Error`, further checks on it are skipped.

### Built-in functions
- Built-in functions (`print`, `assert`, `len`, `push`, `str_*`, `hashmap_*`, `math_*`, etc.) are handled as special cases inside `compile_call()` in codegen (line ~5221 of `lib.rs`). They are not in the AST; the compiler recognizes them by name.
- To add a new built-in: add a branch in `compile_call()`, implement the JIT function pointer in the codegen setup, and add the C implementation in `turbo_rt.c` for AOT.

### Test structure
- Integration tests live in `turbo/tests/phase1/` as pairs: `foo.tb` (source) + `foo.expected` (expected stdout).
- `run_tests.sh` compiles each `.tb` via `turbo run`, captures stdout, and diffs against `.expected`.
- Expected-error tests: if the `.expected` file starts with `ERROR:`, the test runner checks that the compiler error output contains the pattern.
- Unit tests use standard `#[cfg(test)]` modules inside each crate.

## Testing

### Running tests
```bash
# Unit tests (all crates)
cargo test --all --manifest-path turbo/Cargo.toml

# Integration tests (needs release build)
cargo build --release --manifest-path turbo/Cargo.toml
cd turbo && ./tests/run_tests.sh

# Single file
cargo run --manifest-path turbo/Cargo.toml -- run turbo/tests/phase1/fibonacci.tb
```

### Adding a test
1. Create `turbo/tests/phase1/my_feature.tb` with a `fn main()` that prints output.
2. Create `turbo/tests/phase1/my_feature.expected` with the exact expected stdout.
3. For error tests, put `ERROR:<pattern>` as the first line of `.expected`.

## Common Tasks

### Adding a new built-in function

1. **Runtime** (`turbo/crates/turbo-codegen-cranelift/runtime/turbo_rt.c`): implement the C function (e.g. `rt_my_func`).
2. **JIT setup** (`turbo/crates/turbo-codegen-cranelift/src/lib.rs`): register the function pointer in the JIT symbol table so the JIT can call it.
3. **Codegen** (`compile_call()` in the same file): add a branch that matches the function name, builds the Cranelift IR call, and returns the correct `TurboTy`.
4. **Sema** (`turbo/crates/turbo-sema/src/lib.rs`): add the function signature to the built-in type environment so the type checker accepts calls to it.
5. **Test**: add a `.tb`/`.expected` pair in `turbo/tests/phase1/`.

### Adding a new AST node

1. **AST** (`turbo/crates/turbo-ast/src/lib.rs`): add the variant to `Expr`, `Stmt`, or `Item`.
2. **Lexer** (`turbo/crates/turbo-lexer/src/lib.rs`): add any new tokens/keywords.
3. **Parser** (`turbo/crates/turbo-parser/src/lib.rs`): parse the new syntax into the new AST node.
4. **Sema** (`turbo/crates/turbo-sema/src/lib.rs`): add type-checking logic for the new node.
5. **Codegen** (`turbo/crates/turbo-codegen-cranelift/src/lib.rs`): add code generation for the new node.
6. **Formatter** (`turbo/crates/turbo-cli/src/formatter.rs`): handle pretty-printing if applicable.

### Adding a new type

1. **AST**: add to `TypeExpr` if it needs new syntax.
2. **Lexer**: add keyword token if needed.
3. **Parser**: parse the type expression.
4. **Sema**: add to `Ty` enum, implement type-checking rules, update `resolve_type_expr()`.
5. **Codegen**: add to `TurboTy` enum, implement `turbo_ty_from_type_expr()`, handle in `compile_expr()`.
6. **Runtime**: add C support functions in `turbo_rt.c` if the type needs runtime representation.

## File Layout

```
turbo/
  Cargo.toml              # Workspace root
  crates/
    turbo-lexer/          # Token definitions + logos lexer
    turbo-ast/            # AST types (shared by all crates)
    turbo-parser/         # Recursive descent parser
    turbo-sema/           # Type checking + semantic analysis
    turbo-codegen-cranelift/
      src/lib.rs          # Cranelift JIT + AOT codegen (~7k lines)
      runtime/turbo_rt.c  # C runtime linked into AOT binaries
    turbo-cli/            # CLI entry point + formatter + REPL + playground
    turbo-lsp/            # Language Server Protocol server
  tests/
    phase1/               # Integration tests (.tb + .expected pairs)
    adversarial/          # Edge-case / adversarial tests
    regression/           # Regression tests
    run_tests.sh          # Integration test runner script
design/                   # Language specification documents
examples/                 # Example applications (web-api, desktop-app, etc.)
```
