#!/usr/bin/env python3
"""Writes `baud-font/tests/png/`: interlaced PNGs and their non-interlaced
twins, every colour type and bit depth PNG allows, for `tests/bitmap.rs`'s
Adam7 differential.

Each picture is written twice from the same samples, once with interlace
method 1 (Adam7) and once with method 0, so a decoder's two paths must agree
with each other as well as with the oracle. The encoder is this file's own,
from `[[source: w3c-png-2003]]`: the pass lattice from 8.2's 8-by-8 pattern,
scanlines packed and padded per reduced image (7.2), no filter byte for an
empty pass (9.1, 13.10), and the prior scanline taken as zero at the start
of every reduced image (9.2).

The sizes leave passes empty: 1x1 has only pass 1; 2x3 has passes with no
columns (2 and 4) and one with no rows (3); 7x9 and 33x17 have every pass,
33 leaving a partial byte below 8 bits. The first scanline of every pass
after the first non-empty one is filtered Up, Average or Paeth, so a decoder
that carries the previous pass's last row in as "prior" decodes it wrongly;
the script asserts that happens on rows whose would-be prior is non-zero.
The 7x9 pictures carry `tRNS`, and split their zlib stream across `IDAT`
chunks of five bytes, mid-pass; the 33x17 ones split it every 97 bytes.

Usage: python3 scripts/gen-adam7-fixtures.py [DIR]

DIR defaults to `baud-font/tests/png/`; the test that holds the committed
files to this script writes them elsewhere and compares.
"""

from __future__ import annotations

import os
import struct
import sys
import zlib

OUT = os.path.join(os.path.dirname(__file__), "..", "baud-font", "tests", "png")

# 8.2, as the specification prints it.
PATTERN = [
    [1, 6, 4, 6, 2, 6, 4, 6],
    [7, 7, 7, 7, 7, 7, 7, 7],
    [5, 6, 5, 6, 5, 6, 5, 6],
    [7, 7, 7, 7, 7, 7, 7, 7],
    [3, 6, 4, 6, 3, 6, 4, 6],
    [7, 7, 7, 7, 7, 7, 7, 7],
    [5, 6, 5, 6, 5, 6, 5, 6],
    [7, 7, 7, 7, 7, 7, 7, 7],
]

# (name, colour type, channels, depths) -- 11.2.2, table 11.1.
COLOURS = [
    ("g", 0, 1, [1, 2, 4, 8, 16]),
    ("rgb", 2, 3, [8, 16]),
    ("p", 3, 1, [1, 2, 4, 8]),
    ("ga", 4, 2, [8, 16]),
    ("rgba", 6, 4, [8, 16]),
]
SIZES = [(1, 1), (2, 3), (7, 9), (33, 17)]


def lattice(p: int) -> tuple[int, int, int, int]:
    """(x0, y0, dx, dy) of pass `p`, read off the pattern."""
    cells = [(r, c) for r in range(8) for c in range(8) if PATTERN[r][c] == p]
    y0, x0 = cells[0]
    cols = sorted({c for r, c in cells if r == y0})
    rows = sorted({r for r, c in cells if c == x0})
    dx = cols[1] - cols[0] if len(cols) > 1 else 8
    dy = rows[1] - rows[0] if len(rows) > 1 else 8
    return x0, y0, dx, dy


class Rng:
    def __init__(self, seed: int):
        self.s = seed or 1

    def next(self) -> int:
        # xorshift32
        s = self.s
        s ^= (s << 13) & 0xFFFFFFFF
        s ^= s >> 17
        s ^= (s << 5) & 0xFFFFFFFF
        self.s = s
        return s


def pack(row: list[list[int]], depth: int) -> bytes:
    """One scanline, samples big-endian, sub-byte samples high bits first,
    the last byte's unused low bits zero (7.2)."""
    samples = [s for px in row for s in px]
    if depth == 16:
        return b"".join(struct.pack(">H", s) for s in samples)
    if depth == 8:
        return bytes(samples)
    out, acc, n = bytearray(), 0, 0
    for s in samples:
        acc = (acc << depth) | s
        n += depth
        if n == 8:
            out.append(acc)
            acc, n = 0, 0
    if n:
        out.append(acc << (8 - n))
    return bytes(out)


