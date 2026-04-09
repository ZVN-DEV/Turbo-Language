#!/usr/bin/env bash
# check_error_codes.sh — thin wrapper around check_error_codes.py.
#
# The real lint lives in `scripts/check_error_codes.py` so we don't have
# to fight POSIX awk's lack of PCRE support. This wrapper exists so the
# CI job and local invocations stay stable: `./scripts/check_error_codes.sh`.
#
# Exit code is forwarded from the Python script:
#   0 — clean
#   1 — at least one violation
#   2 — script invocation error

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
exec python3 "${REPO_ROOT}/scripts/check_error_codes.py" "$@"
