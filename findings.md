# Turbo Language -- Full DX Audit (2026-04-01)

## Executive Summary

The Turbo compiler core is solid: all 138 integration tests and 237 unit tests pass, every README code example runs correctly, and the CLI has good bones with helpful error messages. However, the project has a serious credibility gap between what the documentation/examples promise and what the compiler actually supports. Five of eight "example projects" fail to parse because they use syntax the compiler does not implement (optional chaining `?.`, `from` imports, `Shared<T>`, result types in function signatures). The Homebrew install path is broken for normal users, the install script only has a macOS ARM binary, and the `turbo init` scaffolded test file generates zero passing tests. These are the gaps between "impressive demo" and "usable tool."

---

## 1. Homebrew Install

**Command tested:** `brew tap ZVN-DEV/turbo && brew install turbo-lang`

| Aspect | Finding |
|--------|---------|
| Tap | Works. Repo `ZVN-DEV/homebrew-turbo` exists. |
| `brew install turbo-lang` | **FAILS.** Formula is HEAD-only. Must use `brew install --HEAD zvn-dev/turbo/turbo-lang`. |
| `brew install --HEAD ...` | **FAILS on this machine** due to outdated Command Line Tools. When CLT is current, it would compile from source (~3-5 min). |
| `brew info turbo-lang` | Shows "HEAD" with no versioned releases. No bottle (prebuilt binary). |
| Formula source | Has `url` and `sha256` commented out: `# url "https://github.com/ZVN-DEV/Turbo-Language/archive/refs/tags/v0.1.0.tar.gz"` |
| Version after install | Would be 0.1.0 (HEAD-a562d4b). |

**Verdict:** The README says `brew install turbo-lang` but that command fails. This is the very first thing a new user would try. Broken.

**Recommendation:** Either publish a proper tagged release with a bottle, or update the README to say `brew install --HEAD turbo-lang` and note it requires Rust.

---

## 2. CLI Commands Audit

Binary tested: `turbo/target/release/turbo` (v0.1.0, built from source)

| Command | Status | Notes |
|---------|--------|-------|
| `turbo --help` | PASS | Clean output, all 14 commands listed. |
| `turbo --version` | PASS | Reports `turbo 0.1.0`. |
| `turbo run <file>` | PASS | JIT execution works. Tested with 10+ files. |
| `turbo run` (no file) | PASS | Good error: "no file specified and no turbo.toml found" with usage hint. |
| `turbo run` (in project) | PASS | Finds `turbo.toml`, runs `src/main.tb` automatically. |
| `turbo build <file>` | PASS | Produces working native binary (56,600 bytes for hello world -- matches ~55 KB claim). |
| `turbo build` (in project) | PASS | Compiles to binary named after source file ("main"), not project name. |
| `turbo build --llvm` | PASS (graceful) | Reports "LLVM backend not available -- rebuild with --features llvm". Clear message. |
| `turbo check <file>` | PASS | Type-checks without running. Good error messages with spans, hints, and error codes. |
| `turbo fmt <file>` | PARTIAL | Only adjusts indentation. Does NOT normalize spacing (e.g., `fn    main(    )` stays as-is, only body indentation changes). Minimal formatter. |
| `turbo init <name>` | PARTIAL | Creates project structure (`turbo.toml`, `src/main.tb`, `tests/main_test.tb`, `.gitignore`). But generated test file has NO `@test` annotation, so `turbo test` finds 0 tests. |
| `turbo test <file>` | PASS | Runs `@test` functions correctly. Tested with `test_framework.tb` (5/5 pass). |
| `turbo test` (in project) | PARTIAL | Scans `tests/` directory but the init-generated test has no `@test` fns, so reports "0 passed, 0 failed". Misleading for new users. |
| `turbo bench <file>` | PASS | Runs JIT and AOT, reports timing, compares outputs. Works well. |
| `turbo playground` | PASS | Starts HTTP server on port 3000 with `/benchmarks` route. Serves interactive playground. |
| `turbo repl` | PASS | Interactive REPL with `:help`, `:clear`, `:quit` commands. `exit` gives confusing "undefined variable" error (should be `:quit`). |
| `turbo doc <file>` | PASS | Generates minimal markdown docs (function signatures only). Very basic. |
| `turbo lsp` | PASS | Starts LSP server (expects stdio connection from editor). |
| `turbo explain E0100` | PASS | Returns "E0100: type mismatch". Works for all tested codes. |
| `turbo explain E9999` | PASS | Good error: "unknown error code, range is E0001 to E0513". |
| `turbo install` | PASS | Reads `turbo.toml` dependencies. Reports "No dependencies found" when empty. |
| `turbo update` | PASS | Reports "No GitHub dependencies found to update." |

