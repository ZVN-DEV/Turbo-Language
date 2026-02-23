using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Globalization;
using System.Security.Cryptography;
using System.Text;
using System.Threading.Tasks;

namespace Benchmarks;

// ---------------------------------------------------------------------------
// JSON helper
// ---------------------------------------------------------------------------
static class JsonResult
{
    public static void Print(string benchmark, double timeMs, string result)
    {
        string time = timeMs.ToString("F4", CultureInfo.InvariantCulture);
        Console.WriteLine(
            $"{{\"language\":\"csharp\",\"benchmark\":\"{benchmark}\",\"time_ms\":{time},\"result\":\"{result}\"}}");
    }
}

// ---------------------------------------------------------------------------
// 1. fib  --  naive recursive fibonacci(40)
// ---------------------------------------------------------------------------
static class FibBenchmark
{
    static long Fib(int n)
    {
        if (n < 2) return n;
        return Fib(n - 1) + Fib(n - 2);
    }

    public static void Run()
    {
        var sw = Stopwatch.StartNew();
        long result = Fib(40);
        sw.Stop();
        JsonResult.Print("fib", sw.Elapsed.TotalMilliseconds, result.ToString());
    }
}

// ---------------------------------------------------------------------------
// 2. trees  --  binary tree of depth 21, class-based nodes, checksum
// ---------------------------------------------------------------------------
static class TreesBenchmark
{
    sealed class TreeNode
    {
        public TreeNode? Left;
        public TreeNode? Right;

        public TreeNode(TreeNode? left, TreeNode? right)
        {
            Left = left;
            Right = right;
        }
    }

    static TreeNode BuildTree(int depth)
    {
        if (depth == 0) return new TreeNode(null, null);
        return new TreeNode(BuildTree(depth - 1), BuildTree(depth - 1));
    }

    static long Checksum(TreeNode node)
    {
        if (node.Left == null) return 1;
        return 1 + Checksum(node.Left) + Checksum(node.Right!);
    }

    public static void Run()
    {
        var sw = Stopwatch.StartNew();
        var tree = BuildTree(21);
        long cs = Checksum(tree);
        sw.Stop();
        JsonResult.Print("trees", sw.Elapsed.TotalMilliseconds, cs.ToString());
    }
}

// ---------------------------------------------------------------------------
// 3. matrix  --  1000x1000 double matrix multiply  C = A * B
// ---------------------------------------------------------------------------
static class MatrixBenchmark
{
    public static void Run()
    {
        const int N = 1000;

        // Deterministic fill
        double[][] a = new double[N][];
        double[][] b = new double[N][];
        double[][] c = new double[N][];

        for (int i = 0; i < N; i++)
        {
            a[i] = new double[N];
            b[i] = new double[N];
            c[i] = new double[N];
            for (int j = 0; j < N; j++)
            {
                a[i][j] = (i * N + j) * 1.0e-6;
                b[i][j] = (j * N + i) * 1.0e-6;
            }
        }

        var sw = Stopwatch.StartNew();

        // Classic ijk multiply
        for (int i = 0; i < N; i++)
        {
            for (int k = 0; k < N; k++)
            {
                double aik = a[i][k];
                for (int j = 0; j < N; j++)
                {
                    c[i][j] += aik * b[k][j];
                }
            }
        }

        sw.Stop();

        string result = c[0][0].ToString("G17", CultureInfo.InvariantCulture);
        JsonResult.Print("matrix", sw.Elapsed.TotalMilliseconds, result);
    }
}

