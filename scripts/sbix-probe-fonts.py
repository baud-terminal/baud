#!/usr/bin/env python3
"""Build modified copies of Apple Color Emoji's face 0 for scripts/sbix-probe.swift.

The system face answers what CoreText does with the records Apple shipped:
every record at offsets (0, 0), every `flip` pointing at a `png `, every
image ppem x ppem, every advance 800 of 800. To ask whose offsets a flip
uses, where its mirror axis is, and what a chain of flips and dupes draws,
the records have to differ; to ask what CoreText draws when strikes are
empty, they have to be emptied. These fonts are those edits, made to the
sbix, hmtx and name tables of a copy and nothing else. Each is a standalone
TrueType file of about 192 MB, so the output directory must be outside the
repository; the script refuses one inside it.

Each copy is renamed -- "Emoji" becomes a five-letter tag in every name
string -- so no two of them, and none of them and the system font, share a
PostScript name. Load them with FONT_FILE=<path>, which goes through CGFont
and CTFontCreateWithGraphicsFont: that path draws on the baseline at the
record's offsets, with none of the system font's own drop or enlargement.

sbix-a.ttf, "AppleColorEmojA": offsets, advances and the mirror axis

| glyph | edit | what it asks |
|---|---|---|
| 838 | none: flip -> 837 | control: a flip draws mirrored |
| 852 | dupe -> 837 | control: a dupe draws its target upright |
| 846 | advance 800 -> 1000 | control: an advance does not move an image |
| 839 | flip -> 836 at its own offsets (6, 4) | does the flip's own offset apply |
| 840 | its png at offsets (6, 4) | the target alone at offsets |
| 841 | none: flip -> 840 | does the target's offset apply through a flip |
| 842 | its png at offsets (-4, 3) | the target alone |
| 845 | flip -> 842 at its own offsets (10, 2) | both carry offsets |
| 844 | advance 800 -> 1000; flip -> 843 | is the axis the flip's advance |
| 847 | none: flip -> 846, whose advance is 1000 | is the axis the target's advance |
| 867 | its png cropped to its left 3/4 | a narrower image alone |
| 868 | none: flip -> 867 | is the axis the image's own width |

sbix-c.ttf, "AppleColorEmojC": chains

| glyph | edit | the chain |
|---|---|---|
| 850 | flip -> 838 | flip -> flip -> png 837 |
| 862 | flip -> 859, and 859 flip -> 856 | flip -> flip -> flip -> png 855 |
| 853 | none: flip -> 852, and 852 dupe -> 837 | flip -> dupe -> png 837 |
| 857 | dupe -> 838 | dupe -> flip -> png 837 |
| 865 | dupe -> 852 | dupe -> dupe -> png 837 |

sbix-f.ttf, "AppleColorEmojF": strike fallback

| glyph | edit |
|---|---|
| 2097 U+1F601 | empty at 64, 96 and 160 |
| 2098 U+1F602 | data at 20 and 160 only |
| 2099 U+1F603 | data at 20 and 26 only |
| 2100 U+1F604 | empty in every strike |
| 2101 U+1F605 | data at 64 only |

Every edit applies in every strike unless the table names strikes. The
crop is the left floor(0.75 ppem + 0.5) columns of each strike's image --
48 at 64, 30 at 40 -- decoded and re-encoded here, so its bytes are not
CoreGraphics' re-encoding; the Swift probe reads its references from the
font it draws, so the comparison is the same bytes either way.

The advances are written by rebuilding hmtx: face 0's numberOfHMetrics is
76, so glyphs 844 and 846 have no advance of their own, and writing 1000 at
4 * glyph -- which the first version of these fonts did -- sets two other
glyphs' left side bearings and leaves every advance at 800. The rebuilt
table carries long metrics through one glyph past the last edited one,
because every glyph after the long metrics takes the last one's advance.
Each written font is read back: every advance and every edited record must
be what was asked, or the script stops.

Before editing, every glyph above is checked to hold the records the cases
assume (a png, or a flip to the named glyph, in every strike), so a newer
Apple Color Emoji that renumbers them stops here instead of building a font
that asks something else. Table checksums and head.checkSumAdjustment are
written correctly.

Standard library only. Usage:

    scripts/sbix-probe-fonts.py <out-dir> [source-font]

source-font defaults to /System/Library/Fonts/Apple Color Emoji.ttc; face 0
is used. The script prints the cases and the probe commands that ask them.
"""

