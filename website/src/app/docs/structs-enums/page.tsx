import Link from "next/link";

export default function StructsEnumsPage() {
  return (
    <article className="text-gray-300 leading-relaxed font-[family-name:var(--font-geist-sans)]">
      <h1 className="text-4xl font-bold text-white mb-4">Structs & Enums</h1>
      <p className="text-lg text-gray-400 mb-8">
        Define custom data types with structs and algebraic data types with
        enums.
      </p>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Struct Definitions
      </h2>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`struct Point {
    x: i64,
    y: i64,
}

fn main() {
    let p = Point { x: 3, y: 4 }
    print(p.x)    // 3
    print(p.y)    // 4
}`}</code>
      </pre>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Mutable Struct Fields
      </h2>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`fn main() {
    let mut p = Point { x: 0, y: 0 }
    p.x = 10
    p.y = 20
    print(p.x)    // 10
}`}</code>
      </pre>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Impl Blocks and Methods
      </h2>
      <p className="mb-4">
        Attach methods to structs with{" "}
        <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
          impl
        </code>{" "}
        blocks:
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`struct Rectangle {
    width: f64,
    height: f64,
}

impl Rectangle {
    fn area(self) -> f64 {
        self.width * self.height
    }

    fn is_square(self) -> bool {
        self.width == self.height
    }
}

fn main() {
    let r = Rectangle { width: 10.0, height: 5.0 }
    print(r.area())         // 50.0
    print(r.is_square())    // false
}`}</code>
      </pre>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Generic Structs
      </h2>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`struct Point<T> {
    x: T,
    y: T,
}

fn main() {
    let int_point = Point { x: 1, y: 2 }
    let float_point = Point { x: 1.5, y: 2.5 }
}`}</code>
      </pre>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Derive Attributes
      </h2>
      <p className="mb-4">
        Auto-generate trait implementations with{" "}
        <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
          @derive
        </code>
        :
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`@derive(Eq, Clone, Display)
struct Point { x: i64, y: i64 }

fn main() {
    let a = Point { x: 1, y: 2 }
    let b = Point { x: 1, y: 2 }
    if a == b { print("equal!") }     // @derive(Eq)
    let c = clone(a)                   // @derive(Clone)
    print(a)                           // @derive(Display) -> "Point { x: 1, y: 2 }"
}`}</code>
      </pre>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Enum Definitions
      </h2>
      <p className="mb-4">
        Enums define types with a fixed set of variants. Use the{" "}
        <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
          type
        </code>{" "}
        keyword:
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`type Color {
    Red,
    Green,
    Blue,
}

fn describe(c: Color) -> str {
    match c {
        Red => "red"
        Green => "green"
        Blue => "blue"
    }
}

fn main() {
    let c = Color.Green
    print(describe(c))    // green
}`}</code>
      </pre>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Data-Carrying Variants
      </h2>
      <p className="mb-4">
        Enum variants can carry data, making them algebraic data types:
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`type Shape {
    Circle(f64)
    Rectangle(f64, f64)
}

fn describe(s: Shape) -> str {
    match s {
        Circle(r) => "circle with radius"
        Rectangle(w, h) => "rectangle"
    }
}

type Option<T> {
    Some(T)
    None
}`}</code>
      </pre>

      <h2
        id="error-handling"
        className="text-2xl font-bold text-white mt-10 mb-4"
      >
        Error Handling
      </h2>
      <p className="mb-4">
        Turbo uses Result types{" "}
        <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
          T ! E
        </code>{" "}
        for recoverable errors and Optional types{" "}
        <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
          T?
        </code>{" "}
        for absent values:
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`// Result type: success or error
fn divide(a: f64, b: f64) -> f64 ! str {
    if b == 0.0 {
        return err("division by zero")
    }
    a / b
}

// Propagate errors with ?
fn safe_math(x: f64, y: f64) -> f64 ! str {
    let result = divide(x, y)?
    result * 2.0
}

// Optional: provide a default with ??
fn find_user(id: i64) -> str? {
    // returns None if not found
}

fn main() {
    let name = find_user(42) ?? "anonymous"
}`}</code>
      </pre>

      <div className="flex gap-4 mt-10">
        <Link
          href="/docs/traits-generics"
          className="inline-flex items-center gap-2 bg-[#00ff88] text-[#0a0a0a] font-semibold px-6 py-3 rounded-lg hover:bg-[#00cc6a] transition-colors"
        >
          Next: Traits & Generics
          <span>&#8594;</span>
        </Link>
      </div>
    </article>
  );
}
