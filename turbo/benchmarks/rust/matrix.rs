fn main() {
    let n = 150usize;
    let mut a = vec![0i64; n * n];
    let mut b = vec![0i64; n * n];
    let mut c = vec![0i64; n * n];
    for i in 0..n * n {
        a[i] = (i % 100) as i64;
        b[i] = ((i * 3) % 100) as i64;
    }
    for row in 0..n {
        for col in 0..n {
            let mut sum = 0i64;
            for k in 0..n {
                sum += a[row * n + k] * b[k * n + col];
            }
            c[row * n + col] = sum;
        }
    }
    let total: i64 = c.iter().sum();
    println!("{}", total);
}
