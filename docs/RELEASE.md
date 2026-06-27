# Turbo Language — Release Checklist

Everything that must happen when shipping a new version. **A release is not done until every box is checked.**

---

## Repositories Involved

| Repo | Path | What lives there |
|------|------|-----------------|
| **Turbo-Language** (main) | `~/Desktop/Coding/ZVN/TurboLang` | Compiler, runtime, website, bundled VS Code extension, docs, local Homebrew formula |
| **homebrew-turbo** (tap) | `~/Desktop/Coding/ZVN/homebrew-turbo` | Homebrew tap formula (`brew install turbo-lang`) |
| **Bundled VS Code extension** | `editors/vscode/turbo-lang` | Syntax, snippets, LSP client, and smoke metadata |
| **turbo-vscode** (marketplace repo, if still used) | `~/Desktop/Coding/ZVN/turbo-vscode` | Published VS Code extension package |
| **tree-sitter-turbo** | `~/Desktop/Coding/ZVN/tree-sitter-turbo` | Tree-sitter grammar for editors |

---

## Files That Contain Version Strings

All must agree on the same version:

| File | Field |
|------|-------|
| `turbo/crates/turbo-cli/Cargo.toml` | `version` (controls `--version` + REPL banner) |
| `turbo/crates/turbo-ast/Cargo.toml` | `version` |
| `turbo/crates/turbo-lexer/Cargo.toml` | `version` |
| `turbo/crates/turbo-parser/Cargo.toml` | `version` |
| `turbo/crates/turbo-formatter/Cargo.toml` | `version` |
| `turbo/crates/turbo-sema/Cargo.toml` | `version` |
| `turbo/crates/turbo-codegen-cranelift/Cargo.toml` | `version` |
| `turbo/crates/turbo-lsp/Cargo.toml` | `version` |
| `turbo/Cargo.lock` | auto-updated by `cargo build` |
| `CHANGELOG.md` | `[X.Y.Z] - YYYY-MM-DD` section header |
| `distribution/homebrew/turbo-lang.rb` | `version`, URLs, sha256, test assertion |
| `editors/vscode/turbo-lang/package.json` | `"version"` |
| `~/Desktop/Coding/ZVN/homebrew-turbo/Formula/turbo-lang.rb` | same as above (tap copy) |
| `~/Desktop/Coding/ZVN/turbo-vscode/package.json` | `"version"` |

Quick verify command:
```bash
./scripts/check_release_consistency.sh
```

---

## Phase 1: Prepare (main repo)

### 1.1 Version Bump

Bump all workspace crate `Cargo.toml` files to the new version:
```bash
# Find-replace old version → new version in all Cargo.toml
grep '^version' turbo/crates/*/Cargo.toml  # verify they all match
```

### 1.2 Changelog

Move `[Unreleased]` content into a new dated section:
```markdown
## [X.Y.Z] - YYYY-MM-DD
### Added
- ...
### Changed
- ...
### Fixed
- ...
```

Leave an empty `[Unreleased]` section at the top.

### 1.3 Update Docs (if applicable)

Check and update these if they reference version numbers or counts:

| File | What to check |
|------|--------------|
| `README.md` | Test badge count, test counts in body, error code range |
| `CLAUDE.md` | Error code range |
| `docs/errors.md` | Any new error codes added since last release |

### 1.4 Run All Tests

```bash
# Unit tests (must be 0 failures)
cargo test --workspace --manifest-path turbo/Cargo.toml

# Release build
cargo build --release --manifest-path turbo/Cargo.toml

# Integration tests (must be 0 failures)
cd turbo && ./tests/run_tests.sh && cd ..

# Installer smoke (must install both turbolang and turbo-lsp from a local fixture)
./scripts/smoke_install_script.sh

# Release metadata consistency (versions, lockfiles, Homebrew, workflows, Docker, installer)
./scripts/check_release_consistency.sh
```

### 1.5 Verify Version Output

```bash
./turbo/target/release/turbolang --version
# Must print: turbolang X.Y.Z
```

### 1.6 Cargo Package Readiness

TurboLang crates use local `path` dependencies with matching `version`
requirements. This keeps local builds fast while preserving the metadata Cargo
needs for registry packaging.

Registry packaging must be checked in dependency order because Cargo rewrites
publishable path dependencies to crates.io dependencies during package
preparation:

1. `turbo-ast`
2. `turbo-lexer`
3. `turbo-parser`
4. `turbo-formatter`
5. `turbo-sema`
6. `turbo-codegen-cranelift`
7. `turbo-cli`
8. `turbo-lsp`

Before publishing higher-level crates, publish or otherwise make their internal
dependencies available at the same version.

```bash
cargo package --manifest-path turbo/Cargo.toml -p turbo-ast --allow-dirty
cargo package --manifest-path turbo/Cargo.toml -p turbo-lexer --allow-dirty
```

If `turbo-cli` or `turbo-lsp` package preparation fails with `no matching
package named turbo-ast found`, the crate metadata is ready but the registry
publish order is not complete yet.

Run the repeatable readiness gate before tagging:

```bash
./scripts/check_cargo_package_readiness.sh
```

