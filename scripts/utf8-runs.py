#!/usr/bin/env python3
"""How long are the text runs a block UTF-8 decoder would work on?

Keiser and Lemire (`[[source: utf8-lookup]]`) price the lookup algorithm at 0.97
instructions per byte against a finite-state decoder's 7.0. That figure is only
reachable if there are *runs* to vectorise over: `baud-core/src/scan/portable.rs`
records that vectorising the ASCII scan cost 12% because printable runs there
average ~32 bytes, and concludes "vectorising a scan is only free when the runs
are long".

So this measures, per recording, the lengths of maximal runs of non-control
bytes -- what `feed_with` would hand to a block decoder -- and, decisively, what
share of *multi-byte* bytes live in runs long enough for one to pay.

Reads the recordings directly. No hot-path change, and re-runnable.
"""

from __future__ import annotations

import sys
from pathlib import Path

REC = Path(__file__).resolve().parent.parent / "baud-core/tests/recordings"


def runs(data: bytes) -> list[tuple[int, int]]:
    """Maximal runs of non-control bytes, as (length, multibyte_bytes)."""
    out = []
    i, n = 0, len(data)
    while i < n:
        if data[i] < 0x20 or data[i] == 0x7F:
            i += 1
            continue
        start = i
        multi = 0
        while i < n and data[i] >= 0x20 and data[i] != 0x7F:
            if data[i] >= 0x80:
                multi += 1
            i += 1
        out.append((i - start, multi))
    return out


def main() -> int:
    names = sys.argv[1:] or sorted(p.name for p in REC.glob("*.raw"))
    print(f"  {'recording':<22} {'runs':>7} {'mean':>7} {'p50':>5} {'p90':>6} "
          f"{'max':>7} {'mb bytes':>9} {'in >=16':>8} {'in >=32':>8}")
    for name in names:
        data = (REC / name).read_bytes()
        rs = runs(data)
        if not rs:
            print(f"  {name:<22} no runs")
            continue
        lens = sorted(r[0] for r in rs)
        total_mb = sum(r[1] for r in rs)
        mb16 = sum(r[1] for r in rs if r[0] >= 16)
        mb32 = sum(r[1] for r in rs if r[0] >= 32)
        pct = lambda a, b: f"{100.0 * a / b:.1f}%" if b else "n/a"
        print(f"  {name:<22} {len(rs):>7} {sum(lens) / len(rs):>7.1f} "
              f"{lens[len(lens) // 2]:>5} {lens[int(len(lens) * 0.9)]:>6} "
              f"{lens[-1]:>7} {total_mb:>9} {pct(mb16, total_mb):>8} "
              f"{pct(mb32, total_mb):>8}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
