#!/usr/bin/env python3
"""Generate baud-domain/src/width_data.rs from pinned Unicode data.

The tables are *generated and committed*, not read at runtime: baud-domain is
`no_std` with no allocator and no build script, and a hand-written range table
is how width data silently goes stale. `width_tables_match_the_pinned_data`
regenerates and compares, so the committed file cannot drift from the source
recorded in MANIFEST.toml.
"""
import re, sys, pathlib

SRC = pathlib.Path(__file__).resolve().parent.parent / "docs/research/sources"
EAW = SRC / "unicode-eastasianwidth.txt"
DGC = SRC / "unicode-derivedgeneralcategory.txt"


def parse(path):
    """(lo, hi, value) triples from a UCD-style file."""
    out = []
    for line in path.read_text(errors="ignore").splitlines():
        line = line.split("#", 1)[0].strip()
        if not line:
            continue
        field, _, value = line.partition(";")
        value = value.strip()
        field = field.strip()
        if ".." in field:
            lo, hi = field.split("..")
        else:
            lo = hi = field
        out.append((int(lo, 16), int(hi, 16), value))
    return out


def coalesce(ranges):
    """Sort and merge adjacent/overlapping ranges."""
    ranges = sorted(ranges)
    merged = []
    for lo, hi in ranges:
        if merged and lo <= merged[-1][1] + 1:
            merged[-1][1] = max(merged[-1][1], hi)
        else:
            merged.append([lo, hi])
    return [(a, b) for a, b in merged]


def subtract(ranges, drop):
    out = []
    for lo, hi in ranges:
        if drop < lo or drop > hi:
            out.append((lo, hi))
            continue
        if lo <= drop - 1:
            out.append((lo, drop - 1))
        if drop + 1 <= hi:
            out.append((drop + 1, hi))
    return out


EMOJI = SRC / "unicode-emoji-data.txt"

eaw = parse(EAW)
dgc = parse(DGC)
emoji = parse(EMOJI)

# East Asian Width W/F, plus everything with Emoji_Presentation=Yes.
#
# Both are needed and neither subsumes the other. Regional indicators are EAW
# `N` -- one column -- yet `Emoji_Presentation`, so a flag measured from EAW
# alone is half as wide as every terminal draws it.
wide = coalesce(
    [(lo, hi) for lo, hi, v in eaw if v in ("W", "F")]
    + [(lo, hi) for lo, hi, v in emoji if v == "Emoji_Presentation"]
)
ambiguous = coalesce([(lo, hi) for lo, hi, v in eaw if v == "A"])
# Mn (nonspacing mark), Me (enclosing mark), Cf (format) are zero-width.
zero = coalesce([(lo, hi) for lo, hi, v in dgc if v in ("Mn", "Me", "Cf")])
# U+00AD SOFT HYPHEN is Cf but every terminal renders it in a column.
zero = subtract(zero, 0x00AD)


def emit(name, ranges, doc):
    # `static`, not `const`: a const array is materialised at each use site.
    lines = [doc, f"pub static {name}: [(u32, u32); {len(ranges)}] = ["]
    for lo, hi in ranges:
        lines.append(f"    (0x{lo:04x}, 0x{hi:04x}),")
    lines.append("];")
    return "\n".join(lines)


header = '''//! Unicode width data, generated from the sources pinned in
//! `docs/research/sources/MANIFEST.toml`.
//!
//! **Do not edit by hand.** Regenerate with `scripts/gen-width-tables.py`;
//! `width_tables_match_the_pinned_data` fails if this file and the pinned data
//! disagree, so a stale table is a test failure rather than a rendering bug
//! nobody notices.
//!
//! Sorted, coalesced, half-open-free `(lo, hi)` inclusive ranges, searched by
//! binary search. Consulted once per **non-ASCII codepoint**, never per byte:
//! printable ASCII never reaches them, which is the ~94% path.
'''

# rstrip: the header ends with a newline and the join adds two more, which
# rustfmt collapses -- and a generator whose output rustfmt rewrites can never
# satisfy its own staleness check.
parts = [
    header.rstrip("\n"),
    emit(
        "ZERO",
        zero,
        "/// General category `Mn`, `Me` or `Cf` — combining marks and format\n"
        "/// characters, which occupy no column. `U+00AD` SOFT HYPHEN is `Cf` but is\n"
        "/// excluded: every terminal gives it a column.",
    ),
    emit(
        "WIDE",
        wide,
        "/// Two columns: East Asian Width `W` or `F`, **or**\n"
        "/// `Emoji_Presentation=Yes`. Regional indicators are EAW `N` and would\n"
        "/// otherwise measure one column, making every flag half-width.",
    ),
    emit(
        "AMBIGUOUS",
        ambiguous,
        "/// East Asian Width `A`. One column or two depending on locale, which is\n"
        "/// what [`AmbiguousWidth`](crate::AmbiguousWidth) selects. `[[source: uax11]]`\n"
        "/// § 5: narrow when the context is unavailable, and for a terminal core with\n"
        "/// no locale it is always unavailable — so narrow is the default.",
    ),
]
out = "\n\n".join(parts) + "\n"

if "--check" in sys.argv:
    cur = pathlib.Path("baud-domain/src/width_data.rs").read_text()
    if cur != out:
        print("STALE")
        sys.exit(1)
    print("OK")
else:
    pathlib.Path("baud-domain/src/width_data.rs").write_text(out)
    print(f"zero={len(zero)} wide={len(wide)} ambiguous={len(ambiguous)} bytes={len(out)}")
