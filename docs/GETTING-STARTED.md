# Getting Started with Turbo

A 15-minute tutorial for developers who just installed TurboLang. By the end, you will have written, run, tested, and built a small project.

---

## 1. Installation

### Homebrew (recommended, macOS / Linux)

```bash
brew tap ZVN-DEV/turbo && brew install turbo-lang
```

### Build from Source

```bash
git clone https://github.com/ZVN-DEV/Turbo-Language.git
cd Turbo-Language/turbo
cargo build --release -p turbo-cli
export PATH="$PWD/target/release:$PATH"
```

### Curl Installer

```bash
curl -fsSL https://raw.githubusercontent.com/ZVN-DEV/Turbo-Language/master/distribution/install.sh | sh
```

### Docker

```bash
docker build -t turbo -f distribution/Dockerfile .
docker run --rm turbo turbolang run hello.tb
```

Verify your installation:

```bash
turbolang --version
```

---

## 2. Hello World

Create a file called `hello.tb`:

```turbo
fn main() {
    print("Hello, world!")
}
```

Run it:

```bash
turbolang run hello.tb
```

Output:

```
Hello, world!
```

Every Turbo program starts with a `fn main()` function. `print` outputs any value followed by a newline. No imports are needed -- all built-in functions are available by default.

---

## 3. Variables and Types

Create `variables.tb`:

```turbo
fn main() {
    // Immutable by default -- type is inferred
    let name = "Turbo"
    let year = 2026
    let pi = 3.14159

    // String interpolation uses {} inside double quotes
    print("Language: {name}")
    print("Year: {year}")
    print("Pi: {pi}")

    // Mutable variables use `let mut`
    let mut count = 0
    count = count + 1
    count += 1
    print("Count: {count}")

    // Explicit type annotations (optional when type can be inferred)
    let x: i64 = 42
    let flag: bool = true
    print("{x}, {flag}")
}
```

```bash
turbolang run variables.tb
```

### Type Summary

| Type | Description | Example |
|------|-------------|---------|
| `i64` | 64-bit signed integer (default integer type, alias: `int`) | `42` |
| `f64` | 64-bit float (default float type, alias: `float`) | `3.14` |
| `bool` | Boolean | `true`, `false` |
| `str` | UTF-8 string | `"hello"` |
| `[T]` | Array of T | `[1, 2, 3]` |
| `T?` | Optional (some or none) | `some(42)`, `none` |
| `T ! E` | Result (ok or err) | `ok(42)`, `err("fail")` |

Turbo also has sized integer types (`i8`, `i16`, `i32`, `u8`, `u16`, `u32`, `u64`, `usize`) and `f32` for low-level control.

---

## 4. Functions

Create `functions.tb`:

```turbo
fn add(a: i64, b: i64) -> i64 {
    a + b
}

fn greet(name: str) -> str {
    "Hello, {name}!"
}

fn fib(n: i64) -> i64 {
    if n <= 1 {
        n
    } else {
        fib(n - 1) + fib(n - 2)
    }
}

fn main() {
    print(add(3, 7))
    print(greet("developer"))
    print("fib(10) = {fib(10)}")
}
```

```bash
turbolang run functions.tb
```

Output:

```
10
Hello, developer!
fib(10) = 55
```

Functions require type annotations on parameters and return types. The last expression in a function body is the return value -- no `return` keyword needed (though `return` is available for early exits).

---

## 5. Structs and Methods

Create `structs.tb`:

```turbo
struct Rect {
    w: i64,
    h: i64,
}

impl Rect {
    fn area(self) -> i64 {
        self.w * self.h
    }

    fn perimeter(self) -> i64 {
        2 * (self.w + self.h)
    }

    fn describe(self) -> str {
        "{self.w}x{self.h} (area={self.area()})"
    }
}

fn main() {
    let r = Rect { w: 5, h: 3 }
    print("Area: {r.area()}")
    print("Perimeter: {r.perimeter()}")
    print(r.describe())
}
```

```bash
turbolang run structs.tb
```

Output:

```
Area: 15
Perimeter: 16
5x3 (area=15)
```

Struct fields are separated by commas. Methods are defined in `impl` blocks. The first parameter `self` gives access to the struct's fields.

