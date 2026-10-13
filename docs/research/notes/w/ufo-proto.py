#!/usr/bin/env python3
"""UFO trail prototype: the Rust-side pose (a pure function of the ignition and
its age) and the per-pixel parts, written the way the Rust reference in
baud-present-macos/tests/cells.rs would port the shader. Pure Python, PPM out.

Frame coordinates: pixels, y down. `c` is the cell height (the unit), `cw` the
cell width.
"""
import math, struct, sys, os

# ---------------------------------------------------------------- constants
# Saucer geometry, in units of c (cell height), before scale.
HULL_W = 0.55     # half-width of the hull, tip to centre
HULL_H = 0.13     # half-thickness of the hull
ROUND = 0.02      # the hull's rounding: the vesica is built this much smaller
DOME_R = 0.22     # dome radius
DOME_Y = -0.04    # dome centre, y (up is negative)
SMIN_K = 0.03     # fillet between dome and hull
LIGHT_R = 0.045   # rim light radius
LIGHT_X = 0.40    # rim lights' orbit half-width (x = LIGHT_X * sin(phi))
LIGHT_Y = 0.03    # rim lights sit just below the equator
N_LIGHTS = 5
A_SIL = 0.50      # opacity of the silhouette (dome glass)
A_HULL = 0.30     # added on the hull
A_LIGHT = 0.20    # added on a lit rim light
SAUCER_R = 0.62   # reach: radius of the saucer's record around its centre, units

# Motion
BOB = 0.035       # hover bob amplitude, units
BOB_HZ = 2.2
TILT = 0.32       # radians at the reference speed
TILT_V = 10.0     # reference speed, units a second
HOVER = 0.80      # saucer centre above the cursor's top edge, units
ARC_MAX = 0.5     # the swoop's peak sideways displacement, units
ARC_FRAC = 0.12   # ... as a fraction of the path
T_APPEAR = 0.08
T_OPEN = 0.10
T_HOLD = 0.14
T_CLOSE = 0.08
T_LEAVE = 0.18
LEAVE_ACC = 92.0  # units a second squared, away from the cursor
LIGHT_HZ = 0.55   # rim rotation, turns a second: 5 lights -> 2.75 passes/s

# Beam
BEAM_A = 0.34
BEAM_HW0 = 0.14   # half-width at the saucer, units (times scale)
BEAM_HW1 = 0.22   # least half-width at the cursor, units
BEAM_PAD = 0.08   # beyond the cursor's half-width, units
RING_P = 0.22     # ring spacing, units
RING_V = 5.0      # rings a second travelling from saucer to cursor
RING_DEPTH = 0.35

# Contrail
TRAIL_T = 0.10
TRAIL_A = 0.30
TRAIL_RA = 0.055
TRAIL_RB = 0.012

# Puffs (exhaust rings)
PUFFS_PER_UNIT = 2.0
PUFF_DRAG = 5.0
PUFF_A = 0.45

def smoothstep(e0, e1, x):
    t = min(max((x - e0) / (e1 - e0), 0.0), 1.0)
    return t * t * (3.0 - 2.0 * t)

def clamp(x, a, b): return min(max(x, a), b)
def mix(a, b, t): return a + (b - a) * t
def fract(x): return x - math.floor(x)

# PCG hash, fire's
M32 = 0xFFFFFFFF
def pcg(v):
    s = (v * 747796405 + 2891336453) & M32
    w = (((s >> ((s >> 28) + 4)) ^ s) * 277803737) & M32
    return (w >> 22) ^ w
def unit(h): return struct.unpack('<f', struct.pack('<I', (h >> 9) | 0x3f800000))[0] - 1.0

