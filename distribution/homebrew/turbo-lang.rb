class TurboLang < Formula
  desc "Compiled, type-safe language with TypeScript DX and Rust performance"
  homepage "https://turbolang.dev"
  version "0.3.0"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.3.0/turbolang-v0.3.0-aarch64-apple-darwin.tar.gz"
      sha256 "PLACEHOLDER_SHA256_ARM_MACOS"
    end
    on_intel do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.3.0/turbolang-v0.3.0-x86_64-apple-darwin.tar.gz"
      sha256 "PLACEHOLDER_SHA256_INTEL_MACOS"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.3.0/turbolang-v0.3.0-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "PLACEHOLDER_SHA256_LINUX"
    end
  end

  def install
    bin.install "turbolang"
    bin.install "turbo-lsp" if File.exist?("turbo-lsp")
  end

  test do
    assert_match "turbolang 0.3.0", shell_output("#{bin}/turbolang --version")
  end
end
