#!/usr/bin/env python3
"""What the Metal shader cache is worth to `baud` at launch: the same
binary launched with the system's shader caches in place (warm) and moved
aside (cold), interleaved, reading the app's own `gpu build:` line.

Row 1 of `docs/research/notes/n-make-faster.md` rests on a research figure
-- "~50 ms per pipeline chain on the first launch after an install or an
OS update" -- that nothing had measured on baud. This is the measurement.
The app, under `BAUD_STARTUP=1`, prints one line from its GPU probe:

    startup: gpu build: device D ms, library L (S), pipelines P, warm pass W, probe T

`S` is where the library came from -- `Library` for the committed
`baud.metallib`, `Compiled` when it would not load and the source was
compiled instead. A cold arm that reads `Library` and a cold library time
is the whole of what row 1 shipped, and an arm that reads `Compiled` is
measuring the old program.

`library` is `newLibraryWithSource` (the shader source to a library) and
`pipelines` the five `newRenderPipelineState` calls; both are served from
the system's cache on a warm launch and compiled on a cold one. `device`
and the warm pass do not depend on the cache and are the control: if they
move between arms, the arms are not what they claim.

WHAT "COLD" IS HERE
    macOS keeps the compiled shaders in two per-user directories under
    `$(getconf DARWIN_USER_CACHE_DIR)`: `com.apple.metalfe` (the front end:
    source to AIR) and `com.apple.metal` (per-GPU function and library
    binaries). A cold arm moves both aside before the launch and restores
    them after it, so every cold launch compiles from nothing -- which is
    what the first launch after an install or an OS update does. The
    directories belong to every app on the box, so restoring them is not
    optional: this script restores them on every exit path, and an entry
    another app wrote while they were aside is lost to it (that app
    recompiles once). Do not run this beside anything that matters.

HOW TO READ IT
    Per arm, the median and [min-max] of each field over the settled
    launches (the first launch of each arm is discarded), plus the wall
    from spawn to the first present. The row's keep rule is >= 20 ms on
    launch: if cold - warm is under that, the cache is not where the launch
    time is and the row closes on the measurement.

    scripts/shader-cache-bench.py [launches=7] [binary=target/release/baud]

EXIT CODES
    0  measured
    1  a launch printed no `gpu build:` line, or the caches could not be
       moved and restored
"""
import os
import re
import shutil
import signal
import subprocess
import sys
import time

launches = int(sys.argv[1]) if len(sys.argv) > 1 else 7
binary = sys.argv[2] if len(sys.argv) > 2 else "target/release/baud"
# Which cache the cold arm moves aside: `both` (the default, a first launch
# after an install or an OS update), `metalfe` (the front end only: what a
# precompiled `.metallib` would spare) or `metal` (the per-GPU binaries
# only: what a binary archive would spare).
aside_which = os.environ.get("BAUD_SHADER_CACHE_ASIDE", "both")

CACHE_ROOT = subprocess.run(
    ["getconf", "DARWIN_USER_CACHE_DIR"], capture_output=True, text=True, check=True
).stdout.strip()
CACHE_NAMES = {
    "both": ("com.apple.metal", "com.apple.metalfe"),
    "metal": ("com.apple.metal",),
    "metalfe": ("com.apple.metalfe",),
}[aside_which]
CACHES = [os.path.join(CACHE_ROOT, d) for d in CACHE_NAMES]
ASIDE = [c + ".shader-cache-bench-aside" for c in CACHES]

BUILD = re.compile(
    r"^startup: gpu build: device ([\d.]+) ms, library ([\d.]+) \((\w+)\), "
    r"pipelines ([\d.]+), "
    r"warm pass ([\d.]+), probe ([\d.]+)"
)
FIRST = re.compile(r"^startup: (first completion|first present)\s")
FIELDS = ["device", "library", "pipelines", "warm", "probe", "wall"]
SOURCE_GROUP = 3  # the `(Library|Compiled)` capture, read as a string not a float


