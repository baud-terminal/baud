#!/usr/bin/env python3
"""Idle CPU: what `baud` costs with a screen that never changes.

Row 11's metric. The app is launched with a child that writes nothing,
so every display-link tick finds no bytes, no dirty rows and no blink --
the tick the row proposes to make cheaper.

**The launch is not idle and must not be in the number.** It is ~0.13 s
of CPU on this box against an idle rate near 0.004 s a second, so a
window total is three quarters launch and its variance swamps the
signal: differencing two window totals from two separate spawns measured
an A/A pair 37% apart. So the reading is taken from the *live* process,
twice, and only the delta is kept -- one spawn, launch outside both
samples.

`ps -o time` needs an entitlement on this macOS and `top` refuses to
sample, so the source is `proc_pid_rusage`. Its `ri_user_time` and
`ri_system_time` are **mach absolute time**, not nanoseconds: a fully
busy process moves them 23.8e6 in a second, which is the 41.67 ns tick
`mach_timebase_info` reports and the same clock the keystroke instrument
runs on. Reading them as nanoseconds understates CPU 24-fold.

    scripts/idle-cpu.py --arm base=target/release/baud \
                        --arm new=/tmp/new/baud --aa --rounds 5

Arms are rotated round to round, the first round is discarded as
warm-up, and `--aa` adds a byte-identical copy of the first arm so the
spread of two identical binaries is the floor a difference must clear.
"""

import argparse
import ctypes
import ctypes.util
import os
import shutil
import statistics
import subprocess
import sys
import tempfile
import time

_libproc = ctypes.CDLL(ctypes.util.find_library("proc") or "/usr/lib/libproc.dylib")
_libsys = ctypes.CDLL(ctypes.util.find_library("System") or "/usr/lib/libSystem.dylib")


class _Timebase(ctypes.Structure):
    _fields_ = [("numer", ctypes.c_uint32), ("denom", ctypes.c_uint32)]


def _tick_ns():
    tb = _Timebase()
    _libsys.mach_timebase_info(ctypes.byref(tb))
    return tb.numer / tb.denom


TICK_NS = _tick_ns()
# rusage_info_v2 opens with ri_uuid[16] -- two words -- then user and
# system time. Read it as words rather than mirroring the whole struct,
# which has grown a flavor at a time and would rot.
_UUID_WORDS = 2
_WORDS = 64


def cpu_seconds(pid):
    """CPU the live process has used, or None if it is gone."""
    buf = (ctypes.c_uint64 * _WORDS)()
    rc = _libproc.proc_pid_rusage(ctypes.c_int(pid), ctypes.c_int(2), ctypes.byref(buf))
    if rc != 0:
        return None
    ticks = buf[_UUID_WORDS] + buf[_UUID_WORDS + 1]
    return ticks * TICK_NS / 1e9


def sample(binary, settle, window, env):
    """CPU-seconds a second the binary spends idling, launch excluded."""
    proc = subprocess.Popen(
        [binary],
        env=env,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        close_fds=False,
    )
    try:
        time.sleep(settle)
        t0, c0 = time.time(), cpu_seconds(proc.pid)
        if c0 is None:
            raise RuntimeError(f"{binary} exited during its settle")
        time.sleep(window)
        t1, c1 = time.time(), cpu_seconds(proc.pid)
        if c1 is None:
            raise RuntimeError(f"{binary} exited during its window")
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=15)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()
    return (c1 - c0) / (t1 - t0)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--arm", action="append", required=True, metavar="NAME=PATH")
    ap.add_argument("--rounds", type=int, default=5)
    ap.add_argument("--settle", type=float, default=3.0)
    ap.add_argument("--window", type=float, default=20.0)
    ap.add_argument("--aa", action="store_true", help="add a copy of the first arm")
    ap.add_argument("--hz", type=float, default=120.0, help="the display's rate, for the per-tick column")
    ap.add_argument("--cmd", default="sleep 600", help="the child, which must write nothing")
    args = ap.parse_args()

    arms = []
    for spec in args.arm:
        name, _, path = spec.partition("=")
        arms.append((name, os.path.abspath(path)))
    tmp = None
    if args.aa:
        tmp = tempfile.mkdtemp(prefix="idle-aa-")
        copy = os.path.join(tmp, "baud")
        shutil.copy2(arms[0][1], copy)
        arms.append(("aa", copy))

    env = dict(os.environ)
    env["BAUD_CMD"] = args.cmd

    rates = {name: [] for name, _ in arms}
    for r in range(args.rounds):
        order = arms[r % len(arms) :] + arms[: r % len(arms)]
        for name, path in order:
            rate = sample(path, args.settle, args.window, env)
            rates[name].append(rate)
            print(
                f"  round {r + 1} {name:6}: {rate * 100:6.3f}% of a core, "
                f"{rate / args.hz * 1e6:6.1f} us a tick",
                flush=True,
            )

    print()
    print(f"{'arm':8} {'idle CPU':>12} {'range':>20} {'a tick':>12}")
    settled = {}
    for name, _ in arms:
        v = sorted(rates[name][1:])  # the first round is warm-up
        if not v:
            continue
        settled[name] = v
        med = statistics.median(v)
        print(
            f"{name:8} {med * 100:11.3f}% [{v[0] * 100:7.3f}-{v[-1] * 100:7.3f}] "
            f"{med / args.hz * 1e6:9.1f} us"
        )
    names = list(settled)
    if len(names) >= 2:
        base = statistics.median(settled[names[0]])
        floor = max(settled[names[-1]]) - min(settled[names[-1]])
        print()
        print(f"floor (the {names[-1]} arm's own spread): {floor * 100:.3f}% of a core")
        for name in names[1:]:
            med = statistics.median(settled[name])
            delta = med - base
            disjoint = min(settled[name]) > max(settled[names[0]]) or max(
                settled[name]
            ) < min(settled[names[0]])
            big = abs(delta) > floor and abs(delta) >= 0.05 * base
            print(
                f"{name} against {names[0]}: {delta * 100:+.3f}% "
                f"({delta / base * 100:+.1f}%), ranges "
                f"{'disjoint' if disjoint else 'overlap'} -- "
                f"{'result' if disjoint and big else 'not a result'}"
            )
    if tmp:
        shutil.rmtree(tmp, ignore_errors=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
