"""A variable font whose composites place components by matched points and
by scaled offsets.

Writes baud-font/tests/var/component-cases.ttf. None of the 2.1 million
components of the fonts on the development Mac or in the tree is anchored
by point numbers (ARGS_ARE_XY_VALUES clear) or carries
SCALED_COMPONENT_OFFSET, so this font is the only thing that reaches
either (`[[source: ms-opentype-glyf]]`, "Composite glyph description").

Simple glyphs, all polygons that start on the curve:

  tri   (0,0) (400,0) (200,300); its apex moves with wght
  bar   a 100 x 40 box; its first point moves with wght
  dot   a 60 x 60 box at x 20, lsb 20: phantoms (0,0) and (300,0)
  dot2  the same box with lsb 0: phantoms (20,0) and (320,0)
  pair  a composite child, two bars 200 apart: 8 points

Composites, by what they place (p/c: the parent's and the child's point):

  anch       bar by points, p2/c0, on tri's apex
  anchrot    bar rotated 90 degrees, p2/c2: the matrix before the alignment
  anchscale  bar scaled by half, p1/c1
  anchadv    dot, p1/c5: the child's advance phantom
  anchlsb    dot2, p1/c4: the child's left phantom, xMin - lsb = 20
  anchtop    dot, p1/c6: the child's top phantom (vertical; the face has no
             vmtx, so it is at (0, yMax))
  anchcomp   pair, p2/c5: a composite child's flattened points
  anchflag   bar by points with a scale and SCALED_COMPONENT_OFFSET, which
             the specification says is ignored without ARGS_ARE_XY_VALUES
  anchvar    as anch, with a delta on the anchored slot (ignored) and one
             on tri's offset (applied)
  nestfirst  anch first, then a bar: the inner points numbered from 0
  nestlate   a bar first, then anch: "a point number in the parent glyph"
             counts from the inner composite's own first point, where
             HarfBuzz counts across the outer glyph
  anchout    p50: past every point
  anchself   p3/c1: the parent has three points, so p3 is none of them
  anchfirst  the first component anchored, which the specification says
             must not be: refused
  scaled     bar at (100,50) scaled by half, SCALED_COMPONENT_OFFSET
  scaledrot  the same rotated 90 degrees
  scaledxy   the same scaled (0.5, 1.5)
  scaledneg  the same scaled (-1, 1)
  scaledvar  as scaled, its offset moved by a delta first
  unscaled   as scaled, UNSCALED_COMPONENT_OFFSET
  neither    as scaled, neither flag
  both       as scaled, both flags
  umm        dot2 under USE_MY_METRICS, then dot: the composite takes
             dot2's phantoms, whose left one starts at 20, so the
             shift by the left phantom's movement must follow it
  pairy      as pair, with a Y delta on its own left phantom, which the
             gvar text ignores ("For AW and LSB points, apply the net X
             delta adjustments, and ignore Y deltas")
  anchcph    pairy, p1/c8: a composite child's left phantom
  anchpadv   dot under USE_MY_METRICS, p4/c0: the parent's own advance
             phantom, numbered after tri's three points, with a Y delta on
             it (ignored), read before this component's metrics replace it
  anchptop   bar, p5/c1: the parent's own top phantom (vertical; no vmtx,
             and the header yMax is 0, so it is at (0, 0))
  anchplx    bar, p3/c1: the parent's own left phantom, with an X delta
             (applied, and the top-level shift follows it) and a Y delta
             (ignored)

Every composite's header xMin and lsb are 0, and every simple glyph's lsb
is its xMin but dot2's, so the left phantom HarfBuzz shifts a top-level
glyph by sits at x 0 except in dot2 and umm, at 20, and in anchplx at 650
and 900, whose own left phantom moves by its X delta, 20 and 40. Axes: wght 100-400-900, no avar. Deltas are even and the
oracle samples sit at scalars of -1, 0.5 and 1, so the varied coordinates
are whole or half units. Deterministic: the head timestamps are fixed.
baud's own font, under the repository's licence
(baud-font/tests/licences.rs lists it).

    nix shell --impure --expr 'with import (builtins.getFlake "nixpkgs") {}; \
      python3.withPackages (p: [ p.fonttools ])' -c python3 scripts/component-cases-font.py
"""
import os
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib.tables.TupleVariation import TupleVariation
from fontTools.ttLib.tables._g_l_y_f import GlyphComponent

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
OUT = os.path.join(ROOT, "baud-font/tests/var/component-cases.ttf")

SCALED, UNSCALED = 0x0800, 0x1000
USE_MY_METRICS = 0x0200
ROT90 = [[0, 1], [-1, 0]]

