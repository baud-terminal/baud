#!/usr/bin/env python3
"""Generate baud-font/src/indic_data.rs: each character's Indic syllabic
category and position, which baud-font's Indic shaper reads.

From the files pinned in docs/research/sources/MANIFEST.toml:
IndicSyllabicCategory.txt (unicode-indicsyllabiccategory),
IndicPositionalCategory.txt (unicode-indicpositionalcategory) and Blocks.txt
(unicode-blocks), Unicode 17.0.0.

**The mapping is HarfBuzz's.** The Unicode categories are finer than the
classes the Indic syllable grammar is written in (Microsoft's "Developing
OpenType Fonts for Devanagari Script", ms-devanagari-script-development: C,
V, N, H, ZWNJ, ZWJ, M, SM, VD, A and NBSP), and a shaper has to say which
class each one is. HarfBuzz 14.5.1's gen-indic-table.py
(harfbuzz-14-5-1-gen-indic-table) says it -- its category_map,
position_map, its overrides and its matra positions per script, re-expressed
here for the nine scripts its Indic shaper takes (Khmer and Myanmar, which
it shapes elsewhere, are left out, and so are the variation selectors it
classes for Myanmar alone). The fixtures in baud-font/tests/shape are
hb-shape's output, so the classes have to be the ones it shaped with.

Run with --check to fail when the committed file is stale.
"""

from __future__ import annotations

import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SRC = ROOT / "docs/research/sources"
DST = ROOT / "baud-font/src/indic_data.rs"

# The shaper's classes, in the order of `indic::Category`.
CATEGORIES = [
    "X", "C", "V", "N", "H", "ZWNJ", "ZWJ", "M", "SM", "A", "PLACEHOLDER",
    "DOTTEDCIRCLE", "RS", "MPst", "Repha", "Ra", "CM", "Symbol", "CS", "SMPst",
]
# The positions in a syllable, left to right, in the order of
# `indic::Position`.
POSITIONS = [
    "START", "RA_TO_BECOME_REPH", "PRE_M", "PRE_C", "BASE_C", "AFTER_MAIN",
    "ABOVE_C", "BEFORE_SUB", "BELOW_C", "AFTER_SUB", "BEFORE_POST", "POST_C",
    "AFTER_POST", "SMVD", "END",
]

# The blocks whose characters the shaper classes, and two single characters.
BLOCKS = [
    "Basic Latin", "Latin-1 Supplement", "Devanagari", "Bengali", "Gurmukhi",
    "Gujarati", "Oriya", "Tamil", "Telugu", "Kannada", "Malayalam",
    "Vedic Extensions", "General Punctuation", "Superscripts and Subscripts",
    "Devanagari Extended",
]
SINGLES = [0x00A0, 0x25CC]

CATEGORY_MAP = {
    "Other": "X",
    "Avagraha": "Symbol",
    "Bindu": "SM",
    "Brahmi_Joining_Number": "PLACEHOLDER",
    "Cantillation_Mark": "A",
    "Consonant": "C",
    "Consonant_Dead": "C",
    "Consonant_Final": "CM",
    "Consonant_Head_Letter": "C",
    "Consonant_Initial_Postfixed": "C",
    "Consonant_Killer": "M",
    "Consonant_Medial": "CM",
    "Consonant_Placeholder": "PLACEHOLDER",
    "Consonant_Preceding_Repha": "Repha",
    "Consonant_Prefixed": "X",
    "Consonant_Subjoined": "CM",
    "Consonant_Succeeding_Repha": "CM",
    "Consonant_With_Stacker": "CS",
    "Gemination_Mark": "SM",
    "Invisible_Stacker": "H",
    "Joiner": "ZWJ",
    "Modifying_Letter": "X",
    "Non_Joiner": "ZWNJ",
    "Nukta": "N",
    "Number": "PLACEHOLDER",
    "Number_Joiner": "PLACEHOLDER",
    "Pure_Killer": "M",
    "Register_Shifter": "RS",
    "Syllable_Modifier": "SM",
    "Tone_Letter": "X",
    "Tone_Mark": "N",
    "Virama": "H",
    "Visarga": "SM",
    "Vowel": "V",
    "Vowel_Dependent": "M",
    "Vowel_Independent": "V",
}
POSITION_MAP = {
    "Not_Applicable": "END",
    "Left": "PRE_C",
    "Top": "ABOVE_C",
    "Bottom": "BELOW_C",
    "Right": "POST_C",
    "Bottom_And_Right": "POST_C",
    "Left_And_Right": "POST_C",
    "Top_And_Bottom": "BELOW_C",
    "Top_And_Bottom_And_Left": "BELOW_C",
    "Top_And_Bottom_And_Right": "POST_C",
    "Top_And_Left": "ABOVE_C",
    "Top_And_Left_And_Right": "POST_C",
    "Top_And_Right": "POST_C",
    "Overstruck": "AFTER_MAIN",
    "Visual_order_left": "PRE_M",
}
CATEGORY_OVERRIDES = {
    0x2015: "PLACEHOLDER", 0x2022: "PLACEHOLDER",
    0x25FB: "PLACEHOLDER", 0x25FC: "PLACEHOLDER",
    0x25FD: "PLACEHOLDER", 0x25FE: "PLACEHOLDER",
    0x0930: "Ra", 0x09B0: "Ra", 0x09F0: "Ra", 0x0A30: "Ra", 0x0AB0: "Ra",
    0x0B30: "Ra", 0x0BB0: "Ra", 0x0C30: "Ra", 0x0CB0: "Ra", 0x0D30: "Ra",
    0x0953: "SM", 0x0954: "SM",
    0x0A40: "MPst",
    0x0A72: "C", 0x0A73: "C",
    0x1CE2: "A", 0x1CE3: "A", 0x1CE4: "A", 0x1CE5: "A", 0x1CE6: "A",
    0x1CE7: "A", 0x1CE8: "A", 0x1CED: "A",
    0xA8F2: "Symbol", 0xA8F3: "Symbol", 0xA8F4: "Symbol", 0xA8F5: "Symbol",
    0xA8F6: "Symbol", 0xA8F7: "Symbol", 0x1CE9: "Symbol", 0x1CEA: "Symbol",
    0x1CEB: "Symbol", 0x1CEC: "Symbol", 0x1CEE: "Symbol", 0x1CEF: "Symbol",
    0x1CF0: "Symbol", 0x1CF1: "Symbol",
    0x0A51: "M",
    0x11301: "SM", 0x11302: "SM", 0x11303: "SM", 0x1133B: "N", 0x1133C: "N",
    0x0AFB: "N", 0x0B55: "N",
    0x09FC: "PLACEHOLDER", 0x0C80: "PLACEHOLDER", 0x0D04: "PLACEHOLDER",
    0x25CC: "DOTTEDCIRCLE",
}
POSITION_OVERRIDES = {0x0A51: "BELOW_C", 0x0B01: "BEFORE_SUB"}
POSITIONED = ("CM", "SM", "RS", "H", "M", "MPst")
CONSONANTS = ("C", "CS", "Ra", "CM", "V", "PLACEHOLDER", "DOTTEDCIRCLE")
MATRAS = ("M", "MPst")
SMVD = ("SM", "SMPst", "VD", "A", "Symbol")