---

## 6. Enums and Pattern Matching

Create `enums.tb`:

```turbo
type Shape {
    Circle(f64),
    Rectangle(f64, f64),
    Triangle(f64, f64, f64),
}

fn describe(s: Shape) -> str {
    match s {
        Circle(r) => "circle with radius {r}"
        Rectangle(w, h) => "rectangle {w}x{h}"
        Triangle(a, b, c) => "triangle with sides {a},{b},{c}"
    }
}

fn main() {
    let s1 = Shape.Circle(3.14)
    let s2 = Shape.Rectangle(5.0, 3.0)

    print(describe(s1))
    print(describe(s2))

    // Match on integers with guards and wildcards
    let n = 42
    let label = match n {
        0 => "zero"
        n if n > 0 => "positive"
        _ => "negative"
    }
    print("{n} is {label}")
}
```

```bash
turbolang run enums.tb
```

Enums in Turbo are defined with `type` and can carry data. The `match` expression must be exhaustive -- the compiler rejects matches that do not cover every variant.

---

## 7. Error Handling

Create `errors.tb`:

```turbo
fn divide(a: i64, b: i64) -> i64 ! str {
    if b == 0 {
        err("division by zero")
    } else {
        ok(a / b)
    }
}

fn main() {
    // Handle results with match
    match divide(10, 3) {
        ok(v) => print("10 / 3 = {v}")
        err(e) => print("error: {e}")
    }

    match divide(10, 0) {
        ok(v) => print("10 / 0 = {v}")
        err(e) => print("error: {e}")
    }

    // Fallible file I/O with try_read_file
    let result = try_read_file("/tmp/nonexistent.txt")
    match result {
        ok(contents) => print("File: {contents}")
        err(e) => print("Could not read file: {e}")
    }
}
```

```bash
turbolang run errors.tb
```

Output:

```
10 / 3 = 3
error: division by zero
Could not read file: ...
```

The `T ! E` syntax means "returns T on success or E on failure." Use `ok(value)` for success and `err(message)` for failure. Handle results with `match`.

---

## 8. Collections

Create `collections.tb`:

```turbo
fn main() {
    // Arrays
    let nums = [10, 20, 30, 40, 50]
    print("Length: {len(nums)}")
    print("First: {nums[0]}")
    print("Last: {nums[4]}")

    // Iterating
    for n in nums {
        print("  {n}")
    }

    // Functional operations (copy-on-write -- returns new arrays)
    let doubled = nums.map(|x: i64| -> i64 { x * 2 })
    print("Doubled: {doubled}")

    let big = nums.filter(|x: i64| -> bool { x > 25 })
    print("Big: {big}")

    let total = reduce(nums, 0, |acc: i64, x: i64| -> i64 { acc + x })
    print("Sum: {total}")

    // Push returns a new array (COW semantics)
    let mut arr = [1, 2, 3]
    arr.push(4)
    print("After push: {arr}")

    // HashMaps
    let m = hashmap()
    hashmap_set(m, "lang", "Turbo")
    hashmap_set(m, "version", "0.8")
    print("Language: {hashmap_get(m, "lang")}")
    print("Has version: {hashmap_has(m, "version")}")
    print("Keys: {hashmap_keys(m)}")

    // Integer-valued hashmaps
    let mut counts = hashmap()
    counts = hashmap_set_int(counts, "apples", 3)
    counts = hashmap_set_int(counts, "bananas", 5)
    print("Apples: {hashmap_get_int(counts, "apples")}")
}
```

```bash
turbolang run collections.tb
```

Note: `push`, `map`, `filter`, `trim`, `upper`, `lower`, `replace`, `repeat`, and `split` are copy-on-write -- they return new values. When used as a statement (`arr.push(4)`), the parser automatically rewrites this to `arr = push(arr, 4)`.

---

## 9. Building a Small Project

Let us build a text statistics analyzer using the full project workflow.

### Create the project

```bash
turbolang init text-stats
cd text-stats
```

This creates a project directory with a `turbo.toml` and `src/main.tb`.

### Write the program

Replace the contents of `src/main.tb`:

