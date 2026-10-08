#!/usr/bin/env python3
"""Generate baud-domain/src/nrcs_data.rs from the pinned xterm source.

The National Replacement Character Sets replace a handful of ASCII positions
per language. None of the DEC texts read gives a Unicode codepoint for them,
and xterm's `charsets.h` does, as `MAP`/`UNI` macros carrying the Unicode
codepoint. Cited as xterm's judgement rather than as DEC's, the same way the
line-drawing table's codepoints are cited as foot's.
"""
import re, sys, pathlib, collections

SRC = pathlib.Path(__file__).resolve().parent.parent / "docs/research/sources/xterm-charsets.h"
OUT = pathlib.Path("baud-domain/src/nrcs_data.rs")

# xterm's macro name -> baud's `Nrcs` variant.
WANTED = {
    "Dutch": "Dutch",
    "Finnish": "Finnish",
    "French": "French",
    "French_Canadian": "FrenchCanadian",
    "German": "German",
    "Italian": "Italian",
    "Norwegian_Danish": "NorwegianDanish",
    "Portuguese": "Portuguese",
    "Spanish": "Spanish",
    "Swedish": "Swedish",
    "Swiss": "Swiss",
    "Greek": "Greek",
    "Hebrew": "Hebrew",
    "Turkish": "Turkish",
}

src = SRC.read_text(errors="ignore")
sets = collections.OrderedDict()
for m in re.finditer(r"#define map_NRCS_(\w+)\(code\)(.*?)\n\n", src, re.S):
    name, body = m.group(1), m.group(2)
    if name not in WANTED:
        continue
    pairs = []
    for line in body.splitlines():
        u = re.search(r"UNI\(0x([0-9A-Fa-f]+),\s*0x([0-9A-Fa-f]+)\)", line)
        if u:
            pairs.append((int(u.group(1), 16), int(u.group(2), 16)))
            continue
        mm = re.search(r"MAP\(0x([0-9A-Fa-f]+),\s*\w+\)\s*/\*\s*U\+([0-9A-Fa-f]+)", line)
        if mm:
            pairs.append((int(mm.group(1), 16), int(mm.group(2), 16)))
    if pairs:
        sets[WANTED[name]] = sorted(pairs)

missing = set(WANTED.values()) - set(sets)
if missing:
    sys.exit(f"no table extracted for {sorted(missing)}")

lines = ['''//! National Replacement Character Sets, generated from the source pinned in
//! `docs/research/sources/MANIFEST.toml`.
//!
//! **Do not edit by hand.** Regenerate with `scripts/gen-nrcs-tables.py`;
//! `nrcs_tables_match_the_pinned_source` fails if this file and the pinned
//! source disagree.
//!
//! Each set replaces a handful of ASCII positions -- "the term *replacement*
//! says that the character set is formed by replacing some of the characters
//! with more useful ones for a given language". Everything not listed is ASCII.

use crate::Nrcs;
''']
for variant, pairs in sets.items():
    const = variant.upper()
    lines.append(f"/// `[[source: xterm-charsets]]`'s `map_NRCS_{variant}`.")
    lines.append(f"static {const}: [(u8, u32); {len(pairs)}] = [")
    for b, cp in pairs:
        lines.append(f"    (0x{b:02x}, 0x{cp:04x}),")
    lines.append("];")
    lines.append("")

lines.append("/// `ESC ( A` — United Kingdom, whose one substitution xterm spells inline")
lines.append("/// in `charsets.c` rather than as a macro.")
lines.append("static UK: [(u8, u32); 1] = [(0x23, 0x00a3)];")
lines.append("")
lines.append("/// The replacement table for a set, or `None` where there is none.")
lines.append("pub fn nrcs_table(set: Nrcs) -> &'static [(u8, u32)] {")
lines.append("    match set {")
for variant in sets:
    lines.append(f"        Nrcs::{variant} => &{variant.upper()},")
lines.append("    }")
lines.append("}")
lines.append("")
lines.append("/// United Kingdom, which is a `Charset` of its own rather than an `Nrcs`.")
lines.append("pub fn uk_table() -> &'static [(u8, u32)] {")
lines.append("    &UK")
lines.append("}")

out = "\n".join(lines) + "\n"
if "--check" in sys.argv:
    if OUT.read_text() != out:
        print("STALE")
        sys.exit(1)
    print("OK")
else:
    OUT.write_text(out)
    print(f"{len(sets)} sets, {sum(len(v) for v in sets.values())} substitutions")
