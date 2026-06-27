#!/usr/bin/env python3
"""Check Cargo package readiness without requiring crates.io credentials.

The first unpublished TurboLang crates can be packaged locally today. Crates
that depend on unpublished internal crates are expected to stop at Cargo's
registry resolution step until those dependencies are published in order.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
MANIFEST = REPO_ROOT / "turbo" / "Cargo.toml"


class CheckFailure(Exception):
    pass


@dataclass(frozen=True)
class Package:
    name: str
    version: str
    manifest_path: Path
    order_deps: tuple[str, ...]
    package_blocking_deps: tuple[str, ...]


@dataclass(frozen=True)
class PackageResult:
    name: str
    status: str
    detail: str


def run(cmd: list[str], cwd: Path | None = None, timeout: int = 180) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        cmd,
        cwd=str(cwd or REPO_ROOT),
        timeout=timeout,
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )


def metadata() -> dict:
    result = run(
        ["cargo", "metadata", "--manifest-path", str(MANIFEST), "--no-deps", "--format-version", "1"],
        timeout=60,
    )
    if result.returncode != 0:
        raise CheckFailure(f"cargo metadata failed: {result.stderr.strip()}")
    return json.loads(result.stdout)


def workspace_packages(meta: dict) -> dict[str, Package]:
    workspace_ids = set(meta["workspace_members"])
    raw_packages = [pkg for pkg in meta["packages"] if pkg["id"] in workspace_ids]
    names = {pkg["name"] for pkg in raw_packages}
    packages: dict[str, Package] = {}
    for pkg in raw_packages:
        package_blocking_deps = sorted(
            dep["name"]
            for dep in pkg.get("dependencies", [])
            if dep["name"] in names
        )
        order_deps = sorted(
            dep["name"]
            for dep in pkg.get("dependencies", [])
            if dep.get("kind") in (None, "normal", "build") and dep["name"] in names
        )
        packages[pkg["name"]] = Package(
            name=pkg["name"],
            version=pkg["version"],
            manifest_path=Path(pkg["manifest_path"]),
            order_deps=tuple(order_deps),
            package_blocking_deps=tuple(package_blocking_deps),
        )
    return packages


def topo_order(packages: dict[str, Package]) -> list[Package]:
    ordered: list[Package] = []
    remaining = dict(packages)
    while remaining:
        ready = sorted(
            [pkg for pkg in remaining.values() if all(dep not in remaining for dep in pkg.order_deps)],
            key=lambda pkg: pkg.name,
        )
        if not ready:
            cycle = ", ".join(sorted(remaining))
            raise CheckFailure(f"internal package dependency cycle or unresolved graph: {cycle}")
        for pkg in ready:
            ordered.append(pkg)
            remaining.pop(pkg.name)
    return ordered


def expected_missing_internal_dep(stderr: str, pkg: Package) -> str | None:
    match = re.search(r"no matching package named `([^`]+)` found", stderr)
    if match and match.group(1) in pkg.package_blocking_deps:
        return match.group(1)
    return None


def cargo_package(pkg: Package, timeout: int, verify: bool) -> subprocess.CompletedProcess[str]:
    cmd = [
        "cargo",
        "package",
        "--manifest-path",
        str(MANIFEST),
        "-p",
        pkg.name,
        "--allow-dirty",
    ]
    if not verify:
        cmd.append("--no-verify")
    return run(cmd, timeout=timeout)


def check_package_readiness(strict_all_published: bool = False, package_timeout: int = 180) -> list[PackageResult]:
    packages = workspace_packages(metadata())
    results: list[PackageResult] = []
    verify = strict_all_published

    for pkg in topo_order(packages):
        result = cargo_package(pkg, timeout=package_timeout, verify=verify)
        output = (result.stderr + "\n" + result.stdout).strip()
        if result.returncode == 0:
            results.append(PackageResult(pkg.name, "packageable", f"{pkg.name} packaged successfully"))
            continue

        missing_dep = expected_missing_internal_dep(output, pkg)
        if missing_dep and not strict_all_published:
            results.append(
                PackageResult(
                    pkg.name,
                    "registry-blocked",
                    f"waiting for internal crate {missing_dep} {pkg.version} to exist on crates.io",
                )
            )
            continue

        hint = "registry publish order incomplete" if missing_dep else output.splitlines()[-1] if output else "unknown error"
        raise CheckFailure(f"{pkg.name} package readiness failed: {hint}\n{output}")

    return results


def write_fixture(root: Path) -> None:
    cargo = root / "cargo"
    cargo.mkdir()
    script = cargo / "cargo"
    script.write_text(
        """#!/usr/bin/env bash
