#!/usr/bin/env python3
"""Write the file `tests/recordings/utf8/vim-unicode.raw` shows vim paging.

    python3 scripts/gen-vim-unicode.py unicode.txt

1600 lines, four kinds repeated for each of 400 line numbers: wide CJK,
emoji with regional-indicator flags and a ZWJ family, Hebrew with its points
beside Arabic and Devanagari (combining marks and right-to-left text), and a
148-column line of ASCII with Cyrillic and Latin letters of ambiguous width.
No final newline, so vim reports `[noeol]`. Nothing is random: the same
command writes the same bytes.
"""

from __future__ import annotations

import sys

KINDS = (
    "{n:04d} 世界へようこそ 안녕하세요 你好世界 — wide CJK at column {i}",
    "{n:04d} 🚀🇯🇵👨\u200d👩\u200d👧 emoji, flags and a ZWJ family",
    "{n:04d} עִבְרִית والعربية नमस्ते combining marks and RTL",
    "{n:04d} " + " ".join(["mixed ascii ЁЖЗ ünïcödé"] * 6),
)


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    lines = [k.format(n=i, i=i) for i in range(400) for k in KINDS]
    with open(sys.argv[1], "wb") as f:
        f.write("\n".join(lines).encode("utf-8"))
    return 0


if __name__ == "__main__":
    sys.exit(main())
