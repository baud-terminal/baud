#!/usr/bin/env python3
"""Generate baud-domain/src/iso96_data.rs: the six 96-character sets that
`ESC - . /` designate, each the right half of an ISO 8859 part.

`[[source: ctlseqs]]` lists the finals: `A` ISO Latin-1 Supplemental, `B`
Latin-2, `F` Greek, `H` Hebrew, `L` Latin-Cyrillic and `M` Latin-5. Each
final is the one ISO-IR registers for the set (100, 101, 126, 138, 144 and
148), and each set is read here from Unicode's mapping file for its ISO 8859
part, pinned in `docs/research/sources/MANIFEST.toml`:

* `A` `[[source: iso8859-1]]`, `B` `[[source: iso8859-2]]`, `L`
  `[[source: iso8859-5]]` and `M` `[[source: iso8859-9]]`, whose version
  histories change no character mapping.
* `F` `[[source: iso8859-7]]`, which maps ISO 8859-7:2003 and says its 2.0
  version added "the three newly added characters 0xA4, 0xA5, 0xAA". ISO-IR
  126, the set `F` names, is ISO 8859-7:1987 ("ISO 8859-7:1987 is equivalent
  to ISO-IR-126, ELOT 928, and ECMA 118"), so those three are unassigned here.
* `H` `[[source: iso8859-8]]`, which maps ISO/IEC 8859-8:1999 and says its 1.1
  version added "mappings for LRM and RLM", at 0xFD and 0xFE. ISO-IR 138 is
  the 1988 set, so those two are unassigned here; 0xAF keeps the file's
  U+00AF, the 1.1 version's correction.

Each table is 96 codepoints indexed by `code - 0x20`, where `code` is the
7-bit code: 2/0 through 7/15 in GL, and the same positions less 0x80 in GR.
An unassigned position holds DEC's error character: `[[source: dec-std-070]]`
070-3 §3.4.3 calls 10/0 "an unassigned graphic character code" that "will
cause the error character (usually imaged as a REVERSE QUESTION MARK) to be
displayed on conforming devices".
"""
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SRC = ROOT / "docs/research/sources"
OUT = ROOT / "baud-domain/src/iso96_data.rs"
ERROR = "E"

# (Rust constant, final, file, ISO-IR, positions the file maps that the
# registered set does not have, and the doc line that says why)
SETS = [
    ("LATIN_1", "A", "iso8859-1.txt", 100, set(), "ISO Latin-1 Supplemental"),
    ("LATIN_2", "B", "iso8859-2.txt", 101, set(), "ISO Latin-2 Supplemental"),
    ("GREEK", "F", "iso8859-7.txt", 126, {0xA4, 0xA5, 0xAA}, "ISO Greek Supplemental"),
    ("HEBREW", "H", "iso8859-8.txt", 138, {0xFD, 0xFE}, "ISO Hebrew Supplemental"),
    ("LATIN_CYRILLIC", "L", "iso8859-5.txt", 144, set(), "ISO Latin-Cyrillic"),
    ("LATIN_5", "M", "iso8859-9.txt", 148, set(), "ISO Latin-5 Supplemental"),
]

KEY = {
    "iso8859-1.txt": "iso8859-1",
    "iso8859-2.txt": "iso8859-2",
    "iso8859-5.txt": "iso8859-5",
    "iso8859-7.txt": "iso8859-7",
    "iso8859-8.txt": "iso8859-8",
    "iso8859-9.txt": "iso8859-9",
}


def read(path):
    """The file's right half: byte -> (codepoint, name)."""
    out = {}
    for line in (SRC / path).read_text(encoding="utf-8").splitlines():
        if not line.startswith("0x"):
            continue
        cols = line.split("\t")
        b, cp = int(cols[0], 16), int(cols[1], 16)
        name = cols[-1].lstrip("#").strip()
        if 0xA0 <= b <= 0xFF:
            out[b] = (cp, name)
    if not out:
        sys.exit(f"{path}: no right-half mappings read")
    return out


lines = [
    "//! The six 96-character sets `ESC - . /` designate, generated from the",
    "//! sources pinned in `docs/research/sources/MANIFEST.toml`.",
    "//!",
    "//! **Do not edit by hand.** Regenerate with `scripts/gen-iso96-sets.py`;",
    "//! `iso96_tables_match_the_pinned_sources` fails if this file and the",
    "//! pinned sources disagree.",
    "//!",
    "//! Each table is indexed by `code - 0x20`, the 7-bit code, 2/0 through",
    "//! 7/15: a 96-character set has a character at SP's and DEL's positions",
    "//! too (`[[source: ecma35]]` 8.3.1). An unassigned position holds DEC's",
    "//! error character.",
    "",
    "use crate::{Codepoint, DEC_ERROR_CHARACTER};",
    "",
    "/// DEC's error character, at an unassigned position.",
    "const E: Codepoint = DEC_ERROR_CHARACTER;",
    "",
]
counts = []
for const, fin, path, ir, drop, title in SETS:
    m = read(path)
    for b in drop:
        if b not in m:
            sys.exit(f"{path}: 0x{b:02X} is listed as added later but the file does not map it")
    lines.append(f"/// {title}, final `{fin}`, ISO-IR {ir}: `[[source: {KEY[path]}]]`'s right half.")
    if drop:
        lines.append("///")
        lines.append(
            "/// Unassigned here and mapped by the file, which maps a later edition: "
            + ", ".join(f"`0x{b:02x}`" for b in sorted(drop))
            + "."
        )
    lines.append(f"pub(crate) static {const}: [Codepoint; 96] = [")
    n = 0
    for b in range(0xA0, 0x100):
        code = b - 0x80
        if b in m and b not in drop:
            cp, name = m[b]
            lines.append(f"    Codepoint(0x{cp:04x}), // 0x{code:02x} {name}")
            n += 1
        else:
            lines.append(f"    {ERROR + ',':<18} // 0x{code:02x} unassigned")
    lines.append("];")
    lines.append("")
    counts.append(f"{fin}: {n}")
out = "\n".join(lines).rstrip("\n") + "\n"

if "--check" in sys.argv:
    if not OUT.exists() or OUT.read_text() != out:
        print("STALE")
        sys.exit(1)
    print("OK")
else:
    OUT.write_text(out)
    print(", ".join(counts))
