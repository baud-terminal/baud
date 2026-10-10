# baud — input coverage

What baud must **send**, enumerated against primary sources, in the same shape
as `vt-coverage.md` does for what it receives.

The base layer is done: `Term::key` encodes the cursor keys, Home/End, the
editing keys, F1-F20, the text keys, the keypad and the VT52 tables, in both
DECCKM and both DECKPAM modes, with modifiers and in both input encodings.
**Both negotiated layers are done too.** `[[source: fixterms]]`'s `CSI u` is
reachable through `modifyOtherKeys`/`formatOtherKeys` (the base layer's
resources), and `[[source: kitty-keyboard]]`'s progressive enhancement is
implemented in full: all five flags, the per-screen stack, and the `CSI ? u`
query answered -- which is the protocol's support claim, made only once every
enhancement is honoured. `vt-coverage.md`'s rule is that a row marked **done**
carries a test, and a row marked `partial` is an operation whose gap is named.
A `todo` row here has neither because it has no code.

## Why this file is not a section of `vt-coverage.md`

The output side has one specification to enumerate against. The input side has
three layers, and a row is meaningless without saying which one it belongs to:

| layer | source | what it is |
|---|---|---|
| base | `[[source: ctlseqs]]` | what every terminal sends. Cursor, editing, function and keypad keys, the `CSI 1 ; modifier <final>` form, and `modifyOtherKeys` |
| CSI-u | `[[source: fixterms]]` | `CSI unicode-key ; modifiers u`, a *proposal* to remove the base's ambiguities. **It defines no negotiation**, which is why it is not independently implementable: a terminal that simply started sending it would break every application that had not asked. baud reaches it through the base layer's `formatOtherKeys`, and the rows below carry it |
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

`scripts/extract-kitty-keys.py` does the same for
`[[source: kitty-keyboard]]`'s "Functional key definitions" table -- **111
rows** mapping a key name to the number and final byte its `CSI` form uses,
from `ESCAPE = 27 u` through `ISO_LEVEL5_SHIFT = 57454 u`. It carries its own
positive control (`ESCAPE`, `CAPS_LOCK` and `ISO_LEVEL5_SHIFT` at known values,
and a floor on the row count) so a broken extraction fails loudly rather than
reporting a plausible subset. It is the source of truth the kitty encoder's
functional-key table is gated against, the way the base rows above are gated
against `extract-keys.py`. ~~The encoder that consumes it is not built yet.~~
It is built, and three tests in `baud-core/tests/kitty_encode.rs` read the
table: `functional_keys_match_the_source` (Esc, the arrows, the editing keys
and F1-F20), `keypad_keys_match_the_source` and `pua_keys_match_the_source`
(the locks, F21-F35, the media and modifier keys). The `KP_LEFT` to
`KP_BEGIN` navigation rows are gated by none, because the keypad does not
model them.

