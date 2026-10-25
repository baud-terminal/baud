"""The rest of the Hebrew repertoire: the glyphs build.py adds after round 5's letters and points,
each drawn by baud from its own parts, none taken from another font.

  U+20AA NEW SHEQEL SIGN   sheqel_rec: two interlocking hooks, drawn here.

Every glyph here is appended after every glyph the earlier rounds made, so no existing glyph ID
moves. An outline is drawn upright; the italics take it through build.italic_outline, the shear and
JetBrains Mono Italic's correction of rounds, as the letters do.
"""
import math

# The two uprights' strokes: the sheqel sign's stems, a little lighter than a letter's (90 and
# 125), so that its four stems and three gaps fit one cell; and its arches' horizontal thickness,
# JetBrains Mono's crossbar (82 and 113, style.py).
STEM = {'Regular': 82, 'Bold': 104}
ARCH = {'Regular': 78, 'Bold': 100}
TOP = 630                 # the Hebrew letters' height
LEFT, RIGHT = 30, 570     # the sign's ink, inside the 600-unit cell
KAPPA = 0.68              # a quarter arc's handle ratio, and SHOULDER its height over half the arch's
SHOULDER = 0.72           # width: squarer than a circle, as JetBrains Mono's n and the built het


def _quarter(cx, cy, rx, ry, a, b):
    """A cubic quarter arc of the ellipse (cx, cy, rx, ry) from angle a to b (multiples of 90)."""
    def pt(t):
        return (cx + rx * math.cos(math.radians(t)), cy + ry * math.sin(math.radians(t)))
    p0, p3 = pt(a), pt(b)
    s = 1 if b > a else -1
    # tangents at a and b
    t0 = (-math.sin(math.radians(a)) * s, math.cos(math.radians(a)) * s)
    t1 = (-math.sin(math.radians(b)) * s, math.cos(math.radians(b)) * s)
    p1 = (p0[0] + KAPPA * rx * t0[0], p0[1] + KAPPA * ry * t0[1])
    p2 = (p3[0] - KAPPA * rx * t1[0], p3[1] - KAPPA * ry * t1[1])
    return p1, p2, p3


def _hook(x0, x1, s, h, top, end):
    """An arch from a stem at x0 (full height, from y 0) to a stem ending at x1 (its right edge),
    down to y `end`: the sign's left hook. Stems `s` wide, the arch `h` thick at its crown."""
    rx = (x1 - x0) / 2
    cx = x0 + rx
    ry = SHOULDER * rx
    cy = top - ry
    rxi, ryi = rx - s, ry - h
    rec = [('moveTo', ((x0, 0),)), ('lineTo', ((x0, cy),))]
    rec.append(('curveTo', _quarter(cx, cy, rx, ry, 180, 90)))
    rec.append(('curveTo', _quarter(cx, cy, rx, ry, 90, 0)))
    rec += [('lineTo', ((x1, end),)), ('lineTo', ((x1 - s, end),)), ('lineTo', ((x1 - s, cy),))]
    rec.append(('curveTo', _quarter(cx, cy, rxi, ryi, 0, 90)))
    rec.append(('curveTo', _quarter(cx, cy, rxi, ryi, 90, 180)))
    rec += [('lineTo', ((x0 + s, 0),)), ('closePath', ())]
    return rec


def sheqel_rec(upright):
    """U+20AA, upright: a left hook (a full stem, an arch, a stem coming down inside the right
    hook) and the same hook turned half a turn about the cell's centre, so that the two interlock --
    four stems, left to right: the left hook's outer, the right hook's inner (up from the base),
    the left hook's inner (down from the top), the right hook's outer: the form of the Currency
    Symbols chart's reference glyph. The Unicode Standard names the character and no more (9.1,
    'Currency Symbol'). Its height is the Hebrew letters', 630, as Arial's and Courier New's,
    which carry Hebrew, draw it at their Hebrew letters' height."""
    s, h = STEM[upright], ARCH[upright]
    g = (RIGHT - LEFT - 4 * s) / 3
    x1 = LEFT + 3 * s + 2 * g            # the left hook's inner stem's right edge
    end = h + g                          # where that stem stops above the right hook's base
    left = _hook(LEFT, x1, s, h, TOP, end)
    cx, cy = (LEFT + RIGHT) / 2, TOP / 2
    right = [(op, tuple((2 * cx - x, 2 * cy - y) for x, y in args)) for op, args in left]
    return left, right


def glyphs(B, ST):
    """(name, rec, advance, codepoint, GDEF class) for every glyph this module adds, in the order
    they are appended. `B` is the build module (its helpers), `ST` the style."""
    left, right = sheqel_rec(ST.UPRIGHT)
    out = [('heb_20AA', B.union_recs(left, right), 600, 0x20AA, 1)]
    return out
