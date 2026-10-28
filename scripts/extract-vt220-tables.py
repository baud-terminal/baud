#!/usr/bin/env python3
"""Write the cells of the VT220's character tables that baud-domain's tests read.

`[[source: dec-special-graphics]]` is vt100.net's transcription of the VT220
Programmer Reference Manual's chapter 2, and `[[source: vt220-rm-ch4]]` its
chapter 4. Neither page is in the repository; this script fetches each by the
URL the manifest records and refuses it unless it hashes to the manifest's
SHA-256. What it keeps is the tables' content, one cell a line, in

    baud-domain/tests/vt220_tables.tsv

which `nrcs_dec.rs`, `dec_sets.rs` and `scripts/gen-dec-sets.py` read; no test
fetches. No prose of either page is kept.

Lines, tab-separated:

    caption  <figure>  <caption text>
    cell     <figure>  <code, hex>  <glyph>  <shaded: 1 or 0>
    final    <set name>  <finals>

* **Tables 2-1 and 2-5 to 2-15** (ASCII and the national sets): each glyph is a
  `<text class="chname">` placed by x and y and followed by a `<text
  class="chnums">` carrying its code as octal, decimal and hex, and each
  substituted cell is shaded by a `<rect fill="#ddd">`. A glyph's cell is read
  from its geometry and must equal the hex the page prints after it, or the
  script fails: the check on the geometry. All 96 cells of columns 2-7 are
  kept, `SP` and `DEL` included, which the tests use as controls.
* **Table 2-3's right half** (`T2-3b`): the 94 GR cells by their code's low
  seven bits, read in document order, with an empty glyph where the table draws
  nothing. Its shaded field is 0.
* **Table 4-4**'s national replacement sets: DEC's name and the finals printed
  beside it, each final checked against the column/row printed above it.

Usage:
    python3 scripts/extract-vt220-tables.py            # fetch, verify, write
    python3 scripts/extract-vt220-tables.py --check    # fetch, verify, compare
    ... --chapter2 FILE --chapter4 FILE                # local copies, still hashed
"""

from __future__ import annotations

import argparse
import hashlib
import html
import re
import sys
import tomllib
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
with open(ROOT / "docs/research/sources/MANIFEST.toml", "rb") as _f:
    _MAN = tomllib.load(_f)["sources"]
CH2 = _MAN["dec-special-graphics"]
CH4 = _MAN["vt220-rm-ch4"]
OUT = ROOT / "baud-domain/tests/vt220_tables.tsv"

NRC_FIGURES = ["T2-1"] + [f"T2-{n}" for n in range(5, 16)]


def fail(msg: str) -> None:
    sys.exit(f"FAIL: {msg}")


def source(pin: dict, path: str | None) -> str:
    if path:
        raw = Path(path).read_bytes()
    else:
        with urllib.request.urlopen(pin["url"], timeout=60) as r:
            raw = r.read()
    got = hashlib.sha256(raw).hexdigest()
    if got != pin["sha256"]:
        fail(f"{pin['key']} hashes {got}, the pin is {pin['sha256']}")
    return raw.decode("utf-8")


def figure(page: str, fid: str) -> str:
    opening = f'<figure id="{fid}">'
    start = page.find(opening)
    if start < 0:
        fail(f"{fid}: no figure")
    rest = page[start + len(opening):]
    return rest[: rest.index("</figure>")]


def attr(el: str, name: str) -> str:
    m = re.search(rf'(?:^|\s){name}="([^"]*)"', el)
    if not m:
        fail(f"no {name} in {el}")
    return m.group(1)


def tenths(s: str) -> int:
    v = float(s)
    t = round(v * 10)
    if abs(v * 10 - t) > 1e-6:
        fail(f"coordinate {s} is finer than tenths")
    return t


def cell(x: int, y: int, x0: int, y0: int) -> int | None:
    """The code at (x, y) on a grid whose column 0, row 0 sits at (x0, y0),
    columns 85 apart and rows 45 apart; None off the grid or outside 2-7."""
    dx, dy = x - x0, y - y0
    if dx < 0 or dy < 0 or dx % 850 or dy % 450:
        return None
    col, row = dx // 850, dy // 450
    return col * 16 + row if 2 <= col <= 7 and row < 16 else None


def unescape(s: str) -> str:
    out = s.replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
    if out != "&" and "&" in out:
        fail(f"an entity this reader does not decode: {s}")
    return out


