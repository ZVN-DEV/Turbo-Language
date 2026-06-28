import Link from "next/link";

export default function ExamplesPage() {
  return (
    <article className="text-gray-300 leading-relaxed font-[family-name:var(--font-geist-sans)]">
      <h1 className="text-4xl font-bold text-white mb-4">Examples</h1>
      <p className="text-lg text-gray-400 mb-8">
        Turbo ships a mix of runnable examples you can execute today and roadmap
        examples that document planned language features. This page separates the
        two so the product surface stays clear.
      </p>

      <div className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-6 mb-8">
        <p className="text-white font-medium mb-2">Two example tiers</p>
        <ul className="list-disc list-inside space-y-2 text-sm text-gray-300">
          <li><strong className="text-white">Runnable today:</strong> examples under <code className="text-[#00ff88] bg-[#0d0d14] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">examples/</code> compile with the current toolchain.</li>
          <li><strong className="text-white">Roadmap / planned:</strong> examples under <code className="text-[#00ff88] bg-[#0d0d14] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">examples/roadmap/</code> are design references and are not expected to compile yet.</li>
        </ul>
      </div>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Running examples today
      </h2>
      <p className="mb-4">
        Runnable examples are simple entry-point projects with README notes and
        executable <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">main.tb</code>{" "}
        files. Run them directly with the CLI:
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-8 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`turbolang run examples/web-dashboard/main.tb
turbolang run examples/simple-script/main.tb
turbolang run examples/speed-server/main.tb
turbolang build examples/todo-cli/main.tb`}
        </code>
      </pre>

      {/* Web Dashboard */}
      <div className="border border-[#1a1a2e] rounded-lg p-6 mb-6">
        <div className="flex items-center gap-3 mb-2">
          <h3 className="text-xl font-bold text-white">
            web-dashboard -- Flagship Browser Demo
          </h3>
          <span className="text-xs font-medium px-2 py-0.5 rounded-full bg-[#00ff88]/10 text-[#00ff88] border border-[#00ff88]/30">
            Runnable today
          </span>
          <span className="text-xs font-medium px-2 py-0.5 rounded-full bg-[#00d4ff]/10 text-[#00d4ff] border border-[#00d4ff]/30">
            Best first run
          </span>
        </div>
        <p className="text-gray-400 text-sm mb-4">Hero example</p>
        <p className="mb-4">
          The clearest single demo of what Turbo can do today: one Turbo file
          serves a styled HTML dashboard plus five JSON benchmark endpoints.
          This is the fastest way to prove the compiler, runtime, and browser
          story all work together.
        </p>
        <div className="grid md:grid-cols-2 gap-4 mb-4 text-sm">
          <div className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4">
            <p className="text-white font-medium mb-2">Quickstart</p>
            <pre className="overflow-x-auto text-gray-300 font-[family-name:var(--font-geist-mono)]">
              <code>{`turbolang run examples/web-dashboard/main.tb
# then open http://localhost:3000`}</code>
            </pre>
          </div>
          <div className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4">
            <p className="text-white font-medium mb-2">Try this in the browser</p>
            <ul className="list-disc list-inside space-y-1 text-gray-300">
              <li>Click <strong>Run All Benchmarks</strong></li>
              <li>Open <code className="text-[#00ff88]">/api/info</code> in a second tab</li>
              <li>Leave the terminal running until you press Ctrl+C</li>
            </ul>
          </div>
        </div>
        <p className="mb-2 text-white font-medium text-sm">Key features demonstrated:</p>
        <ul className="list-disc list-inside space-y-1 mb-4 text-sm">
          <li>HTML UI plus JSON routes from one Turbo process</li>
          <li>HTTP routing and browser-facing responses</li>
          <li>CPU-bound benchmark endpoints you can trigger live</li>
          <li>String-built HTML/CSS/JS without extra dependencies</li>
          <li>Runnable today with the current compiler</li>
        </ul>
      </div>

      {/* Simple Script */}
      <div className="border border-[#1a1a2e] rounded-lg p-6 mb-6">
        <div className="flex items-center gap-3 mb-2">
          <h3 className="text-xl font-bold text-white">
            simple-script -- Text Statistics Analyzer
          </h3>
          <span className="text-xs font-medium px-2 py-0.5 rounded-full bg-[#00ff88]/10 text-[#00ff88] border border-[#00ff88]/30">
            Runnable today
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
let upper = cleaned |> upper`}
          </code>
        </pre>
      </div>

      {/* Speed Server */}
      <div className="border border-[#1a1a2e] rounded-lg p-6 mb-6">
        <div className="flex items-center gap-3 mb-2">
          <h3 className="text-xl font-bold text-white">
            speed-server -- HTTP Speed Server
          </h3>
          <span className="text-xs font-medium px-2 py-0.5 rounded-full bg-[#00ff88]/10 text-[#00ff88] border border-[#00ff88]/30">
            Runnable today
          </span>
        </div>
        <p className="text-gray-400 text-sm mb-4">Intermediate</p>
        <p className="mb-4">
          A high-performance HTTP server example showcasing Turbo&apos;s current
          HTTP primitives, JSON responses, and benchmark-style endpoints.
        </p>
        <p className="mb-2 text-white font-medium text-sm">Key features demonstrated:</p>
        <ul className="list-disc list-inside space-y-1 mb-4 text-sm">
          <li>HTTP routing and request handling</li>
          <li>JSON responses</li>
          <li>CPU-bound benchmark endpoints</li>
          <li>Native compilation for server-style workloads</li>
        </ul>
      </div>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        More runnable examples
      </h2>
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
              ["todo-cli", "Task manager with file I/O and structs", "Runnable today"],
              ["data-pipeline", "Log analysis engine with parsing and aggregation", "Runnable today"],
              ["game-of-life", "Conway's Game of Life with string-grid updates", "Runnable today"],
            ].map(([name, desc, status]) => (
              <tr key={name} className="border-b border-[#1a1a2e]">
                <td className="px-4 py-2">
                  <code className="text-[#00ff88] font-[family-name:var(--font-geist-mono)]">{name}</code>
                </td>
                <td className="px-4 py-2">{desc}</td>
                <td className="px-4 py-2">
                  <span className="text-xs font-medium px-2 py-0.5 rounded-full bg-[#00ff88]/10 text-[#00ff88] border border-[#00ff88]/30">
                    {status}
                  </span>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Roadmap examples
      </h2>
      <p className="mb-4">
        These examples are design references for features still in development.
        They help explain where Turbo is headed, but they are not runnable with
        the current compiler.
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
              ["web-api", "Production-style bookmarking API with JWT auth and WebSocket routes", "Planned / not runnable yet"],
              ["realtime-system", "Trading order matching engine with zero-allocation ambitions", "Planned / not runnable yet"],
              ["edge-wasm", "Edge image processing pipeline targeting future WASM workflows", "Planned / not runnable yet"],
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
          Browse the full set of runnable and roadmap examples in the {" "}
          <a
            href="https://github.com/ZVN-DEV/Turbo-Language/tree/master/examples"
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
