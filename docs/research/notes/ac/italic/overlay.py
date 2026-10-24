"""overlay.py out.png base.ttf opt.ttf SCALE U+... : base grey; ink only in opt red, only in base blue."""
import sys, numpy as np
sys.path.insert(0, __import__('os').path.dirname(__file__))
from glyphsheet import flatten, fill, png
from fontTools.ttLib import TTFont
from fontTools.pens.recordingPen import DecomposingRecordingPen
out, fa, fb, sc = sys.argv[1], sys.argv[2], sys.argv[3], float(sys.argv[4]); cps = [int(a[2:], 16) for a in sys.argv[5:]]
cw, ch = int(720 * sc), int(1100 * sc)
img = np.full((ch, cw * len(cps), 3), 255, np.uint8)
def mask(fp, cp, ci):
    f = TTFont(fp); gs = f.getGlyphSet(); cm = f.getBestCmap()
    r = DecomposingRecordingPen(gs); gs[cm[cp]].draw(r); cs, _ = flatten(r.value)
    m = np.zeros((ch, cw * len(cps), 3), np.uint8); fill(m, cs, ci * cw + 60 * sc, 850 * sc, sc, (1, 1, 1)); return m[:, :, 0] > 0
cnt = []
for ci, cp in enumerate(cps):
    A, B = mask(fa, cp, ci), mask(fb, cp, ci)
    img[A & B] = (150, 150, 150); img[B & ~A] = (220, 30, 30); img[A & ~B] = (30, 60, 220)
    cnt.append((hex(cp), int((A ^ B).sum())))
png(out, img); print(cnt)
