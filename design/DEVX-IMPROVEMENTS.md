# Developer Experience (DevX) Improvement Plan

> **Historical note (2026-04):** This document was written against an earlier version of the Turbo vision that included `agent` / `tool fn` as core-language keywords. That direction has since been retired — those features will ship as a separate `turbo-agent` library on top of the stable core, not as compiler keywords. References below to `agent`, `tool fn`, and `Agent.*` APIs reflect that older plan and are kept here for historical context only. For the current language surface, see **VISION.md**, **SYNTAX.md**, and **COMPATIBILITY.md**.
>
> **Status: design review of proposed syntax — many features here are Planned, not implemented.** This is a naming/ergonomics critique of the *design*, not a catalog of shipping features. In particular, `let ref` / `borrow`, `scope`-based structured concurrency, `region` blocks, `Shared<T>` / `WeakRef<T>`, `actor`, and the async runtime (`async`/`await` beyond OS-thread `spawn`) are **not implemented**. Read the equivalence tables as "how these *would* map to JS," not as a feature list.

## Executive Summary

Turbo markets itself as "JavaScript's soul, Rust's speed." After a thorough audit of every design document and both showcase pages, I found **41 distinct issues** across keyboard accessibility, concept naming, symbol density, progressive disclosure, error messages, and JS-developer familiarity. The design is *mostly* excellent -- the instincts are right -- but there are several places where Rust-isms leak through the "JavaScript feel" promise, where syntax choices will confuse beginners, and where the docs themselves are inconsistent with each other.

This plan is organized by priority:
- **P0 (Must Fix)** -- These will cause JS developers to bounce during their first 10 minutes. They violate the core promise.
- **P1 (Should Fix)** -- These create friction for intermediate users or produce "symbol soup" that hurts readability.
- **P2 (Nice to Have)** -- Polish items that would make Turbo feel even more welcoming.

---

## Section 1: JavaScript Equivalence Table

Before diving into issues, here is the full cognitive gap analysis for every major Turbo concept. This is the foundation for every recommendation that follows.

| # | Turbo Concept | JavaScript Equivalent | Gap (1-5) | Notes |
|---|---|---|---|---|
| 1 | `let x = 5` | `const x = 5` | 1 | Nearly identical. `let` is familiar. |
| 2 | `let mut x = 5` | `let x = 5` | 2 | `mut` is new but self-documenting. |
| 3 | `fn add(a: i32, b: i32) -> i32` | `function add(a, b)` | 2 | `fn` is common (Rust). `->` is unfamiliar. |
| 4 | `(x) => x * 2` | `(x) => x * 2` | 1 | Identical. |
| 5 | `\|x\| x * 2` | `(x) => x * 2` | 3 | Pipe closures are foreign to JS devs. |
| 6 | `"Hello, {name}!"` | `` `Hello, ${name}!` `` | 1.5 | Simpler (no backtick, no $). Slightly different. |
| 7 | `T?` | `T \| undefined` | 2 | Clean. Swift/Kotlin devs get it instantly. |
| 8 | `T ! E` | `throw new Error()` | 4 | **Novel syntax. No JS equivalent.** |
| 9 | `?` operator | `throw` (no equivalent) | 3 | Rust-ism. Must be taught. |
| 10 | `match x { ... }` | `switch (x) { ... }` | 2 | Better switch. Learnable. |
| 11 | `ok(n)` / `err(e)` | N/A | 3 | No JS equivalent. Pattern must be taught. |
| 12 | `some(v)` / `none` | `value` / `undefined` | 2.5 | `none` is intuitive. `some()` wrapper is new. |
| 13 | `if let x = expr` | `if (expr !== undefined)` | 2.5 | Swift-ism. Needs explanation. |
| 14 | `guard let x = expr else {}` | `if (!expr) return;` | 3 | Swift-ism. New keyword combination. |
| 15 | `x \|> f \|> g` | `g(f(x))` | 3 | Pipe operator is unfamiliar to most JS devs (TC39 Stage 2). |
| 16 | `with { } yield { }` | `try { } catch { }` | 3.5 | Elixir-ism. `yield` keyword collision with JS generators. |
| 17 | `struct User { }` | `class User { }` or `type User = { }` | 2 | TS devs know this. JS-only devs need to learn. |
| 18 | `trait Drawable { }` | `interface Drawable { }` | 2 | TS devs get it. JS-only devs less so. |
| 19 | `impl Drawable for Circle` | `class Circle implements Drawable` | 2.5 | Separated impl is unfamiliar. |
| 20 | `#[derive(Debug, Eq)]` | N/A | 4 | **Rust attribute syntax. Alien to JS devs.** |
| 21 | `#[test]` | `describe('test', () => {})` | 3.5 | Attribute syntax is unfamiliar. |
| 22 | `type Shape { Circle(...) }` | Discriminated unions (manual) | 2 | Cleaner than TS equivalent. |
| 23 | `async fn / await` | `async function / await` | 1 | Nearly identical. |
| 24 | `for await token in stream` | `for await (const token of stream)` | 1 | Nearly identical. |
| 25 | `spawn async { }` | N/A | 3 | No direct JS equivalent. |
| 26 | `scope \|s\| { }` | N/A | 4 | **Structured concurrency has no JS equiv.** |
| 27 | `channel::<Message>(buffer: 32)` | N/A | 4 | **Go-ism. No JS equivalent.** |
| 28 | `select { }` | `Promise.race()` (sort of) | 3.5 | Go-ism. |
| 29 | `actor Counter { }` | N/A | 4 | **Erlang-ism. No JS equivalent.** |
| 30 | `agent Assistant { }` | N/A (LangChain class) | 3 | Novel but intuitive in context. |
| 31 | `tool fn get_weather()` | N/A | 2.5 | Novel but self-documenting. |
| 32 | `comptime fn` | N/A | 4.5 | **Zig-ism. Completely foreign to JS devs.** |
| 33 | `rc<T>` / `weak<T>` | N/A | 4.5 | **Memory concept. Foreign to JS devs.** |
| 34 | `region scratch { }` | N/A | 4 | **Memory concept. Foreign to JS devs.** |
| 35 | `#[no_clone]` / `#[manual]` | N/A | 4 | **Memory annotation. Foreign to JS devs.** |
| 36 | `let ref x = y` | N/A | 4 | **Borrow concept. Foreign to JS devs.** |
| 37 | `import { X } from "mod"` | `import { X } from "mod"` | 1 | Identical. |
| 38 | `pub fn` | `export function` | 1.5 | `pub` is shorter than `export`. Learnable. |
| 39 | `resp.json::<User>()` | `resp.json() as User` (TS) | 3 | **Turbofish syntax. Rust-ism.** |
| 40 | `f64<Meters>` | N/A | 3.5 | Units of measure. No JS equiv. |
| 41 | `defer { cleanup() }` | `finally { }` (sort of) | 2 | Go-ism. Intuitive name. |