```turbo
fn count_words(text: str) -> i64 {
    let words = split(text, " ")
    let mut count = 0
    for w in words {
        let trimmed = trim(w)
        if len(trimmed) > 0 {
            count += 1
        }
    }
    count
}

fn count_lines(text: str) -> i64 {
    len(split(text, "\n"))
}

fn find_longest(text: str) -> str {
    let words = split(text, " ")
    let mut longest = ""
    for w in words {
        let cleaned = lower(trim(w))
        if len(cleaned) > len(longest) {
            longest = cleaned
        }
    }
    longest
}

fn word_frequency(text: str) {
    let words = split(lower(text), " ")
    let mut freq = hashmap()
    for w in words {
        let cleaned = trim(w)
        if len(cleaned) > 0 {
            if hashmap_has(freq, cleaned) {
                freq = hashmap_set_int(freq, cleaned, hashmap_get_int(freq, cleaned) + 1)
            } else {
                freq = hashmap_set_int(freq, cleaned, 1)
            }
        }
    }
    let keys = hashmap_keys(freq)
    for k in keys {
        print("  {k}: {hashmap_get_int(freq, k)}")
    }
}

fn main() {
    let text = "the quick brown fox jumps over the lazy dog the fox"

    print("=== Text Statistics ===")
    print("Characters: {len(text)}")
    print("Words: {count_words(text)}")
    print("Lines: {count_lines(text)}")
    print("Longest word: {find_longest(text)}")
    print("")
    print("Word frequency:")
    word_frequency(text)
}
```

### Run it

```bash
turbolang run src/main.tb
```

### Add tests

Create a test file `tests/stats_test.tb`:

```turbo
fn count_words(text: str) -> i64 {
    let words = split(text, " ")
    let mut count = 0
    for w in words {
        let trimmed = trim(w)
        if len(trimmed) > 0 {
            count += 1
        }
    }
    count
}

@test fn test_count_words() {
    assert_eq(count_words("hello world"), 2)
    assert_eq(count_words("one"), 1)
    assert_eq(count_words("a b c d e"), 5)
}

@test fn test_empty_string() {
    assert_eq(count_words(""), 0)
}
```

Run the tests:

```bash
turbolang test tests/stats_test.tb
```

Expected output:

```
  PASS  test_count_words
  PASS  test_empty_string
2 passed, 0 failed
```

### Build a release binary

```bash
turbolang build src/main.tb -o text-stats
./text-stats
```

The resulting binary is a standalone native executable -- no runtime dependencies, no VM, no interpreter.

---

## 10. Next Steps

### Language References

- **[Standard Library Reference](stdlib.md)** -- all 64+ built-in functions with examples
- **[Error Code Reference](errors.md)** -- every compiler error code explained
- **[Safety Narrative](SAFETY.md)** -- what Turbo guarantees and what it does not

### Language Design

- **[Syntax](../design/SYNTAX.md)** -- full syntax reference
- **[Type System](../design/TYPE-SYSTEM.md)** -- generics, traits, algebraic types
- **[Memory Model](../design/MEMORY-MODEL.md)** -- CoW semantics, future plans
- **[Concurrency](../design/CONCURRENCY.md)** -- async/await, spawn, channels

### Examples

- **[examples/web-dashboard/](../examples/web-dashboard/)** -- interactive web app with JSON API
- **[examples/simple-script/](../examples/simple-script/)** -- text statistics analyzer
- **[examples/speed-server/](../examples/speed-server/)** -- REST API benchmark server

### Tooling

| Command | What it does |
|---------|-------------|
| `turbolang run file.tb` | Compile and run via JIT |
| `turbolang build file.tb` | Compile to native binary |
| `turbolang test file.tb` | Run `@test` functions |
| `turbolang check file.tb` | Type-check without running |
| `turbolang fmt file-or-dir` | Format source code |
| `turbolang repl` | Interactive REPL |
| `turbolang playground` | Browser-based playground |
| `turbolang explain E0100` | Explain an error code |
| `turbolang lsp` | Start Language Server (for editor integration) |

### Editor Support

Install the VS Code extension `zvndev.turbo-lang` for syntax highlighting, snippets, and LSP integration (diagnostics, hover, go-to-definition).
