class TurboLang < Formula
  desc "Compiled, type-safe language with TypeScript DX and Rust performance"
  homepage "https://turbolang.dev"
  version "0.9.1"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.9.1/turbolang-v0.9.1-aarch64-apple-darwin.tar.gz"
      sha256 "24e87a53d993bc6b532770b02c7edbac42a9185648cae56155a242cfa6fab843"
    end
    on_intel do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.9.1/turbolang-v0.9.1-x86_64-apple-darwin.tar.gz"
      sha256 "068d1f55c48ebd7f61cf38793c64b5ba5274af9ddb802b8963bd0a2bec4b20b2"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.9.1/turbolang-v0.9.1-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "bea85e814b5b89ea278b957a89020c163aa6046d17b0b04e72dc01a8916c394d"
    end
  end

  def install
    bin.install "turbolang"
    bin.install "turbo-lsp" if File.exist?("turbo-lsp")
  end

  test do
    assert_match "turbolang 0.9.1", shell_output("#{bin}/turbolang --version")
    assert_predicate bin/"turbo-lsp", :exist?
  end
end