# ---------------------------------------------------------------- flight
class Flight:
    """One flight: from S (with carried velocity V0 and visibility enter) to
    the hover point over `hole`. seed decides the arc's side and the puffs."""
    def __init__(self, S, hole, c, frame_h, seed, V0=None, enter=0.0):
        self.c = c; self.hole = hole; self.seed = seed; self.enter = enter
        L, T, R, B = hole
        hx = (L + R) / 2
        above = T - (HOVER + DOME_R + 0.1) * c >= 0.0
        below_fits = B + (HOVER + HULL_H + 0.1) * c <= frame_h
        self.side = 1.0 if (above or not below_fits) else -1.0  # +1: saucer above, beam down
        self.P = (hx, T - HOVER * c) if self.side > 0 else (hx, B + HOVER * c)
        self.S = S
        dx, dy = self.P[0] - S[0], self.P[1] - S[1]
        ln = math.hypot(dx, dy)
        self.len = ln
        self.T = clamp(0.12 + 0.006 * ln / c, 0.16, 0.32)
        self.w = 4.0 / self.T
        if V0 is None:
            # fresh: the swoop, perpendicular to the path, bulging up
            if ln > 1e-3:
                nx, ny = -dy / ln, dx / ln
                if ny > 0 or (abs(ny) < 1e-6 and (seed & 1)):
                    nx, ny = -nx, -ny
            else:
                nx, ny = 0.0, -1.0
            arc = min(ARC_MAX * c, ARC_FRAC * ln)
            k = arc * self.w * math.e
            self.V0 = (nx * k, ny * k)
        else:
            self.V0 = V0
        self.a_b = 0.85 * self.T
        self.a_c = self.a_b + T_OPEN + T_HOLD
        self.a_d = self.a_c + T_CLOSE
        self.phase = 2 * math.pi * unit(pcg(seed ^ 0x5bd1e995))

    def spring(self, a):
        """Position and velocity of the spring alone (no bob, no leave)."""
        w = self.w; u = math.exp(-w * a)
        ex, ey = self.S[0] - self.P[0], self.S[1] - self.P[1]
        X = (self.P[0] + (ex * (1 + w * a) + self.V0[0] * a) * u,
             self.P[1] + (ey * (1 + w * a) + self.V0[1] * a) * u)
        V = ((-ex * w * w * a + self.V0[0] * (1 - w * a)) * u,
             (-ey * w * w * a + self.V0[1] * (1 - w * a)) * u)
        return X, V

    def pose(self, a):
        c = self.c
        X, V = self.spring(a)
        hov = smoothstep(0.5 * self.T, self.T, a)
        y = X[1] + BOB * c * math.sin(2 * math.pi * BOB_HZ * a + self.phase) * hov
        x = X[0]
        k = clamp((a - self.a_d) / T_LEAVE, 0.0, 1.0)
        tl = max(a - self.a_d, 0.0)
        y -= self.side * 0.5 * LEAVE_ACC * c * tl * tl
        theta = TILT * clamp(V[0] / (TILT_V * c), -1.0, 1.0) \
            + 0.05 * math.sin(2 * math.pi * 1.3 * a + 2.0 * self.phase) * hov
        grow = smoothstep(0.0, T_APPEAR, a)
        vis = mix(self.enter, 1.0, grow) * (1.0 - smoothstep(0.0, 1.0, k))
        scale = mix(0.5 + 0.5 * self.enter, 1.0, grow) * (1.0 - 0.6 * k * k)
        rho = 2 * math.pi * fract(LIGHT_HZ * a + unit(pcg(self.seed)))
        return dict(C=(x, y), cs=math.cos(theta), sn=math.sin(theta), s=scale, vis=vis,
                    cr=math.cos(rho), sr=math.sin(rho), V=V)

    def beam(self, a, pose):
        c = self.c; L, T, R, B = self.hole
        s = pose['s']
        yT = pose['C'][1] + self.side * HULL_H * c * s * 0.9
        yE = T if self.side > 0 else B
        fo = smoothstep(self.a_b, self.a_b + T_OPEN, a)
        g = smoothstep(self.a_c, self.a_c + T_CLOSE, a)
        yF = yT + (yE - yT) * fo * (1.0 - g)
        hw0 = BEAM_HW0 * c * s
        hw1 = max(0.5 * (R - L) + BEAM_PAD * c, BEAM_HW1 * c)
        return dict(bx=(L + R) / 2, yT=yT, yF=yF, yE=yE, hw0=hw0, hw1=hw1,
                    alpha=BEAM_A * pose['vis'] * (1.0 if fo * (1 - g) > 0 else 0.0),
                    ring=fract(RING_V * a), side=self.side)

    def trail(self, a, pose):
        c = self.c
        if a > self.T * 1.2:
            return None
        def rear(t):
            X, V = self.spring(t)
            sp = math.hypot(*V)
            if sp < 1e-3: return X
            return (X[0] - V[0] / sp * 0.45 * HULL_W * c, X[1] - V[1] / sp * 0.45 * HULL_W * c)
        A = rear(a); B = rear(max(a - TRAIL_T, 0.0))
        gap = math.hypot(A[0] - B[0], A[1] - B[1])
        al = TRAIL_A * pose['vis'] * smoothstep(0.05 * c, 0.25 * c, gap)
        if al <= 0: return None
        return dict(A=A, B=B, ra=TRAIL_RA * c, rb=TRAIL_RB * c, alpha=al)

    def puffs(self, a):
        c = self.c
        n = int(round(clamp(self.len / c * PUFFS_PER_UNIT, 4, 20)))
        out = []
        for i in range(n):
            h0 = pcg((self.seed ^ ((i * 0x9E3779B9) & M32)) & M32)
            h1 = pcg(h0); h2 = pcg(h1); h3 = pcg(h2)
            born = (i + unit(h0)) / n * 0.9 * self.T
            life = 0.20 + 0.12 * unit(h1)
            t = a - born
            if t < 0 or t > life: continue
            X, V = self.spring(born)
            sp = max(math.hypot(*V), 1e-3)
            bx, by = -V[0] / sp, -V[1] / sp
            p0 = (X[0] + bx * 0.5 * HULL_W * c, X[1] + by * 0.5 * HULL_W * c)
            spd = (1.2 + 1.0 * unit(h2)) * c
            jit = (unit(h3) - 0.5) * 1.0 * c
            v0 = (bx * spd - by * jit, by * spd + bx * jit)
            d = (1 - math.exp(-PUFF_DRAG * t)) / PUFF_DRAG
            k = t / life
            out.append(dict(C=(p0[0] + v0[0] * d, p0[1] + v0[1] * d),
                            r=c * (0.05 + 0.12 * k), alpha=PUFF_A * (1 - k) ** 2))
        return out

    def end(self):
        return max(self.a_d + T_LEAVE, 0.9 * self.T + 0.32)

