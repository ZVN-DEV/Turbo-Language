import Link from "next/link";

export default function Navbar() {
  return (
    <header className="fixed top-0 left-0 right-0 z-50 border-b border-border bg-background/80 backdrop-blur-xl">
      <nav
        aria-label="Primary"
        className="max-w-6xl mx-auto px-6 h-16 flex items-center justify-between"
      >
        <Link
          href="/"
          className="text-xl font-bold bg-gradient-to-r from-accent to-accent-blue bg-clip-text text-transparent font-[family-name:var(--font-geist-sans)]"
        >
          Turbo
        </Link>

        <div className="flex items-center gap-4 sm:gap-8">
          <Link
            href="/docs"
            className="text-sm text-gray-400 hover:text-white transition-colors font-[family-name:var(--font-geist-sans)]"
          >
            Docs
          </Link>
          <Link
            href="/play"
            className="text-sm text-gray-400 hover:text-white transition-colors font-[family-name:var(--font-geist-sans)]"
          >
            Play
          </Link>
          <Link
            href="/docs/examples"
            className="text-sm text-gray-400 hover:text-white transition-colors font-[family-name:var(--font-geist-sans)]"
          >
            Examples
          </Link>
          <a
            href="https://github.com/ZVN-DEV/Turbo-Language"
            target="_blank"
            rel="noopener noreferrer"
            className="hidden items-center gap-1.5 text-sm text-gray-400 transition-colors hover:text-white sm:flex font-[family-name:var(--font-geist-sans)]"
          >
            GitHub
            <svg
              width="14"
              height="14"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
              strokeLinejoin="round"
            >
              <path d="M7 17L17 7M17 7H7M17 7V17" />
            </svg>
          </a>
        </div>
      </nav>
    </header>
  );
}
