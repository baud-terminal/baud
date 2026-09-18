#!/usr/bin/env python3
"""Measure keystroke-to-pixel latency for `baud`, leg by leg, as the app
times its own frames.

Row 0 of `docs/research/notes/n-make-faster.md` is an instrument, not a
change: without a number for typing, half the rows below it cannot be kept
or reverted. This script drives it.

WHAT IT MEASURES
    A helper (`scripts/key-post.swift`) posts plain letters as CGEvents
    stamped with `CLOCK_UPTIME_RAW` -- the same mach host-time clock the app
    reads for `NSEvent.timestamp`, the pty read, the GPU end, and the display
    link. The app, run under `BAUD_KEYLAT=1`, prints one `keylat:` line per
    key with an offset (microseconds) for each stage of the key's journey:

        keylat: seq=<n> event=<ns> handled=+<us> written=+<us> echo=+<us> \\
                fed=+<us> commit=+<us> landed=+<us> shown=+<us> \\
                vsync=+<us> vsync_hi=+<us>

    `event` is absolute nanoseconds (so a posted key can be aligned to its
    keylat line); every other field is an offset from `event`, or `-` for a
    leg that did not happen (the CPU path has no `landed`; a landing whose
    GPU end stamp is not on the clock is refused, `landed=-`, and the app
    prints `keylat: landing refused ended=<ns> now=<ns>` beside it; an
    expired record is emitted with the legs it has). The parser is STRICT
    about the known tokens -- a record missing any of them is a parse
    failure, counted and printed, never a silent `-` -- and tolerant of
    extra tokens a newer app may add. The helper prints one `jitter seed
    <pid> ...` line, then `post <seq> <ns> <char>` per key. Pairing the
    posts to the records by order recovers, per key, the legs:

        stamp    = event    - post     (0 if the system kept our timestamp,
                                        else the delivery time it re-stamped)
        deliver  = handled  - post     (post to the responder running)
        write    = written  - handled  (Term::key + Pty::write)
        child    = echo     - written  (the pty round trip: kernel + child)
        wake     = fed      - echo     (reader thread wake to core fed)
        build    = commit   - fed      (present: convert, encode, commit)
        gpu      = landed   - commit   (GPU start to end; - on the CPU path)
        show     = shown    - landed   (completion to surface on the layer)
        vsync    = vsync    - shown    (surface set to the EARLY vsync bound)
        vsync_hi = vsync_hi - shown    (surface set to the LATE vsync bound)
        to_shown = shown    - post     (post to the surface on the layer;
                                        carries NO vsync quantisation)
        total    = vsync    - post     (keystroke-to-pixel, early bound)
        total_hi = vsync_hi - post     (keystroke-to-pixel, late bound)

    `vsync` is the display-link target announced BEFORE the surface was set
    -- the vsync the frame makes if it met the compositor's deadline -- and
    `vsync_hi` is the target after it. `total` and `total_hi` are therefore
    a BRACKET, not a measurement: on a fixed-period panel with no dropped
    tick the pixel is composited somewhere in [vsync, vsync_hi], and
    `scripts/key-pixel.swift` is the probe that adjudicates where. Its
    adjudication is in the note, `docs/research/notes/o-keystroke-latency.md`,
    not here. Do not fold a probe run's totals into a sweep's: the probe's
    own screen capture perturbs the app -- the sweep measured post->shown
    at ~13 ms under capture against ~3 ms without, same child, same box --
    so a probe run measures the app under capture and its `total`s are
    that run's alone. `to_shown` is the A/B number: it ends where the
    app's own work ends and carries no display-period quantisation.

HOW TO READ IT
    Per arm: the median (with n) and the interquartile range [q1-q3] of each
    leg over the SETTLED rounds (round 1 is warm-up and discarded, per
    CLAUDE.md), the [min-max] beside it, plus keys posted (the helper's
    count) / paired / completed / excluded, and how many shared a frame.
    With `--aa` a second arm runs the SAME binary again; the two are printed
    side by side and the noise floor is given PER LEG as the larger of the
    two medians' difference and either arm's interquartile range -- never
    the max-min spread, which one fat-tail outlier owns (a single 163 ms
    echo once made it read 169 ms). A leg that moves by less than its own
    floor is not a result. Take it on mains, on an idle box; the header
    says which you had.

    With more than one non-`aa` arm the driver also prints, per leg,
    `arm - base` beside that A/A floor, and marks a row `result` only when
    |delta| exceeds the floor AND the two arms' interquartile ranges are
    disjoint; every other row says `not a result` and why (within the
    floor, or the IQRs overlap). Without `--aa` there is no floor and no
    row can be a result. This table is what rows 6, 9 and 11 of
    `docs/research/notes/n-make-faster.md` -- pace the GPU by completion
    for a keystroke, draw the echo before the pty round trip, skip the
    present when nothing changed -- will read.

    A key is `completed` when it paired and reached `vsync`; `excluded` when
    the app's `seq` is not the helper's, its `event` precedes its `post` or
    trails it by >= the pairing window (100 ms, or the MINIMUM jittered
    gap - 5 ms when that minimum is under 100 ms), any leg after `stamp`
    is negative (a stamp off the clock), or its landing was refused (see
    below). Legs with `-` simply do not feed their median, and each leg's
    n says how many did.

    Two exclusions look like defects and are not. A row with `child < 0`
    is the echo stamped INSIDE the write: the line discipline echoes the
    byte during the `write(2)` that carries it, so the reader thread can
    stamp the echo before the post-write stamp lands and `echo - written`
    comes out a few microseconds negative. That is a non-duration; the
    negative-leg rule excludes the row and the per-round `negative=` count
    says how many (2 of 240 keys on the first sweep, -3.6 and -1.1 us).
    And a run in which the app printed `keylat: landing refused` is NOT
    QUOTABLE: the GPU end stamp was off the clock, and a refused present
    can stamp `shown` on a record whose real frame has not presented yet,
    so a clean-looking low `to_shown` in such a run is not clean. Every
    row of that run that reached the GPU (`commit` set) and has no
    landing (`landed=-`) is excluded as `refused-landing`; the arm's
    summary and the sweep's last lines say NOT QUOTABLE and name the run.
    A run on the CPU path has `landed=-` on every row and no refusal
    line, and is untouched.

    The arm order rotates every round, so the A/A arm is not always last
    and a position artefact cannot pass as an arm. Each arm's path and
    sha256 are printed; two byte-identical arms are warned about unless one
    is `aa`, which is byte-identical by design.

    `--gap` is the MEAN inter-key gap. The helper sleeps a uniform random
    duration in [gap - gap/4, gap + gap/4] before each key after the first
    (a gap of 0 stays 0: typeahead), seeded from baud's pid, so a run is
    reproducible and the pid is printed per round. A FIXED gap can
    phase-lock to the refresh: `[[source: hume-latency-tester]]` drove a
    fixed inter-key delay and read a false SD of 1 ms, because every key
    then lands at the same point in the display period and the vsync term
    stops varying -- the noise floor reads smaller than it is, on exactly
    the legs (`vsync`, `total`) that carry the period. The `post` lines
    carry the real stamps, so nothing in the analysis changes with the
    jitter; the one parameter that follows it is the pairing window,
    which is set from the MINIMUM jittered gap so a one-key shift still
    cannot fit inside it.

    `--child cat` measures the tty's own echo: `cat` never reads a byte
    before Return, so the line discipline is the only thing that answers
    and there is no program in the loop. `--child shell` runs the login
    shell, what a user feels. `--child none` runs `stty -echo; sleep 60`,
    the no-echo control: every key is posted and none completes, and the
    sweep exits 1 to say so. (`sleep` alone is NOT a no-echo control --
    the line discipline echoes whatever the child does.)

EXIT CODES
    0  every run had completed keys (or --selftest passed)
    1  a run reported zero completed keys, or bad usage
    2  the Swift helper REFUSED (baud did not own the front window) -- the
       guard did its job; whatever it posted before refusing is paired
    3  --selftest failed
    4  the helper failed (an exit other than 0 or 3): its code and output
       are printed and the sweep is aborted

    scripts/key-latency.py [--rounds 5] [--keys 30] [--gap 150] \\
        [--child cat|shell|none] [--arm name=path] [--aa] [--grid 120x40]
    scripts/key-latency.py --selftest        # no app, no posting: unit tests
"""
import argparse
import collections
import hashlib
import os
import re
import signal
import statistics
import subprocess
import sys
import tempfile
import threading
import time

