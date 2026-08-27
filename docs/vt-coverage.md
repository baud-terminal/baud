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
| **partial** | implemented but known-incomplete; the gap is named in the row — **no row carries this** |
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

**No row carries `partial`, `parsed` or `todo`, and `no_row_is_unfinished` in
`baud-core/tests/coverage.rs` fails if one ever does.** `out` is the only status that
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
| `escape` | done | |
| `escape_intermediate` | done | **collects** intermediates, plural — it kept only the last, which made every two-byte charset final unreachable |
| `csi_entry` | done | its own state — see below |
| `csi_param` | done | `CsiParams` |
| `csi_intermediate` | done | intermediate recorded, in both feed paths |
| `csi_ignore` | done | |
| `dcs_entry` | done | its own state — the marker rule, as `csi_entry` |
| `dcs_param` | done | C0 **ignored**, not executed |
| `dcs_intermediate` | done | `30-3F` ends the sequence — the parameter string is over |
| `dcs_passthrough` | done | payload collected for DECRQSS |
| `dcs_ignore` | done | unhandled DCS consumed, never executed |
| `osc_string` | done | payload captured, bounded, and dispatched |
| `sos_pm_apc_string` | done | payload consumed, never executed |

~~`sos_pm_apc_string` is a correctness and security gap.~~ Resolved: DCS, SOS,
PM and APC payloads are consumed by `StringIgnore` and never executed. Note the
specified limit, which is not a hole: `[[source: williams]]` says a bare `ESC`
*cancels* a control string, so the guarantee is precisely that a payload
containing no ESC is inert.

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

`a_private_marker_after_a_parameter_is_ignored` and
`a_marker_after_a_parameter_does_not_erase` pin both. Splitting the state also
closed two smaller holes in the same arm:

* **C0 inside a control sequence.** `[[source: williams]]` gives `csi_param` an
  `execute` action on `00-17,19,1C-1F` that returns to the same state. baud had
  no C0 branch, so a stray NUL fell through to `csi_ignore` and silently
  cancelled an unrelated SGR. `a_c0_inside_a_csi_does_not_cancel_it` and
  `the_c0_inside_a_csi_actually_executes` pin the rule and the fact that
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

Four arms each wrote `if b.is_c0() { self.execute(b) }` — and `is_c0()` is
`b < 0x20`, which **includes all three**. So an ESC arriving mid-sequence was
executed and the parser stayed in the parameter string: `CSI 1 ESC [ 2 m` lost
its escape, dispatched a nonsense `CSI 1 [`, and printed `2m` as text.

It is handled once now, ahead of the state match, so no arm can forget it. The
states that decline are the ones that must: inside a control string ESC is the
first half of ST, so the payload has to survive until the next byte decides
between *terminator* and *cancel*. Those arms already implement that themselves.

| status | |
|---|---|
| `18` CAN, `1A` SUB — cancel from any state, control strings included | done |
| `1B` ESC — abandon and begin a new sequence | done |
| C0 in `csi_entry` — execute and **stay**, without consuming the marker position | done — `CSI <BS> ? J` is still DECSED |
| C0 in `csi_intermediate` | done — this arm had no C0 branch, so `CSI ! <NUL> p` lost its DECSTR |
| `7F` DEL in `csi_entry` / `csi_param` | done — ignored, and does not consume the marker position |
| SUB additionally displaying the DEC error character `␦` | **out** — williams says VT220-VT500 hardware does; xterm does not, and baud follows xterm as it does for DECAWM. Detail below |

**The sources disagree, and baud follows xterm — the same call, for the same
reason, as DECAWM's default.**

`[[source: williams]]` records that "SUB will also display the error character, a
reversed question mark", describing VT220 through VT500 hardware.
`[[source: ctlseqs]]` documents `SUB` nowhere as producing a glyph, and xterm
does not write one. baud cancels and writes nothing.

The reason given here used to be that this was "a rendering decision made where
no renderer exists". That was wrong and worth correcting rather than quietly
restating: baud stores codepoints, not pixels, so writing `U+2426` would be
storing a *character*, which it does all day. Nothing about the missing renderer
prevents it. The actual reason is the source conflict, and this file already has
a precedent for how to resolve one — DECAWM's default is "No autowrap" in the
VT510 manual and on in xterm, and baud follows xterm because that is what
applications are written against. Same here.

The cost of being wrong is asymmetric, which settles it. Writing a glyph an
application did not send is indistinguishable, afterwards, from text it did; not
writing one loses nothing an application can observe, because the cancellation
itself is the visible behaviour and is implemented.

### Encoding: UTF-8 only, and 8-bit C1 controls are therefore out

