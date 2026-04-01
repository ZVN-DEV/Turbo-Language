import Link from "next/link";

const sections = [
  {
    title: "Getting Started",
    links: [
      { href: "/docs", label: "Introduction" },
      { href: "/docs/installation", label: "Installation" },
      { href: "/docs/hello-world", label: "Hello World" },
    ],
  },
  {
    title: "Language Guide",
    links: [
      { href: "/docs/variables-types", label: "Variables & Types" },
      { href: "/docs/functions", label: "Functions" },
      { href: "/docs/control-flow", label: "Control Flow" },
      { href: "/docs/structs-enums", label: "Structs & Enums" },
      { href: "/docs/traits-generics", label: "Traits & Generics" },
      { href: "/docs/control-flow#pattern-matching", label: "Pattern Matching" },
      { href: "/docs/functions#closures", label: "Closures" },
      { href: "/docs/async", label: "Async & Concurrency" },
      { href: "/docs/structs-enums#error-handling", label: "Error Handling" },
      { href: "/docs/variables-types#collections", label: "Collections" },
      { href: "/docs/agents", label: "Agents" },
    ],
  },
  {
    title: "Toolchain",
    links: [
      { href: "/docs/cli", label: "CLI Reference" },
      { href: "/docs/testing", label: "Testing" },
      { href: "/docs/cli#formatting", label: "Formatting" },
      { href: "/docs/cli#repl", label: "REPL" },
      { href: "/docs/cli#lsp", label: "LSP" },
    ],
  },
  {
    title: "Reference",
    links: [
      { href: "/docs/error-codes", label: "Error Codes" },
      { href: "/docs/cli#builtins", label: "Built-in Functions" },
      { href: "/docs/examples", label: "Examples" },
    ],
  },
  {
    title: "Community",
    links: [
      {
        href: "https://github.com/ZVN-DEV/turbo-lang",
        label: "GitHub",
        external: true,
      },
    ],
  },
];

export default function DocsLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <div className="flex min-h-screen bg-[#0a0a0a]">
      {/* Sidebar */}
      <aside className="hidden md:block w-[250px] shrink-0 border-r border-[#1a1a2e] bg-[#0a0a0a] sticky top-0 h-screen overflow-y-auto">
        <nav className="p-6 pt-8">
          <Link
            href="/docs"
            className="text-[#00ff88] font-bold text-lg font-[family-name:var(--font-geist-sans)] mb-8 block"
          >
            Turbo Docs
          </Link>
          {sections.map((section) => (
            <div key={section.title} className="mb-6">
              <h3 className="text-xs font-semibold uppercase tracking-wider text-gray-500 mb-2 font-[family-name:var(--font-geist-sans)]">
                {section.title}
              </h3>
              <ul className="space-y-1">
                {section.links.map((link) => (
                  <li key={link.href + link.label}>
                    {"external" in link && link.external ? (
                      <a
                        href={link.href}
                        target="_blank"
                        rel="noopener noreferrer"
                        className="block px-3 py-1.5 text-sm text-gray-400 hover:text-[#00ff88] rounded-md transition-colors font-[family-name:var(--font-geist-sans)]"
                      >
                        {link.label}
                        <span className="ml-1 text-xs">&#8599;</span>
                      </a>
                    ) : (
                      <Link
                        href={link.href}
                        className="block px-3 py-1.5 text-sm text-gray-400 hover:text-[#00ff88] hover:bg-[#111118] rounded-md transition-colors font-[family-name:var(--font-geist-sans)]"
                      >
                        {link.label}
                      </Link>
                    )}
                  </li>
                ))}
              </ul>
            </div>
          ))}
        </nav>
      </aside>

      {/* Mobile sidebar */}
      <div className="md:hidden w-full border-b border-[#1a1a2e] bg-[#0a0a0a] overflow-x-auto">
        <nav className="flex gap-4 px-4 py-3 min-w-max">
          {sections.flatMap((section) =>
            section.links
              .filter((link) => !("external" in link))
              .map((link) => (
                <Link
                  key={link.href + link.label}
                  href={link.href}
                  className="text-sm text-gray-400 hover:text-[#00ff88] whitespace-nowrap font-[family-name:var(--font-geist-sans)]"
                >
                  {link.label}
                </Link>
              ))
          )}
        </nav>
      </div>

      {/* Main content */}
      <main className="flex-1 min-w-0">
        <div className="max-w-3xl mx-auto px-6 md:px-12 py-12 md:py-16">
          {children}
        </div>
      </main>
    </div>
  );
}
