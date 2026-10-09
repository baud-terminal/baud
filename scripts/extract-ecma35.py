#!/usr/bin/env python3
"""Extract searchable text from ECMA-35 (6th edition, 1994), whose PDF streams are LZWDecode.

Usage:
    scripts/extract-ecma35.py PDF                 # print the extracted text
    scripts/extract-ecma35.py PDF --find NEEDLE   # de-spaced search, with a positive control

Note AK quotes ECMA-35 15.1 and 15.4 (DOCS) from this text. The PDF is PDF 1.1
with LZW-compressed content streams, which zlib cannot inflate, so an extractor
built for ECMA-48 (scripts/extract-ecma48.py) finds no text in it and every
search misses. This decoder (9-12-bit codes, clear 256, EOD 257, EarlyChange 1)
joins the strings of each text object. As in extract-ecma48.py, a MISS means
absence only when the positive control HITs, and needles are compared with
every non-alphanumeric character removed from both sides.
"""
import re, sys

CONTROL = "graphic character"

def lzw_decode(data):
    out = bytearray()
    bitpos, nbits = 0, 9
    table = [bytes([i]) for i in range(256)] + [b'', b'']
    prev = None
    total = len(data) * 8
    while bitpos + nbits <= total:
        code = 0
        for _ in range(nbits):
            code = (code << 1) | ((data[bitpos >> 3] >> (7 - (bitpos & 7))) & 1)
            bitpos += 1
        if code == 256:
            table = table[:258]; nbits = 9; prev = None; continue
        if code == 257:
            break
        if prev is None:
            entry = table[code]
        elif code < len(table):
            entry = table[code]; table.append(prev + entry[:1])
        else:
            entry = prev + prev[:1]; table.append(entry)
        out += entry
        prev = entry
        n = len(table) + 1
        nbits = 12 if n >= 2048 else 11 if n >= 1024 else 10 if n >= 512 else 9
    return bytes(out)

def strings_of(content):
    parts = []
    for m in re.finditer(rb'\((?:\\.|[^\\)])*\)|BT|ET', content):
        t = m.group(0)
        if t == b'ET':
            parts.append('\n')
        elif t.startswith(b'('):
            s = t[1:-1]
            s = re.sub(rb'\\([nrtbf()\\])', lambda x: {b'n': b'\n', b'r': b'', b't': b' ', b'b': b'', b'f': b'', b'(': b'(', b')': b')', b'\\': b'\\'}[x.group(1)], s)
            s = re.sub(rb'\\([0-7]{1,3})', lambda x: bytes([int(x.group(1), 8) & 0xff]), s)
            parts.append(s.decode('latin-1'))
    return ''.join(parts)

def extract(pdf):
    text, n = [], 0
    for m in re.finditer(rb'<<(.*?)>>\s*stream\r?\n', pdf, re.S):
        d = m.group(1)
        L = re.search(rb'/Length\s+(\d+)', d)
        if b'/LZWDecode' not in d or not L:
            continue
        try:
            t = strings_of(lzw_decode(pdf[m.end():m.end() + int(L.group(1))]))
        except Exception:
            continue
        if t.strip():
            n += 1
            text.append(f'\n=== stream {n}\n' + t)
    return ''.join(text)

def despace(s):
    return re.sub(r'[^a-z0-9]', '', s.lower())

if __name__ == '__main__':
    text = extract(open(sys.argv[1], 'rb').read())
    if '--find' in sys.argv:
        needle = sys.argv[sys.argv.index('--find') + 1]
        flat = despace(text)
        c = flat.count(despace(CONTROL))
        print(f'control {CONTROL!r}: {"HIT" if c else "MISS"} x{c}')
        k = flat.count(despace(needle))
        print(f'{needle!r}: {"HIT" if k else "MISS"} x{k}')
        sys.exit(0 if c else 2)
    sys.stdout.write(text)
