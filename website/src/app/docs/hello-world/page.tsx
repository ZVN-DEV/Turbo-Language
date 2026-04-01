import Link from "next/link";

export default function HelloWorldPage() {
  return (
    <article className="text-gray-300 leading-relaxed font-[family-name:var(--font-geist-sans)]">
      <h1 className="text-4xl font-bold text-white mb-4">Hello World</h1>
      <p className="text-lg text-gray-400 mb-8">
        Write, run, and compile your first Turbo program.
      </p>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Create a Source File
      </h2>
      <p className="mb-4">
        Create a file called{" "}
        <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
          hello.tb
        </code>{" "}
        with the following content:
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`fn main() {
    print("Hello, world!")
}`}</code>
      </pre>
      <p className="mb-6">
        Every Turbo program needs a{" "}
        <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
          main
        </code>{" "}
        function as its entry point. The{" "}
        <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
          print
        </code>{" "}
        function is a built-in that outputs to stdout.
      </p>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Run with JIT
      </h2>
      <p className="mb-4">
        Use{" "}
        <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
          turbo run
        </code>{" "}
        to compile and execute in a single step using the JIT compiler:
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-2 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`$ turbo run hello.tb
Hello, world!`}</code>
      </pre>
      <p className="text-sm text-gray-500 mb-6">
        The JIT (Just-In-Time) compiler uses Cranelift to compile your code to
        machine code in memory and runs it immediately. This is the fastest way
        to iterate during development.
      </p>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Build a Native Binary
      </h2>
      <p className="mb-4">
        Use{" "}
        <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
          turbo build
        </code>{" "}
        to compile to a standalone native executable:
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-2 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`$ turbo build hello.tb
$ ./hello
Hello, world!`}</code>
      </pre>
      <p className="text-sm text-gray-500 mb-6">
        AOT (Ahead-Of-Time) compilation produces a self-contained binary with no
        runtime dependencies. The binary is linked with a small C runtime for
        I/O and memory operations.
      </p>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        JIT vs AOT
      </h2>
      <div className="overflow-x-auto mb-8">
        <table className="w-full text-sm text-left border border-[#1a1a2e] rounded-lg overflow-hidden">
          <thead className="bg-[#111118] text-gray-400">
            <tr>
              <th className="px-4 py-2 border-b border-[#1a1a2e]">Mode</th>
              <th className="px-4 py-2 border-b border-[#1a1a2e]">Command</th>
              <th className="px-4 py-2 border-b border-[#1a1a2e]">Use Case</th>
            </tr>
          </thead>
          <tbody>
            <tr className="border-b border-[#1a1a2e]">
              <td className="px-4 py-2 text-white font-medium">JIT</td>
              <td className="px-4 py-2">
                <code className="text-[#00ff88] font-[family-name:var(--font-geist-mono)] text-xs">turbo run</code>
              </td>
              <td className="px-4 py-2">Development, rapid iteration</td>
            </tr>
            <tr className="border-b border-[#1a1a2e]">
              <td className="px-4 py-2 text-white font-medium">AOT (Cranelift)</td>
              <td className="px-4 py-2">
                <code className="text-[#00ff88] font-[family-name:var(--font-geist-mono)] text-xs">turbo build</code>
              </td>
              <td className="px-4 py-2">Production binaries (default)</td>
            </tr>
            <tr className="border-b border-[#1a1a2e]">
              <td className="px-4 py-2 text-white font-medium">AOT (LLVM)</td>
              <td className="px-4 py-2">
                <code className="text-[#00ff88] font-[family-name:var(--font-geist-mono)] text-xs">turbo build --llvm</code>
              </td>
              <td className="px-4 py-2">Production binaries (requires LLVM 18)</td>
            </tr>
          </tbody>
        </table>
      </div>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        A Slightly Bigger Example
      </h2>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`fn abs(x: i64) -> i64 {
    if x < 0 { 0 - x } else { x }
}

fn clamp(val: i64, lo: i64, hi: i64) -> i64 {
    if val < lo { lo }
    else { if val > hi { hi } else { val } }
}

fn main() {
    assert(abs(-42) == 42)
    assert(clamp(150, 0, 100) == 100)
    print("All checks passed!")
}`}</code>
      </pre>
      <p className="mb-6">
        Notice how{" "}
        <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
          if
        </code>{" "}
        is an expression that returns a value -- no need for ternary operators or
        explicit return statements.
      </p>

      <div className="flex gap-4 mt-10">
        <Link
          href="/docs/variables-types"
          className="inline-flex items-center gap-2 bg-[#00ff88] text-[#0a0a0a] font-semibold px-6 py-3 rounded-lg hover:bg-[#00cc6a] transition-colors"
        >
          Next: Variables & Types
          <span>&#8594;</span>
        </Link>
      </div>
    </article>
  );
}
