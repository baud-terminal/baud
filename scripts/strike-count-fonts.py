#!/usr/bin/env python3
"""Hostile colour-bitmap fonts for pricing baud's strike walk, and copies
CoreGraphics will load. Note J, docs/research/notes/j-colour-bitmap-limits.md.

    python3 scripts/strike-count-fonts.py OUTDIR

writes into OUTDIR (it refuses a directory inside the repository):

  sbix-N.ttf           N copies of the 20 ppem strike of baud's sbix fixture
                       (scripts/gen-sbix-fixture.py), every offset naming one
                       strike: glyph 1 is a png in all of them, glyph 3 a flip
                       to it, glyph 0 holds nothing and glyph 9 is a dupe
                       naming itself.  N = 1, 255, 65535, 1048574 (the last
                       is what a 4 MB table can name).
  sbix-distinct-N.ttf  N strikes at ppem 1..N, each with its own header, all
                       pointing at one shared block of the same records.
                       N = 255, 4096, 65535.
  cblc-N.ttf           N copies of the size record of baud's Noto Color Emoji
                       subset (glyphs 4-45), all naming its one subtable
                       array.  N = 1, 255, 65535.
  cblc-range-N.ttf     the same with each size's range starting at glyph 0,
                       so glyph 0 is in range and in no subtable: a miss
                       walks all 5 subtables in every size.
  cblc-subs-M.ttf      one size whose array holds M copies of its last
                       subtable record (glyphs 40-45), range 0-45: glyph 0
                       walks all M.  M = 5, 255, 65535.
  ct-*.ttf             a copy of each sbix font, and of the committed sbix
                       fixture, with what CoreGraphics needs to open it:
                       maxp 1.0 with an empty glyf/loca, a minimal name table
                       and Apple Color Emoji's OS/2 bytes.  Without them
                       CGFont refuses the file.  Scratch fonts, never shipped.

The wide fonts price building an index of which glyphs have a bitmap, whose
cost is strikes x glyphs, so each carries the most glyphs a face can, 65535:

  sbix-wide-N          N strikes overlapped at a 4-byte stride over one run
                       of identical words, so every strike reads ppem 20 and
                       offsets all equal: every glyph is empty in every
                       strike, and no two strikes share an offset that a
                       build could deduplicate on.  N = 255, 4096.
  sbix-wide-dupe-N     N strikes overlapped the same way whose every record
                       is a dupe: strike k's offsets are strike 0's shifted k
                       entries, rising by 12, and every record lands in a run
                       of b"dupe" -- graphicType 'dupe', target 0x6475, whose
                       own record is the same dupe, a loop the ten-record
                       bound ends.  N = 255, 4096.
  cblc-wide-SxM        S sizes ranging over every glyph, all naming one array
                       of M subtable records that cover only glyph 0xFFFF,
                       past the face: every lookup is in range and walks the
                       whole array.  255x1, 1x255, 255x255, 4096x1.
"""
import importlib.util
import os
import struct
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
FIXTURE = os.path.join(ROOT, "baud-font/tests/fonts/sbix-strikes.ttf")
NOTO = os.path.join(ROOT, "baud-font/tests/fonts/NotoColorEmoji-subset.ttf")
ACE = "/System/Library/Fonts/Apple Color Emoji.ttc"

spec = importlib.util.spec_from_file_location("gen", os.path.join(ROOT, "scripts/gen-sbix-fixture.py"))
gen = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gen)


def tables(d, off=0):
    n = struct.unpack(">H", d[off + 4 : off + 6])[0]
    t = {}
    for i in range(n):
        tag, _, o, l = struct.unpack(">4sIII", d[off + 12 + 16 * i : off + 28 + 16 * i])
        t[tag] = d[o : o + l]
    return t


def font(t, version=0x00010000):
    tags = sorted(t)
    nt = len(tags)
    search = 1
    while search * 2 <= nt:
        search *= 2
    head = struct.pack(">IHHHH", version, nt, search * 16, search.bit_length() - 1, nt * 16 - search * 16)
    at = 12 + 16 * nt
    dirs = b""
    body = b""
    for tag in tags:
        blob = t[tag]
        dirs += struct.pack(">4sIII", tag, 0, at + len(body), len(blob))
        body += blob + b"\0" * (-len(blob) % 4)
    return head + dirs + body


def strike_records():
    """The fixture generator's records at 20 ppem, and their offsets from the first."""
    data, local, at = b"", [], 0
    for r in gen.glyph_records(20):
        local.append(at)
        if r:
            data += r
            at += len(r)
    local.append(at)
    return data, local


def sbix_copies(n):
    data, local = strike_records()
    head = 4 + 4 * (gen.GLYPHS + 1)
    strike = struct.pack(">HH", 20, 72) + struct.pack(f">{gen.GLYPHS + 1}I", *[head + o for o in local]) + data
    first = 8 + 4 * n
    return struct.pack(">HHI", 1, 1, n) + struct.pack(f">{n}I", *([first] * n)) + strike


def sbix_distinct(n):
    data, local = strike_records()
    hdr = 4 + 4 * (gen.GLYPHS + 1)
    first = 8 + 4 * n
    shared = first + n * hdr
    offs, strikes = [], b""
    for k in range(n):
        start = first + k * hdr
        offs.append(start)
        strikes += struct.pack(">HH", k + 1, 72) + struct.pack(f">{gen.GLYPHS + 1}I", *[shared - start + o for o in local])
    return struct.pack(">HHI", 1, 1, n) + struct.pack(f">{n}I", *offs) + strikes + data


def sbix_font(table):
    gen.sbix = lambda: table
    return gen.font()


