import sys, json, re, subprocess, collections, os
REPO = str(__import__('pathlib').Path(__file__).resolve().parents[5])
from fontTools.ttLib import TTFont
from fontTools.pens.boundsPen import BoundsPen
Q = sys.argv[1]
H = "/nix/store/lk0d3svdv7gqk6472n4n1zr6nip1jd57-harfbuzz-13.2.1-dev/bin/hb-shape"
FEAT = "-abvm,-blwm,-locl,-rlig,-clig,-curs,-dist,-kern,-liga,-rclt,-rvrn,-dlig,-hlig"
FACES = [
 (REPO + "/baud-app/fonts/built/Regular.ttf", 0, "built Regular"),
 (REPO + "/baud-app/fonts/built/Bold.ttf", 0, "built Bold"),
 ("/System/Library/Fonts/Supplemental/Courier New.ttf", 0, "Courier New"),
 ("/System/Library/Fonts/Supplemental/Courier New Bold.ttf", 0, "Courier New Bold"),
 ("/System/Library/Fonts/ArialHB.ttc", 0, "Arial Hebrew"),
 ("/System/Library/Fonts/ArialHB.ttc", 6, "Arial Hebrew Scholar"),
 ("/System/Library/Fonts/SFHebrew.ttf", 0, "SF Hebrew"),
 ("/System/Library/Fonts/SFHebrewRounded.ttf", 0, "SF Hebrew Rounded"),
 ("/System/Library/Fonts/Supplemental/NewPeninimMT.ttc", 0, "New Peninim MT"),
 ("/System/Library/Fonts/Supplemental/Raanana.ttc", 0, "Raanana"),
 ("/System/Library/Fonts/Supplemental/Corsiva.ttc", 0, "Corsiva Hebrew"),
 ("/System/Library/Fonts/LucidaGrande.ttc", 0, "Lucida Grande"),
 ("/Library/Fonts/Arial Unicode.ttf", 0, "Arial Unicode MS"),
]
SBL = {10:22,11:15,12:16,13:17,14:23,15:18,16:19,17:20,18:21,19:14,20:24,21:12,22:25,23:13,24:10,25:11,26:26}
import unicodedata
def ccc(c): return unicodedata.combining(chr(c))
def order_differs(cps):
    m = cps[1:]
    return sorted(m, key=ccc) != sorted(m, key=lambda c: SBL.get(ccc(c), ccc(c)))
corp = {}
for name in ("gen5", "pango"):
    corp[name] = json.load(open(f"{Q}/corpus-{name}-occ.json"))
allc = sorted(set(corp["gen5"]) | set(corp["pango"]), key=lambda k: [int(x,16) for x in k.split()])
cfile = f"{Q}/clusters-all.txt"
open(cfile, "w").write("\n".join(allc) + "\n")
tfile = f"{Q}/clusters-all.utf8.txt"
open(tfile, "w", encoding="utf-8").write("\n".join("".join(chr(int(x,16)) for x in k.split()) for k in allc) + "\n")
G = re.compile(r"(\d+)=(\d+)(?:@(-?\d+),(-?\d+))?\+(-?\d+)")
def parse_hb(s, rtl):
    gl = [tuple(int(v) if v is not None else 0 for v in m.groups()) for m in G.finditer(s)]
    pen = 0; placed = []
    for gid, cl, dx, dy, ax in gl:
        placed.append((gid, pen + dx, dy, ax)); pen += ax
    if rtl: placed.reverse()
    left = min([p[1] - (p[1]) for p in placed] or [0])  # single cluster: left edge is pen 0
    return [(g, x, y, ax) for g, x, y, ax in placed], pen
def parse_baud(s):
    return [tuple(int(v) for v in m.groups()) for m in G.finditer(s)]
