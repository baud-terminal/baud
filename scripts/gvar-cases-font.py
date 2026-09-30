"""A variable font built to reach what the real fixture fonts never do.

Writes baud-font/tests/var/gvar-cases.ttf. JetBrains Mono and Fira Code
have no left-phantom delta, no intermediate region, one axis, and no
composite with its own variation data under USE_MY_METRICS; this font has
all of those, so HarfBuzz's drawing of it (scripts/gvar-oracle.py) decides
the rules the real fonts leave untested:

  sq    one contour, a quadratic bump; a tuple over every point that moves
        the left phantom (the outline shifts), a sparse tuple (inference),
        an intermediate region, a two-axis peak, a one-point sparse tuple
  two   two contours, the first with a run of off-curve points; sparse
        tuples with one referenced point per contour, one whose referenced
        neighbours share an x, three sparse tuples that score together, and
        an invalid intermediate region
  one   a single point, which draws nothing
  comp  sq and two as components, scaled and offset, sq under
        USE_MY_METRICS; the composite's own tuple moves both offsets and
        its left phantom (which USE_MY_METRICS then overrides)
  nest  comp and a rotated sq; a sparse tuple on one component offset
  empty no contours; its phantoms vary
  fan64, edge2049, edge2050
        HarfBuzz's graph-edge bound (HB_MAX_GRAPH_EDGE_COUNT, 2048, checked
        before each visit is counted): fan64 is 64 empty components;
        edge2049 is 31 of it, 32 empties and sq -- 2,049 glyphs visited,
        which HarfBuzz draws -- and edge2050 one empty more, which it refuses.
        The square is what makes the refusal visible.

Axes: wght 100-400-900 and wdth 50-100-200, no avar. Deterministic: the
head timestamps are fixed.

    nix shell --impure --expr 'with import (builtins.getFlake "nixpkgs") {}; \
      python3.withPackages (p: [ p.fonttools ])' -c python3 scripts/gvar-cases-font.py
"""
import os
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib.tables.TupleVariation import TupleVariation
from fontTools.ttLib.tables import ttProgram
from fontTools.ttLib.tables._g_l_y_f import Glyph, GlyphComponent, GlyphCoordinates

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
OUT = os.path.join(ROOT, "baud-font/tests/var/gvar-cases.ttf")

order = [".notdef", "sq", "two", "one", "comp", "nest", "empty", "fan64", "edge2049", "edge2050"]
fb = FontBuilder(1000, isTTF=True)
fb.setupGlyphOrder(order)
fb.setupCharacterMap({0x41 + i: n for i, n in enumerate(order[1:])})

glyphs = {}
pen = TTGlyphPen(None)
glyphs[".notdef"] = pen.glyph()

pen = TTGlyphPen(None)
pen.moveTo((100, 0))
pen.lineTo((500, 0))
pen.qCurveTo((600, 250), (500, 500))
pen.lineTo((100, 500))
pen.closePath()
glyphs["sq"] = pen.glyph()

pen = TTGlyphPen(None)
# One on-curve point, then three off-curve ones: implied midpoints. It
# starts on the curve because a contour that starts on two off-curve
# points is where baud's emission (ttf-parser's) and HarfBuzz's (FreeType's)
# pick different start points for the same closed shape.
pen.moveTo((100, 300))
pen.qCurveTo((300, 100), (400, 300), (300, 500), (100, 300))
pen.closePath()
pen.moveTo((500, 0))
pen.lineTo((700, 0))
pen.lineTo((700, 400))
pen.closePath()
glyphs["two"] = pen.glyph()

# A pen drops a one-point contour, so the glyph is spelled out.
one = Glyph()
one.numberOfContours = 1
one.coordinates = GlyphCoordinates([(250, 250)])
one.endPtsOfContours = [0]
one.flags = bytearray([1])
one.program = ttProgram.Program()
one.program.fromBytecode(b"")
glyphs["one"] = one

pen = TTGlyphPen(None)
glyphs["empty"] = pen.glyph()


def component(name, x, y, transform=None, flags=0):
    c = GlyphComponent()
    c.glyphName = name
    c.x, c.y = x, y
    c.flags = flags
    if transform is not None:
        c.transform = transform
    return c


USE_MY_METRICS = 0x0200
pen = TTGlyphPen(None)
glyphs["comp"] = pen.glyph()
glyphs["comp"].numberOfContours = -1
glyphs["comp"].components = [
    component("sq", 50, 20, flags=USE_MY_METRICS),
    component("two", 300, 100, [[0.5, 0], [0, 0.5]]),
]
pen = TTGlyphPen(None)
glyphs["nest"] = pen.glyph()
glyphs["nest"].numberOfContours = -1
glyphs["nest"].components = [
    component("comp", 10, 10),
    component("sq", 0, 600, [[0.8, 0.2], [-0.2, 0.8]]),
]

