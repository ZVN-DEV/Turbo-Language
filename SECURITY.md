# Security Policy

## Supported Versions

| Version | Status         |
|---------|----------------|
| 0.5.x   | Current — supported with security fixes |
| 0.4.x   | End of life    |
| 0.3.x   | End of life    |
| < 0.3   | Not supported  |

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

## Release Signing Key

Every release tarball is published alongside a `checksums.txt` file
listing SHA-256 hashes for each platform artifact. Starting with the
v0.6 series, `checksums.txt` is also accompanied by a detached GPG
signature (`checksums.txt.sig`) produced by the official Turbo release
key.

**Public key (primary):** `https://turbolang.dev/keys/release.asc`

**Public key (fallback):** the same key is published as a `release.asc`
asset on every GitHub release starting with v0.7. If `turbolang.dev` is
unreachable (or you simply prefer to fetch the key from the same
distribution channel as the tarball), download the asset directly from
the release page and import it the same way. The release workflow will
be updated to upload this asset as part of the v0.7 cut — see
[`#release-key-fallback` tracker]
(https://github.com/ZVN-DEV/Turbo-Language/issues?q=label%3Arelease-key-fallback).

To verify a download:

```bash
# 1. Import the public release key (one-time setup).
#    Primary:
curl -sSL https://turbolang.dev/keys/release.asc | gpg --import
#    Fallback (v0.7+):
#    gh release download v0.7.0 --repo ZVN-DEV/Turbo-Language --pattern release.asc \
#      && gpg --import release.asc

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

### Trust gap: `install.sh` does not GPG-verify

`distribution/install.sh` verifies the downloaded tarball against the
`checksums.txt` file from the same release, but it **does not** verify
that `checksums.txt` itself was signed by the release key. A
sufficiently motivated attacker who can intercept the connection
between you and GitHub could serve a modified tarball *and* a matching
modified `checksums.txt` and the install script would be none the
wiser.

This is a known limitation of the convenience installer. If you want
cryptographic guarantees that the binary you're running came from the
official release pipeline, do not use `install.sh`. Instead:

1. Download the release artifact and `checksums.txt.sig` from the
   GitHub release page directly.
2. Run the `gpg --verify` flow above.
3. Extract the verified tarball into your `$PATH` by hand.

We accept this trade-off because the install script is the lowest-
friction onboarding path and most users do not have a GPG keyring set
up. The honest documentation of the gap is the mitigation.

## Disclosure Policy

We follow coordinated disclosure. Once a fix is released, we publish a
brief advisory describing the issue, affected versions, the fix, and
credit to the reporter (unless anonymity was requested).