results = {}
for path, idx, label in FACES:
    f = TTFont(path, fontNumber=idx, lazy=True)
    order = f.getGlyphOrder(); gs = f.getGlyphSet()
    bcache = {}
    def center(gid):
        if gid in bcache: return bcache[gid]
        pen = BoundsPen(gs)
        try: gs[order[gid]].draw(pen)
        except Exception: pass
        b = pen.bounds
        bcache[gid] = None if b is None else (b[0] + b[2]) / 2
        return bcache[gid]
    hb = subprocess.run([H, "--no-glyph-names", f"--features={FEAT}", f"--face-index={idx}", f"--text-file={tfile}", path],
        capture_output=True, text=True, check=True).stdout.strip().split("\n")
    arms = {}
    for arm in ("c1", "c2"):
        out = subprocess.run([f"{Q}/bin/qc-{arm}", "shape", path, str(idx), cfile], capture_output=True, text=True, check=True).stdout.strip().split("\n")
        arms[arm] = out
    assert len(hb) == len(allc) == len(arms["c1"]) == len(arms["c2"]), (label, len(hb), len(allc))
    per = {}
    for i, k in enumerate(allc):
        cps = [int(x,16) for x in k.split()]
        rtl = any(0x0590 <= c <= 0x05FF or 0xFB1D <= c <= 0xFB4F for c in cps)
        want, W_hb = parse_hb(hb[i], rtl)
        row = {"hb": hb[i]}
        def outside_hb():
            n = 0
            for g, x, y, ax in want:
                if ax == 0:
                    c = center(g)
                    if c is not None and not (0 <= x + c < W_hb): n += 1
            return n
        row["hb_out"] = outside_hb()
        for arm in ("c1", "c2"):
            line = arms[arm][i].split("\t")
            assert line[0] == k
            got = parse_baud(line[1])
            meta = dict(kv.split("=") for kv in line[2].split())
            W = got[0][4] if got else 0
            outside = 0
            for j, (g, cl, dx, dy, adv) in enumerate(got):
                if j == 0: continue
                c = center(g)
                if c is not None and not (0 <= dx + c < W): outside += 1
            gp = [(g, dx, dy) for g, cl, dx, dy, adv in got]
            wp = [(g, x, y) for g, x, y, ax in want]
            row[arm] = dict(raw=line[1], exact=gp == wp, ink_equal=sorted(gp) == sorted(wp),
                ids_equal=sorted(g for g, *_ in gp) == sorted(g for g, *_ in wp),
                outside=outside, missing=int(meta["missing"]), ink_refused=int(meta["ink_refused"]))
        row["c1_eq_c2"] = sorted((g, x, y) for g, _, x, y, _ in parse_baud(row["c1"]["raw"])) == sorted((g, x, y) for g, _, x, y, _ in parse_baud(row["c2"]["raw"]))
        row["differs"] = order_differs(cps)
        per[k] = row
    results[label] = per
json.dump(results, open(f"{Q}/measure.json", "w"))
# tables
def agg(label, corpus, subset):
    per = results[label]; occ = corp[corpus]
    keys = [k for k in occ if (not subset or per[k]["differs"])]
    t = collections.Counter()
    for k in keys:
        r = per[k]; w = occ[k]
        t["n"] += 1; t["w"] += w
        for arm in ("c1", "c2"):
            a = r[arm]
            if a["ink_equal"]: t[arm+"_m"] += 1; t[arm+"_mw"] += w
            if a["exact"]: t[arm+"_x"] += 1
            if a["outside"]: t[arm+"_o"] += 1; t[arm+"_ow"] += w
            t[arm+"_miss"] += a["missing"]; t[arm+"_ref"] += a["ink_refused"]
        if r["hb_out"]: t["hb_o"] += 1; t["hb_ow"] += w
        if not r["c1_eq_c2"]: t["c1ne2"] += 1
    return t
print("face | corpus | subset | clusters (distinct/occ) | C1 = hb (distinct/occ) | C2 = hb (distinct/occ) | C1 exact-order | C2 exact-order | mark outside cell: C1 | C2 | hb | C1 != C2 | notdef C1/C2 | ink_refused")
for path, idx, label in FACES:
    for corpus in ("gen5", "pango"):
        for subset in (False, True):
            t = agg(label, corpus, subset)
            print(f"{label} | {corpus} | {'order differs' if subset else 'all'} | {t['n']}/{t['w']} | {t['c1_m']}/{t['c1_mw']} | {t['c2_m']}/{t['c2_mw']} | {t['c1_x']} | {t['c2_x']} | {t['c1_o']}/{t['c1_ow']} | {t['c2_o']}/{t['c2_ow']} | {t['hb_o']}/{t['hb_ow']} | {t['c1ne2']} | {t['c1_miss']}/{t['c2_miss']} | {t['c1_ref']}/{t['c2_ref']}")
