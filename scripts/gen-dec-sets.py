#!/usr/bin/env python3
"""Generate baud-domain/src/dec_sets_data.rs: the DEC 94-character sets baud
translates beyond DEC Special Graphics and the national replacement sets.

Each table is 94 codepoints, indexed by `byte - 0x21`, 2/1 through 7/14, so the
put path indexes it and never searches. A set's every position is in the
table: a defined position holds its character and a reserved one DEC's error
character, so nothing here falls back to the byte.

The standard is DEC's text and xterm's `charsets.h` is practice
(`[[source: xterm-charsets]]`). Table 2-3 is read from
`baud-domain/tests/vt220_tables.tsv`, the cells `scripts/extract-vt220-tables.py`
reads out of the pinned VT220 chapter, fetched by URL and hash-checked; where a
text is `external` and nothing extracts it (EK-PPLV2-PM, the VT382 RM), what was
read from it is a constant below with its citation. xterm's macro for each set is parsed too, and every place it differs from
the table is named in `XTERM_DEPARTS` or the script fails: so the output says
which cells are DEC's and xterm cannot drift silently.

* **DEC Supplemental** (`ESC ( <` and `ESC ( % 5`):
  `[[source: dec-special-graphics]]` Table 2-3's right half, read from the table, 81 glyphs and 13
  empty cells. The empty cells are checked against `[[source: dec-pplv2-pm]]`
  Figure A-2's hatched (reserved) cells, a constant (note AF §3.1).
* **JIS-Katakana** (`ESC ( I`): `[[source: dec-pplv2-pm]]` §A.12, "specified
  by JIS X 0201-1976. There are 63 characters defined", 2/1-5/15, the rest
  reserved; the characters from `[[source: unicode-jis0201]]`, read here,
  0xA1-0xDF.
* **JIS-Roman** (`ESC ( J`): `[[source: vt382-rm]]` §2.4.5, ASCII but 5/12 the
  Yen sign and 7/14 the over line. 7/14 is open (note AF, Q-JIS-Roman), so it
  is listed in `OPEN` and stays `~` until answered.

A reserved position draws `DEC_ERROR_CHARACTER` (`nrcs_data.rs`): "The error
character is printed when one of these reserved characters is received by the
device" (`[[source: dec-pplv2-pm]]` §A.2, §A.12), and on a video terminal
`[[source: dec-std-070]]` 070-3 §3.4.3 says "The character code 10/0 in 8-bit
environments is treated as an unassigned graphic character code, and will cause
the error character (usually imaged as a REVERSE QUESTION MARK) to be displayed
on conforming devices" (by analogy, inferred), as `[[source: vt382-rm]]` §2.4.6
says of JIS-Katakana's reserved codes.
"""
import pathlib, re, sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SRC = ROOT / "docs/research/sources"
VT220_TABLES = ROOT / "baud-domain/tests/vt220_tables.tsv"
CHARSETS_H = SRC / "xterm-charsets.h"
JIS0201 = SRC / "JIS0201.TXT"
UNICODE_DATA = SRC / "unicode-unicodedata.txt"
OUT = pathlib.Path("baud-domain/src/dec_sets_data.rs")

ERROR = "E"  # the generated file's name for DEC_ERROR_CHARACTER
ERROR_CODEPOINT = 0x2426

# `[[source: dec-pplv2-pm]]` Figure A-2 (PDF p. 182), its hatched cells, read
# by stroke detection from the hash-checked research copy (note AF §3.1). A
# second DEC text for DEC Supplemental's mask.
FIGURE_A2_RESERVED = {0x24, 0x26, 0x2C, 0x2D, 0x2E, 0x2F, 0x34, 0x38, 0x3E, 0x50, 0x5E, 0x70, 0x7E}

# `[[source: dec-pplv2-pm]]` §A.12: 63 defined, 2/1-5/15; 6/0-7/14 reserved,
# as `[[source: vt382-rm]]` Figure 2-10 draws them in GR (note AF §3.4).
KATAKANA_DEFINED = range(0x21, 0x60)

