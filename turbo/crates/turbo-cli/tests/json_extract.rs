use std::path::Path;
use std::process::Command;

fn extract(inputs: &[Vec<u8>]) -> [Vec<String>; 2] {
    let directory = tempfile::tempdir().unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/json_extract.tb");
    let binary = directory.path().join("extract");
    let compiler = env!("CARGO_BIN_EXE_turbolang");
    let built = Command::new(compiler)
        .arg("build")
        .arg(&source)
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let paths: Vec<_> = inputs
        .iter()
        .enumerate()
        .map(|(index, bytes)| {
            let path = directory.path().join(format!("case-{index}.json"));
            std::fs::write(&path, bytes).unwrap();
            path
        })
        .collect();
    let mut jit = Command::new(compiler);
    jit.arg("run").arg(&source).arg("--");
    [jit, Command::new(&binary)].map(|mut command| {
        command.args(&paths);
        if turbo_codegen_cranelift::ALLOCATION_PROFILE_BUILD {
            command.env("TURBO_ALLOC_PROFILE", "1");
        } else {
            command.env_remove("TURBO_ALLOC_PROFILE");
        }
        let out = command.output().unwrap();
        assert!(
            out.status.success(),
            "{command:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        if turbo_codegen_cranelift::ALLOCATION_PROFILE_BUILD {
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
        } else {
            assert!(
                out.stderr.is_empty(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        let result: Vec<String> = String::from_utf8(out.stdout)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(result.len(), inputs.len());
        result
    })
}

fn reference(input: &[u8]) -> String {
    let Some(value) = serde_json::from_slice::<serde_json::Value>(input)
        .ok()
        .and_then(|object| object.get("target").cloned())
    else {
        return String::new();
    };
    let text = match value {
        serde_json::Value::String(text) => text,
        other => other.to_string(),
    };
    if text.contains('\0') {
        String::new()
    } else {
        text
    }
}

#[test]
fn json_extract_decodes_strings_and_validates_complete_documents() {
    let controls: String = (1_u8..32).map(char::from).collect();
    let mut inputs = vec![serde_json::to_vec(&serde_json::json!({
        "nested": {"target": "wrong"},
        "target": format!("{controls} / \\\" café 東京 🙂 e\u{301}")
    }))
    .unwrap()];
    for input in [
        r#"{"ta\u0072get":"\u00e9\uD83D\uDE42\uDBFF\uDFFF"}"#,
        r#"{"target":"first","ta\u0072get":"last"}"#,
        r#"{"target":"\/\b\f\n\r\t\"\\"}"#,
        r#"{"target\u0000":"wrong","target":"right"}"#,
        r#"{"target":"prefix\u0000suffix"}"#,
        r#"{"nested":[{"target":"wrong"}],"target":"right"}"#,
        r#"{"target":null}"#,
        r#"{"target":true}"#,
        r#"{"target":false}"#,
        r#"{"target":-12}"#,
        r#"{"target":18446744073709551615}"#,
        r#"{"target":""}"#,
        r#"{"missing":"x"}"#,
        r#"[]"#,
        r#"{"target":"ok","broken":01}"#,
        r#"{"target":"ok","broken":1e9999}"#,
        r#"{"target":"ok","broken":}"#,
        r#"{"target":"ok",}"#,
        r#"{"target":"ok"} garbage"#,
        r#"{"target":"\q"}"#,
        r#"{"target":"\u123"}"#,
        r#"{"target":"\ud800"}"#,
        r#"{"target":"\udc00"}"#,
        r#"{"target":"\ud800\ud800"}"#,
        r#"{"target":"ok","ignored":"\ud800"}"#,
        "{\"target\":\"raw\u{1}control\"}",
        "{\"target\":\"raw\ttab\"}",
        "{\"target\":\"unterminated\\",
        "{\"target\":\"ok\",\"later\":{\"x\":[true,false,null]}}",
    ] {
        inputs.push(input.as_bytes().to_vec());
    }
    // Invalid UTF-8 bytes are exercised directly by the C runtime tests: the
    // JIT read_file API rejects such files before json_get can receive them.
    for depth in [120, 125, 126, 127, 128, 129] {
        inputs.push(
            format!(
                "{{\"target\":\"ok\",\"ignored\":{}0{}}}",
                "[".repeat(depth),
                "]".repeat(depth)
            )
            .into_bytes(),
        );
        inputs.push(
            format!(
                "{{\"target\":\"ok\",\"ignored\":{}{}}}",
                "[".repeat(depth),
                "]".repeat(depth)
            )
            .into_bytes(),
        );
    }
    for number in [
        format!("0.{}1", "0".repeat(200)),
        format!("1{}", "0".repeat(200)),
        format!("1e-{}2", "0".repeat(200)),
    ] {
        inputs.push(format!("{{\"target\":\"ok\",\"number\":{number}}}").into_bytes());
    }
    for number in [
        "1e309",
        "-1e309",
        "1.7976931348623157e308",
        "1.7976931348623159e308",
        "1e-9999",
        "0e9999",
        "-0e9999",
    ] {
        inputs.push(format!("{{\"target\":\"ok\",\"number\":{number}}}").into_bytes());
    }
    inputs.push(format!("{{\"target\":\"ok\",\"number\":1{}}}", "0".repeat(400)).into_bytes());
    inputs.push(
        format!(
            "{{\"target\":\"ok\",\"number\":0.{}1e600}}",
            "0".repeat(600)
        )
        .into_bytes(),
    );
    // Deterministic hostile corpus: incomplete prefixes and token mutations.
    // ASCII-only mutations keep the file-loader's UTF-8 contract separate.
    let seed = br#"{"target":"\u00e9","x":[true,false,null,12],"y":{"z":"\\\""}}"#;
    for cut in 0..seed.len() {
        inputs.push(seed[..cut].to_vec());
    }
    for index in (0..seed.len()).step_by(4) {
        for replacement in *b"\"\\},0" {
            let mut mutant = seed.to_vec();
            mutant[index] = replacement;
            inputs.push(mutant);
        }
    }
    let expected: Vec<_> = inputs.iter().map(|input| reference(input)).collect();
    for outputs in extract(&inputs) {
        for (index, (actual, expected)) in outputs.iter().zip(&expected).enumerate() {
            assert_eq!(
                actual,
                expected,
                "case {index}: {:?}",
                String::from_utf8_lossy(&inputs[index])
            );
        }
    }
}

#[test]
fn json_extract_returns_complete_structured_values() {
    let inputs = [
        br#"{"target": { "z": [1,true,null,"x"], "a": {"b":"\u00e9"} }}"#.to_vec(),
        br#"{"target": [ {"x":2}, [3,4], "comma, quote\"", "\ud83d\ude42" ]}"#.to_vec(),
    ];
    // Nested containers are compared as JSON values, not canonical text. AOT
    // preserves raw spelling; matching serde's formatting is separate work.
    for outputs in extract(&inputs) {
        for (input, output) in inputs.iter().zip(outputs) {
            let expected: serde_json::Value = serde_json::from_slice(input).unwrap();
            let actual: serde_json::Value = serde_json::from_str(&output).unwrap();
            assert_eq!(actual, expected["target"]);
        }
    }
}
