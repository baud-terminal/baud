"""The rest of the Hebrew repertoire: the glyphs build.py adds after round 5's letters and points,
each drawn by baud from its own parts, none taken from another font.

  U+20AA NEW SHEQEL SIGN   sheqel_rec: two interlocking hooks, drawn here.
  U+05C6 NUN HAFUKHA       nun_hafukha_rec: the built nun, reversed.
  U+05EF YOD TRIANGLE      letters(): three of the built yods, a triangle.
  U+05F0-U+05F2            letters(): the Yiddish double vav, vav yod and double yod, two of the
                           built letters side by side in the cell.
  U+FB1D-U+FB4F            later(): the Alphabetic Presentation Forms' Hebrew. Each canonical
                           form is a composite of its decomposition at the mark lookup's own
                           offsets; varika is a mark drawn here; the alternative ayin is the
                           ayin; the wide letters are their letters stretched across the cell;
                           the alternative plus sign and the alef lamed ligature are drawn from
                           this font's own parts.
  U+0591-U+05AF            accent_recs(): the 31 cantillation marks, drawn here, placed by
                           late_anchors() and build.py's mark-to-mark lookups.

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


# ---------------------------------------------------------------- the presentation forms

# UnicodeData.txt 17.0's canonical decompositions of U+FB1D-U+FB4F, taken all the way down
# (U+FB2C and U+FB2D are U+FB49 and a dot; U+FB49 is shin and dagesh): the letter, then its
# points in the order the decomposition gives them.
CANONICAL = {
    0xFB1D: (0x05D9, [0x05B4]), 0xFB1F: (0x05F2, [0x05B7]),
    0xFB2A: (0x05E9, [0x05C1]), 0xFB2B: (0x05E9, [0x05C2]),
    0xFB2C: (0x05E9, [0x05BC, 0x05C1]), 0xFB2D: (0x05E9, [0x05BC, 0x05C2]),
    0xFB2E: (0x05D0, [0x05B7]), 0xFB2F: (0x05D0, [0x05B8]), 0xFB30: (0x05D0, [0x05BC]),
    0xFB31: (0x05D1, [0x05BC]), 0xFB32: (0x05D2, [0x05BC]), 0xFB33: (0x05D3, [0x05BC]),
    0xFB34: (0x05D4, [0x05BC]), 0xFB35: (0x05D5, [0x05BC]), 0xFB36: (0x05D6, [0x05BC]),
    0xFB38: (0x05D8, [0x05BC]), 0xFB39: (0x05D9, [0x05BC]), 0xFB3A: (0x05DA, [0x05BC]),
    0xFB3B: (0x05DB, [0x05BC]), 0xFB3C: (0x05DC, [0x05BC]), 0xFB3E: (0x05DE, [0x05BC]),
    0xFB40: (0x05E0, [0x05BC]), 0xFB41: (0x05E1, [0x05BC]), 0xFB43: (0x05E3, [0x05BC]),
    0xFB44: (0x05E4, [0x05BC]), 0xFB46: (0x05E6, [0x05BC]), 0xFB47: (0x05E7, [0x05BC]),
    0xFB48: (0x05E8, [0x05BC]), 0xFB49: (0x05E9, [0x05BC]), 0xFB4A: (0x05EA, [0x05BC]),
    0xFB4B: (0x05D5, [0x05B9]), 0xFB4C: (0x05D1, [0x05BF]), 0xFB4D: (0x05DB, [0x05BF]),
    0xFB4E: (0x05E4, [0x05BF]),
}
# The <font> decompositions: U+FB20 the alternative ayin, U+FB21-U+FB28 the wide letters.
FONT_FORMS = {0xFB20: 0x05E2, 0xFB21: 0x05D0, 0xFB22: 0x05D3, 0xFB23: 0x05D4, 0xFB24: 0x05DB,
              0xFB25: 0x05DC, 0xFB26: 0x05DD, 0xFB27: 0x05E8, 0xFB28: 0x05EA}
VARIKA = 0xFB1E
VARIKA_HOLLOW = 60        # the crescent's inner depth, above its foot
WIDE_INK = (20, 580)      # a wide letter's ink, stretched across the cell as Courier New, a
                          # monospace face, stretches its wide alef (ink 58..1179 of 1229)


def varika_rec(B):
    """U+FB1E, in a point's own frame (centred on x = 0, its bottom on y = 0, as rafe): a crescent
    opening upward, rafe's width, as thick as rafe's bar at its foot and tapering to its tips. The
    Unicode Standard (9.1): 'The varika, used in Judezmo, is a glyphic variant of rafe'; 'rafe and
    varika go above it'. The Alphabetic Presentation Forms chart draws it as a breve above the
    letter."""
    import pathops
    w, t = B.E.BAR_W, B.E.BAR
    h = t + VARIKA_HOLLOW
    def lower(rx, ry):
        p = pathops.Path()
        pen = p.getPen()
        k = 0.5523
        pen.moveTo((-rx, h)); pen.curveTo((-rx, h - k * ry), (-k * rx, h - ry), (0, h - ry))
        pen.curveTo((k * rx, h - ry), (rx, h - k * ry), (rx, h)); pen.closePath()
        return p
    cup = pathops.op(lower(w / 2, h), lower(w / 2, h - t), pathops.PathOp.DIFFERENCE)
    return B.compiled(B.path_rec(cup))


def late_marks(B, ST):
    """{codepoint: outline in a point's own frame} for the marks emitted in later()."""
    out = {VARIKA: varika_rec(B)}
    out.update(accent_recs(B))
    return out


def late_anchors(B, base_a, report, marks):
    """Every letter's anchors for the late marks. Varika's is its rafe anchor, rafe's glyphic
    variant. A cantillation mark below sits where hiriq does (under the letter, its top on the
    points' line); one above, at the letter's upper dot (over its middle, at the points' height);
    a postpositive one at the holam's place, the letter's upper left, and left of a holam there by
    a mark-to-mark lookup, so that it follows the points (Unicode 9.1: pashta 'goes to the left of
    points'); a prepositive one at the shin dot's place, the letter's upper right. A point below
    moves a cantillation mark below beside it by the mark-to-mark lookups (build.py)."""
    def hw(m):
        b = B.rec_bounds(marks[m])
        return (b[2] - b[0]) / 2
    for u, d in base_a.items():
        d[VARIKA] = d[0x05BF]
        report[u][VARIKA] = report[u][0x05BF]
        for m in range(0x0591, 0x05B0):
            if m in ACCENT_BELOW:
                src = 0x05B4
                d[m] = d[src]
            elif m in ACCENT_LEFT:
                src = 0x05B9
                d[m] = d[src]
            elif m in ACCENT_RIGHT:
                src = 0x05C1
                d[m] = d[src]
            else:
                src = 0x05C4
                d[m] = d[src]
            report[u][m] = report[u][src]


def alt_plus_rec(B, ST):
    """U+FB29 HEBREW LETTER ALTERNATIVE PLUS SIGN: JetBrains Mono's own plus sign of this style
    without its arm below the bar, an inverted T, lowered so that its bar lies where the plus's
    lower arm ends. The Unicode Standard (9.1): it 'is used by those who wish to avoid cross
    symbols'; its chart glyph is the inverted T. Faces that draw both set it low: its bar near the
    plus's foot, its stem a little over half the plus's height (measured on Arial, Times New Roman
    and Courier New). Already this style's (the italics' plus leans), so it takes no shear; it is
    lowered along the italics' slant."""
    from fontTools.ttLib import TTFont
    from fontTools.pens.recordingPen import DecomposingRecordingPen
    from geom import FlatPen, runs_at_y
    import pathops
    j = TTFont(B.JBM)
    gs = j.getGlyphSet()
    r = DecomposingRecordingPen(gs)
    gs['plus'].draw(r)
    x0, y0, x1, y1 = B.rec_bounds(r.value)
    fp = FlatPen(None, steps=8)
    for op, args in r.value:
        getattr(fp, op)(*args)
    # the bar: the heights whose run spans most of the sign's width
    full = [y for y in range(int(y0), int(y1) + 1) if any(b - a > 0.6 * (x1 - x0) for a, b in runs_at_y(fp.polys, y))]
    keep = pathops.op(B.rec_to_path(r.value), B.rec_to_path(B.rect(x0 - 100, min(full), x1 + 100, y1 + 100)),
                      pathops.PathOp.INTERSECTION)
    dy = y0 - min(full)
    return B.map_rec(B.path_rec(keep), lambda x, y: (x + ST.SLANT * ST.TAN * dy, y + dy))


def alef_lamed_rec(B, built):
    """U+FB4F HEBREW LIGATURE ALEF LAMED, upright: the built alef with the built lamed's mast rising
    from its upper left, as the chart draws it -- lamed's ink above the letters' 630 (its mast and
    head), set on the alef's upper-left stroke."""
    import pathops
    alef, lamed = built[0x05D0], built[0x05DC]
    lx0, ly0, lx1, ly1 = B.rec_bounds(lamed)
    mast = pathops.op(B.rec_to_path(lamed), B.rec_to_path(B.rect(lx0 - 50, 560, lx1 + 50, ly1 + 50)),
                      pathops.PathOp.INTERSECTION)
    mx0, _, mx1, _ = B.rec_bounds(B.path_rec(mast))
    # the alef's ink at the top of the cell: its leftmost stroke there
    from geom import FlatPen, runs_at_y
    fp = FlatPen(None, steps=8)
    for op, args in alef:
        getattr(fp, op)(*args)
    run = runs_at_y(fp.polys, 600)[0]
    lm = runs_at_y(B.polys(B.path_rec(mast)), 640)[0]
    dx = run[0] - lm[0]
    return B.union_recs(alef, B.at(B.path_rec(mast), dx, 0))


def later(B, ST, built, letters):
    """The glyphs emitted after every other, in order, each a dict: kind 'mark' (a point from
    late_marks), 'outline' (an upright outline, rec), 'plain' (an outline already this style's)
    or 'composite' (base letter and marks);
    anchors_from names the letter whose anchors a base glyph takes."""
    out = [{'kind': 'mark', 'name': 'heb_FB1E', 'u': VARIKA, 'cls': 3}]
    out.append({'kind': 'plain', 'name': 'heb_FB29', 'u': 0xFB29, 'cls': 1, 'rec': alt_plus_rec(B, ST)})
    out.append({'kind': 'outline', 'name': 'heb_FB4F', 'u': 0xFB4F, 'cls': 1,
                'rec': alef_lamed_rec(B, built), 'anchors_from': 0x05D0})
    for u, b in sorted(FONT_FORMS.items()):
        if u != 0xFB20:
            out.append({'kind': 'outline', 'name': 'heb_%04X' % u, 'u': u, 'cls': 1,
                        'rec': widened(B, built[b]), 'anchors_from': b})
        else:
            out.append({'kind': 'composite', 'name': 'heb_%04X' % u, 'u': u, 'cls': 1, 'base': b,
                        'marks': [], 'anchors_from': b})
    for u, (b, ms) in sorted(CANONICAL.items()):
        out.append({'kind': 'composite', 'name': 'heb_%04X' % u, 'u': u, 'cls': 1, 'base': b,
                    'marks': ms, 'anchors_from': b})
    for u in range(0x0591, 0x05B0):
        out.append({'kind': 'mark', 'name': 'heb_%04X' % u, 'u': u, 'cls': 3})
    return out


def widened(B, rec):
    """A wide letter: its letter stretched across to WIDE_INK, its stems eroded back to their weight
    (build.py's XSCALE)."""
    x0, _, x1, _ = B.rec_bounds(rec)
    k = (WIDE_INK[1] - WIDE_INK[0]) / (x1 - x0)
    c = (x0 + x1) / 2
    rec = B.map_rec(rec, lambda x, y: (300 + (x - c) * k, y))
    return B.path_rec(B.erode(B.rec_to_path(rec), (k - 1) * B.E.STEM_V, 0))


# ---------------------------------------------------------------- the cantillation marks

# The 31 marks U+0591-U+05AF, each drawn here in a point's own frame: centred on x = 0, an above
# mark's bottom on y = 0, a below mark's top on y = 0. Their forms are the Hebrew chart's reference
# glyphs (the Standard's set follows SI 1311.2; Unicode 9.1, 'Cantillation Marks'), drawn with this
# font's own dot and a stroke of 0.85 its points' bar.
ACCENT_ABOVE = (0x0592, 0x0593, 0x0594, 0x0595, 0x0597, 0x0598, 0x0599, 0x059C, 0x059D, 0x059E,
                0x059F, 0x05A0, 0x05A1, 0x05A8, 0x05A9, 0x05AB, 0x05AC, 0x05AE, 0x05AF)
ACCENT_BELOW = tuple(u for u in range(0x0591, 0x05B0) if u not in ACCENT_ABOVE)
ACCENT_LEFT = (0x0599, 0x05A9, 0x05AE)       # postpositive: left of the points (pashta 'goes to the left of points')
ACCENT_RIGHT = (0x059D, 0x05A0)              # prepositive above: at the letter's right
ACCENT_FIRST = (0x059A, 0x05AD)              # yetiv and dehi: right of a point below, 'first (on the right)'
LATE_MARKS = (VARIKA,) + tuple(range(0x0591, 0x05B0))
ABOVE = (VARIKA,) + ACCENT_ABOVE
ACCENT_GAP = 30                              # between a mark and the point beside it
# A mark's size against the font's dot (u, 116 units in Regular): faces that carry the marks draw
# them 150 to 190 units tall in a 1000-unit em, two to three of their dots (measured on Arial, Times
# New Roman, Courier New, SF Hebrew and Arial Hebrew); this font's dot is larger, so 1.6 and 1.8.
ACCENT_H, ACCENT_W = 1.6, 1.8
ACCENT_STROKE = 0.85                         # against the points' bar
ACCENT_FINE = 0.6                            # the winding marks' (shalshelet, zarqa, zinor, darga)


def _stroke(B, pts, t, closed=False):
    """A polyline `t` wide along `pts`, its joins mitred (capped at 2t), its ends square."""
    import math, pathops
    n = len(pts)
    def unit(a, b):
        dx, dy = b[0] - a[0], b[1] - a[1]
        d = math.hypot(dx, dy) or 1.0
        return dx / d, dy / d
    left, right = [], []
    for i, p in enumerate(pts):
        ds = []
        if i > 0 or closed:
            ds.append(unit(pts[i - 1], p))
        if i < n - 1 or closed:
            ds.append(unit(p, pts[(i + 1) % n]))
        nx = sum(-d[1] for d in ds); ny = sum(d[0] for d in ds)
        ln = math.hypot(nx, ny) or 1.0
        nx, ny = nx / ln, ny / ln
        # the miter's length over the half width
        cos_half = nx * -ds[0][1] + ny * ds[0][0]
        k = min(2.0, 1.0 / max(cos_half, 0.5))
        left.append((p[0] + nx * t / 2 * k, p[1] + ny * t / 2 * k))
        right.append((p[0] - nx * t / 2 * k, p[1] - ny * t / 2 * k))
    poly = left + right[::-1]
    p = pathops.Path()
    pen = p.getPen()
    pen.moveTo(poly[0])
    for q in poly[1:]:
        pen.lineTo(q)
    pen.closePath()
    return pathops.simplify(p, clockwise=True)


def _arc(cx, cy, rx, ry, a0, a1, n=12):
    import math
    return [(cx + rx * math.cos(math.radians(a0 + (a1 - a0) * i / n)),
             cy + ry * math.sin(math.radians(a0 + (a1 - a0) * i / n))) for i in range(n + 1)]


def _ring(B, cx, cy, r, t):
    import pathops
    def circ(rr):
        p = pathops.Path()
        pen = p.getPen()
        k = 0.5523 * rr
        pen.moveTo((cx + rr, cy)); pen.curveTo((cx + rr, cy + k), (cx + k, cy + rr), (cx, cy + rr))
        pen.curveTo((cx - k, cy + rr), (cx - rr, cy + k), (cx - rr, cy))
        pen.curveTo((cx - rr, cy - k), (cx - k, cy - rr), (cx, cy - rr))
        pen.curveTo((cx + k, cy - rr), (cx + rr, cy - k), (cx + rr, cy)); pen.closePath()
        return p
    return pathops.op(circ(r), circ(r - t), pathops.PathOp.DIFFERENCE)


def accent_recs(B):
    """{codepoint: outline in a point's own frame} for the 31 cantillation marks."""
    import pathops
    t = round(ACCENT_STROKE * B.E.BAR)        # 70 in Regular, 96 in Bold
    dot = B.jbm_dot()
    db = B.rec_bounds(dot)
    dw = db[2] - db[0]
    u = dw                                    # the unit: one dot's width
    P = lambda rec: B.rec_to_path(rec)
    def U(*paths):
        out = pathops.Path()
        for p in paths:
            out = pathops.op(out, p, pathops.PathOp.UNION)
        return out
    def dotat(x, y):
        return P(B.at(dot, x, y))
    S = lambda pts: _stroke(B, pts, t)
    tf = round(ACCENT_FINE * B.E.BAR)         # the finer stroke of the marks that wind: 49 in Regular
    F = lambda pts: _stroke(B, pts, tf)
    h = ACCENT_H * u                          # a mark's height: 186 units in Regular
    w = ACCENT_W * u                          # a mark's width: 209 units in Regular
    r = {}
    # ---- below (drawn with the top at 0, ink downward)
    arch = S(_arc(0, -h, w / 2 - t / 2, h * 0.55, 180, 0))                 # an arch, legs down to -h
    r[0x0591] = U(arch, S([(0, -h + h * 0.55), (0, 0)]))                 # etnahta: an arch on a stem
    vee = S([(-w / 2 + t / 2, 0), (0, -h * 0.55), (w / 2 - t / 2, 0)])
    r[0x05A2] = U(vee, S([(0, -h * 0.55), (0, -h)]))                    # atnah hafukh: a V on a stem
    hook_l = S(_arc(w / 2 - t / 2, -t / 2, w - t, h - t, 180, 270, 16))   # a quarter curve, down then right
    r[0x0596] = hook_l                                                  # tipeha
    r[0x05AD] = hook_l                                                  # dehi: the same form, placed first
    hook_r = S(_arc(-w / 2 + t / 2, -t / 2, w - t, h - t, 0, -90, 16))    # a quarter curve, down then left
    r[0x05A5] = hook_r                                                  # merkha
    small_r = B.path_rec(hook_r)
    half = B.map_rec(small_r, lambda x, y: (x * 0.6, y))
    r[0x05A6] = U(P(B.at(half, -0.42 * w, 0)), P(B.at(half, 0.42 * w, 0)))   # merkha kefula: two
    r[0x059B] = U(P(B.at(half, 0.3 * w, 0)), dotat(-0.45 * w, -dw / 2 - 10))  # tevir: a dot and a merkha
    r[0x05A3] = S([(-w / 2 + t / 2, 0), (-w / 2 + t / 2, -h + t / 2), (w / 2, -h + t / 2)])   # munah: L
    angle = S([(w / 3, 0), (-w / 3, -h / 2), (w / 3, -h)])
    r[0x05A4] = angle                                                   # mahapakh: <
    r[0x059A] = angle                                                   # yetiv: < , placed first
    import math
    r[0x05A7] = F([(-(0.4 * w) * math.sin(2 * math.pi * i / 16), -tf / 2 - (h - tf) * i / 16)
                   for i in range(17)])                                 # darga: an S, one wave down
    r[0x05AA] = S(_arc(0, -h / 2 + t / 2, w / 2 - t / 2, h / 2 - t / 2, 180, 360))  # yerah ben yomo: a cup
    # ---- above (drawn with the bottom at 0, ink upward)
    r[0x0592] = U(dotat(0, dw * 1.6), dotat(-dw * 0.9, dw / 2), dotat(dw * 0.9, dw / 2))   # segol: three dots, one over two
    zig = []
    for i in range(5):
        zig.append(((-1) ** i * w / 4, i * (1.25 * h - tf) / 4 + tf / 2))   # 1.25 marks tall
    r[0x0593] = F(zig)                                                  # shalshelet: a zigzag
    r[0x0594] = U(dotat(0, dw / 2), dotat(0, dw * 1.5 + 30))            # zaqef qatan: two dots
    r[0x0595] = U(dotat(dw * 0.5, dw / 2), dotat(dw * 0.5, dw * 1.5 + 30),
                  S([(-dw * 0.6, 0), (-dw * 0.6, dw * 2 + 30)]))         # zaqef gadol: a stroke and two dots
    r[0x0597] = _diamond(u * 1.1)                                       # revia: a diamond
    zarqa = F(_arc(-0.3 * w, h / 2, 0.3 * w - tf / 2, h / 3, 0, 180, 16)[::-1] +
              _arc(0.3 * w, h / 2, 0.3 * w - tf / 2, h / 3, 180, 360, 16)[1:])
    r[0x0598] = zarqa                                                   # zarqa: a sideways S
    r[0x05AE] = zarqa                                                   # zinor: the same, placed left
    pashta = S([(-w / 2, h - t / 2)] + _arc(-w / 2 + (w - t) / 2 * 0, 0, w - t, h - t / 2, 90, 0)[1:])
    r[0x0599] = pashta                                                  # pashta: over and down
    r[0x05A8] = pashta                                                  # qadma: the same, centred
    geresh = S([(-w / 4, 0)] + _arc(w / 4, 0, w / 2, h - t / 2, 180, 90)[1:])
    r[0x059C] = geresh                                                  # geresh: up and over
    r[0x059D] = geresh                                                  # geresh muqdam: the same, placed right
    g_half = B.map_rec(B.path_rec(geresh), lambda x, y: (x * 0.6, y))
    r[0x059E] = U(P(B.at(g_half, -0.38 * w, 0)), P(B.at(g_half, 0.38 * w, 0)))   # gershayim: two
    ring_r = u * 0.42
    def lolly(sx):                                                      # a ring on a short stalk
        return U(_ring(B, 0, h - ring_r, ring_r, t * 0.6),
                 S([(sx * ring_r * 0.5, h - 2 * ring_r + t * 0.3), (sx * ring_r * 1.3, t / 2)]))
    r[0x05A0] = lolly(-1)                                               # telisha gedola: stalk down-left
    r[0x05A9] = lolly(1)                                                # telisha qetana: stalk down-right
    r[0x059F] = U(B.rec_to_path(B.map_rec(B.path_rec(lolly(1)), lambda x, y: (x - 0.42 * w, y))),
                  B.rec_to_path(B.map_rec(B.path_rec(lolly(-1)), lambda x, y: (x + 0.42 * w, y))))   # qarney para
    r[0x05A1] = S([(-w / 3, 0), (w / 6, h * 0.75)] + _arc(w / 6 + w / 6, h * 0.75, w / 6, h * 0.25 - t / 2, 180, 0)[1:])   # pazer
    r[0x05AB] = S([(w / 3, 0), (-w / 3, h / 2), (w / 3, h)])           # ole: < above
    r[0x05AC] = S([(-w / 2, t / 2), (w / 2 - t / 2, t / 2), (w / 2 - t / 2, h)])   # iluy: a reversed L above
    r[0x05AF] = _ring(B, 0, ring_r * 1.2, ring_r * 1.2, t * 0.7)        # masora circle
    out = {}
    for cpt, p in r.items():
        rec = B.path_rec(p if isinstance(p, pathops.Path) else B.rec_to_path(p))
        x0, y0, x1, y1 = B.rec_bounds(rec)
        # centre across; an above mark's bottom on 0, a below mark's top on 0
        dy = -y0 if cpt in ACCENT_ABOVE else -y1
        out[cpt] = B.compiled(B.at(rec, -(x0 + x1) / 2, dy))
    return out


def _diamond(s):
    import pathops
    p = pathops.Path()
    pen = p.getPen()
    pen.moveTo((0, 0)); pen.lineTo((s / 2, s / 2)); pen.lineTo((0, s)); pen.lineTo((-s / 2, s / 2)); pen.closePath()
    return p
