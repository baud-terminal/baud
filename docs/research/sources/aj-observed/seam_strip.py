"""The band seam's strip: for each size and page, the frame before the fix
above the frame after it, cropped where the two differ most and magnified,
and the luminance down the crop's columns.

Usage: seam_strip.py <frames dir> <out.ppm> <profile out.txt>
Reads frame-<size>-<page>-<when>-<before|after>.raw: two little-endian u32
(width, height), then width x height ARGB words.
"""

import struct
import sys
from pathlib import Path


def frame(p):
    b = Path(p).read_bytes()
    w, h = struct.unpack_from("<II", b, 0)
    return w, h, struct.unpack_from(f"<{w * h}I", b, 8)


def lum(p):
    def c(v):
        s = (v & 255) / 255.0
        return s / 12.92 if s <= 0.04045 else ((s + 0.055) / 1.055) ** 2.4
    return 0.2126 * c(p >> 16) + 0.7152 * c(p >> 8) + 0.0722 * c(p)


def main():
    d, out, prof = Path(sys.argv[1]), sys.argv[2], sys.argv[3]
    cases = [("1x-dark", 4), ("1x-light", 4), ("2x-dark", 4), ("2x-light", 4)]
    CW, CH = 200, 24
    strips, lines = [], []
    for name, mag in cases:
        w, h, a = frame(d / f"frame-{name}-in-before.raw")
        _, _, b = frame(d / f"frame-{name}-in-after.raw")
        diff = [abs(lum(a[i]) - lum(b[i])) if a[i] != b[i] else 0.0 for i in range(w * h)]
        rows = sorted({i // w for i in range(w * h) if diff[i] > 0})
        best = None
        for y in rows:
            row = diff[y * w:(y + 1) * w]
            run = sum(row[:CW])
            for x0 in range(0, w - CW):
                if best is None or run > best[0]:
                    best = (run, x0, y)
                run += row[x0 + CW] - row[x0]
        _, x0, y = best
        y0 = max(0, min(h - CH, y - CH // 2))
        lines.append(f"{name}: {w}x{h} px, rows that differ {len(rows)}: {rows}")
        lines.append(f"  crop x {x0}..{x0 + CW}, y {y0}..{y0 + CH}, the seam row {y}, magnified {mag}x")
        lines.append("  row  mean luminance before  after")
        for yy in range(y0, y0 + CH):
            mb = sum(lum(a[yy * w + x]) for x in range(x0, x0 + CW)) / CW
            ma = sum(lum(b[yy * w + x]) for x in range(x0, x0 + CW)) / CW
            mark = "  <- the doubled row" if yy == y else ""
            lines.append(f"  {yy:4d}  {mb:.5f}  {ma:.5f}{mark}")
        for px in (a, b):
            img = []
            for yy in range(y0, y0 + CH):
                line = []
                for x in range(x0, x0 + CW):
                    p = px[yy * w + x]
                    line.extend([bytes(((p >> 16) & 255, (p >> 8) & 255, p & 255))] * mag)
                img.extend([b"".join(line)] * mag)
            strips.append((CW * mag, img))
    width = max(s[0] for s in strips)
    gap = b"\x80\x80\x80" * width
    rows_out = []
    for i, (sw, img) in enumerate(strips):
        pad = b"\x80\x80\x80" * (width - sw)
        rows_out.extend(r + pad for r in img)
        rows_out.extend([gap] * (4 if i % 2 == 0 else 16))
    hdr = f"P6 {width} {len(rows_out)} 255\n".encode()
    Path(out).write_bytes(hdr + b"".join(rows_out))
    Path(prof).write_text("\n".join(lines) + "\n")


main()
