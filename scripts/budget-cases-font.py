"""A variable font whose glyphs need more scratch than a renderer's budget,
with a .notdef that has ink.

Writes baud-font/tests/var/budget-cases.ttf. A renderer draws a varied
glyph through scratch, one point a slot, and caps the scratch at
HarfBuzz's HB_GLYF_MAX_POINTS (200,000); a glyph past the cap cannot be
drawn, and TUS 5.3 leaves such a character "some placeholder", so the
cell must show the next face's glyph or this face's .notdef, never a
blank. ink-cases.ttf carries such a glyph but its .notdef is empty, so a
placeholder drawn from it cannot be told from a blank; this face's is a
box.

  .notdef       a box with a hole, the placeholder
  a             a box whose top and right edges move with wght, so the
                face carries gvar and is read at a variation (M maps here
                too, because a renderer takes its cell width from M)
  many, mid, heavy
                heavy is 40 mid, each 50 many, each 120 points: 240,000
                points flattened in 2,041 visits, past the budget at a
                variation; at the default instance it streams, with no
                scratch, and draws
  heavym        heavy, then a placed by matched points (a's point 0 on
                heavy's point 0): drawn through scratch at every
                instance, so past the budget at the default one too
  colrheavy     COLR v0, empty itself, paints heavy and a: its layers
                cannot draw at a variation and its own outline is empty
  colrpast      COLR v0 painting heavy and a, whose own outline is 40 mid
                as heavy's is: at a variation neither its layers nor its
                outline can draw
  full          a simple glyph of 65,536 points, the most a 16-bit
                endPtsOfContours can number, all on one spot so the glyph
                takes under a kilobyte: at a variation it needs 65,540,
                the most any simple glyph can, which a test of the need
                that skips simple glyphs must hold to; unmapped
  colrink       COLR v0 painting heavy and a over an outline of its own,
                a's box: at a variation its layers cannot draw and its
                outline can, so it draws that outline as text
  reflayer      a placed by matched points first, then a: refused at
                every instance
  colrref       COLR v0 painting reflayer and a over a's box: its layers
                cannot draw at any instance and its outline can

cmap: a and M to a, H to heavy, K to heavym, D to colrheavy, E to
colrpast, U+E123 to colrink, U+E124 to colrref; JetBrains Mono maps
neither of the last two. Axes: wght 100-400-900, no avar; only a varies,
by even deltas at the extremes. lsb is xMin. Deterministic: the head
timestamps are fixed. baud's own font, under the repository's licence
(baud-font/tests/licences.rs lists it).

    nix shell --impure --expr 'with import (builtins.getFlake "nixpkgs") {}; \
      python3.withPackages (p: [ p.fonttools ])' -c python3 scripts/budget-cases-font.py
"""
import os
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib.tables.TupleVariation import TupleVariation
from fontTools.ttLib.tables import ttProgram
from fontTools.ttLib.tables._g_l_y_f import Glyph, GlyphComponent, GlyphCoordinates

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
OUT = os.path.join(ROOT, "baud-font/tests/var/budget-cases.ttf")

order = [".notdef", "a", "many", "mid", "heavy", "heavym", "colrheavy", "colrpast", "full",
         "colrink", "reflayer", "colrref"]
cmap = {0x61: "a", 0x4D: "a", 0x48: "heavy", 0x4B: "heavym", 0x44: "colrheavy",
        0x45: "colrpast", 0xE123: "colrink", 0xE124: "colrref"}
fb = FontBuilder(1000, isTTF=True)
fb.setupGlyphOrder(order)
fb.setupCharacterMap(cmap)


def rect(pen, x0, y0, x1, y1):
    """A rectangle, points bottom-left, bottom-right, top-right, top-left."""
    pen.moveTo((x0, y0))
    pen.lineTo((x1, y0))
    pen.lineTo((x1, y1))
    pen.lineTo((x0, y1))
    pen.closePath()


def box(x0, y0, x1, y1):
    pen = TTGlyphPen(None)
    rect(pen, x0, y0, x1, y1)
    return pen.glyph()


glyphs = {}
pen = TTGlyphPen(None)
rect(pen, 50, 0, 550, 700)
# The hole runs the other way round.
pen.moveTo((150, 100))
pen.lineTo((150, 600))
pen.lineTo((450, 600))
pen.lineTo((450, 100))
pen.closePath()
glyphs[".notdef"] = pen.glyph()
glyphs["a"] = box(100, 0, 500, 500)

