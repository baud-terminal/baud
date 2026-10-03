#!/usr/bin/env python3
"""The production path, end to end: launch the built baud the way the Dock
does -- with launchd's environment, which names no locale -- and check what
its shell was given. No unit test can: every test process inherits the
runner's own LANG, so `Pty::fork`'s read of the user's locale never runs
there, and a `Pty::fork` that stopped asking would leave every test green.

`BAUD_CMD` writes the shell's `$LANG` and the number of characters `wc -m`
counts in one Hebrew letter (U+05D9, two bytes) to a file; `BAUD_STARTUP`
makes `shell_locale` print its `startup: locale:` note when it is asked.

Arms, each a fresh launch with only what launchd gives an app (HOME, USER,
LOGNAME, TMPDIR, __CF_USER_TEXT_ENCODING, a system PATH, SHELL):

  dock     no locale variable: LANG is a UTF-8 name, the letter is one
           character, and the note is printed (the read ran);
  empty    LANG= set empty: replaced as in `dock`, never doubled;
  control  LANG=fr_FR.UTF-8: kept as given, the letter one character, and
           no note (the read was skipped).

Exits 1, printing what each arm saw, when any arm disagrees.

  scripts/locale/dock_launch_check.py [binary]   (default target/release/baud)
"""
import os
import re
import signal
import subprocess
import sys
import tempfile
import time

BINARY = sys.argv[1] if len(sys.argv) > 1 else "target/release/baud"
LAUNCHD = ("HOME", "USER", "LOGNAME", "TMPDIR", "__CF_USER_TEXT_ENCODING")
NOTE = re.compile(r"^startup: locale: (\S+) in ([\d.]+) ms$")


def launch(extra, out_dir):
    out = os.path.join(out_dir, "seen")
    env = {k: os.environ[k] for k in LAUNCHD if k in os.environ}
    env.update(
        {
            "PATH": "/usr/bin:/bin:/usr/sbin:/sbin",
            "SHELL": "/bin/zsh",
            "BAUD_STARTUP": "1",
            "BAUD_GRID": "80x24",
            # Written whole and renamed, so a half-written file is never read.
            "BAUD_CMD": (
                f"printf '%s|' \"$LANG\" > '{out}.part'; "
                f"printf '\\327\\231' | /usr/bin/wc -m >> '{out}.part'; "
                f"mv '{out}.part' '{out}'; sleep 1"
            ),
        }
    )
    env.update(extra)
    p = subprocess.Popen(
        [BINARY], env=env, stderr=subprocess.PIPE, stdout=subprocess.DEVNULL, text=True
    )
    deadline = time.monotonic() + 20
    while not os.path.exists(out) and time.monotonic() < deadline and p.poll() is None:
        time.sleep(0.05)
    if p.poll() is None:
        p.send_signal(signal.SIGTERM)
    try:
        _, err = p.communicate(timeout=10)
    except subprocess.TimeoutExpired:
        p.kill()
        _, err = p.communicate()
    seen = open(out).read() if os.path.exists(out) else None
    notes = [m.groups() for m in map(NOTE.match, err.splitlines()) if m]
    return seen, notes


def main():
    if not os.access(BINARY, os.X_OK):
        sys.exit(f"no binary at {BINARY}: build it with `cargo build --release -p baud-app`")
    failures = []
    for arm, extra in [("dock", {}), ("empty", {"LANG": ""}), ("control", {"LANG": "fr_FR.UTF-8"})]:
        with tempfile.TemporaryDirectory() as d:
            seen, notes = launch(extra, d)
        lang, _, count = (seen or "").partition("|")
        count = count.strip()
        if arm == "control":
            ok = lang == "fr_FR.UTF-8" and count == "1" and not notes
        else:
            ok = (
                lang.endswith(".UTF-8")
                and count == "1"
                and len(notes) == 1
                and notes[0][0] == lang
            )
        print(f"{'ok  ' if ok else 'FAIL'} {arm:<8} LANG={lang!r} wc -m={count!r} notes={notes}")
        if not ok:
            failures.append(arm)
    if failures:
        sys.exit(f"failed: {', '.join(failures)}")


main()
