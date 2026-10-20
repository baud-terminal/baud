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
table: `functional_keys_match_the_source` (Esc, Enter, Tab, Backspace, the
arrows, the editing keys and F1-F20), `keypad_keys_match_the_source` (the
digits, operators and `KP_LEFT` to `KP_BEGIN`) and `pua_keys_match_the_source`
(the locks, F21-F35, the media and modifier keys). A fourth,
`every_row_of_the_table_is_gated`, requires the names those three map to be
exactly the extractor's rows: the navigation rows sat ungated while the
protocol was called complete, because each test named its own keys and none
asked which rows no test named.

| ctlseqs line | table | rows | status |
|---|---|---|---|
| 2473 | cursor keys, normal vs application (DECCKM) | 4 | **done** — `Term::key`, both modes, modifiers force `CSI`. `cursor_keys_match_the_specifications_table` |
| 2485 | Home / End, normal vs application | 2 | **done** — same path; ctlseqs puts them in the arrows' table |
| 2498 | keypad, numeric vs application (DECKPAM) | 24 | **done** — all 24 rows, read out of the table itself: the numeric column, and the application column on the thirteen rows it shares with line 2628. **Its application column for the digits and `.` is the navigation keys'** (the row below): since 2026-10-20 a digit and `.` send line 2628's row, `SS3 p`-`SS3 y` and `SS3 n`, where `5` was `CSI E` and `8` `CSI A`, so a program in application keypad mode reads a digit as a digit and not as an arrow. Since 2026-10-19 a Mac's number pad reaches it too: every keypad key code of `[[source: macos-sdk-events-h]]` names its keypad key, so a program in application keypad mode gets the keypad's own sequences from a Mac, keypad Enter is `SS3 M` there rather than Return's `CR`, and the Wayland driver's `KP_Equal` is the keypad's `=` (`SS3 X`). The numeric column sends the character the key typed, its label on a US layout and `,` from a German Mac's decimal key. `the_keypad_matches_the_specifications_table`, `a_keypad_key_sends_what_it_types_from_the_numeric_column`, `every_mac_keypad_code_names_its_keypad_key`, `a_mac_keypad_key_reaches_the_core_as_a_keypad_key`, `kp_equal_is_the_keypads_equal` |
| 2529 | 12 function keys and special-purpose keys | 12 | **done** — F1-F4 are `SS3 P`-`S` *unconditionally*; F5-F12 are `CSI 15/17/18/19/20/21/23/24 ~`, and the gaps at 16 and 22 are real |
| 2552 | the deprecated `oldXtermFKeys` F1-F4 forms | 4 | **out** — `[[source: ctlseqs]]` line 2548 deprecates them in its own words: "since they do not correspond to any hardware terminal, they have been deprecated". `CSI 11 ~`-`CSI 14 ~` are also what a VT220 would read as F1-F4 of a keyboard that reserves those for Setup. Sending them would be sending a fifth spelling of four keys baud already sends correctly |
| 2628 | VT220 application keypad | 24 | **done** — since 2026-10-20 **a keypad digit or `.` under DECKPAM sends this table's row**, `SS3 p`-`SS3 y` and `SS3 n`, as xterm's own code sends the keysyms on its PC keyboard (`kypd_apl`, `[[source: xterm-input-c]]` 1390-1396) and iTerm2 (`[[source: iterm2-7ce037e-vt100output]]`), foot (`[[source: foot-keymap-h-1-21]]`) and ghostty (`[[source: ghostty-d9840f3-function-keys]]`) send them; the eleven rows where line 2498 differs are what xterm's NumLock-off keysyms send once they are made the cursor and editing keys (1185-1189), baud's navigation keys (Jev, VT220_SS3, 0.94/0.94). A Mac reaches it on every keypad digit, a Linux keypad with NumLock on under `?1035` reset. **A modifier is the parameter between `SS3` and the final**, on every `SS3` row of the keypad but PF1-PF4, which keep the F1-F4 form `CSI 1 ; mod P`-`S` (xterm sends `SS3 mod P`, an open difference): Control-keypad-2 is `ESC O 5 r` where it was `CSI 1 ; 5 B`, xterm's `MODIFIER_PARM`, as foot, ghostty and iTerm2 send it. ctlseqs gives the keypad no modifier form and Jev split (NO_MODIFIER 0.10/0.06, then NO_MODIFIER 0.20 against XTERM_PARAM 0.12); built by the standing rule, the owner's "full VT420-class terminal. we need to support such protocols fully, always!" (2026-09-25). NumLock adds no parameter, and VT52 none, as xterm's `allowModifierParm` needs a VT level. `the_keypad_matches_the_specifications_table`, `the_keypad_digits_send_the_vt220_rows_where_the_tables_part`, `a_modified_keypad_key_under_deckpam_carries_its_modifier` |
| 2672 | VT220 cursor keys | 6 | **done** for Insert/Delete/PageUp/PageDown, which DECCKM does not change. Home and End are sent PC-style from the table at 2485 — the VT220 forms here are xterm's `sunKeyboard` alternative, not an addition, and sending both would be sending two different keys |
| 2685 | the 8 additional VT220 function keys | 8 | **done** — `CSI 25/26/28/29/31/32/33/34 ~`, skipping 27 and 30 |
| 2703 | VT52 keys, which do not change with mode | 4 | **done** — `ESC A`-`ESC D`, and DECCKM is asserted *not* to move them: it is an ANSI mode and a VT52 cannot be in one. `the_vt52_cursor_keys_match_the_specification_and_ignore_deccm` |
| — | text keys: printable characters, Control folding, Backspace, Tab, Shift-Tab, Escape, Return | — | **done** — not a ctlseqs table, because ctlseqs tabulates what has no obvious encoding. `control_folds_a_character_into_its_control_code`, `latin1_input_is_encoded_as_latin1` |
| 2713 | VT52 keypad | 24 | **done** — `ESC ? <final>` in application mode, the literal character in numeric, `ESC P`-`ESC S` for PF1-PF4 in both. Its finals are the VT220 table's, not 2498's: keypad `2` is `ESC ? r` where the PC keypad sends `CSI B`, because a VT52 keypad has no cursor duties to borrow. `the_vt52_keypad_matches_the_specifications_table` |
| 1328 | XTMODKEYS `modifyOtherKeys`, `CSI > 4 ; 2 m` | 4 | **done** — levels 0-3. This row did not exist until the resource was implemented, and that is the finding: `modifyOtherKeys` was named in the layer table above as part of the base layer and had **no status row**, so it was invisible to this table's own count while appearing nowhere in the code. A layer named in prose and not enumerated is a layer nothing checks. `the_worked_examples_are_what_baud_sends` |
| 850 | XTFMTKEYS `formatOtherKeys`, `CSI > 4 ; 1 f` | 2 | **done** — picks which sequence the row above sends, and is therefore **the only route to the CSI-u layer**: `[[source: fixterms]]` defines no negotiation, so its form is unreachable without this resource or kitty's stack |
| 1328 | XTMODKEYS `modifyKeyboard`, `modifyCursorKeys`, `modifyFunctionKeys`, `modifyKeypadKeys`, `modifyModifierKeys`, `modifySpecialKeys`, and `CSI > Ps n` | 6 | **done** — since 2026-10-19 a program can choose how the cursor, function, keypad, modifier and special keys carry their modifiers: Control-Up is `CSI 5 A` at modifyCursorKeys 0 and 1, `CSI 1 ; 5 A` at 2 (the first value), `CSI > 1 ; 5 A` at 3, and at 4 `CSI 27 ; 5 ; 57938 ~`, Up's keysym through `keysym2ucs`; modifyFunctionKeys -1 makes Shift-F1 F11 and Control-F1 F21 (`CSI 42 ~`). Keypad, modifier and special keys change at 4 alone; `modifyKeyboard` governs only the legacy and VT220 keyboards baud does not have, and is stored and reported. `modify_cursor_keys_each_value`, `modify_function_keys_each_value`, `the_keypad_modifier_and_special_resources_act_at_4`, `keysyms_are_the_headers` |
| — | kitty progressive enhancement, all five flags | 5 | **done — all five, and the query is answered.** `[[source: kitty-keyboard]]` treats a reply to `CSI ? u` as the support claim, so it is made only now that all five enhancements are honoured: disambiguate, event types, alternate keys, all-keys and associated text (on a press or a repeat, never a release, which generates no text: `a_release_carries_no_associated_text_and_a_repeat_does`), encoded for text keys, the functional keys, the keypad, its navigation keys (the row above) and the Private Use Area keys (locks, F21–F35, media, and the individual modifier keys flag 8 reports), every row of the functional table gated against the extractor. Since 2026-10-19 a keypad digit or operator typed with NumLock on Linux sends its character under disambiguate alone, as "all non text keypad keys" are the ones reported and "the Lock modifiers are not reported for text producing keys": the Wayland driver gives a keypad key the text it types, none while Control, Alt or Super is held, as kitty's keyboard layer does (`a_keypad_key_that_types_keeps_its_text_under_disambiguate`, `a_keypad_key_that_types_is_its_text_under_disambiguate_alone`); it went out as `CSI 57400 ; 129 u`. A lock or modifier key -- the three locks and the individual Shift to ISO Level 5 keys -- is reported only under "report all keys", where "events for pressing modifier keys are reported", as kitty's encoder drops them otherwise (`[[source: kitty-keys-c-9710eb4]]`); under disambiguate it went out as `CSI 57360 u` (`a_lock_or_modifier_key_is_reported_only_under_report_all_keys`). `CSI = / > / <` maintain the per-screen `KittyStack`. `the_query_is_answered_with_the_current_flags`, `functional_keys_match_the_source`, `pua_keys_match_the_source`, `every_row_of_the_table_is_gated` |
| 2498 | the keypad's navigation keys, NumLock off: `KP_LEFT`…`KP_BEGIN` | 11 | **done** — `KeypadKey::Left`…`Begin`, a separate identity from the digit on the same key because `[[source: kitty-keyboard]]` numbers them apart (`KP_4` 57403, `KP_LEFT` 57417). Under any kitty flag each is its own number, `KP_BEGIN` the letter form `1 E`. On the base layer, under DECKPNM each is the key it duplicates (ctlseqs line 2462: the keypad's operations "duplicate the smaller cursor and scrolling keypads"; kitty: "All keypad keys are reported as their equivalent non-keypad keys"), so DECCKM, modifiers and `modifyOtherKeys` reach it as they reach that key; under DECKPAM it sends the row of line 2498 it shares with its digit ("4 (LeftArrow)", `CSI D`); `Begin` is `CSI E` throughout: line 2518's spelling, and the first of the two kitty's table gives (`1 E or 57427 ~`); on a VT52 the arrows are `ESC A`-`ESC D` and the rest send nothing, as the keys they duplicate do, in both keypad modes. This is the equivalent-key rule, not the row rule built for ANSI DECKPAM: under DECKPAM `KP_LEFT` sends `ESC D`, not its digit's `ESC ? t`; line 2713 does not settle which, so this belongs to the open DECKPAM fork. **Open, on the base layer only** (the kitty forms are the table's): the DECKPAM rule (Jev 0.74 for it over the equivalent key, confidence 0.48, twice) and `Begin` under DECCKM (`CSI E` over `SS3 E`, confidence 0.92 then 0.86) are both under the bar; the two rules part only for the arrows, Home and End under DECKPAM. Linux delivers them from the keysym NumLock picks; **macOS cannot**: `[[source: macos-sdk-events-h]]` gives each keypad key one code, by its digit, and no NumLock key. `the_keypad_navigation_keys_follow_their_rows_and_their_keys`, `a_vt52_keypad_navigates_with_its_arrows_and_nothing_else`, `keypad_keys_match_the_source`, `numlock_chooses_between_the_navigation_key_and_the_digit` |
| 2377 | `modifyOtherKeys` level 3 | 1 | **done** — since 2026-10-19 unmodified keys too are sent as sequences, space as `CSI 27 ; 1 ; 32 ~`, the text's example, read from it; a level past 3 is refused and counted. `modify_other_keys_3_sends_every_ordinary_key`, `an_unrepresentable_level_is_refused_and_counted` |

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

