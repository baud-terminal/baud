#!/usr/bin/env python3
"""How the app's frames reached the screen, from a compositor's
WAYLAND_DEBUG=server log of one session (scripts/linux-frames.sh drives
it): what each commit on the window's surface carried and what the
compositor's presentation feedback said of it.

  linux-frames.py <traffic.log> <compositor.txt> <on|off>

compositor.txt is wayland-info's lines for the four globals, which say
what the compositor offered; on or off is the session's `[window]
tearing`. libwayland prints a message as `[ms] -> iface#id.name(args)`
for an event and without the arrow for a request (`@` in place of `#`
before 1.22), the time in milliseconds or, from weston, as a time of
day. The app is the one client in the log.

The checks, each "ok" or "FAILED":

  - presentation feedback arrives, and where the compositor predicts a
    refresh, every `presented` carries it;
  - where fifo-v1 is offered and tearing is off, every commit that
    attaches a buffer sets and waits on the barrier and no other commit
    does, and no frame is discarded -- a picture the barrier holds is
    never replaced unseen; where it is not, or tearing is on, none does;
  - where commit-timing-v1 is offered and tearing is off, at least one
    frame is stamped, and every stamp is half a refresh before a whole
    number of refreshes after a `presented` time the app had been sent
    (exactly, in nanoseconds): the app's frame clock is the compositor's
    feedback; where it is not, or tearing is on, nothing is stamped;
  - every stamped frame is presented no earlier than its stamp;
  - where tearing-control-v1 is offered, tearing on makes one control,
    hinted async before the window's first frame, and tearing off none.

It prints, besides, how many refreshes after its refresh each stamped
frame was shown and how many refreshes apart the frames presented were.
"""

import re
import sys
from collections import Counter

LINE = re.compile(r"^\[[\s\d:.]+\]\s+(?:\{[^}]*\}\s+)?(->\s+)?(\w+)[@#](\d+)\.(\w+)\((.*)\)\s*$")
ID = re.compile(r"(?:new id )?(\w+)[@#](\d+)")
ASYNC = 1


def ids(args):
    return [(m.group(1), m.group(2)) for m in ID.finditer(args)]


def nums(args):
    return [int(a) for a in args.split(", ") if a.strip().isdigit()]


