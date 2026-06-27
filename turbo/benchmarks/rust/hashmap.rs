use std::collections::HashMap;
fn main() {
    let mut map = HashMap::new();
    for i in 0..100_000 {
        map.insert(i.to_string(), (i * 7).to_string());
    }
    let mut found = 0;
    for i in 0..100_000 {
        let expected = (i * 7).to_string();
        if let Some(v) = map.get(&i.to_string()) {
            if *v == expected { found += 1; }
        }
    }
    println!("{}", found);
}
