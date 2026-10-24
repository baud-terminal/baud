"""The italics' optical correction of rounds: JetBrains Mono Italic's, measured at source.

JetBrains Mono's italic masters (sources/JetBrainsMono-Italic.glyphs at cd5227b,
italicAngle = 9 in every master) are drawn as their own outlines, and read against
the upright masters (sources/JetBrainsMono.glyphs) node for node they are the upright
sheared 9 degrees about y ~ 278: on 26 compatible letters and figures the on-curve
nodes sit 0.4 units off the sheared upright on average horizontally (Regular master;
0.3 ExtraBold), while a, e, f, y, Q and nine are redrawn, not sheared. Stems keep
their horizontal width (n, H, l: 90 in both), bars their height (H, E: 82), and the
combining dots are the sheared upright (U+0323, U+0307: equal area, axes 150.4 x 125.3
sheared and in the italic). The scripts and their output are in
docs/research/notes/ac/italic/. Two things are not the shear, both at the rounds.

**The bowls' joints (level_rec).** Of the 350 ends of the Regular master's 175
right-angle round corners, 10 move more than 3 units along their straight, all at a
bowl's curve/straight joint and all in o, O and q: o's outer joints 12.6-12.7 units,
O's outer 10.1, q's inner -10 (ExtraBold: only O's outer, 10.1). The move is the same
at the acute and the obtuse corner (12.6 / 12.7, 10.1 / 10.1), so it is no function
of the sheared angle; what it does is level the joints. Paired side by side, five
upright bowls have an outer straight that stops short of the inner one (the outer
joint farther from the corner than the inner one across the stroke): o 10 units, O
9-10, q 4-5, g 4-5, e 10 (ExtraBold: O 10). The italic levels o and O (their outer
joints move to the inner ones' heights) and q (its inner joints move in by 10), and
leaves g (pure shear) and e (redrawn) short. Its effect is optical: a shear thins a
bowl's obtuse corner (o: 78.5 units along the diagonal against 92.6 at the acute),
and the italic has 3.1-3.2% more ink there than the sheared upright, 0.2-0.6% more at
the acute. Hebrew's bowls stop short by 6.6-31.2 units (Regular) and 23.2-45.5 (Bold):
more than JetBrains Mono ever moved. level_rec moves each outer joint toward its inner
one by the gap, at most LEVEL units (style.py: 13, JetBrains Mono's largest move),
which thickens Hebrew's obtuse corners 1.4-3.0% and its acute 0.5-0.7%, where a full
level would give 4.6-5.1% and 3.7-4.0% (Jev, D13_LEVEL_CAPPED, 0.99 at confidence
0.98 in both orders). The outer arc is scaled along its straight between its other
end's tangent line and the joint, so the joint stays on its straight and both ends
keep their tangents.

**The handles (italic_rec).** A shear turns a right-angled round corner into an acute
one (81 degrees) on one diagonal and an obtuse one (99) on the other, and JetBrains
Mono's italic shortens the handles of the acute corners and lengthens those of the
obtuse: the italic's handle length over the distance to the tangents' intersection,
divided by the upright's (which a shear keeps), fits

    k = 1 + K * (theta - 90)

through k = 1 at a right angle, with K by what meets the corner's arc at its ends
(Regular master; style.py has the ExtraBold's and Bold Italic's):

    a straight stroke on both sides   14 corners (D, t, 5)      K = 0.00022   0.998 / 1.002
    a straight stroke on one side     132 corners (b, n, o, C)   K = 0.00091   0.992 / 1.008
    curve on both sides               29 corners (S, 6, 8)       K = 0.00104   0.991 / 1.009

at 81 / 99 degrees. Most corners are pure shear (medians 1.000 and 1.002); the bowls
(o, O, C, G, b, d, p, q, s, the figures) carry the correction, up to 0.94 and 1.065,
and a corner between two straight strokes is all but never corrected (1 of 14).

italic_rec applies that function to every round corner of a Hebrew letter: each arc
of an outline that turns through a right angle upright (87-93 degrees, between two
points where the tangent is horizontal or vertical) gets the k of the angle the shear
gives it and of its kind, so each Hebrew corner gets what JetBrains Mono gives a
corner like it (Jev, C_PER_KIND_FIT, 0.96 at confidence 0.94 in both orders); 43 arcs
in Italic, 39 in Bold Italic. The letters' outlines are TrueType splines from
Cascadia, not one cubic per corner, so the handle ratio is carried as what it does to
a cubic quarter: the arc's midpoint (where it crosses the corner's diagonal) moves
toward the corner as far as scaling a cubic's handles by k moves that cubic's
midpoint, and every control point of the arc moves toward the corner by that much
times sin^2(2 phi), phi its angle about the corner's opposite point. The arc's ends
stay where level_rec and the shear put them and a point beside an end moves along the
end's tangent line, so the outline stays smooth where an arc meets a straight stroke.
With every K and LEVEL 0 each point is the plain shear's, exactly, and the build is
the plain shear's bytes.
"""
import math