**Average gap: 2.8 / 5.0.** This is reasonable for a systems language targeting JS devs, but the outliers (gap >= 4) need special attention.

---

## Section 2: P0 -- Must Fix (8 issues)

These are issues that will cause a JavaScript developer to stop, get confused, or feel alienated within their first 10 minutes with Turbo. They violate the "write Turbo in 10 minutes" promise.

---

### P0-1: `#[derive(...)]` and `#[test]` Attribute Syntax is Alien to JS Devs

**Files:** SYNTAX.md (lines 65, 70, 268, 544-561), TYPE-SYSTEM.md (lines 169, 254), docs.html (lines 656, 965, 1132)

**Current:**
```
#[derive(Debug, Eq, Serialize)]
struct User { ... }

#[test]
fn test_addition() { ... }
```

**Problem:** The `#[...]` syntax is pure Rust. JS developers have never seen it. It uses `#` and `[` together, which is an unusual combination. The `#` symbol in JS means private class fields (`#myField`), creating a misleading association. Furthermore, `derive` is an opaque word -- what does it mean to "derive" Debug?

**Recommended Fix:** Use `@decorator` syntax, which JS/TS/Python developers already know from TC39 decorators (Stage 3, widely used via Babel/TypeScript):
```
@derive(Debug, Eq, Serialize)
struct User { ... }

@test
fn test_addition() { ... }

@bench
fn bench_sort() { ... }

@deprecated("Use new_function instead")
fn old_function() { }

@inline
fn hot_path() { }

@wasm_export
pub fn process_data(input: str) -> str { ... }
```

**Why it is better:**
- `@` is a single, easy-to-type character on every keyboard layout
- Decorators are a known concept in JS/TS/Python (millions of developers)
- `@test` reads as "this is a test" -- natural English
- `@derive(Serialize)` reads as "derive serialization" -- same meaning, familiar frame
- No bracket soup (`#[` ... `]` is three special characters; `@` is one)

**Scope of change:** Every file that mentions `#[...]`. This is a global find-and-replace across all design docs and showcase HTML.

---

### P0-2: The Turbofish `::<Type>()` Syntax is Cryptic

**Files:** SYNTAX.md (line 38, 468), docs.html (line 475, 1264), getting-started.html (implied)

**Current:**
```
let user = resp.json::<User>()?
let n = "42".parse::<i32>()?
let review = await agent.structured::<MovieReview>("Review Inception")
```

**Problem:** The `::<T>` syntax (called "turbofish" in Rust) is one of Rust's most mocked syntax choices. It exists in Rust because of a parser ambiguity with `<` (less-than operator). Turbo should not inherit this problem. A JS developer seeing `resp.json::<User>()` will be confused by the `::` and the `<>` in a method call position.

**Recommended Fix:** Use one of these alternatives:

Option A -- Call-site angle brackets (like TypeScript):
```
let user = resp.json<User>()?
let n = "42".parse<i32>()?
let review = await agent.structured<MovieReview>("Review Inception")
```

Option B -- Explicit `as` keyword for type-directed parsing:
```
let user = resp.json() as User?
let n = "42".parse() as i32?
```

Option A is strongly preferred because it matches TypeScript generics syntax exactly (`Array.from<number>(...)`, `document.querySelector<HTMLDivElement>(...)`). The parser ambiguity with `<` can be resolved at the parser level (TypeScript and many other languages solve this).

**Why it is better:**
- TypeScript developers use `method<Type>()` daily
- Eliminates the `::` which has no meaning in JS
- Reduces character count and visual noise

---

### P0-3: Pipe-Style Closures `|x| x * 2` Conflict with JS Mental Model

**Files:** SYNTAX.md (lines 57, 165, 202-205, 397-399, 481), CONCURRENCY.md (lines 163-164, 248-249), docs.html (line 455)

**Current:**
```
let double = |x| x * 2
let tasks = urls.map(|url| s.spawn(async { await fetch(url)? }))
scope |s| { ... }
```

**Problem:** Having two lambda syntaxes (`|x| x * 2` AND `(x) => x * 2`) is confusing. A JS developer will ask "what is the difference? when do I use which?" The `|...|` syntax is Rust-specific. The pipe character `|` is already used for bitwise OR and the pipe operator `|>`, creating visual ambiguity. In the expression `scope |s| { ... }`, a JS developer will not recognize `|s|` as a parameter list.

**Recommended Fix:** Standardize on arrow functions `(x) => x * 2` as the ONE lambda syntax. Drop pipe-style closures entirely, or relegate them to a "Rust compatibility" appendix.

Change `scope |s| { ... }` to `scope (s) => { ... }` or better yet `scope { |s| ... }` -- but the arrow form is most JS-familiar:
```
scope (s) => {
  let tasks = urls.map((url) => s.spawn(async { await fetch(url)? }))
  await tasks.collect()?
}
```

**Why it is better:**
- One syntax to learn, not two
- Arrow functions are what JS devs already know
- Removes the "which do I use?" question entirely
- `|` is no longer overloaded (bitwise OR, pipe operator, AND closure delimiter)

**If pipe closures must be kept:** Document them as "shorthand for Rust-familiar developers" and always show the arrow equivalent first in examples. Never use pipe closures in beginner-facing docs.

---

### P0-4: `comptime` is a Zig-ism Nobody Else Knows

**Files:** SYNTAX.md (lines 92, 513-522, 669), TYPE-SYSTEM.md (lines 627-636)

**Current:**
```
comptime fn generate_lookup() -> {str: u32} { ... }
const LOOKUP = comptime generate_lookup()
```

**Problem:** `comptime` is Zig jargon. It means "compile-time" but is abbreviated in a way that is opaque to anyone who has not used Zig. A JS developer will read "comptime" and think it is a typo. The cognitive gap rating is 4.5/5 -- one of the highest in the language.

**Recommended Fix:** Use `const fn` (familiar from C++ constexpr / Rust const fn) or `static fn`:
```
const fn generate_lookup() -> {str: u32} { ... }
const LOOKUP = generate_lookup()  // evaluated at compile time because it's a const fn assigned to a const
```

Or if you want to be explicit about compile-time evaluation at the call site:
```
const LOOKUP = @comptime generate_lookup()
```

But the best approach is to infer it: a `const fn` called in a `const` context is automatically compile-time evaluated. No new keyword needed.

