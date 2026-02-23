import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.Callable;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.Future;

public class Benchmark {

    // -----------------------------------------------------------------------
    // 1. fib  --  naive recursive fibonacci(40)
    // -----------------------------------------------------------------------
    private static long fib(int n) {
        if (n < 2) return n;
        return fib(n - 1) + fib(n - 2);
    }

    private static void benchFib() {
        long start = System.nanoTime();
        long result = fib(40);
        double elapsed = (System.nanoTime() - start) / 1_000_000.0;
        System.out.printf("{\"language\":\"java\",\"benchmark\":\"fib\",\"time_ms\":%.4f,\"result\":\"%d\"}%n",
                elapsed, result);
    }

    // -----------------------------------------------------------------------
    // 2. trees  --  binary tree of depth 21, allocate nodes, compute checksum
    // -----------------------------------------------------------------------
    private static final class TreeNode {
        final TreeNode left;
        final TreeNode right;

        TreeNode(TreeNode left, TreeNode right) {
            this.left = left;
            this.right = right;
        }

        int checksum() {
            if (left == null) return 1;
            return 1 + left.checksum() + right.checksum();
        }
    }

    private static TreeNode buildTree(int depth) {
        if (depth == 0) return new TreeNode(null, null);
        return new TreeNode(buildTree(depth - 1), buildTree(depth - 1));
    }

    private static void benchTrees() {
        long start = System.nanoTime();
        TreeNode root = buildTree(21);
        int checksum = root.checksum();
        double elapsed = (System.nanoTime() - start) / 1_000_000.0;
        System.out.printf("{\"language\":\"java\",\"benchmark\":\"trees\",\"time_ms\":%.4f,\"result\":\"%d\"}%n",
                elapsed, checksum);
    }

    // -----------------------------------------------------------------------
    // 3. matrix  --  1000x1000 double matrix multiply  C = A * B
    // -----------------------------------------------------------------------
    private static void benchMatrix() {
        int n = 1000;
        double[][] a = new double[n][n];
        double[][] b = new double[n][n];
        double[][] c = new double[n][n];

        // deterministic fill
        for (int i = 0; i < n; i++) {
            for (int j = 0; j < n; j++) {
                a[i][j] = (i * n + j) * 1.0e-6;
                b[i][j] = (j * n + i) * 1.0e-6;
            }
        }

        long start = System.nanoTime();
        for (int i = 0; i < n; i++) {
            for (int k = 0; k < n; k++) {
                double aik = a[i][k];
                for (int j = 0; j < n; j++) {
                    c[i][j] += aik * b[k][j];
                }
            }
        }
        double elapsed = (System.nanoTime() - start) / 1_000_000.0;
        System.out.printf("{\"language\":\"java\",\"benchmark\":\"matrix\",\"time_ms\":%.4f,\"result\":\"%.6f\"}%n",
                elapsed, c[0][0]);
    }

    // -----------------------------------------------------------------------
    // 4. strings  --  1 MB deterministic ASCII text: word count, count "the",
    //                  reverse, simple hash
    // -----------------------------------------------------------------------
    private static void benchStrings() {
        // Build a deterministic 1 MB string from a fixed vocabulary
        String[] words = {
            "the", "quick", "brown", "fox", "jumps", "over", "the", "lazy", "dog",
            "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta",
            "iota", "kappa", "lambda", "mu", "nu", "xi", "omicron", "pi", "rho",
            "sigma", "tau", "upsilon", "phi", "chi", "psi", "omega"
        };

        StringBuilder sb = new StringBuilder(1_100_000);
        int targetLen = 1_000_000;
        int idx = 0;
        while (sb.length() < targetLen) {
            if (sb.length() > 0) sb.append(' ');
            sb.append(words[idx % words.length]);
            idx++;
        }
        String text = sb.substring(0, targetLen);

        long start = System.nanoTime();

        // word count
        int wordCount = 0;
        boolean inWord = false;
        for (int i = 0; i < text.length(); i++) {
            char ch = text.charAt(i);
            if (ch == ' ' || ch == '\n' || ch == '\t') {
                inWord = false;
            } else if (!inWord) {
                inWord = true;
                wordCount++;
            }
        }

        // count occurrences of "the"
        int theCount = 0;
        int searchFrom = 0;
        while (true) {
            int pos = text.indexOf("the", searchFrom);
            if (pos < 0) break;
            // ensure whole-word match
            boolean leftOk = (pos == 0 || text.charAt(pos - 1) == ' ');
            boolean rightOk = (pos + 3 >= text.length() || text.charAt(pos + 3) == ' ');
            if (leftOk && rightOk) theCount++;
            searchFrom = pos + 1;
        }

        // reverse the string
        String reversed = new StringBuilder(text).reverse().toString();

        // simple hash (DJB2-style)
        long hash = 5381;
        for (int i = 0; i < reversed.length(); i++) {
            hash = ((hash << 5) + hash) + reversed.charAt(i);
        }

        double elapsed = (System.nanoTime() - start) / 1_000_000.0;

        // Use theCount and hash to prevent dead-code elimination (side-effect via volatile sink)
        volatileSink = theCount + hash;

        System.out.printf("{\"language\":\"java\",\"benchmark\":\"strings\",\"time_ms\":%.4f,\"result\":\"%d\"}%n",
                elapsed, wordCount);
    }

    // volatile sink to prevent the JIT from eliminating work
    private static volatile long volatileSink;

    // -----------------------------------------------------------------------
    // 5. concurrent  --  ExecutorService, 1000 Callables each computing fib(30),
    //                     collect via futures, sum results
    // -----------------------------------------------------------------------
    private static void benchConcurrent() throws Exception {
        int numTasks = 1000;
        int fibArg = 30;
        int nThreads = Runtime.getRuntime().availableProcessors();
        ExecutorService pool = Executors.newFixedThreadPool(nThreads);

        List<Callable<Long>> tasks = new ArrayList<>(numTasks);
        for (int i = 0; i < numTasks; i++) {
            tasks.add(() -> fib(fibArg));
        }

        long start = System.nanoTime();
        List<Future<Long>> futures = pool.invokeAll(tasks);

        long sum = 0;
        for (Future<Long> f : futures) {
            sum += f.get();
        }
        double elapsed = (System.nanoTime() - start) / 1_000_000.0;

        pool.shutdown();

        System.out.printf("{\"language\":\"java\",\"benchmark\":\"concurrent\",\"time_ms\":%.4f,\"result\":\"%d\"}%n",
                elapsed, sum);
    }

    // -----------------------------------------------------------------------
    // CLI dispatcher
    // -----------------------------------------------------------------------
    public static void main(String[] args) throws Exception {
        if (args.length == 0) {
            System.err.println("Usage: java Benchmark <fib|trees|matrix|strings|concurrent|all>");
            System.exit(1);
        }

        String bench = args[0].toLowerCase();

        switch (bench) {
            case "all":
                benchFib();
                benchTrees();
                benchMatrix();
                benchStrings();
                benchConcurrent();
                break;
            case "fib":
                benchFib();
                break;
            case "trees":
                benchTrees();
                break;
            case "matrix":
                benchMatrix();
                break;
            case "strings":
                benchStrings();
                break;
            case "concurrent":
                benchConcurrent();
                break;
            default:
                System.err.println("Unknown benchmark: " + bench);
                System.err.println("Available: fib, trees, matrix, strings, concurrent, all");
                System.exit(1);
        }
    }
}
