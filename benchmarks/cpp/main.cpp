#include <chrono>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <functional>
#include <iostream>
#include <numeric>
#include <sstream>
#include <string>
#include <thread>
#include <vector>

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

static double elapsed_ms(std::chrono::steady_clock::time_point start,
                         std::chrono::steady_clock::time_point end) {
    return std::chrono::duration<double, std::milli>(end - start).count();
}

static void print_result(const char* name, double time_ms,
                         const std::string& result) {
    // Print standardised JSON on a single line.
    std::printf(
        "{\"language\":\"cpp\",\"benchmark\":\"%s\","
        "\"time_ms\":%.3f,\"result\":\"%s\"}\n",
        name, time_ms, result.c_str());
}

// ---------------------------------------------------------------------------
// 1. fib  --  naive recursive fibonacci(40)
// ---------------------------------------------------------------------------

static int64_t fib(int n) {
    if (n < 2) return n;
    return fib(n - 1) + fib(n - 2);
}

static void bench_fib() {
    auto t0 = std::chrono::steady_clock::now();
    int64_t result = fib(40);
    auto t1 = std::chrono::steady_clock::now();
    print_result("fib", elapsed_ms(t0, t1), std::to_string(result));
}

// ---------------------------------------------------------------------------
// 2. trees  --  binary tree depth 21, allocate / checksum / delete
// ---------------------------------------------------------------------------

struct TreeNode {
    TreeNode* left;
    TreeNode* right;

    TreeNode() : left(nullptr), right(nullptr) {}
    ~TreeNode() {
        delete left;
        delete right;
    }
};

static TreeNode* make_tree(int depth) {
    TreeNode* node = new TreeNode();
    if (depth > 0) {
        node->left  = make_tree(depth - 1);
        node->right = make_tree(depth - 1);
    }
    return node;
}

static int64_t checksum_tree(const TreeNode* node) {
    if (!node) return 0;
    return 1 + checksum_tree(node->left) + checksum_tree(node->right);
}

static void bench_trees() {
    const int depth = 21;
    auto t0 = std::chrono::steady_clock::now();
    TreeNode* root = make_tree(depth);
    int64_t cs = checksum_tree(root);
    delete root;
    auto t1 = std::chrono::steady_clock::now();
    print_result("trees", elapsed_ms(t0, t1), std::to_string(cs));
}

// ---------------------------------------------------------------------------
// 3. matrix  --  1000x1000 double matrix multiply (sequential values)
// ---------------------------------------------------------------------------

static void bench_matrix() {
    const int N = 1000;

    // Allocate matrices as flat arrays for cache-friendliness.
    auto* A = new double[N * N];
    auto* B = new double[N * N];
    auto* C = new double[N * N];

    // Fill A and B with sequential values starting at 1.
    for (int i = 0; i < N * N; ++i) {
        A[i] = static_cast<double>(i + 1);
        B[i] = static_cast<double>(i + 1);
    }
    std::memset(C, 0, sizeof(double) * N * N);

    auto t0 = std::chrono::steady_clock::now();

    for (int i = 0; i < N; ++i) {
        for (int k = 0; k < N; ++k) {
            double a_ik = A[i * N + k];
            for (int j = 0; j < N; ++j) {
                C[i * N + j] += a_ik * B[k * N + j];
            }
        }
    }

    auto t1 = std::chrono::steady_clock::now();

    // Verification: C[0][0]
    // C[0][0] = sum_{k=0}^{999} A[0][k]*B[k][0]
    //         = sum_{k=0}^{999} (k+1)*(k*1000+1)
    char buf[64];
    std::snprintf(buf, sizeof(buf), "%.0f", C[0]);

    delete[] A;
    delete[] B;
    delete[] C;

    print_result("matrix", elapsed_ms(t0, t1), buf);
}

// ---------------------------------------------------------------------------
// 4. strings  --  1 MB deterministic ASCII: word-count, count "the", reverse,
//                 hash.  Result = word count.
// ---------------------------------------------------------------------------