**Why it is better:**
- `const fn` is self-documenting: "a function that can be evaluated as a constant"
- C++ developers know `constexpr`, Rust developers know `const fn`
- JS developers understand `const` already
- Eliminates a completely foreign keyword

---

### P0-5: `rc<T>` and `weak<T>` are Opaque Abbreviations

**Files:** MEMORY-MODEL.md (lines 392, 462-483, 855-873, 916-920)

**Current:**
```
struct Node {
    next: rc<Node>?
    prev: weak<Node>?
}
let a = rc(Node { ... })
```

**Problem:** `rc` means "reference counted" but a JS developer will not know that. It looks like a random two-letter abbreviation. `weak` is slightly better but `weak<Node>?` looks like gibberish to someone unfamiliar with weak references. These are Level 3+ memory concepts being exposed with Level 0 naming.

**Recommended Fix:** Use full, self-documenting names:
```
struct Node {
    next: Shared<Node>?      // "shared" -- multiple owners share this
    prev: WeakRef<Node>?     // "weak ref" -- JS devs know WeakRef from ES2021!
}
let a = Shared(Node { ... })
```

`WeakRef` is a real JavaScript API (ES2021). Using the same name instantly reduces the cognitive gap from 4.5 to 2.

Alternative: `Ref<T>` and `WeakRef<T>`, or `Shared<T>` and `Weak<T>`.

**Why it is better:**
- `Shared<T>` reads as "shared ownership" -- self-documenting
- `WeakRef<T>` is a real JS concept that JS developers already know
- No need to learn the abbreviation "rc"

---

### P0-6: `with { } yield { }` Block Collides with JS `yield`

**Files:** SYNTAX.md (lines 376-384), docs.html (lines 593-601)

