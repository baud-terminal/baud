"""Round 2 design decisions (Regular), kept apart from the plumbing in build2.py.

Round 1's letters (design.py: weights, widths, edits) are kept as they are;
round 2 changes the descenders, tav's counter, paseq, every point, and every
anchor. All units are JetBrains Mono's (1000 upm, cell 600 x -300..1020).
"""
import os

FAMILY = 'Proto Hebrew Mono C'

# ---------------------------------------------------------------- letters
DESC_FROM, DESC_TO = -252, -180      # ק ך ן ף ץ: JBM's p q y j g reach -180
DESCENDERS = (0x05E7, 0x05DA, 0x05DF, 0x05E3, 0x05E5)
# tav: move the left leg (not the foot's left end, not the bar) left, so a
# point-sized dagesh has >= 25 units on both sides in the counter
TAV_LEG_DX = float(os.environ.get('TAV_LEG_DX', -12))

# paseq: a plain bar, lighter than vav's 90 stem, no head
PASEQ_W = 60
PASEQ_Y = (-40, 690)

# ---------------------------------------------------------------- points
# The dot is JetBrains Mono's own dot (dotbelowcomb, 142 x 128), scaled.
DOT_W = float(os.environ.get('DOT_W', 116))
GV = float(os.environ.get('GV', 46))      # vertical gap between dot rows
GH = float(os.environ.get('GH', 50))      # horizontal gap between dots
BAR = 82                                  # JBM's horizontal stroke
BAR_W = 220                               # patah, qamats, rafe
QAM_H = 190                               # qamats: bar + stem
QQ_H = 236                                # qamats qatan: longer stem
METEG = (82, 150)
HATAF_BAR_W = 180                         # the vowel in a hataf is narrower
HATAF_GAP = 40                            # vowel .. sheva
HSEG_GH = 40                              # segol's dots inside hataf segol
QUB_DX = float(os.environ.get('QUB_DX', 113))   # qubuts: horizontal step

BELOW_TOP = -36       # top of a below point
ABOVE_BOT = 690       # bottom of an above point
MARK_SHIFT = -300     # JBM's convention: a mark's ink is drawn back over the base's cell
# where a point's ink sits when nothing anchors it (after a space, a Latin
# letter, a digit, or on U+25CC): below points hang where they hang under a
# letter; above points clear JBM's cap height (730) by 30; the dagesh sits at
# the centre of JBM's dotted circle (50..670). The mark anchor is this point.
DEFAULT_Y = {'below': -36, 'above': 760, 'dagesh': 360}

TARGET = 28           # anchor search aims for this many units of clearance
TEXT_MIN = 25         # required for clusters in Genesis 1:1-5 and the test lines
GRID_MIN = 12         # required (or listed) for every letter x point

MARK_CLASS = {u: hex(u) for u in (0x05B0, 0x05B1, 0x05B2, 0x05B3, 0x05B4, 0x05B5, 0x05B6, 0x05B7, 0x05B8,
                                   0x05B9, 0x05BA, 0x05BB, 0x05BC, 0x05BD, 0x05BF, 0x05C1, 0x05C2, 0x05C4,
                                   0x05C5, 0x05C7)}
BELOW = (0x05B0, 0x05B1, 0x05B2, 0x05B3, 0x05B4, 0x05B5, 0x05B6, 0x05B7, 0x05B8, 0x05BB, 0x05C5, 0x05C7)
ABOVE_DOTS = (0x05B9, 0x05BA, 0x05C1, 0x05C2, 0x05C4)

# where the dagesh may go: a box in the letter's final frame (x0, y0, x1, y1),
# searched for the most clearance; within CAP the designer's point is kept
DAGESH_CAP = 46
DAGESH_BOX = {
    0x05D0: (150, 120, 330, 330), 0x05D1: (170, 180, 300, 420), 0x05D2: (200, 250, 330, 450),
    0x05D3: (160, 180, 300, 420), 0x05D4: (220, 180, 380, 460), 0x05D7: (220, 120, 380, 460),
    0x05D8: (220, 140, 380, 440), 0x05DA: (170, 180, 340, 420), 0x05DB: (170, 180, 320, 420),
    0x05DC: (190, 150, 380, 420), 0x05DD: (220, 120, 380, 460), 0x05DE: (300, 280, 360, 320),   # mem: the middle of the counter (29) over its low corner (33)
    0x05E0: (200, 150, 330, 430), 0x05E1: (220, 150, 380, 460), 0x05E2: (240, 300, 400, 560),
    0x05E3: (170, 150, 360, 320), 0x05E4: (170, 140, 360, 320), 0x05E5: (260, 280, 420, 560),
    0x05E6: (160, 150, 360, 400), 0x05E7: (220, 200, 380, 470), 0x05E8: (160, 180, 320, 420),
    0x05E9: (220, 100, 420, 400), 0x05EA: (270, 100, 410, 460),
}
