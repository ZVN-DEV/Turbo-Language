#!/bin/bash
# Turbo Language Installer
# Usage: curl -fsSL https://raw.githubusercontent.com/ZVN-DEV/Turbo-Language/master/distribution/install.sh | bash
# Specific version: VERSION=0.2.0 curl -fsSL ... | bash
# Or: curl -fsSL ... | bash -s -- --version 0.2.0

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
curl -fsSL "${URL}" -o "${TMPDIR}/${TARBALL}"

# Download checksums and verify
echo "Verifying checksum..."
curl -fsSL "${CHECKSUMS_URL}" -o "${TMPDIR}/checksums.txt"

# Extract only the line for our tarball
EXPECTED=$(grep "${TARBALL}" "${TMPDIR}/checksums.txt" || true)
if [ -z "${EXPECTED}" ]; then
    echo "Warning: No checksum found for ${TARBALL} in checksums.txt, skipping verification."
else
    cd "${TMPDIR}"
    if command -v sha256sum &> /dev/null; then
        echo "${EXPECTED}" | sha256sum -c -
    elif command -v shasum &> /dev/null; then
        echo "${EXPECTED}" | shasum -a 256 -c -
    else
        echo "Warning: Neither sha256sum nor shasum found, skipping checksum verification."
    fi
    cd - > /dev/null
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
