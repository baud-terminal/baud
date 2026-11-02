"""A face whose Arabic letters are wider than a terminal cell.

Writes baud-font/tests/var/joining-cases.ttf. A proportional Arabic face
draws a letter at its own advance, and the grid gives every letter one
cell: Noto Sans Arabic's isolated sad is 1306 units at an em of 1000 and
its medial sad 949, against JetBrains Mono's cell of 600. This face holds
such letters with boxes for outlines, so where their ink lies is exact, and
the joining forms and a mark placed by an anchor, so a renderer's joined
cells and its marked ones are reached too.

Glyphs, all boxes from y 0 unless said (x from, x to; advance):

  space         empty; 600
  beh  U+0628   dual-joining: 0-500; 500, and beh.init, beh.medi, beh.fina
                the same
  reh  U+0631   right-joining: 50-350, 0-300 high; 400, and reh.fina the
                same
  sad  U+0635   dual-joining and wider than a cell: isolated 0-1300 (1300),
                sad.init 0-1000, sad.medi 0-950, sad.fina 0-1100, each its
                advance its right edge
  fatha U+064E  a mark, -50 to 50 across, 600-700 high; no advance

GSUB `init`, `medi` and `fina` (script `arab`) take a letter to its form,
and `calt` has a rule that begins on a sad before a reh and puts the sad
back as it was, so a renderer that gathers the pairs its rules begin on
finds one, and draws nothing other for it.
GPOS `mark` hangs the fatha from an anchor on each base: at 1100 on the
isolated sad, 800 on sad.init, 750 on sad.medi, 900 on sad.fina -- past a
600-unit cell, so a mark that is not moved with its fitted base lands
outside the base's cell -- and over the middle of every other letter.
GDEF classes the fatha a mark and every letter a base.

Run with a Python that has fontTools (4.63.0 wrote the committed file):
`nix-shell -p 'python3.withPackages (p: [ p.fonttools ])' --run
'python3 scripts/joining-cases-font.py'`.
"""

import os

from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
OUT = os.path.join(ROOT, "baud-font/tests/var/joining-cases.ttf")

# name: (codepoint or None, (x0, y0, x1, y1) or None, advance)
GLYPHS = {
    ".notdef": (None, None, 600),
    "space": (0x20, None, 600),
    "beh": (0x0628, (0, 0, 500, 200), 500),
    "beh.init": (None, (0, 0, 500, 200), 500),
    "beh.medi": (None, (0, 0, 500, 200), 500),
    "beh.fina": (None, (0, 0, 500, 200), 500),
    "reh": (0x0631, (50, 0, 350, 300), 400),
    "reh.fina": (None, (50, 0, 350, 300), 400),
    "sad": (0x0635, (0, 0, 1300, 300), 1300),
    "sad.init": (None, (0, 0, 1000, 300), 1000),
    "sad.medi": (None, (0, 0, 950, 300), 950),
    "sad.fina": (None, (0, 0, 1100, 300), 1100),
    "fatha": (0x064E, (-50, 600, 50, 700), 0),
}
order = list(GLYPHS)


def box(b):
    pen = TTGlyphPen(None)
    if b is not None:
        x0, y0, x1, y1 = b
        pen.moveTo((x0, y0))
        pen.lineTo((x0, y1))
        pen.lineTo((x1, y1))
        pen.lineTo((x1, y0))
        pen.closePath()
    return pen.glyph()


fb = FontBuilder(1000, isTTF=True)
fb.setupGlyphOrder(order)
fb.setupCharacterMap({cp: n for n, (cp, _, _) in GLYPHS.items() if cp is not None})
fb.setupGlyf({n: box(b) for n, (_, b, _) in GLYPHS.items()})
fb.setupHorizontalMetrics({n: (adv, b[0] if b else 0) for n, (_, b, adv) in GLYPHS.items()})
fb.setupHorizontalHeader(ascent=800, descent=-200)
fb.setupNameTable({"familyName": "baud joining cases", "styleName": "Regular"})
fb.setupOS2(usWeightClass=400, xAvgCharWidth=600)
fb.setupPost()
# A fixed timestamp in fontTools' 1904 epoch, after 1970, so it is kept
# as written.
fb.setupHead(unitsPerEm=1000, created=3_600_000_000, modified=3_600_000_000)
fb.addOpenTypeFeatures(
    """
languagesystem DFLT dflt;
languagesystem arab dflt;
feature init {
    sub beh by beh.init;
    sub sad by sad.init;
} init;
feature medi {
    sub beh by beh.medi;
    sub sad by sad.medi;
} medi;
feature fina {
    sub beh by beh.fina;
    sub reh by reh.fina;
    sub sad by sad.fina;
} fina;
feature calt {
    sub sad' reh by sad;
} calt;
markClass fatha <anchor 0 600> @TOP;
feature mark {
    pos base sad <anchor 1100 400> mark @TOP;
    pos base sad.init <anchor 800 400> mark @TOP;
    pos base sad.medi <anchor 750 400> mark @TOP;
    pos base sad.fina <anchor 900 400> mark @TOP;
    pos base [beh beh.init beh.medi beh.fina] <anchor 250 300> mark @TOP;
    pos base [reh reh.fina] <anchor 200 400> mark @TOP;
} mark;
table GDEF {
    GlyphClassDef [beh beh.init beh.medi beh.fina reh reh.fina sad sad.init sad.medi sad.fina], , [fatha], ;
} GDEF;
"""
)
fb.font.recalcTimestamp = False
fb.font.save(OUT)
print(OUT, os.path.getsize(OUT))
