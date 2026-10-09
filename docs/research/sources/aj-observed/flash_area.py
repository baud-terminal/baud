"""WCAG 2.3.1 area estimate for baud's plasma trail (note AJ, section 1.3).

A model, not a measurement: it reads no files. Its constants are copied from
baud at commit ccb8aaa:
  baud-render/src/plasma.rs   REACH = 1.0, SHEATH = 0.20, TUBE = 0.08,
                              DECAY = 5.0, FILAMENT = 0.95, ions: 8-64,
                              radius max(unit * (0.05 + 0.04 * u) * ..., 2 px)
  shaders.metal               knot = 0.72 + 0.28 * wave; filament width
                              max(PX_W * (1 - 0.5 k), 1.1 px), PX_W <= 0.050
                              cell; gain PX_G <= 1.00; alpha = tube + (1 -
                              tube) * fil; the braid within the cursor's width
  baud-render/src/metrics.rs  cell height = round((ascent - descent + gap)
                              / upem * size); JetBrains Mono hhea 1020/-300/0,
                              so round(1.32 * font size)
  baud-render/src/color.rs    default foreground grey 0xD8 (dark), 0x27
                              (light); xterm palette red (SGR 41) #CD0000
  baud-app/src/trail.rs       REIGNITE = 334 ms
  baud-present-macos/src/metal.rs  BGRA8Unorm targets, premultiplied OVER:
                              blending happens on the encoded (sRGB) values.

Unit: points at 1x, taken as WCAG Note 1's pixels. The threshold is 25% of a
341 x 256 window, 21,824. The longest straight run inside one window is 341
across or 426.4 on its diagonal. Two simplifications act in opposite
directions: a band is not clipped to the window (which overstates the area)
and a band's width is taken as its height across the move (which, on a
diagonal, understates the quad's footprint).

General flash: |dL| >= 0.10 with the darker state below 0.80, L being WCAG
2.2's relative luminance. Three transfer models are run: linear blending
(values are luminance), encoded blending with x^2.2, and encoded blending
with WCAG 2.2's own relative-luminance curve (the piecewise sRGB curve,
0.04045 and exponent 2.4). The least fill opacity that can make a general
flash is the infimum over a grey fill and a grey ground, found by a fine
search (2001 ground levels for black and white fills, 201 levels of fill).

Red flash (WCAG 2.2: "any pair of opposing transitions involving a saturated
red"; the Understanding document's working definition: one state with
R/(R+G+B) >= 0.8 and a CIE 1976 u'v' difference above 0.2). Either state can
be the saturated red: the cursor colour (OSC 12) or the ground (an SGR 41
cell, a dark-red palette background). For a red cursor colour the 0.2 step
was not computed for the body, so the body is counted at full reach as an
upper bound; the knots count over the whole braid region. For a red ground,
the least opacity at which the default foreground (or white) crosses the 0.2
step is computed, and the body and filament bands at that opacity counted.
"""
import math

T = 0.25 * 341 * 256
W_H, W_D = 341.0, math.hypot(341, 256)
REACH, SHEATH, TUBE, DECAY, FIL = 1.0, 0.20, 0.08, 5.0, 0.95
KNOT_SWING = 0.56            # 2 x 0.28
PX_W_MAX, W_FLOOR = 0.050, 1.1
INSIDE = TUBE + SHEATH       # the body's opacity inside the quad, 0.28


def srgb(v):
    return v / 12.92 if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4


def least_alpha(lum, f, g):
    def flash(a):
        if lum is None:
            l1 = g; l2 = (1 - a) * g + a * f
        else:
            l1 = lum(g); l2 = lum((1 - a) * g + a * f)
        return abs(l2 - l1) >= 0.10 and min(l1, l2) < 0.80
    if not flash(1.0):
        return None
    lo, hi = 0.0, 1.0
    for _ in range(50):
        mid = (lo + hi) / 2
        if flash(mid): hi = mid
        else: lo = mid
    return hi


