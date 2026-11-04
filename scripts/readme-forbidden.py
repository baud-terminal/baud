#!/usr/bin/env python3
"""Search the README session's bytes for what must never be filmed.

usage: readme-forbidden.py PATTERNS CONTROLS FILE...

PATTERNS holds one 'label::regex' a line, a Python bytes regex, '#' lines
and blank ones skipped; it is kept outside the repository, since it names
what it forbids. CONTROLS holds 'label<TAB>example' lines, one for every
label, each an example its pattern must find: before any FILE is read,
every label must have a control and every control must be found, by its
own pattern and by the patterns joined, or nothing is searched (exit 2) --
a search that cannot fire looks exactly like a clean one.

Each FILE is searched raw and with its terminal escapes taken out, so a
word a program coloured letter by letter is still found. A hit prints the
label and the file, never what matched (exit 1); none, exit 0.
"""

import re
import sys

ESC = re.compile(
    rb"\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|\x1b\[[0-?]*[ -/]*[@-~]|"
    rb"\x1b[()*+][0-~]|\x1b[ -/]*[0-~]|[\x00-\x08\x0e-\x1f]"
)


def patterns(path):
    out = []
    with open(path, "rb") as f:
        for line in f:
            line = line.rstrip(b"\n")
            if not line.strip() or line.startswith(b"#"):
                continue
            name, sep, rx = line.partition(b"::")
            if not sep:
                sys.exit(f"readme-forbidden: a line with no label in {path}")
            out.append((name.decode(), re.compile(rx)))
    if not out:
        sys.exit(f"readme-forbidden: {path} holds no pattern")
    return out


def controls(path):
    out = {}
    with open(path, "rb") as f:
        for line in f:
            line = line.rstrip(b"\n")
            if not line.strip() or line.startswith(b"#"):
                continue
            k, _, v = line.partition(b"\t")
            out[k.decode()] = v
    return out


def views(data):
    """The bytes as written, and with the terminal's escapes taken out."""
    return [data, ESC.sub(b"", data)]


def main():
    if len(sys.argv) < 4:
        sys.exit(__doc__.strip().splitlines()[2])
    pats = patterns(sys.argv[1])
    big = re.compile(b"|".join(b"(?:%s)" % rx.pattern for _, rx in pats))
    ctl = controls(sys.argv[2])
    bad = []
    for name, rx in pats:
        v = ctl.get(name)
        if v is None:
            bad.append(f"{name}: no control")
        elif not rx.search(v):
            bad.append(f"{name}: its control is not found")
        elif not big.search(v):
            bad.append(f"{name}: the joined patterns miss its control")
    if bad:
        for b in bad:
            print(f"readme-forbidden: {b}", file=sys.stderr)
        sys.exit(2)
    hits = []
    for path in sys.argv[3:]:
        with open(path, "rb") as f:
            data = f.read()
        for view in views(data):
            if not big.search(view):
                continue
            for name, rx in pats:
                if rx.search(view) and (name, path) not in hits:
                    hits.append((name, path))
    for name, path in hits:
        print(f"readme-forbidden: {name} in {path}", file=sys.stderr)
    print(
        f"readme-forbidden: {len(pats)} patterns, each found in its control; "
        f"{len(sys.argv) - 3} files, {len(hits)} hits"
    )
    sys.exit(1 if hits else 0)


if __name__ == "__main__":
    main()