static void bench_strings() {
    // --- Generate 1 MB of deterministic ASCII text --------------------------
    const size_t TARGET = 1024 * 1024; // 1 MB
    const char* words[] = {
        "the",    "quick", "brown",  "fox",    "jumps",
        "over",   "the",   "lazy",   "dog",    "and",
        "the",    "cat",   "sat",    "on",     "the",
        "mat",    "while", "birds",  "sang",   "in",
        "the",    "old",   "oak",    "tree",   "near",
        "the",    "calm",  "river",  "under",  "the",
        "bright", "blue",  "sky"
    };
    const int NWORDS = sizeof(words) / sizeof(words[0]);

    std::string text;
    text.reserve(TARGET + 256);
    int idx = 0;
    while (text.size() < TARGET) {
        if (!text.empty()) text += ' ';
        text += words[idx % NWORDS];
        ++idx;
    }
    text.resize(TARGET); // trim to exactly 1 MB

    auto t0 = std::chrono::steady_clock::now();

    // Word count
    int64_t word_count = 0;
    {
        bool in_word = false;
        for (char c : text) {
            if (c == ' ' || c == '\t' || c == '\n') {
                in_word = false;
            } else if (!in_word) {
                in_word = true;
                ++word_count;
            }
        }
    }

    // Count occurrences of "the"
    int64_t the_count = 0;
    {
        size_t pos = 0;
        while ((pos = text.find("the", pos)) != std::string::npos) {
            ++the_count;
            pos += 3;
        }
    }

    // Reverse the string (in-place then restore)
    {
        size_t n = text.size();
        for (size_t i = 0; i < n / 2; ++i) {
            std::swap(text[i], text[n - 1 - i]);
        }
        // reverse back to original
        for (size_t i = 0; i < n / 2; ++i) {
            std::swap(text[i], text[n - 1 - i]);
        }
    }

    // Simple deterministic hash (djb2)
    uint64_t hash = 5381;
    for (char c : text) {
        hash = ((hash << 5) + hash) + static_cast<uint8_t>(c);
    }

    // (void) results we don't report -- prevent optimisation
    volatile int64_t sink1 = the_count;
    volatile uint64_t sink2 = hash;
    (void)sink1;
    (void)sink2;

    auto t1 = std::chrono::steady_clock::now();
    print_result("strings", elapsed_ms(t0, t1), std::to_string(word_count));
}

// ---------------------------------------------------------------------------
// 5. concurrent  --  1000 threads each computing fib(30), sum results
// ---------------------------------------------------------------------------

static void bench_concurrent() {
    const int NUM_THREADS = 1000;
    const int FIB_N = 30;

    std::vector<int64_t> results(NUM_THREADS, 0);

    auto t0 = std::chrono::steady_clock::now();

    std::vector<std::thread> threads;
    threads.reserve(NUM_THREADS);

    for (int i = 0; i < NUM_THREADS; ++i) {
        threads.emplace_back([&results, i, FIB_N]() {
            results[i] = fib(FIB_N);
        });
    }

    for (auto& t : threads) {
        t.join();
    }

    int64_t sum = 0;
    for (int i = 0; i < NUM_THREADS; ++i) {
        sum += results[i];
    }

    auto t1 = std::chrono::steady_clock::now();
    print_result("concurrent", elapsed_ms(t0, t1), std::to_string(sum));
}

// ---------------------------------------------------------------------------
// Dispatcher
// ---------------------------------------------------------------------------

struct BenchEntry {
    const char* name;
    std::function<void()> fn;
};

int main(int argc, char* argv[]) {
    BenchEntry benchmarks[] = {
        {"fib",        bench_fib},
        {"trees",      bench_trees},
        {"matrix",     bench_matrix},
        {"strings",    bench_strings},
        {"concurrent", bench_concurrent},
    };
    const int N = sizeof(benchmarks) / sizeof(benchmarks[0]);

    if (argc < 2) {
        std::cerr << "Usage: " << argv[0] << " <benchmark|all>\n";
        std::cerr << "Available benchmarks:";
        for (int i = 0; i < N; ++i)
            std::cerr << " " << benchmarks[i].name;
        std::cerr << " all\n";
        return 1;
    }

    std::string arg = argv[1];

    if (arg == "all") {
        for (int i = 0; i < N; ++i)
            benchmarks[i].fn();
    } else {
        bool found = false;
        for (int i = 0; i < N; ++i) {
            if (arg == benchmarks[i].name) {
                benchmarks[i].fn();
                found = true;
                break;
            }
        }
        if (!found) {
            std::cerr << "Unknown benchmark: " << arg << "\n";
            return 1;
        }
    }

    return 0;
}
