fn main() {
    let n: usize = std::env::var("TURBO_BENCH_SIZE")
        .ok().filter(|s| !s.is_empty()).map(|s| s.parse().unwrap()).unwrap_or(33_554_432);
    assert!(n > 0);
    let mut bytes = Vec::<u8>::new();
    for i in 0..n {
        bytes.push(((i * 17 + 23) % 256) as u8);
    }
    let mut checksum = 0_i64;
    for pass in 0..4_i64 {
        for byte in &mut bytes {
            let value = (i64::from(*byte) + pass + 1) % 256;
            *byte = value as u8;
            checksum = (checksum * 33 + value) % 1_000_000_007;
        }
    }
    println!("{checksum}\n{}\n{}", bytes[0], bytes[n - 1]);
}
