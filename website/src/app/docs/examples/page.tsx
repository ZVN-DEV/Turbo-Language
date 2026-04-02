import Link from "next/link";

export default function ExamplesPage() {
  return (
    <article className="text-gray-300 leading-relaxed font-[family-name:var(--font-geist-sans)]">
      <h1 className="text-4xl font-bold text-white mb-4">Examples</h1>
      <p className="text-lg text-gray-400 mb-8">
        Complete example projects demonstrating what Turbo is built for. Each
        includes source code, configuration, and tests.
      </p>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Running Examples
      </h2>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-8 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`cd examples/<example-name>
turbolang run          # Build and run
turbolang test         # Run all tests
turbolang build        # Build only`}</code>
      </pre>

      {/* Simple Script */}
      <div className="border border-[#1a1a2e] rounded-lg p-6 mb-6">
        <div className="flex items-center gap-3 mb-2">
          <h3 className="text-xl font-bold text-white">
            simple-script -- Text Statistics Analyzer
          </h3>
          <span className="text-xs font-medium px-2 py-0.5 rounded-full bg-[#00ff88]/10 text-[#00ff88] border border-[#00ff88]/30">
            Available
          </span>
        </div>
        <p className="text-gray-400 text-sm mb-4">Starter</p>
        <p className="mb-4">
          A text analysis tool that demonstrates Turbo&apos;s core features:
          functions, strings, arrays, hashmaps, the pipe operator, and string
          interpolation.
        </p>
        <p className="mb-2 text-white font-medium text-sm">Key features demonstrated:</p>
        <ul className="list-disc list-inside space-y-1 mb-4 text-sm">
          <li>Pipe operator for data flow</li>
          <li>String operations (trim, upper, lower, split, replace)</li>
          <li>Hashmap-based word frequency analysis</li>
          <li>Array operations (map, filter, reduce)</li>
          <li>String interpolation in print statements</li>
        </ul>
        <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
          <code>{`// Pipe operator for clean data flow
let chars = text |> count_chars
let words = text |> count_words

// String operations pipeline
let sample = "  Hello, Turbo World!  "
let cleaned = sample |> trim
let upper = cleaned |> upper`}</code>
        </pre>
      </div>

      {/* Speed Server */}
      <div className="border border-[#1a1a2e] rounded-lg p-6 mb-6">
        <div className="flex items-center gap-3 mb-2">
          <h3 className="text-xl font-bold text-white">
            speed-server -- HTTP Speed Server
          </h3>
          <span className="text-xs font-medium px-2 py-0.5 rounded-full bg-yellow-500/10 text-yellow-400 border border-yellow-500/30">
            Planned
          </span>
        </div>
        <p className="text-gray-400 text-sm mb-4">Intermediate</p>
        <p className="mb-4">
          A high-performance HTTP server showcasing async I/O, concurrent request
          handling, and native performance.
        </p>
        <p className="mb-2 text-white font-medium text-sm">Key features demonstrated:</p>
        <ul className="list-disc list-inside space-y-1 mb-4 text-sm">
          <li>Async functions and spawn</li>
          <li>Channel-based communication</li>
          <li>Concurrent request processing</li>
          <li>Native compilation performance</li>
        </ul>
      </div>

      {/* Web Dashboard */}
      <div className="border border-[#1a1a2e] rounded-lg p-6 mb-6">
        <div className="flex items-center gap-3 mb-2">
          <h3 className="text-xl font-bold text-white">
            web-dashboard -- Analytics Dashboard
          </h3>
          <span className="text-xs font-medium px-2 py-0.5 rounded-full bg-[#00ff88]/10 text-[#00ff88] border border-[#00ff88]/30">
            Available
          </span>
        </div>
        <p className="text-gray-400 text-sm mb-4">Intermediate</p>
        <p className="mb-4">
          A data analytics dashboard demonstrating structs, enums, pattern
          matching, and data processing pipelines.
        </p>
        <p className="mb-2 text-white font-medium text-sm">Key features demonstrated:</p>
        <ul className="list-disc list-inside space-y-1 mb-4 text-sm">
          <li>Structs and impl blocks</li>
          <li>Enums with data-carrying variants</li>
          <li>Pattern matching for routing logic</li>
          <li>Hashmap-based data aggregation</li>
        </ul>
      </div>

      {/* Additional Examples */}
      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        More Examples
      </h2>
      <p className="mb-4">
        The repository includes additional advanced examples. These are aspirational designs showing where Turbo is headed:
      </p>
      <div className="overflow-x-auto mb-8">
        <table className="w-full text-sm text-left border border-[#1a1a2e] rounded-lg overflow-hidden">
          <thead className="bg-[#111118] text-gray-400">
            <tr>
              <th className="px-4 py-2 border-b border-[#1a1a2e]">Example</th>
              <th className="px-4 py-2 border-b border-[#1a1a2e]">Description</th>
              <th className="px-4 py-2 border-b border-[#1a1a2e]">Status</th>
            </tr>
          </thead>
          <tbody>
            {[
              ["task-agent", "REST API with AI agent for task management", "Planned"],
              ["web-api", "Production bookmarking API with JWT auth, WebSocket, rate limiting", "Planned"],
              ["desktop-app", "Native markdown editor with AI writing assistant", "Planned"],
              ["realtime-system", "Trading order matching engine with zero-alloc hot paths", "Planned"],
              ["edge-functions", "Edge image processing with native compilation", "Planned"],
            ].map(([name, desc, status]) => (
              <tr key={name} className="border-b border-[#1a1a2e]">
                <td className="px-4 py-2">
                  <code className="text-[#00ff88] font-[family-name:var(--font-geist-mono)]">{name}</code>
                </td>
                <td className="px-4 py-2">{desc}</td>
                <td className="px-4 py-2">
                  <span className="text-xs font-medium px-2 py-0.5 rounded-full bg-yellow-500/10 text-yellow-400 border border-yellow-500/30">
                    {status}
                  </span>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      <div className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-6 mt-8">
        <p className="text-gray-300 mb-0">
          All examples are available in the{" "}
          <a
            href="https://github.com/ZVN-DEV/Turbo-Language/tree/main/examples"
            target="_blank"
            rel="noopener noreferrer"
            className="text-[#00ff88] hover:underline"
          >
            examples directory
          </a>{" "}
          of the Turbo repository.
        </p>
      </div>

      <div className="flex gap-4 mt-10">
        <Link
          href="/docs"
          className="inline-flex items-center gap-2 border border-[#1a1a2e] text-gray-300 px-6 py-3 rounded-lg hover:border-[#00ff88] hover:text-[#00ff88] transition-colors"
        >
          <span>&#8592;</span>
          Back to Docs
        </Link>
      </div>
    </article>
  );
}
