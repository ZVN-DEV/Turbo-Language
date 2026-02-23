# Ruby benchmark suite.
#
# 5 benchmarks printing JSON results.
# Usage: ruby benchmark.rb <benchmark_name|all>

require 'json'
require 'digest'

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def emit(name, elapsed_ms, result)
  puts JSON.generate({
    language: "ruby",
    benchmark: name,
    time_ms: elapsed_ms.round(4),
    result: result.to_s
  })
end

def clock_ms
  Process.clock_gettime(Process::CLOCK_MONOTONIC) * 1000.0
end

# ---------------------------------------------------------------------------
# 1. fib  --  naive recursive fibonacci(40)
# ---------------------------------------------------------------------------

def fib(n)
  return n if n < 2
  fib(n - 1) + fib(n - 2)
end

def bench_fib
  start = clock_ms
  result = fib(40)
  elapsed = clock_ms - start
  emit("fib", elapsed, result)
end

# ---------------------------------------------------------------------------
# 2. trees  --  binary tree depth 21, object nodes, compute checksum
# ---------------------------------------------------------------------------

class TreeNode
  attr_accessor :left, :right

  def initialize(left = nil, right = nil)
    @left = left
    @right = right
  end
end

def make_tree(depth)
  return TreeNode.new if depth == 0
  TreeNode.new(make_tree(depth - 1), make_tree(depth - 1))
end

def check_tree(node)
  return 1 if node.left.nil?
  1 + check_tree(node.left) + check_tree(node.right)
end

def bench_trees
  depth = 21
  start = clock_ms
  tree = make_tree(depth)
  checksum = check_tree(tree)
  elapsed = clock_ms - start
  emit("trees", elapsed, checksum)
end

# ---------------------------------------------------------------------------
# 3. matrix  --  1000x1000 float matrix multiply (pure Ruby)
# ---------------------------------------------------------------------------

def bench_matrix
  n = 1000

  # Build two deterministic matrices using simple formulas
  a = Array.new(n) { |i| Array.new(n) { |j| (i * n + j) * 1.0e-6 } }
  b = Array.new(n) { |i| Array.new(n) { |j| (j * n + i) * 1.0e-6 } }

  start = clock_ms

  # Transpose b for cache-friendly access
  bt = Array.new(n) { |j| Array.new(n) { |i| b[i][j] } }

  c = Array.new(n) { Array.new(n, 0.0) }
  n.times do |i|
    ai = a[i]
    n.times do |j|
      btj = bt[j]
      s = 0.0
      n.times do |k|
        s += ai[k] * btj[k]
      end
      c[i][j] = s
    end
  end

  elapsed = clock_ms - start
  emit("matrix", elapsed, c[0][0])
end

# ---------------------------------------------------------------------------
# 4. strings  --  1 MB deterministic ASCII: word count, count "the", reverse,
#                 SHA-256 hash.  Result = word count.
# ---------------------------------------------------------------------------

def bench_strings
  target_len = 1_000_000
  words = %w[
    the quick brown fox jumps over lazy dog
    alpha beta gamma delta epsilon zeta eta theta
    one two three four five six seven eight
    nine ten eleven twelve hello world benchmark test
  ]
  seed = 42
  a_lcg = 1103515245
  c_lcg = 12345
  m_lcg = 2 ** 31

  parts = []
  length = 0
  while length < target_len
    seed = (a_lcg * seed + c_lcg) % m_lcg
    word = words[seed % words.length]
    parts << word
    length += word.length + 1  # +1 for the space
  end
  text = parts.join(" ")
  text = text[0, target_len]

  start = clock_ms

  word_count = text.split.length
  the_count = text.split.count("the")
  reversed_text = text.reverse
  h = Digest::SHA256.hexdigest(reversed_text)
  # Force use of computed values so nothing is optimised away
  _ = [the_count, h]

  elapsed = clock_ms - start
  emit("strings", elapsed, word_count)
end

# ---------------------------------------------------------------------------
# 5. concurrent  --  Ractor (Ruby 3+) or Thread fallback, 1000 tasks each
#                     computing fib(30).  Result = sum of all results.
# ---------------------------------------------------------------------------

def fib_iterative(n)
  return n if n < 2
  a, b = 0, 1
  (n - 1).times { a, b = b, a + b }
  b
end

def bench_concurrent
  num_tasks = 1000
  fib_n = 30

  start = clock_ms

  if defined?(Ractor)
    # Use Ractor-based concurrency (Ruby 3+)
    # Batch tasks across available CPU cores to avoid excessive Ractor overhead
    cpu_count = begin
      require 'etc'
      Etc.nprocessors
    rescue
      4
    end

    batch_size = (num_tasks.to_f / cpu_count).ceil
    ractors = cpu_count.times.map do |core_idx|
      batch_start = core_idx * batch_size
      batch_end = [batch_start + batch_size, num_tasks].min
      count = batch_end - batch_start
      Ractor.new(count, fib_n) do |cnt, fn|
        sum = 0
        cnt.times do
          n = fn
          if n < 2
            sum += n
          else
            a, b = 0, 1
            (n - 1).times { a, b = b, a + b }
            sum += b
          end
        end
        sum
      end
    end

    total = ractors.sum(&:take)
  else
    # Fallback: Thread-based concurrency
    results = Array.new(num_tasks, 0)
    threads = num_tasks.times.map do |i|
      Thread.new(i) do |idx|
        results[idx] = fib_iterative(fib_n)
      end
    end
    threads.each(&:join)
    total = results.sum
  end

  elapsed = clock_ms - start
  emit("concurrent", elapsed, total)
end

# ---------------------------------------------------------------------------
# CLI dispatcher
# ---------------------------------------------------------------------------

BENCHMARKS = {
  "fib"        => method(:bench_fib),
  "trees"      => method(:bench_trees),
  "matrix"     => method(:bench_matrix),
  "strings"    => method(:bench_strings),
  "concurrent" => method(:bench_concurrent),
}.freeze

def main
  if ARGV.empty?
    $stderr.puts "Usage: #{$PROGRAM_NAME} <#{BENCHMARKS.keys.join('|')}|all>"
    exit 1
  end

  target = ARGV[0].downcase

  if target == "all"
    BENCHMARKS.each_value(&:call)
  elsif BENCHMARKS.key?(target)
    BENCHMARKS[target].call
  else
    $stderr.puts "Unknown benchmark: #{target}"
    $stderr.puts "Available: #{BENCHMARKS.keys.join(', ')} or 'all'"
    exit 1
  end
end

main
