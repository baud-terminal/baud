#!/usr/bin/env python3
"""Generate baud-domain/src/vt/iterm_keys.rs: every OSC 1337 key iTerm2's
pinned documentation names, and every key its pinned source compares.

    python3 scripts/gen-iterm2-keys.py --sources DIR           write the file
    python3 scripts/gen-iterm2-keys.py --sources DIR --check   fail if stale
    python3 scripts/gen-iterm2-keys.py --fetch DIR             download them
    python3 scripts/gen-iterm2-keys.py --search                the multipliers

The documents are GPL-2.0 and `external` in the manifest, so they are not in
this tree: `--fetch` downloads each from its pinned URL into DIR, and every
run checks each file in DIR against the SHA-256 the manifest records before
it reads a byte.

The documentation's keys are every word after `OSC 1337` (or `ESC ] 1337`,
or a shell example's `\\e]1337;`, `\\033]1337;` and `<Esc>]1337;`) in the
four pages that define one: `[[source: iterm2-escape-codes]]`,
`[[source: iterm2-images]]`, `[[source: iterm2-badges]]` and
`[[source: iterm2-feature-reporting]]`. The images page's grammar line
`ESC ] 1337 ; key = value ^G` names a placeholder, not a key, and is the one
word left out. The source's keys are the `[key isEqualToString:@"..."]`
compares in `executeXtermSetKvp` (`[[source: iterm2-964a75d-vt100terminal]]`).

`--search` finds the multipliers `iterm.rs`'s two indexes are built with:
FNV-1a over a name, times an odd multiplier, its high bits a slot, as
`NameIndex::bucket` computes it, and one under which no two of the table's
names share a slot, so a lookup is one hash and one compare.

A positive control runs before any miss is believed: SetProfile (written
with no `;` in the page), FileEnd (the images page), SetBadgeFormat (the
badges page) and Capabilities (the feature-reporting page) must be found.
`every_iterm2_key_has_its_row` (baud-domain) requires the table in
`baud-domain/src/vt/iterm.rs` to hold each key listed here, and nothing else.
"""

import argparse
import hashlib
import pathlib
import re
import sys
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parent.parent
MANIFEST = ROOT / "docs/research/sources/MANIFEST.toml"
OUT = ROOT / "baud-domain/src/vt/iterm_keys.rs"

DOCS = [
    "iterm2-escape-codes",
    "iterm2-images",
    "iterm2-badges",
    "iterm2-feature-reporting",
]
SOURCE = "iterm2-964a75d-vt100terminal"
PLACEHOLDER = "key"
CONTROLS = {
    "SetProfile": "iterm2-escape-codes",
    "FileEnd": "iterm2-images",
    "SetBadgeFormat": "iterm2-badges",
    "Capabilities": "iterm2-feature-reporting",
}


def manifest():
    """key -> (url, sha256), from the manifest's blocks."""
    rows, key = {}, None
    for line in MANIFEST.read_text().splitlines():
        m = re.match(r'^\[sources\.(.+)\]$', line)
        if m:
            key = m.group(1)
            rows[key] = {}
            continue
        m = re.match(r'^(url|sha256) *= *"(.*)"$', line)
        if m and key:
            rows[key][m.group(1)] = m.group(2)
    return rows


def local(rows, key, d):
    return pathlib.Path(d) / rows[key]["url"].rsplit("/", 1)[1]


def read(rows, key, d):
    p = local(rows, key, d)
    data = p.read_bytes()
    got = hashlib.sha256(data).hexdigest()
    if got != rows[key]["sha256"]:
        sys.exit(f"{p}: sha256 {got}, the manifest records {rows[key]['sha256']}")
    return data.decode()


def doc_keys(rows, d):
    found = {}
    pat = re.compile(r'(?:OSC|ESC \]|\\e\]|\\033\]|<Esc>\])\s*1337\s*;?\s*([A-Za-z0-9]+)')
    for key in DOCS:
        for m in pat.finditer(read(rows, key, d)):
            word = m.group(1)
            if word != PLACEHOLDER:
                found.setdefault(word, key)
    for word, page in CONTROLS.items():
        if found.get(word) != page:
            sys.exit(f"control: {word} not found in {page}; the extraction is broken")
    return sorted(found)


