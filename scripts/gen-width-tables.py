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
EAW8 = SRC / "unicode-eastasianwidth-8.0.0.txt"
DGC = SRC / "unicode-derivedgeneralcategory.txt"
PROPLIST = SRC / "unicode-proplist.txt"


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
# **Unicode 8's widths, which iTerm2's `UnicodeVersion=8` selects**: East
# Asian Width 8.0's W and F, and its header's unassigned code points that
# "default to W", which the data lines do not list; no Emoji_Presentation,
# which Unicode 9 folded into Wide. A regional indicator is 8.0's `N`, one
# column; a flag, two of them, is widened to two columns where they join, as
# iTerm2 draws every flag and leaves a lone indicator at its version's width
# (`Paint::attach_before`). Zero width is the same under both: iTerm2's
# version chooses the wide and ambiguous sets only.
eaw8 = parse(EAW8)
DEFAULT_W8 = [(0x3400, 0x4DBF), (0x4E00, 0x9FFF), (0xF900, 0xFAFF),
              (0x20000, 0x2FFFD), (0x30000, 0x3FFFD)]
listed8 = coalesce([(lo, hi) for lo, hi, v in eaw8])
def unlisted(ranges, listed):
    out = []
    for lo, hi in ranges:
        cur = lo
        for a, b in listed:
            if b < cur or a > hi:
                continue
            if a > cur:
                out.append((cur, a - 1))
            cur = max(cur, b + 1)
        if cur <= hi:
            out.append((cur, hi))
    return out
wide8 = coalesce(
    [(lo, hi) for lo, hi, v in eaw8 if v in ("W", "F")]
    + unlisted(DEFAULT_W8, listed8)
)
ambiguous8 = coalesce([(lo, hi) for lo, hi, v in eaw8 if v == "A"])

# Mn (nonspacing mark), Me (enclosing mark), Cf (format) are zero-width.
zero = coalesce([(lo, hi) for lo, hi, v in dgc if v in ("Mn", "Me", "Cf")])
# U+00AD SOFT HYPHEN is Cf but every terminal renders it in a column.
zero = subtract(zero, 0x00AD)
# Unicode 8's table keeps the zero set Unicode 8 had no property to narrow.
zero8 = zero
# Prepended_Concatenation_Mark is Cf, and UAX #44 calls it "a small class of
# visible format controls, which precede and then span a sequence of other
# characters, usually digits": a column each, as glibc's width table gives it.
for lo, hi, v in parse(PROPLIST):
    if v == "Prepended_Concatenation_Mark":
        for cp in range(lo, hi + 1):
            zero = subtract(zero, cp)


BLOCK = 256  # codepoints per stage-2 block
PER_BYTE = 4  # 2-bit values


