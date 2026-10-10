#!/usr/bin/env python3
"""XTGETTCAP: table sizes and reply bounds for each option in note AM.

Reads only pinned or captured files. Prints the numbers the note quotes.
Run: python3 bounds.py
"""
import re, pathlib

ROOT = pathlib.Path(__file__).resolve().parents[4]
SRC = ROOT / "docs/research/sources"
HERE = pathlib.Path(__file__).resolve().parent

def parse_ti(path):
    caps = {}
    for line in path.read_text().splitlines():
        if line.startswith("#") or not line.startswith("\t"):
            continue
        item = line.strip().rstrip(",")
        if "=" in item:
            k, v = item.split("=", 1)
            caps[k] = ("str", v)
        elif "#" in item:
            k, v = item.split("#", 1)
            caps[k] = ("num", v)
        else:
            caps[item] = ("bool", None)
    return caps

def decode_ti(v):
    """terminfo(5) escapes to bytes; % parameters left as text (foot's form)."""
    out = bytearray(); i = 0
    while i < len(v):
        c = v[i]
        if c == "^" and i + 1 < len(v):
            n = v[i + 1]; out.append(0x7F if n == "?" else ord(n) & 0x1F); i += 2
        elif c == "\\" and i + 1 < len(v):
            n = v[i + 1]
            if re.match(r"[0-3][0-7][0-7]", v[i + 1:i + 4]):
                out.append(int(v[i + 1:i + 4], 8)); i += 4; continue
            m = {"E": 0x1B, "e": 0x1B, "n": 10, "l": 10, "r": 13, "t": 9, "b": 8,
                 "f": 12, "s": 32, "0": 0x80}
            out.append(m.get(n, ord(n))); i += 2
        else:
            out.append(ord(c)); i += 1
    return bytes(out)

def tcap_aliases():
    """terminfo name -> termcap code, from the pinned terminfo(5) tables."""
    al = {}
    for line in (SRC / "terminfo/terminfo.5").read_text(errors="replace").splitlines():
        m = re.match(r"^([a-z_0-9]+)\t([A-Za-z0-9]+)\t(\S\S)\tT\{", line)
        if m:
            al[m.group(2)] = m.group(3)
    return al

ALIAS = tcap_aliases()

def value_bytes(kind, v, form):
    if kind == "bool":
        return None
    if kind == "num":
        return v.encode()
    if form == "decoded":          # foot: \E -> ESC, % left as text
        return decode_ti(v)
    if form == "source-if-param":  # kitty/ghostty: source text if % present
        return v.encode() if "%" in v else decode_ti(v)
    raise ValueError(form)

INTRO7, ST7 = 2, 2   # ESC P, ESC \ : replies under S7C1T, the longer form
INTRO8, ST8 = 1, 1   # 90, 9C : a query under ESC % @ (Latin-1 C1)

def reply_one(name, val):
    """One DCS per name: DCS 1 + r <hex name> [= <hex value>] ST, 7-bit."""
    body = 3 + 2 * len(name) + (0 if val is None else 1 + 2 * len(val))
    return INTRO7 + body + ST7