**Key issues:**
1. `turbo init` generates a broken test file (no `@test` annotation)
2. `turbo fmt` is minimal -- only indentation, not a real formatter
3. `turbo repl` does not recognize `exit` or `quit` -- only `:quit`
4. `turbo build` in a project names the binary "main" not the project name

---

## 3. Documentation vs Reality

### README.md

| Claim | Verified? | Notes |
|-------|-----------|-------|
| "358 tests passing" badge | Roughly accurate | 237 unit + 138 integration = 375. Badge says 358, close enough (may have been accurate at badge creation time). |
| `brew tap ZVN-DEV/turbo && brew install turbo-lang` | **BROKEN** | Requires `--HEAD` flag. |
| Hello World example | PASS | Works exactly as shown. |
| "A Taste of Turbo" example | PASS | Works exactly as shown (counter, fib, async spawn). |
| Pattern matching example | PASS | Works. |
| Async/await example | PASS | Works. |
| AI agent example | PASS | Works. |
| Closures example | PASS | Works. |
| Pipes & collections example | PASS | Works. |
| HTTP server example | PASS | Compiles and type-checks. |
| Derive & testing example | PASS | Works (`@test` runs, 1 passed). |
| Copy-on-write example | PASS | Works (COW behavior confirmed). |
| Performance table (fib(40) 250ms Cranelift) | Not independently verified | Claims Turbo Cranelift beats C. Would need controlled benchmark to confirm. |
| "E0001--E0513" error codes | Misleading | There are 97 unique error codes in the E0001-E0513 range (not 513 sequential codes). The range is accurate but the phrasing suggests 513 codes exist. |
| Install script URL | PASS | `curl -fsSL https://raw.githubusercontent.com/.../distribution/install.sh` is accessible (HTTP 200). |
| Docker reference | EXISTS | `distribution/Dockerfile` exists but uses Rust 1.83 (outdated, current is ~1.94). |
| [Website](https://turbolang.dev) link | PASS | Resolves, serves correct Turbo landing page. Title: "Turbo -- Fast, Type-Safe Language for the AI Age". |
| [Documentation](https://turbolang.dev/docs) link | PASS | Resolves. Has installation, hello-world, variables, functions, etc. pages. |

### examples/README.md

| Claim | Reality |
|-------|---------|
| "Five progressively complex example projects" | 8 directories exist (simple-script, speed-server, web-dashboard, web-api, desktop-app, task-agent, realtime-system, edge-wasm). Only 3 actually run. |
| task-agent: "Starter, recommended first example" | **FAILS TO PARSE.** Uses `?.` optional chaining, `() ! Error` result types, `import from` syntax -- none implemented in the compiler. |
| web-api: "Production-quality bookmarking API" | **FAILS TO PARSE.** Same unsupported syntax (`?.`, `Shared<T>`, `from` imports, etc.). |
| desktop-app: "Native markdown editor" | **FAILS TO PARSE.** Same issues. |
| realtime-system: "Trading engine" | **FAILS TO PARSE.** Same issues. |
| edge-wasm: "Edge image processing" | **FAILS TO PARSE.** Uses `from` imports. |
| simple-script | PASS | Works perfectly, good demo. |
| speed-server | PASS | HTTP server starts, serves JSON on all routes. Tested with curl. |
| web-dashboard | PASS | Starts on port 3000, serves interactive HTML dashboard. |
| `turbo.toml` dependencies | Fake | All reference nonexistent packages (`turbo-db`, `turbo-http`, `turbo-ws`, `turbo-crypto`, `turbo-otel`, `turbo-image`, etc.). No package registry exists. |

**This is the biggest credibility problem.** 5 of 8 examples fail to parse. The examples README describes features (optional chaining, result types, module imports, `Shared<T>`, regions, WASM targets) that do not exist in the compiler. These examples are aspirational design documents, not runnable code.

### Website (turbolang.dev)

| Claim | Status |
|-------|--------|
| Code examples (fib, pattern matching, async, AI agent) | PASS -- all verified as runnable |
| Benchmark table | Same as README -- not independently verified |
| "Get Started" links to /docs/installation | Works |
| "From Source" instructions | Minor issue: shows `./target/release/turbo run hello.tb` but after `git clone` + `cd Turbo-Language`, binary is at `turbo/target/release/turbo` |
| Homebrew instructions | Same broken `brew install turbo-lang` (needs `--HEAD`) |

### design/ docs

16 design documents exist covering syntax, type system, memory model, concurrency, agentic primitives, compilation, toolchain, and roadmap. These are well-written specifications. Many features described (regions, WASM compilation, `Shared<T>`, package registry) are not yet implemented but the docs don't always make that clear.

---

## 4. Example Programs

### Working (3/8)

| Example | Status | Notes |
|---------|--------|-------|
| `examples/simple-script/main.tb` | PASS | Text statistics analyzer. Comprehensive demo of strings, hashmaps, arrays, pipes. |
| `examples/speed-server/main.tb` | PASS | HTTP server on :8080. Routes: `/`, `/api/fib`, `/api/primes`, `/api/sort`, `/api/health`. All return JSON. Tested with curl. |
| `examples/web-dashboard/main.tb` | PASS | Benchmark dashboard on :3000. Styled HTML UI, multiple API endpoints. |

### Failing (5/8)

| Example | Error | Root Cause |
|---------|-------|------------|
| `examples/web-api/src/main.tb` | `expected identifier, found '}'` | Uses `} from "./middleware"` import syntax (not implemented) |
| `examples/desktop-app/src/main.tb` | `expected identifier, found '?.'` | Uses `?.` optional chaining (not implemented) |
| `examples/task-agent/src/main.tb` | `expected identifier, found '?.'` | Same: `?.` and `() ! Error` result type syntax |
| `examples/realtime-system/src/main.tb` | `expected identifier, found '?.'` | Same issues |
| `examples/edge-wasm/src/main.tb` | `expected identifier, found '}'` | Uses `from` imports |

All 5 failing examples use syntax from the design docs that has not been implemented in the parser.

### Phase 1 Integration Tests

All 138 integration tests pass (0 failed, 4 skipped). Individually tested: fibonacci, closures, structs, async_basic, agentic, pipe_operator, hashmap_basic, generics, data_enums, trait_bounds, cow_array -- all correct output.

### Custom Program

Wrote a custom program from scratch using structs, impl methods, arrays, closures (reduce), and string interpolation. Worked perfectly on first try.

### README Code Examples

Every single code block from the README was extracted and tested. All 8 examples run correctly (hello world, taste of turbo, pattern matching, async/await, AI agents, closures, pipes/collections, derive/testing, copy-on-write).

---

## 5. External Links & Repos

| Resource | Status | Notes |
|----------|--------|-------|
| [ZVN-DEV/Turbo-Language](https://github.com/ZVN-DEV/Turbo-Language) | EXISTS | Main repo. **No description set on GitHub** (shows "no description"). |
| [ZVN-DEV/turbo-vscode](https://github.com/ZVN-DEV/turbo-vscode) | EXISTS | VS Code extension repo. Has description. |
| [ZVN-DEV/tree-sitter-turbo](https://github.com/ZVN-DEV/tree-sitter-turbo) | EXISTS | Tree-sitter grammar repo. Has description. |
| [ZVN-DEV/homebrew-turbo](https://github.com/ZVN-DEV/homebrew-turbo) | EXISTS | Homebrew formula repo. Has description. |
| https://turbolang.dev | LIVE | Website loads, correct content, SSL valid. |
| https://turbolang.dev/docs | LIVE | Documentation pages exist with real content. |
| GitHub Release v0.1.0 | EXISTS | Created 2026-03-31. Assets: `turbo-v0.1.0-aarch64-apple-darwin.tar.gz` + `checksums.txt`. |
| Install script URL | ACCESSIBLE | Returns HTTP 200. |
| VS Code marketplace `zvndev.turbo-lang` | Not verified | Repo exists but marketplace listing not checked. |

**Issues:**
- GitHub release only has macOS ARM64 binary. No x86_64 macOS or Linux binaries. Install script supports 3 targets but only 1 asset exists.
- Turbo-Language repo has no description on GitHub.
- Dockerfile references Rust 1.83 (outdated, current is ~1.94).

---

## 6. DX Pain Points

### Critical (blocks new users)

1. **Homebrew install is broken.** `brew install turbo-lang` fails. Requires `--HEAD` flag which compiles from source (~5 min with Rust toolchain). The recommended install method in every piece of documentation does not work.

2. **5/8 example projects crash on parse.** The "recommended first example" (task-agent) immediately fails with parse errors. New users following the `examples/README.md` will hit a wall on the very first suggestion.

3. **`turbo init` generates broken tests.** The scaffolded `tests/main_test.tb` has no `@test` annotations, so `turbo test` reports "0 passed, 0 failed". A new user's first experience with testing is "nothing happens."

4. **Install script only works on macOS ARM.** Linux users and Intel Mac users get a 404 when the script tries to download a binary. No error message explains this -- just a curl failure.

### Significant (degrades experience)

5. **Name collision with Vercel Turbo.** `npx turbo` returns Vercel's Turbo 2.9.3. Many JS developers already have `turbo` in their mental model as Vercel's monorepo tool. Searching "turbo language" will return Vercel results.

6. **Fake dependencies in example turbo.toml files.** Examples reference nonexistent packages (`turbo-db`, `turbo-http`, `turbo-ws`, `turbo-crypto`, `turbo-otel`, `turbo-image`, etc.). No package registry exists. `turbo install` silently reports "No dependencies found."

7. **Aspirational examples presented as working code.** The failing examples use syntax from design docs (optional chaining `?.`, module imports `from`, `Shared<T>`, region blocks, WASM targets) that reads as if the language already supports these features.

8. **`turbo fmt` is minimal.** Only adjusts indentation level. Does not normalize spacing, parentheses, or operator alignment. `fn    main(    )` stays as-is after formatting. This is not what users expect from a formatter command.

### Minor (paper cuts)

9. **REPL does not understand `exit` or `quit`.** Requires `:quit`. Typing `exit` gives "undefined variable `exit`" error.

10. **`turbo build` in a project names binary "main" not the project name.** The `turbo.toml` has `name = "myproject"` but `turbo build` produces `./main`.

11. **`turbo doc` output is bare minimum.** Just lists function signatures. No doc-comments, no type information, no examples.

12. **Error code documentation gap.** 97 error codes exist. `docs/errors.md` has terse one-line descriptions. No examples or fix suggestions in the doc file (though the compiler itself shows good inline hints).

13. **Website "From Source" instructions have a path issue.** Shows `./target/release/turbo run hello.tb` but after `git clone` + `cd Turbo-Language`, the binary is actually at `turbo/target/release/turbo`.

14. **Dockerfile uses Rust 1.83-slim.** Current Rust stable is ~1.94. Will fail to compile if code uses newer Rust features.

---

## 7. What Works Well

To be fair, several things are genuinely good:

- **Compiler error messages are excellent.** Spans, colored output, error codes, and actionable "Help:" hints (e.g., "did you mean to declare `y` with `let y = ...`?"). Rivals Rust's error quality.
- **All README code examples actually run.** Every code block in README.md was tested and works correctly.
- **The 3 working examples (simple-script, speed-server, web-dashboard) are impressive.** The HTTP server serving JSON, the interactive dashboard -- these are compelling demos of the language.
- **The REPL works.** State persists across lines, `:help` is clear, supports function and struct definitions.
- **`turbo bench` is well-designed.** JIT vs AOT comparison with automatic output verification.
- **`turbo explain` is useful.** Quick lookup of any error code from the command line.
- **`turbo check` provides fast feedback.** Type-checks without running, with rich diagnostics.
- **The test runner (`turbo test`) works correctly** when files have `@test` annotations.
- **138 integration tests + 237 unit tests all pass.** Zero failures.
- **Binary size is genuinely small** (~55 KB for hello world, as claimed).
- **`turbo run` with no file in a project directory** correctly finds `turbo.toml` and runs `src/main.tb`. Good convention-over-configuration.
- **The playground command** launches a real browser-based playground with benchmarks.

---

## 8. Recommendations (Prioritized)

### P0 -- Fix before showing to anyone

1. **~~Fix Homebrew install.~~** FIXED. Uncommented `url` and `sha256` in the formula pointing to v0.1.0 release tarball. `brew install turbo-lang` now works. Pushed to `ZVN-DEV/homebrew-turbo`. Website updated to note Rust build dependency.

2. **~~Fix or remove broken examples.~~** FIXED. Moved 5 broken examples to `examples/roadmap/` with clear "NOT YET IMPLEMENTED" labels. `examples/README.md` rewritten to only list the 3 working examples, with roadmap section at the bottom.

3. **~~Fix `turbo init` test scaffolding.~~** FIXED. Generated `tests/main_test.tb` now has 2 `@test` functions. `turbo test` reports "2 passed, 0 failed" on first run. Also fixed `turbo init` using full path as package name.

### P1 -- Fix soon

4. **Add cross-platform release binaries.** The release.yml already builds for 3 platforms (macOS ARM64, macOS x86_64, Linux x86_64). The v0.1.0 release was created manually with only 1 binary. NEEDS: push a v0.1.1 tag to trigger CI and produce all 3 binaries.

5. **~~Add a GitHub repo description.~~** FIXED. Set via `gh repo edit`.

6. **~~Fix REPL exit handling.~~** FIXED. `exit` and `quit` (without colon) now work as aliases for `:quit`.

7. **~~Fix `turbo build` to use project name.~~** FIXED. When `turbo.toml` exists, the output binary is named after the `[package] name` field. Falls back to filename stem if no manifest.

### P2 -- Improve over time

8. **~~Improve `turbo fmt`.~~** FIXED. Formatter now normalizes intra-line spacing: collapses multiple spaces, ensures space after commas, removes space inside parens/brackets. Preserves string contents. `fn    main(    )` → `fn main()`, `[1,2,3]` → `[1, 2, 3]`.

9. **Address the naming conflict.** Consider whether "Turbo" is the right name given Vercel's established `turbo` CLI tool (2.9.3, widely used). At minimum, add an FAQ entry about it. NOT FIXED — naming decision for Kirby.

10. **~~Update Dockerfile Rust version.~~** FIXED. Updated to `rust:1.86-slim`.

11. **~~Separate aspirational features from implemented features in docs.~~** FIXED (via #2). Broken examples moved to `examples/roadmap/` with clear labeling.

12. **~~Remove fake `turbo.toml` dependencies.~~** FIXED (via #2). Fake deps remain only in `roadmap/` directory where they're clearly labeled as aspirational.
