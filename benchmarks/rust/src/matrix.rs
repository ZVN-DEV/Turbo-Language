use std::time::Instant;

const N: usize = 1000;

/// Multiply two NxN f64 matrices stored as flat Vec<f64> in row-major order.
fn matrix_multiply(a: &[f64], b: &[f64], c: &mut [f64]) {
    for i in 0..N {
        for k in 0..N {
            let a_ik = a[i * N + k];
            for j in 0..N {
                c[i * N + j] += a_ik * b[k * N + j];
            }
        }
    }
}

pub fn run() {
    // Fill matrices with sequential values.
    // A[i][j] = (i * N + j) as f64
    // B[i][j] = ((i * N + j) as f64) * 0.5
    let mut a = vec![0.0_f64; N * N];
    let mut b = vec![0.0_f64; N * N];
    for i in 0..N {
        for j in 0..N {
            let idx = i * N + j;
            a[idx] = idx as f64;
            b[idx] = (idx as f64) * 0.5;
        }
    }
    let mut c = vec![0.0_f64; N * N];

    let start = Instant::now();
    matrix_multiply(&a, &b, &mut c);
    let elapsed = start.elapsed();
    let time_ms = elapsed.as_secs_f64() * 1000.0;

    // Result is element [0][0] of the product matrix.
    let result = c[0];

    crate::print_result("matrix_multiply", time_ms, &format!("{:.6}", result));
}
