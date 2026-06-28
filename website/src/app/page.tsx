import Link from "next/link";
import CodeBlock from "@/components/code-block";
import CodeTabs from "@/components/code-tabs";

const heroCode = `fn fib(n: i64) -> i64 {
    if n <= 1 { n }
    else { fib(n - 1) + fib(n - 2) }
}

fn main() {
    let result = fib(40)
    print("fib(40) = {result}")
}`;

const exampleTabs = [
  {
    label: "Pattern Matching",
    filename: "shapes.tb",
    code: `type Shape {
    Circle(f64)
    Rectangle(f64, f64)
    Triangle(f64, f64)
}

fn area(shape: Shape) -> f64 {
    match shape {
        Circle(r) => 3.14159 * r * r
        Rectangle(w, h) => w * h
        Triangle(b, h) => 0.5 * b * h
    }
}

fn main() {
    let s = Shape.Circle(5.0)
    print("Area: {area(s)}")
}`,
  },
  {
    label: "Concurrency",
    filename: "concurrent.tb",
    code: `// spawn runs a function on a real OS thread; await joins it.
// No event loop and no non-blocking I/O — each spawn is a thread,
// so keep the count reasonable.
async fn fetch_bytes(url: str) -> i64 {
    // http_get blocks this thread until the response arrives
    let body = http_get(url)
    len(body)
}

fn main() {
    // Two blocking fetches run on separate threads, concurrently
    let a = spawn fetch_bytes("https://api.example.com/users")
    let b = spawn fetch_bytes("https://api.example.com/posts")

    // await blocks main until each thread finishes
    print("total bytes: {await a + await b}")
}`,
  },
  {
    label: "HTTP + JSON",
    filename: "api.tb",
    code: `fn main() {
    let app = http_server(8080)

    route(app, "GET", "/health", |req: str| -> str {
        respond_text(200, "ok")
    })

    route(app, "POST", "/api/echo", |req: str| -> str {
        let body = request_body(req)
        respond_text(200, body)
    })

    http_listen(app)
}`,
  },
];

// fib(40), best of 5 wall-clock runs on an Apple M5 Max (macOS 26.5.1,
// 2026-06-27). Turbo (AOT) vs native and interpreted baselines. This is a
// CPU microbenchmark, not a real-world workload.
const benchmarks = [
  { label: "C (clang -O2)", ms: 265, size: "33 KB", highlight: false },
  { label: "Rust (rustc -O)", ms: 265, size: "455 KB", highlight: false },
  { label: "Turbo (AOT)", ms: 330, size: "93 KB", highlight: true },
  { label: "Go (go build)", ms: 340, size: "--", highlight: false },
  { label: "Node.js 22", ms: 680, size: "--", highlight: false },
  { label: "Python 3.10", ms: 13300, size: "--", highlight: false },
];

// word-count: read a ~5 MB text file, tokenize on whitespace, count word
// frequencies in a hashmap, print the top-20 + a total. An end-to-end workload
// (file I/O + strings + hashmaps + sorting), best of 5 on the same machine.
// The C/Rust/Go baselines run the identical algorithm over the identical input;
// run_wordcount.sh enforces byte-for-byte identical output across all four.
const wordcountBenchmarks = [
  { label: "C (clang -O2)", ms: 110, size: "1.00x", highlight: false },
  { label: "Rust (rustc -O)", ms: 125, size: "1.13x", highlight: false },
  { label: "Go (go build)", ms: 130, size: "1.18x", highlight: false },
  { label: "Turbo (AOT)", ms: 240, size: "2.2x", highlight: true },
  { label: "Turbo (JIT)", ms: 220, size: "2.0x", highlight: false },
];

