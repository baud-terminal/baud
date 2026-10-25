"""The rest of the Hebrew repertoire: the glyphs build.py adds after round 5's letters and points,
each drawn by baud from its own parts, none taken from another font.

  U+20AA NEW SHEQEL SIGN   sheqel_rec: two interlocking hooks, drawn here.
  U+05C6 NUN HAFUKHA       nun_hafukha_rec: the built nun, reversed.
  U+05EF YOD TRIANGLE      letters(): three of the built yods, a triangle.
  U+05F0-U+05F2            letters(): the Yiddish double vav, vav yod and double yod, two of the
                           built letters side by side in the cell.

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


def nun_hafukha_rec(B, nun):
    """U+05C6, upright: the built nun reversed about the cell's centre line. The Unicode Standard
    (9.1, 'Nun Hafukha'): 'There is also variation in the glyph used: it often has the appearance of
    a rotated or reversed nun and is very often called inverted nun'; the Hebrew chart's reference
    glyph is the reversed form, its stem on the left and its roof and base reaching right."""
    return B.map_rec(nun, lambda x, y: (600 - x, y))      # to_glyph re-winds every contour


def squeezed(B, rec, kx, ky=1.0):
    """`rec` scaled about its ink's centre by kx across and ky up (ky about its top), its strokes then
    thickened back to the weight they had: the letters' own way to change a width (build.py's
    XSCALE, which erodes what a widening adds)."""
    x0, y0, x1, y1 = B.rec_bounds(rec)
    cx = (x0 + x1) / 2
    rec = B.map_rec(rec, lambda x, y: (cx + (x - cx) * kx, y1 + (y - y1) * ky))
    p = B.rec_to_path(rec)
    p = B.dilate(p, (1 - kx) * B.E.STEM_V, (1 - ky) * B.E.BAR)
    return B.path_rec(p)


def placed(B, rec, cx):
    """`rec` moved across so that its ink is centred on x = cx."""
    x0, _, x1, _ = B.rec_bounds(rec)
    return B.map_rec(rec, lambda x, y: (x - (x0 + x1) / 2 + cx, y))


PAIR_INK = (25, 575)     # a digraph's two letters, across the cell: each 252.5 wide, so a vav
PAIR_GAP = 45            # keeps 0.86 of its width and a yod 0.70; 45 apart, half a stem
TRIANGLE_K = 0.70        # the yod triangle's three yods, against the built yod, 30 apart
TRIANGLE_GAP = 30


def letters(B, ST, built):
    """{codepoint: upright outline} for the letters this module adds, made from the round-5 letters
    in `built`, so that the build's anchor search places points on them as on any letter."""
    vav, yod = built[0x05D5], built[0x05D9]
    w = (PAIR_INK[1] - PAIR_INK[0] - PAIR_GAP) / 2
    right_cx, left_cx = PAIR_INK[1] - w / 2, PAIR_INK[0] + w / 2

    def pair(first, second):
        # right to left: the first letter on the right
        a = squeezed(B, first, w / (B.rec_bounds(first)[2] - B.rec_bounds(first)[0]))
        b = squeezed(B, second, w / (B.rec_bounds(second)[2] - B.rec_bounds(second)[0]))
        return B.union_recs(placed(B, a, right_cx), placed(B, b, left_cx))

    out = {0x05F0: pair(vav, vav), 0x05F1: pair(vav, yod), 0x05F2: pair(yod, yod)}
    # U+05EF: two yods below and one above between them, as the chart draws it
    small = squeezed(B, yod, TRIANGLE_K, TRIANGLE_K)
    sx0, sy0, sx1, sy1 = B.rec_bounds(small)
    sw, sh = sx1 - sx0, sy1 - sy0
    top = B.at(placed(B, small, 300), 0, 630 - sy1)
    drop = 630 - sh - TRIANGLE_GAP - sy1          # the lower row's top, below the upper yod's base
    lo_l = B.at(placed(B, small, 300 - (sw + TRIANGLE_GAP) / 2), 0, drop)
    lo_r = B.at(placed(B, small, 300 + (sw + TRIANGLE_GAP) / 2), 0, drop)
    out[0x05EF] = B.union_recs(top, lo_l, lo_r)
    # the dagesh search's box (build.anchors): the letter's own ink
    for u, rec in out.items():
        B.E.DAGESH_BOX.setdefault(u, tuple(round(v) for v in B.rec_bounds(rec)))
    return {u: [(op, tuple((round(x, 1), round(y, 1)) for x, y in args)) for op, args in rec]
            for u, rec in sorted(out.items())}


def glyphs(B, ST, built, extra):
    """(name, rec, advance, codepoint, GDEF class) for every glyph this module adds, in the order
    they are appended: `built` the round-5 letters, `extra` what letters() made. `B` is the build
    module (its helpers), `ST` the style."""
    left, right = sheqel_rec(ST.UPRIGHT)
    out = [('heb_20AA', B.union_recs(left, right), 600, 0x20AA, 1)]
    out.append(('heb_05C6', nun_hafukha_rec(B, built[0x05E0]), 600, 0x05C6, 1))
    for u, rec in extra.items():
        out.append(('heb_%04X' % u, rec, 600, u, 1))
    return out
