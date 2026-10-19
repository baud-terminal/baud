import itertools
L = ["CIEXYZ:0.3227/0.28133/0.2493", "RGBi:1.0/0.0/0.0", "CIELuv:50.0/0.0/0.0",
     "ciexyz:0.3227/0.28133/0.2493", "rgbi:1/0/0", "CieLab:50/0/0", "TEKHVC:0/100/0", "cieUVY:0.2/0.5/0.5", "CIExyy:0.3127/0.329/0.5"]
f = lambda v: ("%.4f" % v).rstrip("0").rstrip(".") if v != int(v) else "%d" % v
for r, g, b in itertools.product([0, 0.05, 0.2, 0.5, 0.8, 1], repeat=3):
    L.append("rgbi:%s/%s/%s" % (f(r), f(g), f(b)))
for i in range(0, 101, 4):
    L.append("rgbi:%s/%s/%s" % (f(i/100), f(i/100), f(i/100)))
for x, y, z in itertools.product([0, 0.1, 0.3, 0.6, 0.95], [0, 0.05, 0.3, 0.7, 1], [0, 0.1, 0.5, 1.1]):
    L.append("CIEXYZ:%s/%s/%s" % (f(x), f(y), f(z)))
for u, v, Y in itertools.product([0.15, 0.19, 0.25, 0.4], [0.35, 0.45, 0.5, 0.55], [0.05, 0.3, 0.8]):
    L.append("CIEuvY:%s/%s/%s" % (f(u), f(v), f(Y)))
for x, y, Y in itertools.product([0.2, 0.3127, 0.45, 0.6], [0.2, 0.329, 0.45, 0.6], [0.1, 0.5, 1]):
    L.append("CIExyY:%s/%s/%s" % (f(x), f(y), f(Y)))
for l, a, b in itertools.product([0, 3, 7, 8, 20, 50, 75, 100], [-0.5, -0.1, 0, 0.1, 0.4], [-0.5, 0, 0.3]):
    L.append("CIELab:%s/%s/%s" % (f(l), f(a), f(b)))
for l, a, b in itertools.product([30, 60], [-80, 40], [-60, 70]):
    L.append("CIELab:%s/%s/%s" % (f(l), f(a), f(b)))
for l, u, v in itertools.product([0, 5, 10, 50, 90, 100], [-0.5, 0, 0.2, 0.6], [-0.4, 0, 0.3]):
    L.append("CIELuv:%s/%s/%s" % (f(l), f(u), f(v)))
for h, v, c in itertools.product(range(0, 360, 30), [5, 20, 50, 80, 95], [0, 10, 30, 60, 100]):
    L.append("TekHVC:%s/%s/%s" % (f(h), f(v), f(c)))
L += ["TekHVC:-30/50/20", "TekHVC:390/50/20", "TekHVC:725.5/40/10", "TekHVC:0/0/0", "TekHVC:0/100/50",
      "rgbi:.5/5./+1e-1", "rgbi:1E0/0/0", "CIELab:5e1/0/0", "CIELab:-0/0/0",
      "rgbi:", "rgbi:1/0", "rgbi:1//0", "rgbi:a/0/0", "rgbi:./0/0", "rgbi:1e/0/0", "rgbi:-0.5/0/0",
      "CIELab:101/0/0", "CIELab:-1/0/0", "CIEXYZ:0/1.5/0", "CIEXYZ:0/-0.5/0", "CIEuvY:0.2/0.4/1.5",
      "CIExyY:1.2/0.3/0.5", "CIExyY:0.3/-0.1/0.5", "TekHVC:0/101/0", "TekHVC:0/-1/0", "TekHVC:0/50/-1",
      "CIELuv:-5/0/0", "CIELuv:105/0/0", "cie:1/2/3", "xyz:1/2/3", "CIEXYZ:", "CIELab:50/0"]
print("\n".join(L))
