import re, sys, glob
root = sys.argv[1]
files = sorted(glob.glob(root + "/tests/recordings/*/*.raw") + glob.glob(root + "/tests/resize/*.raw"))
csi = re.compile(rb"\x1b\[([0-9;?]*)([ -/]*)([@-~])|\x1bM|\x1bD")
for f in files:
    d = open(f, "rb").read()
    alt = False; cnt = {}
    for m in csi.finditer(d):
        t = m.group(0)
        if t in (b"\x1bM",): k = "RI"
        elif t == b"\x1bD": k = "IND"
        else:
            p, i, fin = m.group(1), m.group(2), m.group(3)[0]
            if p.startswith(b"?") and fin in (ord("h"), ord("l")):
                for n in p[1:].split(b";"):
                    if n in (b"1049", b"47", b"1047"):
                        alt = fin == ord("h")
                        cnt[("mode", n.decode())] = cnt.get(("mode", n.decode()), 0) + 1
                continue
            if i: continue
            k = {ord("r"): "DECSTBM", ord("L"): "IL", ord("M"): "DL", ord("S"): "SU", ord("T"): "SD"}.get(fin)
            if not k or p.startswith(b"?"): continue
        key = ("alt" if alt else "pri", k)
        cnt[key] = cnt.get(key, 0) + 1
    print(f.split("/tests/")[1], {f"{a}:{b}": v for (a, b), v in sorted(cnt.items())})
