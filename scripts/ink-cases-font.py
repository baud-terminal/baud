"""A variable font with no GPOS, built so the shaper's fallback mark
placement reads glyph boxes at a variation.

Writes baud-font/tests/var/ink-cases.ttf. A face without GPOS places its
marks geometrically against the base's ink box and the mark's own
(HarfBuzz's hb-ot-shape-fallback, which baud follows), so a varied face
must hand that placement its varied boxes. Every committed variable font
carries GPOS, so none reaches the fallback; this one does:

  a, o          bases whose top and side edges move with wght
  grave, acute, macron, ring
                marks above (combining class 230) that widen and rise
  dotbelow      a mark below (220) that sinks
  cedilla       a mark attached below (202)
  many, mid, heavy
                heavy is 40 mid, each 50 many, each 120 points: 240,000
                points flattened in 2,041 visits -- inside HarfBuzz's edge
                bound and past HB_GLYF_MAX_POINTS (200,000), so HarfBuzz
                refuses it and a renderer that caps its scratch there
                refuses it too, and counts it
  colr, colrheavy
                COLR v0 glyphs, empty themselves: colr paints a and acute,
                colrheavy paints heavy and a, so a layered glyph at a
                variation is drawn from varied layers, and one whose layer
                is past the budget is refused whole

Every delta is even and the samples sit at wght 100, 250, 650 and 900 --
scalars of -1, -0.5, 0.5 and 1 -- so every varied coordinate is a whole
unit and no rounding rule is under test. No delta moves a left phantom,
so HarfBuzz's shift is zero. lsb is xMin. Axes: wght 100-400-900, no avar.
Deterministic: the head timestamps are fixed. baud's own font, under the
repository's licence (baud-font/tests/licences.rs lists it).

    nix shell --impure --expr 'with import (builtins.getFlake "nixpkgs") {}; \
      python3.withPackages (p: [ p.fonttools ])' -c python3 scripts/ink-cases-font.py
"""
import os
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib.tables.TupleVariation import TupleVariation
from fontTools.ttLib.tables._g_l_y_f import GlyphComponent

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
OUT = os.path.join(ROOT, "baud-font/tests/var/ink-cases.ttf")

order = [".notdef", "a", "o", "grave", "acute", "macron", "ring", "dotbelow",
         "cedilla", "many", "mid", "heavy", "colr", "colrheavy"]
# M maps too, to o, because a renderer takes its cell width from M.
cmap = {0x61: "a", 0x6F: "o", 0x300: "grave", 0x301: "acute", 0x304: "macron",
        0x30A: "ring", 0x323: "dotbelow", 0x327: "cedilla", 0x48: "heavy", 0x4D: "o",
        0x43: "colr", 0x44: "colrheavy"}
fb = FontBuilder(1000, isTTF=True)
fb.setupGlyphOrder(order)
fb.setupCharacterMap(cmap)


def box(x0, y0, x1, y1):
    """A rectangle, points bottom-left, bottom-right, top-right, top-left."""
    pen = TTGlyphPen(None)
    pen.moveTo((x0, y0))
    pen.lineTo((x1, y0))
    pen.lineTo((x1, y1))
    pen.lineTo((x0, y1))
    pen.closePath()
    return pen.glyph()


glyphs = {".notdef": TTGlyphPen(None).glyph()}
glyphs["a"] = box(100, 0, 500, 500)
glyphs["o"] = box(50, 0, 550, 400)
glyphs["grave"] = box(220, 560, 320, 640)
glyphs["acute"] = box(250, 550, 350, 650)
glyphs["macron"] = box(150, 600, 450, 640)
glyphs["ring"] = box(240, 580, 360, 700)
glyphs["dotbelow"] = box(260, -150, 340, -70)
glyphs["cedilla"] = box(280, -180, 330, 0)

