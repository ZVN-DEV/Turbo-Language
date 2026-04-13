#!/bin/bash
# Turbo Language Installer
# Usage: curl -fsSL https://raw.githubusercontent.com/ZVN-DEV/Turbo-Language/master/distribution/install.sh | bash
# Specific version: VERSION=0.2.0 curl -fsSL ... | bash
# Or: curl -fsSL ... | bash -s -- --version 0.2.0
# With GPG verification: curl -fsSL ... | bash -s -- --verify

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

# GPG verification (optional — requires gpg and the signing key)
if [ "${VERIFY_GPG:-false}" = "true" ]; then
    SIG_URL="${BASE_URL}/${TARBALL}.sig"
    echo "Downloading GPG signature..."
    curl -fsSL "${SIG_URL}" -o "${TMPDIR}/${TARBALL}.sig" || {
        echo "warning: GPG signature not available for this release (signatures available from v0.6+)" >&2
        echo "Continuing without GPG verification." >&2
    }

    if [ -f "${TMPDIR}/${TARBALL}.sig" ]; then
        if command -v gpg &> /dev/null; then
            # Import the Turbo signing key if not already present
            KEY_URL="https://raw.githubusercontent.com/ZVN-DEV/Turbo-Language/master/distribution/signing-key.asc"
            curl -fsSL "${KEY_URL}" | gpg --import 2>/dev/null || true

            echo "Verifying GPG signature..."
            if gpg --verify "${TMPDIR}/${TARBALL}.sig" "${TMPDIR}/${TARBALL}" 2>/dev/null; then
                echo "GPG signature verified successfully."
            else
                echo "error: GPG signature verification failed — the download may have been tampered with" >&2
                exit 1
            fi
        else
            echo "error: --verify requires gpg to be installed" >&2
            exit 1
        fi
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
