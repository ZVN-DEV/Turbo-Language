import Link from "next/link";

export default function DocsPage() {
  return (
    <article className="text-gray-300 leading-relaxed font-[family-name:var(--font-geist-sans)]">
      <h1 className="text-4xl font-bold text-white mb-4">
        Introduction to Turbo
      </h1>
      <p className="text-lg text-gray-400 mb-8">
        A compiled, type-safe programming language with JavaScript&apos;s developer
        experience, Rust&apos;s performance, and first-class AI agent primitives.
      </p>

      <div className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-6 mb-8">
        <p className="text-[#00ff88] font-medium text-lg mb-0">
          JavaScript&apos;s soul. Rust&apos;s speed. Built for the AI age.
        </p>
      </div>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        What is Turbo?
      </h2>
      <p className="mb-4">
        Turbo compiles directly to machine code using Cranelift. No interpreter,
        no VM, no garbage collector. Programs start instantly and run at native
        speed. It features strong static typing with type inference, generics,
        traits, and algebraic data types -- all while keeping a clean,
        approachable syntax.
      </p>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Key Features
      </h2>
      <ul className="list-disc list-inside space-y-2 mb-6 text-gray-300">
        <li>
          <strong className="text-white">Native compilation</strong> -- JIT via{" "}
          <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">turbo run</code>,
          AOT via{" "}
          <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">turbo build</code>
        </li>
        <li>
          <strong className="text-white">Type-safe</strong> -- Generics, traits,
          pattern matching, Result/Optional types
        </li>
        <li>
          <strong className="text-white">Thread-based concurrency</strong> -- spawn,
          await, channels, mutex
        </li>
        <li>
          <strong className="text-white">AI agent primitives</strong> --{" "}
          <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">agent</code> and{" "}
          <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">tool fn</code>{" "}
          keywords
        </li>
        <li>
          <strong className="text-white">Modern toolchain</strong> -- built-in
          test runner, formatter, REPL, LSP, package manager
        </li>
        <li>
          <strong className="text-white">Tiny binaries</strong> -- ~55 KB for a
          hello world, no runtime dependencies
        </li>
      </ul>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        A Quick Taste
      </h2>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`fn fib(n: i64) -> i64 {
    if n <= 1 {
        n
    } else {
        fib(n - 1) + fib(n - 2)
    }
}

fn main() {
    let mut i = 0
    while i <= 15 {
        print(fib(i))
        i += 1
    }
}`}</code>
      </pre>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Who is Turbo for?
      </h2>
      <ul className="list-disc list-inside space-y-2 mb-8 text-gray-300">
        <li>Developers who want native performance without Rust&apos;s complexity</li>
        <li>Teams building AI-powered applications with agents and tools</li>
        <li>Anyone who wants a modern language with batteries included</li>
        <li>Systems programmers who appreciate clean, expressive syntax</li>
      </ul>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Performance
      </h2>
      <p className="mb-4">
        Benchmarked on Apple Silicon (fib(40), recursive):
      </p>
      <div className="overflow-x-auto mb-8">
        <table className="w-full text-sm text-left border border-[#1a1a2e] rounded-lg overflow-hidden">
          <thead className="bg-[#111118] text-gray-400">
            <tr>
              <th className="px-4 py-2 border-b border-[#1a1a2e]">Language</th>
              <th className="px-4 py-2 border-b border-[#1a1a2e]">Time</th>
              <th className="px-4 py-2 border-b border-[#1a1a2e]">Binary Size</th>
            </tr>
          </thead>
          <tbody>
            {[
              ["C (gcc -O2)", "170ms", "33 KB"],
              ["Rust (rustc -O)", "180ms", "441 KB"],
              ["Turbo (Cranelift)", "220ms", "55 KB"],
              ["Node.js", "580ms", "N/A"],
              ["Python", "13.1s", "N/A"],
            ].map(([lang, time, size]) => (
              <tr key={lang} className="border-b border-[#1a1a2e]">
                <td className={`px-4 py-2 ${lang?.startsWith("Turbo") ? "text-[#00ff88] font-medium" : "text-gray-300"}`}>
                  {lang}
                </td>
                <td className="px-4 py-2 text-gray-300">{time}</td>
                <td className="px-4 py-2 text-gray-300">{size}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      <div className="flex gap-4 mt-8">
        <Link
          href="/docs/installation"
          className="inline-flex items-center gap-2 bg-[#00ff88] text-[#0a0a0a] font-semibold px-6 py-3 rounded-lg hover:bg-[#00cc6a] transition-colors"
        >
          Get Started
          <span>&#8594;</span>
        </Link>
        <Link
          href="/docs/hello-world"
          className="inline-flex items-center gap-2 border border-[#1a1a2e] text-gray-300 px-6 py-3 rounded-lg hover:border-[#00ff88] hover:text-[#00ff88] transition-colors"
        >
          Hello World
        </Link>
      </div>
    </article>
  );
}
