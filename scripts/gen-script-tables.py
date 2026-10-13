#!/usr/bin/env python3
"""Generate baud-font/src/script_data.rs from pinned Unicode data.

The Unicode Script property (`[[source: unicode-scripts]]`), its
Script_Extensions (`[[source: unicode-scriptextensions]]`), each script's
ISO 15924 code (`[[source: unicode-propertyvaluealiases]]`), and the
OpenType script tags each script selects (`[[source: ms-opentype-scripttags]]`).

Same contract as gen-grapheme-tables.py: generated and committed, and
`script_tables_match_the_pinned_data` (baud-font/tests/script.rs) runs this
with `--check`, so the committed file cannot drift from the pinned data.

**The OpenType tag is derived, and the derivation was checked against the
registry once, by hand, not here:** the registry is a vendor page held
`external` (not committed), so this script cannot read it. Every Unicode 17.0
script's tag below is a registered tag, and the registered tags it does not
produce are `byzm`, `math` and `musc` (not Unicode scripts), `jamo` (the
registry: "Use of this tag is not recommended ... should use 'hang'
instead"), and `jurc`, `pcun` and `seal` (scripts Unicode 17.0 does not
encode). The record has the check, "OpenType script selection, phase 0".
"""
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SRC = ROOT / "docs/research/sources"
DST = ROOT / "baud-font/src/script_data.rs"

# The registry's exceptions to "the ISO 15924 code, lowercased": Hiragana,
# Katakana and their union share `kana`, and the three-letter tags pad with
# spaces (the registry spells Yi's with no-break spaces "to ensure that two
# space characters are displayed. The actual tag byte sequence is 0x79 0x69
# 0x20 0x20").
EXCEPTIONS = {
    "Hira": "kana",
    "Kana": "kana",
    "Hrkt": "kana",
    "Laoo": "lao ",
    "Yiii": "yi  ",
    "Nkoo": "nko ",
    "Vaii": "vai ",
}

# Scripts the registry gives a second tag for its newer layout model, tried
# first: "Use of this tag ['mymr'] is not recommended. Fonts supporting
# Unicode encoding of Myanmar script should use 'mym2' instead", and the
# Indic `v.2` rows, which the registry lists in no order, so putting them
# first is a choice recorded open, not the registry's. HarfBuzz also tries an unregistered `dev3` and its kin;
# baud tries only registered tags.
V2 = {
    "Beng": "bng2",
    "Deva": "dev2",
    "Gujr": "gjr2",
    "Guru": "gur2",
    "Knda": "knd2",
    "Mlym": "mlm2",
    "Orya": "ory2",
    "Taml": "tml2",
    "Telu": "tel2",
    "Mymr": "mym2",
}

# Script values with no OpenType tag of their own: text in them selects
# `DFLT`. Their order fixes indices 0, 1 and 2, which `Script` names.
NEUTRAL = ["Zzzz", "Zyyy", "Zinh"]
BLOCK = 256


def data_lines(path):
    for line in (SRC / path).read_text(encoding="utf-8").splitlines():
        line = line.split("#", 1)[0].strip()
        if line:
            yield [p.strip() for p in line.split(";")]


def span(field):
    lo, _, hi = field.partition("..")
    return int(lo, 16), int(hi or lo, 16)


