#!/usr/bin/env bash
# check_cargo_package_readiness.sh — Cargo package dry-run gate.
#
# Verifies publishable workspace crates package locally and classifies crates
# that are expected to wait on unpublished internal Turbo crates.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
exec python3 "${REPO_ROOT}/scripts/check_cargo_package_readiness.py" "$@"