**Current:**
```
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

**Problem:** In JavaScript, `yield` is used inside generator functions (`function*`). Using `yield` to mean "then return this result" in a `with` block creates a misleading association. A JS developer will think `yield` is about generators/iterators, especially since Turbo ALSO uses `yield` for streaming (SYNTAX.md line 474-475, AGENTIC.md line 354). This is the same keyword with two completely different meanings.

Additionally, the `with` keyword is deprecated in JavaScript strict mode (`with (obj) { ... }`), carrying negative connotations.

**Recommended Fix:** Replace `with { } yield { }` with something clearer:

Option A -- Use `try` blocks (familiar keyword, different semantics from JS try/catch):
```
fn complex_operation() -> Data ! Error {
  let config = load_config()?
  let conn = connect(config.db_url)?
  let data = query(conn, "SELECT *")?
  transform(data)
}
```
Wait -- this already works with just `?` propagation. The `with/yield` block adds no value over sequential `?` usage. **Consider dropping it entirely.**

Option B -- If you must keep it, rename to `do { } then { }`:
```
do {
  let config = load_config()?
  let conn = connect(config.db_url)?
  let data = query(conn, "SELECT *")?
} then {
  transform(data)
}
```

**Why it is better:**
- Eliminates the `yield` keyword collision
- `do/then` reads like English
- Or just drop the feature -- `?` chaining already handles this case

---

### P0-7: `T ! E` Result Syntax Has No Precedent and Uses a Hard-to-Spot Symbol

**Files:** SYNTAX.md (lines 11, 62, 346-356, 609), TYPE-SYSTEM.md (lines 374-424), docs.html

**Current:**
```
fn parse_int(s: str) -> i32 ! ParseError { ... }
```

**Problem:** While `T?` for optionals is well-established (Swift, Kotlin, Dart), `T ! E` for results has zero precedent in any mainstream language. The `!` between two types with spaces on both sides (`i32 ! ParseError`) is visually unusual. It could be confused with the logical NOT operator. In complex signatures, it creates a lot of visual noise:

```
fn load(path: str) -> Config ! IoError | ParseError | ValidationError { ... }
```

That line has `!`, `|`, and `?` all as type-level operators. Symbol soup.

**Recommended Fix:** This one is debatable. The `!` syntax is novel and actually pretty clever once you understand it. I would not change the syntax itself, but I WOULD:

1. **Always show the expanded form alongside the sugar in introductory docs:**
```
// T ! E is shorthand for Result<T, E>
// Read it as: "returns T, or fails with E"
fn parse_int(s: str) -> i32 ! ParseError { ... }
```

2. **Add a prominent "Reading Turbo Types" callout in the Getting Started page** that explains:
   - `T?` = "T or nothing" (optional)
   - `T ! E` = "T or error E" (result)
   - `?` at end of expression = "propagate the error"

3. **Consider offering `Result<T, E>` as an equally-supported alternative** for developers who find the sugar confusing. The TYPE-SYSTEM.md already mentions this but buries it.

4. **For union error types, use `+` instead of `|`** to avoid confusion with bitwise OR and pipe:
```
fn load(path: str) -> Config ! IoError + ParseError { ... }
```

---

### P0-8: Docs Show `map!{}` and `set![]` Macros Not Defined Anywhere

**File:** docs.html (lines 512-513)

**Current (in the Collections table on docs.html):**
```
{str: i32}    HashMap / Dictionary    let scores: {str: i32} = map!{"a" => 1}
{T}           HashSet                 let uniq: {i32} = set![1, 2, 3]
```

**Problem:** `map!{"a" => 1}` and `set![1, 2, 3]` use `!` macro syntax that is never defined anywhere in the design docs. SYNTAX.md shows map literals as `{ "Alice": 100, "Bob": 85 }` and set literals as `{1, 2, 3}`. The docs.html page contradicts the design.

**Fix:** Update docs.html to match SYNTAX.md:
```
{str: i32}    HashMap / Dictionary    let scores: {str: i32} = {"a": 1}
{T}           HashSet                 let uniq: {i32} = {1, 2, 3}
```

---

## Section 3: P1 -- Should Fix (15 issues)

These cause friction for intermediate developers or create readability problems.

---

### P1-1: Two Lambda Syntaxes Create Decision Fatigue

**Files:** SYNTAX.md (lines 165, 180-206)

SYNTAX.md actually says: "Use whichever feels right -- arrows for JS familiarity, pipes for brevity." This is bad guidance. It means every code review will have style debates, every tutorial will have to explain both, and beginners will constantly wonder which is "correct."

**Recommendation:** Pick ONE as the canonical syntax. Arrow functions should be canonical (JS familiarity). Pipe closures should be documented as an accepted alternative but never used in official examples, tutorials, or getting-started docs.

---

### P1-2: `let ref` for Borrowing is Unclear

**File:** MEMORY-MODEL.md (lines 159-176)

**Current:**
```
let ref first = dataset[0]   // borrow, no clone
```

**Problem:** `let ref` reads oddly in English. "Let reference first equals dataset zero"? The `ref` keyword is from Rust pattern matching and is not intuitive. A JS developer will not understand what this does.

**Recommendation:** Use `&` prefix (clearer intent) or a keyword like `borrow`:
```
let first = &dataset[0]       // Option A: & prefix (Rust-like, but more explicit)
let borrow first = dataset[0] // Option B: borrow keyword (self-documenting)
```

Option A is more concise and is already understood by C/C++/Rust developers. But if targeting JS devs specifically, Option B might be clearer.

---

### P1-3: `scope |s| { }` is Opaque for Structured Concurrency

**Files:** SYNTAX.md (line 480-484), CONCURRENCY.md (lines 162-168), docs.html (lines 798-803)

**Current:**
```
scope |s| {
    let tasks = urls.map(|url| s.spawn(async { await fetch(url)? }))
    await tasks.collect()?
}
```

**Problem:** `scope |s|` tells the developer nothing about what it does. The `s` variable has no obvious type or purpose. A JS developer will not understand that this creates a structured concurrency scope with automatic cancellation.

**Recommendation:** Make it more explicit:
```
async scope (s) => {
    let tasks = urls.map((url) => s.spawn(async { await fetch(url)? }))
    await tasks.collect()?
}
```

Or better, provide a named function:
```
let results = await TaskGroup.run((group) => {
    for url in urls {
        group.spawn(async { await fetch(url)? })
    }
})
```

`TaskGroup` is what Swift uses, and it is far more descriptive than `scope`.

---

### P1-4: Numeric Types `i32`, `u64`, `f64` etc. are Unfamiliar to JS Devs

**Files:** TYPE-SYSTEM.md (lines 19-23), docs.html (lines 494-499)

**Problem:** JS has `number` and `bigint`. Turbo has `i8`, `i16`, `i32`, `i64`, `i128`, `isize`, `u8`, `u16`, `u32`, `u64`, `u128`, `usize`, `f32`, `f64`. That is 14 numeric types. A JS developer will be overwhelmed.

**Recommendation:** The progressive disclosure promise means beginners should be able to use a simple type:
1. Define `int` as an alias for `i64` and `float` as an alias for `f64`
2. Use `int` and `float` in all beginner-facing docs and examples
3. Document the full numeric tower as "advanced: when you need control over size"

```
let age: int = 25        // beginner: just works (alias for i64)
let pi: float = 3.14     // beginner: just works (alias for f64)
let precise: i32 = 25    // advanced: explicit size control
```

Alternatively, `number` would be the most JS-familiar alias but might cause confusion if it behaves differently from JS `number`.

---

### P1-5: `pub` vs `export` -- Small but Meaningful Familiarity Gap

**Files:** SYNTAX.md (lines 537-539), docs.html (lines 1022-1027)

**Current:**
```
pub fn public_function() { }
pub struct PublicStruct { }
```

**Problem:** JS/TS uses `export`. `pub` is Rust. This is a small gap (1.5/5) but it is encountered constantly because every public API item uses it. Since the import syntax already mirrors JS (`import { X } from "module"`), using `export` on the other side would be perfectly consistent.

**Recommendation:** Consider `export` as an alias for `pub`:
```
export fn public_function() { }   // JS-familiar
pub fn also_public() { }          // Rust-familiar, also accepted
```

Or just pick `export` and drop `pub`. Since imports are JS-style, exports should be too.

---

### P1-6: `&T` and `&mut T` in Trait Definitions are Exposed Too Early

**Files:** SYNTAX.md (lines 296, 302), TYPE-SYSTEM.md (lines 294-296)

**Current:**
```
trait Drawable {
  fn draw(self, canvas: &Canvas)
}
```

**Problem:** `&Canvas` is a borrowed reference. This is a Level 1+ concept. But traits are a Level 0 concept (any developer defining interfaces will encounter traits). Seeing `&` in a trait definition forces beginners to ask "what does `&` mean?" before they are ready.

**Recommendation:** In the default auto-clone memory model, `&` should never be needed in beginner-facing trait definitions. The compiler should pass by reference automatically (as described in the string semantics section). Change examples to:

```
trait Drawable {
  fn draw(self, canvas: Canvas)   // compiler auto-borrows; no & needed in default mode
}
```

Only show `&T` in the advanced memory model sections.

---

### P1-7: `fn distance(self, other: Point)` -- `self` Without Class Context

**Files:** TYPE-SYSTEM.md (lines 163-165), SYNTAX.md (lines 296-308)

**Current:**
```
impl Point {
  fn distance(self, other: Point) -> f64 {
    sqrt((self.x - other.x).pow(2) + (self.y - other.y).pow(2))
  }
}
```

**Problem:** In JS, `this` is used inside classes/objects. `self` is Python. The `impl` block + `self` as first parameter pattern is pure Rust and is unfamiliar to JS developers. JS developers expect methods to be defined inside the type definition:

```js
class Point {
  distance(other) { /* this.x, this.y */ }
}
```

**Recommendation:** Allow method definitions inside struct bodies as syntactic sugar:
```
struct Point {
  x: f64
  y: f64

