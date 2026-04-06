class TurboLang < Formula
  desc "Compiled, type-safe language with TypeScript DX and Rust performance"
  homepage "https://turbolang.dev"
  version "0.5.0"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.5.0/turbolang-v0.5.0-aarch64-apple-darwin.tar.gz"
      sha256 "3ff866d27ff9a44af12395e83cb1d276b0e2900dc3777ff0cc9266c126d42c4a"
    end
    on_intel do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.5.0/turbolang-v0.5.0-x86_64-apple-darwin.tar.gz"
      sha256 "52a5005627b8abc5342119e62dbe5935d32e710fe7535e7a91f41a0aaa964027"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.5.0/turbolang-v0.5.0-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "373843180107b89b380714a7fa8a684a224014dfc9654b6dee239de2d22d058a"
    end
  end

  def install
    bin.install "turbolang"
    bin.install "turbo-lsp" if File.exist?("turbo-lsp")
  end

  test do
    assert_match "turbolang 0.5.0", shell_output("#{bin}/turbolang --version")
  end
end
