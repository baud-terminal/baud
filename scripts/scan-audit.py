#!/usr/bin/env python3
"""Count the loops on the ingest path that *search* rather than look up.

The architecture is a compile-time database and a runtime that indexes it. A
scan is the shape that says a question was never put to the table.

Not counted, and why:
  * `const fn` builders -- they run at build time and *are* the database.
  * `feed`'s `while done < n` -- that is the iteration, not a search.
  * bounded work loops -- `Paint::wrapped` walks the rows a run crosses and
    `put_run` decodes four characters a step; neither is looking for anything.

Counted: a loop inside a run scanner, i.e. one that advances a byte cursor to
find how far a run goes. Those exist only because the graph is one byte wide,
so an edge can describe a single byte and nothing more.
"""
import re
import sys

SCANNERS = ("run_of_text", "run_of")
PATH = "baud-core/src/write.rs"

src = open(PATH).read().splitlines()
found = []
i = 0
while i < len(src):
    m = re.match(r"^(pub\(crate\) )?fn (\w+)", src[i].strip())
    if m and m.group(2) in SCANNERS:
        name, depth, j = m.group(2), 0, i
        while j < len(src):
            depth += src[j].count("{") - src[j].count("}")
            body = src[j].strip()
            if body.startswith("while ") and j > i:
                found.append((name, j + 1, body[:60]))
            if depth <= 0 and j > i:
                break
            j += 1
        i = j
    i += 1

limit = int(sys.argv[1]) if len(sys.argv) > 1 else 5
for name, line, text in found:
    print(f"  ..   {PATH}:{line}  {name}: {text}")
print(f"  {'ok  ' if len(found) <= limit else 'FAIL'} ingest scans: {len(found)} "
      f"(limit {limit}, target 0)")
sys.exit(0 if len(found) <= limit else 1)
