#!/usr/bin/env python3
"""The set-depth score behind `store_stride` in `baud-domain/src/cache.rs`.

The grid's row pitch is the provisioned width's footprint in cache lines,
made odd, stepped at least three sets clear of a wrap of every L1 way from
4 to 32 KB. This prints, for each line size and each width a caller might
provision, the score of that pitch and of the next few odd totals past it:
over the ways, the widths up to the base and every height to 150 rows,
wherever the dense live-width layout keeps every set within eight lines,
the configurations where the pitch does not, and the deepest set it makes.

The score is what an earlier version of this table was *chosen* by, and it
chose wide: 105 lines for a 200-column grid on a 128-byte line, scoring 3
of 721 against the dense 13 lines' 27. Measured, the two are equal at no
scrollback and the wide one is 0.56x at 2,000 lines of it, because with the
grid as the ring every line of history costs a row of the pitch and 27 MB
of ring is past L2. So the rule prefers dense and the score is kept as the
instrument that says what dense gives up -- and as the thing that agrees
with the measurement about the one pitch that is genuinely bad, a step of
one set a row (p1 on the `MAX_COLS` rows below), which `CLEAR` refuses.
`the_dense_pitch_scores_what_the_search_said_and_the_one_past_a_way_is_refused`
pins three of these scores so the rule cannot drift from them.
"""
import math

CELL = 8
WAYS = (4096, 8192, 16384, 32768)
WIDTHS = (80, 200, 400)
ROWS = range(1, 151)
BASES = [80, 120, 160, 200, 240, 320, 400, 512, 640, 800, 1024, 1280, 1600, 2048]


def deepest(pitch, line, way, cols, rows):
    sets = way // line
    footprint = math.ceil(cols * CELL / line)
    depth = [0] * sets
    for r in range(rows):
        s = (r * pitch) // line
        for j in range(footprint):
            depth[(s + j) % sets] += 1
    return max(depth)


for line in (64, 128, 256):
    for base_cols in BASES:
        base = math.ceil(base_cols * CELL / line)
        widths = [w for w in WIDTHS if w <= base_cols] or [base_cols]
        fits = [
            (w, c, r)
            for w in WAYS
            for c in widths
            for r in ROWS
            if deepest(c * CELL, line, w, c, r) <= 8
        ]
        out = []
        for pad in range(0, 40):
            lines = base + pad
            if lines % 2 == 0:
                continue
            p = lines * line
            over = 0
            deep = 0
            for (w, c, r) in fits:
                g = deepest(p, line, w, c, r)
                deep = max(deep, g)
                over += g > 8
            out.append((pad, over, deep))
            if len(out) >= 6:
                break
        print(
            f"line {line:3} base {base_cols:4} ({base:3} lines) fits {len(fits):3}: "
            + " ".join(f"p{pad}={over}/{deep}" for (pad, over, deep) in out),
            flush=True,
        )