TURN = (87.0, 93.0)       # an arc corrected: its tangent turns through a right angle upright (JBM's corners: within 3 degrees)
MIN_CHORD = 20.0          # and its ends are this far apart: smaller is a joint, not a round
SAMPLES = 24              # samples a curved piece, to find its arcs' ends and midpoints
DEGENERATE = 0.5          # a straight piece shorter than this is a point, not a stroke: an arc runs across it
STROKE = (40.0, 200.0)    # an outer and an inner arc are one stroke's two sides when their straights are this far apart
LEVEL_TOL = 2.5           # level_rec's arcs end within this many degrees of an axis: Cascadia's Bold bowls end 1.7 off

CORRECTED = []            # (letter label, theta, k, largest move in units): what italic_rec did, for the build log
LEVELLED = []             # (letter label, the outer joint, the gap to the inner's in units, the move): what level_rec did


def _pieces(contour):
    """A contour's recording ops as pieces: ('L', p0, p1) or ('Q', p0, c, p1) or ('C', p0, c1, c2, p1),
    each with the recording positions of its control points: (op index, arg index) per point."""
    out = []
    start = cur = contour[0][1][0]
    cur_ref = (0, 0)
    for i, (op, args) in enumerate(contour[1:], 1):
        if op == 'lineTo':
            out.append(('L', [cur, args[0]], [cur_ref, (i, 0)]))
            cur, cur_ref = args[0], (i, 0)
        elif op == 'qCurveTo':
            offs, end = list(args[:-1]), args[-1]
            if end is None:
                raise SystemExit('optical: an all-off-curve contour')
            for j, o in enumerate(offs):
                nxt = end if j == len(offs) - 1 else ((o[0] + offs[j + 1][0]) / 2, (o[1] + offs[j + 1][1]) / 2)
                nref = (i, len(offs)) if j == len(offs) - 1 else None
                out.append(('Q', [cur, o, nxt], [cur_ref, (i, j), nref]))
                cur, cur_ref = nxt, nref
        elif op == 'curveTo':
            out.append(('C', [cur] + list(args), [cur_ref, (i, 0), (i, 1), (i, 2)]))
            cur, cur_ref = args[2], (i, 2)
        elif op in ('closePath', 'endPath'):
            if cur != start:
                out.append(('L', [cur, start], [cur_ref, (0, 0)]))
        else:
            raise SystemExit('optical: op %r' % op)
    return out


def _at(piece, t):
    k, P = piece[0], piece[1]
    if k == 'L':
        (x0, y0), (x1, y1) = P
        return (x0 + (x1 - x0) * t, y0 + (y1 - y0) * t), (x1 - x0, y1 - y0)
    if k == 'Q':
        (x0, y0), (cx, cy), (x1, y1) = P
        m = 1 - t
        return ((m * m * x0 + 2 * m * t * cx + t * t * x1, m * m * y0 + 2 * m * t * cy + t * t * y1),
                (2 * (m * (cx - x0) + t * (x1 - cx)), 2 * (m * (cy - y0) + t * (y1 - cy))))
    (x0, y0), (ax, ay), (bx, by), (x1, y1) = P
    m = 1 - t
    return ((m ** 3 * x0 + 3 * m * m * t * ax + 3 * m * t * t * bx + t ** 3 * x1, m ** 3 * y0 + 3 * m * m * t * ay + 3 * m * t * t * by + t ** 3 * y1),
            (3 * (m * m * (ax - x0) + 2 * m * t * (bx - ax) + t * t * (x1 - bx)), 3 * (m * m * (ay - y0) + 2 * m * t * (by - ay) + t * t * (y1 - by))))


