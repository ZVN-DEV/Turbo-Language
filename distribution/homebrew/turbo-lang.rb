class TurboLang < Formula
  desc "Compiled, type-safe language with TypeScript DX and Rust performance"
  homepage "https://turbolang.dev"
  version "0.7.2"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.7.2/turbolang-v0.7.2-aarch64-apple-darwin.tar.gz"
      sha256 "25619eb5394ccebc8621cadf3530325a6bdc01d2c05c78554195e33fd54ad8c8"
    end
    on_intel do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.7.2/turbolang-v0.7.2-x86_64-apple-darwin.tar.gz"
      sha256 "72c21e9c2e52ca89271139395636efb3e5bb4a1ed8d8e8bec951bde9ec2e1236"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.7.2/turbolang-v0.7.2-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "bf6c1a9cfd3413807915921a8305795d7424c7206c8c4bbc6c8ddd95991ba2ec"
    end
  end

  def install
    bin.install "turbolang"
    bin.install "turbo-lsp" if File.exist?("turbo-lsp")
  end

  test do
    assert_match "turbolang 0.7.2", shell_output("#{bin}/turbolang --version")
  end
end