**Option-Backspace deletes a word in a shell**, since 2026-10-19: the
three modes reach Return, Tab, Backspace and Escape as they reach a
character, so with `?1039` set Alt-Backspace sends `ESC DEL` where it sent
a bare DEL, Alt-Return `ESC CR`, Alt-Tab `ESC TAB`, Alt-Escape `ESC ESC` and
Alt-Shift-Tab `ESC CSI Z`. xterm sends those keys down its text branch
(`[[source: xterm-input-c]]`, `kd.nbytes > 0`), and every cell of
`[[source: kitty-keyboard]]`'s legacy table for them is held by
`alt_prefixes_the_c0_keys_as_it_prefixes_text`.

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
| KAM `2` | `keyboard_locked` | every key: while a program has locked the keyboard, a key sends nothing and draws nothing. **partial** -- VT510's Wait indicator waits on a status line (`vt-coverage.md`'s KAM row) |
| SRM `12` | `local_echo` | reset, a key is drawn as well as sent, through a parser of the echo's own; set at power-on. **done** -- `baud-core/tests/keyboard_modes.rs` |
| DECNKM `?66` | `keypad` | the keypad, as DECKPAM/DECKPNM, and `DECRQM` reads it. **done** |
| DECBKM `?67` | — | Backspace: reset, the default, DEL; set, BS; Control inverts it (xterm's toggle, the kitty legacy table). **done** |
| `?1035` | — | the keypad: set at power-on, NumLock overrides DECKPAM and a keypad key sends its digit, the whole keypad on Linux; on macOS, whose keypad has no NumLock key, nothing changes. **done** |
| `?1037` | — | Delete on the editing keypad: reset, `CSI 3 ~`; set, DEL, unmodified only; the numeric keypad's Delete stays `CSI 3 ~`. **done** |
| `?1036` / `?1039` | `meta_sends_escape`, `alt_sends_escape` | whether Meta/Alt prefix `ESC` |
| `?1034` | `eight_bit_input` | whether Meta/Alt add 128, under ISO 8859-1 |
| `ESC % @` / `ESC % G` | `input_encoding` | whether a typed character is sent as ISO 8859-1 or UTF-8 |
| DECANM `?2` | `terminal_mode` | VT52: a different keyboard, not a different introducer. The keys a VT52 lacks send nothing |
| DECKPAM / DECKPNM | `keypad` | keypad: `1` vs `SS3 F`. **Not DECCKM** — the keypad's application column is fixed per row, and routing it through the cursor keys' introducer sent `CSI F` for a key the table spells `SS3 F` |
| `?2004` bracketed paste | `bracketed_paste` | wraps pasted text in `CSI 200 ~` / `CSI 201 ~` |
| `?2005` / `?2006` readline paste | `resource` | **done** — a program can have a paste quoted for the shell's line editor: `?2005` puts the tty's literal-next character, `c_cc[VLNEXT]` or Control-V, before each byte of the text (never of a bracket), and `?2006` keeps an unbracketed paste's line breaks where they are otherwise sent as CR, as xterm's `_qWriteSelectionData` and `_WriteSelectionData` do (`[[source: xterm-button]]`; `a_paste_body_follows_2005_and_2006`, `readline_quoting_and_newlines_shape_a_paste`). Under `?2004` a bracketed paste keeps its line breaks whatever `?2006` says. xterm's `_WriteSelectionData` turns LF into CR inside the brackets too unless `?2006` is set, and that is a departure, recorded as open. macOS only: the Linux app has no paste path |
| `?2001` readline mouse button-1 | `resource` | **done** — a click on the cursor's line moves the shell's point there with cursor keys, as many as xterm's `ReadLineMovePoint` counts: the click's nearest cell boundary, a whole width for each wrapped row between, less the cursor's column (`a_click_on_the_cursors_line_moves_the_point_by_the_columns_between`, `a_wrapped_line_counts_a_whole_width_a_row_and_an_ended_row_stops_it`). Sent as the wheel's keys are, not as keystrokes. Only an unmodified click with no drag, on the live screen; xterm also bounds the click's time. Both drivers, the Linux one since 2026-10-20 (`a_clean_click_moves_the_point`). **A prompt that asked with `OSC 133` takes the click first** (the row below) |
| `OSC 133 ; A ; cl=` click-to-move, kitty's `click_events=` and `special_key=`, the proposal's `move-keys=` | `Term::prompt_click` | **done** — since 2026-10-19 a click on the line being typed at a shell prompt moves the cursor there, when the prompt asked: `cl=` sends as many Left or Right keys as characters lie between the cursor and the click, "double-width characters only count one", a click in the left prompt counted as the input's first character and one past the input as just after it (`[[source: semantic-prompts]]`; `a_prompt_click_counts_characters_and_keeps_to_the_input`, `a_prompt_click_stays_on_the_cursors_line`). The input begins where `B` or `I` put the cursor and ends at the first cell nothing wrote, so the gap before a right prompt is background; the pad a wide character leaves in the last column when it wraps is neither the end nor a character (`a_wide_character_wrapped_at_the_margin_leaves_no_character`). Every `cl=` value is read as `line`, which the text permits (Jev, SPEC_LINE, 0.80/0.89, the lean taken). The keys are the cursor keys as the wheel sends them, `CSI 1 u` / `CSI 1 ; 1 u` under `special_key=1`, or `move-keys=` as sent. `click_events=1` and `2` send the click to the shell instead, a left press `CSI < 0 ; x ; y M`, the row the screen's or counted from the prompt's top or its later line at or above the click, as kitty's text puts it, and 1-based as SGR is: the text's "y is zero for cells on the top line of the current prompt" is sent as row 1, as kitty's `mouse.c` sends it (`[[source: kitty-shell-integration]]`, `[[source: kitty-6d5d0c4-mouse]]`; `a_prompt_report_is_an_sgr_press`) -- kitty's code counts from the prompt line nearest the cursor and sends nothing for a click above it, practice not followed, for a click at or below the prompt's first row: fish and nushell ask for it. Jev leaned out on kitty's two (0.76/0.60); the standing rule outranks a lean that leaves a protocol unbuilt. Over `?2001` while a prompt asks, and never once an `I` line has ended (`a_click_after_an_i_line_ended_is_no_prompts`). Both drivers, the Linux one since 2026-10-20 through the same rule (`gesture::clicked`, `a_clean_click_moves_the_point`) |
| Command-Up / Command-Down: the previous and the next prompt | `Term::jump_to_prompt` | **done** — since 2026-10-19 the window over the history jumps to the shell's previous or next prompt, its first line at the top, as kitty and Ghostty place it, and past the newest prompt to the live screen; Terminal's own keys ("Jump to previous mark Command-Up Arrow", `[[source: apple-terminal-shortcuts]]`), Ghostty's macOS default. Not a key sent to the program: Command chords are the app's. Jev's lean, CMD_ARROWS (0.56/0.61); the landing TOP at 0.96/0.98. No menu item, which lives in the window crate. macOS only |
| `?1011` scroll on key press | `resource` | **done** — set at power-on, so a key sends the window over the history to the live screen as it always has; a program can turn that off. A paste still snaps |
| `?9`, `?1000`-`?1003` | `mouse_tracking` | which pointer events become reports, through `Term::mouse_report`: `?9` a press of button 1, 2 or 3 and nothing else, `?1000` presses and releases, `?1001` the same with button 1's handshake (below), `?1002` motion with a button held, `?1003` all motion, each move only into another cell. **done** — one level at a time, and any reset turns tracking off. Both apps have a pointer: the macOS app's from AppKit, and since 2026-10-20 the Linux app's from `wl_pointer`, gathered a `frame` at a time (`[[source: wayland]]`), its multi-click counted as xterm's `EvalSelectUnit` counts, the same button within `multiClickTime`'s default 250 ms of the last release (`[[source: xterm-man]]`, `[[source: xterm-button-c]]`; Jev, XTERM_250, 1.00/0.98), and its wheel a line a detent of `axis_value120` or `axis_discrete`, a touchpad's length by the cell's height (Jev's lean, ONE, 0.14/0.27). This row said `?1000`-`?1003` alone and called them "the only input baud already generates"; `?9` was counted and dropped until 2026-10-09 |
| `?1005`, `?1006`, `?1015`, `?1016` | — | how a report is written: UTF-8 coordinates to 2015, SGR, urxvt's decimal X10, SGR in pixels. **done** — one in force, a set replaces it and a reset undoes only its own; `Term::mouse_report` reads it, so there is no accessor. Under `?1016` the embedder gives the pixel on `MouseEvent::px`, and an event without one is refused and counted, but for `?1001`'s short release report, which writes no mouse position. `?1005`, `?1015` and `?1016` were counted and dropped until 2026-10-09 |
| `?1007` alternate scroll | `alternate_scroll` | the wheel on the alternate screen when no tracking takes it: set, the cursor keys through `Term::key`, so in DECCKM's form; reset, nothing. **done** — read by both apps' route (`baud-app/src/wheel.rs`; `the_alternate_screens_wheel_is_the_cursor_keys`). Set at power-on, where xterm's `alternateScroll` resource is off; the owner's call. Until 2026-10-09 the app sent the keys whatever the mode said |
| `?1004` focus reporting | `Term::focus` | **done** — `CSI I` / `CSI O`, sent only while the mode is set. Like the mouse reports above, a report baud generates on its own, not a key it encodes |
| `?1001` highlight tracking | `mouse_report`, `highlight` | **done** — a tracking level: button 1's press is owed the program's answer, the region the answer starts follows a drag within the program's rows, and the release is reported as `CSI t` or `CSI T`; both apps draw `Term::highlight` through the selection's highlight (`tracked_highlight`). `tests/xterm/highlight.rs` |
| DECELR, DECSLE, DECEFR, DECRQLP: DEC's text locator | `Term::locator`, `mouse_tracking` | **done** — since 2026-10-20 a program written for DEC terminals can take the pointer as DEC's locator: both apps hand every move, crossing, press and release to `Term::locator` with the buttons held (`baud-app/src/gesture.rs`, from the macOS view and the Linux seat), a press and a release only when the route gives them to the program, so Shift still selects. Off the text area the position is undefined and a report omits it. The wheel scrolls as with no protocol, and the side button is the locator's fourth. What is sent is DECLRP, in `docs/vt-coverage.md`'s rows (`the_locator_takes_the_pointer_under_decelr`, `the_locator_gets_a_press_only_through_the_child_route`, `the_locator_s_position_is_undefined_off_the_text_area`) |

