# Note AN's scan of the repository's recordings for the Sixel shapes the note's builds
# would change. Run from the repository root over every tracked .raw and .six file:
#   python3 -I docs/research/notes/an/sixel-scan.py $(git ls-files '*.raw' '*.six')
# For each file, every DCS whose prefix is digits and ';' and whose final is 'q' (a
# Sixel, as baud's graph reads one: graph.rs 865-903) is cut at its first ST, CAN, SUB
# or ESC that is not ST, and its body is counted for: SPACE, C0, DEL, 80-FF bytes, a
# raster-attributes command and its Pan;Pad, P1, a '!' count followed by a byte that is
# neither data, digit nor another command baud knows, a colour define with Pu not 2 or
# an RGB value over 100, a register drawn with and never defined, and a body cut by
# something other than ST.
import re
import sys

DCS = re.compile(rb"\x1bP([0-9;]*)q")


def scan(name, data):
    out = []
    for m in DCS.finditer(data):
        start = m.end()
        end = start
        cut = "EOF"
        while end < len(data):
            b = data[end]
            if b == 0x1B:
                cut = "ST" if data[end + 1 : end + 2] == b"\\" else "ESC"
                break
            if b in (0x18, 0x1A):
                cut = "CAN" if b == 0x18 else "SUB"
                break
            end += 1
        body = data[start:end]
        params = m.group(1).decode()
        raster = re.match(rb'"(\d*);(\d*)', body)
        rep_bad = len(re.findall(rb"!\d*[\x21\x25-\x2c\x2e-\x2f\x3a\x3c-\x3e]", body))
        defs = re.findall(rb"#(\d+);(\d*);(\d*);(\d*);(\d*)", body)
        bad_defs = sum(
            1
            for _, pu, x, y, z in defs
            if pu != b"2" or any(int(v or b"0") > 100 for v in (x, y, z))
        )
        # A register drawn with but never defined: a bare '#Pc' selection of a Pc that
        # no '#Pc;...' defines, or data before any colour command (register 0).
        defined = {int(pc) for pc, *_ in defs}
        selections = [
            int(s.group(1))
            for s in re.finditer(rb"#(\d+)(;[0-9;]*)?", body)
            if s.group(2) is None
        ]
        undefined = sum(1 for s in selections if s not in defined)
        first = body.find(b"#")
        before = body if first < 0 else body[:first]
        data_before = sum(1 for c in before if 0x3F <= c <= 0x7E)
        out.append(
            f"{name}: P {params or '-'} body {len(body)} B cut {cut}; "
            f"raster {'%s;%s' % (raster.group(1).decode(), raster.group(2).decode()) if raster else 'none'} first; "
            f"SPACE {body.count(b' ')} C0 {sum(1 for c in body if c < 0x20)} "
            f"DEL {body.count(0x7F)} 80-FF {sum(1 for c in body if c >= 0x80)}; "
            f"repeat then other command {rep_bad}; colour defines {len(defs)}, "
            f"Pu not 2 or value over 100 {bad_defs}; selections {len(selections)}, "
            f"of a register never defined {undefined}; data bytes before any '#' "
            f"{data_before}"
        )
    return out


total = 0
for f in sys.argv[1:]:
    lines = scan(f, open(f, "rb").read())
    total += len(lines)
    for line in lines:
        print(line)
print(f"{total} Sixel strings in {len(sys.argv) - 1} files")
