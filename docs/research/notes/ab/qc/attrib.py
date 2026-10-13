import json, sys, re, collections, unicodedata
from fontTools.ttLib import TTFont
Q = sys.argv[1]
R = json.load(open(f"{Q}/measure.json"))
corp = {n: json.load(open(f"{Q}/corpus-{n}-occ.json")) for n in ("gen5", "pango")}
FACES = {"built Regular": ("baud-app/fonts/built/Regular.ttf", 0), "built Bold": ("baud-app/fonts/built/Bold.ttf", 0),
 "Courier New": ("/System/Library/Fonts/Supplemental/Courier New.ttf", 0), "Courier New Bold": ("/System/Library/Fonts/Supplemental/Courier New Bold.ttf", 0),
 "Arial Hebrew": ("/System/Library/Fonts/ArialHB.ttc", 0), "Arial Hebrew Scholar": ("/System/Library/Fonts/ArialHB.ttc", 6),
 "SF Hebrew": ("/System/Library/Fonts/SFHebrew.ttf", 0), "SF Hebrew Rounded": ("/System/Library/Fonts/SFHebrewRounded.ttf", 0),
 "New Peninim MT": ("/System/Library/Fonts/Supplemental/NewPeninimMT.ttc", 0), "Raanana": ("/System/Library/Fonts/Supplemental/Raanana.ttc", 0),
 "Corsiva Hebrew": ("/System/Library/Fonts/Supplemental/Corsiva.ttc", 0), "Lucida Grande": ("/System/Library/Fonts/LucidaGrande.ttc", 0),
 "Arial Unicode MS": ("/Library/Fonts/Arial Unicode.ttf", 0)}
G = re.compile(r"(\d+)=")
print("| face | GDEF glyph classes | FB1D-FB4F forms mapped | regressions C1=hb, C2!=hb (gen5 / pango) | fixes C1!=hb, C2=hb (gen5 / pango) | hb glyph is an FB presentation form: clusters (gen5 / pango) | of those, order-differs |")
print("|---|---|---|---|---|---|---|")
for face, per in R.items():
    path, idx = FACES[face]
    f = TTFont(path, fontNumber=idx, lazy=True)
    cm = f.getBestCmap() or {}; order = f.getGlyphOrder()
    fb = {order.index(cm[u]) for u in range(0xFB1D, 0xFB50) if u in cm}
    gdef = "GDEF" in f and f["GDEF"].table.GlyphClassDef is not None
    out = []
    for n in ("gen5", "pango"):
        ks = corp[n]
        reg = sum(1 for k in ks if per[k]["c1"]["ink_equal"] and not per[k]["c2"]["ink_equal"])
        fix = sum(1 for k in ks if not per[k]["c1"]["ink_equal"] and per[k]["c2"]["ink_equal"])
        pf = [k for k in ks if any(int(g) in fb for g in G.findall(per[k]["hb"]))]
        out.append((reg, fix, len(pf), sum(1 for k in pf if per[k]["differs"])))
    print(f"| {face} | {'yes' if gdef else 'no'} | {len(fb)} | {out[0][0]} / {out[1][0]} | {out[0][1]} / {out[1][1]} | {out[0][2]} / {out[1][2]} | {out[0][3]} / {out[1][3]} |")
print()
print("| face | corpus | C2 != hb clusters | of which hb has an FB form glyph that C2 lacks | of the rest: C2 ids = hb (position-only) | of the rest: ids differ |")
print("|---|---|---|---|---|---|")
for face, per in R.items():
    path, idx = FACES[face]
    f = TTFont(path, fontNumber=idx, lazy=True)
    cm = f.getBestCmap() or {}; order = f.getGlyphOrder()
    fb = {order.index(cm[u]) for u in range(0xFB1D, 0xFB50) if u in cm}
    for n in ("gen5", "pango"):
        bad = [k for k in corp[n] if not per[k]["c2"]["ink_equal"]]
        pf = [k for k in bad if {int(g) for g in G.findall(per[k]["hb"])} & fb - {int(g) for g in G.findall(per[k]["c2"]["raw"])}]
        rest = [k for k in bad if k not in pf]
        print(f"| {face} | {n} | {len(bad)} | {len(pf)} | {sum(1 for k in rest if per[k]['c2']['ids_equal'])} | {sum(1 for k in rest if not per[k]['c2']['ids_equal'])} |")
