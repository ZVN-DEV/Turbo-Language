#
# Elixir benchmark suite.
#
# 5 benchmarks printing JSON results.
# Usage: elixir benchmark.exs <benchmark_name|all>
#

defmodule Benchmark do
  # ---------------------------------------------------------------------------
  # Helpers
  # ---------------------------------------------------------------------------

  @doc "Print a single benchmark result as JSON."
  def emit(name, elapsed_ms, result) do
    json =
      ~s({"language":"elixir","benchmark":"#{name}","time_ms":#{Float.round(elapsed_ms / 1.0, 4)},"result":"#{result}"})

    IO.puts(json)
  end

  # ---------------------------------------------------------------------------
  # 1. fib  --  naive recursive fibonacci(40)
  # ---------------------------------------------------------------------------

  def fib(0), do: 0
  def fib(1), do: 1
  def fib(n), do: fib(n - 1) + fib(n - 2)

  def bench_fib do
    {elapsed_us, result} = :timer.tc(fn -> fib(40) end)
    emit("fib", elapsed_us / 1000.0, result)
  end

  # ---------------------------------------------------------------------------
  # 2. trees  --  binary tree depth 21, tuple-based nodes, compute checksum
  # ---------------------------------------------------------------------------

  def make_tree(0), do: {:leaf}
  def make_tree(depth), do: {:node, make_tree(depth - 1), make_tree(depth - 1)}

  def check_tree({:leaf}), do: 1
  def check_tree({:node, left, right}), do: 1 + check_tree(left) + check_tree(right)

  def bench_trees do
    depth = 21

    {elapsed_us, checksum} =
      :timer.tc(fn ->
        tree = make_tree(depth)
        check_tree(tree)
      end)

    emit("trees", elapsed_us / 1000.0, checksum)
  end

  # ---------------------------------------------------------------------------
  # 3. matrix  --  500x500 float matrix multiply (pure Elixir, no deps)
  #                Note: size is 500 instead of 1000 due to Elixir overhead.
  # ---------------------------------------------------------------------------

  def bench_matrix do
    n = 500

    # Build two deterministic matrices using simple formulas
    a =
      for i <- 0..(n - 1) do
        for j <- 0..(n - 1) do
          (i * n + j) * 1.0e-6
        end
        |> List.to_tuple()
      end
      |> List.to_tuple()

    b =
      for i <- 0..(n - 1) do
        for j <- 0..(n - 1) do
          (j * n + i) * 1.0e-6
        end
        |> List.to_tuple()
      end
      |> List.to_tuple()

    # Pre-transpose b for cache-friendly access
    bt =
      for j <- 0..(n - 1) do
        for i <- 0..(n - 1) do
          elem(elem(b, i), j)
        end
        |> List.to_tuple()
      end
      |> List.to_tuple()

    {elapsed_us, c00} =
      :timer.tc(fn ->
        # Compute c[0][0] fully, then the rest — but we only need c[0][0] for result
        # Actually compute the full multiply to be a fair benchmark
        c =
          for i <- 0..(n - 1) do
            a_row = elem(a, i)

            for j <- 0..(n - 1) do
              bt_row = elem(bt, j)

              Enum.reduce(0..(n - 1), 0.0, fn k, acc ->
                acc + elem(a_row, k) * elem(bt_row, k)
              end)
            end
          end

        # Extract c[0][0]
        c |> hd() |> hd()
      end)

    emit("matrix (500x500)", elapsed_us / 1000.0, c00)
  end

  # ---------------------------------------------------------------------------
  # 4. strings  --  1 MB deterministic ASCII: word count, count "the", reverse,
  #                 SHA-256 hash.  Result = word count.
  # ---------------------------------------------------------------------------

  def bench_strings do
    target_len = 1_000_000

    words =
      {"the", "quick", "brown", "fox", "jumps", "over", "lazy", "dog",
       "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta",
       "one", "two", "three", "four", "five", "six", "seven", "eight",
       "nine", "ten", "eleven", "twelve", "hello", "world", "benchmark",
       "test"}

    num_words = tuple_size(words)
    a_lcg = 1_103_515_245
    c_lcg = 12_345
    m_lcg = :erlang.bsl(1, 31)

    # Build deterministic text using LCG
    {parts, _seed, _length} =
      build_text([], 42, 0, target_len, words, num_words, a_lcg, c_lcg, m_lcg)

    text =
      parts
      |> Enum.reverse()
      |> Enum.join(" ")
      |> String.slice(0, target_len)

    {elapsed_us, word_count} =
      :timer.tc(fn ->
        split_words = String.split(text)
        wc = length(split_words)
        _the_count = Enum.count(split_words, &(&1 == "the"))
        reversed = String.reverse(text)
        _hash = :crypto.hash(:sha256, reversed) |> Base.encode16(case: :lower)
        wc
      end)

    emit("strings", elapsed_us / 1000.0, word_count)
  end

  defp build_text(parts, seed, length, target, _words, _num_words, _a, _c, _m)
       when length >= target do
    {parts, seed, length}
  end

  defp build_text(parts, seed, length, target, words, num_words, a, c, m) do
    seed = rem(a * seed + c, m)
    word = elem(words, rem(seed, num_words))
    build_text([word | parts], seed, length + byte_size(word) + 1, target, words, num_words, a, c, m)
  end

  # ---------------------------------------------------------------------------
  # 5. concurrent  --  spawn 1000 Task.async each computing fib(30), await all
  #                     Result = sum of all results.
  # ---------------------------------------------------------------------------

  def fib_iter(n) do
    if n < 2 do
      n
    else
      {_a, b} =
        Enum.reduce(1..(n - 1), {0, 1}, fn _i, {a, b} ->
          {b, a + b}
        end)

      b
    end
  end

  def bench_concurrent do
    num_tasks = 1000
    fib_n = 30

    {elapsed_us, total} =
      :timer.tc(fn ->
        tasks =
          for _i <- 1..num_tasks do
            Task.async(fn -> fib_iter(fib_n) end)
          end

        results = Enum.map(tasks, &Task.await(&1, :infinity))
        Enum.sum(results)
      end)

    emit("concurrent", elapsed_us / 1000.0, total)
  end
end

# ---------------------------------------------------------------------------
# CLI dispatcher
# ---------------------------------------------------------------------------

benchmarks = %{
  "fib" => &Benchmark.bench_fib/0,
  "trees" => &Benchmark.bench_trees/0,
  "matrix" => &Benchmark.bench_matrix/0,
  "strings" => &Benchmark.bench_strings/0,
  "concurrent" => &Benchmark.bench_concurrent/0,
}

case System.argv() do
  [target] ->
    target = String.downcase(target)

    if target == "all" do
      Enum.each(["fib", "trees", "matrix", "strings", "concurrent"], fn name ->
        benchmarks[name].()
      end)
    else
      case Map.fetch(benchmarks, target) do
        {:ok, fun} ->
          fun.()

        :error ->
          IO.puts(:stderr, "Unknown benchmark: #{target}")
          IO.puts(:stderr, "Available: #{Enum.join(Map.keys(benchmarks), ", ")} or 'all'")
          System.halt(1)
      end
    end

  _ ->
    IO.puts(:stderr, "Usage: elixir benchmark.exs <fib|trees|matrix|strings|concurrent|all>")
    System.halt(1)
end
