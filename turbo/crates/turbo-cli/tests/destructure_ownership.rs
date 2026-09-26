use std::path::Path;
use std::process::Command;

fn profile_json(stderr: &[u8]) -> Option<serde_json::Value> {
    if !turbo_codegen_cranelift::ALLOCATION_PROFILE_BUILD {
        assert!(stderr.is_empty());
        return None;
    }
    let stderr = String::from_utf8_lossy(stderr);
    let record = stderr
        .strip_prefix("TURBO_ALLOC_PROFILE ")
        .expect("profile record missing");
    Some(serde_json::from_str(record.trim()).unwrap())
}

#[test]
fn destructured_managed_fields_reclaim_in_native_modes() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/phase1");
    let dir = tempfile::tempdir().unwrap();
    let compiler = env!("CARGO_BIN_EXE_turbolang");

    for name in [
        "destructure_call_argument_metadata",
        "destructure_field_types",
        "destructure_managed_alias",
        "destructure_managed_escape",
        "destructure_reordered_literal_release",
        "destructure_subset_generic_owned",
    ] {
        let source = root.join(format!("{name}.tb"));
        let expected = std::fs::read(root.join(format!("{name}.expected"))).unwrap();
        let native = dir.path().join(format!("{name}.exe"));
        let built = Command::new(compiler)
            .arg("build")
            .arg(&source)
            .arg("-o")
            .arg(&native)
            .output()
            .unwrap();
        assert!(
            built.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&built.stderr)
        );

        let mut jit = Command::new(compiler);
        jit.arg("run").arg(&source);

        for mut command in [jit, Command::new(&native)] {
            let out = command.env("TURBO_ALLOC_PROFILE", "1").output().unwrap();
            assert!(
                out.status.success(),
                "{name}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert_eq!(out.stdout, expected, "{name}");
            if let Some(p) = profile_json(&out.stderr) {
                assert_eq!(p["valid"], true, "{name}: {p}");
                assert_eq!(p["errors"], 0, "{name}: {p}");
                assert_eq!(p["unknown_frees"], 0, "{name}: {p}");
                assert_eq!(p["live_allocations"], 0, "{name}: {p}");
                assert_eq!(p["live_data_bytes"], 0, "{name}: {p}");
                assert_eq!(p["allocations"], p["heap_frees"], "{name}: {p}");
                assert!(p["allocations"].as_u64().unwrap() > 0, "{name}: {p}");
            }
        }
    }
}
