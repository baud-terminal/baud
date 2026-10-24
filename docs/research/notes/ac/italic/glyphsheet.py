"""Draw glyph outlines large, with on-curve nodes, to PNG (no GUI): glyphsheet.py out.png font.ttf[,font2.ttf...] U+05D0 ..."""
import sys, zlib, struct, math
import numpy as np
from fontTools.ttLib import TTFont
from fontTools.pens.recordingPen import DecomposingRecordingPen
def flatten(rec):
    cs, cur, nodes = [], [], []
    for op, args in rec:
        if op == 'moveTo': cur = [args[0]]; nodes.append(args[0])
        elif op == 'lineTo': cur.append(args[0]); nodes.append(args[0])
        elif op == 'qCurveTo':
            pts = list(args); p0 = cur[-1]; offs = pts[:-1]; end = pts[-1]
            if end is None: end = ((offs[0][0]+offs[-1][0])/2, (offs[0][1]+offs[-1][1])/2)
            for i, o in enumerate(offs):
                nxt = end if i == len(offs)-1 else ((o[0]+offs[i+1][0])/2, (o[1]+offs[i+1][1])/2)
                for t in np.linspace(0, 1, 16)[1:]:
                    cur.append(((1-t)**2*p0[0]+2*(1-t)*t*o[0]+t*t*nxt[0], (1-t)**2*p0[1]+2*(1-t)*t*o[1]+t*t*nxt[1]))
                p0 = nxt
            nodes.append(end)
        elif op == 'curveTo':
            p0 = np.array(cur[-1]); a, b, c = map(np.array, args)
            for t in np.linspace(0, 1, 24)[1:]:
                cur.append(tuple((1-t)**3*p0+3*(1-t)**2*t*a+3*(1-t)*t*t*b+t**3*c))
            nodes.append(args[-1])
        elif op in ('closePath', 'endPath'):
            if cur: cs.append(np.array(cur, float))
            cur = []
    return cs, nodes
def fill(img, cs, ox, oy, sc, col):
    H, W = img.shape[:2]
    edges = []
    for c in cs:
        for i in range(len(c)):
            a, b = c[i], c[(i+1) % len(c)]
            edges.append((ox + a[0]*sc, oy - a[1]*sc, ox + b[0]*sc, oy - b[1]*sc))
    E = np.array(edges)
    for y in range(H):
        yc = y + 0.5
        m = ((E[:,1] <= yc) & (E[:,3] > yc)) | ((E[:,3] <= yc) & (E[:,1] > yc))
        if not m.any(): continue
        e = E[m]; x = e[:,0] + (yc - e[:,1]) * (e[:,2]-e[:,0]) / (e[:,3]-e[:,1]); w = np.where(e[:,3] > e[:,1], 1, -1)
        o = np.argsort(x); x, w = x[o], w[o]; acc = 0
        for i in range(len(x)-1):
            acc += w[i]
            if acc != 0:
                x0, x1 = int(max(0, math.ceil(x[i]-0.5))), int(min(W, math.floor(x[i+1]-0.5)+1))
                if x1 > x0: img[y, x0:x1] = col
def png(path, img):
    H, W = img.shape[:2]
    raw = b''.join(b'\x00' + img[y].astype(np.uint8).tobytes() for y in range(H))
    ch = lambda t, d: struct.pack('>I', len(d)) + t + d + struct.pack('>I', zlib.crc32(t + d) & 0xffffffff)
    open(path, 'wb').write(b'\x89PNG\r\n\x1a\n' + ch(b'IHDR', struct.pack('>IIBBBBB', W, H, 8, 2, 0, 0, 0)) + ch(b'IDAT', zlib.compress(raw, 9)) + ch(b'IEND', b''))
if __name__ == '__main__':
    out, fonts, cps = sys.argv[1], sys.argv[2].split(','), [int(a[2:], 16) for a in sys.argv[3:]]
    sc = float(__import__('os').environ.get('SCALE', 0.3)); cw, ch_ = int(700*sc), int(1300*sc)
    img = np.full((ch_*len(fonts), cw*len(cps), 3), 255, np.uint8)
    for fi, fp in enumerate(fonts):
        f = TTFont(fp); gs = f.getGlyphSet(); cm = f.getBestCmap()
        for ci, cp in enumerate(cps):
            if cp not in cm: continue
            r = DecomposingRecordingPen(gs); gs[cm[cp]].draw(r)
            cs, nodes = flatten(r.value)
            ox, oy = ci*cw + 50*sc, fi*ch_ + 1000*sc
            img[int(oy), ci*cw:(ci+1)*cw] = (200, 200, 255)                  # baseline
            img[int(oy-630*sc), ci*cw:(ci+1)*cw] = (200, 255, 200)           # Hebrew letter height
            img[fi*ch_:(fi+1)*ch_, int(ox)] = (230, 230, 230); img[fi*ch_:(fi+1)*ch_, int(ox+600*sc)] = (230, 230, 230)
            fill(img, cs, ox, oy, sc, (40, 40, 40))
            for n in nodes:
                x, y = int(ox + n[0]*sc), int(oy - n[1]*sc)
                img[max(0,y-1):y+2, max(0,x-1):x+2] = (230, 40, 40)
    png(out, img)
