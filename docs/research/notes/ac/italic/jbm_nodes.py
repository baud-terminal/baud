"""Node-level comparison of JetBrains Mono's Italic master against its upright, sheared 9 degrees."""
import sys, math
import numpy as np
from glyphs_src import glyph, layer_paths, MID
up_t = open(sys.argv[1]).read(); it_t = open(sys.argv[2]).read()
master = sys.argv[3] if len(sys.argv) > 3 else 'Regular'
TAN = math.tan(math.radians(9))
names = 'o O zero c e n h m u a b d p q g s C G Q D U S six nine eight three two five j t r'.split()
def segs(path):
    """Cubic segments: (p0, c1, c2, p3) with on-curve endpoints, as index tuples."""
    n = len(path); out = []
    for i in range(n):
        if path[i][2].startswith('CURVE'):
            p3 = i; c2 = (i - 1) % n; c1 = (i - 2) % n; p0 = (i - 3) % n
            out.append((p0, c1, c2, p3))
    return out
def inter(p0, d0, p1, d1):
    A = np.array([[d0[0], -d1[0]], [d0[1], -d1[1]]])
    if abs(np.linalg.det(A)) < 1e-9: return None
    s, t = np.linalg.solve(A, np.array(p1) - np.array(p0))
    return np.array(p0) + s * np.array(d0)
KIND = []
rows = []; y0s = []; corner = {'acute': [], 'obtuse': [], 'right': []}; resid_all = []
for g in names:
    try:
        U = layer_paths(glyph(up_t, g), MID[master]); I = layer_paths(glyph(it_t, g), MID[master])
    except Exception as e:
        continue
    if [len(p) for p in U] != [len(p) for p in I] or any(a[2] != b[2] for pu, pi in zip(U, I) for a, b in zip(pu, pi)):
        rows.append((g, 'incompatible')); continue
    on = [(a, b) for pu, pi in zip(U, I) for a, b in zip(pu, pi) if a[2] != 'OFFCURVE']
    if not on:
        rows.append((g, 'components only')); continue
    # x_it = x_up + TAN*(y_up - y0) + e  -> least-squares y0
    ux = np.array([a[0] for a, b in on]); uy = np.array([a[1] for a, b in on]); ix = np.array([b[0] for a, b in on]); iy = np.array([b[1] for a, b in on])
    y0 = np.mean(uy - (ix - ux) / TAN); y0s.append(y0)
    res = ix - (ux + TAN * (uy - y0)); resy = iy - uy
    resid_all += list(np.abs(res))
    rows.append((g, round(y0, 1), round(float(np.abs(res).mean()), 1), round(float(np.abs(res).max()), 1), round(float(np.abs(resy).max()), 1)))
    for pu, pi in zip(U, I):
        for (a, b, c, d) in segs(pu):
            P = [np.array(pu[k][:2]) for k in (a, b, c, d)]; Q = [np.array(pi[k][:2]) for k in (a, b, c, d)]
            Cu = inter(P[0], P[1] - P[0], P[3], P[2] - P[3]); Ci = inter(Q[0], Q[1] - Q[0], Q[3], Q[2] - Q[3])
            if Cu is None or Ci is None: continue
            d0u, d1u = np.linalg.norm(Cu - P[0]), np.linalg.norm(Cu - P[3]); d0i, d1i = np.linalg.norm(Ci - Q[0]), np.linalg.norm(Ci - Q[3])
            if min(d0u, d1u, d0i, d1i) < 20: continue
            fu = (np.linalg.norm(P[1] - P[0]) / d0u, np.linalg.norm(P[2] - P[3]) / d1u)
            fi = (np.linalg.norm(Q[1] - Q[0]) / d0i, np.linalg.norm(Q[2] - Q[3]) / d1i)
            v0, v1 = Q[0] - Ci, Q[3] - Ci
            ang = math.degrees(math.acos(np.dot(v0, v1) / np.linalg.norm(v0) / np.linalg.norm(v1)))
            vu0, vu1 = P[0] - Cu, P[3] - Cu
            angu = math.degrees(math.acos(np.dot(vu0, vu1) / np.linalg.norm(vu0) / np.linalg.norm(vu1)))
            if abs(angu - 90) > 3: continue
            k = 'acute' if ang < 87 else 'obtuse' if ang > 93 else 'right'
            n_ = len(pu)
            before_line = pu[a][2].startswith('LINE')                 # the segment into p0 is straight
            after_line = pu[(d + 1) % n_][2].startswith('LINE')       # the segment out of p3 is straight
            kind = 'both-straight' if before_line and after_line else 'one-straight' if before_line or after_line else 'both-curved'
            corner[k].append((g, round(ang, 1), round((fi[0] + fi[1]) / (fu[0] + fu[1]), 3)))
            KIND.append((kind, ang - 90, (fi[0] + fi[1]) / (fu[0] + fu[1]), g))
