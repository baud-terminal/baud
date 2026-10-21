#!/usr/bin/env python3
"""Generate baud-domain/src/vt/tcap_data.rs from xterm's pinned XTGETTCAP table.

    python3 scripts/gen-tcap-table.py           write the file
    python3 scripts/gen-tcap-table.py --check   fail if the file is not what this writes

`[[source: xterm-xtermcap-c]]` lines 70-244 are the table `DCS + q` answers
from: 110 `DATA` rows and 17 `DGRP` groups of seven, every conditional row
compiled in (`USE_EXTENDED_NAMES`, `XK_ISO_Left_Tab`, `OPT_ISO_COLORS` with
`OPT_DIRECT_COLOR`). Each row gives a terminfo name and a termcap code; xterm's
`lookupTcapByName` takes the first row whose either name matches (347-374), so
a name is kept once, at its first row. Each row's key is turned into the key
baud's encoder takes; a key baud has no code for is `Absent`.
"""
import pathlib, re, sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SRC = ROOT / "docs/research/sources/xterm-xtermcap.c"
OUT = ROOT / "baud-domain/src/vt/tcap_data.rs"

src = SRC.read_text()
body = src[src.index("static const TCAPINFO table[] = {") : src.index("#undef DATA")]
body = "\n".join(l for l in body.splitlines() if not l.lstrip().startswith("#define"))

# The table in source order: DATA rows and DGRP groups interleaved as written.
rows = []
group_base = "k"
for line in body.splitlines():
    s = line.strip()
    if s.startswith('#undef BASE'):
        continue
    m = re.match(r'DATA\(\s*"([^"]*)",\s*"([^"]*)",\s*([^,]+?),\s*(\w+)\s*\)', s)
    if m:
        tc, ti, code, param = m.groups()
        rows.append((tc, ti, code.strip(), {"0": 0, "SHIFT": 2}[param]))
        continue
    m = re.match(r"DGRP\((\w+),\s*(\w+)\)", s)
    if m:
        name, code = m.groups()
        # kp* groups follow `#define BASE "kp"`; the cursor and editing
        # groups come first, under "k".
        base = "kp" if code.startswith("XK_KP_") else "k"
        rows.append(("", base + name, code, 2))
        for p in range(3, 9):
            rows.append(("", base + name + str(p), code, p))

NAMED = {
    "XK_Home": "KeyCode::Home",
    "XK_End": "KeyCode::End",
    "XK_Left": "KeyCode::Left",
    "XK_Right": "KeyCode::Right",
    "XK_Up": "KeyCode::Up",
    "XK_Down": "KeyCode::Down",
    "XK_Delete": "KeyCode::Delete",
    "XK_Insert": "KeyCode::Insert",
    "XK_Next": "KeyCode::PageDown",
    "XK_Prior": "KeyCode::PageUp",
    "XK_BackSpace": "KeyCode::Backspace",
    "XK_KP_Enter": "KeyCode::Keypad(KeypadKey::Enter)",
    "XK_KP_Home": "KeyCode::Keypad(KeypadKey::Home)",
    "XK_KP_End": "KeyCode::Keypad(KeypadKey::End)",
    "XK_KP_Prior": "KeyCode::Keypad(KeypadKey::PageUp)",
    "XK_KP_Next": "KeyCode::Keypad(KeypadKey::PageDown)",
    "XK_KP_Add": "KeyCode::Keypad(KeypadKey::Add)",
    "XK_KP_Separator": "KeyCode::Keypad(KeypadKey::Comma)",
    "XK_KP_Divide": "KeyCode::Keypad(KeypadKey::Divide)",
    "XK_KP_Decimal": "KeyCode::Keypad(KeypadKey::Period)",
    "XK_KP_Multiply": "KeyCode::Keypad(KeypadKey::Multiply)",
    "XK_KP_Subtract": "KeyCode::Keypad(KeypadKey::Minus)",
}
for d in range(10):
    NAMED[f"XK_KP_{d}"] = f"KeyCode::Keypad(KeypadKey::Digit{d})"
ABSENT = {"XK_Help", "XK_Find", "XK_Select", "XK_Clear", "XK_Undo"}