set -euo pipefail
if [ "${1:-}" = "metadata" ]; then
  cat "${FIXTURE_METADATA}"
  exit 0
fi
if [ "${1:-}" = "package" ]; then
  crate=""
  prev=""
  for arg in "$@"; do
    if [ "$prev" = "-p" ]; then crate="$arg"; break; fi
    prev="$arg"
  done
  verify=1
  for arg in "$@"; do
    if [ "$arg" = "--no-verify" ]; then verify=0; fi
  done
  case "$crate" in
    turbo-ast|turbo-lexer)
      if [ "$verify" = "1" ]; then echo "verified $crate" > "${STRICT_VERIFY_MARKER}"; fi
      echo "Packaged $crate" >&2
      exit 0
      ;;
    turbo-parser)
      echo 'error: failed to prepare local package for uploading' >&2
      echo 'Caused by:' >&2
      echo '  no matching package named `turbo-ast` found' >&2
      exit 101
      ;;
    turbo-formatter)
      echo 'error: failed to prepare local package for uploading' >&2
      echo 'Caused by:' >&2
      echo '  no matching package named `turbo-parser` found' >&2
      exit 101
      ;;
    turbo-cli)
      if [ "${FIXTURE_BAD_CLI:-0}" = "1" ]; then
        echo 'error: failed to prepare local package for uploading' >&2
        echo 'Caused by:' >&2
        echo '  dependency metadata was invalid' >&2
        exit 101
      fi
      echo 'error: failed to prepare local package for uploading' >&2
      echo 'Caused by:' >&2
      echo '  no matching package named `turbo-lexer` found' >&2
      exit 101
      ;;
    *)
      echo "unexpected crate: $crate" >&2
      exit 2
      ;;
  esac
