enum Tree {
    Leaf(i64),
    Branch(i64, Box<Tree>, Box<Tree>),
}

fn build(depth: usize, seed: i64) -> Box<Tree> {
    Box::new(if depth == 0 {
        Tree::Leaf(seed % 1000)
    } else {
        Tree::Branch(
            seed % 1000,
            build(depth - 1, (seed * 48271 + 17) % 2147483647),
            build(depth - 1, (seed * 69621 + 31) % 2147483647),
        )
    })
}

fn digest(tree: &Tree) -> i64 {
    match tree {
        Tree::Leaf(value) => *value,
        Tree::Branch(value, left, right) => {
            (digest(left) * 33 + value * 17 + digest(right) * 97) % 1000000007
        }
    }
}

fn run_round(depth: usize, seed: i64) -> i64 {
    let tree = build(depth, seed);
    digest(&tree)
}

fn parameter(name: &str, fallback: usize) -> usize {
    std::env::var(name)
        .ok()
        .filter(|s| !s.is_empty())
        .map(|s| s.parse().unwrap())
        .unwrap_or(fallback)
}

fn main() {
    let depth = parameter("TURBO_BENCH_SIZE", 19);
    let rounds = parameter("TURBO_BENCH_STEPS", 16);
    assert!(depth > 0 && depth <= 20);
    assert!(rounds > 0 && rounds <= 64);
    let nodes = (1_usize << (depth + 1)) - 1;
    let mut checksum = 0_i64;
    for step in 0..rounds {
        checksum = (checksum * 65599 + run_round(depth, 7 + step as i64 * 7919)) % 1000000007;
    }
    println!("{checksum}\n{}\n{rounds}", nodes * rounds);
}
