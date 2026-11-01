#!/usr/bin/env python3
"""Generate baud-domain/src/vt/key/termcap_data.rs, the termcap keyboard's table.

    python3 scripts/gen-termcap-keys.py           write the file
    python3 scripts/gen-termcap-keys.py --check   fail if the file is not what this writes

Under `?1050` a key sends the string the terminal description gives it. xterm's
`xtermcapString` (`[[source: xterm-xtermcap-c]]` 461-511) finds the key's rows
in its table (70-244) by keysym and modifier parameter, in the table's order,
and sends the string of the first one the description has. The description
here is the pinned xterm-256color entry (`[[source: terminfo-xterm-256color]]`),
the `TERM` baud gives the child, so the table is compiled from the two: for each
key and parameter, the first row whose terminfo name the entry defines.

Rows no key reaches are left out: the keypad's Home, End, Prior and Next, which
xterm moves onto the main keys before it reads the table (input.c 1185-1191);
`kcbt`, whose row has no modifier while a back tab carries Shift; Help, Find,
Select, Clear and Undo, and F36 up, which baud has no key for. No row names
Begin, so the entry's `kb2` is not in the table either: a key no row reaches
keeps the default keyboard's encoding, as in xterm.
"""
import pathlib, re, sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
XTERMCAP = ROOT / "docs/research/sources/xterm-xtermcap.c"
ENTRY = ROOT / "docs/research/sources/terminfo/xterm-256color.ti"
OUT = ROOT / "baud-domain/src/vt/key/termcap_data.rs"

# xterm's table in source order, as scripts/gen-tcap-table.py reads it.
src = XTERMCAP.read_text()
body = src[src.index("static const TCAPINFO table[] = {") : src.index("#undef DATA")]
body = "\n".join(l for l in body.splitlines() if not l.lstrip().startswith("#define"))
rows = []
for line in body.splitlines():
    s = line.strip()
    m = re.match(r'DATA\(\s*"([^"]*)",\s*"([^"]*)",\s*([^,]+?),\s*(\w+)\s*\)', s)
    if m:
        tc, ti, code, param = m.groups()
        rows.append((ti, code.strip(), {"0": 0, "SHIFT": 2}[param]))
        continue
    m = re.match(r"DGRP\((\w+),\s*(\w+)\)", s)
    if m:
        name, code = m.groups()
        base = "kp" if code.startswith("XK_KP_") else "k"
        rows.append((base + name, code, 2))
        for p in range(3, 9):
            rows.append((base + name + str(p), code, p))
if len(rows) < 200:
    sys.exit(f"read {len(rows)} rows of xterm's table, expected over 200")

# The entry's string capabilities, `name=value,` one a line.
caps = {}
for line in ENTRY.read_text().splitlines():
    m = re.match(r"\s+([A-Za-z0-9]+)=(.*),$", line)
    if m:
        caps[m.group(1)] = m.group(2)
if caps.get("kcuu1") != r"\EOA":
    sys.exit("the entry's kcuu1 is not \\EOA: the entry was not read")


def unescape(v):
    """terminfo(5)'s string escapes, the ones a key's string can carry."""
    out, i = bytearray(), 0
    while i < len(v):
        c = v[i]
        if c == "\\":
            n = v[i + 1]
            simple = {"E": 0x1B, "e": 0x1B, "n": 0x0A, "r": 0x0D, "t": 0x09,
                      "b": 0x08, "f": 0x0C, "s": 0x20, "^": 0x5E, "\\": 0x5C,
                      ",": 0x2C, ":": 0x3A}
            if n in simple:
                out.append(simple[n])
                i += 2
            elif n.isdigit():
                out.append(int(v[i + 1 : i + 4], 8))
                i += 4
            else:
                sys.exit(f"an escape terminfo(5) does not define: {v!r}")
        elif c == "^":
            out.append(ord(v[i + 1]) & 0x1F if v[i + 1] != "?" else 0x7F)
            i += 2
        else:
            out.append(ord(c))
            i += 1
    return bytes(out)


