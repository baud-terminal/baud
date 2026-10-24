import sys
sys.path.insert(0, sys.argv[1])
from glyphs_src import glyph, layer_paths, MID
up = open(sys.argv[2]).read(); it = open(sys.argv[3]).read()
for m in ('Regular', 'ExtraBold'):
    for g in 'o q b d p g c e a zero O C G D Q U'.split():
        try:
            U = layer_paths(glyph(up, g), MID[m]); I = layer_paths(glyph(it, g), MID[m])
        except Exception as e:
            print(m, g, 'err', e); continue
        def js(paths):
            return [sorted(set(int(p[1]) for p in c if 'SMOOTH' in p[2] and p[2] != 'OFFCURVE' and (p[2].startswith('LINE') or True)))  for c in paths]
        # LINE SMOOTH nodes and CURVE SMOOTH nodes adjacent to a LINE: the joints
        def joints(paths):
            out = []
            for c in paths:
                n = len(c); ys = []
                for i, p in enumerate(c):
                    if p[2] == 'OFFCURVE': continue
                    nxt = c[(i + 1) % n]
                    # a smooth on-curve between a straight and a curve
                    if 'SMOOTH' in p[2] and (p[2].startswith('LINE') or nxt[2].startswith('LINE')):
                        ys.append(int(p[1]))
                out.append(sorted(set(ys)))
            return out
        print(m, g, 'up', joints(U), 'it', joints(I))
