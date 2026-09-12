# baud — input coverage

What baud must **send**, enumerated against primary sources, in the same shape
as `vt-coverage.md` does for what it receives.

The base layer has started: `Term::key` encodes the cursor keys, Home/End, the
editing keys, F1-F20, the text keys, the keypad and the VT52 tables, in both
DECCKM and both DECKPAM modes, with modifiers and in both input encodings.
**Every row of the base layer is now `done` or `out` with a reason.** What
remains is the two negotiated layers -- `[[source: fixterms]]`'s `CSI u` and
`[[source: kitty-keyboard]]`'s progressive enhancement -- which an application
must ask for, and which no application receives by default: `vt-coverage.md`'s rule is that a row marked **done**
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
functional-key table will be gated against, the way the base rows above are
gated against `extract-keys.py`. The encoder that consumes it is not built yet
-- see the kitty row below.

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
| — | kitty progressive enhancement, all five flags | 5 | **out — all-or-nothing by the protocol's own detection.** `[[source: kitty-keyboard]]` treats the presence of a reply to `CSI ? u` as the support claim, so a terminal cannot implement a subset and stay honest: answering while the enhancements are missing makes applications stop sending legacy-compatible input. `CSI ? u` is therefore consumed and counted, unanswered. `CSI = / > / <` now maintain the per-screen flag stack (`KittyStack`), tracked but read by nothing until the encoder honours every enhancement and the reply is turned on in the same change. Its own words: "It does not make sense to implement only a subset." `the_query_is_refused_and_counted` |
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
| `?1000`-`?1003`, `?1006` | `mouse_tracking` | **done** — the only input baud already generates |
| `?1001` highlight tracking | `highlight_tracking`, `highlight_region` | recorded, and needs a selection model to answer |

## Not in scope yet, and why

* **DECUDK** — user-defined keys. `vt-coverage.md` lists it `out` as
  input-side; it stops being out once there is an input side, and it needs the
  DCS payload the parser already collects.
* **Key repeat, compose, dead keys, IME** — these belong to the platform layer
  above baud, which has no event source. Recording them here would be a bit set
  and never read.
