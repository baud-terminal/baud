#!/usr/bin/env python3
"""The seven files tests/recordings/sixel/yazi-walk.raw walks over.

    python3 scripts/gen-yazi-files.py <dir>

Deterministic test images and PDFs, seeded: photo.jpg, flat.png, alpha.png,
screenshot.png, small.png, sparse.pdf and text.pdf. Nothing here is derived
from any user file. The JPEG is written by macOS's sips, so it is made on
macOS.
"""
import zlib, struct, math, random, os, subprocess, sys
if len(sys.argv) != 2:
    sys.exit(__doc__)
OUT = sys.argv[1]
os.makedirs(OUT, exist_ok=True)

def png(path, w, h, px, alpha=False):
    ct = 6 if alpha else 2
    raw = bytearray()
    n = 4 if alpha else 3
    for y in range(h):
        raw.append(0)
        raw += px[y*w*n:(y+1)*w*n]
    def chunk(t, d):
        c = struct.pack(">I", len(d)) + t + d
        return c + struct.pack(">I", zlib.crc32(t + d) & 0xffffffff)
    data = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, ct, 0, 0, 0)) \
        + chunk(b"IDAT", zlib.compress(bytes(raw), 6)) + chunk(b"IEND", b"")
    open(path, "wb").write(data)

rnd = random.Random(20261009)

# 1. photo-like: smooth colour field plus per-pixel noise (like a camera photo)
w, h = 1200, 1600
px = bytearray(w*h*3)
for y in range(h):
    for x in range(w):
        r = 128 + 100*math.sin(x/97.0) * math.cos(y/131.0)
        g = 128 + 90*math.sin((x+y)/173.0)
        b = 128 + 110*math.cos(x/61.0 - y/89.0)
        i = (y*w+x)*3
        px[i]   = max(0, min(255, int(r + rnd.gauss(0, 12))))
        px[i+1] = max(0, min(255, int(g + rnd.gauss(0, 12))))
        px[i+2] = max(0, min(255, int(b + rnd.gauss(0, 12))))
png(f"{OUT}/photo.png", w, h, px)
subprocess.run(["sips", "-s", "format", "jpeg", "-s", "formatOptions", "85", f"{OUT}/photo.png", "--out", f"{OUT}/photo.jpg"], check=True, capture_output=True)
os.remove(f"{OUT}/photo.png")

# 2. flat: four colour blocks, same size
px = bytearray(w*h*3)
for y in range(h):
    for x in range(w):
        c = [(200,40,40),(40,160,60),(40,60,200),(230,200,40)][(x >= w//2) + 2*(y >= h//2)]
        i = (y*w+x)*3
        px[i:i+3] = bytes(c)
png(f"{OUT}/flat.png", w, h, px)

# 3. small: 64x64 gradient
sw = sh = 64
px = bytearray(sw*sh*3)
for y in range(sh):
    for x in range(sw):
        i = (y*sw+x)*3
        px[i:i+3] = bytes((x*4, y*4, 128))
png(f"{OUT}/small.png", sw, sh, px)

# 4. screenshot-like: white page with dark 'text' strokes and a coloured band (UI capture)
w2, h2 = 1600, 1000
px = bytearray(b"\xff" * (w2*h2*3))
for y in range(h2):
    for x in range(w2):
        i = (y*w2+x)*3
        if y < 60:
            px[i:i+3] = bytes((236, 236, 240))
        elif 100 <= (y % 40) + 100 < 118 and 40 < x < 1500 and rnd.random() < 0.55:
            v = rnd.randint(20, 120)
            px[i:i+3] = bytes((v, v, v))
png(f"{OUT}/screenshot.png", w2, h2, px)

# 5. alpha: RGBA with a transparent background and a soft disc
w3, h3 = 800, 800
px = bytearray(w3*h3*4)
for y in range(h3):
    for x in range(w3):
        d = math.hypot(x-400, y-400)
        a = max(0, min(255, int(255*(1 - (d-250)/60)))) if d > 250 else 255
        if d > 310: a = 0
        i = (y*w3+x)*4
        px[i:i+4] = bytes((int(x*255/w3), int(y*255/h3), 180, a))
png(f"{OUT}/alpha.png", w3, h3, px, alpha=True)

# 6. PDFs: hand-written, Helvetica (a base-14 font, no embedding)
def pdf(path, content):
    objs = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
        b"<< /Length %d >>\nstream\n" % len(content) + content + b"\nendstream",
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    ]
    out = bytearray(b"%PDF-1.4\n")
    offs = []
    for i, o in enumerate(objs, 1):
        offs.append(len(out))
        out += b"%d 0 obj\n" % i + o + b"\nendobj\n"
    x = len(out)
    out += b"xref\n0 %d\n0000000000 65535 f \n" % (len(objs)+1)
    for o in offs:
        out += b"%010d 00000 n \n" % o
    out += b"trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (len(objs)+1, x)
    open(path, "wb").write(out)

words = "lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor incididunt ut labore et dolore magna aliqua".split()
lines = []
for i in range(60):
    lines.append(" ".join(rnd.choice(words) for _ in range(14)))
c = b"BT /F1 10 Tf 12 TL 50 760 Td " + b" ".join(b"(" + l.encode() + b") '" for l in lines) + b" ET"
pdf(f"{OUT}/text.pdf", c)
pdf(f"{OUT}/sparse.pdf", b"BT /F1 24 Tf 72 700 Td (A title) Tj ET")
print(sorted(os.listdir(OUT)))
