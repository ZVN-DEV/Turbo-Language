import type { Metadata } from "next";
import Link from "next/link";

export const metadata: Metadata = {
  title: "Roadmap",
  description:
    "Familiar code. Native execution. A path to deeper control. Turbo's honest roadmap for market fit, performance, memory control, and platform gates.",
};

const fitCards = [
  {
    title: "Strongest fit now",
    items: [
      "Native CLIs and developer tools",
      "Local data-processing utilities",
      "Small HTTP/JSON services",
      "Compute workers where startup and packaging matter",
    ],
  },
  {
    title: "Promising, with gaps to close",
    items: [
      "Task servers and durable workers",
      "API-heavy SaaS backends",
      "Embedded-style single binaries",
      "2D tools, simulations, and game-adjacent pipelines",
    ],
  },
  {
    title: "Future gates, not current claims",
    items: [
      "Native macOS, Linux, and Windows GUI apps",
      "Realtime game engines",
      "Kernel, driver, and freestanding system software",
      "Untrusted sandbox execution",
    ],
  },
];

const roadmap = [
  {
    status: "Shipped foundation",
    title: "Familiar language core",
    body: "Type inference, functions, structs, enums, pattern matching, generics, traits, Result/Optional shapes, string interpolation, JIT run, AOT build, tests, formatter, REPL, LSP, HTTP, JSON, SQLite, threads, channels, and mutexes form the current public surface.",
  },
  {
    status: "Current release closeout",
    title: "Correctness before bigger claims",
    body: "The release line is focused on semantics that must be boring: JSON text preservation, ownership around recursive values, AOT/JIT parity, fail-closed benchmarking, and explicit limits where the runtime does not yet cover a platform or edge case.",
  },
  {
    status: "Next performance lane",
    title: "Close the Rust gap with measured compiler/runtime work",
    body: "The next optimizations are representation and allocation driven: compact value layouts, typed lowering, Option/Result specialization, string and hashmap hot paths, fewer ARC operations, and benchmark cases long enough to qualify on macOS ARM64 and Linux x86_64.",
  },
  {
    status: "Next control lane",
    title: "Progressive memory control",
    body: "Turbo should stay approachable by default, then reveal lower-level tools when needed: cost attribution first, borrowed views and owned buffers next, then explicit no-allocation regions that can prove zero allocations after warmup in controlled code.",
  },
  {
    status: "Server lane",
    title: "From small services to task servers",
    body: "The server story needs TLS/client HTTP, bounded concurrency, cancellation, backpressure, streaming, better request/response types, durable-worker patterns, and operational examples before Turbo claims broad backend readiness.",
  },
  {
    status: "Platform lane",
    title: "Desktop, games, and systems only after bindings and frame evidence",
    body: "Turbo can already express game-like simulations and native process work, but real GUI/game/system claims need stable FFI, platform bindings, frame-time benchmarks, asset/tooling stories, Windows parity, and freestanding/runtime boundaries.",
  },
];

const goals = [
  "Managed Turbo CPU geometric mean ≤1.15× Rust on the accepted benchmark suite.",
  "Controlled Turbo CPU geometric mean ≤1.05× Rust where lower-level controls are used.",
  "No individual CPU workload above 1.35× managed or 1.15× controlled.",
  "Live payload memory ≤1.25× Rust managed and ≤1.10× Rust controlled on defined memory workloads.",
  "Controlled no-allocation workloads prove exactly zero allocations and RC activity after warmup.",
  "Every public claim names the host, compiler flags, workload shape, output oracle, and confidence interval.",
];

