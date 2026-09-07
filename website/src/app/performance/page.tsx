import type { Metadata } from "next";
import Link from "next/link";

export const metadata: Metadata = {
  title: "Performance",
  description:
    "Turbo performance status: all current qualification is incomplete, with committed benchmark evidence, gaps against Rust, and public targets.",
};

const currentRows = [
  {
    caseName: "Recursive fib(40)",
    category: "CPU",
    turbo: "233.31 ms",
    rust: "161.09 ms",
    ratio: "1.444×",
    interval: "1.4325–1.4618",
    note: "Pure recursion/function-call diagnostic; misses the current target.",
  },
  {
    caseName: "Existing 5 MiB word count",
    category: "Application",
    turbo: "88.62 ms",
    rust: "22.64 ms",
    ratio: "3.871×",
    interval: "3.7976–3.9549",
    note: "Output-equivalent application case; implementation shapes are not identical.",
  },
  {
    caseName: "Recursive tree diagnostic",
    category: "Diagnostic",
    turbo: "507.60 ms",
    rust: "385.34 ms",
    ratio: "1.317×",
    interval: "not statistical",
    note: "One-pair smoke plus allocation profile; tracked ARC allocations/frees balanced with zero tracked live allocations at return.",
  },
  {
    caseName: "Particle allocation diagnostic",
    category: "Diagnostic",
    turbo: "profile only",
    rust: "not comparator",
    ratio: "not speed claim",
    interval: "not statistical",
    note: "10,000 particles × 512 steps; 5,130,018 tracked allocations/frees and zero tracked live allocations at return.",
  },
];

const method = [
  "Evaluator revision 75f27e5e293c7c7f89e64ca06621287ab24600b3.",
  "Apple M5 Max / macOS 26.5.1; additional macOS ARM64 and Linux x86_64 gates are required before broad claims.",
  "Three batches, three warmup pairs per case per batch, 20 measured pairs per case per batch.",
  "Randomized execution order with paired ratio estimation and 2000 hierarchical bootstrap draws.",
  "Every measured runtime output matched an independent oracle with empty stderr.",
  "Peak RSS is reported separately from allocation, live-payload, and RC counters.",
];

const blockers = [
  "All current qualification is incomplete: this is diagnostic evidence, not a Rust-parity release gate.",
  "Samples below 200ms are still flagged; some workloads need in-program batching.",
  "The fixed evaluator catalog has eight required diagnostic categories: recursive compute, word count/text processing, JSON parse/transform, string/token processing, hashmap churn, buffer scan, packed particle update, and tree traversal.",
  "Word count proves output equivalence, not identical implementation shape, so it is excluded from CPU geometric mean claims.",
  "Controlled Turbo is a future target, not profiled current evidence; borrowed views, owned buffers, regions, and noalloc kernels do not exist as accepted controlled-language features yet.",
  "Allocation and RC instrumentation covers targeted shared-header ARC fixtures, not the whole process heap.",
  "Windows and Linux ARM64 are separate validation surfaces until proven by their own runs.",
];

const targets = [
  ["Managed CPU mean", "≤1.15× Rust", "Readable default Turbo code."],
  ["Controlled CPU mean", "≤1.05× Rust", "Code using explicit lower-level controls."],
  ["Individual CPU cases", "≤1.35× / ≤1.15×", "Managed / controlled upper bounds."],
  ["Live payload memory", "≤1.25× / ≤1.10×", "Managed / controlled memory workloads."],
  ["Controlled no-allocation", "0 after warmup", "Exact allocation and RC counters, not RSS inference."],
];

