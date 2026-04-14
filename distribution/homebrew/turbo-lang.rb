class TurboLang < Formula
  desc "Compiled, type-safe language with TypeScript DX and Rust performance"
  homepage "https://turbolang.dev"
  version "0.7.6"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.7.6/turbolang-v0.7.6-aarch64-apple-darwin.tar.gz"
      sha256 "4fc3a6872015afedb4c6865ec027d98c5234d5ab371403addb73a86bce240f2b"
    end
    on_intel do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.7.6/turbolang-v0.7.6-x86_64-apple-darwin.tar.gz"
      sha256 "0308b04dd4dca81d86fc402e907b1b50e140aa18e3d7a557d6294d9dc5adbe59"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.7.6/turbolang-v0.7.6-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "6ea083db8af2abea1273ec36ab75f14a5007d206f96bef3ff9f602b83ec1cc76"
    end
  end

  def install
    bin.install "turbolang"
    bin.install "turbo-lsp" if File.exist?("turbo-lsp")
  end

  test do
    assert_match "turbolang 0.7.6", shell_output("#{bin}/turbolang --version")
  end
end
