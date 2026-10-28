#!/usr/bin/env python3
"""Generate baud-domain/src/nrcs_data.rs from the pinned xterm source.

The National Replacement Character Sets replace a handful of ASCII positions
per language. None of the DEC texts read gives a Unicode codepoint for them,
and xterm's `charsets.h` does, as `MAP`/`UNI` macros: a `UNI` line carries the
codepoint, and a `MAP` line names an X keysym, resolved here by its `#define`
in `[[source: xorg-keysymdef]]`. By the value, not by the line's `/* U+ */`
comment: this script read the comment, `MAP(0x40, XK_Eacute)` carries none,
and Swedish 4/0 was dropped without a word (note AF §2.3, D1). Those entries
are xterm's judgement and are cited as xterm's, the way the line-drawing
table's codepoints are cited as foot's.

Two named lists stand between xterm's macros and baud's tables:

* `SKIP`, lines the source has and baud does not emit yet, each with the
  question that decides it;
* `DEC_OVERRIDES`, positions xterm's table lacks, typed by hand from the DEC
  text that draws them -- that text is `external`, so nothing can extract it
  here -- each with its citation, which goes into the generated table's doc.

Every check runs before anything is written or compared, so `--check` fails
on its cause rather than on a stale file: a line of a wanted macro that does
not parse, a keysym outside Latin-1, a skip or an override the source no
longer bears out, and a set whose emitted count is not its macro's line count
adjusted by the two lists.
"""
import re, sys, pathlib

ROOT = pathlib.Path(__file__).resolve().parent.parent
SRC = ROOT / "docs/research/sources/xterm-charsets.h"
KEYSYMS = ROOT / "docs/research/sources/xorg-keysymdef.h"
OUT = pathlib.Path("baud-domain/src/nrcs_data.rs")

# xterm's macro name -> baud's `Nrcs` variant, in the order the tables are
# written.
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

# Lines of a wanted macro that baud does not emit, by (macro, byte). Each must
# be an `XXX(byte, UNDEF)` line, so a skip cannot swallow a position xterm maps.
SKIP = {
    # `charsets.h:2380-2381`, marked "unused". xterm draws them as U+2426.
    ("Greek", 0x79): "awaits Q-UNDEF",
    ("Greek", 0x7A): "awaits Q-UNDEF",
}

# Positions xterm's macro lacks, by (macro, byte): (codepoint, citation as the
# doc lines it is emitted as). Each byte must be absent from the macro, so a
# source that gains it fails here.
DEC_OVERRIDES = {
    ("Turkish", 0x21): (
        0x0131,
        (
            "`[[source: dec-pplv2-pm]]` Figure A-26, which draws a",
            "dotless small i at 2/1 (note AF §2.4)",
        ),
    ),
}

# DEC's error character: what a DEC text says is "printed" or "displayed" for
# SUB, a reserved position or A0 with a 94-set in GR. DEC names no codepoint
# (070-3 §3.4.3: "usually imaged as a REVERSE QUESTION MARK"); U+2426 is note
# AE's AE8.1. It is emitted beside the tables so every consumer takes it from
# one place, and `the_dec_error_character_is_symbol_for_substitute_form_two`
# holds it to its name in the pinned UnicodeData.txt.
ERROR_CHARACTER = 0x2426

ENTRY = re.compile(r"\b(MAP|UNI|XXX)\(")
MAP = re.compile(r"MAP\(0x([0-9A-Fa-f]{2}),\s*(XK_\w+)\)(?:\s*/\*(.*?)\*/)?")
UNI = re.compile(r"UNI\(0x([0-9A-Fa-f]{2}),\s*0x([0-9A-Fa-f]{4,6})\)(?:\s*/\*.*?\*/)?")
XXX = re.compile(r"XXX\(0x([0-9A-Fa-f]{2}),\s*UNDEF\)(?:\s*/\*.*?\*/)?")
STRUCTURE = {"", "switch (code) {", "}"}

failures = []


def fail(msg):
    failures.append(msg)


# `#define XK_name value`, where the value is a number or, for an alias,
# another keysym's name.
keysyms = dict(
    re.findall(r"^#define (XK_\w+)\s+(0x[0-9A-Fa-f]+|XK_\w+)\b", KEYSYMS.read_text(), re.M)
)


def keysym(name):
    seen = []
    while name in keysyms and name not in seen:
        seen.append(name)
        value = keysyms[name]
        if not value.startswith("XK_"):
            return int(value, 16)
        name = value
    return None


