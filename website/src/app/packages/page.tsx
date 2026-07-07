import type { Metadata } from "next";
import Link from "next/link";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import CopyButton from "@/components/copy-button";

export const metadata: Metadata = {
  title: "Packages",
  description:
    "Browse the Turbo package registry — a static, git-versioned index of libraries you can install with turbolang install. Publish yours with a pull request.",
};

// The page is fully static: the index is read from disk at build time. The
// canonical index is served at /registry/index.json (the same file the
// `turbolang search` CLI fetches), so the site and the CLI never disagree.
export const dynamic = "force-static";

interface RegistryPackage {
  name: string;
  repo: string;
  description: string;
  categories?: string[];
  min_turbo_version?: string;
  homepage?: string;
}

interface RegistryIndex {
  schema_version: number;
  packages: RegistryPackage[];
}

function loadIndex(): RegistryIndex {
  const file = join(process.cwd(), "public", "registry", "index.json");
  const parsed = JSON.parse(readFileSync(file, "utf8")) as RegistryIndex;
  return {
    schema_version: parsed.schema_version,
    packages: Array.isArray(parsed.packages) ? parsed.packages : [],
  };
}

function PackageCard({ pkg }: { pkg: RegistryPackage }) {
  const installCmd = `turbolang install ${pkg.name}`;
  return (
    <div className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-6 flex flex-col gap-4">
      <div className="flex items-start justify-between gap-4">
        <div className="min-w-0">
          <h3 className="text-lg font-bold text-white font-[family-name:var(--font-geist-mono)] truncate">
            {pkg.name}
          </h3>
          <a
            href={`https://github.com/${pkg.repo}`}
            target="_blank"
            rel="noopener noreferrer"
            className="text-xs text-gray-500 hover:text-[#00ff88] transition-colors font-[family-name:var(--font-geist-mono)]"
          >
            {pkg.repo}
          </a>
        </div>
        {pkg.min_turbo_version && (
          <span className="shrink-0 text-xs text-gray-400 border border-[#1a1a2e] rounded-full px-2.5 py-1 font-[family-name:var(--font-geist-mono)]">
            turbo ≥ {pkg.min_turbo_version}
          </span>
        )}
      </div>

      <p className="text-sm text-gray-400 leading-relaxed">{pkg.description}</p>

      {pkg.categories && pkg.categories.length > 0 && (
        <div className="flex flex-wrap gap-2">
          {pkg.categories.map((category) => (
            <span
              key={category}
              className="text-xs text-[#00ff88] bg-[#00ff88]/10 rounded-full px-2.5 py-0.5 font-[family-name:var(--font-geist-sans)]"
            >
              {category}
            </span>
          ))}
        </div>
      )}

      <div className="mt-auto flex items-center gap-3">
        <div className="relative flex-1 min-w-0">
          <pre className="bg-[#0a0a0a] border border-[#1a1a2e] rounded-md py-2 pl-3 pr-10 overflow-x-auto text-xs font-[family-name:var(--font-geist-mono)] text-gray-300">
            <code>{installCmd}</code>
          </pre>
          <CopyButton text={installCmd} label={`Copy install command for ${pkg.name}`} />
        </div>
        {pkg.homepage && (
          <a
            href={pkg.homepage}
            target="_blank"
            rel="noopener noreferrer"
            className="shrink-0 text-xs text-gray-400 hover:text-white transition-colors font-[family-name:var(--font-geist-sans)]"
          >
            Homepage &#8599;
          </a>
        )}
      </div>
    </div>
  );
}

function EmptyState() {
  return (
    <div className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-10 text-center">
      <div className="mx-auto mb-5 flex h-14 w-14 items-center justify-center rounded-full border border-[#1a1a2e] bg-[#0a0a0a]">
        <svg
          width="26"
          height="26"
          viewBox="0 0 24 24"
          fill="none"
          stroke="#00ff88"
          strokeWidth="1.75"
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
        >
          <path d="M21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16z" />
          <path d="M3.27 6.96 12 12.01l8.73-5.05" />
          <path d="M12 22.08V12" />
        </svg>
      </div>
      <h2 className="text-xl font-bold text-white mb-2">
        The index is brand new
      </h2>
      <p className="text-gray-400 max-w-md mx-auto mb-6">
        No packages have been published yet. The registry is a curated, static
        index — the first entry is a pull request away. Publish yours and it
        shows up here and in{" "}
        <code className="text-[#00ff88] bg-[#0a0a0a] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
          turbolang search
        </code>
        .
      </p>
      <a
        href="#publishing"
        className="inline-flex items-center gap-2 bg-[#00ff88] text-[#0a0a0a] font-semibold px-5 py-2.5 rounded-lg hover:bg-[#00cc6a] transition-colors"
      >
        Publish a package
      </a>
    </div>
  );
}

