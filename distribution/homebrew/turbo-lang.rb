class TurboLang < Formula
  desc "Compiled, type-safe language with TypeScript DX and Rust performance"
  homepage "https://github.com/ZVN-DEV/Turbo-Language"
  version "0.2.0"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.2.0/turbolang-v0.2.0-aarch64-apple-darwin.tar.gz"
      sha256 "978a46ecc5cba2efcea54020be3efef68fb323b0150d56e640b352b3fc22d4a0"
    end
    on_intel do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.2.0/turbolang-v0.2.0-x86_64-apple-darwin.tar.gz"
      sha256 "4100d6fdd73d97e30cc9c9dc311ed2ccac6a464aa954f6f21c5b3325109d0400"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.2.0/turbolang-v0.2.0-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "5206070c553f46f7c099c0b12e4bc14877ebb18434f9aaaec63489c7c09ade02"
    end
  end

  bottle do
    root_url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.2.0"
    sha256 cellar: :any_skip_relocation, arm64_sequoia: "2416226d6a2726d1164c5700388eac22afc2a329172264665c465fe12f9d9cd7"
    sha256 cellar: :any_skip_relocation, sequoia:       "98197cf2e6239bec411e7b7471ed59dcc5b43035b5bbf804221c662bf8981a64"
    sha256 cellar: :any_skip_relocation, x86_64_linux:  "600c57ac9ccf59df01234abeabb0bb1216a233262f12545f5989b0fa1da9913e"
  end

  def install
    bin.install "turbolang"
  end

  test do
    assert_match "turbolang 0.2.0", shell_output("#{bin}/turbolang --version")
  end
end
