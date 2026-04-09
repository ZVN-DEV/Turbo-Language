# game-of-life

Conway's Game of Life implemented over a string-backed grid. Places a Glider,
a Blinker, and a Block onto a 20x10 board, then runs 8 generations of the
standard B3/S23 rules and renders each frame as ASCII art.

## Turbo features shown

- `const` declarations for `WIDTH`, `HEIGHT`, `GENERATIONS`
- Functions with `i64` and `str` parameters and return types
- Strings used as a flat 2D grid via `char_at` and string concatenation
- `for x in 0..WIDTH` numeric ranges and nested `for` loops
- Pure-functional grid updates (each `step` returns a new grid string)
- String interpolation in `print` (`"  Gen {gen}  Alive: {alive}"`)
- Helper functions for pattern placement (`place_glider`, `place_blinker`, ...)

## Run

```bash
turbolang run examples/game-of-life/main.tb
```

## Expected output

```
==================================================
  Conway's Game of Life -- Turbo Edition
==================================================

  Patterns: Glider, Blinker, Block
  Grid: 20x10  Generations: 8

+----------------------+  Gen 0  Alive: 9
| .#..................  |
| ..#.................  |
| ###.................  |
| ....................  |
| ..........###.......  |
| ....................  |
| ................##..  |
| ................##..  |
| ....................  |
| ....................  |
+----------------------+
...
==================================================
  Done! N cells alive after 8 generations
==================================================
```

## Caveats

- `set_cell` rebuilds the entire grid string on every cell write, so a larger
  board or more generations gets quadratic-ish quickly. This is intentional —
  the example is about expressiveness, not throughput.
- The grid is fixed-size; cells off the edge are treated as dead.
