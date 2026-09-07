use std::path::{Path, PathBuf};
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

fn assert_zero_live_profile(stderr: &[u8]) {
    let stderr = String::from_utf8_lossy(stderr);
    let record = stderr
        .strip_prefix("TURBO_ALLOC_PROFILE ")
        .expect("profile missing");
    let p: serde_json::Value = serde_json::from_str(record.trim()).unwrap();
    assert_eq!(p["valid"], true, "{p}");
    assert_eq!(p["errors"], 0);
    assert_eq!(p["unknown_frees"], 0);
    assert_eq!(p["live_allocations"], 0, "{p}");
    assert_eq!(p["live_data_bytes"], 0);
    assert_eq!(p["allocations"], p["heap_frees"]);
}

fn build_rust_example(workspace: &Path, example_source: &Path) -> PathBuf {
    let out = successful(
        Command::new("cargo")
            .args([
                "build",
                "--locked",
                "--offline",
                "--message-format=json",
                "-p",
                "turbo-cli",
                "--example",
                "bench_json_transform",
            ])
            .arg("--manifest-path")
            .arg(workspace.join("Cargo.toml"))
            .arg("--target-dir")
            .arg(workspace.join("target")),
    );
    let expected_source = std::fs::canonicalize(example_source).unwrap();
    let mut executables = Vec::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let Ok(message) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if message["reason"] != "compiler-artifact" {
            continue;
        }
        let target = &message["target"];
        let is_example = target["kind"]
            .as_array()
            .is_some_and(|kinds| kinds.iter().any(|kind| kind == "example"));
        if !is_example {
            continue;
        }
        let Some(src_path) = target["src_path"].as_str() else {
            continue;
        };
        if std::fs::canonicalize(src_path).ok().as_ref() != Some(&expected_source) {
            continue;
        }
        let Some(executable) = message["executable"].as_str() else {
            continue;
        };
        executables.push(PathBuf::from(executable));
    }
    assert_eq!(
        executables.len(),
        1,
        "expected exactly one executable compiler artifact for {}",
        example_source.display()
    );
    executables.remove(0)
}

#[test]
fn json_transform_workload_matches_full_records_in_native_modes() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let source = workspace.join("benchmarks/bench_json_transform.tb");
    let directory = tempfile::tempdir().unwrap();
    let native = directory.path().join("json-turbo");
    let compiler = env!("CARGO_BIN_EXE_turbolang");
    successful(
        Command::new(compiler)
            .arg("build")
            .arg(&source)
            .arg("-o")
            .arg(&native),
    );
    let rust = build_rust_example(
        &workspace,
        &workspace.join("crates/turbo-cli/examples/bench_json_transform.rs"),
    );
    let input = directory.path().join("records.ndjson");
    let rows = [
        serde_json::json!({"meta":{"id":999},"id":1,"score":2,"active":true,"title":"a\tb"}),
        serde_json::json!({"id":2,"score":-3,"active":false,"title":"skip"}),
        serde_json::json!({"id":5,"score":4,"active":true,"title":"skip"}),
        serde_json::json!({"id":-2,"score":10,"active":true,"title":"é🙂\u{1f}\u{2028}"}),
    ];
    let data: String = rows.iter().map(|row| row.to_string() + "\n").collect();
    std::fs::write(&input, data).unwrap();
    let a = "{\"id\":1,\"score\":7,\"title\":\"task:a\\tb\"}";
    let b = "{\"id\":-2,\"score\":28,\"title\":\"task:é🙂\\u001f\u{2028}\"}";
    for rounds in [1, 3] {
        let expected = format!(
            "{b}\t{rounds}\n{a}\t{rounds}\nTOTAL {} {} {}\n",
            2 * rounds,
            (a.len() + b.len()) * rounds,
            4 * rounds
        );
        let mut jit = Command::new(compiler);
        jit.arg("run").arg(&source);
        for (mode, mut command) in [
            ("jit", jit),
            ("aot", Command::new(&native)),
            ("rust", Command::new(&rust)),
        ] {
            command
                .env("JSON_TRANSFORM_INPUT", &input)
                .env("TURBO_BENCH_SIZE", "4")
                .env("TURBO_BENCH_STEPS", rounds.to_string());
            if turbo_codegen_cranelift::ALLOCATION_PROFILE_BUILD && mode != "rust" {
                command.env("TURBO_ALLOC_PROFILE", "1");
            } else {
                command.env_remove("TURBO_ALLOC_PROFILE");
            }
            let out = successful(&mut command);
            assert_eq!(out.stdout, expected.as_bytes(), "{mode}/{rounds}");
            if turbo_codegen_cranelift::ALLOCATION_PROFILE_BUILD && mode != "rust" {
                assert_zero_live_profile(&out.stderr);
            } else {
                assert!(out.stderr.is_empty());
            }
        }
    }
    let inline_source = directory.path().join("serializer_edges.tb");
    let inline_native = directory.path().join("serializer-edges");
    std::fs::write(
        &inline_source,
        r#"struct Projected { id: i64, weight: f64, active: bool, title: str }

fn main() {
    print(to_json(Projected { id: 1, weight: 1.5, active: true, title: "task:a" }))
    print(to_json_array([
        Projected { id: 1, weight: 1.5, active: true, title: "task:a" },
        Projected { id: 2, weight: 2.5, active: false, title: "task:b" },
    ]))
    let empty: [Projected] = []
    print(to_json_array(empty))
}
"#,
    )
    .unwrap();
    successful(
        Command::new(compiler)
            .arg("build")
            .arg(&inline_source)
            .arg("-o")
            .arg(&inline_native),
    );
    let inline_expected = b"{\"id\":1,\"weight\":1.5,\"active\":true,\"title\":\"task:a\"}\n[{\"id\":1,\"weight\":1.5,\"active\":true,\"title\":\"task:a\"},{\"id\":2,\"weight\":2.5,\"active\":false,\"title\":\"task:b\"}]\n[]\n";
    for (mode, mut command) in [
        ("jit", {
            let mut command = Command::new(compiler);
            command.arg("run").arg(&inline_source);
            command
        }),
        ("aot", Command::new(&inline_native)),
    ] {
        if turbo_codegen_cranelift::ALLOCATION_PROFILE_BUILD {
            command.env("TURBO_ALLOC_PROFILE", "1");
        }
        let out = successful(&mut command);
        assert_eq!(out.stdout, inline_expected, "{mode}/inline");
        if turbo_codegen_cranelift::ALLOCATION_PROFILE_BUILD {
            assert_zero_live_profile(&out.stderr);
        } else {
            assert!(out.stderr.is_empty());
        }
    }
    for (size, rounds) in [("0", "1"), ("4097", "1"), ("4", "513"), ("5", "1")] {
        let mut jit = Command::new(compiler);
        jit.arg("run").arg(&source);
        for mut command in [jit, Command::new(&native), Command::new(&rust)] {
            let out = command
                .env("JSON_TRANSFORM_INPUT", &input)
                .env("TURBO_BENCH_SIZE", size)
                .env("TURBO_BENCH_STEPS", rounds)
                .env_remove("TURBO_ALLOC_PROFILE")
                .output()
                .unwrap();
            assert!(!out.status.success());
            assert!(out.stdout.is_empty());
        }
    }
}
