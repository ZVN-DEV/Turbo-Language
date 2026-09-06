use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn compiler() -> Command {
    Command::new(env!("CARGO_BIN_EXE_turbolang"))
}
fn source() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/phase1/allocation_profile_probe.tb")
}
fn invoke(mut command: Command) -> Output {
    let out = command.env("TURBO_ALLOC_PROFILE", "1").output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.stdout, b"profile-probe\n");
    out
}

#[test]
fn allocation_profile_is_explicit_and_matches_native_modes() {
    let dir = tempfile::tempdir().unwrap();
    let binary = dir.path().join("probe.exe");
    let built = compiler()
        .arg("build")
        .arg(source())
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let mut jit = compiler();
    jit.arg("run").arg(source());
    let outputs = [invoke(jit), invoke(Command::new(&binary))];
    for output in &outputs {
        if turbo_codegen_cranelift::ALLOCATION_PROFILE_BUILD {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let encoded = stderr
                .strip_prefix("TURBO_ALLOC_PROFILE ")
                .expect("profile record missing");
            let p: serde_json::Value = serde_json::from_str(encoded.trim()).unwrap();
            assert_eq!(p["coverage"], "shared_header_arc");
            assert_eq!(p["valid"], true);
            assert_eq!(p["allocations"], 1);
            assert_eq!(p["heap_frees"], 1);
            assert_eq!(p["total_data_bytes"], 32);
            assert_eq!(p["total_header_bytes"], 16);
            assert_eq!(p["peak_live_data_bytes"], 32);
            assert_eq!(p["live_allocations"], 0);
            assert_eq!(p["live_data_bytes"], 0);
            assert_eq!(p["retain_calls"], 1);
            assert_eq!(p["retain_ops"], 1);
            assert_eq!(p["release_calls"], 2);
            assert_eq!(p["release_ops"], 2);
        } else {
            assert!(
                output.stderr.is_empty(),
                "normal build must not include profiling hooks"
            );
        }
    }
    assert_eq!(outputs[0].stderr, outputs[1].stderr);
    let without_request = Command::new(binary)
        .env_remove("TURBO_ALLOC_PROFILE")
        .output()
        .unwrap();
    assert!(without_request.status.success());
    assert!(without_request.stderr.is_empty());
    let version = compiler().arg("--version").output().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&version.stdout).contains("+allocation-profile"),
        turbo_codegen_cranelift::ALLOCATION_PROFILE_BUILD
    );
}
