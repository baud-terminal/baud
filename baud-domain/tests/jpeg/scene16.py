# The scene of scene.py at 16 bits: each sample its 8-bit value times 257
# plus a deterministic eight-bit dither, so the low byte carries detail.
import sys
w, h = 61, 45
out = bytearray(b'P6\n%d %d\n65535\n' % (w, h))
seed = 12345
for y in range(h):
    for x in range(w):
        seed = (seed * 1103515245 + 12345) & 0x7fffffff
        n = (seed >> 16) & 15
        r = (x * 4 + n) & 255
        g = (y * 5 + (40 if (x // 6 + y // 6) % 2 else 0)) & 255
        b = 230 if x > 30 and y < 20 else (x * y) & 255
        for i, v in enumerate((r, g, b)):
            seed = (seed * 1103515245 + 12345) & 0x7fffffff
            out += (v * 257 ^ ((seed >> 16) & 255)).to_bytes(2, 'big')
open(sys.argv[1], 'wb').write(out)