def nrc(page: str, fid: str) -> tuple[str, dict[int, str], set[int]]:
    """A national table: its caption, the glyph at each code of columns 2-7,
    and the shaded codes. Column 0's glyphs sit at x 137.5 and its boxes at
    x 110; row 0's glyphs at y 127 and its boxes at y 100."""
    fig = figure(page, fid)
    caption = fig.split("<figcaption>")[1].split("</figcaption>")[0]
    glyphs: dict[int, str] = {}
    for m in re.finditer(r'<text class="chname"([^>]*)>((?:(?!</text>).)*)</text>\s*(<text class="chnums"(?:(?!</text>).)*</text>)', fig, re.S):
        code = cell(tenths(attr(m.group(1), "x")), tenths(attr(m.group(1), "y")), 1375, 1270)
        if code is None:
            continue
        nums = re.findall(r"<tspan[^>]*>([^<]*)</tspan>", m.group(3))
        if len(nums) < 3 or int(nums[2], 16) != code:
            fail(f"{fid}: the glyph placed at 0x{code:02x} carries code {nums[2:3]}")
        if code in glyphs:
            fail(f"{fid}: two glyphs at 0x{code:02x}")
        glyphs[code] = unescape(m.group(2))
    shaded: set[int] = set()
    for m in re.finditer(r"<rect ([^>]*?)/>", fig):
        if 'fill="#ddd"' not in m.group(1):
            continue
        code = cell(tenths(attr(m.group(1), "x")), tenths(attr(m.group(1), "y")), 1100, 1000)
        if code is None:
            fail(f"{fid}: a shaded box off the grid: {m.group(0)}")
        shaded.add(code)
    if sorted(glyphs) != list(range(0x20, 0x80)):
        fail(f"{fid}: {len(glyphs)} of 96 cells extracted")
    return caption, glyphs, shaded


def table_2_3_right(page: str) -> dict[int, str]:
    """Table 2-3's right half by the low seven bits: the glyph, or '' where the
    table draws nothing. A control's name (IND, CSI) is in columns 8 and 9,
    which the range drops."""
    fig = figure(page, "T2-3b")
    items = re.findall(r'<text class="(chname|chnums)"[^>]*?(?:/>|>(.*?)</text>)', fig)
    out: dict[int, str] = {}
    for (cls, text), nxt in zip(items, items[1:]):
        if cls != "chname" or nxt[0] != "chnums" or "<tspan" not in nxt[1]:
            continue
        nums = re.findall(r"<tspan[^>]*>([^<]*)</tspan>", nxt[1])
        code = int(nums[2], 16)
        if not 0x21 <= code <= 0x7E:
            continue
        glyph = html.unescape(re.sub(r"<[^>]+>", "", text)) if text else ""
        if code in out:
            fail(f"T2-3b: 0x{code:02x} twice")
        if len(glyph) > 1:
            fail(f"T2-3b: 0x{code:02x} draws {glyph!r}, more than one character")
        out[code] = glyph
    if sorted(out) != list(range(0x21, 0x7F)):
        fail(f"T2-3b: {len(out)} of 94 cells parsed")
    return out


def table_4_4(page: str) -> list[tuple[str, str]]:
    """Table 4-4's national sets: DEC's name and the finals beside it."""
    start = page.index('<table id="T4-4">')
    table = page[start: page.index("</table>", start)]
    table = table[table.index("National replacement character sets"):]
    sets = []
    for row in table.split("<tr>")[1:]:
        cells = [c[c.index(">") + 1: c.index("</td>")].strip() for c in row.split("<td")[1:]]
        pre = next((c[len("<pre>"):] for c in cells if c.startswith("<pre>")), None)
        if pre is None:
            continue
        name = next(c for c in cells if c and not c.startswith("<pre>"))
        lines = unescape(pre.removesuffix("</pre>")).splitlines()
        codes = [int(c) * 16 + int(r) for c, r in (t.split("/") for t in lines[0].split())]
        finals = [t for t in lines[1].split() if t != "or"]
        if any(len(f) != 1 for f in finals) or codes != [ord(f) for f in finals]:
            fail(f"T4-4 {name}: the column/row and the final disagree")
        sets.append((name, "".join(finals)))
    return sets


def render(ch2: str, ch4: str) -> str:
    out = [
        "# The VT220's character tables, the cells baud-domain's tests read.",
        f"# Chapter 2, [[source: dec-special-graphics]], {CH2['url']}",
        f"#   sha256 {CH2['sha256']}.",
        f"# Chapter 4, [[source: vt220-rm-ch4]], {CH4['url']}",
        f"#   sha256 {CH4['sha256']}.",
        "# Written by scripts/extract-vt220-tables.py; do not edit.",
        "# caption <figure> <text> | cell <figure> <code> <glyph> <shaded> | final <set> <finals>",
    ]
    for fid in NRC_FIGURES:
        caption, glyphs, shaded = nrc(ch2, fid)
        out.append(f"caption\t{fid}\t{caption}")
        out.extend(f"cell\t{fid}\t{c:02x}\t{glyphs[c]}\t{int(c in shaded)}" for c in sorted(glyphs))
    for c, g in sorted(table_2_3_right(ch2).items()):
        out.append(f"cell\tT2-3b\t{c:02x}\t{g}\t0")
    out.extend(f"final\t{n}\t{f}" for n, f in table_4_4(ch4))
    for line in out:
        if line.count("\t") > 4 or "\n" in line:
            fail(f"a field holds a tab or a newline: {line!r}")
    return "\n".join(out) + "\n"


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true")
    ap.add_argument("--chapter2")
    ap.add_argument("--chapter4")
    args = ap.parse_args()
    text = render(source(CH2, args.chapter2), source(CH4, args.chapter4))
    if args.check:
        if not OUT.exists() or OUT.read_text() != text:
            sys.exit(f"STALE: {OUT.relative_to(ROOT)}")
        print("OK")
        return
    OUT.write_text(text)
    print(f"{text.count(chr(10) + 'cell')} cells, {text.count(chr(10) + 'final')} national sets")


if __name__ == "__main__":
    main()
