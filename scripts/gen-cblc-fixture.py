#!/usr/bin/env python3
"""Write `baud-font/tests/bitmap/cblc-cases.ttf`: a size for each arrangement
of CBLC index subtable records the colour-bitmap lookup refuses or reads,
except a nested record, which baud-font/tests/cblc.rs's surgeries and H1
cover. EBLC (`[[source: ms-opentype-eblc]]`): "Records must be sorted by
firstGlyphIndex, and records should not have overlapping glyph ID ranges". Each size
paints its images' left half in its own colour and the right half white,
as `sbix-strikes.ttf` does, so a rendering says which size drew.

| size | ppem | colour | records | what it is |
|---|---|---|---|---|
| 0 | 20 | green | [1..6] f1, [7..10] f3 | control: sorted, disjoint |
| 1 | 30 | red | [7..10] f1, [1..6] f1 | firsts out of order: refused |
| 2 | 40 | blue | [1..6] f1, [7..10] f1, numberOfIndexSubTables 100,000 | records past the table: refused |
| 3 | 50 | cyan / purple | [1..8] f1 cyan, [5..12] f1 purple | sorted, overlapping: first record that holds the glyph |
| 4 | 60 | magenta | [11..12] f1, [13..17] f1, range 11..17 | glyph 17 past numGlyphs (16), with a complete image |
| 5 | 80 | yellow | [1..13] f4, 13 images, n = 12 | the sentinel pair names glyph 13 and points at a whole image |

numGlyphs is 16. cmap (format 12) maps U+E000+g to g for g in 1..15 and
U+E011 to glyph 17, past the face, which only size 4 covers. No outlines.

Usage: gen-cblc-fixture.py <out.ttf>
"""
import struct
import sys
import zlib

UPEM = 800
GLYPHS = 16
WHITE = (240, 240, 240)
COLOUR = {
    "green": (40, 200, 40),
    "red": (220, 40, 40),
    "blue": (40, 80, 230),
    "cyan": (40, 200, 200),
    "purple": (150, 60, 200),
    "magenta": (200, 60, 200),
    "yellow": (230, 210, 30),
}


def chunk(kind, data):
    body = kind + data
    return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))


def png(ppem, colour):
    rows = b""
    half = ppem // 2
    for _ in range(ppem):
        rows += b"\x00"
        for x in range(ppem):
            r, g, b = COLOUR[colour] if x < half else WHITE
            rows += bytes((r, g, b, 255))
    ihdr = struct.pack(">IIBBBBB", ppem, ppem, 8, 6, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(rows, 9)) + chunk(b"IEND", b"")


def image17(ppem, colour):
    """Image format 17: smallGlyphMetrics (height, width, bearingX, bearingY,
    advance), dataLen, the PNG."""
    p = png(ppem, colour)
    return struct.pack(">BBbbB", ppem, ppem, 0, ppem, ppem) + struct.pack(">I", len(p)) + p


class Cbdt:
    def __init__(self):
        self.data = bytearray(struct.pack(">I", 0x00030000))

    def add(self, blob):
        at = len(self.data)
        self.data += blob
        return at


def subtable_f1(cbdt, glyphs, ppem, colour, short=False):
    """Index format 1 (u32 offsets) over `glyphs` consecutive ids, image
    format 17. short=True writes format 3 (u16 offsets)."""
    base = len(cbdt.data)
    offs = [0]
    for _ in glyphs:
        cbdt.add(image17(ppem, colour))
        offs.append(len(cbdt.data) - base)
    if short:
        body = b"".join(struct.pack(">H", o) for o in offs)
        if len(offs) % 2:
            body += b"\x00\x00"
        return struct.pack(">HHI", 3, 17, base) + body
    return struct.pack(">HHI", 1, 17, base) + b"".join(struct.pack(">I", o) for o in offs)


def subtable_f4(cbdt, glyphs, ppem, colour, n):
    """Index format 4 over `glyphs` (each with its own image); `n` pairs are
    counted, so pair n -- the sentinel the reader is told is there -- is the
    pair of glyphs[n], pointing at a whole image."""
    base = len(cbdt.data)
    pairs = []
    for g in glyphs:
        pairs.append((g, len(cbdt.data) - base))
        cbdt.add(image17(ppem, colour))
    pairs.append((0xFFFF, len(cbdt.data) - base))
    body = struct.pack(">I", n) + b"".join(struct.pack(">HH", g, o) for g, o in pairs)
    return struct.pack(">HHI", 4, 17, base) + body


