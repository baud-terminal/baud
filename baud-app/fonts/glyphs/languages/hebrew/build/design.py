"""Design decisions for the prototype, kept apart from the plumbing."""
import math

FAMILY = 'Proto Hebrew Mono C'
import os
CAS_WGHT = int(os.environ.get('CAS_WGHT', 400))
SX = 90 / 193          # Cascadia 400 vertical stem 193 -> 90
EY = float(os.environ.get('EY', 50))   # erode horizontals (Cascadia units): 400 -> 200-50 = 150
EX = 0
DX = float(os.environ.get('DX', 0))    # fatten verticals (Cascadia units)
VAV_CUT = float(os.environ.get('VAV_CUT', 0))
CORNER = float(os.environ.get('CORNER', 0.35))   # keep this share of a round corner's rise above the bar
SY = 630 / (1200 - EY)  # eroded letter height -> 630
SM = float(os.environ.get('SM', 0.52))   # uniform scale for marks (dots stay round)

LETTERS = list(range(0x05D0, 0x05EB))
# widen by scaling x about the centre and eroding x back by the stem's gain,
# so the stems keep their weight: for letters a region cut would kink
XSCALE = {0x05D0: 1.07, 0x05E1: 1.045}
PUNCT = [0x05BE, 0x05C0, 0x05F3, 0x05F4]
NUDGE = {}

TOP = 630
BELOW_Y = float(os.environ.get('BELOW_Y', -50))     # top of a below mark
ABOVE_Y = 690     # bottom of an above mark


def stretch(rec, cut, dx, axis=0):
    """Move every point beyond `cut` (on axis) by dx: weight-preserving."""
    out = []
    for op, args in rec:
        na = []
        for p in args:
            if p[axis] > cut:
                p = (p[0] + dx, p[1]) if axis == 0 else (p[0], p[1] + dx)
            na.append(p)
        out.append((op, tuple(na)))
    return out


# (cut, dx, side): every point on `side` of x=cut moves by dx -- a
# weight-preserving width change, cut only through horizontal strokes.
EDITS = {
    0x05D1: [(250, +37, '<')],    # bet 455 -> 418
    0x05D3: [(250, +40, '<')],    # dalet 458 -> 418
    0x05DE: [(320, -50, '>')],    # mem 469 -> 419
    0x05EA: [(340, -65, '>')],    # tav 485 -> 420
    0x05DB: [(250, -32, '<')],    # kaf 379 -> 411: wider than nun
    0x05E0: [(200, +15, '<'), (300, +40, '<', 400, 1000)],  # nun 339: shorter base and head than kaf
    0x05D9: [(300, -40, '<', 500, 1000)],                    # yod head 320 -> 360
    # zayin head 336 -> 380; the band starts under the head's underside in both
    # weights (Regular 548, Bold 517), so each end moves whole; the leg and its
    # neck lie between the two x cuts, so they stay
    0x05D6: [(240, -22, '<', 500, 1000), (420, +22, '>', 500, 1000)],
    0x05D2: [(250, -20, '<', 500, 1000), (250, -20, '<', -100, 150)],  # gimel head and foot 348 -> 368
    0x05D5: [(300, +VAV_CUT, '<')],    # vav keeps Cascadia's 293 (0.70 of resh): wider heads read as resh in words
    0x05DF: [(300, +VAV_CUT, '<')],    # final nun, the long vav
}


def edit_letter(u, rec):
    # Cascadia lifts its round top corners 29 units above the bar (659 against
    # 630); JetBrains Mono overshoots its rounds by about 10. Flatten the rise.
    rec = [(op, tuple((x, TOP + (y - TOP) * CORNER) if y > TOP else (x, y) for x, y in args))
           for op, args in rec] if u != 0x05DC else rec
    for e in EDITS.get(u, []):
        cut, dx, side = e[:3]
        ylo, yhi = (e[3], e[4]) if len(e) > 3 else (-1e9, 1e9)
        out = []
        for op, args in rec:
            na = []
            for (x, y) in args:
                if ((side == '<' and x < cut) or (side == '>' and x > cut)) and ylo <= y <= yhi:
                    x += dx
                na.append((x, y))
            out.append((op, tuple(na)))
        rec = out
    return rec


