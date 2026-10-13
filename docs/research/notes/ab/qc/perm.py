import sys, json, itertools, unicodedata, subprocess, collections
Q = sys.argv[1]
occ = {}
for n in ("gen5", "pango"): occ.update(json.load(open(f"{Q}/corpus-{n}-occ.json")))
ccc = lambda c: unicodedata.combining(chr(c))
rows, owner = [], []
for k in sorted(occ):
    cps = [int(x,16) for x in k.split()]
    marks = cps[1:]
    if len(marks) < 2: continue
    seen = set()
    for p in itertools.permutations(range(len(marks))):
        seq = [marks[i] for i in p]
        # canonically equivalent iff same-class marks keep their relative order
        ok = all([m for m in seq if ccc(m) == c] == [m for m in marks if ccc(m) == c] for c in set(map(ccc, marks)))
        if ok and tuple(seq) not in seen:
            seen.add(tuple(seq)); rows.append([cps[0]] + seq); owner.append(k)
open(f"{Q}/perms.txt", "w").write("\n".join(" ".join("%04X" % c for c in r) for r in rows) + "\n")
faces = [("baud-app/fonts/built/Regular.ttf", 0), ("/System/Library/Fonts/Supplemental/Courier New.ttf", 0), ("/System/Library/Fonts/ArialHB.ttc", 0), ("/System/Library/Fonts/SFHebrew.ttf", 0)]
print("clusters with >=2 marks:", len(set(owner)), "canonically equivalent spellings:", len(rows))
for path, idx in faces:
    for arm in ("c1", "c2"):
        out = subprocess.run([f"{Q}/bin/qc-{arm}", "shape", path, str(idx), f"{Q}/perms.txt"], capture_output=True, text=True, check=True).stdout.strip().split("\n")
        by = collections.defaultdict(set)
        for o, line in zip(owner, out): by[o].add(line.split("\t")[1])
        varying = sum(1 for v in by.values() if len(v) > 1)
        print(path.split("/")[-1], arm, "clusters whose output varies across equivalent spellings:", varying)