  fn distance(self, other: Point) -> f64 {
    sqrt((self.x - other.x).pow(2) + (self.y - other.y).pow(2))
  }
}
```

This is what the docs.html already implies with the actor examples. Make it consistent. Keep `impl` blocks as an advanced feature for adding methods to types you did not define (extension methods) or implementing traits.

Also consider `this` instead of `self` for JS familiarity (though `self` is acceptable since Python uses it).

---

### P1-8: String Interpolation Without `$` or Backtick May Cause Parsing Ambiguity

**Files:** SYNTAX.md (lines 53, 648), TYPE-SYSTEM.md (lines 73-93)

**Current:**
```
let greeting = "Hello, {name}!"
```

**Problem:** This is actually great -- simpler than JS template literals. BUT: what if you want a literal `{` in a string? The docs never explain this. Also, format specifiers like `{pi:.2}` and `{255:02x}` use the `:` character inside braces, which will surprise JS developers who expect `:` only in object literals.

**Recommendation:** Document the escape hatch (probably `{{` for a literal brace, like Python/Rust format strings). Add this to TYPE-SYSTEM.md:
```
let json = "JSON: {{"key": "value"}}"  // literal braces: {{ and }}
```

Also, document format specifiers prominently since they differ from JS's toFixed/toString:
```
let formatted = "Pi is {pi:.2}"   // like Python's f"{pi:.2f}", NOT like JS toFixed
```

---

### P1-9: `actor` vs `agent` Naming Confusion

**Files:** AGENTIC.md (lines 14-39), CONCURRENCY.md (lines 199-227)

**Problem:** Having both `actor` and `agent` as first-class keywords will cause constant confusion. They look similar, sound similar, and even the design docs need a full section explaining the difference. A developer will inevitably ask "is this an actor or an agent?" every time they design a component.

**Recommendation:** Consider renaming `actor` to `service` or `process`:
```
service Counter {         // was: actor Counter
  state: u64 = 0
  fn increment(self) { self.state += 1 }
}
```

`service` is a familiar concept from microservices architecture. `process` is what Erlang/Elixir actually call them. Either would reduce confusion with `agent`.

---

### P1-10: `spawn async { }` is Not a Single Keyword

**Files:** CONCURRENCY.md (lines 148-152), docs.html (lines 792-795, 811)

**Current:**
```
let handle = spawn async {
  await fetch_data(url)
}
```

**Problem:** `spawn async` is two keywords together. Is it "spawn" an "async block"? Or is "spawn async" a single operation? JS developers are used to `new Worker()` or just calling an async function. The combination is awkward.

**Recommendation:** Use a single function or keyword:
```
let handle = spawn {
  await fetch_data(url)
}
```

If the block contains `await`, it is implicitly async. No need for `spawn async` -- `spawn` alone is sufficient. The `async` is redundant because you are already spawning a concurrent task.

---

### P1-11: `select { }` Block Needs Better Framing for JS Devs

**Files:** CONCURRENCY.md (lines 190-196), docs.html (lines 822-827)

**Current:**
```
select {
  msg = rx1.recv() => handle_a(msg)
  msg = rx2.recv() => handle_b(msg)
  _ = timeout(5.seconds()) => handle_timeout()
}
```

**Problem:** `select` is a Go/Rust concept. JS developers would think of `Promise.race()` or `EventTarget`. The `= channel.recv() =>` syntax with both `=` and `=>` on the same line is visually dense.

**Recommendation:** Add a JS comparison comment in all docs:
```
// Like Promise.race() but for channels -- first one ready wins
select {
  msg = rx1.recv() => handle_a(msg)
  msg = rx2.recv() => handle_b(msg)
  _ = timeout(5.seconds()) => handle_timeout()
}
```

---

### P1-12: `PhantomData<State>` Exposed in Beginner-Adjacent Docs

**File:** TYPE-SYSTEM.md (lines 600-611)

**Current:**
```
struct Email<State> {
  address: str
  _state: PhantomData<State>
}
```

**Problem:** `PhantomData` is a Rust implementation detail. It is used to satisfy the compiler's "all generic parameters must be used" rule. It adds noise and confusion. A JS developer will ask "what is phantom data and why do I need an underscore-prefixed field that does nothing?"

**Recommendation:** Either (a) make phantom types work without the PhantomData marker (the compiler can infer it), or (b) move this entire example to an "Advanced Type System" section that is clearly marked as not-beginner-relevant.

---

### P1-13: The `io` Effect Prefix is Underdocumented

**File:** TYPE-SYSTEM.md (lines 564-577)

**Current:**
```
fn read_file(path: str) -> io str ! Error   // IO effect
```

**Problem:** The `io` keyword before the return type is introduced once and never explained. Is it a keyword? A modifier? A type? It is not listed in the syntax summary table. JS developers will be confused by a random word between `->` and the return type.

**Recommendation:** Either (a) explain effect tracking thoroughly with its own section and syntax reference entry, or (b) drop it from the surface syntax and make it an attribute/annotation:
```
@io
fn read_file(path: str) -> str ! Error
```

---

### P1-14: Inconsistent Semicolons / Statement Terminators

**Files:** All design docs

**Current:** SYNTAX.md says "no semicolons required" but some code examples have inconsistent formatting. Some have trailing commas in struct fields, some do not. Some function bodies use implicit returns, some use explicit `return`.

**Recommendation:** Be explicit about the rule:
- Semicolons are never required and never allowed (like Go's approach)
- Trailing commas are always optional
- Document this prominently in a "Style" subsection

---

### P1-15: `any` Type Needs Stronger Warning

**File:** TYPE-SYSTEM.md (lines 593-597)

**Current:**
```
let dynamic: any = get_from_c_library()
let typed = dynamic as? MyType
```

**Problem:** JS developers come from a world where `any` is widely used (TypeScript `any`). In Turbo, `any` should be strongly discouraged because it defeats the type system. But the docs present it casually as an "escape hatch" without sufficient warning.

**Recommendation:** Add a prominent warning:
```
// WARNING: 'any' disables all type checking. Use only for FFI boundaries.
// In TypeScript, 'any' is common. In Turbo, it should be extremely rare.
// If you find yourself reaching for 'any', consider using generics or traits instead.
```

---

## Section 4: P2 -- Nice to Have (18 issues)

---

### P2-1: Provide a "Rosetta Stone" Page

Create a dedicated page (or section in getting-started.html) that shows JavaScript on the left and Turbo on the right for 20 common patterns. This single artifact would be the most valuable learning resource for the target audience.

---

### P2-2: `usize` Should Be Explained or Aliased

`usize` appears in examples (SYNTAX.md line 138, docs.html line 499) without explanation. JS developers do not know what "platform-sized unsigned integer" means. Consider using `uint` as an alias and explaining `usize` only in systems programming contexts.

---

### P2-3: `as` Casting Syntax Needs Examples

TYPE-SYSTEM.md mentions `as` casting (line 23) but never shows complete examples. Add:
```
let n: i32 = 42
let f: f64 = n.into()   // safe conversion
let b: u8 = n as u8     // potentially truncating cast
```

---

### P2-4: Arrow in Return Type `->` vs `=>` Inconsistency

Arrow functions use `=>` but return type annotations use `->`. This is standard (Rust does the same), but it is worth a note in the docs explaining why:
- `=>` means "this expression evaluates to"
- `->` means "this function returns type"

---

### P2-5: `type Shape { Circle(...) }` -- The `type` Keyword Does Double Duty

`type` is used for both ADTs (`type Shape { ... }`) and type aliases (`type UserId = u64`). This is fine grammatically but could confuse developers who encounter one before the other. Add a note explaining both uses where each is introduced.

---

### P2-6: No Examples of Error Messages in Design Docs

The design docs rarely show what compiler error messages look like. The MEMORY-MODEL.md has one example (line 126-134) but it is the only one. For a language that claims great DX, showing error messages is essential.

**Recommendation:** Add at least 5 error message examples across the docs:
1. Type mismatch error
2. Missing match arm error
3. Use-after-move error (in #[no_clone] mode)
4. Tool function return type not serializable
5. Agent model string invalid

Each should show:
- The code with the error
- The full error message with line numbers, colors, and a suggestion
- The fix

Example:
```
error[E0042]: type mismatch
  --> src/main.tb:15:12
   |
15 |   let age: str = 42
   |            ^^^   ^^ expected `str`, found `i32`
   |
   = help: try converting with `42.to_string()`
```

---

### P2-7: Missing `else if` Documentation

SYNTAX.md shows `if/else` but never shows `else if` chains. Add:
```
let grade = if score >= 90 { "A" }
            else if score >= 80 { "B" }
            else if score >= 70 { "C" }
            else { "F" }
```

---

### P2-8: `defer` Needs a JS-Facing Explanation

`defer` (from Go) is mentioned in the syntax summary but never shown with a full example in the syntax reference. Add:
```
fn process_file(path: str) -> Data ! Error {
  let file = fs.open(path)?
  defer file.close()         // runs when function exits, no matter what
  // ... use file ...
}
// Like try/finally, but cleaner -- cleanup is declared next to acquisition
```

---

### P2-9: Getting Started Page Should Show Error Handling Earlier

The getting-started.html shows 5 progressive examples but error handling (tour #3) uses `! IoError` without explaining what it means. Move error handling to tour #4 (after async, which is more familiar) or add a one-line explanation: "The `!` means this function can fail with an error."

---

### P2-10: The `with` Block in Elixir Comparison is Misleading

SYNTAX.md says `with` blocks are "Elixir-inspired" but they work quite differently from Elixir's `with` (which uses pattern matching on results, not `?` propagation). Either rename the feature or adjust the docs to not reference Elixir.

---

### P2-11: `Stream<Token>` Type Name -- Consider `AsyncIterator<Token>`

For JS developers, `Stream<Token>` has no equivalent. But `AsyncIterator<Token>` or `AsyncIterable<Token>` would match JS's `AsyncIterator` and `Symbol.asyncIterator` protocol. Since Turbo streams are consumed with `for await ... in`, naming them `AsyncIter<Token>` or similar would be more self-documenting.

---

### P2-12: `5.seconds()` Method on Integers is Undocumented

Code examples use `5.seconds()`, `100.ms`, `1.second()` without any documentation of this syntax. Add a note about duration literals:
```
let timeout = 5.seconds()    // Duration type, extension method on integers
let short = 100.ms           // shorthand for milliseconds
let long = 1.minute()
```

---

### P2-13: `assert_eq` Without Parentheses Style

The test examples show `assert_eq(add(2, 3), 5)` -- a function call. In JS test frameworks, assertions are typically `expect(add(2,3)).toBe(5)`. Consider offering both styles or explaining why the function-call style was chosen.

---

### P2-14: File Extension `.tb` Should Be Mentioned Prominently

The `.tb` file extension is used in examples but never formally introduced. The getting-started page mentions `hello.tb` but does not say "Turbo source files use the `.tb` extension."

---

### P2-15: `{:}` Empty Map Literal is Surprising

CONCURRENCY.md line 249 shows `Arc.new(Mutex.new({:}))`. The `{:}` for an empty map is not documented. SYNTAX.md shows `{}` for empty maps (`let lookup: {str: i32} = {}`). Clarify whether `{}` or `{:}` is canonical for empty maps and whether `{}` could be ambiguous with empty blocks.

---

### P2-16: `model_config { }` Block Has No Keyword Context

AGENTIC.md (lines 438-450) introduces `model_config { }` as a top-level block but never explains what it is syntactically. Is it a function call? A special block? A macro? Add context.

---

### P2-17: The `unit` Keyword for Units of Measure is Undocumented in Syntax Reference

TYPE-SYSTEM.md (lines 613-623) introduces `unit Meters` and `f64<Meters>` but this syntax never appears in the SYNTAX.md summary table. Add it.

---

### P2-18: Getting-Started and Docs Pages Have Inconsistent Code

The getting-started.html page shows `some("Alice")` for creating an optional (line 540 of docs.html), but SYNTAX.md shows auto-wrapping without explicit `some()` for most cases. Standardize: show auto-wrapping as the default, `some()` as explicit when needed.

---

## Section 5: Keyboard Accessibility Audit

| Symbol | Usage | Difficulty | Recommendation |
|--------|-------|------------|----------------|
| `#[...]` | Attributes/decorators | Medium (Shift+3 + bracket) | **Replace with `@`** (P0-1) |
| `::<T>` | Turbofish generics | Hard (two colons + angle brackets) | **Replace with `<T>`** (P0-2) |
| `\|x\|` | Pipe closures | Medium (Shift+backslash x2) | **Deprecate in favor of arrow functions** (P0-3) |
| `\|>` | Pipe operator | Medium (Shift+backslash + Shift+period) | Acceptable. Keep. |
| `->` | Return type | Easy (dash + greater-than) | OK. |
| `=>` | Arrow function | Easy (equals + greater-than) | OK. |
| `?` | Error propagation / optional | Easy | OK. |
| `!` | Result type operator | Easy | OK, but see P0-7 about clarity. |
| `??` | Null coalescing | Easy | OK. Matches JS. |
| `?.` | Optional chaining | Easy | OK. Matches JS. |
| `{K: V}` | Map type syntax | Easy | OK. |
| `..` / `..=` | Range operators | Easy | OK. |
| `///` | Doc comments | Easy | OK. |
| `&` | Borrow/reference | Easy | OK, but should be hidden at Level 0. |
| `~` | Not used | N/A | Good -- not used. |
| `` ` `` (backtick) | Not used | N/A | Good -- not used anywhere (unlike JS template literals). |
| `^` | Not used | N/A | Good -- not used. |
| `\` | Raw strings only | Rare | OK -- only in `r"..."` context. |

**Verdict:** The keyboard accessibility is generally good. The three main offenders (`#[...]`, `::<T>`, and `|x|`) are all addressed in P0 recommendations.

---

## Section 6: Symbol Density Audit

Lines flagged as "symbol soup" (too many special characters for readability):

### Worst Offenders

```
// From SYNTAX.md line 481
let tasks = ids.map(|id| s.spawn(async { await fetch_user(id)? }))
//          ^   ^  ^|  |^ ^     ^     ^       ^          ^  ^^  ^^
```
**Count: 14 special characters.** This line has pipes, parens, braces, angle brackets-ish, `?`, and nested closures. A JS developer will struggle to parse this.

**Fix:** Break into multiple lines and use arrow functions:
```
let tasks = ids.map((id) => {
  s.spawn(async { await fetch_user(id)? })
})
```

---

```
// From SYNTAX.md line 467
let user = response.json::<User>()?
//                       ^^    ^^ ^^
```
**Fix:** Drop turbofish: `response.json<User>()?`

---

```
// From TYPE-SYSTEM.md line 516
fn load(path: str) -> Config ! IoError | ParseError { ... }
//                          ^ ^      ^ ^
```
This line has `!` and `|` as type operators, which is readable but unusual. Acceptable with documentation.

---

```
// From CONCURRENCY.md line 249
let shared = Arc.new(Mutex.new({:}))
```
**Fix:** `{:}` is unexplained. Use `{}` or `Map.new()`.

---

## Section 7: Progressive Disclosure Audit

| Feature | Level 0 Path? | Level 1 Path? | Level 2 Path? | Forcing Advanced Concepts? | Verdict |
|---------|:---:|:---:|:---:|:---:|---|
| Variables | Yes (`let x = 5`) | Yes (`let x: i32 = 5`) | N/A | No | GOOD |
| Functions | Yes (`fn add(a, b)`) | No -- requires type annotations on params | N/A | **Yes -- must learn i32/str for any function** | NEEDS WORK: allow inferred param types for private fns |
| Error handling | Yes (`?` propagation) | Yes (`match ok/err`) | Yes (`with` blocks) | No | GOOD |
| Optionals | Yes (`T?`, `??`) | Yes (`if let`, `guard let`) | Yes (`some()/none` matching) | No | GOOD |
| Memory | Yes (auto-clone) | Yes (`let ref`) | Yes (regions, manual) | No | GOOD |
| Concurrency | Yes (`async/await`) | Yes (`all()`, `spawn`) | Yes (actors, channels, select) | No | GOOD |
| Types | Yes (inference) | Yes (`struct`, `type`) | Yes (generics, traits, phantoms) | No | GOOD |
| Agents | Yes (`Agent.quick()`) | Yes (`Agent.new()`) | Yes (full `agent` declaration) | No | GOOD |
| Attributes | **No** -- `#[derive]` is Level 2+ syntax shown in Level 0 examples | -- | -- | **Yes** | NEEDS WORK: use `@derive` |
| Collections | Yes (`[1,2,3]`) | Yes (typed: `[i32]`) | Yes (maps, sets) | No | GOOD |

**Overall: 8/10 features have good progressive disclosure.** The two exceptions (function parameter types and attribute syntax) should be fixed.

---

## Section 8: Error Message UX Recommendations

The design docs mention error messages in passing but never show a comprehensive error message design. Here are 10 error messages Turbo should ship with, designed for JS developers:

### 1. Type Mismatch
```
error: type mismatch
  --> src/main.tb:12:18
   |
12 |   let name: i32 = "hello"
   |             ---   ^^^^^^^ expected `i32`, found `str`
   |             |
   |             type declared here
   |
   = help: if you want to parse a string as a number, use "hello".parse<i32>()?
   = help: if you want to change the type, change the annotation to `str`
```

### 2. Missing Match Arm
```
error: non-exhaustive match
  --> src/main.tb:25:3
   |
25 |   match shape {
26 |     Circle(r) => pi * r * r
27 |   }
   |   ^ missing arm for `Rectangle` and `Triangle`
   |
   = help: add the missing arms:
   |
   |     Rectangle(w, h) => todo()
   |     Triangle(a, b, c) => todo()
   |
   = help: or add a catch-all: `_ => todo()`
```

### 3. Value Used After Move (in #[no_clone] context)
```
error: use of moved value
  --> src/engine.tb:42:11
   |
40 |   let buf = LargeBuffer.new(data)
   |       --- value created here
41 |   transform(buf)
   |             --- value moved here
42 |   print(buf.len())
   |         ^^^^^^^^^ cannot use `buf` after it was moved
   |
   = note: `LargeBuffer` is marked @no_clone, so values are moved, not copied
   = help: clone it explicitly before the move: `transform(buf.clone())`
   = help: or use a reference: `transform(&buf)`
```

### 4. Unused Import
```
warning: unused import
  --> src/main.tb:2:10
   |
2  |   import { HashMap, HashSet } from "std/collections"
   |                     ^^^^^^^ `HashSet` is imported but never used
   |
   = help: remove the unused import, or prefix with `_` to suppress this warning
```

### 5. Agent Tool Not Serializable
```
error: tool return type must be serializable
  --> src/agent.tb:8:42
   |
8  |   tool fn get_data(query: str) -> DbConnection {
   |                                   ^^^^^^^^^^^^ `DbConnection` does not implement `Serialize`
   |
   = note: tool functions must return types that can be serialized to JSON
   = help: add @derive(Serialize) to `DbConnection`
   = help: or return a serializable summary type instead
```

### 6. Forgot `await`
```
warning: unused Future
  --> src/main.tb:10:3
   |
10 |   fetch_user(42)
   |   ^^^^^^^^^^^^^^ this returns a Future<User ! Error> which is never awaited
   |
   = help: add `await` to execute the async operation: `await fetch_user(42)`
   = note: Futures do nothing unless awaited
   = note: if you meant to run it in the background, use `spawn { await fetch_user(42) }`
```

### 7. Wrong `?` Context
```
error: `?` operator requires function to return a result type
  --> src/main.tb:5:25
   |
3  | fn process(input: str) -> str {
   |                           --- this function returns `str`, not a result type
...
5  |   let data = parse(input)?
   |                         ^ cannot use `?` here
   |
   = help: change the return type to `str ! Error` to enable error propagation
   = help: or handle the error explicitly with `match` or `.unwrap_or(default)`
```

### 8. Potential Reference Cycle
```
warning: potential reference cycle detected
  --> src/models.tb:3:5
   |
3  |   parent: Node
4  |   children: [Node]
   |
   = note: `Node` contains `Node` which contains `Node` (cycle)
   = help: use `WeakRef<Node>` for the back-reference to break the cycle:
   |
   |   parent: WeakRef<Node>?
```

### 9. String Interpolation Type Error
```
error: cannot interpolate type `[i32]` into string
  --> src/main.tb:8:28
   |
8  |   let msg = "Items: {items}"
   |                      ^^^^^ `items` is type `[i32]` which does not implement `Display`
   |
   = help: add @derive(Debug) to display a debug representation: "{items:?}"
   = help: or convert manually: "{items.join(", ")}"
```

### 10. Tool Doc Comment Missing
```
warning: tool function `search` has no doc comment
  --> src/tools.tb:3:1
   |
3  |   tool fn search(query: str) -> [Result] {
   |   ^^^^^^^^ missing `///` documentation
   |
   = note: doc comments on tool functions become the tool description sent to the LLM
   = help: add a doc comment:
   |
   |   tool fn search(query: str) -> [Result] {
   |     /// Search the web for information
```

---

## Section 9: Specific File Changes

Below is a summary of every concrete change, organized by file.

### SYNTAX.md Changes
| Line(s) | Current | Proposed | Why |
|---------|---------|----------|-----|
| 57 | `\|x\| x * 2` listed under "From TypeScript/JavaScript" | Remove pipe closures from JS section; list only under "From Rust" | Pipe closures are not from JS |
| 65, 70 | `#[derive(Debug, Eq, Serialize)]`, decorators as `#[test]`, `#[deprecated]` | `@derive(Debug, Eq, Serialize)`, `@test`, `@deprecated(...)` | P0-1: JS-familiar decorator syntax |
| 92 | `comptime` (compile-time execution) from Zig | `const fn` | P0-4: Self-documenting |
| 165, 202-205 | Both `\|x\| x * 2` and `(x) => x * 2` shown as equals | Standardize on arrow functions as canonical | P0-3, P1-1 |
| 268, 544-561 | All `#[...]` attribute syntax | All `@...` decorator syntax | P0-1 |
| 376-384 | `with { } yield { }` | Drop or rename to `do { } then { }` | P0-6 |
| 468 | `resp.json::<User>()?` | `resp.json<User>()?` | P0-2 |
| 480-484 | `scope \|s\| { }` | `TaskGroup.run((s) => { })` or `scope (s) => { }` | P1-3 |
| 513-522 | `comptime fn` / `comptime generate_lookup()` | `const fn` / inferred compile-time | P0-4 |

### TYPE-SYSTEM.md Changes
| Line(s) | Current | Proposed | Why |
|---------|---------|----------|-----|
| 169, 254 | `#[derive(...)]` | `@derive(...)` | P0-1 |
| 600-611 | `PhantomData<State>` exposed | Move to "Advanced Types" section, or eliminate PhantomData requirement | P1-12 |
| 564-577 | `io` effect keyword underdocumented | Add full section or use `@io` annotation | P1-13 |
| 627-636 | `comptime type`, `comptime fn` | `const type`, `const fn` | P0-4 |

### MEMORY-MODEL.md Changes
| Line(s) | Current | Proposed | Why |
|---------|---------|----------|-----|
| 392, 462-483 | `rc<T>`, `weak<T>` | `Shared<T>`, `WeakRef<T>` | P0-5 |
| 159-176 | `let ref x = y` | `let x = &y` or `let borrow x = y` | P1-2 |
| 99 | `#[no_clone]` | `@no_clone` | P0-1 |
| 207 | `#[manual]` | `@manual` | P0-1 |

### CONCURRENCY.md Changes
| Line(s) | Current | Proposed | Why |
|---------|---------|----------|-----|
| 148-152 | `spawn async { }` | `spawn { }` (infer async) | P1-10 |
| 162-168 | `scope \|s\| { }` | `TaskGroup.run((s) => { })` | P1-3 |
| 249 | `Arc.new(Mutex.new({:}))` | `Arc.new(Mutex.new({}))` or explain `{:}` | P2-15 |

### AGENTIC.md Changes
| Line(s) | Current | Proposed | Why |
|---------|---------|----------|-----|
| 256 | `#[derive(Schema)]` | `@derive(Schema)` | P0-1 |
| 274-275 | `#[schema(validate)]` | `@schema(validate)` | P0-1 |
| 298-319 | `#[circuit_breaker(...)]`, `#[retry(...)]` | `@circuit_breaker(...)`, `@retry(...)` | P0-1 |

### COMPILATION.md Changes
| Line(s) | Current | Proposed | Why |
|---------|---------|----------|-----|
| 199 | `#[wasm_export]` | `@wasm_export` | P0-1 |

### docs.html Changes
| Line(s) | Current | Proposed | Why |
|---------|---------|----------|-----|
| 512-513 | `map!{"a" => 1}`, `set![1, 2, 3]` | `{"a": 1}`, `{1, 2, 3}` | P0-8 |
| 656, 965, 1132-1137 | `#[derive(...)]`, `#[test]`, `#[test_case]`, `#[bench]` | `@derive(...)`, `@test`, `@test_case`, `@bench` | P0-1 |
| 475, 771, 1264 | `::<User>`, `::<i32>` (turbofish) | `<User>`, `<i32>` | P0-2 |
| 455, 799-800 | Pipe closures in examples | Arrow functions | P0-3 |

### getting-started.html Changes
| Line(s) | Current | Proposed | Why |
|---------|---------|----------|-----|
| No mention | Missing "Reading Turbo Types" callout | Add callout explaining `T?`, `T ! E`, `?` | P0-7 |
| Tour #3 | Error handling shows `! IoError` without explanation | Add one-line explanation | P2-9 |

---

## Section 10: Summary

### Issue Count by Priority
| Priority | Count | Estimated Effort |
|----------|-------|-----------------|
| P0 (Must Fix) | 8 | 2-3 days across all docs |
| P1 (Should Fix) | 15 | 3-5 days across all docs |
| P2 (Nice to Have) | 18 | 5-7 days across all docs |
| **Total** | **41** | **10-15 days** |

### Top 5 Changes by Impact
1. **`#[...]` to `@...`** -- Affects every file, every example, every page. Single biggest visual change.
2. **Drop turbofish `::<T>`** -- Affects every generic method call. Huge readability win.
3. **Standardize on arrow functions** -- Removes decision fatigue. One syntax to learn.
4. **Rename `comptime` to `const fn`** -- Eliminates the most foreign keyword.
5. **Rename `rc<T>`/`weak<T>` to `Shared<T>`/`WeakRef<T>`** -- Makes memory concepts self-documenting.

### What Turbo Gets Right (Keep These)
- `T?` for optionals -- Universally praised syntax choice
- `none` (lowercase) -- Casual, approachable
- `"Hello, {name}!"` interpolation -- Better than JS template literals
- `import { X } from "module"` -- Identical to JS
- `let` / `let mut` -- Clean, familiar
- `fn` keyword -- Short, Rust-familiar, widely known
- `match` expressions -- Better than switch
- `async/await` -- Matches JS exactly
- `for await ... in` -- Matches JS exactly
- `?.` and `??` operators -- Matches JS exactly
- Auto-clone memory model -- The "JavaScript Promise" is brilliant
- The entire toolchain philosophy -- Best-in-class design
- `agent` and `tool` as first-class keywords -- Bold, differentiated, well-executed
- Progressive memory escape hatches (Level 0-3) -- Exactly right

Turbo's design is fundamentally sound. The issues identified here are surface-level syntax choices that can be fixed without touching the semantics. The core architecture -- auto-clone memory, typed errors, algebraic types, first-class agents -- is excellent. The improvements above will make the surface match the substance.
