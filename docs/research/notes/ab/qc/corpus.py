import sys, unicodedata, re, hashlib, json, collections
REPO = str(__import__('pathlib').Path(__file__).resolve().parents[5])
Q = sys.argv[1]
C = Q + "/pins/corpus"
raw = open(C + "/t-gen5.txt", encoding="utf-8").read()
gen5 = raw.replace("\x1b[?25l", "")
# the untracked test's constant, read-only from the main checkout
src = open(REPO + "/baud-core/tests/pointed_hebrew.rs", encoding="utf-8").read()
m = re.search(r"const GENESIS_1_1_TO_5: \[&str; 5\] = \[(.*?)\];", src, re.S)
lines = re.findall(r'"([^"]*)"', m.group(1))
gen5_lines = [l for l in gen5.split("\n") if l.strip()]
print("t-gen5 stripped lines:", len(gen5_lines), "equal to pointed_hebrew.rs constant:", gen5_lines == lines)
pango = open(C + "/pango-test-hebrew.txt", encoding="utf-8").read()
def extends(ch):
    cat = unicodedata.category(ch)
    return cat in ("Mn", "Me") or ch in "‍‌͏"
def clusters(text):
    out = []
    for line in text.split("\n"):
        cur = None
        for ch in line:
            if cur is not None and extends(ch):
                cur.append(ch)
            else:
                if cur: out.append(cur)
                cur = [ch]
        if cur: out.append(cur)
    return out
SBL = {10:22,11:15,12:16,13:17,14:23,15:18,16:19,17:20,18:21,19:14,20:24,21:12,22:25,23:13,24:10,25:11,26:26}
def ccc(ch): return unicodedata.combining(ch)
def mcc(ch): c = ccc(ch); return SBL.get(c, c)
def sort(cl, key):
    base, marks = cl[0], cl[1:]
    # stable sort of marks by key, starters (key 0) break runs; Hebrew clusters here have one starter
    assert all(key(m) != 0 or m in "‍‌" for m in marks) or True
    return [base] + sorted(marks, key=key)
stats = {}
for name, text in (("gen5", "\n".join(gen5_lines)), ("pango", pango)):
    cl = [c for c in clusters(text) if any(0x0590 <= ord(x) <= 0x05FF or 0xFB1D <= ord(x) <= 0xFB4F for x in c)]
    other = collections.Counter(x for c in clusters(text) for x in c if not (0x0590 <= ord(x) <= 0x05FF))
    occ = collections.Counter(tuple(c) for c in cl)
    multi = {c: n for c, n in occ.items() if len([m for m in c[1:] if ccc(m)]) >= 2}
    differ = {c: n for c, n in occ.items() if sort(list(c), ccc) != sort(list(c), mcc)}
    zero_marks = collections.Counter(m for c in occ for m in c[1:] if ccc(m) == 0)
    # HarfBuzz reorder_marks_hebrew pattern on the SBL-sorted sequence (modified classes):
    # c0 in {20,21} (patah, qamats), c1 in {22,23} (sheva, hiriq), c2 in {25, 220} (meteg, below)
    def reorder_fires(c):
        s = sort(list(c), mcc)
        k = [mcc(x) for x in s]
        return any(k[i-2] in (20,21) and k[i-1] in (22,23) and k[i] in (25,220) for i in range(2, len(k)))
    rf = {c: n for c, n in occ.items() if reorder_fires(c)}
    stats[name] = dict(occurrences=sum(occ.values()), distinct=len(occ),
        distinct_two_plus_marks=len(multi), occ_two_plus_marks=sum(multi.values()),
        distinct_order_differs=len(differ), occ_order_differs=sum(differ.values()),
        distinct_reorder_fires=len(rf), occ_reorder_fires=sum(rf.values()),
        zero_class_marks={"%04X" % ord(k): v for k, v in zero_marks.items()},
        non_hebrew_chars={"%04X" % ord(k): v for k, v in other.items()})
    with open(Q + f"/corpus-{name}.txt", "w") as f:
        for c in sorted(occ, key=lambda c: [ord(x) for x in c]):
            f.write(" ".join("%04X" % ord(x) for x in c) + "\n")
    with open(Q + f"/corpus-{name}-occ.json", "w") as f:
        json.dump({" ".join("%04X" % ord(x) for x in c): n for c, n in occ.items()}, f)
    with open(Q + f"/corpus-{name}-differ.txt", "w") as f:
        for c, n in sorted(differ.items(), key=lambda kv: -kv[1]):
            f.write("%d\t%s\t%s\t%s\n" % (n, " ".join("%04X" % ord(x) for x in c),
                " ".join("%04X" % ord(x) for x in sort(list(c), ccc)), " ".join("%04X" % ord(x) for x in sort(list(c), mcc))))
print(json.dumps(stats, indent=1))