The gate packages crates that can be prepared locally today and reports
`registry-blocked` for crates that are only waiting for unpublished internal
Turbo crates at the same version. Once the internal crates have been published
in order, rerun the stricter final gate:

```bash
./scripts/check_cargo_package_readiness.sh --strict-all-published
```

---

## Phase 2: Ship (git + GitHub)

### 2.1 Commit and Push

```bash
git add -A
git commit -m "vX.Y.Z: <summary of what's new>"
git push origin master
```

### 2.2 Tag and Trigger Release CI

```bash
git tag vX.Y.Z
git push origin vX.Y.Z
```

This triggers `.github/workflows/release.yml` which builds:
- macOS ARM (aarch64-apple-darwin) — Cranelift
- macOS Intel (x86_64-apple-darwin) — Cranelift
- Linux x86_64 (x86_64-unknown-linux-gnu) — Cranelift

### 2.3 Wait for Release CI

```bash
gh run list --workflow=release.yml --limit=1
# Wait for completion, then verify:
gh release view vX.Y.Z --repo ZVN-DEV/Turbo-Language
```

Confirm: 3 tarballs + `checksums.txt` attached to the release. `checksums.txt.sig` is also attached when release signing secrets are configured.

---

## Phase 3: Homebrew

### 3.1 Get Checksums

```bash
gh release download vX.Y.Z --repo ZVN-DEV/Turbo-Language --pattern "checksums.txt" -O /tmp/checksums.txt
cat /tmp/checksums.txt
```

### 3.2 Update Both Formulas

Update version string, download URLs, SHA256 hashes, and test assertion in:

1. `distribution/homebrew/turbo-lang.rb` (local copy in main repo)
2. `~/Desktop/Coding/ZVN/homebrew-turbo/Formula/turbo-lang.rb` (tap repo)

### 3.3 Commit and Push Both

```bash
# Main repo
cd ~/Desktop/Coding/ZVN/TurboLang
git add distribution/homebrew/turbo-lang.rb
git commit -m "Update local Homebrew formula with vX.Y.Z SHA256 hashes"
git push

# Tap repo
cd ~/Desktop/Coding/ZVN/homebrew-turbo
git add Formula/turbo-lang.rb
git commit -m "turbo-lang X.Y.Z"
git push
```

### 3.4 Verify Homebrew Install

```bash
brew update
brew reinstall turbo-lang
turbolang --version   # Must show X.Y.Z
which turbo-lsp       # Must exist
```

---

## Phase 4: VS Code Extension

**Always bump**, even if no syntax changes — keeps version numbers in sync.

```bash
cd ~/Desktop/Coding/ZVN/turbo-vscode
```

### 4.1 Update

- Bump `"version"` in `package.json` to match compiler
- Add any new keywords, snippets, or error codes
- Update grammar if syntax changed

### 4.2 Publish

```bash
vsce package
vsce publish
```

### 4.3 Verify

```bash
code --list-extensions --show-versions | grep turbo
# Should show new version
```

---

## Phase 5: Tree-sitter Grammar (if syntax changed)

Only needed when new syntax was added (new keywords, new expression forms, etc.).

```bash
cd ~/Desktop/Coding/ZVN/tree-sitter-turbo
```

- Update `grammar.js` with new syntax rules
- Run `tree-sitter generate && tree-sitter test`
- Bump version in `package.json`
- Commit and push

---

## Phase 6: Post-Release Verification

### Smoke test from clean install

```bash
# Download and verify release binary directly
gh release download vX.Y.Z --repo ZVN-DEV/Turbo-Language --pattern "*darwin-arm64*" -O /tmp/turbo.tar.gz
rm -rf /tmp/turbo-verify && mkdir -p /tmp/turbo-verify
tar xzf /tmp/turbo.tar.gz -C /tmp/turbo-verify
/tmp/turbo-verify/turbolang --version
ls /tmp/turbo-verify/turbo-lsp  # Must exist

# Verify Homebrew version matches
turbolang --version
```

### Verify version consistency

```bash
# All must report the same version
./scripts/check_release_consistency.sh
turbolang --version
grep '^version' ~/Desktop/Coding/ZVN/TurboLang/turbo/crates/turbo-cli/Cargo.toml
grep '"version"' ~/Desktop/Coding/ZVN/TurboLang/editors/vscode/turbo-lang/package.json
grep '"version"' ~/Desktop/Coding/ZVN/turbo-vscode/package.json
grep 'version "' ~/Desktop/Coding/ZVN/homebrew-turbo/Formula/turbo-lang.rb
```

---

## Important Reminders

- **Both runtimes**: Any runtime change must update BOTH `turbo_rt.c` (C/AOT) AND `runtime.rs` (Rust/JIT)
- **Never skip the tag**: The tag triggers release CI. No tag = no release binaries = can't update Homebrew.
- **Homebrew depends on CI**: Release CI must complete before you can get SHA256 checksums. Don't try to update the formula before the release is built.
- **Version sync is non-negotiable**: All workspace crates, Homebrew formula, VS Code extension, and CHANGELOG must all show the same version.
- **Test before tagging**: Once tagged and pushed, the release is public. Run all tests first.
