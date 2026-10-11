# VT protocol coverage

What baud implements, what it does not, and what it has decided not to. Every
row traces to a primary source in `docs/research/sources/`, cited as
`[[source: <key>]]` and checked by `scripts/check-sources.sh`.

**And it is checked in both directions.** The specification side covers plain
CSI finals, CSI forms with markers and intermediates, the `ESC Fe` controls,
every `OSC Ps` and every `DCS` form — the whole escape surface an application
can reach, rather than the part baud happened to list.

 Every **done** row is fed to a real
`Term` and must reach a handler (`enumerated.rs`); and every plain CSI final
`[[source: ctlseqs]]` defines must be either handled or counted in
`Term::unsupported` (`spec_inventory.rs`). The second matters because this file
is *baud's own* enumeration: an operation the specification defines and baud
never listed is invisible here — not `todo`, not `out`, absent — and checking
the table against the code cannot see a row nobody wrote.

The scope subset is deliberate and the check accommodates it rather than
demanding parity: ctlseqs documents far more than baud commits to, so the
requirement is not that every operation be implemented. It is that none be
*silently consumed*.

**This file is the scope definition.** "Implement the VT protocol" is not a
statement with a truth value until the surface is enumerated, because the
surface is not finite in any useful sense: `[[source: ctlseqs]]` documents 117
CSI sequences alone, many of them xterm extensions with no other implementation.
Tiers below say what baud is committing to and what it is not.

Status values are deliberately blunt:

| | meaning |
|---|---|
| **done** | implemented, dispatched, and covered by a test that would fail if it regressed |
| **partial** | implemented but known-incomplete; the gap is named in the row. Nine rows carry it: DECSC, DECRC, the DEC Special Graphics glyph table, the `dcs_param` and `dcs_passthrough` states, GR bytes under `ESC % @` in an escape or control sequence or an OSC, APC or PM body, the `escape` and `escape_intermediate` states, and `DEL` in `csi_entry` or `csi_intermediate` |
| **parsed** | recognised by the parser and then **dropped** — worse than missing, because it looks supported |
| **todo** | in scope, not implemented |
| **out** | deliberately not in scope; the reason is in the row |

A `parsed` row is a bug. It is the state where a terminal accepts a sequence,
does nothing, and gives the application no way to find out.

**A `done` row cannot quietly become a `parsed` one.**
`every_done_sequence_reaches_a_handler` synthesises each done row's sequence
*from this table* and feeds it to a real `Term`; `Term::unsupported` must not
move. Marking a row done for an operation the parser drops is therefore a test
failure, not something a reader has to notice. Verified against a planted
regression rather than assumed.

Be exact about what that does and does not prove. It proves the parser reaches
a handler for every operation this table claims. It does **not** prove the
handler is correct — that is what the per-protocol suites are for, and no
mechanism can prove it in general.

**No row carries `parsed` or `todo`. Ten carry `**partial**`, and
`no_row_is_unfinished` in `baud-core/tests/coverage.rs` does not see them.**
This paragraph said no row carried any of the three and that the test fails if
one ever does. The test matches a status cell that starts with the bare word,
and these start with `**`, so it passes with DECSC, DECRC and the DEC Special
Graphics glyph table all at `**partial**`, the `dcs_passthrough` state
since 2026-10-09, and since 2026-10-10 GR bytes under `ESC % @`, the `escape`
and `escape_intermediate` states, `DEL` in `csi_entry` or
`csi_intermediate`, `3A` in the `dcs_param` state, and SGR 6's rapid blink. A check that saw them
would fail on all ten, and that is
the right answer: each row names work that is not finished. `out` is the only status that
means "not implemented", and `every_out_row_states_a_reason` requires each one
to say why **in the row** — because otherwise "nothing is partial" would be a
word game, satisfiable by relabelling anything inconvenient as out of scope.
The check cannot judge whether a reason is *good*; that is review. It can insist
one exists where a reader will find it. `out` is the only status
that means "not implemented", and it requires a reason in the row. Do not
relabel an unfinished operation as `out` to satisfy the check: reaching for
`partial` is the signal that the operation is not done, and the fix is the
operation.

---

## Tier 0 — the parser state machine

Everything below depends on this being right, so it is not a tier of features
but the substrate. `[[source: williams]]` specifies the DEC ANSI parser as a
13-state machine. baud now has all of them; the `Utf8` state below is an
addition, not a substitution.

