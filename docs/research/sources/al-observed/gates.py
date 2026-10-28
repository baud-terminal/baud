#!/usr/bin/env python3
"""Note AL's harness gates, recomputed from the four prototypes' raw runs.

    python3 -I gates.py FLASH_FIX_DIR SCRATCH_BIN CENSUS

FLASH_FIX_DIR is the prototypes' working directory (each prototype's runs/ and bin/
under proto-O1 .. proto-O4, and note/to-commit/al-observed/ for the joined
census files); SCRATCH_BIN is the scratch bin/ directory the O2 and O4
prototypes built into; CENSUS is docs/research/sources/aj-observed/
flash-census.tsv. Nothing is re-measured: every figure is read from files the
prototypes wrote, or from the binaries they left.

Sections:
 1. The bench's own controls in every run file: the 4 Hz block must FAIL and
    the 2 Hz block must pass.
 2. The A/A: the base run twice at 13 pt 1x, compared on the frame hash (fnv).
 3. `cmp` of the base and option binaries, where both were kept.
 4. Which harness wrote each run file: O2's harness adds the columns repeat,
    under_dl, under_over, under_red and over_cap in both its builds; O1's, O3's
    and O4's write the same columns as each other.
 5. The shared scratch bin/ as found when this ran: each binary's size,
    modification time and sha256, and whether it carries the string
    "under_dl" (O2's harness's column name). flash-o2 is the positive control;
    O1's and O3's base binaries, kept in their own directories, the negative.
 6. Base rows by provenance: O1's from its own runs or copied from the
    committed census (its analyze.py's fallback), and each source's base rows
    that no other source shares.
"""
import csv
import datetime
import filecmp
import glob
import hashlib
import os
import sys
from collections import Counter, defaultdict

FF, SBIN, CENSUS = sys.argv[1], sys.argv[2], sys.argv[3]
OBS = os.path.join(FF, "note", "to-commit", "al-observed")
PROTOS = ["proto-O1", "proto-O2", "proto-O3", "proto-O4"]
O2COLS = {"repeat", "under_dl", "under_over", "under_red", "over_cap"}


def rows(path):
    with open(path, newline="") as f:
        return list(csv.DictReader(f, delimiter="\t"))


def header(path):
    with open(path) as f:
        return f.readline().rstrip("\n").split("\t")


def runfiles(p):
    return sorted(glob.glob(os.path.join(FF, p, "runs", "*.tsv")))


KEY = ("pt", "scale", "rate", "ground", "ink", "arm", "motion")


def key(r):
    return tuple(r[c] for c in KEY)


print("== 1. the bench's controls, every run file (4 Hz block must FAIL, 2 Hz block must pass)")
for p in PROTOS:
    c = Counter()
    files = 0
    for f in runfiles(p):
        rs = rows(f)
        hit = False
        for r in rs:
            a = r.get("arm", "")
            if a in ("block-4hz", "block-2hz"):
                hit = True
                ok = (r["outcome"] == "FAIL") == (a == "block-4hz")
                c[(a, "as required" if ok else "WRONG")] += 1
        files += hit
    print(f"  {p}: {files} run files carry the controls; "
          + ", ".join(f"{a} {v}: {n}" for (a, v), n in sorted(c.items())))

print("\n== 2. A/A: the base at 13 pt 1x run twice, frame hashes compared case by case")
AA = {
    "proto-O1": (["base-13x1-60-hashA.tsv"], ["base-13x1-60-hashB.tsv"]),
    "proto-O2": (["base-13x1-a.tsv"], ["base-13x1-b.tsv"]),
    "proto-O3": ([f"base-13x1-{g}.tsv" for g in ("dark", "darkred", "light", "red")],
                 [f"base-aa-13x1-{g}.tsv" for g in ("dark", "darkred", "light", "red")]),
    "proto-O4": (["min-base-13x1.tsv"], ["again-base-13x1.tsv"]),
}
for p, (fa, fb) in AA.items():
    def fnvs(names):
        out = {}
        for n in names:
            for r in rows(os.path.join(FF, p, "runs", n)):
                if (r.get("fnv") or "-") != "-":
                    out[key(r)] = r["fnv"]
        return out
    a, b = fnvs(fa), fnvs(fb)
    both = set(a) & set(b)
    eq = sum(a[k] == b[k] for k in both)
    blocks = sum(1 for k in both if k[5].startswith("block"))
    print(f"  {p}: {' '.join(fa)} vs {' '.join(fb)}: {len(both)} rows hashed in both"
          f" ({len(both) - blocks} cases and {blocks} control blocks), {eq} equal")

print("\n== 3. cmp of the base and option binaries, where both were kept")
KEPT = [("proto-O1", "bin/flash-base", "bin/flash-o1"), ("proto-O1", "bin/flash-base", "bin/flash-o1-29"),
        ("proto-O3", "runs/flash-base", "runs/flash-o3")]
for p, a, b in KEPT:
    pa, pb = os.path.join(FF, p, a), os.path.join(FF, p, b)
    same = filecmp.cmp(pa, pb, shallow=False)
    print(f"  {p}: {a} vs {b}: {'IDENTICAL' if same else 'differ'}"
          f" ({os.path.getsize(pa)} and {os.path.getsize(pb)} bytes)")
print("  proto-O2, proto-O4: no binary kept in the prototype's directory; they built into the shared"
      " scratch bin/ (section 5), where later builds replaced earlier ones")