def value(code, param):
    if code == "XK_COLORS":
        return "TcapValue::Colors"
    if code == "XK_RGB":
        return "TcapValue::Rgb"
    if code == "XK_TCAPNAME":
        return "TcapValue::Name"
    if code == "XK_ISO_Left_Tab":
        # The row's state is 0 and its keysym is the shifted Tab; baud's key
        # is Tab with Shift (input.c 1146-1147, "Shift-tab is often mapped to
        # XK_ISO_Left_Tab").
        return "tc_key(KeyCode::Tab, 2)"
    if code in ABSENT:
        return "TcapValue::Absent"
    m = re.fullmatch(r"XK_Fn\((\d+)\)|-(\d+)", code)
    if m:
        n = int(m.group(1) or m.group(2))
        if n <= 20:
            return f"tc_fn({n}, {param})"
        if n <= 35:
            return f"tc_key(KeyCode::Pua(PuaKey::F{n}), {param})"
        return "TcapValue::Absent"
    if code in NAMED:
        k = NAMED[code]
        if k.startswith("KeyCode::Keypad("):
            return f"tc_pad({k[len('KeyCode::Keypad('):-1]}, {param})"
        return f"tc_key({k}, {param})"
    sys.exit(f"no mapping for {code}")


seen = {}
for tc, ti, code, param in rows:
    for name in (ti, tc):
        if name and name not in seen:
            seen[name] = value(code, param)

names = list(seen)
if not (2 <= min(map(len, names)) and max(map(len, names)) <= 7):
    sys.exit("a name outside 2-7 bytes")
if len(names) != 319:
    sys.exit(f"expected xterm's 319 distinct names, found {len(names)}")

# **The index's multiplier, searched here** so the table is found in one
# compare or two: the first of 3,000 odd multipliers (seed 1) whose
# open-addressed index of 1,024 slots, hashed by the high ten bits as
# `tcap.rs`'s `bucket` hashes, leaves no name more than one slot from its
# bucket. `tcap.rs` asserts that bound at compile time.
import random


def pack(n):
    v = len(n)
    for c in n.encode():
        v = (v << 8) | c
    return v


def walk(mul, slots=1024):
    taken, worst = set(), 0
    for n in seen:
        i, k = (pack(n) * mul % 2**64) >> (64 - 10), 0
        while i in taken:
            i, k = (i + 1) % slots, k + 1
        taken.add(i)
        worst = max(worst, k)
    return worst


rng = random.Random(1)
mul = None
for _ in range(3000):
    cand = rng.getrandbits(64) | 1
    if walk(cand) <= 1:
        mul = cand
        break
if mul is None:
    sys.exit("no multiplier keeps every name within one slot")

out = [
    "//! xterm's XTGETTCAP table, generated from the source pinned in",
    "//! `docs/research/sources/MANIFEST.toml` as `xterm-xtermcap-c`.",
    "//!",
    "//! **Do not edit by hand.** Regenerate with `scripts/gen-tcap-table.py`;",
    "//! `the_tcap_table_is_the_pinned_source` fails if this file and the",
    "//! generator disagree.",
    "//!",
    "//! Each of xterm's 319 names, terminfo and termcap, at the first row that",
    "//! names it, as xterm's `lookupTcapByName` finds it, with the key baud's",
    "//! encoder takes for that row. X11 licence (`LICENSES/xterm-COPYING.txt`).",
    "",
    "use super::tcap::{TcapValue, tc_fn, tc_key, tc_pad};",
    "use crate::{KeyCode, KeypadKey, PuaKey};",
    "",
    "/// The index's multiplier: the generator's search, which keeps every",
    "/// name within one slot of its bucket.",
    f"pub(crate) const MUL: u64 = {mul:#018x};",
    "",
    f"pub(crate) const ROWS: [(&[u8], TcapValue); {len(names)}] = [",
]
for name in names:
    lit = name.replace("\\", "\\\\").replace('"', '\\"')
    out.append(f'    (b"{lit}", {seen[name]}),')
out.append("];")
text = "\n".join(out) + "\n"

if sys.argv[1:] == ["--check"]:
    if OUT.read_text() != text:
        sys.exit(f"{OUT.relative_to(ROOT)} is not what scripts/gen-tcap-table.py writes")
    print(f"ok {len(names)} names")
else:
    OUT.write_text(text)
    print(f"wrote {OUT.relative_to(ROOT)}: {len(names)} names")
