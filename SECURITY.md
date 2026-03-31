# Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| 0.1.x   | Yes       |
| < 0.1   | No        |

## Reporting a Vulnerability

**Do NOT open a public issue for security vulnerabilities.**

To report a security vulnerability, use one of the following channels:

- **Email:** security@zvn.dev
- **GitHub:** [Private vulnerability reporting](https://github.com/ZVN-DEV/Turbo-Language/security/advisories/new)

Include as much detail as possible: steps to reproduce, affected versions, and any potential impact.

## Response Timeline

- **Acknowledgment:** within 48 hours of receipt
- **Critical fixes:** within 7 days
- **Non-critical fixes:** addressed in the next scheduled release

You will be kept informed of progress toward a fix and full announcement. We may ask for additional information or guidance during the process.

## Known Limitations

Turbo is pre-1.0 software. The following areas have known limitations and are actively being hardened:

- **Array bounds checking:** Out-of-bounds array access may not be caught in all code paths at runtime.
- **Allocation validation:** The C runtime (`turbo_rt.c`) performs checked allocation (exits on OOM) but does not guard against all classes of invalid size or overflow inputs.
- **Unsafe blocks:** Code inside `@unsafe` blocks bypasses normal safety checks by design. Review unsafe code carefully.
- **No sandboxing:** Compiled Turbo binaries run with full system access. There is no capability-based restriction model.

## Disclosure Policy

We follow coordinated disclosure. Once a fix is released, we will credit reporters (unless anonymity is requested) and publish a brief advisory describing the issue and remediation.
