#!/usr/bin/env python3
"""Check package and release metadata that must move together.

This is intentionally dependency-free so it runs on macOS system Python,
GitHub-hosted runners, and release machines before a tag is pushed.
"""

from __future__ import annotations

import json
import re
import subprocess
import tempfile
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent


class CheckFailure(Exception):
    pass


def read(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except FileNotFoundError as exc:
        raise CheckFailure(f"missing required file: {path.relative_to(REPO_ROOT)}") from exc


def rel(path: Path) -> str:
    try:
        return str(path.relative_to(REPO_ROOT))
    except ValueError:
        return str(path)


def package_version_from_cargo_toml(path: Path) -> str:
    in_package = False
    for line in read(path).splitlines():
        stripped = line.strip()
        if stripped == "[package]":
            in_package = True
            continue
        if in_package and stripped.startswith("[") and stripped.endswith("]"):
            break
        if in_package:
            match = re.match(r'version\s*=\s*"([^"]+)"', stripped)
            if match:
                return match.group(1)
    raise CheckFailure(f"{rel(path)} has no [package] version")


def version_from_json(path: Path) -> str:
    try:
        data = json.loads(read(path))
    except json.JSONDecodeError as exc:
        raise CheckFailure(f"{rel(path)} is not valid JSON: {exc}") from exc
    version = data.get("version")
    if not isinstance(version, str) or not version:
        raise CheckFailure(f"{rel(path)} has no string version field")
    return version


def homebrew_metadata(path: Path) -> tuple[str, list[str], list[str]]:
    text = read(path)
    version_match = re.search(r'^\s*version\s+"([^"]+)"', text, re.MULTILINE)
    if not version_match:
        raise CheckFailure(f"{rel(path)} has no formula version")
    urls = re.findall(r'^\s*url\s+"([^"]+)"', text, re.MULTILINE)
    sha256s = re.findall(r'^\s*sha256\s+"([0-9a-f]{64})"', text, re.MULTILINE)
    return version_match.group(1), urls, sha256s


def local_lock_versions(path: Path, package_names: set[str]) -> dict[str, str]:
    versions: dict[str, str] = {}
    current_name: str | None = None
    current_version: str | None = None

    def flush() -> None:
        if current_name in package_names and current_version is not None:
            versions[current_name] = current_version

    for line in read(path).splitlines():
        if line.strip() == "[[package]]":
            flush()
            current_name = None
            current_version = None
            continue
        name_match = re.match(r'name\s*=\s*"([^"]+)"', line.strip())
        if name_match:
            current_name = name_match.group(1)
            continue
        version_match = re.match(r'version\s*=\s*"([^"]+)"', line.strip())
        if version_match:
            current_version = version_match.group(1)
    flush()
    return versions


def require(condition: bool, message: str) -> None:
    if not condition:
        raise CheckFailure(message)


def tracked_files(repo_root: Path, pathspec: str) -> list[Path]:
    git_dir = repo_root / ".git"
    if not git_dir.exists():
        return []
    result = subprocess.run(
        ["git", "-C", str(repo_root), "ls-files", pathspec],
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    if result.returncode != 0:
        raise CheckFailure(f"failed to list tracked files for {pathspec}: {result.stderr.strip()}")
    return [repo_root / line for line in result.stdout.splitlines() if line]


def check_benchmark_artifacts(repo_root: Path) -> None:
    allowed_suffixes = {
        ".c",
        ".go",
        ".js",
        ".md",
        ".py",
        ".rb",
        ".rs",
        ".sh",
        ".tb",
    }
    generated_dirs = {
        "c",
        "go",
        "js",
        "python",
        "ruby",
        "rust",
    }
    bad: list[str] = []
    for path in tracked_files(repo_root, "turbo/benchmarks"):
        rel_parts = path.relative_to(repo_root).parts
        if len(rel_parts) < 3 or rel_parts[0] != "turbo" or rel_parts[1] != "benchmarks":
            continue
        language_dir = rel_parts[2]
        if language_dir not in generated_dirs:
            continue
        if path.suffix not in allowed_suffixes:
            bad.append(rel(path))
    require(
        not bad,
        "tracked benchmark generated artifacts should be rebuilt from source, not committed: "
        + ", ".join(sorted(bad)),
    )


def check_release_consistency(repo_root: Path = REPO_ROOT) -> list[str]:
    passed: list[str] = []

    crate_manifests = sorted((repo_root / "turbo" / "crates").glob("*/Cargo.toml"))
    require(crate_manifests, "no crate manifests found under turbo/crates")

    crate_versions = {manifest.parent.name: package_version_from_cargo_toml(manifest) for manifest in crate_manifests}
    unique_crate_versions = sorted(set(crate_versions.values()))
    require(
        len(unique_crate_versions) == 1,
        "crate versions differ: "
        + ", ".join(f"{name}={version}" for name, version in sorted(crate_versions.items())),
    )
    version = unique_crate_versions[0]
    passed.append(f"{len(crate_versions)} crate manifests agree on {version}")

    vscode_package = repo_root / "editors" / "vscode" / "turbo-lang" / "package.json"
    vscode_version = version_from_json(vscode_package)
    require(
        vscode_version == version,
        f"{rel(vscode_package)} version {vscode_version} does not match crate version {version}",
    )
    passed.append(f"{rel(vscode_package)} version matches")

    formula = repo_root / "distribution" / "homebrew" / "turbo-lang.rb"
    formula_version, urls, sha256s = homebrew_metadata(formula)
    require(formula_version == version, f"{rel(formula)} version {formula_version} does not match {version}")
    expected_targets = {
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
        "x86_64-unknown-linux-gnu",
    }
    expected_url_fragments = {f"turbolang-v{version}-{target}.tar.gz" for target in expected_targets}
    missing_url_fragments = [
        fragment for fragment in sorted(expected_url_fragments) if not any(fragment in url for url in urls)
    ]
    require(not missing_url_fragments, f"{rel(formula)} missing URLs for: {', '.join(missing_url_fragments)}")
    require(all(f"/releases/download/v{version}/" in url for url in urls), f"{rel(formula)} has URL tag drift")
    require(len(sha256s) == len(expected_targets), f"{rel(formula)} should have one sha256 per release target")
    formula_text = read(formula)
    require(
        f'assert_match "turbolang {version}"' in formula_text,
        f"{rel(formula)} test assertion does not check turbolang {version}",
    )
    require('bin.install "turbo-lsp"' in formula_text, f"{rel(formula)} does not require turbo-lsp install")
    require('bin.install "turbo-lsp" if' not in formula_text, f"{rel(formula)} still allows archives missing turbo-lsp")
    require('assert_predicate bin/"turbo-lsp", :exist?' in formula_text, f"{rel(formula)} does not test turbo-lsp")
    passed.append(f"{rel(formula)} version, URLs, checksums, and tests match")

    package_names = set(crate_versions)
    for lockfile in [repo_root / "turbo" / "Cargo.lock", repo_root / "turbo" / "fuzz" / "Cargo.lock"]:
        lock_versions = local_lock_versions(lockfile, package_names)
        if lockfile.name == "Cargo.lock" and lockfile.parent.name == "turbo":
            missing = sorted(package_names - set(lock_versions))
            require(not missing, f"{rel(lockfile)} missing local packages: {', '.join(missing)}")
        drift = {
            name: lock_version
            for name, lock_version in lock_versions.items()
            if lock_version != crate_versions[name]
        }
        require(
            not drift,
            f"{rel(lockfile)} has local package version drift: "
            + ", ".join(f"{name}={lock_version} expected {crate_versions[name]}" for name, lock_version in sorted(drift.items())),
        )
        passed.append(f"{rel(lockfile)} local package versions match")

    changelog = read(repo_root / "CHANGELOG.md")
    require(f"## [{version}]" in changelog, f"CHANGELOG.md has no section for {version}")
    passed.append("CHANGELOG.md has a section for the current version")

    release_workflow = read(repo_root / ".github" / "workflows" / "release.yml")
    require("turbolang turbo-lsp" in release_workflow, "release.yml does not package both turbolang and turbo-lsp")
    require("Smoke test - AOT build" in release_workflow, "release.yml is missing AOT release smoke")
    require('bin.install "turbo-lsp"' in release_workflow, "release.yml generated Homebrew formula does not require turbo-lsp install")
    require('bin.install "turbo-lsp" if' not in release_workflow, "release.yml generated Homebrew formula still allows archives missing turbo-lsp")
    require('assert_predicate bin/"turbo-lsp", :exist?' in release_workflow, "release.yml generated Homebrew formula does not test turbo-lsp")
    passed.append("release workflow packages, smokes, and publishes the two-binary toolchain")

    nightly_workflow = read(repo_root / ".github" / "workflows" / "nightly.yml")
    require("cp turbo/target/release/turbolang turbo/target/release/turbo-lsp ." in nightly_workflow, "nightly.yml does not stage turbo-lsp")
    require('test -x ./turbo/target/release/turbo-lsp' in nightly_workflow, "nightly.yml does not verify turbo-lsp")
    passed.append("nightly workflow stages and verifies turbo-lsp")

    dockerfile = read(repo_root / "distribution" / "Dockerfile")
    require("-p turbo-cli -p turbo-lsp" in dockerfile, "Dockerfile does not build both CLI and LSP")
    require("/turbo-lsp /usr/local/bin/turbo-lsp" in dockerfile, "Dockerfile does not copy turbo-lsp")
    passed.append("Dockerfile builds and installs both binaries")

    installer = read(repo_root / "distribution" / "install.sh")
    require("install_binary turbolang" in installer, "install.sh does not install turbolang")
    require("install_binary turbo-lsp" in installer, "install.sh does not install turbo-lsp")
    require("release archive did not contain turbo-lsp" in installer, "install.sh does not reject archives missing turbo-lsp")
    require("installed CLI only" not in installer, "install.sh still allows CLI-only release archives")
    smoke = read(repo_root / "scripts" / "smoke_install_script.sh")
    require('test -x "${INSTALL_DIR}/turbolang"' in smoke, "installer smoke does not check turbolang")
    require('test -x "${INSTALL_DIR}/turbo-lsp"' in smoke, "installer smoke does not check turbo-lsp")
    require("BROKEN_STATUS" in smoke, "installer smoke does not test missing turbo-lsp failure")
    passed.append("installer and smoke fixture cover both binaries")

    release_docs = read(repo_root / "docs" / "RELEASE.md")
    require("./scripts/check_release_consistency.sh" in release_docs, "docs/RELEASE.md does not mention release consistency check")
    passed.append("release runbook includes this consistency gate")

    check_benchmark_artifacts(repo_root)
    passed.append("benchmark comparison baselines are source-only")

    return passed


def write(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def write_fixture(root: Path, version: str = "1.2.3") -> None:
    for name in [
        "turbo-ast",
        "turbo-cli",
        "turbo-codegen-cranelift",
        "turbo-lexer",
        "turbo-lsp",
        "turbo-parser",
        "turbo-sema",
    ]:
        write(root / "turbo" / "crates" / name / "Cargo.toml", f'[package]\nname = "{name}"\nversion = "{version}"\n')

    write(root / "editors" / "vscode" / "turbo-lang" / "package.json", json.dumps({"version": version}))
    write(
        root / "distribution" / "homebrew" / "turbo-lang.rb",
        f'''class TurboLang < Formula
  version "{version}"
  url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v{version}/turbolang-v{version}-aarch64-apple-darwin.tar.gz"
  sha256 "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
  url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v{version}/turbolang-v{version}-x86_64-apple-darwin.tar.gz"
  sha256 "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
  url "https://github.com/ZVN-DEV/Turbo-Language/releases/download/v{version}/turbolang-v{version}-x86_64-unknown-linux-gnu.tar.gz"
  sha256 "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
  def install
    bin.install "turbo-lsp"
  end
  test do
    assert_match "turbolang {version}", shell_output("#{{bin}}/turbolang --version")
    assert_predicate bin/"turbo-lsp", :exist?
  end
end
''',
    )
    lock_packages = "\n".join(
        f'[[package]]\nname = "{name}"\nversion = "{version}"\n'
        for name in [
            "turbo-ast",
            "turbo-cli",
            "turbo-codegen-cranelift",
            "turbo-lexer",
            "turbo-lsp",
            "turbo-parser",
            "turbo-sema",
        ]
    )
    write(root / "turbo" / "Cargo.lock", f"version = 4\n\n{lock_packages}")
    fuzz_packages = "\n".join(
        f'[[package]]\nname = "{name}"\nversion = "{version}"\n'
        for name in ["turbo-ast", "turbo-codegen-cranelift", "turbo-lexer", "turbo-parser", "turbo-sema"]
    )
    write(root / "turbo" / "fuzz" / "Cargo.lock", f"version = 4\n\n{fuzz_packages}")
    write(root / "CHANGELOG.md", f"# Changelog\n\n## [{version}] - 2099-01-01\n")
    write(
        root / ".github" / "workflows" / "release.yml",
        'tar czf artifact.tgz turbolang turbo-lsp\nSmoke test - AOT build\nbin.install "turbo-lsp"\nassert_predicate bin/"turbo-lsp", :exist?\n',
    )
    write(
        root / ".github" / "workflows" / "nightly.yml",
        'cp turbo/target/release/turbolang turbo/target/release/turbo-lsp .\ntest -x ./turbo/target/release/turbo-lsp\n',
    )
    write(
        root / "distribution" / "Dockerfile",
        "RUN cargo build --release -p turbo-cli -p turbo-lsp\nCOPY --from=builder /build/turbo/target/release/turbo-lsp /usr/local/bin/turbo-lsp\n",
    )
    write(root / "distribution" / "install.sh", "install_binary turbolang\ninstall_binary turbo-lsp\nrelease archive did not contain turbo-lsp\n")
    write(root / "scripts" / "smoke_install_script.sh", 'test -x "${INSTALL_DIR}/turbolang"\ntest -x "${INSTALL_DIR}/turbo-lsp"\nBROKEN_STATUS=1\n')
    write(root / "docs" / "RELEASE.md", "./scripts/check_release_consistency.sh\n")


def run_self_test() -> int:
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        write_fixture(root)
        check_release_consistency(root)

        formula = root / "distribution" / "homebrew" / "turbo-lang.rb"
        formula.write_text(formula.read_text(encoding="utf-8").replace("turbo-lsp", "missing-lsp", 1), encoding="utf-8")
        try:
            check_release_consistency(root)
        except CheckFailure as exc:
            require("turbo-lsp" in str(exc), f"self-test expected turbo-lsp failure, got: {exc}")
        else:
            raise CheckFailure("self-test expected mutated Homebrew formula to fail")

    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        write_fixture(root)
        subprocess.run(["git", "-C", str(root), "init"], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        write(root / "turbo" / "benchmarks" / "c" / "fib", "generated binary placeholder\n")
        write(root / "turbo" / "benchmarks" / "c" / "fib.c", "int main(void) { return 0; }\n")
        subprocess.run(
            ["git", "-C", str(root), "add", "turbo/benchmarks/c/fib", "turbo/benchmarks/c/fib.c"],
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        try:
            check_release_consistency(root)
        except CheckFailure as exc:
            require("tracked benchmark generated artifacts" in str(exc), f"self-test expected benchmark artifact failure, got: {exc}")
        else:
            raise CheckFailure("self-test expected tracked benchmark artifact to fail")

        subprocess.run(
            ["git", "-C", str(root), "rm", "--cached", "turbo/benchmarks/c/fib"],
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        check_release_consistency(root)

    print("self-test: release consistency fixture checks passed")
    return 0


def main() -> int:
    if len(sys.argv) == 2 and sys.argv[1] == "--self-test":
        try:
            return run_self_test()
        except CheckFailure as exc:
            print(f"release consistency self-test failed: {exc}", file=sys.stderr)
            return 1
    if len(sys.argv) != 1:
        print("usage: check_release_consistency.py [--self-test]", file=sys.stderr)
        return 2

    try:
        passed = check_release_consistency()
    except CheckFailure as exc:
        print(f"release consistency check failed: {exc}", file=sys.stderr)
        return 1

    for item in passed:
        print(f"ok: {item}")
    print("release consistency check passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
