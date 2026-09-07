#!/usr/bin/env python3
"""Frozen v1 NDJSON transform corpus; stdlib only, no network or hidden input."""
import argparse
import json
from pathlib import Path


def corpus(size):
    if not 1 <= size <= 4096:
        raise ValueError("JSON record count must be 1..4096")
    titles = ["quote\"", "back\\slash", "line\nbreak", "tab\tdata",
              "café 東京 🙂 e\u0301", "".join(chr(c) for c in range(1, 32)),
              "'); DROP TABLE jobs; --\u2028", ""]
    state = 7
    lines = []
    for index in range(size):
        state = (state * 48271) % 2147483647
        title = titles[index % len(titles)]
        row = dict(meta=dict(id=-999, title="nested shadow", ratio=1.25),
                   id=index - size // 2, active=state % 4 != 0,
                   score=state % 2001 - 1000, title=title)
        encoded = json.dumps(row, ensure_ascii=index % 2 == 0, separators=(",", ":"))
        if index % 3 == 0:
            encoded = encoded.replace('"title":', '"ti\\u0074le":')
        lines.append(encoded)
    return ("\n".join(lines) + "\n").encode("utf-8")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("size", type=int)
    args = parser.parse_args()
    args.output.write_bytes(corpus(args.size))
