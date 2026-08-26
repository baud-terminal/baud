# VT protocol coverage

What baud implements, what it does not, and what it has decided not to. Every
row traces to a primary source in `docs/research/sources/`, cited as
`[[source: <key>]]` and checked by `scripts/check-sources.sh`.

**This file is the scope definition.** "Implement the VT protocol" is not a
statement with a truth value until the surface is enumerated, because the
surface is not finite in any useful sense: `[[source: ctlseqs]]` documents 117
CSI sequences alone, many of them xterm extensions with no other implementation.
Tiers below say what baud is committing to and what it is not.

Status values are deliberately blunt:

| | meaning |
|---|---|
| **done** | implemented, dispatched, and covered by a test that would fail if it regressed |
| **partial** | implemented but known-incomplete; the gap is named in the row |
| **parsed** | recognised by the parser and then **dropped** — worse than missing, because it looks supported |
| **todo** | in scope, not implemented |
| **out** | deliberately not in scope; the reason is in the row |

A `parsed` row is a bug. It is the state where a terminal accepts a sequence,
does nothing, and gives the application no way to find out.

---

## Tier 0 — the parser state machine

Everything below depends on this being right, so it is not a tier of features
but the substrate. `[[source: williams]]` specifies the DEC ANSI parser as a
13-state machine; baud currently has 5.

| state | in baud | note |
|---|---|---|
| `ground` | done | |
| `escape` | done | |
| `escape_intermediate` | done | final byte consumed; DECALN itself still todo |
| `csi_entry` | partial | folded into `CsiParams` |
| `csi_param` | done | `CsiParams` |
| `csi_intermediate` | done | intermediate recorded, in both feed paths |
| `csi_ignore` | done | |
| `dcs_entry` | partial | payload consumed via `StringIgnore`; no DCS dispatch |
| `dcs_param` | **todo** | |
| `dcs_intermediate` | **todo** | |
| `dcs_passthrough` | **todo** | |
| `dcs_ignore` | **todo** | |
| `osc_string` | partial | `OscString` consumes to ST but dispatches nothing |
| `sos_pm_apc_string` | done | payload consumed, never executed |

~~`sos_pm_apc_string` is a correctness and security gap.~~ Resolved: DCS, SOS,
PM and APC payloads are consumed by `StringIgnore` and never executed. Note the
specified limit, which is not a hole: `[[source: williams]]` says a bare `ESC`
*cancels* a control string, so the guarantee is precisely that a payload
containing no ESC is inert.

baud adds a `Utf8` state, which `[[source: williams]]` does not have — that
parser predates UTF-8 and is defined over bytes. Keeping it is deliberate; the
note in `docs/research/notes/c-vt-parser.md` records why.

---

## Tier 1 — VT100/ANSI core

The set every terminal must implement. No application can be assumed to work
without these.

### C0 controls

| byte | name | status |
|---|---|---|
| `0x00` NUL | ignored | done |
| `0x07` BEL | bell | done — counted; no UI to ring |
| `0x08` BS | backspace | done |
| `0x09` HT | tab | done — real stop table |
| `0x0A` LF | line feed | done — index only unless LNM |
| `0x0B` VT | vertical tab | done — index |
| `0x0C` FF | form feed | done — index |
| `0x0D` CR | carriage return | done |
| `0x0E` SO | shift out (G1) | **todo** |
| `0x0F` SI | shift in (G0) | **todo** |

### ESC sequences

| seq | name | status |
|---|---|---|
| `ESC 7` | DECSC save cursor | done |
| `ESC 8` | DECRC restore cursor | done |
| `ESC D` | IND index | done — column preserved (was resetting it) |
| `ESC E` | NEL next line | done |
| `ESC M` | RI reverse index | done |
| `ESC c` | RIS full reset | done |
| `ESC H` | HTS set tab stop | done |
| `ESC =` / `ESC >` | DECKPAM/DECKPNM | **todo** |
| `ESC # 8` | DECALN screen alignment | **todo** — needs `escape_intermediate` |
| `ESC ( ` / `ESC ) ` | charset designate G0/G1 | **todo** |

### CSI operations

Final bytes from `[[source: ctlseqs]]` § "Functions using CSI".

| seq | name | status |
|---|---|---|
| `CSI @` | ICH insert blanks | done |
| `CSI A` | CUU cursor up | done |
| `CSI B` | CUD cursor down | done |
| `CSI C` | CUF cursor forward | done |
| `CSI D` | CUB cursor back | done |
| `CSI E` | CNL cursor next line | done |
| `CSI F` | CPL cursor preceding line | done |
| `CSI G` | CHA cursor char absolute | done |
| `CSI H` / `CSI f` | CUP / HVP | done |
| `CSI I` | CHT forward tabulation | done |
| `CSI J` | ED erase in display | done |
| `CSI K` | EL erase in line | done |
| `CSI L` | IL insert lines | done |
| `CSI M` | DL delete lines | done |
| `CSI P` | DCH delete chars | done |
| `CSI S` | SU scroll up | done |
| `CSI T` | SD scroll down | done |
| `CSI X` | ECH erase chars | done |
| `CSI Z` | CBT backward tabulation | done |
| `CSI \`` | HPA char position absolute | done |
| `CSI a` | HPR char position relative | done |
| `CSI b` | REP repeat preceding char | **todo** |
| `CSI c` | DA device attributes | done — identifies as VT102 |
| `CSI d` | VPA line position absolute | done |
| `CSI e` | VPR line position relative | done |
| `CSI g` | TBC tab clear | done — 0 and 3; others inert, not aliased |
| `CSI h` | SM set mode | partial — LNM and IRM parsed; IRM not yet acted on |
| `CSI l` | RM reset mode | partial — as SM |
| `CSI m` | SGR character attributes | partial — see SGR below |
| `CSI n` | DSR device status report | done — 5 and 6; others unanswered, not guessed |
| `CSI r` | DECSTBM scroll region | done |
| `CSI s` / `CSI u` | SCOSC / SCORC | done |
| `CSI ? h` / `CSI ? l` | DECSET / DECRST | **parsed** |

