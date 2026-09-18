#!/usr/bin/env python3
"""Capture a shell's output across a sequence of window sizes.

`capture-tui.py` sets `TIOCSWINSZ` once and records raw bytes with no
frame boundaries, which is right for a recording of a program drawing and
wrong for a recording of a *resize*: what a resize test needs is the
bytes **and** the size in force when each of them arrived.

So this re-issues `TIOCSWINSZ` mid-loop and cuts a step at each change,
writing two files:

    <name>.raw     the shell's bytes, exactly as `tests/recordings/`
                   stores them -- no framing, no header, no timestamps
    <name>.steps   one `COLSxROWS <bytelen>` per line: the size in force
                   and how many bytes of the `.raw` belong to it

Two files rather than one JSON blob on purpose. `baud-bench`'s `winch`
parses JSON with a hand-rolled reader whose fields all fall back to zero,
so a malformed capture replays as a `0x0` resize with no bytes instead of
failing; a fixture that gates CI has to fail loudly. The pair is also
diffable, and it keeps the bytes in the shape the rest of the corpus is
already in.

The sizes come from a file of `COLSxROWS` lines, which is what
`BAUD_DEBUG_RESIZE=1` already prints -- so a real session's gesture can
be replayed verbatim. Several sizes on one line are a burst -- applied
with nothing drained between them, the shape the app produces -- and a
line reading `enter` presses Enter:

    grep '^resize asked' baud-resize.log | awk '{print $5}' > drag.sizes
    python3 scripts/capture-resize.py --sizes drag.sizes \\
        --out tests/resize/drag -- /bin/zsh -i

Usage:
    python3 scripts/capture-resize.py --sizes <file> --out <prefix> [--] <cmd>...
"""

from __future__ import annotations

import argparse
import fcntl
import os
import pty
import select
import signal
import struct
import sys
import termios
import time


def set_size(fd: int, cols: int, rows: int) -> None:
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))


def drain(fd: int, seconds: float) -> bytes:
    """Everything the child writes within `seconds`.

    Not "until it goes quiet": the point of a resize capture is what the
    shell managed to emit in the time a gesture gave it, so the window is
    fixed and a shell that is still redrawing when it closes is the case
    under test rather than a capture to retry.
    """
    out = bytearray()
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        ready, _, _ = select.select([fd], [], [], 0.004)
        if not ready:
            continue
        try:
            chunk = os.read(fd, 65536)
        except OSError:
            break
        if not chunk:
            break
        out += chunk
    return bytes(out)


# One line of the sizes file: a burst of sizes applied with nothing drained
# between them, or a key the child is sent.
#
# **A burst is the shape the app produces and a per-size drain cannot.**
# AppKit delivers several `setFrameSize:` callbacks between two display-link
# ticks; each is a `TIOCSWINSZ` with nothing fed, and the shell's redraws
# for all of them land together afterwards -- the first computed for a
# width the screen has already left. `COLS x ROWS COLS x ROWS ...` on one
# line is that: every size set back to back, then one drain, recorded as
# zero-byte steps and one step with the bytes. `enter` writes a carriage
# return and drains, so a fixture can carry what the owner does between
# resizes.
Line = tuple[str, list[tuple[int, int]]]


def read_sizes(path: str) -> list[Line]:
    lines: list[Line] = []
    for n, line in enumerate(open(path, encoding="utf-8"), 1):
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        if line == "enter":
            lines.append(("enter", []))
            continue
        burst = []
        for word in line.split():
            if "x" not in word:
                sys.exit(f"{path}:{n}: expected COLSxROWS or `enter`, got {line!r}")
            c, _, r = word.partition("x")
            try:
                burst.append((int(c), int(r)))
            except ValueError:
                sys.exit(f"{path}:{n}: expected COLSxROWS, got {word!r}")
        lines.append(("sizes", burst))
    if not any(kind == "sizes" for kind, _ in lines):
        sys.exit(f"{path}: no sizes, so there is nothing to capture")
    return lines


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--sizes", required=True, help="file of COLSxROWS lines")
    ap.add_argument("--out", required=True, help="output prefix: writes .raw and .steps")
    ap.add_argument("--settle", type=float, default=2.5, help="seconds before the first resize")
    ap.add_argument("--step", type=float, default=0.05, help="seconds a step gives the shell")
    ap.add_argument("--tail", type=float, default=2.0, help="seconds after the last resize")
    ap.add_argument(
        "--gap",
        type=float,
        default=0.02,
        help="seconds between the sizes of a burst, with nothing drained: a display-link "
        "tick apart, so the shell redraws once per size and every redraw but the last is "
        "computed for a width the screen has left. Zero coalesces them into one redraw.",
    )
    ap.add_argument("cmd", nargs=argparse.REMAINDER)
    args = ap.parse_args()
    cmd = [a for a in args.cmd if a != "--"]
    if not cmd:
        sys.exit("give a command after --, e.g. -- /bin/zsh -i")

    lines = read_sizes(args.sizes)
    first = next(burst[0] for kind, burst in lines if kind == "sizes")
    pid, fd = pty.fork()
    if pid == 0:
        os.environ.setdefault("TERM", "xterm-256color")
        os.execvp(cmd[0], cmd)

    steps: list[tuple[int, int, bytes]] = []
    set_size(fd, *first)
    # The shell's own startup, at the first size: a fixture that began
    # mid-prompt would be a recording of a terminal nobody has.
    steps.append((first[0], first[1], drain(fd, args.settle)))
    at = first
    for kind, burst in lines:
        if kind == "enter":
            os.write(fd, b"\r")
            steps.append((at[0], at[1], drain(fd, args.step)))
            continue
        # Every size of the burst before any drain: the sizes but the last
        # are steps with no bytes, which is what the app's trace records.
        for cols, rows in burst[:-1]:
            set_size(fd, cols, rows)
            steps.append((cols, rows, b""))
            time.sleep(args.gap)
        at = burst[-1]
        set_size(fd, *at)
        steps.append((at[0], at[1], drain(fd, args.step)))
    steps.append((at[0], at[1], drain(fd, args.tail)))

    try:
        os.kill(pid, signal.SIGTERM)
        time.sleep(0.3)
        os.kill(pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    os.close(fd)

    raw = args.out + ".raw"
    stp = args.out + ".steps"
    with open(raw, "wb") as f:
        for _, _, b in steps:
            f.write(b)
    with open(stp, "w", encoding="utf-8") as f:
        f.write("# COLSxROWS <bytes of the .raw that arrived at that size>\n")
        for cols, rows, b in steps:
            f.write(f"{cols}x{rows} {len(b)}\n")
    total = sum(len(b) for _, _, b in steps)
    print(f"{raw}: {total} bytes over {len(steps)} steps")
    print(f"{stp}: {len(steps)} steps")


if __name__ == "__main__":
    main()