# `[[source: vt382-rm]]` §2.4.5, p. 2-17 [img]: "This set differs from the
# ASCII Character Set only in that the Yen sign(¥)replaces the backslash(\\) in
# column 5, row 12 (5/12) and the over line (¯) replaces the tilde (˜) in
# column 7, row 14 (7/14)."
JIS_ROMAN_YEN = (0x5C, 0x00A5)

# Positions a DEC text defines and no answer has given a codepoint yet, by
# (set, byte): each draws its byte until the question is answered.
OPEN = {
    ("JisRoman", 0x7E): (
        "Q-JIS-Roman (note AF): `[[source: vt382-rm]]` §2.4.5 names it the",
        "\"over line\" and `[[source: dec-pplv2-pm]]` Table A-2 \"Macron\";",
        "U+203E or U+00AF",
    ),
}

# Where xterm's macro differs from the table, by (set, byte): what xterm
# draws, and why the table does not.
XTERM_DEPARTS = {
    ("Supplemental", 0x5F): (
        0x005F,
        "xterm's `UNI(0x5F, 0x005F)` draws `_`; Table 2-3,",
        "`[[source: vt320-uu-appe]]` and Figure A-2 have ß (note AF §3.1)",
    ),
    ("JisKatakana", 0x7E): (
        0x007E,
        "xterm's macro lists no 0x7E and so draws `~`; DEC reserves 7/14",
        "(§A.12; `[[source: vt382-rm]]` Figure 2-10)",
    ),
    ("JisRoman", 0x7E): (
        0x203E,
        "xterm's `UNI(0x7E, 0x203E)` is one answer to Q-JIS-Roman, open",
    ),
}

failures = []


def fail(msg):
    failures.append(msg)


names = {}
for line in UNICODE_DATA.read_text().splitlines():
    f = line.split(";")
    names[int(f[0], 16)] = f[1]


def name(cp):
    return names.get(cp, "?")


# ── DEC Table 2-3, right half ────────────────────────────────────────────────
#
# vt220_tables.tsv holds each GR cell as `cell`, `T2-3b`, the code's low seven
# bits in hex, the glyph (empty where the table draws nothing) and a shaded
# field Table 2-3 does not use.
table23 = {}
for line in VT220_TABLES.read_text().splitlines():
    f = line.split("\t")
    if line.startswith("#") or f[:2] != ["cell", "T2-3b"]:
        continue
    if len(f) != 5:
        fail(f"vt220_tables.tsv: a line this script cannot parse: {line!r}")
        continue
    code, glyph = int(f[2], 16), f[3]
    if code in table23:
        fail(f"Table 2-3: 0x{code:02X} twice")
    if len(glyph) > 1:
        fail(f"Table 2-3: 0x{code:02X} draws {glyph!r}, more than one character")
    table23[code] = ord(glyph) if glyph else None
if sorted(table23) != list(range(0x21, 0x7F)):
    fail(f"Table 2-3: {len(table23)} of 94 GR cells parsed")
empty = {c for c, g in table23.items() if g is None}
if empty != FIGURE_A2_RESERVED:
    fail(
        "Table 2-3's empty cells are not Figure A-2's reserved ones: "
        f"{sorted(empty ^ FIGURE_A2_RESERVED)}"
    )
supplemental = [table23[c] if table23.get(c) is not None else ERROR for c in range(0x21, 0x7F)]

# ── JIS X 0201, through Unicode's mapping ────────────────────────────────────
jis = {}
for line in JIS0201.read_text().splitlines():
    if line.startswith("#") or not line.strip():
        continue
    m = re.match(r"0x([0-9A-F]{2})\t0x([0-9A-F]{4})\t#", line)
    if not m:
        fail(f"JIS0201.TXT: a line this script cannot parse: {line}")
        continue
    jis[int(m.group(1), 16)] = int(m.group(2), 16)
