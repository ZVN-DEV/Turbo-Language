import type { Metadata } from "next";
import Link from "next/link";

export const metadata: Metadata = {
  title: "Agents",
  description:
    "Turbo retired agent and tool keywords from the core. These workflows will ship as a separate turbo-agent library on the stable 1.0 core, not as compiler features.",
};

export default function AgentsPage() {
  return (
    <article className="text-gray-300 leading-relaxed font-[family-name:var(--font-geist-sans)]">
      <h1 className="text-4xl font-bold text-white mb-4">
        Agents: Sidecar, Not Core
      </h1>
      <p className="text-lg text-gray-400 mb-8">
        Earlier drafts of Turbo proposed{" "}
        <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
          agent
        </code>{" "}
        and{" "}
        <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
          tool fn
        </code>{" "}
        as first-class language keywords. That direction has been retired.
        These features will ship as a separate library,{" "}
        <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
          turbo-agent
        </code>
        , built on top of the stable 1.0 core — not as compiler features.
      </p>

      <div className="bg-yellow-900/20 border border-yellow-700/50 rounded-lg p-4 mb-8">
        <p className="text-yellow-300 font-medium mb-2">
          No agent keywords exist today, and none are planned for the core
          language.
        </p>
        <p className="text-gray-300 mb-0 text-sm">
          The current compiler does{" "}
          <strong className="text-white">not</strong> parse, type-check, or
          execute <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">agent</code>{" "}
          or{" "}
          <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
            tool fn
          </code>
          , and the 1.0 stability contract (see{" "}
          <a
            href="https://github.com/ZVN-DEV/Turbo-Language/blob/master/COMPATIBILITY.md"
            className="text-[#00ff88] underline"
          >
            COMPATIBILITY.md
          </a>
          ) will not include them either. Agent workflows are a library
          concern, not a language concern.
        </p>
      </div>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Why move this out of the compiler?
      </h2>
      <p className="mb-4">
        LLM tooling — model providers, tool-calling schemas, memory
        strategies, supervision patterns — moves fast and changes shape every
        few months. A core language must not. If we baked those decisions
        into the grammar, every pivot in the AI ecosystem would break source
        compatibility across a whole fleet of Turbo programs.
      </p>
      <p className="mb-4">
        By keeping the core small, we can promise{" "}
        <a
          href="https://github.com/ZVN-DEV/Turbo-Language/blob/master/COMPATIBILITY.md"
          className="text-[#00ff88] underline"
        >
          real stability
        </a>{" "}
        at 1.0. A future{" "}
        <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
          turbo-agent
        </code>{" "}
        library can iterate freely on top of it.
      </p>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        What the core ships that makes a sidecar credible
      </h2>
      <ul className="list-disc list-inside space-y-2 mb-6">
        <li>
          <strong className="text-white">Typed async</strong> —{" "}
          <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
            async fn
          </code>
          ,{" "}
          <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
            spawn
          </code>
          ,{" "}
          <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
            await
          </code>
          , channels, mutex
        </li>
        <li>
          <strong className="text-white">HTTP client + server</strong> —
          enough to talk to any model provider&apos;s API
        </li>
        <li>
          <strong className="text-white">Typed serialization</strong> — JSON
          builtins, struct derive attributes
        </li>
        <li>
          <strong className="text-white">Native compilation</strong> — JIT,
          AOT, and WASM output
        </li>
        <li>
          <strong className="text-white">Tooling integration</strong> —
          formatter, tests, playground, LSP
        </li>
      </ul>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        What&apos;s on the roadmap
      </h2>
      <ul className="list-disc list-inside space-y-2 mb-6">
        <li>
          Ship Turbo core 1.0 with the stability contract in{" "}
          <a
            href="https://github.com/ZVN-DEV/Turbo-Language/blob/master/COMPATIBILITY.md"
            className="text-[#00ff88] underline"
          >
            COMPATIBILITY.md
          </a>
        </li>
        <li>
          Start{" "}
          <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
            turbo-agent
          </code>{" "}
          as a separate repository and SemVer line
        </li>
        <li>
          Provide idiomatic building blocks — provider adapters, tool-calling
          schemas, streaming responses, conversation memory — as library
          types, not compiler keywords
        </li>
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
