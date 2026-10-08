"""The STYLE parameter: one build path for Regular, Bold, Italic and Bold Italic.

  STYLE=Regular|Bold|Italic|BoldItalic   LEAN=R|L (Italic and Bold Italic only)

Every style is an UPRIGHT design (Regular or Bold) plus, for the two italics,
one shear applied after the anchor search to every outline and every anchor.
The upright Regular is round 4 unchanged: `apply` changes nothing for it, and
the Regular build is gated table-for-table against r4/final.

Bold's constants are re-derived from JetBrains Mono Bold's own glyphs
(jbm_stems.py, jbm_desc.py; values quoted where they are set):
  stems   H/n/o vertical 125, H crossbar 113 (E 110/106, T 113)     Regular 90/82
  dot     dotbelowcomb 164 x 145, ink y -225..-80, centre x -300    Regular 142 x 128
  marks   U+0307 bottom 642 (Regular 649), U+0323 top -80 (Regular -85)
  p q y g j descend to -180, as Regular
Cascadia's variable font tops out at wght 700, which is also the weight whose
Hebrew stems come closest to JBM Bold's (cas_weights.py: vertical 258 Cascadia
units = 120.3 in the Regular's x frame against 125; horizontal 270), so Bold
starts from the 700 instance and re-weights only the remainder: DX dilates the
verticals 258 -> 268 (x 90/193 = 125.0), EY erodes the horizontals so that
(270 - EY) * 630 / (1200 - EY) = 113.

baud's copy (bold2/style.py in the research tree): only the paths changed, to
ones relative to this file (HERE, HEBREW, JDIR, WORK below); see build.py's
docstring. Names such as r4/final, fix/, jbm_stems.py and cas_weights.py cite
the research tree, which is not committed.
"""
import os, math

STYLE = os.environ.get('STYLE', 'Regular')
LEAN = os.environ.get('LEAN', 'R')
if STYLE not in ('Regular', 'Bold', 'Italic', 'BoldItalic'):
    raise SystemExit('STYLE must be Regular, Bold, Italic or BoldItalic, not %r' % STYLE)
if LEAN not in ('R', 'L'):
    raise SystemExit('LEAN must be R or L')
UPRIGHT = 'Bold' if STYLE in ('Bold', 'BoldItalic') else 'Regular'
ITALIC = STYLE in ('Italic', 'BoldItalic')
# baud: every path is relative to this file. HERE is this build/ directory and
# HEBREW the committed input directory above it (baud-app/fonts/glyphs/languages/hebrew/);
# the stock JetBrains Mono base stays at baud-app/fonts/jetbrains-mono/. The Cascadia
# instances are not committed: scripts/build-fonts.sh makes them from HEBREW's variable
# font into the directory HEBREW_WORK names (cas400.ttf, cas700.ttf).
HERE = os.path.dirname(os.path.abspath(__file__)) + '/'
HEBREW = os.path.dirname(os.path.dirname(os.path.abspath(__file__))) + '/'
JDIR = os.path.normpath(os.path.join(HEBREW, '..', '..', '..', 'jetbrains-mono')) + '/'
WORK = os.environ.get('HEBREW_WORK', '')
if not WORK:
    raise SystemExit('HEBREW_WORK must name the directory holding cas400.ttf and cas700.ttf (scripts/build-fonts.sh)')
WORK = os.path.join(WORK, '')
JBM_TARGET = JDIR + 'JetBrainsMono-%s.ttf' % STYLE        # the file the Hebrew is merged into
JBM_UPRIGHT = JDIR + 'JetBrainsMono-%s.ttf' % UPRIGHT     # whose dot, colon and mark heights the upright design takes

