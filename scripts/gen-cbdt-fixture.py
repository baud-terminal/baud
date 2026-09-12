#!/usr/bin/env python3
"""Subset Noto Color Emoji into a small committed CBDT/CBLC fixture.

Noto Color Emoji (`[[source: noto-color-emoji]]`, 10.7 MB) is the one
widely-shipped CBDT face and it is too large to commit, so this script takes
its first `N` glyphs and writes a self-contained sfnt that exercises every
index format and every PNG image format the reader in `baud-font/src/bitmap.rs`
implements:

| glyph range | index format | image format |
|---|---|---|
| 4..15 | 1 (u32 offsets) | 17 (small metrics + PNG) |
| 16..27 | 3 (u16 offsets) | 18 (big metrics + PNG) |
| 28..33 | 4 (sparse glyph/offset pairs) | 17 |
| 34..39 | 5 (constant size, sparse ids) | 19 (metrics in the subtable) |
| 40..45 | 2 (constant size, dense) | 19 |

Noto itself uses only index format 1 with image format 17, so the other
arrangements are re-encodings of its records rather than anything Noto
ships; the PNG bytes and the metrics are Noto's own. Glyphs 0..3 have no
bitmap in Noto and are kept so the reader's "no image" answer is exercised.

Constant-size subtables (formats 2 and 5) hold each record in `imageSize`
bytes, so every record in them is padded to the longest; the `dataLen` field
inside format 19 bounds the PNG, and the padding is never read.

`head`, `hhea`, `maxp`, `OS/2`, `post` and the horizontal metrics are
Noto's, truncated to the subset; `cmap` is rebuilt as one format 12 subtable
over the codepoints Noto maps to the kept glyphs.

Usage: gen-cbdt-fixture.py <NotoColorEmoji.ttf> <out.ttf>
"""

import struct
import sys

N = 46  # glyphs kept: 0..3 without bitmaps, then 42 that carry one

# Noto's bitmap range has holes, so the fixture's glyph `k` is the `k`th of
# Noto's glyphs that carries a bitmap (after the four that do not), and
# every table is remapped through `keep[new] = old`.


def tables(d):
    nt = struct.unpack(">H", d[4:6])[0]
    t = {}
    for i in range(nt):
        r = 12 + 16 * i
        tag = d[r : r + 4]
        off, ln = struct.unpack(">II", d[r + 8 : r + 16])
        t[tag] = d[off : off + ln]
    return t


def read_cbdt(cblc, cbdt):
    """Every glyph's (small metrics, png) from a format-1/17 Noto CBLC."""
    n_sizes = struct.unpack(">I", cblc[4:8])[0]
    assert n_sizes == 1, n_sizes
    size = cblc[8 : 8 + 48]
    isa, its, nis = struct.unpack(">III", size[:12])
    ppem_x, ppem_y, depth, flags = struct.unpack(">BBBb", size[44:48])
    out = {}
    for j in range(nis):
        a = isa + 8 * j
        fg, lg, off = struct.unpack(">HHI", cblc[a : a + 8])
        h = isa + off
        idx, img, ido = struct.unpack(">HHI", cblc[h : h + 8])
        assert (idx, img) == (1, 17), (idx, img)
        offs = struct.unpack(">%dI" % (lg - fg + 2), cblc[h + 8 : h + 8 + 4 * (lg - fg + 2)])
        for g in range(fg, lg + 1):
            s, e = offs[g - fg], offs[g - fg + 1]
            if e == s:
                continue
            rec = cbdt[ido + s : ido + e]
            metrics = rec[:5]  # height, width, bearingX, bearingY, advance
            (ln,) = struct.unpack(">I", rec[5:9])
            png = rec[9 : 9 + ln]
            assert png[:8] == b"\x89PNG\r\n\x1a\n", g
            out[g] = (metrics, png)
    return size, (ppem_x, ppem_y, depth, flags), out


def big_metrics(small):
    """smallGlyphMetrics -> bigGlyphMetrics with vertical fields mirrored."""
    h, w, bx, by, adv = struct.unpack(">BBbbB", small)
    return struct.unpack(">BBbbB", small), struct.pack(">BBbbBbbB", h, w, bx, by, adv, bx, by, adv)


