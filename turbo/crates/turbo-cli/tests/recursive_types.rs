use std::path::Path;
use std::process::Command;

#[test]
fn indirect_calls_hold_arguments_in_native_modes() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/phase1");
    let source = root.join("indirect_argument_lifetime.tb");
    let expected = std::fs::read(root.join("indirect_argument_lifetime.expected")).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let native = dir.path().join("indirect");
    let compiler = env!("CARGO_BIN_EXE_turbolang");
    let built = Command::new(compiler)
        .arg("build")
        .arg(&source)
        .arg("-o")
        .arg(&native)
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let mut jit = Command::new(compiler);
    jit.arg("run").arg(&source);
    for mut command in [jit, Command::new(&native)] {
        // Fn/closure environment reclamation is a separate outstanding runtime
        // capability; this test proves argument safety, not whole-heap balance.
        let out = command.env_remove("TURBO_ALLOC_PROFILE").output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(out.stdout, expected);
        assert!(out.stderr.is_empty());
    }
}

#[test]
fn recursive_types_run_and_reclaim_in_native_modes() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/phase1");
    let dir = tempfile::tempdir().unwrap();
    let compiler = env!("CARGO_BIN_EXE_turbolang");
    for name in [
        "recursive_struct_drop",
        "recursive_enum_drop",
        "mutual_recursive_drop",
        "borrowed_call_balance",
        "coalesce_call_balance",
        "call_argument_lifetime",
    ] {
        let source = root.join(format!("{name}.tb"));
        let expected = std::fs::read(root.join(format!("{name}.expected"))).unwrap();
        let native = dir.path().join(name);
        let build = Command::new(compiler)
            .arg("build")
            .arg(&source)
            .arg("-o")
            .arg(&native)
            .output()
            .unwrap();
        assert!(
            build.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&build.stderr)
        );
        let mut jit = Command::new(compiler);
        jit.arg("run").arg(&source);
        let mut profiles = Vec::new();
        for mut command in [jit, Command::new(&native)] {
            let out = command.env("TURBO_ALLOC_PROFILE", "1").output().unwrap();
            assert!(
                out.status.success(),
                "{name}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert_eq!(out.stdout, expected, "{name}");
            if turbo_codegen_cranelift::ALLOCATION_PROFILE_BUILD {
                let stderr = String::from_utf8_lossy(&out.stderr);
                let record = stderr
                    .strip_prefix("TURBO_ALLOC_PROFILE ")
                    .expect("profile missing");
                let p: serde_json::Value = serde_json::from_str(record.trim()).unwrap();
                assert_eq!(p["valid"], true, "{name}: {p}");
                assert_eq!(p["errors"], 0, "{name}: {p}");
                assert_eq!(p["unknown_frees"], 0, "{name}: {p}");
                assert_eq!(p["live_allocations"], 0, "{name}: {p}");
                assert_eq!(p["live_data_bytes"], 0, "{name}: {p}");
                assert_eq!(p["allocations"], p["heap_frees"], "{name}: {p}");
                assert!(p["allocations"].as_u64().unwrap() > 0, "{name}: {p}");
                profiles.push(p);
            } else {
                assert!(out.stderr.is_empty(), "{name}");
            }
        }
        if profiles.len() == 2 {
            assert_eq!(profiles[0], profiles[1], "{name}");
        }
    }
}