### SGR

`SGR` is one final byte and many operations. Current coverage is partial and the
gap has never been enumerated; `docs/research/notes/` will carry the breakdown
once measured against `[[source: ecma48]]` § 8.3.117.

---

## Tier 2 — VT220 / VT420

In scope. Needed by real applications (vim, tmux, htop) but not by every one.

| seq | name | status |
|---|---|---|
| `CSI ! p` | DECSTR soft reset | done |
| `CSI SP q` | DECSCUSR cursor style | **todo** |
| `CSI " q` | DECSCA character protection | **todo** |
| `CSI ? J` | DECSED selective erase display | **todo** |
| `CSI ? K` | DECSEL selective erase line | **todo** |
| `CSI $ p` | DECRQM request mode | **todo** |
| `CSI s` (DECSLRM) | left/right margins | **todo** |
| `CSI ' }` / `CSI ' ~` | DECIC / DECDC | **todo** |

---

## Tier 3 — xterm extensions in universal use

In scope because applications assume them, not because DEC specified them.

| seq | name | status |
|---|---|---|
| `OSC 0/1/2` | window/icon title | **todo** |
| `OSC 4` | set colour palette | **todo** |
| `OSC 8` | hyperlinks | **todo** |
| `OSC 52` | clipboard | **todo** — see security note |
| `OSC 104` | reset palette | **todo** |
| `CSI ? 1049 h/l` | alternate screen buffer | **todo** |
| `CSI ? 2004 h/l` | bracketed paste | **todo** |
| `CSI ? 1000-1006 h/l` | mouse reporting | **todo** |
| `CSI ? 2026 h/l` | synchronised output | **todo** |
| `CSI ? 25 h/l` | DECTCEM cursor visibility | **todo** |

---

## Out of scope, with reasons

Recorded so that "not implemented" is a decision rather than an omission.

| seq | name | why not |
|---|---|---|
| `DCS ... q` | Sixel graphics | An image protocol, not a terminal protocol. Large surface, and it does not interact with the ingest architecture baud exists to test. |
| Kitty graphics `APC G` | image protocol | As above. The `APC` **string must still be consumed** — that is Tier 0, and it is a bug today. |
| `DCS ... | DECUDK` | user-defined keys | Input-side; baud has no input path. |
| `CSI > ... m` | XTMODKEYS | Input-side. |
| `CSI ... T` XTHIMOUSE | mouse highlight tracking | Requires a reply channel and a UI. |
| ReGIS, Tektronix | vector graphics | Historical; no modern application emits them. |
| National replacement charsets | DECNRCM | Superseded by UTF-8. Recorded because `[[source: ctlseqs]]` documents them and a reader will ask. |

---

## Cross-cutting gaps

Two things block whole tiers and are not any single sequence.

**~~No reply channel.~~** Resolved. `Storage.reply` is a 256-byte buffer the
core fills and the caller drains with `Term::reply()` / `Term::clear_reply()`,
mirroring the input side -- the core is `no_std` with no I/O, so it cannot own a
PTY. Overflow is counted in `reply_overflows`, never silent.

DA and DSR go through it. `DECRQM` and `XTVERSION` remain **todo**, but they are
now ordinary work rather than blocked on structure.

Two rules the tests pin, because both failure modes are worse than silence:

* **DA under-claims.** baud answers `CSI ? 6 c` (VT102), which is what it is.
  Claiming VT220 would make applications send sequences baud drops.
* **An unsupported DSR gets no answer at all.** Replying with the wrong report
  shape is worse than not replying: the application parses whatever arrives as
  the report it asked for.

**~~No tab stop table.~~** Resolved. `Storage.tabs` is one flag per column,
defaulted every eight and restored by RIS; `HT`, `HTS`, `TBC`, `CHT` and `CBT`
all go through it. Whether this closes the differential failures the README
names is **unverified** -- that needs a differential run, which needs an
alacritty checkout.

---

## How this file stays true

`docs/vt-coverage.md` is checked by `tests/coverage.rs`: every row marked
**done** must have a named test, and the test asserts the operation is reachable
from `Term::feed`. A row cannot be marked done without one. Documentation that
claims coverage it does not have is the failure mode this file exists to
prevent — the README claimed "~22 operations" while three of them were parsed
and dropped.
