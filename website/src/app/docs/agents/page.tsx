import Link from "next/link";

export default function AgentsPage() {
  return (
    <article className="text-gray-300 leading-relaxed font-[family-name:var(--font-geist-sans)]">
      <h1 className="text-4xl font-bold text-white mb-4">Agents</h1>
      <p className="text-lg text-gray-400 mb-8">
        Turbo has an AI-native language direction, but current releases do not
        yet ship <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">agent</code>{" "}
        or <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">tool fn</code>{" "}
        syntax. This page documents the roadmap so the vision is clear without
        pretending the compiler already supports it.
      </p>

      <div className="bg-yellow-900/20 border border-yellow-700/50 rounded-lg p-4 mb-8">
        <p className="text-yellow-300 font-medium mb-2">Roadmap, not shipped</p>
        <p className="text-gray-300 mb-0 text-sm">
          The examples below are design sketches for future Turbo releases.
          Today&apos;s compiler does <strong className="text-white">not</strong> parse,
          type-check, or execute <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">agent</code>{" "}
          or <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">tool fn</code>.
          The goal is to eventually layer AI integration onto the runtime,
          async, HTTP, and tooling foundations Turbo already ships.
        </p>
      </div>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Why model agents in the language?
      </h2>
      <p className="mb-4">
        Turbo&apos;s long-term bet is that AI-native workflows should feel as
        straightforward as the rest of the language: explicit types, readable
        syntax, and first-class tooling support. The current release focuses on
        the lower layers that make that believable -- native binaries, WASM
        output, async primitives, HTTP building blocks, the playground, and the
        LSP.
      </p>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Planned tool functions
      </h2>
      <p className="mb-4">
        One direction under exploration is a dedicated syntax for declaring
        tool-callable functions:
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`// Planned syntax — not supported in current releases.

tool fn search(q: str) -> str {
    "found: {q}"
}

tool fn calc(x: i64) -> i64 {
    x * 2
}`}
        </code>
      </pre>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Planned agent declarations
      </h2>
      <p className="mb-4">
        The design work also explores declarative agent definitions that keep
        model choice, system prompts, and tool access readable in source:
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`// Planned syntax — not supported in current releases.

tool fn search(q: str) -> str { "found: {q}" }
tool fn calc(x: i64) -> i64 { x * 2 }

agent Helper {
    model: "claude-sonnet"
    tools: [search, calc]
    system: "You are a helpful assistant."
}`}
        </code>
      </pre>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Proposed agent fields
      </h2>
      <p className="mb-4">
        If Turbo lands this feature set, the current design direction looks like
        this:
      </p>
      <div className="overflow-x-auto mb-8">
        <table className="w-full text-sm text-left border border-[#1a1a2e] rounded-lg overflow-hidden">
          <thead className="bg-[#111118] text-gray-400">
            <tr>
              <th className="px-4 py-2 border-b border-[#1a1a2e]">Field</th>
              <th className="px-4 py-2 border-b border-[#1a1a2e]">Type</th>
              <th className="px-4 py-2 border-b border-[#1a1a2e]">Description</th>
            </tr>
          </thead>
          <tbody>
            {[
              ["model", "str", "Model identifier or provider hint"],
              ["system", "str", "System prompt or execution policy"],
              ["tools", "[fn]", "Functions exposed to the runtime tool dispatcher"],
            ].map(([field, type_, desc]) => (
              <tr key={field} className="border-b border-[#1a1a2e]">
                <td className="px-4 py-2">
                  <code className="text-[#00ff88] font-[family-name:var(--font-geist-mono)]">{field}</code>
                </td>
                <td className="px-4 py-2">
                  <code className="text-gray-400 font-[family-name:var(--font-geist-mono)]">{type_}</code>
                </td>
                <td className="px-4 py-2">{desc}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        What ships today
      </h2>
      <ul className="list-disc list-inside space-y-2 mb-6">
        <li>
          <strong className="text-white">Native compilation</strong> -- JIT, AOT, and WASM output are available today
        </li>
        <li>
          <strong className="text-white">Concurrency foundations</strong> -- async primitives, spawn, channels, and mutex already exist
        </li>
        <li>
          <strong className="text-white">Application building blocks</strong> -- HTTP helpers, file I/O, JSON utilities, and CLI workflows are real
        </li>
        <li>
          <strong className="text-white">Tooling integration</strong> -- formatter, tests, playground, and LSP are already part of the product
        </li>
      </ul>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Planned work
      </h2>
      <ul className="list-disc list-inside space-y-2 mb-6">
        <li>Parser and type-system support for <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">agent</code> and <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">tool fn</code></li>
        <li>Schema generation from typed tool signatures</li>
        <li>Runtime tool dispatch and provider integration</li>
        <li>Streaming responses, multi-turn execution, and debugger support</li>
      </ul>

      <div className="flex gap-4 mt-10">
        <Link
          href="/docs/cli"
          className="inline-flex items-center gap-2 bg-[#00ff88] text-[#0a0a0a] font-semibold px-6 py-3 rounded-lg hover:bg-[#00cc6a] transition-colors"
        >
          Next: CLI Reference
          <span>&#8594;</span>
        </Link>
      </div>
    </article>
  );
}
