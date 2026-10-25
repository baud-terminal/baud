"""One build path for every style of the prototype (build-bi, from round 4's
build3.py): Cascadia Mono's Hebrew (GitHub release v2407.24), re-weighted and
fitted to JetBrains Mono's cell, with round 4's points, anchors, descenders,
paseq and OS/2 bits, merged into a copy of the JetBrains Mono file of STYLE.

Run:  python3 build.py OUT.ttf
  env STYLE=Regular|Bold|Italic|BoldItalic, LEAN=R|L (italics), VARIANT=A..E, LAMED=L1|L2,
      DIP_FLOOR=1 LOWEST_DIP=-293 (round 4's Regular floor; Bold has its own, style.py)
Regular is round 4 unchanged (gated against r4/final); Bold is the upright
design from Cascadia's wght 700 with Bold's own constants (style.py); the
italics are the upright Regular / Bold design sheared after the anchor search,
every outline and every anchor with the same map (style.shear_xy).

baud's copy: ported from the research tree (bold2/build.py) for
scripts/build-fonts.sh, which runs it under `env -i` with each style's exact
environment and gates the output's sha256. What changed against the research
copy, and nothing else: the absolute sys.path line is gone (every module is in
this directory, sys.path[0]); exact_gap imports flatten, segs and pt_seg_min
from geom.py instead of the critic's v4_clear.py, which imported uharfbuzz at
module top; and the save pins fontTools' USE_HARFBUZZ_REPACKER to True (see
save_pinned). Comments that name fix/, r2/, critic or *.py files not in this
directory cite the research tree, which is not committed; note AC
(docs/research/notes/ac-built-font.md) is the record of those rounds.

Two conformance changes since the port, both outside the design (2026-10-08,
the owner's rule that a departure from the standard or the licence is a
defect): nameID 7 carries Cascadia's trademark notice after JetBrains Mono's
(OFL-FAQ 1.1-update7 Q3.7, "Any trademark notices must remain in any derivative
fonts"), and GSUB and GPOS each gain a 'hebr' script record naming DFLT's Script
table (with_script_copy). No outline, anchor, lookup or feature changed.

Round 5 (2026-10-24): letter_rec_r1 applies distinct.py's map after design.py's
edits, to the outline and its anchor points alike. It moves dalet's leg to the
roof's right end, so that ד no longer reads as ז; no other letter changes.

Since 2026-10-25, cover.py's glyphs (U+20AA NEW SHEQEL SIGN) are appended after every glyph above,
so no glyph ID moves; an outline drawn upright there takes the italics' shear and correction as the
letters do.
"""
import sys, os, math, json, struct
import numpy as np
import pathops
from fontTools.ttLib import TTFont
from fontTools.ttLib.tables.DefaultTable import DefaultTable
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.pens.cu2quPen import Cu2QuPen
from fontTools.pens.recordingPen import DecomposingRecordingPen, RecordingPen
from fontTools.pens.boundsPen import BoundsPen
from fontTools.otlLib import builder as otl
from fontTools.ttLib.tables import otTables as ot
import design as D          # round 1: letter weights, widths, edits
import design3 as E         # round 3 (round 2's values, then round 3's)
import narrow as N          # narrow-letter variants
import style as ST          # the STYLE parameter
import distinct as DS       # round 5: letters kept apart
import optical as OP        # the italics' correction of rounds (JetBrains Mono Italic's)
import cover as CV          # the rest of the repertoire, appended after every glyph above
import field as FD
from geom import FlatPen, runs_at_y
from pin import save_pinned, table_pinned    # baud: the repacker pin, shared with instance.py

ST.apply(D, E, N)


def italic_outline(rec, label=''):
    """A letter's italic outline: the shear, and JetBrains Mono Italic's correction of its rounds
    (optical.py). Every use of a sheared letter goes through here -- the glyph written and the
    distance fields fit_italic and fit_meteg place points against -- so the anchors are fitted to
    the outline the font carries."""
    return OP.italic_rec(rec, ST.shear_xy, ST.ROUND_K, label, ST.LEVEL)

W = ST.HERE
JBM = ST.JBM_TARGET            # merged into (read only)
JBM_UP = ST.JBM_UPRIGHT        # the upright weight's dot and colon (read only)
CAS = E.CAS                    # read only: GitHub release v2407.24 CascadiaMono.ttf at wght 400 (Regular) or 700 (Bold)

cas = TTFont(CAS)
cgs = cas.getGlyphSet()
ccm = cas.getBestCmap()

# ---------------------------------------------------------------- paths


def rec_to_path(value):
    p = pathops.Path()
    pen = p.getPen()
    for op, args in value:
        getattr(pen, op)(*args)
    return p


def cas_path(name):
    r = DecomposingRecordingPen(cgs)
    cgs[name].draw(r)
    return rec_to_path(r.value)


def translated(p, dx, dy):
    return p.transform(1, 0, 0, 1, dx, dy)


PATHOPS_RETRIES = []           # (op, area plain, area retried): skia-pathops lost part of a shape


def _morph(p, ex, ey, n, op, simplify_each):
    res = pathops.Path(p)
    for k in range(1, n + 1):
        for s in (1, -1):
            for dx, dy in ((0, ey), (ex, 0)):
                if dx or dy:
                    res = pathops.op(res, translated(p, s * dx / 2 * k / n, s * dy / 2 * k / n), op)
                    if simplify_each:
                        res = pathops.simplify(res, clockwise=True)
    return res


def _checked(p, ex, ey, n, op):
    """The plain iterated boolean (round 4's), checked against the same with a
    simplify after every step: skia-pathops can drop most of a contour in the
    iterated form (Bold's samekh: 178695 -> 78309 units^2 for a 5.6-unit
    erosion). The plain result is kept unless the two disagree by >1% of area,
    so a shape the plain form gets right is byte-for-byte round 4's."""
    plain = _morph(p, ex, ey, n, op, False)
    safe = _morph(p, ex, ey, n, op, True)
    if abs(abs(plain.area) - abs(safe.area)) > 0.01 * abs(safe.area):
        PATHOPS_RETRIES.append((op, round(abs(plain.area)), round(abs(safe.area))))
        return safe
    return plain


def erode(p, ex, ey, n=4):
    return _checked(p, ex, ey, n, pathops.PathOp.INTERSECTION)


def dilate(p, dx, dy, n=4):
    return _checked(p, dx, dy, n, pathops.PathOp.UNION)


def path_rec(p):
    r = RecordingPen()
    p.draw(r)
    return r.value


def map_rec(value, fn):
    return [(op, tuple(fn(*a) for a in args)) for op, args in value]


def rec_bounds(value):
    bp = BoundsPen(None)
    for op, args in value:
        getattr(bp, op)(*args)
    return bp.bounds


def union_recs(*recs):
    p = pathops.Path()
    for r in recs:
        p = pathops.op(p, rec_to_path(r), pathops.PathOp.UNION)
    return path_rec(p)


def to_glyph(value):
    p = rec_to_path(value)
    p = pathops.simplify(p, clockwise=True)
    tp = TTGlyphPen(None)
    p.draw(Cu2QuPen(tp, max_err=0.4, reverse_direction=False))
    return tp.glyph()