| state | in baud | note |
|---|---|---|
| `ground` | done | |
| `escape` | **partial** | **A `DEL` ends the sequence**, where `[[source: williams]]`'s `escape` ignores `7F` and stays (286): the `Escape` node takes `7F` to `Consume` into ground, so `ab ESC DEL 7 cd ESC 8 X` draws `ab7cdX`, the DECSC lost and the `7` drawn, where `ab ESC 7 cd ESC 8 X` draws `abXd`. Found 2026-10-10 and not built (the record, "A GR byte in a DCS prefix is its 7-bit byte under ISO 8859-1"). A GR byte under `ESC % @` is the ISO 8859-1 row's gap (below) |
| `escape_intermediate` | **partial** | **collects** intermediates, plural — it kept only the last, which made every two-byte charset final unreachable. **A `DEL` ends the sequence**, where williams' `escape intermediate` ignores `7F` and stays (293): baud's one `Escape` node is both williams states, so `ESC ( DEL 0 q` designates nothing and draws `0q`, where `ESC ( 0 q` draws a DEC Special Graphics glyph. Found 2026-10-10 and not built (`escape`) |
| `csi_entry` | done | its own state — see below. A `DEL` here is the gap of the row "`7F` DEL in `csi_entry` or `csi_intermediate`", and a GR byte under `ESC % @` the ISO 8859-1 row's (both below), each counted there |
| `csi_param` | done | `CsiParams` |
| `csi_intermediate` | done | intermediate recorded, in both feed paths. A `DEL` here is the gap of the row "`7F` DEL in `csi_entry` or `csi_intermediate`", and a GR byte under `ESC % @` the ISO 8859-1 row's (both below), each counted there |
| `csi_ignore` | done | |
| `dcs_entry` | done | its own state — the marker rule, as `csi_entry`; C0 **ignored**, `BEL` among them (`[[source: williams]]` 354), which ended the string until 2026-10-09 (`a_bel_before_a_dcs_body_is_ignored`). **Under `ESC % @` a GR byte is read as its 7-bit byte**, here and in the two rows below: williams, "In all states, these codes are treated identically to GL codes 20-7F" (462); `[[source: dec-std-070]]` 3.5.4.5, in "the introducer sequence to a Device Control String, the eighth bit will be ignored, and they will be treated as their 7-bit equivalent" (PDF p. 135); and `[[source: ecma48]]` §9, "10/00 to 15/14 shall have the same meanings" as "02/00 to 07/14". Until 2026-10-10 each state sent a GR byte to the body, so `ESC P A4 F1 m ST` was no DECRQSS and `90 B0 BB B2 F1` no Sixel (`a_gr_byte_in_a_dcs_prefix_is_read_as_its_seven_bit_byte_under_latin1`, `a_gr_byte_in_a_sixel_introducer_is_read_as_its_seven_bit_byte_under_latin1`, `a_gr_byte_in_a_dcs_prefix_takes_its_seven_bit_edge_under_latin1`; under UTF-8 nothing changed, `a_gr_byte_in_a_dcs_body_or_under_utf8_is_what_it_was`). The texts agree on `A1-FE`; `FF`, which §9 stops short of and williams reads as `7F`, is ignored where `7F` is, and `A0` leads to the body as `20` does. `3A` is collected here as in `dcs_param`, the gap that row names, for Jev |
| `dcs_param` | **partial** | C0 **ignored**, not executed, `BEL` among them (361). Under `ESC % @`, `B0-B9`, `BA` and `BB` read as `0-9`, `:` and `;` and are parameter bytes, and a Sixel's `Pb` is read from them (`dcs_entry`). **The gap, for Jev: `3A`**, and so `BA`, is collected here and in `dcs_entry` with `;`'s action, a parameter separator, so `ESC P 0:1 q` is a Sixel and `ESC P 1 : $ q m ESC \` answers. williams sends it to `dcs ignore`, "the character 3A appears anywhere" (589). `[[source: dec-std-070]]` agrees through its own cross-reference: 3.5.4.1 makes the parameter string "identical in syntax to the parameter string of a control sequence" (PDF p. 133), and 3.5.3.1 says of that string "The bit combination 3/10 is reserved for future standardization. If 3/10 is received within a parameter string, the entire sequence up to and including the final character shall be ignored" (PDF p. 129). `[[source: ecma48]]` alone permits it, and for a control sequence: 5.4.2 b, "bit combination 03/10 may be used as a separator in a parameter sub-string, for example, to separate the fractional part of a decimal number from the integer part of that number", the syntax baud's `csi_param` accepts (`SubSep`); 8.3.27 gives a DCS no parameter string, "The purpose and the format of the command string are specified by the most recent occurrence of IDENTIFY DEVICE CONTROL STRING (IDCS), if any, or depend on the sending and/or the receiving device". No text read makes `3A` a `;`. The choice: ignore the string, as williams and STD 070 do, or read `3A` within one parameter, by ECMA-48 through STD 070 3.5.4.1's "identical in syntax". No test pins either reading; see "The DCS prefix had the same merge" below |
| `dcs_intermediate` | done | `30-3F` ends the sequence — the parameter string is over; C0 ignored, `BEL` among them (368). Under `ESC % @`, `A4` is DECRQSS's `$` and `F1` its `q`, and `F1` alone a Sixel's (`dcs_entry`) |
| `dcs_passthrough` | **partial** | payload collected for DECRQSS and Sixel. **The format effectors `08-0D` are payload and `DEL` is ignored**, williams' *put* and *ignore* (388), the two classes every text agrees on: `[[source: ecma48]]` 8.3.27 lets DCS's command string hold "00/08 to 00/13 and 02/00 to 07/14", `[[source: dec-std-070]]` 3.5.4.4 calls them "valid within control strings", and williams passes them "to the handler" (726); `DEL` williams ignores with no condition, STD 070 3.5.4.4 recommends ignoring "all other control codes not used within a command string", and ECMA-48 F.8.1 lets it be "removed from, a CC-data-element without affecting its information content". Only xterm stores it, practice not followed. Until 2026-10-10 the body took the `osc_string` row for both, so no handler saw a format effector and `DCS $ q m DEL ST` answered invalid (`a_dcs_body_puts_the_format_effectors_and_ignores_del`, `a_del_in_a_decrqss_pt_is_ignored`, `a_format_effector_in_a_dcs_body_reaches_the_handler_and_a_del_does_not`, `the_dcs_lf_del_atom_reaches_the_handler`; `an_osc_and_an_apc_keep_their_rows_for_a_format_effector_and_a_del` holds the other two bodies). **Both handlers drop a format effector**: DECRQSS as STD 070 3.5.4 recommends, "that these characters not affect the interpretation of the control string", so `m CR` names SGR as it did (`a_format_effector_in_a_decrqss_pt_changes_nothing`; xterm's `strcmp` answers invalid), and Sixel as STD 070-9 §9.1 requires, "These codes shall have no effect on the interpretation of Sixel Control strings". **`BEL` is payload and ends nothing**, williams' *put* (388): it ended the string, an `OSC`'s terminator in a DCS, until 2026-10-09, so `DCS $ q m BEL` answered and drew what followed; now only ST ends it, and a DECRQSS whose `Pt` holds one answers invalid (`a_bel_in_a_dcs_body_is_payload_and_only_st_ends_it`, `a_bel_inside_a_sixel_is_payload_and_draws_nothing`, `a_bel_does_not_terminate_a_dcs`). **A Sixel's handler drops every C0 it is put before its decoder**, and the graph drops `DEL`, so neither a put `BEL` nor a `DEL` reaches a Sixel number; a stored `7F` split one until 2026-10-09, `!1 DEL 0~` drawing one column for ten, as a put `BEL` would (`a_c0_or_del_inside_a_sixel_number_splits_nothing`). xterm hands its Sixel parser only `CASE_IGNORE` bytes (`[[source: xterm-reset]]`'s file, 3408-3412), so not the `BEL`, which is `CASE_BELL`; the `DEL` is one, and ends a number there (`[[source: xterm-graphics-sixel]]` 541-555), a departure not followed. **Open, the owner's call, where the texts conflict, each class as it was:** the other C0, `00-06`, `0E-17`, `19`, `1C-1F`, which williams puts ("C0 controls are also passed to the handler", 726) and STD 070 3.5.4.4 recommends "be ignored by hardware", as `[[source: vt510-rm-b]]` Table 4-1 ignores NUL, and which baud ignores while it puts `BEL`, a split no one text makes; within them `BEL` and `ENQ` in a Sixel, where STD 070-9 §9.1 gives each the "same action as in text mode" (PDF p. 926), as xterm rings a `BEL`, and baud's Sixel handler drops the put `BEL` and the graph the `ENQ`; `SUB` in a Sixel, which 070-9 §9.1 says to "process as if a 3/15 code (blank Sixel) was received", where STD 070 3.5.4.4 and williams end the string and baud ends it; SO and SI, which ECMA-48 §9 says "have no effect on the interpretation of ... a control string, but they may, indeed, affect the meanings of bit combinations following", and which baud drops without shifting; and `A0-FF` under ISO 8859-1, stored raw, where ECMA-48 §9 says "the bit combinations in the range 10/00 to 15/14 shall have the same meanings as the corresponding bit combinations in the range 02/00 to 07/14" and STD 070 3.5.4.5 makes a DCS's GR characters "dependent on the internal control string format", which for a Sixel 070-9 §9.4 sets: "The 8th bit is set to zero and the code is then processed as a GL code". **Open beside them**: a string CAN, SUB, ESC or a C1 cancels is not handed to its handler, where williams' *unhook* calls it "with an 'end of data' parameter" (728); and SUB draws no error character (the `1A` rows below). It read `done` until 2026-10-09 with the gap unnamed |
| `dcs_ignore` | done | unhandled DCS consumed, never executed |
| `osc_string` | done | payload captured, bounded, and dispatched. **The one string `BEL` ends**, by `[[source: ctlseqs]]` ("XTerm accepts either BEL or ST for terminating OSC sequences", 2023-2024) where williams ignores it (407) (`a_bel_still_ends_an_osc`). Under `ESC % @` a GR byte is stored as it is, where `[[source: dec-std-070]]` 3.5.4.5 drops its eighth bit in an OSC: the ISO 8859-1 row's gap (below) |
| `sos_pm_apc_string` | done | payload consumed, never executed. **`BEL` is ignored and ends nothing**, williams' row (426) and ctlseqs' "Pt need not be printable characters" (432-433, 2278-2279); it ended the string until 2026-10-09 (`a_bel_in_sos_pm_or_apc_is_ignored_and_only_st_ends_it`). STD 070 3.5.4.5 drops a GR byte's eighth bit in an APC or PM too, and baud does not; no handler reads these bodies, so nothing moves yet (the ISO 8859-1 row, below) |

~~`sos_pm_apc_string` is a correctness and security gap.~~ Resolved: SOS, PM
and APC payloads are consumed by the graph's `SosStr` node and never executed,
and a DCS body by `DcsStr`, which hands it only to the DECRQSS and Sixel
handlers (the `dcs_passthrough` row above). Until 2026-10-10 this named a
`StringIgnore`, which `baud-core` does not have. Note the
specified limit, which is not a hole: `[[source: williams]]` says a bare `ESC`
*cancels* a control string, and so does a C1 control (its anywhere rule, lines
612-615), so the guarantee is precisely that a payload containing no ESC, and
under ISO 8859-1 no byte in `80-9F`, is inert. The C1 half holds in the
`dcs_*`, `osc_string` and `sos_pm_apc_string` states since 2026-10-09; see
the anywhere block. **And until 2026-10-09 a `BEL` broke it too**: one body
node served all five strings and ended each on `BEL`, so a DCS, SOS, PM or
APC payload holding one was not inert past it. Only an `OSC` ends on `BEL`
now, the one string `[[source: ctlseqs]]` lets it end.

baud adds a `Utf8` state, which `[[source: williams]]` does not have — that
parser predates UTF-8 and is defined over bytes. Keeping it is deliberate; the
note in `docs/research/notes/c-vt-parser.md` records why.

### `csi_entry` was folded into `csi_param`, and the fold was the bug

The two states differ in one transition, and it is the one that matters. In
`csi_entry` the bytes `3C-3F` (`<`, `=`, `>`, `?`) are **collected** as a private
marker; in `csi_param` those same bytes go to `csi_ignore`. A marker is legal
only in the position immediately after `CSI`.

Folded into a single state, baud accepted a marker anywhere in the parameter
string — and then *executed* the result, which is the dangerous direction:

* `CSI 2 ? l` is malformed and must be discarded. baud read it as DECRST 2,
  reset DECANM and **switched the terminal into VT52 mode** — a protocol change
  produced by a sequence the spec says to throw away.
* `CSI 1 ? J` is malformed and must be discarded. baud read it as DECSED and
  erased the screen.

`a_marker_after_a_dcs_parameter_is_ignored` and its CSI counterpart pin both. Splitting the state also
closed two smaller holes in the same arm:

* **C0 inside a control sequence.** `[[source: williams]]` gives `csi_param` an
  `execute` action on `00-17,19,1C-1F` that returns to the same state. baud had
  no C0 branch, so a stray NUL fell through to `csi_ignore` and silently
  cancelled an unrelated SGR. core2's pair of C0-inside-a-CSI tests pinned the
  rule and the fact that
  *execute* means executed, not skipped.
* **`<` and `=` were dropped silently.** baud recognised only `?` and `>`, so
  the other two private namespaces fell to `csi_ignore` and vanished without
  reaching dispatch — a `parsed` failure, invisible to `Term::unsupported`.
  `Marker` now carries all four. `Csi::parse` rejects `Lt` and `Equals` before
  the standard table, because falling through would read `CSI < c` as a primary
  DA and **answer a query in the wrong shape**.

Both feed paths share one definition of the rule (`Marker::from_private`), so
the fast path and the state machine cannot drift on it — the divergence class
this parser has had before.

### The "anywhere" block

`[[source: williams]]` has a set of transitions that fire *regardless of the
current state*, and baud had none of them:

> On the VT220, VT420 and VT500, the C0 controls CAN and SUB cancel any escape
> sequence, control sequence or control string in progress and return to ground
> state.

`1B` ESC does the same, starting a fresh sequence rather than ending in ground.

In `baud-core2`, four arms each wrote `if b.is_c0() { self.execute(b) }` — and
`is_c0()` is `b < 0x20`, which **includes all three**. So an ESC arriving
mid-sequence was executed and the parser stayed in the parameter string:
`CSI 1 ESC [ 2 m` lost its escape, dispatched a nonsense `CSI 1 [`, and printed
`2m` as text. That core handled them once, ahead of the state match.

**`baud-core` took half of that fix, and this table said `done` for both
halves until 2026-09-27.** Its graph's `one()` takes `CAN`/`SUB` ahead of the
node functions; `ESC` it left to them, and `escape()` and the four `csi_*`
functions sent it to `c0()`, which consumed it and stayed in the sequence --
the same `CSI 1 [` and `2m`, and `ESC ( ESC 0` designating DEC graphics. The
row's test was the mnemonic `ESC`, which every suite mentions, so nothing could
see it. Now `one()` takes `1B` too, in `Escape` and the four `CSI` nodes, as
`Act::EscStart`, which clears the half-parsed sequence:
`an_esc_abandons_every_sequence_and_keeps_its_meaning_elsewhere` gates the edge
from every node, and `baud-core/tests/esc_abandons.rs` the behaviour, one test
a node, at read sizes 1, 3, 7 and whole.

The states that decline are the ones that must. Inside a control string ESC is
the first half of ST, so the payload has to survive until the next byte decides
between *terminator* and *cancel*; those nodes implement that themselves. A
character cut mid-sequence becomes a replacement and re-dispatches the ESC from
ground. And VT52 has no `CSI`: its escape restarts on a second ESC, and a
direct address takes one as a coordinate, as xterm's does.

| status | |
|---|---|
| `18` CAN, `1A` SUB — cancel from any state, control strings included | done |
| `1B` ESC — abandon and begin a new sequence | done — since 2026-09-27; this row read `done` before and was not, see above |
| `80-9F` C1 under `ESC % @`, inside a control string — cancel it, then execute, or enter `dcs_entry`, `csi_entry`, `osc_string` or `sos_pm_apc_string`; `9C` ends it as ST | done — since 2026-10-09, from the `dcs_*`, `osc_string` and `sos_pm_apc_string` states; see below. Every C1 byte from every string node acts as its 7-bit `ESC Fe` form at every read split (`a_c1_in_any_control_string_acts_as_its_seven_bit_form_under_latin1`), an `OSC` title a C1 cancels is not set (`a_c1_inside_an_osc_acts_under_latin1_and_is_not_stored`), the table holds it exhaustively (`a_latin1_c1_leaves_every_control_string_and_a_utf8_one_stays`), and under UTF-8 the same bytes stay payload (`under_utf8_c1_bytes_in_a_string_stay_in_the_payload`) |
| `80-9F` C1 under `ESC % @`, inside an escape or control sequence — cancel it, then execute, or enter `dcs_entry`, `csi_entry`, `osc_string` or `sos_pm_apc_string`; `9C` goes to ground | done — since 2026-10-09, later the same day, from `escape`, `escape_intermediate` and the four `csi_*` states, and so from an `ESC` inside a control string, which hands its next byte to `escape`; see below. Every C1 byte from each of sixteen such states acts as its 7-bit `ESC Fe` twin at every read size and every split into two reads, and the twin as the control alone from ground (`a_c1_in_any_escape_or_control_sequence_acts_as_its_seven_bit_form_under_latin1`); `ESC 85` is a NEL, `ESC ( 84` an IND that designates nothing, and a `CSI` a C1 interrupts does not dispatch (`a_c1_after_esc_or_inside_a_csi_acts_under_latin1`); the table holds it exhaustively (`a_latin1_c1_cancels_every_escape_and_control_sequence_and_a_utf8_one_does_not`); and under UTF-8 the same bytes are what they were (`under_utf8_a_c1_byte_in_a_sequence_is_what_it_was`) |
| C0 in `csi_entry` — execute and **stay**, without consuming the marker position | done — `CSI <BS> ? J` is still DECSED |
| C0 in `csi_intermediate` | done — this arm had no C0 branch, so `CSI ! <NUL> p` lost its DECSTR |
| `7F` DEL in `csi_param` | done — ignored, and the sequence goes on, as `[[source: williams]]`'s `csi param` ignores it (315): `CSI 5 DEL C` moves the cursor as `CSI 5 C` does, under both encodings and at every read size (`a_del_in_csi_param_is_ignored_and_the_sequence_goes_on`) |
| `7F` DEL in `csi_entry` or `csi_intermediate` | **partial** — the `DEL` draws nothing, but the sequence is ignored with it: each node takes `7F` to `Consume` into `csi_ignore`, which consumes to the final and dispatches nothing, so `CSI DEL 5 C` moves nothing and `CSI ! DEL p` is no DECSTR, where williams' `csi entry` and `csi intermediate` ignore the `DEL` and stay (306, 328). Found 2026-10-10 and not built (the record, "A GR byte in a DCS prefix is its 7-bit byte under ISO 8859-1") |
| `7F` DEL in ground, ANSI and VT52 | done — ignored: no cell and no cursor move, as `[[source: dec-std-070]]` 070-3 §3.4.2 says for a 94-character set in GL ("shall be ignored"), with `[[source: vt510-ch7]]` § 7.2.5 and `[[source: williams]]`'s ground. Until 2026-10-08 it drew U+FFFD, a decoding error for a byte that is well-formed in UTF-8 (`del_in_ground_is_ignored`, at every read size). A 96-character set in GL makes it a character of the set, which waits on note AE |
| SUB additionally displaying the DEC error character `␦` | **out** — baud cancels and writes nothing. The basis this row gave, that xterm writes no glyph and baud follows it as for DECAWM, was wrong: xterm 411 writes one, under conditions read from its source (below). Whether SUB displays the error character is open with the owner: note AF's Q-UNDEF. **Which codepoint it is, is decided**: U+2426, `baud_domain::DEC_ERROR_CHARACTER`, note AE's AE8.1, Jev at 1.00 twice under the owner's 2026-10-10 rule and listed for their veto (`the_dec_error_character_is_symbol_for_substitute_form_two`); nothing draws it yet. Detail below |

**The C1 controls are the block's third rule, and baud takes it from every
ANSI state.** `[[source: williams]]` (lines 612-615): "All C1
controls cancel any escape sequence, control sequence or control string in
progress and are executed. Control functions special to this parser, i.e. DCS,
SOS, CSI, OSC, PM and APC, cause a transition to their appropriate states." In
UTF-8 these bytes are continuation bytes and the rule cannot apply (the
Encoding section below). Under `ESC % @` the string node ended only on `9C`
and stored the other thirty-one, so `85` landed in a window title and the NEL
never ran, and the DCS prefix sent all thirty-two, `9C` included, into the
body. `one()` now gives a C1 inside a string the edge it takes from ground,
and `9C` ends the string. It cancels as `ESC` does: williams uses the one verb
for all four and asks that "the 7-bit (ESC Fe) and 8-bit representations of
C1 controls should act in the same way" (534-537). A cancelled string is not
dispatched, by any of the four, and xterm's `sos_table` reaches `do_osc` only
from `CASE_ST` (`[[source: xterm-vtprstbl-c]]` 7781-7820,
`[[source: xterm-reset]]` 5288-5320); whether it should keep what it has done,
as williams' `unhook` and its "does not undo these effects" suggest, is open
with the owner (`docs/record.md`).

The rule's other half came later the same day. In `escape` a C1 was dropped
and the escape ended without it, so `ESC 85` lost its NEL, and in the `csi_*`
states it was dropped and, from `csi_param`, the sequence carried on, so
`CSI 2 ; 85 H` moved the cursor. williams says it again of `csi entry` and
the states after it (534-537): "C1 controls will cancel the sequence and then
be executed." `one()` now gives a C1 in `Escape` and the four `CSI` nodes the
edge it takes from ground, as the `1B` row above cancels from the same five;
`9C` there takes its ground edge, the `ESC \` it stands for, since there is
no string to end and the diagram draws it to ground with no action (line
268). xterm agrees in every ANSI table: `esc_table`, `csi_table`, `scstable`,
`cigtable` and the rest take `84` to `CASE_IND` and `85` to `CASE_NEL` (for
`esc_table`, `[[source: xterm-vtprstbl-c]]` 6157-6196). So every ANSI state a
Latin-1 C1 can arrive in acts on it: ground, the strings, the sequences, and
an `ESC` inside a string, which hands the byte on to `escape`. The UTF-8
continuation states cannot be reached under Latin-1, where every character
is one byte. VT52 is not williams' parser and is not taken here: a Latin-1 C1
in VT52 ground still takes its ANSI edge, note AE's D3, which waits on DEC's
text for that mode (xterm's `vt52_table` ignores C1).

**A bare ST is no function, and is counted nowhere.** `[[source: ecma48]]`
§ 8.3.143 defines ST only as "the closing delimiter of a control string", so
a `9C` that arrives with no string open, from ground or from a sequence it
cancels, closes nothing, and neither does an `ESC \` there. williams draws
`9C / (no action)` (line 268) and says "there is never any work to do for
9C" (972-973). baud does nothing and counts nothing for either; both fell to
`Esc::Unsupported` and were counted as a function baud lacks until 2026-10-09,
after the sequence half above (`a_bare_st_closes_nothing_and_is_counted_nowhere`).
An ST that closes a string is the string node's, and is unchanged. A `9C` under
`ESC % @` in VT52 ground takes its ANSI edge (note AE's D3) and so is uncounted
too; VT52 has no control strings for it to close.

**The basis this row gave was wrong, and what SUB displays is open.**

`[[source: williams]]` records that "SUB will also display the error character, a
reversed question mark", describing VT220 through VT500 hardware, and
`[[source: dec-pplv2-pm]]` p. 6-4 says it of DEC's printers: "SUB in printable
text causes the error character (a reverse question mark) to be printed."
`[[source: ctlseqs]]` documents `SUB` nowhere as producing a glyph. baud cancels
and writes nothing.

This row said xterm "does not write one", and that baud follows xterm here for
the reason it does on DECAWM's default. xterm 411 does write one, under
conditions, read from its source and not run (note AF §5(a)6, D5). A ground SUB
reaches CASE_SUB through the ground table (`[[source: xterm-vtprstbl-c]]`
`VTPrsTbl.c:188`; `[[source: xterm-reset]]` `charproc.c:3646-3678`), and what
`dotext` goes on to write, after the translation, is:

- **nothing at decTerminalID 52, in any build**: CASE_SUB writes only when
  `terminal_id >= 100` (`:3665`);
- **code 2 at the other IDs, in a build without `OPT_WIDE_CHARS`** (`:3670`),
  below 32, so `xtermCharSetOut` passes it unchanged
  (`[[source: xterm-charsets-c]]` `charsets.c:395-396`);
- in a build with `OPT_WIDE_CHARS`, where CASE_SUB picks 0x2426 when
  `terminal_id > 200` and 0x2592 otherwise (`:3667-3668`):
  - **U+2426 at decTerminalID 420, in a UTF-8 locale, with ASCII in GL and
    GR.** 420 is xterm's default (`[[source: xterm-ptyx-h]]` `ptyx.h:398-400`;
    `charproc.c:516`), a UTF-8 locale selects UTF-8 mode at xterm's defaults,
    and `resetCharsets` then puts ASCII in both, so no translation runs. With
    another set in GL or GR the translation runs and still leaves it: in UTF-8
    mode `xtermCharSetOut` skips any character above 255
    (`charsets.c:384-393`);
  - **`&` outside UTF-8 mode, with ASCII in GL.** At ID 420 `resetCharsets`
    puts ISO Latin-1 Supplemental in GR, so the character goes through
    `xtermCharSetOut`, which takes its low eight bits, 0x26
    (`charsets.c:366-370`), and leaves that byte with ASCII in GL;
  - **U+2592 at the VT100-level IDs** xterm keeps (100, 101, 102, 131 and
    132; `charproc.c:11020-11033`).

The pinned files do not say whether a given build has `OPT_WIDE_CHARS`:
`xterm.h` includes `xtermcfg.h`, which is not pinned, when `HAVE_CONFIG_H` is
defined, and then defines it as 0 if nothing has set it
(`[[source: xterm-xterm-h]]` `xterm.h:43-45`, `:298-300`). The write path past
`dotext`, and xterm's drawing, were not read.

So "baud follows xterm" was wrong as written: under those conditions xterm
writes a glyph, and following xterm, the DECAWM precedent, would not give
"writes nothing". The status stays **out** because changing it is the decision, and the
decision is the owner's: whether SUB displays the error character is note AF's
Q-UNDEF, decided with what every DEC set's reserved positions draw. Which
codepoint the error character is no longer waits: U+2426 SYMBOL FOR SUBSTITUTE
FORM TWO, note AE's AE8.1, picked by Jev at 1.00 on an asking and on a
re-asking with the options reordered, which the owner's 2026-10-10 rule builds
and lists for their veto. It is `baud_domain::DEC_ERROR_CHARACTER`, emitted
beside the national tables by `scripts/gen-nrcs-tables.py`, and
`the_dec_error_character_is_symbol_for_substitute_form_two` holds it to its
name in `[[source: unicode-data]]`. Nothing draws it until SUB's display, A0
with a 94-set in GR (note AE's AE8) or Q-UNDEF is answered.

The reason given before that one was that this was "a rendering decision made
where no renderer exists". That was wrong too, and its correction stands: baud
stores codepoints, not pixels, so writing a glyph for SUB would be storing a
*character*, which it does all day. Nothing about the renderer prevents it.

The asymmetry this section called settling -- a glyph an application did not
send is indistinguishable afterwards from text it did, while writing none loses
nothing it can observe, because the cancellation itself is the visible
behaviour and is implemented -- is an argument for that decision, not a basis
the row can rest on.

### Encoding: UTF-8, or ISO 8859-1, and 8-bit C1 controls only under the second

`[[source: williams]]`'s parser has 8-bit C1 controls throughout — `9B` enters
`csi_entry`, `9D` `osc_string`, `90` `dcs_entry`, `9C` is ST. baud has them
only under `ESC % @` (ISO 8859-1), and a reader comparing against the state
diagram will notice. In UTF-8 the absence is forced rather than chosen:

> a C1 control can be mistaken for badly-formed UTF-8 when the terminal runs in
> UTF-8 mode because C1 controls are valid continuation bytes of a UTF-8 encoded
> (multibyte) value […] it is not possible to use a C1 control obtained from
> decoding the UTF-8 text, because that would require reprocessing the data.
> — `[[source: ctlseqs]]`

`80-9F` cannot be both a control and a continuation byte. A terminal picks one,
and picking UTF-8 makes the choice for every byte at once. So:

| | status |
|---|---|
| UTF-8 decoding, including the `Utf8` parser state williams does not have | done |
| bytes `A0-FF` under UTF-8 are UTF-8, never a right half, whatever G0-G3 hold | done — note AE's Q5(a), decided 2026-10-10 (AE5 part 1; Jev 0.92 on the asking and on a replicate with the options reordered). `[[source: unicode18-ch3]]` C4 has a process interpret a coded character sequence "according to the character semantics established by this standard", D92 gives every UTF-8 byte sequence its scalar value or makes it ill-formed, and `[[source: ecma35]]` 9.1 gives a 7-bit code no GR area. xterm's mapping, decoded U+0080-U+00FF through GR and raw bytes past the decoder whenever GR is not ASCII (`[[source: xterm-ptydata-c]]` ptydata.c:91-97), which shows `C3 A9` as `Ã©` after `ESC * 0`, is practice not followed. Every U+00A0-U+00FF and every raw `A0-FF` after `ESC ) * +` with `0` or `A`, and after `ESC ( 0` and `ESC ) 0 SO`, at every read size (`gr_bytes_under_utf8_are_utf8_whatever_is_designated`, a guard, red under either perturbation in the record). What LS1R-LS3R do under UTF-8 is Q5's other half, open |
| 7-bit `ESC Fe` forms `ESC P`, `ESC [`, `ESC \`, `ESC ]`, `ESC X`, `ESC ^`, `ESC _` | done — `[[source: ecma48]]` § 5.3 specifies these as the representation of C1 in a 7-bit environment, so this *is* the C1 interface, not a substitute for one. `ESC \` is ST, which § 8.3.143 defines only as the closing delimiter of a control string; with none open it closes nothing and is counted nowhere, as `9C` is under `ESC % @` (`a_bare_st_closes_nothing_and_is_counted_nowhere`). Until 2026-10-09 both were counted as unsupported |
| 8-bit C1 introducers `90`, `9B`, `9C`, `9D`, `98`, `9E`, `9F` — **in UTF-8** | **out** — they are continuation bytes there; a stray one is a decode error and renders U+FFFD. Follows ghostty; diverges from alacritty and wezterm |
| the same introducers under `ESC % @` (ISO 8859-1) | done — no multi-byte sequence exists for them to be part of, so `9B` is unambiguously CSI. From inside a control string too, which each cancels (`a_c1_in_any_control_string_acts_as_its_seven_bit_form_under_latin1`, and the anywhere block above); until 2026-10-09 the string stored them and only `9C` ended it. And from inside an escape or control sequence, which each cancels (`a_c1_in_any_escape_or_control_sequence_acts_as_its_seven_bit_form_under_latin1`); until the same day `escape` dropped them and the `csi_*` states consumed them |
| 8-bit `84` IND, `85` NEL, `88` HTS, `8D` RI under `ESC % @` | done — each acts as its 7-bit form, `[[source: ctlseqs]]` pairing the two, at every read split (`eight_bit_ind_nel_hts_and_ri_match_their_seven_bit_forms`), and none inherits the last escape's intermediate (`a_c1_control_does_not_inherit_the_last_escapes_intermediate`). Until 2026-10-08 all four were counted as unsupported, and `84` after `ESC ( 0` replaced G0. `8E` and `8F` map to `ESC N` and `ESC O`, counted as those are until note AE's single shifts are built |
| character width from East Asian Width + general category, generated from pinned Unicode data | done — `AmbiguousWidth` is embedder policy, `[[source: uax11]]` § 5 |
| UAX #29 extended grapheme cluster segmentation | done — `ClusterBreak`, and all **766** cases of Unicode's own conformance suite pass |
| combining marks stored against their base cell | done — `Term::marks`, in `ClusterStore`: bounded at 63 clusters and **refusing** rather than evicting, a refused cluster keeping its base and counted in `marks_dropped` (the combining-marks prose below). They used to be **discarded**. This row said "evicting", `baud-core2`'s table, until 2026-10-10 |
| one cluster per cell — ZWJ sequences, flags and Indic conjuncts as a single cluster | done — `ClusterBreak` consulted in `write.rs`'s `glyph`; continuations attach to the base cell, after printable ASCII too. Until 2026-10-10 a continuation that is not zero-width took a cell of its own there, a SpacingMark (GB9a) or an Extend with a width such as `U+09BE` or `U+1F3FF` (GB9), because `Paint::piece` and `Paint::blit` told the machine nothing came before the run; they tell it `ClusterBreak::after_ascii_run` now. And a control ends the cluster before it (GB4, GB5) **for a continuation that takes a column**: every edge that performs a C0 control but `CR`, `NUL` and `DEL` included, or ends an escape sequence, a control sequence (an SGR and a malformed one that `csi ignore` swallows included) or a control string, leads to `Node::AfterControl`, whose row tells the machine (`Act::EndCluster`) before the next character it is asked about; `CR` leaves the cursor at column 0, where `glyph` restarts the machine. Before, such a continuation after a line feed, a tab, a cursor move or an erase was dropped on an empty cell or joined a letter the cursor had stepped back over (`a_control_function_ends_the_cluster_before_it`, `every_way_out_of_a_control_leads_to_the_ground_after_a_control`). A `U+FFFD` the decoder writes for an ill-formed sequence tells the machine it is class Other, so GB9a joins a SpacingMark to it and GB11 and GB12/13 do not reach across it, after a control too (`a_replacement_character_is_class_other_to_the_cluster_after_it`). `every_line_of_the_suite_clusters_through_the_terminal` feeds every line of `[[source: gbtest]]` through `Term` at every read size, its 211 with NUL, LF and CR as the terminal receives them, and pins what still departs, four open departures: **a zero-width codepoint after a control** still joins the cell before the cursor or is dropped where that cell is empty, where GB4/GB5 give it a cluster of its own, a defective combining character sequence (D57) that TUS § 5.13 would render on a no-break space (note AB, Q24, for Jev); **REP's copy** never joins what came before it, `CSI b` being a control, where the text it expands to would (note AB, Q24); a Prepend joins the cell before it and printable ASCII after it takes its own (GB9b; note AB §7.1); and a cluster the store refuses loses its tail (note AB §7.2). Whether `NUL` and `DEL` end the cluster at all is for Jev too (note AB, Q24) |
| `Emoji_Presentation=Yes` is two columns | done — regional indicators are EAW `N`, so a flag measured from East Asian Width alone is half-width |
| DECRQM `?2027` — grapheme clustering | done — answers `3`, permanently set, and it is true: a `U+FE0F` after a base `[[source: emoji-variation-sequences]]` lists widens it to two cells (keycap bases excepted); configured to one cell it answers `4`; no sequence changes it |
| `ESC % G` — select UTF-8, ISO 2022 | done — switches the live input encoding back to UTF-8 after `ESC % @`, and changes nothing while UTF-8 is live. This row said "satisfied; baud already decodes UTF-8", from before `ESC % @` was built, until 2026-10-10 |
| `ESC % @` — select ISO 8859-1 | done — switches the live input encoding |
| `CSI ? 1020 $ p` — DECRQM on xterm's `utf8` resource | done — mirrors the **live** encoding, `1` or `2`, as xterm does |
| `ESC SP F` — S7C1T, send 7-bit controls | done — the default |
| `ESC SP G` — S8C1T, send 8-bit controls | done — replies switch to `9B` / `90` / `9C`, and keys to `9B` / `8F` (`a_key_follows_s8c1t_as_a_reply_does`) |
| GR bytes `A0-FF` treated as GL `20-7F` inside a sequence — **in UTF-8** | **out** — they are UTF-8 lead and continuation bytes. This row said `out` for both encodings until 2026-10-10, on a reason that holds under UTF-8 alone |
| the same under `ESC % @` (ISO 8859-1) | **partial** — in a DCS prefix since 2026-10-10, `dcs_entry`, `dcs_param` and `dcs_intermediate` reading each GR byte as its 7-bit byte (`dcs_entry` above). **The gap**: an escape or control sequence does not. `escape` ends at a GR byte and drops it, so `ESC A8 B` designates nothing and draws `B`; `csi_entry` and `csi_intermediate` ignore the sequence and `csi_param` drops the byte, so `CSI 1 B2 m` is `CSI 1 m`; and `csi_ignore` waits for a 7-bit final. `[[source: williams]]` treats GR as GL "in all states" (462) and in DEC's notes for both (760, 847, 900); `[[source: ecma48]]` §9 names "Parameter Bytes, Intermediate Bytes, and Final Bytes of a control sequence"; `[[source: dec-std-070]]` 3.5.4.5 calls its DCS rule "the same way 8-bit graphic characters are handled within control sequences"; and xterm's `csi_table` gives `A0-FF` the cases of `20-7F` (`[[source: xterm-vtprstbl-c]]` 679-798). The texts agree and it is unbuilt: `CsiFinal`, `Inter`, `Marker` and `EscFinal` read their byte, so it changes actions, not only table content. **And in a control string's body**: an `OSC` keeps a GR byte as it is, a title's among them, where `[[source: dec-std-070]]` 3.5.4.5 says "GR (8-bit) graphic characters in APC, OSC, and PM control strings will be treated as their 7-bit equivalent (the eighth bit will be ignored)" and ECMA-48 §9 gives "the contents of the command string" the same rule; an `APC` or `PM` body no handler reads, and a DCS body's GR bytes are the open call `dcs_passthrough` names. |
| ISO 8859-1 input, switched by `ESC % @` / `ESC % G` | done — `InputEncoding`; one byte, one character, no decoder state |
| encoding detection | done — `InputEncoding::detect` **reports**; it never switches the terminal, and returns `None` when the sample cannot decide |
| Translating the national replacement sets | done — 14 sets, 164 substitutions: 163 from `[[source: xterm-charsets]]`, cited as xterm's, and Turkish 2/1, U+0131, typed from `[[source: dec-pplv2-pm]]` Figure A-26, which xterm's table lacks. Greek 7/9 and 7/10 wait on note AF's Q-UNDEF. The ten VT220 sets are held to DEC's own tables (`each_vt220_national_table_is_decs`). A cell keeps the codepoint its set translated the byte to when it was written, so designating another set changes nothing already on the screen; this said "the run records which set it was written under so promotion cannot change it", a mechanism that was `baud-core2`'s. The sets act whether or not national mode is set, a departure recorded under Out of scope |

`S7C1T`/`S8C1T` are about what the terminal **sends**, and both are implemented:
"this tells the terminal to send C1 control characters as 8-bit sequences, e.g.,
its responses to queries".

**Sending and accepting are different axes, and conflating them is a mistake
this document made first.** S8C1T was refused here on the grounds that emitting
`9B` would make baud's own output invalid UTF-8. That reasoning does not hold: a
reply is not UTF-8 text, it is a control stream the application asked for by
sending S8C1T, and an application that asks is prepared to read it. baud still
cannot *parse* an 8-bit introducer in UTF-8 — that half is forced by UTF-8 and
stands — and under `ESC % @` it parses all of them (the rows above); there was
never a reason it could not *emit* one. `C1Transmit` carries the
choice and CSI, DCS and ST introducers all follow it, a key's `CSI` and `SS3`
included: xterm writes both through the `unparseseq` its replies use
(`[[source: xterm-input]]`, `[[source: xterm-reset]]` charproc.c 9356-9373).

**`?1020` mirrors the live encoding, and that is a correction.** It answered
`3` — permanently set — for as long as `ESC % @` was refused, which was true
then. Implementing the switch made it false, so it moved in the same commit.
A mode report that outlives the fact it reported is precisely the failure this
document refuses elsewhere, and it is easiest to create by fixing the thing the
report describes and leaving the report behind. xterm answers `MdBool` from the
live `screen->utf8_mode` for the same reason.

dtterm's `is2` opens with `ESC SP F`, which is how the conformance fixtures
caught the first version of this: counting S7C1T as unsupported claimed baud had
refused something it already did.

> **~~Open decision, under active research; the survey will land in
> `docs/research/notes/`.~~** It landed: `notes/g-text-encoding.md`, which this
> file already cites below. Its § 3 table surveys seven terminals, and the
> `ESC % G` / `ESC % @` column is the answer — **xterm supports the selectors,
> gated on `wide_chars`; kitty, ghostty, alacritty, wezterm, foot and Windows
> Terminal have no entry at all.** kitty says why in the same table: "there is
> no actual advantage… now that UTF-8 encoding is universal".
>
> So baud, honouring both selectors, matches xterm and exceeds every other
> terminal surveyed. **No further encoding is indicated by the evidence**, and
> adding one would put baud alone in a dimension no application uses: a program
> emitting Shift-JIS converts to UTF-8 itself, because every terminal it might
> run in requires that.
>
> `Encoding` in `baud-domain/src/store.rs` remains the extension point, and its
> `is_byte_per_column` predicate is still written around "fixed-width
> single-byte encoding" rather than around ASCII — so if a user ever does ask,
> the shape is an embedder-supplied 256-entry table, which covers every ISO
> 8859 part, the Windows code pages and KOI8 with one mechanism. That is a
> different thing from a protocol feature and would not be selected by an
> escape sequence, because none exists to select it.

**~~Grapheme clustering is out, and it is the largest thing on this page that
is.~~** It is done, and this paragraph contradicted the two below it for as long
as they have existed — the opening said `U+1F469 U+200D U+1F4BB` gets **four**
columns and the closing said two. The closing is right.

`[[source: uax29]]` defines the extended grapheme cluster, the unit a *user*
would call a character. `Grapheme::is_break` implements it against the generated
break-property tables, including `InCB` for Indic conjuncts and
`Extended_Pictographic` for emoji sequences, and `tests/encoding/graphemes.rs`
runs every case of `GraphemeBreakTest.txt`.

The stale half is left visible rather than deleted because of what it was:
a *correct description of the old behaviour* that outlived it. Nothing flagged
it — `no_row_is_unfinished` reads table rows, not prose, so a paragraph can
claim the opposite of the paragraph after it indefinitely.

**Combining marks used to be discarded outright.** `baud-core2`'s `put_cp`
returned early on `Width::ZERO`, so `e` + `U+0301` stored `e` and the accent was
gone — not mis-measured, *absent*, and unrecoverable by a renderer, a selection
or a copy. A terminal must not lose a character it was sent. They are stored
now, in `ClusterStore`: **keyed on the cluster's own content**, with its index
riding in the cell's `CellFlags`, so it moves with the cell through every
rotation, reflow and screen switch. ~~Keyed on the cell they attach to, in a
table that evicts on collision~~ was `baud-core2`'s design, and one `viewport()`
call made it lose a cell's marks (CLAUDE.md, "The cluster store is
content-keyed, and the index rides in the cell"). It is bounded at 63 clusters
and **refuses** rather than evicts, because a wrong cluster puts somebody else's
accent on your character; repeats share a slot. `Term::cluster_marks` is what a
renderer reads, and `marks_dropped` counts the refusals.

**And the same table answers full cluster semantics, which needed no cell bits
at all.** The reasoning that said otherwise was looking at the wrong constraint:
a continuation does not need the *cell* to identify its cluster, it needs
somewhere to live, and the marks table already is that. `write.rs`'s `glyph`
asks `ClusterBreak` whether a codepoint continues what came before it, and a
continuation attaches to the base cell instead of taking one of its own. So
`U+1F469 U+200D U+1F4BB` is two columns rather than four, and a flag is two
rather than four.

One thing that fell out of it: **regional indicators are East Asian Width `N`**
— one column — while carrying `Emoji_Presentation=Yes`. A flag measured from
EAW alone is half as wide as every terminal draws it, and the fix is a property
lookup rather than a special case. This is the question the deleted
`EmojiWidth` config was gesturing at, answerable now only because the cluster's
base is known.

**The segmentation half is done and measured, and so is the storage.**
`ClusterBreak` implements the boundary rules and passes all 766 cases of
`GraphemeBreakTest.txt`, which is Unicode's own suite written by the people who
wrote the rules. So the question is no longer "can baud find cluster
boundaries" but "what would it cost to act on them", and half of that now has a
number:

| corpus | width only | + UAX #29 breaks | cost |
|---|---|---|---|
| ASCII | 1119.6 Mcp/s | 338.3 | −70% |
| Latin + combining | 509.4 | 128.0 | −75% |
| CJK | 168.8 | 56.6 | −66% |
| emoji ZWJ, flags, Indic conjuncts | 156.0 | 50.1 | −68% |

Read the ASCII row with care: it measures a path ingest never takes. Printable
ASCII is blitted by the run scanner and never becomes a `Codepoint` at all, so
segmentation would cost nothing on it. The number that matters is CJK — **169 to
57 Mcp/s, about 3x on the non-ASCII decode path**, which at three bytes per
codepoint is roughly 500 MB/s down to 170.

That is a real cost and not a disqualifying one. **The table priced
`baud-domain`'s boundary machine alone**, in a bench deleted with `baud-core2`
and before `ClusterBreak` became a transition table, so it is kept as the
ordering it was measured for rather than as a live number. ~~What still has no number is
storage~~ -- it has one: `ClusterStore` is the side table this paragraph
described, its index in the six `CellFlags` bits that were spare, and it costs
**about 9%** on cat-cjk and less-cjk and **about 2.5%** on vim-unicode, the
three recordings that carry clusters, and nothing measurable on the rest
(CLAUDE.md, "It costs 9%, on the two recordings it serves"). It is no longer a blocker; the bound, 63 distinct
clusters freed only by `RIS`, is what it costs.

`docs/research/notes/g-text-encoding.md` § 4.4 sets out where the bytes would
come from and § 4.5 why there is no consensus to conform to: mode 2027 is a
draft, kitty's text-sizing protocol solves an overlapping problem differently,
and terminals disagree today.

DECRQM `?2027` therefore answers **`3`**, permanently set, which is what
`decrqm_reports_grapheme_clustering_as_permanent` pins. It answered `0` — not
recognized — for as long as clustering was unimplemented, and that was the
honest answer then: `2` would have told an application the mode existed and was
merely off, whereupon it would set it and believe it took effect. The row moved
when the implementation did, which is the only order that keeps the answer
true.

**Until 2026-10-01 the `3` was half false.** Mode 2027's text is about width
as well as clustering, and a text-default emoji followed by `U+FE0F` stayed
one cell. It is two cells now -- the owner's call, after research and Jev's
0.99, recorded in CLAUDE.md -- in place when the row has room and moved whole
to the next row at the right margin, as ghostty does. `vs16_width.rs` pins
it. The answer stays permanent rather than resettable because bubbletea v2
sends `?2027l` on exit, behind the shell.

**ISO 8859-1 is honoured; the rest of the legacy charsets are not.**
`[[source: ctlseqs]]` gives `ESC % @` and `ESC % G` as the two selectors, and
both work: `InputEncoding` decides how a byte above `0x7F` is read, and a
single-byte encoding needs no decoder state at all. This was refused here on
the grounds that baud "cannot leave UTF-8", which was wrong — the encodings
differ only in that one rule.

**Detection reports; it does not switch.** `InputEncoding::detect` takes a
sample and answers one of three things, and the third is the interesting one:

* a UTF-8 BOM at offset zero is a signature, and decisive;
* anything ill-formed as UTF-8 rules UTF-8 out — ISO 8859-1 has no ill-formed
  bytes, all 256 are characters, so it is what *remains* rather than a guess;
* a sample of nothing but ASCII returns **`None`**, because the two encodings
  agree on every byte below `0x80`.

That third case is why detection cannot be a default. Most of what a terminal
sees is ASCII and decides nothing, and a rule that answered anyway would be
inventing evidence. Nothing in baud calls `detect` — an embedder does, and
decides — because guessing an encoding wrong corrupts text silently and gives
the application no way to find out, which is the failure this document refuses
everywhere else. xterm makes it a resource for the same reason.

A sequence truncated by the end of the sample is not evidence either: the next
read may complete it, and treating it as ill-formed would make the answer depend
on where the read boundary fell.

`80..9F` are never printed in Latin-1: ISO 8859-1 reserves them for C1
controls, and printing them would invent characters the application did not
send. Each is read as its 7-bit `ESC` form, so the introducers and IND, NEL,
HTS and RI act, and the rest are taken as their 7-bit forms are: counted,
but for `9C`, which with no string open is an ST that closes nothing and,
like `ESC \` there, is counted nowhere (since 2026-10-09). This said
they "stay ignored" because baud did not act on them, citing xterm's
`CASE_IGNORE`; they were in fact counted, IND, NEL, HTS and RI included.
Inside a control string each cancels the string and then acts the same way,
as `ESC` does; until 2026-10-09 the string kept all but `9C` as payload.
Inside an escape or control sequence each cancels the sequence and acts as
well, since later that day; until then `escape` dropped it and the `csi_*`
states consumed it (the anywhere block).

**The rest of the legacy charsets remain out.** The one charset mechanism
baud implements, `SCS`, is a different thing from an input encoding: a
designated set replaces a few positions of the 94 and leaves the stream's
encoding alone. `ESC ( 0` replaces `0x60..0x7E` through
`[[source: foot-vt100-0]]`'s table and keeps `CellFlags::GRAPHICS` on the cell
(and should replace `0x5F` too; see the DEC Special Graphics row below), and
the national sets replace a handful each through `[[source: xterm-charsets]]`.
This paragraph said `SCS` "annotates rather than translates" and that the cell
keeps the byte; both stopped being true when the tables landed. Under ASCII,
`PackedCell::translate` returns before any table is read. Byte-level
transcoding of a whole encoding would put a table lookup on the per-byte ingest
path, against the rule that the working set stays in cache — and `iconv`
already does it, once, outside the hot loop.

### The DCS prefix had the same merge, and the same three bugs

`dcs_entry`, `dcs_param` and `dcs_intermediate` were one arm, exactly as the CSI
states were, and lost exactly the same three rules:

| rule | `[[source: williams]]` | was |
|---|---|---|
| `3C-3F` is a marker in `dcs_entry` and an error in `dcs_param` | "Occurrences of 3C to 3F in positions other than the first character of the parameter string cause the entire control sequence to be ignored" | accepted anywhere |
| `30-3F` after an intermediate is malformed | `dcs_intermediate` sends them to `dcs_ignore` | walked back into `dcs_param` and accepted the digit |
| C0 in the prefix is **ignored** | "C0 controls other than CAN, SUB and ESC are **not executed** while recognising the first part of a device control string" | sent to `dcs_ignore`, costing the sequence |

Note the third against the CSI rule directly above it: williams says "C0 controls
are executed immediately during the recognition of a control sequence", and *not*
executed during a device control string. Same byte, different state, opposite
action — which is the whole reason these are separate states and not one.

Unlike CSI there is no `[[source: ctlseqs]]` sub-parameter form to accommodate in
a DCS prefix, so `3A` followed williams unmodified and ended the sequence.
It has not since the Sixel parameters were routed to `csi`:
`Dcs` collects `3A` as a separator and the sequence goes on, so `ESC P 0:1 q`
is a Sixel. williams sends it to `dcs ignore` ("the character 3A appears
anywhere", 589), and so does `[[source: dec-std-070]]`, whose 3.5.4.1 makes
the parameter string "identical in syntax to the parameter string of a
control sequence" and whose 3.5.3.1 ignores a control sequence at a 3/10;
`[[source: ecma48]]` 5.4.2 b alone lets 03/10 separate within a control
sequence's parameter sub-string, and gives a DCS no parameter string. xterm
counts `:` among a Sixel's parameter bytes (`[[source: xterm-reset]]`'s
file, 3459). Found 2026-10-10 and not built, and no test pins either
reading: the `dcs_param` row is `**partial**`, the choice for Jev.

### The parameter limit is 35, and it is derived rather than chosen

A CSI may carry more parameters than a fixed array holds, and discarding the
excess is a legitimate limit -- every terminal has one, and xterm's `NPARAM` is
30. What is not legitimate is the two feeds discarding it *differently*.

They did, in `baud-core2`. Its fast path accumulated a parameter into a local
and stored it only under the capacity bound, so an over-capacity parameter was
parsed and dropped. Its state machine wrote each digit straight into
`params[nparam - 1]` with `nparam` pinned *at* capacity, so the separator was
swallowed and the next parameter's digits appended to the last stored one:
`CSI ...;61;62;63 m` stored 6263 where the fast path stored 62. The result
therefore depended on where a read boundary fell, which is a chunk-invariance
bug. core2's ingest property did not find it because its generator did not
emit sequences that long.

| | one read | byte at a time |
|---|---|---|
| `CSI 0;0;...;0;1;4 m` | `BOLD` | nothing |
| `CSI 1;2;...;63;64 m` | `0xffff` | `0x7fff` |

core2's repair was `ParamIdx::next_capped`, which saturates one *past*
`CAPACITY` so that "capacity reached" and "capacity exceeded" stay
distinguishable; it is still in `baud-domain` and nothing calls it. `baud-core`
has one parameter store for both paths, `Csi` in `baud-core/src/write.rs`, with
one slot past the capacity: its `sep` stops advancing there, so every later
parameter folds into a slot nothing reads and is dropped, and its `count`
clamps to the capacity on the way out. See `tests/sgr/param_overflow.rs`. This
paragraph named core2's mechanism as the current one until 2026-10-10.

The capacity itself is `1 + Attrs::WIRE.len() + 1 + 3 * 5` = **35**, which is
the longest parameter list baud emits: the DECRQSS SGR reply. Since DECRQSS
answers with something the application can send back verbatim, a reply the
parser cannot re-read is a reply that does not round-trip -- and at 16 it could
not. Bold with a foreground, a background and an underline colour already
answers with seventeen parameters, which is an editor drawing an undercurl, not
a corner case. Deriving the constant rather than writing a number is what stops
it going back out of step the next time `WIRE` grows, which is how it drifted in
the first place.

### C0 inside an OSC payload is ignored, not stored

`osc_string` takes the *ignore* action on `01-17,19,1C-1F`; only `20-7F` is
`osc_put`. `dcs_passthrough` is not the same: `[[source: williams]]` gives
`00-17,19,1C-1F,20-7E` its *put* action and `7F` *ignore* (388), and baud's DCS
body follows it for `BEL`, the format effectors `08-0D` and `7F`, and the
`osc_string` row for the other C0, where the texts conflict, as its row names.
baud appended every byte that was not a terminator, so a raw control byte
landed in the title — and OSC payloads are attacker-supplied, which is why this
one is in the threat model rather than only in the conformance table.

**This block broke chunk-invariance the moment it landed, and the property
caught it.** `baud-core2`'s fast path scanned an OSC straight to its
terminator, so it could not see a cancel in between while the state machine
could — the same bytes set a title or did not, depending on whether they fit
inside one `read(2)`. Its repair declined any OSC payload containing a C0
byte at all, since a path that scans to the terminator cannot see a cancel
and one that copies the payload wholesale keeps bytes that should be dropped.
`baud-core`'s `fast_esc` takes no control string at all: it folds `ESC [`,
an intermediate and a final, and a bare two-byte `ESC`, and leaves `P`, `X`,
`]`, `^` and `_` to the graph, which walks every string byte by byte (this
said the fast path scans an OSC until 2026-10-10).
`a_c0_in_an_osc_payload_is_ignored` pins it deterministically
rather than leaving it to a saved seed.

---

## Tier 1 — VT100/ANSI core

The set every terminal must implement. No application can be assumed to work
without these.

### C0 controls

| byte | name | status |
|---|---|---|
| `0x00` NUL | ignored | done |
| `0x07` BEL | bell | done — counted; no UI to ring. **Inside a control string it rings nothing**: it ends an `OSC` (`[[source: ctlseqs]]` 2023-2024) and no other string, as `[[source: ecma48]]` 5.6 closes each with ST; it is payload in a DCS body and ignored in the DCS prefix and in an SOS, PM or APC, `[[source: williams]]`'s rows (354-426). Until 2026-10-09 it ended all five; see `dcs_passthrough` and `sos_pm_apc_string` above. xterm rings it in every string but an OSC (`CASE_BELL`, `[[source: xterm-reset]]`'s file 3691-3705), a departure from williams' *put* and *ignore* not followed (`a_bel_ends_an_osc_and_no_other_control_string`); in a Sixel `[[source: dec-std-070]]` 070-9 §9.1 rings it too ("same action as in text mode"), which is open under `dcs_passthrough` |
| `0x08` BS | backspace | done |
| `0x09` HT | tab | done — real stop table |
| `0x0A` LF | line feed | done — index only unless LNM |
| `0x0B` VT | vertical tab | done — index |
| `0x0C` FF | form feed | done — index |
| `0x0D` CR | carriage return | done |
| `0x0E` SO | shift out (G1) | done |
| `0x0F` SI | shift in (G0) | done |

### ESC sequences

| seq | name | status |
|---|---|---|
| `ESC 7` | DECSC save cursor | **partial** — `[[source: vt510-decsc]]` lists seven items and `baud-core` saves five of them whole: cursor position, SGR attributes, the wrap flag (DECAWM), origin mode, and the selective erase attribute. Of the character sets it saves G0-G3 and which slot GL invokes (`a_cursor_restore_brings_back_the_slots_and_the_shift`), and not GR, which this core does not model; **any SS2/SS3 in flight is not saved** because single shifts are not implemented. Both wait on owner questions in note AE (`docs/research/notes/ae-code-extension.md`), as does which wrap state is saved (its Q12). This row said the set was saved "as the one slot this core has" from f3e32d1 until 2026-10-08, stale since 6ecee7c. The row said `done` on the strength of the first two until the manual was read rather than assumed |
| `ESC 8` | DECRC restore cursor | **partial** — restores what DECSC saved, clamped to the grid that exists now. `a_cursor_restore_brings_back_the_wrap_flag_and_origin_mode` pins the two flags that were silently dropped |
| `ESC D` | IND index | done — column preserved (was resetting it); scrolls the region at its bottom and nothing on the last row below it, as DECSTBM's row says |
| `ESC E` | NEL next line | done |
| `ESC M` | RI reverse index | done |
| `ESC c` | RIS full reset | done — the power-on state from either screen, and it **empties the history**. xterm's `CASE_RIS` clears its saved lines where `DECSTR` keeps them (`[[source: xterm-reset]]`), and alacritty (`[[source: alacritty-reset-state]]`, `[[source: alacritty-grid-reset]]`) and foot (`[[source: foot-vt-ris]]`, `[[source: foot-term-reset]]`) do the same; the Linux console (`[[source: linux-vt-ris]]`) and sfte (`[[source: sfte-ris]]`) keep it. The whole primary ring is erased with the rotation kept, so a reset scrolls nothing and leaves nothing a reader of the ring or a later re-cut can bring back (`a_full_reset_clears_the_history_and_the_next_lines_refill_it`, `a_full_reset_leaves_nothing_hidden_below_the_window`, `a_full_reset_leaves_nothing_of_the_old_text_in_the_ring`, `a_full_reset_scrolls_nothing`, the captured `reset(1)` in `a_captured_reset_leaves_no_history`); it leaves the alternate screen before the soft reset, so what `?1049 h` saved does not survive it (`a_hard_reset_on_the_alternate_screen_leaves_the_power_on_state`), and frees the `OSC 8` table (`a_full_reset_frees_the_link_table`). `DECSTR` keeps the history (`a_soft_reset_keeps_the_history`). It left the counts and reset the rotation until 2026-09-23, so the scrollback read rows the reset never wrote. Since 2026-10-09 it moves `Term::history_erasures`, as `CSI 3 J` does, and the macOS app puts a scrolled-back window on the live screen however the read falls (`an_erased_history_puts_the_window_on_the_live_screen_however_the_read_falls`); the rest of what a reset owes the app is note V's V1 |
| `ESC H` | HTS set tab stop | done |
| `ESC =` / `ESC >` | DECKPAM/DECKPNM | done |
| `CSI ? 2 l` | DECANM — enter VT52 | done — see [protocols/vt52.md](protocols/vt52.md) |
| `ESC # 8` | DECALN screen alignment | done |
| `ESC ( ` / `ESC ) ` / `ESC * ` / `ESC + ` | charset designate G0-G3 | done — the mechanism: G0-G3 designated, G0 and G1 invoked into GL by SI and SO. What each set draws is in the rows below |
| `ESC ( 0` / `ESC F` (VT52) | DEC Special Graphics glyph table | **partial** — `0x60..0x7E` translate through `[[source: foot-vt100-0]]`'s 31 entries, and the cell keeps `CellFlags::GRAPHICS`. **`0x5F` stays `_`**, where every DEC text read replaces it with Blank: `[[source: vt100-ug-ch3]]` Table 3-9 ("ASCII codes 137 through 176", octal), `[[source: vt102-ug-ch5]]` Table 5-15, `[[source: dec-special-graphics]]` Table 2-4 and `[[source: dec-std-070]]` Appendix A, Table 2. None of the DEC texts read gives Blank a codepoint, so U+0020 or U+00A0 is the owner's call, open in docs/record.md, "DEC Special Graphics begins at 0x5F, and what it stores there is the owner's call". The gap is the same on the bulk run, the per-byte path and VT52 `ESC F` |
| `ESC ( A` | UK NRCS designation | done — reported as `Charset::Uk`, **not aliased to ASCII**, and `#` is `£` through `[[source: xterm-charsets]]` |
| `ESC ( C 5 H 7 K Q 9 R f Y Z 4 = ` E 6` | NRCS: Finnish, Swedish, German, French Canadian, French, Italian, Spanish, Dutch, Swiss, Norwegian/Danish | done — each named as its own `Nrcs`, and translated through `[[source: xterm-charsets]]`'s tables (`every_national_replacement_renders_as_its_table_says`). This row said "recorded, not translated" after the tables landed |
| `ESC ( < > I J` | DEC Supplemental, Technical, JIS-Katakana, JIS-Roman | done — each named as its own `DecSet` |
| `ESC ( % 6` `" >` `% 2` `% =` | NRCS Portuguese, Greek, Turkish, Hebrew — two-byte finals | done — the parser keeps two intermediates now |
| `ESC ( " 4` `" ?` `% 0` `% 5` `& 4` `% 3` `& 5` | DEC Hebrew, Greek, Turkish, Supplemental Graphics, Cyrillic, and the two whose mappings ctlseqs says are undocumented | done — designated and named; `% 3` and `& 5` share `DecSet::Undocumented` |

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
| `CSI J` | ED erase in display | done — 0, 1 and 2; `3`, the saved lines, is xterm's and has its row in Tier 3. `CSI 4 J` and above stay counted |
| `CSI K` | EL erase in line | done |
| `CSI L` | IL insert lines | done |
| `CSI M` | DL delete lines | done |
| `CSI P` | DCH delete chars | done |
| `CSI S` | SU scroll up | done — **and it now moves the grid**; see CLAUDE.md on the ring advance |
| `CSI T` | SD scroll down | done — **one parameter**; five is XTHIMOUSE |
| `CSI Ps ; Ps ; Ps ; Ps ; Ps T` | XTHIMOUSE initiate highlight tracking | done — the region is recorded; it used to be read as SD and **scrolled the screen** |
| `CSI X` | ECH erase chars | done |
| `CSI Z` | CBT backward tabulation | done |
| ``CSI ` `` | HPA char position absolute | done |
| `CSI a` | HPR char position relative | done |
| `CSI b` | REP repeat preceding char | done |
| `CSI c` | DA device attributes | done — identifies as VT102, and adds Sixel (attribute `4`) only when the embedder opts in (`Term::set_sixel_claim`, the app's `[images] advertise_sixel`, **off by default**) **and** has provisioned image memory. Off by default because an application that reads the claim stops drawing its text fallback (for yazi, chafa's grid of block elements, box-drawing strokes and coloured spaces), and the owner held the claim back until that fallback was proven to render right -- which `baud-core/tests/image_preview.rs` now does, cell for cell, on two real chafa captures. A claim is only read by an application that decides from the DA: yazi takes a terminal brand from `TERM_PROGRAM` first, which baud sets to `baud` so an outer terminal's name does not decide it; an unprovisioned baud refuses every Sixel, so it answers `?6c` even when opted in. `the_primary_da_advertises_sixel_only_when_opted_in_and_provisioned` pins all three |
| `CSI > c` | DA2 secondary device attributes | **out** — answered with silence and counted in `unsupported`. `[[source: ctlseqs]]` line 817 shapes the reply as `CSI > Pp ; Pv ; Pc c`, where `Pp` is a terminal *type* and `Pv` a firmware version. baud has neither honestly: it is not a VT220 and has no firmware, and an application parses whatever arrives as the report it asked for. That is the same refusal primary DA makes by under-claiming as a VT102, one step further. Recorded here because real capture proved it is asked for — vim sends it once in `tests/recordings/vim-unicode.raw`, and until that landed the table did not mention DA2 at all. `every_fixture_owes_the_same_reply` pins the silence *and* the count |
| `CSI > 0 q` | XTVERSION report the terminal's name and version | done — `Ps` 0, omitted included, answers `DCS > | Pt ST`, the one report baud sends that is not a CSI, as `[[source: ctlseqs]]` ("CSI > Ps q, Ps = 0 -> Report xterm name and version (XTVERSION)") shapes it; any other `Ps` is unanswered (`xtversion_answers_with_a_dcs`, `xtversion_defaults_to_zero`, `an_unknown_secondary_query_is_not_answered`, `xtversion_does_not_collide_with_other_q_finals`, `baud-core/tests/xterm/keypad_version.rs`). The table had no row for it until 2026-10-08 |
| `CSI d` | VPA line position absolute | done |
| `CSI e` | VPR line position relative | done |
| `CSI g` | TBC tab clear | done — 0 and 3; others inert, not aliased |
| `CSI h` | SM set mode | done — LNM and IRM |
| `CSI l` | RM reset mode | done — as SM |
| `CSI m` | SGR character attributes | done — see SGR below |
| `CSI n` | DSR device status report | done — 5 and 6; others unanswered, not guessed |
| `CSI ? 996 n` | colour preference query | done — since 2026-09-29, answered `CSI ? 997 ; 1 n` (dark) or `; 2 n` (light), `[[source: contour-palette-notifications]]`'s two replies and nothing else: the class of what `OSC 10`/`11 ; ?` would answer -- the override, else the embedder's default -- by WCAG relative luminance of the background against the text (note Y, decision 5; `baud_domain::ColorScheme`, its linearisation a table held to `f64` in `baud-domain/tests/color_scheme.rs`). Silent where `OSC 11` would be, on a core told no colours. Every other private `n`, DECXCPR's `?6n` included, stays unsupported and counted (`other_private_dsrs_stay_unsupported`). `the_query_answers_the_class_of_the_theme`, `the_query_follows_an_applications_override`, `tmux_starting_is_answered_in_order` (a tmux capture, `tests/scheme/`) |
| `CSI r` | DECSTBM scroll region | done — an `LF`, `IND` or wrap at a region's bottom scrolls the region whatever row that is, a region ending on the last row included (`lf_at_the_bottom_of_a_region_ending_on_the_last_row_scrolls_only_the_region`: it rotated the whole screen and retired the header rows above it), and on the last row *below* a region it neither scrolls nor moves (`lf_on_the_last_row_below_a_region_does_not_scroll`), both as `[[source: foot-linefeed]]`'s `term_linefeed` and alacritty do; `baud-difftest` carries both cases. On the primary a full-width region anchored at the top sends its scrolled line to the history whatever its height (`a_top_anchored_region_retires_its_line_whatever_its_height`), as `[[source: xterm-scroll]]`, kitty and wezterm do; a region in the middle, one narrower than the screen, and the alternate screen keep nothing |
| `CSI s` / `CSI u` | SCOSC / SCORC | done |
| `CSI ? h` / `CSI ? l` | DECSET / DECRST | done — every parameter of a multi-parameter form is applied |

Private modes, one row each. A mode is **done** only when something acts on it;
for the input-side and renderer-side ones "acting" means an accessor an embedder
can read, which is the only channel a `no_std` core with no keyboard and no
renderer has.

| mode | name | status |
|---|---|---|
| `?1` | DECCKM cursor keys | done — `Term::cursor_keys_application()` |
| `?2` | DECANM — **reset enters VT52** | done — see [protocols/vt52.md](protocols/vt52.md) |
| `?3` | DECCOLM 132 columns | done — clears, resets margins, homes; the *resize* is the caller's, via `Term::resize` |
| `?4` | DECSCLM smooth scroll | done — `Term::smooth_scroll()` |
| `?5` | DECSCNM reverse video | done — recorded by the core (`Term::reverse_video()`) and **drawn** since 2026-09-25, by xterm's `ReverseVideo` (util.c 3269-3345, `[[source: xterm-scroll]]`'s file): the default foreground and background swap, palette entries 0 and 7 and entries 8 and 15 swap, and the margin, scrollbar, cursor trail and screen fade follow on the frame that flips. Every background, the reversed default one included, keeps the window's alpha -- the owner's call. `baud-render/tests/reverse_video.rs`, and the pixel oracle's `?5` scene. **A `?5h` and `?5l` in one read are drawn too**, since 2026-09-25: on macOS `tput flash` sleeps its `$<100/>` and writes both in one burst (measured through a pty), so the mode reads as it did and no frame would show the flip. The core counts the changes -- the ones made where a frame could show them (`Term::reverse_flips`) apart from the ones made inside `?2026` (`Term::reverse_flips_held`), which a driver shows only once it stops honouring an update left open -- a frame that finds a count moved with the mode unchanged shows the other video, and the flash ends on the loop's own clock after the entry's padding: the `$<100/>` of the pinned `xterm-256color.ti`, parsed at build time by `baud_domain::terminfo` into a bounded `Delay` (`baud-core/tests/reverse_flips.rs`, `baud-app/src/bell.rs`, `baud-render/tests/video_flash.rs`, the pixel oracle's flash scene). Each driver times an update from its own opening (`UpdateClock`, restarted by `Term::updates_opened`). The drivers' wiring -- the plan, the commit and the refusal, the hold, the due check and the Linux `poll` timeout -- is established by reading, since no test drives the loop, and a flash was not watched on screen. vim's `set vb` and bash's `bell-style visible` write the two halves 102 ms and 110 ms apart, in two reads, and frames draw each -- by reading the frame loop; the app was not watched doing either. **Not built:** an `OSC 4`/`10`/`11` set, query or reset *while* reversed acts on the slot as the unreversed screen names it -- an `OSC 11` then moves the foreground drawn -- and what xterm does there is in its `misc.c`, which is not pinned; and xterm moves a cursor colour equal to the foreground to the background, which an `OSC 12` colour here is not |
| `?6` | DECOM origin | done |
| `?7` | DECAWM autowrap | done |
| `?8` | DECARM auto-repeat | done — set at power-on, as `[[source: vt510-decarm]]` gives it ("Default: Repeat (set)"); `Term::key` drops a repeat while it is reset, on every layer. xterm ignores the mode (its `srm_DECARM` case: "ignore autorepeat"). `DECSTR` leaves it, as DEC's DECSTR table and xterm's soft reset both do. `a_repeat_is_dropped_while_autorepeat_is_reset` |
| `?12` | cursor blink | done — since 2026-09-27 **a second input to the blink, *or*-ed with the style's own**, foot's rule (`[[source: foot-term-reset]]`'s file): `?12h` blinks a steady style and `?12l` cannot stop one that blinks, so the `?12h ?12l` vim sends at start and every `cnorm` leave a configured blink blinking. DECRQM answers the `?12` bit, reset at power-on whatever the style draws, as foot and xterm answer it; `DECSCUSR 0` and `7` leave it and both resets clear it. It is saved with the style at `?1049 h` and restored at `?1049 l` (`baud-core/tests/cursor_blink_rule.rs`, which replays vim's and helix's captured exits from `tests/cursor/`) |
| `?25` | DECTCEM cursor visible | done |
| `?45` | reverse wraparound | done |
| `?47` | alternate screen, pre-1049 | done — switches buffers and does nothing else: no cursor save or restore and no clear, so the alternate screen shows what it held when last left, and the cursor stays where it is (xterm's `srm_ALTBUF`, `[[source: xterm-reset]]`'s file). `DECRQM` answers from which buffer is shown, as for `?1049` -- baud's choice, since the pinned file records `whichBuf` only for XTSAVE and xterm's `DECRQM` code is not pinned. It was marked done while routed nowhere, and its one test passed on a no-op; every test in `baud-core/tests/legacy_alt_screen.rs` fails on a no-op. `RIS` erases the parked alternate surface, which xterm does not, because the reset frees the cluster and link slots its cells name. A resize while `?47` is up anchors the primary on the one shared cursor, which `?47 l` hands back. `?1047` and `?1048` are not implemented and are counted, and the saved cursor is one slot shared by both screens, where xterm is inferred to keep one per buffer |
| `?69` | DECLRMM left/right margins | done |
| `?1034` | eightBitInput — Alt/Meta add 128 | done — `Term::eight_bit_input()`, applied by `Term::key` under ISO 8859-1 only. What terminfo's `smm`/`rmm` send |
| `?1036` | metaSendsEscape — Meta prefixes `ESC` | done — `Term::meta_sends_escape()`, **set** at power-on |
| `?1039` | altSendsEscape — Alt prefixes `ESC` | done — `Term::alt_sends_escape()`, **set** at power-on |
| `?1001` | highlight tracking | done — recorded via `Term::highlight_tracking()`; the region via `highlight_region()`. Not drawn. ~~That needs a selection model~~ -- the reason expired when stream and block selection landed; drawing the region through the selection's highlight is unbuilt |

### SGR

Enumerated against `[[source: ecma48]]` § 8.3.117 and, for the extensions,
`[[source: ctlseqs]]` § "Character Attributes".

| parameters | status |
|---|---|
| 0 reset | done |
| 1, 2 bold/dim · 22 neither | done |
| 3 italic · 23 not | done |
| 4 underline, 21 double, **4:1..4:5** styled · 24 / 4:0 not | done — five styles, exactly one in force: straight, double, and `[[source: kitty-underlines]]`'s curly, dotted and dashed, each its own `Attrs` bit and drawn by the software compositor and the cell shader. A `4:n` *replaces* the family, and so do a bare 4 (the spec's backward-compatible 4:1) and 21 (`[[source: ecma48]]`'s "doubly underlined", the state 4:2 names), so 4:3 then 21 is double alone and 21 and 4:2 are one rendition (practice departs three ways and is not followed: vte reads 21 as bold off, `[[source: alacritty-vte-sgr]]`; the Linux console as a single underline, `[[source: linux-vt-sgr]]`; xterm holds 4 and 21 as two bits, `[[source: xterm-sgr]]`); 24 and 4:0 clear all five; an unknown n clears rather than aborts the walk. **4:3 is not 4;3** -- sfte flattens the colon and gets underline-plus-italic, measured -- and DECRQSS reports the three colon styles as 4:n so the reply round-trips. `each_colon_style_sets_exactly_its_own_bit`, `the_colon_form_is_not_the_semicolon_form`, `a_style_replaces_the_family_and_4_0_clears_it`, `twenty_one_replaces_the_family_in_either_order`, `twenty_one_and_4_2_are_one_rendition`, `a_colon_underline_style_round_trips`, `a_double_underline_after_another_style_reports_alone_and_round_trips` |
| 5, 6 blink · 25 steady | **partial** — **5 is drawn to the spec**: the app blinks `Attrs::BLINK` at 75 a minute (0.4 s shown, 0.4 s hidden, `BLINK_HALF` on the display link's clock), inside `[[source: ecma48]]` § 8.3.117's "slowly blinking (less then 150 per minute)", which a const assertion beside `BLINK_HALF` holds; the CPU path draws both phases (`hidden_blinking_text_is_the_frame_of_its_spaces`, `a_blinking_row_keeps_blinking_through_incremental_frames`) and the cell shader is held to it (`the_cell_arm_draws_what_the_instance_arm_draws`). This row said "no timer", which expired when the display link began driving the phase. **The gap is 6**: it sets the same bit and blinks at the same rate, where the text asks "rapidly blinking (150 per minute or more)" -- a departure until a rapid rate is chosen, which no text or owner decision fixes (for Jev), and with it whether 5 and 6 exclude each other (§ 8.3.117: "The usable combinations of parameter values are determined by the implementation"). Until then 6 is 5's rendition (`rapid_blink_is_blink`) and DECRQSS answers `5` for it, the rendition drawn (`decrqss_answers_5_for_rapid_blink_while_it_blinks_slowly`). 25 clears the bit. The Linux driver blinks nothing yet, and Reduce Motion and Dim Flashing Lights do not reach text blink |
| 7 reverse · 27 positive | done |
| 8 conceal · 28 reveal | done |
| 9 strike · 29 not | done |
| 30-37, 39 foreground · 40-47, 49 background | done |
| 90-97, 100-107 aixterm bright | done — palette indices 8..15 |
| 38/48 `;5;n` 256-colour, `;2;r;g;b` truecolour | done |
| 38/48 `:5:n` / `:2:Pi:r:g:b` sub-parameter form | done — both the six-part form with a colour-space id and the five-part form without |
| 10-19 fonts | done — recorded in `StyleWord`'s spare bits; `Term::style_of` is how a renderer reads one |
| 58/59 underline colour | done — `StyleWord` grew to sixteen bytes; the *cell* holds a `StyleId` index and did not change |
| 20 Fraktur, 26/50 proportional, 51-55 framed/encircled/overlined | done — recorded in `Attrs`, all from `[[source: ecma48]]` § 8.3.117; 23 clears italic **and** fraktur, 54 clears framed **and** encircled. 26 is a *reservation* there ("for proportional spacing as specified in CCITT Recommendation T.61"), so baud records the bit and resolves nothing |
| 56, 57 | **out** — `[[source: ecma48]]` § 8.3.117 reserves 56-59 for future standardization and `[[source: ctlseqs]]` assigns 56 and 57 nothing. 58/59 are done only because ctlseqs assigns *them* the underline colour, which is an extension over ECMA-48 rather than a reading of it. `sgr_56_and_57_are_reserved_and_set_nothing` |
| 60-65 ideogram attributes | done — `[[source: ecma48]]` § 8.3.117 defines all six: 60-64 are ideogram underline, double underline, overline, double overline and stress marking, each named for two edges because which edge is marked follows the writing direction, and 65 cancels all five. `Attrs` went to `u32` for the five bits. See `tests/sgr/ideogram.rs` |

**Fonts moved from `out` to `done` by measuring rather than assuming.** The row
said they needed cell bits that did not exist. A font is a *rendition*, and
rendition lives in `StyleWord` -- which carried sixteen bits of explicit
padding, put there because zerocopy rejects implicit padding. `PackedCell`
never had to grow, and the eight-byte cell was never the constraint. "A bit set
and never read" resolved the way it did for charset designations: baud records
the selection and `Term::style_of` is where a renderer reads it.

Every SGR parameter above is **done** or **out** but 6, **partial** since 2026-10-10 because it blinks at 5's slow rate where § 8.3.117 asks 150 a minute or more. `CSI m` used
to be listed as `partial` on the strength of that last row, which conflated two
different things. `out` is a scope decision with a reason — storing a font
selection no renderer can read would be a bit set and never used, which is the
failure this table exists to catch. `partial` means the row claims something it
does not do. Only the 5, 6 row does, and it says what.

**This was the largest `parsed` gap in the codebase and the table did not show
it.** `SgrAttr::fold` handled `Reset` and foreground only, falling through to "no
change" for `Set`, `Unset` and every `Bg` — because `StyleWord` was a bare `u32`
holding a 24-bit foreground and nothing else. Bold, italic, underline, reverse,
blink, conceal, strike and **all background colours** were parsed and discarded:
support in the parser, nothing on the screen. `StyleWord` now carries
foreground, background and attributes.

---

## Tier 2 — VT220 / VT420

In scope. Needed by real applications (vim, tmux, htop) but not by every one.

| seq | name | status |
|---|---|---|
| `CSI ! p` | DECSTR soft reset | done — resets the DEC-defined state and **not** the alternate screen, `alt_saved`, mouse tracking or its encoding. Those are xterm extensions, so a DEC list cannot mention them and xterm decides: in `ReallyReset` all four are inside the `if (full)` RIS branch, `FromAlternate` next to `ClearScreen` (`[[source: xterm-reset]]`). baud reset them until 2026-08-27, so a full-screen application recovering with `CSI ! p` was dropped onto the primary screen and lost what `?1049 h` had saved. **Of the mode word it restores only what xterm's soft reset restores**: DECTCEM, DECLRMM, DECOM, DECCKM, DECAWM, reverse wraparound and IRM, and releases `?2026`, which xterm lacks; LNM, `?4`, `?5`, `?8`, `?1004`, `?1034`, `?1036`, `?1039` and `?2004` stay, as they do there. It restores the keypad to numeric and `modifyOtherKeys` to its first value, which xterm does on both resets and RIS here did not. Until 2026-09-24 it restored the whole mode word, so a program recovering with `CSI ! p` lost its bracketed paste (`decstr_restores_the_modes_xterm_restores_and_keeps_the_rest`, `decstr_and_ris_restore_the_keypad_and_modify_other_keys`). It clears every `OSC 4` override and keeps the dynamic colours, as xterm's common path does (`both_resets_clear_every_palette_override`, since 2026-09-29). See `tests/xterm/soft_reset_scope.rs` |
| `CSI SP q` | DECSCUSR cursor style | done — the shape and the blink an application asks for, and **drawn with its blink** since 2026-09-25: on macOS at the system caret's rate since 2026-09-27 -- `NSTextInsertionPointBlinkPeriodOn` and `...Off`, AppKit's 650 ms on and 350 off where they are unset -- and held steady by macOS 15's non-blinking switch unless `[cursor] blink = true`, which changes the drawing and not the answer; on Linux xterm's 600 ms on and 300 ms off (`cursorOnTime`/`cursorOffTime`, `[[source: xterm-reset]]`'s file); only while the window has focus and restarted shown by output or a key -- foot's rules (`[[source: foot-term-reset]]`'s file) -- on the display link's clock or the Linux `poll` timeout (`baud-app/src/caret.rs`, `baud-render/tests/cursor_blink.rs`, the pixel oracle's off-phase scene). The power-on style is the embedder's (`Term::set_initial_cursor_style`, `[cursor] shape` and `blink`, a blinking block by default), and `Ps = 7`, `DECSTR` and `RIS` restore it, as xterm restores its resources. **The sources conflict on `Ps = 0`, and baud follows foot**: `[[source: ctlseqs]]` and xterm's `charproc.c` make it a blinking block, foot's `csi.c` resets it "to configured default" (`[[source: foot-csi]]`'s file). The two agree while the configured style is the default; `Ps = 1` stays the blinking block (`zero_and_seven_restore_the_configured_style_and_one_is_the_blinking_block`, `both_resets_restore_the_configured_style`). xterm's `cursorBlinkXOR`, which inverts an application's blink against the resource, is not followed. **The style is saved with the cursor at `?1049 h` and restored at `?1049 l`**, as kitty and wezterm do, and not by `DECSC`, which `[[source: vt510-decsc]]` does not list it for: helix ends its session on the alternate screen with `0 q`, the system entry's `Se` (`2 q`) and `cnorm`, and the shell gets its own cursor back. `DECRQSS` answers the style DECSCUSR set, not what `?12` draws |
| `CSI " q` | DECSCA character protection | done |
| `CSI ? J` | DECSED selective erase display | done — 0, 1 and 2; `3` is in Tier 3 |
| `CSI ? K` | DECSEL selective erase line | done |
| `CSI $ p` | DECRQM request mode | done |
| `CSI s` (DECSLRM) | left/right margins | done — with DECLRMM (`?69`), for writing and for `ICH`/`DCH`/`DECIC`/`DECDC`. **Stated gap: the vertical scrolls ignore the side margins.** `SU`, `SD`, `IND`, `RI`, `LF`, `IL` and `DL` move whole rows under DECLRMM, so a scrolling region is a band of rows and never the rectangle `[[source: vt510-decslrm]]` makes it ("sets the left and right margins to define the scrolling region"); images follow the text as the text moves, whole rows. Found by the region-image review, not yet built |
| `CSI ' }` / `CSI ' ~` | DECIC / DECDC | done |

DECSCA, DECSED and DECSEL land together on purpose: the two erases without the
protection are silent aliases for ED and EL, and an application that protected a
region would watch it vanish.

Protection is **not** an `Attrs` bit. `[[source: vt510-decsca]]`: "DECSCA does
not effect visual character attributes set by the select graphic rendition (SGR)
function" -- so it sits in its own field of `StyleWord`, where a renderer
reading attributes cannot mistake it for one. It *is* part of the interning key,
so protected and unprotected text are distinct renditions and the flag survives
in the template table without widening the 8-byte `PackedCell`.

Two rules follow, and both are pinned by tests. A plain ED/EL erases protected
characters -- only the private forms are selective. And the blank an erase
leaves behind does not inherit the protection, or the first DECSEL under
`DECSCA 1` would protect everything it just cleared.

---

## Tier 3 — xterm extensions in universal use

In scope because applications assume them, not because DEC specified them.

| seq | name | status |
|---|---|---|
| `OSC 0/1/2` | window/icon title | done — stored in the core (`Term::title`, `Term::icon_name`), and **set on the window** by both drivers since 2026-09-24, where until then nothing read it. **Not drawn on macOS**: the window crate's titled glass window hides its title text, so a program's title reaches the Window menu, Mission Control and accessibility, not the titlebar. On Wayland it reaches the compositor, whose titlebar shows it only under server-side decorations, which baud does not negotiate. The app reads the bytes in the encoding in force and turns controls into spaces (`baud-app/src/title.rs`). The icon name is stored and set nowhere: macOS's counterpart, the miniwindow title, is not exposed by the window crate |
| `OSC 4` | set colour palette | done — including the `?` query, **answered for every entry** since 2026-09-29, as xterm's `ReportAnsiColorRequest` does: the application's override, else the embedder's table (`Term::set_default_palette`, pushed with the default colours by both apps), else nothing on a core no embedder told (`baud-core/tests/palette_query.rs`). It answered only overridden entries before. **Both resets clear every override** since 2026-09-29, as xterm's `ReallyReset` calls `ResetAnsiColorRequest` in its common path, and leave `OSC 10`/`11`/`12` as it does (`baud-core/tests/palette_reset.rs`, `a_soft_reset_returns_an_overridden_entry_to_the_theme`); they kept every override before, so a program reset after recolouring the sixteen left its colours on everything drawn after it. A colour *name* (`OSC 4 ; 1 ; red`) is left alone rather than guessed, and **counted** as unsupported since 2026-09-20 -- the spec inventory probes with exactly such a name and found the arm silent |
| `OSC 10` / `OSC 11` | default foreground / background | done — set from an `rgb:` or `#` spec, read by the renderer before its theme (`Term::dynamic_color`), and the `?` query answered in the setter's shape, **four hex digits a channel** (`rgb:efef/efef/efef`), which is xterm's reply form: `rgb:` accepts one to four digits in what is *sent*, but readers parse the reply at fixed offsets, and yazi read baud's two-digit `rgb:ef/ef/ef` as a failure it reports as a dark background whatever the theme, until 2026-09-22 (`yazi_reads_a_light_background_as_light_and_a_dark_one_as_dark`). The `OSC 4` reply takes the same width. **A query on a fresh terminal answers the embedder's theme**, which both apps push in through `Term::set_default_colors` -- the query every theme-aware program sends to learn light from dark; a core that was never told says nothing, the `OSC 4` refusal for the same reason. Only the first spec of a sequence is taken, as with `OSC 4`. `osc_10_and_11_set_the_dynamic_colours`, `osc_11_query_answers_the_embedders_default`, `dynamic_colours_are_chunk_invariant`, and the pixels in `baud-render/tests/dynamic.rs` |
| `OSC 110` / `OSC 111` | reset default foreground / background | done — clears the application's override and leaves the theme's default, which is what reset means. `an_override_wins_the_query_and_reset_restores_the_default` |
| `OSC 12` / `OSC 112` | cursor colour | done — **stored and read** now that a renderer draws a cursor colour, which is the day the old counted-not-stored deferral named. The block cursor fills with it and the bar or underline cursor takes it, on all three paths -- `Renderer::convert_row` for the software and instance arms, the cell shader for the cell arm, which `the_cell_arm_draws_what_the_instance_arm_draws` gates pixel-for-pixel. The `?` query answers in the code-12 shape and `OSC 112` resets it to none; there being no embedder cursor default, none is the fresh-terminal answer too. `osc_12_sets_and_reads_the_cursor_colour` |
| `OSC 8` | hyperlinks | done — bounded link table that **refuses a colliding link rather than evicting the occupant**: a cell never resolves to an address other than the one written under its index, a clipped address is refused too, and both are counted in `links_refused` / `links_clipped`. **Activated in `baud-app` by Cmd-click**: `http`, `https` and `mailto` are opened, a `file` link on this host is revealed in Finder (the opener would *launch* an application bundle), everything else is refused and counted; Cmd-Shift-click is a selection, a repeat click is swallowed whole, and a tracking child never sees the gesture. `a_cell_never_resolves_to_a_uri_other_than_the_one_written_under_its_index`, `a_link_whose_uri_may_have_been_cut_is_refused_and_counted`, `the_four_schemes_are_admitted_and_nothing_else_is`, `a_cmd_click_on_a_link_arms_on_the_down_and_opens_on_the_up`; see security note |
| `OSC 52` | clipboard | done — **writes honoured, reads deliberately refused** (silence, counted in `clipboard_reads_refused`; see security note). Since 2026-10-10, the owner's V11: the body after `52;` leaves the 512-byte OSC buffer for two caller-provided areas (`Owned::with_clipboard`), is decoded as it arrives by `baud-domain`'s RFC 4648 decoder, and is published at the terminator (`Term::clipboard`, `Term::clipboard_writes`), whatever the read boundaries (`baud-core/tests/clipboard.rs`, the `osc52-*` property atoms). `Pc`'s twelve selections are kept as a set and an empty `Pc` is `s 0`; a `Pd` that is neither base64 nor `?` **clears** the selection, ctlseqs' rule. A copy is refused whole and counted, never cut, when the storage has no area, `Pc` names nothing ctlseqs knows, or it outgrows the area (`Term::clipboard_refusals`). The decoder accepts an unpadded final quantum and non-zero pad bits, both allowed by RFC 4648 3.2 and 3.5 and both a choice. macOS puts a write naming `c`, `p` or `s` on the general pasteboard when it is UTF-8, under `[clipboard] osc52` (on by default); `q` and the cut buffers have no counterpart and are refused (`baud-app/src/clip.rs`). Linux has no data device yet and provisions no area, so the core refuses every write and counts it |
| `OSC 133` | semantic prompt marks | done — `A`/`P` open the prompt zone, `B` keeps it, `C` closes it; acted on by `Term::resize`, which does not re-cut the rows a shell will redraw, and counted in `semantic_marks`. **Each screen has its own**: the zone and the block start are swapped by `?1049` as the kitty stacks are, the alternate screen's start fresh on each entry, and the primary's zone alone decides whether the primary re-cuts (`a_mark_on_the_alternate_screen_does_not_redraw_the_primarys_block`, `a_block_opened_on_the_alternate_screen_is_not_redrawn_on_the_primary`, `a_mark_on_the_alternate_screen_does_not_stop_the_primary_re_cutting`); until 2026-09-23 one of each served both screens, and a mark on one was read against the other's ring. `D`'s exit status is **not** parsed, because nothing here would read it |
| `OSC 104` | reset palette | done |
| `CSI ? 1049 h/l` | alternate screen buffer | done |
| `CSI ? 2031 h/l` | colour palette update notifications | done — since 2026-09-29. Reset at power-on and DECRQM answers the bit, so neovim's handshake subscribes (`neovims_handshake_subscribes_and_re_queries`); **nothing is sent on setting it**, as tmux learned the hard way. While set, an embedder colour change that moves the dark/light class sends one `CSI ? 997 ; 1\|2 n`: `Term::set_theme` and `Term::set_default_colors` compare the class before and after the whole change, so a class crossed inside one call is not reported (`a_class_crossed_inside_one_call_is_not_reported`). An application's own colour escapes send nothing and change the next `?996 n` answer. `RIS` clears it; `DECSTR` keeps it -- baud's rule, since xterm has no `?2031` (`ris_clears_the_mode_and_decstr_keeps_it`). The macOS app's OS light/dark flip under `theme = "auto"` reaches it through `tell_defaults` with `Overrides::Drop`, dropping the application's `OSC 4`/`10`/`11`/`12` colours -- only when the resolved appearance changes, since AppKit also calls back for accessibility variants of the same one (`palette_switch`, `a_flip_drops_the_applications_colours_and_reports_once`) |
| `CSI ? 2048 h/l` | in-band resize notifications | done — reports `CSI 48 ; rows ; cols ; height_px ; width_px t` on enablement, on re-enablement, after every resize, and **after a cell-size change even when the grid did not move**, carrying what the terminal **has** rather than what was asked for. The pixel fields are the text area -- the grid times the cell size `Term::set_cell_size` was given -- and zero until one is given, which `[[source: inband-resize]]` defines as the answer of a terminal that cannot report them; "A terminal MUST report pixel sizes if it is capable", and "Font size changes can also affect the text area size", which is why a zoom reports. Until 2026-09-22 the fields were always zero. `the_inband_report_carries_pixels_once_the_cell_is_known`, `a_cell_size_change_reports_even_with_the_grid_unchanged` |
| `CSI 14 t`, `CSI 16 t`, `CSI 18 t` | XTWINOPS text-area and cell size | done — `CSI 4 ; h ; w t` (text area in pixels), `CSI 6 ; h ; w t` (one cell in pixels) and `CSI 8 ; rows ; cols t` (text area in cells), in `[[source: ctlseqs]]`'s shapes, height first. **The two pixel answers need the cell size and are silence and a count without it**, the way an unanswerable `DSR` is; the cell count is always answered. Only the exact one-parameter forms: `CSI 14 ; 2 t` is the *window* with its frame, which this core is never handed, and stays counted. This is the reply an image client sizes its pictures from -- yazi sends `CSI 16 t` in its detection burst and read nothing back before 2026-09-22, so it drew every Sixel at a fixed 600x900. `the_window_queries_answer_from_the_cell_size_and_the_grid`, `without_a_cell_size_only_the_cell_count_is_answered`, `yazi_reads_the_cell_size_from_its_detection_burst`, `every_other_t_is_refused_or_not_xtwinops` |
| `DCS Pa ; Pb ; Ph q ... ST` | Sixel graphics | done — decoded into caller memory with no external decoder (`[[source: vt3xx-sixel]]` is the raster grammar, `[[source: ctlseqs]]` the DCS envelope), placed at the cursor and drawn by all three render paths. **A colour register takes RGB and HLS**: HLS since 2026-10-11, with DEC's hue origin, "Blue: 0 degrees Red: 120 degrees Green: 240 degrees" (`[[source: vt3xx-screen-control]]`), by the double hexcone of `[[source: css-color-4-hsl]]`, within one percent of the VT340's sixteen default colours on that chapter's Table 2-3 (`hls_defines_reproduce_the_vt340_default_colour_map`, `the_hls_hue_origin_is_blue_at_zero_red_at_120_green_at_240`); an HLS value past its range ignores the whole command, `Pc` included, `[[source: dec-std-070]]` 070-9 §8.1.3 (`an_hls_define_out_of_range_is_ignored_whole`). It was counted and left unpainted until then. **An image follows the row it was placed on**: scrolling into the history, a shorter screen retiring its row and a taller one pulling it back, a narrower screen re-cutting the lines above it, and a reader scrolled back to it. Its column is kept, and it is cropped at the edge rather than moved. It goes when its row does -- past the far end of the history, blanked by a reverse scroll, dropped by a reflow that did not fit -- and each is counted in `placements_expired`. **Each screen owns its images**: the shell's are not drawn over a full-screen program, and the program's are deleted when it leaves. Text over an image frees it. **A refused image is counted by the limit it hit**: `ImageStore::refusals` splits `images_refused` into the body scratch (with the body's true length), the pixel arena (with the image's size), the slot table and an image with no pixels, and the app logs one `warn` line a pump naming the limit and the `[images]` key that raises it -- `a_body_past_the_scratch_is_counted_against_the_body_with_its_length`, `an_image_past_the_arena_is_counted_against_the_arena_with_its_size`, `a_full_slot_table_is_counted_against_the_slots`, `a_sixel_with_no_pixels_is_counted_as_empty`. Gates: `baud-core/tests/sixel.rs` (decode and place, chunk invariance, refuse-and-count by cause, text over an image), `baud-core/tests/image_anchor.rs` (every resize and scroll case, asserted by the text beside the image, including foot's blank-line trap and WezTerm's text-on-the-row trap), `baud-render/tests/image.rs` (a pinned frame digest, the one-row scroll, a view scrolled back after a shrink, the per-screen rule) and the pixel oracle. **It follows its row inside the screen too**: a scrolling region, `IL` and `DL` move rows without moving the window, so each moves the images on the rows it moves, and one whose row the operation deletes -- off the top of a region that copies, which is a middle region, or a top-anchored one on the alternate screen or under side margins with at least as many rows under it as in it, or past a region's bottom -- goes and is counted; a top-anchored region that rotates on the primary retires its top line, image and all, to the history; on the alternate screen, which keeps none, the image goes and is counted. A status line under a region keeps its image while the region scrolls, on either screen. **One stated gap**: an image anchored on a wrapped continuation row -- placed after text that wrapped onto its row -- comes back a row higher from a narrow-then-widen round trip, pinned by `an_anchor_on_a_wrapped_row_lands_a_row_higher_after_a_round_trip`; an image printed after a newline starts its line and round-trips exactly. |
| `CSI ? 2004 h/l` | bracketed paste | done — recorded and exposed as `Term::bracketed_paste()` |
| `CSI ? 1004 h/l` | focus reporting | done — `Term::focus` queues `CSI I` on gain and `CSI O` on loss only while the mode is set, and nothing otherwise, because an application that did not ask reads `CSI I` as a keypress. The macOS app polls the window's key status on the tick and before a mouse event and forwards a change (`Driver::poll_focus`), because the window crate's hook reports the *view's* first-responder status, which fired once at `show()` before the driver was installed and never on an app switch -- so **no transition was ever reported on macOS until 2026-09-20**, found by the OSC 8 live check, and a live check reads `CSI I`, `CSI O`, `CSI I` around an app switch now. The Linux app calls it from its keyboard hook. **Dropped uncounted until 2026-09-20**: `1004` was named nowhere in the code and had no row here, while `vim-unicode.raw` sends it on its first line; `set_private_mode`'s wildcard counts now, as the ANSI table's always did. `gain_sends_csi_i_and_loss_sends_csi_o`, `nothing_is_sent_unless_the_application_asked`, `the_vim_recording_enables_it` |
| `CSI ? 9 h/l` | X10 compatibility mouse, a tracking level | done — since 2026-10-09. A press of button 1, 2 or 3 and nothing else: no release, no motion, no wheel and no modifiers (`[[source: ctlseqs]]`, "X10 compatibility mode": "only on button press ... Cb is button-1, where button is 1, 2 or 3"). The encoding in force still applies, as xterm's `EmitButtonCode` writes it (`[[source: xterm-button]]`), so `?9` with `?1006` is `CSI < 0 ; x ; y M`. A level beside `?1000/1002/1003`, and any of the four reset turns tracking off (xterm's `really_set_mousemode`, `[[source: xterm-reset]]`'s file); `RIS` clears it and `DECSTR` keeps it, as for the others. It was counted in `unsupported_param` and dropped before. The Linux app has no pointer and sends no report under it. `x10_compatibility_reports_a_press_and_the_button_alone`, `x10_compatibility_sends_no_release_no_motion_and_no_wheel`, `x10_compatibility_takes_the_encoding_in_force`, `any_tracking_reset_turns_x10_compatibility_off_and_on_is_a_level`, `ris_clears_x10_compatibility_and_the_encoding_and_decstr_keeps_them` |
| `CSI ? 1000/1002/1003 h/l` | mouse tracking | done — a level; any reset turns it off. No level reports a wheel release, "Release events for the wheel buttons are not reported" (`[[source: ctlseqs]]`, `wheel_releases_are_not_reported`). **Motion is reported per cell**, as ctlseqs' button-event text has it: "Motion events are reported only if the mouse pointer has moved to a different character cell". `Term::mouse_report` keeps the cell of the last report that went out -- a press or release as well as a move, as xterm's `EditorButton` sets `mouse_row`/`mouse_col` on every report (`[[source: xterm-button]]`) -- and drops a move into it, comparing the position and not the button or modifiers; a refused or unqueued report does not move it, and a tracking or encoding change and both resets forget it, where xterm forgets only at `VTInitialize` (`[[source: xterm-reset]]`'s file; practice). Presses and releases are never held back. Until 2026-10-09 it kept no last position and the macOS app's `Driver::mouse` hands it every AppKit move and drag, so a pointer inside one cell sent one identical report per event. The Linux app has no pointer and sends no report at any level. `motion_inside_one_cell_is_reported_once`, `a_drag_inside_one_cell_is_reported_once`, `a_modifier_change_inside_the_cell_is_not_a_move`, `a_press_or_release_is_always_sent_and_moves_the_cell`, `a_refused_motion_does_not_move_the_cell`, `a_mode_change_or_a_reset_forgets_the_cell` |
| `CSI ? 1005 h/l` | UTF-8 mouse encoding | done — since 2026-10-09. X10's bytes, each UTF-8 encoded past 127: one byte below one-based 96, two from 96 to 2015 (`[[source: ctlseqs]]`: "expanding the maximum encodable position from 223 to 2015"), and past 2015 refused and counted in `mouse_unencodable`, where xterm clamps and sends a `0` byte (`EmitMousePosition`, `[[source: xterm-button]]`). **The same bytes under `ESC % @`**: the mode is the encoding, ctlseqs names the cost ("will not pass through luit(1) unchanged") rather than a fallback, and xterm's emit path reads no encoding. Counted and dropped before. The Linux app has no pointer and sends no report in it. `utf8_encoding_is_x10_below_ninety_six_and_two_bytes_from_it`, `utf8_encoding_reaches_2015_and_refuses_past_it`, `utf8_encoding_ignores_the_input_encoding`, `utf8_mouse_reports_are_x10_until_127_and_utf8_after` |
| `CSI ? 1006 h/l` | SGR mouse encoding | done — no ordinate limit. **A set replaces the encoding in force and a reset undoes only its own**, xterm's rule for all four ("a reset is only effective against the matching mode", `[[source: xterm-reset]]`'s file); ctlseqs makes them "mutually exclusive" and says no more. `?1006 l` returned to X10 whatever was set until 2026-10-09. The Linux app has no pointer and sends no report in it. `setting_an_encoding_replaces_the_one_in_force`, `resetting_an_encoding_undoes_only_that_encoding`, `a_set_selects_an_encoding_and_a_reset_undoes_only_its_own` |
| `CSI ? 1007 h/l` | alternate scroll | done — since 2026-10-09, `Term::alternate_scroll()`, read by the macOS app's wheel (`wheel::route`): on the alternate screen with no tracking, set sends the cursor keys through `Term::key`, so in DECCKM's form, and reset sends nothing (`[[source: ctlseqs]]`, "Wheel mice": "if Alternate Scroll mode is set, then cursor up/down controls are sent when the terminal is displaying the Alternate Screen Buffer"). **Set at power-on**, which is what baud did before it read the mode; xterm's `alternateScroll` resource defaults to `False` (`[[source: xterm-reset]]`'s file), and ctlseqs leaves the initial state to the resource -- baud's default differs from xterm's, the owner's call. Kept by `RIS` and `DECSTR`, as xterm keeps it: no `ReallyReset` path names it. Tracking takes the wheel whatever it says. The Linux app has no pointer and reads nothing. `after_1007l_the_wheel_on_the_alternate_screen_sends_nothing`, `the_alternate_screen_sends_arrows_until_an_application_says_otherwise`, `tracking_and_the_primary_screen_ignore_1007`, `the_wheels_keys_are_the_cursor_keys_as_decckm_has_them`, `alternate_scroll_survives_both_resets` |
| `CSI ? 1015 h/l` | urxvt mouse encoding | done — since 2026-10-09. `CSI Cb ; Cx ; Cy M`: X10's button value, its `+ 32` and its release code kept, in decimal, and `M` for a release too (`[[source: ctlseqs]]`: "the same button encoding as X10, but printing it as a decimal integer rather than as a single byte"). herdr and zellij send it before `?1006`, which replaces it. Counted and dropped before. The Linux app has no pointer and sends no report in it. `urxvt_is_the_x10_button_value_in_decimal`, `urxvt_mouse_reports_have_the_documented_shape`, `herdrs_start_ends_in_sgr_any_event_and_nothing_is_counted` |
| `CSI ? 1016 h/l` | SGR-Pixels mouse encoding | done — since 2026-10-09. `?1006`'s shape with the position in pixels (`[[source: ctlseqs]]`: "report position in pixels rather than character cells"), one-based on the wire as xterm writes it (`value + 1`, `[[source: xterm-button]]`). The pixel is the event's `PointerPx`: zero-based from the text area's first pixel, in the device pixels `CSI 16 t` and `?2048` answer in; the macOS app derives it and the cell in one function and clamps it to the text area, where xterm reports the border as `0` or less. An event with no pixel, or at `u16::MAX` where `ImagePx` saturates, is refused and counted. **Motion is gated by the cell, not the pixel**: button-event tracking's "moved to a different character cell" names no encoding and `?1016` changes only the format; xterm reports a move per pixel here, its row and column holding pixels in `SET_PIXEL_POSITION_MOUSE` (`[[source: xterm-button]]`), a practice baud departs from and the owner's call. Counted and dropped before. **The Linux app has no pointer**, so it has no pixel to give and sends no report in it; the core takes the pixel from whichever embedder puts one on `MouseEvent::px`, and the macOS app is the only one that does. `sgr_pixels_motion_is_gated_by_the_cell_as_the_text_says`, `sgr_pixels_reports_the_pointer_in_pixels`, `sgr_pixels_refuses_an_event_with_no_pixel_position`, `sgr_pixel_mouse_reports_carry_the_pixel_one_based`, `a_point_names_its_pixel_and_the_cell_that_holds_it`, `a_wheel_under_1016_reports_the_pointer_in_pixels` |
| X10 mouse encoding (`ESC [ M Cb Cx Cy`) | the default, what tracking gives with no encoding mode set | done — positions past 223 refused and counted, not truncated. **A release is code 3** since 2026-10-09, ctlseqs' "3=release": it carried the released button, so under plain `?1000` a left release went out byte for byte as a left press. `an_x10_release_says_release_not_which_button`, `x10_family_releases_are_code_three_and_sgr_keeps_the_button` |
| `CSI ? 1016 $ p` — DECRQM on the mouse modes, `?9`, `?1000`-`?1003`, `?1005`-`?1007`, `?1015`, `?1016` | DECRQM | done — every tracking level and every encoding answers `1` only while it is the one in force, and `?1007` its flag, as xterm's `do_dec_rqm` answers them (`[[source: xterm-misc]]`); `?9`, `?1005`, `?1007`, `?1015` and `?1016` answered `0`, not recognised, before. The answer is the core's state, so the Linux app gives the same answers; it has no pointer, so a mode it answers `1` for sends no report there. `decrqm_answers_every_mouse_mode`, `the_mouse_modes_land_the_same_at_every_read_split` |
| `CSI ? 1001 h/l` | highlight tracking | done — recorded; see the mode table. The "xterm will hang" warning describes an event loop, and `baud-core` has none |
| `CSI ? 2026 h/l` | synchronised output | done — exposed as `Term::synchronized_output()` |
| `CSI 3 J` | ED 3 erase saved lines | done — since 2026-10-09. `[[source: ctlseqs]]`: "Ps = 3 -> Erase Saved Lines, xterm". The history goes and the screen, the cursor and the rendition stay (`erase_saved_lines_empties_the_history_and_keeps_the_screen_and_the_cursor`); **the primary's from either screen**, as xterm's `do_erase_display` zeroes its one `savedlines` count whichever buffer is shown (`[[source: xterm-scroll]]`; `erase_saved_lines_on_the_alternate_screen_empties_the_primarys_history`); every ring row past the screen and the cut rows is erased, not only the count, for `RIS`'s reason (`erase_saved_lines_leaves_nothing_of_them_in_the_ring`). **The rows a shorter window cut below the cursor stay**: they are the bottom of the screen a growth brings back, not saved lines (`erase_saved_lines_keeps_the_rows_a_shrink_cut_below_the_window`). An image on an erased line goes. `Term::history_erasures` moves on it and on `RIS`, and the macOS app puts a window over the history on the live screen and settles a slide when it moves, however the read falls (`an_erased_history_puts_the_window_on_the_live_screen_however_the_read_falls`); a selection naming an erased line is dropped or clipped to the screen by the clip every read applies. EL has no `3`: `CSI 3 K` stays counted |
| `CSI ? 3 J` | DECSED 3 selective erase saved lines | done — since 2026-10-09. "Selective Erase Saved Lines, xterm": DECSED over the history, so what DECSCA protected stays and the lines stay lines (`selective_erase_saved_lines_spares_what_decsca_protected`), from either screen. **xterm erases them whole** -- its `case 3` is one branch for both forms and ignores protection (`[[source: xterm-scroll]]`) -- where DECSCA's text says DECSED cannot erase a protected character (`[[source: vt510-decsca]]`); baud follows the text, decided 2026-10-10 by Jev at 1.00 and 0.98 under the owner's rule of that day and listed for the owner's veto (`docs/record.md`, "Jev's calls on `CSI ? 3 J`, XTSAVE and XTRESTORE, each as built") |
| `CSI ? s` | XTSAVE, `CSI ? Pm s`, save DEC private modes | done — since 2026-10-09. `[[source: ctlseqs]]`: "Ps values are the same as for DECSET ... this uses a one-level cache ... Only those modes listed as parameters are saved." **Every mode DECSET acts on keeps the value `DECRQM` answers**, one slot each (`PrivateMode::slot`), and one saved twice keeps the second (`the_cache_is_one_level_and_a_restore_keeps_it`); a mode DECSET does not act on -- unnamed, the read-only `?1020`, the permanent `?2027` -- is counted in `unsupported_param`, a parameter at a time, as DECSET counts it (`a_mode_decset_does_not_act_on_is_counted_by_both`). Held mode by mode against DECSET's own behaviour over `?0`-`?3000` and `?9999` (`every_mode_decset_changes_is_saved_and_restored_and_no_other_is`). The `?` keeps it from SCOSC and DECSLRM (`the_question_mark_keeps_them_apart_from_scosc_decstbm_and_decslrm`). **`RIS` empties the cache**, ECMA-48 § 8.3.105's "initial state" (`[[source: ecma48]]`), and `DECSTR` keeps it (`ris_empties_the_cache_and_decstr_keeps_it`). xterm saves a different set (`savemodes`, `[[source: xterm-reset]]`): not `?2` or `?8`, and `?3` only under the `c132` resource `?40` sets, where baud follows the text, decided 2026-10-10 by Jev at 1.00 and 1.00 and listed for the owner's veto (`docs/record.md`, "Jev's calls on `CSI ? 3 J`, XTSAVE and XTRESTORE, each as built"); one slot for the four tracking levels and one for the four encodings, the owner's call (XT-4, XT-5); and it keeps the cache across `RIS`, where baud empties it for ECMA-48's initial state, decided 2026-10-10 by Jev at 0.94 and 0.94 and listed for the owner's veto |
| `CSI ? r` | XTRESTORE, `CSI ? Pm r`, restore DEC private modes | done — since 2026-10-09. "The value of Ps previously saved is restored ... Only those modes listed as parameters are restored." **Through DECSET's own path**, so a restore has the effects setting or resetting the mode has -- `?1049` saves the cursor and clears, or leaves and restores it; `?6` homes; `?3` clears (`xtrestore_has_the_side_effects_of_setting_the_mode`) -- and `DECRQM` answers from the state it left (`xtrestore_brings_back_what_xtsave_saved`). **Only a mode whose value differs from the saved one is set or reset**, so restoring a tracking family puts back the level saved in either order, and `?1049` restored set on the alternate screen does not clear it (`a_restore_changes_only_what_differs_from_the_saved_value`). The text is silent on a value already in force; xterm re-applies `?6` whatever its value, and `?3` too under its `c132` resource (`restoremodes`, `[[source: xterm-reset]]`), and baud's no change was decided 2026-10-10 by Jev at 0.96 and 0.94 and is listed for the owner's veto. **A mode never saved restores nothing** (`only_what_was_saved_and_listed_is_restored`): a bare `CSI ? 2 r` stays in ANSI. xterm restores `?1049` as the bare switch of `?47`, where baud follows the text, decided 2026-10-10 by Jev at 0.98 and 0.96 and listed for the owner's veto (`docs/record.md`, "Jev's calls on `CSI ? 3 J`, XTSAVE and XTRESTORE, each as built"); it restores the tracking level saved under any of the four mouse modes (XT-4) and a never-saved mode from a zeroed array -- `?25 r` hides the cursor (XT-1) (`restoremodes`, `VTInitialize`'s `memset`, `[[source: xterm-reset]]`): each the owner's call. The `?` keeps it from DECSTBM. Generated programs reach both through `?25`'s atoms (`properties.rs`) |
| `CSI ? 25 h/l` | DECTCEM cursor visibility | done — state exposed via `cursor_visible()`. A hide that is exactly `ESC [ ? 2 5 l` with a complete `CUP` or `HVP` straight before it also records where the cursor is as `placed_cursor()` -- helix hides its cursor for good and moves it to the selection first on every frame, and an embedder's trail follows the placement; `?25h`, a screen switch, either reset and a change of geometry forget it (`baud-core/src/placed.rs`, `tests/placed_cursor.rs`) |
| `CSI > 4 ; 2 m` | XTMODKEYS `modifyOtherKeys` | done — `Pv` is the level, 0-2, read by `Term::key`. **Level 3 and the other six resources are refused and counted**, not rounded down: an application that asked for a level it did not get must be able to find out, which is the contract `DECRQM` keeps by answering `0` rather than `2`. See `the_worked_examples_are_what_baud_sends` and `an_unrepresentable_level_is_refused_and_counted` |
| `CSI > 4 ; 1 f` | XTFMTKEYS `formatOtherKeys` | done — `Pv` picks `CSI 27 ; mod ; code ~` or `[[source: fixterms]]`'s `CSI code ; mod u`. **This is the only route to the `CSI u` form**, which defines no negotiation of its own |
| `CSI = 1 ; 1 u`, `CSI ? u`, `CSI > 1 u`, `CSI < 1 u` | kitty progressive enhancement | **done -- all five flags, and the query is answered.** `[[source: kitty-keyboard]]` detects support by whether the query is *answered*, so a reply is the claim: `CSI ? u` now answers `CSI ? flags u`, made only after `Term::key` honours every enhancement (disambiguate, event types, alternate keys, all-keys, associated text) for text, functional, keypad and the Private Use Area keys. `CSI = / > / <` maintain the per-screen flag stack. Its own words -- "It does not make sense to implement only a subset" -- is why this was all-or-nothing and the query stayed silent through every partial increment. `the_query_is_answered_with_the_current_flags` |
| `CSI ? Pp m` / `CSI ? Pp g` | XTQMODKEYS / XTQFMTKEYS | **counted and unanswered** — `[[source: ctlseqs]]` line 1310 shapes the reply as `CSI > Pp m` and line 830 as `CSI > Pp f`, both *without* the value, while saying the response "can be used to restore this state" — which it cannot without one. Answering either shape would be answering a query with a shape the application did not ask for, so baud stays silent and counts. The conflict is recorded rather than guessed |

---

## Out of scope, with reasons

Recorded so that "not implemented" is a decision rather than an omission.

| seq | name | why not |
|---|---|---|
| Kitty graphics `APC G` | image protocol | **Out until its PNG path has a `no_std` decoder**: `f=100` hands the terminal a PNG, which needs zlib, and baud-core decodes into caller memory with no allocator and no dependency -- the question `docs/research/notes/s-sixel.md` leaves open. Sixel, the self-contained half, is done in Tier 3. And **not to be confused with `[[source: kitty-keyboard]]`**, which shares a vendor and nothing else: one is images over `APC`, the other key encoding over `CSI ... u`. The `APC` **string must still be consumed** — that is Tier 0. ~~and it is a bug today~~ **It is not: the claim outlived the defect.** Measured rather than assumed, an `APC G` command leaves the screen untouched and is counted, and `every_control_string_is_consumed_and_the_text_after_it_is_not` now pins it for all five string types together with `a_control_string_survives_a_read_boundary`, because an image payload is kilobytes and never arrives in one `read(2)` |
| `DCS ... | DECUDK` | user-defined keys | **The stated reason expired and the row survives on a new one.** It read "Input-side; baud has no input path", and baud has one -- `encode_key` and `Term::key`. What keeps DECUDK out is that it redefines what a *key* sends, which needs a table of user-programmable strings and a policy for who may set them; the resource above it needs neither. Out on scope, not on absence |
| the rest of `CSI Ps t` | XTWINOPS window operations and the title stack | Recognised as XTWINOPS and refused, counted in `unsupported_param` the way a `DSR` baud cannot answer is. Each moves, resizes, iconifies, raises or reports the *window* or the screen, which this core is handed nothing about, or pushes a title onto a stack it does not keep; xterm itself gates them behind `allowWindowOps`. `CSI 22 ; 2 t` / `CSI 23 ; 2 t`, the title save and restore vim sends, is the one with a real consumer and the one to take first. The three reports that *are* answered -- 14, 16, 18 -- are in Tier 3. |
| `CSI ? Pi ; Pa ; Pv S` | XTSMGRAPHICS, Sixel colour registers and geometry | Counted and unanswered. A reply is the claim that the attribute can be read, and the right numbers are the decoder's register count and the image budget's geometry, which is a change of its own. **It does not scroll**: `CSI Ps S` is SU on the same final byte, and a note carried from the Sixel work said the private form scrolled; `xtsmgraphics_does_not_scroll_and_is_counted` says it never did. |
| ReGIS, Tektronix | vector graphics | Historical; no modern application emits them. |
| National replacement character set mode | DECNRCM, `CSI ? 42 h` and `l` | Not recognised (`PrivateMode::Other`), and the national sets above are translated in every mode. That departs from DEC's gate: "These sets are available when you select national mode" (`[[source: dec-special-graphics]]` §2.4.3), with `[[source: vt220-rm-ch4]]` §4.6.19 and Table 4-4's note, `[[source: vt420-rm]]` p. 27 and `[[source: dec-std-070]]` §5.8.1 note 4; xterm gates the same way (`[[source: xterm-reset]]` `charproc.c:1929`). Whether baud gates, and how, is note AF's Q-DECNRCM, the owner's and open. This row said the sets were "Superseded by UTF-8" while the rows above translated fourteen of them. |

---

## A source conflict, recorded

`[[source: vt510-decawm]]` gives DECAWM's default as **"No autowrap"**.
`[[source: ctlseqs]]` documents xterm, where autowrap is on, and every modern
terminal wraps. baud follows xterm: an application emitting a long line expects
it to wrap, and the DEC default would break essentially all of them. The older
and stricter reading is the one baud does **not** implement, and that is a
choice rather than an oversight.

## Why these decisions went the way they did

The reasoning — the refusals, the source conflicts, the things that look like
gaps and are not — is in [protocol-decisions.md](protocol-decisions.md). It was
in this file, 180 lines below the last table, where nobody scanning for an
operation's status was ever going to find it.

---

## How this file stays true

`baud-core/tests/coverage.rs` reads this file and fails if a row marked **done**
names an operation no test mentions. Run by `devenv test`, so it gates CI.

Be precise about what that proves, because this paragraph was itself the
failure it warns about for most of the project's life: it named
`tests/coverage.rs` while **no such file existed**, so the guarantee was
enforced by nothing.

* The test **does** prove every `done` row is mentioned by some test. Marking a
  row done with no test at all fails.
* It **cannot** prove the test would fail if the operation regressed. Nothing
  mechanical can. That stays a review obligation, and it is stated here as one
  rather than dressed up as a guarantee.

A weaker check that is real beats a stronger claim that is not. Documentation
claiming coverage it does not have is the failure mode this file exists to
prevent — the README claimed "~22 operations" while three of them were parsed
and dropped.

### An operation is done on *both screens*, or it is not done

`EL` and `ED` sat at **done**, with tests, and did **nothing at all** on the
alternate screen in `baud-core2`'s `OpMode::Runs`. That core's `erase_line`
had a span-truncation fast path that read the `promoted` flag raw; that flag belonged to
the primary screen, so on
the alternate one it was false for every row, the erase took the span path, and
it returned without touching a cell. `HELLO WORLD` survived an erase-to-end
from column 6.

Every erase test in `baud-core2` ran on the primary. So did every scrolling
test — full-screen scrolling in that core's alternate screen was copying rows
for the same reason, at 26x the cost.

**The enumeration was complete in the wrong dimension.** This file lists the
operations and `coverage.rs` checks each is mentioned; neither asks *in which
state* it was exercised. The alternate screen is not an operation and so has no
row here, but it changes the behaviour of the ones that do — in `baud-core2`
it was where the run table stopped applying — and every full-screen
application spends its whole life there.

Two gates came from that in `baud-core2`, and neither survives as it was
written: the run table and the op modes went with that core (bcefaae), and
this one has neither. Here each screen is a surface of its own -- cells,
extents, ring and rotation -- and `set_alt_screen` swaps the view onto the
other (`View::swap_surface`), so an operation reaches whichever screen is up
through the one view. What stands now:

* `erases_on_the_alternate_screen_are_not_swallowed_by_the_span_path`
  (`baud-core/tests/vt100/scrolling.rs`) runs six erases on the alternate
  screen, once each: there is one mode, and what it pins is that the surface
  erased is the surface shown. core2 ran each in `Computed` as well as `Runs`.
* core2's `screen_isolation.rs` required every function touching its run
  table to be screen-aware, structurally, because a wrongly zeroed run count
  showed through no public accessor. It was deleted with that core, and this
  file then named `baud-core/tests/xterm/alt_screen.rs` in its place, which
  holds no structural check. A write through to the parked primary, or a
  primary back under a wrong rotation, shows in its rows, so the checks read
  rows. A wrong extent does not, until something trusts it: with
  `View::swap_surface` swapping no extents, every row of
  `a_scrolled_primary_comes_back_in_the_order_it_left`'s trip compared
  equal, and the test caught it through what trusts the extents -- the
  history read back, the erase, and an exit that indexed past the screen.
  `a_round_trip_through_the_alternate_screen_preserves_the_primary` and
  `index_on_the_alternate_screen_leaves_the_primary_intact` scroll the
  alternate screen over a primary that never scrolled, so they fail when the
  primary comes back under the alternate's rotation and pass when it comes
  back reset to zero, the rotation it started from.
  `a_scrolled_primary_comes_back_in_the_order_it_left` scrolls the primary
  first, to rotations of 24, 33, 11, 111 and 57 (the last three over
  history), then crosses `?1049` and `?47` and holds every row, the history
  and the cursor to a twin that never switched, at six ways of cutting the
  reads; then it erases rows 42-50 on both and compares again, for the
  extents.

The reviewer's question for a new operation is therefore not only "is there a
test" but "does it run where applications will run it": on the alternate
screen as well as the primary, and over a primary that has scrolled as well
as one that has not.