# Every token a record MUST carry, in emission order. A record lacking one
# is a parse failure, not a `-`: a renamed leg in the app must fail loudly
# here rather than read as "never happened".
KNOWN = ["seq", "event", "handled", "written", "echo", "fed", "commit",
         "landed", "shown", "vsync", "vsync_hi"]
# The offset fields (everything but the two absolutes).
OFFSETS = KNOWN[2:]

# The legs, in report order, as (name, start_stage, end_stage). "post" is the
# helper's stamp; the rest are keylat stages.
LEGS = [
    ("stamp", "post", "event"),
    ("deliver", "post", "handled"),
    ("write", "handled", "written"),
    ("child", "written", "echo"),
    ("wake", "echo", "fed"),
    ("build", "fed", "commit"),
    ("gpu", "commit", "landed"),
    ("show", "landed", "shown"),
    ("vsync", "shown", "vsync"),
    ("vsync_hi", "shown", "vsync_hi"),
    ("to_shown", "post", "shown"),
    ("total", "post", "vsync"),
    ("total_hi", "post", "vsync_hi"),
]
LEG_NAMES = [name for name, _, _ in LEGS]

# Two keys "share a frame" when their reconstructed absolute `commit`s lie
# within this of each other. The offsets print at 0.1 us and every key
# rebuilds its absolute from its own `event`, so two keys on ONE commit can
# differ by up to 100 ns after the round trip; exact equality would miss
# them. Distinct frames are milliseconds apart.
SHARED_FRAME_NS = 200

DEFAULT_WINDOW_MS = 100
# Below a 100 ms gap the window shrinks to `gap - WINDOW_MARGIN_MS`, so a
# pairing shifted by one key cannot fit inside it.
WINDOW_MARGIN_MS = 5
WINDOW_FLOOR_MS = 1
# The helper draws each inter-key gap uniformly from [gap - gap*JITTER,
# gap + gap*JITTER] (key-post.swift, `gapSchedule`); the driver needs the
# fraction for the header and for the pairing window, which follows the
# MINIMUM gap so a one-key shift cannot fit inside it.
GAP_JITTER = 0.25

# The app expires a record 2 s after its event, on a tick; SIGTERM must come
# no sooner than this after the last post, so a no-echo run emits every
# record it opened.
DRAIN_S = 2.6

XCODE_DEVELOPER_DIR = "/Applications/Xcode.app/Contents/Developer"

_POST = re.compile(r"^post\s+(\d+)\s+(\d+)\s+(\S+)\s*$")
_FRONT = re.compile(r"^front\s+(-?\d+)\s+(.*?)\s+app\s+(-?\d+)\s+(.*)$")
_OFFSET = re.compile(r"^\+(\d+(?:\.\d+)?)$")
REFUSAL_PREFIX = "keylat: landing refused"


class KeylatParseError(ValueError):
    """A `keylat: seq=...` record that is not what the app promises to
    emit -- a known token missing, or a value that does not parse."""


def parse_post_line(line):
    """`post <seq> <ns> <char>` -> (seq:int, ns:int, char:str), or None."""
    m = _POST.match(line.strip())
    if not m:
        return None
    return (int(m.group(1)), int(m.group(2)), m.group(3))


def parse_front_line(line):
    """The helper's `--front` line, `front <pid> <name> app <pid> <name>`
    -> (window_owner_pid, window_owner_name, front_app_pid, front_app_name)
    or None."""
    m = _FRONT.match(line.strip())
    if not m:
        return None
    return (int(m.group(1)), m.group(2), int(m.group(3)), m.group(4))


def parse_keylat_line(line):
    """A `keylat: seq=... event=... <offset fields>` line -> a dict with
    `seq`, `event` (ns) and each stage as an absolute ns or None.

    Returns None for any line that is not a per-key record: the `N keys
    open` report line, the `landing refused` line, and all non-keylat
    output. Raises KeylatParseError for a record that IS a per-key line
    (it carries `seq=`) but lacks a KNOWN token or carries a value that is
    neither `+<us>` nor `-`. Tokens outside KNOWN are ignored."""
    line = line.strip()
    if not line.startswith("keylat:"):
        return None
    toks = {}
    for tok in line[len("keylat:"):].split():
        if "=" not in tok:
            return None  # "keylat: 3 keys open", "keylat: landing refused ..."
        k, v = tok.split("=", 1)
        toks[k] = v
    if "seq" not in toks:
        return None
    missing = [k for k in KNOWN if k not in toks]
    if missing:
        raise KeylatParseError(f"missing {','.join(missing)}: {line}")
    try:
        rec = {"seq": int(toks["seq"]), "event": int(toks["event"])}
    except ValueError:
        raise KeylatParseError(f"seq/event not integers: {line}")
    for name in OFFSETS:
        v = toks[name]
        if v == "-":
            rec[name] = None  # a leg that never happened
            continue
        m = _OFFSET.match(v)
        if not m:
            raise KeylatParseError(f"{name}={v} is neither +<us> nor -: {line}")
        # `+<us>`: microseconds from event, one decimal.
        rec[name] = rec["event"] + int(round(float(m.group(1)) * 1000.0))
    return rec


def parse_keylat_lines(lines):
    """Every line of one run -> (records, failures, refusals, reports).
    `failures` are the KeylatParseError messages, `refusals` the count of
    `landing refused` lines, `reports` the `N keys open` lines."""
    records, failures, reports = [], [], []
    refusals = 0
    for line in lines:
        s = line.strip()
        if s.startswith(REFUSAL_PREFIX):
            refusals += 1
            continue
        try:
            rec = parse_keylat_line(s)
        except KeylatParseError as e:
            failures.append(str(e))
            continue
        if rec is not None:
            records.append(rec)
        elif s.startswith("keylat:") and s.endswith("keys open"):
            reports.append(s)
    return records, failures, refusals, reports


def gap_bounds(gap_ms):
    """(lo, hi) in ms: the range the helper draws each inter-key gap from,
    `gap -/+ gap * GAP_JITTER`; a gap of 0 stays (0, 0)."""
    q = gap_ms * GAP_JITTER
    return (gap_ms - q, gap_ms + q)


def window_for_gap(gap_ms):
    """The pairing window in ms: 100, or the MINIMUM jittered gap - 5 when
    that minimum is under 100 ms, floored at 1 (a gap of a few ms cannot
    separate two keys by time at all; the seq check is the only detector
    left there). It follows the minimum, not the mean, so a pairing
    shifted by one key cannot fit inside it however the jitter fell."""
    lo, _ = gap_bounds(gap_ms)
    if lo >= DEFAULT_WINDOW_MS:
        return DEFAULT_WINDOW_MS
    return max(int(lo - WINDOW_MARGIN_MS), WINDOW_FLOOR_MS)


def _stages(row):
    stage = {"post": row["post"], "event": row["event"]}
    for name in OFFSETS:
        stage[name] = row.get(name)
    return stage


def _legs_from_stages(stage):
    out = {}
    for name, a, b in LEGS:
        sa, sb = stage.get(a), stage.get(b)
        out[name] = None if sa is None or sb is None else (sb - sa) / 1000.0
    return out


def negative_legs(stage):
    """The legs after `stamp` that came out negative: a stamp that is not
    on the clock the others are (the GPU end after the wake that reported
    it, a re-stamped event). Such a row is excluded, never pooled."""
    legs = _legs_from_stages(stage)
    return [name for name in LEG_NAMES
            if name != "stamp" and legs[name] is not None and legs[name] < 0]


