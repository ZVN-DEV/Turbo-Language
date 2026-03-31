import Link from "next/link";

export default function FunctionsPage() {
  return (
    <article className="text-gray-300 leading-relaxed font-[family-name:var(--font-geist-sans)]">
      <h1 className="text-4xl font-bold text-white mb-4">Functions</h1>
      <p className="text-lg text-gray-400 mb-8">
        Functions are first-class citizens in Turbo. Define them, pass them
        around, and compose them with pipes.
      </p>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Function Definitions
      </h2>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`fn add(a: i64, b: i64) -> i64 {
    a + b
}

fn greet(name: str) {
    print("Hello, {name}!")
}

fn main() {
    print(add(2, 3))    // 5
    greet("world")       // Hello, world!
}`}</code>
      </pre>
      <p className="mb-6">
        The last expression in a function body is its return value -- no{" "}
        <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
          return
        </code>{" "}
        keyword needed (though you can use{" "}
        <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
          return
        </code>{" "}
        for early returns).
      </p>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Expression-Body Functions
      </h2>
      <p className="mb-4">
        When a function body is a single expression, the braces serve as the
        expression block:
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`fn double(x: i64) -> i64 { x * 2 }
fn is_positive(n: i64) -> bool { n > 0 }
fn identity<T>(x: T) -> T { x }`}</code>
      </pre>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Higher-Order Functions
      </h2>
      <p className="mb-4">
        Functions can accept other functions as parameters:
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`fn apply(f: fn(i64) -> i64, x: i64) -> i64 {
    f(x)
}

fn main() {
    let double = |x: i64| -> i64 { x * 2 }
    print(apply(double, 21))    // 42
}`}</code>
      </pre>

      <h2 id="closures" className="text-2xl font-bold text-white mt-10 mb-4">
        Closures
      </h2>
      <p className="mb-4">
        Closures are anonymous functions defined with the{" "}
        <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
          |args| body
        </code>{" "}
        syntax:
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`fn main() {
    let double = |x: i64| -> i64 { x * 2 }
    print(double(5))        // 10

    let add = |a: i64, b: i64| -> i64 { a + b }
    print(add(3, 7))        // 10

    // Closures work with map, filter, reduce
    let nums = [1, 2, 3, 4, 5]
    let doubled = nums.map(|x: i64| -> i64 { x * 2 })
    let evens = nums.filter(|x: i64| -> bool { x % 2 == 0 })
}`}</code>
      </pre>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        The Pipe Operator
      </h2>
      <p className="mb-4">
        The{" "}
        <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
          |&gt;
        </code>{" "}
        pipe operator passes the left-hand value as the first argument to the
        right-hand function:
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`fn count_chars(text: str) -> i64 { len(text) }
fn count_words(text: str) -> i64 {
    let words = split(text, " ")
    len(words)
}

fn main() {
    let text = "Hello Turbo World"

    // Pipe operator for clean data flow
    let chars = text |> count_chars
    let words = text |> count_words
    print("Chars: {chars}, Words: {words}")

    // Chain string operations
    let sample = "  Hello, World!  "
    let cleaned = sample |> trim
    let upper = cleaned |> upper
    print(upper)    // "HELLO, WORLD!"
}`}</code>
      </pre>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">Recursion</h2>
      <p className="mb-4">
        Functions can call themselves recursively:
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`fn fib(n: i64) -> i64 {
    if n <= 1 {
        n
    } else {
        fib(n - 1) + fib(n - 2)
    }
}

fn factorial(n: i64) -> i64 {
    if n <= 1 { 1 }
    else { n * factorial(n - 1) }
}

fn main() {
    print(fib(10))          // 55
    print(factorial(10))    // 3628800
}`}</code>
      </pre>

      <div className="flex gap-4 mt-10">
        <Link
          href="/docs/control-flow"
          className="inline-flex items-center gap-2 bg-[#00ff88] text-[#0a0a0a] font-semibold px-6 py-3 rounded-lg hover:bg-[#00cc6a] transition-colors"
        >
          Next: Control Flow
          <span>&#8594;</span>
        </Link>
      </div>
    </article>
  );
}
