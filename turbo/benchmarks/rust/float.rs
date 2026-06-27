fn main() {
    let mut sum = 0.0f64;
    let mut sign = 1.0f64;
    for i in 0..50_000_000i64 {
        sum += sign / (2 * i + 1) as f64;
        sign = -sign;
    }
    println!("{:.16}", sum * 4.0);
}
