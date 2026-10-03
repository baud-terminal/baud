"""Does one ERASE in canonical mode remove a whole UTF-8 character, with and
without IUTF8, on this kernel's pty? forkpty with NULL termios, as baud does.
The child turns echo off so only its own report comes back, then reads one
line and prints its bytes in hex between markers."""
import os, pty, select, sys
SCRIPT = r'''stty -echo %s; printf 'READY\n'; IFS= read -r l; printf 'GOT[%%s]\n' "$(printf '%%s' "$l" | od -An -tx1 | tr -d ' \n')"; stty -a | tr ' ' '\n' | grep -E '^-?iutf8$' '''
def run(flag, typed):
    pid, fd = pty.fork()
    if pid == 0:
        os.execve("/bin/sh", ["sh", "-c", SCRIPT % flag], {"PATH": "/usr/bin:/bin", "HOME": "/nowhere"})
    out = b""
    def until(marker):
        nonlocal out
        while marker not in out:
            r, _, _ = select.select([fd], [], [], 10)
            if not r:
                raise SystemExit(f"deadline waiting for {marker!r}: {out!r}")
            try:
                chunk = os.read(fd, 4096)
            except OSError:
                break
            if not chunk:
                break
            out += chunk
    until(b"READY")
    os.write(fd, typed)
    until(b"iutf8")
    os.waitpid(pid, 0)
    got = out.split(b"GOT[")[1].split(b"]")[0].decode()
    tail = out.decode("latin-1").split("]")[-1].strip()
    return got, tail
for flag in ("", "-iutf8", "iutf8"):
    for label, typed in (("yod DEL newline", b"\xd7\x99\x7f\n"), ("control: ab DEL newline", b"ab\x7f\n")):
        got, tail = run(flag, typed)
        print(f"stty {flag or '(nothing: kernel default)':28s} typed {label:24s} -> line bytes [{got}]  flag {tail}")
