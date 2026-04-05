class TurboLang < Formula
  desc "Compiled, type-safe language with TypeScript DX and Rust performance"
  homepage "https://turbolang.dev"
  version "0.3.2"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.3.2/turbolang-v0.3.2-aarch64-apple-darwin.tar.gz"
      sha256 "d58ea3fa629902146768c763a0409807766263ad29b0077dacb5df6f6ba23b7f"
    end
    on_intel do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.3.2/turbolang-v0.3.2-x86_64-apple-darwin.tar.gz"
      sha256 "165cecfe66566956aed7d5e19c677524abd18967495f8fa4396b98f2ce4ef637"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.3.2/turbolang-v0.3.2-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "8b7ed7557f633d34d71f4e20435b0519d9a0c510291ab8324cde72c51e7d9863"
    end
  end

  def install
    bin.install "turbolang"
    bin.install "turbo-lsp" if File.exist?("turbo-lsp")
  end

  test do
    assert_match "turbolang 0.3.2", shell_output("#{bin}/turbolang --version")
  end
end