print("\n== 4. which harness wrote each run file (O2's harness adds " + ", ".join(sorted(O2COLS)) + ")")
for p in PROTOS:
    c = Counter()
    for f in runfiles(p):
        h = set(header(f))
        c["O2's columns" if O2COLS <= h else ("none of them" if not (O2COLS & h) else "SOME")] += 1
    print(f"  {p}: {sum(c.values())} run files: " + ", ".join(f"{v} {n}" for v, n in sorted(c.items())))

print("\n== 5. the shared scratch bin/, as found when this ran")
print(f"  read at {datetime.datetime.now().astimezone().isoformat(timespec='seconds')}")
probe = b"under_dl"
for n in sorted(os.listdir(SBIN)):
    p = os.path.join(SBIN, n)
    data = open(p, "rb").read()
    m = datetime.datetime.fromtimestamp(os.path.getmtime(p)).astimezone()
    print(f"  {n:15} {len(data):>8} bytes  modified {m.isoformat(timespec='seconds')}"
          f"  sha256 {hashlib.sha256(data).hexdigest()}  carries \"under_dl\": {'yes' if probe in data else 'no'}")
for p, rel in (("proto-O1", "bin/flash-base"), ("proto-O3", "runs/flash-base")):
    data = open(os.path.join(FF, p, rel), "rb").read()
    print(f"  negative control {p}/{rel}: carries \"under_dl\": {'yes' if probe in data else 'no'}")

print("\n== 6. base rows by provenance")
o1runs = set()
for f in runfiles("proto-O1"):
    if os.path.basename(f).startswith("base-"):
        for r in rows(f):
            if not r["arm"].startswith("block"):
                o1runs.add(key(r))
o1 = [key(r) for r in rows(os.path.join(OBS, "o1-census.tsv"))]
own = [k for k in o1 if k in o1runs]
cen = {key(r): r for r in rows(CENSUS)}
copied = [k for k in o1 if k not in o1runs]
print(f"  O1: {len(o1)} base rows; {len(own)} from O1's own base runs, {len(copied)} not run by O1"
      f" ({sum(k in cen for k in copied)} of them taken from the committed census)")
# O2's rows by build, as its analyze.py takes them: the first harness build's
# runs (base-*, o2-*; no commit kept), then d9ed4bd's (base2-*, o2b-*), then
# the final 4ccd622's (base3-*, o2c-*), each later one replacing a case.
def o2_build(prefixes):
    got = {}
    for pre in prefixes:
        for f in sorted(glob.glob(os.path.join(FF, "proto-O2", "runs", f"{pre}-*.tsv"))):
            n = os.path.basename(f)[:-4]
            if n.endswith("-b") or any(s in n for s in ("gate", "cells", "png", "drop", "hash")):
                continue
            for r in rows(f):
                if not r["arm"].startswith("block"):
                    got[key(r)] = pre
    return Counter((v, k[0]) for k, v in got.items())


NAMES = {"base": "first build", "o2": "first build", "base2": "d9ed4bd", "o2b": "d9ed4bd",
         "base3": "4ccd622", "o2c": "4ccd622"}
for side, pres in (("base", ("base", "base2", "base3")), ("option", ("o2", "o2b", "o2c"))):
    c = o2_build(pres)
    print(f"  O2's {side} rows by build: " + ", ".join(
        f"{NAMES[b]} {pt} pt {n}" for (b, pt), n in sorted(c.items(), key=lambda kv: (pres.index(kv[0][0]), int(kv[0][1])))))
SRC = {
    "O1-own": set(own),
    "O1-copied": set(copied),
    "O2": {(r["pt"], r["scale"], r["rate"], r["ground"], r["ink"], r["arm"], r["motion"])
           for r in rows(os.path.join(OBS, "o2-census.tsv")) if (r["base_area"] or "").strip()},
    "O3": {(r["pt"], r["scale"], r["rate"], r["ground"], r["ink"], r["effect"], r["motion"])
           for r in rows(os.path.join(OBS, "o3-census.tsv")) if (r["base_area"] or "").strip()},
    "O4": {(r["pt"], r["scale"], r["rate"], r["ground"], r["colour"], r["effect"], r["motion"])
           for r in rows(os.path.join(OBS, "o4-census.tsv")) if (r["base_area"] or "-").strip() != "-"},
    "census": set(cen),
}
others = {"O1-own": ["O2", "O3", "O4"], "O2": ["O1-own", "O3", "O4", "census"],
          "O3": ["O1-own", "O2", "O4", "census"], "O4": ["O1-own", "O2", "O3", "census"]}
for s, os_ in others.items():
    pool = set().union(*(SRC[o] for o in os_))
    un = sorted(SRC[s] - pool)
    print(f"  {s}: {len(SRC[s])} base rows, {len(SRC[s]) - len(un)} shared with another prototype's"
          f" own run{' or the census' if 'census' in os_ else ''}, {len(un)} not shared")
    by = Counter()
    for k in un:
        g = "red grounds" if k[3] in ("red", "darkred") else "light/dark"
        by[(f"{k[0]}x{k[1]}", f"{k[2]} Hz", g, k[5] if k[5] in ("quad", "sweep") else "effect or off",
            "tri-100ms" if k[6] == "tri-100ms" else "")] += 1
    for g, n in sorted(by.items()):
        print(f"      {' '.join(x for x in g if x)}: {n}")
