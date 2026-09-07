use std::path::Path;
use std::process::{Command, Output};

fn successful(command: &mut Command) -> Output {
    let out = command.output().unwrap();
    assert!(
        out.status.success(),
        "{command:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

#[test]
fn recursive_tree_workload_matches_and_reclaims_between_rounds() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../benchmarks");
    let source = root.join("bench_tree_walk.tb");
    let dir = tempfile::tempdir().unwrap();
    let native = dir.path().join("tree-turbo");
    let rust = dir.path().join("tree-rust");
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
            .arg(root.join("rust/tree_walk.rs"))
            .arg("-o")
            .arg(&rust),
    );
    let mut one_round_peak = None;
    for (depth, rounds, checksum) in [
        (1, 1, 66947),
        (2, 3, 904268154),
        (6, 1, 89656641),
        (6, 4, 114150249),
        (8, 2, 15385824),
    ] {
        let nodes = ((1_u64 << (depth + 1)) - 1) * rounds;
        let expected = format!("{checksum}\n{nodes}\n{rounds}\n");
        let mut jit = Command::new(compiler);
        jit.arg("run").arg(&source);
        let mut profiles = Vec::new();
        for (mode, mut command) in [
            ("jit", jit),
            ("aot", Command::new(&native)),
            ("rust", Command::new(&rust)),
        ] {
            let out = successful(
                command
                    .env("TURBO_BENCH_SIZE", depth.to_string())
                    .env("TURBO_BENCH_STEPS", rounds.to_string())
                    .env("TURBO_ALLOC_PROFILE", "1"),
            );
            assert_eq!(out.stdout, expected.as_bytes(), "{mode}/{depth}/{rounds}");
            if turbo_codegen_cranelift::ALLOCATION_PROFILE_BUILD && mode != "rust" {
                let stderr = String::from_utf8_lossy(&out.stderr);
                let encoded = stderr
                    .strip_prefix("TURBO_ALLOC_PROFILE ")
                    .expect("profile missing");
                let p: serde_json::Value = serde_json::from_str(encoded.trim()).unwrap();
                assert_eq!(p["valid"], true, "{p}");
                assert_eq!(p["errors"], 0);
                assert_eq!(p["unknown_frees"], 0);
                assert_eq!(p["live_allocations"], 0, "{p}");
                assert_eq!(p["live_data_bytes"], 0);
                assert_eq!(p["allocations"], p["heap_frees"]);
                assert!(p["allocations"].as_u64().unwrap() >= nodes);
                profiles.push(p);
            } else {
                assert!(out.stderr.is_empty());
            }
        }
        if !profiles.is_empty() {
            assert_eq!(profiles[0], profiles[1]);
            let peak = profiles[0]["peak_live_allocations"].as_u64().unwrap();
            if depth == 6 && rounds == 1 {
                one_round_peak = Some(peak);
            }
            if depth == 6 && rounds == 4 {
                assert_eq!(Some(peak), one_round_peak);
            }
        }
    }
    for (depth, rounds) in [
        ("0", "1"),
        ("21", "1"),
        ("1", "0"),
        ("1", "65"),
        ("bad", "1"),
    ] {
        let mut jit = Command::new(compiler);
        jit.arg("run").arg(&source);
        for mut command in [jit, Command::new(&native), Command::new(&rust)] {
            let out = command
                .env("TURBO_BENCH_SIZE", depth)
                .env("TURBO_BENCH_STEPS", rounds)
                .env_remove("TURBO_ALLOC_PROFILE")
                .output()
                .unwrap();
            assert!(!out.status.success());
            assert!(out.stdout.is_empty());
        }
    }
}
