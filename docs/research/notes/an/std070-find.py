# Note AN's search of DEC STD 070's text layer, kept so a hit or a miss can be re-run.
#
# The PDF is `external` in the manifest (dec-std-070, sha256 d30c2ace...), so it is not
# here. Extract it first with poppler-utils 26.09:
#   pdftotext -layout EL-SM070-00_DEC_STD_070_Video_Systems_Reference_Manual_Dec91.pdf std070.txt
# then:
#   python3 -I docs/research/notes/an/std070-find.py std070.txt DECRQSS "blank Sixel"
#
# The text layer is OCR, so each needle and each page is compared with everything but
# [a-z0-9] removed: "0/7" read as "on" still misses, but spacing and punctuation do not.
# Run a needle known to be there (DECRQSS hits PDF 446, 447, 451, 482 and 1045) before
# believing a miss. Prints the PDF page numbers (1-based) of each hit.
import re
import sys

f = sys.argv[1]
pages = open(f, encoding="utf-8", errors="replace").read().split("\f")
norm = [re.sub(r"[^a-z0-9]", "", p.lower()) for p in pages]
for nd in sys.argv[2:]:
    k = re.sub(r"[^a-z0-9]", "", nd.lower())
    hits = [i + 1 for i, p in enumerate(norm) if k in p]
    print(f"{nd!r}: {len(hits)} pages {hits[:40]}")
