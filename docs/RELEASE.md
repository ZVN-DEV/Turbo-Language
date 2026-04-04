# Turbo Language Release Process

Step-by-step checklist for releasing a new version of Turbo. All version strings must agree across every artifact.

## Files That Contain Version Strings

| File | Field | Notes |
|------|-------|-------|
| `turbo/crates/turbo-cli/Cargo.toml` | `version` | Controls `--version` and REPL banner |
| `turbo/crates/turbo-ast/Cargo.toml` | `version` | Must match |
| `turbo/crates/turbo-lexer/Cargo.toml` | `version` | Must match |
| `turbo/crates/turbo-parser/Cargo.toml` | `version` | Must match |
| `turbo/crates/turbo-sema/Cargo.toml` | `version` | Must match |
| `turbo/crates/turbo-codegen-cranelift/Cargo.toml` | `version` | Must match |
| `turbo/crates/turbo-codegen-llvm/Cargo.toml` | `version` | Must match |
| `turbo/crates/turbo-lsp/Cargo.toml` | `version` | Must match |
| `turbo/Cargo.lock` | auto-updated | `cargo build` updates this |
| `CHANGELOG.md` | section header | `[X.Y.Z] - YYYY-MM-DD` |
| `distribution/homebrew/turbo-lang.rb` | version, URLs, sha256, test | Local copy |
| `homebrew-turbo/Formula/turbo-lang.rb` | version, URLs, sha256, test | Tap repo (separate) |

## Release Pipeline

### 1. Version Bump

Bump version in all 8 crate Cargo.toml files listed above. One-liner to check they agree:

```bash
grep '^version' turbo/crates/*/Cargo.toml
```

### 2. Changelog

Add a dated entry in `CHANGELOG.md` under the new version header:

```
## [X.Y.Z] - YYYY-MM-DD
### Added
- ...
### Fixed
- ...
```

### 3. Run All Tests

```bash
# Unit tests
cargo test --workspace --exclude turbo-codegen-llvm --manifest-path turbo/Cargo.toml

# Integration tests (needs release build first)
cargo build --release --manifest-path turbo/Cargo.toml
cd turbo && ./tests/run_tests.sh
```

All must pass. Zero failures.

### 4. Verify Version

```bash
./turbo/target/release/turbolang --version
# Should print: turbolang X.Y.Z
```

### 5. Commit and Push

```bash
git add -A
git commit -m "vX.Y.Z: <summary>"
git push origin master
```

### 6. Tag and Release

```bash
git tag vX.Y.Z
git push origin vX.Y.Z
```

This triggers `.github/workflows/release.yml` which builds:
- macOS ARM (Apple Silicon)
- macOS Intel (x86_64)
- Linux x86_64

### 7. Verify Release CI

```bash
gh run list --workflow=release.yml --limit=1
# Wait for it to complete
gh release view vX.Y.Z --repo ZVN-DEV/Turbo-Language
```

Check that the release has 3 tarballs + checksums.txt.

### 8. Update Homebrew

```bash
# Download checksums from release
gh release download vX.Y.Z --repo ZVN-DEV/Turbo-Language --pattern "checksums.txt" -O /tmp/checksums.txt
cat /tmp/checksums.txt
```

Update SHA256 hashes in BOTH locations:
1. `distribution/homebrew/turbo-lang.rb` (local copy in this repo)
2. `~/Desktop/Coding/ZVN/homebrew-turbo/Formula/turbo-lang.rb` (tap repo)

Update: version string, download URL version slugs, sha256 hashes, test version assertion.

```bash
# Commit and push both repos
cd ~/Desktop/Coding/ZVN/new-language
git add distribution/homebrew/turbo-lang.rb
git commit -m "Update local Homebrew formula with vX.Y.Z SHA256 hashes"
git push

cd ~/Desktop/Coding/ZVN/homebrew-turbo
git add Formula/turbo-lang.rb
git commit -m "turbo-lang X.Y.Z"
git push
```

### 9. Verify Homebrew Install

```bash
brew update
brew reinstall turbo-lang
turbolang --version
turbo-lsp --version  # or just check it exists
```

### 10. VS Code Extension (if needed)

Only needed if language syntax, snippets, or LSP protocol changed.

```bash
cd ~/Desktop/Coding/ZVN/turbo-vscode
# Update package.json version
# Update any grammar or snippet changes
vsce package
vsce publish
```

### 11. Post-Release Verification

```bash
# Download release tarball and verify contents
gh release download vX.Y.Z --repo ZVN-DEV/Turbo-Language --pattern "*darwin-arm64*" -O /tmp/turbo.tar.gz
tar xzf /tmp/turbo.tar.gz -C /tmp/turbo-verify
/tmp/turbo-verify/turbolang --version
ls /tmp/turbo-verify/turbo-lsp  # Must exist
```

## Important Reminders

- **Both runtimes**: Any runtime change must update BOTH `turbo_rt.c` (C/AOT) and `runtime.rs` (Rust/JIT)
- **Test count**: Update README badge if test count changed
- **Homebrew SHA256**: Release CI must complete before you can get the checksums
- **Don't skip the tag**: The tag triggers release CI. No tag = no release binaries.
