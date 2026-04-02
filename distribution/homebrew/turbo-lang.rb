class TurboLang < Formula
  desc "Compiled, type-safe language with TypeScript DX, Rust performance, and AI agent primitives"
  homepage "https://github.com/ZVN-DEV/Turbo-Language"
  version "0.1.0"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.1.0/turbolang-v0.1.0-aarch64-apple-darwin.tar.gz"
      sha256 "71f87e437d0cc519899f6a657a7947fe76f956001266ae5ae7f2a8978a423383"
    end
  end

  def install
    bin.install "turbolang"
  end

  test do
    (testpath/"hello.tb").write <<~EOS
      fn main() {
          print("Hello from Turbo!")
      }
    EOS
    assert_equal "Hello from Turbo!\n", shell_output("#{bin}/turbolang run #{testpath}/hello.tb")
  end
end