def polys(rec):
    p = FlatPen(None, steps=24)
    for op, args in rec:
        getattr(p, op)(*args)
    return p.polys


def rect(x0, y0, x1, y1):
    return [('moveTo', ((x0, y0),)), ('lineTo', ((x0, y1),)), ('lineTo', ((x1, y1),)),
            ('lineTo', ((x1, y0),)), ('closePath', ())]


# ---------------------------------------------------------------- letters (round 1, then round 2 edits)

SX, SY, EY, EX = D.SX, D.SY, D.EY, D.EX
CAS_ANCHORS = json.load(open(E.CAS_ANCHORS_FILE))
DAGESH_CENTRE = (600, 544.5)   # the ink centre of Cascadia's dagesh, in its own frame (measured: 400 and 700 alike)
MAPPED = {}                    # u -> {'dagesh': (x, y)}: the Cascadia designer's dagesh, final frame


def letter_rec_r1(u):
    """Round 1's letter, unchanged (build.py letter_rec)."""
    name = ccm[u]
    p = cas_path(name)
    if EY or EX:
        p = erode(p, EX, EY)
    if D.DX:
        p = dilate(p, D.DX, 0)
    rec = path_rec(p)
    pts = {}
    da = CAS_ANCHORS.get('uni05BC', {}).get(hex(u))
    if da:
        pts['dagesh'] = (DAGESH_CENTRE[0] + da[0], DAGESH_CENTRE[1] + da[1])
    sc = lambda x, y: (x * SX, (y - EY / 2) * SY)
    rec = map_rec(rec, sc)
    pts = {k: sc(*v) for k, v in pts.items()}
    k = D.XSCALE.get(u)
    if k:
        b = rec_bounds(rec)
        c = (b[0] + b[2]) / 2
        xs = lambda x, y: (c + (x - c) * k, y)
        rec = map_rec(rec, xs)
        pts = {kk: xs(*v) for kk, v in pts.items()}
        rec = path_rec(erode(rec_to_path(rec), (k - 1) * E.STEM_V, 0))
    for edit in (False, True):
        if edit:
            rec = D.edit_letter(u, rec)
            pts = {k: D.edit_letter(u, [('moveTo', (v,))])[0][1][0] for k, v in pts.items()}
            f = DS.point_map(u, rec)
            if f:
                rec = map_rec(rec, f)
                pts = {k: f(*v) for k, v in pts.items()}
        b = rec_bounds(rec)
        cx = (b[0] + b[2]) / 2
        shift = 300 + (D.NUDGE.get(u, 0) if edit else 0) - cx
        rec = map_rec(rec, lambda x, y: (x + shift, y))
        pts = {k: (v[0] + shift, v[1]) for k, v in pts.items()}
    MAPPED[u] = pts
    return map_rec(rec, lambda x, y: (round(x, 1), round(y, 1)))


DESC_FROM_USED = {}


def letter_rec(u):
    rec = letter_rec_r1(u)
    if u in E.DESCENDERS:
        # step 3: the descender's terminal points move up; only the bottom
        # vertices sit below -100 (checked: two points per letter at -252).
        # Bold reads the bottom from its own outline.
        frm = rec_bounds(rec)[1] if E.DESC_MEASURED else E.DESC_FROM
        DESC_FROM_USED[u] = round(frm, 1)
        dy = E.DESC_TO - frm
        rec = map_rec(rec, lambda x, y: (x, y + dy) if y < -100 else (x, y))
    if u == 0x05EA and E.TAV_LEG_DX:
        # tav's counter: the left leg moves left, fading out into the bar's
        # junction above y=420..540; the foot's left end (x<120) stays
        def f(x, y):
            if x < 120 or x > 300 or y > 540:
                return (x, y)
            w = 1.0 if y <= 420 else (540 - y) / 120
            return (round(x + E.TAV_LEG_DX * w, 1), y)
        rec = map_rec(rec, f)
    return rec


def paseq_rec():
    w = E.PASEQ_W
    return rect(300 - w / 2, E.PASEQ_Y[0], 300 + w / 2, E.PASEQ_Y[1])


# ---------------------------------------------------------------- points

def jbm_dot():
    """JetBrains Mono's dot (dotbelowcomb), centred on 0, scaled to DOT_W."""
    j = TTFont(JBM_UP)
    gs = j.getGlyphSet()
    r = DecomposingRecordingPen(gs)
    gs['dotbelowcomb'].draw(r)
    b = rec_bounds(r.value)
    cx, cy = (b[0] + b[2]) / 2, (b[1] + b[3]) / 2
    k = E.DOT_W / (b[2] - b[0])
    ky = k if E.DOT_H is None else E.DOT_H / (b[3] - b[1])   # bold-sheva-r2: the dot's own height
    return map_rec(r.value, lambda x, y: ((x - cx) * k, (y - cy) * ky))


def at(rec, dx, dy):
    return map_rec(rec, lambda x, y: (x + dx, y + dy))


def top_at(rec, y=0):
    b = rec_bounds(rec)
    return at(rec, -(b[0] + b[2]) / 2, y - b[3])


def bottom_at(rec, y=0):
    b = rec_bounds(rec)
    return at(rec, -(b[0] + b[2]) / 2, y - b[1])


def make_marks():
    """Every point in local coordinates: centred on x=0; a below point's top
    at y=0, an above point's bottom at y=0, the dagesh centred on (0, 0).
    The anchor is (0, 0); build() shifts outline and anchor to its home."""
    dot = jbm_dot()
    db = rec_bounds(dot)
    dw, dh = db[2] - db[0], db[3] - db[1]
    row = dh + E.GV                       # sheva and the hatafs: 40 between the rows
    hx = (dw + E.GH) / 2

    def dots(*cs):
        return union_recs(*[at(dot, x, y) for x, y in cs])

    sheva = top_at(dots((0, 0), (0, -row)))
    tsere = top_at(dots((-hx, 0), (hx, 0)))
    # segol: tsere's two dots over one, its rows packed until the diagonal gap is 40
    segol = top_at(dots((-hx, 0), (hx, 0), (0, -E.SEG_PITCH)))
    hiriq = top_at(dot)
    # qubuts: three dots on a diagonal, every gap >= 40, as tall as segol allows
    qub = top_at(dots((-E.QUB_DX, 0), (0, -E.QUB_PITCH / 2), (E.QUB_DX, -E.QUB_PITCH)))
    patah = top_at(rect(-E.BAR_W / 2, -E.BAR, E.BAR_W / 2, 0))

    def qamats(w, h):
        return top_at(union_recs(rect(-w / 2, -E.BAR, w / 2, 0), rect(-E.BAR / 2, -h, E.BAR / 2, -E.BAR + 1)))

    qam = qamats(E.BAR_W, E.QAM_H)
    qq = qamats(E.BAR_W, E.QQ_H)

    def hataf(v, centre_bar=False):
        vb = rec_bounds(v)
        sb = rec_bounds(sheva)
        # a bar (patah, qamats) is centred on the sheva's upper dot, as the
        # two read as one row; its top then sits (dh - BAR) / 2 below the dot's
        dy = -max((dh - E.BAR) / 2, E.HATAF_BAR_MIN_DROP) if centre_bar else 0   # fix-bi: never above the dot
        sh = at(sheva, vb[2] + E.HATAF_GAP - sb[0], -dy)
        return top_at(union_recs(v, sh))

    # hataf segol: its segol on the sheva's rows, so the two lower dots align
    hseg = hataf(top_at(dots((-(dw + E.HSEG_GH) / 2, 0), ((dw + E.HSEG_GH) / 2, 0), (0, -row))))
    hpat = hataf(top_at(rect(-E.HATAF_BAR_W / 2, -E.BAR, E.HATAF_BAR_W / 2, 0)), True)
    hqam = hataf(qamats(E.HATAF_BAR_W, E.QAM_H), True)
    mw, mh = E.METEG
    meteg = rect(-mw / 2, -mh, mw / 2, 0)
    rafe = rect(-E.BAR_W / 2, 0, E.BAR_W / 2, E.BAR)
    above = bottom_at(dot)
    dagesh = dot
    if E.DOT_H is not None:              # bold-sheva-r2: the dagesh keeps JBM's own proportions, DAGESH_W wide
        hold = E.DOT_W, E.DOT_H
        E.DOT_W, E.DOT_H = E.DAGESH_W, None
        dagesh = jbm_dot()
        E.DOT_W, E.DOT_H = hold
    elif E.DAGESH_W != E.DOT_W:          # Bold: a dagesh sits inside a counter, which the bold strokes narrow
        kd = E.DAGESH_W / dw
        dagesh = map_rec(dot, lambda x, y: (x * kd, y * kd))
    out = {
        0x05B0: sheva, 0x05B1: hseg, 0x05B2: hpat, 0x05B3: hqam, 0x05B4: hiriq, 0x05B5: tsere,
        0x05B6: segol, 0x05B7: patah, 0x05B8: qam, 0x05B9: above, 0x05BA: above, 0x05BB: qub,
        0x05BC: dagesh, 0x05BD: meteg, 0x05BF: rafe, 0x05C1: above, 0x05C2: above, 0x05C4: above,
        0x05C5: hiriq, 0x05C7: qq,
    }
    # round every outline the way the font will store it, so anchors, heights
    # and gaps are computed on the compiled shape (integer points)
    return {u: compiled(r) for u, r in out.items()}


