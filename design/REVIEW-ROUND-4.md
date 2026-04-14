# Review Round 4 -- Turbo Language Design

> **Historical note (2026-04):** This review discusses `agent` / `tool fn` as core-language keywords. That direction has been retired — those features now live in the planned `turbo-agent` sidecar library, not the compiler. Preserved for historical context. Current spec: **VISION.md**, **SYNTAX.md**, **COMPATIBILITY.md**.

**Overall Score: 9.0/10** (was 8.4/10 in Round 3)

**Review context:** This review follows major DevX improvements applied between Round 3 and Round 4, including: `@decorator` syntax replacing `#[attribute]`, turbofish removal (`::<T>` to `<T>`), `Shared<T>` / `WeakRef<T>` replacing `rc<T>` / `weak<T>`, `const fn` replacing `comptime fn`, new elegant type syntax (`T?`, `T ! E`, `none`, `ok()`, `err()`), JS-like arrow functions as primary closure syntax, a fully specified Error trait hierarchy, string semantics, and agent testing patterns.

---

## Per-File Scores

| File | Score | Change | Key Assessment |
|------|-------|--------|----------------|
| VISION.md | 9/10 | +0.5 | Clean and consistent. All syntax updated: `@derive(TypeInfo)`, `const fn`, `T?`, `T ! E`. The one-liner is sharp. `tool fn` example uses lowercase `@description` / `@parameter` attributes. No remaining Rust-isms. |
| SYNTAX.md | 9.5/10 | +0.5 | The crown jewel of the design. The "Elegant by Design" opening section is outstanding. Arrow functions are primary, pipe closures are documented as shorthand. The "Real-World Patterns" section at the end consistently uses `@` decorators, `Shared<T>`, `WeakRef<T>`, `const fn`, and arrow syntax. The type sugar reference table is best-in-class. |
| TYPE-SYSTEM.md | 9.5/10 | +1.0 | **Major improvement.** The Error Trait hierarchy (lines 430-558) is now fully specified with base trait, standard error types (`IoError`, `ParseError`, `NetworkError`, `TimeoutError`, `ValidationError`), custom error definition, error composition via `From` trait, and three levels of return type specificity. String semantics (lines 38-150) are fully defined: `str.len()` returns character count, `str[0]` returns a `char` (Unicode scalar), slicing returns `str`. This resolves two of Round 3's top remaining gaps. |
| MEMORY-MODEL.md | 9/10 | +0 | Remains the strongest design document. All syntax is correct: `@no_clone`, `@manual`, arrow functions in closures. The auto-clone code examples use `(p) =>` arrow syntax consistently. Memory report output uses `.tb` extensions. No changes needed since Round 3. |
| CONCURRENCY.md | 8.5/10 | +0.5 | The "Colored Function Solution" section (lines 35-77) now properly addresses the colored function claim with a concrete mechanism: sync code CAN call async functions, the runtime transparently blocks the current task (Go-style), and the compiler emits a warning. This is no longer an unsubstantiated claim -- it is a documented design choice with tradeoffs explained. Arrow syntax used in scope examples. |
| AGENTIC.md | 9/10 | +0.5 | **Major improvement.** The "Testing Agents" section (lines 437-561) is entirely new and addresses Round 3's gap. It covers mocking LLMs with `MockModel`, snapshot testing, multi-step tool use verification, streaming response testing, and error recovery / circuit breaker testing. This is a complete agent testing story with concrete code examples. All examples use `@test` decorator syntax. |
| COMPILATION.md | 8.5/10 | +0 | Solid and unchanged. Uses `@wasm_export` consistently. Sanitizer and diagnostic sections are well-specified. The dual-backend strategy (cranelift for dev, LLVM for release) is realistic. |
| TOOLCHAIN.md | 8.5/10 | +0.5 | The duplicate benchmarking section identified in Round 3 appears to still exist (lines 89-96 and 206-237), but the detailed version is now comprehensive enough that the brief one reads as a summary. Standard library section in TOOLCHAIN.md (turbo/collections) now uses arrow syntax in all examples. `@test`, `@bench`, `@property`, `@test_case` decorators used consistently. |
| POLYGLOT.md | 7.5/10 | -0.5 | **Still uses old `#[` syntax in three places.** Lines 68, 88, 90, and 106 use `#[wasm_export]`, `#[python_module]`, `#[python_fn]`, and reference `#[wasm_export]` functions. These should be `@wasm_export`, `@python_module`, `@python_fn`. This is the only design file with remaining `#[` syntax. |
| VARIANTS.md | 8/10 | +1.0 | **Improved.** The shared core list (line 5) now correctly references `T ! E` and `T?`. The Variant D code example (line 170) now uses `ok(step3)` (lowercase). However, the file's purpose remains unclear post-decision, and it still substantially duplicates MEMORY-MODEL.md content. |
| index.html (Showcase) | 8.5/10 | +0 | The Rust comparison table (lines 699-748) correctly shows `None`, `Some(42)`, `Ok(value)`, `Err(e)` in the Rust column and `none`, auto-wrap, `ok(value)`, `err(e)` in the Turbo column -- this is correct because it is comparing Rust syntax to Turbo syntax. Pipe operator examples on line 636-637 still use `|u|` pipe-style closures instead of the preferred `(u) =>` arrow syntax. Minor inconsistency with the design's stated preference. |
| getting-started.html | 9.5/10 | +0.5 | Excellent. All five progressive examples use correct, current syntax. Error handling example correctly shows `T ! E` with `?`. The agent example uses `tool fn` and `Agent.new()`. No old syntax anywhere. The best onboarding page in the set. |
| docs.html | 9/10 | +1.5 | **Major improvement.** Round 3's critical issue is fully resolved. The error handling section (line 582-584) now correctly uses `ok(n)` / `err(e)` (lowercase). The pattern matching section (lines 624, 639-643) correctly uses `ok(user)`, `ok({ status: 200, body })`, `err(e)`. The async function example (line 473) correctly uses `User ! Error`. The structured concurrency scope examples (lines 799-802) still use pipe-style `|s|` and `|url|` closures instead of the preferred `(s) =>` and `(url) =>` arrow syntax. Map/set collection examples (line 512-513) use `map!{"a" => 1}` and `set![1, 2, 3]` macro syntax that is not documented anywhere in SYNTAX.md. |

