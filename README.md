# Turbo

A compiled, type-safe programming language with JavaScript's developer experience, Rust's performance, and first-class AI agent primitives. Compiles to native code via Cranelift and targets WebAssembly.

**North star:** JavaScript's soul. Rust's speed. Built for the AI age.

## Quick Start

### Installation

```bash
# Clone and build from source
git clone https://github.com/ZVN-DEV/Turbo-Language.git
cd Turbo-Language/turbo
cargo build --release

# Add to your PATH
export PATH="$PWD/target/release:$PATH"

# Verify installation
turbo --version
```

### Hello, World

Create `hello.tb`:

```turbo
fn main() {
    print("Hello, world!")
}
```

Run it:

```bash
turbo run hello.tb
```

Compile to a native binary:

```bash
turbo build hello.tb
./hello
```

### A Taste of Turbo

```turbo
/// Compute the nth Fibonacci number.
fn fib(n: i64) -> i64 {
    if n <= 1 {
        n
    } else {
        fib(n - 1) + fib(n - 2)
    }
}

fn main() {
    let mut i = 0
    while i <= 15 {
        print(fib(i))
        i += 1
    }
}
```

```turbo
/// Absolute value.
fn abs(x: i64) -> i64 {
    if x < 0 { 0 - x } else { x }
}

/// Clamp a value to a range.
fn clamp(val: i64, lo: i64, hi: i64) -> i64 {
    if val < lo { lo }
    else { if val > hi { hi } else { val } }
}

fn main() {
    assert(abs(-42) == 42)
    assert(clamp(150, 0, 100) == 100)
    print("All checks passed!")
}
```

## Features

### Compiled to Native Code

Turbo compiles directly to machine code using Cranelift. No interpreter, no VM, no garbage collector. Programs start instantly and run at native speed.

- **JIT execution** via `turbo run` for rapid development (Cranelift)
- **AOT compilation** via `turbo build` for production binaries (Cranelift)
- **Optimized AOT** via `turbo build --llvm` for maximum performance (LLVM 18)
- Beats C and Rust on recursive benchmarks (fib(40): 160ms vs C 170ms, Rust 180ms)

### Type System

Strong static typing with type inference, generics, traits, and algebraic data types:

```turbo
struct Point<T> {
    x: T,
    y: T,
}

type Option<T> {
    Some(T)
    None
}

trait Display {
    fn to_string(self) -> str
}

fn identity<T>(x: T) -> T { x }
```

Supported types: `i32`, `i64`, `u32`, `u64`, `f32`, `f64`, `bool`, `str`, `()`, arrays `[T]`, optionals `T?`, results `T ! E`, futures `Future<T>`.

### Expressions Everywhere

`if`, `while`, `match`, and blocks are expressions that return values:

```turbo
fn classify(n: i64) -> str {
    if n > 0 { "positive" }
    else { if n < 0 { "negative" } else { "zero" } }
}
```

### Pattern Matching

```turbo
type Shape {
    Circle(f64)
    Rectangle(f64, f64)
}

fn describe(s: Shape) -> str {
    match s {
        Circle(r) => "circle"
        Rectangle(w, h) => "rectangle"
    }
}
```

### Async Runtime

Real async/await with thread-based concurrency:

```turbo
async fn fetch_data() -> i64 {
    sleep(100)
    42
}

fn main() {
    let handle = spawn fetch_data()
    let result = await handle
    print(result)
}
```

### AI Agent Primitives

First-class `agent` and `tool fn` keywords for building AI-powered applications:

```turbo
agent Assistant {
    model: "claude-sonnet"
    system: "You help with tasks."
}

fn main() {
    let a = Assistant {}
    print(a.model)
}
```

### Closures and Higher-Order Functions

```turbo
fn apply(f: fn(i64) -> i64, x: i64) -> i64 {
    f(x)
}

fn main() {
    let double = |x: i64| -> i64 { x * 2 }
    print(apply(double, 21))
}
```

### Module System