# ---------------------------------------------------------------- per pixel
def cov(dpx): return clamp(0.5 - dpx, 0.0, 1.0)

def sd_vesica_h(qx, qy, r, d):
    """iq's vesica (munrocket's spelling) with x and y swapped: tips on x."""
    px, py = abs(qy), abs(qx)       # p' = (y, x)
    b = math.sqrt(r * r - d * d)
    if (py - b) * d > px * b:
        return math.hypot(px, py - b)
    return math.hypot(px + d, py) - r

def sd_cut_disk(px, py, r, h):
    """iq's cut disk, y up: keeps the cap above y = h."""
    w = math.sqrt(r * r - h * h)
    px = abs(px)
    s = max((h - r) * px * px + w * w * (h + r - 2.0 * py), h * px - w * py)
    if s < 0.0: return math.hypot(px, py) - r
    if px < w: return h - py
    return math.hypot(px - w, py - h)

def smin(a, b, k):
    k *= 4.0
    h = max(k - abs(a - b), 0.0) / k
    return min(a, b) - h * h * k * 0.25

LIGHT_CS = [(math.cos(2 * math.pi * k / N_LIGHTS), math.sin(2 * math.pi * k / N_LIGHTS)) for k in range(N_LIGHTS)]

def body_alpha(p, pose, c):
    s = pose['s']; C = pose['C']
    dx, dy = p[0] - C[0], p[1] - C[1]
    # local = R(-theta)(p - C) / s, in units of c
    qx = (pose['cs'] * dx + pose['sn'] * dy) / (s * c)
    qy = (-pose['sn'] * dx + pose['cs'] * dy) / (s * c)
    H = HULL_H - ROUND; W = HULL_W - ROUND
    r = (W * W + H * H) / (2 * H); d = r - H
    dh = sd_vesica_h(qx, qy, r, d) - ROUND
    dd = sd_cut_disk(qx, DOME_Y - qy, DOME_R, 0.0)   # flip to y up about the dome centre
    sil = smin(dh, dd, SMIN_K)
    unitpx = s * c
    light = 0.0
    if abs(qy - LIGHT_Y) < 2 * LIGHT_R:
        for (ck, sk) in LIGHT_CS:
            cphi = pose['cr'] * ck - pose['sr'] * sk
            sphi = pose['sr'] * ck + pose['cr'] * sk
            v = smoothstep(0.0, 0.35, cphi)
            lx = LIGHT_X * sphi
            light = max(light, v * cov((math.hypot(qx - lx, qy - LIGHT_Y) - LIGHT_R) * unitpx))
    a = A_SIL * cov(sil * unitpx) + cov(dh * unitpx) * (A_HULL + A_LIGHT * light)
    return a * pose['vis']

