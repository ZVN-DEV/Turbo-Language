class TurboLang < Formula
  desc "Compiled, type-safe language with TypeScript DX and Rust performance"
  homepage "https://turbolang.dev"
  version "0.7.7"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.7.7/turbolang-v0.7.7-aarch64-apple-darwin.tar.gz"
      sha256 "ecc6be4223bf537e609542c87b844905ab3d0a6c9d8caca059f07302cdda816c"
    end
    on_intel do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.7.7/turbolang-v0.7.7-x86_64-apple-darwin.tar.gz"
      sha256 "cdb06b980cf680ce3bd6773e437fae276068169dddeb0581f779282ae71c5dd9"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.7.7/turbolang-v0.7.7-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "cee1b239fcf8dedc3f1f743b87ccd1293f44c2b394891dbffd773d4d31f2043a"
    end
  end

  def install
    bin.install "turbolang"
    bin.install "turbo-lsp" if File.exist?("turbo-lsp")
  end

  test do
    assert_match "turbolang 0.7.7", shell_output("#{bin}/turbolang --version")
  end
end
