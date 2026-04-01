import Link from "next/link";

export default function AgentsPage() {
  return (
    <article className="text-gray-300 leading-relaxed font-[family-name:var(--font-geist-sans)]">
      <h1 className="text-4xl font-bold text-white mb-4">Agents</h1>
      <p className="text-lg text-gray-400 mb-8">
        First-class AI agent primitives built into the language. Define tools,
        configure agents, and build AI-powered applications natively.
      </p>

      <div className="bg-yellow-900/20 border border-yellow-700/50 rounded-lg p-4 mb-8">
        <p className="text-yellow-300 font-medium mb-2">Experimental Syntax</p>
        <p className="text-gray-300 mb-0 text-sm">
          The{" "}
          <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">agent</code>{" "}
          and{" "}
          <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">tool fn</code>{" "}
          keywords are parsed and type-checked by the compiler, but they do not perform LLM calls,
          generate JSON schemas, or invoke tools at runtime. Agent definitions store metadata
          (model, system prompt, tool list) that you can access as fields. Full AI integration --
          including actual LLM communication and tool dispatch -- is planned for a future release.
        </p>
      </div>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Tool Functions
      </h2>
      <p className="mb-4">
        Define functions that an AI agent can call with the{" "}
        <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
          tool fn
        </code>{" "}
        keyword:
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`tool fn search(q: str) -> str {
    "found: {q}"
}

tool fn calc(x: i64) -> i64 {
    x * 2
}

fn main() {
    // Tool functions can also be called directly
    print(search("turbo"))    // found: turbo
    print(calc(21))           // 42
}`}</code>
      </pre>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        The Agent Keyword
      </h2>
      <p className="mb-4">
        Define an agent with a model, system prompt, and a set of tools:
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`tool fn search(q: str) -> str { "found: {q}" }
tool fn calc(x: i64) -> i64 { x * 2 }

agent Helper {
    model: "claude-sonnet"
    tools: [search, calc]
    system: "You are a helpful assistant."
}

fn main() {
    let a = Helper {}
    print(a.model)     // claude-sonnet
}`}</code>
      </pre>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Agent Fields
      </h2>
      <p className="mb-4">Agents expose their configuration as fields:</p>
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
              ["model", "str", "The model identifier (e.g. \"claude-sonnet\")"],
              ["system", "str", "The system prompt"],
              ["tools", "[fn]", "Array of tool functions available to the agent"],
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
        Multi-Agent Patterns
      </h2>
      <p className="mb-4">
        Define multiple specialized agents that work together:
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`tool fn web_search(q: str) -> str { "results for: {q}" }
tool fn summarize(text: str) -> str { "summary of: {text}" }
tool fn write_code(spec: str) -> str { "code for: {spec}" }

agent Researcher {
    model: "claude-sonnet"
    tools: [web_search, summarize]
    system: "You research topics thoroughly."
}

agent Coder {
    model: "claude-sonnet"
    tools: [write_code]
    system: "You write clean, tested code."
}

fn main() {
    let researcher = Researcher {}
    let coder = Coder {}
    print(researcher.model)    // claude-sonnet
    print(coder.model)         // claude-sonnet
}`}</code>
      </pre>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        What Works Today
      </h2>
      <ul className="list-disc list-inside space-y-2 mb-6">
        <li>
          <strong className="text-white">Type-checked tools</strong> -- The
          compiler validates that agent tools exist and are marked with{" "}
          <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
            tool fn
          </code>
        </li>
        <li>
          <strong className="text-white">Static validation</strong> -- Agent
          configurations (model, system prompt, tool list) are checked at compile time
        </li>
        <li>
          <strong className="text-white">Metadata access</strong> -- Agent fields
          (model, system, tools) are accessible at runtime as struct-like fields
        </li>
        <li>
          <strong className="text-white">Tool functions are real functions</strong> -- {" "}
          <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
            tool fn
          </code>{" "}
          functions can be called directly like any other function
        </li>
      </ul>
      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Planned for Future Releases
      </h2>
      <ul className="list-disc list-inside space-y-2 mb-6">
        <li>LLM API communication (sending prompts, receiving responses)</li>
        <li>Auto-generated JSON schemas from tool function signatures</li>
        <li>Tool dispatch and orchestration at runtime</li>
        <li>Streaming responses and multi-turn conversations</li>
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