def punct(u, cas_path, path_rec, map_rec):
    import build as B
    return B.letter_rec(u)


# ---------------------------------------------------------------- marks

def _bounds(rec):
    from build import rec_bounds
    return rec_bounds(rec)


def _norm(rec, map_rec, s=SM):
    """Scale uniformly, centre the ink at x=0 with its top at y=0."""
    r = map_rec(rec, lambda x, y: (x * s, y * s))
    b = _bounds(r)
    cx = (b[0] + b[2]) / 2
    return map_rec(r, lambda x, y: (x - cx, y - b[3]))


def _at(rec, map_rec, dx, dy):
    return map_rec(rec, lambda x, y: (x + dx, y + dy))


def make_marks(cas_path, path_rec, map_rec, union_recs):
    dot = _norm(path_rec(cas_path('uni05BC')), map_rec)
    bar = _norm(path_rec(cas_path('uni05B7')), map_rec)
    qam = _norm(path_rec(cas_path('uni05B8')), map_rec)
    db = _bounds(dot)
    d = db[2] - db[0]          # dot diameter
    bb = _bounds(bar)
    bw = bb[2] - bb[0]
    bh = bb[3] - bb[1]
    qb = _bounds(qam)
    gv = float(os.environ.get('GV', 32))   # vertical gap between dot rows
    gh = 50                    # horizontal gap between dots
    row2 = -(d + gv)

    def centred(rec):
        b = _bounds(rec)
        cx = (b[0] + b[2]) / 2
        return map_rec(rec, lambda x, y: (x - cx, y - b[3]))

    sheva = union_recs(dot, _at(dot, map_rec, 0, row2))
    tsere = union_recs(_at(dot, map_rec, -(d + gh) / 2, 0), _at(dot, map_rec, (d + gh) / 2, 0))
    segol = union_recs(_at(dot, map_rec, -(d + gh) / 2, 0), _at(dot, map_rec, (d + gh) / 2, 0),
                       _at(dot, map_rec, 0, row2))
    hiriq = dot
    # qubuts: upper-left to lower-right
    st = d * 0.84
    qub = union_recs(_at(dot, map_rec, -st, 0), _at(dot, map_rec, 0, row2 * 0.55),
                     _at(dot, map_rec, st, row2 * 1.1))

    def hataf(v):
        vb = _bounds(v)
        sh = _at(sheva, map_rec, vb[2] + 34 + d / 2, 0)
        return centred(union_recs(v, sh))

    # the hatafs' vowel is a little narrower, so the pair stays under its letter
    nb = _norm(path_rec(cas_path('uni05B7')), map_rec, SM * 0.82)
    nq = _norm(path_rec(cas_path('uni05B8')), map_rec, SM * 0.82)
    nq = map_rec(nq, lambda x, y: (x, y / 0.82))          # keep the qamats' height
    nb = map_rec(nb, lambda x, y: (x, y / 0.82))
    seg2 = union_recs(_at(dot, map_rec, -(d + 40) / 2, 0), _at(dot, map_rec, (d + 40) / 2, 0),
                      _at(dot, map_rec, 0, row2))
    h_segol = hataf(seg2)
    h_patah = hataf(nb)
    h_qamats = hataf(nq)
    # meteg: short vertical bar, a little lighter than a stem
    mw, mh = 46, 150
    meteg = [('moveTo', ((-mw / 2, 0),)), ('lineTo', ((mw / 2, 0),)), ('lineTo', ((mw / 2, -mh),)),
             ('lineTo', ((-mw / 2, -mh),)), ('closePath', ())]
    # rafe: thinner, shorter bar, its bottom at 0
    rw, rh = 180, 60
    rafe = [('moveTo', ((-rw / 2, 0),)), ('lineTo', ((-rw / 2, rh),)), ('lineTo', ((rw / 2, rh),)),
            ('lineTo', ((rw / 2, 0),)), ('closePath', ())]
    # qamats qatan: qamats with a longer stem
    qq = stretch(qam, -bh - 4, 0, axis=1)
    qq = [(op, tuple((x, y - 40) if y < -bh - 4 else (x, y) for x, y in args)) for op, args in qam]

    above_dot = _at(dot, map_rec, 0, d)        # bottom at 0
    centre_dot = _at(dot, map_rec, 0, d / 2)   # centre at 0
    return {
        0x05B0: sheva, 0x05B1: h_segol, 0x05B2: h_patah, 0x05B3: h_qamats,
        0x05B4: hiriq, 0x05B5: tsere, 0x05B6: segol, 0x05B7: bar, 0x05B8: qam,
        0x05B9: above_dot, 0x05BA: above_dot, 0x05BB: qub, 0x05BC: centre_dot,
        0x05BD: meteg, 0x05BF: rafe, 0x05C1: above_dot, 0x05C2: above_dot,
        0x05C4: above_dot, 0x05C5: dot, 0x05C7: qq,
    }


