class TurboLang < Formula
  desc "Compiled, type-safe language with TypeScript DX, Rust performance, and AI agent primitives"
  homepage "https://github.com/ZVN-DEV/Turbo-Language"
  url "https://github.com/ZVN-DEV/Turbo-Language/archive/refs/tags/v0.2.0.tar.gz"
  sha256 "6c205118c32465e0c8ca2b001c53e35526c17774518e70b6276ba52401a7327b"
  version "0.2.0"
  license "MIT"
  head "https://github.com/ZVN-DEV/Turbo-Language.git", branch: "master"

  depends_on "rust" => :build

  def install
    cd "turbo" do
      system "cargo", "build", "--release", "-p", "turbo-cli", "-p", "turbo-lsp"
      bin.install "target/release/turbolang"
      bin.install "target/release/turbo-lsp"
    end
  end

  test do
    (testpath/"hello.tb").write <<~TURBO
      fn main() {
          print("Hello, Turbo!")
      }
    TURBO
    assert_match "Hello, Turbo!", shell_output("#{bin}/turbolang run #{testpath}/hello.tb")
  end
end
