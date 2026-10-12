"""A CFF2 font built to reach what the real CFF2 fixture never does.

Writes baud-font/tests/var/cff2-cases.otf. Source Code VF
(baud-font/tests/fonts/SourceCodeVF-Upright.otf) and SFIndia carry no
hints, no hintmask, no flex, no global subroutines, one Font DICT, one
axis and no Private DICT vsindex; this font has all of those, so
HarfBuzz's drawing of it (scripts/cff2-oracle.py) decides the rules the
real fonts leave untested:

  .notdef  a square, nothing varies
  blend    rmoveto with one n=2 blend and k=3 deltas; hlineto and vlineto
           with n=1 blends; the contour ends away from its start, so the
           close is the specification's implicit lineto
  stems    hstemhm and vstemhm whose operands carry blends, an implied
           vstem before the first mask (its operands blended too), a
           cntrmask, a hintmask, and a hintmask between two contours, in
           the text's order "hs* vs* cm* hm* {mt <path constructing
           data>}*": the mask length is the stem count after blending,
           nine stems and two bytes, where
           six without the implied ones would be one byte, so a wrong count
           misreads the path that follows
  flex     flex, flex1 (both of its branches), hflex and hflex1, with
           Fixed (16.16) operands
  subrs    local and global subroutines, nested three deep, one that only
           pushes operands, one that blends
  vsindex  a charstring vsindex selecting the second data set (k=2), then
           an n=6 blend on rrcurveto
  fd1      a glyph of the second Font DICT, whose Private DICT selects the
           third data set (k=1), and its own local subroutines
  stacked  a value blended twice before its operator (the specification's
           "advanced uses of blend"), and a blend whose n is less than the
           operator's operand count
  curves   hvcurveto and vhcurveto at 4, 5, 8, 9, 12 and 13 operands,
           hhcurveto and vvcurveto with a leading odd operand, rcurveline
           and rlinecurve
  moves    a moveto that draws nothing, then a contour

Axes: wght 100-400-900 and wdth 50-100-200. Regions: R0 wght (0,1,1),
R1 wght (-1,-1,0), R2 wdth (0,1,1), R3 wght (0,0.5,1), R4 wght (0,1,1) and
wdth (0,1,1). Data sets: 0 = R0 R1 R2, 1 = R3 R4, 2 = R2. FDSelect is
format 4. Deterministic: the head timestamps are fixed.

    nix shell --impure --expr 'with import (builtins.getFlake "nixpkgs") {}; \
      python3.withPackages (p: [ p.fonttools ])' -c python3 scripts/cff2-cases-font.py
"""
import os
from fontTools.fontBuilder import FontBuilder
from fontTools.misc.psCharStrings import T2CharString
from fontTools.cffLib import FDSelect, SubrsIndex, VarStoreData
from fontTools.varLib.builder import buildVarRegionList, buildVarData, buildVarStore

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
OUT = os.path.join(ROOT, "baud-font/tests/var/cff2-cases.otf")

order = [".notdef", "blend", "stems", "flex", "subrs", "vsindex", "fd1", "stacked", "curves", "moves"]
BIAS = 107  # fewer than 1240 subroutines in every INDEX here


def fixed(v):
    """A Fixed operand: fontTools writes a float as 16.16 (encoding 255)."""
    return float(v)


