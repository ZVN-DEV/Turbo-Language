#!/usr/bin/env julia

# =============================================================================
# Julia Benchmark Suite — 5 benchmarks, JSON output, stdlib only
# Usage:  julia benchmark.jl <name|all>
# =============================================================================

using Printf

# ---- helpers ----------------------------------------------------------------

function emit(name::String, time_ms::Float64, result::String)
    println("{\"language\":\"julia\",\"benchmark\":\"$name\",\"time_ms\":$(@sprintf("%.3f", time_ms)),\"result\":\"$result\"}")
end

# =============================================================================
# 1. fib — naive recursive Fibonacci(40)
# =============================================================================

function fib(n::Int)::Int
    n <= 1 && return n
    return fib(n - 1) + fib(n - 2)
end

function bench_fib()
    t0 = time_ns()
    result = fib(40)
    t1 = time_ns()
    elapsed_ms = (t1 - t0) / 1_000_000
    emit("fib", elapsed_ms, string(result))
end

# =============================================================================
# 2. trees — binary tree of depth 21, checksum
# =============================================================================

mutable struct Node
    left::Union{Node, Nothing}
    right::Union{Node, Nothing}
end

function make_tree(depth::Int)::Node
    depth == 0 && return Node(nothing, nothing)
    return Node(make_tree(depth - 1), make_tree(depth - 1))
end

function checksum(node::Node)::Int
    node.left === nothing && return 1
    return 1 + checksum(node.left) + checksum(node.right)
end

function bench_trees()
    t0 = time_ns()
    tree = make_tree(21)
    cs = checksum(tree)
    t1 = time_ns()
    elapsed_ms = (t1 - t0) / 1_000_000
    emit("trees", elapsed_ms, string(cs))
end

# =============================================================================
# 3. matrix — 1000x1000 Float64 multiply (BLAS via built-in *)
# =============================================================================

function bench_matrix()
    n = 1000
    # Deterministic fill using a simple LCG so results are reproducible
    A = zeros(Float64, n, n)
    B = zeros(Float64, n, n)
    seed::UInt64 = 42
    for j in 1:n, i in 1:n
        seed = (seed * 6364136223846793005 + 1442695040888963407) % UInt64
        A[i, j] = Float64(seed >> 33) / 1e9
        seed = (seed * 6364136223846793005 + 1442695040888963407) % UInt64
        B[i, j] = Float64(seed >> 33) / 1e9
    end

    t0 = time_ns()
    C = A * B          # uses BLAS under the hood
    t1 = time_ns()

    elapsed_ms = (t1 - t0) / 1_000_000
    result_val = @sprintf("%.6f", C[1, 1])
    emit("matrix", elapsed_ms, result_val)
end

# =============================================================================
# 4. strings — 1 MB deterministic ASCII: word count, "the" count, reverse, hash
# =============================================================================

function generate_text(size::Int)::String
    # Deterministic pseudo-random ASCII text, ~1 MB
    words = ["the", "quick", "brown", "fox", "jumps", "over", "lazy", "dog",
             "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta",
             "theta", "iota", "kappa", "lambda", "mu", "nu", "xi", "omicron",
             "pi", "rho", "sigma", "tau", "upsilon", "phi", "chi", "psi", "omega"]
    buf = IOBuffer()
    seed::UInt64 = 12345
    while position(buf) < size
        seed = (seed * 6364136223846793005 + 1442695040888963407) % UInt64
        idx = Int(seed >> 48) % length(words) + 1
        if position(buf) > 0
            write(buf, ' ')
        end
        write(buf, words[idx])
    end
    return String(take!(buf))
end

function simple_hash(s::String)::UInt64
    h::UInt64 = 5381
    for c in s
        h = ((h << 5) + h) + UInt64(c)
    end
    return h
end

function bench_strings()
    text = generate_text(1_000_000)   # ~1 MB

    t0 = time_ns()
    # Count words
    word_count = length(split(text))
    # Count occurrences of "the"
    the_count = count("the", text)
    # Reverse
    reversed_text = reverse(text)
    # Hash
    h = simple_hash(reversed_text)
    t1 = time_ns()

    elapsed_ms = (t1 - t0) / 1_000_000
    emit("strings", elapsed_ms, string(word_count))
end

# =============================================================================
# 5. concurrent — 1000 tasks computing fib(30) via Threads.@spawn, sum results
# =============================================================================

function bench_concurrent()
    n_tasks = 1000

    t0 = time_ns()
    tasks = Vector{Task}(undef, n_tasks)
    for i in 1:n_tasks
        tasks[i] = Threads.@spawn fib(30)
    end
    total = 0
    for i in 1:n_tasks
        total += fetch(tasks[i])
    end
    t1 = time_ns()

    elapsed_ms = (t1 - t0) / 1_000_000
    emit("concurrent", elapsed_ms, string(total))
end

# =============================================================================
# CLI dispatcher
# =============================================================================

const BENCHMARKS = Dict{String, Function}(
    "fib"        => bench_fib,
    "trees"      => bench_trees,
    "matrix"     => bench_matrix,
    "strings"    => bench_strings,
    "concurrent" => bench_concurrent,
)

function main()
    if isempty(ARGS)
        println(stderr, "Usage: julia benchmark.jl <benchmark|all>")
        println(stderr, "Available: ", join(sort(collect(keys(BENCHMARKS))), ", "), ", all")
        exit(1)
    end

    name = ARGS[1]
    if name == "all"
        for k in sort(collect(keys(BENCHMARKS)))
            BENCHMARKS[k]()
        end
    elseif haskey(BENCHMARKS, name)
        BENCHMARKS[name]()
    else
        println(stderr, "Unknown benchmark: $name")
        println(stderr, "Available: ", join(sort(collect(keys(BENCHMARKS))), ", "), ", all")
        exit(1)
    end
end

main()
