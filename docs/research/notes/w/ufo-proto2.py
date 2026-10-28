#!/usr/bin/env python3
"""UFO trail, second prototype: one member of the effect family (f202aea).

The saucer rides kitty's quad: its centre is the centroid of the corners the
uniform carries this frame (baud-render/src/trail.rs Segment, ported below),
lifted HOVER cells above (or below, near the top) -- so a retarget, which the
driver already does from where the corners are, re-flies it with no state.
Everything the fragment draws is a pure function of the EffectU prefix
(corners, hole, origin, tail, dir, len, unit, age, life, seed, reach) and the
effect's own k[8]. Particles (exhaust rings) are a pure function of the
Ignition. Written the way the Rust reference in cells.rs would port it.
"""
import math, struct, sys

# ---- the effect's own numbers, k[0..8] in EffectU (Rust owns them)
K = dict(
    hover=1.00,      # k0: saucer centre above the cursor's near edge, units
    size=1.00,       # k1: saucer scale (1 = hull 1.1 units wide)
    beam_a=0.34,     # k2: beam opacity
    wake_a=0.30,     # k3: wake opacity
    light_hz=0.55,   # k4: rim rotation, turns a second (5 lights -> 2.75 passes/s)
    arc=0.50,        # k5: swoop peak, units (also capped at 0.12 of the path)
    tilt=0.32,       # k6: bank at full lean, radians
    depart=0.54,     # k7: age the saucer leaves at, seconds
)
LIFE = 0.72          # prefix `life`: depart + T_LEAVE
REACH = 2.8          # prefix `reach`, units from the quad

# ---- shader literals (the saucer's shape and the timeline's fixed shape)
HULL_W, HULL_H, ROUND = 0.55, 0.13, 0.02
DOME_R, DOME_Y, SMIN_K = 0.22, -0.04, 0.03
LIGHT_R, LIGHT_X, LIGHT_Y, N_LIGHTS = 0.045, 0.40, 0.03, 5
A_SIL, A_HULL, A_LIGHT = 0.50, 0.30, 0.20
SAUCER_R = 0.62
BOB, BOB_HZ = 0.035, 2.2
TILT_GAP = 3.0
T_APPEAR, T_LEAVE, LEAVE_ACC = 0.08, 0.18, 92.0
A_OPEN, T_OPEN, A_CLOSE, T_CLOSE = 0.15, 0.10, 0.46, 0.08
BEAM_HW0, BEAM_HW1, BEAM_PAD = 0.14, 0.22, 0.08
RING_P, RING_V, RING_DEPTH = 0.22, 5.0, 0.22
WAKE_RA, WAKE_RB = 0.055, 0.012
SIDE_ROOM = 1.35     # hover above only if the cursor's top is this many units down

def smoothstep(e0, e1, x):
    t = min(max((x - e0) / (e1 - e0), 0.0), 1.0)
    return t * t * (3.0 - 2.0 * t)
def clamp(x, a, b): return min(max(x, a), b)
def mix(a, b, t): return a + (b - a) * t
def fract(x): return x - math.floor(x)
def sinpi(x): return math.sin(math.pi * x)
def cospi(x): return math.cos(math.pi * x)

M32 = 0xFFFFFFFF
def pcg(v):
    s = (v * 747796405 + 2891336453) & M32
    w = (((s >> ((s >> 28) + 4)) ^ s) * 277803737) & M32
    return (w >> 22) ^ w
def unit(h): return struct.unpack('<f', struct.pack('<I', (h >> 9) | 0x3f800000))[0] - 1.0

# ---- the quad, trail.rs Segment ported, its rates as note W, section 8.4,
# derives them
TAU_FAST, TAU_SLOW = 0.1, 0.4
def rect_corners(r):
    L, T, R, B = r
    return [(R, T), (R, B), (L, B), (L, T)]
class Segment:
    def __init__(self, frm, to):
        self.frm, self.to = frm, to
        tgt = rect_corners(to); L, T, R, B = to
        c = ((L + R) / 2, (T + B) / 2)
        leads = []
        for i in range(4):
            dx, dy = tgt[i][0] - frm[i][0], tgt[i][1] - frm[i][1]
            ox, oy = tgt[i][0] - c[0], tgt[i][1] - c[1]
            travel, out = math.hypot(dx, dy), math.hypot(ox, oy)
            leads.append((dx * ox + dy * oy) / (travel * out) if travel >= 1e-3 and out > 0 else None)
        live = [l for l in leads if l is not None]
        lo, hi = (min(live), max(live)) if live else (0.0, 0.0)
        def rate(l):
            if l is None or hi - lo <= 1.2e-7: return TAU_SLOW
            s = (l - lo) / (hi - lo); return TAU_FAST * s + TAU_SLOW * (1 - s)
        self.tau = [rate(l) for l in leads]
    def at(self, t):
        tgt = rect_corners(self.to); t = max(t, 0.0)
        return [(tgt[i][0] + (self.frm[i][0] - tgt[i][0]) * 2 ** (-10 * t / self.tau[i]),
                 tgt[i][1] + (self.frm[i][1] - tgt[i][1]) * 2 ** (-10 * t / self.tau[i])) for i in range(4)]

