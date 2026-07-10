# NON-AUTHORITATIVE TEMPLATE — DO NOT TRUST THE sha256 VALUES BELOW.
#
# The AUTHORITATIVE FORMULA LIVES IN THE ZVN-DEV/homebrew-turbo TAP; these shas
# are placeholders. Real, verified checksums are only known after the release
# artifacts are built, so .github/workflows/release.yml regenerates the formula
# with the true sha256 values and pushes it to that tap at release time. Users
# install via `brew install zvn-dev/turbo/turbo-lang`, which pulls the real
# formula from the tap — never this file.
#
# The sha256 fields below are the all-zero sentinel on purpose: an unmistakable
# placeholder that cannot be confused for a verified checksum. This is enforced
# by scripts/check_release_consistency.py (Homebrew template policy) so this
# mirror can never silently drift into looking like a real, trustworthy formula.
class TurboLang < Formula
  desc "Compiled, type-safe language with TypeScript DX and Rust performance"
  homepage "https://turbolang.dev"
  version "0.15.0"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.15.0/turbolang-v0.15.0-aarch64-apple-darwin.tar.gz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000"
    end
    on_intel do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.15.0/turbolang-v0.15.0-x86_64-apple-darwin.tar.gz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v0.15.0/turbolang-v0.15.0-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000"
    end
  end

  def install
    bin.install "turbolang"
    bin.install "turbo-lsp"
  end

  test do
    assert_match "turbolang 0.15.0", shell_output("#{bin}/turbolang --version")
    assert_predicate bin/"turbo-lsp", :exist?
  end
end
