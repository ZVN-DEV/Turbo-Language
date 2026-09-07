import type { Metadata } from "next";
import Link from "next/link";

export const metadata: Metadata = {
  title: "Introduction",
  description:
    "Familiar code. Native execution. A path to deeper control. Turbo is a compiled, type-safe language with TypeScript- and JavaScript-like ergonomics.",
};

export default function DocsPage() {
  return (
    <article className="text-gray-300 leading-relaxed font-[family-name:var(--font-geist-sans)]">
      <h1 className="text-4xl font-bold text-white mb-4">
        Introduction to Turbo
      </h1>
      <p className="text-lg text-gray-400 mb-8">
        A compiled, type-safe programming language with JavaScript&apos;s developer
        experience, native execution, and a modern built-in toolchain.
      </p>

      <div className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-6 mb-8">
        <p className="text-[#00ff88] font-medium text-lg mb-0">
          Familiar code. Native execution. A path to deeper control.
        </p>
      </div>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        What is Turbo?
      </h2>
      <p className="mb-4">
        Turbo compiles directly to machine code using Cranelift. No interpreter,
        no VM, and no tracing garbage collector. It features strong static
        typing with type inference, generics, traits, and algebraic data types
        while keeping a clean, approachable syntax. Performance work is measured
        publicly against Rust, with current gaps and targets called out instead
        of hidden behind a slogan.
      </p>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Key Features
      </h2>
      <ul className="list-disc list-inside space-y-2 mb-6 text-gray-300">
        <li>
          <strong className="text-white">Native compilation</strong> -- JIT via{" "}
          <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">turbolang run</code>,
          AOT via{" "}
          <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">turbolang build</code>
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
          <strong className="text-white">Small, honest core</strong> -- Turbo
          keeps the compiler focused on a general-purpose language.
          Agent/tool workflows will ship in a separate{" "}
          <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">turbo-agent</code>{" "}
          library after 1.0, not as compiler keywords
        </li>
        <li>
          <strong className="text-white">Modern toolchain</strong> -- built-in
          test runner, formatter, REPL, LSP, package manager
        </li>
        <li>
          <strong className="text-white">Native binaries</strong> -- AOT builds
          produce self-contained executables; exact size and startup claims are
          tracked per benchmark instead of treated as universal promises
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
        <li>Developers who like TypeScript/JavaScript ergonomics but want native binaries</li>
        <li>Teams building CLIs, local tools, small services, and compute workers</li>
        <li>People who want simple code first and lower-level control later</li>
        <li>Systems, GUI, and game developers evaluating a future direction, not a finished platform today</li>
      </ul>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Performance
      </h2>
      <p className="mb-4">
        The current public baseline is the committed G2.1 initial diagnostic
        run from 2026-09-06 on Apple M5 Max / macOS 26.5.1. It uses paired,
        randomized runs with warmups, bootstrap intervals, and output-oracle
        checks. It is incomplete and should not be read as a Rust-parity claim:
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
              ["Rust fib(40)", "161.09 ms", "1.000×"],
              ["Turbo AOT fib(40)", "233.31 ms", "1.444× paired"],
              ["Rust word count", "22.64 ms", "1.000×"],
              ["Turbo AOT word count", "88.62 ms", "3.871× paired"],
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
      <p className="mb-4 text-sm text-gray-400">
        The fib subset misses the CPU target today; word count is an application
        diagnostic with proven output equivalence but non-identical
        implementation shape. Reproduce the baseline with{" "}
        <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">python3 benchmarks/evaluator.py --output benchmarks/results/a-new-run-name</code>.
      </p>

      <h3 className="text-xl font-bold text-white mt-8 mb-4">
        Real-world workload: word-count
      </h3>
      <p className="mb-4">
        The next performance milestone is not a slogan. Turbo is aiming for
        Rust-class performance under explicit gates: CPU geometric mean at or
        below 1.15× for managed code and 1.05× for controlled code, no individual
        CPU workload above 1.35× / 1.15×, and memory profiles that can prove
        live payload and allocation behavior rather than relying on RSS alone.
      </p>

      <div className="flex gap-4 mt-8">
        <Link
          href="/performance"
          className="inline-flex items-center gap-2 bg-[#00ff88] text-[#0a0a0a] font-semibold px-6 py-3 rounded-lg hover:bg-[#00cc6a] transition-colors"
        >
          Performance status
          <span>&#8594;</span>
        </Link>
        <Link
          href="/roadmap"
          className="inline-flex items-center gap-2 border border-[#1a1a2e] text-gray-300 px-6 py-3 rounded-lg hover:border-[#00ff88] hover:text-[#00ff88] transition-colors"
        >
          Roadmap
        </Link>
      </div>
    </article>
  );
}
