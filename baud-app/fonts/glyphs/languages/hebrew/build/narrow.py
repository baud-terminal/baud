"""Narrow-letter variants (vav, final nun, yod, zayin) for the round 2 build.

Every variant replaces only these letters; everything else in the font is
round 2's. Units are JetBrains Mono's: cell 600 wide, baseline 0, letter top
630, horizontal stroke 82, vertical stem 90, descender -180. Every outline
is re-centred so its ink is centred in the cell (x 300), as round 2's are.

A shape is built from rectangles (exact stems and bars, flat terminals) or,
for the curved letters (yod, zayin), from round 2's own outline plus
rectangles, so their curves stay the designer's.
"""
import os

TOP, BAR, STEM = 630, 82, 90
HEAD_Y0 = TOP - BAR          # 548: the head bar's bottom
DESC = -180
HEAD_ADD = 0                 # build-bi: added to every drawn head width, so a variant keeps its width relative to
                             # that weight's own (variant A) vav head: Regular 292 -> 0; Bold's A head is 318 -> 26


def rect(x0, y0, x1, y1):
    return [('moveTo', ((x0, y0),)), ('lineTo', ((x0, y1),)), ('lineTo', ((x1, y1),)),
            ('lineTo', ((x1, y0),)), ('closePath', ())]


def vav_like(B, head_w, bottom=0, foot=None, tag=None):
    """A vav (bottom 0) or final nun (bottom -180): a head bar head_w wide
    whose right end is the stem's right edge, the stem 90 wide down to
    `bottom`. foot = (left, right): a bar 82 thick on the bottom, reaching
    `left` units left of the stem and `right` units right of it. tag =
    (w, depth): a block w wide at the head's left end, hanging `depth`
    units below the bar (a kotz). Returned with its ink centred on 300."""
    head_w = head_w + HEAD_ADD
    sr = 1000.0                  # build at an arbitrary x, centre afterwards
    sl = sr - STEM
    parts = [rect(sr - head_w, HEAD_Y0, sr, TOP), rect(sl, bottom, sr, TOP)]
    if foot:
        fl, fr = foot
        parts.append(rect(sl - fl, bottom, sr + fr, bottom + BAR))
    if tag:
        w, d = tag
        parts.append(rect(sr - head_w, HEAD_Y0 - d, sr - head_w + w, TOP))
    return centred(B, B.union_recs(*parts))


def centred(B, rec):
    b = B.rec_bounds(rec)
    dx = 300 - (b[0] + b[2]) / 2
    return B.map_rec(rec, lambda x, y: (round(x + dx, 1), round(y, 1)))


def with_parts(B, base, parts_fn):
    """Round 2's outline `base` united with rectangles parts_fn(bounds, stem run), re-centred."""
    P = B.polys(base)
    st = B.runs_at_y(P, 200)
    extra = parts_fn(B.rec_bounds(base), st)
    return centred(B, B.union_recs(base, *extra))


def shift_left_end(B, rec, cut, dx, ylo=-1e9, yhi=1e9):
    """Move every point left of x=cut (inside ylo..yhi) by dx: a weight-preserving
    width change through a horizontal stroke (round 1's edit method)."""
    return [(op, tuple((x + dx, y) if x < cut and ylo <= y <= yhi else (x, y) for x, y in args))
            for op, args in rec]


# ---------------------------------------------------------------- the variants
# Each entry maps a codepoint to a function (B, round-2 outline) -> outline.
# A letter not listed keeps round 2's outline.

VAV, FNUN, YOD, ZAYIN = 0x05D5, 0x05DF, 0x05D9, 0x05D6


def variant_B():
    """Wider plain heads: vav and final nun 372 (0.62 of the cell), sharp corner;
    yod (360) and zayin (380) are already in 0.60-0.65 and stay round 2's."""
    W = float(os.environ.get('B_HEAD', 372))
    return {VAV: lambda B, r: vav_like(B, W),
            FNUN: lambda B, r: vav_like(B, W, bottom=DESC)}


def variant_C():
    """A short foot. Which side is set by C_FOOT ('right', 'sym', 'left'); the head
    stays round 2's 292. Zayin gets the same foot under its stem, final nun at the
    end of its descender. Yod has no stem on the baseline and keeps round 2's."""
    side = os.environ.get('C_FOOT', 'right')
    n = float(os.environ.get('C_FOOT_LEN', 90))
    head = float(os.environ.get('C_HEAD', 292))
    foot = {'right': (0, n), 'sym': (n / 2, n / 2), 'left': (n, 0)}[side]

    def zayin(B, r):
        def parts(b, st):
            sl, sr = st[-1]
            fl, fr = foot
            return [rect(sl - fl, 0, sr + fr, BAR)]
        return with_parts(B, r, parts)
    return {VAV: lambda B, r: vav_like(B, head, foot=foot),
            FNUN: lambda B, r: vav_like(B, head, bottom=DESC, foot=foot),
            ZAYIN: zayin}


def variant_D():
    """A head with a kotz: a short block hanging from the head's left end, the
    traditional vav/yod head drawn flat. Vav and final nun D_HEAD wide; yod's head
    gets the same kotz; zayin stays round 2's."""
    W = float(os.environ.get('D_HEAD', 348))
    tw = float(os.environ.get('D_TAG_W', 90))
    td = float(os.environ.get('D_TAG_D', 70))
    yod_tag = os.environ.get('D_YOD', '1') == '1'

    def yod(B, r):
        def parts(b, st):
            return [rect(b[0], HEAD_Y0 - td, b[0] + tw, TOP)]
        return with_parts(B, r, parts) if yod_tag else r
    return {VAV: lambda B, r: vav_like(B, W, tag=(tw, td)),
            FNUN: lambda B, r: vav_like(B, W, bottom=DESC, tag=(tw, td)),
            YOD: yod}


