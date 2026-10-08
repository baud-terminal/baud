# geometry helpers: flatten glyphs to polygons, measure run widths
import math
import numpy as np
import pathops
from fontTools.pens.basePen import BasePen
class FlatPen(BasePen):
    def __init__(self, gs, steps=16):
        super().__init__(gs); self.polys=[]; self.cur=None; self.steps=steps
    def _moveTo(self,p): self.cur=[p]; self.polys.append(self.cur)
    def _lineTo(self,p): self.cur.append(p)
    def _curveToOne(self,p1,p2,p3):
        p0=self.cur[-1]
        for i in range(1,self.steps+1):
            t=i/self.steps; mt=1-t
            self.cur.append((mt**3*p0[0]+3*mt*mt*t*p1[0]+3*mt*t*t*p2[0]+t**3*p3[0], mt**3*p0[1]+3*mt*mt*t*p1[1]+3*mt*t*t*p2[1]+t**3*p3[1]))
    def _qCurveToOne(self,p1,p2):
        p0=self.cur[-1]
        for i in range(1,self.steps+1):
            t=i/self.steps; mt=1-t
            self.cur.append((mt*mt*p0[0]+2*mt*t*p1[0]+t*t*p2[0], mt*mt*p0[1]+2*mt*t*p1[1]+t*t*p2[1]))
    def _closePath(self): pass
    def _endPath(self): pass
def polys_of(font, name):
    gs=font.getGlyphSet(); p=FlatPen(gs); gs[name].draw(p); return p.polys
def runs_at_y(polys, y):
    xs=[]
    for poly in polys:
        n=len(poly)
        for i in range(n):
            (x0,y0),(x1,y1)=poly[i],poly[(i+1)%n]
            if (y0<=y<y1) or (y1<=y<y0):
                xs.append(x0+(y-y0)*(x1-x0)/(y1-y0))
    xs.sort(); return [(round(xs[i]),round(xs[i+1])) for i in range(0,len(xs)-1,2)]
def runs_at_x(polys, x):
    ys=[]
    for poly in polys:
        n=len(poly)
        for i in range(n):
            (x0,y0),(x1,y1)=poly[i],poly[(i+1)%n]
            if (x0<=x<x1) or (x1<=x<x0):
                ys.append(y0+(x-x0)*(y1-y0)/(x1-x0))
    ys.sort(); return [(round(ys[i]),round(ys[i+1])) for i in range(0,len(ys)-1,2)]


# Exact outline distance (build.py exact_gap), moved here verbatim from the critic's
# v4_clear.py so the build imports no HarfBuzz: v4_clear imported uharfbuzz at module top.

def flatten(path, step=2.0):
    """pathops path -> list of closed polylines (numpy Nx2)."""
    polys, cur, last, start = [], [], None, None
    for verb, args in path:
        if verb == pathops.PathVerb.MOVE:
            if len(cur) > 1:
                polys.append(np.array(cur))
            last = start = args[0]; cur = [last]
        elif verb == pathops.PathVerb.LINE:
            last = args[0]; cur.append(last)
        elif verb == pathops.PathVerb.QUAD:
            c, a = args
            n = max(2, int((math.dist(last, c) + math.dist(c, a)) / step))
            for t in range(1, n + 1):
                t /= n
                cur.append(((1 - t) ** 2 * last[0] + 2 * (1 - t) * t * c[0] + t * t * a[0],
                            (1 - t) ** 2 * last[1] + 2 * (1 - t) * t * c[1] + t * t * a[1]))
            last = a
        elif verb == pathops.PathVerb.CUBIC:
            c1, c2, a = args
            n = max(2, int((math.dist(last, c1) + math.dist(c1, c2) + math.dist(c2, a)) / step))
            for t in range(1, n + 1):
                t /= n; u = 1 - t
                cur.append((u ** 3 * last[0] + 3 * u * u * t * c1[0] + 3 * u * t * t * c2[0] + t ** 3 * a[0],
                            u ** 3 * last[1] + 3 * u * u * t * c1[1] + 3 * u * t * t * c2[1] + t ** 3 * a[1]))
            last = a
        elif verb == pathops.PathVerb.CLOSE:
            if start is not None:
                cur.append(start)
            if len(cur) > 1:
                polys.append(np.array(cur))
            cur = []; last = start
    if len(cur) > 1:
        polys.append(np.array(cur))
    return polys


def segs(polys):
    S = [np.stack([p[:-1], p[1:]], 1) for p in polys if len(p) > 1]
    return np.concatenate(S) if S else np.zeros((0, 2, 2))


def pt_seg_min(P, S):
    """min distance from points P (n,2) to segments S (m,2,2)."""
    if len(P) == 0 or len(S) == 0:
        return 1e9
    best = 1e18
    A = S[:, 0]; B = S[:, 1]; AB = B - A; L2 = (AB ** 2).sum(1); L2[L2 == 0] = 1e-9
    for k in range(0, len(P), 256):
        p = P[k:k + 256, None, :]
        t = (((p - A[None]) * AB[None]).sum(-1) / L2[None]).clip(0, 1)
        q = A[None] + t[..., None] * AB[None]
        d = ((p - q) ** 2).sum(-1).min()
        best = min(best, d)
    return math.sqrt(best)
