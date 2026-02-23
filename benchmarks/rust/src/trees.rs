use std::time::Instant;

/// A node in a complete binary tree.
enum Tree {
    Leaf(i64),
    Node(i64, Box<Tree>, Box<Tree>),
}

impl Tree {
    /// Build a complete binary tree of the given depth.
    /// Each node stores its own index value for checksum purposes.
    fn build(depth: i32, value: i64) -> Box<Tree> {
        if depth == 0 {
            Box::new(Tree::Leaf(value))
        } else {
            let left = Tree::build(depth - 1, value * 2);
            let right = Tree::build(depth - 1, value * 2 + 1);
            Box::new(Tree::Node(value, left, right))
        }
    }

    /// Compute a checksum by traversing every node.
    fn checksum(&self) -> i64 {
        match self {
            Tree::Leaf(v) => *v,
            Tree::Node(v, left, right) => *v + left.checksum() - right.checksum(),
        }
    }
}

pub fn run() {
    let depth = 21;

    let start = Instant::now();

    // Build the tree
    let tree = Tree::build(depth, 1);

    // Compute checksum (traverses all nodes)
    let checksum = tree.checksum();

    // Drop the tree (deallocation included in timing)
    drop(tree);

    let elapsed = start.elapsed();
    let time_ms = elapsed.as_secs_f64() * 1000.0;

    crate::print_result("binary_trees", time_ms, &checksum.to_string());
}