VARIANTS = {'A': lambda: {}, 'B': variant_B, 'C': variant_C, 'D': variant_D}


def poly(*pts):
    out = [('moveTo', (pts[0],))]
    out += [('lineTo', (p,)) for p in pts[1:]]
    return out + [('closePath', ())]


def wedge_vav(B, head_w, depth, run, bottom=0):
    """A vav whose head's underside slopes down toward its left end: the bar is
    82 thick at the stem and 82+depth at the left terminal (still a vertical
    cut), the slope spanning the head's last `run` units. Ink centred."""
    head_w = head_w + HEAD_ADD
    sr = 1000.0
    sl = sr - STEM
    x0 = sr - head_w
    run = min(run, sl - x0)
    head = poly((x0, TOP), (sr, TOP), (sr, HEAD_Y0), (x0 + run, HEAD_Y0), (x0, HEAD_Y0 - depth))
    return centred(B, B.union_recs(head, rect(sl, bottom, sr, TOP)))


def variant_W():
    """D candidate: a wedge head (kotz drawn as a slope under the head's left end)."""
    W = float(os.environ.get('W_HEAD', 312))
    dp = float(os.environ.get('W_DEPTH', 50))
    run = float(os.environ.get('W_RUN', 150))
    yod_on = os.environ.get('W_YOD', '1') == '1'
    zay_on = os.environ.get('W_ZAYIN', '0') == '1'

    def yod(B, r):
        def parts(b, st):
            x0 = b[0]
            return [poly((x0, TOP), (x0 + run + 30, TOP), (x0 + run, HEAD_Y0 - 1), (x0, HEAD_Y0 - 1 - dp))]
        return with_parts(B, r, parts) if yod_on else r

    def zayin(B, r):
        def parts(b, st):
            x0 = b[0]
            return [poly((x0, TOP), (x0 + run + 30, TOP), (x0 + run, HEAD_Y0), (x0, HEAD_Y0 - dp))]
        return with_parts(B, r, parts) if zay_on else r
    return {VAV: lambda B, r: wedge_vav(B, W, dp, run),
            FNUN: lambda B, r: wedge_vav(B, W, dp, run, bottom=DESC),
            YOD: yod, ZAYIN: zayin}


VARIANTS['W'] = variant_W


# ---------------------------------------------------------------- the final set, explicit parameters
KOTZ = dict(depth=54, run=150)      # the wedge under a head's left end: 82 -> 136 thick at the terminal


def wedge_on(B, r, depth, run, bar_bottom=None):
    """Round 2's curved letter (yod, zayin) with the same wedge under its head's left end."""
    if bar_bottom is None:
        bar_bottom = HEAD_Y0         # read at call time, so a style that sets HEAD_Y0 gets its own

    def parts(b, st):
        x0 = b[0]
        return [poly((x0, TOP), (x0 + run + 30, TOP), (x0 + run, bar_bottom), (x0, bar_bottom - depth))]
    return with_parts(B, r, parts)


def footed(head, foot):
    def zayin(B, r):
        def parts(b, st):
            sl, sr = st[-1]
            return [rect(sl - foot[0], 0, sr + foot[1], BAR)]
        return with_parts(B, r, parts)
    return {VAV: lambda B, r: vav_like(B, head, foot=foot),
            FNUN: lambda B, r: vav_like(B, head, bottom=DESC, foot=foot),
            ZAYIN: zayin}


FINAL = {
    # A: round 2 as it is
    'A': lambda: {},
    # B: the brief's wide head, 372 = 0.62 of the cell (resh is 418), sharp corner, and the
    # kotz wedge as the tag that resh lacks; yod (360) and zayin (380) already sit in 0.60-0.65
    'B': lambda: {VAV: lambda B, r: wedge_vav(B, 372, **KOTZ),
                  FNUN: lambda B, r: wedge_vav(B, 372, bottom=DESC, **KOTZ)},
    # C: round 2's 292 head and a short foot on the right only, 60 beyond the stem (bet's
    # base overhangs its stem by 82); zayin the same under its stem, final nun at -180;
    # yod has no stem on the baseline and stays round 2's
    'C': lambda: footed(292, (0, 60)),
    # D: a 312 head (0.52) whose underside slopes down to its left terminal (the kotz,
    # flat-cut), the same wedge on yod's and zayin's heads, final nun as vav
    'D': lambda: {VAV: lambda B, r: wedge_vav(B, 312, **KOTZ),
                  FNUN: lambda B, r: wedge_vav(B, 312, bottom=DESC, **KOTZ),
                  YOD: lambda B, r: wedge_on(B, r, bar_bottom=HEAD_Y0 - 1, **KOTZ),
                  ZAYIN: lambda B, r: wedge_on(B, r, **KOTZ)},
    # E: C with the foot on both sides, 40 each (JBM's i has a full slab foot)
    'E': lambda: footed(292, (40, 40)),
}
VARIANTS.update({'F' + k: v for k, v in FINAL.items()})


# ---------------------------------------------------------------- supplementary builds (not variants)
NUN, GIMEL = 0x05E0, 0x05D2
SUPP = {
    # B without the kotz: the plain 372 head, round 1's "wider square head"
    'B0': lambda: {VAV: lambda B, r: vav_like(B, 372), FNUN: lambda B, r: vav_like(B, 372, bottom=DESC)},
    # D with the same wedge on nun's and gimel's heads, for the nun/gimel question
    'DNG': lambda: {**FINAL['D'](), NUN: lambda B, r: wedge_on(B, r, **KOTZ),
                    GIMEL: lambda B, r: wedge_on(B, r, **KOTZ)},
}
VARIANTS.update({'S' + k: v for k, v in SUPP.items()})
