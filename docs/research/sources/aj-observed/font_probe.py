"""Note AJ, section 3.1: the embedded faces and the lockup's geometry.

Run from the repository root. Reads each embedded face's cmap (formats 4
and 12), head, hhea, hmtx, OS/2, loca and glyf, and prints: the glyph ids of
the lockup's characters (control letters b a u d), units per em, advances,
cap height, weight class, whether an fvar table exists, file sizes, and the
lockup's advance-box spans and ink gaps at the sizes assets/logo uses
(wordmark 62 px at x 40 with -2.8 tracking; prompt 54 px at x 192; block
rect x 238, 22 x 46). The wordmark's weight 500 is in no face, so its ink is
taken from the 400 face (and, for comparison, the 700 face).
"""
import struct, os
FACES = ['baud-app/fonts/built/Regular.ttf', 'baud-app/fonts/built/Bold.ttf',
         'baud-app/fonts/jetbrains-mono/JetBrainsMono-Italic.ttf',
         'baud-app/fonts/jetbrains-mono/JetBrainsMono-BoldItalic.ttf',
         'baud-app/fonts/symbols-nerd-font/SymbolsNerdFontMono-Regular.ttf',
         'baud-app/fonts/stix-two-math/STIXTwoMath-Regular.otf']
UPSTREAM = ['baud-app/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf',
            'baud-app/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf']
def tables(b):
    n = struct.unpack('>H', b[4:6])[0]
    return {b[12+16*i:16+16*i]: struct.unpack('>I', b[20+16*i:24+16*i])[0] for i in range(n)}
def cmap(b, t, cps):
    off = t[b'cmap']; num = struct.unpack('>H', b[off+2:off+4])[0]; best = None
    for i in range(num):
        pid, eid, so = struct.unpack('>HHI', b[off+4+8*i:off+12+8*i])
        fmt = struct.unpack('>H', b[off+so:off+so+2])[0]
        if fmt == 12 or (fmt == 4 and best is None): best = (fmt, off+so)
    fmt, st = best; res = {}
    if fmt == 12:
        ng = struct.unpack('>I', b[st+12:st+16])[0]
        for cp in cps:
            for g in range(ng):
                s, e, sg = struct.unpack('>III', b[st+16+12*g:st+28+12*g])
                if s <= cp <= e: res[cp] = sg + cp - s
    else:
        segx2 = struct.unpack('>H', b[st+6:st+8])[0]; seg = segx2 // 2
        ends = struct.unpack('>%dH' % seg, b[st+14:st+14+segx2])
        starts = struct.unpack('>%dH' % seg, b[st+16+segx2:st+16+2*segx2])
        deltas = struct.unpack('>%dh' % seg, b[st+16+2*segx2:st+16+3*segx2])
        rp = st+16+3*segx2; roffs = struct.unpack('>%dH' % seg, b[rp:rp+segx2])
        for cp in cps:
            for k in range(seg):
                if starts[k] <= cp <= ends[k]:
                    if roffs[k] == 0: res[cp] = (cp + deltas[k]) & 0xffff
                    else:
                        a = rp + 2*k + roffs[k] + 2*(cp - starts[k]); g = struct.unpack('>H', b[a:a+2])[0]
                        res[cp] = (g + deltas[k]) & 0xffff if g else 0
                    break
    return res
def adv(b, t, g):
    nhm = struct.unpack('>H', b[t[b'hhea']+34:t[b'hhea']+36])[0]; g = min(g, nhm-1)
    return struct.unpack('>H', b[t[b'hmtx']+4*g:t[b'hmtx']+4*g+2])[0]
def bbox(b, t, g):
    fmt = struct.unpack('>h', b[t[b'head']+50:t[b'head']+52])[0]; loca = t[b'loca']
    off = struct.unpack('>H', b[loca+2*g:loca+2*g+2])[0]*2 if fmt == 0 else struct.unpack('>I', b[loca+4*g:loca+4*g+4])[0]
    return struct.unpack('>hhhh', b[t[b'glyf']+off+2:t[b'glyf']+off+10])
CPS = [0x62, 0x61, 0x75, 0x64, 0x276F, 0x2588]
for p in FACES:
    b = open(p, 'rb').read(); t = tables(b); os2 = t[b'OS/2']
    w = struct.unpack('>H', b[os2+4:os2+6])[0]
    line = f"{p}: {len(b)} bytes, usWeightClass {w}, fvar {b'fvar' in t}"
    if p.startswith('baud-app/fonts/built/'):
        upem = struct.unpack('>H', b[t[b'head']+18:t[b'head']+20])[0]
        cap = struct.unpack('>h', b[os2+88:os2+90])[0]
        m = cmap(b, t, CPS)
        line += f", upem {upem}, cap height {cap}, glyphs " + ' '.join(f"U+{cp:04X}={m.get(cp)}" for cp in CPS)
        line += f", advances a {adv(b, t, m[0x61])} U+276F {adv(b, t, m[0x276F])} U+2588 {adv(b, t, m[0x2588])}"
    print(line)
for p in UPSTREAM:
    print(f"{p}: {os.path.getsize(p)} bytes")
R = open(FACES[0], 'rb').read(); Bd = open(FACES[1], 'rb').read(); tr, tb = tables(R), tables(Bd)
d4 = bbox(R, tr, cmap(R, tr, [0x64])[0x64]); d7 = bbox(Bd, tb, cmap(Bd, tb, [0x64])[0x64])
p7 = bbox(Bd, tb, cmap(Bd, tb, [0x276F])[0x276F])
pitch = 600*62/1000 - 2.8; dorig = 40 + 3*pitch
print(f"wordmark pitch {pitch:.1f} px; 'baud' advance boxes x 40-{dorig + 600*62/1000:.1f}; prompt box x 192-{192 + 600*54/1000:.1f}")
for name, d in (("d at 400", d4), ("d at 700", d7)):
    print(f"ink gap d -> U+276F ({name}, U+276F at 700): {192 + p7[0]*54/1000 - (dorig + d[2]*62/1000):.1f} px")
print(f"ink gap U+276F -> block: {238 - (192 + p7[2]*54/1000):.1f} px; block 22 x 46 = {22/62:.2f} x {46/62:.2f} em at 62 px")
