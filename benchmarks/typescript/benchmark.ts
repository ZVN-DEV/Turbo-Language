import { performance } from "perf_hooks";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function emit(name: string, timeMs: number, result: string): void {
  const json = JSON.stringify({
    language: "typescript",
    benchmark: name,
    time_ms: parseFloat(timeMs.toFixed(4)),
    result,
  });
  console.log(json);
}

// ---------------------------------------------------------------------------
// 1. fib  --  naive recursive Fibonacci(40)
// ---------------------------------------------------------------------------

function fib(n: number): number {
  if (n <= 1) return n;
  return fib(n - 1) + fib(n - 2);
}

function benchFib(): void {
  const start = performance.now();
  const result = fib(40);
  const elapsed = performance.now() - start;
  emit("fib", elapsed, String(result));
}

// ---------------------------------------------------------------------------
// 2. trees  --  binary tree depth 21, allocate + checksum
// ---------------------------------------------------------------------------

interface TreeNode {
  left: TreeNode | null;
  right: TreeNode | null;
}

function makeTree(depth: number): TreeNode {
  if (depth === 0) return { left: null, right: null };
  return { left: makeTree(depth - 1), right: makeTree(depth - 1) };
}

function checksum(node: TreeNode | null): number {
  if (node === null) return 0;
  if (node.left === null) return 1;
  return 1 + checksum(node.left) + checksum(node.right);
}

function benchTrees(): void {
  const depth = 21;
  const start = performance.now();
  const tree = makeTree(depth);
  const cs = checksum(tree);
  const elapsed = performance.now() - start;
  emit("trees", elapsed, String(cs));
}

// ---------------------------------------------------------------------------
// 3. matrix  --  1000x1000 matrix multiply, result = C[0][0]
// ---------------------------------------------------------------------------

function benchMatrix(): void {
  const N = 1000;

  // Allocate and fill A and B with deterministic values
  const A: number[][] = new Array(N);
  const B: number[][] = new Array(N);
  for (let i = 0; i < N; i++) {
    A[i] = new Array(N);
    B[i] = new Array(N);
    for (let j = 0; j < N; j++) {
      A[i][j] = (i * N + j) % 100;
      B[i][j] = (j * N + i) % 100;
    }
  }

  const start = performance.now();

  // C = A * B
  const C: number[][] = new Array(N);
  for (let i = 0; i < N; i++) {
    C[i] = new Array(N).fill(0);
    for (let k = 0; k < N; k++) {
      const aik = A[i][k];
      for (let j = 0; j < N; j++) {
        C[i][j] += aik * B[k][j];
      }
    }
  }

  const elapsed = performance.now() - start;
  emit("matrix", elapsed, String(C[0][0]));
}

// ---------------------------------------------------------------------------
// 4. strings  --  1 MB deterministic ASCII: word count, count "the", reverse,
//                 simple hash.  Result = word count.
// ---------------------------------------------------------------------------

function benchStrings(): void {
  // Build a deterministic ~1 MB string from a fixed vocabulary
  const words = [
    "the", "quick", "brown", "fox", "jumps", "over", "the", "lazy", "dog",
    "and", "the", "cat", "sat", "on", "the", "mat", "while", "birds", "fly",
    "high", "above", "the", "clouds", "in", "the", "bright", "blue", "sky",
  ];

  const parts: string[] = [];
  let totalLen = 0;
  const target = 1_000_000;
  let idx = 0;
  while (totalLen < target) {
    const w = words[idx % words.length];
    if (totalLen > 0) {
      parts.push(" ");
      totalLen += 1;
    }
    parts.push(w);
    totalLen += w.length;
    idx++;
  }
  const text = parts.join("");

  const start = performance.now();

  // Word count
  const wordList = text.split(/\s+/);
  const wordCount = wordList.length;

  // Count occurrences of "the"
  let theCount = 0;
  for (const w of wordList) {
    if (w === "the") theCount++;
  }

  // Reverse the whole string
  const reversed = text.split("").reverse().join("");

  // Simple hash (djb2-style)
  let hash = 5381;
  for (let i = 0; i < reversed.length; i++) {
    hash = ((hash * 33) ^ reversed.charCodeAt(i)) >>> 0;
  }

  // Touch theCount and hash so they are not optimised away
  void theCount;
  void hash;

  const elapsed = performance.now() - start;
  emit("strings", elapsed, String(wordCount));
}

// ---------------------------------------------------------------------------
// 5. concurrent  --  Promise.all with 1000 promises each computing fib(30).
//    JS is single-threaded, so they run sequentially.  Result = sum.
// ---------------------------------------------------------------------------

function fibSmall(n: number): number {
  if (n <= 1) return n;
  return fibSmall(n - 1) + fibSmall(n - 2);
}

async function benchConcurrent(): Promise<void> {
  const N = 1000;
  const start = performance.now();

  const promises: Promise<number>[] = [];
  for (let i = 0; i < N; i++) {
    promises.push(Promise.resolve().then(() => fibSmall(30)));
  }

  const results = await Promise.all(promises);
  let sum = 0;
  for (const r of results) sum += r;

  const elapsed = performance.now() - start;
  emit("concurrent", elapsed, String(sum));
}

// ---------------------------------------------------------------------------
// CLI dispatcher
// ---------------------------------------------------------------------------

const benchmarks: Record<string, () => void | Promise<void>> = {
  fib: benchFib,
  trees: benchTrees,
  matrix: benchMatrix,
  strings: benchStrings,
  concurrent: benchConcurrent,
};

async function main(): Promise<void> {
  const arg = process.argv[2] ?? "all";

  if (arg === "all") {
    for (const name of Object.keys(benchmarks)) {
      await benchmarks[name]();
    }
  } else if (benchmarks[arg]) {
    await benchmarks[arg]();
  } else {
    console.error(`Unknown benchmark: ${arg}`);
    console.error(`Available: ${Object.keys(benchmarks).join(", ")}, all`);
    process.exit(1);
  }
}

main();
