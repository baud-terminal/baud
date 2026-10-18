#!/usr/bin/env python3
"""Generate the codepoints `SGR 64`'s stress mark skips, for both paths.

The web's emphasis marks (`[[source: css-text-decor-3]]`), which baud
draws for `SGR 64`, "are not drawn for":

* "Word separators or other characters that belong to the Unicode
  separator classes (Z*)";
* "Punctuation--specifically, any characters that belong to the Unicode
  P* general category and do not NFKD normalize [UAX15] to any of the
  following symbols": # % U+2030 U+2031 U+066A U+0609 U+060A & U+204A @
  U+00A7 U+00B6 U+204B U+2053 U+303D;
* "Characters belonging to the Unicode classes for control codes and
  unassigned characters (Cc, Cf, Cn)".

A typographic character unit is classed by its base character, and the
Z* bullet's own exception -- "emphasis marks are drawn for a space that
combines with any combining characters" -- is the one way a cluster
whose base is skipped takes the mark. A combining character is gc M*
(`[[source: unicode17-ch3]]` D52), so a space with only a ZWJ, a tag or
an emoji modifier attached stays bare. The CPU decides a cluster from
the Z* list and an M* trie written beside the skip trie, both Rust-only:
the cell shader reads its answer for a cluster from the overlay.

The general category is `[[source: gc-data]]`'s, which lists every
codepoint, Cn included. NFKD of one codepoint is its compatibility
decomposition (`[[source: unicode17-ch3]]` D65, D119): the mappings of
`[[source: unicode-data]]` field 5, tagged and untagged, applied until
nothing decomposes; no P* character is a Hangul syllable, and reordering
moves nothing in a decomposition to one symbol.

The set is emitted as a two-stage bit trie -- stage 1 maps a codepoint's
high bits (cp >> 8) to a block, stage 2 holds each distinct 256-bit block
as 32 bytes, low bit first -- into `baud-render/src/stress_data.rs` as
byte strings, and into the generated block of
`baud-present-macos/src/shaders.metal` as `uchar` arrays of the same
bytes. The trie is checked against the direct definition at every one of
the 1,114,112 codepoints before anything is written.

Usage: gen-stress-table.py [--check]
"""

import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SOURCES = ROOT / "docs/research/sources"
GC = SOURCES / "unicode-derivedgeneralcategory.txt"
UD = SOURCES / "unicode-unicodedata.txt"
RUST = ROOT / "baud-render/src/stress_data.rs"
METAL = ROOT / "baud-present-macos/src/shaders.metal"
BEGIN = "// BEGIN GENERATED: stress mark skips, scripts/gen-stress-table.py\n"
END = "// END GENERATED: stress mark skips\n"

# CSS's exceptions: punctuation that NFKD-normalizes to one of these keeps
# the mark.
KEEPS = [
    0x0023, 0x0025, 0x2030, 0x2031, 0x066A, 0x0609, 0x060A, 0x0026,
    0x204A, 0x0040, 0x00A7, 0x00B6, 0x204B, 0x2053, 0x303D,
]


def categories():
    gc = {}
    for line in open(GC, encoding="utf-8"):
        line = line.split("#")[0].strip()
        if not line:
            continue
        cps, cat = [f.strip() for f in line.split(";")[:2]]
        a, b = cps.split("..") if ".." in cps else (cps, cps)
        for cp in range(int(a, 16), int(b, 16) + 1):
            gc[cp] = cat
    assert len(gc) == 0x110000, f"the file lists {len(gc)} codepoints"
    return gc


def decompositions():
    dec = {}
    for line in open(UD, encoding="utf-8"):
        f = line.split(";")
        if f[5]:
            parts = f[5].split()
            if parts[0].startswith("<"):
                parts = parts[1:]
            dec[int(f[0], 16)] = [int(x, 16) for x in parts]
    return dec


def nfkd(cp, dec):
    if cp not in dec:
        return [cp]
    out = []
    for c in dec[cp]:
        out += nfkd(c, dec)
    return out


def skips():
    gc, dec = categories(), decompositions()
    keep = set(KEEPS)
    out = set()
    for cp in range(0x110000):
        c = gc[cp]
        if c[0] == "Z" or c in ("Cc", "Cf", "Cn"):
            out.add(cp)
        elif c[0] == "P":
            n = nfkd(cp, dec)
            if not (len(n) == 1 and n[0] in keep):
                out.add(cp)
    # Positive controls, each a reading of the text above.
    for cp in (0x20, 0x3000, 0x2029, 0x21, 0x2C, 0x3001, 0x2022, 0xAD, 0x0378, 0x0007):
        assert cp in out, f"U+{cp:04X} must be skipped"
    for cp in (0x61, 0x4E2D, 0x23, 0x40, 0xFF03, 0xFE5F, 0xE000, 0x1F600):
        assert cp not in out, f"U+{cp:04X} must take the mark"
    return out


def separators(gc):
    """Z*, ascending."""
    return sorted(cp for cp in range(0x110000) if gc[cp][0] == "Z")


def combining(gc):
    """gc M*: Mn, Mc and Me, D52's combining characters."""
    out = {cp for cp in range(0x110000) if gc[cp][0] == "M"}
    for cp in (0x301, 0x5B4, 0x93E, 0x20DD, 0xFE0F):
        assert cp in out, f"U+{cp:04X} is combining"
    for cp in (0x200D, 0x200C, 0xFF9E, 0x1F3FB, 0xE0041, 0x61):
        assert cp not in out, f"U+{cp:04X} is not combining"
    return out