export default function PerformancePage() {
  return (
    <article className="font-[family-name:var(--font-geist-sans)]">
      <section className="relative overflow-hidden border-b border-[#1a1a2e]">
        <div className="absolute top-[-220px] left-1/2 h-[520px] w-[760px] -translate-x-1/2 rounded-full bg-gradient-to-b from-[#00ff8810] via-[#00d4ff08] to-transparent blur-3xl" />
        <div className="relative mx-auto max-w-5xl px-6 py-24 md:py-32">
          <p className="mb-4 text-xs uppercase tracking-[0.28em] text-[#00ff88]">
            Performance status
          </p>
          <h1 className="mb-6 max-w-4xl text-4xl font-bold leading-tight text-white md:text-6xl">
            The goal is Rust-class speed and control. Every current
            qualification is incomplete.
          </h1>
          <p className="max-w-3xl text-lg leading-relaxed text-gray-400 md:text-xl">
            Turbo compiles to native code today, but native code is not the same
            as a blanket Rust-performance claim. This page tracks the committed
            evidence, the eight diagnostic categories still being qualified, and
            the concrete gates Turbo must pass before stronger claims are earned.
          </p>
        </div>
      </section>

      <section className="mx-auto max-w-5xl px-6 py-20">
        <div className="mb-8 flex flex-col gap-3 md:flex-row md:items-end md:justify-between">
          <div>
            <h2 className="mb-3 text-3xl font-bold text-white">
              Committed baseline
            </h2>
            <p className="max-w-3xl text-gray-400">
              Committed diagnostics recorded on 2026-09-06. The statistical
              baseline is{" "}
              <code className="rounded bg-[#111118] px-1.5 py-0.5 font-[family-name:var(--font-geist-mono)] text-[#00ff88]">
                benchmarks/results/g2-initial-20260906
              </code>
              ; tree and particle allocation profiles are separate committed
              diagnostics. None of these qualify Rust parity.
            </p>
          </div>
          <Link
            href="/roadmap"
            className="text-sm text-[#00ff88] transition-colors hover:text-[#00d4ff]"
          >
            Roadmap →
          </Link>
        </div>

        <div className="overflow-x-auto">
          <table className="w-full min-w-[760px] overflow-hidden rounded-lg border border-[#1a1a2e] text-left text-sm">
            <thead className="bg-[#111118] text-gray-400">
              <tr>
                <th className="border-b border-[#1a1a2e] px-4 py-3">Case</th>
                <th className="border-b border-[#1a1a2e] px-4 py-3">
                  Category
                </th>
                <th className="border-b border-[#1a1a2e] px-4 py-3">Turbo</th>
                <th className="border-b border-[#1a1a2e] px-4 py-3">Rust</th>
                <th className="border-b border-[#1a1a2e] px-4 py-3">
                  Paired ratio
                </th>
                <th className="border-b border-[#1a1a2e] px-4 py-3">
                  95% interval
                </th>
              </tr>
            </thead>
            <tbody>
              {currentRows.map((row) => (
                <tr key={row.caseName} className="border-b border-[#1a1a2e]">
                  <td className="px-4 py-3 text-white">
                    <div className="font-medium">{row.caseName}</div>
                    <div className="mt-1 text-xs text-gray-500">
                      {row.note}
                    </div>
                  </td>
                  <td className="px-4 py-3 text-gray-300">{row.category}</td>
                  <td className="px-4 py-3 text-[#00ff88]">{row.turbo}</td>
                  <td className="px-4 py-3 text-gray-300">{row.rust}</td>
                  <td className="px-4 py-3 text-gray-300">{row.ratio}</td>
                  <td className="px-4 py-3 text-gray-300">{row.interval}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>

      <section className="border-t border-[#1a1a2e]">
        <div className="mx-auto grid max-w-5xl gap-8 px-6 py-20 lg:grid-cols-2">
          <div>
            <h2 className="mb-4 text-3xl font-bold text-white">
              Measurement method
            </h2>
            <ul className="space-y-3 text-sm leading-relaxed text-gray-300">
              {method.map((item) => (
                <li
                  key={item}
                  className="rounded-lg border border-[#1a1a2e] bg-[#111118]/60 p-4"
                >
                  {item}
                </li>
              ))}
            </ul>
          </div>
          <div>
            <h2 className="mb-4 text-3xl font-bold text-white">
              Known blockers
            </h2>
            <ul className="space-y-3 text-sm leading-relaxed text-gray-300">
              {blockers.map((item) => (
                <li
                  key={item}
                  className="rounded-lg border border-[#1a1a2e] bg-[#111118]/60 p-4"
                >
                  {item}
                </li>
              ))}
            </ul>
          </div>
        </div>
      </section>

      <section className="border-t border-[#1a1a2e]">
        <div className="mx-auto max-w-5xl px-6 py-20">
          <h2 className="mb-4 text-3xl font-bold text-white">
            Public performance targets
          </h2>
          <p className="mb-8 max-w-3xl text-gray-400">
            These targets guide compiler/runtime work. They are public because
              Turbo&apos;s market only makes sense if users can trust when a claim
              has actually been earned. Controlled-code targets are included as
              goals, not current profiled results.
          </p>
          <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
            {targets.map(([name, target, scope]) => (
              <div
                key={name}
                className="rounded-xl border border-[#1a1a2e] bg-[#111118]/60 p-5"
              >
                <p className="mb-2 text-sm text-gray-400">{name}</p>
                <p className="mb-2 text-2xl font-bold text-[#00ff88]">
                  {target}
                </p>
                <p className="text-sm leading-relaxed text-gray-400">{scope}</p>
              </div>
            ))}
          </div>
        </div>
      </section>

      <section className="border-t border-[#1a1a2e]">
        <div className="mx-auto max-w-5xl px-6 py-20">
          <div className="rounded-2xl border border-[#1a1a2e] bg-[#111118] p-8">
            <h2 className="mb-4 text-2xl font-bold text-white">
              What Turbo can honestly say today
            </h2>
            <p className="mb-4 leading-relaxed text-gray-400">
              Turbo has native execution, a friendly syntax surface, and early
              evidence that simple CPU code can get within striking distance of
              Rust. It also has clear gaps in strings, hashmaps, JSON
              equivalence, memory attribution, service primitives, controlled
              memory features, and cross-platform proof.
            </p>
            <p className="leading-relaxed text-gray-400">
              That is a good release posture: useful now for the strongest fit
              cases, transparent about the gaps, and pointed at specific work
              that can turn the original “TypeScript feel, Rust-class control”
              concept into something measurable.
            </p>
          </div>
        </div>
      </section>
    </article>
  );
}