def matra_position(u: int, pos: str, block: str) -> str:
    """Where a dependent vowel sign goes, by script: HarfBuzz's
    matra_pos_left/right/top/bottom."""
    if pos == "PRE_C":
        return "PRE_M"
    if pos == "POST_C":
        return {
            "Devanagari": "AFTER_SUB", "Bengali": "AFTER_POST",
            "Gurmukhi": "AFTER_POST", "Gujarati": "AFTER_POST",
            "Oriya": "AFTER_POST", "Tamil": "AFTER_POST",
            "Telugu": "BEFORE_SUB" if u <= 0x0C42 else "AFTER_SUB",
            "Kannada": "BEFORE_SUB" if u < 0x0CC3 or u > 0x0CD6 else "AFTER_SUB",
            "Malayalam": "AFTER_POST",
        }.get(block, "AFTER_SUB")
    if pos == "ABOVE_C":
        return {
            "Devanagari": "AFTER_SUB", "Gurmukhi": "AFTER_POST",
            "Gujarati": "AFTER_SUB", "Oriya": "AFTER_MAIN", "Tamil": "AFTER_SUB",
            "Telugu": "BEFORE_SUB", "Kannada": "BEFORE_SUB",
        }.get(block, "AFTER_SUB")
    if pos == "BELOW_C":
        return {
            "Devanagari": "AFTER_SUB", "Bengali": "AFTER_SUB",
            "Gurmukhi": "AFTER_POST", "Gujarati": "AFTER_POST",
            "Oriya": "AFTER_SUB", "Tamil": "AFTER_POST", "Telugu": "BEFORE_SUB",
            "Kannada": "BEFORE_SUB", "Malayalam": "AFTER_POST",
        }.get(block, "AFTER_SUB")
    raise SystemExit(f"U+{u:04X}: a matra at {pos}")


def read(path: pathlib.Path) -> tuple[dict[int, str], list[str]]:
    out: dict[int, str] = {}
    header = []
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.startswith("#") and len(header) < 2:
            header.append(line)
        body = line.split("#", 1)[0]
        fields = [f.strip() for f in body.split(";")]
        if len(fields) < 2:
            continue
        lo, _, hi = fields[0].partition("..")
        start = int(lo, 16)
        end = int(hi, 16) if hi else start
        for u in range(start, end + 1):
            out[u] = fields[1]
    return out, header


