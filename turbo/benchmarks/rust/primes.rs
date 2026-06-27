fn is_prime(n: i64) -> bool {
    if n < 2 { return false; }
    let mut i = 2i64;
    while i * i <= n {
        if n % i == 0 { return false; }
        i += 1;
    }
    true
}
fn main() {
    let mut count = 0;
    for n in 2..100_000i64 {
        if is_prime(n) { count += 1; }
    }
    println!("{}", count);
}
