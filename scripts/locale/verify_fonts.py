# Independent spot-check (fontTools, a third reader) of the Hebrew coverage
# claims. Positive controls: each face must map its own control code point;
# SF Hebrew and Courier New must map Hebrew, else the reader is broken.
import sys
from pathlib import Path
from fontTools.ttLib import TTFont, TTCollection

LETTERS = range(0x05D0, 0x05EB)   # 27
POINTS = range(0x05B0, 0x05C8)    # 24
# The checkout this script is in, wherever it was cloned.
REPO = str(Path(__file__).resolve().parents[2])
LIB = "/nix/store/pfzjm3l816ma7v047d7nxq1q8p3hqp5r-liberation-fonts-2.1.5/share/fonts/truetype/LiberationMono-Regular.ttf"
FACES = [
    ("FiraCode NFM (owner primary)", "/Library/Fonts/Nix Fonts/xpks2i31dv5v2cpyp13ccwrfyhnzczyn-nerd-fonts-fira-code-3.5.0+6.2/share/fonts/truetype/NerdFonts/FiraCode/FiraCodeNerdFontMono-Regular.ttf", None, 0x41),
    ("Hiragino Sans GB @0", "/System/Library/Fonts/Hiragino Sans GB.ttc", 0, 0x4E00),
    ("Apple SD Gothic Neo @0", "/System/Library/Fonts/AppleSDGothicNeo.ttc", 0, 0xAC00),
    ("Apple Color Emoji @0", "/System/Library/Fonts/Apple Color Emoji.ttc", 0, 0x1F600),
    ("Apple Symbols", "/System/Library/Fonts/Apple Symbols.ttf", None, 0x2600),
    # Stock JetBrains Mono, as embedded when this check was run. The embedded
    # Regular is now baud-app/fonts/built/Regular.ttf, this file with Hebrew added.
    ("JetBrains Mono Regular (stock)", REPO + "/baud-app/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf", None, 0x41),
    ("STIX Two Math (embedded)", REPO + "/baud-app/fonts/stix-two-math/STIXTwoMath-Regular.otf", None, 0x23F5),
    ("Symbols NFM (embedded)", REPO + "/baud-app/fonts/symbols-nerd-font/SymbolsNerdFontMono-Regular.ttf", None, 0xF07B),
    ("Menlo @0", "/System/Library/Fonts/Menlo.ttc", 0, 0x41),
    ("CONTROL SF Hebrew", "/System/Library/Fonts/SFHebrew.ttf", None, 0x05D0),
    ("CONTROL Courier New", "/System/Library/Fonts/Supplemental/Courier New.ttf", None, 0x41),
    ("Liberation Mono 2.1.5", LIB, None, 0x41),
]

def load(path, idx):
    if idx is None:
        return TTFont(path, lazy=True)
    return TTCollection(path, lazy=True).fonts[idx]

for name, path, idx, ctl in FACES:
    try:
        f = load(path, idx)
        cmap = f.getBestCmap() or {}
    except Exception as e:
        print(f"{name}: LOAD FAILED {e!r}")
        continue
    nl = sum(1 for c in LETTERS if c in cmap)
    npts = sum(1 for c in POINTS if c in cmap)
    fffd = 0xFFFD in cmap
    print(f"{name}: letters {nl}/27 points {npts}/24 control U+{ctl:04X} {'found' if ctl in cmap else 'MISSING'} U+05FF {'MAPPED?!' if 0x05FF in cmap else 'absent'} U+FFFD {'mapped' if fffd else 'absent'} fsType {f['OS/2'].fsType if 'OS/2' in f else '-'}")

# Liberation Mono: widths of Hebrew letters/points vs Latin, and the licence.
f = TTFont(LIB)
upem = f['head'].unitsPerEm
hm = f['hmtx'].metrics
cmap = f.getBestCmap()
def adv(c):
    return hm[cmap[c]][0] if c in cmap else None
lat = {adv(c) for c in range(0x21, 0x7F)}
heb = {adv(c) for c in LETTERS}
pts = {adv(c) for c in POINTS}
print("Liberation upem", upem, "latin advances", sorted(lat), "hebrew letter advances", sorted(heb), "point advances", sorted(pts))
print("Liberation post.isFixedPitch", f['post'].isFixedPitch, "panose proportion", f['OS/2'].panose.bProportion)
for rec in f['name'].names:
    if rec.nameID in (5, 13, 14) and rec.platformID == 3:
        print("Liberation name", rec.nameID, ":", rec.toUnicode()[:160])
import os
print("Liberation size", os.path.getsize(LIB))