`[[source: williams]]`'s parser has 8-bit C1 controls throughout — `9B` enters
`csi_entry`, `9D` `osc_string`, `90` `dcs_entry`, `9C` is ST. baud has none of
them, and a reader comparing against the state diagram will notice. The absence
is forced rather than chosen:

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
| 7-bit `ESC Fe` forms `ESC P`, `ESC [`, `ESC \`, `ESC ]`, `ESC X`, `ESC ^`, `ESC _` | done — `[[source: ecma48]]` § 5.3 specifies these as the representation of C1 in a 7-bit environment, so this *is* the C1 interface, not a substitute for one |
| 8-bit C1 introducers `90`, `9B`, `9C`, `9D`, `98`, `9E`, `9F` | **out** — they are UTF-8 continuation bytes; a stray one is a decode error and renders U+FFFD. Follows ghostty; diverges from alacritty and wezterm, which accept them |
| character width from East Asian Width + general category, generated from pinned Unicode data | done — `AmbiguousWidth` is embedder policy, `[[source: uax11]]` § 5 |
| UAX #29 extended grapheme cluster segmentation | done — `ClusterBreak`, and all **766** cases of Unicode's own conformance suite pass |
| combining marks stored against their base cell | done — `Term::marks`, bounded and evicting; they used to be **discarded** |
| one cluster per cell — ZWJ sequences, flags and Indic conjuncts as a single cluster | done — `ClusterBreak` consulted in `put_cp`; continuations attach to the base cell |
| `Emoji_Presentation=Yes` is two columns | done — regional indicators are EAW `N`, so a flag measured from East Asian Width alone is half-width |
| DECRQM `?2027` — grapheme clustering | done — answers `3`, permanently set; baud has no mode that stops clustering |
| `ESC % G` — select UTF-8, ISO 2022 | done — satisfied; baud already decodes UTF-8 |
| `ESC % @` — select ISO 8859-1 | done — switches the live input encoding |
| `CSI ? 1020 $ p` — DECRQM on xterm's `utf8` resource | done — mirrors the **live** encoding, `1` or `2`, as xterm does |
| `ESC SP F` — S7C1T, send 7-bit controls | done — the default |
| `ESC SP G` — S8C1T, send 8-bit controls | done — replies switch to `9B` / `90` / `9C` |
| GR bytes `A0-FF` treated as GL `20-7F` inside a sequence | **out** — same reason; they are UTF-8 lead and continuation bytes |
| ISO 8859-1 input, switched by `ESC % @` / `ESC % G` | done — `InputEncoding`; one byte, one character, no decoder state |
| encoding detection | done — `InputEncoding::detect` **reports**; it never switches the terminal, and returns `None` when the sample cannot decide |
| National replacement sets and other legacy charsets | **out** — no escape sequence in scope selects them, and `iconv` transcodes once outside the ingest loop; `InputEncoding` is where they would go |

`S7C1T`/`S8C1T` are about what the terminal **sends**, and both are implemented:
"this tells the terminal to send C1 control characters as 8-bit sequences, e.g.,
its responses to queries".

**Sending and accepting are different axes, and conflating them is a mistake
this document made first.** S8C1T was refused here on the grounds that emitting
`9B` would make baud's own output invalid UTF-8. That reasoning does not hold: a
reply is not UTF-8 text, it is a control stream the application asked for by
sending S8C1T, and an application that asks is prepared to read it. baud still
cannot *parse* an 8-bit introducer — that half is forced by UTF-8 and stands —
but there was never a reason it could not *emit* one. `C1Transmit` carries the
choice and CSI, DCS and ST introducers all follow it.

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

> **Open decision, under active research.** The paragraph below states the
> position baud holds *today* and the reasoning behind it. Accepting other input
> encodings — with detection — is being reconsidered deliberately, and the
> literature and terminal survey for it will land in
> `docs/research/notes/`. Do not treat this as settled; `Encoding` in
> `baud-domain/src/store.rs` is already the extension point, and its
> `is_byte_per_column` predicate is written around "fixed-width single-byte
> encoding", not around ASCII, for exactly this reason.

**Grapheme clustering is out, and it is the largest thing on this page that
is.** `[[source: uax29]]` defines the extended grapheme cluster — the unit a
*user* would call a character — and baud stores one codepoint per cell. The
consequence is arithmetic, not opinion: `U+1F469 U+200D U+1F4BB` is one cluster
and one glyph, and baud gives it **four columns**, because the two emoji are
each East Asian Wide and the joiner is zero. A combining sequence longer than
one mark has the same shape.

**Combining marks used to be discarded outright.** `put_cp` returned early on
`Width::ZERO`, so `e` + `U+0301` stored `e` and the accent was gone — not
mis-measured, *absent*, and unrecoverable by a renderer, a selection or a copy.
A terminal must not lose a character it was sent. They are stored now, keyed on
the cell they attach to, in a bounded table that evicts on collision for the
reason the style and link tables do: the key is attacker-supplied, and a page
can emit as many marks as it likes. `Term::marks` is what a renderer reads;
`marks_dropped` and `marks_evicted` are how the bound stays visible.

**And the same table answers full cluster semantics, which needed no cell bits
at all.** The reasoning that said otherwise was looking at the wrong
constraint: a continuation does not need the *cell* to identify its cluster, it
needs somewhere to live, and the marks table already is that. `put_cp` asks
`ClusterBreak` whether a codepoint continues what came before it, and a
continuation attaches to the base cell instead of taking one of its own. So
`U+1F469 U+200D U+1F4BB` is two columns rather than four, and a flag is two
rather than four.

One thing that fell out of it: **regional indicators are East Asian Width `N`**
— one column — while carrying `Emoji_Presentation=Yes`. A flag measured from
EAW alone is half as wide as every terminal draws it, and the fix is a property
lookup rather than a special case. This is the question the deleted
`EmojiWidth` config was gesturing at, answerable now only because the cluster's
base is known.

**The segmentation half is done and measured; full cluster storage is not.**
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

That is a real cost and not a disqualifying one. What still has no number is
**storage**: `PackedCell` is eight measured bytes and a cluster does not fit, so
it means a side table keyed per cell — the shape `OSC 8` links already use, with
the index riding in spare `CellFlags` bits — and the cost of that against the
working-set rule is untested. That is the remaining blocker, and it is now
stated precisely rather than as "unmeasured" covering both halves.

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

`80..9F` stay ignored in Latin-1 rather than printed: ISO 8859-1 reserves them
for C1 controls, which baud does not act on, so printing them would invent
characters the application did not send. That is xterm's `CASE_IGNORE`, and
what ghostty does citing xterm.

**The rest of the legacy charsets remain out.** baud already
has the model for the one charset mechanism it does implement: `SCS`
(`ESC ( 0`) sets `CellFlags::GRAPHICS` and the cell keeps the byte, so a
renderer with the DEC table loses nothing and baud never guesses a glyph. It
annotates rather than translates. Byte-level transcoding would put a table
lookup on the per-byte ingest path, against the rule that the working set stays
in cache — and `iconv` already does it, once, outside the hot loop.

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
a DCS prefix, so `3A` follows williams unmodified and ends the sequence.

### C0 inside a control string payload is ignored, not stored

`osc_string` takes the *ignore* action on `01-17,19,1C-1F`; only `20-7F` is
`osc_put`, and the same holds for `dcs_passthrough`. baud appended every byte
that was not a terminator, so a raw control byte landed in the title — and OSC
payloads are attacker-supplied, which is why this one is in the threat model
rather than only in the conformance table.

**This block broke chunk-invariance the moment it landed, and the property
caught it.** `fast_esc` scans an OSC straight to its terminator, so it could not
see a cancel in between while the state machine could — the same bytes set a
title or did not, depending on whether they fit inside one `read(2)`. The fast
path now declines any OSC payload containing a C0 byte at all — it cannot
reproduce either rule, since it scans to the terminator (so it cannot see a
cancel) and copies the payload wholesale (so it would keep bytes that should be
dropped).
`a_cancel_inside_an_osc_is_honoured_in_both_feeds` pins it deterministically
rather than leaving it to a saved seed.

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
| `0x0E` SO | shift out (G1) | done |
| `0x0F` SI | shift in (G0) | done |

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
| `ESC =` / `ESC >` | DECKPAM/DECKPNM | done |
| `CSI ? 2 l` | DECANM — enter VT52 | done — see [protocols/vt52.md](protocols/vt52.md) |
| `ESC # 8` | DECALN screen alignment | done |
| `ESC ( ` / `ESC ) ` / `ESC * ` / `ESC + ` | charset designate G0-G3 | done — mechanism; glyph tables blocked, see below |
| `ESC ( A` | UK NRCS designation | done — reported as `Charset::Uk`, **not aliased to ASCII** |
| `ESC ( C 5 H 7 K Q 9 R f Y Z 4 = ` E 6` | NRCS: Finnish, Swedish, German, French Canadian, French, Italian, Spanish, Dutch, Swiss, Norwegian/Danish | done — each named as its own `Nrcs`; recorded, not translated |
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
| `CSI J` | ED erase in display | done |
| `CSI K` | EL erase in line | done |
| `CSI L` | IL insert lines | done |
| `CSI M` | DL delete lines | done |
| `CSI P` | DCH delete chars | done |
| `CSI S` | SU scroll up | done — **and it now moves the grid**; see CLAUDE.md on the ring advance |
| `CSI T` | SD scroll down | done |
| `CSI X` | ECH erase chars | done |
| `CSI Z` | CBT backward tabulation | done |
| `CSI \`` | HPA char position absolute | done |
| `CSI a` | HPR char position relative | done |
| `CSI b` | REP repeat preceding char | done |
| `CSI c` | DA device attributes | done — identifies as VT102 |
| `CSI d` | VPA line position absolute | done |
| `CSI e` | VPR line position relative | done |
| `CSI g` | TBC tab clear | done — 0 and 3; others inert, not aliased |
| `CSI h` | SM set mode | done — LNM and IRM |
| `CSI l` | RM reset mode | done — as SM |
| `CSI m` | SGR character attributes | done — see SGR below |
| `CSI n` | DSR device status report | done — 5 and 6; others unanswered, not guessed |
| `CSI r` | DECSTBM scroll region | done |
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
| `?5` | DECSCNM reverse video | done — `Term::reverse_video()` |
| `?6` | DECOM origin | done |
| `?7` | DECAWM autowrap | done |
| `?8` | DECARM auto-repeat | done — `Term::auto_repeat()` |
| `?12` | cursor blink | done — the blink axis of DECSCUSR, reached by another route |
| `?25` | DECTCEM cursor visible | done |
| `?45` | reverse wraparound | done |
| `?47` | alternate screen, pre-1049 | done — switches buffers, does **not** save the cursor |
| `?69` | DECLRMM left/right margins | done |
| `?1034` | meta sends escape | done — `Term::meta_sends_escape()` |
| `?1001` | highlight tracking | **out** — "requires a cooperating program, else xterm will hang" |

### SGR

Enumerated against `[[source: ecma48]]` § 8.3.117 and, for the extensions,
`[[source: ctlseqs]]` § "Character Attributes".

| parameters | status |
|---|---|
| 0 reset | done |
| 1, 2 bold/dim · 22 neither | done |
| 3 italic · 23 not | done |
| 4 underline, 21 double · 24 not | done — one underline bit, so double is an underline |
| 5, 6 blink · 25 steady | done — no timer, so rapid and slow are the same bit |
| 7 reverse · 27 positive | done |
| 8 conceal · 28 reveal | done |
| 9 strike · 29 not | done |
| 30-37, 39 foreground · 40-47, 49 background | done |
| 90-97, 100-107 aixterm bright | done — palette indices 8..15 |
| 38/48 `;5;n` 256-colour, `;2;r;g;b` truecolour | done |
| 38/48 `:5:n` / `:2:Pi:r:g:b` sub-parameter form | done — both the six-part form with a colour-space id and the five-part form without |
| 10-19 fonts, 20 Fraktur, 26, 50-65 | **out** — no renderer consumes them, and a bit set and never read is the failure this table exists to catch |

Every SGR parameter above is **done** or **out**; none is partial. `CSI m` used
to be listed as `partial` on the strength of that last row, which conflated two
different things. `out` is a scope decision with a reason — storing a font
selection no renderer can read would be a bit set and never used, which is the
failure this table exists to catch. `partial` means the row claims something it
does not do. Nothing here does.

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
| `CSI ! p` | DECSTR soft reset | done |
| `CSI SP q` | DECSCUSR cursor style | done |
| `CSI " q` | DECSCA character protection | done |
| `CSI ? J` | DECSED selective erase display | done |
| `CSI ? K` | DECSEL selective erase line | done |
| `CSI $ p` | DECRQM request mode | done |
| `CSI s` (DECSLRM) | left/right margins | done — with DECLRMM (`?69`) |
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
| `OSC 0/1/2` | window/icon title | done |
| `OSC 4` | set colour palette | done — including the `?` query |
| `OSC 8` | hyperlinks | done — bounded, evicting link table |
| `OSC 52` | clipboard | consumed; **read deliberately refused** — see security note |
| `OSC 104` | reset palette | done |
| `CSI ? 1049 h/l` | alternate screen buffer | done |
| `CSI ? 2004 h/l` | bracketed paste | done — recorded and exposed as `Term::bracketed_paste()` |
| `CSI ? 1000/1002/1003 h/l` | mouse tracking | done |
| `CSI ? 1006 h/l` | SGR mouse encoding | done — the only encoding, see below |
| `CSI ? 1001 h/l` | highlight tracking | **out of scope** — "requires a cooperating program, else xterm will hang" |
| `CSI ? 2026 h/l` | synchronised output | done — exposed as `Term::synchronized_output()` |
| `CSI ? 25 h/l` | DECTCEM cursor visibility | done — state exposed via `cursor_visible()` |

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