simple = ["tri", "bar", "dot", "dot2"]
composites = {
    "pair": [("bar", (0, 0)), ("bar", (200, 0))],
    "anch": [("tri", (0, 0)), ("bar", "2/0")],
    "anchrot": [("tri", (0, 0)), ("bar", "2/2", ROT90)],
    "anchscale": [("tri", (0, 0)), ("bar", "1/1", 0.5)],
    "anchadv": [("tri", (0, 0)), ("dot", "1/5")],
    "anchlsb": [("tri", (0, 0)), ("dot2", "1/4")],
    "anchtop": [("tri", (0, 0)), ("dot", "1/6")],
    "anchcomp": [("tri", (0, 0)), ("pair", "2/5")],
    "anchflag": [("tri", (0, 0)), ("bar", "2/0", 0.5, SCALED)],
    "anchvar": [("tri", (0, 0)), ("bar", "2/0")],
    "nestfirst": [("anch", (0, 0)), ("bar", (500, 0))],
    "nestlate": [("bar", (500, 0)), ("anch", (0, 0))],
    "anchout": [("tri", (0, 0)), ("bar", "50/0")],
    "anchself": [("tri", (0, 0)), ("bar", "3/1")],
    "anchfirst": [("bar", "1/0"), ("tri", (0, 0))],
    "scaled": [("tri", (0, 0)), ("bar", (100, 50), 0.5, SCALED)],
    "scaledrot": [("tri", (0, 0)), ("bar", (100, 50), ROT90, SCALED)],
    "scaledxy": [("tri", (0, 0)), ("bar", (100, 50), (0.5, 1.5), SCALED)],
    "scaledneg": [("tri", (0, 0)), ("bar", (100, 50), (-1, 1), SCALED)],
    "scaledvar": [("tri", (0, 0)), ("bar", (100, 50), 0.5, SCALED)],
    "unscaled": [("tri", (0, 0)), ("bar", (100, 50), 0.5, UNSCALED)],
    "neither": [("tri", (0, 0)), ("bar", (100, 50), 0.5)],
    "both": [("tri", (0, 0)), ("bar", (100, 50), 0.5, SCALED | UNSCALED)],
    "umm": [("dot2", (0, 0), None, USE_MY_METRICS), ("dot", (100, 0))],
    "pairy": [("bar", (0, 0)), ("bar", (200, 0))],
    "anchcph": [("tri", (0, 0)), ("pairy", "1/8")],
    "anchpadv": [("tri", (0, 0)), ("dot", "4/0", None, USE_MY_METRICS)],
    "anchptop": [("tri", (0, 0)), ("bar", "5/1")],
    "anchplx": [("tri", (0, 0)), ("bar", "3/1")],
}
order = [".notdef"] + simple + list(composites)

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


glyphs = {".notdef": TTGlyphPen(None).glyph()}
glyphs["tri"] = poly([(0, 0), (400, 0), (200, 300)])
glyphs["bar"] = poly([(0, 0), (100, 0), (100, 40), (0, 40)])
glyphs["dot"] = poly([(20, 0), (80, 0), (80, 60), (20, 60)])
glyphs["dot2"] = poly([(20, 0), (80, 0), (80, 60), (20, 60)])


def component(name, place, transform=None, flags=0):
    c = GlyphComponent()
    c.glyphName = name
    if isinstance(place, str):
        c.firstPt, c.secondPt = (int(v) for v in place.split("/"))
    else:
        c.x, c.y = place
    if isinstance(transform, (int, float)):
        transform = [[transform, 0], [0, transform]]
    elif isinstance(transform, tuple):
        transform = [[transform[0], 0], [0, transform[1]]]
    if transform is not None:
        c.transform = transform
    c.flags = flags
    return c


for name, parts in composites.items():
    g = TTGlyphPen(None).glyph()
    g.numberOfContours = -1
    g.components = [component(*p) for p in parts]
    g.xMin = g.yMin = g.xMax = g.yMax = 0
    glyphs[name] = g

fb.setupGlyf(glyphs, calcGlyphBounds=False)
glyf = fb.font["glyf"]
for n in simple:
    glyf[n].recalcBounds(glyf)
lsb = {n: getattr(glyf[n], "xMin", 0) for n in order}
lsb["dot2"] = 0
for n in composites:
    lsb[n] = 0
fb.setupHorizontalMetrics({n: (300 if n.startswith("dot") else 600, lsb[n]) for n in order})
fb.setupHorizontalHeader(ascent=900, descent=-200)
fb.setupNameTable({"familyName": "baud component cases", "styleName": "Regular"})
fb.setupOS2()
fb.setupPost()
fb.setupHead(unitsPerEm=1000, created=0, modified=0)
fb.setupFvar(axes=[("wght", 100, 400, 900, "Weight")], instances=[])

UP = {"wght": (0, 1, 1)}
DOWN = {"wght": (-1, -1, 0)}
Z = (0, 0)
variations = {
    # 3 points, then four phantoms.
    "tri": [TupleVariation(UP, [Z, Z, (40, 60), Z, Z, Z, Z]),
            TupleVariation(DOWN, [Z, Z, (-20, -30), Z, Z, Z, Z])],
    "bar": [TupleVariation(UP, [(10, 10), Z, Z, Z, Z, Z, Z, Z])],
    # 2 component slots, then four phantoms: the anchored slot's delta is
    # ignored, the offset slot's applied.
    "anchvar": [TupleVariation(UP, [(30, 0), (78, 78), Z, Z, Z, Z])],
    # The delta-adjusted offset is what is scaled.
    "scaledvar": [TupleVariation(UP, [Z, (20, 40), Z, Z, Z, Z])],
    # A composite's own left and advance phantoms take only X deltas.
    "pairy": [TupleVariation(UP, [Z, Z, (0, 50), Z, Z, Z])],
    "anchpadv": [TupleVariation(UP, [Z, Z, Z, (30, 50), Z, Z])],
    "anchplx": [TupleVariation(UP, [Z, Z, (40, 50), Z, Z, Z])],
}
fb.setupGvar(variations)
# Composites keep the header box written above rather than one recomputed
# from points: fontTools' own point matching cannot reach a phantom point
# or a number past the glyph, which half of these use on purpose.
fb.font.recalcBBoxes = False
head = fb.font["head"]
head.xMin, head.yMin, head.xMax, head.yMax = 0, 0, 600, 400
fb.font.save(OUT)
print(OUT)
