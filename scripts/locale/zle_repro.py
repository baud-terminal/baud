#!/usr/bin/env python3
"""Reproduce what zsh's line editor echoes for typed Hebrew, with and without
a UTF-8 locale, in a pty opened the way baud opens one (forkpty with a NULL
termios; CPython's os.forkpty passes NULL for name and termp).

The child environment imitates what launchd hands a Dock-launched app
(measured with `launchctl print gui/501/<service>`), plus the four variables
baud sets (baud-app/src/pty.rs OURS). zsh runs with -f (no rc files, so the
owner's dotfiles are neither read nor written) and -i (interactive, so ZLE
runs). Nothing is written to any history file: -f leaves HISTFILE unset.
"""
import os, pty, select, sys, time

SHELLS = ["/bin/zsh", "/etc/profiles/per-user/user/bin/zsh"]
BASE = {
    "HOME": os.environ["HOME"],
    "USER": "user",
    "LOGNAME": "user",
    "PATH": "/usr/bin:/bin:/usr/sbin:/sbin",
    "SHELL": "/bin/zsh",
    "TMPDIR": os.environ.get("TMPDIR", "/tmp"),
    "__CF_USER_TEXT_ENCODING": "0x1F5:0x0:0x0",
    "COMMAND_MODE": "unix2003",
    # baud's own four (pty.rs:185-191)
    "TERM": "xterm-256color",
    "COLORTERM": "truecolor",
    "TERM_PROGRAM": "baud",
    "TERM_PROGRAM_VERSION": "1.0.0-beta.1",
    "PS1": "PROMPT> ",
}
ARMS = [
    ("launchd-like, no LANG", {}),
    ("LANG=en_US.UTF-8 (positive control)", {"LANG": "en_US.UTF-8"}),
]
TYPED = [("yod U+05D9", b"\xd7\x99"), ("het U+05D7", b"\xd7\x97")]


def drain(fd, until=None, timeout=3.0):
    out = b""
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        r, _, _ = select.select([fd], [], [], max(0.0, end - time.monotonic()))
        if not r:
            break
        try:
            chunk = os.read(fd, 4096)
        except OSError:
            break
        if not chunk:
            break
        out += chunk
        if until is not None and until in out:
            # keep reading briefly for trailing bytes
            end = min(end, time.monotonic() + 0.3)
    return out


def run(shell, extra):
    env = dict(BASE)
    env.update(extra)
    pid, fd = pty.fork()
    if pid == 0:
        os.execve(shell, ["zsh", "-f", "-i"], env)
    try:
        pre = drain(fd, until=b"PROMPT> ", timeout=5.0)
        results = []
        for name, seq in TYPED:
            os.write(fd, seq)
            echo = drain(fd, timeout=1.0)
            results.append((name, seq, echo))
        # print the codeset the shell sees: kill the line, then a command
        os.write(fd, b"\x15")
        drain(fd, timeout=0.5)
        os.write(fd, b"zmodload zsh/langinfo; print -r -- CODESET=$langinfo[CODESET] LANG=${LANG-unset} MB=$options[multibyte]\r")
        info = drain(fd, until=b"MB=", timeout=2.0)
        os.write(fd, b"stty -a\r")
        stty = drain(fd, until=b"PROMPT> ", timeout=2.0)
        os.write(fd, b"exit\r")
        drain(fd, timeout=1.0)
    finally:
        try:
            os.close(fd)
        except OSError:
            pass
        os.waitpid(pid, 0)
    return pre, results, info, stty


for shell in SHELLS:
    for label, extra in ARMS:
        pre, results, info, stty = run(shell, extra)
        print(f"=== {shell} | {label}")
        print(f"  prompt seen: {b'PROMPT> ' in pre}")
        for name, seq, echo in results:
            print(f"  typed {name} {seq.hex(' ')} -> echoed {echo!r}")
        line = [l for l in info.decode('utf-8', 'replace').splitlines() if 'CODESET=' in l and 'print' not in l]
        print(f"  shell reports: {line}")
        flags = [w for w in stty.decode('latin-1').split() if w.lstrip('-') in ('iutf8', 'icanon', 'echo', 'ixany', 'imaxbel')]
        print(f"  termios flags: {flags}")