KEYS = {
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
    "XK_KP_Add": "KeyCode::Keypad(KeypadKey::Add)",
    "XK_KP_Separator": "KeyCode::Keypad(KeypadKey::Comma)",
    "XK_KP_Divide": "KeyCode::Keypad(KeypadKey::Divide)",
    "XK_KP_Decimal": "KeyCode::Keypad(KeypadKey::Period)",
    "XK_KP_Multiply": "KeyCode::Keypad(KeypadKey::Multiply)",
    "XK_KP_Subtract": "KeyCode::Keypad(KeypadKey::Minus)",
}
for d in range(10):
    KEYS[f"XK_KP_{d}"] = f"KeyCode::Keypad(KeypadKey::Digit{d})"
UNREACHED = {"XK_KP_Home", "XK_KP_End", "XK_KP_Prior", "XK_KP_Next", "XK_ISO_Left_Tab",
             "XK_Help", "XK_Find", "XK_Select", "XK_Clear", "XK_Undo",
             "XK_COLORS", "XK_RGB", "XK_TCAPNAME"}


def key(code):
    """The pattern for a row's key, a function key's number, or None."""
    m = re.fullmatch(r"XK_Fn\((\d+)\)", code)
    if m:
        n = int(m.group(1))
        return ("fn", n) if n <= 20 else (f"KeyCode::Pua(PuaKey::F{n})", None) if n <= 35 else None
    if re.fullmatch(r"-\d+", code) or code in UNREACHED:
        return None
    if code in KEYS:
        return (KEYS[code], None)
    sys.exit(f"no key for {code}")


keyed, fns = {}, {}
for ti, code, param in rows:
    k = key(code)
    if k is None or ti not in caps:
        continue
    value = unescape(caps[ti])
    if k[0] == "fn":
        fns.setdefault((k[1], param), (ti, value))
    else:
        keyed.setdefault((k[0], param), (ti, value))
if fns.get((13, 0), (None, b""))[1] != b"\x1b[1;2P":
    sys.exit("kf13 is not \\E[1;2P: the join went wrong")


def lit(b):
    return 'b"' + "".join(chr(c) if 0x20 <= c < 0x7F and chr(c) not in '"\\' else f"\\x{c:02x}" for c in b) + '"'


out = [
    "//! The termcap keyboard's table, `?1050`: each key of xterm's XTGETTCAP",
    "//! table that the pinned xterm-256color entry gives a string, with its",
    "//! modifier parameter, and the string of the key's first row that the entry",
    "//! has.",
    "//!",
    "//! **Do not edit by hand.** Regenerate with `scripts/gen-termcap-keys.py`;",
    "//! `the_termcap_table_is_the_pinned_entry` fails if this file and the",
    "//! generator disagree. The strings are ncurses' xterm-256color entry's",
    "//! (`LICENSES/ncurses-COPYING.txt`), the rows and their order xterm's",
    "//! xtermcap.c (`LICENSES/xterm-COPYING.txt`).",
    "",
    "use crate::{KeyCode, KeypadKey, PuaKey};",
    "",
    "/// The string the entry gives `code` pressed with modifier parameter",
    "/// `param` (0 for none, else xterm's `1 + mask`), or `None` for a key",
    "/// and parameter it gives none.",
    "pub(super) fn string(code: KeyCode, param: u8) -> Option<&'static [u8]> {",
    "    Some(match (code, param) {",
]
for (k, param), (ti, value) in keyed.items():
    out.append(f"        // {ti}")
    out.append(f"        ({k}, {param}) => {lit(value)},")
out.append("        (KeyCode::Function(f), p) => return function(f.number(), p),")
out.append("        _ => return None,")
out.append("    })")
out.append("}")
out.append("")
out.append("/// The string for function key `n`, F1 to F20, with parameter `param`.")
out.append("fn function(n: u8, param: u8) -> Option<&'static [u8]> {")
out.append("    Some(match (n, param) {")
for (n, param), (ti, value) in sorted(fns.items()):
    out.append(f"        // {ti}")
    out.append(f"        ({n}, {param}) => {lit(value)},")
out.append("        _ => return None,")
out.append("    })")
out.append("}")
text = "\n".join(out) + "\n"

count = len(keyed) + len(fns)
if sys.argv[1:] == ["--check"]:
    if not OUT.exists() or OUT.read_text() != text:
        sys.exit(f"{OUT.relative_to(ROOT)} is not what scripts/gen-termcap-keys.py writes")
    print(f"ok {count} keys")
else:
    OUT.write_text(text)
    print(f"wrote {OUT.relative_to(ROOT)}: {count} keys")
