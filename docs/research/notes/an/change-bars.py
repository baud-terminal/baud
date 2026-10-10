# Note AN's change-bar check on DEC STD 070's page images. A change bar is a vertical
# stroke in a margin, apart from the text. Render the pages to greyscale PGM first:
#   pdftoppm -gray -r 60 -f 908 -l 933 <the PDF> p
# then:
#   python3 -I docs/research/notes/an/change-bars.py p-0908.pgm ... p-0933.pgm
# Prints, per page, each column in the outer 30% of the page that holds at least 10
# dark pixels with six blank columns between it and the text. The control: 070-3's
# barred pages, PDF 134, 135 and 191, each print one such column; run them first.
import sys


def load(path):
    data = open(path, "rb").read()
    fields, i = [], 0
    while len(fields) < 4:
        while data[i : i + 1].isspace():
            i += 1
        if data[i : i + 1] == b"#":
            while data[i : i + 1] != b"\n":
                i += 1
            continue
        j = i
        while not data[j : j + 1].isspace():
            j += 1
        fields.append(data[i:j])
        i = j
    w, h, maxval = int(fields[1]), int(fields[2]), int(fields[3])
    assert fields[0] == b"P5" and maxval < 256
    return w, h, data[i + 1 : i + 1 + w * h]


for path in sys.argv[1:]:
    w, h, px = load(path)
    lo, hi = int(h * 0.06), int(h * 0.94)
    dark = [sum(px[y * w + x] < 110 for y in range(lo, hi)) for x in range(w)]
    bars = []
    for x in list(range(0, int(w * 0.3))) + list(range(int(w * 0.7), w - 7)):
        apart = dark[x + 1 : x + 7] if x < w // 2 else dark[x - 6 : x]
        if dark[x] >= 10 and not any(apart):
            bars.append((x, dark[x]))
    print(path, bars)