def compiled(rec):
    g = to_glyph(rec)
    r = RecordingPen()
    g.draw(r, None)
    return r.value


# ---------------------------------------------------------------- anchors

def samples(rec):
    """Outline points plus interior points of a mark (local frame)."""
    P = polys(rec)
    dense = []
    for poly in P:
        for i in range(len(poly)):
            (x0, y0), (x1, y1) = poly[i], poly[(i + 1) % len(poly)]
            n = max(1, int(math.hypot(x1 - x0, y1 - y0) / 3))
            dense += [(x0 + (x1 - x0) * t / n, y0 + (y1 - y0) * t / n) for t in range(n)]
    g = FD.raster(P)
    iy, ix = np.nonzero(g[::3, ::3])
    inner = np.stack([FD.X0 + (ix * 3 + 0.5) * FD.RES, FD.Y0 + (iy * 3 + 0.5) * FD.RES], 1)
    return np.vstack([np.array(dense), inner])


def clear_at(fld, S, x, y):
    return float(FD.sample(fld, S + (x, y)).min()) - FD.RES / 2


def bbox_at(S, x, y):
    return S[:, 0].min() + x, S[:, 1].min() + y, S[:, 0].max() + x, S[:, 1].max() + y


def scan_x(fld, S, y, natural, lo=6, hi=594, target=None):
    """Round 2's search, kept for the descending letters only: the anchor x
    nearest `natural` whose clearance reaches target with the ink inside
    [lo, hi]; else the in-cell maximum; else overflow."""
    target = target or E.TARGET
    xs = np.arange(-200, 801, 2.0)
    rows = []
    for x in xs:
        b = bbox_at(S, x, y)
        c = clear_at(fld, S, x, y)
        rows.append((x, c, b))
    ok = [(abs(x - natural), x, c) for x, c, b in rows if c >= target and b[0] >= lo and b[2] <= hi]
    if ok:
        _, x, c = min(ok)
        return x, c, None
    inc = [(c, -abs(x - natural), x) for x, c, b in rows if b[0] >= lo and b[2] <= hi]
    if inc:
        c, _, x = max(inc)
        if c >= E.GRID_MIN + 2:
            return x, c, 'in-cell max'
    ov = [(abs(x - natural), x, c) for x, c, b in rows if c >= target and b[0] >= -60 and b[2] <= 660]
    if ov:
        _, x, c = min(ov)
        return x, c, 'overflow'
    ov = [(abs(x - natural), x, c) for x, c, b in rows if c >= target and b[0] >= -200 and b[2] <= 800]
    if ov:
        _, x, c = min(ov)
        return x, c, 'overflow wide'
    ov = [(c, x) for x, c, b in rows if b[0] >= -200 and b[2] <= 800]
    c, x = max(ov)
    return x, c, 'overflow max'


def top_candidates(h, lowest=None, two_row=False):
    """Tops for a below point of height h: round 2's -36, then 2 and 4 lower;
    none may take the ink below `lowest` (LOWEST unless the letter dips), and
    a point too tall for -36 rises."""
    tmin = math.ceil((E.LOWEST if lowest is None else lowest) + h - 1e-6)
    if lowest is not None and E.DIP_FLOOR and two_row:   # sheva and the hatafs (round 4's 'h > 245': 249)
        return [tmin]                 # round 4: a tall point under a dipping letter sits on `lowest`
    c = [E.BELOW_TOP - d for d in E.DROP if E.BELOW_TOP - d >= tmin]
    return c or [tmin]


def pick_top(fld, S, x, h, lowest=None, two_row=False):
    """Item 1: x is fixed; the first drop reaching WANT, else ACCEPT, else the best."""
    tried = [(y, clear_at(fld, S, x, y)) for y in top_candidates(h, lowest, two_row)]
    for need in (E.WANT, E.ACCEPT):
        for y, c in tried:
            if c >= need:
                return y, c, None
    y, c = max(tried, key=lambda t: t[1])
    return y, c, 'below %d (listed)' % E.ACCEPT


DESC = (0x05E7, 0x05DA, 0x05DF, 0x05E3, 0x05E5)    # ק ך ן ף ץ
NEIGHBOUR_SHIN = {}                                  # filled by anchors(): shin's shin-dot anchor


