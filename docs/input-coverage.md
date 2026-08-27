# baud — input coverage

What baud must **send**, enumerated against primary sources, in the same shape
as `vt-coverage.md` does for what it receives.

Nothing here is implemented yet beyond the mouse reports the parser already
needed, so almost every row is `todo`. That is a phase that has not started,
not work left half-done: `vt-coverage.md`'s rule is that a row marked **done**
carries a test, and a row marked `partial` is an operation whose gap is named.
A `todo` row here has neither because it has no code.

## Why this file is not a section of `vt-coverage.md`

The output side has one specification to enumerate against. The input side has
three layers, and a row is meaningless without saying which one it belongs to:

| layer | source | what it is |
|---|---|---|
| base | `[[source: ctlseqs]]` | what every terminal sends. Cursor, editing, function and keypad keys, the `CSI 1 ; modifier <final>` form, and `modifyOtherKeys` |
| CSI-u | `[[source: fixterms]]` | `CSI unicode-key ; modifiers u`, a *proposal* to remove the base's ambiguities |
| kitty | `[[source: kitty-keyboard]]` | progressive enhancement over CSI-u: `CSI > flags u` to push, `CSI < u` to pop, with event types, alternate keys and associated text |

An application asks for the layer it can parse, which is the same shape DECRQM
gives the output side. Confusing them is how a terminal comes to send a
sequence nothing asked for.

## The key tables are extracted, not transcribed

`scripts/extract-keys.py` reads the ten tables out of `[[source: ctlseqs]]` --
**112 key rows** -- and prints them with the line each was
found at. A key table written from memory is exactly the "guessed table that
would be unrecoverable and would look like it worked" this project refused for
DEC line drawing, so it is generated and re-runnable rather than pasted.

| ctlseqs line | table | rows | status |
|---|---|---|---|
| 2473 | cursor keys, normal vs application (DECCKM) | 4 | todo |
| 2485 | Home / End, normal vs application | 2 | todo |
| 2498 | keypad, numeric vs application (DECKPAM) | 24 | todo |
| 2529 | 12 function keys and special-purpose keys | 12 | todo |
| 2552 | the deprecated F1-F5 forms | 4 | todo — deprecated upstream; recorded so the choice is explicit |
| 2628 | VT220 application keypad | 24 | todo |
| 2672 | VT220 cursor keys | 6 | todo |
| 2685 | the 8 additional VT220 function keys | 8 | todo |
| 2703 | VT52 keys, which do not change with mode | 4 | todo |
| 2713 | VT52 keypad | 24 | todo |

## Modifiers

`[[source: ctlseqs]]` line 2565 onward: the parameter is `1 + bitmask`, with
Shift 1, Alt 2, Control 4, Meta 8 -- so shift-F5 is `CSI 1 5 ; 2 ~`. Meta only
appears when `alwaysUseMods` is set, which makes 9 through 16 reachable.

Encoding a modifier is not the same as *having* one. Whether Alt prefixes ESC
or sets the high bit is a resource in xterm (`metaSendsEscape`,
`altSendsEscape`), and the table at line 2338 gives all eight combinations. baud
has to pick, and record which, because an application cannot tell.

## Modes that change what a key sends

Each already exists on the receive side with an accessor and no producer. They
are the reason the input path is next rather than the renderer: the parser work
created the demand.

| mode | accessor | affects |
|---|---|---|
| DECCKM `?1` | `cursor_keys_application` | cursor keys: `CSI A` vs `SS3 A` |
| DECKPAM / DECKPNM | `keypad` | keypad: `1` vs `SS3 q` |
| `?2004` bracketed paste | `bracketed_paste` | wraps pasted text in `CSI 200 ~` / `CSI 201 ~` |
| `?1000`-`?1003`, `?1006` | `mouse_tracking` | **done** — the only input baud already generates |
| `?1001` highlight tracking | `highlight_tracking`, `highlight_region` | recorded, and needs a selection model to answer |

## Not in scope yet, and why

* **DECUDK** — user-defined keys. `vt-coverage.md` lists it `out` as
  input-side; it stops being out once there is an input side, and it needs the
  DCS payload the parser already collects.
* **Key repeat, compose, dead keys, IME** — these belong to the platform layer
  above baud, which has no event source. Recording them here would be a bit set
  and never read.
