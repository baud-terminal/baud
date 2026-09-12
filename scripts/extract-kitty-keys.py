#!/usr/bin/env python3
"""Extract the functional-key table from the pinned kitty-keyboard source.

`[[source: kitty-keyboard]]`'s "Functional key definitions" table maps a key
name to the number and final byte its `CSI` form uses -- ESCAPE to `27 u`,
HOME to `1 H or 7 ~`, CAPS_LOCK to `57358 u`, and so on across 86 rows. baud's
kitty encoder needs that mapping, and a key table written from memory is a
guessed table that looks like it worked -- the failure this repo refused for
DEC line drawing and enumerated the base input layer against.

So it is read from the source. `tests/kitty_functional_keys.rs` re-runs this
and asserts the Rust table matches, the way `cursor_keys_match_the_
specifications_table` gates the base layer.

The HTML renders the two-column table as alternating text lines:

    Name          <- key name
    27 u          <- CSI form: number, then final byte (or "A or B" alternates)

Usage:
    python3 scripts/extract-kitty-keys.py            # name<TAB>number<TAB>final
    python3 scripts/extract-kitty-keys.py --count    # just the row total
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

SRC = (
    Path(__file__).resolve().parent.parent
    / "docs/research/sources/kitty-keyboard-protocol.html"
)

# The CSI form is "<number> <final>", optionally "<n> <f> or <n2> <f2>". The
# canonical form baud sends is the FIRST alternative -- the second is the
# legacy `~` form kitty also accepts, and sending one spelling is enough.
FORM = re.compile(r"^(\d+)\s+([A-Za-z~])(?:\s+or\s+.*)?$")
# A key name: uppercase letters, digits and underscores, at least one letter.
NAME = re.compile(r"^[A-Z][A-Z0-9_]*$")


def clean(html: str) -> list[str]:
    text = re.sub(r"<[^>]*>", "", html)
    for a, b in (("&gt;", ">"), ("&lt;", "<"), ("&amp;", "&"), ("&quot;", '"'),
                 ("&#39;", "'")):
        text = text.replace(a, b)
    text = text.replace("\u00b6", "")
    return [ln.strip() for ln in text.splitlines() if ln.strip()]


def rows(lines: list[str]) -> list[tuple[str, int, str]]:
    # The table is bounded by its own headers: it opens at "Functional key
    # codes" and the last row is ISO_LEVEL5_SHIFT, after which a "Note" follows.
    try:
        start = lines.index("Functional key codes")
    except ValueError:
        sys.exit("FAIL: 'Functional key codes' header not found -- source moved")
    out: list[tuple[str, int, str]] = []
    i = start + 1
    while i < len(lines) - 1:
        name, form = lines[i], lines[i + 1]
        if name in ("Note", "Legacy ctrl mapping of ASCII keys"):
            break
        m = FORM.match(form)
        if NAME.match(name) and m:
            out.append((name, int(m.group(1)), m.group(2)))
            i += 2
        else:
            # Header cells ("Name", "CSI") and prose sit between the table and
            # its heading; skip a single line and resync on the next pair.
            i += 1
    return out


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--count", action="store_true")
    args = ap.parse_args()
    table = rows(clean(SRC.read_text()))
    # Positive control: the table has a known shape. ESCAPE=27, the PUA runs
    # from CAPS_LOCK=57358, and the last row is ISO_LEVEL5_SHIFT. If any of
    # these is gone the extraction is broken and the count is meaningless --
    # the "run a positive control before believing a MISS" rule.
    byname = {n: (num, f) for n, num, f in table}
    for want_name, want in (("ESCAPE", (27, "u")), ("CAPS_LOCK", (57358, "u")),
                            ("ISO_LEVEL5_SHIFT", (57454, "u"))):
        if byname.get(want_name) != want:
            sys.exit(f"FAIL: control {want_name} = {byname.get(want_name)}, want {want}")
    if len(table) < 80:
        sys.exit(f"FAIL: only {len(table)} rows parsed -- the extractor is reading nothing")
    if args.count:
        print(len(table))
        return
    for n, num, f in table:
        print(f"{n}\t{num}\t{f}")


if __name__ == "__main__":
    main()
