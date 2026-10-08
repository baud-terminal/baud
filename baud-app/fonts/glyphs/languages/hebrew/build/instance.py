"""Instance Cascadia Mono's variable font at one weight.

  instance.py VF WGHT OUT.ttf

The same call the research tree made its two instances with (inst_gh.py at
wght 400, cas_weights.py at wght 700): fontTools' instancer on the whole font,
saved. With SOURCE_DATE_EPOCH set, wght 400 and 700 come out byte-identical to
the instances the gated fonts were built from; scripts/build-fonts.sh records
both hashes in baud-app/fonts/built/MANIFEST and checks them. Only the save is
pinned (pin.py).
"""
import sys
from fontTools.ttLib import TTFont
from fontTools.varLib import instancer
from pin import save_pinned

vf, wght, out = sys.argv[1], int(sys.argv[2]), sys.argv[3]
save_pinned(instancer.instantiateVariableFont(TTFont(vf), {'wght': wght}), out)
