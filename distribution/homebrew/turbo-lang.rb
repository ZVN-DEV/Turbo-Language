class TurboLang < Formula
  desc "Compiled, type-safe language with TypeScript DX and Rust performance"
  homepage "https://turbolang.dev"
  version "0.6.0"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.6.0/turbolang-v0.6.0-aarch64-apple-darwin.tar.gz"
      sha256 "72f4a9a47a9e587a45dbb56e9bc0a1a95041ae26265b59d7bcf8ec6b8905c4be"
    end
    on_intel do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.6.0/turbolang-v0.6.0-x86_64-apple-darwin.tar.gz"
      sha256 "6268c78aa90b12600b04a119ab1db91f892aaa9efbd9a422eca7cc36ca7ba3af"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.6.0/turbolang-v0.6.0-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "851b7e892c152def9991dbb72dc2369116665358e5e77244d915af0c376ded71"
    end
  end

  def install
    bin.install "turbolang"
    bin.install "turbo-lsp" if File.exist?("turbo-lsp")
  end

  test do
    assert_match "turbolang 0.6.0", shell_output("#{bin}/turbolang --version")
  end
end
