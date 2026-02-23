# The Polyglot / Meta-Language Strategy

## Assessment
The full polyglot meta-language (convert any language to/from Turbo) is NOT feasible as a primary goal. Here's what IS feasible, organized into tiers.

## Tier 1 — Ship With the Language

### C FFI (Zero-Overhead)
Like Zig's C interop. Call any C library directly, no wrapper needed.
- Import C headers directly
- Zero-overhead: no marshaling, no copying
- Access the entire C ecosystem (POSIX, OpenSSL, SQLite, etc.)
- Turbo's types map directly to C types for FFI-compatible structs

```
// Import a C library
extern "C" {
  fn sqlite3_open(filename: *const c_char, db: *mut *mut sqlite3) -> c_int
  fn sqlite3_close(db: *mut sqlite3) -> c_int
}

// Or higher-level: auto-generate bindings from C headers
import sqlite from "ffi/c_import(sqlite3.h)"

fn main() {
  let db = sqlite.open("test.db")?
  defer db.close()
  // ...
}
```

### WASM Component Model Interop
The emerging standard for language-agnostic binary interop.
- Components written in any language can interop at the WASM level
- Typed interfaces via WIT (WASM Interface Types)
- No shared memory — capabilities-based security
- The future of cross-language interop

```
// Define a component interface (WIT)
// component.wit:
// interface calculator {
//   add: func(a: f64, b: f64) -> f64
// }

// Import a component written in any language
import calc from "wasm/component(calculator.wasm)"

let result = calc.add(1.0, 2.0)
```

### JS Interop (WASM Target)
When targeting WASM for browser/Node.js:
- Call JS functions from Turbo
- Expose Turbo functions to JS
- Auto-generated TypeScript type definitions
- DOM access through a bridge layer

```
// Declare JS imports
extern "js" {
  fn alert(msg: str)
  fn document_querySelector(selector: str) -> JsElement
  async fn fetch(url: str) -> JsResponse
}

// Export to JS
@wasm_export
pub fn process(data: str) -> str {
  transform(data)
}

// Generated TypeScript:
// export function process(data: string): string;
```

## Tier 2 — Community / Tooling

### Python Interop
Like PyO3 for Rust. Write high-performance extensions for Python.
- Write Python extension modules in Turbo
- Call Python from Turbo (embed the interpreter)
- Numpy-compatible array types
- Seamless with Python's async

```
// Create a Python module
@python_module
mod fast_math {
  @python_fn
  fn fibonacci(n: u64) -> u64 {
    match n {
      0 | 1 => n
      _ => fibonacci(n - 1) + fibonacci(n - 2)
    }
  }
}

// In Python:
// import fast_math
// fast_math.fibonacci(40)  # 200x faster than pure Python
```

### TypeScript Type Generation
Generate .d.ts files from Turbo's types:
- Automatic for all `@wasm_export` functions
- Full type fidelity (generics, unions, etc.)
- npm package generation: `turbo build --target wasm --npm`

## Tier 3 — Future Aspirations

### Source-to-Source Translation (AI-Assisted)
- Not a language feature — a tool
- AI-assisted code conversion from Python/JS/Go to Turbo
- Like Amazon Q Code Transformation
- Useful for migration, not interop

### Bidirectional Transpilation
- Theoretically possible for a subset of the language
- Haxe's approach is the most realistic model (target-specific code + conditional compilation)
- Would require defining a "portable subset" of the language
- Low priority — focus on being excellent at native/WASM

## The Realistic Path

**Be an excellent native/WASM language with seamless C interop.** Let WASM Component Model handle cross-language interop at the binary level. Don't try to be Haxe — focus on being the best version of Turbo.

### Interop Priority Matrix

| Language | Mechanism | Priority | Status |
|----------|-----------|----------|--------|
| C | Direct FFI (zero-overhead) | P0 — ship with 1.0 | Core feature |
| JavaScript | WASM bridge + auto-bindings | P0 — ship with 1.0 | Core feature |
| TypeScript | .d.ts generation | P0 — ship with 1.0 | Core feature |
| Python | Extension modules (PyO3-style) | P1 — ship within 6mo | Tooling |
| WASM components | Component Model support | P1 — ship within 6mo | Core feature |
| Go | Via C FFI (cgo) | P2 — community | Guide only |
| Rust | Via C FFI (extern "C") | P2 — community | Guide only |
| Java/JVM | Via JNI or WASM | P3 — future | Aspirational |

## Why Not Full Transpilation

1. **Semantic gaps** — Languages have fundamentally different semantics (ownership vs GC, exceptions vs Result, null vs Option)
2. **Lowest common denominator** — Targeting all languages means you can only use features common to all
3. **Maintenance burden** — Each target is a separate compiler backend to maintain
4. **Quality ceiling** — Generated code is never idiomatic; developers hate non-idiomatic code
5. **Haxe's lesson** — Multi-target works but limits the language to the intersection of all targets

## What Makes Turbo Interop Special

1. **C FFI is zero-cost** — Not through a bridge, not through marshaling. Direct.
2. **WASM bridge is auto-generated** — Write code, get JS/TS bindings for free
3. **Type-safe interop** — The type system carries across the boundary where possible
4. **Agent tools work across boundaries** — Define tools in Turbo, call them from Python agents (or vice versa)
