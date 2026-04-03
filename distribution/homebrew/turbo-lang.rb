class TurboLang < Formula
  desc "Compiled, type-safe language with TypeScript DX and Rust performance"
  homepage "https://github.com/ZVN-DEV/Turbo-Language"
  version "0.2.1"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.2.1/turbolang-v0.2.1-aarch64-apple-darwin.tar.gz"
      sha256 "f8c7b8e96f7001be068047c6d862c6896f4c2340517f10ad7ee99c90c3ba008b"
    end
    on_intel do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.2.1/turbolang-v0.2.1-x86_64-apple-darwin.tar.gz"
      sha256 "e87a93275f53295857fddf0454cf042b6041b48fe286e9f936bdd8c629cd36e6"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.2.1/turbolang-v0.2.1-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "e6f65894c71a5cfceb7732ec75b49190db4d896933a0b1f011839dd1f2966aab"
    end
  end

  def install
    bin.install "turbolang"
    bin.install "turbo-lsp" if File.exist?("turbo-lsp")
  end

  test do
    assert_match "turbolang 0.2.1", shell_output("#{bin}/turbolang --version")
  end
end