src_lines = SRC.read_text(errors="ignore").splitlines()

# Each wanted macro's body: its `#define` line and every line a trailing
# backslash continues. Greek, Hebrew and Turkish are defined twice, with a
# `switch` under `#if OPT_WIDE_CHARS` and as `/* nothing */` under its `#else`
# (`charsets.h:2533-2541`); those continue nothing, and are not bodies.
bodies = {}
for i, line in enumerate(src_lines):
    m = re.match(r"#define map_NRCS_(\w+)\(code\)", line)
    if not m or m.group(1) not in WANTED:
        continue
    body, j = [], i
    while src_lines[j].rstrip().endswith("\\"):
        j += 1
        body.append((j + 1, src_lines[j]))
    if not body:
        continue
    if m.group(1) in bodies:
        fail(f"map_NRCS_{m.group(1)}: two definitions with a body")
    bodies[m.group(1)] = body

sets = {}
used_skips, used_overrides = set(), set()
for name in WANTED:
    if name not in bodies:
        fail(f"map_NRCS_{name}: no definition with a body")
        continue
    pairs, unemitted, source = [], [], 0
    for n, raw in bodies[name]:
        text = raw.strip().removesuffix("\\").strip()
        if text in STRUCTURE:
            continue
        if not ENTRY.search(text):
            fail(f"charsets.h:{n}: neither an entry nor the switch around them: {text}")
            continue
        source += 1
        if m := UNI.fullmatch(text):
            pairs.append((int(m.group(1), 16), int(m.group(2), 16)))
        elif m := MAP.fullmatch(text):
            byte, value = int(m.group(1), 16), keysym(m.group(2))
            if value is None:
                fail(f"charsets.h:{n}: {m.group(2)} has no #define in xorg-keysymdef.h")
                continue
            # A keysym is its codepoint only in Latin-1; past it the two
            # numberings part, and this would need a mapping it does not have.
            if not (0x20 <= value <= 0x7E or 0xA0 <= value <= 0xFF):
                fail(f"charsets.h:{n}: {m.group(2)} is {value:#x}, outside Latin-1")
                continue
            said = re.search(r"U\+([0-9A-Fa-f]{4,6})", m.group(3) or "")
            if said and int(said.group(1), 16) != value:
                fail(
                    f"charsets.h:{n}: {m.group(2)} is U+{value:04X}, "
                    f"its comment says U+{said.group(1)}"
                )
                continue
            pairs.append((byte, value))
        elif m := XXX.fullmatch(text):
            # `UNDEF` is a choice nobody has made for baud: the line is
            # emitted only once an answer gives it a codepoint, and until
            # then it is skipped by name or the count below fails.
            byte = int(m.group(1), 16)
            if (name, byte) in SKIP:
                used_skips.add((name, byte))
            else:
                unemitted.append(f"charsets.h:{n} XXX(0x{byte:02x}, UNDEF)")
        else:
            fail(f"charsets.h:{n}: an entry this script cannot parse: {text}")
    mapped = {b for b, _ in pairs}
    if len(mapped) != len(pairs):
        fail(f"{name}: two entries for one byte")
    for (set_, byte), (cp, _) in DEC_OVERRIDES.items():
        if set_ != name:
            continue
        if byte in mapped:
            fail(f"{name} {byte:#04x}: xterm's macro has it now; retire the DEC override")
            continue
        used_overrides.add((set_, byte))
        pairs.append((byte, cp))
    skipped = sum(1 for k in used_skips if k[0] == name)
    added = sum(1 for k in used_overrides if k[0] == name)
    if len(pairs) != source - skipped + added:
        fail(
            f"{name}: {len(pairs)} emitted for {source} lines in map_NRCS_{name}, "
            f"{skipped} skipped and {added} added; not emitted: {', '.join(unemitted) or 'none'}"
        )
    sets[name] = sorted(pairs)

for key in SKIP.keys() - used_skips:
    fail(f"skip {key[0]} {key[1]:#04x}: no `XXX(..., UNDEF)` line there any more")
for key in DEC_OVERRIDES.keys() - used_overrides:
    fail(f"DEC override {key[0]} {key[1]:#04x}: not applied")

if failures:
    sys.exit("\n".join(failures))

