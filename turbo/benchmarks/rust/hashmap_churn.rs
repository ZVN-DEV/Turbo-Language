use std::collections::HashMap;

fn main() {
    let n: usize = std::env::var("TURBO_BENCH_STEPS")
        .ok().filter(|s| !s.is_empty()).map(|s| s.parse().unwrap()).unwrap_or(2_097_152);
    assert!(n > 0);
    let keys: Vec<String> = (0..4096).map(|i| format!("k{i}")).collect();
    let mut ints: HashMap<i64, i64> = HashMap::new();
    let mut strings: HashMap<&str, i64> = HashMap::new();
    let mut state = 7_i64;
    for i in 0..n {
        state = (state * 1_103_515_245 + 12345) % 2_147_483_648;
        let key = state % 4096;
        let name = keys[key as usize].as_str();
        ints.insert(key, ints.get(&key).copied().unwrap_or(0) + 1);
        strings.insert(name, strings.get(name).copied().unwrap_or(0) + 1);
        if i % 7 == 0 {
            let removed = (key + 17) % 4096;
            ints.remove(&removed);
            strings.remove(keys[removed as usize].as_str());
        }
    }
    let mut numeric = 0_i64;
    let mut textual = 0_i64;
    for i in 0..4096_i64 {
        numeric += ints.get(&i).copied().unwrap_or(0) * (i + 1);
        textual += strings.get(keys[i as usize].as_str()).copied().unwrap_or(0) * (i + 1);
    }
    println!("{numeric}\n{textual}\n{}\n{}", ints.len(), strings.len());
}