def threshold(lum):
    best = (1.0, None, None)
    cands = [(f, j / 2000) for f in (0.0, 1.0) for j in range(2001)]
    cands += [(i / 200, j / 200) for i in range(201) for j in range(201)]
    for f, g in cands:
        a = least_alpha(lum, f, g)
        if a is not None and a < best[0]:
            best = (a, f, g)
    return best


def cell(font):
    return round(1.32 * font)


def band(a):
    d_sheath = max(math.log(SHEATH / a) / DECAY, 0.0)
    e_knot = math.sqrt(math.log(FIL * KNOT_SWING / a))
    e_sway = math.sqrt(math.log(FIL / a))
    return d_sheath, e_knot, e_sway


def fil_w(h):
    return max(PX_W_MAX * h, W_FLOOR)


def bound(k, run):
    hmax = T / (k * run)
    return f"cell <= {hmax:5.1f} pt (font < {(hmax + 0.5) / 1.32:5.2f} pt)"


models = [("linear", None), ("encoded x^2.2", lambda v: v ** 2.2),
          ("encoded, WCAG 2.2's sRGB curve", srgb)]
mins = {name: threshold(lum) for name, lum in models}
print(f"threshold area {T:.0f}; window run {W_H:.0f} across, {W_D:.1f} diagonal")
for name, _ in models:
    a, f, g = mins[name]
    d, ek, es = band(a)
    print(f"{name}: least flashing opacity {a:.4f} (fill grey {f:.3f} over ground grey {g:.4f}); "
          f"sheath band +{d:.4f} cell each side ({1 + 2 * d:.3f} cells); knot +-{ek:.3f}, sway +-{es:.3f} filament widths")

print("\nper font size (cell = round(1.32 x font)):")
for font in (13, 14, 16, 20, 30, 38):
    h = cell(font)
    full = (1 + 2 * REACH) * h
    row = f"font {font:>2} pt, cell {h:>2}: body full reach {full * W_H:6.0f} / {full * W_D:6.0f} diag"
    for name, _ in models:
        a = mins[name][0]
        d, ek, es = band(a)
        body = (1 + 2 * d) * h
        row += (f"\n    [{name}] body {body * W_H:5.0f} / {body * W_D:5.0f} diag, knots {3 * 2 * ek * fil_w(h) * W_D:5.0f},"
                f" sway {3 * 2 * es * fil_w(h) * W_D:5.0f}")
    ion = 64 * math.pi * max(0.09 * h, 2.0) ** 2
    row += f"\n    ions {ion:5.0f}; red cursor colour, braid region {h * W_D:6.0f}"
    print(row)

print("\nlargest cell (and font) at which each part stays under the threshold:")
print(f"body, full reach (the red-cursor upper bound): across {bound(1 + 2 * REACH, W_H)}; diagonal {bound(1 + 2 * REACH, W_D)}")
for name, _ in models:
    a = mins[name][0]
    d, ek, es = band(a)
    print(f"[{name}] body, opacity >= {a:.4f}: across {bound(1 + 2 * d, W_H)}; diagonal {bound(1 + 2 * d, W_D)}")
    print(f"[{name}] knots: diagonal {bound(3 * 2 * ek * PX_W_MAX, W_D)}; sway: diagonal {bound(3 * 2 * es * PX_W_MAX, W_D)}")
print(f"red cursor colour, knots over the whole braid region (one cell high): diagonal {bound(1.0, W_D)}")
print(f"ions: cell <= {math.sqrt(T / (64 * math.pi)) / 0.09:5.1f} pt")


# ---- red flashes: the 0.2 u'v' step, encoded blending ----
def xyz(c):
    r, g, b = map(srgb, c)
    return (0.4124 * r + 0.3576 * g + 0.1805 * b,
            0.2126 * r + 0.7152 * g + 0.0722 * b,
            0.0193 * r + 0.1192 * g + 0.9505 * b)


