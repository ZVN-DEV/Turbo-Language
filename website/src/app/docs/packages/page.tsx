import type { Metadata } from "next";
import Link from "next/link";

export const metadata: Metadata = {
  title: "Packages",
  description:
    "Install, update, and publish Turbo packages. How turbo.toml dependencies, turbo.lock pinning, registry resolution, and the PR-based publishing flow work.",
};

const code =
  "text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]";
const pre =
  "bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300";

export default function PackagesDocsPage() {
  return (
    <article className="text-gray-300 leading-relaxed font-[family-name:var(--font-geist-sans)]">
      <h1 className="text-4xl font-bold text-white mb-4">Packages</h1>
      <p className="text-lg text-gray-400 mb-8">
        Turbo has a small, built-in package manager. Dependencies are declared
        in <code className={code}>turbo.toml</code>, pinned in{" "}
        <code className={code}>turbo.lock</code>, and installed into a local{" "}
        <code className={code}>turbo_modules/</code> directory. There is no
        central package server — packages are Git repositories, and the registry
        is a curated static index.
      </p>

      <div className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-6 mb-8">
        <p className="text-[#00ff88] font-medium mb-0">
          Browse published packages at{" "}
          <Link href="/packages" className="underline">
            turbolang.dev/packages
          </Link>{" "}
          or from the terminal with{" "}
          <code className="text-[#00ff88] font-[family-name:var(--font-geist-mono)]">
            turbolang search
          </code>
          .
        </p>
      </div>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Declaring dependencies
      </h2>
      <p className="mb-4">
        Add dependencies under <code className={code}>[dependencies]</code> (or{" "}
        <code className={code}>[dev-dependencies]</code>) in{" "}
        <code className={code}>turbo.toml</code>. Three source forms are
        supported:
      </p>
      <pre className={pre}>
        <code>{`[dependencies]
# 1. Registry version — resolved to a GitHub repo + a matching git tag
turbo-http = "0.1"

# 2. GitHub repo — pin by tag range (version), exact commit (rev), or lockfile
agent-kit = { github = "owner/agent-kit", version = "1.2" }
fixed     = { github = "owner/fixed", rev = "a1b2c3d" }

# 3. Local path — symlinked into turbo_modules (great for workspaces)
mylib = { path = "../mylib" }

[dev-dependencies]
turbo-test-utils = "0.1"`}</code>
      </pre>
      <p className="mb-4">
        A version like <code className={code}>&quot;0.1&quot;</code> selects the
        latest <code className={code}>0.1.x</code> tag; a full{" "}
        <code className={code}>&quot;0.1.0&quot;</code> pins that exact tag. Tags
        may be written with or without a leading{" "}
        <code className={code}>v</code>.
      </p>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Installing
      </h2>
      <pre className={pre}>
        <code>{`turbolang install`}</code>
      </pre>
      <p className="mb-4">
        <code className={code}>install</code> reads{" "}
        <code className={code}>turbo.toml</code>, resolves every dependency, and
        places it under <code className={code}>turbo_modules/&lt;name&gt;</code>:
      </p>
      <ul className="list-disc list-inside space-y-2 mb-6">
        <li>
          <strong className="text-white">Path</strong> dependencies are
          symlinked (copied on non-Unix).
        </li>
        <li>
          <strong className="text-white">GitHub</strong> and{" "}
          <strong className="text-white">registry</strong> dependencies are
          cloned and checked out at the resolved commit, then recorded in{" "}
          <code className={code}>turbo.lock</code>.
        </li>
        <li>
          If a dependency is already present, it is re-pinned to the resolved
          commit rather than re-cloned.
        </li>
      </ul>
      <p className="mb-4">
        In your source, import a package by its name — the resolver looks for{" "}
        <code className={code}>turbo_modules/&lt;name&gt;/src/lib.tb</code> (then{" "}
        <code className={code}>src/&lt;name&gt;.tb</code>):
      </p>
      <pre className={pre}>
        <code>{`import turbo_http`}</code>
      </pre>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        The lockfile
      </h2>
      <p className="mb-4">
        <code className={code}>turbo.lock</code> pins every Git-backed
        dependency to an exact commit for reproducible installs. It is generated
        and overwritten by <code className={code}>install</code> and{" "}
        <code className={code}>update</code>, and looks like:
      </p>
      <pre className={pre}>
        <code>{`[github]
turbo-http = "ZVN-DEV/turbo-http#a1b2c3d4e5f6..."`}</code>
      </pre>
      <p className="mb-4">
        When a dependency lists neither a <code className={code}>rev</code> nor a{" "}
        <code className={code}>version</code>, <code className={code}>install</code>{" "}
        reuses the commit recorded here — so committing{" "}
        <code className={code}>turbo.lock</code> gives your collaborators the
        exact same dependency tree. Commit it for applications; libraries
        typically leave it out.
      </p>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">Updating</h2>
      <pre className={pre}>
        <code>{`turbolang update`}</code>
      </pre>
      <p className="mb-6">
        <code className={code}>update</code> moves Git-backed dependencies to the
        newest commit allowed by their declaration (the latest tag for a{" "}
        <code className={code}>version</code>, a fast-forward pull for a bare
        GitHub repo) and rewrites <code className={code}>turbo.lock</code>. Path
        dependencies are left untouched. Run{" "}
        <code className={code}>install</code> first if a dependency isn&apos;t
        present yet.
      </p>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Registry resolution
      </h2>
      <p className="mb-4">
        A bare registry dependency (form 1) is a name, not a repo. Turbo maps the
        name to a GitHub repo by checking, in order:
      </p>
      <ol className="list-decimal list-inside space-y-2 mb-6">
        <li>
          An explicit <code className={code}>[registries]</code> entry in your{" "}
          <code className={code}>turbo.toml</code> (a per-project override).
        </li>
        <li>
          The published registry index at{" "}
          <code className={code}>turbolang.dev/registry/index.json</code>.
        </li>
        <li>
          The built-in default: any <code className={code}>turbo-*</code> name
          maps to <code className={code}>ZVN-DEV/&lt;name&gt;</code>.
        </li>
      </ol>
      <pre className={pre}>
        <code>{`[registries]
# Point a name at any repo you control
cool-lib = "myorg/cool-lib"`}</code>
      </pre>
      <p className="mb-6">
        If the index can&apos;t be reached, resolution simply falls back to the
        built-in default — a registry outage never blocks an install.
      </p>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Finding packages
      </h2>
      <pre className={pre}>
        <code>{`turbolang search json        # match name, description, or category
turbolang search             # list every published package`}</code>
      </pre>
      <p className="mb-6">
        <code className={code}>search</code> fetches the published index and
        filters it case-insensitively, printing each match with a ready-to-run{" "}
        <code className={code}>turbolang install</code> command.
      </p>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Publishing a package
      </h2>
      <p className="mb-4">
        The registry is a single JSON file in the Turbo repository. Publishing is
        an ordinary pull request — no account, no upload:
      </p>
      <ol className="list-decimal list-inside space-y-2 mb-6">
        <li>
          Ship a Git repo with a <code className={code}>turbo.toml</code> and a{" "}
          <code className={code}>src/lib.tb</code> entry point, tagged with a
          semver release (e.g. <code className={code}>v0.1.0</code>).
        </li>
        <li>
          Open a PR against <code className={code}>registry/index.json</code>{" "}
          appending one entry to the <code className={code}>packages</code> array.
        </li>
        <li>
          On merge it&apos;s served at{" "}
          <code className={code}>turbolang.dev/registry/index.json</code> and
          appears on the{" "}
          <Link href="/packages" className="text-[#00ff88] hover:underline">
            packages page
          </Link>{" "}
          and in <code className={code}>turbolang search</code>.
        </li>
      </ol>
      <pre className={pre}>
        <code>{`{
  "name": "turbo-json",              // unique; matches the turbo_modules dir name
  "repo": "owner/turbo-json",        // GitHub "owner/name"
  "description": "A fast JSON parser and serializer",
  "categories": ["serialization"],   // lowercase tags
  "min_turbo_version": "0.10.0",     // optional: semver the package needs
  "homepage": "https://example.com"  // optional
}`}</code>
      </pre>

      <div className="flex gap-4 mt-10">
        <Link
          href="/packages"
          className="inline-flex items-center gap-2 bg-[#00ff88] text-[#0a0a0a] font-semibold px-6 py-3 rounded-lg hover:bg-[#00cc6a] transition-colors"
        >
          Browse packages
          <span>&#8594;</span>
        </Link>
      </div>
    </article>
  );
}
