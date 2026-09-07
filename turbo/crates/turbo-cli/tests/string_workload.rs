//! Exact output checks against the actual string benchmark in three modes.
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
fn utf8_token_workload_matches_in_jit_aot_and_rust() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../benchmarks");
    let source = root.join("bench_string_tokens.tb");
    let dir = tempfile::tempdir().unwrap();
    let native = dir.path().join("strings-turbo");
    let rust = dir.path().join("strings-rust");
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
            .arg(root.join("rust/string_tokens.rs"))
            .arg("-o")
            .arg(&rust),
    );
    let alternate = dir.path().join("alternate.txt");
    let text = " INFO: Ω |WARN: ß| INFO: 漢字|INFO: e\u{301}|WARN: 👩‍💻|\n||\n";
    std::fs::write(&alternate, text).unwrap();
    // Three visits: first record twice, second record once. Byte accounting
    // excludes record separators and preserves the emoji's ZWJ sequence.
    let first_bytes = text.split('\n').next().unwrap().len();
    let alternate_expected = format!(
        "info: e\u{301} 2\ninfo: Ω 2\ninfo: 漢字 2\nwarn: ß 2\nwarn: 👩‍💻 2\nTOTAL 10 5 {}\n",
        first_bytes * 2 + 2
    );
    for (corpus, steps, expected) in [
        (
            root.join("string_tokens_corpus.txt"),
            "1",
            "info: alpha 1\ninfo: 東京 1\nwarn: café 1\nTOTAL 3 3 42\n".to_owned(),
        ),
        (
            root.join("string_tokens_corpus.txt"),
            "8",
            std::fs::read_to_string(root.join("string_tokens_corpus.expected")).unwrap(),
        ),
        (alternate, "3", alternate_expected),
    ] {
        let mut jit = Command::new(compiler);
        jit.arg("run").arg(&source);
        for mut command in [jit, Command::new(&native), Command::new(&rust)] {
            let out = successful(
                command
                    .env("STRING_TOKENS_INPUT", &corpus)
                    .env("TURBO_BENCH_STEPS", steps),
            );
            assert_eq!(out.stdout, expected.as_bytes(), "{command:?}");
            assert!(out.stderr.is_empty(), "{command:?}");
        }
    }
    for steps in ["0", "16777217", "bad"] {
        let mut jit = Command::new(compiler);
        jit.arg("run").arg(&source);
        for mut command in [jit, Command::new(&native), Command::new(&rust)] {
            let out = command
                .env_remove("TURBO_ALLOC_PROFILE")
                .env("STRING_TOKENS_INPUT", root.join("string_tokens_corpus.txt"))
                .env("TURBO_BENCH_STEPS", steps)
                .output()
                .unwrap();
            assert!(!out.status.success(), "{command:?}");
            assert!(out.stdout.is_empty());
        }
    }
}