def pair_by_order(posts, keylats, window_ns=DEFAULT_WINDOW_MS * 1_000_000):
    """Pair posts to keylat records BY ORDER (the spec's rule: one key = one
    echo = one frame under a cadence slower than a frame; no per-frame
    identity plumbing). Returns (rows, diagnostics) where each row carries
    the absolute ns of every stage plus the pairing verdict. A pair is
    EXCLUDED, with the reason, when the app's `seq` is not the helper's
    (`seq-mismatch`: a shifted or foreign pairing), `event < post` (a stamp
    that cannot be this key's), `event - post >= window` (a lost or
    mispaired line), or a leg after `stamp` is negative."""
    rows = []
    reasons = collections.Counter()
    n = min(len(posts), len(keylats))
    for i in range(n):
        pseq, post_ns, char = posts[i]
        rec = keylats[i]
        event = rec["event"]
        delta = event - post_ns
        row = {"seq": pseq, "app_seq": rec["seq"], "char": char,
               "post": post_ns, "event": event}
        for name in OFFSETS:
            row[name] = rec[name]
        reason = None
        if rec["seq"] != pseq:
            reason = "seq-mismatch"
        elif event < post_ns:
            reason = "event<post"
        elif delta >= window_ns:
            reason = "event-post>=window"
        else:
            neg = negative_legs(_stages(row))
            if neg:
                reason = "negative " + ",".join(neg)
        row["excluded"] = reason is not None
        row["exclude_reason"] = reason
        if reason:
            reasons["negative" if reason.startswith("negative") else reason] += 1
        rows.append(row)
    diag = {
        "posted": len(posts),
        "keylats": len(keylats),
        "paired": n,
        "unpaired_posts": len(posts) - n,
        "unpaired_keylats": len(keylats) - n,
        "seq_mismatch": reasons["seq-mismatch"],
        "event_before_post": reasons["event<post"],
        "window": reasons["event-post>=window"],
        "negative": reasons["negative"],
    }
    return rows, diag


def exclude_refused_landings(rows, refusals):
    """When the app printed `keylat: landing refused` `refusals` times in
    this run, exclude every row that reached the GPU (`commit` set) and
    has no landing (`landed=-`) as `refused-landing`: a refused present
    can stamp `shown` on a record whose real frame has not presented, so
    its show/vsync/to_shown are not this key's. Rows already excluded keep
    their reason; a row with no `commit` never reached the GPU and is
    merely incomplete. A run with no refusal line -- the CPU path, where
    every row has `landed=-` -- is untouched. Returns how many rows it
    excluded; the caller marks the run NOT QUOTABLE when `refusals` > 0."""
    if refusals <= 0:
        return 0
    n = 0
    for r in rows:
        if r["excluded"] or r.get("commit") is None or r.get("landed") is not None:
            continue
        r["excluded"] = True
        r["exclude_reason"] = "refused-landing"
        n += 1
    return n


def legs_of(row):
    """The per-key legs in microseconds (float), None where an endpoint is
    missing or the row is excluded."""
    if row["excluded"]:
        return {name: None for name in LEG_NAMES}
    return _legs_from_stages(_stages(row))


def is_completed(row):
    """Completed = paired (not excluded) and reached vsync."""
    return not row["excluded"] and row.get("vsync") is not None


def shared_frame_count(rows, tolerance_ns=SHARED_FRAME_NS):
    """How many keys landed in a frame they share with another key: absolute
    `commit`s within `tolerance_ns` of the group's first. Under typeahead
    several keys ride one frame; this is that count, so the reader knows how
    much the medians pooled."""
    commits = sorted(r["commit"] for r in rows
                     if not r["excluded"] and r.get("commit") is not None)
    shared = 0
    group = []
    for c in commits:
        if group and c - group[0] > tolerance_ns:
            if len(group) > 1:
                shared += len(group)
            group = []
        group.append(c)
    if len(group) > 1:
        shared += len(group)
    return shared


def leg_stats(values):
    """{median, q1, q3, min, max, n} of a list of floats, or None if empty.
    Quartiles are inclusive, so they lie within the data."""
    xs = [v for v in values if v is not None]
    if not xs:
        return None
    if len(xs) == 1:
        q1 = q3 = xs[0]
    else:
        q1, _, q3 = statistics.quantiles(xs, n=4, method="inclusive")
    return {"median": statistics.median(xs), "q1": q1, "q3": q3,
            "min": min(xs), "max": max(xs), "n": len(xs)}


def summarize(rows, diag=None):
    """Fold paired rows (over one or many rounds) into per-leg stats and the
    posted/paired/completed/excluded counts. `posted` is the helper's count
    when a diag is given (it can exceed `paired` when lines were lost)."""
    completed = [r for r in rows if is_completed(r)]
    excluded = [r for r in rows if r["excluded"]]
    reasons = collections.Counter(
        ("negative" if r["exclude_reason"].startswith("negative")
         else r["exclude_reason"]) for r in excluded)
    leg_vals = {name: [] for name in LEG_NAMES}
    for r in rows:
        legs = legs_of(r)
        for name in LEG_NAMES:
            if legs[name] is not None:
                leg_vals[name].append(legs[name])
    return {
        "posted": diag["posted"] if diag else len(rows),
        "paired": len(rows),
        "completed": len(completed),
        "excluded": len(excluded),
        "reasons": dict(reasons),
        "shared": shared_frame_count(rows),
        "legs": {name: leg_stats(leg_vals[name]) for name in LEG_NAMES},
    }


def merge_diags(diags):
    out = collections.Counter()
    for d in diags:
        out.update(d)
    return dict(out)


def _fmt_med(st):
    if st is None:
        return f"{'-':>9} {'':>5}"
    return f"{st['median']:9.1f} ({st['n']:3d})"


def _fmt_iqr(st):
    if st is None:
        return "-"
    return f"[{st['q1']:.1f}-{st['q3']:.1f}]"


def _fmt_range(st):
    if st is None:
        return "-"
    return f"[{st['min']:.1f}-{st['max']:.1f}]"


def print_summary(name, summ, diag=None):
    print(f"\n== arm {name} ==")
    print(f"posted={summ['posted']} paired={summ['paired']} "
          f"completed={summ['completed']} excluded={summ['excluded']} "
          f"shared_a_frame={summ['shared']}")
    if summ["reasons"]:
        print("excluded by reason: " + ", ".join(
            f"{k}={v}" for k, v in sorted(summ["reasons"].items())))
    if diag:
        print(f"unpaired posts={diag.get('unpaired_posts', 0)} "
              f"unpaired keylats={diag.get('unpaired_keylats', 0)} "
              f"parse failures={diag.get('parse_failures', 0)} "
              f"landings refused={diag.get('landing_refused', 0)}")
        if diag.get("landing_refused", 0):
            print(f"NOT QUOTABLE -- {diag['landing_refused']} landing(s) refused "
                  f"in the runs pooled here: a GPU end stamp off the clock; "
                  f"{summ['reasons'].get('refused-landing', 0)} row(s) on the "
                  f"GPU path with landed=- excluded as refused-landing")
    print(f"{'leg':>9}  {'median us':>9} {'(n)':>5}  {'IQR [q1-q3]':<24} range [min-max]")
    for legname in LEG_NAMES:
        st = summ["legs"][legname]
        print(f"{legname:>9}  {_fmt_med(st)}  {_fmt_iqr(st):<24} {_fmt_range(st)}")


def aa_floor(base_legs, aa_legs):
    """Per leg, the A/A noise floor: the larger of |median(base) -
    median(aa)| and either arm's interquartile range. Never max-min. None
    for a leg one arm has no value for."""
    out = {}
    for name in LEG_NAMES:
        b, a = base_legs.get(name), aa_legs.get(name)
        if b is None or a is None:
            out[name] = None
            continue
        dmed = abs(b["median"] - a["median"])
        iqr_b = b["q3"] - b["q1"]
        iqr_a = a["q3"] - a["q1"]
        out[name] = {"dmed": dmed, "iqr_base": iqr_b, "iqr_aa": iqr_a,
                     "floor": max(dmed, iqr_b, iqr_a)}
    return out


def print_aa(base_name, base, aa):
    floors = aa_floor(base["legs"], aa["legs"])
    print(f"\n== A/A: {base_name} against aa (the same binary) ==")
    print("floor per leg = max(|median delta|, IQR base, IQR aa); a leg that "
          "moves by less than its floor is not a result.")
    print(f"{'leg':>9}  {base_name:>10}  {'aa':>10}  {'|d med|':>8}  "
          f"{'IQR base':>9}  {'IQR aa':>9}  {'floor us':>9}")
    for name in LEG_NAMES:
        b, a, f = base["legs"][name], aa["legs"][name], floors[name]
        if f is None:
            print(f"{name:>9}  {'-':>10}  {'-':>10}  {'-':>8}  {'-':>9}  {'-':>9}  {'-':>9}")
            continue
        print(f"{name:>9}  {b['median']:10.1f}  {a['median']:10.1f}  "
              f"{f['dmed']:8.1f}  {f['iqr_base']:9.1f}  {f['iqr_aa']:9.1f}  "
              f"{f['floor']:9.1f}")
    return floors


