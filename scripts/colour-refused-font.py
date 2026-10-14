"""A colour font whose colour glyph has a refused glyf outline.

Writes baud-font/tests/var/colour-refused-cases.ttf. `play`, mapped from
U+25B6 BLACK RIGHT-POINTING TRIANGLE, is a `COLR` v0 glyph of two layers,
and its own `glyf` entry is a composite whose first component is placed by
matched points, which "cannot be used for the first component glyph"
(`[[source: ms-opentype-glyf]]`): baud refuses that outline
(`Drawn::Refused`). U+25B6 has an emoji variation sequence
(`[[source: emoji-variation-sequences]]`: "25B6 FE0F ; emoji style") and is
not Emoji_Presentation, so it asks for colour only with U+FE0F. The atlas
draws a colour glyph from its layers before its outline, so a refused
outline does not stop the glyph drawing in colour; baud-render's
tests/refused_glyph.rs holds the stack to that.

Glyphs, each advance 600 and lsb its xMin:

  .notdef  a 500 x 700 box at x 50
  A        a 400 x 600 box at x 100, the control, mapped from A and from
           M, which a renderer measures its cell by
  tri      (0,0) (400,0) (200,300), the first layer, palette entry 0 (red)
  bar      a 100 x 40 box, the second layer, palette entry 1 (blue)
  play     bar p0/c0 first, then tri: refused; COLR layers tri, bar

Deterministic: the head timestamps are fixed. baud's own font, under the
repository's licence (baud-font/tests/licences.rs lists it).

    nix shell --impure --expr 'with import (builtins.getFlake "nixpkgs") {}; \
      python3.withPackages (p: [ p.fonttools ])' -c python3 scripts/colour-refused-font.py
"""
import os
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib.tables._g_l_y_f import GlyphComponent

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
OUT = os.path.join(ROOT, "baud-font/tests/var/colour-refused-cases.ttf")

order = [".notdef", "A", "tri", "bar", "play"]

fb = FontBuilder(1000, isTTF=True)
fb.setupGlyphOrder(order)
fb.setupCharacterMap({0x41: "A", 0x4D: "A", 0x25B6: "play"})


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


glyphs = {
    ".notdef": poly([(50, 0), (550, 0), (550, 700), (50, 700)]),
    "A": poly([(100, 0), (500, 0), (500, 600), (100, 600)]),
    "tri": poly([(0, 0), (400, 0), (200, 300)]),
    "bar": poly([(0, 0), (100, 0), (100, 40), (0, 40)]),
}
play = TTGlyphPen(None).glyph()
play.numberOfContours = -1
play.components = [component("bar", "0/0"), component("tri", (0, 0))]
play.xMin, play.yMin, play.xMax, play.yMax = 0, 0, 400, 300
glyphs["play"] = play

fb.setupGlyf(glyphs, calcGlyphBounds=False)
glyf = fb.font["glyf"]
for n in [".notdef", "A", "tri", "bar"]:
    glyf[n].recalcBounds(glyf)
lsb = {n: getattr(glyf[n], "xMin", 0) for n in order}
fb.setupHorizontalMetrics({n: (600, lsb[n]) for n in order})
fb.setupHorizontalHeader(ascent=900, descent=-200)
fb.setupNameTable({"familyName": "baud colour refused cases", "styleName": "Regular"})
fb.setupOS2()
fb.setupPost()
fb.setupHead(unitsPerEm=1000, created=0, modified=0)
fb.setupCOLR({"play": [("tri", 0), ("bar", 1)]}, version=0)
fb.setupCPAL([[(1.0, 0.0, 0.0, 1.0), (0.0, 0.0, 1.0, 1.0)]])
# `play` keeps the header box written above: fontTools' own point matching
# would place its first component, which is the case under test.
fb.font.recalcBBoxes = False
head = fb.font["head"]
head.xMin, head.yMin, head.xMax, head.yMax = 0, 0, 550, 700
fb.font.save(OUT)
print(OUT)