MARK_CLASS = {
    0x05B0: 'below', 0x05B1: 'below', 0x05B2: 'below', 0x05B3: 'below', 0x05B4: 'below',
    0x05B5: 'below', 0x05B6: 'below', 0x05B7: 'below', 0x05B8: 'below', 0x05BB: 'below',
    0x05C5: 'below', 0x05C7: 'below',
    0x05BD: 'meteg',
    0x05B9: 'holam', 0x05BA: 'holamhaser',
    0x05BF: 'above', 0x05C4: 'above',
    0x05C1: 'shindot', 0x05C2: 'sindot',
    0x05BC: 'dagesh',
}

# per-letter box (x0, y0, x1, y1) in which the dagesh takes the point
# farthest from ink, or a fixed point (x, y)
DAGESH = {
    0x05D0: (110, 200, 300, 450), 0x05D1: (150, 140, 330, 480), 0x05D2: (200, 250, 330, 450),
    0x05D3: (140, 200, 320, 480), 0x05D4: (200, 200, 400, 460), 0x05D5: None,
    0x05D6: None, 0x05D7: (300, 315), 0x05D8: (200, 140, 400, 470),
    0x05D9: None, 0x05DA: (140, 200, 380, 470), 0x05DB: (150, 140, 380, 480),
    0x05DC: (180, 150, 400, 460), 0x05DD: (300, 315), 0x05DE: (230, 150, 400, 470),
    0x05DF: None, 0x05E0: (200, 150, 340, 470), 0x05E1: (200, 150, 400, 480),
    0x05E2: (230, 200, 400, 470), 0x05E3: (150, 150, 380, 330), 0x05E4: (150, 140, 380, 330),
    0x05E5: (250, 250, 420, 480), 0x05E6: (200, 150, 400, 470), 0x05E7: (200, 200, 400, 470),
    0x05E8: (140, 200, 380, 480), 0x05E9: (200, 200, 420, 470), 0x05EA: (270, 150, 400, 470),
}
DAGESH_OWN = set()   # letters whose dagesh ignores Cascadia's anchor
# below-mark anchor x overrides (mark top y in the second slot, else BELOW_Y)
BELOW = {}


def _stem(rec, y=300):
    from geom import runs_at_y
    r = runs_at_y(_polys(rec), y)
    return r[-1] if r else None


def _ink_band(rec, y0, y1):
    """x extent of ink inside a horizontal band (via flattening)."""
    from geom import runs_at_y
    from build import rec_to_path
    import pathops
    polys = _polys(rec)
    xs = []
    for y in range(int(y0), int(y1), 10):
        for a, b in runs_at_y(polys, y + 0.5):
            xs += [a, b]
    return (min(xs), max(xs)) if xs else None


def _polys(rec):
    from geom import FlatPen
    p = FlatPen(None)
    for op, args in rec:
        getattr(p, op)(*args)
    return p.polys