def anchors(letters, marks):
    S = {u: samples(r) for u, r in marks.items()}
    MB = {u: rec_bounds(r) for u, r in marks.items()}
    hw = {u: (MB[u][2] - MB[u][0]) / 2 for u in MB}
    H = {u: MB[u][3] - MB[u][1] for u in MB}
    mw = MB[0x05BD][2] - MB[0x05BD][0]
    base_a, report, fields = {}, {}, {}
    for u, rec in letters.items():
        P = polys(rec)
        fld, ink = FD.distance(P)
        fields[u] = fld
        x0, y0, x1, y1 = rec_bounds(rec)
        cx = (x0 + x1) / 2
        st = runs_at_y(P, 300)
        d, rep = {}, {}
        # ---- below points
        if u not in DESC:
            nat = cx
            if u in (0x05D5, 0x05D6):                # vav, zayin: under the stem, read from this outline
                nat = (st[-1][0] + st[-1][1]) / 2
            for m in E.BELOW:
                low = E.LOWEST_DIP if u in E.DIP else (E.LOWEST_TWO if (m in E.TWO_ROW and E.LOWEST_TWO != E.LOWEST) else None)   # fix-bi
                y, c, fl = pick_top(fld, S[m], nat, H[m], low, m in E.TWO_ROW)
                d[m] = (nat, y); rep[m] = (round(c, 1), fl)
        elif u == 0x05DA:                             # final kaf: inside, centred on mid-height
            for m in E.BELOW:
                y = 300 + H[m] / 2
                x, c, fl = scan_x(fld, S[m], y, 255)
                d[m] = (x, y); rep[m] = (round(c, 1), fl)
        else:                                         # ק ן ף ץ: beside the descender
            leg = runs_at_y(P, -100)                  # the descender's run below the baseline
            for m in E.BELOW:
                for y in top_candidates(H[m]):
                    x, c, fl = scan_x(fld, S[m], y, cx)
                    if fl is None:
                        break
                if fl and fl.startswith('overflow'):
                    # no in-cell place: the least overflow that still clears ACCEPT
                    best = None
                    for xx in np.arange(-200, 801, 2.0):
                        b = bbox_at(S[m], xx, y)
                        cc = clear_at(fld, S[m], xx, y)
                        if cc >= E.ACCEPT:
                            over = max(0, -b[0]) + max(0, b[2] - 600)
                            if best is None or (over, abs(xx - cx)) < best[:2]:
                                best = (over, abs(xx - cx), xx, cc)
                    if best:
                        x, c, fl = best[2], best[3], 'overflow %d (least)' % best[0]
                if u == 0x05E7 and m in E.METEG_VOWELS and E.QOF_ROOM:
                    # qof: leave the meteg room between the leg and the vowel
                    need = leg[0][1] + E.QOF_LEG_GAP + mw + E.METEG_GAP + hw[m]
                    if need + hw[m] <= 600 and need > x:
                        c2 = clear_at(fld, S[m], need, y)
                        if c2 >= E.ACCEPT:
                            x, c, fl = need, c2, None
                d[m] = (x, y); rep[m] = (round(c, 1), fl)
        # ---- above points
        top = E.ABOVE_BOT
        if u == 0x05DC:                               # lamed: placed after the loop (needs shin)
            mast = runs_at_y(P, 700)[0]
            x, c, fl = scan_x(fld, S[0x05BF], top, mast[1] + E.TARGET + 2 + hw[0x05BF])
            d[0x05BF] = (x, top); rep[0x05BF] = (round(c, 1), fl)
        else:
            nat_h = x0 + 10
            if u == 0x05D5:
                nat_h = (x0 + x1) / 2 - 10            # holam male: over the vav
            x, c, fl = scan_x(fld, S[0x05B9], top, nat_h)
            d[0x05B9] = (x, top); rep[0x05B9] = (round(c, 1), fl)
            nat_hh = x0 - 50 if u == 0x05D5 else nat_h
            x, c, fl = scan_x(fld, S[0x05BA], top, nat_hh)
            d[0x05BA] = (x, top); rep[0x05BA] = (round(c, 1), fl)
            x, c, fl = scan_x(fld, S[0x05C2], top, x0 + 46)
            d[0x05C2] = (x, top); rep[0x05C2] = (round(c, 1), fl)
            x, c, fl = scan_x(fld, S[0x05BF], top, cx)
            d[0x05BF] = (x, top); rep[0x05BF] = (round(c, 1), fl)
        x, c, fl = scan_x(fld, S[0x05C1], top, x1 - 46)
        d[0x05C1] = (x, top); rep[0x05C1] = (round(c, 1), fl)
        x, c, fl = scan_x(fld, S[0x05C4], top, cx)
        d[0x05C4] = (x, top); rep[0x05C4] = (round(c, 1), fl)
        if u == 0x05E9:                               # shin: holam and sin dot are one dot
            d[0x05B9] = d[0x05BA] = d[0x05C2]
            rep[0x05B9] = rep[0x05BA] = rep[0x05C2]
            NEIGHBOUR_SHIN['dot'] = d[0x05C1]
        # ---- dagesh (round 2's)
        Sd = S[0x05BC]
        if u in (0x05D5, 0x05D6, 0x05DF):
            sx = st[-1][0]
            p = (sx - E.TARGET - 4 - hw[0x05BC], 300)
            c, fl = clear_at(fld, Sd, *p), 'left of stem'
        elif u == 0x05D9:
            sx = runs_at_y(P, 420)[-1][0]
            p = (sx - E.TARGET - 4 - hw[0x05BC], 400)
            c, fl = clear_at(fld, Sd, *p), 'left of tail'
        else:
            box = E.DAGESH_BOX[u]
            pref = MAPPED.get(u, {}).get('dagesh') or ((box[0] + box[2]) / 2, (box[1] + box[3]) / 2)
            best = (-1e9, None, None)
            for yy in np.arange(box[1], box[3] + 0.1, 4):
                for xx in np.arange(box[0], box[2] + 0.1, 4):
                    if FD.sample(fld, np.array([[xx, yy]]))[0] <= 0:
                        continue
                    cc = clear_at(fld, Sd, xx, yy)
                    s = min(cc, E.DAGESH_CAP) - 0.02 * math.hypot(xx - pref[0], yy - pref[1])
                    if s > best[0]:
                        best = (s, (float(xx), float(yy)), cc)
            p, c, fl = best[1], best[2], 'box'
            need = E.DAGESH_NEED.get(u, E.GRID_MIN + 1)
            if E.DAGESH_WIDEN and c < need:
                # fix-bi (Bold): round 2's box was drawn on the Regular outline. Where it holds no place with
                # the clearance needed, search the letter's own interior (bbox inset 30), same score: first
                # with the whole dagesh inside the letter's convex hull (it reads as the letter's); only if
                # nothing there reaches `need`, anywhere in the inset box, so the dot is seen, never lost in ink
                from scipy.spatial import ConvexHull
                hull = ConvexHull(np.vstack([np.array(q) for q in P])).equations
                db = (Sd[:, 0].min(), Sd[:, 1].min(), Sd[:, 0].max(), Sd[:, 1].max())
                corners = np.array([[db[0], 0], [db[2], 0], [0, db[1]], [0, db[3]], [db[0] * .7, db[1] * .7],
                                    [db[2] * .7, db[3] * .7], [db[0] * .7, db[3] * .7], [db[2] * .7, db[1] * .7]])
                wb = (x0 + 30, max(y0, 0) + 30, x1 - 30, min(y1, 630) - 30)
                for inside in (True, False):
                    wbest = (-1e9, None, -1e9)
                    for yy in np.arange(wb[1], wb[3] + 0.1, 4):
                        for xx in np.arange(wb[0], wb[2] + 0.1, 4):
                            if FD.sample(fld, np.array([[xx, yy]]))[0] <= 0:
                                continue
                            if inside and ((corners + (xx, yy)) @ hull[:, :2].T + hull[:, 2]).max() > 0:
                                continue
                            cc = clear_at(fld, Sd, xx, yy)
                            s = min(cc, E.DAGESH_CAP) - 0.02 * math.hypot(xx - pref[0], yy - pref[1])
                            if s > wbest[0]:
                                wbest = (s, (float(xx), float(yy)), cc)
                    if wbest[2] >= need:
                        p, c, fl = wbest[1], wbest[2], 'widened, %s' % ('inside the letter' if inside else 'beside the letter (no room inside)')
                        break
        d[0x05BC] = p; rep[0x05BC] = (round(c, 1), fl)
        # ---- meteg on its own (no below vowel before it: shuruk, holam, a bare letter):
        # under the letter where a hiriq would be; under vav, under the shuruk's dot
        mx = d[0x05BC][0] if u == 0x05D5 else d[0x05B4][0]
        my = E.BELOW_TOP
        if u == 0x05DA:                               # final kaf: below the letter, not inside it
            mx = 255
        c = clear_at(fld, S[0x05BD], mx, my)
        d[0x05BD] = (mx, my); rep[0x05BD] = (round(c, 1), None if c >= E.ACCEPT else 'below %d' % E.ACCEPT)
        base_a[u] = d
        report[u] = rep

    # ---- lamed's holam, holam haser and (meaningless on lamed) sin dot
    u = 0x05DC
    P = polys(letters[u]); fld = fields[u]
    d, rep = base_a[u], report[u]
    if E.LAMED == 'L1':
        mast = runs_at_y(P, 700)[0]
        lx = mast[0] - E.L1_MAST_GAP - hw[0x05B9]
        # the shin dot of a shin to lamed's left, in lamed's frame
        sx, sy = NEIGHBOUR_SHIN['dot']
        nb = at(marks[0x05C1], sx - 600, sy)
        y = E.ABOVE_BOT
        while True:
            hol = at(marks[0x05B9], lx, y)
            g = exact_gap(hol, nb)
            if g >= E.L1_NEIGHBOUR and clear_at(fld, S[0x05B9], lx, y) >= E.TARGET:
                break
            y += 2
            if y > 940:
                raise SystemExit('L1: no height clears a left shin dot by %d' % E.L1_NEIGHBOUR)
        for m in (0x05B9, 0x05BA, 0x05C2):
            d[m] = (lx, y)
            rep[m] = (round(clear_at(fld, S[m], lx, y), 1), 'L1: left of the mast, raised to %d; %.1f from a left shin dot' % (y, g))
    else:
        lx, y = E.L2_ANCHOR
        for m in (0x05B9, 0x05BA, 0x05C2):
            d[m] = (lx, y)
            rep[m] = (round(clear_at(fld, S[m], lx, y), 1), 'L2: above, in the cell')
    for u in base_a:
        base_a[u] = {m: (round(v[0]), round(v[1])) for m, v in base_a[u].items()}
    return base_a, report