def main():
    check = "--check" in sys.argv[1:]

    # Long name -> ISO 15924 code, for every Script value.
    code_of = {}
    for p in data_lines("unicode-propertyvaluealiases.txt"):
        if p[0] == "sc":
            code_of[p[2]] = p[1]
    assert code_of["Hebrew"] == "Hebr" and code_of["Common"] == "Zyyy"

    sc = [0] * 0x110000  # filled with indices once the order is known
    named = {}
    for p in data_lines("unicode-scripts.txt"):
        lo, hi = span(p[0])
        named.setdefault(code_of[p[1]], []).append((lo, hi))

    codes = NEUTRAL + sorted(c for c in named if c not in NEUTRAL)
    assert len(codes) <= 255, "a script index is a u8"
    index = {c: i for i, c in enumerate(codes)}
    # Unassigned code points are Unknown (Zzzz), index 0, by default.
    for code, ranges in named.items():
        for lo, hi in ranges:
            for cp in range(lo, hi + 1):
                sc[cp] = index[code]

    # Script_Extensions: a set per listed code point, by short alias. The
    # file lists only code points whose value is not {Script}; everything
    # else has {Script} and is stored as 0, "none listed".
    short_to_code = {}
    for p in data_lines("unicode-propertyvaluealiases.txt"):
        if p[0] == "sc":
            for alias in p[1:]:
                short_to_code[alias] = p[1]
    sets = [()]
    set_index = {(): 0}
    scx = [0] * 0x110000
    for p in data_lines("unicode-scriptextensions.txt"):
        lo, hi = span(p[0])
        members = tuple(sorted(index[short_to_code[a]] for a in p[1].split()))
        if members not in set_index:
            set_index[members] = len(sets)
            sets.append(members)
        for cp in range(lo, hi + 1):
            scx[cp] = set_index[members]
    assert len(sets) <= 256, "a set index is a u8"
    # UAX #24: Script_Extensions(c) != {Script(c)} only where Script(c) is
    # Common or Inherited. The resolver relies on it, so it is checked here.
    for cp in range(0x110000):
        if scx[cp]:
            assert sc[cp] in (1, 2) or sets[scx[cp]] != (sc[cp],), hex(cp)

    tags = []
    for c in codes:
        if c in NEUTRAL:
            tags.append(("", ""))
        else:
            first = V2.get(c, EXCEPTIONS.get(c, c.lower()))
            second = EXCEPTIONS.get(c, c.lower()) if c in V2 else ""
            tags.append((first, second))
    assert tags[index["Hebr"]] == ("hebr", "")
    assert tags[index["Deva"]] == ("dev2", "deva")
    assert tags[index["Hira"]] == ("kana", "")

    def trie(values):
        blocks, idx = {}, []
        for base in range(0, 0x110000, BLOCK):
            key = bytes(values[base : base + BLOCK])
            if key not in blocks:
                blocks[key] = len(blocks)
            idx.append(blocks[key])
        assert len(blocks) <= 256, f"{len(blocks)} blocks will not fit a u8 stage 1"
        packed = b"".join(blocks)
        for cp in range(0x110000):
            assert packed[idx[cp >> 8] * BLOCK + (cp & 0xFF)] == values[cp]
        return idx, packed, len(blocks)

    def bytestring(name, data, doc):
        body = []
        for i in range(0, len(data), 32):
            chunk = "".join(f"\\x{v:02x}" for v in data[i : i + 32])
            body.append(f"    {chunk}\\")
        return "\n".join([doc, f'pub static {name}: &[u8; {len(data)}] = b"\\', *body, '";'])

    sc_idx, sc_packed, sc_blocks = trie(sc)
    scx_idx, scx_packed, scx_blocks = trie(scx)

    # Every table is a byte string: rustfmt rewraps array literals to its
    # width, so a generator emitting one could never pass its own --check.
    codes_bytes = b"".join(c.encode() for c in codes)
    tags_bytes = b"".join((a or "\0\0\0\0").encode() + (b or "\0\0\0\0").encode() for a, b in tags)
    widest = max(len(m) for m in sets)
    slot = widest + 1
    sets_bytes = b"".join(bytes(m) + b"\xff" * (slot - len(m)) for m in sets)

    out = [
        "//! The Script property and Script_Extensions, and the OpenType script",
        "//! tags each script selects, generated from the sources pinned in",
        "//! `docs/research/sources/MANIFEST.toml`: `[[source: unicode-scripts]]`,",
        "//! `[[source: unicode-scriptextensions]]`,",
        "//! `[[source: unicode-propertyvaluealiases]]` and",
        "//! `[[source: ms-opentype-scripttags]]`.",
        "//!",
        "//! **Do not edit by hand.** Regenerate with `scripts/gen-script-tables.py`.",
        "//! `script_tables_match_the_pinned_data` fails if this file and the pinned",
        "//! data disagree.",
        "",
        f"/// How many Script values there are; a `Script` indexes below this.",
        f"pub const SCRIPT_COUNT: usize = {len(codes)};",
        "",
        f"/// The bytes of one Script_Extensions set in [`EXTENSION_SETS`].",
        f"pub const EXTENSION_SLOT: usize = {slot};",
        "",
    ]
    out.append(bytestring(
        "CODES", codes_bytes,
        "/// Each script's ISO 15924 code, four bytes a script in index order:\n"
        "/// `Zzzz` (Unknown), `Zyyy` (Common) and `Zinh` (Inherited) first, then\n"
        "/// by code.",
    ))
    out.append(bytestring(
        "TAGS", tags_bytes,
        "/// Each script's two OpenType tags, eight bytes a script, the one to try\n"
        "/// first first; a zero tag is none. Unknown, Common and Inherited have\n"
        "/// none.",
    ))
    out.append(bytestring(
        "EXTENSION_SETS", sets_bytes,
        "/// The distinct Script_Extensions sets as script indices, one\n"
        "/// [`EXTENSION_SLOT`]-byte slot each, ended by `0xff`; set 0 is the\n"
        "/// empty set, \"the Script value alone\".",
    ))
    out.append(bytestring(
        "SCRIPT_INDEX", sc_idx,
        "/// The Script property as a two-stage trie. Stage 1: code point >> 8\n"
        f"/// to one of {sc_blocks} distinct blocks of 256 script indices.",
    ))
    out.append(bytestring("SCRIPT_BLOCKS", sc_packed, "/// Stage 2 of [`SCRIPT_INDEX`]."))
    out.append(bytestring(
        "EXTENSION_INDEX", scx_idx,
        "/// Script_Extensions as a two-stage trie of set indices into\n"
        f"/// [`EXTENSION_SETS`]. Stage 1: code point >> 8 to one of {scx_blocks} blocks.",
    ))
    out.append(bytestring("EXTENSION_BLOCKS", scx_packed, "/// Stage 2 of [`EXTENSION_INDEX`]."))
    text = "\n".join(out) + "\n"

    if check:
        have = DST.read_text(encoding="utf-8") if DST.exists() else ""
        if have != text:
            print(f"{DST} is stale; rerun scripts/gen-script-tables.py", file=sys.stderr)
            sys.exit(1)
        return
    DST.write_text(text, encoding="utf-8")
    print(
        f"{DST}: {len(codes)} scripts, {len(sets)} extension sets, "
        f"{len(sc_packed) + len(sc_idx)} + {len(scx_packed) + len(scx_idx)} bytes"
    )


if __name__ == "__main__":
    main()
