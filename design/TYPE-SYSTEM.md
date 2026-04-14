# Type System

## Core Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Null | **No null. `T?` optionals only.** | Swift, Kotlin, Dart all prove `T?` is the right syntax. Java/JS null is the billion-dollar mistake. |
| Typing | **Sound, static, structural for interfaces + nominal for data** | TypeScript's structural typing is practical; Dart's sound null safety is the gold standard; avoid Java's type erasure |
| Generics | **Monomorphized** with opt-in runtime type metadata via `@derive(TypeInfo)` | Zero-cost by default; opt-in reflection when needed |
| Sum types | **First-class algebraic data types** with exhaustive matching | Rust enums, F# discriminated unions — universally loved |
| Error handling | **`T ! E` with `?` propagation** | Rust's model with cleaner syntax — no exceptions, no checked exceptions, typed errors |
| Inference | **Hindley-Milner-inspired local inference** | F#/Rust-style — minimal annotations, maximum safety |
| Effects | **Tracked async/IO in the type system** (lightweight, not Haskell-heavy) | Know what a function does from its signature |
| Immutability | **Immutable by default** | F#, Clojure, Rust all prove this is correct |

## Primitive Types

### Numeric Types
- `i8`, `i16`, `i32`, `i64`, `i128`, `isize` — signed integers
- `u8`, `u16`, `u32`, `u64`, `u128`, `usize` — unsigned integers
- `f32`, `f64` — IEEE 754 floating point
- `bool` — boolean
- No implicit numeric conversions (explicit `.into()` or `as` casting)

### String Types
- `str` — the primary string type, UTF-8 encoded, immutable, heap-allocated
- `&str` — borrowed string slice (only at FFI boundaries and advanced use)
- No `String` vs `&str` confusion like Rust — `str` just works for 99% of use cases
- The compiler optimizes small strings (SSO) and static strings automatically

### Other Primitives
- `char` — Unicode scalar value
- `()` — unit type (like void)
- `never` — bottom type (function never returns)

## String Semantics

Turbo has **one string type**: `str`. No `String` vs `&str` vs `OsString` confusion. Just `str`. It works like strings in JavaScript or Python — you use it, it does the right thing, and the compiler handles the rest.

### Core Properties

- **`str` is the ONE string type.** There is no second string type to learn. (`&str` exists only at FFI boundaries and in low-level library internals — you will likely never write it.)
- **UTF-8 encoded.** All strings are valid UTF-8 at all times. The compiler enforces this.
- **Immutable by default.** Like all values in Turbo, strings are immutable unless declared `let mut`. Concatenation and modification produce new strings.
- **Passed by reference automatically.** Like JavaScript strings, `str` has value semantics at the surface but the compiler passes it by reference under the hood. No copying on function calls. No `&` needed.
- **Compiler-optimized.** Small strings use SSO (small string optimization). Static strings are stored in the binary. The compiler picks the best representation — you never think about it.

### Indexing and Slicing

```
let s = "Hello, world!"

// Indexing returns a character (Unicode scalar value), NOT a byte
let first = s[0]          // 'H' — type: char
let emoji = "Hello 🌍"
let earth = emoji[6]      // '🌍' — works correctly, not a partial byte

// Slicing returns a substring
let hello = s[0..5]       // "Hello" — type: str
let world = s[7..12]      // "world"

// Length
let chars = s.len()       // 13 — character count (what you almost always want)
let bytes = s.byte_len()  // 13 — byte count (same for ASCII, differs for multi-byte)

let jp = "こんにちは"
jp.len()                  // 5 — five characters
jp.byte_len()             // 15 — fifteen UTF-8 bytes
```

### String Interpolation

