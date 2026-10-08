"""Round 3 design decisions (Regular), kept apart from the plumbing in build3.py.

Round 2's letters, descenders, tav, paseq and 116 dots are kept (design2.py).
Round 3 changes five things, each a numbered item of the brief:

  1. below vowels on a non-descending letter share one x centre (the
     letter's optical centre: its ink centre, or vav's and zayin's stem);
     clearance is bought by dropping 2-4 units, never by sliding sideways
  2. meteg attaches to the vowel before it (mark-to-mark, in JBM's mkmk
     feature, through a mark filtering set that steps over dagesh), left of
     it by METEG_GAP; qof's vowels move right of its leg far enough to leave
     the meteg room
  3. two-row points end at or above -275: dot rows 40 apart (sheva, the
     hatafs), segol's rows packed diagonally, qubuts relaid at >= 40 gaps
  4. lamed's holam: L1 beside the mast (raised until 70 from a neighbour's
     shin dot) or L2 above it, in the cell
  5. unattached points at JetBrains Mono's own combining heights
All units are JetBrains Mono's (1000 upm, cell 600 x -300..1020).
"""
import os
from design2 import *          # noqa: F401,F403 -- round 2's letters, dots and boxes
import design2 as _R2

# ---------------------------------------------------------------- 3. two-row points
GV = float(os.environ.get('GV', 40))          # sheva / hataf rows: 40 between the dots
SEG_PITCH = float(os.environ.get('SEG_PITCH', 128))   # segol: centre-to-centre of its rows; the
                                              # diagonal gap is 40.8 at 127 (dotlayout.py)
QUB_DX = float(os.environ.get('QUB_DX', 142))         # qubuts: horizontal step
QUB_PITCH = float(os.environ.get('QUB_PITCH', 134.4))  # qubuts: top row to bottom row, centres
LOWEST = -275          # no attached point's ink below this
# round 4: under the letters whose bottom dips to -10 (ג ט ל ס ש) the two-row
# points may go lower, so the sheva's top dot does not join the letter at 28 px
DIP = (0x05D2, 0x05D8, 0x05DC, 0x05E1, 0x05E9)
LOWEST_DIP = float(os.environ.get('LOWEST_DIP', LOWEST))
DIP_FLOOR = os.environ.get('DIP_FLOOR') == '1'   # tall points sit on LOWEST_DIP, not just above it

# ---------------------------------------------------------------- 1. below vowels
BELOW_TOP = -36        # round 2's top of a below point
DROP = (0, 2, 4)       # extra drop tried, in order, for clearance
WANT, ACCEPT = 28, 25  # first drop reaching WANT, else first reaching ACCEPT

# ---------------------------------------------------------------- 2. meteg
METEG = (82, 150)
METEG_GAP = float(os.environ.get('METEG_GAP', 24))   # meteg's right edge .. the vowel's left edge
# round 4: off by default -- moving every qof's vowels right for a rare meteg put
# tsere and segol at the cell edge (459, against 392 in round 2)
QOF_ROOM = os.environ.get('QOF_ROOM') == '1'
QOF_LEG_GAP = 21                                     # meteg's left edge .. qof's leg
# the marks meteg may see past (mark filtering set): meteg and the below vowels
METEG_VOWELS = (0x05B0, 0x05B1, 0x05B2, 0x05B3, 0x05B4, 0x05B5, 0x05B6, 0x05B7, 0x05B8, 0x05BB, 0x05C7)

# ---------------------------------------------------------------- 4. lamed
LAMED = os.environ.get('LAMED', 'L1')
L1_MAST_GAP = 28        # holam's right edge .. the mast's left edge
L1_NEIGHBOUR = 70       # holam .. the shin dot of the letter to its left, exact distance
L2_ANCHOR = (58, 825)   # holam ink x 0..116, y 825..930 (the critic's in-cell probe)

# ---------------------------------------------------------------- 5. unattached homes
# JBM's own combining dots: U+0307 dotaccentcomb ink 649..777, U+0323
# dotbelowcomb ink -213..-85. An above point's bottom sits at 649, a below
# point's top at -85 -- unless that would take its ink below LOWEST, in which
# case its top rises until its bottom is at LOWEST (the tall points: qamats
# qatan, sheva, segol, qubuts, the hatafs). The dagesh, unattached, is a dot
# below at U+0323's height: after a digit or a Latin letter it is then seen,
# not lost inside the glyph.
HOME_ABOVE_BOTTOM = 649
HOME_BELOW_TOP = -85
