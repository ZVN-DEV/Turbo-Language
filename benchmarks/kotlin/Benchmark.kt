import java.util.concurrent.Executors
import java.util.concurrent.Future

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fun printResult(benchmark: String, timeMs: Double, result: String) {
    // Manually build JSON to avoid any dependency
    println("""{"language":"kotlin","benchmark":"$benchmark","time_ms":${"%.6f".format(timeMs)},"result":"$result"}""")
}

// ---------------------------------------------------------------------------
// 1. fib – naive recursive fibonacci(40)
// ---------------------------------------------------------------------------

fun fib(n: Int): Long =
    if (n < 2) n.toLong() else fib(n - 1) + fib(n - 2)

fun benchFib() {
    val start = System.nanoTime()
    val result = fib(40)
    val elapsed = (System.nanoTime() - start) / 1_000_000.0
    printResult("fib", elapsed, result.toString())
}

// ---------------------------------------------------------------------------
// 2. trees – binary tree of depth 21 with checksum
// ---------------------------------------------------------------------------

data class TreeNode(val value: Int, val left: TreeNode? = null, val right: TreeNode? = null)

fun buildTree(depth: Int, value: Int = 0): TreeNode {
    if (depth == 0) return TreeNode(value)
    val left = buildTree(depth - 1, 2 * value + 1)
    val right = buildTree(depth - 1, 2 * value + 2)
    return TreeNode(value, left, right)
}

fun checksum(node: TreeNode?): Long {
    if (node == null) return 0L
    return node.value.toLong() + checksum(node.left) - checksum(node.right)
}

fun benchTrees() {
    val start = System.nanoTime()
    val tree = buildTree(21)
    val cs = checksum(tree)
    val elapsed = (System.nanoTime() - start) / 1_000_000.0
    printResult("trees", elapsed, cs.toString())
}

// ---------------------------------------------------------------------------
// 3. matrix – 1000x1000 Double matrix multiply
// ---------------------------------------------------------------------------

fun benchMatrix() {
    val n = 1000
    // Deterministic fill
    val a = Array(n) { i -> DoubleArray(n) { j -> ((i * n + j) % 1000).toDouble() / 1000.0 } }
    val b = Array(n) { i -> DoubleArray(n) { j -> (((i + 1) * (j + 1)) % 1000).toDouble() / 1000.0 } }
    val c = Array(n) { DoubleArray(n) }

    val start = System.nanoTime()
    for (i in 0 until n) {
        for (k in 0 until n) {
            val aik = a[i][k]
            for (j in 0 until n) {
                c[i][j] += aik * b[k][j]
            }
        }
    }
    val elapsed = (System.nanoTime() - start) / 1_000_000.0
    printResult("matrix", elapsed, "%.6f".format(c[0][0]))
}

// ---------------------------------------------------------------------------
// 4. strings – 1 MB deterministic ASCII text processing
// ---------------------------------------------------------------------------

fun benchStrings() {
    // Build ~1 MB of deterministic ASCII text
    val words = arrayOf(
        "the", "quick", "brown", "fox", "jumps", "over", "the", "lazy", "dog",
        "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta",
        "iota", "kappa", "lambda", "mu", "nu", "xi", "omicron", "pi", "rho",
        "sigma", "tau", "upsilon", "phi", "chi", "psi", "omega"
    )
    val sb = StringBuilder(1_100_000)
    var idx = 0
    while (sb.length < 1_000_000) {
        if (sb.isNotEmpty()) sb.append(' ')
        sb.append(words[idx % words.size])
        idx++
    }
    val text = sb.toString()

    val start = System.nanoTime()

    // Count words
    var wordCount = 0
    var inWord = false
    for (ch in text) {
        if (ch == ' ' || ch == '\n' || ch == '\t') {
            inWord = false
        } else if (!inWord) {
            inWord = true
            wordCount++
        }
    }

    // Count occurrences of "the"
    var theCount = 0
    var searchFrom = 0
    while (true) {
        val pos = text.indexOf("the", searchFrom)
        if (pos < 0) break
        // Check word boundaries
        val before = pos == 0 || text[pos - 1] == ' '
        val after = pos + 3 >= text.length || text[pos + 3] == ' '
        if (before && after) theCount++
        searchFrom = pos + 1
    }

    // Reverse the string
    val reversed = StringBuilder(text).reverse().toString()

    // Simple hash of reversed string
    var hash = 0L
    for (ch in reversed) {
        hash = hash * 31 + ch.code.toLong()
    }

    val elapsed = (System.nanoTime() - start) / 1_000_000.0
    printResult("strings", elapsed, wordCount.toString())
}

// ---------------------------------------------------------------------------
// 5. concurrent – 1000 tasks computing fib(30) via Java ExecutorService
// ---------------------------------------------------------------------------

fun benchConcurrent() {
    val numTasks = 1000
    val numThreads = Runtime.getRuntime().availableProcessors()
    val executor = Executors.newFixedThreadPool(numThreads)

    val start = System.nanoTime()

    val futures = mutableListOf<Future<Long>>()
    for (i in 0 until numTasks) {
        futures.add(executor.submit<Long> { fib(30) })
    }

    var sum = 0L
    for (f in futures) {
        sum += f.get()
    }

    executor.shutdown()

    val elapsed = (System.nanoTime() - start) / 1_000_000.0
    printResult("concurrent", elapsed, sum.toString())
}

// ---------------------------------------------------------------------------
// CLI dispatcher
// ---------------------------------------------------------------------------

fun main(args: Array<String>) {
    val benchmarks = mapOf(
        "fib" to ::benchFib,
        "trees" to ::benchTrees,
        "matrix" to ::benchMatrix,
        "strings" to ::benchStrings,
        "concurrent" to ::benchConcurrent
    )

    val requested = if (args.isEmpty()) listOf("all") else args.toList()

    if ("all" in requested) {
        for ((_, fn) in benchmarks) fn()
    } else {
        for (name in requested) {
            val fn = benchmarks[name]
            if (fn != null) {
                fn()
            } else {
                System.err.println("Unknown benchmark: $name. Available: ${benchmarks.keys.joinToString(", ")}")
            }
        }
    }
}
