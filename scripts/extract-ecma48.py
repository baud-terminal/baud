#!/usr/bin/env python3
"""Extract searchable text from the pinned ECMA-48 PDF.

`docs/vt-coverage.md` said for a long time that this document had "no readable
primary source ... a scanned PDF whose text extracts letter-spaced and
unsearchable", and refused SGR 60-65 on that basis. Only the first half was
true. The text layer is intact; it extracts *letter-spaced*, so it renders as

    Operat i ng sy st em          not     Operating system
    fram ed                       not     framed

and an exact substring search for `framed` cannot match. Six probes missed for
that reason and the miss was read as absence.

The fix is to strip every non-alphanumeric character from both the haystack and
the needle before comparing. That is all this script does, plus the part that
was actually missing the first time: a **positive control**. A MISS only means
absence once a term known to be present HITs. If the control misses, the
extraction is broken and the document is not exonerated.

Usage:
    python3 scripts/extract-ecma48.py                 # run the controls and probes
    python3 scripts/extract-ecma48.py --dump          # write the de-spaced text
    python3 scripts/extract-ecma48.py --find NEEDLE   # search for one term
    python3 scripts/extract-ecma48.py --section sgr   # print SGR § 8.3.117
"""

from __future__ import annotations

import argparse
import re
import sys
import zlib
from pathlib import Path

PDF = Path(__file__).resolve().parent.parent / "docs/research/sources/ecma-48-5th-1991.pdf"

# Terms whose presence is independently known from `[[source: ctlseqs]]` and
# from xterm's own SGR implementation. If any of these misses, the extraction
# is at fault -- not the document.
CONTROL = [
    "selectgraphicrendition",
    "blink",
    "negativeimage",
    "underline",
    "italicized",
    "concealed",
]

# The terms the original search missed. Every one of them is present.
PROBE = [
    "fraktur",
    "gothic",
    "proportional",
    "framed",
    "encircled",
    "overlined",
    "notframed",
    "notoverlined",
    "ideogram",
    "stressmarking",
]


def streams(data: bytes) -> list[bytes]:
    out = []
    for m in re.finditer(rb"stream\r?\n", data):
        start = m.end()
        end = data.find(b"endstream", start)
        if end < 0:
            continue
        try:
            out.append(zlib.decompress(data[start:end]))
        except zlib.error:
            pass  # not every stream is Flate; images and fonts are skipped
    return out


def raw_text(data: bytes) -> str:
    """Every parenthesised text-showing operand, in document order."""
    parts = []
    for chunk in streams(data):
        for m in re.finditer(rb"\((?:\\.|[^\\()])*\)", chunk):
            s = re.sub(rb"\\([()\\])", rb"\1", m.group(0)[1:-1])
            parts.append(s.decode("latin-1"))
    return " ".join(parts)


KEEP = set("abcdefghijklmnopqrstuvwxyz0123456789")


def despace(s: str) -> str:
    """Lowercase and drop everything that is not [a-z0-9].

    `flatten` below must use exactly this predicate. When it did not -- this
    used `[^a-z0-9]` while the offset index used `str.isalnum()`, which also
    accepts the likes of `\xb2` and `\xb5` -- the two ran out of step and every
    window landed in the wrong section. A search that reports the wrong place
    is the same failure as one that reports nothing: it looks like an answer.
    """
    return "".join(ch for ch in s.lower() if ch in KEEP)


def flatten(s: str) -> tuple[str, list[int]]:
    """The de-spaced text, and where each character came from in `s`."""
    kept, index = [], []
    for i, ch in enumerate(s):
        lo = ch.lower()
        if lo in KEEP:
            kept.append(lo)
            index.append(i)
    return "".join(kept), index


def unspace(s: str) -> str:
    """Best-effort re-join of letter-spaced words, for human reading."""
    s = re.sub(r"\s+", " ", s)
    return re.sub(r"(?<=\b\w) (?=\w\b)", "", s)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--dump", action="store_true")
    ap.add_argument("--find")
    ap.add_argument("--section", choices=["sgr"])
    args = ap.parse_args()

    if not PDF.exists():
        print(f"missing {PDF}; run scripts/fetch-sources.sh", file=sys.stderr)
        return 2

    text = raw_text(PDF.read_bytes())
    flat, index = flatten(text)

    if args.dump:
        sys.stdout.write(flat)
        return 0

    def window(needle: str, before: int, after: int) -> str | None:
        at = flat.find(despace(needle))
        if at < 0:
            return None
        a = index[max(0, at - before)]
        b = index[min(len(index) - 1, at + after)]
        return unspace(text[a:b])

    if args.section == "sgr":
        w = window("sgrisusedtoestablish", 40, 4000)
        if w is None:
            print("SGR section not found -- extraction is broken", file=sys.stderr)
            return 1
        print(w)
        return 0

    if args.find:
        n = flat.count(despace(args.find))
        print(f"{'HIT ' if n else 'MISS'} {args.find} x{n}")
        if n:
            print()
            print(window(args.find, 60, 400))
        return 0 if n else 1

    bad = 0
    print("positive control -- these are known present:")
    for t in CONTROL:
        n = flat.count(t)
        print(f"  {'HIT ' if n else 'MISS'} {t:<24} x{n}")
        if not n:
            bad += 1
    if bad:
        print(f"\n{bad} control term(s) missed: the extraction is broken, not the PDF.")
        return 1

    print("\nprobes -- these were once reported absent:")
    for t in PROBE:
        n = flat.count(t)
        print(f"  {'HIT ' if n else 'MISS'} {t:<24} x{n}")
        if not n:
            bad += 1
    print()
    print("controls passed, so a MISS above would be real absence." if not bad
          else f"{bad} probe(s) missed.")
    return 1 if bad else 0


if __name__ == "__main__":
    raise SystemExit(main())
