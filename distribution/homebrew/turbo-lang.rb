class TurboLang < Formula
  desc "Compiled, type-safe language with TypeScript DX and Rust performance"
  homepage "https://turbolang.dev"
  version "0.3.1"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.3.1/turbolang-v0.3.1-aarch64-apple-darwin.tar.gz"
      sha256 "99c7a6455e54fd205d7c94466569be2ea2a0b9fcfb69897cc584ece75c574bff"
    end
    on_intel do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.3.1/turbolang-v0.3.1-x86_64-apple-darwin.tar.gz"
      sha256 "fb33cbbdf7ce6d7fe07344d9eb0e3022aee264e44b10d2e5061fb8bacebc1727"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.3.1/turbolang-v0.3.1-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "b758a788ae10781c2d5b063cfc1ba52146bc799c265441204f636c733b436aba"
    end
  end

  def install
    bin.install "turbolang"
    bin.install "turbo-lsp" if File.exist?("turbo-lsp")
  end

  test do
    assert_match "turbolang 0.3.1", shell_output("#{bin}/turbolang --version")
  end
end
