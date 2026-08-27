#!/usr/bin/env python3
"""Extract xterm's key tables from the pinned `ctlseqs` source.

The output side was enumerated by reading `[[source: ctlseqs]]`'s own lists
back at baud (`tests/conformance/spec_inventory.rs`). The input side needs the
same thing, and the raw material is a set of fixed-width tables:

    Key            Normal     Application
    Cursor Up      CSI A      SS3 A
    ...

Written down rather than remembered, for the reason this repo keeps
rediscovering: a key table transcribed by hand is a guessed table that looks
like it worked. `docs/input-coverage.md` is generated against this.

Usage:
    python3 scripts/extract-keys.py            # every table, with line numbers
    python3 scripts/extract-keys.py --count    # just the totals
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

SRC = Path(__file__).resolve().parent.parent / "docs/research/sources/xterm-ctlseqs.txt"

# A table starts on a header line whose first column is "Key".
HEADER = re.compile(r"^\s+Key\s{2,}(\S.*)$")
# The tables are pipe-delimited, and splitting on whitespace instead gets a
# long key name wrong: "Cursor Right" leaves a single space before the bar and
# lands in the second column. The header has no bars, so it still splits on
# runs of spaces.
ROW = re.compile(r"^\s+([^|]+?)\s*\|(.*)$")
RULE = re.compile(r"^\s*[-+]{4,}")


def tables(lines: list[str]) -> list[tuple[int, str, list[tuple[str, list[str]]]]]:
    out = []
    for i, line in enumerate(lines):
        h = HEADER.match(line)
        if not h:
            continue
        cols = re.split(r"\s{2,}", h.group(1).strip())
        rows: list[tuple[str, list[str]]] = []
        for l in lines[i + 1 : i + 60]:
            if RULE.match(l):
                continue
            if not l.strip():
                if rows:
                    break
                continue
            m = ROW.match(l)
            if not m:
                if rows:
                    break
                continue
            cells = [c.strip() for c in m.group(2).split("|")]
            rows.append((m.group(1).strip(), cells))
        if rows:
            out.append((i + 1, " | ".join(cols), rows))
    return out


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--count", action="store_true")
    args = ap.parse_args()

    if not SRC.exists():
        print(f"missing {SRC}", file=sys.stderr)
        return 2
    lines = SRC.read_text(errors="replace").split("\n")
    ts = tables(lines)

    total = sum(len(r) for _, _, r in ts)
    if args.count:
        print(f"{len(ts)} tables, {total} key rows")
        return 0

    for at, cols, rows in ts:
        print(f"--- line {at}: Key | {cols}  ({len(rows)} rows)")
        for k, v in rows:
            print(f"    {k:<18} {'  '.join(v)}")
        print()
    print(f"{len(ts)} tables, {total} key rows")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