# Data set 0 has k = 3 (R0 R1 R2), data set 1 k = 2 (R3 R4), data set 2 k = 1 (R2).
programs = {
    ".notdef": [100, 0, "rmoveto", 400, 700, -400, "hlineto"],
    "blend": [
        # rmoveto 120 40, its two operands blended in one n=2 blend.
        120, 40, 30, -20, 10, 5, 0, -8, 2, "blend", "rmoveto",
        300, 60, -40, 25, 1, "blend", "hlineto",
        500, -50, 30, 0, 1, "blend", "vlineto",
        -150, 20, 0, 0, 1, "blend", -200, "rlineto",
    ],
    "stems": [
        # Two blended horizontal stems (4 operands, n=4 blend), two plain.
        0, 80, 300, 80, 10, 0, 0, 5, 0, 0, -10, 0, 0, 5, 0, 3, 4, "blend", 200, 60, 100, 40,
        "hstemhm",
        # Two vertical stems defined by vstemhm, the first blended.
        100, 70, 20, 0, 0, 0, 0, 5, 2, "blend", 300, 40, "vstemhm",
        # The implied vstemhm before the first mask, a cntrmask, which "cm*"
        # puts before every hintmask: three pairs, the first blended --
        # nine stems in all, so two mask bytes, where the
        # six defined without it would make one. Stem 9's bit is set in
        # the second byte of the last mask, so a reader that takes one byte
        # reads 0x80 as the number -11 and the rmoveto after it as malformed.
        400, 70, 15, 0, 0, 0, 0, 0, 2, "blend", 500, 70, 100, 30, "cntrmask", b"\x50\x00",
        "hintmask", b"\xa8\x80",
        50, 0, "rmoveto", 600, 700, -600, "hlineto",
        "hintmask", b"\x54\x80",
        200, -500, 10, 0, 0, 0, 0, 0, 2, "blend", "rmoveto", 200, 300, -200, "hlineto",
    ],
    "flex": [
        100, 300, "rmoveto",
        fixed(50.5), fixed(10.25), 50, 10, 50, 0, 50, 0, 50, -10, fixed(50.75), -10, 50, "flex",
        # flex1 whose summed dx dominates: the last operand is dx.
        50, 20, 50, 20, 50, 0, 50, -20, 50, -20, 40, "flex1",
        # flex1 whose summed dy dominates: the last operand is dy.
        10, 50, 10, 50, 0, 50, -10, 50, -10, 50, -30, "flex1",
        -60, fixed(20.5), 30, -50, -50, fixed(-20.5), -60, "hflex",
        -50, 10, -50, 20, -50, -50, -50, -20, -50, "hflex1",
        -200, -400, 15, -10, 0, 0, 0, 0, 2, "blend", "rlineto",
    ],
    "subrs": [
        # gsubr 0 pushes the moveto's operands; lsubr 0 draws, and calls
        # lsubr 1, which calls gsubr 1.
        0 - BIAS, "callgsubr", "rmoveto",
        0 - BIAS, "callsubr",
        -300, 0, "rlineto",
    ],
    "vsindex": [
        1, "vsindex",
        100, 100, 30, 0, 0, 0, 2, "blend", "rmoveto",
        100, 0, 100, 100, 0, 100,
        10, 5, 0, 0, 5, 5, 0, 0, 10, 0, 0, 10, 6, "blend", "rrcurveto",
        -200, 0, "rlineto",
    ],
    "fd1": [
        # The Private DICT selects data set 2 (k = 1): every blend has one
        # delta per value.
        150, 150, 40, -40, 2, "blend", "rmoveto",
        0 - BIAS, "callsubr",
        -250, 20, 1, "blend", "hlineto",
    ],
    "stacked": [
        # 100 is blended by (5,-5,7), and the result blended again by
        # (3,2,-1): both deltas reach the moveto's dx.
        100, 5, -5, 7, 1, "blend", 3, 2, -1, 1, "blend", 100, "rmoveto",
        # rrcurveto's six operands with two separate n=1 blends.
        50, 10, 0, 0, 1, "blend", 50, 50, 0, 0, -10, 20, 0, 5, 1, "blend", "rrcurveto",
        -100, -100, "rlineto",
    ],
    "curves": [
        100, 100, "rmoveto",
        50, 20, 30, 40, "hvcurveto",
        40, 20, 30, 50, 10, "vhcurveto",
        50, 20, 30, 40, 40, 20, 30, 50, "hvcurveto",
        40, 20, 30, 50, 50, 20, 30, 40, 10, "vhcurveto",
        50, 20, 30, 40, 40, 20, 30, 50, 50, 20, 30, 40, "hvcurveto",
        40, 20, 30, 50, 50, 20, 30, 40, 40, 20, 30, 50, 10, "vhcurveto",
        5, 30, 20, 30, 40, "hhcurveto",
        5, 30, 20, 30, 40, "vvcurveto",
        30, 10, 20, 20, 10, 30, 40, 0, "rcurveline",
        20, 0, 0, 20, 10, 10, 20, 20, 10, 30, "rlinecurve",
        -300, -300, "rlineto",
    ],
    "moves": [
        50, 50, "rmoveto",
        100, 100, "rmoveto", 200, 200, -200, "hlineto",
    ],
}