---

## Critical Issues (Must Fix)

### 1. POLYGLOT.md Still Uses `#[` Attribute Syntax

POLYGLOT.md is the only remaining design file with old Rust-style `#[` attributes. Four instances need updating:

**Line 68:**
```
#[wasm_export]
```
Should be:
```
@wasm_export
```

**Lines 88 and 90:**
```
#[python_module]
mod fast_math {
  #[python_fn]
```
Should be:
```
@python_module
mod fast_math {
  @python_fn
```

**Line 106:**
```
- Automatic for all `#[wasm_export]` functions
```
Should be:
```
- Automatic for all `@wasm_export` functions
```

This is a critical issue because POLYGLOT.md is an active design file that contributors will reference. Having `#[` syntax here undermines the consistency of the `@` decorator decision.

### 2. docs.html Collections Table Uses Undocumented Macro Syntax

The collections table in docs.html (line 512-513) shows:
```html
<code>let scores: {str: i32} = map!{"a" => 1}</code>
<code>let uniq: {i32} = set![1, 2, 3]</code>
```

The `map!{}` and `set![]` macro syntax appears nowhere in SYNTAX.md, TYPE-SYSTEM.md, or any other design file. SYNTAX.md defines map and set literals as:
```
let scores = { "Alice": 100, "Bob": 85 }
let uniq = {1, 2, 3}
```

The docs page should use the canonical literal syntax, not undocumented macros. This is a critical issue because docs.html is the reference new users learn from.

---

## Warnings (Should Fix)

### 1. Pipe-Style Closures Used Where Arrow Syntax Is Preferred

SYNTAX.md explicitly states: "Arrow functions are the primary/canonical form; pipes are accepted shorthand." Several showcase and documentation files still use pipe-style closures `|x|` in prominent examples where arrow syntax `(x) =>` should be preferred:

