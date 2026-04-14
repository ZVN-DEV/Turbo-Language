# Syntax & Ergonomics

## Elegant by Design

Turbo's surface syntax follows one north star: **JavaScript simplicity on the surface, Rust power underneath. Progressive disclosure.**

The language should feel *casual* and *intuitive* for the first 10 minutes, then reveal its depth as you need it. We don't copy Rust's syntax conventions when a simpler, more familiar alternative exists. Under the hood, the same powerful machinery runs — discriminated unions, ownership, zero-cost abstractions — but the surface is clean, lowercase, and immediately readable.

**Key elegance choices:**
- `T?` instead of `Option<T>` — Kotlin/Swift/Dart all prove this is the right call
- `T ! E` instead of `Result<T, E>` — the `!` means "this can fail"
- `none` instead of `None` — lowercase, casual, like `null` but safe
- `some(v)` / `ok(v)` / `err(e)` in patterns — lowercase consistency
- `{K: V}` for maps, `{T}` for sets — literal type syntax, not generics noise
- Auto-wrapping — return `"hello"` from a `str?` function, it just works

These aren't just cosmetic. They embody the principle: **make the common case beautiful, make the advanced case possible.**

## Familiar Feel

Turbo is designed so that **if you know JavaScript or TypeScript, you can write Turbo in 10 minutes**. We deliberately chose syntax that feels like home for the millions of JS/TS developers out there, while giving you Rust-level performance and safety under the hood.

- **Curly braces, arrow functions, template literals, destructuring** — all familiar from JS/TS
- **The type system helps you, it doesn't slow you down** — type inference handles most cases, you only annotate when it adds clarity
- **Progressive disclosure** — start writing code that looks like JavaScript. As you need more performance, add type annotations, ownership hints, and memory controls. You're never forced into complexity upfront
- **No surprises** — if a pattern works in JavaScript, it probably works in Turbo (minus the footguns like type coercion and `typeof null === "object"`)

```
// This looks like JavaScript — because it should
let name = "world"
let items = [1, 2, 3, 4, 5]
let doubled = items.map((x) => x * 2)
let { host, port, ...rest } = config
let greeting = "Hello, {name}!"

// But you get Rust-level safety
let result = await fetch("/api/data")  // str ! Error, not any
let user = response.json<User>()?       // Type-safe parsing
let city = user?.address?.city ?? "Unknown"  // Safe optional access
```

> **Status legend:** `[Implemented]` = compiles and runs today. `[Planned]` = language design only, not yet in compiler.

## Design Principles
- Readability over writability
- Familiarity for JS/TS/Rust developers
- Expressions over statements (everything returns a value)
- Immutable by default
- Minimal noise (no semicolons required, type inference where possible)

## What We Steal From Each Language

### From TypeScript/JavaScript
- Destructuring (objects and arrays): `let {name, age} = user`
- Template literals / string interpolation: `"Hello, {name}!"`
- Optional chaining: `user?.address?.city`
- Async/await feel
- Object/array literal syntax
- Arrow-style short lambdas: `(x) => x * 2`