export default function RoadmapPage() {
  return (
    <article className="font-[family-name:var(--font-geist-sans)]">
      <section className="relative overflow-hidden border-b border-[#1a1a2e]">
        <div className="absolute top-[-220px] left-1/2 h-[520px] w-[760px] -translate-x-1/2 rounded-full bg-gradient-to-b from-[#00ff8810] via-[#00d4ff08] to-transparent blur-3xl" />
        <div className="relative mx-auto max-w-5xl px-6 py-24 md:py-32">
          <p className="mb-4 text-xs uppercase tracking-[0.28em] text-[#00ff88]">
            Honest roadmap
          </p>
          <h1 className="mb-6 max-w-4xl text-4xl font-bold leading-tight text-white md:text-6xl">
            Familiar code. Native execution. A path to deeper control.
          </h1>
          <p className="max-w-3xl text-lg leading-relaxed text-gray-400 md:text-xl">
            Turbo is not trying to claim every native market at once. The near
            wedge is approachable native tooling, local/system-adjacent
            backends, small services, and compute workers. The long-term
            ambition is Rust-class speed and memory control without making every
            program start at Rust&apos;s complexity ceiling.
          </p>
        </div>
      </section>

      <section className="mx-auto max-w-5xl px-6 py-20">
        <div className="mb-10">
          <h2 className="mb-3 text-3xl font-bold text-white">
            Where Turbo fits
          </h2>
          <p className="max-w-3xl text-gray-400">
            The market is clearest where TypeScript-shaped ergonomics meet the
            practical value of a native binary: fast local tools, compact
            deployment artifacts, and code that can later expose more control
            when profiling proves it needs it.
          </p>
        </div>
        <div className="grid gap-6 md:grid-cols-3">
          {fitCards.map((card) => (
            <div
              key={card.title}
              className="rounded-xl border border-[#1a1a2e] bg-[#111118]/60 p-6"
            >
              <h3 className="mb-4 text-lg font-semibold text-white">
                {card.title}
              </h3>
              <ul className="space-y-2 text-sm leading-relaxed text-gray-400">
                {card.items.map((item) => (
                  <li key={item}>• {item}</li>
                ))}
              </ul>
            </div>
          ))}
        </div>
      </section>

      <section className="border-t border-[#1a1a2e]">
        <div className="mx-auto max-w-5xl px-6 py-20">
          <h2 className="mb-10 text-3xl font-bold text-white">
            Roadmap lanes
          </h2>
          <div className="space-y-5">
            {roadmap.map((item) => (
              <div
                key={item.title}
                className="rounded-xl border border-[#1a1a2e] bg-[#111118]/50 p-6"
              >
                <p className="mb-2 text-xs font-semibold uppercase tracking-[0.22em] text-[#00ff88]">
                  {item.status}
                </p>
                <h3 className="mb-3 text-xl font-bold text-white">
                  {item.title}
                </h3>
                <p className="leading-relaxed text-gray-400">{item.body}</p>
              </div>
            ))}
          </div>
        </div>
      </section>

      <section className="border-t border-[#1a1a2e]">
        <div className="mx-auto max-w-5xl px-6 py-20">
          <div className="grid gap-8 lg:grid-cols-[0.9fr_1.1fr]">
            <div>
              <h2 className="mb-4 text-3xl font-bold text-white">
                Performance goals
              </h2>
              <p className="text-gray-400">
                These are targets, not guarantees. They become claims only after
                the accepted harness proves them with comparable algorithms,
                output oracles, repeated paired runs, confidence intervals, and
                required host coverage.
              </p>
            </div>
            <ul className="space-y-3 text-sm leading-relaxed text-gray-300">
              {goals.map((goal) => (
                <li
                  key={goal}
                  className="rounded-lg border border-[#1a1a2e] bg-[#111118]/60 p-4"
                >
                  {goal}
                </li>
              ))}
            </ul>
          </div>
        </div>
      </section>

      <section className="border-t border-[#1a1a2e]">
        <div className="mx-auto max-w-5xl px-6 py-20">
          <div className="rounded-2xl border border-[#1a1a2e] bg-[#111118] p-8">
            <h2 className="mb-4 text-2xl font-bold text-white">
              What this means for releases
            </h2>
            <p className="mb-6 leading-relaxed text-gray-400">
              Release notes should say exactly what changed and what remains
              limited. Turbo can be exciting without pretending that early
              native execution, ARC/COW memory management, and current server
              primitives already equal mature Rust, Go, or Node ecosystems.
            </p>
            <div className="flex flex-wrap gap-4">
              <Link
                href="/performance"
                className="inline-flex items-center gap-2 bg-[#00ff88] px-5 py-3 text-sm font-semibold text-[#0a0a0a] transition-colors hover:bg-[#00cc6a] rounded-lg"
              >
                Read performance status
              </Link>
              <Link
                href="/docs"
                className="inline-flex items-center gap-2 rounded-lg border border-[#1a1a2e] px-5 py-3 text-sm text-gray-300 transition-colors hover:border-[#00ff88] hover:text-[#00ff88]"
              >
                Read docs
              </Link>
            </div>
          </div>
        </div>
      </section>
    </article>
  );
}
