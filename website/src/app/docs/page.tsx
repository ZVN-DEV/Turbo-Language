import type { Metadata } from "next";
import Link from "next/link";

export const metadata: Metadata = {
  title: "Introduction",
  description:
    "Turbo is a compiled, type-safe language that builds straight to machine code via Cranelift — no VM, no garbage collector. Generics, traits, and pattern matching.",
};

export default function DocsPage() {
  return (
    <article className="text-gray-300 leading-relaxed font-[family-name:var(--font-geist-sans)]">
      <h1 className="text-4xl font-bold text-white mb-4">
        Introduction to Turbo
      </h1>
      <p className="text-lg text-gray-400 mb-8">
        A compiled, type-safe programming language with JavaScript&apos;s developer
        experience, Rust&apos;s performance, and a modern built-in toolchain.
      </p>

      <div className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-6 mb-8">
        <p className="text-[#00ff88] font-medium text-lg mb-0">
          JavaScript&apos;s soul. Rust&apos;s speed. Honest about what ships today.
        </p>
      </div>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        What is Turbo?
      </h2>
      <p className="mb-4">
        Turbo compiles directly to machine code using Cranelift. No interpreter,
        no VM, no garbage collector. Programs start
        instantly and run at native speed. It features strong static typing with
        type inference, generics, traits, and algebraic data types -- all while
        keeping a clean, approachable syntax.
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
          <strong className="text-white">Tiny binaries</strong> -- ~93 KB for a
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
        <li>Teams that want native-speed tooling today with a core small enough to stay stable</li>
        <li>Anyone who wants a modern language with batteries included</li>
        <li>Systems programmers who appreciate clean, expressive syntax</li>
      </ul>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Performance
      </h2>
      <p className="mb-4">
        Recursive <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">fib(40)</code>{" "}
        is a CPU microbenchmark (function-call and recursion overhead), not a
        real-world workload. Best of 5 wall-clock runs on an Apple M5 Max
        (macOS 26.5.1, 2026-06-27), Turbo&apos;s AOT build vs native and
        interpreted baselines:
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
              ["C (clang -O2)", "~265 ms", "33 KB"],
              ["Rust (rustc -O)", "~265 ms", "455 KB"],
              ["Turbo (AOT, Cranelift)", "~330 ms", "93 KB"],
              ["Go (go build)", "~340 ms", "--"],
              ["Node.js 22", "~680 ms", "--"],
              ["Python 3.10", "~13.3 s", "--"],
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
        On this microbenchmark Turbo&apos;s native output runs about 1.25&ndash;1.3x
        slower than C and Rust, sits in the same range as Go, and is far ahead of
        the interpreted runtimes &mdash; while emitting a self-contained ~93 KB
        binary. Reproduce with{" "}
        <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">./turbo/benchmarks/run_comparison.sh</code>.
      </p>

      <h3 className="text-xl font-bold text-white mt-8 mb-4">
        Real-world workload: word-count
      </h3>
      <p className="mb-4">
        fib40 only exercises the integer call stack. The{" "}
        <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">word-count</code>{" "}
        benchmark is end-to-end: read a ~5 MB text file (1.05M words), tokenize
        on whitespace, count word frequencies in a hashmap, and print the top-20
        words plus a total &mdash; exercising file I/O, strings, hashmaps, and
        sorting. The C/Rust/Go baselines implement the identical algorithm over
        the identical, deterministically generated input, and the runner fails
        unless all four languages produce byte-for-byte identical output.
      </p>
      <div className="overflow-x-auto mb-8">
        <table className="w-full text-sm text-left border border-[#1a1a2e] rounded-lg overflow-hidden">
          <thead className="bg-[#111118] text-gray-400">
            <tr>
              <th className="px-4 py-2 border-b border-[#1a1a2e]">Language</th>
              <th className="px-4 py-2 border-b border-[#1a1a2e]">Time</th>
              <th className="px-4 py-2 border-b border-[#1a1a2e]">vs C</th>
            </tr>
          </thead>
          <tbody>
            {[
              ["C (clang -O2)", "~108 ms", "1.00x"],
              ["Rust (rustc -O)", "~110 ms", "~1.02x"],
              ["Go (go build)", "~120 ms", "~1.11x"],
              ["Turbo (AOT, Cranelift)", "~150 ms", "~1.4x"],
              ["Turbo (JIT)", "~205 ms", "~1.9x"],
            ].map(([lang, time, ratio]) => (
              <tr key={lang} className="border-b border-[#1a1a2e]">
                <td className={`px-4 py-2 ${lang?.startsWith("Turbo") ? "text-[#00ff88] font-medium" : "text-gray-300"}`}>
                  {lang}
                </td>
                <td className="px-4 py-2 text-gray-300">{time}</td>
                <td className="px-4 py-2 text-gray-300">{ratio}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <p className="mb-4 text-sm text-gray-400">
        Honest framing: on this string/hashmap-heavy workload Turbo&apos;s native
        output runs about 1.4x slower than C (down from ~2.2x). The earlier gap
        came from the str&rarr;int map re-stringifying, re-parsing, and
        re-allocating the value on every increment; int values are now stored
        inline in the hashmap entry, so the counter loop does a single hash +
        single probe with no per-update allocation. It&apos;s a real workload
        with reproducible numbers. Reproduce with{" "}
        <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">./turbo/benchmarks/run_wordcount.sh</code>.
      </p>

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