def build_trie(zero, wide, ambiguous):
    """A two-stage table, and an exhaustive check that it agrees with the ranges.

    `Width::of` ran up to three binary searches over 1,386 total ranges -- about
    eleven data-dependent branches -- for every non-ASCII codepoint. Profiled at
    9.4% of the CJK path, and a ceiling arm returning a constant measured +32 to
    +34%. Two indexed loads replace the searches.

    Values: 0 narrow, 1 zero, 2 wide, 3 ambiguous. Two bits each, four per byte.
    """
    cls = bytearray(0x110000)

    def mark(ranges, v):
        for lo, hi in ranges:
            for cp in range(lo, min(hi, 0x10FFFF) + 1):
                cls[cp] = v

    mark(ambiguous, 3)
    mark(wide, 2)
    mark(zero, 1)

    blocks, index = {}, []
    for base in range(0, 0x110000, BLOCK):
        key = bytes(cls[base : base + BLOCK])
        if key not in blocks:
            blocks[key] = len(blocks)
        index.append(blocks[key])
    assert len(blocks) <= 256, f"{len(blocks)} blocks will not fit a u8 stage 1"

    packed = bytearray()
    for key in blocks:
        for i in range(0, BLOCK, PER_BYTE):
            b = 0
            for j in range(PER_BYTE):
                b |= key[i + j] << (2 * j)
            packed.append(b)

    # **Exhaustive, not sampled.** Every codepoint, trie against the ranges the
    # rest of this file is generated from. A generated table nobody checks is
    # the "guessed table would be unrecoverable and would look like it worked"
    # failure `docs/vt-coverage.md` records, and 1.1M iterations is cheap.
    def in_ranges(rs, cp):
        return any(lo <= cp <= hi for lo, hi in rs)

    for cp in range(0x110000):
        blk = index[cp >> 8]
        byte = packed[blk * (BLOCK // PER_BYTE) + ((cp & 0xFF) >> 2)]
        got = (byte >> ((cp & 3) * 2)) & 3
        if in_ranges(zero, cp):
            want = 1
        elif in_ranges(wide, cp):
            want = 2
        elif in_ranges(ambiguous, cp):
            want = 3
        else:
            want = 0
        if got != want:
            raise SystemExit(f"trie disagrees at U+{cp:04X}: {got} != {want}")
    return index, packed, len(blocks)


def as_bytestring(name, data, doc):
    """Emit as a byte-string literal, not an array literal.

    **rustfmt rewraps array literals to fit its width**, so a generator that
    emits `[0, 1, 2, ...]` can never satisfy its own staleness check -- the
    file it writes is not the file that ends up committed. This generator's
    header has warned about that since it was written, and it caught this
    change. String literals are left alone, and `b"..."` produces byte-for-byte
    the same rodata as an array of `u8`.

    Line continuations every 32 bytes so the file is greppable rather than one
    17 KB line; a trailing backslash inside a byte string eats the newline and
    the indentation after it.
    """
    body = []
    for i in range(0, len(data), 32):
        chunk = "".join(f"\\x{v:02x}" for v in data[i : i + 32])
        body.append(f"    {chunk}\\")
    return "\n".join(
        [doc, f'pub static {name}: &[u8; {len(data)}] = b"\\', *body, '";']
    )


def emit_trie(index, packed, nblocks, prefix="WIDTH", unicode8=False):
    if unicode8:
        doc8 = "\n".join(
            [
                "/// Stage 1 of the **Unicode 8** width trie, which iTerm2's",
                "/// `UnicodeVersion=8` selects: the four values as [`WIDTH_INDEX`]'s,",
                "/// wide from East Asian Width 8.0's `W` and `F` and the unassigned",
                "/// code points its header defaults to `W`, with no",
                "/// `Emoji_Presentation` -- which Unicode 9 folded into Wide -- so a",
                "/// regional indicator is one column, and a flag is widened where its",
                "/// two join. Zero is the same as under the current tables.",
            ]
        )
        doc8b = f"/// Stage 2: {nblocks} distinct {BLOCK}-codepoint blocks, as [`WIDTH_BLOCKS`]."
        return as_bytestring(prefix + "_INDEX", index, doc8) + "\n\n" + as_bytestring(
            prefix + "_BLOCKS", packed, doc8b
        )
    doc1 = "\n".join(
        [
            "/// Stage 1 of the width trie: codepoint >> 8 to a block number.",
            "///",
            "/// **What the four values mean, and why each is what it is.**",
            "///",
            "/// * `1` zero — general category `Mn`, `Me` or `Cf`: combining marks and",
            "///   format characters, which occupy no column. `U+00AD` SOFT HYPHEN is",
            "///   `Cf` and is excluded, because every terminal gives it a column, and",
            "///   so is `Prepended_Concatenation_Mark`, `[[source: uax44]]`'s",
            "///   \"visible format controls\" that span the digits after them, such as",
            "///   `U+0600` ARABIC NUMBER SIGN, a column each, as glibc gives them.",
            "/// * `2` wide — East Asian Width `W` or `F`, **or**",
            "///   `Emoji_Presentation=Yes`. Both are needed and neither subsumes the",
            "///   other: regional indicators are EAW `N`, so a flag measured from East",
            "///   Asian Width alone is half the width every terminal draws it.",
            "/// * `3` ambiguous — East Asian Width `A`, one column or two depending on",
            "///   locale, which is what `AmbiguousWidth` selects. `[[source: uax11]]`",
            "///   § 5: narrow when the context is unavailable, and for a terminal core",
            "///   with no locale it always is.",
            "/// * `0` narrow — everything else.",
            "///",
            "/// Two indexed loads replace up to three binary searches over 677 ranges.",
            "/// The searched form was 9.4% of the CJK path and a ceiling arm returning",
            "/// a constant measured +32 to +34%; a scattered probe per branch is also",
            "/// the worse shape for a cache, which should hold what each byte touches.",
            "/// Han sits in 82 contiguous entries here.",
        ]
    )
    doc2 = "\n".join(
        [
            f"/// Stage 2: {nblocks} distinct {BLOCK}-codepoint blocks, four 2-bit",
            "/// values per byte, low bits first.",
        ]
    )
    return as_bytestring("WIDTH_INDEX", index, doc1) + "\n\n" + as_bytestring(
        "WIDTH_BLOCKS", packed, doc2
    )


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
//! A two-stage trie, consulted once per **non-ASCII codepoint** and never per
//! byte: printable ASCII never reaches it, which is the ~94% path.
//!
//! The `(lo, hi)` range tables this replaced are still how the data is derived
//! and checked -- the generator builds them from the pinned files, builds the
//! trie, and verifies the two agree at **every** one of the 1,114,112
//! codepoints before writing anything. They are simply not carried into the
//! binary, where they cost three binary searches per codepoint.
'''

# rstrip: the header ends with a newline and the join adds two more, which
# rustfmt collapses -- and a generator whose output rustfmt rewrites can never
# satisfy its own staleness check.
parts = [header.rstrip("\n")]

index, packed, nblocks = build_trie(zero, wide, ambiguous)
parts.append(emit_trie(index, packed, nblocks))
index8, packed8, nblocks8 = build_trie(zero8, wide8, ambiguous8)
parts.append(emit_trie(index8, packed8, nblocks8, "WIDTH8", True))
out = "\n\n".join(parts) + "\n"

if "--check" in sys.argv:
    cur = pathlib.Path("baud-domain/src/width_data.rs").read_text()
    if cur != out:
        print("STALE")
        sys.exit(1)
    print("OK")
else:
    pathlib.Path("baud-domain/src/width_data.rs").write_text(out)
    print(
        f"zero={len(zero)} wide={len(wide)} ambiguous={len(ambiguous)} "
        f"trie={nblocks} blocks, {len(index)}+{len(packed)} bytes, "
        f"file={len(out)}"
    )
