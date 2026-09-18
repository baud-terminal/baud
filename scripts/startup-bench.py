#!/usr/bin/env python3
"""Launch `baud` repeatedly and report the wall from `fork` to its first
presented frame, with the in-app stages -- one arm, or several arms
interleaved with an A/A copy, which is how a launch change is kept or
reverted under the rule of `docs/research/notes/n-make-faster.md`.

The app prints `startup: <stage> <ms>` lines under `BAUD_STARTUP=1`, timed
from its own `main`; this script times from the spawn, so the gap between
the two is the loader's share. Each launch runs `BAUD_CMD` (a sleep, so
the window is not closed by the child exiting before it has presented) at
a fixed grid, and is killed once the stage the wall is taken to has
arrived.

    scripts/startup-bench.py [runs=7] [binary=target/release/baud]
    scripts/startup-bench.py --rounds 7 --arm base=<binary> --arm new=<binary> [--aa]

With `--arm`, every round launches each arm once in an order that rotates
round by round, and `--aa` adds a second launch of the first arm's binary
under the name `aa`: the same program measured twice in the same sweep,
whose spread is the noise floor a difference between arms has to clear.
The first round is discarded, the way the first round of any sweep here
is. A `binary` may be the executable inside an `.app` bundle
(`target/baud.app/Contents/MacOS/baud`): `NSBundle` resolves the bundle
from the executable's path, so that is how a bundle is measured.

Per arm the report gives the median and [min-max] of the wall and of
every in-app stage that appeared in all of the arm's settled launches --
so a stage one arm does not print (a Dock icon it no longer sets) is
absent from that arm rather than a zero. With two or more arms it then
prints, for every stage both arms carry, arm minus base beside the A/A
spread of the same stage: a difference inside that spread is not a
result.

Take it on mains, on an idle box, and read the range as well as the
median: the first launch after a build pays the loader's cold cache and
is reported but excluded from the median.

    BAUD_BENCH_STAGE   the line the wall is taken to: `first completion|first
                       present` by default, or `first content` (the first
                       frame after the child wrote)
    BAUD_CMD           the child: `shell` runs the login shell the app would
                       run; unset is `sleep 5`
    BAUD_GRID          the grid, `200x50` by default

EXIT CODES
    0  measured
    1  a launch never reached the stage, or bad usage
"""
import os
import re
import signal
import subprocess
import sys
import time

STAGE = os.environ.get("BAUD_BENCH_STAGE", "first completion|first present")
FIRST = re.compile(rf"^startup: ({STAGE})\s")
MARK = re.compile(r"^startup: (.+?)\s+([\d.]+) ms$")


def parse_args(argv):
    """Positional `[runs] [binary]` for one arm, or `--rounds/--arm/--aa`."""
    rounds, arms, aa = 7, [], False
    positional = []
    it = iter(argv)
    for a in it:
        if a == "--rounds":
            rounds = int(next(it))
        elif a == "--arm":
            name, _, path = next(it).partition("=")
            if not path:
                sys.exit("--arm takes name=path")
            arms.append((name, path))
        elif a == "--aa":
            aa = True
        elif a.startswith("-"):
            sys.exit(__doc__)
        else:
            positional.append(a)
    if positional:
        rounds = int(positional[0])
    if not arms:
        arms = [("base", positional[1] if len(positional) > 1 else "target/release/baud")]
    if aa:
        arms.append(("aa", arms[0][1]))
    return rounds, arms


def child_env():
    env = dict(os.environ)
    env.update({"BAUD_STARTUP": "1"})
    env.setdefault("BAUD_GRID", "200x50")
    # `BAUD_CMD=shell` runs the login shell the app would run, for the wall
    # to `first content`; anything else is the child, and unset is `sleep 5`.
    if env.get("BAUD_CMD") == "shell":
        del env["BAUD_CMD"]
    else:
        env.setdefault("BAUD_CMD", "sleep 5")
    return env