def _axis_roots(piece):
    """Interior parameters where the piece's tangent is horizontal or vertical."""
    k, P = piece[0], piece[1]
    ts = []
    for a in (0, 1):
        if k == 'Q':
            d0, d1 = P[1][a] - P[0][a], P[2][a] - P[1][a]
            if d0 != d1:
                ts.append(d0 / (d0 - d1))
        elif k == 'C':
            p0, p1, p2, p3 = (q[a] for q in P)
            A, B, C = -p0 + 3 * p1 - 3 * p2 + p3, 2 * (p0 - 2 * p1 + p2), p1 - p0
            if abs(A) < 1e-12:
                if abs(B) > 1e-12:
                    ts.append(-C / B)
            else:
                disc = B * B - 4 * A * C
                if disc >= 0:
                    ts += [(-B + s * math.sqrt(disc)) / (2 * A) for s in (1, -1)]
    return sorted(t for t in ts if 1e-6 < t < 1 - 1e-6)


def _ang(v):
    return math.degrees(math.atan2(v[1], v[0]))


def _turn(a, b):
    return (b - a + 180) % 360 - 180


def _length(pc):
    P = pc[1]
    return sum(math.dist(a, b) for a, b in zip(P, P[1:]))


def _arcs(pcs, end_tol=1.0):
    """The contour's right-angle arcs, upright: (s0, s1, P0, P3, d0, d3, kind) with s the position along
    the contour (piece index + t), P the ends, d their tangents, kind and which ends meet a straight. A
    straight piece shorter than DEGENERATE (Cascadia's outlines carry runs of them where a contour
    starts) is not a stroke: an arc runs across it, and what meets an arc is the first piece past it."""
    n = len(pcs)
    real = [pc[0] != 'L' or _length(pc) >= DEGENERATE for pc in pcs]

    def joined(s_end, s_start):
        # s_start follows s_end with only degenerate pieces between
        if abs(s_end - s_start) < 1e-9:
            return True
        if s_end != math.floor(s_end) or s_start != math.floor(s_start) or s_start < s_end:
            return False
        return not any(real[i % n] for i in range(int(s_end), int(s_start)))

    def neighbour(i, step):
        for _ in range(n):
            if real[i % n]:
                break
            i += step
        return pcs[i % n]
    # the curved spans: each curve piece cut at its interior axis-aligned tangents
    arcs = []
    for i, pc in enumerate(pcs):
        if pc[0] == 'L':
            continue
        ts = [0.0] + _axis_roots(pc) + [1.0]
        arcs += [(i + t0, i + t1, pc, t0, t1) for t0, t1 in zip(ts, ts[1:]) if t1 > t0]
    # merge consecutive curved spans whose tangents meet smoothly, until an axis-aligned tangent ends them
    merged = []
    for s0, s1, pc, t0, t1 in arcs:
        p0, d0 = _at(pc, t0)
        p1, d1 = _at(pc, t1)
        if merged and joined(merged[-1][1], s0) and abs(_turn(_ang(merged[-1][5]), _ang(d0))) < 2.0 \
                and not _axial(merged[-1][5]):
            m = merged[-1]
            merged[-1] = (m[0], s1, m[2], p1, m[4], d1)
        else:
            merged.append((s0, s1, p0, p1, d0, d1))
    # the first and last may be one arc across the contour's start
    if len(merged) > 1 and joined(merged[-1][1], merged[0][0] + n) \
            and abs(_turn(_ang(merged[-1][5]), _ang(merged[0][4]))) < 2.0 and not _axial(merged[-1][5]):
        a, b = merged[-1], merged.pop(0)
        merged[-1] = (a[0], b[1] + len(pcs), a[2], b[3], a[4], b[5])
    out = []
    for s0, s1, p0, p1, d0, d1 in merged:
        turn = abs(_turn(_ang(d0), _ang(d1)))
        if TURN[0] <= turn <= TURN[1] and _axial(d0, end_tol) and _axial(d1, end_tol) and math.dist(p0, p1) >= MIN_CHORD:
            # what meets the arc at each end: a straight piece, or more curve (the arc was cut at an
            # axis-aligned tangent inside a curve)
            before = s0 == math.floor(s0) and neighbour(int(s0) - 1, -1)[0] == 'L'
            after = s1 == math.floor(s1) and neighbour(int(s1), 1)[0] == 'L'
            kind = 'both-straight' if before and after else 'one-straight' if before or after else 'both-curved'
            out.append((s0, s1, p0, p1, d0, d1, kind, (before, after)))
    return out


def _axial(d, tol=1.0):
    a = _ang(d) % 90
    return min(a, 90 - a) <= tol


def _inter(p, d, q, e):
    det = d[0] * (-e[1]) - d[1] * (-e[0])
    if abs(det) < 1e-12:
        return None
    rx, ry = q[0] - p[0], q[1] - p[1]
    s = (rx * (-e[1]) - ry * (-e[0])) / det
    return (p[0] + s * d[0], p[1] + s * d[1])


