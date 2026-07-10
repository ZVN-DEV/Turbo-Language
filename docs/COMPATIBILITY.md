# Platform Compatibility

TurboLang targets macOS and Linux as first-class platforms. Windows support is
**experimental** and lands in tiers. This page documents what works where and,
in particular, the current shape of Windows support.

## Platform support matrix

| Capability | macOS | Linux | Windows |
|---|:---:|:---:|:---:|
| JIT (`turbolang run`) | Yes | Yes | Yes (Tier A) |
| Unit tests (`cargo test`) | Yes | Yes | Yes (Tier A) |
| Native AOT (`turbolang build`) — core language + stdlib | Yes | Yes | Yes (Tier B) |
| AOT: prints, strings/ARC, arrays, structs/enums/match | Yes | Yes | Yes |
| AOT: hashmaps, math, JSON, sorting | Yes | Yes | Yes |
| AOT: SQLite, file I/O, date/time | Yes | Yes | Yes |
| AOT: spawn / channels / mutex (threads) | Yes | Yes | Stub¹ |
| AOT: HTTP client (`http_get`/`http_get_raw`/`http_post`) | Yes | Yes | Stub¹ |
| AOT: HTTP server (`http_server`/`respond`) | Yes | Yes | Stub¹ |
| WASM target (`--target wasm`) | Yes² | Yes² | No |

¹ Compiled in as a fail-loud runtime-error stub — see "Windows AOT" below.
² Requires an LLVM/`wasm-ld` + WASI sysroot toolchain.

## Windows

### JIT (Tier A)

`turbolang run` executes programs through the in-process Cranelift JIT and the
pure-Rust runtime (`src/runtime.rs`). It does **not** compile the C runtime, so
it is fully supported on `x86_64-pc-windows-msvc`, including SQLite (the
vendored amalgamation is compiled with clang and archived with `llvm-lib` at
build time). If you can build the compiler on Windows, `turbolang run` works.

### Native AOT (Tier B)

`turbolang build` produces a native `.exe` for programs that use the **core
language and standard library**: prints, strings and reference counting,
arrays, structs, enums and `match`, hashmaps (legacy and generic), math, JSON,
sorting, SQLite, file I/O, and date/time.

**Toolchain requirement.** The AOT linker driver on Windows is **clang**
(`cc` does not exist under MSVC). clang ships on the GitHub `windows-latest`
runner image and with Visual Studio's "Desktop development with C++" workload.
It compiles the C runtime (`turbo_rt.c`) and links the executable through the
MSVC toolchain. Override it with the `CC` environment variable if needed. If no
usable C driver is found, `turbolang build` fails with an actionable message
naming the requirement (JIT needs no C toolchain).

**Unsupported builtins are fail-loud, not silent.** The concurrency primitives
(`spawn`, channels, `mutex`), the HTTP client/server, and process execution
(`exec`/`shell_exec`) rely on POSIX APIs (pthreads, `fork`/`exec`, BSD sockets)
that the port does not yet provide on Windows. They are **compiled in as stubs** so that any program links, but
calling one aborts immediately with, for example:

```
runtime error: spawn/async concurrency is not yet supported in Windows AOT
(`turbolang build`) binaries; use `turbolang run` (JIT) or build on a
non-Windows target
```

Use `turbolang run` (JIT) for these features on Windows, or build on macOS or
Linux.

### How it is implemented

`turbo_rt.c` is a POSIX-first C file that the JIT never touches; only AOT links
it. For Windows it compiles under clang/`windows-msvc` via a small set of
`#ifdef _WIN32` shims that keep the POSIX code byte-identical:

- BSD/POSIX string and filesystem calls are mapped to their UCRT equivalents
  (`strcasecmp` → `_stricmp`, `access` → `_access`, `mkdir` → `_mkdir`,
  `S_ISDIR`, `localtime_r` → `localtime_s` with corrected argument order).
- `gettimeofday` is replaced with `GetSystemTimeAsFileTime`; directory listing
  uses `FindFirstFile`/`FindNextFile` instead of `opendir`/`readdir`.
- The NON-CORE sections (spawn/channels/mutex, HTTP client, HTTP server) are
  `#ifndef _WIN32`-guarded and replaced by the runtime-error stubs above.

CI's advisory `test-windows` job builds and runs a core subset of the phase1
programs as native `.exe` on every push to catch regressions.