def source_keys(rows, d):
    src = read(rows, SOURCE, d)
    a = src.index("- (void)executeXtermSetKvp:")
    b = src.index("\n- (", a + 1)
    keys = re.findall(r'\[key isEqualToString:@"([^"]+)"\]', src[a:b])
    if "CursorShape" not in keys or "ExecFailed" not in keys:
        sys.exit("control: CursorShape or ExecFailed missing from executeXtermSetKvp")
    return sorted(set(keys))


def render(doc, src):
    def block(name, what, keys):
        out = [f"/// {what}", f"pub const {name}: [&[u8]; {len(keys)}] = ["]
        out += [f'    b"{k}",' for k in keys]
        out.append("];")
        return out

    lines = [
        "//! iTerm2's `OSC 1337` keys, generated from the sources pinned in",
        "//! `docs/research/sources/MANIFEST.toml`.",
        "//!",
        "//! **Do not edit by hand.** Regenerate with `scripts/gen-iterm2-keys.py`,",
        "//! which reads the pinned pages and source (external, GPL-2.0) and",
        "//! checks each against its SHA-256; `every_iterm2_key_has_its_row` fails",
        "//! if the table in `iterm.rs` misses a key listed here.",
        "",
    ]
    lines += block(
        "DOC_KEYS",
        "Every key `[[source: iterm2-escape-codes]]`, `[[source: iterm2-images]]`,\n"
        "/// `[[source: iterm2-badges]]` and `[[source: iterm2-feature-reporting]]`\n"
        "/// name after `OSC 1337`.",
        doc,
    )
    lines.append("")
    lines += block(
        "SOURCE_KEYS",
        "Every key `executeXtermSetKvp` compares\n"
        "/// (`[[source: iterm2-964a75d-vt100terminal]]`).",
        src,
    )
    return "\n".join(lines) + "\n"


def fnv(b):
    h = 0xCBF29CE484222325
    for c in b:
        h = ((h ^ c) * 0x100000001B3) & (2**64 - 1)
    return h


def collides(names, slots, mul):
    shift = 64 - (slots.bit_length() - 1)
    seen = set()
    for n in names:
        i = ((fnv(n) * mul) & (2**64 - 1)) >> shift
        if i in seen:
            return True
        seen.add(i)
    return False


def search():
    """A multiplier for each of iterm.rs's indexes, by a seeded search."""
    import random

    src = (ROOT / "baud-domain/src/vt/iterm.rs").read_text()
    for kind, slots in (("ItermKey", 128), ("ColorTarget", 64)):
        names = [m.encode() for m in re.findall(r'^    \(b"([^"]+)", ' + kind, src, re.M)]
        rng = random.Random(7)
        while True:
            mul = rng.getrandbits(64) | 1
            if not collides(names, slots, mul):
                print(f"{kind}: {len(names)} names, {slots} slots, {mul:#018x}")
                break


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--sources")
    ap.add_argument("--fetch")
    ap.add_argument("--check", action="store_true")
    ap.add_argument("--search", action="store_true")
    a = ap.parse_args()
    if a.search:
        return search()
    rows = manifest()
    if a.fetch:
        d = pathlib.Path(a.fetch)
        d.mkdir(parents=True, exist_ok=True)
        for key in DOCS + [SOURCE]:
            with urllib.request.urlopen(rows[key]["url"]) as r:
                local(rows, key, d).write_bytes(r.read())
            read(rows, key, d)
        return
    if not a.sources:
        sys.exit(__doc__)
    text = render(doc_keys(rows, a.sources), source_keys(rows, a.sources))
    if a.check:
        if OUT.read_text() != text:
            sys.exit(f"{OUT} is stale: run scripts/gen-iterm2-keys.py --sources DIR")
        return
    OUT.write_text(text)


if __name__ == "__main__":
    main()
