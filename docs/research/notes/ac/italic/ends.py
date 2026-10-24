"""Arc-end displacement at JBM's right-angle round corners: italic against the upright sheared 9 deg."""
import sys, math
import numpy as np
sys.path.insert(0, sys.argv[1])
from glyphs_src import glyph, layer_paths, MID
up_t = open(sys.argv[2]).read(); it_t = open(sys.argv[3]).read(); master = sys.argv[4]
TAN = math.tan(math.radians(9))
names = 'o O zero c e n h m u a b d p q g s C G Q D U S six nine eight three two five j t r'.split()
def segs(path):
    n = len(path); out = []
    for i in range(n):
        if path[i][2].startswith('CURVE'):
            out.append(((i - 3) % n, (i - 2) % n, (i - 1) % n, i))
    return out
def inter(p0, d0, p1, d1):
    A = np.array([[d0[0], -d1[0]], [d0[1], -d1[1]]])
    if abs(np.linalg.det(A)) < 1e-9: return None
    s, t = np.linalg.solve(A, np.array(p1) - np.array(p0))
    return np.array(p0) + s * np.array(d0)
rows = []
for g in names:
    try:
        U = layer_paths(glyph(up_t, g), MID[master]); I = layer_paths(glyph(it_t, g), MID[master])
    except Exception:
        continue
    if [len(p) for p in U] != [len(p) for p in I] or any(a[2] != b[2] for pu, pi in zip(U, I) for a, b in zip(pu, pi)):
        continue
    on = [(a, b) for pu, pi in zip(U, I) for a, b in zip(pu, pi) if a[2] != 'OFFCURVE']
    if not on: continue
    ux = np.array([a[0] for a, b in on]); uy = np.array([a[1] for a, b in on]); ix = np.array([b[0] for a, b in on])
    y0 = np.mean(uy - (ix - ux) / TAN)
    sh = lambda p: np.array([p[0] + TAN * (p[1] - y0), p[1]])
    for ci, (pu, pi) in enumerate(zip(U, I)):
        n = len(pu)
        for (a, b, c, d) in segs(pu):
            P = [np.array(pu[k][:2]) for k in (a, b, c, d)]; Q = [np.array(pi[k][:2]) for k in (a, b, c, d)]
            Cu = inter(P[0], P[1] - P[0], P[3], P[2] - P[3])
            if Cu is None: continue
            if min(np.linalg.norm(Cu - P[0]), np.linalg.norm(Cu - P[3])) < 20: continue
            vu0, vu1 = P[0] - Cu, P[3] - Cu
            angu = math.degrees(math.acos(np.dot(vu0, vu1) / np.linalg.norm(vu0) / np.linalg.norm(vu1)))
            if abs(angu - 90) > 3: continue
            S = [sh(p) for p in P]; Cs = sh(Cu)
            ang = math.degrees(math.acos(np.dot(S[0] - Cs, S[3] - Cs) / np.linalg.norm(S[0] - Cs) / np.linalg.norm(S[3] - Cs)))
            before = pu[(a - 1) % n][2].startswith('LINE') or pu[a][2].startswith('LINE')
            before = pu[a][2].startswith('LINE')           # p0 is a LINE node: the segment into it is straight
            after = pu[(d + 1) % n][2].startswith('LINE')
            kind = 'both-straight' if before and after else 'one-straight' if before or after else 'both-curved'
            # each end's move along its tangent, toward the corner positive (the arc shortened)
            for end, (pe, qe, adj) in (('p0', (S[0], Q[0], before)), ('p3', (S[3], Q[3], after))):
                t = (Cs - pe); L = np.linalg.norm(t); t = t / L
                along = float(np.dot(qe - pe, t)); w = qe - pe; across = float(t[0] * w[1] - t[1] * w[0])
                rows.append((g, ci, kind, round(ang, 1), end, 'straight' if adj else 'curve', round(along, 1), round(across, 1), round(L, 1)))
big = [r for r in rows if abs(r[6]) > 3]
print('master', master, 'corners', len(rows) // 2, 'ends', len(rows), 'ends moved >3 along:', len(big))
for r in big: print('  ', *r)
for kind in ('both-straight', 'one-straight', 'both-curved'):
    for adj in ('straight', 'curve'):
        for sense in ('acute', 'obtuse'):
            v = [r[6] for r in rows if r[2] == kind and r[5] == adj and ((r[3] < 90) == (sense == 'acute'))]
            if v: print('%-13s end at %-8s %-6s n=%3d mean along %+.2f  median %+.2f  |>3| %d' % (kind, adj, sense, len(v), np.mean(v), np.median(v), sum(abs(x) > 3 for x in v)))