for c in KATAKANA_DEFINED:
    if c | 0x80 not in jis:
        fail(f"JIS-Katakana 0x{c:02X}: JIS0201.TXT maps no 0x{c | 0x80:02X}")
if failures:
    sys.exit("\n".join(failures))
katakana = [jis[c | 0x80] if c in KATAKANA_DEFINED else ERROR for c in range(0x21, 0x7F)]
if sum(1 for v in katakana if v != ERROR) != 63:
    fail("JIS-Katakana: not 63 defined positions")
if jis.get(JIS_ROMAN_YEN[0]) != JIS_ROMAN_YEN[1]:
    fail("JIS0201.TXT does not map 0x5C to U+00A5, which VT382 §2.4.5 names")
roman = list(range(0x21, 0x7F))
roman[JIS_ROMAN_YEN[0] - 0x21] = JIS_ROMAN_YEN[1]

TABLES = {
    "Supplemental": supplemental,
    "JisKatakana": katakana,
    "JisRoman": roman,
}

# ── xterm's macros, as practice ──────────────────────────────────────────────
charsets_h = CHARSETS_H.read_text(errors="ignore").splitlines()
MACROS = {
    "Supplemental": "map_DEC_Supp_Graphic",
    "JisKatakana": "map_JIS_Katakana",
    "JisRoman": "map_JIS_Roman",
}


def macro(name):
    """The `UNI`/`XXX` entries of one macro, and whether it has a default arm."""
    for i, line in enumerate(charsets_h):
        if re.match(rf"#define {name}\(", line):
            body, j = [], i
            while charsets_h[j].rstrip().endswith("\\"):
                j += 1
                body.append(charsets_h[j])
            if body:
                break
    else:
        fail(f"charsets.h: no {name} with a body")
        return {}, False
    entries, default = {}, False
    for raw in body:
        text = raw.strip().removesuffix("\\").strip()
        if m := re.fullmatch(r"UNI\(0x([0-9A-F]{2}), 0x([0-9A-F]{4})\)(?:\s*/\*.*\*/)?", text):
            entries[int(m.group(1), 16)] = int(m.group(2), 16)
        elif m := re.fullmatch(r"XXX\(0x([0-9A-F]{2}), UNDEF\)(?:\s*/\*.*\*/)?", text):
            entries[int(m.group(1), 16)] = ERROR_CODEPOINT
        elif text.startswith("default:"):
            default = True
        elif text in ("", "switch (code) {", "}", "} \\", "begin_CODEPAGE(94)", "end_CODEPAGE()"):
            continue
        else:
            fail(f"charsets.h, {name}: an entry this script cannot parse: {text}")
    return entries, default


for set_, mac in MACROS.items():
    entries, default = macro(mac)
    for c in range(0x21, 0x7F):
        # Unlisted: the default arm, `dft`, is GR's Latin-1 code in the one
        # macro that has it (`charsets.c:443` passes `chr = eight`), and the
        # byte itself in a macro without one.
        if c in entries:
            xterm = entries[c]
        elif default:
            xterm = c | 0x80
        else:
            xterm = c
        ours = TABLES[set_][c - 0x21]
        ours = ERROR_CODEPOINT if ours == ERROR else ours
        if (set_, c) in XTERM_DEPARTS:
            if xterm != XTERM_DEPARTS[(set_, c)][0] or xterm == ours:
                fail(f"{set_} 0x{c:02X}: xterm's departure is not what XTERM_DEPARTS records")
        elif xterm != ours:
            fail(f"{set_} 0x{c:02X}: xterm U+{xterm:04X}, the table U+{ours:04X}, and no departure named")

if failures:
    sys.exit("\n".join(failures))

