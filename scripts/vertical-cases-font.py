"""A variable font with vertical metrics whose composites place components
on the vertical phantom points, and one that places a nested composite's
first component by points.

Writes baud-font/tests/var/vertical-cases.ttf. A component placed by
matched points may name any phantom of the parent or the child
(`[[source: ms-opentype-glyf]]`: "Phantom points from the parent or the
child may be referenced"), and the vertical two are the "top origin" and
the "advance height point" (`[[source: ms-opentype-tt-instructing-glyphs]]`),
which `vmtx` places: the top origin at `yMax + tsb`, the advance height an
advance height below it (`[[source: ms-opentype-vmtx]]`).
`component-cases.ttf` has no `vmtx`, and its composites' header boxes are
all 0, so its parent's top phantom sits at the origin; this font has
`vhea` and `vmtx`, a top side bearing and an advance height of its own on
every glyph, and composites whose header yMax is not 0, so no phantom
coincides with another or with the origin.

Simple glyphs, polygons that start on the curve, (advance height, tsb):

  tri    (0,0) (400,0) (200,300), (1000, 100): top 400, bottom -600; its
         apex moves (40, 60) at wght 900
  bar    a 100 x 40 box, (800, 60): top 100, bottom -700
  dot    a 60 x 60 box at x 20, (900, 140): top 200, bottom -700; its top
         phantom moves (30, 50) at wght 900 -- a simple glyph's phantom
         takes both deltas
  short  dot's box, (1000, 120): top 180, bottom -820; the last glyph, so
         its tsb is in vmtx's trailing array

Composites, header box (0, 0, 400, 300) but where noted, each (1000, 50):
top 350, bottom -650 (p/c: the parent's and the child's point):

  pairv   bar, then bar at (200, 0); box (0, 0, 300, 40), (700, 30): top
          70, bottom -630; its own top phantom moves (40, 50) at wght 900,
          of which the gvar text applies the Y alone
  vtop    tri, dot p1/c6: the child's top phantom
  vbot    tri, dot p1/c7: the child's advance height phantom
  vcomp   tri, pairv p1/c10: a composite child's top phantom
  vshort  tri, short p1/c6: a top phantom whose tsb is in the trailing
          array
  vptop   tri, bar p5/c1: the parent's own top phantom, numbered after
          tri's three points and its left and advance phantoms; it moves
          (40, 50) at wght 900, of which the Y applies
  vpbot   tri, bar p6/c1: the parent's own advance height phantom
  vpast   tri, bar p7/c1: past the parent's four phantoms, which no text
          names
  firstm  bar p0/c0 first, then tri: a first component placed by points,
          which "cannot be used for the first component glyph"
          (`[[source: ms-opentype-glyf]]`)
  vnest   tri, then firstm at (500, 0): the same, one level down

.notdef is a 500 x 700 box at x 50, so a refused glyph's placeholder
shows: a cell drawn from it is not blank (baud-render's
tests/refused_glyph.rs).

Every glyph's lsb is its xMin, so no glyph is shifted by its left phantom.
Axis: wght 100-400-900, no avar; only the 900 side carries deltas, so 650
is half of each. Deterministic: the head timestamps are fixed. baud's own
font, under the repository's licence (baud-font/tests/licences.rs lists
it).

    nix shell --impure --expr 'with import (builtins.getFlake "nixpkgs") {}; \
      python3.withPackages (p: [ p.fonttools ])' -c python3 scripts/vertical-cases-font.py
"""
import os
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib.tables.TupleVariation import TupleVariation
from fontTools.ttLib.tables._g_l_y_f import GlyphComponent

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
OUT = os.path.join(ROOT, "baud-font/tests/var/vertical-cases.ttf")