# ---- the uniform: EffectU (prefix + k)
def uniform(corners, hole, tail, head, unit_px, age, seed, origin=(0.0, 0.0)):
    dx, dy = head[0] - tail[0], head[1] - tail[1]
    ln = max(math.hypot(dx, dy), 1.0)
    return dict(c=corners, hole=hole, origin=origin, tail=tail, dir=(dx / ln, dy / ln), len=ln,
                unit=unit_px, age=age, life=LIFE, seed=seed, reach=REACH, k=K)

# ---- the pose, per pixel in the shader (the same for every pixel of a frame)
def ufo_pose(f):
    c = f['unit']; k = f['k']; a = f['age']
    L, T, R, B = f['hole']
    cx = sum(p[0] for p in f['c']) / 4; cy = sum(p[1] for p in f['c']) / 4
    hx, hy = (L + R) / 2, (T + B) / 2
    gx, gy = hx - cx, hy - cy                      # what the quad has still to close
    gap = math.hypot(gx, gy)
    side = 1.0 if T >= SIDE_ROOM * c else -1.0     # +1: above the cursor, beam down
    s_prog = clamp(1.0 - gap / max(f['len'], 1.0), 0.0, 1.0)
    arc = min(k['arc'] * c, 0.12 * f['len']) * 4.0 * s_prog * (1.0 - s_prog)
    # the swoop bulges away from the cursor's near edge: up when above
    if gap > 1e-3:
        nx, ny = -gy / gap, gx / gap
        if ny * side > 0.0: nx, ny = -nx, -ny
    else:
        nx, ny = 0.0, -side
    hov = 1.0 - smoothstep(0.1 * c, 0.6 * c, gap)
    h1 = unit(pcg(f['seed'] ^ 0x5bd1e995)); h2 = unit(pcg(f['seed']))
    bob = BOB * c * sinpi(2.0 * fract(BOB_HZ * a + h1) - 1.0) * hov
    tl = max(a - k['depart'], 0.0)
    kk = clamp(tl / T_LEAVE, 0.0, 1.0)
    x = cx + nx * arc
    y = cy + ny * arc - side * k['hover'] * c + bob - side * 0.5 * LEAVE_ACC * c * tl * tl
    theta = k['tilt'] * clamp(gx / (TILT_GAP * c), -1.0, 1.0) \
        + 0.05 * sinpi(2.0 * fract(1.3 * a + h2) - 1.0) * hov
    grow = smoothstep(0.0, T_APPEAR, a)
    vis = grow * (1.0 - smoothstep(0.0, 1.0, kk))
    scale = k['size'] * mix(0.5, 1.0, grow) * (1.0 - 0.6 * kk * kk)
    rho = 2.0 * fract(k['light_hz'] * a + h2) - 1.0
    settle = 1.0 - smoothstep(0.15 * c, 0.6 * c, gap)
    ext = smoothstep(A_OPEN, A_OPEN + T_OPEN, a) * (1.0 - smoothstep(A_CLOSE, A_CLOSE + T_CLOSE, a))
    return dict(C=(x, y), cs=math.cos(theta), sn=math.sin(theta), s=scale, vis=vis,
                cr=cospi(rho), sr=sinpi(rho), side=side, gap=gap, g=(gx, gy), cen=(cx, cy),
                beam=ext * settle, ext=ext)

def cov(dpx): return clamp(0.5 - dpx, 0.0, 1.0)

def sd_vesica_h(qx, qy, r, d):
    px, py = abs(qy), abs(qx)
    b = math.sqrt(r * r - d * d)
    if (py - b) * d > px * b: return math.hypot(px, py - b)
    return math.hypot(px + d, py) - r

def sd_cut_disk(px, py, r, h):
    w = math.sqrt(r * r - h * h); px = abs(px)
    s = max((h - r) * px * px + w * w * (h + r - 2.0 * py), h * px - w * py)
    if s < 0.0: return math.hypot(px, py) - r
    if px < w: return h - py
    return math.hypot(px - w, py - h)

