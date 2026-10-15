# `morx` walked in the order its coverage names, before and after, against
# hb-shape 13.2.1 at each face's chain defaults (`-kern`, as
# gen-shape-fixtures.sh's `morx_features` computes it for every face here):
# every distinct cluster of the Genesis and Pango corpora
# (../qc/clusters-all.txt, ../qc/corpus-*-occ.json). ../qc-fallback/ shaped
# these faces with the grid set, whose `-rlig` and `-liga` turn Raanana's
# and Lucida Grande's default-on flag 0x2 off; baud runs the defaults.
#
#   python3 corpus_cmp.py ../qc <dir holding qc-before and qc-after>
#
# qc-before and qc-after are ../qc/qc.rs.txt built as baud-font's example
# `qc` in `git archive` trees of the commit before and of this commit.
# Output: corpus.txt.
# Each distinct corpus cluster shaped alone by hb-shape and by both arms;
# "= hb" is the same (glyph, x, y) set relative to the cell, as note AB's
# measure.py counts it.
import sys, json, re, subprocess
Q, S = sys.argv[1], sys.argv[2]
H = "/nix/store/lk0d3svdv7gqk6472n4n1zr6nip1jd57-harfbuzz-13.2.1-dev/bin/hb-shape"
FEAT = "-kern"
FACES = [
 ("/System/Library/Fonts/Supplemental/Raanana.ttc", 0, "Raanana"),
 ("/System/Library/Fonts/Supplemental/Raanana.ttc", 1, "Raanana Bold"),
 ("/System/Library/Fonts/Supplemental/NewPeninimMT.ttc", 0, "New Peninim MT"),
 ("/System/Library/Fonts/Supplemental/Corsiva.ttc", 0, "Corsiva Hebrew"),
 ("/System/Library/Fonts/Supplemental/Corsiva.ttc", 1, "Corsiva Hebrew Bold"),
 ("/System/Library/Fonts/LucidaGrande.ttc", 0, "Lucida Grande"),
 ("/System/Library/Fonts/LucidaGrande.ttc", 1, "Lucida Grande Bold"),
]
corp = {n: json.load(open(f"{Q}/corpus-{n}-occ.json")) for n in ("gen5", "pango")}
allc = [l.strip() for l in open(f"{Q}/clusters-all.txt") if l.strip()]
tfile = f"{S}/clusters-all.utf8.txt"
open(tfile, "w", encoding="utf-8").write("\n".join("".join(chr(int(x, 16)) for x in k.split()) for k in allc) + "\n")
G = re.compile(r"(\d+)=(\d+)(?:@(-?\d+),(-?\d+))?\+(-?\d+)")
def hb_set(s, rtl):
    gl = [tuple(int(v) if v is not None else 0 for v in m.groups()) for m in G.finditer(s)]
    pen = 0; placed = []
    for gid, cl, dx, dy, ax in gl:
        placed.append((gid, pen + dx, dy)); pen += ax
    return sorted(placed)
def baud_set(s):
    return sorted((int(g), int(dx), int(dy)) for g, cl, dx, dy, adv in G.findall(s))
for path, idx, label in FACES:
    hb = subprocess.run([H, "--no-glyph-names", f"--features={FEAT}", f"--face-index={idx}", f"--text-file={tfile}", path],
                        capture_output=True, text=True, check=True).stdout.strip().split("\n")
    arm = {a: subprocess.run([f"{S}/qc-{a}", "shape", path, str(idx), f"{Q}/clusters-all.txt"],
                             capture_output=True, text=True, check=True).stdout.strip().split("\n") for a in ("before", "after")}
    assert len(hb) == len(allc) == len(arm["before"]) == len(arm["after"])
    eq = {}
    for i, k in enumerate(allc):
        want = hb_set(hb[i], True)
        eq[k] = {a: baud_set(arm[a][i].split("\t")[1]) == want for a in arm}
        eq[k]["changed"] = arm["before"][i] != arm["after"][i]
    for n in ("gen5", "pango"):
        occ = corp[n]; keys = list(occ)
        def c(pred): 
            ks = [k for k in keys if pred(eq[k])]; return f"{len(ks)}/{sum(occ[k] for k in ks)}"
        print(f"{label} | {n} | {len(keys)}/{sum(occ.values())} | before = hb {c(lambda e: e['before'])} | after = hb {c(lambda e: e['after'])} | "
              f"changed {c(lambda e: e['changed'])} | worse {c(lambda e: e['before'] and not e['after'])} | newly = hb {c(lambda e: e['after'] and not e['before'])}")
