class TurboLang < Formula
  desc "Compiled, type-safe language with TypeScript DX and Rust performance"
  homepage "https://turbolang.dev"
  version "0.7.5"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.7.5/turbolang-v0.7.5-aarch64-apple-darwin.tar.gz"
      sha256 "e9228269a7dfda963a191dadf48f4047f0acaf1d5add20cc060d24b3614e103f"
    end
    on_intel do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.7.5/turbolang-v0.7.5-x86_64-apple-darwin.tar.gz"
      sha256 "d7a04bb0b72da73e400f0131683c7fe91406ea64d5478b10ff9e555fc661e833"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.7.5/turbolang-v0.7.5-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "94bed3cc5b02aeed42f7800417287161aabd3bae6f5586866f2ea36900d24e4e"
    end
  end

  def install
    bin.install "turbolang"
    bin.install "turbo-lsp" if File.exist?("turbo-lsp")
  end

  test do
    assert_match "turbolang 0.7.5", shell_output("#{bin}/turbolang --version")
  end
end