def main() -> None:
    check = "--check" in sys.argv[1:]
    cats, h1 = read(SRC / "unicode-indicsyllabiccategory.txt")
    poss, h2 = read(SRC / "unicode-indicpositionalcategory.txt")
    blocks, h3 = read(SRC / "unicode-blocks.txt")
    listed = set(cats) | set(poss)
    data: dict[int, tuple[str, str]] = {}
    for u in sorted(listed):
        block = blocks.get(u, "No_Block")
        if u not in SINGLES and block not in BLOCKS:
            continue
        cat = CATEGORY_MAP[cats.get(u, "Other")]
        pos_u = poss.get(u, "Not_Applicable")
        if cat == "SM" and pos_u == "Not_Applicable":
            cat = "SMPst"
        data[u] = (cat, POSITION_MAP[pos_u])
    for u, cat in CATEGORY_OVERRIDES.items():
        pos = data.get(u, ("X", "END"))[1]
        data[u] = (cat, pos)
    for u, (cat, pos) in list(data.items()):
        if cat not in POSITIONED:
            pos = "END"
        if cat in CONSONANTS:
            pos = "BASE_C"
        elif cat in MATRAS:
            pos = matra_position(u, pos, blocks.get(u, "No_Block"))
        elif cat in SMVD:
            pos = "SMVD"
        data[u] = (cat, pos)
    for u, pos in POSITION_OVERRIDES.items():
        data[u] = (data.get(u, ("X", "END"))[0], pos)

    # The dense block: U+0900 to U+0D7F, the nine scripts' blocks.
    lo, hi = 0x0900, 0x0D80
    dense = [data.get(u, ("X", "END")) for u in range(lo, hi)]
    latin1 = [data.get(u, ("X", "END")) for u in range(0x100)]
    sparse = sorted(
        (u, c, p)
        for u, (c, p) in data.items()
        if not lo <= u < hi and u >= 0x100 and (c, p) != ("X", "END")
    )

    def code(c: str, p: str) -> bytes:
        return bytes([CATEGORIES.index(c), POSITIONS.index(p)])

    # Every table is a byte string: rustfmt rewraps array literals to its
    # width, so a generator emitting one could never pass its own --check.
    def bytestring(name: str, data: bytes, doc: list[str]) -> list[str]:
        body = []
        for i in range(0, len(data), 32):
            chunk = "".join(f"\\x{v:02x}" for v in data[i : i + 32])
            body.append(f"    {chunk}\\")
        return doc + [f'pub static {name}: &[u8; {len(data)}] = b"\\', *body, '";']

    out = [
        "//! Each character's class in an Indic syllable and its position, which",
        "//! the Indic shaper reads (`crate::indic`), generated from the sources",
        "//! pinned in `docs/research/sources/MANIFEST.toml`:",
        "//! `[[source: unicode-indicsyllabiccategory]]`,",
        "//! `[[source: unicode-indicpositionalcategory]]` and",
        "//! `[[source: unicode-blocks]]`, through HarfBuzz 14.5.1's mapping",
        "//! (`[[source: harfbuzz-14-5-1-gen-indic-table]]`). A class and a",
        "//! position are each a byte, the numbers of `crate::syllabic`'s",
        "//! `Category` and `Position`.",
        "//!",
        "//! **Do not edit by hand.** Regenerate with `scripts/gen-indic-tables.py`.",
        "//! `indic_tables_match_the_pinned_data` fails if this file and the pinned",
        "//! data disagree.",
        "//!",
        "//! Generated from files with these headers:",
    ]
    for h in h1 + h2 + h3:
        out.append(f"//! {h}")
    out.append("")
    out.append(f"/// The first character of [`DENSE`]: U+{lo:04X}.")
    out.append(f"pub const DENSE_FIRST: u32 = 0x{lo:04X};")
    out.append("")
    out += bytestring(
        "DENSE",
        b"".join(code(c, p) for c, p in dense),
        [
            f"/// The class and position of U+{lo:04X}..U+{hi - 1:04X}, two bytes a",
            "/// character, indexed by the character less [`DENSE_FIRST`].",
        ],
    )
    out.append("")
    out += bytestring(
        "LATIN1",
        b"".join(code(c, p) for c, p in latin1),
        [
            "/// The class and position of U+0000..U+00FF, as [`DENSE`]'s: the",
            "/// digits, NO-BREAK SPACE and the superscripts two and three are classed.",
        ],
    )
    out.append("")
    blocks = sorted({u >> 8 for u, _, _ in sparse})
    out.append("/// The 256-character blocks [`SPARSE`] holds characters of, each a")
    out.append("/// character shifted right by eight, in ascending order: a character")
    out.append("/// in none of them is unclassed without a search.")
    out.append(
        f"pub static SPARSE_BLOCKS: [u32; {len(blocks)}] = ["
        + ", ".join(f"0x{b:03X}" for b in blocks)
        + "];"
    )
    out.append("")
    out += bytestring(
        "SPARSE",
        b"".join(u.to_bytes(3, "big") + code(c, p) for u, c, p in sparse),
        [
            "/// Every other classed character, five bytes each: the character in",
            "/// three, big-endian, then its class and position; in ascending order.",
        ],
    )
    text = "\n".join(out) + "\n"
    if check:
        have = DST.read_text(encoding="utf-8") if DST.exists() else ""
        if have != text:
            print(f"{DST} is stale; rerun scripts/gen-indic-tables.py", file=sys.stderr)
            sys.exit(1)
        return
    DST.write_text(text, encoding="utf-8")
    print(f"{DST}: {hi - lo} dense, {len(sparse)} sparse")


if __name__ == "__main__":
    main()
