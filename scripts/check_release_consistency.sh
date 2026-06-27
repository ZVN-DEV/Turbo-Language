#!/usr/bin/env bash
# check_release_consistency.sh — package/release metadata drift guard.
#
# Keeps crate versions, lockfiles, editor metadata, Homebrew formula, release
# workflows, Docker, and installer surfaces aligned before tags are pushed.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
exec python3 "${REPO_ROOT}/scripts/check_release_consistency.py" "$@"