fi
echo "unexpected cargo command: $*" >&2
exit 2
""",
        encoding="utf-8",
    )
    script.chmod(0o755)

    packages = [
        {
            "name": "turbo-ast",
            "version": "1.2.3",
            "id": "fixture turbo-ast",
            "manifest_path": str(root / "turbo/crates/turbo-ast/Cargo.toml"),
            "dependencies": [],
        },
        {
            "name": "turbo-lexer",
            "version": "1.2.3",
            "id": "fixture turbo-lexer",
            "manifest_path": str(root / "turbo/crates/turbo-lexer/Cargo.toml"),
            "dependencies": [],
        },
        {
            "name": "turbo-parser",
            "version": "1.2.3",
            "id": "fixture turbo-parser",
            "manifest_path": str(root / "turbo/crates/turbo-parser/Cargo.toml"),
            "dependencies": [{"name": "turbo-ast", "kind": None}, {"name": "turbo-lexer", "kind": None}],
        },
        {
            "name": "turbo-formatter",
            "version": "1.2.3",
            "id": "fixture turbo-formatter",
            "manifest_path": str(root / "turbo/crates/turbo-formatter/Cargo.toml"),
            "dependencies": [{"name": "turbo-lexer", "kind": None}, {"name": "turbo-parser", "kind": None}],
        },
        {
            "name": "turbo-cli",
            "version": "1.2.3",
            "id": "fixture turbo-cli",
            "manifest_path": str(root / "turbo/crates/turbo-cli/Cargo.toml"),
            "dependencies": [{"name": "turbo-formatter", "kind": None}, {"name": "turbo-parser", "kind": "dev"}],
        },
    ]
    meta = {"packages": packages, "workspace_members": [pkg["id"] for pkg in packages]}
    (root / "metadata.json").write_text(json.dumps(meta), encoding="utf-8")


def run_self_test() -> int:
    global REPO_ROOT, MANIFEST
    original_root = REPO_ROOT
    original_manifest = MANIFEST
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        write_fixture(root)
        REPO_ROOT = root
        MANIFEST = root / "turbo/Cargo.toml"
        old_env_path = dict()

        old_env_path["PATH"] = os.environ.get("PATH", "")
        old_env_path["FIXTURE_METADATA"] = os.environ.get("FIXTURE_METADATA", "")
        old_env_path["STRICT_VERIFY_MARKER"] = os.environ.get("STRICT_VERIFY_MARKER", "")
        old_env_path["FIXTURE_BAD_CLI"] = os.environ.get("FIXTURE_BAD_CLI", "")
        os.environ["PATH"] = f"{root / 'cargo'}:{old_env_path['PATH']}"
        os.environ["FIXTURE_METADATA"] = str(root / "metadata.json")
        os.environ["STRICT_VERIFY_MARKER"] = str(root / "strict-verify-marker")
        try:
            os.environ["FIXTURE_BAD_CLI"] = "1"
            try:
                check_package_readiness()
            except CheckFailure as exc:
                require("turbo-cli package readiness failed" in str(exc), f"expected bad metadata failure, got: {exc}")
            else:
                raise CheckFailure("self-test expected turbo-cli metadata failure")
            os.environ.pop("FIXTURE_BAD_CLI", None)

            original_meta = json.loads((root / "metadata.json").read_text(encoding="utf-8"))
            meta = json.loads(json.dumps(original_meta))
            meta["packages"] = [pkg for pkg in meta["packages"] if pkg["name"] != "turbo-cli"]
            meta["workspace_members"] = [pkg["id"] for pkg in meta["packages"]]
            (root / "metadata.json").write_text(json.dumps(meta), encoding="utf-8")
            results = check_package_readiness()
            statuses = {result.name: result.status for result in results}
            require(statuses["turbo-ast"] == "packageable", "expected turbo-ast packageable")
            require(statuses["turbo-parser"] == "registry-blocked", "expected turbo-parser registry-blocked")
            require(statuses["turbo-formatter"] == "registry-blocked", "expected turbo-formatter registry-blocked")

            meta = json.loads(json.dumps(original_meta))
            meta["packages"] = [
                pkg for pkg in meta["packages"] if pkg["name"] not in ("turbo-formatter", "turbo-parser")
            ]
            meta["workspace_members"] = [pkg["id"] for pkg in meta["packages"]]
            for pkg in meta["packages"]:
                if pkg["name"] == "turbo-cli":
                    pkg["dependencies"] = [{"name": "turbo-lexer", "kind": "dev"}]
            (root / "metadata.json").write_text(json.dumps(meta), encoding="utf-8")
            results = check_package_readiness()
            statuses = {result.name: result.status for result in results}
            require(statuses["turbo-cli"] == "registry-blocked", "expected dev-dependency registry blocker")

            meta["packages"] = [pkg for pkg in meta["packages"] if pkg["name"] == "turbo-ast"]
            meta["workspace_members"] = [pkg["id"] for pkg in meta["packages"]]
            (root / "metadata.json").write_text(json.dumps(meta), encoding="utf-8")
            marker = root / "strict-verify-marker"
            marker.unlink(missing_ok=True)
            check_package_readiness(strict_all_published=True)
            require(marker.exists(), "strict mode should run cargo package verification without --no-verify")
        finally:
            os.environ["PATH"] = old_env_path["PATH"]
            if old_env_path["FIXTURE_METADATA"]:
                os.environ["FIXTURE_METADATA"] = old_env_path["FIXTURE_METADATA"]
            else:
                os.environ.pop("FIXTURE_METADATA", None)
            if old_env_path["STRICT_VERIFY_MARKER"]:
                os.environ["STRICT_VERIFY_MARKER"] = old_env_path["STRICT_VERIFY_MARKER"]
            else:
                os.environ.pop("STRICT_VERIFY_MARKER", None)
            if old_env_path["FIXTURE_BAD_CLI"]:
                os.environ["FIXTURE_BAD_CLI"] = old_env_path["FIXTURE_BAD_CLI"]
            else:
                os.environ.pop("FIXTURE_BAD_CLI", None)
            REPO_ROOT = original_root
            MANIFEST = original_manifest
    print("self-test: cargo package readiness fixture checks passed")
    return 0


def require(condition: bool, message: str) -> None:
    if not condition:
        raise CheckFailure(message)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true", help="run fixture tests for the readiness checker")
    parser.add_argument(
        "--strict-all-published",
        action="store_true",
        help="fail if any workspace crate is still blocked by unpublished internal crates",
    )
    parser.add_argument("--package-timeout", type=int, default=180, help="timeout per cargo package command in seconds")
    args = parser.parse_args()

    try:
        if args.self_test:
            return run_self_test()
        results = check_package_readiness(
            strict_all_published=args.strict_all_published,
            package_timeout=args.package_timeout,
        )
    except (CheckFailure, subprocess.TimeoutExpired) as exc:
        print(f"cargo package readiness check failed: {exc}", file=sys.stderr)
        return 1

    for result in results:
        print(f"{result.status}: {result.name} - {result.detail}")

    blocked = [result for result in results if result.status == "registry-blocked"]
    if blocked:
        print(
            "cargo package readiness check passed with expected registry blockers; "
            "publish internal crates in dependency order before strict release packaging"
        )
    else:
        print("cargo package readiness check passed; all workspace crates package locally")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