def exact_gap(rec_a, rec_b):
    """Exact distance between two placed outlines (flattened, vertex to segment both ways)."""
    from geom import flatten, segs, pt_seg_min
    pa, pb = rec_to_path(rec_a), rec_to_path(rec_b)
    inter = pathops.op(pa, pb, pathops.PathOp.INTERSECTION)
    if abs(inter.area) > 0.5:
        return -math.sqrt(abs(inter.area))
    A, B = flatten(pa), flatten(pb)
    PA, PB = np.concatenate(A), np.concatenate(B)
    return min(pt_seg_min(PA, segs(B)), pt_seg_min(PB, segs(A)))


# ---------------------------------------------------------------- the italics: back into the cell

FIT_LOG = []


def fit_italic(letters, marks, up_a, base_a, report):
    """The shear moves every point sideways with its height (below points ~73 units one way at
    y -150, above points ~67 the other way at y 740), so some leave the cell they stayed in upright.
    For each (letter, point) whose sheared ink overflows the cell by more than the upright did, the
    letter's anchor moves along x: among the x whose clearance to the SHEARED letter (distance field,
    as anchors() measures) reaches ACCEPT, the one with the least overflow beyond the upright's, then
    nearest the sheared x. If no x reaches ACCEPT the sheared x is kept (listed). Attachment is
    untouched: only the letter's anchor moves."""
    lin = lambda x, y: (x + ST.SLANT * ST.TAN * y, y)       # the shear's linear part: a point's frame about its anchor
    S_up = {m: rec_bounds(r) for m, r in marks.items()}
    S_it = {m: rec_bounds(map_rec(r, lin)) for m, r in marks.items()}
    samp, fields = {}, {}
    for u, d in base_a.items():
        for m, (ax, ay) in list(d.items()):
            ux, uy = up_a[u][m]
            allow_l, allow_r = max(0, -(ux + S_up[m][0])), max(0, ux + S_up[m][2] - 600)

            def excess(x):
                return max(0, -(x + S_it[m][0]) - allow_l) + max(0, x + S_it[m][2] - 600 - allow_r)
            if excess(ax) <= 0:
                continue
            if u not in fields:
                fields[u] = FD.distance(polys(italic_outline(letters[u])))[0]
            if m not in samp:
                samp[m] = samples(map_rec(marks[m], lin))
            fld = fields[u]
            c0 = clear_at(fld, samp[m], ax, ay)
            cand = []
            for x in np.arange(ax - 300, ax + 301, 2.0):
                c = clear_at(fld, samp[m], x, ay)
                if c >= E.ACCEPT:
                    cand.append((excess(x), abs(x - ax), x, c))
            if cand:
                ex, _, nx, c1 = min(cand)
                how = 'fitted' if ex <= 0 else 'fitted, %d beyond the upright overflow' % round(ex)
            else:
                nx, c1, how = ax, c0, 'kept: no x clears %d' % E.ACCEPT
            d[m] = (round(nx), ay)
            FIT_LOG.append((chr(u), hex(m), ax, round(nx), round(c0, 1), round(c1, 1), how))
            report[u][m] = (round(c1, 1), how + ' (italic)')


METEG_LOG = []


