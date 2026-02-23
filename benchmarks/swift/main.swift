import Foundation
import Dispatch

// MARK: - Utilities

/// Print a benchmark result in the standardized JSON format.
func printResult(_ benchmark: String, timeMs: Double, result: String) {
    print("{\"language\":\"swift\",\"benchmark\":\"\(benchmark)\",\"time_ms\":\(String(format: "%.3f", timeMs)),\"result\":\"\(result)\"}")
}

// MARK: - 1. Fibonacci (naive recursive)

func fib(_ n: Int) -> Int {
    if n <= 1 { return n }
    return fib(n - 1) + fib(n - 2)
}

func runFib() {
    let start = CFAbsoluteTimeGetCurrent()
    let result = fib(40)
    let elapsed = (CFAbsoluteTimeGetCurrent() - start) * 1000.0
    printResult("fib", timeMs: elapsed, result: "\(result)")
}

// MARK: - 2. Binary Trees

final class TreeNode {
    let value: Int
    let left: TreeNode?
    let right: TreeNode?

    init(value: Int, left: TreeNode? = nil, right: TreeNode? = nil) {
        self.value = value
        self.left = left
        self.right = right
    }
}

func buildTree(depth: Int, index: Int) -> TreeNode {
    if depth == 0 {
        return TreeNode(value: index)
    }
    return TreeNode(
        value: index,
        left: buildTree(depth: depth - 1, index: 2 * index),
        right: buildTree(depth: depth - 1, index: 2 * index + 1)
    )
}

func checksum(_ node: TreeNode?) -> Int {
    guard let node = node else { return 0 }
    return node.value + checksum(node.left) - checksum(node.right)
}

func runTrees() {
    let depth = 21

    let start = CFAbsoluteTimeGetCurrent()
    let tree = buildTree(depth: depth, index: 1)
    let cs = checksum(tree)
    // ARC frees the tree when it goes out of scope after this function.
    let elapsed = (CFAbsoluteTimeGetCurrent() - start) * 1000.0

    printResult("trees", timeMs: elapsed, result: "\(cs)")
}

// MARK: - 3. Matrix Multiply

func runMatrix() {
    let n = 1000

    // Initialize matrices with sequential values starting at 1.
    var a = [Double](repeating: 0.0, count: n * n)
    var b = [Double](repeating: 0.0, count: n * n)

    var counter = 1.0
    for i in 0..<n {
        for j in 0..<n {
            let idx = i * n + j
            a[idx] = counter
            b[idx] = counter
            counter += 1.0
        }
    }

    var c = [Double](repeating: 0.0, count: n * n)

    let start = CFAbsoluteTimeGetCurrent()

    // Standard O(n^3) matrix multiplication with ikj loop order for cache friendliness.
    for i in 0..<n {
        for k in 0..<n {
            let aik = a[i * n + k]
            for j in 0..<n {
                c[i * n + j] += aik * b[k * n + j]
            }
        }
    }

    let elapsed = (CFAbsoluteTimeGetCurrent() - start) * 1000.0

    printResult("matrix", timeMs: elapsed, result: String(format: "%.6f", c[0]))
}

// MARK: - 4. String Processing

func runStrings() {
    let targetSize = 1024 * 1024 // 1 MB

    // Deterministic word list matching Go implementation.
    let words = [
        "the", "quick", "brown", "fox", "jumps", "over", "lazy", "dog",
        "a", "an", "is", "was", "are", "were", "be", "been",
        "have", "has", "had", "do", "does", "did", "will", "would",
        "could", "should", "may", "might", "shall", "can", "need", "dare",
        "ought", "used", "to", "am", "about", "above", "after", "again",
        "all", "also", "and", "another", "any", "back", "because", "before",
        "between", "both", "but", "by", "came", "come", "day", "each",
        "even", "find", "first", "for", "from", "get", "give", "go",
        "great", "hand", "here", "high", "him", "his", "how", "its",
        "just", "know", "large", "last", "left", "life", "like", "line",
        "long", "look", "made", "make", "man", "many", "most", "much",
        "must", "my", "name", "never", "new", "next", "no", "not",
        "now", "number", "of", "old", "on", "one", "only", "or",
        "other", "our", "out", "own", "part", "people", "place", "point",
        "right", "same", "say", "she", "small", "so", "some", "still",
        "such", "take", "tell", "than", "that", "their", "them", "then"
    ]

    // Generate deterministic text using LCG (same parameters as Go).
    var seed: UInt64 = 42
    var text = ""
    text.reserveCapacity(targetSize + 100)
    var written = 0

    while written < targetSize {
        seed = seed &* 6364136223846793005 &+ 1442695040888963407
        let idx = Int((seed >> 33)) % words.count
        let word = words[idx]

        if !text.isEmpty {
            text.append(" ")
            written += 1
        }
        text.append(word)
        written += word.count
    }

    let start = CFAbsoluteTimeGetCurrent()

    // 1. Count words (split by whitespace).
    let fields = text.split(separator: " ", omittingEmptySubsequences: true)
    let wordCount = fields.count

    // 2. Count occurrences of "the" as a word.
    var theCount = 0
    for w in fields {
        if w == "the" {
            theCount += 1
        }
    }

    // 3. Reverse the string.
    let reversed = String(text.reversed())

    // 4. Compute a simple DJB2 hash of the reversed string.
    var hash: UInt64 = 5381
    for byte in reversed.utf8 {
        hash = hash &* 33 &+ UInt64(byte)
    }

    let elapsed = (CFAbsoluteTimeGetCurrent() - start) * 1000.0

    // Suppress unused-variable warnings.
    _ = theCount
    _ = hash

    printResult("strings", timeMs: elapsed, result: "\(wordCount)")
}

// MARK: - 5. Concurrent Fan-out (fib(30) x 1000)

func fibSmall(_ n: Int) -> Int {
    if n <= 1 { return n }
    return fibSmall(n - 1) + fibSmall(n - 2)
}

func runConcurrent() {
    let numTasks = 1000
    let fibN = 30

    let start = CFAbsoluteTimeGetCurrent()

    // Thread-safe collection of results.
    let results = UnsafeMutablePointer<Int>.allocate(capacity: numTasks)
    results.initialize(repeating: 0, count: numTasks)

    DispatchQueue.concurrentPerform(iterations: numTasks) { i in
        results[i] = fibSmall(fibN)
    }

    var sum = 0
    for i in 0..<numTasks {
        sum += results[i]
    }
    results.deallocate()

    let elapsed = (CFAbsoluteTimeGetCurrent() - start) * 1000.0

    printResult("concurrent", timeMs: elapsed, result: "\(sum)")
}

// MARK: - CLI Dispatcher

func main() {
    let args = CommandLine.arguments
    guard args.count >= 2 else {
        fputs("Usage: \(args[0]) <fib|trees|matrix|strings|concurrent|all>\n", stderr)
        exit(1)
    }

    let benchmark = args[1]

    switch benchmark {
    case "fib":
        runFib()
    case "trees":
        runTrees()
    case "matrix":
        runMatrix()
    case "strings":
        runStrings()
    case "concurrent":
        runConcurrent()
    case "all":
        runFib()
        runTrees()
        runMatrix()
        runStrings()
        runConcurrent()
    default:
        fputs("Unknown benchmark: \(benchmark)\n", stderr)
        fputs("Valid options: fib, trees, matrix, strings, concurrent, all\n", stderr)
        exit(1)
    }
}

main()
