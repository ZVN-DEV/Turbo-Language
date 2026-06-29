# Turbo Safety Narrative

How Turbo keeps your programs correct -- and where responsibility shifts to you.

Turbo occupies a deliberate point on the safety spectrum: safer than C, comparable to Go, less restrictive than Rust. It prevents the most common classes of memory and type errors at compile time while keeping the language approachable for developers coming from TypeScript, Python, or Go. This document explains exactly what is guaranteed, what is checked, and what is left to the programmer.

---

## What Memory Errors Are Impossible

These classes of bugs cannot occur in safe Turbo code. The compiler or runtime structurally prevents them.

### Null Pointer Dereference

Turbo has no `null`. Optional values use the `T?` type, which is a tagged union of `some(T)` and `none`. You cannot access the inner value without explicitly handling both cases:

```turbo
fn find_user(id: i64) -> User? {
    if id == 1 {
        some(User { name: "Alice", age: 30 })
    } else {
        none
    }
}

fn main() {
    let u = find_user(2)
    // u.name          -- compile error: cannot access field on optional type
    // print(u.name)   -- compile error

    // You must handle both cases:
    let name = u?.name
    if let some(n) = name {
        print("Name: {n}")
    } else {
        print("No user found")
    }
}
```

The null coalescing operator `??` provides a concise default:

```turbo
let name = u?.name ?? "Anonymous"
```

There is no way to accidentally dereference a missing value.

### Use of Uninitialized Variables

Every variable must be initialized at the point of declaration. The parser rejects `let x: i64` without an initializer. There is no default zero-initialization that could silently produce wrong results.

```turbo
let x: i64         // compile error: variable must be initialized
let x: i64 = 0     // ok
let x = 42          // ok (type inferred)
```

### Type Confusion

Turbo's static type system prevents treating one type as another. There are no implicit conversions between unrelated types, no type coercion, and no `any` type in normal code.

```turbo
let x: i64 = 42
let s: str = x       // compile error [E0110]: type annotation does not match value type
let y: f64 = x       // compile error: no implicit numeric conversion
let y: f64 = 42.0    // ok
```

Struct types are nominal -- two structs with identical fields are distinct types:

```turbo
struct Meters { value: f64 }
struct Feet { value: f64 }

fn runway_length(m: Meters) -> f64 { m.value }

let f = Feet { value: 100.0 }
runway_length(f)    // compile error [E0100]: expected Meters, got Feet
```

---

## What Is Caught at Compile Time

The Turbo compiler (`turbo-sema`) performs exhaustive static analysis before any code runs. These errors are caught before compilation, with precise source locations and actionable error messages.

### Type Mismatches

Every expression has a statically known type. Assignments, function arguments, return values, and operator operands are all checked:

```turbo
fn add(a: i64, b: i64) -> i64 { a + b }

add(1, "hello")     // error [E0133]: builtin function argument type mismatch
add(1, 2, 3)        // error [E0513]: wrong number of arguments
```

### Non-Exhaustive Pattern Matching

The compiler rejects `match` expressions that do not cover every possible case:

```turbo
type Color { Red, Green, Blue }

fn name(c: Color) -> str {
    match c {
        Red => "red"
        Green => "green"
        // error [E0200]: match expression is not exhaustive
        // missing variant: Blue
    }
}
```

When you add a variant to an enum, the compiler flags every match expression that needs updating. This prevents the class of bugs where new enum values silently fall through to a default case.

### Undefined Variables and Functions

Name resolution catches references to variables, functions, structs, enums, and traits that do not exist:

```turbo
fn main() {
    print(x)           // error [E0300]: undefined variable `x`
    unknown_func()     // error [E0301]: undefined function `unknown_func`
    let p = Foo { }    // error [E0302]: undefined struct `Foo`
}
```

### Incorrect Function Arity

The compiler checks that every function call provides the correct number and types of arguments:

```turbo
fn greet(name: str) -> str { "Hello, {name}!" }

greet()                // error [E0513]: wrong number of arguments
greet("a", "b")        // error [E0513]: wrong number of arguments
```

### Immutability Enforcement

Variables are immutable by default. Attempting to mutate an immutable variable is a compile error:

```turbo
let x = 5
x = 10             // error [E0501]: cannot assign to immutable variable `x`

let mut y = 5
y = 10             // ok
```

### Result Type Enforcement

The `?` operator can only be used inside functions that return a `Result` type, and only on expressions that are themselves `Result` typed:

