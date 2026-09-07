// Same safe AoS algorithm as Turbo; Rust stores records inline in Vec.
struct Particle {
    x: f64,
    y: f64,
    vx: f64,
    vy: f64,
    ax: f64,
    ay: f64,
}

fn parameter(name: &str, fallback: usize) -> usize {
    std::env::var(name)
        .ok()
        .filter(|s| !s.is_empty())
        .map(|s| s.parse().unwrap())
        .unwrap_or(fallback)
}

fn main() {
    let n = parameter("TURBO_BENCH_SIZE", 10000);
    let steps = parameter("TURBO_BENCH_STEPS", 32768);
    assert!(n > 0 && n <= 10000);
    assert!(steps > 0 && steps <= 65536);
    let mut particles = Vec::new();
    let mut seed = 7_i64;
    let mut next = || {
        seed = (seed * 48271) % 2147483647;
        (seed % 2048 - 1024) as f64 / 1024.0
    };
    for _ in 0..n {
        particles.push(Particle {
            x: next(),
            y: next(),
            vx: next(),
            vy: next(),
            ax: next(),
            ay: next(),
        });
    }
    for _ in 0..steps {
        for p in &mut particles {
            p.vx += p.ax;
            p.vy += p.ay;
            p.x += p.vx * 0.015625;
            p.y += p.vy * 0.015625;
        }
    }
    let mut checksum = 0_i64;
    for p in &particles {
        for value in [p.x, p.y, p.vx, p.vy, p.ax, p.ay] {
            let quantized = (value * 65536.0) as i64;
            checksum = (checksum * 33 + quantized % 1000000007 + 1000000007) % 1000000007;
        }
    }
    println!("{checksum}\n{n}\n{steps}");
}
