# Sprint Plan — Turbo 0.8.0 "The Safe Core"
Generated: 2026-04-15
Based on: product review findings (conversation), v0.7.7 baseline

## Sprint Goal
Close the correctness, memory-safety, and stdlib gaps that block "use Turbo in production" messaging. No new features — only safety and completeness of existing surface. After this sprint, `read_file` can fail gracefully, hashmaps support int values, the runtime has no shell-injection path, and integer overflow is at least detectable.

## Success Criteria
- [ ] `rt_exec` shell injection path removed or gated behind argv-array API (no `/bin/sh -c`)
- [ ] `rt_pow` and core arithmetic helpers checked for overflow (trap via `rt_int_overflow` infra)
- [ ] `read_file` / `write_file` return `Result<str, str>`; existing tests updated
- [ ] Hashmap supports `int` values in addition to `str` values
- [ ] `read_fd_to_string` realloc corruption fixed for responses >8KB
- [ ] AOT linker: `-l<lib>` arguments validated against allowlist regex `^[A-Za-z0-9_.+-]+$`
- [ ] `./tests/run_tests.sh` passes (ideally 185/185, minimum no new failures)
- [ ] Release build succeeds (`cargo build --release`)

## Dev Tracks

### Track 1: Runtime C Hardening — turbo_rt.c safety
**Agent:** C / runtime safety specialist
**Files touched (exclusive):**
- `turbo/crates/turbo-codegen-cranelift/runtime/turbo_rt.c` lines 811–1073 ONLY (I/O + exec + arithmetic zone)
**Tasks:**
- **TASK-1A (P0): Patch `rt_exec` shell injection** at `turbo_rt.c:1026–1073`.
  The current `execlp("/bin/sh","sh","-c",cmd,...)` is RCE with any user-controlled input.
  Fix: Replace with argv-vector execution: tokenize `cmd` on whitespace (no shell metacharacters), reject if any of `; | & $ \` ( ) < > \n` appear in the string, then `execvp(argv[0], argv)`. Preserve the pipe+capture semantics. Keep the exported symbol name `rt_exec` and the `const char *rt_exec(const char *cmd)` signature unchanged.
