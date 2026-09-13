"""The comparison table from `vt-compare.sh`'s results file.

usage: vt-table.py <results> [<baseline>]

Per terminal and recording: the median throughput of the settled rounds
(the first round is discarded) in MiB/s -- mebibytes, the unit `ingest2`
prints -- with the range, and the terminal process's CPU seconds for the
run, with its startup subtracted when a baseline file -- the same runs at
zero replays -- is given. Every run is a fresh process, so a run's CPU
time is startup plus the work, and the baseline is startup plus nothing.
"""
import sys
from collections import defaultdict


def seconds(t):
    """`top`'s `MM:SS.CC`, or `HH:MM:SS`, as seconds."""
    parts = t.split(":")
    if len(parts) == 2:
        return int(parts[0]) * 60 + float(parts[1])
    if len(parts) == 3:
        return int(parts[0]) * 3600 + int(parts[1]) * 60 + float(parts[2])
    return float("nan")


def load(path):
    rows = defaultdict(list)
    meta = {}
    for line in open(path):
        f = line.split()
        if len(f) < 8:
            continue
        term, rec, size, total, t0, t1, cpu, comm = f[:8]
        reply = f[8] if len(f) > 8 else "?"
        wall = float(t1) - float(t0)
        rows[(term, rec)].append((int(total), wall, seconds(cpu), reply))
        meta[term] = (size, comm)
    return rows, meta


rows, meta = load(sys.argv[1])
base = {}
if len(sys.argv) > 2:
    brows, _ = load(sys.argv[2])
    for (term, _), runs in brows.items():
        cpus = sorted(c for _, _, c, _ in runs[1:] or runs)
        base[term] = cpus[len(cpus) // 2]
terms = sorted({t for t, _ in rows})
recs = sorted({r for _, r in rows})
print("Throughput, MiB/s, median of the settled rounds [range]:")
print()
print("| recording | " + " | ".join(terms) + " |")
print("|---|" + "---|" * len(terms))
for rec in recs:
    cells = []
    for term in terms:
        runs = rows.get((term, rec), [])
        runs = runs[1:] or runs
        if not runs:
            cells.append("--")
            continue
        rates = sorted(t / w / (1 << 20) for t, w, _, _ in runs)
        med = rates[len(rates) // 2]
        cells.append(f"**{med:.0f}** [{rates[0]:.0f}-{rates[-1]:.0f}]")
    print(f"| {rec} | " + " | ".join(cells) + " |")
print()
label = "CPU seconds for the run, startup subtracted" if base else "CPU seconds at the end of the run, startup included"
print(f"{label}, median of the settled rounds [range]:")
print()
print("| recording | " + " | ".join(terms) + " |")
print("|---|" + "---|" * len(terms))
for rec in recs:
    cells = []
    for term in terms:
        runs = rows.get((term, rec), [])
        runs = runs[1:] or runs
        if not runs:
            cells.append("--")
            continue
        cpus = sorted(c - base.get(term, 0.0) for _, _, c, _ in runs)
        med = cpus[len(cpus) // 2]
        cells.append(f"**{med:.2f}** [{cpus[0]:.2f}-{cpus[-1]:.2f}]")
    print(f"| {rec} | " + " | ".join(cells) + " |")
print()
print("| terminal | geometry the child saw | process measured | startup CPU, s |")
print("|---|---|---|---|")
for term in terms:
    size, comm = meta.get(term, ("?", "?"))
    print(f"| {term} | {size} | {comm} | {base.get(term, float('nan')):.2f} |")
bad = [(t, r, rep) for (t, r), runs in rows.items() for _, _, _, rep in runs if not rep.endswith("52")]
if bad:
    print()
    print(f"{len(bad)} runs did not end in a cursor report and are not results: " + ", ".join(f"{t}/{r}" for t, r, _ in bad[:8]))
