#!/usr/bin/env python3
"""The least opacity at which a trail drawn in the cursor's colour over an
opaque ground makes a WCAG 2.3.1 transition, for every ink x ground pair the
census ran, and how far out each effect term stays above it.

The blend is the census's: premultiplied source over the stored byte, in
ENCODED space, rounded to a byte (`BGRA8Unorm`; `over` in
baud-present-macos/tests/cells.rs). Relative luminance and u'v' are WCAG
2.2's and the Understanding page's, as baud-bench/src/flash.rs reads them:

* general: |L(composite) - L(ground)| >= 0.10 with the darker of the two
  below 0.80;
* red: u'v' distance > 0.2 and either state a saturated red,
  R/(R+G+B) >= 0.8 on the linear values. Black has the white point's u'v'.

Then, per effect term (read from the shaders, baud-present-macos/src/
shaders.metal, and the Rust models in baud-render/src), the distance in cell
heights at which that term alone falls under the least flashing opacity.
This is a model of the formulas, not a measurement of frames.
"""
import math

GROUNDS = {"dark": (0x10, 0x10, 0x10), "light": (0xEF, 0xEF, 0xEF),
           "red": (0xCD, 0x00, 0x00), "darkred": (0x1A, 0x00, 0x00)}
# The theme's default foreground is the default ink: #D8D8D8 on the dark
# theme (dark, red, darkred grounds) and #272727 on the light one.
def inks(ground):
    d = (0x27, 0x27, 0x27) if ground == "light" else (0xD8, 0xD8, 0xD8)
    return {"default": d, "white": (255, 255, 255), "black": (0, 0, 0),
            "red": (255, 0, 0)}


def lin(c8):
    c = c8 / 255.0
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


def lum(rgb):
    r, g, b = (lin(v) for v in rgb)
    return 0.2126 * r + 0.7152 * g + 0.0722 * b


WHITE_UV = None


def uv(rgb):
    global WHITE_UV
    r, g, b = (lin(v) for v in rgb)
    x = 0.4124564 * r + 0.3575761 * g + 0.1804375 * b
    y = 0.2126729 * r + 0.7151522 * g + 0.0721750 * b
    z = 0.0193339 * r + 0.1191920 * g + 0.9503041 * b
    den = x + 15 * y + 3 * z
    if den == 0:
        if WHITE_UV is None:
            WHITE_UV = uv((255, 255, 255))
        return WHITE_UV
    return (4 * x / den, 9 * y / den)


def sat_red(rgb):
    r, g, b = (lin(v) for v in rgb)
    s = r + g + b
    return s > 0 and r / s >= 0.8


def over(ink, ground, a):
    return tuple(int(round((c / 255.0 * a + d / 255.0 * (1 - a)) * 255)) for c, d in zip(ink, ground))


def least(ink, ground):
    """Least alpha (to 1/2000) for a general and for a red transition."""
    lg, ug = lum(ground), uv(ground)
    gen = red = None
    for i in range(0, 2001):
        a = i / 2000
        c = over(ink, ground, a)
        lc = lum(c)
        if gen is None and abs(lc - lg) >= 0.10 and min(lc, lg) < 0.80:
            gen = a
        if red is None:
            u = uv(c)
            if math.dist(u, ug) > 0.2 and (sat_red(c) or sat_red(ground)):
                red = a
        if gen is not None and red is not None:
            break
    return gen, red


def smoothstep(e0, e1, x):
    t = min(max((x - e0) / (e1 - e0), 0.0), 1.0)
    return t * t * (3 - 2 * t)


def solve(f, lo, hi, target):
    """Largest x in [lo, hi] with f(x) >= target, f decreasing; None if never."""
    if f(lo) < target:
        return None
    if f(hi) >= target:
        return hi
    for _ in range(60):
        m = (lo + hi) / 2
        if f(m) >= target:
            lo = m
        else:
            hi = m
    return lo


def terms(a_star):
    """Distance, in cell heights from the term's own edge, out to which each
    term alone stays at or above a_star at full envelope (burn, env = 1)."""
    out = {}
    # Fire (fire_flame). Halo: 0.12 exp(-2.5 d0), d0 cells outside the quad.
    out["fire halo, outside the quad"] = solve(lambda d: 0.12 * math.exp(-2.5 * d), 0, 2.0, a_star)
    # Body inside the quad at d0 < -0.25: heat = 0.8 (0.5 + 0.6 n), n the fbm.
    for n in (0.3, 0.5, 0.7):
        heat = 0.8 * (0.5 + 0.6 * n)
        a = min(1.0, smoothstep(0.05, 0.95, heat) + 0.12)
        out[f"fire body inside the quad, n={n}: opacity"] = a
    # A tongue: body 1 (the lifted sample deep in the quad), cooling with d0:
    # heat = (1 - 0.85 d0/1.6)(0.5 + 0.6 n); opacity smoothstep + halo.
    for n in (0.6, 0.85):
        f = lambda d: min(1.0, smoothstep(0.05, 0.95, (1 - 0.85 * min(d / 1.6, 1)) * (0.5 + 0.6 * n))
                          + 0.12 * math.exp(-2.5 * d))
        out[f"fire tongue tip reach above the quad, n={n}"] = solve(f, 0, 1.6, a_star)
    # Orb (orb::light): body 0.45..1.0 inside r = 0.40; halo 0.35 exp(-hd^2),
    # hd = (d - r) / 0.35, cells; comet 0.55 at its head.
    out["orb halo reach past its radius (0.40)"] = solve(lambda d: 0.35 * math.exp(-(d / 0.35) ** 2), 0, 2, a_star)
    out["orb comet peak"] = 0.55
    out["orb body rim"] = 0.45
    # Plasma (plasma_tube, plasma::params): tube k3 inside, sheath k4 e^(-k5 d0).
    return out


def main():
    print("Least opacity for a transition, ink over ground (encoded blend, byte-rounded)")
    print(f"{'ground':<8} {'ink':<8} {'ground L':>9} {'ink L':>7} {'general a*':>11} {'red a*':>8}")
    table = {}
    for g, grgb in GROUNDS.items():
        for i, irgb in inks(g).items():
            gen, red = least(irgb, grgb)
            table[(g, i)] = (gen, red)
            fmt = lambda v: "never" if v is None else f"{v:.4f}"
            print(f"{g:<8} {i:<8} {lum(grgb):9.4f} {lum(irgb):7.4f} {fmt(gen):>11} {fmt(red):>8}")
    print()
    print("Per term, at the least general opacity of four pairs (cells from the edge):")
    for pair in [("light", "black"), ("light", "default"), ("dark", "white"), ("dark", "default")]:
        a_star = table[pair][0]
        print(f"\n  {pair[1]} on {pair[0]}: a* = {a_star:.4f}")
        for k, v in terms(a_star).items():
            if "opacity" in k or k in ("orb comet peak", "orb body rim"):
                print(f"    {k:<52} {v:.3f}  ({'flashes' if v >= a_star else 'does not'})")
            else:
                print(f"    {k:<52} {'never' if v is None else f'{v:.3f} cells'}")


if __name__ == "__main__":
    main()