for name, fans, empties in (("fan64", 0, 64), ("edge2049", 31, 32), ("edge2050", 31, 33)):
    pen = TTGlyphPen(None)
    glyphs[name] = pen.glyph()
    glyphs[name].numberOfContours = -1
    comps = [component("fan64", 0, 0) for _ in range(fans)]
    comps += [component("empty", 0, 0) for _ in range(empties)]
    if name != "fan64":
        comps.append(component("sq", 0, 0))
    glyphs[name].components = comps

fb.setupGlyf(glyphs)
glyf = fb.font["glyf"]
for n in order:
    glyf[n].recalcBounds(glyf)
# lsb is xMin, so HarfBuzz's absolute shift is the left phantom's delta.
fb.setupHorizontalMetrics({n: (900, getattr(glyf[n], "xMin", 0)) for n in order})
fb.setupHorizontalHeader(ascent=900, descent=-100)
fb.setupNameTable({"familyName": "baud gvar cases", "styleName": "Regular"})
fb.setupOS2()
fb.setupPost()
fb.setupHead(unitsPerEm=1000, created=0, modified=0)
fb.setupFvar(
    axes=[("wght", 100, 400, 900, "Weight"), ("wdth", 50, 100, 200, "Width")],
    instances=[],
)

W, D = "wght", "wdth"


def tv(axes, coords):
    return TupleVariation(axes, coords)


variations = {
    "sq": [
        # 5 points, then left, right, top, bottom phantoms.
        tv({W: (0, 1, 1)}, [(10, 0), (20, 5), (30, -10), (0, 15), (-5, 5), (7, 0), (25, 0), (0, 0), (0, 0)]),
        tv({W: (-1, -1, 0)}, [(-20, -10), None, None, (15, 20), None, (-6, 0), None, None, None]),
        tv({W: (0.2, 0.5, 0.8)}, [None, None, (40, 40), None, None, None, None, None, None]),
        tv({W: (0, 1, 1), D: (0, 0.5, 1)}, [(3, 3), (3, 3), (3, 3), (3, 3), (3, 3), (0, 0), (0, 0), (0, 0), (0, 0)]),
        tv({D: (-1, -1, 0)}, [None, (0, -30), None, None, None, None, None, None, None]),
    ],
    "two": [
        # 4 + 3 points, then the phantoms.
        # Two points of the first contour move apart, so its shape changes:
        # a later sparse tuple that read these positions instead of the
        # defaults would infer differently.
        tv({W: (0, 1, 1)}, [(12, 0), None, (-10, 6), None, None, (0, 18), None, None, None, None, None]),
        tv({D: (0, 1, 1)}, [None, None, (-9, 4), None, (5, 5), None, None, None, None, None, None]),
        # Points 5 and 6 share an x (700) and are both referenced: point 4,
        # between them round the contour, takes the equal-position rule.
        tv({W: (-1, -1, 0)}, [None, None, None, None, None, (3, 0), (4, 0), None, None, None, None]),
        # A third sparse tuple scoring beside the first two (wght and wdth
        # both above default): the positions inference reads are the
        # defaults, captured once, not what the earlier flushes left.
        tv({W: (0, 1, 1), D: (0, 1, 1)}, [None, (-7, 11), None, (6, -4), None, None, (5, 9), None, None, None, None]),
        # An invalid intermediate region (it straddles 0 with a nonzero
        # peak): the axis is ignored, except that a coordinate of 0 still
        # ends the tuple first.
        tv({D: (-0.5, 0.5, 1)}, [(2, 2), (2, 2), (2, 2), (2, 2), (2, 2), (2, 2), (2, 2), (0, 0), (0, 0), (0, 0), (0, 0)]),
    ],
    "comp": [
        # 2 component offsets, then the phantoms.
        tv({W: (0, 1, 1)}, [(30, 10), (-15, 25), (12, 0), (40, 0), (0, 0), (0, 0)]),
    ],
    "nest": [
        tv({D: (0, 1, 1)}, [None, (20, -20), None, None, None, None]),
    ],
    "one": [
        tv({W: (0, 1, 1)}, [(50, 50), (9, 0), (0, 0), (0, 0), (0, 0)]),
    ],
    "empty": [
        tv({W: (0, 1, 1)}, [(5, 0), (60, 0), (0, 0), (0, 0)]),
    ],
}
fb.setupGvar(variations)
fb.font.save(OUT)
print(OUT)
