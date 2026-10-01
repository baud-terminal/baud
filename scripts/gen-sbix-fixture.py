#!/usr/bin/env python3
"""Write a small committed `sbix` fixture: every case a per-glyph strike
choice and Apple's undocumented `'flip'` have to get right.

Apple Color Emoji is where these cases live -- 177 glyphs empty in some of
its nine strikes and present in others, 108 `'flip'` records a strike -- but
it is a system file on one platform, so the tests that must run everywhere
read this instead. Four strikes, at 10, 20, 40 and 80 ppem; each strike's
images paint their left half in that strike's colour and their right half
white, so a rendering says which strike it came from and whether it was
mirrored.

| glyph | codepoint | what each strike holds |
|---|---|---|
| 1 A | U+E001 | `png ` in every strike |
| 2 B | U+E002 | `png ` at 80 only |
| 3 C | U+E003 | `flip` to A in every strike, at offsets (7, 3) A does not have |
| 4 D | U+E004 | `flip` to J, which is a `dupe` of A |
| 5 E | U+E005 | `png ` everywhere, but at 40 a PNG whose zlib stream is invalid |
| 6 F | U+E006 | nothing anywhere |
| 7 G | U+E007 | `flip` to C: two flips on the way to A |
| 8 H | U+E008 | `png ` at 10 only |
| 9 I | U+E009 | `dupe` to itself |
| 10 J | U+E00A | `dupe` to A |

Every record but C's sits at offsets (0, 0); every image is ppem x ppem.
The corrupt PNG's chunks carry correct CRCs, so it parses and only its
inflate fails. `head`, `hhea`, `maxp`, `hmtx`, `cmap` (format 12) and
`post` (version 3) are written from scratch; there are no outlines.

Usage: gen-sbix-fixture.py <out.ttf>
"""

import struct
import sys
import zlib

UPEM = 800
STRIKES = [10, 20, 40, 80]
COLOUR = {10: (220, 40, 40), 20: (40, 200, 40), 40: (40, 80, 230), 80: (230, 210, 30)}
WHITE = (240, 240, 240)
GLYPHS = 11


def chunk(kind, data):
    body = kind + data
    return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))


def png(ppem, corrupt=False):
    rows = b""
    half = ppem // 2
    for _ in range(ppem):
        rows += b"\x00"
        for x in range(ppem):
            r, g, b = COLOUR[ppem] if x < half else WHITE
            rows += bytes((r, g, b, 255))
    ihdr = struct.pack(">IIBBBBB", ppem, ppem, 8, 6, 0, 0, 0)
    # A zlib header, then a block whose type is the reserved 0b11.
    idat = b"\x78\x9c\xff\xff\xff\xff" if corrupt else zlib.compress(rows, 9)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", idat) + chunk(b"IEND", b"")


def record(kind, data, x=0, y=0):
    return struct.pack(">hh4s", x, y, kind) + data


def ref(g):
    return struct.pack(">H", g)


def glyph_records(ppem):
    """The record each glyph holds in this strike, or None for nothing."""
    out = [None] * GLYPHS
    out[1] = record(b"png ", png(ppem))
    out[2] = record(b"png ", png(ppem)) if ppem == 80 else None
    out[3] = record(b"flip", ref(1), x=7, y=3)
    out[4] = record(b"flip", ref(10))
    out[5] = record(b"png ", png(ppem, corrupt=(ppem == 40)))
    out[7] = record(b"flip", ref(3))
    out[8] = record(b"png ", png(ppem)) if ppem == 10 else None
    out[9] = record(b"dupe", ref(9))
    out[10] = record(b"dupe", ref(1))
    return out


def sbix():
    strikes = []
    for ppem in STRIKES:
        recs = glyph_records(ppem)
        head = 4 + 4 * (GLYPHS + 1)
        offsets, data, at = [], b"", head
        for r in recs:
            offsets.append(at)
            if r:
                data += r
                at += len(r)
        offsets.append(at)
        strikes.append(struct.pack(">HH", ppem, 72) + struct.pack(f">{GLYPHS + 1}I", *offsets) + data)
    header = 8 + 4 * len(STRIKES)
    out, at, offs = b"", header, []
    for s in strikes:
        offs.append(at)
        out += s
        at += len(s)
    return struct.pack(">HHI", 1, 1, len(STRIKES)) + struct.pack(f">{len(STRIKES)}I", *offs) + out


def head():
    return struct.pack(
        ">IIIIHHqqhhhhHHhhh",
        0x00010000, 0x00010000, 0, 0x5F0F3CF5, 0, UPEM,
        0, 0, 0, -200, UPEM, 800, 0, 8, 2, 0, 0,
    )


def hhea():
    return struct.pack(
        ">IhhhHhhhhhhhhhhhH",
        0x00010000, 800, -200, 0, UPEM, 0, 0, UPEM, 1, 0, 0, 0, 0, 0, 0, 0, GLYPHS,
    )


def maxp():
    return struct.pack(">IH", 0x00005000, GLYPHS)


def hmtx():
    return b"".join(struct.pack(">Hh", UPEM, 0) for _ in range(GLYPHS))


def cmap():
    groups = [(0xE000 + g, 0xE000 + g, g) for g in range(1, GLYPHS)]
    sub = struct.pack(">HHIII", 12, 0, 16 + 12 * len(groups), 0, len(groups))
    sub += b"".join(struct.pack(">III", *g) for g in groups)
    return struct.pack(">HHHHI", 0, 1, 3, 10, 12) + sub


def post():
    return struct.pack(">IIhhIIIII", 0x00030000, 0, 0, 0, 1, 0, 0, 0, 0)


def checksum(data):
    data += b"\x00" * (-len(data) % 4)
    return sum(struct.unpack(f">{len(data) // 4}I", data)) & 0xFFFFFFFF


def font():
    tables = {
        b"cmap": cmap(),
        b"head": head(),
        b"hhea": hhea(),
        b"hmtx": hmtx(),
        b"maxp": maxp(),
        b"post": post(),
        b"sbix": sbix(),
    }
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
    return whole[: head_at + 8] + struct.pack(">I", adjust) + whole[head_at + 12 :]


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    with open(sys.argv[1], "wb") as f:
        f.write(font())