# 120 points: thirty squares of four, each its own contour.
pen = TTGlyphPen(None)
for i in range(30):
    x = 10 + (i % 10) * 50
    y = 10 + (i // 10) * 50
    pen.moveTo((x, y))
    pen.lineTo((x + 30, y))
    pen.lineTo((x + 30, y + 30))
    pen.lineTo((x, y + 30))
    pen.closePath()
glyphs["many"] = pen.glyph()


def composite(parts):
    g = TTGlyphPen(None).glyph()
    g.numberOfContours = -1
    g.components = []
    for name in parts:
        c = GlyphComponent()
        c.glyphName = name
        c.x, c.y = 0, 0
        c.flags = 0
        g.components.append(c)
    return g


glyphs["mid"] = composite(["many"] * 50)
glyphs["heavy"] = composite(["mid"] * 40)
glyphs["colr"] = TTGlyphPen(None).glyph()
glyphs["colrheavy"] = TTGlyphPen(None).glyph()

fb.setupGlyf(glyphs)
glyf = fb.font["glyf"]
for n in order:
    glyf[n].recalcBounds(glyf)
marks = {"grave", "acute", "macron", "ring", "dotbelow", "cedilla"}
fb.setupHorizontalMetrics(
    {n: (0 if n in marks else 600, getattr(glyf[n], "xMin", 0)) for n in order}
)
fb.setupHorizontalHeader(ascent=900, descent=-200)
fb.setupNameTable({"familyName": "baud ink cases", "styleName": "Regular"})
fb.setupOS2()
fb.setupPost()
fb.setupHead(unitsPerEm=1000, created=0, modified=0)
fb.setupFvar(axes=[("wght", 100, 400, 900, "Weight")], instances=[])

W = "wght"
UP = {W: (0, 1, 1)}
DOWN = {W: (-1, -1, 0)}
Z = (0, 0)


def tv(axes, four):
    """Deltas for a box's four points, then four still phantoms."""
    return TupleVariation(axes, list(four) + [Z, Z, Z, Z])


variations = {
    # Top edge up and right edge out at 900; down and in at 100.
    "a": [tv(UP, [Z, (40, 0), (40, 100), (0, 100)]),
          tv(DOWN, [Z, (-20, 0), (-20, -80), (0, -80)])],
    "o": [tv(UP, [(-40, 0), Z, (0, 60), (-40, 60)]),
          tv(DOWN, [(20, 0), Z, (0, -40), (20, -40)])],
    "grave": [tv(UP, [Z, (20, 0), (20, 0), Z])],
    "acute": [tv(UP, [Z, (60, 0), (60, 40), (0, 40)]),
              tv(DOWN, [Z, (-40, 0), (-40, -20), (0, -20)])],
    "macron": [tv(UP, [(-50, 0), (50, 0), (50, 20), (-50, 20)])],
    "ring": [tv(UP, [(0, 30), (40, 30), (40, 70), (0, 70)])],
    "dotbelow": [tv(UP, [(0, -40), (20, -40), (20, 0), Z]),
                 tv(DOWN, [(0, 20), (-10, 20), (-10, 0), Z])],
    "cedilla": [tv(UP, [(0, -60), (30, -60), (30, 0), Z])],
}
fb.setupGvar(variations)
fb.setupCOLR({"colr": [("a", 0), ("acute", 1)], "colrheavy": [("heavy", 0), ("a", 1)]})
fb.setupCPAL([[(1.0, 0.0, 0.0, 1.0), (0.0, 0.0, 1.0, 1.0)]])
# heavy's 240,000 points do not fit maxp's 16-bit maxCompositePoints, so
# the totals are written by hand, saturated, rather than recomputed.
fb.font.recalcBBoxes = False
maxp = fb.font["maxp"]
maxp.maxPoints, maxp.maxContours = 120, 30
maxp.maxCompositePoints, maxp.maxCompositeContours = 65535, 65535
maxp.maxComponentElements, maxp.maxComponentDepth = 50, 2
head = fb.font["head"]
head.xMin, head.yMin, head.xMax, head.yMax = 0, -240, 600, 760
fb.font.save(OUT)
print(OUT)