def _in(s, s0, s1, n):
    return s0 - 1e-9 <= s <= s1 + 1e-9 or s0 - 1e-9 <= s + n <= s1 + 1e-9


def _unit(v):
    h = math.hypot(*v)
    return (v[0] / h, v[1] / h)


def _dot(a, b):
    return a[0] * b[0] + a[1] * b[1]


def level_rec(rec, cap, label=''):
    """`rec` (upright) with its bowls' joints levelled, as JetBrains Mono's italic levels its own: where
    a bowl's outer arc meets its straight farther from the corner than the inner arc across the stroke
    meets its own -- the outer straight stopping short of the inner -- the outer arc is shortened along
    its straight by that gap, at most `cap` units. The arc is scaled along the straight between its other
    end's tangent line and the joint, so the joint stays on its straight and both ends keep their
    tangents. 0 is no change."""
    if not cap:
        return rec
    out = [[op, list(args)] for op, args in rec]
    starts = [i for i, (op, _) in enumerate(rec) if op == 'moveTo']
    found = []          # (contour start, pieces, positions, s0, s1, corner, ends: [(point, unit from corner, straight?)])
    for ci, a in enumerate(starts):
        b = starts[ci + 1] if ci + 1 < len(starts) else len(rec)
        pcs = _pieces(rec[a:b])
        pos = {}
        for i, pc in enumerate(pcs):
            refs = pc[2]
            if refs[0] is not None:
                pos.setdefault(refs[0], float(i))
            if pc[0] == 'Q':
                pos[refs[1]] = i + 0.5
            elif pc[0] == 'C':
                pos[refs[1]], pos[refs[2]] = i + 1 / 3, i + 2 / 3
        last = pcs[-1][2][-1]            # a contour closed by a curve ends on its start point, recorded twice
        if last is not None and last not in pos:
            pos[last] = float(len(pcs))
        for s0, s1, P0, P3, d0, d3, kind, (before, after) in _arcs(pcs, LEVEL_TOL):
            C = _inter(P0, d0, P3, d3)
            if C is None:
                continue
            ends = [(P0, _unit((P0[0] - C[0], P0[1] - C[1])), before), (P3, _unit((P3[0] - C[0], P3[1] - C[1])), after)]
            found.append((a, pcs, pos, s0, s1, C, kind, ends))
    for A in found:
        a, pcs, pos, s0, s1, C, kind, ends = A
        if kind != 'one-straight':
            continue
        (J, u, _), (E, e, _) = ends if ends[0][2] else ends[::-1]
        best = None
        for B in found:
            if B is A:
                continue
            CB = B[5]
            if _dot((CB[0] - C[0], CB[1] - C[1]), (u[0] + e[0], u[1] + e[1])) <= 0:
                continue                 # B's corner is not inside A's
            for k in (0, 1):
                JB, uB, straight = B[7][k]
                EB, eB, _ = B[7][1 - k]
                if not straight or _dot(uB, u) < 0.999 or _dot(eB, e) < 0.999:
                    continue
                sep = abs(_dot((JB[0] - J[0], JB[1] - J[1]), e))
                if STROKE[0] <= sep <= STROKE[1] and (best is None or sep < best[0]):
                    best = (sep, JB)
        if best is None:
            continue
        gap = _dot((J[0] - C[0], J[1] - C[1]), u) - _dot((best[1][0] - C[0], best[1][1] - C[1]), u)
        if gap <= 0.5:
            continue
        m = min(gap, cap)
        hJ = _dot((J[0] - E[0], J[1] - E[1]), u)
        scale = (hJ - m) / hJ
        n = len(pcs)
        for ref, s in pos.items():
            if not _in(s, s0, s1, n):
                continue
            op_i, arg_i = ref
            x, y = out[a + op_i][1][arg_i]
            h = _dot((x - E[0], y - E[1]), u)
            if h > 0:
                out[a + op_i][1][arg_i] = (x + (h * scale - h) * u[0], y + (h * scale - h) * u[1])
        if label:
            LEVELLED.append((label, tuple(round(c, 1) for c in J), round(gap, 1), round(m, 1)))
    return [(op, tuple(args)) for op, args in out]


