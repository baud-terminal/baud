#!/usr/bin/env python3
"""Generate the cursive-joining tables from pinned Unicode data.

Same contract as gen-bidi-tables.py: generated and committed, because
neither crate has a build script, and `joining_tables_match_the_pinned_data`
regenerates and compares so the committed files cannot drift. Two outputs:

* **baud-domain/src/joining_data.rs**, one byte a code point as a two-stage
  trie indexed by `codepoint >> 8` and then the low byte: Joining_Type in the
  low three bits (`JoiningType`'s declaration order: U, T, C, D, R, L) and,
  in bits 3-4, the two Joining_Group values the Unicode Standard's
  obligatory ligature rules name, LAM (1) and ALEF (2) (chapter 9.2.3,
  `[[source: unicode-ch9]]`). Both come from ArabicShaping.txt
  (`[[source: unicode-arabicshaping]]`); a code point it does not list is T
  when its General Category is Mn, Me or Cf and U otherwise, as its header
  says (`[[source: unicode-derivedgeneralcategory]]`). The result is held to
  DerivedJoiningType.txt (`[[source: unicode-derivedjoiningtype]]`) at
  every code point.
* **baud-font/src/presentation.rs**, the Arabic presentation forms a face
  with no `GSUB` forms is drawn through: each character whose
  UnicodeData.txt decomposition is `<isolated>`, `<final>`, `<initial>` or
  `<medial>` of one code point, keyed by that code point and the form, and
  the eight lam-alef ligatures U+FEF5..U+FEFC, `<isolated>` or `<final>` of
  lam and an alef (`[[source: unicode-data]]`).
"""
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SRC = ROOT / "docs/research/sources"
OUT = ROOT / "baud-domain/src/joining_data.rs"
FORMS_OUT = ROOT / "baud-font/src/presentation.rs"

TYPES = ["U", "T", "C", "D", "R", "L"]
LONG = {"Non_Joining": "U", "Transparent": "T", "Join_Causing": "C",
        "Dual_Joining": "D", "Right_Joining": "R", "Left_Joining": "L"}
GROUPS = {"LAM": 1, "ALEF": 2}
FORMS = ["isolated", "final", "initial", "medial"]
BLOCK = 256


def data_lines(path):
    for line in (SRC / path).read_text(encoding="utf-8").splitlines():
        line = line.split("#", 1)[0].strip()
        if line:
            yield [p.strip() for p in line.split(";")]


def span(field):
    lo, _, hi = field.partition("..")
    return int(lo, 16), int(hi or lo, 16)


# The header's default: T for an unlisted Mn, Me or Cf, U for the rest.
jt = ["U"] * 0x110000
for p in data_lines("unicode-derivedgeneralcategory.txt"):
    if p[1] in ("Mn", "Me", "Cf"):
        lo, hi = span(p[0])
        for cp in range(lo, hi + 1):
            jt[cp] = "T"
group = [0] * 0x110000
listed = 0
for p in data_lines("unicode-arabicshaping.txt"):
    cp = int(p[0], 16)
    jt[cp] = p[2]
    group[cp] = GROUPS.get(p[3], 0)
    listed += 1
assert listed > 600, listed
# Positive controls: beh dual, alef right, hamza and ZWNJ non-joining,
# tatweel and ZWJ join-causing, fatha transparent (Mn, unlisted), ALM
# transparent (Cf, unlisted), the isolates non-joining as UAX #9 3.5 says,
# a Latin letter non-joining, lam in LAM and alef with madda in ALEF.
assert jt[0x0628] == "D" and jt[0x0627] == "R" and jt[0x0621] == "U"
assert jt[0x200C] == "U" and jt[0x0640] == "C" and jt[0x200D] == "C"
assert jt[0x064E] == "T" and jt[0x061C] == "T" and jt[0x0041] == "U"
assert all(jt[c] == "U" for c in range(0x2066, 0x206A))
assert group[0x0644] == 1 and group[0x0622] == 2 and group[0x0628] == 0

# Held to the derived file, its `@missing` line included.
derived = ["U"] * 0x110000
missing = 0
for line in (SRC / "unicode-derivedjoiningtype.txt").read_text(encoding="utf-8").splitlines():
    m = re.match(r"# @missing: ([0-9A-F]+)\.\.([0-9A-F]+); (\w+)$", line)
    if m:
        assert (m[1], m[2], m[3]) == ("0000", "10FFFF", "Non_Joining"), line
        missing += 1
assert missing == 1
for p in data_lines("unicode-derivedjoiningtype.txt"):
    lo, hi = span(p[0])
    for cp in range(lo, hi + 1):
        derived[cp] = p[1] if p[1] in TYPES else LONG[p[1]]
diff = [cp for cp in range(0x110000) if jt[cp] != derived[cp]]
assert not diff, [hex(c) for c in diff[:10]]

values = bytearray(0x110000)
for cp in range(0x110000):
    values[cp] = TYPES.index(jt[cp]) | (group[cp] << 3)


def trie(values):
    blocks, index = {}, []
    for base in range(0, 0x110000, BLOCK):
        key = bytes(values[base:base + BLOCK])
        if key not in blocks:
            blocks[key] = len(blocks)
        index.append(blocks[key])
    assert len(blocks) <= 256, f"{len(blocks)} blocks will not fit a u8 stage 1"
    flat = b"".join(blocks)
    for cp in range(0x110000):
        assert flat[index[cp >> 8] * BLOCK + (cp & 0xFF)] == values[cp], hex(cp)
    return bytes(index), flat, len(blocks)


