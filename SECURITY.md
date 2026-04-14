# Security Policy

## Supported Versions

| Version | Status |
|---------|--------|
| 0.7.x   | Current — supported with security fixes |
| 0.6.x   | Security fixes only while 0.7.x is the current series |
| < 0.6   | Not supported |

## Reporting a Vulnerability

**Do NOT open a public issue for security vulnerabilities.**

To report a vulnerability, use one of these private channels:

- **Email:** `security@turbolang.dev`
- **GitHub:** [Private vulnerability reporting](https://github.com/ZVN-DEV/Turbo-Language/security/advisories/new)

Please include:
- A clear description of the issue and its impact
- A minimal reproducer (a `.tb` source file is ideal)
- The affected version (`turbolang --version`)
- Your assessment of severity, if you have one

## Response Timeline

- **Acknowledgment:** within **48 hours** of receipt
- **Critical fixes:** target **7 days** from acknowledgment
- **Non-critical fixes:** rolled into the next scheduled release

You will be kept informed of progress and credited (unless you request
anonymity) once a fix ships.

## Scope

**In scope:**
- The Turbo compiler (`turbo-cli`, `turbo-parser`, `turbo-sema`,
  `turbo-codegen-cranelift`)
- The C runtime (`turbo/crates/turbo-codegen-cranelift/runtime/turbo_rt.c`)
- The LSP server (`turbo-lsp`)
- The install script and Homebrew formula
- Any feature documented as stable in `README.md` or `docs/`

**Out of scope:**
- Experimental features explicitly flagged in `CHANGELOG.md` as unstable
  or experimental (currently: `tool fn` agent primitives, the WASM
  target, and the LLVM backend)
- Crashes triggered only by `@unsafe` code or raw pointer arithmetic —
  by design these bypass safety checks
- Issues in third-party dependencies (please report upstream)

## Known Hardening Limits

The following are documented limitations rather than vulnerabilities;
fixing them is tracked in `CHANGELOG.md` and `TODO.md`:

- **HTTP server primitives are experimental.** `http_server` /
  `http_listen` are intended for development and demos. They are not
  hardened for direct exposure to untrusted networks. As of v0.5.1
  the default bind is `127.0.0.1`; the explicit
  `http_server_public(port)` opt-in binds `0.0.0.0`. **Always put a
  reverse proxy (nginx, Caddy) in front of a public deployment.**
- **No reference counting yet.** `rt_release` is currently a no-op,
  so long-running services leak memory at allocation rate (~2.5 KB
  per request on the example HTTP server). Real ARC is planned for
  v0.6 — see `TODO.md`.
- **Compiled binaries run with full system privileges.** Turbo has no
  capability/sandbox model. Treat compiled `.tb` programs the same way
  you would any compiled C program.

## Release Signing and Verification

Every release tarball is published alongside a `checksums.txt` file
listing SHA-256 hashes for each platform artifact. Stable releases also
ship a detached GPG signature for that manifest (`checksums.txt.sig`).
The manifest is signed by the official Turbo release key, published at:

- **Release key URL:** `https://turbolang.dev/keys/release.asc`

The release automation that builds, signs, and publishes those artifacts
is pinned to immutable GitHub Action SHAs in:

- `.github/workflows/ci.yml`
- `.github/workflows/release.yml`
- `.github/workflows/nightly.yml`

This reduces supply-chain drift in CI itself: action upgrades are now
explicit code changes instead of silent tag movement.

To verify a download manually:

```bash
# 1. Import the public release key (one-time setup).
curl -sSL https://turbolang.dev/keys/release.asc | gpg --import

# 2. Verify the manifest signature.
gpg --verify checksums.txt.sig checksums.txt
# Expect: "Good signature from Turbo Language Releases <release@turbolang.dev>"

# 3. Verify the tarball you downloaded matches the signed manifest.
sha256sum --check --ignore-missing checksums.txt
```

If `gpg --verify` reports anything other than `Good signature`, **stop
and report it to security@turbolang.dev** — do not install the binary.

The release private key lives only in GitHub Actions secrets
(`RELEASE_GPG_PRIVATE_KEY` + `RELEASE_GPG_PASSPHRASE`) and is used by
`.github/workflows/release.yml`. If the key is rotated, the new public
key replaces the file at `https://turbolang.dev/keys/release.asc` and a
release advisory is published to announce the rotation.

### `install.sh --verify`

`distribution/install.sh --verify` now performs the same signed-manifest
flow automatically:

1. Download the requested tarball, `checksums.txt`, and `checksums.txt.sig`
   from the GitHub release.
2. Download the release public key from `https://turbolang.dev/keys/release.asc`
   into a temporary GnuPG home.
3. Verify that `checksums.txt.sig` matches `checksums.txt`.
4. Verify that the tarball hash matches the signed manifest entry.

This is materially stronger than checksum-only verification because a
modified tarball now also needs a forged manifest signature.

### Remaining bootstrap trust assumption

The convenience installer still has one unavoidable bootstrap
assumption: on first use it trusts HTTPS/DNS for `turbolang.dev` when it
retrieves the public release key. That is better than pulling the key
from the same GitHub release channel as the tarball, but it is still a
network trust dependency.

If you want the strongest assurance available today:

1. Import the release key yourself ahead of time from a channel you
   trust.
2. Run the manual `gpg --verify` flow above before extracting the
   tarball.
3. Prefer vendoring the release key in internal build/install systems
   instead of fetching it at install time.

## Disclosure Policy

We follow coordinated disclosure. Once a fix is released, we publish a
brief advisory describing the issue, affected versions, the fix, and
credit to the reporter (unless anonymity was requested).
