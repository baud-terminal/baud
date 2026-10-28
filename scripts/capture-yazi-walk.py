#!/usr/bin/env python3
"""Record yazi walking down a directory of pictures and back, as a terminal
with Sixel graphics sees it.

    python3 scripts/capture-yazi-walk.py --out yazi-walk.raw -- yazi

`tests/recordings/sixel/yazi-walk.raw` is this script's output. yazi asks
what the terminal is before it draws a preview, and draws none for a
terminal that does not answer, so `capture-tui.py`, which answers nothing,
cannot record one. This answers yazi's two bursts of queries as a VT102
with Sixel graphics and a 17x34-pixel cell would, the answers being the
ones the recording it replaces was given:

    after the first DA1:  DCS > | baud ST, CSI 6 ; 34 ; 17 t,
                          OSC 11 ; rgb:1010/1010/1010 BEL, CSI ? 6 ; 4 c
    after the second:     DCS 1 $ r 1 SP q ST, CSI ? 12 ; 2 $ y,
                          CSI ? 0 u, CSI ? 6 ; 4 c

Then it presses `j` six times and `k` six times, each key once the preview
before it has arrived whole (its Sixel's ST) and yazi has been quiet for
--quiet seconds, so every key draws its own preview: thirteen in all. The
window is --rows by --cols. The bytes are written exactly as the pty
returned them; nothing yazi sent is answered but those two bursts.
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

DA1 = b"\x1b[0c"
REPLIES = (
    b"\x1bP>|baud\x1b\\\x1b[6;34;17t\x1b]11;rgb:1010/1010/1010\x07\x1b[?6;4c",
    b"\x1bP1$r1 q\x1b\\\x1b[?12;2$y\x1b[?0u\x1b[?6;4c",
)
KEYS = b"jjjjjjkkkkkk"
SIXEL = b"\x1bP9;1q"
ST = b"\x1b\\"


def finished_previews(out: bytes) -> int:
    """How many Sixel images in `out` have arrived up to their ST."""
    n, at = 0, 0
    while (i := out.find(SIXEL, at)) >= 0:
        j = out.find(ST, i)
        if j < 0:
            break
        n, at = n + 1, j + 2
    return n


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", required=True)
    ap.add_argument("--rows", type=int, default=40)
    ap.add_argument("--cols", type=int, default=117)
    ap.add_argument("--quiet", type=float, default=0.5)
    ap.add_argument("--limit", type=float, default=120.0, help="seconds before giving up")
    ap.add_argument("cmd", nargs=argparse.REMAINDER)
    args = ap.parse_args()
    cmd = [c for c in args.cmd if c != "--"]
    if not cmd:
        sys.exit("give a command after --, e.g. -- yazi")

    pid, fd = pty.fork()
    if pid == 0:
        os.execvp(cmd[0], cmd)
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", args.rows, args.cols, 0, 0))

    out = bytearray()
    answered = 0
    keys = 0
    last = time.monotonic()
    start = last
    log = []
    while time.monotonic() - start < args.limit:
        ready, _, _ = select.select([fd], [], [], 0.02)
        if ready:
            try:
                chunk = os.read(fd, 65536)
            except OSError:
                break
            if not chunk:
                break
            out += chunk
            last = time.monotonic()
        # A burst is answered once its DA1 has arrived: the queries before
        # it are the ones the answer covers.
        if answered < len(REPLIES) and out.count(DA1) > answered:
            os.write(fd, REPLIES[answered])
            log.append((len(out), REPLIES[answered]))
            answered += 1
            continue
        if ready or time.monotonic() - last < args.quiet:
            continue
        done = finished_previews(bytes(out))
        if keys < len(KEYS) and done == keys + 1:
            os.write(fd, KEYS[keys : keys + 1])
            log.append((len(out), KEYS[keys : keys + 1]))
            keys += 1
        elif keys == len(KEYS) and done == len(KEYS) + 1:
            break
    else:
        print(f"gave up after {args.limit} s: {finished_previews(bytes(out))} previews", file=sys.stderr)

    try:
        os.kill(pid, signal.SIGTERM)
        time.sleep(0.3)
        os.kill(pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    os.close(fd)
    with open(args.out, "wb") as f:
        f.write(out)
    for at, sent in log:
        print(f"{at}\t{sent!r}")
    print(f"{len(out)} bytes, {finished_previews(bytes(out))} previews -> {args.out}")
    return 0 if finished_previews(bytes(out)) == len(KEYS) + 1 else 1


if __name__ == "__main__":
    sys.exit(main())