import array
import os
import struct
import sys
import zlib

SYSTEM = "/System/Library/Fonts/Apple Color Emoji.ttc"
PNG_SIG = b"\x89PNG\r\n\x1a\n"


def u16(d, o):
    return struct.unpack_from(">H", d, o)[0]


def u32(d, o):
    return struct.unpack_from(">I", d, o)[0]


class Face:
    """Face 0 of a TTC, or a TTF: its table directory and its sbix strikes."""

    def __init__(self, path):
        with open(path, "rb") as f:
            self.data = d = f.read()
        base = u32(d, 12) if d[:4] == b"ttcf" else 0
        self.tables = {}
        for i in range(u16(d, base + 4)):
            r = base + 12 + 16 * i
            self.tables[d[r : r + 4].decode("latin1")] = (u32(d, r + 8), u32(d, r + 12))
        for tag in ("maxp", "hhea", "hmtx", "head", "name", "sbix"):
            if tag not in self.tables:
                sys.exit(f"{path} has no {tag} table")
        self.num_glyphs = u16(d, self.tables["maxp"][0] + 4)
        self.nhm = u16(d, self.tables["hhea"][0] + 34)
        sb, sblen = self.tables["sbix"]
        count = u32(d, sb + 4)
        if 8 + 4 * count > sblen:
            sys.exit(f"sbix claims {count} strikes, more than its table holds")
        self.sbix_version_flags = d[sb : sb + 4]
        self.strikes = []
        for s in range(count):
            so = sb + u32(d, sb + 8 + 4 * s)
            offs = struct.unpack_from(f">{self.num_glyphs + 1}I", d, so + 4)
            self.strikes.append((u16(d, so), u16(d, so + 2), so, offs))
        self.ppems = [p for p, _, _, _ in self.strikes]

    def table(self, tag):
        o, n = self.tables[tag]
        return self.data[o : o + n]

    def record(self, ppem, g):
        """The record's bytes: offsets, graphic type and payload; b'' when empty."""
        for p, _, so, offs in self.strikes:
            if p == ppem:
                return self.data[so + offs[g] : so + offs[g + 1]]
        raise KeyError(ppem)

    def advance(self, g):
        return u16(self.data, self.tables["hmtx"][0] + 4 * min(g, self.nhm - 1))


def kind(rec):
    return rec[4:8] if len(rec) >= 8 else None


def target(rec):
    return u16(rec, 8)


def flip_to(g, ox=0, oy=0):
    return struct.pack(">hh", ox, oy) + b"flip" + struct.pack(">H", g)


def dupe_to(g, ox=0, oy=0):
    return struct.pack(">hh", ox, oy) + b"dupe" + struct.pack(">H", g)


def at_offsets(rec, ox, oy):
    return struct.pack(">hh", ox, oy) + rec[4:]


# ---- PNG: decode, crop to the left columns, re-encode --------------------


def png_chunks(png):
    if png[:8] != PNG_SIG:
        raise ValueError("not a PNG")
    pos, out = 8, []
    while pos < len(png):
        n = u32(png, pos)
        out.append((png[pos + 4 : pos + 8], png[pos + 8 : pos + 8 + n]))
        pos += 12 + n
    return out


def chunk(tag, body):
    return struct.pack(">I", len(body)) + tag + body + struct.pack(">I", zlib.crc32(tag + body))


