# hb-shape each line with the script fixture's features (gen-shape-fixtures.sh
# SC_FEATURES), script and direction guessed, converted to baud's form
# (glyph=cluster@offset-from-cluster-left,dy) as tests/shape.rs does.
import sys, subprocess, re
HB = "/nix/store/lk0d3svdv7gqk6472n4n1zr6nip1jd57-harfbuzz-13.2.1-dev/bin/hb-shape"
FEAT = "-abvm,-blwm,-locl,-rlig,-clig,-curs,-dist,-kern,-liga,-rclt,-rvrn,-dlig,-hlig"
for line in open(sys.argv[1]):
    p, i, cps = line.rstrip('\n').split('\t')
    extra = sys.argv[2:]  # e.g. --script=Zyyy
    out = subprocess.run([HB, "--no-glyph-names", "--features=" + FEAT, "--face-index=" + i, "-u", cps.replace(' ', ','), *extra, p], capture_output=True, text=True).stdout.strip()
    gl = []
    for g in out.strip('[]').split('|'):
        m = re.match(r'(\d+)=(\d+)(?:@(-?\d+),(-?\d+))?\+(-?\d+)', g)
        gid, cl, dx, dy, ax = m.groups()
        gl.append((int(gid), int(cl), int(dx or 0), int(dy or 0), int(ax)))
    pen = 0; placed = []
    for g in gl: placed.append((g, pen)); pen += g[4]
    left = {}
    for g, pn in placed: left[g[1]] = min(left.get(g[1], pn), pn)
    conv = "|".join("%d=%d@%d,%d" % (g[0], g[1], pn + g[2] - left[g[1]], g[3]) for g, pn in placed)
    print("%s\t%s\t%s\t[%s]\t%s" % (p, i, cps, conv, out))
