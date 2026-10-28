"""Meteg beside its vowel, inside the cell (2026-10-26).

The Unicode Standard 17.0, 9.1: "Typically, meteg is placed to the left of the
vowel", and "to display meteg to the right of the sheva ..., the sequence with
meteg preceding sheva with an intervening CGJ can be used: <meteg, CGJ, sheva>";
for a hataf vowel, "right-positioned meteg: <meteg, CGJ, hataf patah>". So the
text order says the side: <vowel, meteg> puts meteg left of the vowel, <meteg,
CGJ, vowel> right of it.

Meteg rides on its vowel by mark-to-mark (build.py's meteg lookup), METEG_GAP
from its ink with tops aligned. A vowel and its meteg together are wider than
the vowel alone, so where the vowel's own place leaves meteg outside the cell or
on the letter (hataf segol and qubuts take it past the cell's left edge; qof's
leg stands where it lands), the pair moves -- only in a cluster that carries a
meteg, through a chained context in the mkmk feature that places the vowel and
meteg again by further sets of base anchors. A cluster with no meteg draws as
before.

For each base letter and each vowel meteg may follow (METEG_VOWELS) this module
gives, as base anchors in the style's own frame (after the shear and the
italics' cell fits):

  left[u][v], lmeteg[u][v]    the vowel's and meteg's when meteg follows the vowel
  right[u][v], rmeteg[u][v]   the vowel's and meteg's when meteg precedes it
                              (through CGJ)

The rule. Meteg stays on the side the text order gives and MIN_CLEAR from the
letter, the check's bound; left of the vowel it stands METEG_GAP from it, and
right of it METEG_GAP or further, so that it may stand past a descender rather
than on it (left of qof's leg, with the vowel beyond it, was the other way to
give meteg room there; on qof the lean is the vowel past the cell's right
edge instead). The vowel slides along its row from its own place, ACCEPT from
the letter (or as far as it was, if that was less) all the way: it never passes
through the letter's ink to its other side. Among those places: first the least of meteg's ink outside the
cell (x 0..600); then the least of the vowel's beyond what its own place already
had; then the least shortfall from the anchor search's own bounds (meteg's ink
inside [LO, HI] and ACCEPT from the letter); then the least movement, the
vowel's move along x plus meteg's distance beyond METEG_GAP. Where meteg already
sits inside [LO, HI] and ACCEPT from the letter beside the vowel's own place,
nothing moves.
"""

LO, HI = 6, 594          # a mark's ink in the cell: the anchor search's own bounds (scan_x)
MIN_CLEAR = 22           # meteg to its letter, at the least: the round 3 critic's check's 20 (note AC), plus
                         # the distance field's 2-unit cell, which measured 19.0 exact where it read 20
STEP = 2
REACH = 400              # how far the vowel may move, either way, and meteg stand further

LOG = []                 # (side, letter, vowel, vowel moved by, meteg further by, meteg's ink out, the vowel's added, shortfall)


def place(B, E, letter_recs, vowel_glyph, meteg_glyph, vowel_anchor, meteg_anchor, rel_left, rel_right, base_a):
    """letter_recs: {u: the letter's outline as the font carries it}; vowel_glyph[v], meteg_glyph:
    the marks' outlines as the font carries them (glyph frame); vowel_anchor[v], meteg_anchor: their
    mark anchors; rel_left[v], rel_right[v]: the point of v's glyph frame meteg's mark anchor lands on,
    left and right of it, METEG_GAP from its ink; base_a: {u: {mark: anchor}} the style's final base
    anchors. Returns {'left', 'lmeteg', 'right', 'rmeteg'}: {u: {v: anchor}} each."""
    SV = {v: B.samples(r) for v, r in vowel_glyph.items()}
    SM = B.samples(meteg_glyph)
    mx0, mx1 = SM[:, 0].min(), SM[:, 0].max()
    out = {k: {} for k in ('left', 'lmeteg', 'right', 'rmeteg')}
    for u, rec in letter_recs.items():
        d = base_a[u]
        fld = B.FD.distance(B.polys(rec))[0]
        for k in out:
            out[k][u] = {}
        for v in E.METEG_VOWELS:
            if v not in d:
                continue
            A = d[v]
            vx0, vx1 = SV[v][:, 0].min(), SV[v][:, 0].max()
            ox0, oy = A[0] - vowel_anchor[v][0], A[1] - vowel_anchor[v][1]

            def vout(ox):
                return max(0, -(ox + vx0)) + max(0, ox + vx1 - 600)
            own0 = vout(ox0)
            need_v = min(E.ACCEPT, B.clear_at(fld, SV[v], ox0, oy)) - 1e-6
            cv = {}

            def clear_v(ox):
                if ox not in cv:
                    cv[ox] = B.clear_at(fld, SV[v], ox, oy)
                return cv[ox]
            # how far the vowel slides each way before it would come closer to the letter than allowed
            reach = []
            for sign_ in (-1, 1):
                k = 0
                while k + STEP <= REACH and clear_v(ox0 + sign_ * (k + STEP)) >= need_v:
                    k += STEP
                reach.append(sign_ * k)
            for side, rel in (('left', rel_left[v]), ('right', rel_right[v])):
                sign = 1 if side == 'right' else -1
                dm = (rel[0] - meteg_anchor[0], rel[1] - meteg_anchor[1])   # meteg's offset from the vowel's
                omy = oy + dm[1]
                cm = {}

                def score(s, g):
                    """(meteg's ink out of the cell, the vowel's added, shortfall from the search's
                    bounds, movement), or None where a clearance fails."""
                    if not reach[0] <= s <= reach[1]:
                        return None
                    ox = ox0 + s
                    om = ox + dm[0] + sign * g
                    if om not in cm:
                        cm[om] = B.clear_at(fld, SM, om, omy)
                    if cm[om] < MIN_CLEAR:
                        return None
                    a, b = om + mx0, om + mx1
                    short = max(0, LO - a) + max(0, b - HI) + max(0, E.ACCEPT - cm[om])
                    return (max(0, -a) + max(0, b - 600), max(0, vout(ox) - own0), short, abs(s) + g)
                here = score(0, 0)
                s, g = 0, 0
                if here is None or here[:3] != (0, 0, 0):
                    # by movement, least first, so the first place inside both bounds is the answer
                    found = None
                    for cost in range(STEP, 2 * REACH + 1, STEP):
                        for g_ in range(0, (min(cost, REACH) if side == 'right' else 0) + 1, STEP):
                            a = cost - g_
                            if a > REACH:
                                continue
                            # the vowel moves away from meteg's side first
                            for s_ in ((0,) if a == 0 else (-sign * a, sign * a)):
                                r = score(s_, g_)
                                if r is not None and (found is None or r < found[0]):
                                    found = (r, s_, g_)
                        if found and found[0][:3] == (0, 0, 0):
                            break
                    if found is None and here is None:
                        raise SystemExit('meteg.py: no place clears %s %s, meteg %s' % (chr(u), hex(v), side))
                    if found is not None and (here is None or found[0] < here):
                        r, s, g = found
                        LOG.append((side, chr(u), hex(v), s, g, round(r[0]), round(r[1]), round(r[2])))
                a = (A[0] + s, A[1])
                out['left' if side == 'left' else 'right'][u][v] = a
                out['lmeteg' if side == 'left' else 'rmeteg'][u][v] = (
                    a[0] - vowel_anchor[v][0] + rel[0] + sign * g, a[1] - vowel_anchor[v][1] + rel[1])
    return out
