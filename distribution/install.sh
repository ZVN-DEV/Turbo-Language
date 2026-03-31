#!/bin/bash
# Turbo Language Installer
# Usage: curl -fsSL https://raw.githubusercontent.com/ZVN-DEV/Turbo-Language/master/distribution/install.sh | bash

set -euo pipefail

VERSION="0.1.0"
INSTALL_DIR="/usr/local/bin"

echo "Installing Turbo v${VERSION}..."

# Detect platform
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)

case "${OS}-${ARCH}" in
    darwin-arm64)
        TARGET="aarch64-apple-darwin"
        ;;
    darwin-x86_64)
        echo "Error: macOS x86_64 pre-built binary not yet available."
        echo "Install from source: cargo install --path turbo/crates/turbo-cli"
        exit 1
        ;;
    linux-x86_64)
        echo "Error: Linux x86_64 pre-built binary not yet available."
        echo "Install from source: cargo install --path turbo/crates/turbo-cli"
        exit 1
        ;;
    *)
        echo "Error: Unsupported platform: ${OS}-${ARCH}"
        echo "Install from source: cargo install --path turbo/crates/turbo-cli"
        exit 1
        ;;
esac

URL="https://github.com/ZVN-DEV/Turbo-Language/releases/download/v${VERSION}/turbo-v${VERSION}-${TARGET}.tar.gz"

# Download and extract
TMPDIR=$(mktemp -d)
echo "Downloading from ${URL}..."
curl -fsSL "${URL}" | tar xz -C "${TMPDIR}"

# Install
if [ -w "${INSTALL_DIR}" ]; then
    mv "${TMPDIR}/turbo" "${INSTALL_DIR}/turbo"
else
    echo "Need sudo to install to ${INSTALL_DIR}"
    sudo mv "${TMPDIR}/turbo" "${INSTALL_DIR}/turbo"
fi

rm -rf "${TMPDIR}"

echo ""
echo "Turbo v${VERSION} installed to ${INSTALL_DIR}/turbo"
echo ""
echo "Get started:"
echo "  turbo init myproject"
echo "  cd myproject"
echo "  turbo run"
echo ""
echo "Or try the REPL:"
echo "  turbo repl"
