"""Round 5 (2026-10-24): letters kept apart. One edit, to dalet.

As round 1 left them, ד (U+05D3) and ז (U+05D6) were the same shape at text
sizes: a top bar over one leg. Round 1 had evened the wide letters to 410-422
units (dalet 458 -> 418, design.py EDITS) and widened zayin's head 336 -> 380,
so the roofs were 418 and 380 wide, and dalet's leg (335-425) sat 64 units
right of zayin's (271-361; Bold 60), under roofs of the same centre. Measured
at 14 px through baud's rasteriser, the two legs' offsets under their roofs
differed by 0.80-0.92 px over the four styles, and 'זו' read 'דו'.

The rule is Shulchan Arukh, Orach Chayim 36:1, "no letter should resemble
another". The scribal rules name what sets each letter apart: dalet's roof is
long, its corner square "so that it doesn't look like a resh", with "a good
heel" behind the leg; zayin's head "should pass over on both sides so it
doesn't resemble a vav". The Unicode chart's glyphs are "not prescriptive".

So dalet's leg moves right until the roof passes it by HEEL units: the leg
stands at the roof's right end, as the scribal dalet's does, and the heel is
halved, from the 84 units (0.93 of a stem) round 1 left to half a Regular
stem, kept for the resh distinction (in the 14 px renders a heel of 15 or 0
made ד read as ר). The roof keeps its width, so the wide letters stay even.
Zayin is untouched: narrowing its head (Z40, 300 units) would change
narrow-letter variant A, the interim pick for the owner's open call 1a, and
Z40 is none of that call's options A-E. The heel is Jev's lean (H45:
probability 0.83 and 0.81 in the two orders, confidence 0.79 and 0.76, under
the 0.9 bar on the second ask, so the lean is taken; re-asked with dalet's
nearness to vav: probability 0.70 and 0.81, confidence 0.62 and 0.76); note
AC 4d records the options.
At 14 px the legs now stand 1.41-1.51 px apart
(`baud-font/tests/sister_letters.rs`).

The edit runs in the frame build.py's letter_rec_r1 edits in (centred on
x = 300, 630 tall), after design.py's, so Bold reads its own leg and both
italics shear the result.
"""
from geom import FlatPen, runs_at_y

DALET = 0x05D3
HEEL = 45.0        # units the roof passes the leg on the right (Regular stem 90, Bold 125)


def _polys(rec):
    p = FlatPen(None, steps=24)
    for op, args in rec:
        getattr(p, op)(*args)
    return p.polys


def point_map(u, rec):
    """The map round 5 applies to letter u's outline and its anchor points, or
    None. Dalet: every point of the leg, read at y = 300, and the roof's
    underside where it meets the leg (y <= 560) moves right by the same dx."""
    if u != DALET:
        return None
    sl, sr = runs_at_y(_polys(rec), 300)[0]
    right = max(x for _, args in rec for x, _ in args)
    dx = right - HEEL - sr

    def f(x, y):
        if sl - 2 <= x <= sr + 2 and y <= 560:
            return (x + dx, y)
        return (x, y)
    return f
