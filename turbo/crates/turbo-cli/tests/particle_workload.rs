//! Run the real evaluator fixtures, not reduced copies of their implementation.
use std::path::Path;
use std::process::{Command, Output};

fn successful(command: &mut Command) -> Output {
    let out = command.env_remove("TURBO_ALLOC_PROFILE").output().unwrap();
    assert!(
        out.status.success(),
        "{command:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

#[test]
fn particle_trajectories_match_in_jit_aot_and_rust() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../benchmarks");
    let source = root.join("bench_particle_update.tb");
    let dir = tempfile::tempdir().unwrap();
    let native = dir.path().join("particles-turbo");
    let rust = dir.path().join("particles-rust");
    let compiler = env!("CARGO_BIN_EXE_turbolang");
    successful(
        Command::new(compiler)
            .arg("build")
            .arg(&source)
            .arg("-o")
            .arg(&native),
    );
    successful(
        Command::new("rustc")
            .args(["-O", "-D", "warnings"])
            .arg(root.join("rust/particle_update.rs"))
            .arg("-o")
            .arg(&rust),
    );
    // Frozen oracle values cover negative coordinates, multiple particles and
    // the largest permitted step count. Python separately proves the oracle
    // against an integer-lattice simulation, not this native implementation.
    for (size, steps, digest) in [
        (1, 1, 92189340),
        (3, 7, 484233806),
        (17, 64, 707923829),
        (257, 513, 562550284),
        (1, 65536, 149579324),
    ] {
        let expected = format!("{digest}\n{size}\n{steps}\n");
        let mut jit = Command::new(compiler);
        jit.arg("run").arg(&source);
        for mut command in [jit, Command::new(&native), Command::new(&rust)] {
            let out = successful(
                command
                    .env("TURBO_BENCH_SIZE", size.to_string())
                    .env("TURBO_BENCH_STEPS", steps.to_string()),
            );
            assert_eq!(out.stdout, expected.as_bytes(), "{command:?}");
            assert!(out.stderr.is_empty(), "{command:?}");
        }
    }
    for (size, steps) in [("0", "1"), ("10001", "1"), ("1", "65537"), ("1", "bad")] {
        let mut jit = Command::new(compiler);
        jit.arg("run").arg(&source);
        for mut command in [jit, Command::new(&native), Command::new(&rust)] {
            let out = command
                .env_remove("TURBO_ALLOC_PROFILE")
                .env("TURBO_BENCH_SIZE", size)
                .env("TURBO_BENCH_STEPS", steps)
                .output()
                .unwrap();
            assert!(!out.status.success(), "{command:?}");
            assert!(out.stdout.is_empty(), "invalid input emitted a digest");
        }
    }
}
