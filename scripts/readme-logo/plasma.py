"""The README logo's plasma: the frames' primitives, for the logo film.

    plasma.py <geometry dir> <dark|light> <on_ms> <off_ms> <out>

The plasma moves round and through the lockup `baud ❯█` as if each glyph
were a solid wall: in front of the letters or behind them, changing side
only through an opening -- the counters of b, a and d, the slots between
the letters, the u's open top, over, under or round the ends -- hidden
behind a letter but for what shows through its holes, and against a
stroke squashed flat and run along it. A swirling current carries it,
and a short tail of wisps slides round the letters.

**The head** follows a chain of moves drawn from a seeded random number
(`Rng`), never a lap: a sweep to a side, a turn inside the word, a drop
through a counter, a weave round a letter through the slots either side,
a graze along a stroke and a hit against one. Each move is timed, laid as
a Hermite spline through its waypoints, and kept only if the head stays
in the envelope, never comes behind the block and never sits in the
letters' plane inside a stroke (`validate`); a director steers the mix
toward the time behind, the time in contact and the counter passes the
targets ask, and the last move closes the chain on its start, so the
chain is a loop of `LOOP` seconds. The head's depth is `ZMAX` in front or
behind and passes through the plane only at an opening; within `CONTACT`
of it the walls act (`front_of`, `squeezed`, `push`). A hit squashes the
head against the stroke and throws two tongues along its contour.

**The current** is Bridson's curl noise: the curl of a stream function
of twelve waves, each periodic in the loop, its potential ramped to zero
at the slabs' 3D distance so the flow runs along a stroke and never into
it (`flow`). It carries the head off the chain, held still round each
crossing and contact (`p2_displacement`), and three strands of parcels
shed from the head, which settle onto the slabs' face and slide along a
stroke they meet; the links between successive parcels are the tail
(`parcel_rope`), each living 0.30 s, longer hugging a stroke. Three
surges a loop brighten it. A faint residual flickers on two iso-lines
round the letters, brighter where the head passes (`residual`).

**What it writes**: the README film's frames, each `F <i> <cs>`, its time
from the film's start in hundredths, then a line a plasma strand,
`C front light bx by fx fy width unit age phase fine k hz ions seed` in
lockup units: its weight in front of the glyphs (the rest behind), its
light, the chord from `b` to `f`, its width and plasma cell, its body's
age and its detail clock (`effect::Live`), its filaments' fine wiggle
(amplitude, cycles a cell and a second), its ions a cell, and its seed.
The film (`baud-app/src/logo_film.rs`) draws each through the renderer's
splash layers.

**The film is 4.5 s of the loop**: nine of the default blink's 500 ms
cycles, which `on_ms` and `off_ms` name and the frame grid is built on
(the frame grid is 20 ms, with 30 ms closing each 250 ms phase), so the
blink plays whole cycles under WCAG 2.2's five seconds. It is the stretch
with the most of the chain's events -- crossings, wall hits, grazes and
surges -- between its fades, of those the one with the least light under
the fades, starting on a blink cycle (`choose_window`). The plasma fades
in over its first 0.3 s and out over its last 0.4 s, so the film starts
and ends on the bare logo, and a last frame at 4.5 s holds the bare logo
with the block shown.

Lockup units (u); the canvas is px = (u + 60) * 2, py = (v + 75) * 2, and
the geometry is `geometry.py`'s. All arithmetic is IEEE's own or `det.py`'s,
so the frames are the same on every platform.
"""

import bisect, json, math, os, sys

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import det  # noqa: E402

if len(sys.argv) != 6 or sys.argv[2] not in ("dark", "light"):
    raise SystemExit("usage: plasma.py <geometry dir> <dark|light> <on_ms> <off_ms> <out>")
GEOM, HALF, OUT = sys.argv[1], sys.argv[2], sys.argv[5]
if (sys.argv[3], sys.argv[4]) != ("250", "250"):
    raise SystemExit(f"plasma: the frame grid is built on 250 ms phases, not {sys.argv[3]} and {sys.argv[4]}")
DARK = HALF == "dark"
LOOP = 10.0
STEP = 0.0125              # a chord every 12.5 ms
NCH = int(round(LOOP / STEP))
ZMAX = 24.0
TAIL = 0.30
TAU = 6.0                  # slab thickness, u
CONTACT = 7.0
P_WIDTH, P_UNIT = 20.0, 30.0   # the head's chord across, and its plasma cell, u
HW = P_WIDTH / 2
GRAZE_C = 8.0              # the head's centre off the stroke while grazing, u
BLOCK = (223.5, 45.0, 244.0, 91.0)
ENVELOPE = (8.0, 18.0, 290.0, 116.0)

W_, H_ = 840, 580


def field(name, dtype):
    a = np.fromfile(os.path.join(GEOM, name), dtype)
    if a.size != W_ * H_:
        raise SystemExit(f"plasma: {name} holds {a.size} values, not {W_ * H_}")
    return a.reshape(H_, W_)


SDF, NX, NY = (field(f"{k}.f32", "<f4").astype(np.float64) for k in ("sdf", "nx", "ny"))
HOLES = field("holes.u8", np.uint8)


def h01(n):
    x = ((n * 0x9E3779B9) & 0xFFFFFFFF) ^ 0x85EBCA6B
    x ^= x >> 16
    x = (x * 0x7FEB352D) & 0xFFFFFFFF
    x ^= x >> 15
    x = (x * 0x846CA68B) & 0xFFFFFFFF
    x ^= x >> 16
    return (x >> 8) / 16777216.0


class Rng:
    def __init__(self, salt):
        self.salt, self.i = salt, 0

    def __call__(self, lo=0.0, hi=1.0):
        self.i += 1
        return lo + (hi - lo) * h01((self.salt * 7919 + self.i * 104729) & 0xFFFFFFFF)

    def pick(self, weights):
        x, acc = self(), 0.0
        tot = sum(w for _, w in weights)
        for name, w in weights:
            acc += w / tot
            if x < acc:
                return name
        return weights[-1][0]


def bil(F, u, v):
    """Field F (canvas px) at lockup (u, v), bilinear."""
    x, y = (u + 60.0) * 2.0, (v + 75.0) * 2.0
    x = min(max(x, 0.0), W_ - 1.001)
    y = min(max(y, 0.0), H_ - 1.001)
    i, j = int(x), int(y)
    fx, fy = x - i, y - j
    return (F[j, i] * (1 - fx) * (1 - fy) + F[j, i + 1] * fx * (1 - fy) + F[j + 1, i] * (1 - fx) * fy + F[j + 1, i + 1] * fx * fy)


def sdf_u(u, v):
    return bil(SDF, u, v) / 2.0


def normal(u, v):
    nx, ny = bil(NX, u, v), bil(NY, u, v)
    l = det.hypot(nx, ny) or 1.0
    return nx / l, ny / l


def in_hole(u, v):
    x, y = int(round((u + 60.0) * 2.0)), int(round((v + 75.0) * 2.0))
    return 0 <= x < W_ and 0 <= y < H_ and HOLES[y, x] > 0


def smooth(e0, e1, x):
    k = min(max((x - e0) / (e1 - e0), 0.0), 1.0)
    return k * k * (3 - 2 * k)


def off_block(x, y):
    dx = max(BLOCK[0] - x, x - BLOCK[2], 0.0)
    dy = max(BLOCK[1] - y, y - BLOCK[3], 0.0)
    return det.hypot(dx, dy)


def ease(x):
    x = min(max(x, 0.0), 1.0)
    y = 1 - x
    return 1 - y * y


def contour_walk(p, sgn, dist, iso_px, step=1.0):
    """Walk the iso-line sdf = iso_px from p, `sgn` along the tangent, `dist` u."""
    pts = [p]
    x, y = p
    for _ in range(int(dist / step)):
        nx, ny = normal(x, y)
        tx, ty = -ny * sgn, nx * sgn
        x, y = x + tx * step, y + ty * step
        for _ in range(3):
            nx, ny = normal(x, y)
            e = (bil(SDF, x, y) - iso_px) / 2.0
            x, y = x - nx * e, y - ny * e
        pts.append((x, y))
    return pts


GLYPH_BOX = {"b": (55, 42.5, 83.5, 88.5), "a": (88.5, 53, 117.5, 88.5), "u": (123.5, 54, 151.5, 88.5),
             "d": (158, 42.5, 186, 88.5), ">": (207.5, 55, 218, 80.5), "#": (223.5, 45, 244, 91)}


