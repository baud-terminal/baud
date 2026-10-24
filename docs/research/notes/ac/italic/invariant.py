"""Does a bowl's outer straight reach at least as far as its inner one, in JetBrains Mono's upright
and italic masters? invariant.py DIR upright.glyphs italic.glyphs MASTER

Pairs each bowl corner (a right-angle round with a straight on one side) with the corner across the
stroke as optical.py's level_rec does: the same orientation, the inner corner inside the outer one,
the two straights 40-200 units apart. The gap is how much farther from the corner the outer joint
sits than the inner one; positive is an outer straight stopping short. Gaps under a unit in the
italic are the unshear's rounding; m's pairs (and ExtraBold a's) join corners of two different
strokes and are not a bowl's two sides."""
import sys, math
import numpy as np
sys.path.insert(0, sys.argv[1])
from glyphs_src import glyph, layer_paths, MID
up_t = open(sys.argv[2]).read(); it_t = open(sys.argv[3]).read(); master = sys.argv[4]
names = 'o O zero c e n h m u a b d p q g s C G Q D U S six nine eight three two five j t r'.split()
def inter(p0, d0, p1, d1):
    A = np.array([[d0[0], -d1[0]], [d0[1], -d1[1]]])
    if abs(np.linalg.det(A)) < 1e-9: return None
    s, t = np.linalg.solve(A, np.array(p1) - np.array(p0))
    return np.array(p0) + s * np.array(d0)
def corners(paths, tan):
    out = []
    for path in paths:
        n = len(path)
        for i in range(n):
            if not path[i][2].startswith('CURVE'): continue
            a, b, c, d = (i - 3) % n, (i - 2) % n, (i - 1) % n, i
            P = [np.array([path[k][0] - tan * path[k][1], path[k][1]]) for k in (a, b, c, d)]   # unsheared
            C = inter(P[0], P[1] - P[0], P[3], P[2] - P[3])
            if C is None or min(np.linalg.norm(C - P[0]), np.linalg.norm(C - P[3])) < 20: continue
            v0, v3 = (P[0] - C) / np.linalg.norm(P[0] - C), (P[3] - C) / np.linalg.norm(P[3] - C)
            if abs(math.degrees(math.acos(np.clip(np.dot(v0, v3), -1, 1))) - 90) > 3: continue
            before, after = path[a][2].startswith('LINE'), path[(d + 1) % n][2].startswith('LINE')
            ends = [(P[0], v0, before), (P[3], v3, after)]
            out.append((C, ends, 'one' if before != after else 'other'))
    return out
TAN = math.tan(math.radians(9))
for label, text, tan in (('upright', up_t, 0.0), ('italic', it_t, TAN)):
    for g in names:
        try:
            paths = layer_paths(glyph(text, g), MID[master])
        except Exception:
            continue
        cs = corners(paths, tan)
        for A in cs:
            C, ends, kind = A
            if kind != 'one': continue
            (J, u, _), (E, e, _) = ends if ends[0][2] else ends[::-1]
            best = None
            for B in cs:
                if B is A or np.dot(B[0] - C, u + e) <= 0: continue
                for k in (0, 1):
                    JB, uB, st = B[1][k]; EB, eB, _ = B[1][1 - k]
                    if not st or np.dot(uB, u) < 0.999 or np.dot(eB, e) < 0.999: continue
                    sep = abs(np.dot(JB - J, e))
                    if 40 <= sep <= 200 and (best is None or sep < best[0]): best = (sep, JB)
            if best is None: continue
            gap = np.dot(J - C, u) - np.dot(best[1] - C, u)
            print('%s %-8s %-5s outer joint (%5.0f,%5.0f) inner (%5.0f,%5.0f) gap %+5.1f%s' % (master, label, g, J[0], J[1], best[1][0], best[1][1], gap, '  STOPS SHORT' if gap > 0.5 else ''))
