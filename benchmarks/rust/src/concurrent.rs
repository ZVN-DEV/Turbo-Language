use std::time::Instant;
use rayon::prelude::*;

/// Naive recursive Fibonacci (same as fib.rs but local to this module).
fn fibonacci(n: u64) -> u64 {
    if n <= 1 {
        return n;
    }
    fibonacci(n - 1) + fibonacci(n - 2)
}

pub fn run() {
    let num_tasks: usize = 1000;
    let fib_n: u64 = 30;

    let start = Instant::now();

    // Spawn 1000 parallel tasks using rayon, each computing fib(30).
    let results: Vec<u64> = (0..num_tasks)
        .into_par_iter()
        .map(|_| fibonacci(fib_n))
        .collect();

    let sum: u64 = results.iter().sum();

    let elapsed = start.elapsed();
    let time_ms = elapsed.as_secs_f64() * 1000.0;

    crate::print_result("concurrent_fanout", time_ms, &sum.to_string());
}
