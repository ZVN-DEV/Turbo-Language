import Link from "next/link";

export default function CliPage() {
  return (
    <article className="text-gray-300 leading-relaxed font-[family-name:var(--font-geist-sans)]">
      <h1 className="text-4xl font-bold text-white mb-4">CLI Reference</h1>
      <p className="text-lg text-gray-400 mb-8">
        The Turbo CLI provides everything you need: compilation, testing,
        formatting, REPL, and more.
      </p>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">Commands</h2>

      <div className="overflow-x-auto mb-8">
        <table className="w-full text-sm text-left border border-[#1a1a2e] rounded-lg overflow-hidden">
          <thead className="bg-[#111118] text-gray-400">
            <tr>
              <th className="px-4 py-2 border-b border-[#1a1a2e]">Command</th>
              <th className="px-4 py-2 border-b border-[#1a1a2e]">Description</th>
            </tr>
          </thead>
          <tbody>
            {[
              ["turbo run <file.tb>", "Compile and run via JIT (Cranelift)"],
              ["turbo build <file.tb>", "Compile to a native binary (AOT)"],
              ["turbo build --llvm <file.tb>", "Compile with LLVM optimizations"],
              ["turbo test <file.tb>", "Run @test functions"],
              ["turbo bench <file.tb>", "Benchmark with timing"],
              ["turbo init <name>", "Create a new project"],
              ["turbo install", "Install dependencies from turbo.toml"],
              ["turbo update", "Update GitHub dependencies"],
              ["turbo fmt <file.tb>", "Format source code"],
              ["turbo doc <file.tb>", "Generate documentation"],
              ["turbo repl", "Interactive REPL"],
              ["turbo lsp", "Start Language Server Protocol server"],
              ["turbo explain <code>", "Explain an error code (e.g. E0100)"],
            ].map(([cmd, desc]) => (
              <tr key={cmd} className="border-b border-[#1a1a2e]">
                <td className="px-4 py-2">
                  <code className="text-[#00ff88] font-[family-name:var(--font-geist-mono)] text-xs whitespace-nowrap">
                    {cmd}
                  </code>
                </td>
                <td className="px-4 py-2">{desc}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        turbo run
      </h2>
      <p className="mb-4">
        Compiles and executes a Turbo source file using the JIT compiler. Best
        for development.
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`$ turbo run hello.tb
Hello, world!

# With verbose output
$ turbo run --verbose hello.tb`}</code>
      </pre>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        turbo build
      </h2>
      <p className="mb-4">
        Compiles to a standalone native binary. The binary is linked with the C
        runtime and has no external dependencies.
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`# Default (Cranelift backend)
$ turbo build hello.tb
$ ./hello

# With custom output name
$ turbo build hello.tb --output my-app
$ ./my-app

# With LLVM optimizations (requires LLVM 18)
$ turbo build --llvm hello.tb`}</code>
      </pre>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        turbo test
      </h2>
      <p className="mb-4">
        Runs all functions marked with{" "}
        <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
          @test
        </code>
        :
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`$ turbo test myfile.tb
  PASS  test_add
  PASS  test_subtract
2 passed, 0 failed`}</code>
      </pre>

      <h2 id="formatting" className="text-2xl font-bold text-white mt-10 mb-4">
        turbo fmt
      </h2>
      <p className="mb-4">
        Formats source code according to the standard Turbo style:
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`# Format a file in place
$ turbo fmt myfile.tb

# Check formatting without modifying
$ turbo fmt --check myfile.tb`}</code>
      </pre>

      <h2 id="repl" className="text-2xl font-bold text-white mt-10 mb-4">
        turbo repl
      </h2>
      <p className="mb-4">
        Start an interactive read-eval-print loop for experimenting:
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`$ turbo repl
turbo> print("hello")
hello
turbo> let x = 42
turbo> print(x * 2)
84`}</code>
      </pre>

      <h2 id="lsp" className="text-2xl font-bold text-white mt-10 mb-4">
        turbo lsp
      </h2>
      <p className="mb-4">
        Starts the Language Server Protocol server for editor integration.
        Provides:
      </p>
      <ul className="list-disc list-inside space-y-2 mb-6">
        <li>Real-time diagnostics and error highlighting</li>
        <li>Hover information for types and functions</li>
        <li>Go-to-definition</li>
        <li>Code completions</li>
        <li>Document symbols</li>
      </ul>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`# Usually started automatically by your editor
$ turbo lsp`}</code>
      </pre>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        turbo explain
      </h2>
      <p className="mb-4">
        Look up any compiler error code:
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`$ turbo explain E0100
E0100: Type mismatch
  The compiler expected one type but found another.

$ turbo explain E0200
E0200: Match expression is not exhaustive
  Your match is missing one or more possible patterns.`}</code>
      </pre>

      <h2 id="builtins" className="text-2xl font-bold text-white mt-10 mb-4">
        Built-in Functions
      </h2>
      <p className="mb-4">
        These functions are available globally without any imports:
      </p>
      <div className="overflow-x-auto mb-8">
        <table className="w-full text-sm text-left border border-[#1a1a2e] rounded-lg overflow-hidden">
          <thead className="bg-[#111118] text-gray-400">
            <tr>
              <th className="px-4 py-2 border-b border-[#1a1a2e]">Category</th>
              <th className="px-4 py-2 border-b border-[#1a1a2e]">Functions</th>
            </tr>
          </thead>
          <tbody>
            {[
              ["I/O", "print, assert, assert_eq, assert_ne"],
              ["Strings", "len, split, trim, upper, lower, replace, contains, starts_with, ends_with, join, repeat, to_str"],
              ["Arrays", "len, push, map, filter, reduce, sort"],
              ["Hashmaps", "hashmap, hashmap_set, hashmap_get, hashmap_has, hashmap_len, hashmap_keys, hashmap_remove"],
              ["Math", "math_sqrt, math_abs, math_pow, math_min, math_max, math_floor, math_ceil"],
              ["Async", "spawn, channel, send, recv, mutex, mutex_get, mutex_set, sleep"],
              ["Conversion", "to_str, clone"],
            ].map(([category, fns]) => (
              <tr key={category} className="border-b border-[#1a1a2e]">
                <td className="px-4 py-2 text-white font-medium">{category}</td>
                <td className="px-4 py-2">
                  <code className="text-gray-400 font-[family-name:var(--font-geist-mono)] text-xs">
                    {fns}
                  </code>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      <div className="flex gap-4 mt-10">
        <Link
          href="/docs/testing"
          className="inline-flex items-center gap-2 bg-[#00ff88] text-[#0a0a0a] font-semibold px-6 py-3 rounded-lg hover:bg-[#00cc6a] transition-colors"
        >
          Next: Testing
          <span>&#8594;</span>
        </Link>
      </div>
    </article>
  );
}