index, flat, nblocks = trie(values)


def as_bytestring(name, data, doc):
    """A byte string, which rustfmt leaves as written (gen-grapheme-tables.py)."""
    body = []
    for i in range(0, len(data), 32):
        chunk = "".join(f"\\x{v:02x}" for v in data[i:i + 32])
        body.append(f"    {chunk}\\")
    return "\n".join([doc, f'pub static {name}: &[u8; {len(data)}] = b"\\', *body, '";'])


parts = ["""//! Cursive-joining data, generated from the sources pinned in
//! `docs/research/sources/MANIFEST.toml`.
//!
//! **Do not edit by hand.** Regenerate with `scripts/gen-joining-tables.py`.
//! `joining_tables_match_the_pinned_data` fails if this file and the pinned
//! data disagree."""]
parts.append(as_bytestring("JOINING_INDEX", index, "\n".join([
    "/// Joining_Type and the LAM and ALEF joining groups, as a two-stage trie.",
    f"/// Stage 1: codepoint >> 8 to one of {nblocks} distinct blocks.",
    "///",
    "/// **Low three bits** are `JoiningType` discriminants in declaration",
    "/// order (`[[source: unicode-arabicshaping]]`, its header's T and U",
    "/// defaults by General Category); bits 3-4 are 1 for the LAM joining",
    "/// group and 2 for ALEF, the two chapter 9.2.3's ligature rules name.",
])))
parts.append(as_bytestring("JOINING_BLOCKS", flat, "/// Stage 2: one byte a code point."))
out = "\n\n".join(parts) + "\n"

# The presentation forms.
single, lam_alef = {}, {}
for line in (SRC / "unicode-unicodedata.txt").read_text(encoding="utf-8").splitlines():
    f = line.split(";")
    m = re.match(r"<(isolated|final|initial|medial)> ([0-9A-F ]+)$", f[5])
    if not m:
        continue
    cp, form, of = int(f[0], 16), FORMS.index(m[1]), [int(x, 16) for x in m[2].split()]
    if len(of) == 1:
        assert (of[0], form) not in single, hex(cp)
        single[(of[0], form)] = cp
    elif len(of) == 2 and of[0] == 0x0644 and group[of[1]] == 2:
        assert form in (0, 1), hex(cp)
        lam_alef.setdefault(of[1], [0, 0])[form] = cp
# Controls: beh's four forms are U+FE8F..U+FE92, peh's (Presentation
# Forms-A) U+FB56..U+FB59, and lam with alef U+FEFB and U+FEFC.
assert [single[(0x0628, k)] for k in range(4)] == [0xFE8F, 0xFE90, 0xFE91, 0xFE92]
assert [single[(0x067E, k)] for k in range(4)] == [0xFB56, 0xFB57, 0xFB58, 0xFB59]
assert lam_alef[0x0627] == [0xFEFB, 0xFEFC] and len(lam_alef) == 4
keys = sorted((base << 2) | form for base, form in single)
forms_rs = "\n".join([
    "//! The Arabic presentation forms a face with no `GSUB` forms is drawn",
    "//! through, generated from the sources pinned in",
    "//! `docs/research/sources/MANIFEST.toml`.",
    "//!",
    "//! **Do not edit by hand.** Regenerate with `scripts/gen-joining-tables.py`.",
    "//! `joining_tables_match_the_pinned_data` fails if this file and the pinned",
    "//! data disagree.",
    "",
    "/// Each presentation form whose UnicodeData.txt decomposition is",
    "/// `<isolated>`, `<final>`, `<initial>` or `<medial>` of one code point",
    "/// (`[[source: unicode-data]]`), sorted by key: the code point shifted",
    "/// left two, with the form below it (0 isolated, 1 final, 2 initial,",
    "/// 3 medial), and the presentation form.",
    "#[rustfmt::skip]",
    f"pub static FORMS: [(u32, u32); {len(keys)}] = [",
    *["    " + ", ".join(f"(0x{k:06X}, 0x{single[(k >> 2, k & 3)]:04X})" for k in keys[i:i + 4]) + ","
      for i in range(0, len(keys), 4)],
    "];",
    "",
    "/// The lam-alef ligatures U+FEF5..U+FEFC: each alef lam forms one with,",
    "/// and the ligature's `<isolated>` and `<final>` forms.",
    "#[rustfmt::skip]",
    f"pub static LAM_ALEF: [(u32, u32, u32); {len(lam_alef)}] = [",
    *[f"    (0x{a:04X}, 0x{v[0]:04X}, 0x{v[1]:04X})," for a, v in sorted(lam_alef.items())],
    "];",
]) + "\n"

if "--check" in sys.argv:
    stale = [p.name for p, text in ((OUT, out), (FORMS_OUT, forms_rs)) if p.read_text() != text]
    if stale:
        print("STALE", *stale)
        sys.exit(1)
    print("OK")
else:
    OUT.write_text(out)
    FORMS_OUT.write_text(forms_rs)
    print(f"joining blocks={nblocks} bytes={len(index) + len(flat)} forms={len(keys)} lam-alef={len(lam_alef)}")
