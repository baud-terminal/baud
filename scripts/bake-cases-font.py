"""A variable font built to reach what JetBrains Mono never makes the bake do,
and fontTools' instances of it as the oracle.

Writes baud-font/tests/var/bake/cases.ttf and cases-<wght>.ttf for each
weight below, the instances made as bake/'s JetBrains Mono instances were
(instantiateVariableFont with its defaults, recalcTimestamp off).
JetBrains Mono has no glyph program, no degenerate component, no
composite whose offsets stay whole beside one whose do not, no left-phantom
delta, one advance, no intermediate region and no avar; the bake's rules
for each were green under every perturbation until this font. The glyphs:

  sq     one contour with a glyph program; a tuple over every point that
         moves the left phantom (so lsb is no longer xMin) and the advance,
         a sparse tuple that wraps round the contour (inference), and an
         intermediate region, whose scalar is not a 2.14 value
  dot    a single point: a degenerate glyph
  wide   another advance, which moves, so OS/2's average width does
  intc   dot and sq, no variation data: every offset stays whole, so its
         bounds are its components' -- dot skipped as degenerate
  fracc  dot and sq with fractional deltas on both offsets, one pushed
         from a byte to a word: it is flattened, dot included
  nestc  fracc and intc, whole offsets: the union of the two bounds above
  instc  sq under USE_MY_METRICS and ROUND_XY_TO_GRID, with a program
  acc    a mark, attached to sq by anchors that vary through GDEF's store;
         GDEF names no mark glyph sets, so it drops to 1.0, not 1.2
  e1-e3  empty; e2 and e3 share an advance, the trailing run hmtx trims
         to one record, which leaves its 46 bytes padded; twelve glyphs
         leave the short loca's 26 padded too. Their side bearings are
         not zero, so neither is what a compaction that left stale bytes
         in the padding would leave there

Axis: wght 100-400-900 with an avar. Deterministic: the head timestamps
are fixed, at a value fontTools leaves alone.

    nix shell --impure --expr 'with import (builtins.getFlake "nixpkgs") {}; \\
      python3.withPackages (p: [ p.fonttools ])' -c python3 scripts/bake-cases-font.py
"""
import os
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib import TTFont, newTable
from fontTools.ttLib.tables.TupleVariation import TupleVariation
from fontTools.ttLib.tables import ttProgram
from fontTools.ttLib.tables._g_l_y_f import Glyph, GlyphComponent, GlyphCoordinates
from fontTools.varLib import instancer

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
OUT = os.path.join(ROOT, "baud-font/tests/var/bake")
WEIGHTS = (250, 550, 700, 875)

order = [".notdef", "sq", "dot", "wide", "intc", "fracc", "nestc", "instc", "acc", "e1", "e2", "e3"]
fb = FontBuilder(1000, isTTF=True)
fb.setupGlyphOrder(order)
fb.setupCharacterMap({0x41 + i: n for i, n in enumerate(order[1:])})


def program(code):
    p = ttProgram.Program()
    p.fromBytecode(code)
    return p


glyphs = {}
glyphs[".notdef"] = TTGlyphPen(None).glyph()

pen = TTGlyphPen(None)
pen.moveTo((100, 0))
pen.lineTo((500, 0))
pen.qCurveTo((600, 250), (500, 500))
pen.lineTo((100, 500))
pen.closePath()
glyphs["sq"] = pen.glyph()
# PUSHB[0] 1, POP: a program is bytes to the bake, carried through.
glyphs["sq"].program = program(b"\xb0\x01\x21")

dot = Glyph()
dot.numberOfContours = 1
dot.coordinates = GlyphCoordinates([(250, 250)])
dot.endPtsOfContours = [0]
dot.flags = bytearray([1])
dot.program = program(b"")
glyphs["dot"] = dot

pen = TTGlyphPen(None)
pen.moveTo((50, 0))
pen.lineTo((1200, 0))
pen.lineTo((1200, 300))
pen.closePath()
glyphs["wide"] = pen.glyph()

pen = TTGlyphPen(None)
glyphs["e1"] = TTGlyphPen(None).glyph()
glyphs["e2"] = TTGlyphPen(None).glyph()
glyphs["e3"] = TTGlyphPen(None).glyph()
pen = TTGlyphPen(None)
pen.moveTo((200, 600))
pen.lineTo((300, 600))
pen.lineTo((250, 700))
pen.closePath()
glyphs["acc"] = pen.glyph()


def component(name, x, y, flags=0):
    c = GlyphComponent()
    c.glyphName = name
    c.x, c.y = x, y
    c.flags = flags
    return c


def composite(comps, code=None):
    g = TTGlyphPen(None).glyph()
    g.numberOfContours = -1
    g.components = comps
    if code is not None:
        g.program = program(code)
    return g