- **index.html line 636-637:** `filter(|u| u.age > 18)` and `sort_by(|u| u.name)` -- should be `filter((u) => u.age > 18)` and `sort_by((u) => u.name)`
- **index.html line 651-653:** `scope |s| { ... urls.map(|url| ...)` -- should be `scope (s) => { ... urls.map((url) => ...)`
- **docs.html line 745:** `items.reduce(|a, b| if a > b { a } else { b })` -- should be `items.reduce((a, b) => if a > b { a } else { b })`
- **docs.html lines 799-800:** `scope |s| { ... urls.map(|url| ...)` -- should be `scope (s) => { ... urls.map((url) => ...)`
- **docs.html line 871:** `map_stream(|t| t.text.to_upper())` and `filter_stream(|t| !t.is_empty())` -- should use arrow syntax

The design files (SYNTAX.md, MEMORY-MODEL.md, AGENTIC.md) have been updated to use arrow syntax consistently. The showcase and docs pages should follow suit for consistency.

### 2. `scope` Block Syntax Is Inconsistent Across Files

Three different syntaxes for structured concurrency scopes appear across the design:

- SYNTAX.md line 480-483: `scope (s) => { ... }` (arrow syntax)
- CONCURRENCY.md line 163: `scope (s) => { ... }` (arrow syntax)
- index.html line 651: `scope |s| { ... }` (pipe syntax)
- docs.html line 799: `scope |s| { ... }` (pipe syntax)

The design files have converged on `scope (s) => { ... }`. The showcase pages should be updated to match.

### 3. VARIANTS.md Purpose Remains Unclear

Round 3 flagged this and it is still unresolved. Now that MEMORY-MODEL.md explicitly declares CTRC as the chosen default and the other files have been updated to use the new syntax, VARIANTS.md's primary content is redundant. The file serves as a historical record of the variant strategy but could confuse contributors who encounter it without context. It should either be:
- (a) Moved to an `archive/` directory with a note explaining it is historical
- (b) Reduced to a brief summary referencing MEMORY-MODEL.md for details
- (c) Rewritten to focus solely on the non-memory variant axes (compilation profiles, concurrency runtime variants)

### 4. VectorMemory Still Couples to ChromaDB

AGENTIC.md (line 330) and docs.html (line 947) still show `ChromaDB.connect("localhost:8000")` as the vector memory example. Round 3 flagged this. The example should use a trait-based abstraction to make clear the language is backend-agnostic:

```
store: VectorStore.connect("localhost:8000", backend: .chroma)
```

### 5. Agent `model` Field Remains an Untyped String

The `model: "claude-sonnet"` pattern is still a string literal with no compile-time safety. Round 3 flagged this. While AGENTIC.md mentions compile-time validation against known providers, the mechanism is still unspecified. At minimum, the design should show what happens when a user misspells the model name:

```
agent MyAgent {
  model: "calude-sonet"  // COMPILE WARNING: unknown model "calude-sonet". Did you mean "claude-sonnet"?
}
```

### 6. Structural Typing for Traits Still Underspecified

TYPE-SYSTEM.md says "Any type with a `to_string` method automatically satisfies Printable" but still does not address: method name collisions, visibility rules (does a private method satisfy a public trait?), or how structural matching works with generic traits. This has been flagged since Round 2 and remains a specification gap.

### 7. Effect System Still Lacks Formal Specification

TYPE-SYSTEM.md lists effects (`async`, `io`, `unsafe`, `throws`, `diverges`) but still does not specify: composition rules (can a function be `async + io`?), syntax for multi-effect functions, whether effects are inferred or declared, or effect polymorphism. The `io` effect appears in the example `fn read_file(path: str) -> io str ! Error` but the syntax for combining multiple effects is not shown. This has been flagged since Round 2.

### 8. Duplicated Benchmarking Section in TOOLCHAIN.md

