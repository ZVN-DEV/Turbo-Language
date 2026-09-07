//! Runtime reference using the CLI crate's existing serde_json dependency.
use std::collections::HashMap;

fn parameter(name: &str, fallback: usize) -> usize {
    std::env::var(name)
        .ok()
        .filter(|s| !s.is_empty())
        .map(|s| s.parse().unwrap())
        .unwrap_or(fallback)
}

fn main() {
    let size = parameter("TURBO_BENCH_SIZE", 2048);
    let rounds = parameter("TURBO_BENCH_STEPS", 256);
    assert!(size > 0 && size <= 4096);
    assert!(rounds > 0 && rounds <= 512);
    let path = std::env::var("JSON_TRANSFORM_INPUT")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "json_transform_input.ndjson".into());
    let input = std::fs::read_to_string(path).unwrap();
    assert!(input.ends_with('\n'));
    let lines: Vec<_> = input.split_terminator('\n').collect();
    assert_eq!(lines.len(), size);
    let mut counts = HashMap::<String, u64>::new();
    let mut bytes = 0_usize;
    for _ in 0..rounds {
        for line in &lines {
            // Parse once per record: do not pessimize the idiomatic reference
            // to mimic Turbo's currently string-based per-field lookup API.
            let row: serde_json::Value = serde_json::from_str(line).unwrap();
            let active = row["active"].as_bool().unwrap();
            let id = row["id"].as_i64().unwrap();
            if active && id % 5 != 0 {
                let score = row["score"].as_i64().unwrap() * 3 + id;
                let title = format!("task:{}", row["title"].as_str().unwrap());
                let encoded = serde_json::to_string(&serde_json::json!({
                    "id": id, "score": score, "title": title
                }))
                .unwrap();
                bytes += encoded.len();
                *counts.entry(encoded).or_insert(0) += 1;
            }
        }
    }
    let mut keys: Vec<_> = counts.keys().collect();
    keys.sort();
    let mut selected = 0_u64;
    for key in keys {
        let count = counts[key];
        selected += count;
        println!("{key}\t{count}");
    }
    println!("TOTAL {selected} {bytes} {}", size * rounds);
}
