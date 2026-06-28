#!/usr/bin/env python3
"""Deterministic word-count benchmark input generator.

Emits a text file of lowercase ASCII words separated by spaces and newlines,
with a Zipf-like frequency distribution so the top-N word ranking is stable
and meaningful. Fully deterministic: a fixed LCG seed and a fixed vocabulary
mean the same target size always yields byte-for-byte identical output,
independent of Python version or platform.

Only ONE copy of this generator runs per benchmark invocation; every language
implementation then reads the exact same file, so output equality across
languages does not depend on cross-platform float reproducibility.

Usage: gen_wordcount_input.py <output_path> [target_megabytes]
"""
import bisect
import sys

# A fixed vocabulary of common lowercase English words. Order matters: it is
# the Zipf rank order (index 0 is the most frequent word, and so on), so the
# generated top-N ranking is determined by this list plus the LCG.
VOCAB = [
    "the", "and", "that", "have", "for", "not", "with", "you", "this", "but",
    "his", "from", "they", "say", "her", "she", "will", "one", "all", "would",
    "there", "their", "what", "out", "about", "who", "get", "which", "when", "make",
    "can", "like", "time", "just", "him", "know", "take", "people", "into", "year",
    "your", "good", "some", "could", "them", "see", "other", "than", "then", "now",
    "look", "only", "come", "its", "over", "think", "also", "back", "after", "use",
    "two", "how", "our", "work", "first", "well", "way", "even", "new", "want",
    "because", "any", "these", "give", "day", "most", "system", "data", "program", "value",
    "function", "object", "memory", "thread", "buffer", "pointer", "string", "array", "table", "index",
    "node", "graph", "edge", "stack", "queue", "heap", "tree", "list", "map", "set",
    "compile", "runtime", "native", "binary", "source", "target", "module", "crate", "parser", "lexer",
    "token", "syntax", "semantic", "type", "trait", "structure", "enumerate", "match", "loop", "branchy",
    "register", "cache", "vector", "matrix", "scalar", "floating", "integer", "boolean", "unsigned", "signed",
    "kernel", "process", "socket", "packet", "stream", "channel", "future", "asynchronous", "await", "spawn",
    "garbage", "collector", "arena", "allocate", "release", "acquire", "atomic", "mutex", "lock", "barrier",
    "client", "server", "request", "response", "header", "payload", "method", "route", "handler", "router",
    "render", "frame", "pixel", "shader", "texture", "framebuffer", "canvas", "window", "widget", "layout",
    "query", "record", "column", "schema", "cursor", "commit", "rollback", "transaction", "replica", "shard",
    "branch", "merge", "committed", "rebase", "stash", "remote", "origin", "upstream", "tagged", "blob",
    "encode", "decode", "encrypt", "decrypt", "hash", "digest", "cipher", "signature", "verify", "validate",
    "input", "output", "format", "buffered", "flush", "scan", "printed", "written", "reading", "open",
    "close", "seek", "append", "truncate", "rename", "deleted", "create", "modify", "access", "permission",
    "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta", "iota", "kappa",
    "north", "south", "east", "west", "river", "mountain", "valley", "forest", "desert", "ocean",
]


def main() -> int:
    if len(sys.argv) < 2:
        sys.stderr.write("usage: gen_wordcount_input.py <output_path> [target_megabytes]\n")
        return 2
    out_path = sys.argv[1]
    target_mb = float(sys.argv[2]) if len(sys.argv) > 2 else 5.0
    target_bytes = int(target_mb * 1024 * 1024)

    n = len(VOCAB)
    # Zipf cumulative distribution: weight(rank r, 1-indexed) = 1 / r.
    weights = [1.0 / (i + 1) for i in range(n)]
    total_w = sum(weights)
    cum = []
    acc = 0.0
    for w in weights:
        acc += w
        cum.append(acc / total_w)

    # 64-bit LCG (constants from Knuth/MMIX) for a self-contained, deterministic
    # uniform source. The seed is fixed so output is reproducible.
    state = 0x2545F4914F6CDD1D & 0xFFFFFFFFFFFFFFFF

    def rnd() -> float:
        nonlocal state
        state = (state * 6364136223846793005 + 1442695040888963407) & 0xFFFFFFFFFFFFFFFF
        return (state >> 11) / float(1 << 53)

    words_per_line = 12
    written = 0
    buf = []
    line_words = 0
    with open(out_path, "w", encoding="ascii", newline="\n") as f:
        while written < target_bytes:
            u = rnd()
            idx = bisect.bisect_left(cum, u)
            if idx >= n:
                idx = n - 1
            word = VOCAB[idx]
            buf.append(word)
            line_words += 1
            written += len(word) + 1
            if line_words >= words_per_line:
                f.write(" ".join(buf))
                f.write("\n")
                buf = []
                line_words = 0
        if buf:
            f.write(" ".join(buf))
            f.write("\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