def glyph_at(u, v, pad=8):
    best, bd = "?", 1e9
    for g, (x0, y0, x1, y1) in GLYPH_BOX.items():
        dx = max(x0 - u, u - x1, 0.0)
        dy = max(y0 - v, v - y1, 0.0)
        d = det.hypot(dx, dy)
        if d < bd:
            best, bd = g, d
    return best if bd <= pad else "?"


# ---- the openings ----------------------------------------------------------
# kind, centre, ramp (s): a hole is a closed counter, a slot the channel
# between two letters (threaded vertically), the notch the u's open top.

HOLE = {"b": (69.25, 69.0), "a": (103.6, 77.0), "d": (171.75, 69.5)}
NOTCH = (137.8, 62.0)
SLOT = {"ba": 86.0, "au": 120.5, "ud": 154.75, "d>": 196.75}
SLOT_Y = {"ba": 72.0, "au": 72.0, "ud": 72.0, "d>": 68.0}
RAMP = {"hole": 0.15, "slot": 0.12, "notch": 0.14, "end": 0.22, "edge": 0.18}
WEAVE = {"a": ("ba", "au"), "u": ("au", "ud"), "d": ("ud", "d>")}

SLOW = 80.0      # u/s threading a hole
SLOTV = 170.0    # u/s down a channel
WALL_IN = 230.0  # u/s arriving at a wall
HOLD = 0.14      # the squash, s
SLIDE_V = 170.0


def portal_kind(nm):
    if nm in HOLE:
        return "hole"
    if nm in SLOT:
        return "slot"
    if nm == "u":
        return "notch"
    if nm in ("endL", "endR"):
        return "end"
    return "edge"


def hole_way(nm, r, vertical=None, dirx=1.0, down=None):
    cx, cy = HOLE[nm]
    if vertical:
        s = 1.0 if down else -1.0
        jx = r(-1.5, 1.5)
        return [(cx + jx, cy - 30 * s, "slow", nm), (cx + jx, cy - 4 * s, "slow", nm), (cx, cy, "cross", nm),
                (cx - jx, cy + 4 * s, "slow", nm), (cx - jx, cy + 30 * s, "slow", nm)]
    vs = 1.0 if r() < 0.5 else -1.0
    ang_in = vs * math.radians(r(40, 68))
    ang_out = ang_in + math.radians(r(-14, 14))
    din = (dirx * det.cos(ang_in), det.sin(ang_in))
    dout = (dirx * det.cos(ang_out), det.sin(ang_out))
    return [(cx - din[0] * 24, cy - din[1] * 24, "slow", nm), (cx - din[0] * 3, cy - din[1] * 3, "slow", nm), (cx, cy, "cross", nm),
            (cx + dout[0] * 3, cy + dout[1] * 3, "slow", nm), (cx + dout[0] * 26, cy + dout[1] * 26, "slow", nm)]


def slot_way(nm, down):
    cx, cy = SLOT[nm], SLOT_Y[nm]
    ya, yb = (36.0, 100.0) if down else (100.0, 36.0)
    return [(cx, ya, "slot_in", nm), (cx, cy, "cross", nm), (cx, yb, "slot_out", nm)]


def notch_way(r, down):
    cx, cy = NOTCH
    if down:
        return [(cx + r(-2, 2), 34.0, "slow", "u"), (cx, cy, "cross", "u"), (cx + r(-2, 2), 34.0 + r(0, 6), "slow", "u")]
    return [(cx + r(-2, 2), 36.0, "slow", "u"), (cx, cy, "cross", "u"), (cx + r(-2, 2), 30.0, "slow", "u")]


def portal_way(nm, r, dirx, y_now):
    if nm in HOLE:
        if r() < 0.4:
            return hole_way(nm, r, vertical=True, down=y_now < HOLE[nm][1])
        return hole_way(nm, r, dirx=dirx)
    if nm in SLOT:
        return slot_way(nm, down=y_now < SLOT_Y[nm] if r() < 0.8 else r() < 0.5)
    if nm == "u":
        return notch_way(r, down=True)
    if nm == "top":
        x = r(75, 205)
        return [(x, 31.0, "xfree", "top")]
    if nm == "bot":
        x = r(75, 205)
        return [(x, 101.0, "xfree", "bot")]
    if nm == "endL":
        return [(r(33, 40), r(46, 86), "xfree", "endL")]
    if nm == "endR":
        return [(r(262, 270), r(30, 104), "xfree", "endR")]
    raise SystemExit(nm)


def portal_x(nm):
    if nm in HOLE:
        return HOLE[nm][0]
    if nm in SLOT:
        return SLOT[nm]
    return {"u": NOTCH[0], "top": 140.0, "bot": 140.0, "endL": 36.0, "endR": 266.0}[nm]


PORTAL_W = [("b", 0.17), ("a", 0.09), ("d", 0.17), ("u", 0.07), ("ba", 0.07), ("au", 0.08), ("ud", 0.08), ("d>", 0.10),
            ("top", 0.05), ("bot", 0.05), ("endL", 0.05), ("endR", 0.02)]


# ---- the contact points ----------------------------------------------------
def contact_points():
    """Points on the iso-line GRAZE_C round the glyphs, out of the counters
    and clear of the block, with the outward normal, and the free ray."""
    out = []
    iso = GRAZE_C * 2.0
    for v in np.arange(34.0, 100.0, 1.5):
        for u in np.arange(40.0, 232.0, 1.5):
            s = bil(SDF, u, v)
            if abs(s - iso) > 0.8 or in_hole(u, v):
                continue
            nx, ny = normal(u, v)
            if off_block(u, v) < HW + 7.0:
                continue
            ok = all(bil(SDF, u + nx * d, v + ny * d) > iso - 0.6 for d in np.arange(1.0, 14.0, 1.0))
            if not ok:
                continue
            g = glyph_at(u - nx * GRAZE_C, v - ny * GRAZE_C)
            if g in ("#", "?"):
                continue
            out.append((float(u), float(v), nx, ny, g))
    return out


CPTS = contact_points()