def smin(a, b, k):
    k *= 4.0; h = max(k - abs(a - b), 0.0) / k
    return min(a, b) - h * h * k * 0.25

LIGHTS = [(cospi(2.0 * i / N_LIGHTS), sinpi(2.0 * i / N_LIGHTS)) for i in range(N_LIGHTS)]

def body(p, P, c):
    s = P['s']; C = P['C']
    dx, dy = p[0] - C[0], p[1] - C[1]
    u = s * c
    if dx * dx + dy * dy > (SAUCER_R * u + 1.0) ** 2: return 0.0
    qx = (P['cs'] * dx + P['sn'] * dy) / u
    qy = (-P['sn'] * dx + P['cs'] * dy) / u
    H = HULL_H - ROUND; W = HULL_W - ROUND
    r = (W * W + H * H) / (2 * H); d = r - H
    dh = sd_vesica_h(qx, qy, r, d) - ROUND
    dd = sd_cut_disk(qx, DOME_Y - qy, DOME_R, 0.0)
    sil = smin(dh, dd, SMIN_K)
    light = 0.0
    if abs(qy - LIGHT_Y) < 2 * LIGHT_R:
        for (ck, sk) in LIGHTS:
            cphi = P['cr'] * ck - P['sr'] * sk
            sphi = P['sr'] * ck + P['cr'] * sk
            light = max(light, smoothstep(0.0, 0.35, cphi) * cov((math.hypot(qx - LIGHT_X * sphi, qy - LIGHT_Y) - LIGHT_R) * u))
    return (A_SIL * cov(sil * u) + cov(dh * u) * (A_HULL + A_LIGHT * light)) * P['vis']

def beam(p, P, f):
    if P['beam'] <= 0.0: return 0.0
    c = f['unit']; L, T, R, B = f['hole']; side = P['side']; s = P['s']
    top = (P['C'][0], P['C'][1] + side * HULL_H * c * s * 0.9)
    end = ((L + R) / 2, T if side > 0 else B)
    ax, ay = end[0] - top[0], end[1] - top[1]
    ln = math.hypot(ax, ay)
    if ln < 1.0: return 0.0
    nx, ny = ax / ln, ay / ln
    dx, dy = p[0] - top[0], p[1] - top[1]
    along = dx * nx + dy * ny
    across = abs(dx * ny - dy * nx)
    front = ln * P['ext']
    ff = clamp(along / ln, 0.0, 1.0)
    hw = mix(BEAM_HW0 * c * s, max(0.5 * (R - L) + BEAM_PAD * c, BEAM_HW1 * c), ff)
    edge = clamp(hw - across + 0.5, 0.0, 1.0) * clamp(front - along + 0.5, 0.0, 1.0) * clamp(along + 0.5, 0.0, 1.0)
    if edge <= 0.0: return 0.0
    core = 1.0 - smoothstep(0.0, 0.6, across / max(hw, 1.0))
    fr = fract(along / (RING_P * c) - fract(RING_V * f['age']))
    ring = 1.0 - RING_DEPTH * 4.0 * fr * (1.0 - fr)
    return f['k']['beam_a'] * P['vis'] * P['beam'] * edge * (0.55 + 0.45 * core) * (0.6 + 0.4 * ff) * ring

def wake(p, P, f):
    """A tapered capsule from under the saucer's quad centroid back to the
    quad's trailing edge, lifted with the saucer: kitty's own lag is the wake."""
    c = f['unit']; gap = P['gap']
    if gap < 0.1 * c: return 0.0
    gx, gy = P['g'][0] / gap, P['g'][1] / gap
    cen = P['cen']
    d = sorted((((q[0] - cen[0]) * gx + (q[1] - cen[1]) * gy), q) for q in f['c'])
    bx = (d[0][1][0] + d[1][1][0]) / 2; by = (d[0][1][1] + d[1][1][1]) / 2
    lift = -P['side'] * f['k']['hover'] * c
    A = (P['C'][0] - gx * 0.45 * HULL_W * c * P['s'], P['C'][1] - gy * 0.45 * HULL_W * c * P['s'])
    Bp = (bx, by + lift)
    vx, vy = Bp[0] - A[0], Bp[1] - A[1]
    px, py = p[0] - A[0], p[1] - A[1]
    l2 = vx * vx + vy * vy
    if l2 < 1.0: return 0.0
    h = clamp((px * vx + py * vy) / l2, 0.0, 1.0)
    dd = math.hypot(px - vx * h, py - vy * h) - mix(WAKE_RA * c, WAKE_RB * c, h)
    return f['k']['wake_a'] * P['vis'] * cov(dd) * (1.0 - h) * smoothstep(0.1 * c, 0.6 * c, math.sqrt(l2))