DOCS = {
    "Supplemental": [
        "/// DEC Supplemental, `ESC ( <` and `ESC ( % 5`: `[[source: dec-special-graphics]]`",
        "/// Table 2-3's right half, its 81 glyphs and, in its 13 empty cells, the",
        "/// error character -- the cells `[[source: dec-pplv2-pm]]` Figure A-2 hatches",
        "/// and §A.2 says print it.",
    ],
    "JisKatakana": [
        "/// JIS-Katakana, `ESC ( I`: 2/1-5/15 are JIS X 0201's katakana, which",
        "/// `[[source: dec-pplv2-pm]]` §A.12 names, through `[[source: unicode-jis0201]]`;",
        "/// 6/0-7/14 are reserved and draw the error character (§A.12;",
        "/// `[[source: vt382-rm]]` §2.4.6 and Figure 2-10).",
    ],
    "JisRoman": [
        "/// JIS-Roman, `ESC ( J`: ASCII with 5/12 the Yen sign (`[[source: vt382-rm]]`",
        "/// §2.4.5; `[[source: unicode-jis0201]]`).",
    ],
}

lines = ['''//! The DEC 94-character sets beyond DEC Special Graphics and the national
//! replacement sets, generated from the sources pinned in
//! `docs/research/sources/MANIFEST.toml`.
//!
//! **Do not edit by hand.** Regenerate with `scripts/gen-dec-sets.py`;
//! `dec_set_tables_match_the_pinned_sources` fails if this file and the pinned
//! sources disagree.
//!
//! Each table is indexed by `byte - 0x21`, 2/1 through 7/14, and holds every
//! position: SP and DEL are not graphic characters of a 94-character set
//! (`[[source: dec-std-070]]` 070-3 §3.4.1-3.4.2) and never index it. A
//! reserved position holds DEC's error character.

use crate::{Codepoint, DEC_ERROR_CHARACTER};

/// DEC's error character, at a reserved position.
const E: Codepoint = DEC_ERROR_CHARACTER;
''']
CONST = {"Supplemental": "DEC_SUPPLEMENTAL", "JisKatakana": "JIS_KATAKANA", "JisRoman": "JIS_ROMAN"}
for set_, table in TABLES.items():
    lines.extend(DOCS[set_])
    opens = [(b, why) for (s, b), why in OPEN.items() if s == set_]
    departs = [(b, d) for (s, b), d in XTERM_DEPARTS.items() if s == set_]
    if opens:
        lines.append("///")
        lines.append("/// Open, drawing its byte until answered:")
        for b, why in opens:
            lines.append("///")
            lines.append(f"/// * `0x{b:02x}`: {why[0]}")
            lines.extend(f"///   {w}" for w in why[1:])
            lines[-1] += "."
    if departs:
        lines.append("///")
        lines.append("/// Where `[[source: xterm-charsets]]`' macro, practice, differs:")
        for b, (_, *why) in departs:
            lines.append("///")
            lines.append(f"/// * `0x{b:02x}`: {why[0]}")
            lines.extend(f"///   {w}" for w in why[1:])
            lines[-1] += "."
    lines.append(f"pub(crate) static {CONST[set_]}: [Codepoint; 94] = [")
    for c, v in zip(range(0x21, 0x7F), table):
        if v == ERROR:
            # Padded as rustfmt aligns a run of trailing comments, so the
            # formatter leaves the generated file as written.
            lines.append(f"    {ERROR + ',':<18} // 0x{c:02x} reserved")
        else:
            lines.append(f"    Codepoint(0x{v:04x}), // 0x{c:02x} {name(v)}")
    lines.append("];")
    lines.append("")
out = "\n".join(lines).rstrip("\n") + "\n"

if "--check" in sys.argv:
    if not OUT.exists() or OUT.read_text() != out:
        print("STALE")
        sys.exit(1)
    print("OK")
else:
    OUT.write_text(out)
    print(", ".join(f"{s}: {sum(1 for v in t if v != ERROR)} defined" for s, t in TABLES.items()))
