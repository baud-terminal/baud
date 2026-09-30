"""fontTools' instances of the committed variable fonts, as the oracle for
baud-font's build-time bake (note Z, step 5).

Writes baud-font/tests/var/bake/<file>-<wght>.ttf: JetBrains Mono upright
and italic at wght 400 and 700 -- the four instances baud embeds -- made by
fontTools' instancer with its defaults (overlap flags set, font names left
as they are), the timestamp left as the variable file's so the output is
deterministic. tests/bake.rs compares baud's bake against each glyph,
metric and anchor.

    nix shell --impure --expr 'with import (builtins.getFlake "nixpkgs") {}; \\
      python3.withPackages (p: [ p.fonttools ])' -c python3 scripts/bake-oracle.py
"""
import os
import fontTools
from fontTools.ttLib import TTFont
from fontTools.varLib import instancer

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
TESTS = os.path.join(ROOT, "baud-font/tests")
OUT = os.path.join(TESTS, "var/bake")
os.makedirs(OUT, exist_ok=True)
for name in ("JetBrainsMono-Variable", "JetBrainsMono-Italic-Variable"):
    for wght in (400, 700):
        font = TTFont(os.path.join(TESTS, f"fonts/{name}.ttf"), recalcTimestamp=False)
        inst = instancer.instantiateVariableFont(font, {"wght": wght})
        inst.recalcTimestamp = False
        path = os.path.join(OUT, f"{name}-{wght}.ttf")
        inst.save(path)
        print(path, os.path.getsize(path))
with open(os.path.join(OUT, "VERSION"), "w") as f:
    f.write(f"fontTools {fontTools.version}\n")