# ---------------------------------------------------------------- the italic shear
# x' = x + SLANT * tan(9 deg) * (y - SHEAR_Y0): RIGHT leans the top right, as
# JBM Italic's Latin (post.italicAngle -9.0); LEFT leans it left, with the
# reading direction. SHEAR_Y0 = 315, half the Hebrew letter height (630): a
# letter's mid-height stays where the upright put it (centred on x = 300) and
# its top and bottom move by +-49.9 units, so the sheared letter stays centred
# in its cell. (JBM Italic's own marks imply an origin near y = 274, half its
# x-height; Hebrew has no x-height, and 274 would put every Hebrew letter's
# middle 6.4 units right of the cell centre.)
TAN = math.tan(math.radians(9.0))
SLANT = 0 if not ITALIC else (1 if LEAN == 'R' else -1)
SHEAR_Y0 = 315.0


def shear_xy(x, y):
    return (x + SLANT * TAN * (y - SHEAR_Y0), y)


def suffix():
    """File and nameID-3 tag of this style."""
    return STYLE + ('-' + {'R': 'right', 'L': 'left'}[LEAN] if ITALIC else '')


def apply(D, E, N):
    """Set the upright weight's constants on design (D), design3 (E) and narrow (N)."""
    E.TWO_ROW = (0x05B0, 0x05B1, 0x05B2, 0x05B3)   # sheva and the hatafs: what round 4's 'h > 245' meant
    # fix-bi: the two-row points' HOME floor (unattached, and every non-dipping letter) apart from the
    # single-row floor LOWEST; in Regular they are the same (-275), so Regular is unchanged
    E.LOWEST_TWO = float(os.environ.get('LOWEST_TWO', E.LOWEST))   # fix-bi: env only for the italic floor search
    # fix-bi: a hataf's bar never rises above its sheva's top dot (Regular's centred bar sits 11.3 below it,
    # so the clamp changes nothing there)
    E.HATAF_BAR_MIN_DROP = 0.0
    # fix-bi: Regular searches round 2's DAGESH_BOX only; Bold widens the search where the box holds no place
    E.DAGESH_WIDEN = False
    # fix-bi: point -> home top, overriding the floor rule (Bold's segol and qubuts; empty elsewhere, so Regular and
    # Italic are unchanged)
    E.MULTI_ROW_HOME = {}
    E.DAGESH_NEED = {}                              # fix-bi: letter -> clearance the box must give before widening (default GRID_MIN + 2)
    E.DAGESH_W = E.DOT_W                            # Regular: the dagesh is the dot
    E.DOT_H = None                                  # bold-sheva-r2: the vowel dot's height; None = JBM's own proportions
    E.STEM_V = 90.0                                 # the vertical stem XSCALE erodes back
    E.CAS = WORK + 'cas400.ttf'                     # instanced from HEBREW's CascadiaMono.ttf at wght 400 (the research tree's cas400gh.ttf)
    E.CAS_ANCHORS_FILE = HEBREW + 'anchors-400.json'  # the research tree's cas-anchors.json, byte for byte
    E.DESC_MEASURED = False                         # Regular: round 2's constant -252
    if UPRIGHT == 'Regular' and os.environ.get('REG_DOT_H'):   # bold-sheva-r2 round 3: a Regular dash CANDIDATE only
        E.DOT_H = float(os.environ['REG_DOT_H'])
        E.DAGESH_W = float(os.environ.get('REG_DAGESH_W', 116))
    if UPRIGHT == 'Regular':
        if ITALIC and 'LOWEST_TWO' not in os.environ:
            # fix-bi: the italic's own floors, from the real-render floor search (fix/floorsearch.sh, .py;
            # out/floorsearch.json): first clean (<= 40, col1 and 2-D) on all ten dipping cells at 28 px, then the
            # fewest sizes in 22-56 px with any dipping cell's 2-D above 40, then the smallest sum of the per-size
            # maxima. Copied Regular floors (-275 / -293) read 74 (right) and 72 (left) at 28 px.
            E.LOWEST_TWO, E.LOWEST_DIP = {'R': (-281.0, -300.0), 'L': (-279.0, -299.0)}[LEAN]
        return
    # ---- Bold
    E.CAS = WORK + 'cas700.ttf'                      # instanced from HEBREW's CascadiaMono.ttf at wght 700
    E.CAS_ANCHORS_FILE = HEBREW + 'anchors-700.json'
    E.STEM_V = float(os.environ.get('BOLD_STEM', 125))
    D.DX = float(os.environ.get('BOLD_DX', 10))      # 258 + 10 = 268 Cascadia units = 125.0 at x 90/193
    D.EY = float(os.environ.get('BOLD_EY', 66.7))    # (270 - EY) * 630 / (1200 - EY) = 113
    D.SY = 630 / (1200 - D.EY)
    E.DESC_MEASURED = True                          # descender bottoms read from the bold outline
    # points: JBM Bold's dot is 164 wide (Regular 142), so 116 * 164 / 142
    # fix-bi: the vowel dot. 134 (= 116 x 164/142, JBM Bold's dot ratio) makes the sheva 267 tall, which cannot
    # keep a letter gap under the dipping letters inside the cell's 8 pixel rows below the baseline at 28 px
    # (with the dipping bottom >= -300, a home at or below -33 and a push of >= 21 units, the two-row point must be
    # <= ~246 tall; 120 / GV 30 gives 242). Chosen with the in-memory simulator (fix/sim.py, validated within 3 levels
    # on 414 cells at unseen sizes; fix/sweep3*.py, candB.py, candDD.py); every reported figure is from heb-render.
    # One dot for every Bold vowel point, the hatafs' dots included.
    E.DOT_W = float(os.environ.get('BOLD_DOT_W', 120))
    # bold-sheva-r2: a vowel dot flatter than JBM Bold's own (164 x 145, w/h 1.131), so the two-row points free
    # vertical room without losing weight; unset = the fix round's uniform scaling (byte-identical builds)
    E.DOT_H = float(os.environ['BOLD_DOT_H']) if os.environ.get('BOLD_DOT_H') else None
    # the dagesh sits inside a counter, and the bold strokes narrow every counter by ~50 units (counters.py:
    # tav 173 -> 126, mem 174 -> 121, he 227 -> 176): JBM Bold's 134 dot overlaps tav, mem, ayin, alef by 1
    # unit or more (dbg_dagesh.py). So the Bold dagesh keeps the Regular's dot, 116 wide, and tav and mem are
    # narrowed 40 units less than in Regular (counter +40)
    # fix-bi: 116 leaves the bold shin at 21.8 and ayin at 7 wherever it goes (fix/dagesh_room.py). 104 is chosen
    # with about one unit of margin: 106 also passes (text min 27.0 Bold, 25.7 Bold Italic right; ayin 13 inside the
    # letter), 108 fails Bold Italic right (shin 24.6) and pushes alef and ayin outside the letter (fix/cand/*dagesh*)
    E.DAGESH_W = float(os.environ.get('BOLD_DAGESH_W', 104))
    E.DAGESH_WIDEN = os.environ.get('BOLD_DAGESH_WIDEN', '1') == '1'
    relief = float(os.environ.get('BOLD_RELIEF', 40))  # tav and mem: narrowed this much less than Regular, for the counter
    D.EDITS = dict(D.EDITS)
    D.EDITS[0x05EA] = [(340, -65 + relief, '>')]
    D.EDITS[0x05DE] = [(320, -50 + relief, '>')]
    E.BAR = float(os.environ.get('BOLD_BAR', 113))  # JBM Bold's horizontal stroke (H crossbar, T)
    E.METEG = (E.BAR, 150)                          # Regular's meteg is BAR wide
    E.PASEQ_W = 60 * 125 / 90                       # lighter than vav's stem by Regular's ratio
    E.QAM_H = float(os.environ.get('BOLD_QAM_H', 190 + (E.BAR - 82)))   # qamats: the stem under the bar keeps Regular's 108 (bold-sheva-r2 round 3: env, Job 2 candidates only)
    # fix-bi: qamats qatan 267 put its top at -33 on the -300 floor, 3 units above every other below point
    # (BELOW_TOP -36) and 22 from a dipping letter's bottom: 66 at 56 px where Regular reads 16. 264 puts its
    # top on BELOW_TOP like qamats; its stem is 151 (Regular 154)
    E.QQ_H = float(os.environ.get('BOLD_QQ_H', 264))
    E.GV = float(os.environ.get("BOLD_GV", 30))       # fix/sweep3.py at 16-56 px
    # fix-bi: the first segol pitch and qubuts step that keep every dot gap >= 40 for the 120 dot
    # (dotlayout_style.py Bold 120 50: segol pitch 128, gap 40.43; qubuts at Regular's pitch 134.4, dx 146, gap 40.65)
    E.SEG_PITCH = float(os.environ.get('BOLD_SEG_PITCH', 128))
    E.QUB_DX = float(os.environ.get('BOLD_QUB_DX', 146))
    E.QUB_PITCH = float(os.environ.get('BOLD_QUB_PITCH', 134.4))
    E.LOWEST = float(os.environ.get("BOLD_LOWEST", -300))   # single-row points: no ink below the cell bottom
    # fix-bi: the two-row points get Regular's two floors back. Their home (unattached, and on every letter
    # that does not dip) sits LOWEST_TWO + h; under ג ט ל ס ש they attach LOWEST_DIP + h, X units lower, which
    # baud draws one whole pixel lower from px >= 500 / X (X 22: 23-67 px). Regular's own split is 18 units
    # (-275 / -293): one pixel only from 28 px. The dipping bottom stays >= -300.
    # chosen per style by the same real-render floor search as the italics (fix/floorsearch.*): Bold -278 / -300
    # (home -36, push 22), Bold Italic (both leans) -277 / -300 (home -35, push 23)
    LT0, LD0 = (-277.0, -300.0) if ITALIC else (-278.0, -300.0)
    E.LOWEST_TWO = float(os.environ.get('BOLD_LOWEST_TWO', LT0))
    E.LOWEST_DIP = float(os.environ.get('BOLD_LOWEST_DIP', LD0))
    # fix-bi: segol (234) and qubuts (240.5) would sit, unattached, on the -300 floor (homes -66 / -60), 30 / 24 units
    # below where they attach (-36); baud rounds that offset to whole pixels and at 28 and 56 px draws them about 6
    # units ABOVE -36 (segol under a dipping letter 105 -> 118 at 28 px, 16 -> 96 at 56: fix/allbelow_real.py). They
    # keep the builder's Bold homes, -42 (= Regular's segol home): 6 units below -36, which rounds to no move up to 83 px
    E.MULTI_ROW_HOME = {0x05B6: float(os.environ.get('BOLD_SEG_HOME', -42)), 0x05BB: float(os.environ.get('BOLD_QUB_HOME', -42))}   # bold-sheva-r2: env
    # bold-sheva-r2: the hatafs' horizontal gaps (round 2's 40 / 40), env so hataf segol's within-row fusion can be screened
    E.HSEG_GH = float(os.environ.get('BOLD_HSEG_GH', E.HSEG_GH))
    E.HATAF_GAP = float(os.environ.get('BOLD_HATAF_GAP', E.HATAF_GAP))
    E.HATAF_BAR_MIN_DROP = float(os.environ.get('BOLD_HATAF_BAR_MIN_DROP', 3))   # fix-bi: the bar's flat top 3 below the round dot's (simulator, fix/candB.py)
    E.BELOW_TOP = float(os.environ.get('BOLD_BELOW_TOP', -36))
    E.HOME_ABOVE_BOTTOM = 642                       # JBM Bold U+0307 ink 642..786
    E.HOME_BELOW_TOP = -80                          # JBM Bold U+0323 ink -225..-80
    E.TAV_LEG_DX = float(os.environ.get('BOLD_TAV_LEG_DX', -12))
    lx = os.environ.get('BOLD_L2')
    E.L2_ANCHOR = tuple(map(float, lx.split(','))) if lx else (round(E.DOT_W / 2), 825)
    # narrow letters, built from rectangles: Bold's stem and bar
    N.STEM, N.BAR = E.STEM_V, E.BAR
    N.HEAD_Y0 = N.TOP - N.BAR
    N.KOTZ = dict(depth=round(54 * E.BAR / 82), run=150)
    N.HEAD_ADD = float(os.environ.get('BOLD_HEAD_ADD', 318 - 292))   # Bold's variant-A vav head (narrowstems.py) minus Regular's
