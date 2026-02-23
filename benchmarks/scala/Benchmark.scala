import java.util.concurrent.{Executors, Callable, Future}

object Benchmark {

  // ---------------------------------------------------------------------------
  // Helpers
  // ---------------------------------------------------------------------------

  private def printResult(name: String, timeMs: Double, result: String): Unit =
    println(s"""{"language":"scala","benchmark":"$name","time_ms":$timeMs,"result":"$result"}""")

  private def timed[A](block: => A): (A, Double) = {
    val t0 = System.nanoTime()
    val res = block
    val t1 = System.nanoTime()
    (res, (t1 - t0) / 1e6)
  }

  // ---------------------------------------------------------------------------
  // 1. fib  --  naive recursive fibonacci(40)
  // ---------------------------------------------------------------------------

  private def fib(n: Int): Long =
    if (n < 2) n.toLong else fib(n - 1) + fib(n - 2)

  private def runFib(): Unit = {
    val (result, ms) = timed { fib(40) }
    printResult("fib", ms, result.toString)
  }

  // ---------------------------------------------------------------------------
  // 2. trees  --  binary tree depth 21, checksum
  // ---------------------------------------------------------------------------

  private case class TreeNode(left: Option[TreeNode], right: Option[TreeNode])

  private def buildTree(depth: Int): TreeNode =
    if (depth == 0) TreeNode(None, None)
    else TreeNode(Some(buildTree(depth - 1)), Some(buildTree(depth - 1)))

  private def checksum(node: TreeNode): Long = node match {
    case TreeNode(None, None) => 1L
    case TreeNode(Some(l), Some(r)) => 1L + checksum(l) + checksum(r)
    case _ => 1L
  }

  private def runTrees(): Unit = {
    val depth = 21
    val (result, ms) = timed {
      val tree = buildTree(depth)
      checksum(tree)
    }
    printResult("trees", ms, result.toString)
  }

  // ---------------------------------------------------------------------------
  // 3. matrix  --  1000x1000 Double matrix multiply
  // ---------------------------------------------------------------------------

  private def runMatrix(): Unit = {
    val n = 1000
    val a = Array.tabulate(n, n)((i, j) => (i + j).toDouble / n)
    val b = Array.tabulate(n, n)((i, j) => (i - j).toDouble / n)

    val (result, ms) = timed {
      val c = Array.ofDim[Double](n, n)
      var i = 0
      while (i < n) {
        var k = 0
        while (k < n) {
          val aik = a(i)(k)
          var j = 0
          while (j < n) {
            c(i)(j) += aik * b(k)(j)
            j += 1
          }
          k += 1
        }
        i += 1
      }
      c(0)(0)
    }
    printResult("matrix", ms, f"$result%.6f")
  }

  // ---------------------------------------------------------------------------
  // 4. strings  --  1 MB deterministic ASCII text processing
  // ---------------------------------------------------------------------------

  private def runStrings(): Unit = {
    // Build ~1 MB of deterministic ASCII text
    val words = Array("the", "quick", "brown", "fox", "jumps", "over", "the", "lazy", "dog",
      "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta")
    val sb = new java.lang.StringBuilder(1100000)
    var idx = 0
    while (sb.length() < 1000000) {
      if (sb.length() > 0) sb.append(' ')
      sb.append(words(idx % words.length))
      idx += 1
    }
    val text = sb.toString

    val (wordCount, ms) = timed {
      // Count total words
      var wc = 0
      var inWord = false
      var i = 0
      val len = text.length
      while (i < len) {
        val c = text.charAt(i)
        if (c == ' ' || c == '\n' || c == '\t') {
          inWord = false
        } else {
          if (!inWord) { wc += 1; inWord = true }
        }
        i += 1
      }

      // Count occurrences of "the"
      var theCount = 0
      var pos = 0
      while (pos <= len - 3) {
        if (text.charAt(pos) == 't' && text.charAt(pos + 1) == 'h' && text.charAt(pos + 2) == 'e') {
          // make sure it's a whole word
          val before = pos == 0 || text.charAt(pos - 1) == ' '
          val after = pos + 3 >= len || text.charAt(pos + 3) == ' '
          if (before && after) theCount += 1
        }
        pos += 1
      }

      // Reverse the string
      val reversed = new java.lang.StringBuilder(text).reverse().toString

      // Simple hash (sum of chars)
      var hash = 0L
      i = 0
      while (i < reversed.length) {
        hash = hash * 31 + reversed.charAt(i)
        i += 1
      }

      wc
    }
    printResult("strings", ms, wordCount.toString)
  }

  // ---------------------------------------------------------------------------
  // 5. concurrent  --  ExecutorService, 1000 tasks computing fib(30), sum
  // ---------------------------------------------------------------------------

  private def runConcurrent(): Unit = {
    val numTasks = 1000
    val pool = Executors.newFixedThreadPool(Runtime.getRuntime.availableProcessors())

    val (result, ms) = timed {
      val futures = new Array[Future[Long]](numTasks)
      var i = 0
      while (i < numTasks) {
        futures(i) = pool.submit(new Callable[Long] {
          override def call(): Long = fib(30)
        })
        i += 1
      }

      var sum = 0L
      i = 0
      while (i < numTasks) {
        sum += futures(i).get()
        i += 1
      }
      sum
    }

    pool.shutdown()
    printResult("concurrent", ms, result.toString)
  }

  // ---------------------------------------------------------------------------
  // CLI dispatcher
  // ---------------------------------------------------------------------------

  private val benchmarks: Map[String, () => Unit] = Map(
    "fib"        -> (() => runFib()),
    "trees"      -> (() => runTrees()),
    "matrix"     -> (() => runMatrix()),
    "strings"    -> (() => runStrings()),
    "concurrent" -> (() => runConcurrent())
  )

  private val orderedNames = List("fib", "trees", "matrix", "strings", "concurrent")

  def main(args: Array[String]): Unit = {
    val targets = if (args.isEmpty || args.contains("all")) orderedNames
                  else args.toList.filter(benchmarks.contains)

    if (targets.isEmpty) {
      System.err.println("Usage: scala Benchmark <fib|trees|matrix|strings|concurrent|all>")
      sys.exit(1)
    }

    targets.foreach(name => benchmarks(name)())
  }
}