| ctlseqs line | table | rows | status |
|---|---|---|---|
| 2473 | cursor keys, normal vs application (DECCKM) | 4 | **done** — `Term::key`, both modes, modifiers force `CSI`. `cursor_keys_match_the_specifications_table` |
| 2485 | Home / End, normal vs application | 2 | **done** — same path; ctlseqs puts them in the arrows' table |
| 2498 | keypad, numeric vs application (DECKPAM) | 24 | **done** — all 24 rows, both columns, read out of the table itself. `the_keypad_matches_the_specifications_table` |
| 2529 | 12 function keys and special-purpose keys | 12 | **done** — F1-F4 are `SS3 P`-`S` *unconditionally*; F5-F12 are `CSI 15/17/18/19/20/21/23/24 ~`, and the gaps at 16 and 22 are real |
| 2552 | the deprecated `oldXtermFKeys` F1-F4 forms | 4 | **out** — `[[source: ctlseqs]]` line 2548 deprecates them in its own words: "since they do not correspond to any hardware terminal, they have been deprecated". `CSI 11 ~`-`CSI 14 ~` are also what a VT220 would read as F1-F4 of a keyboard that reserves those for Setup. Sending them would be sending a fifth spelling of four keys baud already sends correctly |
| 2628 | VT220 application keypad | 24 | out — the `sunKeyboard` alternative to 2498, not an addition. The two disagree on 11 rows (keypad `2` is `SS3 r` there and `CSI B` at 2498), so sending both would be sending two different keys. `the_vt220_keypad_is_a_different_table_and_is_not_what_baud_sends` asserts the disagreement rather than assuming it |
| 2672 | VT220 cursor keys | 6 | **done** for Insert/Delete/PageUp/PageDown, which DECCKM does not change. Home and End are sent PC-style from the table at 2485 — the VT220 forms here are xterm's `sunKeyboard` alternative, not an addition, and sending both would be sending two different keys |
| 2685 | the 8 additional VT220 function keys | 8 | **done** — `CSI 25/26/28/29/31/32/33/34 ~`, skipping 27 and 30 |
| 2703 | VT52 keys, which do not change with mode | 4 | **done** — `ESC A`-`ESC D`, and DECCKM is asserted *not* to move them: it is an ANSI mode and a VT52 cannot be in one. `the_vt52_cursor_keys_match_the_specification_and_ignore_deccm` |
| — | text keys: printable characters, Control folding, Backspace, Tab, Shift-Tab, Escape, Return | — | **done** — not a ctlseqs table, because ctlseqs tabulates what has no obvious encoding. `control_folds_a_character_into_its_control_code`, `latin1_input_is_encoded_as_latin1` |
| 2713 | VT52 keypad | 24 | **done** — `ESC ? <final>` in application mode, the literal character in numeric, `ESC P`-`ESC S` for PF1-PF4 in both. Its finals are the VT220 table's, not 2498's: keypad `2` is `ESC ? r` where the PC keypad sends `CSI B`, because a VT52 keypad has no cursor duties to borrow. `the_vt52_keypad_matches_the_specifications_table` |
| 1271 | XTMODKEYS `modifyOtherKeys`, `CSI > 4 ; 2 m` | 3 | **done** — levels 0-2. This row did not exist until the resource was implemented, and that is the finding: `modifyOtherKeys` was named in the layer table above as part of the base layer and had **no status row**, so it was invisible to this table's own count while appearing nowhere in the code. A layer named in prose and not enumerated is a layer nothing checks. `the_worked_examples_are_what_baud_sends` |
| 795 | XTFMTKEYS `formatOtherKeys`, `CSI > 4 ; 1 f` | 2 | **done** — picks which sequence the row above sends, and is therefore **the only route to the CSI-u layer**: `[[source: fixterms]]` defines no negotiation, so its form is unreachable without this resource or kitty's stack |
| — | kitty progressive enhancement, all five flags | 5 | **done — all five, and the query is answered.** `[[source: kitty-keyboard]]` treats a reply to `CSI ? u` as the support claim, so it is made only now that all five enhancements are honoured: disambiguate, event types, alternate keys, all-keys and associated text, encoded for text keys, the functional keys, the keypad and the Private Use Area keys (locks, F21–F35, media, and the individual modifier keys flag 8 reports). `CSI = / > / <` maintain the per-screen `KittyStack`. **One stated boundary:** the keypad *navigation* keys (`KP_LEFT`…`KP_BEGIN`) are not modelled, because baud's keypad is digit-based — an embedder that needs them adds them, and no flag's behaviour depends on them. `the_query_is_answered_with_the_current_flags`, `functional_keys_match_the_source`, `pua_keys_match_the_source` |
| 2241 | `modifyOtherKeys` level 3 | 1 | **out** — unmodified keys sent as sequences. Refused and counted rather than rounded down to level 2, so an application can tell its request did not take. `an_unrepresentable_level_is_refused_and_counted` |

## Modifiers

`[[source: ctlseqs]]` line 2565 onward: the parameter is `1 + bitmask`, with
Shift 1, Alt 2, Control 4, Meta 8 -- so shift-F5 is `CSI 1 5 ; 2 ~`. Meta only
appears when `alwaysUseMods` is set, which makes 9 through 16 reachable.

Encoding a modifier is not the same as *having* one. Whether Alt or Meta
prefixes `ESC` or sets the high bit is a pair of resources in xterm, and the
table at line 2337 gives all sixteen combinations across three modes:

| mode | resource | what it does | baud |
|---|---|---|---|
| `?1036` | `metaSendsEscape` | Meta prefixes `ESC` | **done**, and **set** at power-on |
| `?1039` | `altSendsEscape` | Alt prefixes `ESC` | **done**, and **set** at power-on |
| `?1034` | `eightBitInput` | Alt/Meta add 128 instead | **done**, applied under ISO 8859-1 only |

**The defaults are baud's choice and this is where it is recorded.**
`[[source: ctlseqs]]` documents all three and states no default for any of them,
so following it alone would leave the question open. `[[source: foot-meta-defaults]]`
answers it — `meta.esc_prefix = true` in both foot's constructor and its reset
path — and baud follows, because the alternative cannot work under UTF-8: a byte
above 127 is a continuation byte there rather than a character. Same shape as
DECAWM, where the VT510 manual and xterm disagree and `vt-coverage.md` records
which baud follows.

`?1034` is implemented rather than refused because terminfo reaches it: `smm`
and `rmm` are `CSI ? 1034 h` / `l` in every `xterm*` entry, and
`tests/conformance` counted them as recognised-and-dropped for as long as they
were. It is applied only under ISO 8859-1, where a byte above 127 *is* a
character. Under UTF-8 the mode is recorded and reported truthfully and the
shift does not happen — which is a condition on acting, like DECAWM's on the
margin, not a bit set and never read.