### From Rust
- Pattern matching with `match` expressions
- Trait-based polymorphism
- `T?` optionals and `T ! E` results (inspired by Rust's `Option`/`Result`, with cleaner syntax)
- `?` operator for error propagation
- `let` for immutable, `let mut` for mutable
- Derive macros: `@derive(Debug, Eq, Serialize)`
- Expression-based (last expression is return value)

### From Python
- Clean readability
- Decorators (as attributes: `@test`, `@deprecated`)
- Comprehensions: `[x * 2 for x in items if x > 0]`
- F-string-style interpolation

### From Elixir
- Pipe operator: `data |> parse |> validate |> transform`
- Pattern matching in function heads
- `with` blocks for chaining operations that can fail

### From Swift
- Trailing closures
- `T?` optional type syntax
- Guard statements: `guard let value = optional else { return }`
- `if let` for optional unwrapping
- Protocol-oriented design

### From Go
- Fast compilation philosophy
- `defer` for cleanup
- Simple cross-compilation

### From Zig
- `const fn` (compile-time execution)
- Explicit allocators as an option

### From Kotlin
- Null safety via `T?` syntax — clean, intuitive optional types
- Scope functions (let, also, apply style)
- Sealed types
- Data classes / records

### From Ruby
- Everything is an expression
- Blocks
- DSL-friendly syntax

### From F#
- Computation expressions
- Units of measure
- Railway-oriented error handling

### From Clojure
- Immutable-by-default data

## What We Explicitly Avoid
- JavaScript's type coercion, `typeof null === "object"`, automatic semicolon insertion
- Rust's lifetime annotation complexity, `Pin<T>`, the string type zoo (`String`, `&str`, `OsString`, `CString`...)
- Python's GIL, indentation-as-syntax, packaging chaos
- Go's `if err != nil` verbosity, lack of sum types (Turbo uses `T ! E` and `?`)
- C++'s undefined behavior, header files, preprocessor macros
- Java's checked exceptions, type erasure, extreme verbosity
- Scala's sbt complexity, ecosystem fragmentation
- Haskell's lazy-by-default causing space leaks

## Complete Syntax Reference

### Variables and Constants `[Implemented]`

> **Coming from JavaScript?** Here's how variable declarations map:
> - JS `const x = 5` &rarr; Turbo `let x = 5` (immutable binding -- the default, and the right default)
> - JS `let x = 5` &rarr; Turbo `let mut x = 5` (mutable binding -- opt-in, explicit)
> - JS `var` &rarr; Does not exist in Turbo. No hoisting, no function-scoping surprises. Ever.
>
> Turbo flips the script: immutability is the default, mutability is opt-in. This is what `const` should have been in JavaScript.

```
// Immutable by default (like JS const, but better -- it's the default)
let name = "world"
let age: u32 = 25

// Mutable with `mut` (like JS let -- you explicitly ask for mutability)
let mut counter = 0
counter += 1

// Constants (compile-time known -- truly constant, evaluated at compile time)
const MAX_SIZE: usize = 1024
const PI: f64 = 3.14159265358979
```

### Functions `[Implemented]`
```
// Basic function
fn add(a: i32, b: i32) -> i32 {
  a + b  // last expression is return value
}

// Generic function
fn first<T>(items: [T]) -> T? {
  match items {
    [head, ..] => head    // auto-wrapped into T?
    [] => none
  }
}

// Default parameters
fn greet(name: str, greeting: str = "Hello") -> str {
  "{greeting}, {name}!"
}

// Named arguments at call site
greet(name: "Alice", greeting: "Hi")

// Closures / lambdas (arrow syntax is preferred; pipe syntax also works)
let double = (x) => x * 2
let add = (a, b) => a + b
let complex = (x: i32) => {
  let y = x * 2
  y + 1
}

// Async functions
async fn fetch(url: str) -> Response ! Error {
  let resp = await http.get(url)?
  resp
}
```

### Arrow Functions `[Implemented]`
Arrow functions work just like JavaScript — concise syntax for lambdas and callbacks.

```
// Short lambdas (single expression, implicit return)
let double = (x) => x * 2
let add = (a, b) => a + b

// With type annotations when needed
let parse = (s: str) => s.parse<i32>()

// Multi-line with braces (last expression is return value)
let process = (data) => {
  let cleaned = data.filter((d) => d.valid)
  cleaned.map((d) => d.value)
}

// Arrow functions are great in higher-order functions — just like JS
let names = users
  .filter((u) => u.active)
  .map((u) => u.name)
  .sort_by((a, b) => a.cmp(b))

// Pipe-style Rust closures also work (|x| x + 1) but arrow syntax is preferred
// Arrow functions are the primary/canonical form; pipes are accepted shorthand
let arrow = items.map((x) => x * 2)  // preferred
let short = items.map(|x| x * 2)     // also works (Rust-style shorthand)
```

### Destructuring `[Implemented]`
Destructuring works just like JavaScript/TypeScript — pull values out of objects and arrays with clean syntax.

```
// Object destructuring
let { name, age, ...rest } = user

// Array destructuring
let [first, second, ...tail] = items

// In function parameters — no need to unpack inside the body
fn greet({ name, title }: User) -> str {
  "{title} {name}"
}

// Nested destructuring
let { address: { city, zip } } = user

// With defaults (like JS default values)
let { name, role = "user" } = config

// Destructuring in for loops
for { name, score } in students {
  print("{name}: {score}")
}

// Combine with pattern matching
match response {
  ok({ status: 200, body }) => process(body)
  ok({ status: 404, .. }) => print("Not found")
  err(e) => print("Error: {e}")
}
```

### Optional Chaining & Null Coalescing `[Implemented]`
Works just like JavaScript's `?.` and `??` operators, but integrated with Turbo's `T?` type system.

```
// Optional chaining (works with T?)
let city = user?.address?.city  // str?

// Null coalescing (provide a default for T?)
let name = user?.name ?? "Anonymous"

// Optional method calls
let len = items?.len() ?? 0

// Chain as deep as you need
let zip = company?.ceo?.address?.zip_code ?? "00000"

// Works with method calls too
let upper_name = user?.name?.to_upper() ?? "UNKNOWN"

// Combine with T ! E and ? operator
let city = await fetch_user(id)?.address?.city ?? "Unknown"
```

### Types `[Implemented]`
```
// Structs
@derive(Debug, Eq, Serialize)
struct User {
  name: str
  email: str
  age: u32
}

// Struct instantiation
let user = User { name: "Alice", email: "alice@example.com", age: 30 }

// Algebraic data types (enums with data)
type Shape {
  Circle(radius: f64)
  Rectangle(width: f64, height: f64)
  Triangle(a: f64, b: f64, c: f64)
}

// Type aliases
type UserId = u64

// Tuple types
type Point = (f64, f64)

// Traits (interfaces)
trait Drawable {
  fn draw(self, canvas: &Canvas)
  fn bounds(self) -> Rect

  // Default implementation
  fn is_visible(self) -> bool { true }
}

// Implement traits
impl Drawable for Circle {
  fn draw(self, canvas: &Canvas) {
    canvas.circle(self.center, self.radius)
  }
  fn bounds(self) -> Rect {
    Rect.from_center(self.center, self.radius * 2, self.radius * 2)
  }
}
```

### Pattern Matching `[Implemented]`
```
// Match expressions (exhaustive)
fn area(shape: Shape) -> f64 {
  match shape {
    Circle(r) => PI * r * r
    Rectangle(w, h) => w * h
    Triangle(a, b, c) => {
      let s = (a + b + c) / 2.0
      sqrt(s * (s - a) * (s - b) * (s - c))
    }
  }
}

// Match with guards
fn classify(n: i32) -> str {
  match n {
    0 => "zero"
    n if n > 0 => "positive"
    _ => "negative"
  }
}

// Destructuring in let
let User { name, age, .. } = user
let [first, second, ..rest] = items
let (x, y) = point

// Pattern matching in function heads
fn factorial(0) -> u64 { 1 }
fn factorial(n: u64) -> u64 { n * factorial(n - 1) }
```

### Error Handling `[Implemented]`
```
// Result type — T ! E means "returns T or fails with E"
fn parse_int(s: str) -> i32 ! ParseError {
  // ...
}

// ? operator propagates errors
fn process(input: str) -> Output ! Error {
  let n = parse_int(input)?
  let data = fetch_data(n)?
  transform(data)  // auto-wrapped into ok
}

// match on results
match parse_int("42") {
  ok(n) => print("Got {n}")
  err(e) => print("Error: {e}")
}

// if let — for simple optional/result unwrapping [Implemented]
if let some(user) = find_user(42) {
  print("Found {user.name}")
}

// guard let — unwrap or early return (Swift-inspired) [Planned]
guard let config = load_config() else {
  return err("No config found")
}
// config is now unwrapped and available here

// with blocks (Elixir-inspired) [Planned]
fn complex_operation() -> Data ! Error {
  with {
    let config = load_config()?
    let conn = connect(config.db_url)?
    let data = query(conn, "SELECT *")?
  } yield {
    transform(data)
  }
}
```

### Pipe Operator `[Implemented]`
```
let result = raw_data
  |> parse
  |> validate?     // ? works inside pipes
  |> transform
  |> serialize

// Pipe with arguments (arrow syntax preferred in callbacks)
let filtered = users
  |> filter((u) => u.age > 18)
  |> sort_by((u) => u.name)
  |> take(10)
```

### Control Flow `[Implemented]`
```
// If/else (expression)
let status = if age >= 18 { "adult" } else { "minor" }

// For loops
for item in collection {
  process(item)
}

// For with index
for (i, item) in collection.enumerate() {
  print("{i}: {item}")
}

// While
while condition {
  // ...
}

// Loop (infinite, break to exit)
let result = loop {
  let attempt = try_something()
  if attempt.is_ok() { break attempt }
}

// Comprehensions [Planned]
let squares = [x * x for x in 1..=10]
let evens = [x for x in items if x % 2 == 0]
let pairs = [(x, y) for x in xs for y in ys]
```

### Collections `[Implemented]`
```
// Arrays — just like JavaScript
let nums = [1, 2, 3, 4, 5]

// Dynamic arrays [Implemented]
let mut items = [1, 2, 3]
items.push(4)              // push() builtin [Implemented]

// Maps — JS-like object literal syntax [Implemented]
let mut scores = {
  "Alice": 100,
  "Bob": 85,
}

// Sets — like maps but just values [Planned]
let uniq = {1, 2, 3, 4}

// Typed collections using type sugar
let names: [str] = []              // array of strings
let lookup: {str: i32} = {}        // map from str to i32
let ids: {u64} = {}                // set of u64

// Ranges
let r = 1..10      // exclusive end
let r = 1..=10     // inclusive end
```

### Literal Disambiguation `[Implemented]`

Because `{}` is used for blocks, maps, and sets, the parser uses the following rules to disambiguate:

| Syntax | Meaning | Example |
|--------|---------|---------|
| `{}` | Empty block expression (returns unit) | `if cond {}` |
| `{:}` | Empty map | `let scores: {str: i32} = {:}` |
| `{,}` or `Set.new()` | Empty set | `let ids: {u64} = {,}` |
| `{ "key": value }` | Map literal (at least one colon-separated pair) | `{ "Alice": 100, "Bob": 85 }` |
| `{ expr }` | Block expression (no commas between items) | `{ let x = 1; x + 2 }` |
| `{1, 2, 3}` | Set literal (comma-separated values, no colons) | `{ "a", "b", "c" }` |

**Parser rule:** If the first non-whitespace token inside `{` is followed by `:` and a value, it is a map. If items are comma-separated without colons, it is a set. Otherwise it is a block expression. The empty-literal forms `{:}` and `{,}` resolve the ambiguity for zero-element maps and sets.

### Async / Streaming `[Implemented]`
```
// Async function
async fn fetch_user(id: u64) -> User ! Error {
  let resp = await http.get("/users/{id}")?
  resp.json<User>()?
}

// Streaming [Planned]
async fn stream_tokens(prompt: str) -> Stream<Token> {
  let response = await llm.stream(prompt)
  for await token in response {
    yield token
  }
}

// Concurrent execution [Planned]
async fn fetch_all(ids: [u64]) -> [User] ! Error {
  scope (s) => {
    let tasks = ids.map((id) => s.spawn(async { await fetch_user(id)? }))
    await tasks.collect()?
  }
}
```

### Agents and Tools `[Sidecar — not in core]`

Earlier drafts of this spec proposed `agent` / `tool fn` as core-language keywords. That direction has been retired: these features will ship as a `turbo-agent` library on top of the stable 1.0 core, not as compiler keywords. Nothing in the language grammar defined in this document includes `agent` or `tool fn`. See **COMPATIBILITY.md** and the "A small, honest core" pillar in **VISION.md**.

### Compile-Time Execution `[Planned]`
```
const fn generate_lookup() -> {str: u32} {
  let mut map = {}
  for (i, name) in NAMES.iter().enumerate() {
    map[name] = i as u32
  }
  map
}

const LOOKUP = generate_lookup()  // evaluated at compile time because it's a const fn assigned to a const
```

### Modules and Imports `[Implemented]`
```
// Import specific items
import { HashMap, HashSet } from "std/collections"
import { Read, Write } from "std/io"

// Import module
import http from "http"

// Aliased import
import { HashMap as Map } from "std/collections"

// Public visibility (private by default)
pub fn public_function() { }
pub struct PublicStruct { }
fn private_function() { }
```

### Attributes / Decorators `[Implemented]`
```
@test
fn test_addition() {
  assert_eq(add(2, 3), 5)
}

@bench
fn bench_sort() {
  // ...
}

@deprecated("Use new_function instead")
fn old_function() { }

@inline
fn hot_path() { }

@derive(Debug, Eq, Hash, Serialize, Deserialize)
struct Config { }
```

## Metaprogramming `[Implemented]`

Turbo takes a deliberately minimal approach to metaprogramming. Instead of a macro system, Turbo relies on three mechanisms that cover 95% of use cases with zero magic:

**1. `@derive(...)` — Automatic trait implementations.** The compiler generates boilerplate implementations for common traits. This replaces Rust's `derive` procedural macros with a built-in, well-defined expansion.

```
@derive(Debug, Eq, Hash, Clone, Serialize, Schema)
struct User { name: str, age: u32 }
// The compiler generates: Debug.fmt(), Eq.eq(), Hash.hash(), Clone.clone(),
// Serialize.serialize(), and a JSON Schema — all from the struct definition.
```

**2. `const fn` — Compile-time computation.** Any function marked `const fn` runs at compile time when called in a `const` context. This replaces traditional macro-based code generation with regular, debuggable, type-checked functions.

```
const fn generate_routes(prefix: str, names: [str]) -> [Route] {
  names.map((name) => Route { path: "{prefix}/{name}", handler: name })
}
const ROUTES = generate_routes("/api", ["users", "posts", "comments"])
```

**3. Generics + traits.** Generic functions with trait bounds cover the abstraction patterns that macros handle in other languages. Conditional compilation uses `@cfg` attributes, not preprocessor directives.

**What Turbo does NOT have:**
- No Rust-style procedural macros (`macro_rules!`, `proc_macro`)
- No C/C++ preprocessor macros (`#define`, `#ifdef`)
- No Lisp-style code-as-data macros
- No template metaprogramming (C++ TMP)

**Philosophy:** Prefer `const fn` + generics + `@derive` over macro systems. Macros are powerful but they produce code that is hard to read, hard to debug, and opaque to tooling. Turbo's approach keeps all code visible, type-checked, and IDE-friendly.

**Future direction:** If a general-purpose macro system becomes necessary, it will be hygienic and AST-based (like Scala 3 inline/macro or Swift macros) — never textual substitution. This is a post-1.0 consideration.

## Type Sugar Reference `[Implemented]`

Turbo provides elegant type sugar so you never need to write verbose generic types for common patterns. Under the hood, these are all full discriminated unions — the sugar is purely a surface convenience.

| Sugar | Expands To | Meaning |
|-------|-----------|---------|
| `T?` | Discriminated union: `some(T) \| none` | Optional value — may or may not exist |
| `T ! E` | Discriminated union: `ok(T) \| err(E)` | Result — succeeds with T or fails with E |
| `[T]` | `Array<T>` | Dynamic array of T |
| `{K: V}` | `Map<K, V>` | Map from K to V |
| `{T}` | `Set<T>` | Set of T |

### Optional (`T?`) `[Implemented]`
```
// Type annotation
let name: str? = "Alice"         // has a value
let missing: str? = none         // no value

// Function returns
fn find_user(id: u64) -> User? {
  let user = db.get(id)?
  user                            // auto-wrapped into User?
}

// Pattern matching (lowercase)
match find_user(42) {
  some(user) => print("Found {user.name}")
  none => print("Not found")
}

// Optional chaining + coalescing
let city = user?.address?.city ?? "Unknown"

// if let — unwrap for a block [Implemented]
if let some(user) = find_user(42) {
  print("Hello, {user.name}")
}

// guard let — unwrap or bail [Planned]
guard let user = find_user(42) else {
  return none
}
print(user.name)  // user is unwrapped here
```

### Result (`T ! E`) `[Implemented]`
```
// Type annotation
fn parse(input: str) -> Config ! ParseError {
  // ...
}

// Generic error
fn risky() -> str ! Error {
  // ...
}

// ? operator propagates errors
fn process(input: str) -> Output ! Error {
  let config = parse(input)?
  let data = fetch(config)?
  transform(data)                 // auto-wrapped into ok
}

// Pattern matching (lowercase)
match parse("config.toml") {
  ok(config) => use(config)
  err(e) => print("Failed: {e}")
}
```

### Collections `[Implemented]`
```
let names: [str] = ["Alice", "Bob"]       // array of strings
let scores: {str: i32} = {"Alice": 100}   // map str -> i32
let ids: {u64} = {1, 2, 3}               // set of u64
```

## Syntax Summary Table

| Feature | Syntax | Inspired By | Status |
|---------|--------|-------------|--------|
| Immutable binding | `let x = 5` | Rust | Implemented |
| Mutable binding | `let mut x = 5` | Rust | Implemented |
| String interpolation | `"Hello, {name}"` | Python f-strings | Implemented |
| Optional type | `T?` | Swift/Kotlin | Implemented |
| Result type | `T ! E` | Novel (inspired by Rust) | Implemented |
| No value | `none` | Swift (`nil`), Turbo-style | Implemented |
| Pipe | `x \|> f \|> g` | Elixir | Implemented |
| Error propagation | `expr?` | Rust | Implemented |
| Pattern match | `match x { ... }` | Rust | Implemented |
| if let | `if let some(v) = expr { }` | Swift | Implemented |
| guard let | `guard let x = expr else { }` | Swift | Planned |
| Arrow function (preferred) | `(x) => x + 1` | JavaScript | Implemented |
| Lambda (pipe, shorthand) | `\|x\| x + 1` | Rust | Implemented |
| Destructuring | `let { a, b } = obj` | JavaScript | Implemented |
| Optional chaining | `x?.y?.z` | TypeScript | Implemented |
| Null coalescing | `x ?? default` | JavaScript | Implemented |
| Comprehension | `[x for x in xs]` | Python | Planned |
| Array type | `[T]` | Swift | Implemented |
| Map type | `{K: V}` | Novel | Implemented |
| Set type | `{T}` | Novel | Planned |
| Async | `async fn / await` | JS/Rust | Implemented |
| Compile-time | `const fn` | Zig (adapted) | Planned |
| Defer | `defer { cleanup() }` | Go | Implemented |

## Real-World Patterns

Complete mini-programs showing how Turbo feels in practice. These use the canonical syntax: `@` decorators, `T?`, `T ! E`, `Shared<T>`, `WeakRef<T>`, `const fn`, and arrow functions.

### Web API Server

A complete REST API in under 20 lines. No framework to install, no boilerplate -- just import and go.

```
import { Server, Router } from "turbo/http"
import { log } from "turbo/log"

let router = Router.new()

router.get("/users/:id", async (req) => {
  let user = await db.find_user(req.params.id)?
  Response.json(user)
})

router.post("/users", async (req) => {
  let input = req.json<CreateUser>()?
  let user = await db.create_user(input)?
  Response.json(user, status: 201)
})

router.delete("/users/:id", async (req) => {
  await db.delete_user(req.params.id)?
  Response.json({ deleted: true })
})

let server = Server.new(router, middleware: [cors(), rate_limit(100)])
log.info("Starting server", { port: 3000 })
await server.listen(3000)
```

### CLI Tool

A command-line word counter with proper error handling and multi-file support.

```
import { args, exit } from "turbo/process"
import { fs } from "turbo/io"

fn main() {
  let files = args().skip(1)

  if files.is_empty() {
    print("Usage: wordcount <file1> [file2] ...")
    exit(1)
  }

  let mut total = 0

  for file in files {
    let content = fs.read(file) ?? {
      print("Error: Could not read {file}")
      continue
    }
    let words = content.split_whitespace().len()
    total += words
    print("{file}: {words} words")
  }

  if files.len() > 1 {
    print("Total: {total} words")
  }
}
```

### Data Processing Pipeline

The pipe operator makes data transformations read top-to-bottom, like a Unix pipeline.

```
import { json } from "turbo/json"
import { fs } from "turbo/io"

@derive(Schema, Debug)
struct SalesRecord {
  region: str
  revenue: f64
  active: bool
}

fn main() {
  let data = await fs.read("sales.json")?
    |> json.parse<[SalesRecord]>()?

  let top_regions = data
    |> filter((d) => d.active)
    |> map((d) => { ...d, score: calculate_score(d) })
    |> sort_by((d) => d.score, order: .desc)
    |> take(10)
    |> collect()

  for { region, score, .. } in top_regions {
    print("{region}: {score:.2}")
  }
}
```

### Concurrent Data Fetching

Fetch multiple resources in parallel using structured concurrency.

```
import { http } from "turbo/http"

@derive(Schema)
struct Dashboard {
  user: UserProfile
  posts: [Post]
  notifications: [Notification]
}

async fn load_dashboard(user_id: u64) -> Dashboard ! Error {
  // All three fetches run concurrently — structured concurrency
  let (user, posts, notifications) = await (
    http.get<UserProfile>("/api/users/{user_id}"),
    http.get<[Post]>("/api/users/{user_id}/posts"),
    http.get<[Notification]>("/api/users/{user_id}/notifications"),
  )?

  Dashboard { user, posts, notifications }
}
```

### Type-Safe Configuration

Compile-time validated config with defaults, environment overrides, and exhaustive matching.

```
import { fs } from "turbo/io"
import { json } from "turbo/json"

@derive(Schema)
struct AppConfig {
  host: str = "0.0.0.0"
  port: u16 = 3000
  database: DbConfig
  log_level: LogLevel = .info
}

@derive(Schema)
struct DbConfig {
  url: str
  pool_size: u32 = 10
  timeout: Duration = 30.seconds()
}

type LogLevel { trace, debug, info, warn, error }

fn load_config(path: str) -> AppConfig ! IoError | ParseError {
  let raw = await fs.read(path)?
  let mut config = json.parse<AppConfig>(raw)?

  // Environment overrides take precedence
  if let port = env("PORT") {
    config.port = port.parse<u16>()?
  }
  if let db_url = env("DATABASE_URL") {
    config.database.url = db_url
  }

  config
}
```

### Shared State with `Shared<T>`

Thread-safe shared state using `Shared<T>` instead of raw reference counting.

```
import { Shared } from "turbo/sync"

struct AppState {
  cache: {str: str}
  request_count: u64
}

fn main() {
  let state = Shared.new(AppState {
    cache: {},
    request_count: 0,
  })

  let router = Router.new()

  router.get("/stats", async (req) => {
    let s = state.read()
    Response.json({ requests: s.request_count, cached: s.cache.len() })
  })

  router.get("/data/:key", async (req) => {
    let key = req.params.key

    // Check cache (read lock)
    if let cached = state.read().cache.get(key) {
      return Response.json({ data: cached, cached: true })
    }

    // Fetch and cache (write lock)
    let data = await fetch_from_db(key)?
    state.write().cache[key] = data
    state.write().request_count += 1

    Response.json({ data, cached: false })
  })

  await Server.new(router).listen(3000)
}
```

## JavaScript to Turbo Cheat Sheet

If you know JavaScript or TypeScript, you already know most of Turbo. This table maps every common JS pattern to its Turbo equivalent.

| JavaScript | Turbo | Notes |
|-----------|-------|-------|
| `const x = 5` | `let x = 5` | Immutable by default |
| `let x = 5` | `let mut x = 5` | Mutable binding -- opt-in |
| `var x = 5` | *(does not exist)* | No hoisting, no surprises |
| `x === y` | `x == y` | No loose equality in Turbo -- `==` is always strict |
| `x !== y` | `x != y` | Same -- no `===`/`!==` distinction needed |
| `x?.y?.z` | `x?.y?.z` | Same syntax! |
| `x ?? fallback` | `x ?? fallback` | Same syntax! |
| `async function f() {}` | `async fn f() {}` | Same concept, shorter keyword |
| `await promise` | `await future` | Same syntax! (`Future` = `Promise`) |
| `arr.map(x => x * 2)` | `arr.map((x) => x * 2)` | Same! (parens always required in Turbo) |
| `const { a, b } = obj` | `let { a, b } = obj` | Same destructuring |
| `const [x, ...rest] = arr` | `let [x, ...rest] = arr` | Same array destructuring |
| `interface Foo {}` | `trait Foo {}` | Traits have default implementations |
| `class Foo {}` | `struct Foo {}` | Value types by default |
| `Promise.all([...])` | `all([...])` | Simpler -- no `Promise.` prefix |
| `Promise.race([...])` | `race([...])` | Same |
| `try { } catch(e) { }` | `match result { ok(v) => ..., err(e) => ... }` | Pattern matching -- errors are values |
| `throw new Error()` | `return err(...)` | Errors are values, not exceptions |
| `null` / `undefined` | `none` | One concept, not two |
| `console.log()` | `print()` | Simpler |
| `JSON.parse(s)` | `json.parse<T>(s)` | Type-safe -- you specify the target type |
| `fetch(url)` | `http.get(url)` | Same idea, explicit HTTP method |
| `new Map()` | `{:}` | Map literal syntax |
| `new Set()` | `{,}` or `Set.new()` | Set literal syntax |
| `` `Hello ${name}` `` | `"Hello {name}"` | No backticks, no `$` needed |
| `for (const x of arr)` | `for x in arr` | Cleaner -- no parens, no `const`/`of` |
| `for await (const x of stream)` | `for await x in stream` | Same concept, cleaner syntax |
| `export function f() {}` | `pub fn f() {}` | `pub` = `export` |
| `import { X } from "mod"` | `import { X } from "mod"` | Identical! |
| `x instanceof Foo` | `x is Foo` | Reads like English |
| `typeof x === "string"` | *(not needed -- static types)* | The compiler knows every type at compile time |
| `Array.isArray(x)` | *(not needed -- static types)* | Types are known at compile time |
| `switch (x) { case ... }` | `match x { ... }` | Exhaustive, expression-based, pattern matching |
| `// @ts-ignore` | *(does not exist)* | No escape hatch needed -- the type system works |

### Quick Equivalence Summary

```
// JavaScript                        // Turbo
// --------------------------------  // --------------------------------
const name = "Alice"                 let name = "Alice"
let count = 0                        let mut count = 0
count++                              count += 1

const greet = (name) => {            let greet = (name: str) => {
  return `Hello, ${name}!`             "Hello, {name}!"
}                                    }

async function fetchUser(id) {       async fn fetch_user(id: u64) -> User ! Error {
  const res = await fetch(url)         let res = await http.get("/users/{id}")?
  const data = await res.json()        res.json<User>()?
  return data                        }
}

try {                                match fetch_user(42) {
  const user = await fetchUser(42)     ok(user) => print("Found {user.name}")
  console.log(user.name)               err(e) => print("Error: {e}")
} catch (e) {                        }
  console.error(e)
}

const nums = [1, 2, 3]              let nums = [1, 2, 3]
const doubled = nums.map(x => x*2)  let doubled = nums.map((x) => x * 2)
const evens = nums.filter(x=>x%2==0) let evens = nums.filter((x) => x % 2 == 0)
```
