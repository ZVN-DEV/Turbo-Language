#!/bin/bash
# Turbo Language Installer
# Usage: curl -fsSL https://raw.githubusercontent.com/ZVN-DEV/Turbo-Language/master/distribution/install.sh | bash
# Specific version: VERSION=0.2.0 curl -fsSL ... | bash
# Or: curl -fsSL ... | bash -s -- --version 0.2.0
# With signed-manifest verification: curl -fsSL ... | bash -s -- --verify

set -euo pipefail

INSTALL_DIR="/usr/local/bin"

# Parse arguments
while [[ $# -gt 0 ]]; do
    case "$1" in
        --version)
            VERSION="$2"
            shift 2
            ;;
        --version=*)
            VERSION="${1#*=}"
            shift
            ;;
        --verify)
            VERIFY_GPG=true
            shift
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

# If VERSION not set via env or flag, fetch latest from GitHub API
if [ -z "${VERSION:-}" ]; then
    echo "Fetching latest release version..."
    VERSION=$(curl -fsSL https://api.github.com/repos/ZVN-DEV/Turbo-Language/releases/latest | grep '"tag_name"' | sed 's/.*"v\(.*\)".*/\1/')
    if [ -z "${VERSION}" ]; then
        echo "Error: Could not determine latest version from GitHub API."
        echo "Try specifying a version: VERSION=0.1.0 bash install.sh"
        exit 1
    fi
fi

echo "Installing Turbo v${VERSION}..."

# Detect platform
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)

case "${OS}-${ARCH}" in
    darwin-arm64)
        TARGET="aarch64-apple-darwin"
        ;;
    darwin-x86_64)
        TARGET="x86_64-apple-darwin"
        ;;
    linux-x86_64)
        TARGET="x86_64-unknown-linux-gnu"
        ;;
    *)
        echo "Error: Unsupported platform: ${OS}-${ARCH}"
        echo "Install from source: cargo install --path turbo/crates/turbo-cli"
        exit 1
        ;;
esac

TARBALL="turbolang-v${VERSION}-${TARGET}.tar.gz"
BASE_URL="https://github.com/ZVN-DEV/Turbo-Language/releases/download/v${VERSION}"
URL="${BASE_URL}/${TARBALL}"
CHECKSUMS_URL="${BASE_URL}/checksums.txt"
CHECKSUM_SIG_URL="${BASE_URL}/checksums.txt.sig"
KEY_URL="${TURBO_RELEASE_KEY_URL:-https://turbolang.dev/keys/release.asc}"

# Pinned SHA256 of the in-repo copy of the Turbo release public key
# (distribution/turbo-release-key.asc). The --verify flow downloads the key
# from KEY_URL and refuses to import anything whose SHA256 does not match
# this value — so a one-time compromise of turbolang.dev cannot silently
# backdoor the installer. Rotate this value in the same commit that rotates
# the in-repo key file.
EXPECTED_KEY_SHA256="0fc30cb7508089d0066241f4dbb9c7211a28c32d28752183a9b33d24043c4738"
PINNED_KEY_RAW_URL="${TURBO_PINNED_KEY_URL:-https://raw.githubusercontent.com/ZVN-DEV/Turbo-Language/master/distribution/turbo-release-key.asc}"

# Download and extract
TMPDIR=$(mktemp -d)
trap 'rm -rf "${TMPDIR}"' EXIT

echo "Downloading from ${URL}..."
curl -fsSL "${URL}" -o "${TMPDIR}/${TARBALL}" || {
    echo "error: failed to download ${URL}" >&2
    exit 1
}

# Download checksums and verify
echo "Verifying checksum..."
curl -fsSL "${CHECKSUMS_URL}" -o "${TMPDIR}/checksums.txt" || {
    echo "error: failed to download checksums.txt from ${CHECKSUMS_URL} — refusing to install" >&2
    exit 1
}

# Extract only the line for our exact tarball (column-anchored so a future
# sibling artifact like "foo.tar.gz.sig" can't accidentally match).
EXPECTED=$(awk -v f="${TARBALL}" '$2==f {print; exit}' "${TMPDIR}/checksums.txt")
if [ -z "${EXPECTED}" ]; then
    echo "error: checksum for ${TARBALL} not found in checksums.txt — refusing to install" >&2
    exit 1
fi

cd "${TMPDIR}"
if command -v sha256sum &> /dev/null; then
    echo "${EXPECTED}" | sha256sum -c -
elif command -v shasum &> /dev/null; then
    echo "${EXPECTED}" | shasum -a 256 -c -
else
    echo "error: neither sha256sum nor shasum is available on this system — cannot verify download integrity, refusing to install" >&2
    exit 1
fi
cd - > /dev/null

