// Word-frequency count baseline (Rust).
//
// Reads the file given as argv[1] (or $WORDCOUNT_INPUT), tokenizes on ASCII
// whitespace, counts word frequencies in a HashMap, then prints the top-20
// words by (count desc, word asc) followed by a final "TOTAL <words> <unique>"
// line. Output must match wordcount.tb byte-for-byte.
use std::collections::HashMap;
use std::env;
use std::fs;

const TOP_N: usize = 20;

fn main() {
    let path = env::args()
        .nth(1)
        .or_else(|| env::var("WORDCOUNT_INPUT").ok())
        .unwrap_or_else(|| "wordcount_input.txt".to_string());

    let text = fs::read_to_string(&path).unwrap_or_else(|e| {
        eprintln!("cannot read {path}: {e}");
        std::process::exit(1);
    });

    let mut counts: HashMap<&str, u64> = HashMap::new();
    let mut total: u64 = 0;
    for word in text.split_ascii_whitespace() {
        *counts.entry(word).or_insert(0) += 1;
        total += 1;
    }

    let unique = counts.len();
    let mut list: Vec<(&str, u64)> = counts.into_iter().collect();
    // Sort by count descending, then word ascending.
    list.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));

    let mut out = String::new();
    for (word, count) in list.iter().take(TOP_N) {
        out.push_str(&format!("{word} {count}\n"));
    }
    out.push_str(&format!("TOTAL {total} {unique}\n"));
    print!("{out}");
}