## Terminfo input capabilities baud does not send

An application that reads a terminfo entry learns which bytes each key
sends there. baud sends xterm's (`TERM=xterm-256color`), and of the ten
pinned profiles it sends 139 of `xterm`'s 140 input capabilities, all 22 of
`vt100`'s, 38 of `rxvt`'s 99, 43 of `nsterm`'s 45, 26 of `dtterm`'s 32
and 7 of `ansi`'s 8, each profile's `smkx` modes included and the mouse
report `kmous` prefixes counted. Every one it does not send is named below
with the reason; `the_unsent_input_capabilities_are_named` requires this
table to name exactly the ones the encoder does not produce, and each
profile's count in `docs/protocols/profiles/` to be the measured one.

| profiles | capabilities | why baud does not send them |
|---|---|---|
| `xterm` `xterm-16color` `xterm-256color` | `kb2` | the keypad's centre key, Begin, as `SS3 E` under DECCKM. baud sends `CSI E`, the form at `[[source: ctlseqs]]` line 2518; the DECCKM form is the open Begin fork above, under the bar at 0.92 then 0.86 |
| `dtterm` `rxvt` | `kf1` `kf2` `kf3` `kf4` | `CSI 11 ~`-`CSI 14 ~`, the `oldXtermFKeys` forms line 2548 deprecates; baud sends F1-F4 as `SS3 P`-`SS3 S`, the out row above |
| `dtterm` `rxvt` | `kfnd` `kslt` | Find and Select, `CSI 1 ~` and `CSI 4 ~`: the DEC column of the "Non-Function Keys" table, whose header row is line 2771, which xterm uses for the X keys of those names. baud has no Find or Select key: neither a Mac keyboard nor its Linux driver delivers one, and kitty's functional table numbers neither. Open, beside Help, Menu and Remove from the same table |
| `nsterm` | `kLFT3` `kRIT3` | `ESC b` and `ESC f`, Terminal.app's Option-Left and Option-Right as Emacs word motion. baud sends `CSI 1 ; 3 D` and `CSI 1 ; 3 C`, ctlseqs' modifier form |
| `ansi` | `kich1` | `CSI L`, Insert in the SCO column of the table whose header row is line 2771. baud sends `CSI 2 ~`, the DEC form |
| `rxvt` | `kDC` `kEND` `kHOM` `kIC` `kNXT` `kPRV` `kDC5` `kDC6` `kEND5` `kEND6` `kHOM5` `kHOM6` `kIC5` `kIC6` `kNXT5` `kNXT6` `kPRV5` `kPRV6` `kel` `kf21` `kf22` `kf23` `kf24` `kf25` `kf26` `kf27` `kf28` `kf29` `kf30` `kf31` `kf32` `kf33` `kf34` `kf35` `kf36` `kf37` `kf38` `kf39` `kf40` `kf41` `kf42` `kf43` `kf44` | rxvt's own modified keys: Shift ends the editing keys' sequence in `$`, Control in `^`, both in `@`, and its F21-F44 are its function keys with those three suffixes. baud sends ctlseqs' `CSI 3 ; 2 ~` form, which an application told `TERM=xterm-256color` reads |
| `rxvt` | `kLFT` `kRIT` `kUP` `kDN` `kind` `kri` `kLFT5` `kRIT5` `kUP5` `kDN5` `khome` `kend` | rxvt's shifted and controlled arrows, `CSI a`-`CSI d` and `SS3 a`-`SS3 d`, and its Home and End, `CSI 7 ~` and `CSI 8 ~`: rxvt's keyboard, not ctlseqs' |

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
* **A Mac's number pad is a keypad.** Its keys are named by key code, not
  by `NSEventModifierFlagNumericPad`, which AppKit also sets on the arrows,
  and each carries the text it types, as on Linux, so kitty's disambiguate
  flag keeps a typed digit a digit. A Mac keypad has no NumLock state:
  no NumLock is reported, so `?1035` changes nothing there and DECKPAM
  always reaches the digits. Clear, the key where a PC keypad has NumLock,
  is NumLock: the HID tables name the one key "Keypad Num Lock and Clear"
  (`[[source: usb-hid-usage-tables-1-6]]`, usage 0x53) and kitty's macOS
  layer names it `NUM_LOCK` (`[[source: kitty-glfw-cocoa-window-851cb80]]`),
  over iTerm2's `ESC` (`[[source: iterm2-7ce037e-standard-key-mapper]]`)
  and its kitty mapper's `KP_SEPARATOR`, "a blind guess" (`[[source:
  iterm2-7ce037e-modern-key-mapper]]`) -- Jev, NUM_LOCK, 0.95/0.96. It sends
  nothing outside kitty's "report all keys", as before. A keypad key is
  drawn ahead of the echo only from the numeric column.
* **A keypad key types what the layout puts on it.** From the numeric
  column, and under kitty's disambiguate flag, a keypad key sends the
  character it typed, which is its label on a US layout and not on every
  layout: a Mac's decimal key types `,` on German and French layouts and
  its digits type Arabic-Indic digits on Arabic ones. A decimal key that
  types `,` is the keypad's comma, line 2498's `SS3 l` under DECKPAM and
  kitty's `KP_SEPARATOR`, as xkeyboard-config makes a German Linux
  keypad's (`[[source: xkeyboard-config-kpdl-193cc6c]]`), where kitty's
  and iTerm2's macOS layers keep `KP_DECIMAL` -- Jev's lean,
  SEPARATOR_BY_TEXT, below the bar
  (`a_keypad_key_sends_what_it_types_from_the_numeric_column`,
  `a_mac_keypad_key_sends_what_the_layout_types_on_it`).