def main():
    log, compositor, tearing = sys.argv[1], sys.argv[2], sys.argv[3]
    offered = open(compositor).read()
    fifo = "wp_fifo_manager_v1" in offered
    timing = "wp_commit_timing_manager_v1" in offered
    tear = "wp_tearing_control_manager_v1" in offered
    torn = tear and tearing == "on"

    window = None
    pending = {}
    commits = []
    owner = {}
    presented = []
    controls = 0
    hints = []
    failed = []

    def check(name, ok, said):
        print(f"{'ok' if ok else 'FAILED':8}{name}: {said}")
        if not ok:
            failed.append(name)

    for line in open(log, errors="replace"):
        m = LINE.match(line.rstrip("\n"))
        if not m:
            continue
        event, iface, obj, name, args = m.groups()
        if event:
            if iface == "wp_presentation_feedback" and obj in owner and name != "sync_output":
                c = commits[owner.pop(obj)]
                if name == "presented":
                    hi, lo, ns, refresh = nums(args)[:4]
                    t = ((hi << 32 | lo) * 1_000_000_000) + ns
                    c["shown"] = (t, refresh)
                    presented.append((t, refresh))
                elif name == "discarded":
                    c["shown"] = "discarded"
            continue
        if iface == "xdg_wm_base" and name == "get_xdg_surface" and window is None:
            window = ids(args)[1][1]
        elif iface == "wl_surface" and obj == window and name == "attach":
            pending["attach"] = not args.startswith("nil")
        elif iface == "wp_presentation" and name == "feedback":
            pending["feedback"] = ids(args)[1][1]
        elif iface == "wp_fifo_v1":
            pending[name] = True
        elif iface == "wp_commit_timer_v1" and name == "set_timestamp":
            hi, lo, ns = nums(args)
            pending["stamp"] = ((hi << 32 | lo) * 1_000_000_000) + ns
            pending["known"] = list(presented[-3:])
        elif iface == "wp_tearing_control_manager_v1" and name == "get_tearing_control":
            controls += 1
        elif iface == "wp_tearing_control_v1" and name == "set_presentation_hint":
            hints.append((nums(args)[0], len(commits)))
        elif iface == "wl_surface" and obj == window and name == "commit":
            pending.setdefault("attach", False)
            commits.append(pending)
            if "feedback" in pending:
                owner[pending["feedback"]] = len(commits) - 1
            pending = {}

    frames = [c for c in commits if "feedback" in c]
    shown = [c for c in frames if isinstance(c.get("shown"), tuple)]
    discarded = [c for c in frames if c.get("shown") == "discarded"]
    stamped = [c for c in commits if "stamp" in c]
    print(
        f"        {len(commits)} commits on the window's surface, {len(frames)} asking"
        f" feedback, {sum(c['attach'] for c in frames)} with a buffer;"
        f" {len(shown)} presented, {len(discarded)} discarded, {len(stamped)} stamped"
    )

    refreshes = sorted({r for _, r in presented})
    check(
        "presentation feedback arrives",
        len(presented) >= 10,
        f"{len(presented)} presented, refresh {refreshes} ns",
    )
    if timing:
        check(
            "every presented frame says the refresh",
            presented and all(r > 0 for _, r in presented),
            f"refresh {refreshes} ns",
        )

    barred = [c for c in commits if c.get("set_barrier") or c.get("wait_barrier")]
    if fifo and not torn:
        wrong = [
            c
            for c in commits
            if bool(c.get("set_barrier")) != c["attach"] or bool(c.get("wait_barrier")) != c["attach"]
        ]
        check(
            "every commit with a buffer sets and waits on the barrier, and only those",
            not wrong and barred,
            f"{len(barred)} with the barrier, {len(wrong)} wrong",
        )
        check("no frame is discarded", not discarded, f"{len(discarded)} discarded")
    else:
        check("no commit carries the barrier", not barred, f"{len(barred)} with it")

    if timing and not torn:
        off = []
        for c in stamped:
            s = c["stamp"]
            fits = False
            for p, r in c["known"]:
                at = s + r // 2 - p
                if r and at > 0 and at % r == 0:
                    fits = True
            if not fits:
                off.append(c)
        check(
            "every stamp is half a refresh before a refresh counted from the feedback",
            stamped and not off,
            f"{len(stamped)} stamped, {len(off)} off the feedback's grid",
        )
    else:
        check("no frame is stamped", not stamped, f"{len(stamped)} stamped")

    early = [c for c in stamped if isinstance(c.get("shown"), tuple) and c["shown"][0] < c["stamp"]]
    check("no stamped frame is shown before its stamp", not early, f"{len(early)} early")

    if tear:
        if torn:
            first = min((i for i, c in enumerate(commits) if "feedback" in c), default=None)
            ok = controls == 1 and hints and hints[0][0] == ASYNC and first is not None and hints[0][1] <= first
            check("tearing on: one control, hinted async before the first frame", ok, f"{controls} controls, hints {[h for h, _ in hints]}")
        else:
            check("tearing off: no control", controls == 0 and not hints, f"{controls} controls")

    if (not fifo or torn) and discarded:
        print(f"        {len(discarded)} discarded, with no barrier to hold them")
    late = Counter()
    for c in stamped:
        if isinstance(c.get("shown"), tuple):
            t, r = c["shown"]
            late[round((t - (c["stamp"] + r // 2)) / r)] += 1
    if late:
        print("        stamped frames shown n refreshes after their refresh: " + ", ".join(f"{k}: {v}" for k, v in sorted(late.items())))
    gaps = Counter()
    for (a, r), (b, _) in zip(presented, presented[1:]):
        if r:
            gaps[round((b - a) / r)] += 1
    if gaps:
        print("        frames presented n refreshes apart: " + ", ".join(f"{k}: {v}" for k, v in sorted(gaps.items())))
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