def iqr_disjoint(b, a):
    """True when the two interquartile ranges do not overlap."""
    return b["q3"] < a["q1"] or a["q3"] < b["q1"]


def arm_deltas(base_legs, arm_legs, floors):
    """Per leg, `median(arm) - median(base)` against the A/A floor: the
    verdict is `result` only when |delta| exceeds the floor AND the two
    arms' interquartile ranges are disjoint; otherwise `within floor --
    not a result` (|delta| at or under the floor), `IQRs overlap -- not a
    result` (above the floor, ranges overlapping), or `no A/A floor -- not
    a result` when `floors` is None or lacks the leg. None for a leg one
    arm has no value for."""
    out = {}
    for name in LEG_NAMES:
        b, a = base_legs.get(name), arm_legs.get(name)
        if b is None or a is None:
            out[name] = None
            continue
        delta = a["median"] - b["median"]
        disjoint = iqr_disjoint(b, a)
        f = floors.get(name) if floors else None
        floor = f["floor"] if f else None
        if floor is None:
            verdict = "no A/A floor -- not a result"
        elif abs(delta) <= floor:
            verdict = f"within floor -- not a result (|delta| {abs(delta):.1f} <= floor {floor:.1f})"
        elif not disjoint:
            verdict = "IQRs overlap -- not a result"
        else:
            verdict = "result"
        out[name] = {"delta": delta, "disjoint": disjoint, "floor": floor,
                     "verdict": verdict}
    return out


def print_arm_deltas(base_name, arm_name, base, arm, floors):
    deltas = arm_deltas(base["legs"], arm["legs"], floors)
    print(f"\n== A/B: {arm_name} - {base_name}, per leg, beside the A/A floor ==")
    if floors is None:
        print("no A/A floor (run with --aa): no row can be a result")
    print("result = |delta| above the floor AND the two arms' IQRs disjoint; "
          "anything else is not a result")
    print(f"{'leg':>9}  {base_name:>10}  {arm_name:>10}  {'delta us':>9}  "
          f"{'floor us':>9}  {'IQR ' + base_name:>16}  {'IQR ' + arm_name:>16}  verdict")
    for name in LEG_NAMES:
        d = deltas[name]
        if d is None:
            print(f"{name:>9}  {'-':>10}  {'-':>10}  {'-':>9}  {'-':>9}  "
                  f"{'-':>16}  {'-':>16}  -")
            continue
        b, a = base["legs"][name], arm["legs"][name]
        floor = "-" if d["floor"] is None else f"{d['floor']:.1f}"
        print(f"{name:>9}  {b['median']:10.1f}  {a['median']:10.1f}  "
              f"{d['delta']:+9.1f}  {floor:>9}  {_fmt_iqr(b):>16}  "
              f"{_fmt_iqr(a):>16}  {d['verdict']}")
    return deltas


def rotate(items, rnd):
    """The arm order for round `rnd` (1-based): rotated left by rnd-1, so
    every arm takes every position over a sweep and none is always last."""
    if not items:
        return []
    k = (rnd - 1) % len(items)
    return items[k:] + items[:k]


def settled_rounds(rounds):
    """Round 1 is warm-up and discarded. With a single round there is
    nothing else, and the caller must say so: (rounds, warmup_used)."""
    if len(rounds) > 1:
        return rounds[1:], False
    return rounds, True


def warmup_measured_nothing(rounds):
    """True when there is a discarded warm-up round (more than one round)
    and it completed zero keys -- a launch that never worked in round 1
    must be said, not absorbed. A single round is used, not discarded,
    and `settled_rounds` flags it instead."""
    return len(rounds) > 1 and summarize(rounds[0])["completed"] == 0


def refused_runs(per_arm_diag):
    """(arm, round, count) for every run whose app printed `keylat:
    landing refused`, in arm then round order."""
    out = []
    for name, diags in per_arm_diag.items():
        for i, d in enumerate(diags):
            k = d.get("landing_refused", 0)
            if k:
                out.append((name, i + 1, k))
    return out


def child_env(child):
    """What BAUD_CMD the child mode sets: `cat` the tty's own echo, `shell`
    the login shell (unset), `none` a tty with echo off -- the control."""
    if child == "cat":
        return {"BAUD_CMD": "cat"}
    if child == "none":
        return {"BAUD_CMD": "stty -echo; sleep 60"}
    return {}


