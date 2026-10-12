"""Two variable fonts whose glyphs' left side bearings are not their xMin.

Writes baud-font/tests/var/bearing-cases.ttf, with `head` flags bit 1
clear, and bearing-cases-bit1.ttf, the same font with bit 1 set. The left
side bearing point -- the first phantom point, the origin -- sits at
`pp1 = xMin - lsb` (`[[source: ms-opentype-hmtx]]`,
`[[source: ms-opentype-head]]`), and a glyph is drawn from it: "The origin
point is the original pen position. Imagine that the pen moves the
left-side bearing distance, draws the glyph" (`[[source: apple-ttrm-ch2]]`).
With bit 1 set the `head` text puts that point at x = 0 for every glyph,
and requires xMin and the lsb to be equal, which these glyphs break: the
second font is the malformed case, kept for Jev (note Z, step 8).

Simple glyphs, all a 60 x 60 box at x 20 (xMin 20), polygons that start on
the curve:

  box     lsb 0: pp1 20
  neg     lsb 50: pp1 -30
  same    lsb 20, xMin: pp1 0, and its left phantom moves +10 at wght 900
  boxvar  lsb 0: pp1 20, its left phantom moves +30 and its first point
          +10 in x at wght 900

Composites, each header box written by hand (no recomputed one: fontTools
cannot match a phantom point):

  comp     box at (100, 0): xMin 120, lsb 105, pp1 15; its own left
           phantom moves +20 at wght 900 (the gvar text applies X alone)
  umm      neg at (0, 0) under USE_MY_METRICS, then box at (200, 0): the
           composite takes neg's phantoms, pp1 -30
  ummlast  neg, then box at (200, 0), both under USE_MY_METRICS: the last
           one's, box's, taken before it is placed: pp1 20
  ummnest  umm under USE_MY_METRICS, then box at (300, 0): umm's, which is
           neg's: pp1 -30
  anch     box, then neg with its point 0 laid on box's point 1 (80, 0):
           the matched walk at the default instance; xMin 20, lsb -5,
           pp1 25
  anchlsb  box, then neg with its left phantom, point 4, laid on box's
           point 1: neg's phantom starts at its pp1, -30, so neg moves
           110; xMin 20, lsb 20, pp1 0

Axis: wght 100-400-900, no avar; only the 900 side carries deltas, so 650
is half of each. Deterministic: the head timestamps are fixed. baud's own
font, under the repository's licence (baud-font/tests/licences.rs lists
it).

    nix shell --impure --expr 'with import (builtins.getFlake "nixpkgs") {}; \
      python3.withPackages (p: [ p.fonttools ])' -c python3 scripts/bearing-cases-font.py
"""
import os
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib.tables.TupleVariation import TupleVariation
from fontTools.ttLib.tables._g_l_y_f import GlyphComponent

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
OUT = os.path.join(ROOT, "baud-font/tests/var")

USE_MY_METRICS = 0x0200

simple = {"box": 0, "neg": 50, "same": 20, "boxvar": 0}
# name: (components, header box, lsb)
composites = {
    "comp": ([("box", (100, 0))], (120, 0, 180, 60), 105),
    "umm": ([("neg", (0, 0), USE_MY_METRICS), ("box", (200, 0))], (20, 0, 280, 60), 20),
    "ummlast": (
        [("neg", (0, 0), USE_MY_METRICS), ("box", (200, 0), USE_MY_METRICS)],
        (20, 0, 280, 60),
        20,
    ),
    "ummnest": ([("umm", (0, 0), USE_MY_METRICS), ("box", (300, 0))], (20, 0, 380, 60), 20),
    "anch": ([("box", (0, 0)), ("neg", "1/0")], (20, 0, 140, 60), -5),
    "anchlsb": ([("box", (0, 0)), ("neg", "1/4")], (20, 0, 190, 60), 20),
}
order = [".notdef"] + list(simple) + list(composites)


def poly(points):
    pen = TTGlyphPen(None)
    pen.moveTo(points[0])
    for p in points[1:]:
        pen.lineTo(p)
    pen.closePath()
    return pen.glyph()


def component(name, place, flags=0):
    c = GlyphComponent()
    c.glyphName = name
    if isinstance(place, str):
        c.firstPt, c.secondPt = (int(v) for v in place.split("/"))
    else:
        c.x, c.y = place
    c.flags = flags
    return c


def build(path, flags):
    fb = FontBuilder(1000, isTTF=True)
    fb.setupGlyphOrder(order)
    fb.setupCharacterMap({0x41 + i: n for i, n in enumerate(order[1:])})
    glyphs = {".notdef": TTGlyphPen(None).glyph()}
    for n in simple:
        glyphs[n] = poly([(20, 0), (80, 0), (80, 60), (20, 60)])
    for n, (parts, box, _) in composites.items():
        g = TTGlyphPen(None).glyph()
        g.numberOfContours = -1
        g.components = [component(*p) for p in parts]
        g.xMin, g.yMin, g.xMax, g.yMax = box
        glyphs[n] = g
    fb.setupGlyf(glyphs, calcGlyphBounds=False)
    glyf = fb.font["glyf"]
    for n in simple:
        glyf[n].recalcBounds(glyf)
    lsb = {".notdef": 0, **simple, **{n: c[2] for n, c in composites.items()}}
    fb.setupHorizontalMetrics({n: (600, lsb[n]) for n in order})
    fb.setupHorizontalHeader(ascent=900, descent=-200)
    fb.setupNameTable({"familyName": "baud bearing cases", "styleName": "Regular"})
    fb.setupOS2()
    fb.setupPost()
    fb.setupHead(unitsPerEm=1000, created=0, modified=0)
    fb.setupFvar(axes=[("wght", 100, 400, 900, "Weight")], instances=[])
    up = {"wght": (0, 1, 1)}
    z = (0, 0)
    fb.setupGvar({
        # 4 points, then the left, right, top and bottom phantoms.
        "same": [TupleVariation(up, [z, z, z, z, (10, 0), z, z, z])],
        "boxvar": [TupleVariation(up, [(10, 0), z, z, z, (30, 0), z, z, z])],
        # 1 component slot, then four phantoms.
        "comp": [TupleVariation(up, [z, (20, 0), z, z, z])],
    })
    fb.font.recalcBBoxes = False
    head = fb.font["head"]
    head.xMin, head.yMin, head.xMax, head.yMax = 0, 0, 380, 60
    head.flags = flags
    fb.font.save(path)
    print(path)


# Bit 0, the baseline at y = 0, in both; bit 1 in the second alone.
build(os.path.join(OUT, "bearing-cases.ttf"), 0x0001)
build(os.path.join(OUT, "bearing-cases-bit1.ttf"), 0x0003)