lines = ['''//! National Replacement Character Sets, generated from the sources pinned in
//! `docs/research/sources/MANIFEST.toml`: `[[source: xterm-charsets]]`'s
//! macros, their keysyms resolved through `[[source: xorg-keysymdef]]`, and
//! the positions those macros lack that a DEC text draws, each cited on its
//! table.
//!
//! **Do not edit by hand.** Regenerate with `scripts/gen-nrcs-tables.py`;
//! `nrcs_tables_match_the_pinned_source` fails if this file and the pinned
//! source disagree.
//!
//! Each set replaces a handful of ASCII positions -- "the term *replacement*
//! says that the character set is formed by replacing some of the characters
//! with more useful ones for a given language". Everything not listed is ASCII.

use crate::{Codepoint, Nrcs};
''']
lines.append(f"/// DEC's error character, stored as U+{ERROR_CHARACTER:04X} SYMBOL FOR SUBSTITUTE FORM TWO.")
lines.append("///")
lines.append("/// DEC names no codepoint for it: `[[source: dec-std-070]]` §3.4.3 has it")
lines.append('/// "usually imaged as a REVERSE QUESTION MARK", and `[[source: dec-pplv2-pm]]`')
lines.append('/// p. 6-4 calls it "a reverse question mark". U+2426 is ISO 2047\'s picture of')
lines.append("/// SUB, the control DEC ties it to, and its reference glyph is a reversed")
lines.append("/// question mark (`[[source: unicode18-chart-2400]]`). U+FFFD, one of the")
lines.append("/// three other options asked (with U+2E2E and U+2592), departs from that")
lines.append("/// description by name and by glyph (note AF, Q-UNDEF; note AE's AE8.1).")
lines.append("///")
lines.append("/// **It is drawn at a reserved position of a DEC set baud translates**,")
lines.append("/// DEC Supplemental's 13 and JIS-Katakana's 31 (`dec_sets_data.rs`): \"The")
lines.append("/// error character is printed when one of these reserved characters is")
lines.append("/// received by the device\" (`[[source: dec-pplv2-pm]]` §A.2, §A.12). Where")
lines.append("/// else it is drawn waits on open questions: A0 with a 94-set in GR (note AE,")
lines.append("/// AE8), SUB's display (`docs/vt-coverage.md`), and Greek NRC 7/9 and 7/10,")
lines.append("/// which no DEC table charts (note AF, Q-UNDEF).")
lines.append(f"pub const DEC_ERROR_CHARACTER: Codepoint = Codepoint(0x{ERROR_CHARACTER:04x});")
lines.append("")
for name, pairs in sets.items():
    const = WANTED[name].upper()
    overrides = sorted((b, v) for (s, b), v in DEC_OVERRIDES.items() if s == name)
    skips = sorted((b, why) for (s, b), why in SKIP.items() if s == name)
    if overrides:
        lines.append(f"/// `[[source: xterm-charsets]]`'s `map_NRCS_{name}`, and what it lacks,")
        lines.append("/// typed from the DEC text that draws it:")
        for b, (cp, cite) in overrides:
            lines.append("///")
            lines.append(f"/// * `0x{b:02x}`, U+{cp:04X}: {cite[0]}")
            lines.extend(f"///   {c}" for c in cite[1:])
            lines[-1] += "."
    else:
        lines.append(f"/// `[[source: xterm-charsets]]`'s `map_NRCS_{name}`.")
    if skips:
        lines.append("///")
        lines.append("/// Not emitted, each until its question is answered:")
        lines.append("///")
        for b, why in skips:
            lines.append(f"/// * `0x{b:02x}`, `XXX(..., UNDEF)` in the macro: {why} (note AF).")
    lines.append(f"static {const}: [(u8, u32); {len(pairs)}] = [")
    for b, cp in pairs:
        lines.append(f"    (0x{b:02x}, 0x{cp:04x}),")
    lines.append("];")
    lines.append("")

lines.append("/// `ESC ( A` — United Kingdom, whose one substitution xterm spells inline")
lines.append("/// in `charsets.c` rather than as a macro.")
lines.append("static UK: [(u8, u32); 1] = [(0x23, 0x00a3)];")
lines.append("")
lines.append("/// The replacement table for a set: the positions it replaces, by byte.")
lines.append("pub const fn nrcs_table(set: Nrcs) -> &'static [(u8, u32)] {")
lines.append("    match set {")
for name in sets:
    lines.append(f"        Nrcs::{WANTED[name]} => &{WANTED[name].upper()},")
lines.append("    }")
lines.append("}")
lines.append("")
lines.append("/// United Kingdom, which is a `Charset` of its own rather than an `Nrcs`.")
lines.append("pub const fn uk_table() -> &'static [(u8, u32)] {")
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
