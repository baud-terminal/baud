# A deterministic 61 x 45 scene: gradients, a hard edge and a checker, so
# every sampling ratio and the IDCT see detail at block edges.
import sys
w, h = 61, 45
out = bytearray(b'P6\n%d %d\n255\n' % (w, h))
seed = 12345
for y in range(h):
    for x in range(w):
        seed = (seed * 1103515245 + 12345) & 0x7fffffff
        n = (seed >> 16) & 15
        r = (x * 4 + n) & 255
        g = (y * 5 + (40 if (x // 6 + y // 6) % 2 else 0)) & 255
        b = 230 if x > 30 and y < 20 else (x * y) & 255
        out += bytes([r, g, b])
open(sys.argv[1], 'wb').write(out)