TOOLCHAIN.md still has two benchmarking sections: a brief one under "The Complete Toolchain" (lines 89-96) and a detailed one under "Performance Monitoring & Observability" (lines 206-237). The brief one should be removed or reduced to a cross-reference.

### 9. `Arc.new(Mutex.new({:}))` in CONCURRENCY.md Uses Non-Turbo Idiom

CONCURRENCY.md line 249 uses `Arc.new(Mutex.new({:}))` which is a direct Rust-ism. In the context of Turbo's auto-clone + CTRC model with `Shared<T>` as the escape hatch, this should likely be `Shared.new(Mutex.new({}))` or simply `let shared = Mutex.new({})` (since CTRC handles sharing by default). The `Arc` type is never defined in any Turbo design document.

---

## Improvements Since Round 3

### 1. docs.html Syntax: FULLY RESOLVED
Round 3's single most important fix is done. All error handling and pattern matching examples now use lowercase `ok()` / `err()`. The async function example uses `User ! Error` instead of `Result<User, Error>`. This was the #1 priority from Round 3 and it is completely addressed.

### 2. Error Type Hierarchy: FULLY SPECIFIED (NEW)
TYPE-SYSTEM.md now includes a complete "Error Type Hierarchy" section (lines 429-558) with:
- Base `Error` trait with `message()`, `source()`, and `stack()` methods
- Five standard error types: `IoError`, `ParseError`, `NetworkError`, `TimeoutError`, `ValidationError`
- Custom error definition pattern
- Error return signature levels (generic `! Error`, specific `! ParseError`, union `! IoError | ParseError`)
- Error composition via `From` trait with `?` auto-conversion
- Clear design principles: errors are values, errors are just types, progressive specificity

This was the #1 remaining gap from Round 3 and it is now thoroughly addressed.

### 3. String Semantics: FULLY SPECIFIED (NEW)
TYPE-SYSTEM.md now includes complete string semantics (lines 38-150) answering all questions flagged in Round 3:
- `str.len()` returns character count (not byte count)
- `str[0]` returns a `char` (Unicode scalar value, not a byte)
- `str.byte_len()` for byte-level access
- Multi-byte strings handled correctly (`"Hello 🌍"[6]` returns `'🌍'`)
- String interpolation, multi-line strings, raw strings, and common operations all documented

### 4. Agent Testing: FULLY SPECIFIED (NEW)
AGENTIC.md now includes a complete "Testing Agents" section (lines 437-561) with:
- `MockModel` for deterministic, free, no-API-call testing
- Tool mocking with `mock()` and call count assertions
- Multi-step tool use verification (tool call order and argument checking)
- Streaming response testing
- Error recovery and circuit breaker testing
- Snapshot testing for structured agent output

This was a top remaining gap from Round 3 and it is now thoroughly addressed.

### 5. Colored Function Problem: PROPERLY EXPLAINED
CONCURRENCY.md's "Colored Function Solution" section (lines 35-77) now provides a concrete mechanism rather than an unsubstantiated claim:
1. `await` is required when calling async functions (no hidden suspension)
2. Sync code CAN call async functions without `await` -- the runtime blocks the current task (Go-style)
3. The compiler emits a warning (not an error) when this happens
4. Making a function async never breaks callers

This is a genuine design choice with clear tradeoffs, not a magical claim. The "Why this works" subsection provides four concrete justifications.

### 6. `@` Decorator Syntax: FULLY APPLIED (Except POLYGLOT.md)
All design files except POLYGLOT.md now consistently use `@` decorator syntax. VISION.md, SYNTAX.md, TYPE-SYSTEM.md, MEMORY-MODEL.md, AGENTIC.md, COMPILATION.md, and TOOLCHAIN.md all use `@derive`, `@test`, `@bench`, `@deprecated`, `@inline`, `@wasm_export`, `@no_clone`, `@manual` consistently. All three showcase HTML pages use `@` syntax.

### 7. Arrow Functions: PRIMARY IN DESIGN FILES
SYNTAX.md, MEMORY-MODEL.md, AGENTIC.md, and TOOLCHAIN.md all use arrow functions `(x) => ...` as the primary closure syntax. Pipe-style `|x|` is documented as accepted shorthand. The design files are internally consistent on this. Only the showcase HTML pages still use pipe-style in some prominent examples (see Warnings).