def size_block(records, cbdt, ppem):
    """records: [(first, last, colour, kind, extra)] -> (record bytes, subtables)
    for one IndexSubTableList; offsets relative to the list's start."""
    n = len(records)
    head_len = 8 * n
    subs = b""
    recs = b""
    for first, last, colour, kind, extra in records:
        glyphs = list(range(first, last + 1))
        if kind == "f1":
            sub = subtable_f1(cbdt, glyphs, ppem, colour)
        elif kind == "f3":
            sub = subtable_f1(cbdt, glyphs, ppem, colour, short=True)
        elif kind == "f4":
            sub = subtable_f4(cbdt, glyphs, ppem, colour, extra)
        recs += struct.pack(">HHI", first, last, head_len + len(subs))
        subs += sub
        subs += b"\x00" * (-len(subs) % 4)
    return recs + subs


SIZES = [
    # ppem, start, end, records, numberOfIndexSubTables override
    (20, 1, 10, [(1, 6, "green", "f1", None), (7, 10, "green", "f3", None)], None),
    (30, 1, 10, [(7, 10, "red", "f1", None), (1, 6, "red", "f1", None)], None),
    (40, 1, 10, [(1, 6, "blue", "f1", None), (7, 10, "blue", "f1", None)], 100_000),
    (50, 1, 12, [(1, 8, "cyan", "f1", None), (5, 12, "purple", "f1", None)], None),
    (60, 11, 17, [(11, 12, "magenta", "f1", None), (13, 17, "magenta", "f1", None)], None),
    (80, 1, 13, [(1, 13, "yellow", "f4", 12)], None),
]


def cblc_cbdt():
    cbdt = Cbdt()
    blocks = []
    for ppem, start, end, records, count in SIZES:
        blocks.append((size_block(records, cbdt, ppem), len(records) if count is None else count))
    header = 8 + 48 * len(SIZES)
    out = struct.pack(">II", 0x00030000, len(SIZES))
    lists = b""
    for (ppem, start, end, records, _), (block, count) in zip(SIZES, blocks):
        list_at = header + len(lists)
        hori = vert = b"\x00" * 12  # sbitLineMetrics, unread
        out += struct.pack(">IIII", list_at, len(block), count, 0) + hori + vert
        out += struct.pack(">HHBBBb", start, end, ppem, ppem, 32, 1)
        lists += block
    return out + lists, bytes(cbdt.data)


def head():
    return struct.pack(">IIIIHHqqhhhhHHhhh", 0x00010000, 0x00010000, 0, 0x5F0F3CF5, 0, UPEM,
                       0, 0, 0, -200, UPEM, 800, 0, 8, 2, 0, 0)


def hhea():
    return struct.pack(">IhhhHhhhhhhhhhhhH", 0x00010000, 800, -200, 0, UPEM, 0, 0, UPEM, 1,
                       0, 0, 0, 0, 0, 0, 0, GLYPHS)


def maxp():
    return struct.pack(">IH", 0x00005000, GLYPHS)


def hmtx():
    return b"".join(struct.pack(">Hh", UPEM, 0) for _ in range(GLYPHS))


def cmap():
    groups = [(0xE000 + g, 0xE000 + g, g) for g in range(1, GLYPHS)] + [(0xE011, 0xE011, 17)]
    sub = struct.pack(">HHIII", 12, 0, 16 + 12 * len(groups), 0, len(groups))
    sub += b"".join(struct.pack(">III", *g) for g in groups)
    return struct.pack(">HHHHI", 0, 1, 3, 10, 12) + sub


def post():
    return struct.pack(">IIhhIIIII", 0x00030000, 0, 0, 0, 1, 0, 0, 0, 0)


def checksum(data):
    data += b"\x00" * (-len(data) % 4)
    return sum(struct.unpack(f">{len(data) // 4}I", data)) & 0xFFFFFFFF


def font():
    cblc, cbdt = cblc_cbdt()
    tables = {b"CBDT": cbdt, b"CBLC": cblc, b"cmap": cmap(), b"head": head(), b"hhea": hhea(),
              b"hmtx": hmtx(), b"maxp": maxp(), b"post": post()}
    n = len(tables)
    search = 1
    while search * 2 <= n:
        search *= 2
    selector = search.bit_length() - 1
    out = struct.pack(">IHHHH", 0x00010000, n, search * 16, selector, n * 16 - search * 16)
    at = 12 + 16 * n
    body = b""
    for tag in sorted(tables):
        data = tables[tag]
        out += struct.pack(">4sIII", tag, checksum(data), at, len(data))
        padded = data + b"\x00" * (-len(data) % 4)
        body += padded
        at += len(padded)
    whole = out + body
    adjust = (0xB1B0AFBA - checksum(whole)) & 0xFFFFFFFF
    head_at = whole.index(head()[:12], 12 + 16 * n)
    return whole[: head_at + 8] + struct.pack(">I", adjust) + whole[head_at + 12:]


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    with open(sys.argv[1], "wb") as f:
        f.write(font())
