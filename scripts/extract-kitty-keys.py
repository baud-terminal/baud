#!/usr/bin/env python3
"""Write baud's two kitty key tables from the pinned kitty-keyboard source.

`[[source: kitty-keyboard]]`'s "Functional key definitions" table maps a key
name to the number and final byte its `CSI` form uses -- ESCAPE to `27 u`,
HOME to `1 H or 7 ~`, CAPS_LOCK to `57358 u`, and so on across 111 rows -- and
its "C0 controls" table gives the legacy bytes of Enter, Escape, Backspace,
Tab and Space under seven modifier columns. baud's encoders need both, and a
key table written from memory is a guessed table that looks like it worked --
the failure this repo refused for DEC line drawing and enumerated the base
input layer against.

So both are read from the source, and kept as baud's own data:

    baud-core/tests/kitty_functional_keys.tsv   name<TAB>number<TAB>final
    baud-core/tests/kitty_legacy_c0.tsv         key<TAB>column<TAB>cell

`kitty_encode.rs` and `key_encode.rs` read those files; no test fetches.

The source is the page's reStructuredText at kitty v0.49.2,
`[[source: kitty-keyboard-rst]]`, fetched by its URL and refused unless it
hashes to the SHA-256 the manifest records. Its two `csv-table`s gave the
same 111 rows and 35 cells as the rendered page this script read before
(`[[source: kitty-keyboard]]`, whose URL no longer serves the pinned bytes).
The canonical CSI form baud sends is the FIRST alternative of a cell such as
`1 H or 7 ~` -- the second is the legacy `~` form kitty also accepts, and
sending one spelling is enough.

Usage:
    python3 scripts/extract-kitty-keys.py            # fetch, verify, write
    python3 scripts/extract-kitty-keys.py --check    # fetch, verify, compare
    ... --source FILE                                # a local copy, still hashed
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import re
import sys
import tomllib
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
with open(ROOT / "docs/research/sources/MANIFEST.toml", "rb") as _f:
    _PIN = tomllib.load(_f)["sources"]["kitty-keyboard-rst"]
URL, SHA256 = _PIN["url"], _PIN["sha256"]
FUNCTIONAL = ROOT / "baud-core/tests/kitty_functional_keys.tsv"
LEGACY = ROOT / "baud-core/tests/kitty_legacy_c0.tsv"

FORM = re.compile(r"^(\d+)\s+([A-Za-z~])(?:\s+or\s+.*)?$")
NAME = re.compile(r"^[A-Z][A-Z0-9_]*$")
COLUMNS = ["No mods", "Ctrl", "Alt", "Shift", "Ctrl + Shift", "Alt + Shift", "Ctrl + Alt"]
C0_KEYS = ["Enter", "Escape", "Backspace", "Tab", "Space"]


def source(path: str | None) -> str:
    if path:
        raw = Path(path).read_bytes()
    else:
        with urllib.request.urlopen(URL, timeout=60) as r:
            raw = r.read()
    got = hashlib.sha256(raw).hexdigest()
    if got != SHA256:
        sys.exit(f"FAIL: the source hashes {got}, the pin is {SHA256}")
    return raw.decode("utf-8")


def csv_table(text: str, title: str) -> tuple[list[str], list[list[str]]]:
    """The header and rows of the `.. csv-table:: <title>` directive."""
    lines = text.splitlines()
    try:
        at = lines.index(f".. csv-table:: {title}")
    except ValueError:
        sys.exit(f"FAIL: no csv-table '{title}' -- the source moved")
    header: list[str] = []
    body: list[str] = []
    for ln in lines[at + 1:]:
        if ln.strip().startswith(":header:"):
            header = next(csv.reader([ln.split(":header:", 1)[1]], skipinitialspace=True))
            continue
        if not ln.strip():
            if body:
                break
            continue
        if not ln.startswith(" "):
            break
        body.append(ln.strip())
    rows = [[c.strip() for c in r] for r in csv.reader(body, skipinitialspace=True)]
    return [h.strip() for h in header], rows


def functional(text: str) -> list[tuple[str, int, str]]:
    header, rows = csv_table(text, "Functional key codes")
    if header != ["Name", "CSI", "Name", "CSI"]:
        sys.exit(f"FAIL: functional header {header}")
    out = []
    for r in rows:
        for name, form in zip(r[0::2], r[1::2]):
            m = FORM.match(form.strip("`"))
            if not (NAME.match(name) and m):
                sys.exit(f"FAIL: unreadable row {name!r} {form!r}")
            out.append((name, int(m.group(1)), m.group(2)))
    return out


def legacy(text: str) -> list[tuple[str, str, str]]:
    header, rows = csv_table(text, "C0 controls")
    if header != ["Key", *COLUMNS]:
        sys.exit(f"FAIL: C0 header {header}")
    if [r[0] for r in rows] != C0_KEYS:
        sys.exit(f"FAIL: C0 rows {[r[0] for r in rows]}")
    return [(r[0], col, cell) for r in rows for col, cell in zip(COLUMNS, r[1:], strict=True)]


def render(text: str) -> tuple[str, str]:
    keys = functional(text)
    cells = legacy(text)
    # Positive controls: ESCAPE=27, the PUA runs from CAPS_LOCK=57358, the
    # last row is ISO_LEVEL5_SHIFT, and Alt-Backspace is ESC DEL. If one is
    # gone the extraction is broken and the tables are meaningless -- the
    # "run a positive control before believing a MISS" rule.
    byname = {n: (num, f) for n, num, f in keys}
    for want_name, want in (("ESCAPE", (27, "u")), ("CAPS_LOCK", (57358, "u")),
                            ("ISO_LEVEL5_SHIFT", (57454, "u"))):
        if byname.get(want_name) != want:
            sys.exit(f"FAIL: control {want_name} = {byname.get(want_name)}, want {want}")
    if len(keys) != 111 or len(byname) != len(keys) or len(cells) != 35:
        sys.exit(f"FAIL: {len(keys)} rows and {len(cells)} cells, want 111 and 35")
    if ("Backspace", "Alt", "0x1b 0x7f") not in cells:
        sys.exit("FAIL: control Alt-Backspace is not 0x1b 0x7f")
    pin = f"# From [[source: kitty-keyboard-rst]], sha256 {SHA256}.\n"
    a = ("# kitty's functional keys: name, CSI number, final byte (the first\n"
         "# alternative). Written by scripts/extract-kitty-keys.py; do not edit.\n" + pin
         + "".join(f"{n}\t{num}\t{f}\n" for n, num, f in keys))
    b = ("# kitty's legacy C0 table: key, modifier column, bytes as the table\n"
         "# spells them. Written by scripts/extract-kitty-keys.py; do not edit.\n" + pin
         + "".join(f"{k}\t{c}\t{v}\n" for k, c, v in cells))
    return a, b


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true")
    ap.add_argument("--source")
    args = ap.parse_args()
    a, b = render(source(args.source))
    if args.check:
        stale = [p.name for p, t in ((FUNCTIONAL, a), (LEGACY, b))
                 if not p.exists() or p.read_text() != t]
        if stale:
            sys.exit(f"STALE: {', '.join(stale)}")
        print("OK")
        return
    FUNCTIONAL.write_text(a)
    LEGACY.write_text(b)
    print("111 functional keys, 35 legacy cells")


if __name__ == "__main__":
    main()
