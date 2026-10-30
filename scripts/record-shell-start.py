#!/usr/bin/env python3
"""Record a login shell's start through a pty, with each read's time.

The shell is started much as baud starts it (note AJ, section 18):
`$SHELL` as a login shell (argv[0] `-zsh` for zsh) in the home directory,
on a 120 by 36 pty, with `TERM` xterm-256color, `COLORTERM` truecolor and
`TERM_PROGRAM` baud. The rest of the caller's environment is kept,
including the outer terminal's own variables, which baud's spawn removes
(`OUTER` in baud-app/src/pty.rs). It is read for the given seconds and then sent `exit`.

Usage: record-shell-start.py <out> [seconds]

<out> holds each read as a little-endian f64 of seconds since the fork, a
u32 length and the bytes: what `cargo bench -p baud-bench --bench
splash_shell -- <out>` replays under the launch logo. Each read's time and
length is printed; the bytes are not, and the file should stay on the
machine it was made on, because a shell's output names the machine and
the user.
"""

import fcntl
import os
import pty
import select
import signal
import struct
import sys
import termios
import time

if len(sys.argv) < 2:
    sys.exit(__doc__)
out = sys.argv[1]
seconds = float(sys.argv[2]) if len(sys.argv) > 2 else 3.0
shell = os.environ.get("SHELL", "/bin/zsh")
env = dict(os.environ)
env.update(TERM="xterm-256color", COLORTERM="truecolor", TERM_PROGRAM="baud")
start = time.monotonic()
pid, fd = pty.fork()
if pid == 0:
    os.chdir(os.environ.get("HOME", "/"))
    os.execve(shell, ["-" + os.path.basename(shell)], env)
fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 36, 120, 0, 0))
reads = []
end = start + seconds
while (left := end - time.monotonic()) > 0:
    ready, _, _ = select.select([fd], [], [], left)
    if not ready:
        continue
    try:
        b = os.read(fd, 4096)
    except OSError:
        break
    if not b:
        break
    reads.append((time.monotonic() - start, b))
# The shell's last output is read and dropped: a shell exits only once the
# tty has drained what it wrote, and closing the pty first ends it.
os.write(fd, b"exit\r")
end = time.monotonic() + 2.0
while (left := end - time.monotonic()) > 0:
    ready, _, _ = select.select([fd], [], [], left)
    try:
        if ready and not os.read(fd, 4096):
            break
    except OSError:
        break
os.close(fd)
try:
    os.kill(pid, signal.SIGKILL)
except ProcessLookupError:
    pass
os.waitpid(pid, 0)
with open(out, "wb") as f:
    for at, b in reads:
        f.write(struct.pack("<dI", at, len(b)))
        f.write(b)
for at, b in reads:
    print(f"{at * 1e3:9.1f} ms {len(b):6d} bytes")