export default function PackagesPage() {
  const index = loadIndex();
  const packages = [...index.packages].sort((a, b) =>
    a.name.localeCompare(b.name)
  );

  return (
    <div className="max-w-5xl mx-auto px-6 py-16 md:py-20 font-[family-name:var(--font-geist-sans)]">
      <header className="mb-12">
        <h1 className="text-4xl md:text-5xl font-bold text-white mb-4">
          Packages
        </h1>
        <p className="text-lg text-gray-400 max-w-2xl">
          A static, git-versioned registry of Turbo libraries. Install any of
          them with{" "}
          <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
            turbolang install
          </code>
          , or find them from your terminal with{" "}
          <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
            turbolang search
          </code>
          .
        </p>
      </header>

      {packages.length > 0 ? (
        <>
          <p className="text-sm text-gray-500 mb-6">
            {packages.length} package{packages.length === 1 ? "" : "s"} in the
            index.
          </p>
          <div className="grid grid-cols-1 md:grid-cols-2 gap-5 mb-16">
            {packages.map((pkg) => (
              <PackageCard key={pkg.name} pkg={pkg} />
            ))}
          </div>
        </>
      ) : (
        <div className="mb-16">
          <EmptyState />
        </div>
      )}

      <section id="publishing" className="scroll-mt-24">
        <h2 className="text-2xl font-bold text-white mb-4">
          Publishing a package
        </h2>
        <p className="text-gray-400 mb-6 max-w-2xl">
          There is no account to create and no server to push to. The registry
          is a single JSON file in the Turbo repository, and publishing is an
          ordinary pull request that appends one entry.
        </p>

        <ol className="space-y-4 mb-8">
          {[
            {
              title: "Build a Turbo package",
              body: (
                <>
                  A package is a Git repository with a{" "}
                  <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
                    turbo.toml
                  </code>{" "}
                  and a{" "}
                  <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
                    src/lib.tb
                  </code>{" "}
                  entry point, tagged with a semver release (e.g.{" "}
                  <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
                    v0.1.0
                  </code>
                  ).
                </>
              ),
            },
            {
              title: "Add one entry to the index",
              body: (
                <>
                  Open a pull request against{" "}
                  <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
                    registry/index.json
                  </code>{" "}
                  in the Turbo repository, appending your package object to the{" "}
                  <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
                    packages
                  </code>{" "}
                  array.
                </>
              ),
            },
            {
              title: "Merge",
              body: (
                <>
                  Once merged, the entry is served at{" "}
                  <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
                    turbolang.dev/registry/index.json
                  </code>{" "}
                  and appears on this page and in{" "}
                  <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
                    turbolang search
                  </code>
                  .
                </>
              ),
            },
          ].map((step, i) => (
            <li key={step.title} className="flex gap-4">
              <span className="shrink-0 flex h-8 w-8 items-center justify-center rounded-full bg-[#00ff88]/10 text-[#00ff88] text-sm font-bold font-[family-name:var(--font-geist-mono)]">
                {i + 1}
              </span>
              <div>
                <h3 className="text-white font-semibold mb-1">{step.title}</h3>
                <p className="text-sm text-gray-400 leading-relaxed">
                  {step.body}
                </p>
              </div>
            </li>
          ))}
        </ol>

        <h3 className="text-lg font-bold text-white mb-3">Index entry schema</h3>
        <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
          <code>{`{
  "name": "turbo-json",              // unique; matches the turbo_modules dir name
  "repo": "owner/turbo-json",        // GitHub "owner/name"
  "description": "A fast JSON parser and serializer",
  "categories": ["serialization"],   // lowercase tags
  "min_turbo_version": "0.10.0",     // optional: semver the package needs
  "homepage": "https://example.com"  // optional
}`}</code>
        </pre>

        <p className="text-gray-400 max-w-2xl">
          Full details, including how{" "}
          <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
            install
          </code>
          /
          <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
            update
          </code>{" "}
          and lockfiles work, are in the{" "}
          <Link
            href="/docs/packages"
            className="text-[#00ff88] hover:underline"
          >
            packages documentation
          </Link>
          .
        </p>
      </section>
    </div>
  );
}
