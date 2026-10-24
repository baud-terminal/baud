"""Minimal reader for a Glyphs 2 source: one glyph's paths in one master layer (by layer id)."""
import re, sys
MID = {'Thin': 'EDF5EB54-295C-4C90-9A2F-67CD82979CFF', 'Regular': 'CCC0BD40-BA8C-414E-B576-CF0B94E6C996', 'ExtraBold': '9ADF6715-3D0F-493D-8567-18A6FBCFC567'}
def glyph(t, name):
    i = t.index('\nglyphname = %s;\n' % name)
    j = t.find('\nglyphname = ', i + 10)
    return t[i:j]
def layer_paths(g, mid):
    k = g.index('layerId = "%s";' % mid)
    # the layer block: back to its opening brace, forward to 'width = '
    s = g.rfind('\n{\n', 0, k); e = g.index('\nwidth = ', k)
    L = g[s:e]
    if 'associatedMasterId' in L: raise SystemExit('brace layer')
    paths = []
    for pm in re.finditer(r'closed = 1;\nnodes = \((.*?)\);', L, re.S):
        nodes = re.findall(r'"(-?[\d.]+) (-?[\d.]+) (LINE|CURVE|OFFCURVE|QCURVE)( SMOOTH)?"', pm.group(1))
        paths.append([(float(x), float(y), k2 + (s2 or '')) for x, y, k2, s2 in nodes])
    comps = re.findall(r'name = ([\w.\-]+);', L.split('paths = (')[0]) if 'components' in L else []
    return paths
if __name__ == '__main__':
    t = open(sys.argv[1]).read()
    for p in layer_paths(glyph(t, sys.argv[2]), MID[sys.argv[3]]):
        print(len(p), p)