def uv(c):
    X, Y, Z = xyz(c)
    d = X + 15 * Y + 3 * Z
    return (4 * X / d, 9 * Y / d) if d > 0 else None


def duv(a, b):
    p, q = uv(a), uv(b)
    if p is None or q is None:
        return 0.0
    return math.hypot(p[0] - q[0], p[1] - q[1])


def over(f, g, a):
    return tuple((1 - a) * gi + a * fi for fi, gi in zip(f, g))


def red_alpha(f, g):
    for i in range(1, 10001):
        a = i / 10000
        if duv(g, over(f, g, a)) > 0.2:
            return a
    return None


print("\nred ground (R/(R+G+B) >= 0.8) under a light fill, encoded blending, at the default font:")
FILLS = {"white": (1.0, 1.0, 1.0), "default fg, dark theme (0xD8)": (0xD8 / 255,) * 3}
GROUNDS = {"#FF0000": (1.0, 0.0, 0.0), "SGR 41, xterm #CD0000": (205 / 255, 0.0, 0.0),
           "#800000": (128 / 255, 0.0, 0.0), "#1A0000": (26 / 255, 0.0, 0.0)}
h = cell(13)
for gn, g in GROUNDS.items():
    for fn, f in FILLS.items():
        a = red_alpha(f, g)
        if a is None:
            print(f"  {fn} over {gn}: never crosses 0.2"); continue
        if a <= INSIDE:
            d = max(math.log(SHEATH / a) / DECAY, 0.0)
            area = (1 + 2 * d) * h * W_D
            what = f"the body to +{d:.3f} cell: {area:5.0f} at the default font, diagonal; {bound(1 + 2 * d, W_D)}"
        elif a <= INSIDE + (1 - INSIDE) * FIL:
            need = (a - INSIDE) / (1 - INSIDE)
            e = math.sqrt(math.log(FIL / need))
            area = 3 * 2 * e * fil_w(h) * W_D
            what = f"the filament cores only, +-{e:.3f} widths: {area:5.0f} at the default font, diagonal"
        else:
            what = "nothing the trail draws reaches it"
        print(f"  {fn} over {gn}: crosses 0.2 at alpha {a:.4f} -> {what}")

print("\nthe logo's amber as the fill, over grounds with R/(R+G+B) >= 0.8 (encoded blending; pure-red grounds")
print("#010000-#FF0000 and reds with small green and blue, alpha in steps of 1/400):")
AMBER = (("#F4A32C", (244 / 255, 163 / 255, 44 / 255)), ("#C8801F", (200 / 255, 128 / 255, 31 / 255)))
ALPHAS = [k / 400 for k in range(1, 401)]
for name, f in AMBER:
    worst, last_over = (0.0, None, None), None
    for r in range(1, 256):
        for gg in (0, 2, 8, 32):
            for bb in (0, 2, 8, 32):
                g = (r / 255, gg / 255, bb / 255)
                if g[0] / sum(g) < 0.8:
                    continue
                m = max((duv(g, over(f, g, a)), a) for a in ALPHAS)
                if m[0] > worst[0]:
                    worst = (m[0], tuple(round(x * 255) for x in g), m[1])
                if gg == 0 and bb == 0 and m[0] > 0.2:
                    last_over = r
    bright = max(max(duv((r / 255, 0, 0), over(f, (r / 255, 0, 0), a)) for a in ALPHAS)
                 for r in range(8, 256))
    print(f"  {name}: R/(R+G+B) = {f[0] / sum(f):.3f}; largest u'v' step {worst[0]:.3f} over ground {worst[1]}"
          f" at alpha {worst[2]:.4f}; it exceeds 0.2 only over pure-red grounds up to #{last_over:02X}0000;"
          f" over #080000 and brighter its largest step is {bright:.3f}")