def in_hole(p, f):
    L, T, R, B = f['hole']
    return L <= p[0] < R and T <= p[1] < B

def ufo_band(p, f):
    """ufo_fragment for a band: the parts composited in the fragment (a band
    is one row of pixels no other band draws), saucer over beam over wake."""
    if in_hole(p, f): return 0.0
    P = ufo_pose(f)
    a = wake(p, P, f)
    b = beam(p, P, f); a = b + a * (1 - b)
    s = body(p, P, f['unit']); a = s + a * (1 - s)
    return a

# ---- particles: exhaust rings, a pure function of the Ignition
PUFFS_PER_UNIT, PUFF_DRAG, PUFF_A = 2.0, 5.0, 0.45
def puffs(f):
    c = f['unit']; a = f['age']
    n = int(round(clamp(f['len'] / c * PUFFS_PER_UNIT, 4, 20)))
    head_y = f['tail'][1] + f['dir'][1] * f['len']
    side = 1.0 if head_y - 0.5 * c >= SIDE_ROOM * c else -1.0
    out = []
    for i in range(n):
        h0 = pcg((f['seed'] ^ ((i * 0x9E3779B9) & M32)) & M32)
        h1 = pcg(h0); h2 = pcg(h1); h3 = pcg(h2)
        u = (i + unit(h0)) / n * 0.9
        born = -math.log2(1.0 - u) / 25.0           # where kitty's slow corners pass
        life = 0.20 + 0.12 * unit(h1)
        t = a - born
        if t < 0 or t > life: continue
        dx, dy = f['dir']
        p0 = (f['tail'][0] + dx * f['len'] * u, f['tail'][1] + dy * f['len'] * u - side * K['hover'] * c)
        spd = (1.2 + 1.0 * unit(h2)) * c; jit = (unit(h3) - 0.5) * c
        v0 = (-dx * spd - dy * jit, -dy * spd + dx * jit)
        dr = (1 - math.exp(-PUFF_DRAG * t)) / PUFF_DRAG
        kk = t / life
        out.append(dict(C=(p0[0] + v0[0] * dr, p0[1] + v0[1] * dr), r=c * (0.05 + 0.12 * kk),
                        glow=PUFF_A * (1 - kk) ** 2))
    return out

def puff_px(uu, vv, glow):
    return glow * (1.0 - smoothstep(0.0, 0.22, abs(math.hypot(uu, vv) - 0.72)))

# ---- a frame: the bands (quad +- reach), then particles, as the renderer does
def render(path, f, W, H, fg, bg, stats=None):
    img = [[list(bg) for _ in range(W)] for _ in range(H)]
    L, T, R, B = f['hole']
    for y in range(int(T), int(B)):
        for x in range(int(L), int(R)):
            if 0 <= x < W and 0 <= y < H: img[y][x] = list(fg)
    def over(x, y, al):
        if al <= 0 or not (0 <= x < W and 0 <= y < H): return
        px = img[y][x]
        for k in range(3): px[k] = fg[k] * al + px[k] * (1 - al)
    c = f['unit']; r = f['reach'] * c
    xs = [q[0] for q in f['c']]; ys = [q[1] for q in f['c']]
    # the bands' union is the quad's bounds +- reach (the renderer clips per row; a box is a superset)
    x0, x1 = max(int(min(xs) - r), 0), min(int(max(xs) + r) + 1, W)
    y0, y1 = max(int(min(ys) - r), 0), min(int(max(ys) + r) + 1, H)
    lit = 0; far = 0.0
    for y in range(y0, y1):
        for x in range(x0, x1):
            al = ufo_band((x + .5, y + .5), f)
            if al > 0.0:
                lit += al >= 0.1
            over(x, y, al)
    # everything the fragment can draw must be inside the box: check a margin ring outside it
    for pf in puffs(f):
        side = max(math.ceil(2 * pf['r']), 2)
        X0 = math.floor(pf['C'][0] - side / 2); Y0 = math.floor(pf['C'][1] - side / 2)
        for y in range(Y0, Y0 + side):
            for x in range(X0, X0 + side):
                over(x, y, puff_px((x + .5 - X0) / side * 2 - 1, (y + .5 - Y0) / side * 2 - 1, pf['glow']))
    if stats is not None: stats['lit'] = lit
    with open(path, 'wb') as fh:
        fh.write(b'P6 %d %d 255\n' % (W, H))
        for row in img: fh.write(bytes(int(round(clamp(ch, 0, 255))) for px in row for ch in px))