- **TASK-1B (P0): Checked `rt_pow`** at `turbo_rt.c:842–847`.
  The current loop `result *= base` wraps silently. Fix: use `__builtin_mul_overflow` on the per-iteration multiply; on overflow call the existing `rt_int_overflow` helper (search the file — it's already exported). Keep signature `long long rt_pow(long long base, long long exp)`.
- **TASK-1C (P0): Fix `read_fd_to_string` realloc corruption** at `turbo_rt.c:904–918`.
  When inside an arena the `turbo_realloc` only copies `size/2` bytes on grow (see comment block at lines 180–194). Fix: track `len` (not `cap/2`) and pass `len` as the copy size when replacing the arena pointer. Simplest fix: stop using `turbo_realloc` here — allocate a fresh buffer via `malloc`/`realloc` directly and, once complete, copy into arena memory in a single shot. Document inline. Ensure HTTP responses ≥16KB round-trip correctly.
- **TASK-1D (P1): Quick adversarial test** — add `turbo/tests/adversarial/exec_injection.tb` and `.expected` that invokes `shell_exec("echo hi; echo pwned")` — expected output should be a rejection or escaped literal, NOT `hi\npwned`. (If you have to pick one `.expected`: make it the rejection path — print an error string via `fprintf(stderr, ...)` and return `""` from rt_exec on rejection.)

**Files you must NOT edit (other agents own):**
- `turbo_rt.c` lines OUTSIDE 811–1073 (Track 2 owns hashmap 1360–1500, Track 3 owns nothing in this file — you have full I/O zone)
- `src/aot.rs` (Track 4)
- Any `.rs` file under `src/` — do not touch compiler code.

**Commit message:** `Track 1: Runtime C hardening — remove shell exec path, check rt_pow, fix read_fd_to_string realloc`

---

### Track 2: Generic Hashmap (int values) — runtime + compiler
**Agent:** Compiler + runtime specialist
**Files touched (exclusive):**
- `turbo/crates/turbo-codegen-cranelift/runtime/turbo_rt.c` lines **1360–1500 ONLY** (hashmap section)
- `turbo/crates/turbo-codegen-cranelift/src/builtins.rs` (add new entries; do not reorder existing)
- `turbo/crates/turbo-codegen-cranelift/src/expr.rs` (add new builtin dispatch; do not reorder)
- `turbo/crates/turbo-codegen-cranelift/src/lib.rs` (declare_rt_fn for new symbols)
- `turbo/crates/turbo-codegen-cranelift/src/jit.rs` (symbol table entries)
- `turbo/crates/turbo-sema/src/lib.rs` (add builtin signatures)
- `docs/stdlib.md` (document new variants)

**Tasks:**
- **TASK-2A (P1): Add int-valued hashmap variants.** New runtime functions in `turbo_rt.c` (append at end of hashmap section before line 1500):
  - `void rt_hashmap_set_int(void *map_ptr, const char *key, long long value)`
  - `long long rt_hashmap_get_int(const void *map_ptr, const char *key)` — returns 0 on miss (document this)
  Store ints inline in the existing hashmap value field by stringifying with a tag byte — OR extend the internal value union to carry either `char*` or `int64`. Pick the simpler approach that doesn't break existing `rt_hashmap_set`/`get`/`has`/`keys`/`remove`.
- **TASK-2B (P1): Wire into compiler** — builtin dispatch for `hashmap_set_int`, `hashmap_get_int` in `expr.rs`; JIT symbol registration in `jit.rs` at BOTH call sites (lines ~126 and ~274); `declare_rt_fn` in `lib.rs`; sema signatures in `turbo-sema/src/lib.rs`.
- **TASK-2C (P1): Test** — `turbo/tests/phase1/hashmap_int.tb` + `.expected` doing a word-frequency counter:
  ```
  fn main() {
    let words = ["a", "b", "a", "c", "a", "b"]
    let mut m = hashmap()
    let mut i = 0
    while i < len(words) {
      let w = words[i]
      if hashmap_has(m, w) {
        m = hashmap_set_int(m, w, hashmap_get_int(m, w) + 1)
      } else {
        m = hashmap_set_int(m, w, 1)
      }
      i = i + 1
    }
    print(hashmap_get_int(m, "a"))
    print(hashmap_get_int(m, "b"))
    print(hashmap_get_int(m, "c"))
  }
  ```
  Expected: `3\n2\n1\n`.
- **TASK-2D (P2): Update `docs/stdlib.md`** in the HashMap section — document both variants, clarify the `str→str` vs `str→int` distinction honestly.

**Files you must NOT edit (other agents own):**
- `turbo_rt.c` lines OUTSIDE 1360–1500 (Track 1 owns 811–1073; ALL other sections are off-limits)
- `src/aot.rs` (Track 4)
- The I/O builtins (`read_file`, `write_file`) — Track 3 owns those.

**Commit message:** `Track 2: Generic hashmap — add int-valued variants (str→int)`

---

### Track 3: Result-Returning I/O
**Agent:** Compiler + stdlib specialist
**Files touched (exclusive):**
- `turbo/crates/turbo-codegen-cranelift/runtime/turbo_rt.c` — ADD NEW FUNCTIONS ONLY at end of file (after line 2050). DO NOT modify any existing function in this file.
- `turbo/crates/turbo-codegen-cranelift/src/builtins.rs`
- `turbo/crates/turbo-codegen-cranelift/src/expr.rs`
- `turbo/crates/turbo-codegen-cranelift/src/lib.rs`
- `turbo/crates/turbo-codegen-cranelift/src/jit.rs`
- `turbo/crates/turbo-sema/src/lib.rs`
- `docs/stdlib.md` (I/O section)

**Tasks:**
- **TASK-3A (P0): New runtime helpers** — append to the end of `turbo_rt.c`:
  - `const char *rt_read_file_checked(const char *path, char **err_out)` — returns file contents on success (err_out set to NULL), or NULL on failure (err_out set to an allocated error message string). Use `rt_str_dup` or equivalent arena-aware allocation already available in the file.
  - `long long rt_write_file_checked(const char *path, const char *content, char **err_out)` — returns 0 on success, -1 on failure with err_out set.
- **TASK-3B (P0): New language-level builtins** that return Turbo `Result<str, str>`:
  - `try_read_file(path: str) -> str ! str`
  - `try_write_file(path: str, content: str) -> bool ! str` (or `unit ! str` — match existing Result builtin patterns in the codebase)
  Leave the existing `read_file` and `write_file` alone — do NOT change their signatures (backward compat). Callers that want to recover from errors use `try_*`.
  Wire through `expr.rs` dispatch, JIT symbol table (both sites), `declare_rt_fn` in `lib.rs`, and builtin signatures in `turbo-sema/src/lib.rs`.
- **TASK-3C (P0): Test** — `turbo/tests/phase1/try_read_file.tb` + `.expected`:
  ```
  fn main() {
    let r = try_read_file("/tmp/nonexistent-turbo-xyz-12345.txt")
    match r {
      Ok(s) => print("ok: {s}"),
      Err(e) => print("err"),
    }
  }
  ```
  Expected: `err`.
- **TASK-3D (P1): Docs** — add a "Fallible I/O" subsection to `docs/stdlib.md` documenting both APIs and when to use each.

**Files you must NOT edit (other agents own):**
- `turbo_rt.c` lines 1–2050 (ANY existing function — append only)
- `src/aot.rs` (Track 4)

**Commit message:** `Track 3: Result-returning I/O — add try_read_file / try_write_file builtins`

---

### Track 4: AOT Linker Allowlist
**Agent:** Compiler build specialist
**Files touched (exclusive):**
- `turbo/crates/turbo-codegen-cranelift/src/aot.rs`

**Tasks:**
- **TASK-4A (P0): Linker-flag injection fix** at `aot.rs:110–112`.
  Current code: `for lib in link_libs { cmd.arg(format!("-l{}", lib)); }`.
  Risk: `--link "m -o /etc/passwd"` or `--link "@/tmp/attacker.rsp"` injects linker flags.
  Fix: before the loop, validate every `lib` against the regex `^[A-Za-z0-9_.+-]+$`. On any mismatch, return `CodegenError { code: ErrorCode::E0XXX, message: format!("invalid library name '{lib}' in --link; must match [A-Za-z0-9_.+-]+") }`. Pick an unused error code (check `turbo-ast/src/errors.rs` first). If adding a new code, also add the docs entry per `CONTRIBUTING.md` — `docs/errors/EXXXX.md` AND the symlinked source-of-truth at `turbo/crates/turbo-cli/src/errors/EXXXX.md`. Follow the checklist precisely or the build script will fail.
- **TASK-4B (P1): Same allowlist for any other user-supplied linker arg you find** in `aot.rs`. Grep for `.arg(format!` in aot.rs — validate every `{user_value}` formatted into a flag. (Lines ~341, 387 look like build-internal values, not user-supplied — leave those unless you confirm otherwise.)
- **TASK-4C (P2): Unit test** in `aot.rs` `#[cfg(test)]` block verifying rejection of `"m -o /tmp/x"`, `"@/etc/passwd"`, `"../../lib"`, and acceptance of `"m"`, `"ssl"`, `"c++"`, `"z_.+-"`.

**Files you must NOT edit (other agents own):**
- `turbo_rt.c` (Tracks 1, 2, 3)
- Any other `.rs` file beyond `aot.rs` (avoid merge conflicts)
- `turbo-sema/src/lib.rs`, `src/builtins.rs`, `src/expr.rs`, `src/jit.rs`, `src/lib.rs` — all reserved for Tracks 2 and 3

**Commit message:** `Track 4: AOT linker-flag allowlist — reject injection in --link values`

---

## Intentionally Deferred
- **Full generic hashmap** `HashMap<K,V>` with arbitrary types — would require generics + monomorphization work in codegen; deferred to v0.9.0. This sprint ships the pragmatic `str→int` add-on only.
- **Checked arithmetic everywhere** — only `rt_pow` this sprint; comprehensive checked-arith for `+ - * /` in codegen is a bigger change (debug/release split). Deferred.
- **String interpolation sigil flip** — ergonomic change that breaks every existing program; needs a deprecation cycle, wrong sprint.
- **Module system, Windows, browser WASM, regex** — out of scope.

## Manual Actions Required After Sprint
- Bump version to `0.8.0-dev` in `turbo/Cargo.toml` (and ecosystem repos) once this lands and is reviewed.
- Update CHANGELOG.md with the four tracks.
- No credential rotation needed.
