class TurboLang < Formula
  desc "Compiled, type-safe language with TypeScript DX and Rust performance"
  homepage "https://turbolang.dev"
  version "0.2.2"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.2.2/turbolang-v0.2.2-aarch64-apple-darwin.tar.gz"
      sha256 "5089cc847cffded53f1c866a9f7e4649abc9da5958747658087a4a646572ac17"
    end
    on_intel do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.2.2/turbolang-v0.2.2-x86_64-apple-darwin.tar.gz"
      sha256 "58573c0af564901d75ad752f6cf6376be39c603a0ee7f39b4fac46ad764387af"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.2.2/turbolang-v0.2.2-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "a8ae6a6c530070c7b355a58d3dc4270e971ab4d7064c6276fb93855d0ee6ac2d"
    end
  end

  def install
    bin.install "turbolang"
    bin.install "turbo-lsp" if File.exist?("turbo-lsp")
  end

  test do
    assert_match "turbolang 0.2.2", shell_output("#{bin}/turbolang --version")
  end
end