```turbo
fn risky() -> i64 ! str {
    let data = read_file("config.txt")    // read_file returns str, not Result
    // data?                              // error [E0120]: `?` operator requires a Result type
    ok(42)
}
```

---

## What Is Caught at Runtime

Some checks cannot be performed statically. Turbo inserts runtime checks for these cases, aborting with a clear error message rather than producing undefined behavior.

### Array Bounds Checking

Every array index operation checks that the index is within bounds:

```turbo
let arr = [10, 20, 30]
print(arr[5])     // runtime error: array index 5 out of bounds (length 3)
```

This prevents buffer overruns -- the single most exploited vulnerability class in C and C++ code.

### Integer Overflow in `pow`

The `pow` built-in checks for integer overflow. Instead of silently wrapping (as C does), Turbo aborts:

```turbo
let x = pow(2, 63)    // runtime error: integer overflow in pow
```

Negative exponents are also rejected at runtime rather than silently returning 1.

### Stack Overflow Protection

The compiler enforces a recursion depth limit of 256 levels. Deeply recursive or pathologically nested input produces a clear diagnostic (error code E0516) instead of crashing the process with a stack overflow.

### Division by Zero

Integer division by zero is caught at runtime:

```turbo
let x = 10 / 0    // runtime error: division by zero
```

### Shell Command Injection Prevention

The `exec` / `shell_exec` built-in rejects commands containing shell metacharacters (`;`, `|`, `&`, `$`, backticks, parentheses, `<`, `>`, newlines, backslashes). Commands are tokenized on whitespace and executed directly via `execvp` -- no shell is involved. This prevents shell injection attacks, though the underlying command still runs with the process's full OS permissions.

---

## What Is the Programmer's Responsibility

Turbo is honest about what it does not protect against. The following areas require programmer awareness and discipline.

### Memory Lifecycle in JIT Mode

Turbo's runtime uses a thread-local string arena. HTTP servers reclaim per-request memory on every request — the JIT (`turbolang run`) resets the arena to a per-request high-water mark and AOT (`turbolang build`) uses a per-request arena — so a long-running server's memory stays bounded (measured flat over thousands of requests on both backends), and state held in hashmaps persists correctly across requests (server-state maps are allocated outside the per-request arena, so they survive the per-request reset). The remaining case is a *non-server* long-running program that loops forever while continuously allocating strings: those arena allocations are freed when the program exits, not individually — ensure such a loop terminates or periodically restart the process. Proper ARC-based string deallocation is planned for a future release.

### File I/O Safety

Turbo provides no path sandboxing or chroot-style isolation. `read_file`, `write_file`, and `exec` operate with the full permissions of the running process. A program can read, write, or delete any file the OS user has access to.

For recoverable I/O, use `try_read_file` and `try_write_file`, which return `Result` types instead of crashing on failure. But neither variant restricts which paths can be accessed.

### HTTP Server Security

The built-in HTTP server (`http_server`, `http_listen`, `route`) is designed for development and demos. It is not hardened for direct exposure to untrusted networks:

- No TLS termination
- Request size limits enforced (8 KB per header line, 64 KB total headers, 32 MB body, 256 max connections) but not configurable
- No authentication or authorization middleware
- Connection cap with 503 backpressure exists but is tuned for development loads

**For production deployment, put a reverse proxy (nginx, Caddy) in front of any Turbo HTTP server.** See [SECURITY.md](../SECURITY.md) for the full threat model.

### Unsafe FFI

The `@unsafe extern "C"` block allows calling any C function. These calls bypass all of Turbo's safety checks:

```turbo
@unsafe
extern "C" {
    fn free(ptr: i64)
}
```

Inside `unsafe` blocks, you can dereference raw memory addresses and write to arbitrary locations. This is equivalent to writing C -- all memory safety guarantees are void. Turbo's `deref` and `store` builtins are only available in unsafe context for this reason.

### Concurrent Data Access

Turbo provides `mutex`, `channel`, `send`, and `recv` for concurrent programming. However, sharing mutable state across `spawn`-ed tasks without proper synchronization is the programmer's responsibility. Turbo does not have Rust's `Send`/`Sync` trait system to statically prevent data races.

---

## Comparison with Other Languages

### vs. C

