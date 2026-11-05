#!/usr/bin/env python3
"""Extract searchable text from Adobe TN #5176 (CFF) and #5177 (Type 2).

Both PDFs are **RC4-encrypted with an empty user password** (standard security
handler, /R 2, 40-bit). `extract-ecma48.py`'s plain inflater finds *zero*
streams in them, so every probe misses -- the probe-that-cannot-fire failure
the letter-spaced ECMA-48 extraction had first. This
script implements the PDF 1.4 standard security handler (Algorithm 3.2 / 3.1)
in the standard library, decrypts each stream, inflates it, and runs a
positive control per document so a broken extraction fails loudly.

    python3 scripts/extract-adobe-cff.py docs/research/sources/adobe-5176-cff.pdf
    python3 scripts/extract-adobe-cff.py docs/research/sources/adobe-5177-type2.pdf [probe ...]

Text comes out with the same letter-spacing/word-joining artefacts as the
ECMA-48 PDF; probes are matched on the alphanumeric-only fold of both sides,
as `extract-ecma48.py` does.
"""
from __future__ import annotations
import hashlib, re, sys, zlib
from pathlib import Path

PAD = bytes([0x28,0xBF,0x4E,0x5E,0x4E,0x75,0x8A,0x41,0x64,0x00,0x4E,0x56,0xFF,0xFA,0x01,0x08,
             0x2E,0x2E,0x00,0xB6,0xD0,0x68,0x3E,0x80,0x2F,0x0C,0xA9,0xFE,0x64,0x53,0x69,0x7A])

def rc4(key: bytes, data: bytes) -> bytes:
    S = list(range(256)); j = 0
    for i in range(256):
        j = (j + S[i] + key[i % len(key)]) & 255; S[i], S[j] = S[j], S[i]
    out = bytearray(); i = j = 0
    for c in data:
        i = (i + 1) & 255; j = (j + S[i]) & 255; S[i], S[j] = S[j], S[i]
        out.append(c ^ S[(S[i] + S[j]) & 255])
    return bytes(out)

def pdf_string(s: bytes) -> bytes:
    """Decode a PDF literal string `(...)` with backslash escapes, or a hex string."""
    if s.startswith(b"<"):
        return bytes.fromhex(re.sub(rb"[^0-9A-Fa-f]", b"", s[1:-1]).decode())
    body = s[1:-1].replace(b"\r\n", b"\n").replace(b"\r", b"\n"); out = bytearray(); i = 0
    while i < len(body):
        c = body[i]
        if c == 0x5C and i + 1 < len(body):
            n = body[i+1]; i += 2
            if n in b"nrtbf": out.append({0x6E:10,0x72:13,0x74:9,0x62:8,0x66:12}[n])
            elif 0x30 <= n <= 0x37:
                oct_ = bytes([n]); 
                while i < len(body) and len(oct_) < 3 and 0x30 <= body[i] <= 0x37: oct_ += bytes([body[i]]); i += 1
                out.append(int(oct_, 8) & 255)
            else: out.append(n)
        else: out.append(c); i += 1
    return bytes(out)

def find_string(dict_bytes: bytes, key: bytes) -> bytes:
    m = re.search(re.escape(key) + rb"\s*(\((?:\\.|\((?:\\.|[^\\()])*\)|[^\\()])*\)|<[0-9A-Fa-f\s]*>)", dict_bytes, re.S)
    if not m: raise SystemExit(f"{key.decode()} not found in Encrypt dict")
    return pdf_string(m.group(1))

def file_key(data: bytes) -> bytes:
    enc = re.search(rb"/Encrypt\s+(\d+)\s+(\d+)\s+R", data)
    if not enc: raise SystemExit("no /Encrypt: this PDF is not encrypted; use extract-ecma48.py")
    obj = re.search(rb"(?<![0-9])" + enc.group(1) + rb"\s+" + enc.group(2) + rb"\s+obj\s*(<<.*?>>)\s*endobj", data, re.S)
    d = obj.group(1)
    o = find_string(d, b"/O")
    p = int(re.search(rb"/P\s+(-?\d+)", d).group(1))
    r = int(re.search(rb"/R\s+(\d+)", d).group(1))
    ln = re.search(rb"/Length\s+(\d+)", d); nbytes = (int(ln.group(1)) // 8) if (ln and r >= 3) else 5
    ids = re.search(rb"/ID\s*\[\s*(<[0-9A-Fa-f]+>)", data)
    id0 = bytes.fromhex(ids.group(1)[1:-1].decode()) if ids else b""
    h = hashlib.md5(PAD + o[:32] + (p & 0xFFFFFFFF).to_bytes(4, "little") + id0)
    key = h.digest()[:nbytes]
    if r >= 3:
        for _ in range(50): key = hashlib.md5(key).digest()[:nbytes]
    u = find_string(d, b"/U")
    if r == 2 and rc4(key, PAD) != u[:32]:
        raise SystemExit("file key does not reproduce /U -- the key derivation is wrong, not the document")
    return key

def streams(data: bytes, key: bytes) -> list[bytes]:
    out = []
    for m in re.finditer(rb"(?<![0-9])(\d+)\s+(\d+)\s+obj\s*(<<(?:(?!endobj).)*?>>)\s*stream\r?\n", data, re.S):
        num, gen, d = int(m.group(1)), int(m.group(2)), m.group(3)
        L = re.search(rb"/Length\s+(\d+)(?!\s+\d+\s+R)", d)
        if not L: continue
        start = m.end(); raw = data[start:start + int(L.group(1))]
        okey = hashlib.md5(key + num.to_bytes(3, "little") + gen.to_bytes(2, "little")).digest()[:min(len(key) + 5, 16)]
        dec = rc4(okey, raw)
        if b"/FlateDecode" in d:
            try: dec = zlib.decompress(dec)
            except zlib.error: continue
        out.append(dec)
    return out

def raw_text(data: bytes, key: bytes) -> str:
    parts = []
    for s in streams(data, key):
        for tj in re.finditer(rb"\[((?:\((?:\\.|[^\\)])*\)|[^\]])*)\]\s*TJ|\(((?:\\.|[^\\)])*)\)\s*Tj", s, re.S):
            body = tj.group(1) if tj.group(1) is not None else b"(" + tj.group(2) + b")"
            for lit in re.finditer(rb"\((?:\\.|[^\\)])*\)", body, re.S):
                parts.append(pdf_string(lit.group(0)).decode("latin-1"))
            parts.append(" ")
    return "".join(parts)

def fold(s: str) -> str: return re.sub(r"[^a-z0-9]", "", s.lower())

CONTROLS = {
    "5176": ["compactfontformat", "charstrings", "fdselect", "privatedict", "index"],
    "5177": ["type2charstring", "hintmask", "callsubr", "endchar", "transientarray"],
}

def main() -> int:
    path = Path(sys.argv[1]); probes = sys.argv[2:]
    data = path.read_bytes(); key = file_key(data)
    text = raw_text(data, key); f = fold(text)
    which = "5176" if "5176" in path.name else "5177" if "5177" in path.name else None
    print(f"{path.name}: {len(text)} chars of text from {len(streams(data, key))} decrypted streams")
    bad = 0
    if which:
        for c in CONTROLS[which]:
            hit = c in f; bad += not hit; print(f"  control {c:22s} {'HIT' if hit else 'MISS'}")
    for p in probes:
        print(f"  probe   {p:22s} {'HIT' if fold(p) in f else 'MISS'}")
    if bad: print("POSITIVE CONTROL FAILED -- the extraction is broken, the document is not exonerated"); return 1
    return 0

if __name__ == "__main__": sys.exit(main())