simple = ["tri", "bar", "dot"]
composites = {
    "pairv": ([("bar", (0, 0)), ("bar", (200, 0))], (0, 0, 300, 40), (700, 30)),
    "vtop": ([("tri", (0, 0)), ("dot", "1/6")], None, None),
    "vbot": ([("tri", (0, 0)), ("dot", "1/7")], None, None),
    "vcomp": ([("tri", (0, 0)), ("pairv", "1/10")], None, None),
    "vshort": ([("tri", (0, 0)), ("short", "1/6")], None, None),
    "vptop": ([("tri", (0, 0)), ("bar", "5/1")], None, None),
    "vpbot": ([("tri", (0, 0)), ("bar", "6/1")], None, None),
    "vpast": ([("tri", (0, 0)), ("bar", "7/1")], None, None),
    "firstm": ([("bar", "0/0"), ("tri", (0, 0))], None, None),
    "vnest": ([("tri", (0, 0)), ("firstm", (500, 0))], None, None),
}
order = [".notdef"] + simple + list(composites) + ["short"]

fb = FontBuilder(1000, isTTF=True)
fb.setupGlyphOrder(order)
fb.setupCharacterMap({0x41 + i: n for i, n in enumerate(order[1:])})


def poly(points):
    pen = TTGlyphPen(None)
    pen.moveTo(points[0])
    for p in points[1:]:
        pen.lineTo(p)
    pen.closePath()
    return pen.glyph()


def component(name, place):
    c = GlyphComponent()
    c.glyphName = name
    if isinstance(place, str):
        c.firstPt, c.secondPt = (int(v) for v in place.split("/"))
    else:
        c.x, c.y = place
    c.flags = 0
    return c


glyphs = {".notdef": poly([(50, 0), (550, 0), (550, 700), (50, 700)])}
glyphs["tri"] = poly([(0, 0), (400, 0), (200, 300)])
glyphs["bar"] = poly([(0, 0), (100, 0), (100, 40), (0, 40)])
glyphs["dot"] = poly([(20, 0), (80, 0), (80, 60), (20, 60)])
glyphs["short"] = poly([(20, 0), (80, 0), (80, 60), (20, 60)])
for name, (parts, box, _) in composites.items():
    g = TTGlyphPen(None).glyph()
    g.numberOfContours = -1
    g.components = [component(*p) for p in parts]
    g.xMin, g.yMin, g.xMax, g.yMax = box or (0, 0, 400, 300)
    glyphs[name] = g

fb.setupGlyf(glyphs, calcGlyphBounds=False)
glyf = fb.font["glyf"]
for n in [".notdef"] + simple + ["short"]:
    glyf[n].recalcBounds(glyf)
lsb = {n: getattr(glyf[n], "xMin", 0) for n in order}
fb.setupHorizontalMetrics({n: (600, lsb[n]) for n in order})
fb.setupHorizontalHeader(ascent=900, descent=-200)
vertical = {".notdef": (1000, 0), "tri": (1000, 100), "bar": (800, 60), "dot": (900, 140),
            "short": (1000, 120)}
for name, (_, _, v) in composites.items():
    vertical[name] = v or (1000, 50)
fb.setupVerticalMetrics({n: vertical[n] for n in order})
fb.setupVerticalHeader(ascent=900, descent=-100)
fb.setupNameTable({"familyName": "baud vertical cases", "styleName": "Regular"})
fb.setupOS2()
fb.setupPost()
fb.setupHead(unitsPerEm=1000, created=0, modified=0)
fb.setupFvar(axes=[("wght", 100, 400, 900, "Weight")], instances=[])

UP = {"wght": (0, 1, 1)}
Z = (0, 0)
variations = {
    # 3 points, then left, right, top, bottom.
    "tri": [TupleVariation(UP, [Z, Z, (40, 60), Z, Z, Z, Z])],
    # 4 points, then the four phantoms: the top one moves in x and y.
    "dot": [TupleVariation(UP, [Z, Z, Z, Z, Z, Z, (30, 50), Z])],
    # 2 component slots, then the four phantoms: the top one's X is ignored.
    "pairv": [TupleVariation(UP, [Z, Z, Z, Z, (40, 50), Z])],
    "vptop": [TupleVariation(UP, [Z, Z, Z, Z, (40, 50), Z])],
}
fb.setupGvar(variations)
# Composites keep the header box written above: fontTools' own point
# matching cannot reach a phantom point, which every composite here names.
fb.font.recalcBBoxes = False
head = fb.font["head"]
head.xMin, head.yMin, head.xMax, head.yMax = 0, 0, 600, 400
fb.font.save(OUT)
print(OUT)