# Signed-manifest verification (optional — requires gpg and the official
# release key published at turbolang.dev). This verifies that checksums.txt
# was signed by the Turbo release key, then the tarball is checked against
# that signed manifest.
if [ "${VERIFY_GPG:-false}" = "true" ]; then
    if ! command -v gpg &> /dev/null; then
        echo "error: --verify requires gpg to be installed" >&2
        exit 1
    fi

    echo "Downloading signed checksum manifest..."
    curl -fsSL "${CHECKSUM_SIG_URL}" -o "${TMPDIR}/checksums.txt.sig" || {
        echo "error: signed checksum manifest not available at ${CHECKSUM_SIG_URL}" >&2
        echo "Releases before the signed-manifest flow cannot be verified with --verify." >&2
        exit 1
    }

    echo "Importing Turbo release key from ${KEY_URL}..."
    curl -fsSL "${KEY_URL}" -o "${TMPDIR}/release.asc" || {
        echo "error: failed to download the Turbo release key from ${KEY_URL}" >&2
        echo "For manual verification instructions, see SECURITY.md." >&2
        exit 1
    }

    # Supply-chain pin: compare the fetched key's SHA256 against the value
    # pinned in this installer (EXPECTED_KEY_SHA256). The pinned value tracks
    # the in-repo copy at distribution/turbo-release-key.asc, which is
    # audited in version control. If the hosted key at ${KEY_URL} is ever
    # swapped or tampered with, this check aborts the install before gpg
    # imports the attacker's key.
    #
    # Escape hatch: set TURBO_TRUST_REMOTE_KEY=1 to skip this check. Intended
    # for the brief window during a legitimate key rotation where the pinned
    # SHA256 has not yet been updated — NOT for routine use. If you're
    # considering this variable in production, stop and update the pin
    # instead.
    if [ "${TURBO_TRUST_REMOTE_KEY:-0}" = "1" ]; then
        echo "warning: TURBO_TRUST_REMOTE_KEY=1 set — skipping pinned SHA256 check on ${KEY_URL}" >&2
    else
        if command -v sha256sum &> /dev/null; then
            ACTUAL_KEY_SHA256=$(sha256sum "${TMPDIR}/release.asc" | awk '{print $1}')
        elif command -v shasum &> /dev/null; then
            ACTUAL_KEY_SHA256=$(shasum -a 256 "${TMPDIR}/release.asc" | awk '{print $1}')
        else
            echo "error: neither sha256sum nor shasum is available — cannot pin the release key, refusing to install" >&2
            exit 1
        fi
        if [ "${ACTUAL_KEY_SHA256}" != "${EXPECTED_KEY_SHA256}" ]; then
            echo "error: Turbo release key from ${KEY_URL} does not match the pinned SHA256." >&2
            echo "       expected: ${EXPECTED_KEY_SHA256}" >&2
            echo "       actual:   ${ACTUAL_KEY_SHA256}" >&2
            echo "       This could mean a legitimate key rotation OR a tampered key-hosting URL." >&2
            echo "       Cross-check against the in-repo copy at ${PINNED_KEY_RAW_URL}" >&2
            echo "       before proceeding. Refusing to install." >&2
            exit 1
        fi
        echo "Pinned release-key SHA256 verified."
    fi

    export GNUPGHOME="$(mktemp -d "${TMPDIR}/gnupg.XXXXXX")"
    chmod 700 "${GNUPGHOME}"

    gpg --batch --import "${TMPDIR}/release.asc" >/dev/null 2>&1 || {
        echo "error: failed to import the Turbo release key" >&2
        exit 1
    }

    echo "Verifying signed checksum manifest..."
    if gpg --batch --verify "${TMPDIR}/checksums.txt.sig" "${TMPDIR}/checksums.txt"; then
        echo "Signed manifest verified successfully."
        echo "Note: --verify bootstraps trust from the HTTPS-served key at ${KEY_URL}."
        echo "For the strongest assurance, import that key yourself before running the installer."
    else
        echo "error: signed checksum verification failed — refusing to install" >&2
        exit 1
    fi
fi

# Extract
tar xz -C "${TMPDIR}" -f "${TMPDIR}/${TARBALL}"

# Install
if [ -w "${INSTALL_DIR}" ]; then
    mv "${TMPDIR}/turbolang" "${INSTALL_DIR}/turbolang"
else
    echo "Need sudo to install to ${INSTALL_DIR}"
    sudo mv "${TMPDIR}/turbolang" "${INSTALL_DIR}/turbolang"
fi

echo ""
echo "Turbo v${VERSION} installed to ${INSTALL_DIR}/turbolang"
echo ""
echo "Get started:"
echo "  turbolang init myproject"
echo "  cd myproject"
echo "  turbolang run"
echo ""
echo "Or try the REPL:"
echo "  turbolang repl"