def _empty_centre(rec, box):
    """Point in box farthest from any ink (and inside no ink)."""
    polys = _polys(rec)
    segs = []
    for poly in polys:
        for i in range(len(poly)):
            segs.append((poly[i], poly[(i + 1) % len(poly)]))

    def inside(x, y):
        c = 0
        for (x0, y0), (x1, y1) in segs:
            if (y0 <= y < y1) or (y1 <= y < y0):
                if x0 + (y - y0) * (x1 - x0) / (y1 - y0) > x:
                    c += 1
        return c % 2 == 1

    def dist(x, y):
        best = 1e9
        for (x0, y0), (x1, y1) in segs:
            dx, dy = x1 - x0, y1 - y0
            L = dx * dx + dy * dy
            t = 0 if L == 0 else max(0, min(1, ((x - x0) * dx + (y - y0) * dy) / L))
            px, py = x0 + t * dx, y0 + t * dy
            best = min(best, math.hypot(x - px, y - py))
        return best
    bestp, bestd = None, -1
    x0, y0, x1, y1 = box
    for xi in range(int(x0), int(x1) + 1, 8):
        for yi in range(int(y0), int(y1) + 1, 8):
            if inside(xi, yi):
                continue
            dd = dist(xi, yi)
            if dd > bestd:
                bestd, bestp = dd, (xi, yi)
    return bestp, bestd


def anchors(letters, marks, MAPPED):
    from build import rec_bounds
    mark_a = {}
    for u, c in MARK_CLASS.items():
        mark_a[u] = (c, (0, 0))
    base_a = {}
    for u, rec in letters.items():
        x0, y0, x1, y1 = rec_bounds(rec)
        cx = (x0 + x1) / 2
        bx, by = cx, BELOW_Y
        st = _stem(rec)
        from geom import runs_at_y
        desc = runs_at_y(_polys(rec), -100)
        if u in (0x05D5, 0x05D6):                    # vav, zayin: under the stem
            bx = (st[0] + st[1]) / 2
        if desc and u != 0x05DA:                     # a descender: the roomier side of it
            dl, dr = desc[0]
            lo, hi = min(x0, 60), max(x1, 540)
            left, right = (dl - 30) - lo, hi - (dr + 30)
            bx = (lo + dl - 30) / 2 if left >= right else (dr + 30 + hi) / 2
        if u == 0x05DA:                              # final kaf: inside, left of the stem
            bx, by = (x0 + st[0]) / 2, 400
        d = {}
        d['below'] = (bx, by)
        d['meteg'] = (bx - 160, by) if u != 0x05DA else (bx, -50)
        if u == 0x05E7:                              # qof: between the leg and the vowel
            d['meteg'] = (desc[0][1] + 45, by)
        d['above'] = (cx, ABOVE_Y)
        d['holam'] = (x0 + 10, ABOVE_Y)
        d['holamhaser'] = (x0 + 10, ABOVE_Y)
        d['shindot'] = (x1 - 46, ABOVE_Y)
        d['sindot'] = (x0 + 46, ABOVE_Y)
        if u == 0x05E9:                              # shin: holam and sin dot are one dot
            d['holam'] = d['holamhaser'] = d['sindot']
        if u == 0x05DC:                              # lamed: left of the mast
            mast = runs_at_y(_polys(rec), 700)[0]
            d['holam'] = d['holamhaser'] = d['sindot'] = (mast[0] - 52, ABOVE_Y)
        if u == 0x05D5:                              # holam male over the vav; haser to its left
            d['holam'] = ((x0 + x1) / 2 - 10, ABOVE_Y)
            d['holamhaser'] = (x0 - 20, ABOVE_Y)
        box = DAGESH.get(u)
        if 'dagesh' in MAPPED.get(u, {}) and u not in DAGESH_OWN:
            d['dagesh'] = MAPPED[u]['dagesh']        # the Cascadia designer's placement
        elif u == 0x05D5:                              # shuruk: left of the stem, mid-height
            d['dagesh'] = (st[0] - 75, 300)
        elif u == 0x05D6:
            d['dagesh'] = (st[0] - 75, 300)
        elif u == 0x05DF:
            d['dagesh'] = (st[0] - 75, 300)
        elif u == 0x05D9:                            # yod: left of the tail
            d['dagesh'] = (_stem(rec, 420)[0] - 70, 400)
        elif box and len(box) == 4:
            p, _ = _empty_centre(rec, box)
            d['dagesh'] = p or (cx, 300)
        else:
            d['dagesh'] = box or (cx, 300)
        base_a[u] = {k: (round(v[0]), round(v[1])) for k, v in d.items()}
    return mark_a, base_a