def fit_meteg(letters, marks, up_a, base_a, report, home):
    """bold-sheva-r2 round 3 (METEG_FIT=1, right lean): fit_italic for the vowel together with the meteg it carries.
    Meteg rides on its vowel by mark-to-mark, left of its ink by METEG_GAP with tops aligned; the shear keeps that
    relation (both anchors go through the same map) but carries the pair ~70 units left at below-point height, so in
    the right lean meteg leaves the cell where the upright's did not. baud's GPOS has mark attachment only (types 4,
    6, 9) and GSUB must stay JBM's, so the vowel cannot move only when a meteg follows: where the vowel + meteg
    group overflows the cell by more than the upright group did, the LETTER's anchor for that vowel moves along x,
    exactly as fit_italic moves it (clearance to the sheared letter >= ACCEPT, least excess, then nearest). The meteg's
    mark-to-mark anchors are untouched, so its relation to the vowel stays the upright one sheared."""
    lin = lambda x, y: (x + ST.SLANT * ST.TAN * y, y)
    MB = {m: rec_bounds(r) for m, r in marks.items()}
    mw, mh = E.METEG
    met = marks[0x05BD]
    samp, fields = {}, {}
    for u, d in base_a.items():
        for v in E.METEG_VOWELS:
            if v not in d:
                continue
            raised = max(0, math.ceil((E.LOWEST_TWO if v in E.TWO_ROW else E.LOWEST) + (MB[v][3] - MB[v][1]) - 1e-6) - E.BELOW_TOP)
            dvec = (MB[v][0] - E.METEG_GAP - mw / 2, MB[v][3] - raised)
            g_up = rec_bounds(union_recs(marks[v], at(met, *dvec)))
            g_it = rec_bounds(union_recs(map_rec(marks[v], lin), map_rec(at(met, *dvec), lin)))
            ax, ay = d[v]
            ux, uy = up_a[u][v]
            allow_l, allow_r = max(0, -(ux + g_up[0])), max(0, ux + g_up[2] - 600)

            def excess(x):
                return max(0, -(x + g_it[0]) - allow_l) + max(0, x + g_it[2] - 600 - allow_r)
            v_it = rec_bounds(map_rec(marks[v], lin))

            def own_out(x):   # the vowel alone, past the cell's edges: never more than before this fit (clear.py's grid)
                return max(0, -(x + v_it[0])) + max(0, x + v_it[2] - 600)
            if excess(ax) <= 0:
                continue
            if u not in fields:
                fields[u] = FD.distance(polys(italic_outline(letters[u])))[0]
            if v not in samp:
                samp[v] = samples(map_rec(marks[v], lin))
            fld = fields[u]
            c0 = clear_at(fld, samp[v], ax, ay)
            cand = []
            # never further right than undoing the shear at the vowel's own height: the italic vowel does not pass its
            # upright place, and no point jumps across a descender
            cap = ST.TAN * (ST.SHEAR_Y0 - (ay + (MB[v][1] + MB[v][3]) / 2))
            for x in np.arange(ax, ax + cap + 1e-9, 2.0):
                if own_out(x) > own_out(ax) + 1e-9:
                    continue
                c = clear_at(fld, samp[v], x, ay)
                if c >= max(E.ACCEPT, c0 - 1e-6):   # never closer to its letter than before this fit
                    cand.append((excess(x), abs(x - ax), x, c))
            if not cand:
                METEG_LOG.append((chr(u), hex(v), ax, ax, round(c0, 1), round(c0, 1), 'kept: no x clears %d' % E.ACCEPT))
                continue
            ex, _, nx, c1 = min(cand)
            d[v] = (round(nx), ay)
            how = 'meteg-fitted' if ex <= 0 else 'meteg-fitted, %d beyond the upright group' % round(ex)
            METEG_LOG.append((chr(u), hex(v), ax, round(nx), round(c0, 1), round(c1, 1), how))
            report[u][v] = (round(c1, 1), how + ' (italic)')
    print('meteg cell fit (right lean): %d vowel anchors moved' % len(METEG_LOG))
    for row in METEG_LOG:
        print('    %s %s %s %s %s %s %s' % row)


# ---------------------------------------------------------------- homes (item 5)

def homes(marks):
    """Where each point's ink sits when nothing anchors it, as the y of its
    local origin (a below point's top, an above point's bottom, the dagesh's
    centre). That y is also the mark's anchor, so attached placement is the
    same whatever the home."""
    out = {}
    for m, r in marks.items():
        b = rec_bounds(r)
        if m in E.ABOVE_DOTS or m == 0x05BF:
            out[m] = E.HOME_ABOVE_BOTTOM
        elif m == 0x05BC:
            out[m] = round(E.HOME_BELOW_TOP - (b[3] - b[1]) / 2)
        else:                                             # below points and meteg
            h = b[3] - b[1]
            floor = E.LOWEST_TWO if m in E.TWO_ROW else E.LOWEST   # fix-bi: Regular: the same -275
            out[m] = E.MULTI_ROW_HOME.get(m, max(E.HOME_BELOW_TOP, math.ceil(floor + h - 1e-6)))   # fix-bi: Bold segol, qubuts
    return out


# ---------------------------------------------------------------- build

def with_script_copy(data, tag=b'hebr', like=b'DFLT'):
    """A GSUB or GPOS table's bytes with one ScriptRecord added: `tag`, naming
    `like`'s Script table, so its default LangSys, its required feature and its
    feature indices are `like`'s exactly.

    Why hebr: Microsoft's 'Developing OpenType Fonts for Hebrew Script'
    registers it, and HarfBuzz's Hebrew shaper applies GPOS only when the
    script it chose is hebr (note AB 7.3, hb-ot-shaper-hebrew.cc); under DFLT
    alone it places the points by combining class and the anchors go unused.
    baud selects latn first (baud-font/src/layout.rs), so baud is unaffected.

    Why on the bytes: a deep copy of DFLT's Script compiles to exactly this --
    fontTools' writer shares the identical table, and both records name one
    offset (measured on Regular's GSUB: 0x04b2 twice) -- but decompiling GSUB and
    compiling it again re-lays all of its 25 KB (same length, another order;
    measured), and GSUB is JetBrains Mono's, byte for byte. So the record goes
    in at its place in tag order (OpenType: ScriptRecords sorted by tag), the
    offsets that cross it move 6, and every byte after the record array is the
    input's, in the same order (asserted)."""
    major, minor, sl, fl, ll = struct.unpack_from('>5H', data)
    if (major, minor) != (1, 0) or sl != 10:
        raise SystemExit('with_script_copy: version %d.%d with its ScriptList at %d; want 1.0 at 10' % (major, minor, sl))
    n, = struct.unpack_from('>H', data, sl)
    recs = [(data[sl + 2 + 6 * i:sl + 6 + 6 * i], struct.unpack_from('>H', data, sl + 6 + 6 * i)[0]) for i in range(n)]
    tags = [t for t, _ in recs]
    if tags != sorted(tags) or tag in tags or like not in tags:
        raise SystemExit('with_script_copy: script records %s' % tags)
    end = sl + 2 + 6 * n
    # An offset is unsigned, so every table lies after the table that names it.
    # When the FeatureList, the LookupList and every Script table lie at or past
    # the record array's end, nothing but the header and the ScriptList's count
    # and records lies before it, and no offset but these crosses the 6 bytes.
    if min([fl, ll] + [sl + o for _, o in recs]) < end:
        raise SystemExit('with_script_copy: a table lies inside the ScriptList records')
    recs.insert(sum(t < tag for t in tags), (tag, dict(recs)[like]))
    if max([fl, ll] + [o for _, o in recs]) + 6 > 0xFFFF:
        raise SystemExit('with_script_copy: an offset would overflow')
    out = (struct.pack('>5H', major, minor, sl, fl + 6, ll + 6) + struct.pack('>H', n + 1)
           + b''.join(t + struct.pack('>H', o + 6) for t, o in recs) + data[end:])
    if out[end + 6:] != data[end:]:
        raise SystemExit('with_script_copy: the bytes after the records moved')
    return out


# nameID 7, the trademark notices, each read from its own font and required to
# be this text byte for byte (OFL-FAQ 1.1-update7 Q3.7: "Any trademark notices
# must remain in any derivative fonts").
TM_JBM = 'JetBrains Mono is a trademark of JetBrains s.r.o.'
TM_CAS = 'Cascadia Code is a trademark of the Microsoft group of companies.'


def trademark(font, want):
    """`font`'s nameID 7, required to be `want` in one Windows-English record."""
    got = [(r.platformID, r.platEncID, r.langID, r.toUnicode()) for r in font['name'].names if r.nameID == 7]
    if got != [(3, 1, 0x409, want)]:
        raise SystemExit('nameID 7 is %r; want only %r at 3/1/0x409' % (got, want))
    return want


