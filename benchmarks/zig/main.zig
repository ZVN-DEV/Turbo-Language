const std = @import("std");

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn printResult(benchmark: []const u8, time_ns: u64, result: []const u8) void {
    const time_ms: f64 = @as(f64, @floatFromInt(time_ns)) / 1_000_000.0;
    const stdout = std.io.getStdOut().writer();
    stdout.print(
        "{{\"language\":\"zig\",\"benchmark\":\"{s}\",\"time_ms\":{d:.6},\"result\":\"{s}\"}}\n",
        .{ benchmark, time_ms, result },
    ) catch {};
}

// ---------------------------------------------------------------------------
// 1. Fibonacci – naive recursive fib(40)
// ---------------------------------------------------------------------------

fn fibonacci(n: u64) u64 {
    if (n <= 1) return n;
    return fibonacci(n - 1) + fibonacci(n - 2);
}

fn runFib() void {
    var timer = std.time.Timer.start() catch unreachable;
    const result = fibonacci(40);
    const elapsed = timer.read();

    var buf: [32]u8 = undefined;
    const slice = std.fmt.bufPrint(&buf, "{d}", .{result}) catch unreachable;
    printResult("fibonacci", elapsed, slice);
}

// ---------------------------------------------------------------------------
// 2. Binary trees – depth 21, allocate, checksum, free
// ---------------------------------------------------------------------------

const Tree = struct {
    value: i64,
    left: ?*Tree,
    right: ?*Tree,
};

fn buildTree(allocator: std.mem.Allocator, depth: i32, value: i64) *Tree {
    const node = allocator.create(Tree) catch unreachable;
    if (depth == 0) {
        node.* = .{ .value = value, .left = null, .right = null };
    } else {
        node.* = .{
            .value = value,
            .left = buildTree(allocator, depth - 1, value * 2),
            .right = buildTree(allocator, depth - 1, value * 2 + 1),
        };
    }
    return node;
}

fn checksum(node: *const Tree) i64 {
    if (node.left == null) return node.value;
    return node.value + checksum(node.left.?) - checksum(node.right.?);
}

fn freeTree(allocator: std.mem.Allocator, node: *Tree) void {
    if (node.left) |left| freeTree(allocator, left);
    if (node.right) |right| freeTree(allocator, right);
    allocator.destroy(node);
}

fn runTrees() void {
    const depth: i32 = 21;
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const allocator = gpa.allocator();

    var timer = std.time.Timer.start() catch unreachable;

    const tree = buildTree(allocator, depth, 1);
    const cs = checksum(tree);
    freeTree(allocator, tree);

    const elapsed = timer.read();

    var buf: [32]u8 = undefined;
    const slice = std.fmt.bufPrint(&buf, "{d}", .{cs}) catch unreachable;
    printResult("binary_trees", elapsed, slice);
}

// ---------------------------------------------------------------------------
// 3. Matrix multiply – 1000x1000 f64
// ---------------------------------------------------------------------------

const MAT_N: usize = 1000;

fn matrixMultiply(a: []const f64, b: []const f64, c: []f64) void {
    for (0..MAT_N) |i| {
        for (0..MAT_N) |k| {
            const a_ik = a[i * MAT_N + k];
            for (0..MAT_N) |j| {
                c[i * MAT_N + j] += a_ik * b[k * MAT_N + j];
            }
        }
    }
}

fn runMatrix() void {
    const allocator = std.heap.page_allocator;
    const size = MAT_N * MAT_N;

    const a = allocator.alloc(f64, size) catch unreachable;
    defer allocator.free(a);
    const b = allocator.alloc(f64, size) catch unreachable;
    defer allocator.free(b);
    const c = allocator.alloc(f64, size) catch unreachable;
    defer allocator.free(c);

    for (0..MAT_N) |i| {
        for (0..MAT_N) |j| {
            const idx = i * MAT_N + j;
            a[idx] = @floatFromInt(idx);
            b[idx] = @as(f64, @floatFromInt(idx)) * 0.5;
        }
    }
    @memset(c, 0.0);

    var timer = std.time.Timer.start() catch unreachable;
    matrixMultiply(a, b, c);
    const elapsed = timer.read();

    var buf: [64]u8 = undefined;
    const slice = std.fmt.bufPrint(&buf, "{d:.6}", .{c[0]}) catch unreachable;
    printResult("matrix_multiply", elapsed, slice);
}

// ---------------------------------------------------------------------------
// 4. String processing – 1 MB deterministic ASCII
// ---------------------------------------------------------------------------

const Xorshift32 = struct {
    state: u32,

    fn init(seed: u32) Xorshift32 {
        return .{ .state = seed };
    }

    fn next(self: *Xorshift32) u32 {
        var x = self.state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.state = x;
        return x;
    }

    fn nextAscii(self: *Xorshift32) u8 {
        return @intCast(self.next() % 95 + 32);
    }
};

