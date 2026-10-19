def pack(vals, w, fmt):
    out = []
    for i in range(0, len(vals), 3):
        t = (vals[i:i+3] + [0, 0])[:3]
        out.append(fmt(["%0*x" % (w, v) for v in t]))
    return out
L = ["#3a7", "#3000a0007000", "rgb:00/ff/00", "rgb:ea/75/52", "rgb:ccc/320/320", "rgb:ff/a5/0", "rgb:ccc/32/0"]
L += ["RGB:FF/00/00", "Rgb:ff/A5/0", "rGb:CcC/320/320", "#3A7", "#ABCDEF"]
rgb = lambda t: "rgb:" + "/".join(t)
sharp = lambda t: "#" + "".join(t)
for w, step in ((1, 1), (2, 1), (3, 13), (4, 251)):
    vals = list(range(0, 16 ** w, step)) + [16 ** w - 1]
    L += pack(vals, w, rgb)
    L += pack(vals, w, sharp)
L += ["rgb:f/ff/fff", "rgb:fff/ff/f", "rgb:1/22/333", "rgb:8000/800/80", "rgb:0081/009/0"]
L += ["rgb:", "rgb:1//2", "rgb:/1/2", "rgb:1/2", "rgb:12345/0/0", "rgb:g/0/0", "rgb:1/2/3/", "rgb:1/2/3/4",
      "#", "#f", "#ff", "#ffff", "#fffff", "#fffffffffffffff", "#ggg", "# fff", "#fff ", "rgb: 1/2/3"]
print("\n".join(L))