def launch(binary, env):
    """One launch: the wall to the stage, and every stage's own clock."""
    t0 = time.perf_counter()
    # `close_fds=False`: the default walks and closes every descriptor before
    # exec, ~3 ms a launch on this box that the terminal never pays.
    p = subprocess.Popen(
        [binary], env=env, stderr=subprocess.PIPE, stdout=subprocess.DEVNULL, text=True, close_fds=False
    )
    stages = {}
    lines = []
    wall = None
    for line in p.stderr:
        line = line.rstrip("\n")
        if line.startswith("startup:"):
            lines.append(line)
            m = MARK.match(line)
            # A note (`startup: walk: 0 map misses, ... 0.0 ms`) is not a
            # stage: it carries a colon, and a stage name never does.
            if m and ":" not in m.group(1) and m.group(1) not in stages:
                stages[m.group(1)] = float(m.group(2))
        if FIRST.match(line):
            wall = (time.perf_counter() - t0) * 1e3
            break
    p.send_signal(signal.SIGTERM)
    try:
        p.wait(timeout=5)
    except subprocess.TimeoutExpired:
        p.kill()
    return wall, stages, lines


def median(xs):
    s = sorted(xs)
    return s[len(s) // 2]


def rotate(items, rnd):
    k = rnd % len(items) if items else 0
    return items[k:] + items[:k]


def main():
    rounds, arms = parse_args(sys.argv[1:])
    env = child_env()
    results = {name: [] for name, _ in arms}
    last_lines = {}
    for rnd in range(rounds):
        for name, binary in rotate(arms, rnd):
            wall, stages, lines = launch(binary, env)
            if wall is None:
                print(f"round {rnd} arm {name}: never reached the stage; stages seen: {lines}")
                sys.exit(1)
            results[name].append((wall, stages))
            last_lines[name] = lines
            tag = "  (discarded)" if rnd == 0 else ""
            print(f"round {rnd} arm {name:5s}: {wall:8.1f} ms to the stage{tag}")
            time.sleep(0.5)

    print()
    if len(arms) == 1:
        for line in last_lines[arms[0][0]]:
            print(line)
        print()

    settled = {name: (rs[1:] if len(rs) > 1 else rs) for name, rs in results.items()}
    summary = {}
    for name, _ in arms:
        rs = settled[name]
        walls = [w for w, _ in rs]
        common = set.intersection(*(set(s) for _, s in rs)) if rs else set()
        per_stage = {st: [s[st] for _, s in rs] for st in common}
        summary[name] = (walls, per_stage)
        print(f"== arm {name} ({len(rs)} settled launches) ==")
        print(f"  wall to the stage: median {median(walls):.1f} ms, range [{min(walls):.1f}-{max(walls):.1f}]")
        for st in sorted(per_stage, key=lambda s: median(per_stage[s])):
            v = per_stage[st]
            print(f"  {st:20s} median {median(v):8.1f} ms  [{min(v):7.1f}-{max(v):7.1f}]")
        print()

    if len(arms) > 1:
        base_name = arms[0][0]
        base_walls, base_stages = summary[base_name]
        aa = summary.get("aa")
        print(f"== against {base_name}, per stage: arm - base, beside the A/A spread ==")
        for name, _ in arms[1:]:
            if name == "aa":
                continue
            walls, stages = summary[name]
            d = median(walls) - median(base_walls)
            floor = ""
            if aa:
                aw = aa[0]
                spread = max(abs(median(aw) - median(base_walls)), max(aw) - min(aw), max(base_walls) - min(base_walls))
                verdict = "result" if abs(d) > spread and (max(walls) < min(base_walls) or min(walls) > max(base_walls)) else "inside the A/A spread -- not a result"
                floor = f"  (A/A spread {spread:.1f} ms: {verdict})"
            print(f"  {name}: wall {d:+.1f} ms{floor}")
            for st in sorted(set(stages) & set(base_stages)):
                ds = median(stages[st]) - median(base_stages[st])
                fl = ""
                if aa and st in aa[1]:
                    av = aa[1][st]
                    fl = f"  (A/A {abs(median(av) - median(base_stages[st])):.1f})"
                print(f"    {st:20s} {ds:+8.1f} ms{fl}")
            gone = sorted(set(base_stages) - set(stages))
            new = sorted(set(stages) - set(base_stages))
            if gone:
                print(f"    stages the arm no longer prints: {', '.join(gone)}")
            if new:
                print(f"    stages new to the arm: {', '.join(new)}")


if __name__ == "__main__":
    main()
