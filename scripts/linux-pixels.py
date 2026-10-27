#!/usr/bin/env python3
"""Compare frames pixel for pixel: the `pixels` bench's PAMs and a
compositor's PNG screenshots (scripts/linux-pixels.sh drives it).

Standard library only, so it runs on the Mac and in the container alike.

  linux-pixels.py over-black <in.pam> <out.pam>
      the frame as a compositor shows it over a black output: each channel
      premultiplied, rounded as both presenters round, (c a + 127) / 255
  linux-pixels.py crop <in.png|pam> <x> <y> <w> <h> <out.pam>
  linux-pixels.py png <in.pam|png> <out.png> [scale]
  linux-pixels.py diff <a> <b> [<diff.png>]
      prints the count of pixels that differ, the largest channel
      difference, the per-channel histogram of differences and the box
      that holds them; writes the differing pixels in white over a dark
      copy of <a>, scaled up four times, when a path is given. Exits 1 when
      any pixel differs or the sizes differ, 0 when the two are identical.

Alpha is compared too: a PAM's fourth channel against a PNG's, where a PNG
with no alpha reads as opaque.
"""

import struct
import sys
import zlib


def read_pam(path):
    data = open(path, "rb").read()
    end = data.index(b"ENDHDR\n") + len(b"ENDHDR\n")
    head = dict(
        line.split(" ", 1)
        for line in data[:end].decode().splitlines()[1:-1]
        if " " in line
    )
    w, h, depth = int(head["WIDTH"]), int(head["HEIGHT"]), int(head["DEPTH"])
    body = data[end:]
    assert len(body) == w * h * depth, (path, len(body), w, h, depth)
    if depth == 3:
        body = b"".join(body[i : i + 3] + b"\xff" for i in range(0, len(body), 3))
    assert depth in (3, 4), (path, depth)
    return w, h, bytearray(body)


def paeth(a, b, c):
    p = a + b - c
    pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
    if pa <= pb and pa <= pc:
        return a
    return b if pb <= pc else c


def read_png(path):
    data = open(path, "rb").read()
    assert data[:8] == b"\x89PNG\r\n\x1a\n", path
    at, idat = 8, b""
    while at < len(data):
        (n,) = struct.unpack(">I", data[at : at + 4])
        kind, body = data[at + 4 : at + 8], data[at + 8 : at + 8 + n]
        at += 12 + n
        if kind == b"IHDR":
            w, h, depth, ctype, _, _, interlace = struct.unpack(">IIBBBBB", body)
            assert depth == 8 and ctype in (2, 6) and interlace == 0, (depth, ctype)
        elif kind == b"IDAT":
            idat += body
    raw = zlib.decompress(idat)
    bpp = 4 if ctype == 6 else 3
    stride = w * bpp
    out = bytearray(w * h * 4)
    prev = bytearray(stride)
    for y in range(h):
        f = raw[y * (stride + 1)]
        line = bytearray(raw[y * (stride + 1) + 1 : (y + 1) * (stride + 1)])
        for i in range(stride):
            a = line[i - bpp] if i >= bpp else 0
            b = prev[i]
            c = prev[i - bpp] if i >= bpp else 0
            if f == 1:
                line[i] = (line[i] + a) & 255
            elif f == 2:
                line[i] = (line[i] + b) & 255
            elif f == 3:
                line[i] = (line[i] + (a + b) // 2) & 255
            elif f == 4:
                line[i] = (line[i] + paeth(a, b, c)) & 255
        for x in range(w):
            px = line[x * bpp : x * bpp + bpp]
            out[(y * w + x) * 4 : (y * w + x) * 4 + 4] = px if bpp == 4 else px + b"\xff"
        prev = line
    return w, h, out


def read(path):
    return read_png(path) if path.endswith(".png") else read_pam(path)


def write_pam(path, w, h, px):
    head = f"P7\nWIDTH {w}\nHEIGHT {h}\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR\n"
    open(path, "wb").write(head.encode() + bytes(px))


def write_png(path, w, h, px, scale=1):
    rows = []
    for y in range(h * scale):
        sy = y // scale
        line = bytearray()
        for x in range(w):
            p = px[(sy * w + x) * 4 : (sy * w + x) * 4 + 3]
            line += p * scale
        rows.append(b"\x00" + bytes(line))
    W, H = w * scale, h * scale

    def chunk(kind, body):
        c = struct.pack(">I", len(body)) + kind + body
        return c + struct.pack(">I", zlib.crc32(kind + body) & 0xFFFFFFFF)

    open(path, "wb").write(
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", W, H, 8, 2, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(b"".join(rows), 9))
        + chunk(b"IEND", b"")
    )


def over_black(px):
    out = bytearray(px)
    for i in range(0, len(px), 4):
        a = px[i + 3]
        for k in range(3):
            out[i + k] = (px[i + k] * a + 127) // 255
        out[i + 3] = 255
    return out


def crop(w, h, px, x0, y0, cw, ch):
    assert x0 + cw <= w and y0 + ch <= h, (w, h, x0, y0, cw, ch)
    out = bytearray()
    for y in range(y0, y0 + ch):
        out += px[(y * w + x0) * 4 : (y * w + x0 + cw) * 4]
    return out


def diff(a, b, out=None):
    (wa, ha, pa), (wb, hb, pb) = read(a), read(b)
    if (wa, ha) != (wb, hb):
        print(f"sizes differ: {wa}x{ha} against {wb}x{hb}")
        return 1
    n, worst, hist = 0, 0, {}
    box = [wa, ha, -1, -1]
    mark = bytearray(wa * ha * 4)
    for i in range(0, len(pa), 4):
        d = max(abs(pa[i + k] - pb[i + k]) for k in range(4))
        p = i // 4
        x, y = p % wa, p // wa
        if d:
            n += 1
            worst = max(worst, d)
            hist[d] = hist.get(d, 0) + 1
            box = [min(box[0], x), min(box[1], y), max(box[2], x), max(box[3], y)]
            mark[i : i + 4] = b"\xff\xff\xff\xff"
        else:
            mark[i : i + 3] = bytes(c // 4 for c in pa[i : i + 3])
            mark[i + 3] = 255
    total = wa * ha
    print(f"{n} of {total} pixels differ, largest channel difference {worst}")
    if n:
        top = sorted(hist.items(), key=lambda kv: -kv[1])[:8]
        print("  by difference: " + ", ".join(f"{d}: {c}" for d, c in top))
        print(f"  within x {box[0]}..{box[2]}, y {box[1]}..{box[3]}")
    if out:
        write_png(out, wa, ha, mark, 4)
    return 1 if n else 0


def main(argv):
    cmd = argv[1]
    if cmd == "over-black":
        w, h, px = read(argv[2])
        write_pam(argv[3], w, h, over_black(px))
    elif cmd == "crop":
        w, h, px = read(argv[2])
        x0, y0, cw, ch = map(int, argv[3:7])
        write_pam(argv[7], cw, ch, crop(w, h, px, x0, y0, cw, ch))
    elif cmd == "png":
        w, h, px = read(argv[2])
        write_png(argv[3], w, h, px, int(argv[4]) if len(argv) > 4 else 1)
    elif cmd == "diff":
        return diff(argv[2], argv[3], argv[4] if len(argv) > 4 else None)
    else:
        print(__doc__)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
