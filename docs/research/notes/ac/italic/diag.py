"""Diagonal stroke thickness at a bowl corner: the ink along the line from the outer corner to the
inner corner (sheared), against the horizontal stem width of the bowl's side at a given height."""
import sys, math, numpy as np
sys.path.insert(0, sys.argv[1])
from glyphsheet import flatten
from fontTools.ttLib import TTFont
from fontTools.pens.recordingPen import DecomposingRecordingPen
TAN = math.tan(math.radians(9))
def segs(font, cp=None, name=None):
    f = TTFont(font); gs = f.getGlyphSet(); n = name or f.getBestCmap()[cp]
    r = DecomposingRecordingPen(gs); gs[n].draw(r); cs, _ = flatten(r.value)
    return [(c[i], c[(i + 1) % len(c)]) for c in cs for i in range(len(c))]
def crossings(S, P, Q):
    P, Q = np.array(P, float), np.array(Q, float); d = Q - P; ts = []
    for a, b in S:
        a, b = np.array(a), np.array(b); e = b - a
        den = d[0] * e[1] - d[1] * e[0]
        if abs(den) < 1e-12: continue
        w = a - P; t = (w[0] * e[1] - w[1] * e[0]) / den; s = (w[0] * d[1] - w[1] * d[0]) / den
        if 0 <= s < 1: ts.append(t)
    return sorted(ts), np.linalg.norm(d)
def ink_at(S, P, Q, t0=0.5):
    ts, L = crossings(S, P, Q)
    ts = [t for t in ts if t > 0]          # the outer corner lies outside the ink: the first two crossings past it bound the stroke
    return (ts[1] - ts[0]) * L if len(ts) >= 2 else None
def stem(S, y, x0, x1):
    ts, L = crossings(S, (x0, y), (x1, y))
    return [round((b - a) * L, 1) for a, b in zip(ts[::2], ts[1::2])]
def sh(p, y0): return (p[0] + TAN * (p[1] - y0), p[1])
mode = sys.argv[2]
if mode == 'jbm':
    up, it = sys.argv[3], sys.argv[4]; y0 = 277.6
    Su = [(sh(a, y0), sh(b, y0)) for a, b in segs(up, name='o')]; Si = segs(it, name='o')
    for lab, Co, Ci in (('o bottom-right (99)', (516, -8), (426, 72)), ('o top-right (81)', (516, 558), (426, 478)), ('o bottom-left (81)', (84, -8), (174, 72)), ('o top-left (99)', (84, 558), (174, 478))):
        P, Q = sh(Co, y0), sh(Ci, y0)
        print('JBM', lab, 'sheared upright %.1f  italic %.1f  ratio %.3f' % (ink_at(Su, P, Q), ink_at(Si, P, Q), ink_at(Si, P, Q) / ink_at(Su, P, Q)), '| stem at y=275', stem(Su, 275, 0, 700), stem(Si, 275, 0, 700))
else:
    fonts = sys.argv[3].split(','); y0 = 315.0
    corners = {'Italic': [(0x05E1, 'samekh bottom-left', (98.9, -9.1), (188.9, 71.2)), (0x05E1, 'samekh bottom-right', (510.5, -9.0), (420.5, 71.2)),
                          (0x05D8, 'tet bottom-right', (508.4, -11.0), (416.1, 77.8)), (0x05E9, 'shin bottom-right', (508.0, -10.4), (418.0, 71.8)),
                          (0x05E9, 'shin middle', (338.2, 259.1), (263.6, 298.6)), (0x05D8, 'tet top-right', (508.4, 630.0), (416.1, 545.6)), (0x05DE, 'mem top-right', (509.5, 631.2), (419.1, 551.1))],
               'BoldItalic': [(0x05E1, 'samekh bottom-left', (91.7, -10.2), (217.2, 101.9)), (0x05E1, 'samekh bottom-right', (515.4, -10.2), (390.4, 101.9)),
                              (0x05E9, 'shin bottom-right', (520.6, -10.6), (395.6, 103.7)), (0x05DE, 'mem top-right', (542.3, 631.2), (416.9, 520.0))]}[sys.argv[4]]
    for cp, lab, Co, Ci in corners:
        P, Q = sh(Co, y0), sh(Ci, y0)
        vals = [ink_at(segs(f, cp), P, Q) for f in fonts]
        print(lab, ' '.join('%.1f' % v if v else '-' for v in vals), ' ratio to first:', ' '.join('%.3f' % (v / vals[0]) if v and vals[0] else '-' for v in vals))
