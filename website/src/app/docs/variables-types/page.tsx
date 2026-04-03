import Link from "next/link";

export default function VariablesTypesPage() {
  return (
    <article className="text-gray-300 leading-relaxed font-[family-name:var(--font-geist-sans)]">
      <h1 className="text-4xl font-bold text-white mb-4">Variables & Types</h1>
      <p className="text-lg text-gray-400 mb-8">
        Turbo is statically typed with type inference. You get safety without
        verbosity.
      </p>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Variable Declarations
      </h2>
      <p className="mb-4">
        Use{" "}
        <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
          let
        </code>{" "}
        to declare immutable variables and{" "}
        <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
          let mut
        </code>{" "}
        for mutable ones:
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`fn main() {
    let name = "Turbo"          // immutable, type inferred as str
    let age: i64 = 1            // explicit type annotation
    let mut counter = 0         // mutable
    counter += 1                // OK -- counter is mutable

    // name = "other"           // ERROR: cannot assign to immutable variable
}`}</code>
      </pre>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Primitive Types
      </h2>
      <div className="overflow-x-auto mb-8">
        <table className="w-full text-sm text-left border border-[#1a1a2e] rounded-lg overflow-hidden">
          <thead className="bg-[#111118] text-gray-400">
            <tr>
              <th className="px-4 py-2 border-b border-[#1a1a2e]">Type</th>
              <th className="px-4 py-2 border-b border-[#1a1a2e]">Description</th>
              <th className="px-4 py-2 border-b border-[#1a1a2e]">Example</th>
            </tr>
          </thead>
          <tbody>
            {[
              ["i32", "32-bit signed integer", "let x: i32 = 42"],
              ["i64", "64-bit signed integer", "let x = 42"],
              ["u32", "32-bit unsigned integer", "let x: u32 = 42"],
              ["u64", "64-bit unsigned integer", "let x: u64 = 42"],
              ["f32", "32-bit float", "let x: f32 = 3.14"],
              ["f64", "64-bit float", "let x = 3.14"],
              ["bool", "Boolean", "let x = true"],
              ["str", "String", 'let x = "hello"'],
              ["()", "Unit (void)", "fn noop() { }"],
            ].map(([type, desc, example]) => (
              <tr key={type} className="border-b border-[#1a1a2e]">
                <td className="px-4 py-2">
                  <code className="text-[#00ff88] font-[family-name:var(--font-geist-mono)]">{type}</code>
                </td>
                <td className="px-4 py-2">{desc}</td>
                <td className="px-4 py-2">
                  <code className="text-gray-400 font-[family-name:var(--font-geist-mono)] text-xs">{example}</code>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Type Inference
      </h2>
      <p className="mb-4">
        Turbo infers types from context. You rarely need to write type
        annotations:
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`fn main() {
    let x = 42              // inferred as i64
    let y = 3.14            // inferred as f64
    let name = "hello"      // inferred as str
    let flag = true         // inferred as bool
    let nums = [1, 2, 3]   // inferred as [i64]
}`}</code>
      </pre>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        String Interpolation
      </h2>
      <p className="mb-4">
        Embed expressions inside strings using curly braces:
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`fn main() {
    let name = "Turbo"
    let version = 1
    print("Welcome to {name} v{version}!")
    // Output: Welcome to Turbo v1!
}`}</code>
      </pre>
      <p className="mb-6 text-sm text-gray-400">
        <strong className="text-gray-300">Note:</strong> Expressions inside{" "}
        <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
          {"{...}"}
        </code>{" "}
        cannot contain quoted strings. If you need to call a function with a
        string argument, assign it to a variable first:{" "}
        <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
          let val = greet(&quot;world&quot;)
        </code>{" "}
        then use{" "}
        <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
          {'"Result: {val}"'}
        </code>
        .
      </p>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">Constants</h2>
      <p className="mb-4">
        Top-level constants are declared with{" "}
        <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
          const
        </code>
        :
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`const MAX_SIZE: i64 = 1024
const PI: f64 = 3.14159

fn main() {
    print(MAX_SIZE)
    print(PI)
}`}</code>
      </pre>

      <h2 id="collections" className="text-2xl font-bold text-white mt-10 mb-4">
        Arrays
      </h2>
      <p className="mb-4">
        Arrays are ordered, homogeneous collections:
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`fn main() {
    let nums = [1, 2, 3, 4, 5]
    print(nums[0])              // 1
    print(len(nums))            // 5

    let mut items = [10, 20, 30]
    items[0] = 99               // mutation requires let mut
    push(items, 40)             // append to array
    print(items.len())          // 4
}`}</code>
      </pre>

      <h3 className="text-xl font-bold text-white mt-8 mb-4">
        Array Operations
      </h3>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`fn main() {
    let nums = [3, 1, 4, 1, 5, 9, 2, 6]

    // map, filter, reduce
    let doubled = nums.map(|x: i64| -> i64 { x * 2 })
    let big = nums.filter(|x: i64| -> bool { x > 4 })
    let sum = reduce(nums, 0, |acc: i64, x: i64| -> i64 { acc + x })
    print(sum)
}`}</code>
      </pre>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Hashmaps
      </h2>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`fn main() {
    let m = hashmap()
    hashmap_set(m, "name", "Turbo")
    hashmap_set(m, "version", "1")
    print(hashmap_get(m, "name"))       // Turbo
    print(hashmap_has(m, "version"))    // true
    print(hashmap_len(m))               // 2

    let keys = hashmap_keys(m)
    print(keys.len())                   // 2
}`}</code>
      </pre>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Optionals and Results
      </h2>
      <p className="mb-4">
        Turbo has built-in optional and result types for safe error handling:
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`// Optional: T? represents a value that may be absent
fn find(items: [str], target: str) -> str? {
    // ...
}

// Unwrap with ?? (default value)
let result = find(items, "key") ?? "not found"

// Result: T ! E represents success or error
fn parse(input: str) -> i64 ! str {
    // ...
}

// Propagate errors with ?
fn process() -> i64 ! str {
    let value = parse("42")?    // returns error if parse fails
    value * 2
}`}</code>
      </pre>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Copy-on-Write Semantics
      </h2>
      <p className="mb-4">
        Turbo uses copy-on-write for safe value semantics without a garbage
        collector:
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`fn main() {
    let a = [1, 2, 3]
    let b = a           // shared (cheap)
    b[0] = 99           // copy-on-write (safe)
    print(a[0])          // 1 -- original unchanged
    print(b[0])          // 99 -- independent copy
}`}</code>
      </pre>

      <div className="flex gap-4 mt-10">
        <Link
          href="/docs/functions"
          className="inline-flex items-center gap-2 bg-[#00ff88] text-[#0a0a0a] font-semibold px-6 py-3 rounded-lg hover:bg-[#00cc6a] transition-colors"
        >
          Next: Functions
          <span>&#8594;</span>
        </Link>
      </div>
    </article>
  );
}
