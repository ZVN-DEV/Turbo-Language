use std::time::Instant;

/// Naive recursive Fibonacci — no memoization.
fn fibonacci(n: u64) -> u64 {
    if n <= 1 {
        return n;
    }
    fibonacci(n - 1) + fibonacci(n - 2)
}

pub fn run() {
    let start = Instant::now();
    let result = fibonacci(40);
    let elapsed = start.elapsed();
    let time_ms = elapsed.as_secs_f64() * 1000.0;

    crate::print_result("fibonacci", time_ms, &result.to_string());
}
