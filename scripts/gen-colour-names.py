#!/usr/bin/env python3
"""Generate baud-domain's two colour-name tables: kitty's, and the X server's.

kitty 1e1486e looks a colour name up before any other form
(`[[source: kitty-colors-1e1486e]]` 1525-1526), in a table its
gen/color_names.py holds (`[[source: kitty-color-names-1e1486e]]`): 753
lower-case names. That table is X.Org's rgb.txt at 749182c
(`[[source: xorg-rgb-749182c]]`), each name lower-cased, plus one name
X.Org's rgb.txt does not have, `debianred` (215, 7, 81; kitty's line 161):
Debian's addition to its rgb.txt in 2004
(`[[source: debian-bug-243288]]`, `[[source: debian-xorg-rgb-7-7-26]]`
478), a name and a value. kitty is GPL-3.0 and is not in this tree, so
the table is built from rgb.txt and the one name, and `--kitty FILE` checks the result against kitty's own file,
fetched at the pinned commit:

    python3 scripts/gen-colour-names.py                write both tables
    python3 scripts/gen-colour-names.py --check        fail if one is stale
    python3 scripts/gen-colour-names.py --kitty F      fail unless kitty's F
                                                       holds the same names

The lookup is a two-level perfect hash, so a name is found by two hashes and
one comparison, never a scan: FNV-1a over the lower-cased name picks one of
256 buckets, the bucket's seed picks one of 1024 slots, and the slot's name
is compared.

The second table, baud-domain/src/vt/x_colour_names.rs, is the names an
`OSC 4`, `10`, `11` or `12` resolves, as xterm's XParseColor sends a name to
the X server (`[[source: xserver-oscolor-98aed9d]]`): X.Org's rgb.txt at
0d2caec (`[[source: xorg-rgb-0d2caec]]`), 782 names, lower-cased, with the
same two-level hash. The server ignores case and nothing else, and both
tables are keyed the same way.
"""

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SRC = ROOT / "docs/research/sources/xorg-rgb-749182c.txt"
OUT = ROOT / "baud-domain/src/themefile/colour_names.rs"
X_SRC = ROOT / "docs/research/sources/xorg-rgb-0d2caec.txt"
X_OUT = ROOT / "baud-domain/src/vt/x_colour_names.rs"
BUCKETS = 256
SLOTS = 1024
# The one name kitty's table holds that X.Org's rgb.txt at 749182c does
# not: Debian's DebianRed, debian/local/rgb.txt line 478.
EXTRA = {"debianred": (215, 7, 81)}


def fnv(name: bytes, seed: int) -> int:
    h = 0x811C9DC5 ^ seed
    for b in name:
        h = ((h ^ b) * 0x01000193) & 0xFFFFFFFF
    return h


def rgb_txt(path: pathlib.Path) -> dict:
    out = {}
    for line in path.read_text(encoding="ascii").splitlines():
        if not line or line.startswith("!"):
            continue
        r, g, b, name = line.split(None, 3)
        v = (int(r), int(g), int(b))
        # Two spellings of one name differ only in case, and with one
        # value: a lower-cased key loses nothing.
        assert out.setdefault(name.strip().lower(), v) == v, name
    return out


def names() -> dict:
    out = rgb_txt(SRC)
    for k, v in EXTRA.items():
        assert k not in out, k
        out[k] = v
    return out


def build(table: dict):
    buckets = [[] for _ in range(BUCKETS)]
    for name in table:
        buckets[fnv(name.encode(), 0) % BUCKETS].append(name)
    seeds = [0] * BUCKETS
    slots = [None] * SLOTS
    for i in sorted(range(BUCKETS), key=lambda i: -len(buckets[i])):
        if not buckets[i]:
            continue
        for seed in range(1, 1 << 16):
            at = [fnv(n.encode(), seed) % SLOTS for n in buckets[i]]
            if len(set(at)) == len(at) and all(slots[a] is None for a in at):
                for a, n in zip(at, buckets[i]):
                    slots[a] = n
                seeds[i] = seed
                break
        else:
            sys.exit(f"no seed for bucket {i}")
    return seeds, slots