def beam_alpha(p, bm, c):
    if bm['alpha'] <= 0: return 0.0
    side = bm['side']
    along = (p[1] - bm['yT']) * side          # 0 at the saucer, growing toward the cursor
    front = (bm['yF'] - bm['yT']) * side
    full = max((bm['yE'] - bm['yT']) * side, 1.0)
    if along < -0.5 or along > front + 0.5: return 0.0
    f = clamp(along / full, 0.0, 1.0)
    hw = mix(bm['hw0'], bm['hw1'], f)
    ax = abs(p[0] - bm['bx'])
    edge = clamp(hw - ax + 0.5, 0.0, 1.0) * clamp(front - along + 0.5, 0.0, 1.0) \
        * clamp(along + 0.5, 0.0, 1.0)
    core = 1.0 - smoothstep(0.0, 0.6, ax / max(hw, 1.0))
    fr = fract(along / (RING_P * c) - bm['ring'])
    ring = 1.0 - RING_DEPTH * 4.0 * fr * (1.0 - fr)
    return bm['alpha'] * edge * (0.55 + 0.45 * core) * (0.6 + 0.4 * f) * ring

def trail_alpha(p, tr):
    A, B = tr['A'], tr['B']
    bx, by = B[0] - A[0], B[1] - A[1]
    px, py = p[0] - A[0], p[1] - A[1]
    h = clamp((px * bx + py * by) / max(bx * bx + by * by, 1e-6), 0.0, 1.0)
    d = math.hypot(px - bx * h, py - by * h) - mix(tr['ra'], tr['rb'], h)
    return tr['alpha'] * cov(d) * (1.0 - h)

def puff_alpha(u, v, al):
    l = math.hypot(u, v)
    a = 1.0 - smoothstep(0.0, 0.22, abs(l - 0.72))
    return al * a

