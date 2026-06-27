fn main() {
    let base = "the quick brown fox jumps over the lazy dog";
    let mut count = 0;
    for _ in 0..1000 {
        let words: Vec<&str> = base.split(' ').collect();
        let joined = words.join("-");
        let has_fox = joined.contains("fox");
        let _replaced = joined.replace("fox", "cat");
        if has_fox { count += 1; }
    }
    println!("{}", count);
}