KITTY_HEAD = [
    "//! kitty's colour names, generated from the source pinned in",
    "//! `docs/research/sources/MANIFEST.toml`.",
    "//!",
    "//! **Do not edit by hand.** Regenerate with `scripts/gen-colour-names.py`;",
    "//! `kitty_names_are_rgb_txt_and_debianred` fails if a name of the pinned",
    "//! source is missing or has the wrong colour, and",
    "//! `colour_name_tables_match_the_generator` fails if this file is not",
    "//! what the script writes, an extra entry included.",
    "//!",
    "//! {n} names: X.Org's rgb.txt at 749182c",
    "//! (`[[source: xorg-rgb-749182c]]`), lower-cased, and `debianred`, Debian's",
    "//! addition to its rgb.txt (`[[source: debian-xorg-rgb-7-7-26]]` 478), which",
    "//! kitty's table carries (`[[source: kitty-color-names-1e1486e]]` 161): a",
    "//! name and a value. A name is looked up by",
    "//! [`SEEDS`] then [`SLOTS`]: two hashes and one comparison.",
]

X_HEAD = [
    "//! The X server's colour names, generated from the source pinned in",
    "//! `docs/research/sources/MANIFEST.toml`.",
    "//!",
    "//! **Do not edit by hand.** Regenerate with `scripts/gen-colour-names.py`;",
    "//! `every_name_in_rgb_txt_resolves_in_any_case` fails if a name of the",
    "//! pinned source is missing or has the wrong colour, and",
    "//! `colour_name_tables_match_the_generator` fails if this file is not",
    "//! what the script writes, an extra entry included.",
    "//!",
    "//! {n} names: X.Org's rgb.txt at 0d2caec",
    "//! (`[[source: xorg-rgb-0d2caec]]`), lower-cased, the table the X.Org",
    "//! server answers XParseColor's LookupColor from",
    "//! (`[[source: xserver-oscolor-98aed9d]]`). A name is looked up by",
    "//! [`SEEDS`] then [`SLOTS`]: two hashes and one comparison.",
]


def render(table: dict, head: list) -> str:
    seeds, slots = build(table)
    pool = b""
    rows = []
    for n in slots:
        if n is None:
            rows.append("    (0, 0, [0, 0, 0]),")
            continue
        at = len(pool)
        pool += n.encode()
        r, g, b = table[n]
        rows.append(f"    ({at}, {len(n)}, [{r}, {g}, {b}]), // {n}")
    out = [
        *(line.format(n=len(table)) for line in head),
        "",
        "/// The seed of each of the 256 buckets the first hash picks.",
        "#[rustfmt::skip]",
        f"pub(super) static SEEDS: [u16; {BUCKETS}] = [",
    ]
    for i in range(0, BUCKETS, 16):
        out.append("    " + ", ".join(str(s) for s in seeds[i : i + 16]) + ",")
    out += [
        "];",
        "",
        "/// Each slot's name, as an offset and a length in [`POOL`], and its",
        "/// colour; a length of 0 is an empty slot.",
        "#[rustfmt::skip]",
        f"pub(super) static SLOTS: [(u16, u8, [u8; 3]); {SLOTS}] = [",
        *rows,
        "];",
        "",
        "/// Every name, end to end.",
        "#[rustfmt::skip]",
        "pub(super) static POOL: &[u8] = concat!(",
    ]
    text = pool.decode()
    for i in range(0, len(text), 64):
        out.append(f'    "{text[i : i + 64]}",')
    out.append(")")
    out.append(".as_bytes();")
    return "\n".join(out) + "\n"


def kitty_names(path: str) -> dict:
    src = pathlib.Path(path).read_text(encoding="utf-8")
    pat = r"'([^']+)': Color\((\d+), (\d+), (\d+)\)"
    return {m[1]: (int(m[2]), int(m[3]), int(m[4])) for m in re.finditer(pat, src)}


def main() -> None:
    table = names()
    if "--kitty" in sys.argv:
        theirs = kitty_names(sys.argv[sys.argv.index("--kitty") + 1])
        if theirs != table:
            only = sorted(set(theirs) ^ set(table))[:10]
            diff = [n for n in table if n in theirs and theirs[n] != table[n]][:10]
            sys.exit(f"kitty's table differs: names {only}, values {diff}")
        print(f"OK: kitty's {len(theirs)} names are rgb.txt's and debianred")
        return
    x = rgb_txt(X_SRC)
    pairs = [(OUT, render(table, KITTY_HEAD), table), (X_OUT, render(x, X_HEAD), x)]
    if "--check" in sys.argv:
        stale = [str(path.relative_to(ROOT)) for path, out, _ in pairs if path.read_text() != out]
        if stale:
            print("STALE", *stale)
            sys.exit(1)
        print("OK")
    else:
        for path, out, t in pairs:
            path.write_text(out)
            print(f"{path.relative_to(ROOT)}: {len(t)} names")


if __name__ == "__main__":
    main()
