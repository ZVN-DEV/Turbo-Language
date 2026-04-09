# simple-script

A text-statistics analyzer that takes a built-in sample paragraph and reports
character / word / line counts, unique-word count, longest word, and a top-5
word frequency table. Also runs short demos of string operations and array
methods (`map`, `filter`, `reduce`).

## Turbo features shown

- Triple-quoted string literals for multi-line input
- Pipe operator (`text |> count_chars`, `trimmed |> upper`)
- `hashmap()` + `hashmap_set` / `hashmap_get` / `hashmap_has` / `hashmap_keys`
- String builtins: `trim`, `lower`, `upper`, `replace`, `contains`,
  `starts_with`, `ends_with`, `len`
- `split` / `join` for word and line tokenization
- Closure literals: `|x: i64| -> i64 { x * 2 }`
- Array methods: `nums.map(...)`, `nums.filter(...)`, `reduce(...)`, `nums.len()`
- String interpolation in `print` (`"  Words:             {word_count}"`)

## Run

```bash
turbolang run examples/simple-script/main.tb
```

## Expected output

```
========================================
      Turbo Text Analyzer
========================================

Analyzing sample text...

----------------------------------------
  Statistics
----------------------------------------
  Characters:        446
  Words:             64
  Lines:             1
  Unique words:      52
  Longest word:      "programming" (11 chars)
  Total word length: 376

----------------------------------------
  Top Repeated Words
----------------------------------------
  1. "and" (4x)
  2. "turbo" (4x)
  3. "the" (3x)
  4. "a" (2x)
  5. "language" (2x)

----------------------------------------
  String Operations Demo
----------------------------------------
  Original:     "  Hello, Turbo World!  "
  Trimmed:      "Hello, Turbo World!"
  Upper:        "HELLO, TURBO WORLD!"
  Lower:        "hello, turbo world!"
  Replaced:     "Hello, Turbo Language!"
  Contains 'Turbo':  true
  Starts w/ 'Hello': true
  Ends with '!':     true

----------------------------------------
  Array Operations Demo
----------------------------------------
  Numbers:        3, 1, 4, 1, 5, 9, 2, 6, 5, 3
  Sum:            39
  Doubled:        6, 2, 8, 2, 10, 18, 4, 12, 10, 6
  Doubled count:  10
  Filtered (>4):  5, 9, 6, 5
  Filtered count: 4
  Total count:    10

========================================
  Analysis complete!
========================================
```

## Caveats

- The frequency table caps at "10+" because counts are stored as small
  string buckets, not as integers. Adequate for the demo, not for prose.
- The "Doubled" / "Filtered" display rows in the array demo print
  pre-formatted strings rather than `to_str` of the computed arrays —
  the computations themselves are real, and the `Doubled count` /
  `Filtered count` lines are derived from the actual array values via
  `.len()`.
