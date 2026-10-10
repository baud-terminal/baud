# Note AN's page printer for DEC STD 070's extracted text (see std070-find.py for the
# extraction). Prints PDF pages `first` to `last` with "p<page>:<line>|" prefixes, the
# form the note's line references were read from:
#   python3 -I docs/research/notes/an/std070-page.py std070.txt 925 926
# 070-9, the Sixel chapter, is PDF pages 908-935; printed page n is PDF page 911 + n.
# Every quote the note takes from it was then read against the page image:
#   pdftoppm -r 110 -f 925 -l 926 -png <the PDF> page
import sys

f, a = sys.argv[1], int(sys.argv[2])
b = int(sys.argv[3]) if len(sys.argv) > 3 else a
pages = open(f, encoding="utf-8", errors="replace").read().split("\f")
for n in range(a, b + 1):
    for i, line in enumerate(pages[n - 1].split("\n"), 1):
        print(f"p{n}:{i:3d}| {line}")
