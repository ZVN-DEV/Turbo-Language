#!/usr/bin/env bash
# Smoke-test distribution/install.sh without touching GitHub or /usr/local/bin.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMPROOT="$(mktemp -d)"
RELEASE_DIR="${TMPROOT}/release"
INSTALL_DIR="${TMPROOT}/install"
PKG_DIR="${TMPROOT}/pkg"

cleanup() {
    if [ -n "${SERVER_PID:-}" ]; then
        kill "${SERVER_PID}" 2>/dev/null || true
        wait "${SERVER_PID}" 2>/dev/null || true
    fi
    if [ -n "${BROKEN_SERVER_PID:-}" ]; then
        kill "${BROKEN_SERVER_PID}" 2>/dev/null || true
        wait "${BROKEN_SERVER_PID}" 2>/dev/null || true
    fi
    rm -rf "${TMPROOT}"
}
trap cleanup EXIT

mkdir -p "${RELEASE_DIR}" "${INSTALL_DIR}" "${PKG_DIR}"

OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"
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
        echo "error: unsupported smoke-test platform: ${OS}-${ARCH}" >&2
        exit 1
        ;;
esac

VERSION="99.0.0-test"
TARBALL="turbolang-v${VERSION}-${TARGET}.tar.gz"

cat > "${PKG_DIR}/turbolang" <<'EOF'
#!/usr/bin/env sh
echo "fake turbolang $*"
EOF

cat > "${PKG_DIR}/turbo-lsp" <<'EOF'
#!/usr/bin/env sh
echo "fake turbo-lsp $*"
EOF

chmod +x "${PKG_DIR}/turbolang" "${PKG_DIR}/turbo-lsp"

(
    cd "${PKG_DIR}"
    tar czf "${RELEASE_DIR}/${TARBALL}" turbolang turbo-lsp
)

(
    cd "${RELEASE_DIR}"
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "${TARBALL}" > checksums.txt
    else
        shasum -a 256 "${TARBALL}" > checksums.txt
    fi
)

PORT="$(python3 - <<'PY'
import socket
with socket.socket() as s:
    s.bind(("127.0.0.1", 0))
    print(s.getsockname()[1])
PY
)"

python3 -m http.server "${PORT}" --bind 127.0.0.1 --directory "${RELEASE_DIR}" \
    > "${TMPROOT}/http.log" 2>&1 &
SERVER_PID="$!"

for _ in $(seq 1 50); do
    if curl -fsS "http://127.0.0.1:${PORT}/checksums.txt" >/dev/null 2>&1; then
        break
    fi
    sleep 0.1
done

curl -fsS "http://127.0.0.1:${PORT}/checksums.txt" >/dev/null

VERSION="${VERSION}" \
TURBO_INSTALL_BASE_URL="http://127.0.0.1:${PORT}" \
TURBO_INSTALL_DIR="${INSTALL_DIR}" \
bash "${ROOT_DIR}/distribution/install.sh"

test -x "${INSTALL_DIR}/turbolang"
test -x "${INSTALL_DIR}/turbo-lsp"

"${INSTALL_DIR}/turbolang" smoke | grep -F "fake turbolang smoke" >/dev/null
"${INSTALL_DIR}/turbo-lsp" smoke | grep -F "fake turbo-lsp smoke" >/dev/null

BROKEN_RELEASE_DIR="${TMPROOT}/broken-release"
BROKEN_PKG_DIR="${TMPROOT}/broken-pkg"
BROKEN_INSTALL_DIR="${TMPROOT}/broken-install"
mkdir -p "${BROKEN_RELEASE_DIR}" "${BROKEN_PKG_DIR}" "${BROKEN_INSTALL_DIR}"
cp "${PKG_DIR}/turbolang" "${BROKEN_PKG_DIR}/turbolang"
(
    cd "${BROKEN_PKG_DIR}"
    tar czf "${BROKEN_RELEASE_DIR}/${TARBALL}" turbolang
)
(
    cd "${BROKEN_RELEASE_DIR}"
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "${TARBALL}" > checksums.txt
    else
        shasum -a 256 "${TARBALL}" > checksums.txt
    fi
)

BROKEN_PORT="$(python3 - <<'PY'
import socket
with socket.socket() as s:
    s.bind(("127.0.0.1", 0))
    print(s.getsockname()[1])
PY
)"

python3 -m http.server "${BROKEN_PORT}" --bind 127.0.0.1 --directory "${BROKEN_RELEASE_DIR}" \
    > "${TMPROOT}/broken-http.log" 2>&1 &
BROKEN_SERVER_PID="$!"

for _ in $(seq 1 50); do
    if curl -fsS "http://127.0.0.1:${BROKEN_PORT}/checksums.txt" >/dev/null 2>&1; then
        break
    fi
    sleep 0.1
done

set +e
BROKEN_OUTPUT="$(
    VERSION="${VERSION}" \
    TURBO_INSTALL_BASE_URL="http://127.0.0.1:${BROKEN_PORT}" \
    TURBO_INSTALL_DIR="${BROKEN_INSTALL_DIR}" \
    bash "${ROOT_DIR}/distribution/install.sh" 2>&1
)"
BROKEN_STATUS=$?
set -e

if [ "${BROKEN_STATUS}" -eq 0 ]; then
    echo "error: install.sh accepted archive missing turbo-lsp" >&2
    exit 1
fi
printf '%s\n' "${BROKEN_OUTPUT}" | grep -F "release archive did not contain turbo-lsp" >/dev/null
test ! -e "${BROKEN_INSTALL_DIR}/turbolang"
test ! -e "${BROKEN_INSTALL_DIR}/turbo-lsp"

echo "install.sh smoke passed for ${TARGET}"