const features = [
  {
    title: "Native Speed",
    description:
      "Compiles to machine code via Cranelift. No interpreter, no VM. Within ~1.3x of C and Rust on simple CPU microbenchmarks.",
    icon: (
      <svg
        className="w-6 h-6"
        fill="none"
        viewBox="0 0 24 24"
        stroke="currentColor"
        strokeWidth={1.5}
      >
        <path
          strokeLinecap="round"
          strokeLinejoin="round"
          d="M3.75 13.5l10.5-11.25L12 10.5h8.25L9.75 21.75 12 13.5H3.75z"
        />
      </svg>
    ),
  },
  {
    title: "Type Safety",
    description:
      "Generics, traits, pattern matching, Result and Optional types. Catch bugs at compile time, not in production.",
    icon: (
      <svg
        className="w-6 h-6"
        fill="none"
        viewBox="0 0 24 24"
        stroke="currentColor"
        strokeWidth={1.5}
      >
        <path
          strokeLinecap="round"
          strokeLinejoin="round"
          d="M9 12.75L11.25 15 15 9.75m-3-7.036A11.959 11.959 0 013.598 6 11.99 11.99 0 003 9.749c0 5.592 3.824 10.29 9 11.623 5.176-1.332 9-6.03 9-11.622 0-1.31-.21-2.571-.598-3.751h-.152c-3.196 0-6.1-1.248-8.25-3.285z"
        />
      </svg>
    ),
  },
  {
    title: "Small, Honest Core",
    description:
      "Turbo keeps the compiler focused on a general-purpose, compiled language. Framework-shaped features (agents, GPU kernels, distributed actors) live in sidecar libraries, not keywords — so the core stays stable.",
    icon: (
      <svg
        className="w-6 h-6"
        fill="none"
        viewBox="0 0 24 24"
        stroke="currentColor"
        strokeWidth={1.5}
      >
        <path
          strokeLinecap="round"
          strokeLinejoin="round"
          d="M9.813 15.904L9 18.75l-.813-2.846a4.5 4.5 0 00-3.09-3.09L2.25 12l2.846-.813a4.5 4.5 0 003.09-3.09L9 5.25l.813 2.846a4.5 4.5 0 003.09 3.09L15.75 12l-2.846.813a4.5 4.5 0 00-3.09 3.09zM18.259 8.715L18 9.75l-.259-1.035a3.375 3.375 0 00-2.455-2.456L14.25 6l1.036-.259a3.375 3.375 0 002.455-2.456L18 2.25l.259 1.035a3.375 3.375 0 002.455 2.456L21.75 6l-1.036.259a3.375 3.375 0 00-2.455 2.456z"
        />
      </svg>
    ),
  },
  {
    title: "Thread Concurrency",
    description:
      "spawn runs work on real OS threads; await joins them. Plus channels and mutex — straightforward concurrency with no event loop and no hidden runtime.",
    icon: (
      <svg
        className="w-6 h-6"
        fill="none"
        viewBox="0 0 24 24"
        stroke="currentColor"
        strokeWidth={1.5}
      >
        <path
          strokeLinecap="round"
          strokeLinejoin="round"
          d="M7.5 21L3 16.5m0 0L7.5 12M3 16.5h13.5m0-13.5L21 7.5m0 0L16.5 12M21 7.5H7.5"
        />
      </svg>
    ),
  },
  {
    title: "Zero GC",
    description:
      "No garbage collector pauses. Deterministic memory management with ~93 KB binaries. Deploy anywhere with no runtime.",
    icon: (
      <svg
        className="w-6 h-6"
        fill="none"
        viewBox="0 0 24 24"
        stroke="currentColor"
        strokeWidth={1.5}
      >
        <path
          strokeLinecap="round"
          strokeLinejoin="round"
          d="M20.25 6.375c0 2.278-3.694 4.125-8.25 4.125S3.75 8.653 3.75 6.375m16.5 0c0-2.278-3.694-4.125-8.25-4.125S3.75 4.097 3.75 6.375m16.5 0v11.25c0 2.278-3.694 4.125-8.25 4.125s-8.25-1.847-8.25-4.125V6.375"
        />
      </svg>
    ),
  },
  {
    title: "Great DX",
    description:
      "Built-in test runner, formatter, REPL, LSP server, and VS Code extension. Everything works out of the box.",
    icon: (
      <svg
        className="w-6 h-6"
        fill="none"
        viewBox="0 0 24 24"
        stroke="currentColor"
        strokeWidth={1.5}
      >
        <path
          strokeLinecap="round"
          strokeLinejoin="round"
          d="M6.75 7.5l3 2.25-3 2.25m4.5 0h3m-9 8.25h13.5A2.25 2.25 0 0021 18V6a2.25 2.25 0 00-2.25-2.25H5.25A2.25 2.25 0 003 6v12a2.25 2.25 0 002.25 2.25z"
        />
      </svg>
    ),
  },
];

