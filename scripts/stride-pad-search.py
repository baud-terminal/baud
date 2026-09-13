#!/usr/bin/env python3
"""The search behind `PAD_LINES` in `baud-domain/src/cache.rs`.

The grid's row pitch is `MAX_COLS` cells plus a pad of `p` cache lines, with
the total an odd number of lines so it is a multiple of no power-of-two way
or page. This picks `p`: for each line size, every odd-total pad up to one
16 KB period is laid out against every L1 way size from 4 to 32 KB, three
widths and every height to 150 rows, and scored by how many of the
configurations where the dense live-width layout keeps every set within
eight lines the padded layout does not. The winner per line size is the
table; `the_row_starts_spread_wherever_the_dense_grid_fits` pins its
violation profile so the table cannot drift from the search.

The one-line pad this repo shipped first stacks thirteen rows of a
200-column screen into each set of a 16 KB way and measured 10% down on a
scrolling payload; any step of three sets or more recovered it. This search
is that measurement generalised to every geometry baud might run on.
"""
import math

MAX_COLS, CELL = 2048, 8
WAYS, WIDTHS, ROWS = (4096, 8192, 16384, 32768), (80, 200, 400), range(1, 151)


def deepest(pitch, line, way, cols, rows):
    sets, per_row = way // line, math.ceil(cols * CELL / line)
    depth = [0] * sets
    for r in range(rows):
        start = r * pitch // line
        for j in range(per_row):
            depth[(start + j) % sets] += 1
    return max(depth)


for line in (64, 128, 256):
    base = math.ceil(MAX_COLS * CELL / line)
    fits = [(w, c, r) for w in WAYS for c in WIDTHS for r in ROWS
            if deepest(c * CELL, line, w, c, r) <= 8]
    scored = []
    for pad in range(1, 16384 // line + 1):
        lines = base + pad
        if lines % 2 == 0:
            continue
        pitch = lines * line
        depths = [deepest(pitch, line, w, c, r) for (w, c, r) in fits]
        scored.append((sum(d > 8 for d in depths), max(depths), pad))
    scored.sort()
    v, d, pad = scored[0]
    print(f"line {line:3}: pad {pad:3} lines -> {base + pad} lines, "
          f"{(base + pad) * line // CELL} cells; {v} of {len(fits)} configurations "
          f"over eight deep, deepest {d}; runners-up {scored[1:4]}")
