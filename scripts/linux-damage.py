#!/usr/bin/env python3
"""Summarise a scripts/linux-damage.sh run: <dir> holds one folder a
compositor, each with the `present` bench's output.

  linux-damage.py <dir>

For each compositor and workload, each arm's `present` time -- the median
of the per-round medians, the first round dropped, and the range of those
medians -- with the bytes copied and the rows damaged a frame, the waits
for a buffer and the compositor's CPU time a frame. **A VM's frames run
at two speeds**, some four times apart, a whole run at a time and the
render with them, so a round is told by its render time, which is the
same work on both arms: at 0.8 or more of the workload's slowest round
it is slow, at 0.35 or less fast, and between, a run that changed speed,
counted and left out. Each speed is reported apart. Then the screen
checks: the pixels of the opaque runs that are not the frame (0 is
right), and whether the two arms' screens hash the same frame by frame,
opaque and at the default dim. Then the frame traffic of the opaque `rows`
run: which buffer each frame was attached to, whether one was used for
two frames running, and whether a buffer's release came before or after
the next frame's commit.

Standard library only. Exits 1 when a check fails.
"""

import pathlib
import re
import statistics
import sys

LINE = re.compile(
    r"^(\w+)\s+frames (\d+) present_us ([\d.]+) \[([\d.]+)-([\d.]+)\] "
    r"render_us ([\d.]+) \[[\d.]+-[\d.]+\] copied_bytes/frame (\d+) "
    r"damaged_rows/frame (\d+) waits (\d+) compositor_ms/frame (\S+)"
)
SHOT = re.compile(r"^shot (\w+) (\d+) (\S+) differ (\d+) copied (\d+) damaged (\d+)")


def timed(folder):
    """{(work, arm): [per-round fields]}, round 1 dropped."""
    runs = {}
    for f in sorted(folder.glob("time-*-*.txt")):
        _, rnd, arm = f.stem.split("-")
        if rnd == "1":
            continue
        for line in f.read_text().splitlines():
            m = LINE.match(line)
            if m:
                runs.setdefault((m[1], arm), []).append(m)
    return runs


def shots(path):
    out = []
    for line in path.read_text().splitlines() if path.exists() else []:
        m = SHOT.match(line)
        if m:
            out.append((m[1], int(m[2]), m[3], int(m[4]), int(m[5]), int(m[6])))
    return out


def traffic(path):
    """The attaches, and how the releases fell against the commits."""
    attached, damage, releases = [], [], []
    held = None
    after_next, before_next = 0, 0
    for line in path.read_text().splitlines() if path.exists() else []:
        m = re.search(r"\.attach\(wl_buffer#(\d+)", line)
        if m:
            attached.append(m[1])
            continue
        m = re.search(r"\.damage_buffer\((\d+), (\d+), (\d+), (\d+)\)", line)
        if m:
            damage.append(int(m[4]))
            continue
        if line.endswith(".commit()") and attached:
            # The buffer of the frame before is released after this commit,
            # or was before it.
            if held is not None and held != attached[-1]:
                if held in releases:
                    before_next += 1
                else:
                    after_next += 1
            held = attached[-1]
            releases = []
            continue
        m = re.search(r"wl_buffer#(\d+)\.release", line)
        if m:
            releases.append(m[1])
    again = sum(1 for a, b in zip(attached, attached[1:]) if a == b)
    return attached, damage, again, before_next, after_next


def main():
    top = pathlib.Path(sys.argv[1])
    failed = False
    for folder in sorted(p for p in top.iterdir() if p.is_dir()):
        comp = (folder / "compositor.txt").read_text().strip() if (
            folder / "compositor.txt"
        ).exists() else "?"
        print(f"== {folder.name}: {comp}")
        runs = timed(folder)
        works = sorted({w for w, _ in runs}, key=["line", "walk", "scroll", "full"].index)
        for work in works:
            slowest = max(float(m[6]) for (w, _), ms in runs.items() if w == work for m in ms)
            for speed, keep in [
                ("slow", lambda r: r >= 0.8 * slowest),
                ("fast", lambda r: r <= 0.35 * slowest),
            ]:
                for arm in ["whole", "rows"]:
                    every = runs.get((work, arm), [])
                    ms = [m for m in every if keep(float(m[6]))]
                    mixed = sum(
                        1 for m in every if 0.35 * slowest < float(m[6]) < 0.8 * slowest
                    )
                    if not ms:
                        continue
                    med = [float(m[3]) for m in ms]
                    comp_ms = [float(m[10]) for m in ms if m[10] != "-"]
                    cpu = (
                        f"{statistics.median(comp_ms):.3f} [{min(comp_ms):.3f}-{max(comp_ms):.3f}]"
                        if comp_ms
                        else "-"
                    )
                    print(
                        f"  {work:6} {speed} {arm:5} present {statistics.median(med):6.1f} us "
                        f"[{min(med):.1f}-{max(med):.1f}], {len(ms)} rounds ({mixed} mixed); "
                        f"copied {ms[0][7]} B/frame, damaged {ms[0][8]} rows/frame, "
                        f"waits {sum(int(m[9]) for m in ms)}; compositor {cpu} ms/frame"
                    )
        for alpha in ["255", "dim"]:
            a = shots(folder / f"shot-{alpha}-whole.txt")
            b = shots(folder / f"shot-{alpha}-rows.txt")
            same = [x[:2] + (x[2],) for x in a] == [x[:2] + (x[2],) for x in b]
            line = f"  screens, page {alpha}: {len(a)} and {len(b)} shots, the arms' hashes {'the same' if same and a else 'DIFFER'}"
            if alpha == "255":
                differ = sum(x[3] for x in a), sum(x[3] for x in b)
                line += f"; pixels not the frame: whole {differ[0]}, rows {differ[1]}"
                if differ != (0, 0):
                    failed = True
            if not (same and a):
                failed = True
            print(line)
        attached, damage, again, before, after = traffic(folder / "trace-255-rows.txt")
        print(
            f"  traffic, opaque rows: {len(attached)} attaches over {len(set(attached))} buffers, "
            f"{again} onto the buffer just shown; the last buffer released before "
            f"the next commit {before} times, after it {after}; damage heights "
            f"{sorted(set(damage))}"
        )
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