// ---------------------------------------------------------------------------
// 4. strings  --  1 MB deterministic ASCII text processing
// ---------------------------------------------------------------------------
static class StringsBenchmark
{
    public static void Run()
    {
        // Build 1 MB of deterministic ASCII text
        string[] words = {
            "the", "quick", "brown", "fox", "jumps", "over", "the", "lazy", "dog",
            "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta",
            "iota", "kappa", "lambda", "mu", "nu", "xi", "omicron", "pi", "rho",
            "sigma", "tau", "upsilon", "phi", "chi", "psi", "omega"
        };

        const int targetSize = 1024 * 1024; // 1 MB
        var sb = new StringBuilder(targetSize + 256);
        int idx = 0;
        while (sb.Length < targetSize)
        {
            if (sb.Length > 0) sb.Append(' ');
            sb.Append(words[idx % words.Length]);
            idx++;
        }
        string text = sb.ToString();

        var sw = Stopwatch.StartNew();

        // Count words
        int wordCount = 0;
        bool inWord = false;
        for (int i = 0; i < text.Length; i++)
        {
            if (text[i] == ' ')
            {
                inWord = false;
            }
            else if (!inWord)
            {
                inWord = true;
                wordCount++;
            }
        }

        // Count occurrences of "the"
        int theCount = 0;
        int searchFrom = 0;
        while (true)
        {
            int pos = text.IndexOf("the", searchFrom, StringComparison.Ordinal);
            if (pos < 0) break;
            // Only count whole words
            bool leftOk = pos == 0 || text[pos - 1] == ' ';
            bool rightOk = pos + 3 >= text.Length || text[pos + 3] == ' ';
            if (leftOk && rightOk) theCount++;
            searchFrom = pos + 1;
        }

        // Reverse the string
        char[] chars = text.ToCharArray();
        Array.Reverse(chars);
        string reversed = new string(chars);

        // SHA-256 hash of the reversed string
        byte[] hashBytes = SHA256.HashData(Encoding.UTF8.GetBytes(reversed));
        StringBuilder hashSb = new StringBuilder(64);
        foreach (byte b in hashBytes)
            hashSb.Append(b.ToString("x2"));
        string hash = hashSb.ToString();

        sw.Stop();

        // Prevent dead-code elimination: use theCount, reversed.Length, hash
        _ = theCount;
        _ = reversed.Length;
        _ = hash;

        JsonResult.Print("strings", sw.Elapsed.TotalMilliseconds, wordCount.ToString());
    }
}

// ---------------------------------------------------------------------------
// 5. concurrent  --  Task.WhenAll with 1000 tasks computing fib(30)
// ---------------------------------------------------------------------------
static class ConcurrentBenchmark
{
    static long Fib(int n)
    {
        if (n < 2) return n;
        return Fib(n - 1) + Fib(n - 2);
    }

    public static void Run()
    {
        const int taskCount = 1000;

        var sw = Stopwatch.StartNew();

        var tasks = new Task<long>[taskCount];
        for (int i = 0; i < taskCount; i++)
        {
            tasks[i] = Task.Run(() => Fib(30));
        }
        Task.WhenAll(tasks).GetAwaiter().GetResult();

        long sum = 0;
        for (int i = 0; i < taskCount; i++)
        {
            sum += tasks[i].Result;
        }

        sw.Stop();
        JsonResult.Print("concurrent", sw.Elapsed.TotalMilliseconds, sum.ToString());
    }
}

// ---------------------------------------------------------------------------
// CLI dispatcher
// ---------------------------------------------------------------------------
class Program
{
    static readonly Dictionary<string, Action> Benchmarks = new()
    {
        ["fib"]        = FibBenchmark.Run,
        ["trees"]      = TreesBenchmark.Run,
        ["matrix"]     = MatrixBenchmark.Run,
        ["strings"]    = StringsBenchmark.Run,
        ["concurrent"] = ConcurrentBenchmark.Run,
    };

    static void Main(string[] args)
    {
        if (args.Length == 0)
        {
            Console.Error.WriteLine("Usage: Benchmark <name|all> [name ...]");
            Console.Error.WriteLine("Available: " + string.Join(", ", Benchmarks.Keys));
            Environment.Exit(1);
        }

        var toRun = new List<string>();

        foreach (string arg in args)
        {
            if (arg.Equals("all", StringComparison.OrdinalIgnoreCase))
            {
                toRun.AddRange(Benchmarks.Keys);
            }
            else if (Benchmarks.ContainsKey(arg))
            {
                toRun.Add(arg);
            }
            else
            {
                Console.Error.WriteLine($"Unknown benchmark: {arg}");
                Console.Error.WriteLine("Available: " + string.Join(", ", Benchmarks.Keys));
                Environment.Exit(1);
            }
        }

        foreach (string name in toRun)
        {
            Benchmarks[name]();
        }
    }
}