def paeth(a: int, b: int, c: int) -> int:
    p = a + b - c
    pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
    if pa <= pb and pa <= pc:
        return a
    return b if pb <= pc else c


def filt(kind: int, line: bytes, prior: bytes, bpp: int) -> bytes:
    out = bytearray()
    for x, o in enumerate(line):
        a = line[x - bpp] if x >= bpp else 0
        b = prior[x]
        c = prior[x - bpp] if x >= bpp else 0
        pred = [0, a, b, (a + b) // 2, paeth(a, b, c)][kind]
        out.append((o - pred) & 0xFF)
    return bytes([kind]) + bytes(out)


def reduced_images(px, w, h, interlace):
    if interlace == 0:
        yield px
        return
    for p in range(1, 8):
        x0, y0, dx, dy = lattice(p)
        rows = [[px[y][x] for x in range(x0, w, dx)] for y in range(y0, h, dy)]
        if rows and rows[0]:
            yield rows


def encode(px, w, h, depth, channels, interlace, stats):
    bpp = max(1, depth * channels // 8)
    data, previous_last = bytearray(), None
    for k, image in enumerate(reduced_images(px, w, h, interlace)):
        lines = [pack(r, depth) for r in image]
        prior = bytes(len(lines[0]))  # 9.2: zero for the first scanline
        for j, line in enumerate(lines):
            if j == 0 and k > 0:
                kind = 2 + k % 3
                if kind in (2, 3, 4) and previous_last and any(previous_last):
                    stats["reset_tested"] += 1
            else:
                kind = (j + k) % 5
            data += filt(kind, line, prior, bpp)
            prior = line
        previous_last = lines[-1]
    return bytes(data)


def chunk(kind: bytes, body: bytes) -> bytes:
    return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body))


def png(w, h, depth, ctype, channels, interlace, px, plte, trns, split, stats):
    raw = encode(px, w, h, depth, channels, interlace, stats)
    z = zlib.compress(raw, 9)
    out = b"\x89PNG\r\n\x1a\n"
    out += chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, depth, ctype, 0, 0, interlace))
    if plte is not None:
        out += chunk(b"PLTE", plte)
    if trns is not None:
        out += chunk(b"tRNS", trns)
    step = split or len(z)
    for i in range(0, len(z), step):
        out += chunk(b"IDAT", z[i : i + step])
    out += chunk(b"IEND", b"")
    return out


def main() -> int:
    out = sys.argv[1] if len(sys.argv) > 1 else OUT
    os.makedirs(out, exist_ok=True)
    stats = {"reset_tested": 0}
    seed = 0x9E3779B9
    written = 0
    for name, ctype, channels, depths in COLOURS:
        for depth in depths:
            for w, h in SIZES:
                seed = (seed * 1103515245 + 12345) & 0xFFFFFFFF
                rng = Rng(seed)
                entries = min(1 << depth, 200)
                top = entries if ctype == 3 else 1 << depth
                px = [
                    [[rng.next() % top for _ in range(channels)] for _ in range(w)]
                    for _ in range(h)
                ]
                plte = trns = None
                if ctype == 3:
                    plte = bytes(rng.next() & 0xFF for _ in range(entries * 3))
                tagged = (w, h) == (7, 9)
                if tagged and ctype == 3:
                    trns = bytes(rng.next() & 0xFF for _ in range(max(1, entries // 2)))
                elif tagged and ctype == 0:
                    trns = struct.pack(">H", px[0][0][0])
                elif tagged and ctype == 2:
                    trns = struct.pack(">HHH", *px[0][0])
                split = {(7, 9): 5, (33, 17): 97}.get((w, h))
                for interlace, tag in ((1, "i"), (0, "n")):
                    path = os.path.join(out, f"{name}{depth}-{w}x{h}-{tag}.png")
                    with open(path, "wb") as f:
                        f.write(png(w, h, depth, ctype, channels, interlace, px, plte, trns, split, stats))
                    written += 1
    # Every pass after the first starts on a row filtered against "prior";
    # count those whose wrongly-carried prior would have been non-zero.
    assert stats["reset_tested"] >= 100, stats
    print(f"{written} files, {stats['reset_tested']} pass starts test the prior reset")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
