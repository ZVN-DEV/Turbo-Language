import type { Metadata } from "next";
import Link from "next/link";

export const metadata: Metadata = {
  title: "Traits & Generics",
  description:
    "Define shared behavior with traits and write polymorphic, reusable code with generics and trait bounds in Turbo's static type system.",
};

export default function TraitsGenericsPage() {
  return (
    <article className="text-gray-300 leading-relaxed font-[family-name:var(--font-geist-sans)]">
      <h1 className="text-4xl font-bold text-white mb-4">Traits & Generics</h1>
      <p className="text-lg text-gray-400 mb-8">
        Define shared behavior with traits and write polymorphic code with
        generics.
      </p>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Trait Definitions
      </h2>
      <p className="mb-4">
        Traits define a set of methods that types can implement:
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`trait Display {
    fn to_string(self) -> str
}

trait Area {
    fn area(self) -> f64
}`}</code>
      </pre>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Implementing Traits
      </h2>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`struct Circle {
    radius: f64,
}

impl Area for Circle {
    fn area(self) -> f64 {
        3.14159 * self.radius * self.radius
    }
}

impl Display for Circle {
    fn to_string(self) -> str {
        "Circle"
    }
}

fn main() {
    let c = Circle { radius: 5.0 }
    print(c.area())          // 78.53975
    print(c.to_string())    // Circle
}`}</code>
      </pre>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Default Methods
      </h2>
      <p className="mb-4">
        Traits can provide default implementations:
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`trait Describable {
    fn name(self) -> str
    fn describe(self) -> str {
        "I am a " + self.name()
    }
}`}</code>
      </pre>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Generic Functions
      </h2>
      <p className="mb-4">
        Use type parameters to write functions that work with any type:
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`fn identity<T>(x: T) -> T { x }

fn main() {
    print(identity(42))        // 42
    print(identity("hello"))   // hello
    print(identity(true))      // true
}`}</code>
      </pre>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Generic Structs
      </h2>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`struct Pair<A, B> {
    first: A,
    second: B,
}

struct Point<T> {
    x: T,
    y: T,
}

type Option<T> {
    Some(T)
    None
}`}</code>
      </pre>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Trait Bounds
      </h2>
      <p className="mb-4">
        Constrain type parameters to types that implement specific traits:
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`fn print_area<T: Area>(shape: T) {
    print(shape.area())
}

fn describe<T: Display>(item: T) -> str {
    item.to_string()
}`}</code>
      </pre>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Built-in Derive Traits
      </h2>
      <div className="overflow-x-auto mb-8">
        <table className="w-full text-sm text-left border border-[#1a1a2e] rounded-lg overflow-hidden">
          <thead className="bg-[#111118] text-gray-400">
            <tr>
              <th className="px-4 py-2 border-b border-[#1a1a2e]">Trait</th>
              <th className="px-4 py-2 border-b border-[#1a1a2e]">Effect</th>
              <th className="px-4 py-2 border-b border-[#1a1a2e]">Usage</th>
            </tr>
          </thead>
          <tbody>
            {[
              ["Eq", "Enables == and != comparison", "a == b"],
              ["Clone", "Enables clone() function", "clone(a)"],
              ["Display", "Enables print() and to_string()", "print(a)"],
            ].map(([trait_, effect, usage]) => (
              <tr key={trait_} className="border-b border-[#1a1a2e]">
                <td className="px-4 py-2">
                  <code className="text-[#00ff88] font-[family-name:var(--font-geist-mono)]">{trait_}</code>
                </td>
                <td className="px-4 py-2">{effect}</td>
                <td className="px-4 py-2">
                  <code className="text-gray-400 font-[family-name:var(--font-geist-mono)] text-xs">{usage}</code>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      <div className="flex gap-4 mt-10">
        <Link
          href="/docs/async"
          className="inline-flex items-center gap-2 bg-[#00ff88] text-[#0a0a0a] font-semibold px-6 py-3 rounded-lg hover:bg-[#00cc6a] transition-colors"
        >
          Next: Async & Concurrency
          <span>&#8594;</span>
        </Link>
      </div>
    </article>
  );
}
