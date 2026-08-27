#!/usr/bin/env python3
"""Generate baud-domain/src/dec_graphics.rs from the pinned foot source.

The DEC-authored table is images, not text -- `MANIFEST.toml` records that the
page was fetched and checked. foot's `vt100_0[]` is the same mapping in
machine-readable form, and is cited as foot's rather than as DEC's, because
that is whose judgement it is.
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
//! Indexed by `byte - 0x60`, covering `` ` `` through `~`, which is the whole
//! of the set that differs from ASCII.

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