def report(label, caps, form, with_termcap):
    rows = []
    for k, (kind, v) in caps.items():
        val = value_bytes(kind, v, form)
        names = [k] + ([ALIAS[k]] if with_termcap and k in ALIAS else [])
        for n in names:
            rows.append((n, k, val))
    longest = max(rows, key=lambda r: (0 if r[2] is None else len(r[2])))
    # worst single-name ratio: shortest query is 90 + q <hex> 9C (8-bit C1)
    def q_single(n): return INTRO8 + 2 + 2 * len(n) + ST8
    worst_single = max(rows, key=lambda r: reply_one(r[0], r[2]) / q_single(r[0]))
    # worst marginal ratio for one more name in a list: in ";<hex>", out
    # ";<hex>=<hex val>" (single-response form) or a whole DCS (one per name)
    def marg_in(n): return 1 + 2 * len(n)
    def marg_out_single(n, v): return 1 + 2 * len(n) + (0 if v is None else 1 + 2 * len(v))
    worst_m_single = max(rows, key=lambda r: marg_out_single(r[0], r[2]) / marg_in(r[0]))
    worst_m_each = max(rows, key=lambda r: reply_one(r[0], r[2]) / marg_in(r[0]))
    table = sum(len(n) + (0 if v is None else len(v)) + 2 for n, _, v in rows)
    prebuilt = sum(reply_one(n, v) for n, _, v in rows)
    print(f"== {label}")
    print(f"   names answered: {len(rows)} ({len(caps)} capabilities"
          f"{', with termcap codes' if with_termcap else ''})")
    print(f"   longest value: {longest[1]} via '{longest[0]}', "
          f"{len(longest[2])} B -> one reply {reply_one(longest[0], longest[2])} B")
    n, _, v = worst_single
    print(f"   worst single-name ratio: '{n}' {reply_one(n, v)} B / {q_single(n)} B = "
          f"{reply_one(n, v) / q_single(n):.2f}")
    n, _, v = worst_m_single
    print(f"   worst marginal ratio, one response: '{n}' {marg_out_single(n, v)} / "
          f"{marg_in(n)} = {marg_out_single(n, v) / marg_in(n):.2f}")
    n, _, v = worst_m_each
    print(f"   worst marginal ratio, one DCS per name: '{n}' {reply_one(n, v)} / "
          f"{marg_in(n)} = {reply_one(n, v) / marg_in(n):.2f}")
    print(f"   table, names + values: {table} B; as prebuilt replies: {prebuilt} B")
    return rows

pinned = parse_ti(SRC / "terminfo/xterm-256color.ti")
captured = parse_ti(SRC / "terminfo/xterm-256color-x.ti")
print(f"terminfo(5) aliases parsed: {len(ALIAS)} (positive control: colors->"
      f"{ALIAS.get('colors')}, sgr->{ALIAS.get('sgr')}, kbs->{ALIAS.get('kbs')})")
print(f"pinned entry: {len(pinned)} capabilities; captured -x entry: {len(captured)}")
extra = sorted(set(captured) - set(pinned))
print(f"in the -x capture and not the pin ({len(extra)}): {' '.join(extra)}")
print(f"pinned kbs = {pinned['kbs'][1]!r} (baud's Backspace sends 0x7f, key.rs)")

for form in ("decoded", "source-if-param"):
    report(f"T: the pinned entry, terminfo names, {form}", pinned, form, False)
    report(f"T+tc: the pinned entry, terminfo and termcap names, {form}", pinned, form, True)
report("T-x: the -x capture, terminfo and termcap names, source-if-param", captured,
       "source-if-param", True)

# Option K: xterm's built-in names. Values are what the encoder sends, so the
# bound is KeyBytes::MAX_LEN (48, baud-domain/src/vt/key.rs) for a key;
# TN, Co and RGB are fixed.
KEYMAX = 48
k_rows = [("TN", b"xterm-256color"), ("name", b"xterm-256color"), ("Co", b"256"),
          ("colors", b"256"), ("RGB", b"8"), ("kb", b"x" * KEYMAX), ("k1", b"x" * KEYMAX)]
print("== K: xterm's table (keys from the encoder, bounded by KeyBytes::MAX_LEN = 48)")
for n, v in k_rows:
    q = INTRO8 + 2 + 2 * len(n) + ST8
    print(f"   '{n}': reply {reply_one(n, v)} B / query {q} B = {reply_one(n, v) / q:.2f};"
          f" marginal (one DCS each) {reply_one(n, v)}/{1 + 2 * len(n)} ="
          f" {reply_one(n, v) / (1 + 2 * len(n)):.2f}")

# Real askers' replies, the bytes each needs. Arithmetic over an assumed
# reply shape (C), not a measurement: baud answers no XTGETTCAP.
print("== real queries, 7-bit replies (C: an assumed reply shape)")
ms = b"\x1b]52;%p1%s;%p2%s\x07"
print(f"   neovim Ms (captured entry's value, decoded, % left as text), one name: query"
      f" {2 + 2 + 4 + 2} B, reply {reply_one('Ms', ms)} B")
# Tc as a boolean (no value), RGB as `8` (foot's and ghostty's answer; kitty
# answers RGB 0+r), and setrgbf/setrgbb as kitty's own source text, `\E` and
# colons (kitty/terminfo.py 281, 283, pinned as kitty-9710eb4-terminfo-py),
# which is how kitty sends a `%` string (terminfo.py 576-578).
four = [("Tc", None), ("RGB", b"8"), ("setrgbf", rb"\E[38:2:%p1%d:%p2%d:%p3%dm"),
        ("setrgbb", rb"\E[48:2:%p1%d:%p2%d:%p3%dm")]