def png_rows(png):
    """Unfiltered rows of an 8-bit, non-interlaced PNG of any colour type."""
    chunks = png_chunks(png)
    w, h, depth, colour, comp, filt, lace = struct.unpack(">IIBBBBB", chunks[0][1])
    if depth != 8 or lace != 0 or comp != 0 or filt != 0:
        raise ValueError(f"unsupported PNG: depth {depth}, interlace {lace}")
    bpp = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}[colour]
    raw = zlib.decompress(b"".join(b for t, b in chunks if t == b"IDAT"))
    stride = w * bpp
    rows, prev, p = [], bytearray(stride), 0
    for _ in range(h):
        f = raw[p]
        line = bytearray(raw[p + 1 : p + 1 + stride])
        p += 1 + stride
        for i in range(stride):
            a = line[i - bpp] if i >= bpp else 0
            b = prev[i]
            c = prev[i - bpp] if i >= bpp else 0
            if f == 0:
                break
            if f == 1:
                line[i] = (line[i] + a) & 255
            elif f == 2:
                line[i] = (line[i] + b) & 255
            elif f == 3:
                line[i] = (line[i] + (a + b) // 2) & 255
            elif f == 4:
                pa, pb, pc = abs(b - c), abs(a - c), abs(a + b - 2 * c)
                pred = a if pa <= pb and pa <= pc else (b if pb <= pc else c)
                line[i] = (line[i] + pred) & 255
            else:
                raise ValueError(f"filter type {f}")
        rows.append(bytes(line))
        prev = line
    return chunks, (w, h, colour, bpp), rows


def png_crop_left(png, width):
    chunks, (w, h, colour, bpp), rows = png_rows(png)
    if not 0 < width <= w:
        raise ValueError(f"crop to {width} of {w}")
    body = b"".join(b"\x00" + r[: width * bpp] for r in rows)
    out = PNG_SIG + chunk(b"IHDR", struct.pack(">IIBBBBB", width, h, 8, colour, 0, 0, 0))
    for t, b in chunks:
        if t in (b"PLTE", b"tRNS"):
            out += chunk(t, b)
    out += chunk(b"IDAT", zlib.compress(body, 9)) + chunk(b"IEND", b"")
    # The round trip: the new image's rows are the old image's left columns.
    _, (w2, h2, _, _), rows2 = png_rows(out)
    assert (w2, h2) == (width, h) and rows2 == [r[: width * bpp] for r in rows]
    return out


# ---- writing a TrueType file --------------------------------------------


def checksum(b):
    b = b + b"\0" * (-len(b) % 4)
    a = array.array("I", b)
    assert a.itemsize == 4
    if sys.byteorder == "little":
        a.byteswap()
    return sum(a) & 0xFFFFFFFF


def build_sbix(face, edits):
    """edits: {(ppem or None, glyph): record bytes}. Returns the table's bytes."""
    n = face.num_glyphs
    blobs = []
    for ppem, ppi, _, _ in face.strikes:
        recs = []
        for g in range(n):
            key = (ppem, g) if (ppem, g) in edits else (None, g)
            recs.append(edits[key] if key in edits else face.record(ppem, g))
        offs, at = [], 4 + 4 * (n + 1)
        for r in recs:
            offs.append(at)
            at += len(r)
        offs.append(at)
        blobs.append(struct.pack(">HH", ppem, ppi) + struct.pack(f">{n + 1}I", *offs) + b"".join(recs))
    at, starts = 8 + 4 * len(blobs), []
    for b in blobs:
        starts.append(at)
        at += len(b)
    head = face.sbix_version_flags + struct.pack(">I", len(blobs))
    return head + struct.pack(f">{len(blobs)}I", *starts) + b"".join(blobs)


def hmtx_with(face, advances):
    """hmtx with the given advances. Apple Color Emoji's numberOfHMetrics is
    76, so a glyph past 75 has no advance of its own: it takes glyph 75's, and
    the four bytes at 4 * glyph are other glyphs' left side bearings. The
    table is rebuilt with enough long metrics to hold every edited glyph."""
    d, hm = face.data, face.tables["hmtx"][0]

    def lsb(g):
        at = hm + 4 * g + 2 if g < face.nhm else hm + 4 * face.nhm + 2 * (g - face.nhm)
        return struct.unpack_from(">h", d, at)[0]

    # One long metric past the last edited glyph: every glyph after the long
    # metrics takes the last one's advance, which must stay the original.
    nhm = min(face.num_glyphs, max(face.nhm, max(advances) + 2))
    assert nhm == face.num_glyphs or face.advance(nhm - 1) == face.advance(face.num_glyphs - 1)
    out = b"".join(struct.pack(">Hh", advances.get(g, face.advance(g)), lsb(g)) for g in range(nhm))
    return bytearray(out + b"".join(struct.pack(">h", lsb(g)) for g in range(nhm, face.num_glyphs)))


def write_font(face, path, edits, advances, tag5):
    tables = {t: bytearray(face.table(t)) for t in face.tables if t != "sbix"}
    tables["sbix"] = build_sbix(face, edits)
    name = bytes(tables["name"])
    renamed = name.replace(b"Emoji", tag5).replace("Emoji".encode("utf-16-be"), tag5.decode().encode("utf-16-be"))
    if renamed == name:
        sys.exit("the name table holds no 'Emoji' to rename")
    tables["name"] = renamed
    if advances:
        tables["hmtx"] = hmtx_with(face, advances)
        struct.pack_into(">H", tables["hhea"], 34, len(tables["hmtx"]) // 2 - face.num_glyphs)
    struct.pack_into(">I", tables["head"], 8, 0)  # checkSumAdjustment, set below
    order = sorted(tables)
    n = len(order)
    es = max(i for i in range(8) if (1 << i) <= n)
    header = struct.pack(">IHHHH", 0x00010000, n, (1 << es) * 16, es, n * 16 - (1 << es) * 16)
    pos, entries, sums = 12 + 16 * n, b"", 0
    for t in order:
        b = bytes(tables[t])
        s = checksum(b)
        sums += s
        entries += t.encode("latin1") + struct.pack(">III", s, pos, len(b))
        pos += len(b) + (-len(b) % 4)
    adjust = (0xB1B0AFBA - (checksum(header + entries) + sums)) & 0xFFFFFFFF
    struct.pack_into(">I", tables["head"], 8, adjust)
    with open(path, "wb") as f:
        f.write(header + entries)
        for t in order:
            b = bytes(tables[t])
            f.write(b + b"\0" * (-len(b) % 4))
    # Read the file back: every advance is the one asked for, and every edited
    # record is the bytes it was given.
    back = Face(path)
    for g in range(face.num_glyphs):
        if back.advance(g) != advances.get(g, face.advance(g)):
            sys.exit(f"{path}: glyph {g} reads advance {back.advance(g)}")
    for (p, g), rec in edits.items():
        for q in [p] if p is not None else face.ppems:
            if back.record(q, g) != rec:
                sys.exit(f"{path}: glyph {g} at {q} ppem does not read back")
    print(f"wrote {path}: {os.path.getsize(path)} bytes, PostScript name AppleColor{tag5.decode()}")


# ---- the cases -----------------------------------------------------------


def expect(face, g, what, tgt=None):
    """Every strike holds `what` for g (a png, or a flip/dupe to tgt)."""
    for p in face.ppems:
        r = face.record(p, g)
        if kind(r) != what or (tgt is not None and target(r) != tgt):
            sys.exit(f"glyph {g} at {p} ppem is not {what.decode()}{'' if tgt is None else f' -> {tgt}'}; this font is not the one the cases were written for")


def main():
    if len(sys.argv) not in (2, 3):
        sys.exit("usage: sbix-probe-fonts.py <out-dir> [source-font]")
    repo = os.path.realpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), os.pardir))
    out = os.path.realpath(sys.argv[1])
    if out == repo or out.startswith(repo + os.sep):
        sys.exit(f"{out} is inside the repository; each font is about 192 MB")
    os.makedirs(out, exist_ok=True)
    src = sys.argv[2] if len(sys.argv) == 3 else SYSTEM
    face = Face(src)
    print(f"source: {src} face 0, {face.num_glyphs} glyphs, strikes {face.ppems}")

    for g in (836, 837, 840, 842, 843, 846, 855, 867, 2097, 2098, 2099, 2100, 2101):
        expect(face, g, b"png ")
    for g, t in ((838, 837), (839, 836), (841, 840), (844, 843), (845, 842), (847, 846), (853, 852), (856, 855), (868, 867)):
        expect(face, g, b"flip", t)
    for g in (836, 837, 840, 842, 843, 846, 855, 867):
        if any(face.record(p, g)[:4] != b"\0\0\0\0" for p in face.ppems):
            sys.exit(f"glyph {g} already carries offsets")
    for g in (844, 846):
        if face.advance(g) != 800:
            sys.exit(f"glyph {g} has advance {face.advance(g)}, not 800")

    a = {}
    for p in face.ppems:
        a[(p, 852)] = dupe_to(837)
        a[(p, 839)] = flip_to(836, 6, 4)
        a[(p, 840)] = at_offsets(face.record(p, 840), 6, 4)
        a[(p, 842)] = at_offsets(face.record(p, 842), -4, 3)
        a[(p, 845)] = flip_to(842, 10, 2)
        rec = face.record(p, 867)
        cut = int(0.75 * p + 0.5)
        a[(p, 867)] = rec[:8] + png_crop_left(rec[8:], cut)
        print(f"  867 at {p} ppem cropped to {cut} x {p}")
    write_font(face, os.path.join(out, "sbix-a.ttf"), a, {844: 1000, 846: 1000}, b"EmojA")

    c = {}
    for p in face.ppems:
        c[(p, 850)] = flip_to(838)
        c[(p, 859)] = flip_to(856)
        c[(p, 862)] = flip_to(859)
        c[(p, 852)] = dupe_to(837)
        c[(p, 857)] = dupe_to(838)
        c[(p, 865)] = dupe_to(852)
    write_font(face, os.path.join(out, "sbix-c.ttf"), c, {}, b"EmojC")

    f = {}
    keep = {2097: [p for p in face.ppems if p not in (64, 96, 160)], 2098: [20, 160], 2099: [20, 26], 2100: [], 2101: [64]}
    for g, kept in keep.items():
        for p in face.ppems:
            if p not in kept:
                f[(p, g)] = b""
        print(f"  {g}: data at {kept}")
    write_font(face, os.path.join(out, "sbix-f.ttf"), f, {}, b"EmojF")

    print(
        f"""
Cases, and the commands that ask them (from the directory holding the built probe):

  sbix-a.ttf: controls 838 852 846; offsets 839 840 841 842 845; axis 844 847 867 868
    FONT_FILE={out}/sbix-a.ttf ./sbix-probe flip 838,852,846,839,840,841,842,845,844,847,867,868 64,40
    FONT_FILE={out}/sbix-a.ttf ./sbix-probe flip 840,841 56,44,100
  sbix-c.ttf: chains 850 (flip flip png) 862 (flip flip flip png) 853 (flip dupe png)
              857 (dupe flip png) 865 (dupe dupe png)
    FONT_FILE={out}/sbix-c.ttf ./sbix-probe flip 850,862,853,857,865 64,40
  sbix-f.ttf: fallback 2097 (none at 64/96/160) 2098 (20, 160) 2099 (20, 26) 2100 (none) 2101 (64)
    FONT_FILE={out}/sbix-f.ttf ./sbix-probe fallback"""
    )


if __name__ == "__main__":
    main()