# ---------------------------------------------------------------- render
def render(path, fl, a, W, H, fg, bg, hole, dump=False):
    img = [[list(bg) for _ in range(W)] for _ in range(H)]
    c = fl.c
    # the cursor block
    L, T, R, B = hole
    for y in range(int(T), int(B)):
        for x in range(int(L), int(R)):
            if 0 <= x < W and 0 <= y < H: img[y][x] = list(fg)
    def over(x, y, al):
        if al <= 0 or not (0 <= x < W and 0 <= y < H): return
        if L <= x + 0.5 < R and T <= y + 0.5 < B: return      # the hole
        px = img[y][x]
        for k in range(3): px[k] = fg[k] * al + px[k] * (1 - al)
    pose = fl.pose(a)
    tr = fl.trail(a, pose)
    if tr:
        x0 = int(min(tr['A'][0], tr['B'][0]) - tr['ra'] - 2); x1 = int(max(tr['A'][0], tr['B'][0]) + tr['ra'] + 2)
        y0 = int(min(tr['A'][1], tr['B'][1]) - tr['ra'] - 2); y1 = int(max(tr['A'][1], tr['B'][1]) + tr['ra'] + 2)
        for y in range(y0, y1):
            for x in range(x0, x1): over(x, y, trail_alpha((x + .5, y + .5), tr))
    for pf in fl.puffs(a):
        side = max(math.ceil(2 * pf['r']), 2)
        X0 = math.floor(pf['C'][0] - side / 2); Y0 = math.floor(pf['C'][1] - side / 2)
        for y in range(Y0, Y0 + side):
            for x in range(X0, X0 + side):
                u = (x + .5 - X0) / side * 2 - 1; v = (y + .5 - Y0) / side * 2 - 1
                over(x, y, puff_alpha(u, v, pf['alpha']))
    bm = fl.beam(a, pose)
    if bm['alpha'] > 0:
        y0 = int(min(bm['yT'], bm['yF']) - 1); y1 = int(max(bm['yT'], bm['yF']) + 2)
        x0 = int(bm['bx'] - bm['hw1'] - 2); x1 = int(bm['bx'] + bm['hw1'] + 2)
        for y in range(y0, y1):
            for x in range(x0, x1): over(x, y, beam_alpha((x + .5, y + .5), bm, c))
    R0 = SAUCER_R * c * pose['s'] + 1
    C = pose['C']
    for y in range(int(C[1] - R0), int(C[1] + R0) + 1):
        for x in range(int(C[0] - R0), int(C[0] + R0) + 1):
            over(x, y, body_alpha((x + .5, y + .5), pose, c))
    with open(path, 'wb') as f:
        f.write(b'P6 %d %d 255\n' % (W, H))
        for row in img:
            f.write(bytes(int(round(clamp(ch, 0, 255))) for px in row for ch in px))
    return pose

if __name__ == '__main__':
    out = sys.argv[1] if len(sys.argv) > 1 else '.'
    for c, cw in [(34.0, 17.0), (17.0, 8.5)]:
        cols, rows = 34, 9
        W, H = int(cols * cw), int(rows * c)
        from_cell = (3, 6); to_cell = (28, 5)
        S = ((from_cell[0] + .5) * cw, (from_cell[1] + .5) * c)
        hole = (to_cell[0] * cw, to_cell[1] * c, (to_cell[0] + 1) * cw, (to_cell[1] + 1) * c)
        fl = Flight(S, hole, c, H, seed=7)
        print(f'c={c}: T_fly={fl.T:.3f} a_b={fl.a_b:.3f} a_d={fl.a_d:.3f} end={fl.end():.3f} side={fl.side}')
        for theme, fg, bg in [('dark', (230, 230, 230), (22, 22, 26)), ('light', (40, 40, 44), (238, 238, 236))]:
            for a in [0.04, 0.09, 0.16, 0.26, 0.36, 0.50, 0.58, 0.64]:
                p = render(f'{out}/ufo-c{int(c)}-{theme}-{a:.2f}.ppm', fl, a, W, H, fg, bg, hole)
                if theme == 'dark':
                    print(f'  a={a:.2f} C=({p["C"][0]:.1f},{p["C"][1]:.1f}) theta={math.degrees(math.atan2(p["sn"], p["cs"])):.1f} s={p["s"]:.2f} vis={p["vis"]:.2f}')