def move_aside():
    """The caches out of the way; a leftover from an interrupted run is
    restored first so nothing is buried twice."""
    restore()
    for c, a in zip(CACHES, ASIDE):
        if os.path.isdir(c):
            os.rename(c, a)


def restore():
    """The originals back, over whatever the cold launch wrote."""
    for c, a in zip(CACHES, ASIDE):
        if os.path.isdir(a):
            if os.path.isdir(c):
                shutil.rmtree(c)
            os.rename(a, c)


def launch():
    env = dict(os.environ)
    env.update({"BAUD_STARTUP": "1", "BAUD_GRID": "120x40", "BAUD_CMD": "sleep 5"})
    t0 = time.perf_counter()
    p = subprocess.Popen(
        [binary], env=env, stderr=subprocess.PIPE, stdout=subprocess.DEVNULL, text=True, close_fds=False
    )
    got = {}
    wall = None
    for line in p.stderr:
        m = BUILD.match(line)
        if m:
            g = list(m.groups())
            got["source"] = g.pop(SOURCE_GROUP - 1)
            for name, v in zip(FIELDS[:5], g):
                got[name] = float(v)
        if FIRST.match(line):
            wall = (time.perf_counter() - t0) * 1e3
            break
    p.send_signal(signal.SIGTERM)
    try:
        p.wait(timeout=5)
    except subprocess.TimeoutExpired:
        p.kill()
    if wall is None or "probe" not in got:
        return None
    got["wall"] = wall
    return got


def median(xs):
    s = sorted(xs)
    return s[len(s) // 2]


results = {"warm": [], "cold": []}
try:
    for i in range(launches):
        # Interleaved, the cold arm first so a warm launch never follows a
        # cold one that left the caches aside.
        for arm in ("cold", "warm"):
            if arm == "cold":
                move_aside()
            try:
                got = launch()
            finally:
                if arm == "cold":
                    restore()
            if got is None:
                print(f"launch {i} {arm}: no gpu build line or no first present; stopping")
                sys.exit(1)
            results[arm].append(got)
            print(
                f"launch {i} {arm:4s}: library {got['library']:6.1f} ms ({got.get('source', '?')}), "
                f"pipelines {got['pipelines']:6.1f}, "
                f"device {got['device']:5.1f}, warm pass {got['warm']:5.1f}, probe {got['probe']:6.1f}, "
                f"wall to first present {got['wall']:6.1f}" + ("  (discarded)" if i == 0 else "")
            )
            time.sleep(0.5)
finally:
    restore()

print()
print(f"{'field':10s} {'warm median [min-max]':28s} {'cold median [min-max]':28s} cold - warm")
for f in FIELDS:
    w = [r[f] for r in results["warm"][1:]] or [r[f] for r in results["warm"]]
    c = [r[f] for r in results["cold"][1:]] or [r[f] for r in results["cold"]]
    wm, cm = median(w), median(c)
    print(
        f"{f:10s} {wm:7.1f} [{min(w):6.1f}-{max(w):6.1f}]       {cm:7.1f} [{min(c):6.1f}-{max(c):6.1f}]       {cm - wm:+7.1f} ms"
    )
print(
    f"\n{len(results['warm']) - 1} settled launches an arm; the first of each arm is discarded; "
    f"the cold arm moved aside: {', '.join(CACHE_NAMES)}."
)

# **Where the library came from, asserted and not merely printed.** A field
# that is shown per launch and never read is the counter this repo keeps
# recording: an arm whose launches disagree with each other -- a fallback
# firing on some and not others -- would average two programs and read as
# one result. Comparing a source-only binary against a `.metallib` one is
# the whole point of this script, so the arms are allowed to differ from
# each other; what is refused is an arm that differs from itself.
mixed = False
for arm in ("warm", "cold"):
    seen = sorted({r.get("source", "?") for r in results[arm][1:]})
    print(f"{arm:5s} library from: {', '.join(seen)}")
    if len(seen) > 1:
        mixed = True
if mixed:
    print(
        "an arm's launches did not all take the same library, so its median "
        "is two programs averaged; discard this run",
        file=sys.stderr,
    )
    sys.exit(1)
