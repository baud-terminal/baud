# What the C2 != hb-shape clusters whose glyph ids differ hold, on Arial Hebrew
# and New Peninim MT (attrib.md's "ids differ" column). Reads measure.json.gz and
# the two -occ.json files beside it; writes to stdout (residuals.txt).
import gzip, json, os, re

D = os.path.dirname(os.path.abspath(__file__))
R = json.load(gzip.open(os.path.join(D, "measure.json.gz"), "rt"))
corp = {n: json.load(open(os.path.join(D, f"corpus-{n}-occ.json"))) for n in ("gen5", "pango")}
G = re.compile(r"(\d+)=")


def marks(k):
    return [int(x, 16) for x in k.split()[1:]]


def cant(c):
    # cantillation U+0591-05AE, and meteg
    return 0x0591 <= c <= 0x05AE or c == 0x05BD


for face in ("Arial Hebrew", "New Peninim MT"):
    per = R[face]
    for n in ("gen5", "pango"):
        bad = [k for k in corp[n] if not per[k]["c2"]["ink_equal"] and not per[k]["c2"]["ids_equal"]]
        g268 = sum(1 for k in bad if "268" in G.findall(per[k]["c2"]["raw"]))
        mc = sum(1 for k in bad if any(cant(c) for c in marks(k)))
        od = sum(1 for k in bad if per[k]["differs"])
        same = sum(1 for k in bad if per[k]["c1_eq_c2"])
        nc2 = lambda k: len(G.findall(per[k]["c2"]["raw"]))
        nhb = lambda k: len(G.findall(per[k]["hb"]))
        fewer = sum(1 for k in bad if nhb(k) < nc2(k))
        more = sum(1 for k in bad if nhb(k) > nc2(k))
        print(
            f"{face} {n}: ids differ {len(bad)}; C2 holds glyph 268: {g268}; "
            f"holds meteg or a cantillation mark: {mc}; order-differs: {od}; C1 output = C2: {same}; "
            f"hb-shape fewer glyphs than C2: {fewer}; more: {more}"
        )
k = "05E8 05B5"
print(f"example, New Peninim MT {k}: C2 {R['New Peninim MT'][k]['c2']['raw']}, hb {R['New Peninim MT'][k]['hb']}")
