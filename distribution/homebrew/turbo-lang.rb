class TurboLang < Formula
  desc "Compiled, type-safe language with TypeScript DX and Rust performance"
  homepage "https://turbolang.dev"
  version "0.5.1"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.5.1/turbolang-v0.5.1-aarch64-apple-darwin.tar.gz"
      sha256 "fa45798828a3417065424e0cc58980ad1fad0f51aa10a5971268f495b9d51665"
    end
    on_intel do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.5.1/turbolang-v0.5.1-x86_64-apple-darwin.tar.gz"
      sha256 "e33a5175fdaab353f3e52bd930a1ba040120c8c243925a6e0f915b3d02076b0c"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.5.1/turbolang-v0.5.1-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "8f25132ea0ab390f30d44eb626a2f90ef067bf98cad87d3db2b99102b37cc6d0"
    end
  end

  def install
    bin.install "turbolang"
    bin.install "turbo-lsp" if File.exist?("turbo-lsp")
  end

  test do
    assert_match "turbolang 0.5.1", shell_output("#{bin}/turbolang --version")
  end
end