def cblc_variants():
    d = open(NOTO, "rb").read()
    t = tables(d)
    cblc = t[b"CBLC"]
    rec, rest = cblc[8:56], cblc[56:]
    out = {}
    for n in (1, 255, 65535):
        r = bytearray(rec)
        arr = struct.unpack(">I", r[:4])[0]
        struct.pack_into(">I", r, 0, arr + 48 * (n - 1))
        plain = struct.pack(">HHI", 3, 0, n) + bytes(r) * n + rest
        out[f"cblc-{n}"] = plain
        struct.pack_into(">H", r, 40, 0)
        out[f"cblc-range-{n}"] = struct.pack(">HHI", 3, 0, n) + bytes(r) * n + rest
    first, last, off4 = struct.unpack(">HHI", rest[32:40])
    for m in (5, 255, 65535):
        r = bytearray(rec)
        struct.pack_into(">I", r, 8, m)
        struct.pack_into(">H", r, 40, 0)
        array = struct.pack(">HHI", first, last, off4 + 8 * (m - 5)) * m
        out[f"cblc-subs-{m}"] = struct.pack(">HHI", 3, 0, 1) + bytes(r) + array + rest[40:]
    version = struct.unpack(">I", d[:4])[0]
    return {k: font({**t, b"CBLC": v}, version) for k, v in out.items()}


WIDE = 65535


def strike_offsets(n):
    return struct.pack(">HHI", 1, 1, n) + struct.pack(f">{n}I", *[8 + 4 * n + 4 * k for k in range(n)])


def sbix_wide(n):
    words = n + WIDE + 1
    return strike_offsets(n) + struct.pack(">HH", 20, 72) * words


def sbix_wide_dupe(n):
    entries = WIDE + n
    first = 4 + 4 * entries
    offsets = struct.pack(f">{entries}I", *[first + 12 * j for j in range(entries)])
    run = 12 * (WIDE + n) + 4 * n + 16
    return strike_offsets(n) + struct.pack(">HH", 20, 72) + offsets + b"dupe" * (run // 4 + 1)


def cblc_wide(sizes, subtables):
    array_at = 8 + 48 * sizes
    array = struct.pack(">HHI", 0xFFFF, 0xFFFF, 8 * subtables) * subtables
    header = struct.pack(">HHI", 1, 17, 0) + struct.pack(">II", 0, 0)
    size = struct.pack(">IIII", array_at, 8 * subtables + len(header), subtables, 0) + b"\0" * 24
    size += struct.pack(">HHBBBb", 0, WIDE - 1, 109, 109, 32, 1)
    return struct.pack(">HHI", 3, 0, sizes) + size * sizes + array + header


def wide_font(**swap):
    """The fixture generator's skeleton at 65535 glyphs, its `sbix` swapped for
    the tables given."""
    narrow = gen.GLYPHS
    gen.GLYPHS = WIDE
    try:
        t = tables(gen.font())
    finally:
        gen.GLYPHS = narrow
    del t[b"sbix"]
    for tag, blob in swap.items():
        t[tag.encode()] = blob
    return font(t)


def name_table(family):
    recs, strings = [], b""
    for nid, s in [(1, family), (2, "Regular"), (4, family + " Regular"), (6, family.replace(" ", "") + "-Regular")]:
        for plat, enc, lang, data in [(1, 0, 0, s.encode("mac_roman")), (3, 1, 0x409, s.encode("utf-16-be"))]:
            recs.append((plat, enc, lang, nid, len(data), len(strings)))
            strings += data
    recs.sort()
    head = struct.pack(">HHH", 0, len(recs), 6 + 12 * len(recs))
    return head + b"".join(struct.pack(">HHHHHH", *r) for r in recs) + strings


def coregraphics_copy(blob, family):
    ace = open(ACE, "rb").read()
    os2 = tables(ace, struct.unpack(">I", ace[12:16])[0])[b"OS/2"]
    t = tables(blob)
    n = struct.unpack(">H", t[b"maxp"][4:6])[0]
    t[b"maxp"] = struct.pack(">IH", 0x00010000, n) + struct.pack(">13H", 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0)
    t[b"glyf"] = b""
    t[b"loca"] = b"\0\0" * (n + 1)
    t[b"name"] = name_table(family)
    t[b"OS/2"] = os2
    return font(t)


def main():
    out = os.path.abspath(sys.argv[1])
    if out == ROOT or out.startswith(ROOT + os.sep):
        sys.exit("refusing to write fonts inside the repository")
    os.makedirs(out, exist_ok=True)
    made = {}
    for n in (1, 255, 65535, 1048574):
        made[f"sbix-{n}"] = sbix_font(sbix_copies(n))
    for n in (255, 4096, 65535):
        made[f"sbix-distinct-{n}"] = sbix_font(sbix_distinct(n))
    made.update(cblc_variants())
    for name in [k for k in made if k.startswith("sbix")]:
        made["ct-" + name] = coregraphics_copy(made[name], "Strike Probe " + name.split("-")[-1])
    made["ct-fixture"] = coregraphics_copy(open(FIXTURE, "rb").read(), "Sbix Fixture")
    for n in (255, 4096):
        made[f"sbix-wide-{n}"] = wide_font(sbix=sbix_wide(n))
        made[f"sbix-wide-dupe-{n}"] = wide_font(sbix=sbix_wide_dupe(n))
    for s, m in ((255, 1), (1, 255), (255, 255), (4096, 1)):
        made[f"cblc-wide-{s}x{m}"] = wide_font(CBLC=cblc_wide(s, m), CBDT=struct.pack(">I", 0x00030000))
    for name, blob in made.items():
        with open(os.path.join(out, name + ".ttf"), "wb") as f:
            f.write(blob)
        print(f"{name}.ttf {len(blob)} bytes")


if __name__ == "__main__":
    main()
