import Link from "next/link";

export default function InstallationPage() {
  return (
    <article className="text-gray-300 leading-relaxed font-[family-name:var(--font-geist-sans)]">
      <h1 className="text-4xl font-bold text-white mb-4">Installation</h1>
      <p className="text-lg text-gray-400 mb-8">
        Get Turbo up and running on your machine.
      </p>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Prerequisites
      </h2>
      <ul className="list-disc list-inside space-y-2 mb-6">
        <li>
          <strong className="text-white">Rust</strong> (stable toolchain) --{" "}
          <a href="https://rustup.rs" className="text-[#00ff88] hover:underline">
            rustup.rs
          </a>
        </li>
        <li>
          <strong className="text-white">C compiler</strong> -- gcc or clang (for
          linking AOT binaries)
        </li>
      </ul>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Option 1: Homebrew (macOS)
      </h2>
      <p className="mb-4">The fastest way to install on macOS:</p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`brew tap ZVN-DEV/turbo
brew install turbo-lang`}</code>
      </pre>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Option 2: Build from Source
      </h2>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`# Clone the repository
git clone https://github.com/ZVN-DEV/Turbo-Language.git
cd turbo-lang/turbo

# Build in release mode
cargo build --release

# The binary is at target/release/turbo`}</code>
      </pre>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Option 3: Build with LLVM Backend (Optional)
      </h2>
      <p className="mb-4">
        Turbo includes an optional LLVM backend in addition to the default
        Cranelift backend. This requires LLVM 18 to be installed on your system.
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`# Install LLVM 18 (macOS)
brew install llvm@18

# Build Turbo with LLVM support enabled
cd turbo-lang/turbo
cargo build --release --features llvm

# Use the LLVM backend with the --llvm flag
./target/release/turbo build --llvm hello.tb`}</code>
      </pre>
      <p className="text-sm text-gray-500 mb-6">
        The LLVM backend is fully functional (116/116 tests pass) but requires
        LLVM 18 as an external dependency. Cranelift is the default and requires
        no extra setup.
      </p>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Add to PATH
      </h2>
      <p className="mb-4">
        Add the Turbo binary to your shell&apos;s PATH so you can use it from
        anywhere:
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`# Add to ~/.bashrc, ~/.zshrc, or equivalent
export PATH="$HOME/turbo-lang/turbo/target/release:$PATH"`}</code>
      </pre>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Verify Installation
      </h2>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`turbo --version`}</code>
      </pre>
      <p className="mb-6">
        You should see the current version number printed to the terminal.
      </p>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Editor Support
      </h2>
      <p className="mb-4">
        Install the{" "}
        <strong className="text-white">Turbo VS Code extension</strong> for
        syntax highlighting, snippets, and LSP integration:
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`# Search "Turbo Language" in the VS Code Extensions panel
# Or install from the command line:
code --install-extension zvndev.turbo-lang`}</code>
      </pre>

      <div className="flex gap-4 mt-10">
        <Link
          href="/docs/hello-world"
          className="inline-flex items-center gap-2 bg-[#00ff88] text-[#0a0a0a] font-semibold px-6 py-3 rounded-lg hover:bg-[#00cc6a] transition-colors"
        >
          Next: Hello World
          <span>&#8594;</span>
        </Link>
      </div>
    </article>
  );
}
