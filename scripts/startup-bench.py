#!/usr/bin/env python3
"""Launch `baud` N times and report the wall from `fork` to its first
presented frame, with the in-app stages of the last launch.

The app prints `startup: <stage> <ms>` lines under `BAUD_STARTUP=1`,
timed from its own `main`; this script times from the spawn, so the gap
between the two is the loader's share. Each launch runs `BAUD_CMD` (a
sleep, so the window is not closed by the child exiting before it has
presented) at a fixed grid, and is killed once the first-present line
has arrived.

    scripts/startup-bench.py [runs=7] [binary=target/release/baud]

Take it on mains, on an idle box, and read the range as well as the
median: the first launch after a build pays the loader's cold cache and
is reported but excluded from the median, the way the first round of a
sweep is.
"""
import os
import re
import signal
import subprocess
import sys
import time

runs = int(sys.argv[1]) if len(sys.argv) > 1 else 7
binary = sys.argv[2] if len(sys.argv) > 2 else "target/release/baud"
# `BAUD_BENCH_STAGE` names the line the wall is taken to: the first frame
# by default, or `first content` for the first frame after the child wrote.
STAGE = os.environ.get("BAUD_BENCH_STAGE", "first completion|first present")
FIRST = re.compile(rf"^startup: ({STAGE})\s")

env = dict(os.environ)
env.update({"BAUD_STARTUP": "1", "BAUD_GRID": "200x50"})
# `BAUD_CMD=shell` runs the login shell the app would run, for the wall to
# `first content`; anything else is the child, and unset is `sleep 5`.
if env.get("BAUD_CMD") == "shell":
    del env["BAUD_CMD"]
else:
    env.setdefault("BAUD_CMD", "sleep 5")

walls = []
last_lines = []
for i in range(runs):
    t0 = time.perf_counter()
    # `close_fds=False`: the default walks and closes every descriptor before
    # exec, ~3 ms a launch on this box that the terminal never pays.
    p = subprocess.Popen([binary], env=env, stderr=subprocess.PIPE, stdout=subprocess.DEVNULL, text=True, close_fds=False)
    lines = []
    wall = None
    for line in p.stderr:
        line = line.rstrip("\n")
        if line.startswith("startup:"):
            lines.append(line)
        if FIRST.match(line):
            wall = (time.perf_counter() - t0) * 1e3
            break
    p.send_signal(signal.SIGTERM)
    try:
        p.wait(timeout=5)
    except subprocess.TimeoutExpired:
        p.kill()
    if wall is None:
        print(f"run {i}: no first-present line; stages seen: {lines}")
        sys.exit(1)
    walls.append(wall)
    last_lines = lines
    print(f"run {i}: {wall:8.1f} ms to first present" + ("  (discarded)" if i == 0 else ""))
    time.sleep(0.5)

print()
for line in last_lines:
    print(line)
settled = sorted(walls[1:]) if len(walls) > 1 else walls
median = settled[len(settled) // 2]
print(f"\nwall to first present: median {median:.1f} ms, range [{settled[0]:.1f}-{settled[-1]:.1f}] over {len(settled)} settled runs")
