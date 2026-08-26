#!/usr/bin/env python3
"""kitty's VT parser, through a real PTY, on the same bytes as multibench.

kitty cannot be a Rust core in multibench. Its parser is a CPython object --
`Parser` is declared `PyObject_HEAD` in kitty/vt-parser.h, and
`alloc_vt_parser(id_type window_id)` resolves its dispatch target through
kitty's global window registry -- so there is nothing to link against.

What it does expose is the same buffer-protocol cycle baud uses, via the
`test_*` methods its own test suite drives the parser with:

    dest = screen.test_create_write_buffer()      ~ baud: t.read_slot()
    n    = screen.test_commit_write_buffer(d, dest) ~ baud: slot.write(...)
    screen.test_parse_written_data()              ~ baud: t.feed(); t.commit_read()

So this is kitty's real parser over its real buffer protocol. The cost that is
*not* kitty's is three Python calls per 4096-byte chunk, which is why the
`ceiling` row exists: it runs the identical PTY loop with a sink that does
nothing. Any kitty number is bounded by it, and the gap between them is the
only part that is kitty.

The payloads are not generated here. multibench writes the exact bytes it
measures with:

    DUMP_PAYLOADS=<dir> cargo run --release -p multibench

so "same payloads" is a fact rather than two generators that agree today.
"""

import os
import signal
import statistics
import sys
import time
import tty

CHUNK = 4096
COLS, ROWS = 200, 50
TOTAL = 32 << 20
REPS = 9


def through_pty(payload: bytes, total: int, sink) -> float:
    """Fork a writer on a PTY slave, hand the master to `sink` CHUNK bytes at a time.

    Deliberately the same shape as multibench's through_pty: same fork, same
    raw-mode slave, same read size, same "stop once `total` bytes have arrived".
    """
    m, s = os.openpty()
    tty.setraw(s)

    pid = os.fork()
    if pid == 0:
        os.close(m)
        try:
            sent = 0
            while sent < total:
                o = 0
                while o < len(payload):
                    o += os.write(s, payload[o:])
                sent += len(payload)
        except OSError:
            pass
        finally:
            os._exit(0)

    os.close(s)
    got = 0
    t0 = time.monotonic()
    while True:
        try:
            b = os.read(m, CHUNK)
        except OSError:
            break
        if not b:
            break
        sink(b)
        got += len(b)
        if got >= total:
            break
    dt = time.monotonic() - t0

    os.kill(pid, signal.SIGKILL)
    os.waitpid(pid, 0)
    os.close(m)
    return got / dt / 1048576.0


def stat(n, f):
    """Median and range, matching multibench's `stat` and baud-bench's reporting."""
    v = sorted(f() for _ in range(n))
    return v[len(v) // 2], v[0], v[-1]


class Callbacks:
    """Absorbs whatever the parser dispatches.

    Anything the parser reports -- title changes, bells, clipboard, DA replies --
    is work a real kitty would do downstream of parsing and is not what this
    measures. A no-op for every name keeps that cost out without needing to
    enumerate the callback surface, which changes between kitty versions.
    """

    def __getattr__(self, name):
        return lambda *a, **k: None


def make_screen():
    from kitty.fast_data_types import Screen

    c = Callbacks()
    # (callbacks, lines, columns, scrollback, cell_width, cell_height,
    #  window_id, test_callbacks) -- as kitty_tests/base.py:create_screen.
    s = Screen(c, ROWS, COLS, 10_000, 10, 20, 0, c)
    c.color_profile = s.color_profile
    return s


def kitty_sink(screen):
    def sink(chunk):
        data = memoryview(chunk)
        while data:
            dest = screen.test_create_write_buffer()
            n = screen.test_commit_write_buffer(data, dest)
            data = data[n:]
            screen.test_parse_written_data()

    return sink


def main():
    if len(sys.argv) < 2:
        sys.exit(f"usage: {sys.argv[0]} <payload-dir>   (see DUMP_PAYLOADS above)")
    d = sys.argv[1]
    with open(os.path.join(d, "stream.bin"), "rb") as fh:
        stream = fh.read()
    with open(os.path.join(d, "tui.bin"), "rb") as fh:
        tui = fh.read()

    print(f"  kitty via Screen.test_* -- same payloads, same real PTY, "
          f"{CHUNK}-byte reads, median of {REPS}")
    print(f"  stream.bin {len(stream)} B, tui.bin {len(tui)} B\n")
    print(f"  {'core':<28} {'streaming median [min-max]':>21}  {'TUI median [min-max]':>21}")

    def row(label, s, t):
        print(f"  {label:<28} {s[0]:>7.1f} [{s[1]:>5.1f}-{s[2]:>5.1f}]  "
              f"{t[0]:>7.1f} [{t[1]:>5.1f}-{t[2]:>5.1f}]")

    # The harness ceiling. Not a terminal -- the PTY read loop with a sink that
    # discards. kitty cannot exceed this, and the distance to it is the Python.
    f = lambda p: stat(REPS, lambda: through_pty(p, TOTAL, lambda _c: None))  # noqa: E731
    row("ceiling (discard, no parse)", f(stream), f(tui))

    f = lambda p: stat(REPS, lambda: through_pty(p, TOTAL, kitty_sink(make_screen())))  # noqa: E731
    row("kitty (Screen.test_*)", f(stream), f(tui))


if __name__ == "__main__":
    main()
