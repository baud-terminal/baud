#!/usr/bin/env python3
"""Generate baud-font/src/script_data.rs from pinned Unicode data.

The Unicode Script property (`[[source: unicode-scripts]]`), its
Script_Extensions (`[[source: unicode-scriptextensions]]`), each script's
ISO 15924 code (`[[source: unicode-propertyvaluealiases]]`), the
OpenType script tags each script selects (`[[source: ms-opentype-scripttags]]`),
and each script's direction with the letters and numbers that can turn a
run of a right-to-left script left to right (`[[source: unicode-derivedbidiclass]]`,
`[[source: gc-data]]`), and the paired brackets UAX #9 pairs
(`[[source: unicode-bidibrackets]]`, with `[[source: unicode-data]]`
for the canonical equivalents BD16 names).

**A bracket pair takes the script of the text enclosing it**
(`[[source: uax24]]` 5.1): "paired punctuation, such as brackets or
quotation marks, belongs to the enclosing or outer level of the text and
should therefore match the script of the enclosing text". Which characters
pair is Bidi_Paired_Bracket and its type, and a pair is matched as UAX #9's
BD16 matches one (`[[source: uax9-bidi]]`), U+2329/U+3008 and U+232A/U+3009
"treated as equivalent". Each pair gets one number, shared by the two
equivalents, so the resolver compares numbers and never searches the file.

**A script is right to left when every letter in it is Bidi_Class R or
AL.** UAX #9 resolves such a character to an odd level whatever the
paragraph's direction (`[[source: uax9-bidi]]`, W3 and I1-I2), so a run of
the script is laid out right to left. Over 17.0.0 that is 37 scripts:
HarfBuzz's `hb_script_get_horizontal_direction` list and Old Hungarian,
which HarfBuzz leaves to the caller. A script whose letters disagree stops
the generator.

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


def classes(path):
    """A property value per code point from a UCD file; `None` where unlisted."""
    out = [None] * 0x110000
    for p in data_lines(path):
        lo, hi = span(p[0])
        for cp in range(lo, hi + 1):
            out[cp] = p[1]
    return out


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

    # Direction: right to left when every letter (gc L*) of the script is
    # Bidi_Class R or AL, from the files themselves; Common's letters are
    # mixed and it stays left to right.
    gc = classes("unicode-derivedgeneralcategory.txt")
    bidi = classes("unicode-derivedbidiclass.txt")
    letters = {}
    for cp in range(0x110000):
        if gc[cp] is not None and gc[cp].startswith("L"):
            assert bidi[cp] is not None, hex(cp)
            letters.setdefault(sc[cp], set()).add(bidi[cp] in ("R", "AL"))
    rtl = set()
    for i, kinds in letters.items():
        if i in (0, 1, 2):
            continue
        assert len(kinds) == 1, f"{codes[i]}: letters of both directions"
        if True in kinds:
            rtl.add(i)
    assert index["Hebr"] in rtl and index["Arab"] in rtl and index["Latn"] not in rtl
    assert len(rtl) == 37, len(rtl)
    directions = bytes(int(i in rtl) for i in range(len(codes)))

    # What can turn a run of a right-to-left script left to right: a
    # decimal digit or a regional indicator with no letter beside it, the
    # rule HarfBuzz's `hb_ensure_native_direction` applies when its caller
    # asks for left to right. Only a code point
    # a right-to-left run can hold is classed -- its Script or one of its
    # Script_Extensions is right to left, or it is Common or Inherited --
    # which keeps the table to the blocks those scripts touch.
    LETTER, NUMBER = 1, 2
    run_kind = [0] * 0x110000
    for cp in range(0x110000):
        held = sc[cp] in rtl or sc[cp] in (1, 2) or any(m in rtl for m in sets[scx[cp]])
        if not held or gc[cp] is None:
            continue
        if gc[cp].startswith("L"):
            run_kind[cp] = LETTER
        elif gc[cp] == "Nd" or 0x1F1E6 <= cp <= 0x1F1FF:
            run_kind[cp] = NUMBER
    assert run_kind[0x05D0] == LETTER and run_kind[0x0661] == NUMBER
    assert run_kind[0x0031] == NUMBER and run_kind[0x1F1E6] == NUMBER
    assert run_kind[0x02B9] == LETTER and run_kind[0x0041] == 0 and run_kind[0x0915] == 0

    # Paired brackets: 0 where a code point is none, else its pair's number
    # times two, plus one for a closer. BidiBrackets.txt's header: "two
    # characters, A and B, form a bracket pair if A has gc=Ps and B has
    # gc=Pe, both have bc=ON and Bidi_M=Y, and bmg of A is B"; each is
    # checked against the files pinned beside it.
    bpb, bpt = {}, {}
    for p in data_lines("unicode-bidibrackets.txt"):
        cp = int(p[0], 16)
        if p[2] == "n":
            continue
        bpb[cp], bpt[cp] = int(p[1], 16), p[2]
    assert len(bpb) == 128 and sum(t == "o" for t in bpt.values()) == 64, len(bpb)
    for cp, other in bpb.items():
        assert bpb[other] == cp and {bpt[cp], bpt[other]} == {"o", "c"}, hex(cp)
        assert gc[cp] == ("Ps" if bpt[cp] == "o" else "Pe"), hex(cp)
        assert bidi[cp] == "ON", hex(cp)
        # A pair is in one Script value: Common, or both in one script
        # (Tibetan's and Ogham's), which needs no resolving.
        assert sc[cp] == sc[other] and sets[scx[cp]] == sets[scx[other]], hex(cp)
    # BD16's equivalents, from UnicodeData.txt's canonical decompositions:
    # a bracket whose decomposition is one code point is that bracket.
    canonical = {}
    for line in (SRC / "unicode-unicodedata.txt").read_text(encoding="utf-8").splitlines():
        f = line.split(";")
        cp, dec = int(f[0], 16), f[5]
        if cp in bpb and dec and not dec.startswith("<") and " " not in dec:
            canonical[cp] = int(dec, 16)
    assert canonical == {0x2329: 0x3008, 0x232A: 0x3009}, canonical
    pair_of = {}
    for cp in sorted(c for c in bpb if bpt[c] == "o" and c not in canonical):
        pair_of[cp] = len(pair_of) + 1
    for cp, to in canonical.items():
        if bpt[cp] == "o":
            pair_of[cp] = pair_of[to]
    assert len(set(pair_of.values())) == 63 and max(pair_of.values()) * 2 + 1 <= 255
    bracket = [0] * 0x110000
    for cp, n in pair_of.items():
        bracket[cp] = n * 2
        bracket[bpb[cp]] = n * 2 + 1
    assert bracket[0x28] == 2 and bracket[0x29] == 3
    assert bracket[0x2329] == bracket[0x3008] and bracket[0x232A] == bracket[0x3009]

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
    run_idx, run_packed, run_blocks = trie(run_kind)
    br_idx, br_packed, br_blocks = trie(bracket)

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
        "//! `[[source: unicode-propertyvaluealiases]]`,",
        "//! `[[source: ms-opentype-scripttags]]`, `[[source: unicode-derivedbidiclass]]`,",
        "//! `[[source: gc-data]]`, `[[source: unicode-bidibrackets]]` and",
        "//! `[[source: unicode-data]]`.",
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
    out.append(bytestring(
        "DIRECTIONS", directions,
        "/// Each script's direction, a byte a script in index order: 1 where\n"
        "/// every letter of the script is Bidi_Class R or AL, else 0.",
    ))
    out.append(bytestring(
        "RUN_INDEX", run_idx,
        "/// What a code point is to a right-to-left run's direction, as a\n"
        "/// two-stage trie: 1 a letter, 2 a decimal digit or regional indicator,\n"
        "/// 0 neither or out of reach of such a run. Stage 1: code point >> 8 to\n"
        f"/// one of {run_blocks} blocks.",
    ))
    out.append(bytestring("RUN_BLOCKS", run_packed, "/// Stage 2 of [`RUN_INDEX`]."))
    out.append(bytestring(
        "BRACKET_INDEX", br_idx,
        "/// Each code point's paired bracket, as a two-stage trie: 0 none, else\n"
        "/// its pair's number times two, plus one for a closer; U+2329 and\n"
        "/// U+3008 share a number, as U+232A and U+3009 do. Stage 1: code point\n"
        f"/// >> 8 to one of {br_blocks} blocks.",
    ))
    out.append(bytestring("BRACKET_BLOCKS", br_packed, "/// Stage 2 of [`BRACKET_INDEX`]."))
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
        f"{len(sc_packed) + len(sc_idx)} + {len(scx_packed) + len(scx_idx)} + "
        f"{len(run_packed) + len(run_idx)} + {len(br_packed) + len(br_idx)} bytes, "
        f"{len(rtl)} right to left, {len(set(pair_of.values()))} bracket pairs"
    )


if __name__ == "__main__":
    main()
