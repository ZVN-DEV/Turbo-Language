class TurboLang < Formula
  desc "Compiled, type-safe language with TypeScript DX and Rust performance"
  homepage "https://turbolang.dev"
  version "0.7.1"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.7.1/turbolang-v0.7.1-aarch64-apple-darwin.tar.gz"
      sha256 "5c7f6ea0a7eed3d61baccada2e1955efe025eb4e9db3fcdeea94d9fc733731eb"
    end
    on_intel do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.7.1/turbolang-v0.7.1-x86_64-apple-darwin.tar.gz"
      sha256 "89c289b3f93838a4c3c12f95165cfc346ae4753bac6789407f47dbb43993524e"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.7.1/turbolang-v0.7.1-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "5453247791854a077b8435a540f7eba857aff1e79e20423ade61d76db217d929"
    end
  end

  def install
    bin.install "turbolang"
    bin.install "turbo-lsp" if File.exist?("turbo-lsp")
  end

  test do
    assert_match "turbolang 0.7.1", shell_output("#{bin}/turbolang --version")
  end
end
