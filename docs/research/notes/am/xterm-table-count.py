#!/usr/bin/env python3
"""Count the names xterm 411's XTGETTCAP table answers (docs/research/sources/xterm-xtermcap.c
70-244), with USE_EXTENDED_NAMES expanded. Run: python3 xterm-table-count.py"""
import re, pathlib
src = (pathlib.Path(__file__).resolve().parents[2] / "sources/xterm-xtermcap.c").read_text()
body = src[src.index("static const TCAPINFO table[] = {"):src.index("#undef DATA")]
body = "\n".join(l for l in body.splitlines() if not l.lstrip().startswith("#define"))
base = [(m.group(1), m.group(2)) for m in re.finditer(r'DATA\(\s*"([^"]*)",\s*"([^"]*)",', body)]
groups = re.findall(r"DGRP\((\w+),\s*(\w+)\)", body)
ext = []
for i, (n, _) in enumerate(groups):
    b = "k" if i < 10 else "kp"
    ext += [b + n] + [b + n + str(p) for p in range(3, 9)]
ti = [t for _, t in base] + ext
tc = [c for c, _ in base if c]
names = set(ti) | set(tc)
print(f"DATA rows {len(base)}, modifier groups {len(groups)}")
print(f"distinct terminfo names {len(set(ti))}, distinct termcap codes {len(set(tc))}")
print(f"distinct names {len(names)}, longest {max(map(len, names))} characters")
