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

With `--steps`, each `read(2)` is also written as a `COLSxROWS <bytes>`
line, the format of `tests/resize/`'s `.steps`, and each key sent as a
`# sent <key>` line before the first read after it, so a replay can feed
the bytes at the boundaries they arrived at and say which key each read
answered.

With `--answer QUERY=REPLY`, as Python escape strings, each `QUERY` the
program writes is answered with `REPLY`, as a terminal answers it: fish
waits on its `DA1` before it draws a prompt. Give the replies baud's
core gives (`Term::reply`), so the capture is the session baud would
have had.
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
    # What to split --keys on, so that each piece is written whole. A mouse
    # report sent a byte a delay arrives in pieces, and the program reads its
    # ESC as the Escape key.
    ap.add_argument("--key-sep", default="")
    # Seconds before the first key, --key-delay when not given. A program that
    # waits out its start-up queries reads the keys sent before then in one
    # burst.
    ap.add_argument("--first-key", type=float, default=None)
    # Where to write each read's length, one `COLSxROWS <bytes>` a line.
    ap.add_argument("--steps", default=None)
    ap.add_argument("--answer", action="append", default=[])
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

    raw = args.keys.encode().decode("unicode_escape").encode("latin-1")
    if args.key_sep:
        keys = [k for k in raw.split(args.key_sep.encode()) if k]
    else:
        keys = [raw[i : i + 1] for i in range(len(raw))]
    first = args.key_delay if args.first_key is None else args.first_key
    next_key = time.monotonic() + first
    ki = 0

    def esc(s: str) -> bytes:
        return s.encode().decode("unicode_escape").encode("latin-1")

    answers = [tuple(esc(x) for x in a.split("=", 1)) for a in args.answer]
    answered = [0] * len(answers)
    seen = b""

    sel = selectors.DefaultSelector()
    sel.register(master, selectors.EVENT_READ)
    end = time.monotonic() + args.seconds
    total = 0
    steps: list[str] = []
    with open(args.out, "wb") as f:
        while time.monotonic() < end:
            if ki < len(keys) and time.monotonic() >= next_key:
                os.write(master, keys[ki])
                steps.append(f"# sent {keys[ki]!r}")
                ki += 1
                next_key = time.monotonic() + args.key_delay
            # Wake for the next key as well as for output, so a key is not
            # sent up to a whole timeout late.
            wait = 0.2 if ki >= len(keys) else max(0.0, min(0.2, next_key - time.monotonic()))
            for _ in sel.select(timeout=wait):
                try:
                    chunk = os.read(master, 65536)
                except OSError:
                    chunk = b""
                if not chunk:
                    end = 0  # child closed the pty
                    break
                f.write(chunk)
                total += len(chunk)
                steps.append(f"{args.cols}x{args.rows} {len(chunk)}")
                seen += chunk
                for i, (q, r) in enumerate(answers):
                    while seen.count(q) > answered[i]:
                        os.write(master, r)
                        answered[i] += 1
                        steps.append(f"# answered {q!r} with {r!r}")

    os.kill(pid, signal.SIGTERM)
    time.sleep(0.3)
    try:
        os.kill(pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    os.waitpid(pid, os.WNOHANG)
    os.close(master)

    if args.steps:
        with open(args.steps, "w") as f:
            f.write("# COLSxROWS <bytes of the .raw that one read returned>\n")
            f.write("".join(f"{line}\n" for line in steps))
    print(f"{total} bytes -> {args.out}")
    return 0 if total else 1


if __name__ == "__main__":
    raise SystemExit(main())
