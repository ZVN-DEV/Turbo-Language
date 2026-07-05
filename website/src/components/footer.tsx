import Link from "next/link";

export default function Footer() {
  return (
    <footer className="border-t border-border bg-background">
      <div className="max-w-6xl mx-auto px-6 py-12">
        <div className="grid grid-cols-2 md:grid-cols-4 gap-8 mb-12">
          <div>
            <h3 className="text-sm font-semibold text-white mb-4 font-[family-name:var(--font-geist-sans)]">
              Language
            </h3>
            <ul className="space-y-2">
              <li>
                <Link
                  href="/docs"
                  className="text-sm text-gray-400 hover:text-gray-300 transition-colors"
                >
                  Documentation
                </Link>
              </li>
              <li>
                <Link
                  href="/docs/installation"
                  className="text-sm text-gray-400 hover:text-gray-300 transition-colors"
                >
                  Installation
                </Link>
              </li>
              <li>
                <Link
                  href="/docs/examples"
                  className="text-sm text-gray-400 hover:text-gray-300 transition-colors"
                >
                  Examples
                </Link>
              </li>
              <li>
                <Link
                  href="/play"
                  className="text-sm text-gray-400 hover:text-gray-300 transition-colors"
                >
                  Playground
                </Link>
              </li>
            </ul>
          </div>
          <div>
            <h3 className="text-sm font-semibold text-white mb-4 font-[family-name:var(--font-geist-sans)]">
              Toolchain
            </h3>
            <ul className="space-y-2">
              <li>
                <Link
                  href="/docs/cli"
                  className="text-sm text-gray-400 hover:text-gray-300 transition-colors"
                >
                  CLI
                </Link>
              </li>
              <li>
                <Link
                  href="/docs/testing"
                  className="text-sm text-gray-400 hover:text-gray-300 transition-colors"
                >
                  Testing
                </Link>
              </li>
              <li>
                <Link
                  href="/docs/cli#lsp"
                  className="text-sm text-gray-400 hover:text-gray-300 transition-colors"
                >
                  LSP
                </Link>
              </li>
            </ul>
          </div>
          <div>
            <h3 className="text-sm font-semibold text-white mb-4 font-[family-name:var(--font-geist-sans)]">
              Community
            </h3>
            <ul className="space-y-2">
              <li>
                <a
                  href="https://github.com/ZVN-DEV/Turbo-Language"
                  target="_blank"
                  rel="noopener noreferrer"
                  className="text-sm text-gray-400 hover:text-gray-300 transition-colors"
                >
                  GitHub
                </a>
              </li>
              <li>
                <a
                  href="https://github.com/ZVN-DEV/Turbo-Language/issues"
                  target="_blank"
                  rel="noopener noreferrer"
                  className="text-sm text-gray-400 hover:text-gray-300 transition-colors"
                >
                  Issues
                </a>
              </li>
            </ul>
          </div>
          <div>
            <h3 className="text-sm font-semibold text-white mb-4 font-[family-name:var(--font-geist-sans)]">
              Ecosystem
            </h3>
            <ul className="space-y-2">
              <li>
                <a
                  href="https://github.com/ZVN-DEV/turbo-vscode"
                  target="_blank"
                  rel="noopener noreferrer"
                  className="text-sm text-gray-400 hover:text-gray-300 transition-colors"
                >
                  VS Code Extension
                </a>
              </li>
              <li>
                <a
                  href="https://github.com/ZVN-DEV/tree-sitter-turbo"
                  target="_blank"
                  rel="noopener noreferrer"
                  className="text-sm text-gray-400 hover:text-gray-300 transition-colors"
                >
                  Tree-sitter Grammar
                </a>
              </li>
            </ul>
          </div>
        </div>
        <div className="border-t border-border pt-8 flex flex-col md:flex-row items-center justify-between gap-4">
          <p className="text-sm text-gray-400 font-[family-name:var(--font-geist-sans)]">
            Built with Cranelift. A small, honest core.
          </p>
          <p className="text-sm text-gray-400 font-[family-name:var(--font-geist-sans)]">
            &copy; {new Date().getFullYear()} Turbo Language
          </p>
        </div>
      </div>
    </footer>
  );
}
