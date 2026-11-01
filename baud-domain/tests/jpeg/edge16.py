# A 16 by 8 16-bit grey picture whose neighbours differ by half the range
# and more, so a lossless difference reaches 32768 (SSSS 16) and the
# arithmetic coder's magnitude its top category.
import sys
w, h = 16, 8
rows = [
    [0, 32768] * 8,
    [32768, 0] * 8,
    [65535, 32767] * 8,
    [1, 32769] * 8,
]
seed = 4242
for y in range(4, h):
    row = []
    for x in range(w):
        seed = (seed * 1103515245 + 12345) & 0x7fffffff
        row.append(((seed >> 8) & 0xFFFF) ^ (0x8000 if (x + y) % 2 else 0))
    rows.append(row)
out = bytearray(b'P5\n%d %d\n65535\n' % (w, h))
for row in rows:
    for v in row:
        out += v.to_bytes(2, 'big')
open(sys.argv[1], 'wb').write(out)