// Max ms for the bar chart scale (Python excluded from visual scale)
const BAR_MAX = 700;
const WORDCOUNT_BAR_MAX = 280;

export default function Home() {
  return (
    <div className="font-[family-name:var(--font-geist-sans)]">
      {/* ── Hero ────────────────────────────────────────────── */}
      <section className="relative overflow-hidden">
        {/* Gradient orb background */}
        <div className="absolute top-[-200px] left-1/2 -translate-x-1/2 w-[800px] h-[600px] bg-gradient-to-b from-[#00ff8810] via-[#00d4ff08] to-transparent rounded-full blur-3xl pointer-events-none" />

        <div className="max-w-5xl mx-auto px-6 pt-28 pb-24 md:pt-40 md:pb-32">
          <div className="grid lg:grid-cols-2 gap-12 lg:gap-16 items-center">
            <div>
              <div className="inline-flex items-center gap-2 px-3 py-1 rounded-full border border-[#1a1a2e] text-xs text-gray-400 mb-6">
                <span className="w-1.5 h-1.5 rounded-full bg-[#00ff88] animate-pulse" />
                Now open source
              </div>

              <h1 className="text-5xl md:text-[3.75rem] font-bold text-white leading-[1.05] tracking-tight mb-6">
                JavaScript&apos;s Soul.
                <br />
                <span className="bg-gradient-to-r from-[#00ff88] to-[#00d4ff] bg-clip-text text-transparent">
                  Rust&apos;s Speed.
                </span>
              </h1>

              <p className="text-lg md:text-xl text-gray-400 leading-relaxed mb-8 max-w-lg">
                A compiled, type-safe language with native performance and a
                modern toolchain. Small, honest core. No VM, no GC, no
                compromise.
              </p>

              <div className="flex flex-wrap gap-4">
                <Link
                  href="/docs/installation"
                  className="inline-flex items-center gap-2 bg-[#00ff88] text-[#0a0a0a] font-semibold px-6 py-3 rounded-lg hover:bg-[#00cc6a] transition-colors text-sm"
                >
                  Get Started
                  <svg
                    width="16"
                    height="16"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="2.5"
                  >
                    <path d="M5 12h14M12 5l7 7-7 7" />
                  </svg>
                </Link>
                <Link
                  href="/docs/examples"
                  className="inline-flex items-center gap-2 border border-[#1a1a2e] text-gray-300 px-6 py-3 rounded-lg hover:border-[#00ff88] hover:text-[#00ff88] transition-colors text-sm"
                >
                  Run the flagship demo
                </Link>
                <a
                  href="https://github.com/ZVN-DEV/Turbo-Language"
                  target="_blank"
                  rel="noopener noreferrer"
                  className="inline-flex items-center gap-2 border border-[#1a1a2e] text-gray-300 px-6 py-3 rounded-lg hover:border-[#00ff88] hover:text-[#00ff88] transition-colors text-sm"
                >
                  <svg
                    width="18"
                    height="18"
                    viewBox="0 0 24 24"
                    fill="currentColor"
                  >
                    <path d="M12 0c-6.626 0-12 5.373-12 12 0 5.302 3.438 9.8 8.207 11.387.599.111.793-.261.793-.577v-2.234c-3.338.726-4.033-1.416-4.033-1.416-.546-1.387-1.333-1.756-1.333-1.756-1.089-.745.083-.729.083-.729 1.205.084 1.839 1.237 1.839 1.237 1.07 1.834 2.807 1.304 3.492.997.107-.775.418-1.305.762-1.604-2.665-.305-5.467-1.334-5.467-5.931 0-1.311.469-2.381 1.236-3.221-.124-.303-.535-1.524.117-3.176 0 0 1.008-.322 3.301 1.23.957-.266 1.983-.399 3.003-.404 1.02.005 2.047.138 3.006.404 2.291-1.552 3.297-1.23 3.297-1.23.653 1.653.242 2.874.118 3.176.77.84 1.235 1.911 1.235 3.221 0 4.609-2.807 5.624-5.479 5.921.43.372.823 1.102.823 2.222v3.293c0 .319.192.694.801.576 4.765-1.589 8.199-6.086 8.199-11.386 0-6.627-5.373-12-12-12z" />
                  </svg>
                  GitHub
                </a>
              </div>
            </div>

            <div className="relative">
              <div className="absolute inset-0 bg-gradient-to-r from-[#00ff8808] to-[#00d4ff08] rounded-xl blur-xl" />
              <div className="relative">
                <CodeBlock code={heroCode} filename="fib.tb" />
              </div>
            </div>
          </div>
        </div>
      </section>

      {/* ── Flagship Demo ─────────────────────────────────── */}
      <section className="border-t border-[#1a1a2e]">
        <div className="max-w-5xl mx-auto px-6 py-20 md:py-24">
          <div className="grid lg:grid-cols-[1.2fr_0.8fr] gap-8 items-start">
            <div>
              <p className="text-xs uppercase tracking-[0.28em] text-[#00ff88] mb-3">
                Best first proof
              </p>
              <h2 className="text-3xl md:text-4xl font-bold text-white mb-4">
                Start with the web-dashboard demo
              </h2>
              <p className="text-gray-400 text-lg max-w-2xl mb-6">
                The clearest runnable example today is{" "}
                <code className="text-[#00ff88] bg-[#111118] px-2 py-1 rounded text-base font-[family-name:var(--font-geist-mono)]">
                  examples/web-dashboard/main.tb
                </code>
                . It serves a styled browser UI and five JSON benchmark endpoints
                from one Turbo file.
              </p>
              <div className="grid sm:grid-cols-3 gap-3 text-sm mb-6">
                {[
                  "Run the Turbo file",
                  "Open localhost:3000",
                  "Click Run All Benchmarks",
                ].map((step, i) => (
                  <div
                    key={step}
                    className="rounded-lg border border-[#1a1a2e] bg-[#111118]/60 p-4 text-gray-300"
                  >
                    <div className="text-[#00ff88] font-semibold mb-1">
                      0{i + 1}
                    </div>
                    {step}
                  </div>
                ))}
              </div>
              <div className="flex flex-wrap gap-4">
                <Link
                  href="/docs/examples"
                  className="inline-flex items-center gap-2 bg-[#00ff88] text-[#0a0a0a] font-semibold px-5 py-3 rounded-lg hover:bg-[#00cc6a] transition-colors text-sm"
                >
                  Open demo docs
                </Link>
                <a
                  href="https://github.com/ZVN-DEV/Turbo-Language/tree/master/examples/web-dashboard"
                  target="_blank"
                  rel="noopener noreferrer"
                  className="inline-flex items-center gap-2 border border-[#1a1a2e] text-gray-300 px-5 py-3 rounded-lg hover:border-[#00ff88] hover:text-[#00ff88] transition-colors text-sm"
                >
                  View source
                </a>
              </div>
            </div>

            <div className="rounded-2xl border border-[#1a1a2e] bg-[#111118] p-6">
              <p className="text-sm font-semibold text-white mb-4">
                Quickstart
              </p>
              <pre className="text-sm font-[family-name:var(--font-geist-mono)] text-gray-300 leading-relaxed overflow-x-auto mb-4">
                <code>{`turbolang run examples/web-dashboard/main.tb
# then open http://localhost:3000`}</code>
              </pre>
              <ul className="space-y-2 text-sm text-gray-400">
                <li>• Browser UI plus JSON endpoints in one process</li>
                <li>• Good first demo for people evaluating Turbo quickly</li>
                <li>• Ships today — no roadmap syntax required</li>
              </ul>
            </div>
          </div>
        </div>
      </section>

      {/* ── Features Grid ──────────────────────────────────── */}
      <section className="max-w-5xl mx-auto px-6 py-24 md:py-32">
        <div className="text-center mb-16">
          <h2 className="text-3xl md:text-4xl font-bold text-white mb-4">
            Why Turbo?
          </h2>
          <p className="text-gray-400 text-lg max-w-2xl mx-auto">
            A language designed from scratch for the next era of software --
            fast today, with a small core the authors are willing to freeze.
          </p>
        </div>

        <div className="grid md:grid-cols-2 lg:grid-cols-3 gap-6">
          {features.map((f) => (
            <div
              key={f.title}
              className="group p-6 rounded-xl border border-[#1a1a2e] bg-[#111118]/50 hover:border-[#00ff8830] hover:bg-[#111118] transition-all duration-300"
            >
              <div className="w-10 h-10 rounded-lg bg-[#00ff8815] text-[#00ff88] flex items-center justify-center mb-4 group-hover:bg-[#00ff8825] transition-colors">
                {f.icon}
              </div>
              <h3 className="text-lg font-semibold text-white mb-2">
                {f.title}
              </h3>
              <p className="text-sm text-gray-400 leading-relaxed">
                {f.description}
              </p>
            </div>
          ))}
        </div>
      </section>

      {/* ── Code Examples ──────────────────────────────────── */}
      <section className="border-t border-[#1a1a2e]">
        <div className="max-w-5xl mx-auto px-6 py-24 md:py-32">
          <div className="text-center mb-16">
            <h2 className="text-3xl md:text-4xl font-bold text-white mb-4">
              Expressive by Default
            </h2>
            <p className="text-gray-400 text-lg max-w-2xl mx-auto">
              Pattern matching, thread concurrency, and HTTP+JSON servers --
              all with clean, readable syntax.
            </p>
          </div>

          <div className="max-w-3xl mx-auto">
            <CodeTabs tabs={exampleTabs} />
          </div>
        </div>
      </section>

      {/* ── Benchmarks ─────────────────────────────────────── */}
      <section className="border-t border-[#1a1a2e]">
        <div className="max-w-5xl mx-auto px-6 py-24 md:py-32">
          <div className="text-center mb-16">
            <h2 className="text-3xl md:text-4xl font-bold text-white mb-4">
              Performance
            </h2>
            <p className="text-gray-400 text-lg max-w-2xl mx-auto">
              Best of 5 wall-clock runs on an Apple M5 Max (macOS 26.5.1,
              2026-06-27). Every baseline runs the same algorithm over the same
              input and the harnesses enforce byte-for-byte identical output —
              honest numbers, not best-case marketing. Run them yourself with
              the benchmark scripts in{" "}
              <code className="font-[family-name:var(--font-geist-mono)] text-gray-300">
                turbo/benchmarks
              </code>
              .
            </p>
          </div>

          <div className="max-w-2xl mx-auto mb-6">
            <h3 className="text-sm font-semibold text-gray-300 uppercase tracking-wider">
              fib(40) — recursion microbenchmark
            </h3>
            <p className="text-gray-500 text-sm mt-1">
              Pure function-call overhead. Turbo&apos;s native build lands within
              ~1.3x of C and Rust, in the same range as Go, and far ahead of
              interpreted runtimes.
            </p>
          </div>

          <div className="max-w-2xl mx-auto space-y-4">
            {benchmarks.map((b) => {
              const barWidth = Math.min((b.ms / BAR_MAX) * 100, 100);
              const displayMs =
                b.ms >= 1000
                  ? `${(b.ms / 1000).toFixed(1)}s`
                  : `${b.ms}ms`;
              return (
                <div key={b.label} className="group">
                  <div className="flex items-center justify-between mb-1.5">
                    <span
                      className={`text-sm font-medium ${
                        b.highlight ? "text-[#00ff88]" : "text-gray-300"
                      }`}
                    >
                      {b.label}
                    </span>
                    <div className="flex items-center gap-3">
                      <span className="text-xs text-gray-500">{b.size}</span>
                      <span
                        className={`text-sm font-[family-name:var(--font-geist-mono)] font-medium ${
                          b.highlight ? "text-[#00ff88]" : "text-gray-400"
                        }`}
                      >
                        {displayMs}
                      </span>
                    </div>
                  </div>
                  <div className="h-2.5 bg-[#111118] rounded-full overflow-hidden">
                    <div
                      className={`h-full rounded-full transition-all duration-700 ${
                        b.highlight
                          ? "bg-gradient-to-r from-[#00ff88] to-[#00d4ff]"
                          : "bg-gray-700"
                      }`}
                      style={{ width: `${barWidth}%` }}
                    />
                  </div>
                </div>
              );
            })}
          </div>

          <div className="max-w-2xl mx-auto mt-16 mb-6">
            <h3 className="text-sm font-semibold text-gray-300 uppercase tracking-wider">
              word-count — real-world workload
            </h3>
            <p className="text-gray-500 text-sm mt-1">
              Read a ~5 MB file (1.05M words), tokenize, count frequencies in a
              hashmap, print the top 20 — file I/O, strings, hashmaps, sorting.
              On this string/hashmap-heavy work Turbo&apos;s native build is about
              2.2x slower than C and ~1.9x slower than Rust and Go: further behind
              than fib40, with the gap coming from runtime hashmap/string
              handling rather than codegen. Real numbers, named headroom.
            </p>
          </div>

          <div className="max-w-2xl mx-auto space-y-4">
            {wordcountBenchmarks.map((b) => {
              const barWidth = Math.min((b.ms / WORDCOUNT_BAR_MAX) * 100, 100);
              const displayMs = `${b.ms}ms`;
              return (
                <div key={b.label} className="group">
                  <div className="flex items-center justify-between mb-1.5">
                    <span
                      className={`text-sm font-medium ${
                        b.highlight ? "text-[#00ff88]" : "text-gray-300"
                      }`}
                    >
                      {b.label}
                    </span>
                    <div className="flex items-center gap-3">
                      <span className="text-xs text-gray-500">{b.size}</span>
                      <span
                        className={`text-sm font-[family-name:var(--font-geist-mono)] font-medium ${
                          b.highlight ? "text-[#00ff88]" : "text-gray-400"
                        }`}
                      >
                        {displayMs}
                      </span>
                    </div>
                  </div>
                  <div className="h-2.5 bg-[#111118] rounded-full overflow-hidden">
                    <div
                      className={`h-full rounded-full transition-all duration-700 ${
                        b.highlight
                          ? "bg-gradient-to-r from-[#00ff88] to-[#00d4ff]"
                          : "bg-gray-700"
                      }`}
                      style={{ width: `${barWidth}%` }}
                    />
                  </div>
                </div>
              );
            })}
          </div>
        </div>
      </section>

      {/* ── Installation ───────────────────────────────────── */}
      <section className="border-t border-[#1a1a2e]">
        <div className="max-w-5xl mx-auto px-6 py-24 md:py-32">
          <div className="text-center mb-16">
            <h2 className="text-3xl md:text-4xl font-bold text-white mb-4">
              Get Started in Seconds
            </h2>
            <p className="text-gray-400 text-lg max-w-2xl mx-auto">
              Clone, build, run. Or install via Homebrew.
            </p>
          </div>

          <div className="grid grid-cols-1 md:grid-cols-2 gap-6 max-w-4xl mx-auto w-full">
            <div className="rounded-xl border border-[#1a1a2e] bg-[#111118] p-6 min-w-0">
              <h3 className="text-sm font-semibold text-gray-400 uppercase tracking-wider mb-4">
                From Source
              </h3>
              <pre className="text-sm font-[family-name:var(--font-geist-mono)] text-gray-300 leading-relaxed overflow-x-auto">
                <code>
                  <span className="text-gray-500">$</span>{" "}
                  <span className="text-[#00ff88]">git clone</span>{" "}
                  https://github.com/ZVN-DEV/Turbo-Language.git{"\n"}
                  <span className="text-gray-500">$</span>{" "}
                  <span className="text-[#00ff88]">cd</span> Turbo-Language{"\n"}
                  <span className="text-gray-500">$</span>{" "}
                  <span className="text-[#00ff88]">cargo build</span> --release
                  -p turbo-cli --manifest-path turbo/Cargo.toml{"\n"}
                  <span className="text-gray-500">$</span>{" "}
                  <span className="text-[#00ff88]">./target/release/turbolang</span>{" "}
                  run hello.tb
                </code>
              </pre>
            </div>

            <div className="rounded-xl border border-[#1a1a2e] bg-[#111118] p-6 min-w-0">
              <h3 className="text-sm font-semibold text-gray-400 uppercase tracking-wider mb-4">
                Homebrew
              </h3>
              <pre className="text-sm font-[family-name:var(--font-geist-mono)] text-gray-300 leading-relaxed overflow-x-auto">
                <code>
                  <span className="text-gray-500">$</span>{" "}
                  <span className="text-[#00ff88]">brew tap</span>{" "}
                  ZVN-DEV/turbo{"\n"}
                  <span className="text-gray-500">$</span>{" "}
                  <span className="text-[#00ff88]">brew install</span> turbo-lang
                  {"\n\n"}
                  <span className="text-gray-500">$</span>{" "}
                  <span className="text-[#00ff88]">turbolang</span> run hello.tb
                </code>
              </pre>
            </div>
          </div>
        </div>
      </section>

      {/* ── CTA ────────────────────────────────────────────── */}
      <section className="border-t border-[#1a1a2e]">
        <div className="max-w-6xl mx-auto px-6 py-24 md:py-32 text-center">
          <h2 className="text-4xl md:text-5xl font-bold text-white mb-6">
            Ready to build?
          </h2>
          <p className="text-gray-400 text-xl mb-12 max-w-lg mx-auto">
            Start writing Turbo today. Native speed, modern syntax, and a
            roadmap that stays honest about what&apos;s shipped.
          </p>
          <div className="flex flex-wrap justify-center gap-4">
            <Link
              href="/docs/installation"
              className="inline-flex items-center gap-2 bg-[#00ff88] text-[#0a0a0a] font-semibold px-8 py-3.5 rounded-lg hover:bg-[#00cc6a] transition-colors"
            >
              Get Started
              <svg
                width="16"
                height="16"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="2.5"
              >
                <path d="M5 12h14M12 5l7 7-7 7" />
              </svg>
            </Link>
            <Link
              href="/docs"
              className="inline-flex items-center gap-2 border border-[#1a1a2e] text-gray-300 px-8 py-3.5 rounded-lg hover:border-[#00ff88] hover:text-[#00ff88] transition-colors"
            >
              Read the Docs
            </Link>
          </div>
        </div>
      </section>
    </div>
  );
}