q = 2 + 2 + sum(2 * len(n) for n, _ in four) + 3 + 2
one_resp = 2 + 3 + sum(2 * len(n) + (0 if v is None else 1 + 2 * len(v)) for n, v in four) + 3 + 2
each = sum(reply_one(n, v) for n, v in four)
print(f"   neovim Tc;RGB;setrgbf;setrgbb, every name answered (Tc boolean, RGB=8,"
      f" setrgbf/setrgbb in kitty's source text): query {q} B,"
      f" one response {one_resp} B, one DCS each {each} B")
DECRQSS_REPLY, DECRQSS_Q = 111, 5
print(f"   (DECRQSS, note AG: {DECRQSS_REPLY}/{DECRQSS_Q} = {DECRQSS_REPLY / DECRQSS_Q:.1f})")

# ---- Per-DCS and per-read bounds against note AG (OSC_MAX = 512, READ_CHUNK = 4096).
OSC_MAX, L = 512, 4096
AG_LARGEST, AG_CARRY, AG_PER = 1131, 106, 111   # note AG 1.2 (corrected)
AG_BOUND = AG_LARGEST + ((L - 1) // 5) * AG_PER + AG_CARRY
REFUSAL = INTRO7 + 3 + ST7                       # DCS 0 + r ST, 7-bit: 7 B
print(f"== per-DCS and per-read bounds (OSC_MAX {OSC_MAX}, READ_CHUNK {L}); AG never-drops bound {AG_BOUND} B, 96 KiB = {96*1024}")

# How many body bytes the names get. A prefix node that consumes `+ q`, as
# Node::DcsDollar and Act::DcsQuery consume DECRQSS's `$ q` (graph.rs 861-868
# at 5477bf7), leaves all of OSC_MAX to the names: BODY_PREFIX. Today's path
# takes `+` by the catch-all into Node::Str and stores the `q` in
# strs.pending, so 511 bytes are left: BODY_TODAY (xtgettcap-controls-output.txt
# Q3: 511 B of hex after `q` truncate nothing, 512 truncate one). The note's
# figures assume BODY_PREFIX; BODY_TODAY's are printed beside them.
BODY_PREFIX, BODY_TODAY = OSC_MAX, OSC_MAX - 1

def per_dcs(name_hex_len, val_len, form, body=BODY_PREFIX):
    k = (body + 1) // (name_hex_len + 1)               # names in a full body
    pair = name_hex_len + (0 if val_len is None else 1 + 2 * val_len)
    if form == "one":
        return k, INTRO7 + 3 + k * pair + (k - 1) + ST7
    return k, k * (INTRO7 + 3 + pair + ST7)

def refusals_one_body(body=BODY_PREFIX, echo=True):
    """R2's largest reply drawn by invalid names alone: a body of names each
    refused in a DCS of its own, the name echoed (kitty, foot, wezterm) or not.
    Sweeps every name length; an empty name (`;;`) is a name of length 0."""
    best = (0, 0, 0)
    for h in range(0, body + 1):
        k = (body + 1) // (h + 1)
        total = k * (REFUSAL + (h if echo else 0))
        best = max(best, (total, h, k))
    return best

def mixed_each(nh, v, body=BODY_PREFIX):
    """R2's largest reply from one body that mixes valid names (hex length nh,
    value v) with refused ones, each in a DCS of its own. a valid and e
    refused names fit when a*nh + e*h + (a + e - 1) <= body. Refused names are
    taken empty (h = 0): refusals_one_body sweeps every h and shows h = 0 is
    the largest, since (7 + h) / (h + 1) <= 7. Returns (bytes, a, e)."""
    best = (0, 0, 0)
    valid = INTRO7 + 3 + nh + (0 if v is None else 1 + 2 * v) + ST7
    for a in range(0, (body + 1) // (nh + 1) + 1):
        e = body + 1 - a * (nh + 1)
        best = max(best, (a * valid + e * REFUSAL, a, e))
    return best

def body_reply(nh, v, form, b):
    """The largest reply one query with a body of b bytes can draw."""
    if form == "each":
        return mixed_each(nh, v, b)[0]
    k = (b + 1) // (nh + 1)
    pair = nh + (0 if v is None else 1 + 2 * v)
    names = INTRO7 + 3 + k * pair + (k - 1) + ST7 if k else 0
    return max(names, REFUSAL + b)      # or one invalid name, echoed whole

_PER_READ = {}
def per_read(name_hex_len, val_len, form, length=L, body=BODY_PREFIX):
    """Bytes owed by a read made wholly of XTGETTCAP queries, 90 + q <b> 9C
    with 0 <= b <= body, counting only queries wholly inside it: a dynamic
    programme over every body length, so short bodies and leftover bytes are
    priced rather than assumed away."""
    key = (name_hex_len, val_len, form, body)
    if key not in _PER_READ:
        rep = [body_reply(name_hex_len, val_len, form, b) for b in range(body + 1)]
        f = [0] * (L + 1)
        for x in range(1, L + 1):
            best = f[x - 1]
            for b in range(0, min(body, x - INTRO8 - 2 - ST8) + 1):
                best = max(best, rep[b] + f[x - (INTRO8 + 2 + ST8) - b])
            f[x] = best
        _PER_READ[key] = f
    return _PER_READ[key][length]

# The longest value one name can answer. Key rows: keymax-output.txt, baud's
# encode_key at 5477bf7 measured over each domain (M). TN: the TERM pty.rs
# exports (C). 48: KeyBytes::MAX_LEN, a buffer size, not a measurement.
cases = [
    ("K, KeyBytes::MAX_LEN (48, a bound)", 4, 48),
    ("K, encode_key's global maximum (40, D0: any key, modifier byte, kind, text, alternates, state)", 4, 40),
    ("K, table keys under any of the 256 modifier bytes (12, D3: presses, no text, no alternates, every KeyState)", 4, 12),
    ("K, table keys under xterm's shift/alt/ctrl (11, D2: presses, no text, no alternates, every KeyState)", 4, 11),
    ("K, xterm's table rows exactly (10, D1: each row's own modifiers, presses, no text, no alternates, every KeyState)", 4, 10),
    ("K, TN = xterm-256color (14)", 4, 14),
    ("K, the key probe's longest, base layer, 105 names, two states (6)", 4, 6),
    ("T+tc, 'Ic' (initc, 97 B source form)", 4, 97),
]
for label, nh, v in cases:
    for form in ("one", "each"):
        k, d = per_dcs(nh, v, form)
        r = per_read(nh, v, form)
        print(f"   {label}, {'one response' if form=='one' else 'one DCS per name'}: "
              f"{k} names fill a body; one DCS answers {d} B; an all-XTGETTCAP read,"
              f" counting only queries wholly inside it, {r} B")

for body, tag in ((BODY_PREFIX, "prefix consumes q"), (BODY_TODAY, "q stored, today's path")):
    tot, h, k = refusals_one_body(body, echo=True)
    tot0, h0, k0 = refusals_one_body(body, echo=False)
    print(f"   R2, invalid names only, body {body} B ({tag}): largest reply {tot} B, from {k} names of"
          f" {h} hex digits, echoed; {tot0} B unechoed ({k0} names of {h0} digits)")
print(f"   R2, a read of empty-name queries alone (90 q ;;; 9C): {per_read(0, None, 'each')} B,"
      f" against DECRQSS's {((L - 1) // 5) * AG_PER} B for L-1 bytes: the per-byte term stays DECRQSS's")

# ---- The straddle (B1 of the verifier's report). baud keeps a DCS body across
# reads, and the terminator alone in the next read runs the action there:
# straddle-output.txt measures it on DECRQSS (A1-A4), on a 509-byte DCS + q
# body (B1, B2), and measures AG's own shape on DECRQSS, 820 answers owed by
# one 4096-byte read (C1). So note AG's `floor((L-1)/5)` term is the
# straddle-correct bound, not a loose one: the largest action opens the read
# with one byte, and the other L-1 bytes carry the per-byte maximum. Under N1
# that maximum is DECRQSS's (22.2) as long as a key value is at most 49 B (one
# DCS per name) or 52 B (one response); under N3 it is the capability's own.
# Under R2 the largest single body is also at least the invalid-names body
# (refusals_one_body), which an empty-name body of `;` reaches.
print(f"== the straddle: largest action opened by a 1-byte terminator, then L-1 = {L - 1} bytes")
def one_body(nh, v, form, body=BODY_PREFIX):
    _, d = per_dcs(nh, v, form, body)
    if form == "each":
        d = max(d, mixed_each(nh, v, body)[0])
    return d
def per_read_upper(nh, v, form, length, body=BODY_PREFIX):
    """A cheap ceiling on per_read: the best bytes-out per byte-in of any one
    query, times the read. When it is under DECRQSS's term the DP is moot."""
    return max(body_reply(nh, v, form, b) / (INTRO8 + 2 + ST8 + b) for b in range(body + 1)) * length
def straddle(nh, v, form, body=BODY_PREFIX, per=AG_PER):
    decrqss = ((L - 1) // 5) * per
    if per_read_upper(nh, v, form, L - 1, body) <= decrqss:
        rest = decrqss
    else:
        rest = max(decrqss, per_read(nh, v, form, L - 1, body))
    return max(AG_LARGEST, one_body(nh, v, form, body)) + rest + AG_CARRY
LIMIT = 96 * 1024
for label, nh, v in cases:
    for form in ("one", "each"):
        b = straddle(nh, v, form)
        print(f"   {label}, {'R1 one response' if form == 'one' else 'R2 one DCS per name'}: "
              f"one full body {one_body(nh, v, form)} B; a read it opens {b} B"
              f" ({'over' if b > LIMIT else 'within'} 96 KiB = {LIMIT})")
for label, nh, v in cases:
    tot, a, e = mixed_each(nh, v)
    print(f"   R2 one body, {label}: largest {tot} B = {a} answered names + {e} refused empty names"
          f" (names alone {per_dcs(nh, v, 'each')[1]} B; refusals alone {refusals_one_body()[0]} B)")
QUERY_FULL = INTRO8 + 2 + OSC_MAX + ST8          # 90 + q <512 body> 9C: 516 B
for v in (48, 12):
    r1, r2 = one_body(4, v, "one"), one_body(4, v, "each")
    print(f"   per body byte at {v} B a key: R1 {r1}/{OSC_MAX} = {r1 / OSC_MAX:.1f}, R2 {r2}/{OSC_MAX} ="
          f" {r2 / OSC_MAX:.1f}; per query byte (the whole {QUERY_FULL}-byte query) R1 {r1}/{QUERY_FULL} ="
          f" {r1 / QUERY_FULL:.1f}, R2 {r2}/{QUERY_FULL} = {r2 / QUERY_FULL:.1f};"
          f" marginal name {reply_one('kb', b'x' * v)}/5 = {reply_one('kb', b'x' * v) / 5:.1f};"
          f" an empty name {REFUSAL}/1; DECRQSS {AG_PER}/5 = {AG_PER / 5:.1f}")
for form in ("one", "each"):
    vmax = max(v for v in range(0, 200) if straddle(4, v, form) <= LIMIT)
    print(f"   {'R1' if form == 'one' else 'R2'}: the longest value a two-character name may answer"
          f" and keep 96 KiB is {vmax} B (straddle at {vmax}: {straddle(4, vmax, form)} B;"
          f" at {vmax + 1}: {straddle(4, vmax + 1, form)} B)")
print(f"   q stored (body {BODY_TODAY} B), where it differs: R2 at 10 B {straddle(4, 10, 'each', BODY_TODAY)} B,"
      f" at 11 B {straddle(4, 11, 'each', BODY_TODAY)} B, at 6 B {straddle(4, 6, 'each', BODY_TODAY)} B;"
      f" R1 and R2 at 12 B {straddle(4, 12, 'one', BODY_TODAY)} / {straddle(4, 12, 'each', BODY_TODAY)} B;"
      f" thresholds R1 {max(v for v in range(200) if straddle(4, v, 'one', BODY_TODAY) <= LIMIT)} B,"
      f" R2 {max(v for v in range(200) if straddle(4, v, 'each', BODY_TODAY) <= LIMIT)} B")
for body, tag in ((BODY_PREFIX, "prefix consumes q"), (BODY_TODAY, "q stored, today's path")):
    k_tn, _ = per_dcs(4, 0, "one", body)
    k_name, _ = per_dcs(8, 0, "one", body)
    print(f"   TN, body {body} B ({tag}): a full body holds {k_tn} 'TN' names, so the reply grows"
          f" {2 * k_tn} B per byte of TERM ('name': {k_name} names, {2 * k_name} B per byte); TERM is"
          f" embedder-set, so its length is bounded by the same limit as a key value")

# ---- With the `q` stored (a 511-byte body), every case: where a figure moves,
# it is R2's, through the refused empty names that fill the body's last bytes.
print(f"== q stored (body {BODY_TODAY} B), every case, against the prefix body ({BODY_PREFIX} B)")
for label, nh, v in cases:
    for form in ("one", "each"):
        a, b = straddle(nh, v, form), straddle(nh, v, form, BODY_TODAY)
        print(f"   {label}, {'R1' if form == 'one' else 'R2'}: a read it opens {a} B with the prefix,"
              f" {b} B with the q stored{'' if a == b else ' (moves)'}")

# ---- DECRQSS in the colon form (question 6). The colon form spells a direct
# colour `38:2::r:g:b`, one byte longer than `38;2;r;g;b` (the empty colour-space
# field); an indexed colour, `38:5:n`, is as long as `38;5;n`. The richest
# rendition (baud-core/tests/vt220/dcs.rs 132-133, RICH) carries three direct
# colours, 58, 38 and 48, so its 111-byte answer becomes 114 B (C).
AG_PER_COLON = AG_PER + 3
print(f"== DECRQSS in the colon form: the richest answer {AG_PER} -> {AG_PER_COLON} B (C),"
      f" per byte {AG_PER / 5:.1f} -> {AG_PER_COLON / 5:.1f}")
print(f"   AG's never-drops buffer: {AG_LARGEST} + {(L - 1) // 5} x {AG_PER_COLON} + {AG_CARRY} ="
      f" {AG_LARGEST + ((L - 1) // 5) * AG_PER_COLON + AG_CARRY} B (was {AG_BOUND} B)")
for body, tag in ((BODY_PREFIX, "prefix consumes q"), (BODY_TODAY, "q stored")):
    for form in ("one", "each"):
        vmax = max(v for v in range(0, 200) if straddle(4, v, form, body, AG_PER_COLON) <= LIMIT)
        print(f"   {'R1' if form == 'one' else 'R2'} ({tag}): the longest value a two-character name may"
              f" answer and keep 96 KiB is {vmax} B (at {vmax}: {straddle(4, vmax, form, body, AG_PER_COLON)} B;"
              f" at {vmax + 1}: {straddle(4, vmax + 1, form, body, AG_PER_COLON)} B)")
for label, nh, v in cases:
    for form in ("one", "each"):
        b = straddle(nh, v, form, BODY_PREFIX, AG_PER_COLON)
        print(f"   colon form, {label}, {'R1' if form == 'one' else 'R2'}: a read it opens {b} B"
              f" ({'over' if b > LIMIT else 'within'} 96 KiB)")

# ---- N2: N1 plus named truths, priced at each name's own length. A full body
# can mix names, so the largest reply one body draws is an unbounded knapsack
# over the body's bytes: each name costs its hex digits plus one `;` (the body
# plus one, 513, holds them), and returns its pair (R1) or its own DCS (R2).
# Values are the longest spelling any source here gives for the name, in the
# terminfo source text kitty and ghostty send for a `%` string (C, an assumed
# value: baud's own would be fixed by the test that shows baud acts on it).
def knap(items, form, body=BODY_PREFIX):
    """items: (name, value length or None). The largest reply one body draws."""
    cap = body + 1
    best = [0] * (cap + 1)
    for c in range(1, cap + 1):
        b = best[c - 1]
        for n, v in items:
            cost = 2 * len(n) + 1
            if cost > c:
                continue
            extra = 0 if v is None else 1 + 2 * v
            gain = (2 * len(n) + 1 + extra) if form == "one" else (INTRO7 + 3 + 2 * len(n) + extra + ST7)
            b = max(b, best[c - cost] + gain)
        if form == "each":
            b = max(b, best[c - 1] + REFUSAL)           # an empty name, refused in its own DCS
        best[c] = b
    return (INTRO7 + 3 + ST7 - 1 + best[cap]) if form == "one" else best[cap]

N1_ITEMS = [("kb", 10), ("TN", 14), ("name", 14), ("Co", 3), ("colors", 3), ("RGB", 1)]
xcap = captured
def srcval(k):
    kind, v = xcap[k]
    return len(value_bytes(kind, v, "source-if-param"))
N2_ITEMS = [
    ("Tc", None), ("Su", None),
    ("Smulx", len(r"\E[4:%p1%dm")),                                         # foot.info 45, kitty terminfo.py 252
    ("Setulc", len(r"\E[58:2::%p1%{65536}%/%d:%p1%{256}%/%{255}%&%d:%p1%{255}%&%d%;m")),  # foot.info 44, ghostty.zig 114
    ("setrgbf", len(r"\E[38:2::%p1%d:%p2%d:%p3%dm")),                       # foot.info 261
    ("setrgbb", len(r"\E[48:2::%p1%d:%p2%d:%p3%dm")),                       # foot.info 260
    ("Sync", len(r"\E[?2026%?%p1%{1}%-%tl%eh%;")),                          # foot.info 47
    ("Ss", srcval("Ss")), ("Se", srcval("Se")), ("Cr", srcval("Cr")), ("Cs", srcval("Cs")),
    ("E3", srcval("E3")),
]
print("== N2: N1 plus named truths, each priced at its own name length (C: assumed values, the longest spelling read)")
print("   values: " + ", ".join(f"{n} {'boolean' if v is None else str(v) + ' B'}" for n, v in N2_ITEMS))
for form in ("one", "each"):
    worst = max(N1_ITEMS + N2_ITEMS, key=lambda it: (
        (2 * len(it[0]) + 1 + (0 if it[1] is None else 1 + 2 * it[1])) if form == "one"
        else (INTRO7 + 3 + 2 * len(it[0]) + (0 if it[1] is None else 1 + 2 * it[1]) + ST7)) / (2 * len(it[0]) + 1))
    ms_src = len(value_bytes("str", xcap["Ms"][1], "source-if-param"))
    for items, tag in ((N1_ITEMS, "N1 alone"), (N1_ITEMS + N2_ITEMS, "N1 + N2"),
                       (N1_ITEMS + N2_ITEMS + [("Ms", ms_src)], "N1 + N2 + Ms as source text")):
        d = knap(items, form)
        b = max(AG_LARGEST, d) + ((L - 1) // 5) * AG_PER + AG_CARRY
        bc = max(AG_LARGEST, d) + ((L - 1) // 5) * AG_PER_COLON + AG_CARRY
        print(f"   {'R1' if form == 'one' else 'R2'}, {tag}: largest full body {d} B; a read it opens {b} B"
              f" ({'over' if b > LIMIT else 'within'} 96 KiB); with DECRQSS in the colon form {bc} B"
              f" ({'over' if bc > LIMIT else 'within'})")
    n, v = worst
    print(f"   {'R1' if form == 'one' else 'R2'}: the highest marginal ratio of any one name is {n}'s; every"
          f" name's is under DECRQSS's {AG_PER / 5:.1f}, so the per-byte term stays DECRQSS's")
def name_limit(n, form, per):
    """The longest value name n may answer, beside N1's names, and keep 96 KiB."""
    v = 0
    while max(AG_LARGEST, knap(N1_ITEMS + [(n, v + 1)], form)) + ((L - 1) // 5) * per + AG_CARRY <= LIMIT:
        v += 1
    return v
MS_DECODED = len(value_bytes("str", xcap["Ms"][1], "decoded"))
MS_SOURCE = len(value_bytes("str", xcap["Ms"][1], "source-if-param"))
for n, v in N2_ITEMS + [("Ms", MS_SOURCE)]:
    lim = {(per, f): name_limit(n, f, per) for per in (AG_PER, AG_PER_COLON) for f in ("one", "each")}
    vals = [v] if n != "Ms" else [MS_DECODED, MS_SOURCE]
    verdicts = []
    for x in vals:
        if x is None:
            continue
        fails = [f"{'R1' if f == 'one' else 'R2'} at {per} B" for (per, f), l in lim.items() if x > l]
        verdicts.append(f"{x} B {'fits every limit' if not fails else 'is over the limit for ' + ', '.join(fails)}")
    print(f"   '{n}' ({len(n)} characters{', question 3, not N2' if n == 'Ms' else ''}): the longest value it may"
          f" answer and keep 96 KiB beside N1 is {lim[(AG_PER, 'one')]} B (R1) or {lim[(AG_PER, 'each')]} B (R2),"
          f" and with DECRQSS in the colon form {lim[(AG_PER_COLON, 'one')]} B (R1) or"
          f" {lim[(AG_PER_COLON, 'each')]} B (R2); the value read: "
          + ("a boolean" if v is None else "; ".join(verdicts))
          + (f" ({MS_DECODED} B decoded, {MS_SOURCE} B as source text)" if n == "Ms" else ""))

# ---- Question 4: N2's booleans in each of the three forms. Tc and Su priced
# as xterm-shaped answers with no value (kitty, foot, ghostty: no `=`), with an
# empty value (xterm's own valid-with-empty-value shape, `=` and nothing), and
# with wezterm's `=31`, the value "1": 0, 1 and 3 B more per name than the bare
# form. Every other N2 value as above.
print("== question 4: N2's booleans (Tc, Su) in each form, everything else as N2 above (C)")
for label, bv in (("1+r<hex>, no '='", None), ("1+r<hex>=, an empty value", 0), ("1+r<hex>=31, the value '1'", 1)):
    items = N1_ITEMS + [(n, bv) if v is None else (n, v) for n, v in N2_ITEMS]
    parts = []
    for form in ("one", "each"):
        d = knap(items, form)
        b = max(AG_LARGEST, d) + ((L - 1) // 5) * AG_PER + AG_CARRY
        bc = max(AG_LARGEST, d) + ((L - 1) // 5) * AG_PER_COLON + AG_CARRY
        parts.append(f"{'R1' if form == 'one' else 'R2'} largest full body {d} B, a read it opens {b} B,"
                     f" colon form {bc} B")
    extra = 0 if bv is None else 1 + 2 * bv
    print(f"   {label} ({extra} B more per boolean name than the bare form): " + "; ".join(parts))

# ---- XTSETTCAP (question 5): the pinned entries' key strings, which an
# XTGETTCAP that switches among compiled entries would answer from. The ten
# entries pinned besides the -x capture; the capture, which lands with the
# note in the same directory, is excluded by name and counted on its own.
def input_strings(caps):
    ks = {k: kv for k, kv in caps.items() if k.startswith("k")}
    names = nbytes = 0
    long_ = (0, "")
    for k, (kind, v) in ks.items():
        val = value_bytes(kind, v, "source-if-param") if kind == "str" else b""
        for nm in [k] + ([ALIAS[k]] if k in ALIAS else []):
            names += 1
            nbytes += len(nm) + len(val) + 2
        if len(val) > long_[0]:
            long_ = (len(val), k)
    return len(ks), names, nbytes, long_
print("== the pinned entries' k-prefixed capabilities (every capability whose name starts with k, as"
      " terminfo::is_input tests, booleans such as km and values with no \\E or ^ such as nsterm's kbs=\\177"
      " included, both of which input_caps.rs's parse() leaves out), each under its terminfo name and its"
      " termcap code where it has one")
entries = [ti for ti in sorted((SRC / "terminfo").glob("*.ti")) if ti.name != "xterm-256color-x.ti"]
tot_names = tot_bytes = 0
longest = (0, "", "")
for ti in entries:
    n_caps, names, nbytes, (lv, lk) = input_strings(parse_ti(ti))
    tot_names += names
    tot_bytes += nbytes
    if lv > longest[0]:
        longest = (lv, ti.stem, lk)
    print(f"   {ti.stem}: {n_caps} k-prefixed capabilities, {names} names")
print(f"   all {len(entries)} entries besides the -x capture: {tot_names} names, {tot_bytes} B of names and"
      f" values; the longest value {longest[0]} B ({longest[1]} {longest[2]})")
n_caps, names, nbytes, (lv, lk) = input_strings(captured)
print(f"   the -x capture on its own: {n_caps} k-prefixed capabilities, {names} names, {nbytes} B; the longest value"
      f" {lv} B ({lk})")