String interpolation uses `{}` inside double-quoted strings — no prefix needed (unlike Python's `f""`). Any expression works inside the braces.

```
let name = "Alice"
let age = 30

// Simple variable interpolation
let greeting = "Hello, {name}!"          // "Hello, Alice!"

// Expressions inside braces
let msg = "In 10 years: {age + 10}"      // "In 10 years: 40"
let info = "Name length: {name.len()}"   // "Name length: 5"

// Nested expressions
let status = "Status: {if age >= 18 { "adult" } else { "minor" }}"

// Format specifiers (optional)
let pi = 3.14159
let formatted = "Pi is {pi:.2}"          // "Pi is 3.14"
let hex = "Color: {255:02x}"             // "Color: ff"
```

### Multi-Line Strings

Triple-quoted strings preserve newlines and indentation. Leading whitespace is stripped based on the closing `"""` position.

```
let html = """
  <div>
    <h1>Hello, {name}</h1>
    <p>Welcome to Turbo.</p>
  </div>
  """
// Leading indentation (matched to closing """) is stripped.
// Interpolation works inside triple-quoted strings.
```

### Raw Strings

Raw strings disable escape sequence processing. Useful for regex patterns, file paths, and any string where backslashes should be literal.

```
let pattern = r"^\d{3}-\d{4}$"           // No escapes — \ is literal
let path = r"C:\Users\Alice\Documents"   // Backslashes preserved as-is

// Raw strings can also be multi-line
let regex = r"""
  (\d{4})   # year
  -(\d{2})  # month
  -(\d{2})  # day
  """
```

### Common Operations

```
let s = "Hello, world!"

// Checking
s.contains("world")          // true
s.starts_with("Hello")       // true
s.ends_with("!")             // true
s.is_empty()                 // false

// Transforming (returns new str — originals are immutable)
s.to_upper()                 // "HELLO, WORLD!"
s.to_lower()                 // "hello, world!"
s.trim()                     // strips whitespace from both ends
s.replace("world", "Turbo")  // "Hello, Turbo!"

// Splitting and joining
s.split(", ")                // ["Hello", "world!"]
["a", "b", "c"].join(", ")  // "a, b, c"

// Conversion
"42".parse<i32>()?           // 42 — returns i32 ! ParseError
42.to_string()               // "42"
```

## Composite Types

### Structs (Product Types)
```
struct Point {
  x: f64
  y: f64
}

// With methods
impl Point {
  fn distance(self, other: Point) -> f64 {
    sqrt((self.x - other.x).pow(2) + (self.y - other.y).pow(2))
  }
}

// Record/data class shorthand
@derive(Debug, Eq, Hash, Clone)
struct User {
  name: str
  email: str
  age: u32
}
```

### Algebraic Data Types (Sum Types)
```
// T? and T ! E are built-in sugar for these discriminated unions:
//   T?    =>  type Optional<T> { some(T), none }
//   T ! E =>  type Result<T, E> { ok(T), err(E) }
// You never need to define them yourself — they're language primitives.

type Shape {
  Circle(radius: f64)
  Rectangle(width: f64, height: f64)
  Triangle(a: f64, b: f64, c: f64)
}

type Json {
  Null
  Bool(bool)
  Number(f64)
  Str(str)
  Array([Json])
  Object({str: Json})
}
```

### Tuples
```
let point: (f64, f64) = (1.0, 2.0)
let (x, y) = point  // destructure
```

### Arrays and Collections
```
let fixed: [i32; 5] = [1, 2, 3, 4, 5]   // fixed-size array
let dynamic: [i32] = [1, 2, 3]           // dynamic array
let map: {str: i32} = {}                 // map from str to i32
let set: {i32} = {1, 2, 3}              // set of i32
```

## Generics

### Basic Generics
```
fn identity<T>(value: T) -> T { value }

struct Stack<T> {
  items: [T]
}

impl<T> Stack<T> {
  fn push(mut self, item: T) { self.items.push(item) }
  fn pop(mut self) -> T? { self.items.pop() }
}
```

### Constrained Generics (Trait Bounds)
```
fn largest<T: Ord>(items: [T]) -> T? {
  items.iter().max()
}

fn serialize<T: Serialize + Debug>(value: T) -> str {
  // T must implement both Serialize and Debug
}

// Where clause for complex bounds
fn process<T, E>(value: T) -> T ! E
where T: Clone + Send
      E: Error + From<IoError>
{
  // ...
}
```

### Runtime Type Metadata (Opt-In)
```
// Generics are monomorphized by default (zero-cost, no runtime type info).
// Opt in to runtime type metadata with @derive(TypeInfo):

@derive(TypeInfo)
struct User { name: str, age: u32 }

// With TypeInfo derived, runtime type checks and reflection work:
fn type_name<T: TypeInfo>() -> str {
  T.name  // Works when T has TypeInfo derived.
}

fn is_type<T: TypeInfo>(value: any) -> bool {
  value is T  // Runtime type check works with TypeInfo types
}
```

## Traits (Interfaces)

> **Coming from TypeScript?** Traits are like TypeScript `interface`s, but better:
> - Like interfaces, traits define a contract that types must satisfy
> - Unlike interfaces, traits can have **default implementations** -- methods with actual code, not just signatures
> - Like interfaces, traits support structural typing -- if a type has the right methods, it satisfies the trait automatically
> - Unlike interfaces, you can add traits to types you didn't define (extension methods via `impl`)
>
> Think of traits as "interfaces with superpowers."

### TypeScript Interface vs Turbo Trait

```
// TypeScript                          // Turbo
// --------------------------------    // --------------------------------
// interface Printable {               trait Printable {
//   toString(): string                  fn to_string(self) -> str
// }                                   }
//
// interface Iterator<T> {             trait Iterator<T> {
//   next(): T | undefined               fn next(mut self) -> T?
//   // No default implementations!
// }                                     // Default implementations included!
//                                       fn map<U>(self, f: fn(T) -> U) -> MapIterator<T, U> { ... }
//                                       fn filter(self, pred: fn(T) -> bool) -> FilterIterator<T> { ... }
//                                     }
//
// class Dog implements Printable {    impl Printable for Dog {
//   toString() { return this.name }     fn to_string(self) -> str { self.name }
// }                                   }
```

### Structural Typing for Traits
```
trait Printable {
  fn to_string(self) -> str
}

// Any type with a to_string method automatically satisfies Printable
// No explicit `impl Printable for X` needed for structural matching
// This is like TypeScript's structural typing -- if the shape matches, it works
// But explicit impl is available for clarity and documentation
```

### Nominal Typing for Structs/Enums
```
struct Meters(f64)
struct Feet(f64)
// Meters and Feet are different types even though both wrap f64
// Prevents unit confusion (Mars Climate Orbiter bug!)
```

### Trait Features
```
trait Iterator<T> {
  fn next(mut self) -> T?

  // Default methods -- like having method implementations right in the interface
  // (TypeScript interfaces can't do this!)
  fn map<U>(self, f: fn(T) -> U) -> MapIterator<T, U> { ... }
  fn filter(self, pred: fn(T) -> bool) -> FilterIterator<T> { ... }
  fn collect<C: FromIterator<T>>(self) -> C { ... }
}

// Associated types (like TypeScript's generic interface constraints)
trait Container {
  type Item
  fn get(self, index: usize) -> Self.Item?
}

// Trait inheritance (like TypeScript's interface extends)
// interface ReadWrite extends Read, Write { }
trait ReadWrite: Read + Write {
  // Inherits all methods from Read and Write
}
```

## Type Inference

### Local Inference (Hindley-Milner inspired)
```
let x = 42          // inferred as i32
let name = "Alice"  // inferred as str
let items = [1, 2, 3]  // inferred as [i32; 3]

// Return type inference for private functions
fn double(x: i32) -> _ { x * 2 }  // returns i32

// Closure type inference (arrow syntax preferred; pipe syntax also works)
let f = (x) => x + 1  // inferred from usage context

// Generic type inference
let mut stack = Stack.new()  // Stack<???>
stack.push(42)               // Now inferred as Stack<i32>
```

### Where Annotations Are Required
- Public function signatures (parameters and return types)
- Struct field types
- Trait method signatures
- When the compiler can't infer (ambiguous expressions)

## Optionals and Results

### `T?` — The Null Replacement
The `T?` syntax is sugar for a discriminated union with two variants: `some(T)` and `none`. Under the hood, it is a zero-cost abstraction — the compiler represents it as an efficient tagged value. But on the surface, it is as simple as adding a `?` to any type.

```
// Creating
let x: i32? = 42             // auto-wrapped into some(42)
let y: i32? = none            // no value

// Accessing safely
match x {
  some(val) => print("Got {val}")
  none => print("Nothing")
}

// if let — simple unwrapping
if let val = x {
  print("Got {val}")
}

// guard let — unwrap or early return
guard let val = x else { return none }
print("Got {val}")

// Chaining (arrow syntax preferred)
let result = x
  .map((v) => v * 2)
  .filter((v) => v > 10)
  .unwrap_or(0)

// Optional chaining (like TypeScript ?.)
let city = user?.address?.city

// Null coalescing
let name = user?.name ?? "Anonymous"
```

### `T ! E` — Typed Errors
The `T ! E` syntax means "returns T or fails with error E." The `!` is a visual signal: this operation can fail. Under the hood, it's a discriminated union with `ok(T)` and `err(E)` variants.

```
// The ? operator
fn process() -> Data ! Error {
  let input = read_file("data.txt")?   // Returns err early if fails
  let parsed = parse(input)?
  let validated = validate(parsed)?
  transform(validated)                  // auto-wrapped into ok
}

// Pattern matching (lowercase)
match parse("data.txt") {
  ok(data) => use(data)
  err(e) => print("Failed: {e}")
}

// Error type composition
type AppError {
  Io(IoError)
  Parse(ParseError)
  Validation(ValidationError)
}

// Automatic From conversions
impl From<IoError> for AppError { ... }
// Now ? auto-converts IoError to AppError
```

### Sugar vs. Power

The `T?` and `T ! E` syntax is pure surface sugar. Advanced users can access the full discriminated union when needed:

```
// These are equivalent:
let x: i32? = 42
let x: Optional<i32> = some(42)

// These are equivalent:
fn parse(s: str) -> Config ! ParseError { ... }
fn parse(s: str) -> Result<Config, ParseError> { ... }

// The full union types are available for advanced use cases:
// type Optional<T> { some(T), none }
// type Result<T, E> { ok(T), err(E) }

// You can still use the full generic forms in trait bounds,
// type-level programming, or anywhere the sugar doesn't reach.
// But for 99% of code, T? and T ! E are all you need.
```

This is progressive disclosure in action: beginners see `str?` and intuit "a string that might not be there." Power users know the full union type is underneath and can reach for it when needed.

## Error Type Hierarchy

Errors in Turbo are just types that implement the `Error` trait. No checked exceptions. No `throws` declarations. No magic — just the type system doing its job.

### The Error Trait

```
// Base error trait — every error type implements this
trait Error {
  fn message(self) -> str
  fn source(self) -> Error?       // T? — optional chain to the underlying cause
  fn stack(self) -> [StackFrame]   // [T] — array of stack frames
}
```

### Standard Error Types

Turbo ships with a set of built-in error types for common failure modes. Each implements the `Error` trait:

```
// Built-in error types — always available, no imports needed
type IoError: Error {
  NotFound(path: str)
  PermissionDenied(path: str)
  BrokenPipe
  UnexpectedEof
}

type ParseError: Error {
  InvalidSyntax(line: u32, col: u32, message: str)
  UnexpectedToken(expected: str, got: str)
  UnexpectedEof
}

type NetworkError: Error {
  ConnectionRefused(host: str, port: u16)
  DnsResolution(host: str)
  TlsHandshake(reason: str)
}

type TimeoutError: Error {
  Elapsed(duration: Duration)
}

type ValidationError: Error {
  FieldInvalid(field: str, reason: str)
  MissingField(field: str)
  ConstraintViolation(message: str)
}
```

### Custom Errors

Custom errors are just types that implement `Error`. No inheritance ceremony, no exception class trees — define a type, implement the trait, done.

```
// Define your own — just implement Error
type AppError: Error {
  NotFound(resource: str)
  Unauthorized(reason: str)
  RateLimit(retry_after: Duration)
}

// The compiler auto-generates message() from variant names + fields,
// but you can override any trait method:
impl AppError {
  fn message(self) -> str {
    match self {
      NotFound(r) => "Resource not found: {r}"
      Unauthorized(r) => "Unauthorized: {r}"
      RateLimit(d) => "Rate limited, retry after {d}"
    }
  }
}
```

### Error Return Signatures

The `! E` syntax makes the error type visible in the signature without forcing callers to handle every variant. Three levels of specificity:

```
// Generic error — when you don't care about the specific type
fn risky() -> str ! Error { ... }

// Specific error — when you want callers to know exactly what can fail
fn parse(input: str) -> Config ! ParseError { ... }

// Multiple error types via union — when different failures are possible
fn load(path: str) -> Config ! IoError | ParseError { ... }
```

### Error Composition

When a function can fail with multiple error types, you have two choices: union types or a wrapper type.

```
// Option 1: Union types in the signature (simple, ad-hoc)
fn load_and_validate(path: str) -> Config ! IoError | ParseError | ValidationError {
  let raw = read_file(path)?            // IoError propagated via ?
  let config = parse(raw)?              // ParseError propagated via ?
  validate(config)?                     // ValidationError propagated via ?
  config
}

// Option 2: Wrapper type (better for libraries with many error sources)
type ConfigError: Error {
  Io(IoError)
  Parse(ParseError)
  Validation(ValidationError)
}

// Auto-conversion via From trait — ? converts automatically
impl From<IoError> for ConfigError { fn from(e: IoError) -> Self { .Io(e) } }
impl From<ParseError> for ConfigError { fn from(e: ParseError) -> Self { .Parse(e) } }
impl From<ValidationError> for ConfigError { fn from(e: ValidationError) -> Self { .Validation(e) } }

fn load_and_validate(path: str) -> Config ! ConfigError {
  let raw = read_file(path)?            // IoError auto-converts to ConfigError
  let config = parse(raw)?              // ParseError auto-converts
  validate(config)?                     // ValidationError auto-converts
  config
}
```

### Key Design Principles

- **Errors are values, not control flow.** No stack unwinding, no hidden `throw`. Errors are returned like any other value via `T ! E`.
- **Errors are just types.** Any type that implements the `Error` trait can be used as an error. No special syntax, no registering, no inheritance tree.
- **Progressive specificity.** Use `! Error` when prototyping, `! ParseError` for precision, `! IoError | ParseError` for unions. Refine as your code matures.
- **`?` does the heavy lifting.** The propagation operator converts between error types automatically via `From` implementations.
- **No checked exceptions.** Callers can always see `! E` in the signature, but they choose how to handle it — `?` to propagate, `match` to handle, or `.unwrap()` to crash.

## Effect System (Lightweight)

### Tracked Effects
```
// Functions declare their effects
fn pure_math(x: i32) -> i32 { x * 2 }           // No effects — pure
async fn fetch(url: str) -> Data ! Error           // async effect
fn read_file(path: str) -> io str ! Error          // IO effect

// The compiler tracks:
// - async: function may suspend
// - io: function performs I/O
// - unsafe: function uses unsafe operations
// - throws: function may return an error (via T ! E)

// Pure functions are guaranteed referentially transparent
// The compiler can optimize, cache, and parallelize them freely
```

## Special Types

### Never Type
```
fn panic(msg: str) -> never {
  // Never returns
}

fn infinite_loop() -> never {
  loop { }
}
```

### Any Type (Escape Hatch)
```
// For FFI and very dynamic scenarios only
let dynamic: any = get_from_c_library()
let typed = dynamic as? MyType  // Returns MyType?
```

### Phantom Types
```
struct Validated;
struct Unvalidated;

struct Email<State> {
  address: str
  _state: PhantomData<State>
}

fn validate(email: Email<Unvalidated>) -> Email<Validated> ! Error { ... }
fn send(email: Email<Validated>) { ... }  // Can only send validated emails!
```

## Units of Measure (F#-inspired)
```
unit Meters
unit Seconds
unit MetersPerSecond = Meters / Seconds

let distance: f64<Meters> = 100.0
let time: f64<Seconds> = 9.58
let speed: f64<MetersPerSecond> = distance / time  // Type-safe!
// let wrong = distance + time  // COMPILE ERROR: can't add meters to seconds
```

## Compile-Time Type Computation
```
const type JsonSchema<T> {
  // Generate a JSON schema type from any struct at compile time
  // Used for API validation, schema generation for future sidecar libraries, etc.
}

const fn fields_of<T>() -> [(str, Type)] {
  // Reflect on T's fields at compile time
  T.fields()
}
```

## Subtyping Rules
- Structural subtyping for traits (if it has the methods, it satisfies the trait)
- Nominal subtyping for data types (structs and enums)
- Covariance for read-only generic views: `&[Dog]` can be used where `&[Animal]` is expected if Dog implements Animal. Mutable collections are invariant.
- Invariance for mutable generics
- `never` is a subtype of all types
- All types are a subtype of `any`

## Comparison With Other Languages

| Feature | Turbo | Rust | TypeScript | Go | Haskell |
|---------|-------------|------|------------|-----|---------|
| Null safety | `T?` (sugar for discriminated union) | Option<T> | T \| undefined | nil checks | Maybe a |
| Generics | Monomorphized (opt-in type metadata) | Monomorphized | Erased | Reified-ish | Erased |
| Sum types | First-class | First-class | Tagged unions | None (interfaces) | First-class |
| Inference | HM-local | HM-local | Bidirectional | Minimal | Full HM |
| Effects | Lightweight tracking | None (traits) | None | None | Monads |
| Structural typing | For traits | No | Yes | Yes (interfaces) | Type classes |
| Soundness | Sound | Sound | Unsound | Sound | Sound |