```turbo
import { Point, distance } from "geometry"

fn main() {
    let p = Point { x: 3, y: 4 }
    print(distance(p))
}
```

## CLI Commands

| Command | Description |
|---------|-------------|
| `turbo run <file.tb>` | Compile and run via JIT |
| `turbo run <file.tb> -v` | Run with verbose output (tokens, AST, timing) |
| `turbo build <file.tb>` | Compile to native binary (Cranelift) |
| `turbo build --llvm <file.tb>` | Compile with LLVM optimizations |
| `turbo build <file.tb> -o name` | Compile with custom output name |
| `turbo install` | Install dependencies from turbo.toml |
| `turbo doc <file.tb>` | Generate markdown documentation |
| `turbo fmt <file.tb>` | Format source code |
| `turbo init <name>` | Create a new project |
| `turbo repl` | Interactive REPL |
| `turbo lsp` | Start the Language Server |
| `turbo playground` | Launch interactive playground in browser |

## Project Structure

```
turbo/
  crates/
    turbo-lexer/                # Tokenizer (logos-based)
    turbo-ast/                  # Abstract syntax tree definitions
    turbo-parser/               # Recursive descent parser
    turbo-sema/                 # Semantic analysis and type checking
    turbo-codegen-cranelift/    # Code generation (Cranelift JIT + AOT)
    turbo-codegen-llvm/         # Optimized code generation (LLVM 18)
    turbo-cli/                  # CLI frontend (run/build/doc/fmt/repl)
    turbo-lsp/                  # Language Server Protocol implementation
  tests/
    phase1/                     # End-to-end test programs
design/                         # Language specification documents
examples/                       # Full application examples
```

## Language Design

The complete language specification spans design documents in `design/`:

- **SYNTAX.md** -- Full syntax reference
- **TYPE-SYSTEM.md** -- Types, generics, error handling (`T ! E`), algebraic data types
- **MEMORY-MODEL.md** -- CTRC ownership, auto-clone, regions, arenas
- **CONCURRENCY.md** -- Async/await, actors, channels, structured concurrency
- **AGENTIC.md** -- AI agent primitives: `tool fn`, `agent` keyword, streaming
- **COMPILATION.md** -- Cranelift backend, WASM pipeline
- **TOOLCHAIN.md** -- Testing framework, package manager, formatter, profiler

## Current Status

The compiler implements:

- Full lexer with keyword and operator support
- Recursive descent parser for all language constructs
- Type checker with inference, generics, traits, and mutability tracking
- Cranelift-based code generation (JIT execution and AOT native binaries)
- Structs with fields and impl blocks (methods)
- Enums with data-carrying variants and pattern matching
- Generics with trait bounds
- Closures with variable capture
- For-in loops with ranges and arrays
- Async/await with thread-based spawn and sleep
- Agent definitions with field access and instantiation
- Import/module system
- Result and Optional types
- String interpolation
- Higher-order functions (map, filter, reduce)
- Standard library builtins (math, strings, I/O)
- Interactive REPL
- LSP server with diagnostics, hover, and go-to-definition
- VS Code extension for syntax highlighting
- Code formatter (`turbo fmt`)
- Documentation generator (`turbo doc`)

## LLVM Backend (Optional)

For maximum performance, install LLVM 18 and rebuild:

```bash
# macOS
brew install llvm@18

# Build with LLVM support
LLVM_SYS_180_PREFIX=/opt/homebrew/opt/llvm@18 cargo build --release

# Compile with LLVM optimizations
turbo build --llvm myapp.tb -o myapp
```

## Performance

Benchmarked on Apple Silicon (fib(40), recursive):

| Language | Time | Binary Size |
|----------|------|-------------|
| **Turbo (LLVM)** | **160ms** | **35 KB** |
| C (cc -O2) | 170ms | 33 KB |
| Rust (rustc -O) | 180ms | 441 KB |
| Turbo (Cranelift) | 220ms | 35 KB |
| Node.js | 580ms | N/A |
| Python | 13.1s | N/A |

## License

MIT License. See [LICENSE](LICENSE) for details.