# 120 points: thirty squares of four, each its own contour.
pen = TTGlyphPen(None)
for i in range(30):
    x = 10 + (i % 10) * 50
    y = 10 + (i // 10) * 50
    rect(pen, x, y, x + 30, y + 30)
glyphs["many"] = pen.glyph()


def component(name, place=(0, 0)):
    c = GlyphComponent()
    c.glyphName = name
    if isinstance(place, str):
        c.firstPt, c.secondPt = (int(v) for v in place.split("/"))
    else:
        c.x, c.y = place
    c.flags = 0
    return c


def composite(parts):
    g = TTGlyphPen(None).glyph()
    g.numberOfContours = -1
    g.components = [component(*p) if isinstance(p, tuple) else component(p) for p in parts]
    return g


glyphs["mid"] = composite(["many"] * 50)
glyphs["heavy"] = composite(["mid"] * 40)
glyphs["heavym"] = composite(["heavy", ("a", "0/0")])
glyphs["colrheavy"] = TTGlyphPen(None).glyph()
glyphs["colrpast"] = composite(["mid"] * 40)
# Built by hand: a pen drops a point that repeats the one before it.
full = Glyph()
full.numberOfContours = 1
full.coordinates = GlyphCoordinates([(300, 300)] * 65_536)
full.flags = bytearray([1] * 65_536)
full.endPtsOfContours = [65_535]
full.program = ttProgram.Program()
full.program.fromBytecode(b"")
glyphs["full"] = full
assert glyphs["full"].endPtsOfContours == [65_535], "65,536 points"
glyphs["colrink"] = box(100, 0, 500, 500)
glyphs["reflayer"] = composite([("a", "0/0"), "a"])
glyphs["colrref"] = box(100, 0, 500, 500)

# fontTools cannot bound a composite whose first component is placed by
# points (recalcBounds raises IndexError), so reflayer's bounds, a's box,
# are written by hand.
fb.setupGlyf(glyphs, calcGlyphBounds=False)
glyf = fb.font["glyf"]
for n in order:
    if n == "reflayer":
        g = glyf[n]
        g.xMin, g.yMin, g.xMax, g.yMax = 100, 0, 500, 500
        continue
    glyf[n].recalcBounds(glyf)
fb.setupHorizontalMetrics({n: (600, getattr(glyf[n], "xMin", 0)) for n in order})
fb.setupHorizontalHeader(ascent=900, descent=-200)
fb.setupNameTable({"familyName": "baud budget cases", "styleName": "Regular"})
fb.setupOS2()
fb.setupPost()
fb.setupHead(unitsPerEm=1000, created=0, modified=0)
fb.setupFvar(axes=[("wght", 100, 400, 900, "Weight")], instances=[])

W = "wght"
Z = (0, 0)
fb.setupGvar({
    # Top edge up and right edge out at 900; down and in at 100. Then four
    # still phantoms.
    "a": [TupleVariation({W: (0, 1, 1)}, [Z, (40, 0), (40, 100), (0, 100), Z, Z, Z, Z]),
          TupleVariation({W: (-1, -1, 0)}, [Z, (-20, 0), (-20, -80), (0, -80), Z, Z, Z, Z])],
})
fb.setupCOLR({
    "colrheavy": [("heavy", 0), ("a", 1)],
    "colrpast": [("heavy", 0), ("a", 1)],
    "colrink": [("heavy", 0), ("a", 1)],
    "colrref": [("reflayer", 0), ("a", 1)],
})
fb.setupCPAL([[(1.0, 0.0, 0.0, 1.0), (0.0, 0.0, 1.0, 1.0)]])
# heavy's 240,000 points do not fit maxp's 16-bit maxCompositePoints, so
# the totals are written by hand, saturated, rather than recomputed.
fb.font.recalcBBoxes = False
maxp = fb.font["maxp"]
maxp.maxPoints, maxp.maxContours = 65535, 30
maxp.maxCompositePoints, maxp.maxCompositeContours = 65535, 65535
maxp.maxComponentElements, maxp.maxComponentDepth = 50, 3
head = fb.font["head"]
head.xMin, head.yMin, head.xMax, head.yMax = 0, 0, 600, 700
fb.font.save(OUT)
print(OUT)
