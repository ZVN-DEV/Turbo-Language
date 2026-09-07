use std::collections::HashMap;

fn process(line: &str, counts: &mut HashMap<String, i64>) -> i64 {
    let mut total = 0;
    for field in line.split('|') {
        let field = field.trim_matches([' ', '\t', '\r', '\n']);
        let label = field.replace("INFO:", "info:").replace("WARN:", "warn:");
        let token = label.replace('—', "-");
        if !token.is_empty() {
            *counts.entry(token).or_insert(0) += 1;
            total += 1;
        }
    }
    total
}

fn main() {
    let steps: usize = std::env::var("TURBO_BENCH_STEPS")
        .ok()
        .filter(|s| !s.is_empty())
        .map(|s| s.parse().unwrap())
        .unwrap_or(1048576);
    assert!(steps > 0 && steps <= 16777216);
    let path = std::env::var("STRING_TOKENS_INPUT")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "turbo/benchmarks/string_tokens_corpus.txt".into());
    let text = std::fs::read_to_string(path).unwrap();
    assert!(!text.is_empty() && text.ends_with('\n'));
    let lines: Vec<_> = text.split_terminator('\n').collect();
    let mut counts = HashMap::new();
    let mut total = 0_i64;
    let mut input_bytes = 0_usize;
    for step in 0..steps {
        let line = lines[step % lines.len()];
        total += process(line, &mut counts);
        input_bytes += line.len();
    }
    let mut keys: Vec<_> = counts.keys().collect();
    keys.sort();
    for key in &keys {
        println!("{key} {}", counts[*key]);
    }
    println!("TOTAL {total} {} {input_bytes}", keys.len());
}
