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

- **Email:** `security@turbolang.org` *(TODO: update this address before
  publishing — the project maintainer should swap in a real disclosure
  inbox)*
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

## Disclosure Policy

We follow coordinated disclosure. Once a fix is released, we publish a
brief advisory describing the issue, affected versions, the fix, and
credit to the reporter (unless anonymity was requested).
