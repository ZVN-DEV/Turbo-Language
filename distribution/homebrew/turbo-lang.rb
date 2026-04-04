class TurboLang < Formula
  desc "Compiled, type-safe language with TypeScript DX and Rust performance"
  homepage "https://turbolang.dev"
  version "0.3.0"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.3.0/turbolang-v0.3.0-aarch64-apple-darwin.tar.gz"
      sha256 "e747b8354be28f66b60c05a8d5e64b59f59493acd822f20f3562bf778d666c31"
    end
    on_intel do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.3.0/turbolang-v0.3.0-x86_64-apple-darwin.tar.gz"
      sha256 "d40810c766cc3abcc7772874e7c762929551cb54ed778be28d83f83fc3bc5e4b"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.3.0/turbolang-v0.3.0-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "a33034d3e7b915d02dcbfad8658be929cfb5c7044c16e909d8240e73dd69ce21"
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