def cellrect(col, row, cw, c): return (col * cw, row * c, (col + 1) * cw, (row + 1) * c)
def centre(r): return ((r[0] + r[2]) / 2, (r[1] + r[3]) / 2)

def reach_check(cw, c, moves, frame):
    """The farthest pixel the fragment lights, in units from the quad: must be < REACH."""
    worst = 0.0
    for frm, to in moves:
        fr, tr = cellrect(*frm, cw, c), cellrect(*to, cw, c)
        seg = Segment(rect_corners(fr), tr)
        for i in range(0, 76):
            a = i * 0.01
            f = uniform(seg.at(a), tr, centre(fr), centre(tr), c, a, 7)
            P = ufo_pose(f)
            # sample the saucer's bound, the beam and the wake
            for y in range(int(P['C'][1] - 3 * c), int(P['C'][1] + 3 * c), 2):
                for x in range(int(P['C'][0] - 3 * c), int(P['C'][0] + 3 * c), 2):
                    p = (x + .5, y + .5)
                    if ufo_band(p, f) > 0:
                        worst = max(worst, quad_dist(p, f['c']) / c)
    return worst

def quad_dist(p, q):
    d = 1e9; wn = 0
    for i in range(4):
        a, b = q[i], q[(i + 1) % 4]
        bx, by = b[0] - a[0], b[1] - a[1]; px, py = p[0] - a[0], p[1] - a[1]
        h = clamp((px * bx + py * by) / max(bx * bx + by * by, 1e-6), 0, 1)
        d = min(d, math.hypot(px - bx * h, py - by * h))
        side = bx * py - by * px
        if a[1] <= p[1]:
            if b[1] > p[1] and side > 0: wn += 1
        elif b[1] <= p[1] and side < 0: wn -= 1
    return -d if wn else d

if __name__ == '__main__':
    out = 'render2'
    import os; os.makedirs(out, exist_ok=True)
    scen = {'right': ((3, 6), (28, 5)), 'upleft': ((29, 7), (4, 2)), 'toprow': ((5, 6), (25, 0)),
            'vertical': ((16, 8), (17, 1)), 'short': ((10, 4), (14, 4))}
    ages = [0.03, 0.07, 0.12, 0.20, 0.30, 0.42, 0.58, 0.66]
    for c, cw in [(34.0, 17.0), (17.0, 8.5)]:
        cols, rows = 34, 9
        W, H = int(cols * cw), int(rows * c)
        for name, (fc, tc) in scen.items():
            if c < 30 and name != 'right': continue
            fr, tr = cellrect(*fc, cw, c), cellrect(*tc, cw, c)
            seg = Segment(rect_corners(fr), tr)
            for theme, fg, bg in [('dark', (230, 230, 230), (22, 22, 26)), ('light', (40, 40, 44), (238, 238, 236))]:
                if theme == 'light' and name != 'right': continue
                for a in ages:
                    f = uniform(seg.at(a), tr, centre(fr), centre(tr), c, a, 7)
                    st = {}
                    render(f'{out}/{name}-c{int(c)}-{theme}-{a:.2f}.ppm', f, W, H, fg, bg, st)
                    if theme == 'dark' and c > 30 and name == 'right':
                        P = ufo_pose(f)
                        print(f'{name} a={a:.2f} C=({P["C"][0]:.0f},{P["C"][1]:.0f}) gap={P["gap"]/c:.2f}u tilt={math.degrees(math.atan2(P["sn"],P["cs"])):.1f} s={P["s"]:.2f} vis={P["vis"]:.2f} beam={P["beam"]:.2f} lit(>=0.1)={st["lit"]}px')
    # retarget: a second long move at 0.20 s, the quad retargeted from where it is
    c, cw = 34.0, 17.0; W, H = int(34 * cw), int(9 * c)
    fr, tr, tr2 = cellrect(3, 6, cw, c), cellrect(28, 5, cw, c), cellrect(12, 7, cw, c)
    seg = Segment(rect_corners(fr), tr)
    seg2 = Segment(seg.at(0.20), tr2)
    for a in [0.22, 0.28, 0.36, 0.46]:
        f = uniform(seg2.at(a - 0.20), tr2, centre(fr), centre(tr), c, a, 7)
        render(f'{out}/retarget-c34-dark-{a:.2f}.ppm', f, W, H, (230, 230, 230), (22, 22, 26))
    w = reach_check(cw, c, [((3, 6), (28, 5)), ((29, 7), (4, 2)), ((5, 6), (25, 0)), ((16, 8), (17, 1)), ((10, 4), (14, 4))], (W, H))
    print(f'farthest lit pixel from the quad: {w:.2f} units (REACH {REACH})')