**All three were one bit under one number.** `?1034` was mapped to what is
actually `?1036`'s behaviour, so an application asking DECRQM which of the two
opposite branches was in force got the same answer for both, and `?1039` did not
exist. Nothing could detect it while no encoder read the bit; writing one turned
a dormant misnumbering into a wrong answer, which is how it was found.

## Modes that change what a key sends

Each already exists on the receive side with an accessor and no producer. They
are the reason the input path is next rather than the renderer: the parser work
created the demand.

| mode | accessor | affects |
|---|---|---|
| DECCKM `?1` | `cursor_keys_application` | cursor keys: `CSI A` vs `SS3 A` |
| LNM `20` | — | Return: `CR` vs `CR LF` |
| `?1036` / `?1039` | `meta_sends_escape`, `alt_sends_escape` | whether Meta/Alt prefix `ESC` |
| `?1034` | `eight_bit_input` | whether Meta/Alt add 128, under ISO 8859-1 |
| `ESC % @` / `ESC % G` | `input_encoding` | whether a typed character is sent as ISO 8859-1 or UTF-8 |
| DECANM `?2` | `terminal_mode` | VT52: a different keyboard, not a different introducer. The keys a VT52 lacks send nothing |
| DECKPAM / DECKPNM | `keypad` | keypad: `1` vs `SS3 F`. **Not DECCKM** — the keypad's application column is fixed per row, and routing it through the cursor keys' introducer sent `CSI F` for a key the table spells `SS3 F` |
| `?2004` bracketed paste | `bracketed_paste` | wraps pasted text in `CSI 200 ~` / `CSI 201 ~` |
| `?9`, `?1000`-`?1003` | `mouse_tracking` | which pointer events become reports, through `Term::mouse_report`: `?9` a press of button 1, 2 or 3 and nothing else, `?1000` presses and releases, `?1002` motion with a button held, `?1003` all motion, each move only into another cell. **done** — one level at a time, and any reset turns tracking off. The macOS app is the one embedder with a pointer; the Linux app has none, so this row and the two below it do not reach it. This row said `?1000`-`?1003` alone and called them "the only input baud already generates"; `?9` was counted and dropped until 2026-10-09 |
| `?1005`, `?1006`, `?1015`, `?1016` | — | how a report is written: UTF-8 coordinates to 2015, SGR, urxvt's decimal X10, SGR in pixels. **done** — one in force, a set replaces it and a reset undoes only its own; `Term::mouse_report` reads it, so there is no accessor. Under `?1016` the embedder gives the pixel on `MouseEvent::px`, and an event without one is refused and counted. `?1005`, `?1015` and `?1016` were counted and dropped until 2026-10-09 |
| `?1007` alternate scroll | `alternate_scroll` | the wheel on the alternate screen when no tracking takes it: set, the cursor keys through `Term::key`, so in DECCKM's form; reset, nothing. **done** — read by the macOS app's route (`baud-app/src/wheel.rs`). Set at power-on, where xterm's `alternateScroll` resource is off; the owner's call. Until 2026-10-09 the app sent the keys whatever the mode said |
| `?1004` focus reporting | `Term::focus` | **done** — `CSI I` / `CSI O`, sent only while the mode is set. Like the mouse reports above, a report baud generates on its own, not a key it encodes |
| `?1001` highlight tracking | `highlight_tracking`, `highlight_region` | recorded, and the region is not drawn. ~~Needs a selection model to answer~~ -- that reason expired when stream and block selection landed; drawing the region through the selection's highlight is unbuilt |

## Not in scope yet, and why

* **DECUDK** — user-defined keys. ~~It stops being out once there is an
  input side.~~ There is one, and `vt-coverage.md` keeps DECUDK `out` on
  scope rather than on absence: it redefines what a key sends, which needs a
  table of user-programmable strings and a policy for who may set them.
* **Compose, dead keys, IME** — these belong to the platform layer above
  baud's core. Recording them here would be a bit set and never read.
* **Key repeat is the platform's to generate and the core's to encode.** An
  embedder delivers a held key as `KeyEventKind::Repeat`, and the encoder sends
  it as the press everywhere but `[[source: kitty-keyboard]]`'s event types,
  which report it as `:2`: "Normally only key press events are reported and
  key repeat events are treated as key press events." A release sends nothing
  outside event types. `baud-app` on macOS delivers AppKit's repeats, and
  `baud-present-wayland` repeats a held key itself, as Wayland leaves it to
  the client, and delivers releases; macOS delivers none, because the window
  crate has no `keyUp:`. `?8` DECARM is set at power-on, and `Term::key`
  drops a repeat while an application has reset it.
