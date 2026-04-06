class TurboLang < Formula
  desc "Compiled, type-safe language with TypeScript DX and Rust performance"
  homepage "https://turbolang.dev"
  version "0.4.0"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.4.0/turbolang-v0.4.0-aarch64-apple-darwin.tar.gz"
      sha256 "fbea2a29b6e4da0a0dd0a32472197f43953138efd168e4c54189fa85ccf31bd5"
    end
    on_intel do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.4.0/turbolang-v0.4.0-x86_64-apple-darwin.tar.gz"
      sha256 "a23420527fdd78249d1612739c392821fb9c2a1c6c50bd9fcb71d0fe2fad24f1"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.4.0/turbolang-v0.4.0-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "6ed09b56a2370389bcc6ce7115806f229bcfd1c497d72ae6cd3fb79768d3baec"
    end
  end

  def install
    bin.install "turbolang"
    bin.install "turbo-lsp" if File.exist?("turbo-lsp")
  end

  test do
    assert_match "turbolang 0.4.0", shell_output("#{bin}/turbolang --version")
  end
end