def build(out, letter_hook=None, tag=''):
    jbm = TTFont(JBM)
    order = jbm.getGlyphOrder()
    glyf = jbm['glyf']
    hmtx = jbm['hmtx']
    new = {}
    letters = {}
    for u in D.LETTERS:
        rec = letter_rec(u)
        if letter_hook:
            rec = letter_hook(u, rec)
        letters[u] = rec
        new['heb_%04X' % u] = (rec, 600, u, 1)
    for u in (0x05BE, 0x05F3, 0x05F4):
        new['heb_%04X' % u] = (letter_rec_r1(u), 600, u, 1)
    new['heb_05C0'] = (paseq_rec(), 600, 0x05C0, 1)
    # sof pasuq: the upright weight's colon (for the italics, sheared with the letters)
    jup = TTFont(JBM_UP)
    jgs = jup.getGlyphSet()
    r = DecomposingRecordingPen(jgs)
    jgs['colon'].draw(r)
    new['heb_05C3'] = (r.value, 600, 0x05C3, 1)
    marks = make_marks()
    base_a, report = anchors(letters, marks)
    home = homes(marks)
    # ---- the italics: one shear for every outline and every anchor (after the
    # upright anchor search), plus, per point, an x shift `delta` that moves its
    # outline and all its anchors together, so attachment is unchanged and the
    # unattached point lands on JBM Italic's own mark axis (U+0323 for the
    # below points, meteg and dagesh; U+0307 for the above points and rafe).
    delta = {m: 0.0 for m in marks}
    if ST.ITALIC:
        jt = TTFont(JBM)
        tgs = jt.getGlyphSet()
        axis = {}
        for key, gname in (('below', 'dotbelowcomb'), ('above', 'uni0307')):
            bp = BoundsPen(tgs)
            tgs[gname].draw(bp)
            b = bp.bounds
            axis[key] = ((b[0] + b[2]) / 2, (b[1] + b[3]) / 2)
        for m, rec in marks.items():
            b = rec_bounds(rec)
            yc = home[m] + (b[1] + b[3]) / 2          # the unattached ink centre's height
            xc = E.MARK_SHIFT + (b[0] + b[2]) / 2     # -300: every point is centred on its anchor x
            cx, cy = axis['above' if (m in E.ABOVE_DOTS or m == 0x05BF) else 'below']
            want = cx + ST.TAN * (yc - cy)            # JBM Italic's mark axis (its marks lean right)
            delta[m] = want - ST.shear_xy(xc, yc)[0]
        # the letters and the marks of punctuation drawn as letters take the rounds' correction;
        # sof pasuq is JetBrains Mono's colon, whose italic dots are its upright ones sheared to
        # within 2.2 units (optical.py), so it takes the plain shear, as the points do
        for name, (rec, adv, u, cls) in list(new.items()):
            sheared = map_rec(rec, ST.shear_xy) if u == 0x05C3 else italic_outline(rec, chr(u))
            new[name] = (sheared, adv, u, cls)
        up_a = base_a
        base_a = {u: {m: tuple(round(c) for c in ST.shear_xy(*xy)) for m, xy in d.items()} for u, d in base_a.items()}
        fit_italic(letters, marks, up_a, base_a, report)
        if os.environ.get('METEG_FIT') == '1' and ST.SLANT == 1:   # bold-sheva-r2 round 3: the right lean only
            fit_meteg(letters, marks, up_a, base_a, report, home)

    def mark_xy(m, x, y):
        """A point of mark m's own frame (its outline or one of its anchors), placed."""
        sx, sy = ST.shear_xy(x, y) if ST.ITALIC else (x, y)
        return (sx + delta[m], sy)

    for u, rec in marks.items():
        new['heb_%04X' % u] = (map_rec(at(rec, E.MARK_SHIFT, home[u]), lambda x, y, u=u: mark_xy(u, x, y)), 0, u, 3)

    # cover.py: appended after every glyph above, so no glyph ID moves; an upright outline takes
    # the italics' shear and correction as the letters do
    for name, rec, adv, u, cls in CV.glyphs(sys.modules[__name__], ST):
        new[name] = (italic_outline(rec, chr(u)) if ST.ITALIC else rec, adv, u, cls)

    for name, (rec, adv, u, cls) in new.items():
        g = to_glyph(rec)
        glyf.glyphs[name] = g
        order.append(name)
        g.recalcBounds(glyf)
        hmtx.metrics[name] = (adv, g.xMin if g.numberOfContours else 0)
    jbm.setGlyphOrder(order)
    glyf.glyphOrder = order
    for t in jbm['cmap'].tables:
        if t.isUnicode():
            for name, (rec, adv, u, cls) in new.items():
                t.cmap[u] = name
    gdef = jbm['GDEF'].table
    for name, (rec, adv, u, cls) in new.items():
        gdef.GlyphClassDef.classDefs[name] = cls
    gmap = {n: i for i, n in enumerate(order)}

    # lookup 5 (mark): one mark class per point; every letter x point has its own anchor
    cls_ids = {m: i for i, m in enumerate(sorted(marks))}
    mk = {('heb_%04X' % m): (cls_ids[m], otl.buildAnchor(*[round(c) for c in mark_xy(m, E.MARK_SHIFT, home[m])])) for m in marks}
    bs = {('heb_%04X' % u): {cls_ids[m]: otl.buildAnchor(*xy) for m, xy in d.items()} for u, d in base_a.items()}
    lk = ot.Lookup()
    lk.LookupType = 4
    lk.LookupFlag = 0
    lk.SubTable = otl.buildMarkBasePos(mk, bs, gmap)
    lk.SubTableCount = len(lk.SubTable)

    # lookup 6 (mkmk): meteg on the vowel before it, left of its ink by METEG_GAP,
    # tops aligned. Its mark filtering set holds meteg and the below vowels only,
    # so a dagesh (ccc 21, between a vowel and meteg's 22) is stepped over.
    MB = {m: rec_bounds(marks[m]) for m in marks}
    mw = MB[0x05BD][2] - MB[0x05BD][0]
    m1 = {'heb_05BD': (0, otl.buildAnchor(*[round(c) for c in mark_xy(0x05BD, E.MARK_SHIFT, home[0x05BD])]))}
    m2 = {}
    for v in E.METEG_VOWELS:
        ax = E.MARK_SHIFT + MB[v][0] - E.METEG_GAP - mw / 2
        # tops aligned -- except beside a point the -275 rule raised above
        # BELOW_TOP (sheva, the hatafs): there meteg keeps BELOW_TOP's height
        raised = max(0, math.ceil((E.LOWEST_TWO if v in E.TWO_ROW else E.LOWEST) + (MB[v][3] - MB[v][1]) - 1e-6) - E.BELOW_TOP)   # fix-bi
        ay = home[v] + MB[v][3] - raised
        m2['heb_%04X' % v] = {0: otl.buildAnchor(*[round(c) for c in mark_xy(v, ax, ay)])}
    mm = ot.Lookup()
    mm.LookupType = 6
    mm.LookupFlag = 0x0010                     # UseMarkFilteringSet
    st = ot.MarkMarkPos()
    st.Format = 1
    st.ClassCount = 1
    st.Mark1Coverage = otl.buildCoverage(m1, gmap)
    st.Mark2Coverage = otl.buildCoverage(m2, gmap)
    st.Mark1Array = otl.buildMarkArray(m1, gmap)
    st.Mark2Array = ot.Mark2Array()
    st.Mark2Array.Mark2Count = len(st.Mark2Coverage.glyphs)
    st.Mark2Array.Mark2Record = [otl.buildMark2Record([m2[g][0]]) for g in st.Mark2Coverage.glyphs]
    mm.SubTable = [st]
    mm.SubTableCount = len(mm.SubTable)
    msd = gdef.MarkGlyphSetsDef
    set_glyphs = ['heb_05BD'] + ['heb_%04X' % v for v in E.METEG_VOWELS]
    msd.Coverage.append(otl.buildCoverage(set_glyphs, gmap))
    msd.MarkSetCount = len(msd.Coverage)
    mm.MarkFilteringSet = msd.MarkSetCount - 1

    gpos = jbm['GPOS'].table
    gpos.LookupList.Lookup += [lk, mm]
    gpos.LookupList.LookupCount = len(gpos.LookupList.Lookup)
    i_mark, i_mkmk = gpos.LookupList.LookupCount - 2, gpos.LookupList.LookupCount - 1
    seen = set()
    for fr in gpos.FeatureList.FeatureRecord:
        if fr.FeatureTag == 'mark':
            fr.Feature.LookupListIndex.append(i_mark); seen.add('mark')
        elif fr.FeatureTag == 'mkmk':
            fr.Feature.LookupListIndex.append(i_mkmk); seen.add('mkmk')
        fr.Feature.LookupCount = len(fr.Feature.LookupListIndex)
    if seen != {'mark', 'mkmk'}:
        raise SystemExit('mark/mkmk feature missing: %s' % seen)

    # the hebr script records (with_script_copy): GSUB is still JetBrains Mono's
    # own bytes here, and GPOS is compiled under the save's pin; both are then
    # written as these bytes.
    for t in ('GSUB', 'GPOS'):
        spliced = DefaultTable(t)
        spliced.data = with_script_copy(table_pinned(jbm, t))
        jbm[t] = spliced

    os2 = jbm['OS/2']
    os2.ulUnicodeRange1 |= 1 << 11
    os2.ulCodePageRange1 |= 1 << 5

    fam = E.FAMILY
    nm = jbm['name']
    sub = {'Regular': 'Regular', 'Bold': 'Bold', 'Italic': 'Italic', 'BoldItalic': 'Bold Italic'}[ST.STYLE]
    ps = fam.replace(' ', '') + '-' + ST.STYLE
    uid = ps + ';prototype-r3' + tag if ST.STYLE == 'Regular' else ps + ';' + os.environ.get('UID_TAG', 'prototype-bi-fix') + tag + '-' + ST.suffix()   # bold-sheva-r2: UID_TAG
    tm = trademark(jbm, TM_JBM) + ' ' + trademark(cas, TM_CAS)
    for rec in list(nm.names):
        if rec.nameID == 0:
            rec.string = (str(rec.toUnicode()) + ' Hebrew glyphs derived from Cascadia Mono '
                          '(GitHub release v2407.24): Copyright (c) 2019 - Present, Microsoft Corporation, '
                          'with Reserved Font Name Cascadia Code (the source file states: (c) 2021 Microsoft Corporation). '
                          'Modified 2026 (re-weighted, re-fitted, points redrawn) as a prototype.')
        elif rec.nameID in (1, 16):
            rec.string = fam
        elif rec.nameID in (4,):
            rec.string = fam + ' ' + sub
        elif rec.nameID == 6:
            rec.string = ps
        elif rec.nameID == 3:
            rec.string = uid
        elif rec.nameID == 17:
            rec.string = sub
        elif rec.nameID == 7:
            rec.string = tm
    save_pinned(jbm, out)
    json.dump({'bases': {hex(u): {hex(m): v for m, v in d.items()} for u, d in base_a.items()},
               'report': {hex(u): {hex(m): v for m, v in d.items()} for u, d in report.items()},
               'homes': {hex(m): v for m, v in home.items()},
               'mkmk_mark2': {k: [v[0].XCoordinate, v[0].YCoordinate] for k, v in m2.items()},
               'mark_bounds_local': {hex(m): [round(c, 1) for c in rec_bounds(marks[m])] for m in marks},
               'style': ST.suffix(), 'italic_delta': {hex(m): round(v, 2) for m, v in delta.items()},
               'desc_from': {hex(u): v for u, v in DESC_FROM_USED.items()},
               'constants': {k: getattr(E, k) for k in ('DOT_W', 'DAGESH_W', 'BAR', 'METEG', 'PASEQ_W', 'QAM_H', 'QQ_H', 'GV', 'SEG_PITCH', 'QUB_DX', 'QUB_PITCH', 'LOWEST', 'LOWEST_TWO', 'LOWEST_DIP', 'HATAF_BAR_MIN_DROP', 'DAGESH_WIDEN', 'BELOW_TOP', 'HOME_ABOVE_BOTTOM', 'HOME_BELOW_TOP', 'TAV_LEG_DX', 'L2_ANCHOR', 'STEM_V', 'DIP_FLOOR', 'LAMED')} | {'SX': D.SX, 'SY': D.SY, 'EY': D.EY, 'DX': D.DX, 'CAS': CAS}},
              open(out + '.anchors.json', 'w'), indent=0)
    flagged = [(chr(u), hex(m), v) for u, d in report.items() for m, v in d.items()
               if v[1] and v[1] not in ('box', 'left of stem', 'left of tail') and not str(v[1]).startswith(('L1', 'L2', 'widened, inside'))]
    print('saved', out, len(new), 'glyphs; anchors flagged:', len(flagged))
    print('pathops retries (plain area -> checked area):', PATHOPS_RETRIES or 'none')
    if ST.ITALIC:
        print('rounds corrected (ROUND_K %s): %d arcs (letter, kind, sheared angle, handle ratio, largest move in units):' % (ST.ROUND_K, len(OP.CORRECTED)))
        for c in OP.CORRECTED:
            print('   ', *c)
        print('bowl joints levelled (LEVEL %s): %d (letter, outer joint upright, gap to the inner joint, move in units):' % (ST.LEVEL, len(OP.LEVELLED)))
        for c in OP.LEVELLED:
            print('   ', *c)
        print('italic cell fit: %d anchors moved (letter, point, x sheared -> x fitted, clearance before -> after):' % len(FIT_LOG))
        for f in FIT_LOG:
            print('   ', *f)
    for f in flagged:
        print('  ', *f)
    for m in (0x05B9,):
        print('   lamed', E.LAMED, report[0x05DC][m])


if __name__ == '__main__':
    key = os.environ.get('VARIANT', 'A')
    over = N.FINAL[key]()
    mod = sys.modules[__name__]
    hook = (lambda u, r: over[u](mod, r) if u in over else r)
    build(sys.argv[1], hook, '-%s-%s' % (key, E.LAMED))
    print('style', ST.suffix(), '; variant', key, 'replaced', ' '.join(chr(u) for u in over) or 'nothing', '; lamed', E.LAMED)
