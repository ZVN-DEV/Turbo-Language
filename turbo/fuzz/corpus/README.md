# codegen-fuzz corpus

Repro material for the `codegen-fuzz` harness (`turbo/fuzz/src/codegen_fuzz.rs`).

## Layout

- `seed/` — a small, checked-in set of generator-shaped programs. These
  mirror the four `gen_exec_program` templates (arithmetic, bounded `while`,
  array fold, bounded recursion). They are guaranteed-terminating and produce
  deterministic output, so JIT and AOT must agree byte-for-byte. Handy as a
  quick sanity run and as documentation of what the execute/diff modes feed
  the compiler.

- `findings/` — runtime-written repro cases. On any crash, JIT/AOT stdout
  divergence, or hang, the harness drops the offending program (`<kind>_seed<N>.tb`)
  plus a `.txt` detail report here. Contents are git-ignored (only `.gitkeep`
  is tracked) — a finding is a bug to file, not something to commit.

## Reproducing a finding

Every finding is fully determined by its seed. The filename encodes it
(`diff_seed42.tb` → seed 42). To re-run just that program:

```bash
cargo build --release --manifest-path turbo/Cargo.toml            # build turbolang
turbolang run   turbo/fuzz/corpus/findings/diff_seed42.tb          # JIT
turbolang build turbo/fuzz/corpus/findings/diff_seed42.tb -o /tmp/x && /tmp/x   # AOT
```

## Running the modes

```bash
# compile-only smoke (CI-gating; unchanged legacy contract)
cargo run --release --manifest-path turbo/fuzz/Cargo.toml --bin codegen-fuzz -- 200

# execute generated programs via `turbolang run` (jit_run)
cargo run --release --manifest-path turbo/fuzz/Cargo.toml --bin codegen-fuzz -- --execute 50

# differential JIT vs AOT stdout comparison
cargo run --release --manifest-path turbo/fuzz/Cargo.toml --bin codegen-fuzz -- --diff 20
```

Execute/diff modes need the `turbolang` binary — found via `$TURBOLANG_BIN`
or `turbo/target/{release,debug}/turbolang`. Per-run timeout:
`TURBO_FUZZ_EXEC_TIMEOUT_MS` (default 5000).