for r in rows: print(*r)
print('origin y0: median %.1f, spread %.1f-%.1f' % (np.median(y0s), min(y0s), max(y0s)))
print('on-curve residual after the best shear: mean %.1f units, 90th pct %.1f, max %.1f' % (np.mean(resid_all), np.percentile(resid_all, 90), max(resid_all)))
for k, v in corner.items():
    r = [x[2] for x in v]
    if r: print('%-6s corners %3d  handle-fullness ratio italic/upright: median %.3f  mean %.3f  (10-90 pct %.3f-%.3f)' % (k, len(v), np.median(r), np.mean(r), np.percentile(r, 10), np.percentile(r, 90)))
print('corners off pure shear by more than 1.5%:')
for k, v in corner.items():
    for g, ang, r in v:
        if abs(r - 1) > 0.015: print('  ', k, g, ang, r)
BOWL = set('o O zero c C G six eight s S two three five b d p q g D U'.split())
for label, pick in (('all', lambda g: True), ('bowls', lambda g: g in BOWL), ('arches', lambda g: g not in BOWL)):
    pts = [(ang - 90, r) for k, v in corner.items() for g, ang, r in v if pick(g) and k != 'right']
    X = np.array([p[0] for p in pts]); Y = np.array([p[1] for p in pts])
    slope = np.sum(X * (Y - 1)) / np.sum(X * X)   # through (0, 1): a right angle stays as it is
    ac = Y[X < 0]; ob = Y[X > 0]
    print('%-6s n=%3d  fit ratio = 1 + %.5f*(angle-90)  -> 81 deg %.3f, 99 deg %.3f | means acute %.3f obtuse %.3f | medians %.3f %.3f' % (label, len(pts), slope, 1 - 9 * slope, 1 + 9 * slope, ac.mean(), ob.mean(), np.median(ac), np.median(ob)))
pts = [(ang - 90, r) for k, v in corner.items() for g, ang, r in v if k != 'right' and abs(r - 1) > 0.015]
X = np.array([p[0] for p in pts]); Y = np.array([p[1] for p in pts])
slope = np.sum(X * (Y - 1)) / np.sum(X * X)
print('corrected-only n=%d fit slope %.5f -> 81 %.3f 99 %.3f' % (len(pts), slope, 1 - 9 * slope, 1 + 9 * slope))

for kind in ('both-straight', 'one-straight', 'both-curved'):
    pts = [(x, r) for kd, x, r, g in KIND if kd == kind and abs(x) > 3]
    if not pts: continue
    X = np.array([p[0] for p in pts]); Y = np.array([p[1] for p in pts])
    slope = np.sum(X * (Y - 1)) / np.sum(X * X)
    nc = sum(1 for x, r in pts if abs(r - 1) > 0.015)
    print('kind %-13s n=%3d corrected(>1.5%%) %2d  slope %.5f -> 81 %.3f 99 %.3f  glyphs %s' % (kind, len(pts), nc, slope, 1 - 9 * slope, 1 + 9 * slope, ' '.join(sorted(set(g for kd, x, r, g in KIND if kd == kind)))))
