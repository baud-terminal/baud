"""a* per ink x ground, by the method baud-render's `under_threshold` uses.

The blend is the census's `over` (baud-present-macos/tests/cells.rs), in f32:
premultiplied ink over the ground byte, rounded to a byte. As the alpha rises
continuously the composite only changes where a channel crosses a half
integer, so the composites an alpha in [0, 1] can give are exactly one per
interval between those breakpoints. The first flashing alpha is the start of
the first interval whose composite makes a WCAG transition from the ground:
general (|dL| >= 0.10, darker < 0.80) or red (u'v' > bound, either state a
saturated red; bound 0.2, halved to 0.1 on a saturated-red ground).

a* = ceil(255 a_f) - 2: the largest byte strictly below a_f, less one step.
255 when nothing flashes."""
import math
import os
import struct
import sys

# thresholds.py sits beside this file.
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import thresholds as T  # noqa: E402


def f32(x):
    return struct.unpack("f", struct.pack("f", x))[0]


def over(ink, ground, a):
    a = f32(a)
    out = []
    for c, d in zip(ink, ground):
        s = f32(f32(c / 255.0) * a)
        dd = f32(d / 255.0)
        v = f32(f32(s + f32(dd * f32(1.0 - a))) * 255.0)
        out.append(int(min(max(math.floor(v + 0.5), 0), 255)))
    return tuple(out)


def breakpoints(ink, ground):
    pts = {0.0, 1.0}
    for c, d in zip(ink, ground):
        if c == d:
            continue
        lo, hi = min(c, d), max(c, d)
        for n in range(lo, hi):
            a = (n + 0.5 - d) / (c - d)
            if 0.0 < a < 1.0:
                pts.add(a)
    return sorted(pts)


def first(ink, ground, rule, bound=0.2):
    lg, ug = T.lum(ground), T.uv(ground)
    pts = breakpoints(ink, ground)
    for a0, a1 in zip(pts, pts[1:]):
        c = over(ink, ground, (a0 + a1) / 2)
        if rule == "general":
            lc = T.lum(c)
            if abs(lc - lg) >= 0.10 and min(lc, lg) < 0.80:
                return a0
        else:
            if math.dist(T.uv(c), ug) > bound and (T.sat_red(c) or T.sat_red(ground)):
                return a0
    return None


def fmt(v):
    return "never" if v is None else f"{v:.4f}"


for g, grgb in T.GROUNDS.items():
    for i, irgb in T.inks(g).items():
        gen = first(irgb, grgb, "general")
        red = first(irgb, grgb, "red", 0.2)
        halved = 0.1 if T.sat_red(grgb) else 0.2
        redh = first(irgb, grgb, "red", halved)
        fs = [x for x in (gen, redh) if x is not None]
        if fs:
            astar = max(math.ceil(min(fs) * 255) - 2, 0)
        else:
            astar = 255
        print(f"{g:8} {i:8} gen={fmt(gen)} red@0.2={fmt(red)} red@{halved}={fmt(redh)} a*={astar} ({astar / 255:.4f})")
