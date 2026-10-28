#!/usr/bin/env python3
"""Generate baud-font's CFF Standard Encoding table from the pinned Adobe TN 5176.

`seac` (the accent-composition form of `endchar`) names its two glyphs by
Standard Encoding *codes*, and the code-to-SID table is Appendix B of
`[[source: adobe-5176-cff]]`. That PDF is RC4-encrypted with an empty user
password; `scripts/extract-adobe-cff.py` decrypts it, and this script parses
the appendix's "code / SID / name" rows -- which the text layer runs together
with no separator, so the sequential code is the anchor -- into a 256-byte
table. Emitted as a byte string, not an array literal, because rustfmt rewraps
arrays and the committed file would stop matching the generator's output.

Usage: gen-cff-standard-encoding.py <5176.CFF.pdf> > baud-font/src/standard_encoding.rs
"""
import re, sys
from pathlib import Path

# What CC BY-ND 4.0 section 3(a)(1) asks a copy of the material to carry:
# the creator, the copyright notice, the licence and its URI, a notice that
# refers to the disclaimer of warranties, a link to the material, and
# whether it was modified. The copyright line is TN 5176's own, from its
# second page; the grant is the LICENSE.md of Adobe's font-tech-notes
# repository, pinned in the manifest as adobe-font-tech-notes-license.
ATTRIBUTION = """\
//! The table is part of Appendix B, "Predefined Encodings", of The Compact
//! Font Format Specification, Technical Note #5176, version 1.0, by Adobe
//! Systems Incorporated:
//! https://adobe-type-tools.github.io/font-tech-notes/pdfs/5176.CFF.pdf
//!
//! Copyright \u00a9 1996\u20131998, 2000, 2003 by Adobe Systems Incorporated.
//! All rights reserved.
//!
//! Adobe licenses its font technical notes under the Creative Commons
//! Attribution-NoDerivatives 4.0 International licence (CC BY-ND 4.0),
//! https://creativecommons.org/licenses/by-nd/4.0/, in the LICENSE.md of
//! https://github.com/adobe-type-tools/font-tech-notes. The material is
//! offered as-is and as-available, with no warranties of any kind: see the
//! licence's section 5, Disclaimer of Warranties and Limitation of
//! Liability.
//!
//! Not modified. The appendix's code and SID columns are reproduced in
//! part, as the licence's section 2(a)(1)(A) allows: its name column is
//! left out, and each SID is one byte at its code. Writing the same values
//! in another format is a technical modification under section 2(a)(4),
//! which never produces Adapted Material.
"""

HERE = Path(__file__).resolve().parent
src = (HERE / "extract-adobe-cff.py").read_text().replace('if __name__ == "__main__": sys.exit(main())', "")
ns = {}
exec(compile(src, "extract-adobe-cff.py", "exec"), ns)

data = Path(sys.argv[1]).read_bytes()
text = ns["raw_text"](data, ns["file_key"](data))
start = text.rfind("Standard Encoding  (code / SID / name)")
if start < 0:
    sys.exit("POSITIVE CONTROL FAILED: the appendix header is not in the extracted text")
body = text[start:]
# Rows are "{code}{sid}{name}" with names of letters, digits and dots; page
# furniture ("38Appendix B Predefined Encodings(4 Dec 03)") interleaves and is
# skipped by anchoring each row on the next expected code.
tokens = re.findall(r"(\d+)([A-Za-z.][A-Za-z0-9.]*)", body)
table = [None] * 256
code = 0
for digits, name in tokens:
    if code > 255:
        break
    prefix = str(code)
    if not digits.startswith(prefix) or len(digits) == len(prefix):
        continue
    sid = int(digits[len(prefix):])
    table[code] = (sid, name)
    code += 1
if code != 256 or any(t is None for t in table):
    sys.exit(f"POSITIVE CONTROL FAILED: parsed {code} rows, expected 256")
# Anchors from the specification's own text, not from another implementation.
for c, sid, name in [(32, 1, "space"), (65, 34, "A"), (97, 66, "a"), (0, 0, ".notdef"), (251, 149, "germandbls")]:
    if table[c] != (sid, name):
        sys.exit(f"POSITIVE CONTROL FAILED: code {c} parsed as {table[c]}, expected {(sid, name)}")
if max(s for s, _ in table) > 255:
    sys.exit("a SID does not fit in a byte")
out = "".join(f"\\x{s:02x}" for s, _ in table)
print("//! CFF Standard Encoding, code to SID, generated from Appendix B of")
print("//! `[[source: adobe-5176-cff]]` by `scripts/gen-cff-standard-encoding.py`.")
print("//! Do not edit; regenerate.")
print("//!")
print(ATTRIBUTION, end="")
print("//!")
print("//! Indexed by the code `seac` supplies; the value is the standard string id")
print("//! the face's charset then maps to a glyph. Every SID in Standard Encoding is")
print("//! below 150, so a byte holds it.")
print()
print("/// `STANDARD_ENCODING[code]` is the SID; 0 is `.notdef`.")
print(f'pub const STANDARD_ENCODING: &[u8; 256] = b"{out}";')