def trie(skip):
    blocks, index = {}, []
    for hi in range(0x1100):
        bits = bytes(
            sum(1 << b for b in range(8) if (hi << 8 | k * 8 + b) in skip)
            for k in range(32)
        )
        index.append(blocks.setdefault(bits, len(blocks)))
    assert len(blocks) < 256, "a block number is one byte"
    packed = b"".join(sorted(blocks, key=blocks.get))
    for cp in range(0x110000):
        byte = packed[index[cp >> 8] * 32 + ((cp & 0xFF) >> 3)]
        assert ((byte >> (cp & 7)) & 1 == 1) == (cp in skip), f"U+{cp:04X}"
    return bytes(index), packed, len(blocks)


def rust(index, packed, nblocks, seps, cindex, cpacked, cblocks):
    def bytestring(name, data, doc):
        body = []
        for i in range(0, len(data), 32):
            chunk = "".join(f"\\x{v:02x}" for v in data[i : i + 32])
            body.append(f"    {chunk}\\")
        return "\n".join(
            [doc, f'pub static {name}: &[u8; {len(data)}] = b"\\', *body, '";']
        )

    header = "\n".join(
        [
            "//! The codepoints `SGR 64`'s stress mark skips, generated by",
            "//! `scripts/gen-stress-table.py` from `[[source: gc-data]]` and",
            "//! `[[source: unicode-data]]` by `[[source: css-text-decor-3]]`'s",
            "//! rule: Z*, P* but the punctuation that normalizes to one of its",
            "//! exceptions, Cc, Cf and Cn -- and, for a cluster on a skipped",
            "//! base, the separators and combining characters that decide",
            "//! whether it is \"a space that combines with any combining",
            "//! characters\". **Do not edit by hand**;",
            "//! `the_stress_mark_skips_what_css_leaves_bare` recomputes the sets",
            "//! from the pinned files at every codepoint, and the cell shader's",
            "//! copy of the skip trie is pinned to this one by",
            "//! `the_stress_mark_table_matches_the_shader`.",
        ]
    )
    doc1 = "\n".join(
        [
            "/// Stage 1: codepoint >> 8 to a block number, `0x1100` entries.",
        ]
    )
    doc2 = "\n".join(
        [
            f"/// Stage 2: {nblocks} distinct 256-codepoint blocks, 32 bytes each,",
            "/// a set bit for a codepoint the mark skips, low bit first.",
        ]
    )
    sep = "\n".join(
        [
            "/// The separator classes (Z*), ascending: a cluster on one of these",
            "/// takes the mark when a combining character attaches.",
            f"pub static SEPARATORS: [u32; {len(seps)}] = [",
            # rustfmt's layout: twelve to a line.
            *(
                "    " + ", ".join(f"0x{c:04X}" for c in seps[i : i + 12]) + ","
                for i in range(0, len(seps), 12)
            ),
            "];",
        ]
    )
    doc3 = "/// Stage 1 of the combining characters (gc M*, D52), as above."
    doc4 = "\n".join(
        [
            f"/// Stage 2: {cblocks} distinct blocks, a set bit for a combining",
            "/// character.",
        ]
    )
    return (
        header
        + "\n\n"
        + bytestring("STRESS_SKIP_INDEX", index, doc1)
        + "\n\n"
        + bytestring("STRESS_SKIP_BLOCKS", packed, doc2)
        + "\n\n"
        + sep
        + "\n\n"
        + bytestring("COMBINING_INDEX", cindex, doc3)
        + "\n\n"
        + bytestring("COMBINING_BLOCKS", cpacked, doc4)
        + "\n"
    )


def metal(index, packed, nblocks):
    def array(name, data):
        lines = [f"constant uchar {name}[{len(data)}] = {{"]
        for i in range(0, len(data), 16):
            lines.append("    " + ", ".join(f"0x{v:02x}" for v in data[i : i + 16]) + ",")
        lines.append("};")
        return "\n".join(lines) + "\n"

    return (
        BEGIN
        + "// `baud_render::STRESS_SKIP_INDEX` and `STRESS_SKIP_BLOCKS`, the same\n"
        + f"// bytes: {nblocks} blocks. Pinned by `the_stress_mark_table_matches_the_shader`.\n"
        + array("STRESS_SKIP_INDEX", index)
        + array("STRESS_SKIP_BLOCKS", packed)
        + END
    )


def main():
    index, packed, nblocks = trie(skips())
    gc = categories()
    cindex, cpacked, cblocks = trie(combining(gc))
    rs = rust(index, packed, nblocks, separators(gc), cindex, cpacked, cblocks)
    src = METAL.read_text()
    assert src.count(BEGIN) == 1 and src.count(END) == 1, "the shader's markers"
    head, rest = src.split(BEGIN)
    _, tail = rest.split(END)
    mt = head + metal(index, packed, nblocks) + tail
    if "--check" in sys.argv:
        stale = [p for p, want in ((RUST, rs), (METAL, mt)) if not p.exists() or p.read_text() != want]
        if stale:
            print("STALE: " + ", ".join(str(p.relative_to(ROOT)) for p in stale))
            sys.exit(1)
        print("OK")
        return
    RUST.write_text(rs)
    METAL.write_text(mt)
    print(f"{nblocks} blocks, {len(index)} + {len(packed)} bytes")


if __name__ == "__main__":
    main()
