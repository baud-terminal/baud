#!/usr/bin/env python3
"""Capture a real TUI's escape stream through a real PTY.

Every payload baud has been measured against so far was written by the same
people who wrote baud. `baud-bench/src/payload.rs` says so itself: the
synthetic payloads want validating against real capture "before it is designed
around". This produces that capture.

`script(1)` is not enough on its own. Backgrounded it has no stdin, so the
child sees EOF and exits immediately -- 152 bytes of setup and nothing else --
and it has no window size to give, which a full-screen program needs before it
will draw anything. So the PTY is opened here and `TIOCSWINSZ` is set on it
explicitly.

Usage:
    python3 scripts/capture-tui.py --seconds 10 --out capture.raw -- btop
"""

from __future__ import annotations

import argparse
import fcntl
import os
import pty
import selectors
import signal
import struct
import sys
import termios
import time


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--seconds", type=float, default=10.0)
    ap.add_argument("--rows", type=int, default=30)
    ap.add_argument("--cols", type=int, default=100)
    ap.add_argument("--out", required=True)
    # Keys to drive the program with, as a Python escape string. Without this a
    # capture of an editor is one static screen; with it the stream contains
    # the scrolling, and scrolling is where DECSTBM, IL/DL and the scroll
    # region actually get used.
    ap.add_argument("--keys", default="")
    ap.add_argument("--key-delay", type=float, default=0.35)
    ap.add_argument("cmd", nargs=argparse.REMAINDER)
    args = ap.parse_args()

    cmd = [c for c in args.cmd if c != "--"]
    if not cmd:
        print("no command given", file=sys.stderr)
        return 2

    pid, master = pty.fork()
    if pid == 0:
        os.environ["TERM"] = os.environ.get("TERM", "xterm-256color")
        try:
            os.execvp(cmd[0], cmd)
        except OSError as e:
            print(f"exec {cmd[0]}: {e}", file=sys.stderr)
            os._exit(127)

    # The size the child draws for. Without this it sees 0x0 and many
    # full-screen programs refuse to render at all.
    fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", args.rows, args.cols, 0, 0))

    keys = args.keys.encode().decode("unicode_escape").encode("latin-1")
    next_key = time.monotonic() + args.key_delay
    ki = 0

    sel = selectors.DefaultSelector()
    sel.register(master, selectors.EVENT_READ)
    end = time.monotonic() + args.seconds
    total = 0
    with open(args.out, "wb") as f:
        while time.monotonic() < end:
            if ki < len(keys) and time.monotonic() >= next_key:
                os.write(master, keys[ki : ki + 1])
                ki += 1
                next_key = time.monotonic() + args.key_delay
            for _ in sel.select(timeout=0.2):
                try:
                    chunk = os.read(master, 65536)
                except OSError:
                    chunk = b""
                if not chunk:
                    end = 0  # child closed the pty
                    break
                f.write(chunk)
                total += len(chunk)

    os.kill(pid, signal.SIGTERM)
    time.sleep(0.3)
    try:
        os.kill(pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    os.waitpid(pid, os.WNOHANG)
    os.close(master)

    print(f"{total} bytes -> {args.out}")
    return 0 if total else 1


if __name__ == "__main__":
    raise SystemExit(main())