def sha256_of(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


# ---------------------------------------------------------------------------
# The sweep (needs a built `baud` and the Swift helper; only its pure helpers
# are exercised by the self-test).
# ---------------------------------------------------------------------------

def developer_dir():
    """The Xcode developer directory with a compiler in it: Xcode.app when
    it exists, else what `xcode-select -p` answers if that carries a
    toolchain, else None. This box's DEVELOPER_DIR points at a nix SDK with
    no compiler, and `xcode-select -p` honours DEVELOPER_DIR, so Xcode.app
    is preferred outright."""
    if os.path.isdir(XCODE_DEVELOPER_DIR):
        return XCODE_DEVELOPER_DIR
    try:
        p = subprocess.run(["xcode-select", "-p"], capture_output=True,
                           text=True, timeout=5)
    except (OSError, subprocess.TimeoutExpired):
        return None
    cand = p.stdout.strip()
    if p.returncode == 0 and os.path.isfile(
            f"{cand}/Toolchains/XcodeDefault.xctoolchain/usr/bin/swift"):
        return cand
    return None


def swift_argv(script, args):
    """The argv to run a Swift script. `$BAUD_SWIFT` overrides the whole
    command (space-split); otherwise Xcode.app's toolchain with an explicit
    SDK, then `xcode-select -p`, then PATH `swift` for a normal box."""
    override = os.environ.get("BAUD_SWIFT")
    if override:
        return override.split() + [script] + [str(a) for a in args]
    dd = developer_dir()
    if dd:
        sdk = f"{dd}/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk"
        if os.path.isdir(sdk):
            return (["/usr/bin/env", f"DEVELOPER_DIR={dd}", "swift", "-sdk", sdk]
                    + [script] + [str(a) for a in args])
    return ["swift", script] + [str(a) for a in args]


def wait_for(stderr_lines, stop, needles, timeout):
    """Block until every needle has appeared as a prefix in the shared line
    list, the app's stderr closed (`stop`), or timeout. Returns (ok, why)."""
    deadline = time.perf_counter() + timeout
    pending = set(needles)
    while pending and time.perf_counter() < deadline and not stop.is_set():
        for line in list(stderr_lines):
            for n in list(pending):
                if line.startswith(n):
                    pending.discard(n)
        if pending:
            time.sleep(0.02)
    if not pending:
        return True, None
    if stop.is_set():
        return False, "app exited before presenting"
    return False, "app never presented (timeout)"


def activate(pid, helper_script):
    """Raise baud to the front from the driver: the helper's own
    NSRunningApplication.activate() is blocked by focus-stealing prevention
    (it is a separate, unbundled process), so System Events sets the
    process frontmost, and the helper's `--front` mode is polled up to 2 s
    for both the front layer-0 window and the frontmost application to be
    baud's. Returns (ok, last front line). The helper's guard is still the
    gate; this only makes it pass."""
    script = ('tell application "System Events" to set frontmost of '
              f'(first process whose unix id is {pid}) to true')
    try:
        subprocess.run(["osascript", "-e", script], capture_output=True,
                       text=True, timeout=5)
    except (OSError, subprocess.TimeoutExpired) as e:
        return False, f"osascript failed: {e}"
    last = "?"
    deadline = time.perf_counter() + 2.0
    while time.perf_counter() < deadline:
        try:
            fp = subprocess.run(swift_argv(helper_script, ["--front"]),
                                capture_output=True, text=True, timeout=10)
        except (OSError, subprocess.TimeoutExpired) as e:
            return False, f"helper --front failed: {e}"
        for line in fp.stdout.splitlines():
            f = parse_front_line(line)
            if f:
                last = line.strip()
                if f[0] == pid and f[2] == pid:
                    return True, last
        time.sleep(0.1)
    return False, last


def run_arm(binary, args, scratch, round_idx, arm_name, window_ns):
    """Launch `binary`, wait for it to present, post `args.keys` letters with
    the Swift helper, collect the keylat lines, and return a dict with the
    rows, diagnostics, and the flags `refused` (the helper exited 3) and
    `fatal` (the helper failed some other way -- abort the sweep). SIGTERMs
    the app no sooner than DRAIN_S after the last post."""
    env = dict(os.environ)
    env.update({"BAUD_KEYLAT": "1", "BAUD_STARTUP": "1", "BAUD_GRID": args.grid})
    env.pop("BAUD_CMD", None)
    env.update(child_env(args.child))

    stderr_lines = []
    stop = threading.Event()
    out = {"rows": [], "diag": {}, "refused": False, "fatal": False}

    # `close_fds=False`: the default walks and closes every descriptor before
    # exec, ~3 ms a launch this box never pays -- see startup-bench.py.
    p = subprocess.Popen(
        [binary], env=env, stderr=subprocess.PIPE, stdout=subprocess.DEVNULL,
        text=True, close_fds=False,
    )

    def reader():
        for line in p.stderr:
            stderr_lines.append(line.rstrip("\n"))
        stop.set()

    th = threading.Thread(target=reader, daemon=True)
    th.start()

    def stop_app():
        if p.poll() is None:
            p.send_signal(signal.SIGTERM)
        try:
            p.wait(timeout=5)
        except subprocess.TimeoutExpired:
            p.kill()
        stop.wait(timeout=2)

    needles = ["startup: first present"]
    if args.child == "shell":
        needles.append("startup: first content")
    ok, why = wait_for(stderr_lines, stop, needles, timeout=30.0)
    if not ok:
        stop_app()
        print(f"  round {round_idx} arm {arm_name}: {why}; "
              f"stages seen: {[l for l in stderr_lines if l.startswith('startup:')]}")
        return out

    time.sleep(0.4)  # let the shell settle before typing

    helper_script = "scripts/key-post.swift"
    front_ok, front = activate(p.pid, helper_script)
    if not front_ok:
        print(f"  round {round_idx} arm {arm_name}: could not bring baud to "
              f"the front ({front}); the helper's guard will decide")

    keylat_start = len(stderr_lines)  # only lines from here belong to this run
    helper = swift_argv(helper_script, [p.pid, args.keys, args.gap, args.chars])
    hp = subprocess.run(helper, capture_output=True, text=True)
    posts = [parse_post_line(l) for l in hp.stdout.splitlines()]
    posts = [x for x in posts if x]
    refused = hp.returncode == 3 or "REFUSED" in hp.stdout or "REFUSED" in hp.stderr
    out["refused"] = refused
    if refused:
        print(f"  round {round_idx} arm {arm_name}: helper REFUSED:"
              f" {hp.stdout.strip()} {hp.stderr.strip()}")
    elif hp.returncode != 0:
        print(f"  round {round_idx} arm {arm_name}: helper FAILED, exit "
              f"{hp.returncode}\n  helper argv: {' '.join(helper)}\n"
              f"  helper stdout:\n{hp.stdout}\n  helper stderr:\n{hp.stderr}")
        out["fatal"] = True
        stop_app()
        return out

    # Drain: the app expires a record 2 s after its event, on a tick, so a
    # key that never echoes is only emitted after that; wait it out.
    time.sleep(DRAIN_S)
    stop_app()

    keylats, failures, refusals, _reports = parse_keylat_lines(stderr_lines[keylat_start:])
    if posts and not keylats:
        print(f"  round {round_idx} arm {arm_name}: keys posted, none reached "
              f"the app (Accessibility? wrong target?)")
    for fmsg in failures:
        print(f"  round {round_idx} arm {arm_name}: PARSE FAILURE {fmsg}")
    rows, diag = pair_by_order(posts, keylats, window_ns)
    diag["parse_failures"] = len(failures)
    diag["landing_refused"] = refusals
    diag["refused_landing_rows"] = exclude_refused_landings(rows, refusals)
    if refusals:
        print(f"  round {round_idx} arm {arm_name}: NOT QUOTABLE -- {refusals} "
              f"landing(s) refused; {diag['refused_landing_rows']} GPU-path "
              f"row(s) with landed=- excluded as refused-landing")
    for r in rows:
        if r["excluded"]:
            print(f"  round {round_idx} arm {arm_name}: excluded seq={r['seq']} "
                  f"(app seq={r['app_seq']}): {r['exclude_reason']}")
    out["rows"], out["diag"] = rows, diag

    # Raw rows to a TSV for anyone who wants to look.
    tsv = os.path.join(scratch, f"keylat-{arm_name}-{round_idx}.tsv")
    with open(tsv, "w") as f:
        cols = ["seq", "app_seq", "char", "post", "event", "excluded",
                "exclude_reason"] + OFFSETS
        f.write("\t".join(cols) + "\n")
        for r in rows:
            f.write("\t".join(str(r.get(c, "")) for c in cols) + "\n")
    print(f"  round {round_idx} arm {arm_name}: pid={p.pid} posted={len(posts)} "
          f"keylats={len(keylats)} paired={diag['paired']} "
          f"unpaired posts={diag['unpaired_posts']} "
          f"unpaired keylats={diag['unpaired_keylats']} "
          f"seq mismatches={diag['seq_mismatch']} negative={diag['negative']} "
          f"parse failures={len(failures)} landings refused={refusals} -> {tsv}")
    return out


def print_header(args, arm_list, window_ms):
    print("== keystroke-to-pixel sweep ==")
    quotable = True
    try:
        pm = subprocess.run(["pmset", "-g", "batt"], capture_output=True,
                            text=True, timeout=5).stdout.splitlines()[:2]
    except (OSError, subprocess.TimeoutExpired, IndexError):
        pm = ["pmset: unavailable"]
    for line in pm:
        print(f"power: {line.strip()}")
    if any("Battery Power" in l for l in pm) or not any("AC Power" in l for l in pm):
        quotable = False
        print("power: NOT QUOTABLE -- on battery (or power source unknown); "
              "CLAUDE.md: battery numbers are not quotable")
    helper = swift_argv("scripts/key-post.swift", ["--info"])
    try:
        ip = subprocess.run(helper, capture_output=True, text=True, timeout=60)
        rate = [l for l in ip.stdout.splitlines() if l.startswith("refresh")]
        print(f"display: {rate[0] if rate else 'refresh unknown'}"
              + ("" if ip.returncode == 0 else f" (helper --info exit {ip.returncode}: {ip.stderr.strip()})"))
    except (OSError, subprocess.TimeoutExpired) as e:
        print(f"display: refresh unknown ({e})")
    print(f"swift: {' '.join(swift_argv('scripts/key-post.swift', []))}")
    print(f"child: {args.child} ({child_env(args.child).get('BAUD_CMD', 'the login shell')})")
    lo, hi = gap_bounds(args.gap)
    print(f"gap: {args.gap} ms MEAN; each inter-key gap is uniform in "
          f"[{lo:g}-{hi:g}] ms, seeded from baud's pid (printed per round) -- "
          f"a fixed gap phase-locks to the refresh and narrows vsync/total falsely")
    print(f"pairing window: {window_ms} ms (minimum jittered gap {lo:g} ms)")
    if lo < DEFAULT_WINDOW_MS:
        print(f"  minimum gap under {DEFAULT_WINDOW_MS} ms: the window is that "
              f"minimum - {WINDOW_MARGIN_MS} ms so a one-key shift cannot fit inside it")
    print("A/B number: to_shown (post -> the surface on the layer; no "
          "display-period quantisation). vsync/total are a bracket, not a "
          "measurement -- see the doc.")
    shas = {}
    for name, path in arm_list:
        try:
            shas[name] = sha256_of(path)
        except OSError as e:
            shas[name] = f"unreadable ({e})"
        print(f"arm {name}: {path} sha256={shas[name]}")
    if args.aa:
        print("arm aa is the first arm's binary again, byte-identical by design")
    names = [n for n, _ in arm_list]
    for i in range(len(names)):
        for j in range(i + 1, len(names)):
            if "aa" in (names[i], names[j]):
                continue
            if shas[names[i]] == shas[names[j]]:
                print(f"WARNING: arms {names[i]} and {names[j]} are byte-identical "
                      f"-- an A/B of one binary against itself measures the hour")
    print("arm order rotates every round so no arm is always last")
    return quotable


def sweep(args):
    scratch = args.scratch
    os.makedirs(scratch, exist_ok=True)
    arm_list = [tuple(a.split("=", 1)) for a in args.arm]
    if args.aa:
        arm_list.append(("aa", arm_list[0][1]))  # the same binary again
    window_ms = window_for_gap(args.gap)
    window_ns = window_ms * 1_000_000
    quotable = print_header(args, arm_list, window_ms)

    per_arm = {name: [] for name, _ in arm_list}
    per_arm_diag = {name: [] for name, _ in arm_list}
    refused_any = False
    for rnd in range(1, args.rounds + 1):
        order = rotate(arm_list, rnd)
        print(f"\nround {rnd}: order {[n for n, _ in order]}")
        for name, path in order:  # interleaved: all arms each round
            res = run_arm(path, args, scratch, rnd, name, window_ns)
            if res["fatal"]:
                print("\nthe helper failed; sweep aborted")
                return 4
            refused_any = refused_any or res["refused"]
            per_arm[name].append(res["rows"])
            per_arm_diag[name].append(res["diag"])

    # Round 1 is warm-up, discarded.
    zero_runs = []
    zero_pooled = []
    summaries = {}
    warmup_used = False
    warmup_empty = []
    for name, _ in arm_list:
        rounds = per_arm[name]
        settled, warm = settled_rounds(rounds)
        warmup_used = warmup_used or warm
        if warmup_measured_nothing(rounds):
            warmup_empty.append(name)
        first = 1 if warm else 2
        for i, rows in enumerate(settled):
            if summarize(rows)["completed"] == 0:
                zero_runs.append((name, first + i))
        pooled = [r for rnd in settled for r in rnd]
        sdiag = merge_diags(settled_rounds(per_arm_diag[name])[0])
        summ = summarize(pooled, sdiag)
        summaries[name] = summ
        print_summary(name, summ, sdiag)
        if summ["completed"] == 0:
            zero_pooled.append(name)

    if warmup_used:
        print("\nWARM-UP ROUND USED -- --rounds 1 has no settled round; not quotable")
    if not quotable:
        print("\nNOT QUOTABLE -- taken on battery (or an unknown power source)")

    base_name = arm_list[0][0]
    floors = None
    if args.aa and "aa" in summaries:
        floors = print_aa(base_name, summaries[base_name], summaries["aa"])
    for other in [n for n, _ in arm_list if n not in ("aa", base_name)]:
        print_arm_deltas(base_name, other, summaries[base_name],
                         summaries[other], floors)

    # The notes a reader must see last: a warm-up that measured nothing,
    # and any run whose landings were refused (NOT QUOTABLE, named).
    for name in warmup_empty:
        print(f"\narm {name}: round 1 measured nothing (warm-up, discarded)")
    for name, rnd, k in refused_runs(per_arm_diag):
        if rnd == 1 and args.rounds > 1:
            print(f"\narm {name}: round 1 had {k} landing(s) refused "
                  f"(warm-up, discarded)")
        else:
            print(f"\nNOT QUOTABLE -- landings refused: arm {name} round {rnd} "
                  f"({k}); its GPU-path rows with landed=- were excluded as "
                  f"refused-landing")

    if refused_any:
        return 2
    for name, rnd in zero_runs:
        print(f"\nzero completed keys: arm {name} round {rnd}")
    if zero_runs or zero_pooled:
        print("\nzero completed keys in a run -- the instrument reported "
              "nothing; not a pass.")
        return 1
    return 0


# ---------------------------------------------------------------------------
# Self-test: exercise the pure functions against the on-disk fixtures and a
# few hand-built cases. No app, no posting.
# ---------------------------------------------------------------------------

def _approx(a, b, eps=1e-6):
    return a is not None and abs(a - b) < eps


def _rec(seq, event, **over):
    """A well-formed record line with every known token, overridable."""
    f = {"handled": "+300.0", "written": "+350.0", "echo": "+360.0",
         "fed": "+365.0", "commit": "+370.0", "landed": "+380.0",
         "shown": "+385.0", "vsync": "+400.0", "vsync_hi": "+416.7"}
    f.update(over)
    return (f"keylat: seq={seq} event={event} " +
            " ".join(f"{k}={f[k]}" for k in OFFSETS))


def selftest():
    here = os.path.dirname(os.path.abspath(__file__))
    # The fixtures are committed beside the resize ones, in tests/keylat/;
    # `BAUD_KEYLAT_FIXTURES` points the self-test at another pair.
    fix_dir = os.environ.get(
        "BAUD_KEYLAT_FIXTURES",
        os.path.join(here, "..", "tests", "keylat"),
    )
    kfile = os.path.join(fix_dir, "keylat-fixture.txt")
    pfile = os.path.join(fix_dir, "keylat-posts-fixture.txt")
    failures = []

    with open(kfile) as f:
        klines = f.readlines()
    with open(pfile) as f:
        plines = f.readlines()

    # Parsing filters non-keylat noise, the "N keys open" report line and
    # the `landing refused` line; a record missing a known token is a
    # counted parse failure (seq=10 lacks vsync_hi), not a silent `-`.
    keylats, pfail, refusals, reports = parse_keylat_lines(klines)
    posts = [x for x in (parse_post_line(l) for l in plines) if x]
    if len(keylats) != 9:
        failures.append(f"expected 9 keylat records, got {len(keylats)}")
    if len(posts) != 10:
        failures.append(f"expected 10 posts, got {len(posts)}")
    if len(pfail) != 1 or "vsync_hi" not in pfail[0] or "seq=10" not in pfail[0]:
        failures.append(f"expected one parse failure naming vsync_hi on seq=10, got {pfail}")
    if refusals != 1:
        failures.append(f"expected 1 landing-refused line, got {refusals}")
    if reports != ["keylat: 3 keys open"]:
        failures.append(f"expected the one report line, got {reports}")

    rows, diag = pair_by_order(posts, keylats)
    if diag["paired"] != 9 or diag["unpaired_posts"] != 1 or diag["unpaired_keylats"] != 0:
        failures.append(f"pairing counts wrong: {diag}")
    diag["parse_failures"] = len(pfail)  # what run_arm adds beside the pairing
    diag["landing_refused"] = refusals
    # The run printed one `landing refused` line, so seq 9 -- committed to
    # the GPU, `landed=-` -- is a refused landing and is excluded rather
    # than pooled with a `shown` from the wrong surface.
    diag["refused_landing_rows"] = exclude_refused_landings(rows, refusals)
    if diag["refused_landing_rows"] != 1:
        failures.append(f"expected 1 refused-landing row, got {diag['refused_landing_rows']}")

    # Exclusions: seq 6's event precedes its post; seq 7's trails it by
    # 200 ms (>= the 100 ms window); post 8 met the app's seq=88; seq 9's
    # landing was refused.
    ex = {r["seq"]: r["exclude_reason"] for r in rows if r["excluded"]}
    want_ex = {6: "event<post", 7: "event-post>=window", 8: "seq-mismatch",
               9: "refused-landing"}
    if ex != want_ex:
        failures.append(f"expected exclusions {want_ex}, got {ex}")
    if diag["seq_mismatch"] != 1 or diag["window"] != 1 or diag["event_before_post"] != 1:
        failures.append(f"diag counts wrong: {diag}")

    summ = summarize(rows, diag)
    if summ["posted"] != 10 or summ["paired"] != 9:
        failures.append(f"posted/paired: {summ['posted']}/{summ['paired']} != 10/9")
    if summ["completed"] != 5:
        failures.append(f"expected 5 completed, got {summ['completed']}")
    if summ["excluded"] != 4:
        failures.append(f"expected 4 excluded, got {summ['excluded']}")
    if summ["reasons"].get("refused-landing") != 1:
        failures.append(f"refused-landing must be counted by reason: {summ['reasons']}")
    if summ["shared"] != 0:
        failures.append(f"expected 0 shared-frame keys, got {summ['shared']}")

    # seq 9 carries `landed=-` and an unknown token: it parses (the token
    # is dropped) and, the run having printed a refusal, is excluded -- so
    # every leg has n=5, from seq 1-5 alone.
    expected = {
        "stamp": (0.0, 5), "deliver": (300.0, 5), "write": (50.0, 5),
        "child": (10.0, 5), "wake": (5.0, 5), "build": (5.0, 5),
        "gpu": (10.0, 5), "show": (5.0, 5), "vsync": (35.0, 5),
        "vsync_hi": (51.7, 5), "to_shown": (385.0, 5), "total": (420.0, 5),
        "total_hi": (436.7, 5),
    }
    for name, (want, n) in expected.items():
        st = summ["legs"][name]
        if st is None or not _approx(st["median"], want) or st["n"] != n:
            failures.append(f"leg {name}: {st} != median {want} n {n}")
    # The IQR is inclusive and within the data; never the max-min spread.
    st = summ["legs"]["total"]
    if st and not (st["min"] <= st["q1"] <= st["median"] <= st["q3"] <= st["max"]):
        failures.append(f"quartiles out of order: {st}")

    # A `-` leg parses to None and is left out of its median; a key whose
    # vsync is `-` is not completed.
    partial = parse_keylat_line(_rec(9, 2000000000, landed="-", shown="-",
                                     vsync="-", vsync_hi="-"))
    if partial is None or partial["landed"] is not None or partial["vsync"] is not None:
        failures.append(f"missing legs should parse to None: {partial}")
    prow, _ = pair_by_order([(9, 2000000000, "z")], [partial])
    if is_completed(prow[0]):
        failures.append("a key with vsync=- must not count as completed")
    plegs = legs_of(prow[0])
    if plegs["gpu"] is not None or plegs["total"] is not None:
        failures.append("legs touching a missing stage must be None")
    if not _approx(plegs["build"], 5.0):
        failures.append(f"build leg of the partial row: {plegs['build']} != 5.0")

    # Strictness: a record missing a known token, or with a value that is
    # neither +<us> nor -, is a parse failure; unknown tokens are ignored.
    for bad in [
        "keylat: seq=1 event=1 handled=+1.0 written=+2.0 echo=- fed=- commit=- landed=- shown=- vsync=-",
        "keylat: seq=1 event=1 handled=1.0 written=+2.0 echo=- fed=- commit=- landed=- shown=- vsync=- vsync_hi=-",
        "keylat: seq=x event=1 handled=+1.0 written=+2.0 echo=- fed=- commit=- landed=- shown=- vsync=- vsync_hi=-",
    ]:
        try:
            parse_keylat_line(bad)
            failures.append(f"must be a parse failure: {bad}")
        except KeylatParseError:
            pass
    tol = parse_keylat_line(_rec(4, 4000000000) + " future=+1.0 note=x")
    if tol is None or tol["seq"] != 4 or "future" in tol:
        failures.append(f"unknown tokens must be tolerated and dropped: {tol}")

    # The `N keys open` report line, the refusal line and unrelated lines
    # are not records.
    for noise in ["keylat: 3 keys open",
                  "keylat: landing refused ended=1003500390000 now=1003500385000",
                  "startup: first present  101.0 ms"]:
        if parse_keylat_line(noise) is not None:
            failures.append(f"must not parse as a record: {noise}")

    # A negative leg after `stamp` -- here the GPU end after the wake that
    # reported it, show < 0 -- excludes the row and names the leg.
    neg = parse_keylat_line(_rec(5, 5000000000, landed="+390.0", shown="+385.0"))
    nrow, ndiag = pair_by_order([(5, 5000000000, "n")], [neg])
    if not nrow[0]["excluded"] or nrow[0]["exclude_reason"] != "negative show":
        failures.append(f"expected 'negative show' exclusion, got {nrow[0]['exclude_reason']}")
    if ndiag["negative"] != 1:
        failures.append(f"negative count: {ndiag}")

    # Shared-frame detection: commits within 200 ns share a frame. The
    # events are NOT multiples of 1000 ns and the true commit is one
    # instant, 5_000_123_456; each key's offset prints at 0.1 us from its
    # own event, so the two reconstruct 36 ns apart -- exact equality would
    # miss them and the case cannot pass by construction.
    c = 5_000_123_456
    e1, e2, e3 = 5_000_000_017, 5_000_010_053, 5_000_500_000
    off = lambda e, t: f"+{(t - e) / 1000.0:.1f}"

    def frame(e, at):  # one commit at `at`; the legs before it inside 40 us
        return dict(handled="+10.0", written="+20.0", echo="+30.0", fed="+40.0",
                    commit=off(e, at), landed=off(e, at + 10_000),
                    shown=off(e, at + 15_000), vsync=off(e, at + 30_000),
                    vsync_hi=off(e, at + 16_697_000))
    shared_klines = [
        _rec(1, e1, **frame(e1, c)),
        _rec(2, e2, **frame(e2, c)),
        _rec(3, e3, **frame(e3, e3 + 370_000)),  # its own frame
    ]
    sposts = [(1, e1, "a"), (2, e2, "b"), (3, e3, "c")]
    skeylats = [parse_keylat_line(l) for l in shared_klines]
    if skeylats[0]["commit"] == skeylats[1]["commit"]:
        failures.append("the shared-frame case must not reconstruct equal commits")
    if abs(skeylats[0]["commit"] - skeylats[1]["commit"]) > SHARED_FRAME_NS:
        failures.append("the shared-frame case must reconstruct within the tolerance")
    srows, _ = pair_by_order(sposts, skeylats)
    ssum = summarize(srows)
    if ssum["shared"] != 2:
        failures.append(f"expected 2 keys sharing a frame, got {ssum['shared']}")
    if shared_frame_count(srows, tolerance_ns=0) != 0:
        failures.append("with exact equality the case must NOT be shared (positive control)")

    # Refused landings. The CPU path -- every row `landed=-`, no refusal
    # line -- is untouched and complete; the same rows in a run that DID
    # print a refusal are all refused landings (the positive control); a
    # row that never reached the GPU (`commit=-`) is incomplete, not
    # refused, and an already-excluded row keeps its own reason.
    cpu_posts = [(i, 7_000_000_000 + i * 1_000_000, "c") for i in (1, 2, 3)]
    cpu_klines = [parse_keylat_line(_rec(i, 7_000_000_000 + i * 1_000_000, landed="-"))
                  for i in (1, 2, 3)]
    cpu_rows, _ = pair_by_order(cpu_posts, cpu_klines)
    if exclude_refused_landings(cpu_rows, refusals=0) != 0:
        failures.append("a CPU-path run (no refusal line) must exclude nothing")
    csum = summarize(cpu_rows)
    if csum["completed"] != 3 or csum["excluded"] != 0 or csum["legs"]["gpu"] is not None:
        failures.append(f"CPU-path rows must complete with no gpu leg: {csum['completed']}/{csum['excluded']}")
    gpu_rows, _ = pair_by_order(cpu_posts, cpu_klines)
    if exclude_refused_landings(gpu_rows, refusals=1) != 3:
        failures.append("with a refusal line every committed landed=- row is a refused landing")
    gsum = summarize(gpu_rows)
    if gsum["completed"] != 0 or gsum["reasons"].get("refused-landing") != 3:
        failures.append(f"refused-landing rows must not complete: {gsum['completed']} {gsum['reasons']}")
    if any(r["exclude_reason"] != "refused-landing" for r in gpu_rows):
        failures.append("the refused-landing reason must be on every row")
    mixed = [parse_keylat_line(_rec(1, 8_000_000_000, commit="-", landed="-",
                                    shown="-", vsync="-", vsync_hi="-")),
             parse_keylat_line(_rec(2, 8_001_000_000, landed="-"))]
    mrows, _ = pair_by_order([(1, 8_000_000_000, "m"), (3, 8_001_000_000, "m")], mixed)
    if exclude_refused_landings(mrows, refusals=2) != 0:
        failures.append("a never-committed row and an already-excluded row are not refused landings")
    if mrows[0]["excluded"] or is_completed(mrows[0]):
        failures.append("a row with commit=- is incomplete, neither excluded nor completed")
    if mrows[1]["exclude_reason"] != "seq-mismatch":
        failures.append(f"an excluded row keeps its reason: {mrows[1]['exclude_reason']}")
    if refused_runs({"base": [{"landing_refused": 0}, {"landing_refused": 2}],
                     "aa": [{"landing_refused": 0}, {}]}) != [("base", 2, 2)]:
        failures.append("refused_runs must name the arm, the round and the count")

    # The warm-up round: a discarded round 1 that completed nothing is
    # said; a round 1 that worked is not; a single round is not discarded.
    wrow, _ = pair_by_order([(1, 9_000_000_000, "w")],
                            [parse_keylat_line(_rec(1, 9_000_000_000))])
    if not warmup_measured_nothing([[], [wrow[0]]]):
        failures.append("an empty warm-up round must be reported")
    if warmup_measured_nothing([[wrow[0]], [wrow[0]]]):
        failures.append("a warm-up round that completed keys must not be reported")
    if warmup_measured_nothing([[]]):
        failures.append("a single round is used, not discarded; settled_rounds flags it")

    # The gap bounds are gap -/+ a quarter (0 stays 0), and the pairing
    # window follows the MINIMUM jittered gap below 100 ms, floored at 1.
    if gap_bounds(150) != (112.5, 187.5) or gap_bounds(0) != (0, 0):
        failures.append(f"gap_bounds: {gap_bounds(150)} {gap_bounds(0)}")
    for gap, want in [(150, 100), (134, 100), (100, 70), (50, 32), (6, 1), (0, 1)]:
        if window_for_gap(gap) != want:
            failures.append(f"window_for_gap({gap}) = {window_for_gap(gap)} != {want}")
    wrow, wdiag = pair_by_order([(1, 1_000_000_000, "a")],
                                [parse_keylat_line(_rec(1, 1_050_000_000))],
                                window_ns=45 * 1_000_000)
    if wrow[0]["exclude_reason"] != "event-post>=window":
        failures.append("a 50 ms stamp delta must be excluded under a 45 ms window")

    # The arm order rotates: over three rounds each of three arms is first
    # once and `aa` is last only once.
    arms = [("base", "b"), ("new", "n"), ("aa", "b")]
    orders = [rotate(arms, r) for r in (1, 2, 3)]
    if sorted(o[0][0] for o in orders) != ["aa", "base", "new"]:
        failures.append(f"rotation must put every arm first once: {orders}")
    if sum(1 for o in orders if o[-1][0] == "aa") != 1:
        failures.append(f"aa must not always be last: {orders}")

    # The A/A floor is robust: one 163 ms outlier in a leg of 10 us values
    # must not own it (max-min would be ~163 ms).
    st_b = leg_stats([10.0, 11.0, 10.0, 12.0, 10.0, 11.0, 163000.0])
    st_a = leg_stats([12.0, 10.0, 13.0, 11.0, 10.0, 12.0, 11.0])
    fl = aa_floor({"total": st_b}, {"total": st_a})["total"]
    if fl is None or fl["floor"] > 100.0 or fl["floor"] < fl["dmed"]:
        failures.append(f"the A/A floor must be robust to one outlier: {fl}")
    if st_b["max"] - st_b["min"] < 100000.0:
        failures.append("the outlier control did not carry an outlier")

    # Arm deltas against the floor: `result` needs |delta| above the floor
    # AND disjoint IQRs; a delta inside the floor is not one even with
    # disjoint IQRs, a delta above the floor is not one when the IQRs
    # overlap, and with no floor nothing is.
    d_base = leg_stats([95.0, 98.0, 100.0, 102.0, 105.0])       # IQR [98-102]
    d_res = leg_stats([190.0, 195.0, 200.0, 205.0, 210.0])      # +100, [195-205]
    d_in = leg_stats([100.0, 103.0, 105.0, 107.0, 110.0])       # +5, [103-107]
    d_ov = leg_stats([80.0, 90.0, 150.0, 160.0, 170.0])         # +50, [90-160]
    fl10 = {"total": {"floor": 10.0}}
    dd = arm_deltas({"total": d_base}, {"total": d_res}, fl10)["total"]
    if dd["verdict"] != "result" or not _approx(dd["delta"], 100.0) or not dd["disjoint"]:
        failures.append(f"a delta above the floor with disjoint IQRs is a result: {dd}")
    dd = arm_deltas({"total": d_base}, {"total": d_in}, fl10)["total"]
    if not dd["verdict"].startswith("within floor -- not a result") or not dd["disjoint"]:
        failures.append(f"a delta inside the floor is not a result even with disjoint IQRs: {dd}")
    dd = arm_deltas({"total": d_base}, {"total": d_ov}, fl10)["total"]
    if dd["verdict"] != "IQRs overlap -- not a result" or dd["disjoint"]:
        failures.append(f"overlapping IQRs are not a result whatever the delta: {dd}")
    dd = arm_deltas({"total": d_base}, {"total": d_res}, None)["total"]
    if dd["verdict"] != "no A/A floor -- not a result":
        failures.append(f"without an A/A floor nothing is a result: {dd}")
    if arm_deltas({"total": d_base}, {"total": None}, fl10)["total"] is not None:
        failures.append("a leg one arm lacks has no delta")

    # A single round is warm-up and must be reported as such.
    if settled_rounds([["x"]]) != ([["x"]], True):
        failures.append("one round must be used and flagged as warm-up")
    if settled_rounds([["x"], ["y"], ["z"]]) != ([["y"], ["z"]], False):
        failures.append("round 1 must be discarded when there are more")

    # The child modes, including the no-echo control.
    if child_env("none").get("BAUD_CMD") != "stty -echo; sleep 60":
        failures.append(f"--child none must set the no-echo control: {child_env('none')}")
    if child_env("cat").get("BAUD_CMD") != "cat" or child_env("shell") != {}:
        failures.append("cat/shell child env wrong")

    # The helper's --front line.
    fr = parse_front_line("front 1390 Safari app 1390 Safari")
    if fr != (1390, "Safari", 1390, "Safari"):
        failures.append(f"front line parse: {fr}")

    if failures:
        print("SELFTEST FAILED:")
        for fmsg in failures:
            print(f"  - {fmsg}")
        return 3
    print("SELFTEST OK")
    print(f"  fixtures: {kfile}")
    print(f"            {pfile}")
    print(f"  parsed {len(keylats)} keylat records, {len(posts)} posts, "
          f"{len(pfail)} parse failure ({pfail[0].split(':')[0]} on seq=10), "
          f"{refusals} landing refused")
    print(f"  pairing: 5 completed, 4 excluded ({', '.join(f'seq={k}: {v}' for k, v in sorted(ex.items()))})")
    print_summary("fixture", summ, diag)
    print("\n  extra cases: strict tokens (missing/bare value -> parse failure, "
          "unknown token dropped); `-` legs -> None and not completed;\n"
          "  `N keys open`/`landing refused`/noise skipped; negative leg "
          "excluded; 2 keys share a frame within 200 ns off non-multiple-\n"
          "  of-1000 events (0 with exact equality); refused landings: CPU "
          "path untouched, GPU path with a refusal excludes every\n"
          "  committed landed=- row, commit=- stays incomplete, an excluded "
          "row keeps its reason, refused_runs names arm/round/count;\n"
          "  empty warm-up round reported; gap bounds -/+ a quarter and the "
          "window follows the MINIMUM gap; arm order rotates;\n"
          "  A/A floor ignores a 163 ms outlier; arm deltas: result only "
          "above the floor with disjoint IQRs, three not-a-result\n"
          "  verdicts; one round flagged warm-up; --child none is the "
          "no-echo control.")
    return 0


def main():
    ap = argparse.ArgumentParser(description="keystroke-to-pixel latency for baud")
    ap.add_argument("--selftest", action="store_true",
                    help="run the parser/pairing/statistics unit tests and exit")
    ap.add_argument("--rounds", type=int, default=5)
    ap.add_argument("--keys", type=int, default=30)
    ap.add_argument("--gap", type=int, default=150,
                    help="MEAN ms between keys; the helper jitters each gap uniformly "
                         "by +-25%% (0 stays 0: typeahead, the guard bounds the rate)")
    ap.add_argument("--child", choices=["cat", "shell", "none"], default="cat",
                    help="cat: the tty's own echo; shell: the login shell; "
                         "none: `stty -echo; sleep 60`, the no-echo control")
    ap.add_argument("--arm", action="append", default=None,
                    help="name=path/to/baud (repeatable)")
    ap.add_argument("--aa", action="store_true",
                    help="add an A/A arm running the first arm's binary again")
    ap.add_argument("--grid", default="120x40")
    ap.add_argument("--chars", default="asdfjkl",
                    help="plain letters the helper cycles through")
    ap.add_argument("--scratch", default=os.environ.get(
        "BAUD_SCRATCH", tempfile.gettempdir()))
    args = ap.parse_args()

    if args.selftest:
        sys.exit(selftest())

    if args.rounds < 1 or args.keys < 1 or args.gap < 0:
        print("--rounds and --keys must be >= 1, --gap >= 0")
        sys.exit(1)
    if not args.arm:
        args.arm = ["base=target/release/baud"]
    for a in args.arm:
        if "=" not in a:
            print(f"--arm wants name=path, got {a!r}")
            sys.exit(1)
    sys.exit(sweep(args))


if __name__ == "__main__":
    main()