### 8. `Shared<T>` / `WeakRef<T>`: FULLY APPLIED
All files consistently use `Shared<T>` and `WeakRef<T>` instead of the old `rc<T>` / `weak<T>`. MEMORY-MODEL.md, SYNTAX.md, and VARIANTS.md all use the new naming. The SYNTAX.md "Shared State with `Shared<T>`" example (lines 879-921) is a clean, complete demonstration.

### 9. `const fn`: FULLY APPLIED
All files use `const fn` instead of the old `comptime fn`. SYNTAX.md, VISION.md, TYPE-SYSTEM.md, and VARIANTS.md all reference `const fn` consistently.

---

## What's Working Well

### 1. The Design Is Genuinely Coherent Across All Files
For the first time in the review process, all ten design files tell a consistent story. There are no contradictions between files about the memory model, generics strategy, syntax choices, or error handling. A contributor reading any file will form an accurate picture of the language.

### 2. The "JavaScript Promise" Is Fully Delivered End-to-End
The design now delivers on its core pitch from syntax to memory to async to errors:
- **Syntax:** `let`, arrow functions, destructuring, template literals, optional chaining, `??`, `import from`
- **Memory:** Auto-clone + CTRC means zero "value moved here" errors at Level 0
- **Async:** `async`/`await`, `all()`, `race()`, `for await...in`, top-level `await`
- **Errors:** `T ! E` with `?` propagation, full error hierarchy with standard types
- **Strings:** One string type (`str`), character-based indexing, interpolation without prefix
- **Testing:** `@test` decorator, snapshot testing, mocking -- familiar patterns

A JavaScript developer could write production-quality Turbo code without ever encountering a memory annotation, lifetime parameter, or borrow checker error. This is the design's strongest achievement.

### 3. The Error Hierarchy Is Well-Designed
The new Error Trait section in TYPE-SYSTEM.md is the best error handling design for a new systems language I have seen. Key strengths:
- Three levels of specificity (`! Error` for prototyping, `! ParseError` for precision, `! IoError | ParseError` for unions)
- Standard error types cover the common cases without being exhaustive
- Error composition via `From` trait is documented with both options (union types vs. wrapper types)
- The design explicitly avoids checked exceptions, exception hierarchies, and registration ceremony
- The `?` operator handles error conversion automatically via `From`

### 4. Agent Testing Is First-Class
The new testing section in AGENTIC.md is comprehensive and practical. Key strengths:
- `MockModel.new(responses: [...])` is clean and intuitive
- Tool mocking with `mock(get_weather, returns: ...)` integrates with the standard `turbo/test` framework
- Multi-step tool verification (`mock_search.last_args().query`) enables detailed behavior testing
- Error recovery testing (circuit breaker, retry) uses the same mock infrastructure
- No separate test harness -- everything works with `@test` and `turbolang test`

### 5. The Syntax Sugar System Is a Genuine Differentiator
The `T?` / `T ! E` / `none` / `some()` / `ok()` / `err()` / `[T]` / `{K: V}` / `{T}` sugar system is now the most readable approach to optionals, errors, and collections in any systems language. The TYPE-SYSTEM.md "Sugar vs. Power" section showing equivalence (`T?` = `Optional<T>`, `T ! E` = `Result<T, E>`) is exemplary progressive disclosure.

### 6. The Showcase Pages Are Production-Ready
All three HTML pages are visually polished and technically accurate. docs.html has been fully corrected from Round 3's critical issues. getting-started.html remains the strongest onboarding experience for any pre-release language. The animated benchmark charts, memory ladder visualization, and Rust comparison table are effective communication tools.

---

## Remaining Gaps (Ordered by Severity)

### 1. No Formal Effect System Specification (Severity: Medium)
Flagged in Rounds 2 and 3. Still unaddressed. Effects are listed (`async`, `io`, `unsafe`, `throws`, `diverges`) but composition rules, syntax, inference behavior, and effect polymorphism are absent. The `io` effect's syntax (`fn read_file(path: str) -> io str ! Error`) places the effect keyword before the return type, but combining `async + io` is not shown.