gsubrs = [
    [200, 100, 40, -20, 5, 0, 0, 0, 2, "blend"],
    [0, 100, "rlineto"],
]
# FD 0's local subroutines: 0 draws and calls 1, 1 calls gsubr 1.
lsubrs0 = [
    [100, 20, 0, 0, 1, "blend", 0, 0, 100, "rlineto", 1 - BIAS, "callsubr"],
    [100, 0, "rlineto", 1 - BIAS, "callgsubr"],
]
# FD 1's: one subroutine that blends with its Private DICT's k = 1.
lsubrs1 = [
    [200, 30, 1, "blend", "vlineto", 100, -10, 1, "blend", "hlineto"],
]

fb = FontBuilder(1000, isTTF=False)
fb.setupGlyphOrder(order)
fb.setupCharacterMap({0x41 + i: n for i, n in enumerate(order[1:])})
fb.setupHorizontalMetrics({n: (800, 0) for n in order})
fb.setupHorizontalHeader(ascent=800, descent=-200)
fb.setupNameTable({"familyName": "baud CFF2 cases", "styleName": "Regular"})
fb.setupOS2()
fb.setupPost()
fb.setupFvar(
    [("wght", 100, 400, 900, "Weight"), ("wdth", 50, 100, 200, "Width")],
    [],
)

charstrings = {n: T2CharString(program=list(p)) for n, p in programs.items()}
fb.setupCFF2(charstrings, fdArrayList=[{}, {}])

cff = fb.font["CFF2"].cff
top = cff.topDictIndex[0]
for p in gsubrs:
    cff.GlobalSubrs.append(T2CharString(program=list(p)))

regions = [
    {"wght": (0, 1.0, 1.0)},
    {"wght": (-1.0, -1.0, 0)},
    {"wdth": (0, 1.0, 1.0)},
    {"wght": (0, 0.5, 1.0)},
    {"wght": (0, 1.0, 1.0), "wdth": (0, 1.0, 1.0)},
]
store = buildVarStore(
    buildVarRegionList(regions, ["wght", "wdth"]),
    [
        buildVarData([0, 1, 2], None, optimize=False),
        buildVarData([3, 4], None, optimize=False),
        buildVarData([2], None, optimize=False),
    ],
)
vstore = VarStoreData(otVarStore=store)
top.VarStore = vstore

for i, (fd, subrs) in enumerate(zip(top.FDArray, [lsubrs0, lsubrs1])):
    private = fd.Private
    private.vstore = vstore
    index = SubrsIndex()
    for p in subrs:
        index.append(T2CharString(program=list(p), private=private, globalSubrs=cff.GlobalSubrs))
    private.Subrs = index
    if i == 1:
        private.vsindex = 2
    else:
        # Seven blended BlueValues pairs: 14 + 14 * 3 + 1 = 57 operands
        # before one DICT operator, past CFF's limit of 48 and within
        # CFF2's 513. A reader that keeps CFF's limit loses every key the
        # scan meets after it.
        private.BlueValues = [[v, 2, -1, 1] for v in (-12, 0, 450, 462, 500, 512, 600, 612, 650, 662, 700, 712, 750, 762)]

select = FDSelect(format=4)
for n in order:
    select.append(1 if n == "fd1" else 0)
top.FDSelect = select
for n in order:
    cs = top.CharStrings[n]
    cs.private = top.FDArray[select[order.index(n)]].Private
    cs.globalSubrs = cff.GlobalSubrs
top.CharStrings.charStringsAreIndexed = False

fb.font["head"].created = 0
fb.font["head"].modified = 0
fb.save(OUT)
print(OUT)