def build_cbdt(records):
    """Return (cblc, cbdt) covering glyphs 4..45 in the five arrangements."""
    cbdt = bytearray(b"\x00\x03\x00\x00")  # CBDT header, version 3.0
    subtables = []  # (first, last, header_bytes)

    def emit(first, last, index_format, image_format, body_builder):
        ido = len(cbdt)
        body = body_builder(ido)
        header = struct.pack(">HHI", index_format, image_format, ido) + body
        subtables.append((first, last, header))

    # format 1 / image 17: offsets u32, record = small metrics + len + png
    def fmt1(ido):
        offs = []
        for g in range(4, 16):
            offs.append(len(cbdt) - ido)
            m, png = records[g]
            cbdt.extend(m + struct.pack(">I", len(png)) + png)
        offs.append(len(cbdt) - ido)
        return struct.pack(">%dI" % len(offs), *offs)

    emit(4, 15, 1, 17, fmt1)

    # format 3 / image 18: offsets u16, record = big metrics + len + png
    def fmt3(ido):
        offs = []
        for g in range(16, 28):
            offs.append(len(cbdt) - ido)
            m, png = records[g]
            _, big = big_metrics(m)
            cbdt.extend(big + struct.pack(">I", len(png)) + png)
        offs.append(len(cbdt) - ido)
        assert offs[-1] < 0x10000
        body = struct.pack(">%dH" % len(offs), *offs)
        if len(offs) % 2:
            body += b"\x00\x00"  # keep the next subtable 4-aligned
        return body

    emit(16, 27, 3, 18, fmt3)

    # format 4 / image 17: sparse (glyph, offset) pairs plus a sentinel
    def fmt4(ido):
        pairs = []
        for g in range(28, 34):
            pairs.append((g, len(cbdt) - ido))
            m, png = records[g]
            cbdt.extend(m + struct.pack(">I", len(png)) + png)
        pairs.append((0xFFFF, len(cbdt) - ido))
        body = struct.pack(">I", len(pairs) - 1)
        for g, o in pairs:
            assert o < 0x10000
            body += struct.pack(">HH", g, o)
        return body

    emit(28, 33, 4, 17, fmt4)

    # format 5 / image 19: constant imageSize, shared big metrics, sparse ids
    def fmt5(ido):
        gs = list(range(34, 40))
        size = max(4 + len(records[g][1]) for g in gs)
        _, big = big_metrics(records[gs[0]][0])
        for g in gs:
            png = records[g][1]
            rec = struct.pack(">I", len(png)) + png
            cbdt.extend(rec + b"\x00" * (size - len(rec)))
        return struct.pack(">I", size) + big + struct.pack(">I", len(gs)) + struct.pack(">%dH" % len(gs), *gs)

    emit(34, 39, 5, 19, fmt5)

    # format 2 / image 19: constant imageSize, shared big metrics, dense
    def fmt2(ido):
        gs = list(range(40, 46))
        size = max(4 + len(records[g][1]) for g in gs)
        _, big = big_metrics(records[gs[0]][0])
        for g in gs:
            png = records[g][1]
            rec = struct.pack(">I", len(png)) + png
            cbdt.extend(rec + b"\x00" * (size - len(rec)))
        return struct.pack(">I", size) + big

    emit(40, 45, 2, 19, fmt2)

    # CBLC: header, one BitmapSize, then the IndexSubTableArray and headers.
    array_len = 8 * len(subtables)
    headers = b""
    array = b""
    for first, last, header in subtables:
        array += struct.pack(">HHI", first, last, array_len + len(headers))
        headers += header
        if len(headers) % 4:
            headers += b"\x00" * (4 - len(headers) % 4)
    isa = 8 + 48
    return bytes(cbdt), array, headers, isa, len(subtables)


def read_cmap12(cmap):
    n = struct.unpack(">H", cmap[2:4])[0]
    for i in range(n):
        pid, eid, off = struct.unpack(">HHI", cmap[4 + 8 * i : 12 + 8 * i])
        fmt = struct.unpack(">H", cmap[off : off + 2])[0]
        if fmt == 12:
            ngroups = struct.unpack(">I", cmap[off + 12 : off + 16])[0]
            groups = []
            for k in range(ngroups):
                s, e, g = struct.unpack(">III", cmap[off + 16 + 12 * k : off + 28 + 12 * k])
                groups.append((s, e, g))
            return groups
    raise SystemExit("no format 12 cmap")


