import 'dart:isolate';
import 'dart:convert';

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

void printResult(String name, double timeMs, String result) {
  final json = jsonEncode({
    'language': 'dart',
    'benchmark': name,
    'time_ms': double.parse(timeMs.toStringAsFixed(4)),
    'result': result,
  });
  print(json);
}

// ---------------------------------------------------------------------------
// 1. fib  --  naive recursive fibonacci(40)
// ---------------------------------------------------------------------------

int fib(int n) {
  if (n < 2) return n;
  return fib(n - 1) + fib(n - 2);
}

void benchFib() {
  final sw = Stopwatch()..start();
  final result = fib(40);
  sw.stop();
  printResult('fib', sw.elapsedMicroseconds / 1000.0, result.toString());
}

// ---------------------------------------------------------------------------
// 2. trees  --  binary tree depth 21, checksum
// ---------------------------------------------------------------------------

class TreeNode {
  final TreeNode? left;
  final TreeNode? right;

  TreeNode(this.left, this.right);

  int check() {
    if (left == null) return 1;
    return 1 + left!.check() + right!.check();
  }
}

TreeNode buildTree(int depth) {
  if (depth == 0) return TreeNode(null, null);
  return TreeNode(buildTree(depth - 1), buildTree(depth - 1));
}

void benchTrees() {
  const depth = 21;
  final sw = Stopwatch()..start();
  final tree = buildTree(depth);
  final checksum = tree.check();
  sw.stop();
  printResult('trees', sw.elapsedMicroseconds / 1000.0, checksum.toString());
}

// ---------------------------------------------------------------------------
// 3. matrix  --  1000x1000 double matrix multiply
// ---------------------------------------------------------------------------

void benchMatrix() {
  const n = 1000;

  // Allocate matrices as flat lists for cache friendliness.
  final a = List<double>.generate(n * n, (i) => (i ~/ n + 1) * 1.0);
  final b = List<double>.generate(n * n, (i) => (i % n + 1) * 1.0);
  final c = List<double>.filled(n * n, 0.0);

  final sw = Stopwatch()..start();
  for (int i = 0; i < n; i++) {
    final iOff = i * n;
    for (int k = 0; k < n; k++) {
      final aik = a[iOff + k];
      final kOff = k * n;
      for (int j = 0; j < n; j++) {
        c[iOff + j] += aik * b[kOff + j];
      }
    }
  }
  sw.stop();

  printResult('matrix', sw.elapsedMicroseconds / 1000.0, c[0].toString());
}

// ---------------------------------------------------------------------------
// 4. strings  --  1 MB deterministic ASCII text processing
// ---------------------------------------------------------------------------

void benchStrings() {
  // Build a deterministic ~1 MB ASCII string.
  const totalBytes = 1024 * 1024; // 1 MB
  final words = <String>[
    'the', 'quick', 'brown', 'fox', 'jumps', 'over', 'the', 'lazy', 'dog',
    'alpha', 'beta', 'gamma', 'delta', 'epsilon', 'zeta', 'eta', 'theta',
  ];
  final buf = StringBuffer();
  int wi = 0;
  while (buf.length < totalBytes) {
    if (buf.length > 0) buf.write(' ');
    buf.write(words[wi % words.length]);
    wi++;
  }
  final text = buf.toString();

  final sw = Stopwatch()..start();

  // Count words
  int wordCount = 0;
  bool inWord = false;
  for (int i = 0; i < text.length; i++) {
    final c = text.codeUnitAt(i);
    if (c == 0x20 || c == 0x0A || c == 0x0D || c == 0x09) {
      inWord = false;
    } else {
      if (!inWord) wordCount++;
      inWord = true;
    }
  }

  // Count occurrences of "the"
  int theCount = 0;
  int idx = 0;
  while (true) {
    idx = text.indexOf('the', idx);
    if (idx == -1) break;
    // Check word boundary (space or start/end)
    final before = idx == 0 || text.codeUnitAt(idx - 1) == 0x20;
    final after = idx + 3 >= text.length || text.codeUnitAt(idx + 3) == 0x20;
    if (before && after) theCount++;
    idx += 3;
  }

  // Reverse the string
  final reversed = String.fromCharCodes(text.runes.toList().reversed);

  // Simple hash (djb2)
  int hash = 5381;
  for (int i = 0; i < reversed.length; i++) {
    hash = ((hash << 5) + hash + reversed.codeUnitAt(i)) & 0x7FFFFFFF;
  }

  sw.stop();

  // Use theCount and hash to prevent dead-code elimination (via identity).
  _ = theCount;
  _ = hash;

  printResult('strings', sw.elapsedMicroseconds / 1000.0, wordCount.toString());
}

// Prevent tree-shaking / dead-code elimination.
dynamic _ = null;

// ---------------------------------------------------------------------------
// 5. concurrent  --  isolates computing fib(30)
//    100 isolates, each computing fib(30) x 10 = 1000 total fibs
// ---------------------------------------------------------------------------

int fibIsolateWork(int unused) {
  int total = 0;
  for (int i = 0; i < 10; i++) {
    total += fib(30);
  }
  return total;
}

Future<void> benchConcurrent() async {
  const numIsolates = 100;

  final sw = Stopwatch()..start();

  final futures = <Future<int>>[];
  for (int i = 0; i < numIsolates; i++) {
    futures.add(Isolate.run(() => fibIsolateWork(i)));
  }
  final results = await Future.wait(futures);
  final sum = results.fold<int>(0, (a, b) => a + b);

  sw.stop();
  printResult('concurrent', sw.elapsedMicroseconds / 1000.0, sum.toString());
}

// ---------------------------------------------------------------------------
// CLI dispatcher
// ---------------------------------------------------------------------------

Future<void> main(List<String> args) async {
  final benchmarks = <String, Future<void> Function()>{
    'fib': () async => benchFib(),
    'trees': () async => benchTrees(),
    'matrix': () async => benchMatrix(),
    'strings': () async => benchStrings(),
    'concurrent': () async => await benchConcurrent(),
  };

  if (args.isEmpty) {
    print('Usage: benchmark <name|all>');
    print('Available: ${benchmarks.keys.join(", ")}, all');
    return;
  }

  final requested = args[0];

  if (requested == 'all') {
    for (final entry in benchmarks.entries) {
      await entry.value();
    }
  } else if (benchmarks.containsKey(requested)) {
    await benchmarks[requested]!();
  } else {
    print('Unknown benchmark: $requested');
    print('Available: ${benchmarks.keys.join(", ")}, all');
  }
}
