class TurboLang < Formula
  desc "Compiled, type-safe language with TypeScript DX and Rust performance"
  homepage "https://turbolang.dev"
  version "0.7.3"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.7.3/turbolang-v0.7.3-aarch64-apple-darwin.tar.gz"
      sha256 "f515bf3ba38488bdda2a38e10a3b265a312b8c953d6b988c84e29487c9eb2512"
    end
    on_intel do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.7.3/turbolang-v0.7.3-x86_64-apple-darwin.tar.gz"
      sha256 "e9c234224b7f4ee89f0ee43abf35aac1e80413964b459d2c2aa8951eb8476b59"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.7.3/turbolang-v0.7.3-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "16ef4d3cfc56cc98793c37ddf2bf23f15d745952a97599f59ce2af30fd861b70"
    end
  end

  def install
    bin.install "turbolang"
    bin.install "turbo-lsp" if File.exist?("turbo-lsp")
  end

  test do
    assert_match "turbolang 0.7.3", shell_output("#{bin}/turbolang --version")
  end
end
