fn collatz_len(n: i64) -> i64 {
    let (mut x, mut l) = (n, 0i64);
    while x != 1 {
        x = if x % 2 == 0 { x / 2 } else { 3 * x + 1 };
        l += 1;
    }
    l
}
fn main() {
    let mut max_len = 0i64;
    for i in 1..=1_000_000i64 {
        let l = collatz_len(i);
        if l > max_len { max_len = l; }
    }
    println!("{}", max_len);
}