### 2. Structural Typing for Traits Underspecified (Severity: Medium)
Flagged in Rounds 2 and 3. Still unaddressed. Method name collisions, visibility rules, and interaction with generics are unspecified for structural trait matching.

### 3. No Macro/Metaprogramming System Design (Severity: Medium)
`@derive(...)` and `const fn` are documented, but the general metaprogramming system is not specified. Can `const fn` generate types? Generate trait implementations? Generate entire modules? How does compile-time computation interact with the type checker? This matters because `@derive` is used extensively and the expansion mechanism is undefined.

### 4. No Package Versioning/Edition Strategy (Severity: Low-Medium)
`turbo.toml` shows `edition = "2026"` but editions are not explained. How do breaking changes work? How do dependencies on different editions interoperate?

### 5. Interaction Between Auto-Clone and Concurrency (Severity: Medium)
If the compiler auto-clones a value to pass it to a spawned task, is the clone `Send`? What if the auto-clone races with a mutation? The intersection of auto-clone and `Send`/`Sync` is where the hardest bugs will hide. This was flagged in Round 3 and remains unaddressed.

### 6. No Governance Model (Severity: Low)
VISION.md mentions an RFC process but merge authority, release cadence, and breaking change decisions are not specified.

---

## JS-Like Feel Assessment

**Score: 9/10. Turbo genuinely feels like JavaScript now.**

A JavaScript developer encountering Turbo code would recognize:
- `let` / `const` / `let mut` bindings
- `(x) => x * 2` arrow functions
- `let { name, age } = user` destructuring
- `"Hello, {name}!"` template literals
- `user?.address?.city` optional chaining
- `name ?? "default"` null coalescing
- `async`/`await` with `all()` and `race()`
- `for await token in stream` async iteration
- `import { X } from "module"` imports
- `@test`, `@deprecated` decorators