# ---- the chain -------------------------------------------------------------
class Chain:
    def __init__(self):
        self.keys = []       # (t, x, y, v or None)
        self.cross = []      # (t, ramp, portal)
        self.contact = []    # (t0, t1)
        self.walls = []      # hits: t, t_hold, t_slide, p, n, glyph
        self.grazes = []     # t0, t1, p, n, glyph, walk
        self.squeeze = []    # (t0, t1, portal)
        self.moves = []
        self.cyclic = False
        self.s0 = 1

    def prepare(self):
        ks = self.keys
        n = len(ks)
        T = [k[0] for k in ks]
        P = [(k[1], k[2]) for k in ks]
        cyc = self.cyclic

        def tp(i):
            if cyc:
                return T[i % n] + LOOP * (i // n), P[i % n]
            i = min(max(i, 0), n - 1)
            return T[i], P[i]
        V = []
        for i in range(n):
            if ks[i][3] is not None:
                V.append(ks[i][3])
                continue
            (ta, pa), (tb, pb), (tc, pc) = tp(i - 1), tp(i), tp(i + 1)
            if tb - ta <= 1e-9 and tc - tb <= 1e-9:
                V.append((0.0, 0.0))
                continue
            if tb - ta <= 1e-9:
                V.append(((pc[0] - pb[0]) / (tc - tb), (pc[1] - pb[1]) / (tc - tb)))
                continue
            if tc - tb <= 1e-9:
                V.append(((pb[0] - pa[0]) / (tb - ta), (pb[1] - pa[1]) / (tb - ta)))
                continue
            v1 = ((pb[0] - pa[0]) / (tb - ta), (pb[1] - pa[1]) / (tb - ta))
            v2 = ((pc[0] - pb[0]) / (tc - tb), (pc[1] - pb[1]) / (tc - tb))
            wa, wb = (tc - tb), (tb - ta)
            V.append(((v1[0] * wa + v2[0] * wb) / (wa + wb), (v1[1] * wa + v2[1] * wb) / (wa + wb)))
        self.T, self.P, self.V, self.n = T, P, V, n
        self.cr = sorted(self.cross)
        self.cr_t = [c[0] for c in self.cr]

    def xy(self, t):
        T, P, V, n = self.T, self.P, self.V, self.n
        if self.cyclic:
            t = t % LOOP
            i = bisect.bisect_right(T, t) - 1
            if i < 0:
                i = n - 1
                ta = T[i] - LOOP
            else:
                ta = T[i]
            j = (i + 1) % n
            tb = T[j] + (LOOP if j <= i else 0.0)
        else:
            i = bisect.bisect_right(T, t) - 1
            if i < 0:
                return P[0]
            if i >= n - 1:
                return P[-1]
            j = i + 1
            ta, tb = T[i], T[j]
        h = tb - ta
        s = (t - ta) / h if h > 0 else 0.0
        (x0, y0), (x1, y1) = P[i], P[j]
        (vx0, vy0), (vx1, vy1) = V[i], V[j]
        s2 = s * s
        s3 = s2 * s
        h00 = 2 * s3 - 3 * s2 + 1
        h10 = s3 - 2 * s2 + s
        h01_ = -2 * s3 + 3 * s2
        h11 = s3 - s2
        return (h00 * x0 + h10 * h * vx0 + h01_ * x1 + h11 * h * vx1, h00 * y0 + h10 * h * vy0 + h01_ * y1 + h11 * h * vy1)

    def sign(self, t):
        if self.cyclic:
            t = t % LOOP
        k = bisect.bisect_right(self.cr_t, t)
        return self.s0 * (-1) ** k

    def z(self, t):
        cr = self.cr
        if self.cyclic:
            t = t % LOOP
        k = bisect.bisect_right(self.cr_t, t)
        s = self.s0 * (-1) ** k
        if cr:
            if self.cyclic:
                prev = cr[k - 1] if k > 0 else (cr[-1][0] - LOOP, cr[-1][1])
                nxt = cr[k] if k < len(cr) else (cr[0][0] + LOOP, cr[0][1])
            else:
                prev = cr[k - 1] if k > 0 else (-10.0, 0.1)
                nxt = cr[k] if k < len(cr) else (t + 10.0, 0.1)
            a = min(ease((t - prev[0]) / prev[1]), ease((nxt[0] - t) / nxt[1]))
        else:
            a = 1.0
        z = s * ZMAX * a
        b = self.contact_w(t)
        if b > 0:
            z *= 1 - (1 - 3.0 / ZMAX) * b
        return z

    def contact_w(self, t):
        best = 0.0
        for (c0, c1) in self.contact:
            for off in ((0.0, LOOP, -LOOP) if self.cyclic else (0.0,)):
                tt = t + off
                if c0 - 0.08 <= tt <= c1 + 0.10:
                    best = max(best, smooth(c0 - 0.08, c0, tt) * (1 - smooth(c1, c1 + 0.10, tt)))
        return best

    def in_contact(self, t, pad=0.04):
        for (c0, c1) in self.contact:
            for off in (0.0, LOOP, -LOOP):
                if c0 - pad <= t + off <= c1 + pad:
                    return True
        return False

    def in_squeeze(self, t):
        for a, b, _ in self.squeeze:
            for off in (0.0, LOOP, -LOOP):
                if a - 0.02 <= t + off <= b + 0.02:
                    return True
        return False


# ---- the moves -------------------------------------------------------------
def side_point(r, side, last_y):
    for _ in range(20):
        y = r(30, 104)
        if last_y.get(side) is None or abs(y - last_y[side]) >= 14:
            break
    x = r(16, 32) if side == "L" else r(258, 282)
    return x, y


def pick_portals(r, x_from, x_to, n, avoid):
    lo, hi = min(x_from, x_to), max(x_from, x_to)
    cand = [(nm, w) for nm, w in PORTAL_W if lo + 4 < portal_x(nm) < hi - 4 and nm not in avoid]
    out = []
    for _ in range(n):
        if not cand:
            break
        nm = r.pick(cand)
        out.append(nm)
        cand = [(a, w) for a, w in cand if a != nm and abs(portal_x(a) - portal_x(nm)) > 25]
    return sorted(out, key=lambda nm: portal_x(nm) * (1 if x_to > x_from else -1))


def mv_sweep(r, st):
    x, y = st["p"]
    here = "L" if x < 147 else "R"
    same = r() < 0.42
    side = here if same else ("R" if here == "L" else "L")
    tx, ty = side_point(r, side, st["last_y"])
    way = []
    if same:
        # in, a turn inside the word, and back out to the same side
        xt = r(85, 205)
        yt = min(max(y + r(-30, 30), 30), 104)
        dirx = 1.0 if xt > x else -1.0
        n = 1 if r() < 0.5 else 0
        for nm in pick_portals(r, x, xt, n, st["recent"]):
            way += portal_way(nm, r, dirx, y)
        way.append((xt, yt, "free", None))
        n = 1 if r() < 0.35 else 0
        for nm in pick_portals(r, xt, tx, n, st["recent"]):
            way += portal_way(nm, r, -dirx, yt)
    else:
        dirx = 1.0 if tx > x else -1.0
        c = r()
        n = 0 if c < 0.4 else 1 if c < 0.85 else 2
        for nm in pick_portals(r, x, tx, n, st["recent"]):
            way += portal_way(nm, r, dirx, y)
        if n == 0:
            way.append((r(80, 200), min(max(y + (ty - y) * 0.5 + r(-22, 22), 30), 104), "free", None))
    way.append((tx, ty, "free", None))
    return way, side


def mv_turn(r, st):
    x, y = st["p"]
    xt = r(75, 210)
    if abs(xt - x) < 30:
        return None
    yt = r(30, 104)
    dirx = 1.0 if xt > x else -1.0
    way = []
    if r() < 0.5:
        for nm in pick_portals(r, x, xt, 1, st["recent"]):
            way += portal_way(nm, r, dirx, y)
    way.append((xt, yt, "free", None))
    xe = xt - dirx * r(14, 40)
    ye = min(max(yt + (r(16, 34) if r() < 0.5 else -r(16, 34)), 28), 106)
    way.append((xe, ye, "free", None))
    return way, None


def mv_drop(r, st):
    x, y = st["p"]
    nm = r.pick([("b", 0.32), ("a", 0.18), ("d", 0.32), ("u", 0.18)])
    if nm in st["recent"]:
        return None
    if nm == "u":
        cx = NOTCH[0]
        way = [(cx + r(-14, 14), r(28, 32), "free", None)] + notch_way(r, True)
        way.append((cx + r(-30, 30), r(28, 40), "free", None))
        return way, None
    cx, cy = HOLE[nm]
    down = y < cy if r() < 0.75 else r() < 0.5
    w = hole_way(nm, r, vertical=True, down=down)
    s = 1.0 if down else -1.0
    way = [(cx + r(-10, 10), cy - 40 * s, "free", None)] + w
    way.append((cx + r(-24, 24), cy + r(38, 46) * s, "free", None))
    return way, None


def mv_weave(r, st):
    x, y = st["p"]
    g = r.pick([("a", 0.34), ("u", 0.33), ("d", 0.33)])
    g1, g2 = WEAVE[g]
    if abs(SLOT[g2] - x) < abs(SLOT[g1] - x):
        g1, g2 = g2, g1
    if g1 in st["recent"] or g2 in st["recent"]:
        return None
    top_first = r() < 0.6
    ya, yb = (36.0, 100.0) if top_first else (100.0, 36.0)
    s = 1.0 if top_first else -1.0
    c1, c2 = SLOT[g1], SLOT[g2]
    gx = 0.5 * (c1 + c2)
    ylow = SLOT_Y[g1] + 12.0 * s
    way = [(c1, ya, "slot_in", g1), (c1, SLOT_Y[g1], "cross", g1), (c1, ylow, "free", None),
           (gx + r(-3, 3), SLOT_Y[g1] + r(10, 16) * s, "free", None),
           (c2, SLOT_Y[g2] + 12.0 * s, "free", None), (c2, SLOT_Y[g2], "cross", g2), (c2, ya, "slot_out", g2)]
    dirx = 1.0 if c2 > c1 else -1.0
    way.append((c2 + dirx * r(10, 34), ya + r(-6, 6) * s, "free", None))
    return way, None


def mv_graze(r, st):
    x, y = st["p"]
    # a contact point not too far, approached from open plane
    cand = [c for c in CPTS if 20 < det.hypot(c[0] - x, c[1] - y) < 150]
    if not cand:
        return None
    c = cand[int(r() * len(cand)) % len(cand)]
    u, v, nx, ny, g = c
    gv = r(110, 170)
    dur = r(0.35, 0.95)
    sg = 1.0 if r() < 0.5 else -1.0
    walk = contour_walk((u, v), sg, gv * dur, GRAZE_C * 2.0, step=1.0)
    keep = [walk[0]]
    for p in walk[1:]:
        if off_block(*p) < HW + 7.0 or in_hole(*p) or abs(bil(SDF, *p) - GRAZE_C * 2) > 1.5:
            break
        keep.append(p)
    if len(keep) < 20:
        return None
    walk = keep
    a = (u + nx * r(12, 18), v + ny * r(12, 18))
    if not inside(a):
        return None
    way = [(a[0], a[1], "contact_in", None)]
    for i in range(6, len(walk), 6):
        way.append((walk[i][0], walk[i][1], "graze", (gv, len(st["grazes"]), g)))
    if way[-1][2] != "graze" or len(way) < 3:
        return None
    ex, ey = way[-1][0], way[-1][1]
    enx, eny = normal(ex, ey)
    tx, ty = -eny * sg, enx * sg
    L = r(14, 30)
    way.append((ex + enx * L + tx * r(0, 14), ey + eny * L + ty * r(0, 14), "graze_end", None))
    if not inside(way[-1]):
        return None
    st["grazes"].append({"p": (u, v), "n": (nx, ny), "glyph": g, "walk": walk, "sg": sg})
    return way, None


def hit_points():
    out = []
    for c in CPTS:
        u, v, nx, ny, g = c
        if max(abs(nx), abs(ny)) < 0.93:
            continue
        ray = 12.0 if abs(nx) > 0.9 else 24.0
        if all(bil(SDF, u + nx * d, v + ny * d) > GRAZE_C * 2 - 0.5 for d in np.arange(1.0, ray, 1.0)):
            out.append((u, v, nx, ny, g, ray))
    return out


HPTS = hit_points()


def mv_hit(r, st):
    x, y = st["p"]
    cand = [c for c in HPTS if 20 < det.hypot(c[0] - x, c[1] - y) < 160]
    if not cand:
        return None
    u, v, nx, ny, g, ray = cand[int(r() * len(cand)) % len(cand)]
    a = (u + nx * ray, v + ny * ray)
    if not inside(a):
        return None
    tx, ty = -ny, nx
    sg = 1.0 if r() < 0.5 else -1.0
    cont = contour_walk((u, v), sg, 0.2 * SLIDE_V, GRAZE_C * 2.0)
    for p in cont:
        if off_block(*p) < HW + 7.0 or in_hole(*p):
            return None
    way = [(a[0], a[1], "contact_in", None), (u, v, "wall", (nx, ny, g)), (u, v, "hold", (nx, ny, g)),
           (cont[len(cont) // 2][0], cont[len(cont) // 2][1], "slide", None), (cont[-1][0], cont[-1][1], "slide_end", None)]
    ex, ey = cont[-1]
    enx, eny = normal(ex, ey)
    way.append((ex + enx * r(16, 30) + tx * sg * r(0, 12), ey + eny * r(16, 30) + ty * sg * r(0, 12), "free", None))
    if not inside(way[-1]):
        return None
    return way, None


MOVES = {"sweep": mv_sweep, "turn": mv_turn, "drop": mv_drop, "weave": mv_weave, "graze": mv_graze, "hit": mv_hit}


def inside(p, m=4.0):
    return ENVELOPE[0] + m <= p[0] <= ENVELOPE[2] - m and ENVELOPE[1] + m <= p[1] <= ENVELOPE[3] - m


def time_way(way, start, t0, dur, lo=90.0, speed=None):
    """Times for each waypoint after `start`; fixed-speed segments first,
    the free ones share what is left. None if the speed is out of range."""
    pts = [(start[0], start[1], "start", None)] + way
    fixed_t, free_len, segs = 0.0, 0.0, []
    for a, b in zip(pts, pts[1:]):
        L = det.hypot(b[0] - a[0], b[1] - a[1])
        if b[2] == "hold":
            dt = HOLD
        elif b[2] in ("slide", "slide_end"):
            dt = L / SLIDE_V
        elif b[2] == "graze":
            dt = L / b[3][0]
        elif b[2] == "cross" or a[2] == "cross":
            slot = (b[2] == "cross" and a[2] == "slot_in") or (a[2] == "cross" and b[2] == "slot_out")
            dt = L / (SLOTV if slot else SLOW)
        elif a[2] == "slow" and b[2] == "slow":
            dt = L / (1.6 * SLOW)
        elif b[2] == "wall":
            dt = L / WALL_IN
        elif b[2] == "slot_in" and a[2] == "slot_out":
            dt = None
            free_len += L
        else:
            dt = None
            free_len += L
        if dt is not None:
            fixed_t += dt
        segs.append((a, b, L, dt))
    if speed is not None:
        dur = fixed_t + free_len / speed
    left = dur - fixed_t
    if left < 0.06:
        return None
    speed = free_len / left
    if not (lo <= speed <= 760.0):
        return None
    t = t0
    out = []
    for a, b, L, dt in segs:
        t += dt if dt is not None else L / speed
        out.append((t, b))
    return out, speed


def apply_move(ch, timed, st, kind, t0, side):
    keys, cross, contact, walls, squeeze = [], [], [], [], []
    g_first = g_last = None
    graze_meta = None
    for t, b in timed:
        v = (0.0, 0.0) if b[2] in ("wall", "hold") else None
        keys.append((t, b[0], b[1], v))
        if b[2] in ("cross", "xfree"):
            nm = b[3]
            cross.append((t, RAMP[portal_kind(nm)], nm))
        if b[2] == "slot_in":
            squeeze.append([t, None, b[3]])
        if b[2] == "slot_out" and squeeze:
            squeeze[-1][1] = t
        if b[2] == "wall":
            walls.append({"t": t, "p": (b[0], b[1]), "n": b[3][:2], "glyph": b[3][2]})
        if b[2] == "hold":
            walls[-1]["t_hold"] = t
        if b[2] == "slide_end":
            walls[-1]["t_slide"] = t
        if b[2] == "contact_in":
            t_in = t
        if b[2] == "graze":
            if g_first is None:
                g_first = t
                graze_meta = b[3]
            g_last = t
    for w in walls:
        contact.append((w["t"] - 0.05, w["t_slide"]))
    graze = None
    if g_first is not None:
        gm = st["grazes"][-1]
        gm.update({"t0": t_in, "t1": g_last})
        contact.append((t_in - 0.01, g_last + 0.02))
        graze = gm
    return keys, cross, contact, walls, [tuple(s) for s in squeeze if s[1] is not None], graze


def validate(ch, t0, t1):
    """Sample the head over [t0, t1]: never behind by the block, never in
    the plane inside a stroke, and inside the envelope."""
    t = t0
    while t <= t1 + 1e-9:
        x, y = ch.xy(t)
        z = ch.z(t)
        if not (ENVELOPE[0] <= x <= ENVELOPE[2] and ENVELOPE[1] <= y <= ENVELOPE[3]):
            return "envelope"
        if z < 3.0 and off_block(x, y) < HW + 4.5:
            return "block"
        if abs(z) < CONTACT and sdf_u(x, y) < 1.2:
            return "ink"
        if abs(z) < CONTACT and in_hole(x, y) is False and sdf_u(x, y) < 1.2:
            return "ink"
        t += 0.005
    return None


def director(ch, st, t, mix, sign_now):
    """The mix, steered by what the chain has so far: contact toward 28%,
    time behind toward 45%, and at least three hole passes, a hit, a
    weave and a turnaround by the end. Still a random pick."""
    w = dict(mix)
    if t < 0.4:
        return list(w.items())
    contact = sum(b - a for a, b in ch.contact)
    cr = sorted(c[0] for c in ch.cross)
    behind, s, last = 0.0, ch.s0, 0.0
    for c in cr:
        if s < 0:
            behind += c - last
        s, last = -s, c
    if s < 0:
        behind += t - last
    if contact < 0.27 * t - 0.15:
        w["graze"] *= 2.5
        w["hit"] *= 1.8
    elif contact > 0.31 * t + 0.15:
        w["graze"] *= 0.25
        w["hit"] *= 0.4
    share = behind / t
    flip = (sign_now > 0 and share < 0.43) or (sign_now < 0 and share > 0.47)
    if flip:
        w["drop"] *= 2.5
        w["sweep"] *= 1.4
    else:
        w["drop"] *= 0.4
    k = {}
    for m in ch.moves:
        k[m["type"]] = k.get(m["type"], 0) + 1
    holes = sum(1 for c in ch.cross if c[2] in HOLE or c[2] == "u")
    late = t / LOOP
    if holes < 3:
        w["drop"] *= 1 + 2.0 * late * (3 - holes)
    for kind in ("hit", "weave", "turn"):
        if k.get(kind, 0) == 0:
            w[kind] *= 1 + 4.0 * late
    return list(w.items())


def make_chain(seed, mix):
    r = Rng(seed)
    ch = Chain()
    y0 = r(52, 82)
    x0 = 24.0
    ch.s0 = 1 if r() < 0.5 else -1
    ch.keys.append((0.0, x0, y0, None))
    st = {"p": (x0, y0), "last_y": {"L": y0}, "recent": [], "grazes": []}
    t = 0.0
    k = 0
    fails = {}
    while True:
        remaining = LOOP - t
        closing = remaining < 2.0
        ok = False
        for attempt in range(60):
            rp = Rng(seed * 1009 + k * 97 + attempt)
            sign_now = ch.s0 * (-1) ** len(ch.cross)
            if closing:
                kind = "close"
                dur = remaining
                x, y = st["p"]
                way = []
                if sign_now != ch.s0:
                    nm = None
                    cand = [(a, w) for a, w in PORTAL_W if a not in st["recent"] and a not in ("endR",) and portal_x(a) < x - 4]
                    if not cand:
                        cand = [("endL", 1.0)]
                    nm = rp.pick(cand)
                    way += portal_way(nm, rp, -1.0, y)
                nd = int(rp() * 3)
                for _ in range(nd):
                    way.append((rp(30, 250), rp(28, 106), "free", None))
                way.append((x0, y0, "free", None))
                side = "L"
            else:
                kind = rp.pick(director(ch, st, t, mix, sign_now))
                ng = len(st["grazes"])
                got = MOVES[kind](rp, st)
                if got is None:
                    while len(st["grazes"]) > ng:
                        st["grazes"].pop()
                    continue
                way, side = got
                u = rp()
                dur = 0.3 + 1.5 * u * u if kind not in ("graze", "hit") else rp(0.5, 1.6)
                if t + dur > LOOP - 0.35:
                    while len(st["grazes"]) > ng:
                        st["grazes"].pop()
                    continue
            ng = len(st["grazes"])
            got = time_way(way, st["p"], t, dur, lo=60.0 if kind == "close" else 90.0, speed=rp(220, 520) if kind in ("graze", "hit") else None)
            if got is not None and kind in ("graze", "hit") and got[0][-1][0] > LOOP - 0.35:
                got = None
            if got is None:
                fails["speed"] = fails.get("speed", 0) + 1
                if kind == "graze":
                    st["grazes"].pop()
                continue
            timed, speed = got
            keys, cross, contact, walls, squeeze, graze = apply_move(ch, timed, st, kind, t, side)
            # the closing move lands on the start key: drop it, the chain is cyclic
            trial = Chain()
            trial.s0 = ch.s0
            trial.keys = ch.keys + keys
            trial.cross = ch.cross + cross
            trial.contact = ch.contact + contact
            trial.prepare()
            why = validate(trial, max(0.0, t - 0.1), timed[-1][0])
            if why:
                fails[why] = fails.get(why, 0) + 1
                if graze:
                    st["grazes"].pop()
                continue
            ok = True
            break
        if not ok:
            raise SystemExit(f"seed {seed}: no move fits at t={t:.2f} closing={closing} {fails}")
        ch.keys += keys
        ch.cross += cross
        ch.contact += contact
        ch.walls += walls
        ch.squeeze += squeeze
        if graze:
            ch.grazes.append(graze)
        x_start = st["p"][0]
        t_end = timed[-1][0]
        x_end = keys[-1][1]
        ch.moves.append({"type": kind, "t0": round(t, 4), "t1": round(t_end, 4), "dur": round(t_end - t, 3),
                         "x0": round(x_start, 1), "x1": round(x_end, 1), "speed": round(speed, 1),
                         "portals": [c[2] for c in cross], "walls": [w["glyph"] for w in walls], "graze": graze["glyph"] if graze else None})
        st["p"] = (keys[-1][1], keys[-1][2])
        if side:
            st["last_y"][side] = keys[-1][2]
        st["recent"] = [c[2] for c in cross]
        t = t_end
        k += 1
        if kind == "close":
            break
    last = ch.keys.pop()
    assert abs(last[0] - LOOP) < 1e-6 and abs(last[1] - x0) < 1e-9 and abs(last[2] - y0) < 1e-9, last
    assert len(ch.cross) % 2 == 0, "depth parity round the loop"
    ch.cyclic = True
    ch.prepare()
    why = validate(ch, 0.0, LOOP)
    if why:
        raise SystemExit(f"seed {seed}: the closed loop fails: {why}")
    return ch


def stats(ch):
    ts = np.arange(0.0, LOOP, 0.01)
    zs = np.array([ch.z(t) for t in ts])
    behind = float((zs < 0).mean())
    contact = float(np.mean([ch.in_contact(t, pad=0.0) for t in ts]))
    durs = np.array([m["dur"] for m in ch.moves])
    mean = det.fsum_mean(durs)
    cv = math.sqrt(det.fsum_mean([(d - mean) * (d - mean) for d in durs])) / mean
    same = 0
    for m in ch.moves:
        if (m["x0"] < 147) == (m["x1"] < 147):
            same += 1
    holes = sum(1 for c in ch.cross if c[2] in HOLE or c[2] == "u")
    slots = sum(1 for c in ch.cross if c[2] in SLOT)
    kinds = {}
    for m in ch.moves:
        kinds[m["type"]] = kinds.get(m["type"], 0) + 1
    return {"behind": round(behind, 3), "contact": round(contact, 3), "moves": len(ch.moves), "dur_cv": round(cv, 3),
            "dur_min": round(float(durs.min()), 3), "dur_max": round(float(durs.max()), 3),
            "same_side": round(same / len(ch.moves), 3), "holes": holes, "slots": slots, "crossings": len(ch.cross),
            "grazes": len(ch.grazes), "hits": len(ch.walls), "kinds": kinds}


# ---- the seed, the mix of moves, and the targets --------------------------
PROFILE = {
    "p2": {"seed": 2241, "mix": [("sweep", 0.15), ("turn", 0.10), ("drop", 0.14), ("weave", 0.12), ("graze", 0.35), ("hit", 0.15)]},
}
TARGET = {"behind": (0.40, 0.50), "contact": (0.25, 0.32), "same_side": (0.30, 0.60), "dur_cv": (0.30, 9.0), "holes": (3, 99),
          "weaves": (1, 99), "hits": (1, 99)}


def fits(s):
    k = s["kinds"]
    return (TARGET["behind"][0] <= s["behind"] <= TARGET["behind"][1] and TARGET["contact"][0] <= s["contact"] <= TARGET["contact"][1]
            and TARGET["same_side"][0] <= s["same_side"] <= TARGET["same_side"][1] and s["dur_cv"] >= TARGET["dur_cv"][0]
            and s["holes"] >= TARGET["holes"][0] and k.get("weave", 0) >= 1 and s["hits"] >= 1 and k.get("turn", 0) >= 1)


PROF = PROFILE["p2"]
CH, STATS, seed = None, None, None
for sd in range(PROF["seed"], PROF["seed"] + 400):
    try:
        c = make_chain(sd, PROF["mix"])
    except SystemExit:
        continue
    s_ = stats(c)
    if fits(s_):
        CH, STATS, seed = c, s_, sd
        break
if CH is None:
    raise SystemExit("plasma: no chain from its seed meets the targets")


def head_z(t):
    return CH.z(t)


def head_v(t):
    a, b = CH.xy(t - 0.005), CH.xy(t + 0.005)
    return ((b[0] - a[0]) / 0.01, (b[1] - a[1]) / 0.01)


# ---- surges: three a loop, at seeded times, 2 s apart at least -------------
r_s = Rng(9001 + seed)
SURGES = []
while len(SURGES) < 3:
    x = r_s(0.3, 9.7)
    if all(min(abs(x - y), LOOP - abs(x - y)) >= 2.0 for y in SURGES):
        SURGES.append(x)
SURGES.sort()


def surge_shape(dt):
    if dt < -1.0:
        return 0.0
    if dt < 0.0:
        x = dt + 1.0
        return 0.25 * x * x
    if dt < 0.12:
        x = dt / 0.12
        return 0.25 + 0.4 * x * x * (3 - 2 * x)
    return 0.65 * det.exp(-(dt - 0.12) / 0.35)


def surge(t):
    s = 0.0
    for r0 in SURGES:
        dt = t - r0
        dt -= LOOP * math.floor(dt / LOOP + 0.5)
        s += surge_shape(dt)
    return s


def in_surge(t):
    for r0 in SURGES:
        dt = t - r0
        dt -= LOOP * math.floor(dt / LOOP + 0.5)
        if 0.0 <= dt < 0.4:
            return True
    return False


# ---- the current ------------------------------------------------------------
rw = Rng(424242 + seed)
WAVES = []
for lam, A in ((30.0, 1.0), (15.0, 0.55), (8.0, 0.3)):
    for m in range(4):
        th = 2 * math.pi * rw()
        WAVES.append((det.cos(th) / lam, det.sin(th) / lam, int(4 + rw() * 14) * (1 if rw() < 0.5 else -1), rw(), A * lam))


def psi(x, y, t):
    """The stream function, exactly periodic in 10 s (vectorised)."""
    s = 0.0
    for (a, b, j, ph, A) in WAVES:
        s = s + A * det.sin(2 * np.pi * (a * x + b * y + j * t / LOOP + ph))
    return s


def sdf_field(x, y):
    px = np.clip((np.asarray(x) + 60.0) * 2.0, 0, W_ - 1.001)
    py = np.clip((np.asarray(y) + 75.0) * 2.0, 0, H_ - 1.001)
    i, j = px.astype(int), py.astype(int)
    fx, fy = px - i, py - j
    F = SDF
    return (F[j, i] * (1 - fx) * (1 - fy) + F[j, i + 1] * fx * (1 - fy) + F[j + 1, i] * (1 - fx) * fy + F[j + 1, i + 1] * fx * fy)


def nrm_field(x, y):
    px = np.clip(np.round((np.asarray(x) + 60.0) * 2.0).astype(int), 0, W_ - 1)
    py = np.clip(np.round((np.asarray(y) + 75.0) * 2.0).astype(int), 0, H_ - 1)
    return NX[py, px], NY[py, px]


def ramp(r):
    r = np.clip(r, 0.0, 1.0)
    r3 = r * r * r
    return 15 / 8 * r - 10 / 8 * r3 + 3 / 8 * (r3 * r * r)


FLOW_K = 1.0


def flow(x, y, z, t, rt=1.5):
    """Bridson's curl noise, the potential ramped to zero at the slabs'
    3D distance so the flow runs along them, never into them."""
    def pp(xx, yy):
        a = np.maximum(sdf_field(xx, yy) / 2.0 - rt, 0.0)
        b = np.maximum(np.abs(z) - TAU / 2, 0.0)
        Dd = np.sqrt(a * a + b * b)
        return ramp(Dd / 10.0) * psi(xx, yy, t)
    h = 0.5
    dpdy = (pp(x, y + h) - pp(x, y - h)) / (2 * h)
    dpdx = (pp(x + h, y) - pp(x - h, y)) / (2 * h)
    return FLOW_K * dpdy, -FLOW_K * dpdx


# scale the flow to an rms speed of 170 u/s over the logo
gx, gy = np.meshgrid(np.arange(30, 270, 4.0), np.arange(30, 104, 4.0))
fx, fy = flow(gx.ravel(), gy.ravel(), np.full(gx.size, 20.0), np.zeros(gx.size))
FLOW_K = 170.0 / math.sqrt(det.fsum_mean(fx * fx + fy * fy))


def p2_displacement():
    """The head carried by the flow: D' = flow(H + D) - 2.5 D on a 5 ms
    grid over one lap from D = 0; the flow is not contracting, so the lap's
    end misses its start, and the miss is taken off linearly (D - t/10 s *
    D(10 s)) so the loop closes. Held to zero round each crossing and
    contact."""
    dt = 0.005
    n = int(round(LOOP / dt))
    D = np.zeros(2)
    out = np.zeros((n + 1, 2))
    for i in range(n):
        t = i * dt
        hx, hy = CH.xy(t)
        z = CH.z(t)
        fx, fy = flow(np.array([hx + D[0]]), np.array([hy + D[1]]), np.array([z]), np.array([t]))
        D = D + dt * (np.array([fx[0], fy[0]]) - 2.5 * D)
        out[i + 1] = D
    miss = out[n].copy()
    out = out[:n] - (np.arange(n) / n)[:, None] * miss[None, :]
    g = np.ones(n)
    for i in range(n):
        t = i * dt
        near = min([abs(((t - c[0]) + LOOP / 2) % LOOP - LOOP / 2) for c in CH.cross] + [9.0])
        w = smooth(0.06, 0.22, near)
        if CH.in_contact(t, pad=0.08) or CH.in_squeeze(t):
            w = 0.0
        g[i] = w
    # a light low-pass on the gate so it never steps
    w21 = 1.0 / 21.0
    gs = np.array([math.fsum(g[(i + d) % n] * w21 for d in range(-10, 11)) for i in range(n)])
    return out * gs[:, None]


DISP = p2_displacement()


def head_draw(t):
    """Where the head is drawn: the chain, carried by the current."""
    x, y = CH.xy(t)
    i = (t % LOOP) / 0.005
    i0 = int(math.floor(i)) % len(DISP)
    i1 = (i0 + 1) % len(DISP)
    f = i - math.floor(i)
    d = DISP[i0] * (1 - f) + DISP[i1] * f
    return x + d[0], y + d[1]


# ---- the frames ------------------------------------------------------------
GAIN = 0.80 if DARK else 0.78


def fmt(*v):
    return " ".join(f"{x:.4f}" if isinstance(x, float) else str(x) for x in v)


def front_of(z, x, y, t):
    """A chord's front weight off its frozen z; in the plane against a
    stroke it is drawn behind (the keyline parts it from the stroke), except
    by the block, where nothing is ever behind."""
    f = smooth(-3.0, 3.0, z)
    if CH.in_contact(t % LOOP) and abs(z) < CONTACT:
        f = 0.0 if off_block(x, y) > HW + 5 else 1.0
    if f < 1.0 and off_block(x, y) < HW + 4.5:
        f = 1.0
    return f


def squeezed(x, y, z, t, width):
    """In the plane, or threading a channel, the chord fits the opening:
    its width min(w, sdf - keyline) and its light sqrt(w / w0)."""
    tt = t % LOOP
    if abs(z) < CONTACT or CH.in_squeeze(tt):
        s_px = bil(SDF, x, y)
        k = min(2.0, 0.3 * max(s_px, 0.0))
        w = max(2.5, min(width, s_px - k))
        return w, math.sqrt(w / width)
    return width, 1.0


def push(x, y, z, t, r_u):
    """Out of the ink, against a stroke only."""
    if abs(z) >= CONTACT or not CH.in_contact(t % LOOP):
        return x, y
    s = bil(SDF, x, y)
    need = r_u * 0.6 * 2 + 1.5
    if s < need:
        nx, ny = normal(x, y)
        d = (need - s) / 2.0
        return x + nx * d, y + ny * d
    return x, y


def block_dim(x, y, front):
    """In front over the block, the plasma's body would take its amber
    away: a chord whose body reaches the block is dimmed toward 0.12 of
    itself, so the block stays amber when shown."""
    if front <= 0.5:
        return 1.0
    return 0.12 + 0.88 * smooth(1.0, HW + 6.0, off_block(x, y))


def chain_chords(t_ms2, out, tail=TAIL):
    """The head and its tail: chord k laid at k * 12.5 ms, its frozen depth,
    oldest first. t_ms2: the frame's time in half-milliseconds."""
    t = t_ms2 / 2000.0
    klast = t_ms2 // 25
    kfirst = klast - int(tail / STEP)
    for k in range(kfirst, klast + 1):
        age2 = t_ms2 - 25 * k
        age = age2 / 2000.0
        if age < 0 or age >= tail:
            continue
        kk = k % NCH
        s = kk * STEP
        newest = age < STEP
        sa = s - 0.015
        sb = (s + 0.015) if not newest else (s + age)
        b = head_draw(sa)
        f = head_draw(sb)
        z = head_z(s)
        wall = None
        for w in CH.walls:
            for off in (0.0, -LOOP, LOOP):
                if w["t"] - 0.005 <= s + off <= w["t_hold"] + 0.01:
                    wall = (w, s + off - w["t"])
        width = P_WIDTH
        light_k = 1.0
        if wall is not None:
            w, tau = wall
            nx, ny = w["n"]
            tx, ty = -ny, nx
            e = ease(tau / 0.10)
            st = 1.0 + 1.6 * e
            L = 0.5 * P_WIDTH * st
            px_, py_ = w["p"]
            px_, py_ = px_ - nx * 3.0 * e, py_ - ny * 3.0 * e
            b = (px_ - tx * L, py_ - ty * L)
            f = (px_ + tx * L, py_ + ty * L)
            width = P_WIDTH * (1.0 - 0.5 * e)
            light_k = 1.35
        else:
            b = push(b[0], b[1], z, s, HW)
            f = push(f[0], f[1], z, s, HW)
            mx, my = 0.5 * (b[0] + f[0]), 0.5 * (b[1] + f[1])
            width, light_k = squeezed(mx, my, z, s, P_WIDTH)
        mx, my = 0.5 * (b[0] + f[0]), 0.5 * (b[1] + f[1])
        front = front_of(z, mx, my, s)
        weight = det.exp(-age / 0.07) + 0.6 * det.exp(-age / 0.04)
        weight *= block_dim(mx, my, front)
        p_age = 0.06 + 0.045 * age
        e = 1 - p_age / 0.55
        env = e * e
        light = GAIN * weight / env * light_k * (1.0 + surge(s if not newest else t % LOOP))
        young = max(0.0, 1.0 - age / 0.35)
        ions = 0.6 * (2.0 if in_surge(s) else 1.0)
        out.append("C " + fmt(front, light, b[0], b[1], f[0], f[1], width, P_UNIT, p_age, age, 0.1 * young * young, 1.4, 5.0, ions, 0xE70000 + kk))
        if wall is not None and tau > 0.02:
            # the bulge: the squashed mass lips up and away at both ends
            w, tau = wall
            nx, ny = w["n"]
            tx, ty = -ny, nx
            e = ease(tau / 0.10)
            for sgn in (-1.0, 1.0):
                a0 = (b if sgn < 0 else f)
                a1 = (a0[0] + sgn * tx * 5.0 * e + nx * 9.0 * e, a0[1] + sgn * ty * 5.0 * e + ny * 9.0 * e)
                out.append("C " + fmt(front, light * 0.7, a0[0], a0[1], a1[0], a1[1], 0.45 * P_WIDTH, 0.6 * P_UNIT, p_age, age, 0.1, 1.4, 5.0, ions, 0xE80000 + kk * 2 + (sgn > 0)))


def tongue_paths():
    """Each contact's two tongues, along the stroke both ways, at a close iso."""
    out = []
    iso = GRAZE_C * 2.0 - 7.0
    for i, w in enumerate(CH.walls):
        pa = contour_walk(w["p"], 1.0, 80, iso)
        pb = contour_walk(w["p"], -1.0, 80, iso)
        out.append({"t": w["t"], "life": 0.36, "speed": 320.0, "paths": (pa, pb), "gain": 1.3, "id": i})
    for i, g in enumerate(CH.grazes):
        # ahead of the graze, the light runs on along the stroke; behind it, a shorter one
        p = g["walk"][0]
        pa = contour_walk(p, g["sg"], 90, iso)
        pb = contour_walk(p, -g["sg"], 45, iso)
        out.append({"t": g["t0"] + 0.02, "life": 0.34, "speed": 300.0, "paths": (pa, pb), "gain": 1.0, "id": 100 + i})
    for c in out:
        for k in range(2):
            pts = c["paths"][k]
            cut = len(pts)
            for j, p in enumerate(pts):
                if off_block(*p) < 10.0 or in_hole(*p):
                    cut = j
                    break
            c["paths"] = tuple(c["paths"][m] if m != k else pts[:cut] for m in range(2))
    return out


TONGUES = tongue_paths()


def tongues(t, out):
    for c in TONGUES:
        for off in (0.0, LOOP, -LOOP):
            age = t - (c["t"] + off)
            if not (0.0 <= age < c["life"]):
                continue
            for side, pts in enumerate(c["paths"]):
                reach = c["speed"] * age * (1 - 0.5 * age / c["life"])
                tip = min(int(reach), len(pts) - 1)
                fade = (1 - smooth(0.55 * c["life"], c["life"], age)) * smooth(0.0, 0.03, age)
                if tip < 2:
                    continue
                for m in range(7):
                    a_i = max(0, tip - (m + 1) * 6)
                    b_i = max(0, tip - m * 6)
                    if b_i <= a_i:
                        break
                    pa, pb = pts[a_i], pts[b_i]
                    k = 1.0 - 0.6 * m / 7.0
                    light = GAIN * c["gain"] * fade * k * (0.6 if side == 1 else 1.0)
                    width = 7.0
                    out.append("C " + fmt(0.0, light, pa[0], pa[1], pb[0], pb[1], width, 12.0, 0.06 + 0.03 * age, age, 0.05, 1.4, 5.0, 0.6,
                                          0xEA0000 + (c["id"] * 16 + side * 8 + m) % 65536))


# ---- the residual: energy clinging to the letters' outline -----------------
def residual_segments():
    """Points on two iso-lines (3.5 u and 6.5 u off the strokes, and inside
    the counters), 5 u apart; each its own seeded flickers."""
    segs = []
    for iso_u, spacing in ((3.5, 5.5), (6.5, 9.0)):
        iso = iso_u * 2.0
        ys, xs = np.nonzero(np.abs(SDF - iso) < 0.5)
        us, vs = xs / 2.0 - 60.0, ys / 2.0 - 75.0
        keep = (us > 40) & (us < 236) & (vs > 30) & (vs < 104)
        us, vs = us[keep], vs[keep]
        order = np.lexsort((us, vs))
        taken = []
        grid = {}
        for i in order:
            u, v = float(us[i]), float(vs[i])
            if off_block(u, v) < 9.0:
                continue
            gk = (int(u // spacing), int(v // spacing))
            close = False
            for dx in (-1, 0, 1):
                for dy in (-1, 0, 1):
                    for (a, b) in grid.get((gk[0] + dx, gk[1] + dy), []):
                        if (a - u) * (a - u) + (b - v) * (b - v) < spacing * spacing:
                            close = True
            if close:
                continue
            grid.setdefault(gk, []).append((u, v))
            taken.append((u, v, iso_u))
        segs += taken
    out = []
    for j, (u, v, iso_u) in enumerate(segs):
        r = Rng(0x5E0000 + j + 7919 * seed)
        nx, ny = normal(u, v)
        n = 6 + int(r() * 10)
        ev = []
        for m in range(n):
            ev.append((r(0.0, LOOP), r(0.16, 0.6), r(0.5, 1.0), r(-18, 18), r(5.0, 9.0)))
        out.append({"p": (u, v), "t": (-ny, nx), "ev": ev, "id": j, "inner": in_hole(u, v)})
    return out


RES = residual_segments()
RES_GAIN = 0.20 if DARK else 0.14


def residual(t, out):
    hx, hy = head_draw(t)
    for sgm in RES:
        u, v = sgm["p"]
        tx, ty = sgm["t"]
        near = 1.0 + 1.6 * smooth(34.0, 10.0, det.hypot(u - hx, v - hy))
        for m, (t0, life, amp, crawl, L) in enumerate(sgm["ev"]):
            for off in (0.0, LOOP, -LOOP):
                age = t - (t0 + off)
                if not (0.0 <= age < life):
                    continue
                w = det.sin(math.pi * age / life)
                w = w * w
                light = RES_GAIN * amp * w * near
                if light < 2e-3:
                    continue
                c = (u + tx * crawl * age, v + ty * crawl * age)
                b = (c[0] - tx * L * 0.5, c[1] - ty * L * 0.5)
                f = (c[0] + tx * L * 0.5, c[1] + ty * L * 0.5)
                out.append("C " + fmt(0.0, light, b[0], b[1], f[0], f[1], 3.0, 6.0, 0.06 + 0.04 * age, age, 0.03, 1.4, 5.0, 0.3,
                                      0xE50000 + (sgm["id"] * 16 + m) % 65536))


NSTRAND = 3
STRAND_OFF = (0.0, -7.0, 7.0)
PLIFE = 0.56


def simulate_parcels():
    """Three strands of parcels, one every 12.5 ms each, off the head and
    7 u either side of it; each advected by the bounded flow for its life,
    settling onto the slabs' face and sliding along a stroke where it meets
    one there. Positions every 5 ms; column j = strand * NCH + k."""
    births = np.tile(np.arange(NCH) * STEP, NSTRAND)
    off = np.repeat(np.array(STRAND_OFF), NCH)
    steps = int(PLIFE / 0.005) + 1
    n = len(births)
    hp = np.array([head_draw(b) for b in births[:NCH]])
    v0 = np.array([head_v(b) for b in births[:NCH]])
    sp = np.maximum(det.hypot(v0[:, 0], v0[:, 1]), 1e-6)
    nxh, nyh = -v0[:, 1] / sp, v0[:, 0] / sp
    x = np.tile(hp[:, 0], NSTRAND) + off * np.tile(nxh, NSTRAND)
    y = np.tile(hp[:, 1], NSTRAND) + off * np.tile(nyh, NSTRAND)
    z = np.tile(np.array([head_z(b) for b in births[:NCH]]), NSTRAND)
    vx, vy = 0.3 * np.tile(v0[:, 0], NSTRAND), 0.3 * np.tile(v0[:, 1], NSTRAND)
    X = np.zeros((steps, n)); Y = np.zeros((steps, n)); Z = np.zeros((steps, n)); HUG = np.zeros((steps, n))
    X[0], Y[0], Z[0] = x, y, z
    for s in range(1, steps):
        age = (s - 1) * 0.005
        tt = (births + age) % LOOP
        dec = det.exp(-age / 0.06)
        fx, fy = flow(x, y, z, tt)
        ux = fx + vx * dec
        uy = fy + vy * dec
        sd = sdf_field(x, y) / 2.0
        inplane = np.abs(z) < CONTACT
        nx_, ny_ = nrm_field(x, y)
        into = ux * nx_ + uy * ny_
        wall = inplane & (sd < 6.0) & (into < 0)
        # the wall: the normal part taken off, the rest kept, so it slides along
        ux = np.where(wall, ux - into * nx_, ux)
        uy = np.where(wall, uy - into * ny_, uy)
        x = x + 0.005 * ux
        y = y + 0.005 * uy
        sd = sdf_field(x, y) / 2.0
        bad = inplane & (sd < 2.5)
        nx_, ny_ = nrm_field(x, y)
        x = np.where(bad, x + nx_ * (2.5 - sd), x)
        y = np.where(bad, y + ny_ * (2.5 - sd), y)
        # the depth settles onto the slabs' face, never through it
        zt = np.sign(z) * np.maximum(np.abs(z) * det.exp(-0.005 / 0.08), 3.0)
        z = np.where(np.abs(z) > 3.0, zt, z)
        X[s], Y[s], Z[s] = x, y, z
        HUG[s] = (np.abs(z) < CONTACT) & (sdf_field(x, y) / 2.0 < 7.0)
    return births, X, Y, Z, HUG


PB, PX, PY, PZ, PH = simulate_parcels()


def parcel_rope(t_ms2, out):
    """The tail as three ropes through the parcels, newest first, each link
    a chord tapering with age; a rope breaks where the flow has torn two
    parcels apart. A link hugging a stroke lives longer and brighter."""
    t = t_ms2 / 2000.0
    klast = t_ms2 // 25
    for strand in range(NSTRAND):
        prev = None
        base_w = 12.0 if strand == 0 else 8.0
        gain = 1.0 if strand == 0 else 0.7
        for k in range(klast, klast - int(PLIFE / STEP) - 1, -1):
            age = (t_ms2 - 25 * k) / 2000.0
            if age < 0 or age >= PLIFE:
                prev = None
                continue
            kk = k % NCH
            j = strand * NCH + kk
            si = min(int(round(age / 0.005)), PX.shape[0] - 1)
            p = (float(PX[si, j]), float(PY[si, j]))
            zz = float(PZ[si, j])
            hug = float(PH[max(si - 8, 0):si + 1, j].mean()) if si > 0 else 0.0
            if prev is not None:
                q, qz = prev
                d = det.hypot(p[0] - q[0], p[1] - q[1])
                life = 0.30 + 0.25 * hug
                fade = max(0.0, 1.0 - age / life)
                if 0.2 < d < 14.0 and fade > 0.01:
                    weight = (det.exp(-age / 0.10) + 0.5 * det.exp(-age / 0.04)) * fade + 0.45 * hug * fade
                    width = base_w * (1.0 - 0.45 * min(age / 0.35, 1.0))
                    zm = 0.5 * (zz + qz)
                    mx, my = 0.5 * (p[0] + q[0]), 0.5 * (p[1] + q[1])
                    front = smooth(-3.0, 3.0, zm)
                    if abs(zm) < CONTACT:
                        front = 0.0
                        s_px = bil(SDF, mx, my)
                        width = max(2.5, min(width, s_px - min(2.0, 0.3 * s_px)))
                    if front < 1.0 and off_block(mx, my) < HW + 4.5:
                        front = 1.0
                    ob = off_block(mx, my)
                    if ob < 3.0:
                        prev = (p, zz)
                        continue
                    weight *= block_dim(mx, my, front)
                    p_age = 0.06 + 0.045 * age
                    e = 1 - p_age / 0.55
                    env = e * e
                    light = GAIN * gain * weight / env * (1.0 + surge(t % LOOP))
                    young = max(0.0, 1.0 - age / 0.35)
                    ions = 0.6 * (2.0 if in_surge(t % LOOP) else 1.0)
                    out.append("C " + fmt(front, light, p[0], p[1], q[0], q[1], width, 0.7 * P_UNIT, p_age, age, 0.12 * young * young, 1.4, 5.0, ions,
                                          0xE70000 + (strand * NCH + kk) % 65536))
            prev = (p, zz)


def frame_lines(t_ms2):
    t = (t_ms2 % 20000) / 2000.0
    out = []
    residual(t, out)
    tongues(t, out)
    # the head itself, the newest 40 ms, then the rope the flow carries
    chain_chords(t_ms2, out, tail=0.05)
    parcel_rope(t_ms2, out)
    return out


FADE_IN, FADE_OUT = 0.3, 0.4
FILM_CS = 450


def phase_times_cs(a):
    """The frames of the 250 ms phase from `a` cs: every 20 ms, the last 30."""
    out, tt = [], a
    while tt < a + 25:
        out.append(tt)
        tt += 3 if (a + 25 - tt) == 3 else 2
    return out


def light_of(lines):
    """A frame's light: each strand's light by its width, the residual's
    flicker left out."""
    return math.fsum(float(x[2]) * float(x[7]) for x in (l.split() for l in lines) if x[8] != "6.0000")


def events():
    """The chain's events in the loop: its crossings, wall hits, grazes and
    surges, seconds."""
    return [c[0] for c in CH.cross] + [w["t"] for w in CH.walls] + [g["t0"] for g in CH.grazes] + SURGES


def choose_window(light):
    """The film's start in the loop, cs: of the starts on a blink cycle,
    the one with the most events between its fades, then the least light
    under them."""
    best = None
    for start in range(0, 1000, 50):
        n = sum(1 for e in events() if FADE_IN <= (e - start / 100.0) % LOOP < FILM_CS / 100.0 - FADE_OUT)
        ends = [cs for cs in light if (cs - start) % 1000 < FADE_IN * 100 or (cs - start) % 1000 >= FILM_CS - FADE_OUT * 100]
        under = math.fsum(light[cs] for cs in ends if (cs - start) % 1000 < FILM_CS)
        key = (n, -under, -start)
        if best is None or key > best[0]:
            best = (key, start)
    return best[1], best[0][0]


def envelope(t):
    """The plasma's fade, `t` seconds into the film."""
    return smooth(0.0, FADE_IN, t) * (1.0 - smooth(FILM_CS / 100.0 - FADE_OUT, FILM_CS / 100.0, t))


def scaled(line, k):
    """A strand's line with its light times `k`."""
    x = line.split()
    x[2] = f"{float(x[2]) * k:.4f}"
    return " ".join(x)


def main():
    loop = [cs for a in range(0, 1000, 25) for cs in phase_times_cs(a)]
    frames = {cs: frame_lines(cs * 20) for cs in loop}
    start, n = choose_window({cs: light_of(v) for cs, v in frames.items()})
    film = [cs for a in range(0, FILM_CS, 25) for cs in phase_times_cs(a)]
    with open(OUT, "w") as f:
        for i, cs in enumerate(film):
            k = envelope(cs / 100.0)
            f.write(f"F {i} {cs}\n")
            for l in frames[(start + cs) % 1000]:
                if float(l.split()[2]) * k >= 1e-4:
                    f.write(scaled(l, k) + "\n")
        f.write(f"F {len(film)} {FILM_CS}\n")
    json.dump({"seed": seed, "start_cs": start, "events": n, "frames": len(film) + 1, "stats": STATS}, sys.stdout)
    print()


main()
