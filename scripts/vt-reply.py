"""Reads the terminal's reply to a query from the tty: everything that
arrives until the expected reply -- `ESC [ row ; col R` for the row and
column given, which the child put the cursor at first, so a reply to a
query the *recording* made (vim's asks for the cursor position and the
device attributes on every replay) cannot be mistaken for it -- or until
`timeout` seconds pass, and prints it as hex, or `none`. A shell `read`
cannot do this portably: bash 3.2's `read -t` discards what it had read
when it times out, and a killed `od` never flushes."""
import os
import select
import sys
import time

fd = sys.stdin.fileno()
buf = b""
timeout = float(sys.argv[1]) if len(sys.argv) > 1 else 10.0
want = f"\x1b[{sys.argv[2]};{sys.argv[3]}R".encode() if len(sys.argv) > 3 else b"R"
end = time.time() + timeout
while time.time() < end:
    ready, _, _ = select.select([fd], [], [], 0.05)
    if ready:
        buf += os.read(fd, 256)
        if want in buf:
            break
print(buf.hex() or "none")