The only constructs that would feel unfamiliar:
- `let mut` (instead of `let` vs `const` -- but immediately understandable)
- `T?` and `T ! E` type annotations (but cleaner than TypeScript's `T | undefined`)
- `match` exhaustive pattern matching (no JS equivalent, but intuitive)
- `fn` keyword (instead of `function`, but shorter and common across many languages)
- `trait` / `impl` (instead of `interface` / `implements`, but similar concept)

**Not encountered at Level 0:**
- Lifetime annotations
- Borrow checker errors
- "Value moved here" errors
- Any memory-related compiler messages
- Ownership transfer concepts
- Reference counting details

### Progressive Disclosure Validation

| Complexity Level | What the Developer Sees | Familiar From |
|-----------------|------------------------|---------------|
| Day 1 | `let`, arrow functions, `print()`, string interpolation | JavaScript |
| Day 2 | `T?`, optional chaining, `??`, `if let` | TypeScript/Swift |
| Day 3 | `T ! E`, `?` propagation, `match` | Novel but clean |
| Week 1 | `struct`, `trait`, `impl`, generics | TypeScript interfaces |
| Week 2 | `async`/`await`, `Stream<T>`, `tool fn`, `agent` | JS async + novel |
| Month 1 | `let ref`, `region {}`, `@no_clone` | New concepts (opt-in) |
| Never (unless needed) | `@manual`, explicit lifetimes, raw pointers | Rust (opt-in) |

This progression is well-calibrated. A developer can be productive on Day 1 and only encounters systems programming concepts when they choose to optimize.

---

## Syntax Consistency Audit

### `@` Decorators
| File | Status |
|------|--------|
| VISION.md | `@derive(TypeInfo)`, `@description`, `@parameter` |
| SYNTAX.md | `@derive`, `@test`, `@bench`, `@deprecated`, `@inline` |
| TYPE-SYSTEM.md | `@derive(TypeInfo)`, `@derive(Debug, Eq, Hash, Clone)` |
| MEMORY-MODEL.md | `@no_clone`, `@manual` |
| CONCURRENCY.md | No decorators used (correct) |
| AGENTIC.md | `@derive(Schema)`, `@schema(validate)`, `@circuit_breaker`, `@retry`, `@test` |
| COMPILATION.md | `@wasm_export` |
| TOOLCHAIN.md | `@test`, `@bench`, `@property`, `@test_case` |
| POLYGLOT.md | **FAIL: `#[wasm_export]`, `#[python_module]`, `#[python_fn]`** |
| VARIANTS.md | No decorators used (correct -- code examples are minimal) |
| index.html | `@manual` (correct) |
| getting-started.html | No decorators in examples (correct for intro) |
| docs.html | `@derive(Debug, Eq, Serialize)`, `@derive(Schema)`, `@test`, `@test_case`, `@bench` |

**Result:** 12/13 files consistent. POLYGLOT.md is the sole holdout.

### Arrow Functions vs Pipe Closures
| File | Primary Syntax Used | Consistent With Policy? |
|------|-------------------|------------------------|
| SYNTAX.md | Arrow `(x) => ...` with pipe `\|x\|` documented as shorthand | Yes (defines policy) |
| MEMORY-MODEL.md | Arrow `(p) => ...`, `(r) => ...` | Yes |
| CONCURRENCY.md | Arrow `(url) => ...` in scope, `(t) => ...` in streams | Yes |
| AGENTIC.md | Arrow `(t) => ...` throughout | Yes |
| TOOLCHAIN.md | Arrow `(d) => ...` | Yes |
| index.html | **Pipe `\|u\|` in pipe operator and scope examples** | No |
| docs.html | **Pipe `\|x\|`, `\|s\|`, `\|url\|`, `\|t\|`, `\|a, b\|` in several places** | No |
| getting-started.html | No closures in examples | N/A |

**Result:** Design files are consistent. Showcase pages lag behind.

### `T?` / `T ! E` / `none` / `ok()` / `err()`
| File | Status |
|------|--------|
| All design files | Correct |
| index.html | Correct (Rust column shows `None`/`Some`/`Ok`/`Err` for comparison) |
| getting-started.html | Correct |
| docs.html | **Correct** (fully fixed from Round 3) |

**Result:** Full consistency achieved.

---

## Summary

The Turbo design has reached a high level of maturity and coherence. The score increase from 8.4 to 9.0 reflects three significant additions that close the most important gaps from Round 3: the Error Trait hierarchy in TYPE-SYSTEM.md, string semantics specification, and agent testing patterns in AGENTIC.md. Combined with the full resolution of docs.html's syntax inconsistencies, the design is now internally consistent across all major files.

The remaining issues fall into three categories:
1. **One file with old syntax** (POLYGLOT.md using `#[` instead of `@`) -- a quick fix
2. **Showcase HTML lagging behind design files** (pipe closures and undocumented macro syntax in docs.html) -- moderate fix
3. **Specification gaps** (effect system, structural typing details, macro system, auto-clone + concurrency interaction) -- deeper design work needed for completeness

None of these remaining issues affect the language's core coherence or would block a developer from understanding and using Turbo. They are refinements, not structural problems.

**The single most important fix for Round 5:** Update POLYGLOT.md to use `@` decorator syntax, and update index.html and docs.html to use arrow functions instead of pipe closures in prominent examples.

**Next priorities:**
1. Fix POLYGLOT.md `#[` to `@` syntax (4 instances)
2. Fix docs.html collection examples (`map!{}` / `set![]` to literal syntax)
3. Update index.html and docs.html closures from `|x|` to `(x) =>` in prominent examples
4. Fix CONCURRENCY.md `Arc.new()` to `Shared.new()` or remove the Rust-ism
5. Formally specify the effect system (composition, syntax, inference)
6. Specify structural typing edge cases (collisions, visibility, generics)
7. Design the macro/metaprogramming system
8. Specify the auto-clone + Send/Sync interaction
9. Consolidate or archive VARIANTS.md
10. Add model name validation mechanism to AGENTIC.md
