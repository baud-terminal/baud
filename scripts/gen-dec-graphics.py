#!/usr/bin/env python3
"""Generate baud-domain/src/dec_graphics.rs from the pinned foot source.

DEC's tables name each glyph of the set and give no Unicode codepoint:
`[[source: vt100-ug-ch3]]` Table 3-9 names all 32 positions, 137 through 176
octal, and `[[source: dec-special-graphics]]` draws them. foot's `vt100_0[]` is
one implementation's choice of codepoint for 31 of them, `0x60..0x7E`, and is
cited as foot's rather than as DEC's, because that is whose judgement it is.
It omits `0x5F`, which DEC names "Blank"; which codepoint that stores is an
open question. Until it is answered, this table is 31 entries and baud leaves
`0x5F` as `_`.
"""
import pathlib, re, sys, unicodedata

SRC = pathlib.Path(__file__).resolve().parent.parent / "docs/research/sources/foot-terminal.c"
OUT = pathlib.Path("baud-domain/src/dec_graphics.rs")

src = SRC.read_text(errors="ignore")
i = src.index("static const char32_t vt100_0[]")
chars = re.findall(r"U'(.)'", src[i:src.index("};", i)])
if len(chars) != 31:
    sys.exit(f"expected 31 entries, found {len(chars)}")

lines = ['''//! DEC Special Graphics, generated from the source pinned in
//! `docs/research/sources/MANIFEST.toml`.
//!
//! **Do not edit by hand.** Regenerate with `scripts/gen-dec-graphics.py`;
//! `dec_graphics_table_matches_the_pinned_source` fails if this file and the
//! pinned source disagree.
//!
//! Indexed by `byte - 0x60`, covering `` ` `` through `~`: foot's 31 entries.
//! The set is 32. DEC replaces `0x5F` as well, with "Blank"
//! (`[[source: vt100-ug-ch3]]` Table 3-9), and foot does not, so neither does
//! this table: an open departure, the codepoint `0x5F` stores still to be
//! chosen.

/// `[[source: foot-vt100-0]]`'s `vt100_0[]`.
pub static DEC_GRAPHICS: [u32; 31] = [''']
for n, c in enumerate(chars):
    lines.append(f"    0x{ord(c):04x}, // {chr(0x60+n)} -> {c}  {unicodedata.name(c, '?')}")
lines.append("];")
out = "\n".join(lines) + "\n"

if "--check" in sys.argv:
    if OUT.read_text() != out:
        print("STALE")
        sys.exit(1)
    print("OK")
else:
    OUT.write_text(out)
    print(f"{len(chars)} entries")