fn generateText(allocator: std.mem.Allocator, size: usize) []u8 {
    var rng = Xorshift32.init(42);
    const buf = allocator.alloc(u8, size) catch unreachable;

    for (0..size) |i| {
        if (i % 200 == 0) {
            buf[i] = 't';
        } else if (i % 200 == 1) {
            buf[i] = 'h';
        } else if (i % 200 == 2) {
            buf[i] = 'e';
        } else if (i % 200 == 3) {
            buf[i] = ' ';
        } else if (i % 5 == 0) {
            buf[i] = ' ';
        } else {
            const ch = rng.nextAscii();
            buf[i] = if (ch < 32) 'a' else ch;
        }
    }
    return buf;
}

fn countWords(text: []const u8) usize {
    var count: usize = 0;
    var in_word = false;
    for (text) |ch| {
        if (ch == ' ' or ch == '\t' or ch == '\n' or ch == '\r') {
            if (in_word) {
                count += 1;
                in_word = false;
            }
        } else {
            in_word = true;
        }
    }
    if (in_word) count += 1;
    return count;
}

fn countOccurrences(text: []const u8, pattern: []const u8) usize {
    if (pattern.len == 0 or text.len < pattern.len) return 0;
    var count: usize = 0;
    var i: usize = 0;
    while (i <= text.len - pattern.len) : (i += 1) {
        if (std.mem.eql(u8, text[i .. i + pattern.len], pattern)) {
            count += 1;
        }
    }
    return count;
}

fn reverseString(allocator: std.mem.Allocator, text: []const u8) []u8 {
    const reversed = allocator.alloc(u8, text.len) catch unreachable;
    for (0..text.len) |i| {
        reversed[i] = text[text.len - 1 - i];
    }
    return reversed;
}

fn djb2Hash(text: []const u8) u64 {
    var hash: u64 = 5381;
    for (text) |byte| {
        hash = hash *% 33 +% @as(u64, byte);
    }
    return hash;
}

fn runStrings() void {
    const size: usize = 1_000_000;
    const allocator = std.heap.page_allocator;

    var timer = std.time.Timer.start() catch unreachable;

    const text = generateText(allocator, size);
    defer allocator.free(text);

    const word_count = countWords(text);
    _ = countOccurrences(text, "the");

    const reversed = reverseString(allocator, text);
    defer allocator.free(reversed);
    _ = djb2Hash(reversed);

    const elapsed = timer.read();

    var buf: [32]u8 = undefined;
    const slice = std.fmt.bufPrint(&buf, "{d}", .{word_count}) catch unreachable;
    printResult("string_processing", elapsed, slice);
}

// ---------------------------------------------------------------------------
// 5. Concurrent – spawn 1000 threads each computing fib(30)
// ---------------------------------------------------------------------------

fn fibWorker(result_ptr: *u64) void {
    result_ptr.* = fibonacci(30);
}

fn runConcurrent() void {
    const num_tasks: usize = 1000;
    const allocator = std.heap.page_allocator;

    const results = allocator.alloc(u64, num_tasks) catch unreachable;
    defer allocator.free(results);
    const threads = allocator.alloc(std.Thread, num_tasks) catch unreachable;
    defer allocator.free(threads);

    var timer = std.time.Timer.start() catch unreachable;

    for (0..num_tasks) |i| {
        threads[i] = std.Thread.spawn(.{}, fibWorker, .{&results[i]}) catch unreachable;
    }

    for (0..num_tasks) |i| {
        threads[i].join();
    }

    var sum: u64 = 0;
    for (results) |r| {
        sum += r;
    }

    const elapsed = timer.read();

    var buf: [32]u8 = undefined;
    const slice = std.fmt.bufPrint(&buf, "{d}", .{sum}) catch unreachable;
    printResult("concurrent_fanout", elapsed, slice);
}

// ---------------------------------------------------------------------------
// CLI dispatcher
// ---------------------------------------------------------------------------

pub fn main() void {
    const args = std.process.argsAlloc(std.heap.page_allocator) catch unreachable;
    defer std.process.argsFree(std.heap.page_allocator, args);

    const benchmark_name: []const u8 = if (args.len > 1) args[1] else "all";

    if (std.mem.eql(u8, benchmark_name, "fib")) {
        runFib();
    } else if (std.mem.eql(u8, benchmark_name, "trees")) {
        runTrees();
    } else if (std.mem.eql(u8, benchmark_name, "matrix")) {
        runMatrix();
    } else if (std.mem.eql(u8, benchmark_name, "strings")) {
        runStrings();
    } else if (std.mem.eql(u8, benchmark_name, "concurrent")) {
        runConcurrent();
    } else if (std.mem.eql(u8, benchmark_name, "all")) {
        runFib();
        runTrees();
        runMatrix();
        runStrings();
        runConcurrent();
    } else {
        const stderr = std.io.getStdErr().writer();
        stderr.print("Unknown benchmark: {s}\n", .{benchmark_name}) catch {};
        stderr.print("Available: fib, trees, matrix, strings, concurrent, all\n", .{}) catch {};
        std.process.exit(1);
    }
}
