# Security Policy

## Supported Versions

| Version | Status |
|---------|--------|
| 0.10.x  | Current — supported with security fixes |
| 0.9.x   | Security fixes only while 0.10.x is the current series |
| < 0.9   | Not supported |

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
  or experimental (currently: the WASM target).
  Agent/tool language primitives are not shipped — and are not planned
  for the core — so they are out of scope by omission, not by experiment
- Crashes triggered only by `@unsafe` code or raw pointer arithmetic —
  by design these bypass safety checks
- Issues in third-party dependencies (please report upstream)

## Security Model

### 1. JIT Execution (`turbolang run`)

`turbolang run` compiles source code and executes it in-process with
full OS permissions. There is no sandboxing, no capability restriction,
and no isolation between the Turbo program and the host system.

**Treat `.tb` files like executables.** Do not run untrusted `.tb`
source files. A malicious `.tb` file can read, write, or delete any
file accessible to the user, make network requests, and execute
arbitrary commands (via the `exec` built-in).

### 2. Playground (`turbolang playground`)

The playground starts a local HTTP server on `localhost` with CSRF
protection (origin checking) and random session tokens (128-bit OS
randomness). Source files are written via exclusive-create temp files
to prevent TOCTOU races.

The playground is designed for single-user, local development. It is
**not designed for multi-tenant deployment.** Do not expose the
playground to untrusted networks.

### 3. HTTP Server

The built-in HTTP server (`http_server`, `route`, `http_listen`) is
development-grade:

- `http_server(port)` binds to `127.0.0.1` (localhost only)
- `http_server_public(port)` binds to `0.0.0.0` (all interfaces)
- No TLS termination
- Request headers are capped at 16 KiB
- Request bodies are capped at 32 MiB via `Content-Length` validation
- Connection cap with 503 backpressure (tuned for development loads)
- Invalid server IDs are rejected

**For production deployment, run behind a reverse proxy** (nginx,
Caddy, or similar) that provides TLS termination, rate limiting,
request filtering, and authentication.

### 4. AOT Binaries

Binaries produced by `turbolang build` are native executables. They
inherit OS-level permissions of the user who runs them. Turbo provides
no built-in sandboxing, capability model, or privilege restriction.

AOT linker flags are allowlisted (alphanumeric + `_.+-` only) to
prevent a dependency from smuggling malicious flags through the build.

### 5. C FFI

The `@unsafe extern "C"` block allows calling arbitrary C functions.
These calls bypass all of Turbo's safety checks. A C function accessed
via FFI can corrupt memory, dereference arbitrary pointers, or perform
any operation the C ABI permits.

Within `@unsafe` blocks, the `deref` and `store` builtins allow
reading from and writing to arbitrary memory addresses.

### 6. File I/O

`read_file`, `write_file`, `try_read_file`, and `try_write_file`
operate on any path the OS user has access to. There is no path
sanitization, no chroot, and no allowlist of accessible directories.

### 7. Shell Execution (`exec` / `shell_exec`)

As of v0.8.0, `exec` and `shell_exec` reject commands containing shell
metacharacters (`;`, `|`, `&`, `$`, backticks, parentheses, `<`, `>`,
newlines, backslashes). Commands are tokenized on whitespace and
executed directly via `execvp` with a 64-argument cap -- no shell is
involved.

This prevents shell injection attacks (e.g., `exec("echo hi; rm -rf /")`
is rejected). However, the underlying command still runs with the full
OS permissions of the Turbo process.

## Known Hardening Limits

The following are documented limitations rather than vulnerabilities;
fixing them is tracked in `CHANGELOG.md`:

- **HTTP server primitives are development-grade.** See Security Model
  section 3 above for details. **Always put a reverse proxy (nginx,
  Caddy) in front of a public deployment.**
- **JIT string arena is not individually freed.** The runtime uses a
  thread-local string arena that is freed after each JIT execution.
  Long-running AOT servers should be monitored for memory usage.
  Proper ARC-based string deallocation is planned for a future release.
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