def italic_rec(rec, shear, slant_k, label='', level=0):
    """`rec` sheared by `shear` (a point map), its bowls' joints first levelled by up to `level` units
    (level_rec), with each right-angle round's handles re-balanced by
    k = 1 + slant_k[kind] * (theta - 90), theta the corner's sheared angle and kind what meets the arc
    at its ends ('both-straight', 'one-straight', 'both-curved'). All zero is the plain shear."""
    rec = level_rec(rec, level, label)
    plain = [(op, tuple(None if a is None else shear(*a) for a in args)) for op, args in rec]
    if not any(slant_k.values()):
        return plain
    out = [[op, list(args)] for op, args in plain]
    # contours: [start, end) op ranges
    starts = [i for i, (op, _) in enumerate(rec) if op == 'moveTo']
    for ci, a in enumerate(starts):
        b = starts[ci + 1] if ci + 1 < len(starts) else len(rec)
        contour = rec[a:b]
        pcs = _pieces(contour)
        n = len(pcs)
        arcs = _arcs(pcs)
        if not arcs:
            continue
        # every control point's position along the contour: an on-curve at its piece's start, an
        # off-curve at its piece's middle (a cubic's at thirds)
        pos = {}
        for i, pc in enumerate(pcs):
            refs = pc[2]
            if refs[0] is not None:
                pos.setdefault(refs[0], float(i))
            if pc[0] == 'Q':
                pos[refs[1]] = i + 0.5
            elif pc[0] == 'C':
                pos[refs[1]], pos[refs[2]] = i + 1 / 3, i + 2 / 3
        moves = {}
        for s0, s1, P0, P3, d0, d3, kind, _ in arcs:
            C = _inter(P0, d0, P3, d3)
            if C is None:
                continue
            P0s, P3s, Cs = shear(*P0), shear(*P3), shear(*C)
            v0 = (P0s[0] - Cs[0], P0s[1] - Cs[1])
            v3 = (P3s[0] - Cs[0], P3s[1] - Cs[1])
            theta = math.degrees(math.acos(max(-1, min(1, (v0[0] * v3[0] + v0[1] * v3[1]) / math.hypot(*v0) / math.hypot(*v3)))))
            k = 1 + slant_k[kind] * (theta - 90)
            O = (P0s[0] + P3s[0] - Cs[0], P0s[1] + P3s[1] - Cs[1])
            eu = (P3s[0] - O[0], P3s[1] - O[1])         # u: toward P3; v: toward P0; the corner is (1, 1)
            ev = (P0s[0] - O[0], P0s[1] - O[1])
            det = eu[0] * ev[1] - eu[1] * ev[0]

            def uv(p):
                rx, ry = p[0] - O[0], p[1] - O[1]
                return ((rx * ev[1] - ry * ev[0]) / det, (eu[0] * ry - eu[1] * rx) / det)
            # the arc's fullness: where it crosses the corner's diagonal u = v
            best = None
            lo, hi = s0, s1
            for q in range(SAMPLES * max(1, math.ceil(hi - lo)) + 1):
                s = lo + (hi - lo) * q / (SAMPLES * max(1, math.ceil(hi - lo)))
                i = min(int(s) % n, n - 1)
                t = s - math.floor(s)
                if s >= n and abs(s - n) < 1e-12:
                    i, t = n - 1, 1.0
                p, _ = _at(pcs[i], t)
                u, v = uv(shear(*p))
                if best is None or abs(u - v) < best[0]:
                    best = (abs(u - v), (u + v) / 2)
            um = best[1]
            f = min(0.95, max(0.05, (8 * um - 4) / 3))   # the cubic quarter with this midpoint has handles f
            W = 3 * (k - 1) * f / (4 - 3 * f)             # its midpoint's move toward the corner under handles k f
            biggest = 0.0
            for ref, s in pos.items():
                if not _in(s, s0, s1, n):
                    continue
                op_i, arg_i = ref
                p = tuple(out[a + op_i][1][arg_i])
                u, v = uv(p)
                phi = math.atan2(max(v, 0.0), max(u, 0.0))
                w = W * math.sin(2 * phi) ** 2
                dx, dy = w * (Cs[0] - p[0]), w * (Cs[1] - p[1])
                moves[ref] = (dx, dy)
                biggest = max(biggest, math.hypot(dx, dy))
            if label:
                CORRECTED.append((label, kind, round(theta, 1), round(k, 4), round(biggest, 2)))
        for (op_i, arg_i), (dx, dy) in moves.items():
            x, y = out[a + op_i][1][arg_i]
            out[a + op_i][1][arg_i] = (x + dx, y + dy)
    return [(op, tuple(args)) for op, args in out]
