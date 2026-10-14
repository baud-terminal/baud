import sys
from fontTools.ttLib import TTFont, TTCollection
out = []
paths = [l.strip() for l in open(sys.argv[1]) if l.strip()]
paths += ["baud-app/fonts/built/Regular.ttf", "baud-app/fonts/built/Bold.ttf"]
seen = set()
for p in paths:
    if p in seen: continue
    seen.add(p)
    try:
        n = len(TTCollection(p).fonts) if p.lower().endswith((".ttc", ".otc")) else 1
    except Exception as e:
        print("skip", p, e, file=sys.stderr); continue
    for i in range(n):
        try:
            f = TTFont(p, fontNumber=i, lazy=True)
            cm = f.getBestCmap() or {}
        except Exception as e:
            print("skip", p, i, e, file=sys.stderr); continue
        pts = [c for c in range(0x05B0, 0x05C8) if c in cm]
        if 0x05E9 in cm and len(pts) >= 2:
            name = f["name"].getDebugName(4) or p
            out.append((p, i, name, len(pts)))
for p, i, n, k in out:
    print(f"{p}\t{i}\t{n}\t{k}")
