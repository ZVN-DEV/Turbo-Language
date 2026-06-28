import type { Metadata } from "next";
import Link from "next/link";

export const metadata: Metadata = {
  title: "Control Flow",
  description:
    "Turbo's control flow is expression-based: if/else and while return values, and exhaustive match enables pattern matching you can bind directly.",
};

export default function ControlFlowPage() {
  return (
    <article className="text-gray-300 leading-relaxed font-[family-name:var(--font-geist-sans)]">
      <h1 className="text-4xl font-bold text-white mb-4">Control Flow</h1>
      <p className="text-lg text-gray-400 mb-8">
        In Turbo, control flow constructs are expressions that return values.
      </p>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        If / Else Expressions
      </h2>
      <p className="mb-4">
        <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
          if
        </code>{" "}
        is an expression -- it returns the value of whichever branch executes:
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`fn classify(n: i64) -> str {
    if n > 0 { "positive" }
    else { if n < 0 { "negative" } else { "zero" } }
}

fn main() {
    let x = 10
    let label = if x > 5 { "big" } else { "small" }
    print(label)    // big
}`}</code>
      </pre>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        While Loops
      </h2>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`fn main() {
    let mut i = 0
    while i < 5 {
        print(i)
        i += 1
    }
}`}</code>
      </pre>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        For-In Loops and Ranges
      </h2>
      <p className="mb-4">
        Iterate over arrays or ranges with{" "}
        <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
          for..in
        </code>
        :
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`fn main() {
    // Range: 0, 1, 2, 3, 4
    for i in 0..5 {
        print(i)
    }

    // Iterate over an array
    let names = ["Alice", "Bob", "Charlie"]
    for name in names {
        print("Hello, {name}!")
    }
}`}</code>
      </pre>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Break and Continue
      </h2>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`fn main() {
    let mut i = 0
    while i < 100 {
        if i == 5 {
            break               // exit the loop
        }
        if i % 2 == 0 {
            i += 1
            continue            // skip to next iteration
        }
        print(i)
        i += 1
    }
}`}</code>
      </pre>

      <h2
        id="pattern-matching"
        className="text-2xl font-bold text-white mt-10 mb-4"
      >
        Match Expressions
      </h2>
      <p className="mb-4">
        <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
          match
        </code>{" "}
        is a powerful expression for pattern matching on values:
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`fn describe(n: i64) -> str {
    match n {
        0 => "zero"
        1 => "one"
        2 => "two"
        _ => "many"
    }
}

fn main() {
    print(describe(0))     // zero
    print(describe(1))     // one
    print(describe(42))    // many
}`}</code>
      </pre>

      <h3 className="text-xl font-bold text-white mt-8 mb-4">Match Guards</h3>
      <p className="mb-4">
        Add conditions to match arms with{" "}
        <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
          if
        </code>{" "}
        guards:
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`fn classify(n: i64) -> str {
    match n {
        0 => "zero"
        n if n > 0 => "positive"
        _ => "negative"
    }
}

fn main() {
    print(classify(5))     // positive
    print(classify(0))     // zero
    print(classify(-3))    // negative
}`}</code>
      </pre>

      <h3 className="text-xl font-bold text-white mt-8 mb-4">
        Pattern Matching on Enums
      </h3>
      <p className="mb-4">
        Match expressions destructure enum variants:
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`type Shape {
    Circle(f64)
    Rectangle(f64, f64)
}

fn area(s: Shape) -> str {
    match s {
        Circle(r) => "circle"
        Rectangle(w, h) => "rectangle"
    }
}

fn main() {
    let s = Shape.Circle(5.0)
    print(area(s))    // circle
}`}</code>
      </pre>
      <p className="text-sm text-gray-500 mb-6">
        The compiler checks that match expressions are exhaustive -- if you miss
        a variant, you get a compile error (E0200).
      </p>

      <div className="flex gap-4 mt-10">
        <Link
          href="/docs/structs-enums"
          className="inline-flex items-center gap-2 bg-[#00ff88] text-[#0a0a0a] font-semibold px-6 py-3 rounded-lg hover:bg-[#00cc6a] transition-colors"
        >
          Next: Structs & Enums
          <span>&#8594;</span>
        </Link>
      </div>
    </article>
  );
}