| Safety Property | C | Turbo |
|----------------|---|-------|
| Null pointer dereference | Possible (undefined behavior) | Impossible (`T?` requires explicit handling) |
| Buffer overflow | Possible (no bounds checking) | Prevented (runtime bounds checking) |
| Use-after-free | Possible | Not applicable (CoW value semantics) |
| Double free | Possible | Not applicable (no manual memory management in safe code) |
| Uninitialized variables | Possible (compiler may warn) | Impossible (enforced by parser) |
| Type confusion | Possible (implicit casts) | Impossible (no implicit conversions) |
| Integer overflow | Silent wraparound | Checked in `pow`; standard arithmetic wraps (matching hardware behavior) |
| Shell injection | Easy (via `system()`) | Metacharacter blocking on `exec`; no shell invocation |

**Turbo eliminates the top 5 vulnerability classes in C code** (buffer overflows, null dereferences, use-after-free, double free, format string bugs) while maintaining comparable performance.

### vs. Go

| Safety Property | Go | Turbo |
|----------------|-----|-------|
| Null safety | Runtime panic on nil dereference | Compile-time prevention via `T?` |
| Type safety | Sound, with `interface{}` escape | Sound, with no `any` in normal code |
| Memory management | Garbage collector (GC pauses possible) | CoW value semantics (no GC, deterministic) |
| Exhaustive matching | No sum types | Enforced on all `match` expressions |
| Error handling | `error` interface (can be ignored) | `T ! E` Result types (compiler tracks them) |
| Data race prevention | Race detector (runtime, opt-in) | Mutex/channel primitives (no static analysis) |
| Bounds checking | Yes | Yes |

Turbo and Go offer similar levels of runtime safety. Turbo catches more at compile time (null safety, exhaustive matching, Result types) while Go has the advantage of a mature race detector. Turbo avoids GC pauses, which matters for latency-sensitive applications.

### vs. Rust

| Safety Property | Rust | Turbo |
|----------------|------|-------|
| Memory safety | Guaranteed by borrow checker | CoW semantics prevent most issues; no ownership tracking |
| Data race prevention | Compile-time (`Send`/`Sync`) | Runtime (mutex/channel; no static analysis) |
| Null safety | `Option<T>` | `T?` (equivalent) |
| Type safety | Sound | Sound |
| Exhaustive matching | Enforced | Enforced |
| Lifetime tracking | Explicit lifetimes at API boundaries | Not applicable (no borrowing model) |
| Unsafe escape hatch | `unsafe` blocks | `@unsafe` blocks |

**Rust provides stronger guarantees.** Rust's borrow checker prevents data races at compile time and guarantees memory safety without runtime cost. Turbo's CoW value semantics are simpler to learn and use but do not provide the same level of compile-time proof. In particular:

- Turbo cannot statically prevent data races in concurrent code.
- Turbo's arena-based allocation frees memory in bulk (per-request for HTTP servers, at exit otherwise) rather than freeing each allocation individually.
- Turbo does not track lifetimes, so dangling references in `@unsafe` code are the programmer's problem.

The tradeoff is deliberate: Turbo trades Rust's maximum safety for a dramatically lower learning curve. For most application-level code (web servers, CLI tools, data processing), Turbo's safety level is sufficient. For kernel code, safety-critical systems, or adversarial environments, Rust's guarantees are worth the complexity.

---

## The Safety Summary

| Category | Guarantee Level |
|----------|----------------|
| Null dereference | **Impossible** -- `T?` requires handling |
| Uninitialized variables | **Impossible** -- parser enforces initialization |
| Type confusion | **Impossible** -- static type system, no implicit casts |
| Undefined names | **Compile error** -- all names resolved before codegen |
| Non-exhaustive match | **Compile error** -- all variants must be covered |
| Immutability violation | **Compile error** -- `let` vs `let mut` enforced |
| Array bounds | **Runtime check** -- index out of bounds aborts |
| Integer overflow (pow) | **Runtime check** -- overflow aborts |
| Stack overflow | **Compile/runtime limit** -- 256-level recursion cap |
| Division by zero | **Runtime check** -- division by zero aborts |
| Shell injection | **Runtime blocked** -- metacharacters rejected |
| Data races | **Programmer responsibility** -- use mutex/channel |
| Memory lifecycle | **Per-request for servers** -- arena reclaimed each request; freed at exit otherwise |
| File I/O paths | **Programmer responsibility** -- no sandboxing |
| HTTP security | **Programmer responsibility** -- use reverse proxy |
| FFI safety | **Programmer responsibility** -- `@unsafe` voids guarantees |

Turbo's safety story is: **safe enough for application development, honest about its limits, and transparent about what you must handle yourself.**