def build_cmap(groups, old_to_new):
    out = []
    for s, e, g in groups:
        for c in range(s, e + 1):
            gid = old_to_new.get(g + (c - s))
            if gid is not None:
                out.append((c, gid))
    out.sort()
    # coalesce into sequential groups
    runs = []
    for c, gid in out:
        if runs and runs[-1][1] + 1 == c and runs[-1][2] + (runs[-1][1] - runs[-1][0]) + 1 == gid:
            runs[-1] = (runs[-1][0], c, runs[-1][2])
        else:
            runs.append((c, c, gid))
    sub = struct.pack(">HHIII", 12, 0, 16 + 12 * len(runs), 0, len(runs))
    for s, e, g in runs:
        sub += struct.pack(">III", s, e, g)
    return struct.pack(">HH", 0, 1) + struct.pack(">HHI", 3, 10, 12) + sub, len(out)


def checksum(b):
    b = b + b"\x00" * ((4 - len(b) % 4) % 4)
    return sum(struct.unpack(">%dI" % (len(b) // 4), b)) & 0xFFFFFFFF


def sfnt(tabs):
    tags = sorted(tabs)
    n = len(tags)
    es = 1
    while (es << 1) <= n:
        es <<= 1
    search = es * 16
    head = struct.pack(">IHHHH", 0x00010000, n, search, es.bit_length() - 1, n * 16 - search)
    off = 12 + 16 * n
    dir_ = b""
    body = b""
    for tag in tags:
        data = tabs[tag]
        pad = data + b"\x00" * ((4 - len(data) % 4) % 4)
        dir_ += struct.pack(">4sIII", tag, checksum(data), off + len(body), len(data))
        body += pad
    return head + dir_ + body


def main():
    src, dst = sys.argv[1], sys.argv[2]
    d = open(src, "rb").read()
    t = tables(d)
    size, (ppem_x, ppem_y, depth, flags), records = read_cbdt(t[b"CBLC"], t[b"CBDT"])
    assert not any(g in records for g in range(0, 4))
    keep = [0, 1, 2, 3] + sorted(records)[: N - 4]
    assert len(keep) == N
    old_to_new = {g: k for k, g in enumerate(keep)}
    kept = {old_to_new[g]: records[g] for g in keep[4:]}
    cbdt, array, headers, isa, nsub = build_cbdt(kept)
    bitmap_size = (
        struct.pack(">III", isa, len(array) + len(headers), nsub)
        + size[12:40]  # colorRef, hori, vert
        + struct.pack(">HHBBBb", 4, N - 1, ppem_x, ppem_y, depth, flags)
    )
    cblc = struct.pack(">HHI", 3, 0, 1) + bitmap_size + array + headers

    maxp = bytearray(t[b"maxp"][:6])
    maxp[4:6] = struct.pack(">H", N)
    hhea = bytearray(t[b"hhea"])
    n_hm = struct.unpack(">H", hhea[34:36])[0]
    hhea[34:36] = struct.pack(">H", N)
    src_hmtx = t[b"hmtx"]
    hmtx = b""
    for g in keep:
        if g < n_hm:
            hmtx += src_hmtx[4 * g : 4 * g + 4]
        else:
            adv = src_hmtx[4 * (n_hm - 1) : 4 * (n_hm - 1) + 2]
            lsb = src_hmtx[4 * n_hm + 2 * (g - n_hm) : 4 * n_hm + 2 * (g - n_hm) + 2]
            hmtx += adv + lsb
    cmap, mapped = build_cmap(read_cmap12(t[b"cmap"]), old_to_new)
    out = {
        b"CBDT": cbdt,
        b"CBLC": cblc,
        b"OS/2": t[b"OS/2"],
        b"cmap": cmap,
        b"head": t[b"head"],
        b"hhea": bytes(hhea),
        b"hmtx": hmtx,
        b"maxp": bytes(maxp),
        b"post": t[b"post"][:32],
    }
    data = sfnt(out)
    open(dst, "wb").write(data)
    print(f"{dst}: {len(data)} bytes, {N} glyphs, {N - 4} bitmaps from Noto glyphs {keep[4]}..{keep[-1]}, {mapped} codepoints mapped, CBDT {len(cbdt)} B")


if __name__ == "__main__":
    main()