USE_MY_METRICS = 0x0200
ROUND_XY_TO_GRID = 0x0004
glyphs["intc"] = composite([component("dot", 650, 900), component("sq", 0, 0)])
glyphs["fracc"] = composite([component("dot", 0, 800), component("sq", 120, 0)])
glyphs["nestc"] = composite([component("fracc", 5, 5), component("intc", 1000, 0)])
glyphs["instc"] = composite(
    [component("sq", 0, 0, USE_MY_METRICS | ROUND_XY_TO_GRID), component("dot", -300, 40)],
    b"\xb0\x02\x21",
)

fb.setupGlyf(glyphs)
glyf = fb.font["glyf"]
for n in order:
    glyf[n].recalcBounds(glyf)
advances = {n: 900 for n in order}
advances["wide"] = 1300
advances["acc"] = 0
advances["e1"] = 700
# The empty glyphs keep side bearings of their own, so what the trimmed
# hmtx leaves behind it is not zero by accident.
bearings = {n: getattr(glyf[n], "xMin", 0) for n in order}
bearings.update(e1=7, e2=5, e3=3)
fb.setupHorizontalMetrics({n: (advances[n], bearings[n]) for n in order})
fb.setupHorizontalHeader(ascent=900, descent=-100)
fb.setupNameTable({"familyName": "baud bake cases", "styleName": "Regular"})
fb.setupOS2(usWeightClass=400, xAvgCharWidth=333)
fb.setupPost()
# A timestamp before 1970 in fontTools' 1904 epoch is read back as unix
# time and rewritten, so the fixed value is a real one.
fb.setupHead(unitsPerEm=1000, created=3_600_000_000, modified=3_600_000_000)
fb.setupFvar(axes=[("wght", 100, 400, 900, "Weight")], instances=[])
avar = newTable("avar")
avar.segments = {"wght": {-1.0: -1.0, -0.5: -0.3, 0.0: 0.0, 0.4: 0.55, 1.0: 1.0}}
fb.font["avar"] = avar

W = "wght"
variations = {
    "sq": [
        # 5 points, then left, right, top, bottom phantoms.
        TupleVariation({W: (0, 1, 1)}, [(10, 0), (20, 5), (31, -10), (0, 15), (-5, 5), (7, 0), (45, 0), (0, 0), (0, 0)]),
        TupleVariation({W: (-1, -1, 0)}, [None, (-20, -10), None, None, (15, 21), (-6, 0), None, None, None]),
        TupleVariation({W: (0.2, 0.5, 0.8)}, [None, None, (41, 43), None, None, None, None, None, None]),
    ],
    "dot": [TupleVariation({W: (0, 1, 1)}, [(50, 50), (0, 0), (0, 0), (0, 0), (0, 0)])],
    "wide": [TupleVariation({W: (0, 1, 1)}, [(0, 0), (40, 0), (40, 7), (0, 0), (133, 0), (0, 0), (0, 0)])],
    "fracc": [
        TupleVariation({W: (0, 1, 1)}, [(7, 3), (21, -5), (0, 0), (0, 0), (0, 0), (0, 0)]),
        TupleVariation({W: (-1, -1, 0)}, [(-3, 11), (-9, 2), (0, 0), (0, 0), (0, 0), (0, 0)]),
    ],
    "instc": [TupleVariation({W: (0, 1, 1)}, [(0, 0), (-17, 13), (0, 0), (0, 0), (0, 0), (0, 0)])],
}
fb.setupGvar(variations)

# The mark: its anchor and sq's vary through GDEF's store. feaLib writes
# a variable anchor as format 3 with a VariationIndex device; the second
# mark class has a plain one.
fb.addOpenTypeFeatures(
    """
languagesystem DFLT dflt;
markClass acc <anchor (wght=100:240 wght=400:250 wght=900:273) 600> @TOP;
markClass dot <anchor 250 0> @LOW;
feature mark {
    pos base sq <anchor (wght=100:280 wght=400:300 wght=900:337) (wght=100:505 wght=400:500 wght=900:531)> mark @TOP
                <anchor 300 0> mark @LOW;
} mark;
table GDEF {
    GlyphClassDef [sq wide], , [acc], ;
} GDEF;
"""
)

path = os.path.join(OUT, "cases.ttf")
fb.font.save(path)
print(path, os.path.getsize(path))
for w in WEIGHTS:
    font = TTFont(path, recalcTimestamp=False)
    inst = instancer.instantiateVariableFont(font, {"wght": w})
    inst.recalcTimestamp = False
    p = os.path.join(OUT, f"cases-{w}.ttf")
    inst.save(p)
    print(p, os.path.getsize(p))
