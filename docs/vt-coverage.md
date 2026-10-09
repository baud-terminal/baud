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
| **partial** | implemented but known-incomplete; the gap is named in the row. Three rows carry it: DECSC, DECRC and the DEC Special Graphics glyph table |
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

**No row carries `parsed` or `todo`. Three carry `**partial**`, and
`no_row_is_unfinished` in `baud-core/tests/coverage.rs` does not see them.**
This paragraph said no row carried any of the three and that the test fails if
one ever does. The test matches a status cell that starts with the bare word,
and these start with `**`, so it passes with DECSC, DECRC and the DEC Special
Graphics glyph table all at `**partial**`. A check that saw them would fail on
all three, and that is the right answer: each row names work that is not
finished. `out` is the only status that
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
| 7-bit `ESC Fe` forms `ESC P`, `ESC [`, `ESC \`, `ESC ]`, `ESC X`, `ESC ^`, `ESC _` | done — `[[source: ecma48]]` § 5.3 specifies these as the representation of C1 in a 7-bit environment, so this *is* the C1 interface, not a substitute for one |
| 8-bit C1 introducers `90`, `9B`, `9C`, `9D`, `98`, `9E`, `9F` — **in UTF-8** | **out** — they are continuation bytes there; a stray one is a decode error and renders U+FFFD. Follows ghostty; diverges from alacritty and wezterm |
| the same introducers under `ESC % @` (ISO 8859-1) | done — no multi-byte sequence exists for them to be part of, so `9B` is unambiguously CSI |
| 8-bit `84` IND, `85` NEL, `88` HTS, `8D` RI under `ESC % @` | done — each acts as its 7-bit form, `[[source: ctlseqs]]` pairing the two, at every read split (`eight_bit_ind_nel_hts_and_ri_match_their_seven_bit_forms`), and none inherits the last escape's intermediate (`a_c1_control_does_not_inherit_the_last_escapes_intermediate`). Until 2026-10-08 all four were counted as unsupported, and `84` after `ESC ( 0` replaced G0. `8E` and `8F` map to `ESC N` and `ESC O`, counted as those are until note AE's single shifts are built |
| character width from East Asian Width + general category, generated from pinned Unicode data | done — `AmbiguousWidth` is embedder policy, `[[source: uax11]]` § 5 |
| UAX #29 extended grapheme cluster segmentation | done — `ClusterBreak`, and all **766** cases of Unicode's own conformance suite pass |
| combining marks stored against their base cell | done — `Term::marks`, bounded and evicting; they used to be **discarded** |
| one cluster per cell — ZWJ sequences, flags and Indic conjuncts as a single cluster | done — `ClusterBreak` consulted in `put_cp`; continuations attach to the base cell |
| `Emoji_Presentation=Yes` is two columns | done — regional indicators are EAW `N`, so a flag measured from East Asian Width alone is half-width |
| DECRQM `?2027` — grapheme clustering | done — answers `3`, permanently set, and it is true: a `U+FE0F` after a base `[[source: emoji-variation-sequences]]` lists widens it to two cells (keycap bases excepted); configured to one cell it answers `4`; no sequence changes it |
| `ESC % G` — select UTF-8, ISO 2022 | done — satisfied; baud already decodes UTF-8 |
| `ESC % @` — select ISO 8859-1 | done — switches the live input encoding |
| `CSI ? 1020 $ p` — DECRQM on xterm's `utf8` resource | done — mirrors the **live** encoding, `1` or `2`, as xterm does |
| `ESC SP F` — S7C1T, send 7-bit controls | done — the default |
| `ESC SP G` — S8C1T, send 8-bit controls | done — replies switch to `9B` / `90` / `9C` |
| GR bytes `A0-FF` treated as GL `20-7F` inside a sequence | **out** — same reason; they are UTF-8 lead and continuation bytes |
| ISO 8859-1 input, switched by `ESC % @` / `ESC % G` | done — `InputEncoding`; one byte, one character, no decoder state |
| encoding detection | done — `InputEncoding::detect` **reports**; it never switches the terminal, and returns `None` when the sample cannot decide |
| Translating the national replacement sets | done — 14 sets, 162 substitutions, from `[[source: xterm-charsets]]`; the run records which set it was written under so promotion cannot change it |

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

**Combining marks used to be discarded outright.** `put_cp` returned early on
`Width::ZERO`, so `e` + `U+0301` stored `e` and the accent was gone — not
mis-measured, *absent*, and unrecoverable by a renderer, a selection or a copy.
A terminal must not lose a character it was sent. They are stored now, in
`ClusterStore`: **keyed on the cluster's own content**, with its index riding in
the cell's `CellFlags`, so it moves with the cell through every rotation,
reflow and screen switch. ~~Keyed on the cell they attach to, in a table that
evicts on collision~~ was `baud-core2`'s design, and one `viewport()` call made
it lose a cell's marks (CLAUDE.md, "The cluster store is content-keyed, and the
index rides in the cell"). It is bounded at 63 clusters and **refuses** rather
than evicts, because a wrong cluster puts somebody else's accent on your
character; repeats share a slot. `Term::cluster_marks` is what a renderer
reads, and `marks_dropped` counts the refusals.

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
HTS and RI act, and the rest are counted as their 7-bit forms are. This said
they "stay ignored" because baud did not act on them, citing xterm's
`CASE_IGNORE`; they were in fact counted, IND, NEL, HTS and RI included.

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
a DCS prefix, so `3A` follows williams unmodified and ends the sequence.

### The parameter limit is 35, and it is derived rather than chosen

A CSI may carry more parameters than a fixed array holds, and discarding the
excess is a legitimate limit -- every terminal has one, and xterm's `NPARAM` is
30. What is not legitimate is the two feeds discarding it *differently*.

They did. `fast.rs` accumulates a parameter into a local and stores it only
under the capacity bound, so an over-capacity parameter was parsed and dropped.
`machine.rs` wrote each digit straight into `params[nparam - 1]` with `nparam`
pinned *at* capacity, so the separator was swallowed and the next parameter's
digits appended to the last stored one: `CSI ...;61;62;63 m` stored 6263 where
the fast path stored 62. The result therefore depended on where a read boundary
fell, which is a chunk-invariance bug. core2's ingest property did not find
it because its generator does not emit sequences that long.

| | one read | byte at a time |
|---|---|---|
| `CSI 0;0;...;0;1;4 m` | `BOLD` | nothing |
| `CSI 1;2;...;63;64 m` | `0xffff` | `0x7fff` |

`ParamIdx::next_capped` now saturates one *past* `CAPACITY`, so "capacity
reached" and "capacity exceeded" stay distinguishable and the digit branch knows
to stop; `Term::nparams` clamps on the way out. See `tests/sgr/param_overflow.rs`.

The capacity itself is `1 + Attrs::WIRE.len() + 1 + 3 * 5` = **35**, which is
the longest parameter list baud emits: the DECRQSS SGR reply. Since DECRQSS
answers with something the application can send back verbatim, a reply the
parser cannot re-read is a reply that does not round-trip -- and at 16 it could
not. Bold with a foreground, a background and an underline colour already
answers with seventeen parameters, which is an editor drawing an undercurl, not
a corner case. Deriving the constant rather than writing a number is what stops
it going back out of step the next time `WIRE` grows, which is how it drifted in
the first place.

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
| `ESC 7` | DECSC save cursor | **partial** — `[[source: vt510-decsc]]` lists seven items and `baud-core` saves five: cursor position, SGR attributes, the wrap flag, origin mode, and the selective erase attribute. The character set is saved as the one slot this core has rather than G0-G3 across GL and GR, and **any SS2/SS3 in flight is not saved** because single shifts are not implemented. The row said `done` on the strength of the first two until the manual was read rather than assumed |
| `ESC 8` | DECRC restore cursor | **partial** — restores what DECSC saved, clamped to the grid that exists now. `a_cursor_restore_brings_back_the_wrap_flag_and_origin_mode` pins the two flags that were silently dropped |
| `ESC D` | IND index | done — column preserved (was resetting it); scrolls the region at its bottom and nothing on the last row below it, as DECSTBM's row says |
| `ESC E` | NEL next line | done |
| `ESC M` | RI reverse index | done |
| `ESC c` | RIS full reset | done — the power-on state from either screen, and it **empties the history**. xterm's `CASE_RIS` clears its saved lines where `DECSTR` keeps them (`[[source: xterm-reset]]`), and alacritty (`[[source: alacritty-reset-state]]`, `[[source: alacritty-grid-reset]]`) and foot (`[[source: foot-vt-ris]]`, `[[source: foot-term-reset]]`) do the same; the Linux console (`[[source: linux-vt-ris]]`) and sfte (`[[source: sfte-ris]]`) keep it. The whole primary ring is erased with the rotation kept, so a reset scrolls nothing and leaves nothing a reader of the ring or a later re-cut can bring back (`a_full_reset_clears_the_history_and_the_next_lines_refill_it`, `a_full_reset_leaves_nothing_hidden_below_the_window`, `a_full_reset_leaves_nothing_of_the_old_text_in_the_ring`, `a_full_reset_scrolls_nothing`, the captured `reset(1)` in `a_captured_reset_leaves_no_history`); it leaves the alternate screen before the soft reset, so what `?1049 h` saved does not survive it (`a_hard_reset_on_the_alternate_screen_leaves_the_power_on_state`), and frees the `OSC 8` table (`a_full_reset_frees_the_link_table`). `DECSTR` keeps the history (`a_soft_reset_keeps_the_history`). It left the counts and reset the rotation until 2026-09-23, so the scrollback read rows the reset never wrote |
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
| `CSI J` | ED erase in display | done |
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
| 4 underline, 21 double, **4:1..4:5** styled · 24 / 4:0 not | done — five styles, exactly one in force: straight, double, and `[[source: kitty-underlines]]`'s curly, dotted and dashed, each its own `Attrs` bit and drawn by the software compositor and the cell shader. A `4:n` *replaces* the family and so does a bare 4 (the spec's backward-compatible 4:1); 24 and 4:0 clear all five; an unknown n clears rather than aborts the walk. **4:3 is not 4;3** -- sfte flattens the colon and gets underline-plus-italic, measured -- and DECRQSS reports the three colon styles as 4:n so the reply round-trips. `each_colon_style_sets_exactly_its_own_bit`, `the_colon_form_is_not_the_semicolon_form`, `a_style_replaces_the_family_and_4_0_clears_it`, `a_colon_underline_style_round_trips` |
| 5, 6 blink · 25 steady | done — no timer, so rapid and slow are the same bit |
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
| `CSI ! p` | DECSTR soft reset | done — resets the DEC-defined state and **not** the alternate screen, `alt_saved`, mouse tracking or its encoding. Those are xterm extensions, so a DEC list cannot mention them and xterm decides: in `ReallyReset` all four are inside the `if (full)` RIS branch, `FromAlternate` next to `ClearScreen` (`[[source: xterm-reset]]`). baud reset them until 2026-08-27, so a full-screen application recovering with `CSI ! p` was dropped onto the primary screen and lost what `?1049 h` had saved. **Of the mode word it restores only what xterm's soft reset restores**: DECTCEM, DECLRMM, DECOM, DECCKM, DECAWM, reverse wraparound and IRM, and releases `?2026`, which xterm lacks; LNM, `?4`, `?5`, `?8`, `?1004`, `?1034`, `?1036`, `?1039` and `?2004` stay, as they do there. It restores the keypad to numeric and `modifyOtherKeys` to its first value, which xterm does on both resets and RIS here did not. Until 2026-09-24 it restored the whole mode word, so a program recovering with `CSI ! p` lost its bracketed paste (`decstr_restores_the_modes_xterm_restores_and_keeps_the_rest`, `decstr_and_ris_restore_the_keypad_and_modify_other_keys`). It clears every `OSC 4` override and keeps the dynamic colours, as xterm's common path does (`both_resets_clear_every_palette_override`, since 2026-09-29). See `tests/xterm/soft_reset_scope.rs` |
| `CSI SP q` | DECSCUSR cursor style | done — the shape and the blink an application asks for, and **drawn with its blink** since 2026-09-25: on macOS at the system caret's rate since 2026-09-27 -- `NSTextInsertionPointBlinkPeriodOn` and `...Off`, AppKit's 650 ms on and 350 off where they are unset -- and held steady by macOS 15's non-blinking switch unless `[cursor] blink = true`, which changes the drawing and not the answer; on Linux xterm's 600 ms on and 300 ms off (`cursorOnTime`/`cursorOffTime`, `[[source: xterm-reset]]`'s file); only while the window has focus and restarted shown by output or a key -- foot's rules (`[[source: foot-term-reset]]`'s file) -- on the display link's clock or the Linux `poll` timeout (`baud-app/src/caret.rs`, `baud-render/tests/cursor_blink.rs`, the pixel oracle's off-phase scene). The power-on style is the embedder's (`Term::set_initial_cursor_style`, `[cursor] shape` and `blink`, a blinking block by default), and `Ps = 7`, `DECSTR` and `RIS` restore it, as xterm restores its resources. **The sources conflict on `Ps = 0`, and baud follows foot**: `[[source: ctlseqs]]` and xterm's `charproc.c` make it a blinking block, foot's `csi.c` resets it "to configured default" (`[[source: foot-csi]]`'s file). The two agree while the configured style is the default; `Ps = 1` stays the blinking block (`zero_and_seven_restore_the_configured_style_and_one_is_the_blinking_block`, `both_resets_restore_the_configured_style`). xterm's `cursorBlinkXOR`, which inverts an application's blink against the resource, is not followed. **The style is saved with the cursor at `?1049 h` and restored at `?1049 l`**, as kitty and wezterm do, and not by `DECSC`, which `[[source: vt510-decsc]]` does not list it for: helix ends its session on the alternate screen with `0 q`, the system entry's `Se` (`2 q`) and `cnorm`, and the shell gets its own cursor back. `DECRQSS` answers the style DECSCUSR set, not what `?12` draws |
| `CSI " q` | DECSCA character protection | done |
| `CSI ? J` | DECSED selective erase display | done |
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
| `OSC 52` | clipboard | done — **read deliberately refused**, and both halves counted in `clipboard_reads_refused` / `clipboard_writes_dropped`; see security note |
| `OSC 133` | semantic prompt marks | done — `A`/`P` open the prompt zone, `B` keeps it, `C` closes it; acted on by `Term::resize`, which does not re-cut the rows a shell will redraw, and counted in `semantic_marks`. **Each screen has its own**: the zone and the block start are swapped by `?1049` as the kitty stacks are, the alternate screen's start fresh on each entry, and the primary's zone alone decides whether the primary re-cuts (`a_mark_on_the_alternate_screen_does_not_redraw_the_primarys_block`, `a_block_opened_on_the_alternate_screen_is_not_redrawn_on_the_primary`, `a_mark_on_the_alternate_screen_does_not_stop_the_primary_re_cutting`); until 2026-09-23 one of each served both screens, and a mark on one was read against the other's ring. `D`'s exit status is **not** parsed, because nothing here would read it |
| `OSC 104` | reset palette | done |
| `CSI ? 1049 h/l` | alternate screen buffer | done |
| `CSI ? 2031 h/l` | colour palette update notifications | done — since 2026-09-29. Reset at power-on and DECRQM answers the bit, so neovim's handshake subscribes (`neovims_handshake_subscribes_and_re_queries`); **nothing is sent on setting it**, as tmux learned the hard way. While set, an embedder colour change that moves the dark/light class sends one `CSI ? 997 ; 1\|2 n`: `Term::set_theme` and `Term::set_default_colors` compare the class before and after the whole change, so a class crossed inside one call is not reported (`a_class_crossed_inside_one_call_is_not_reported`). An application's own colour escapes send nothing and change the next `?996 n` answer. `RIS` clears it; `DECSTR` keeps it -- baud's rule, since xterm has no `?2031` (`ris_clears_the_mode_and_decstr_keeps_it`). The macOS app's OS light/dark flip under `theme = "auto"` reaches it through `tell_defaults` with `Overrides::Drop`, dropping the application's `OSC 4`/`10`/`11`/`12` colours -- only when the resolved appearance changes, since AppKit also calls back for accessibility variants of the same one (`palette_switch`, `a_flip_drops_the_applications_colours_and_reports_once`) |
| `CSI ? 2048 h/l` | in-band resize notifications | done — reports `CSI 48 ; rows ; cols ; height_px ; width_px t` on enablement, on re-enablement, after every resize, and **after a cell-size change even when the grid did not move**, carrying what the terminal **has** rather than what was asked for. The pixel fields are the text area -- the grid times the cell size `Term::set_cell_size` was given -- and zero until one is given, which `[[source: inband-resize]]` defines as the answer of a terminal that cannot report them; "A terminal MUST report pixel sizes if it is capable", and "Font size changes can also affect the text area size", which is why a zoom reports. Until 2026-09-22 the fields were always zero. `the_inband_report_carries_pixels_once_the_cell_is_known`, `a_cell_size_change_reports_even_with_the_grid_unchanged` |
| `CSI 14 t`, `CSI 16 t`, `CSI 18 t` | XTWINOPS text-area and cell size | done — `CSI 4 ; h ; w t` (text area in pixels), `CSI 6 ; h ; w t` (one cell in pixels) and `CSI 8 ; rows ; cols t` (text area in cells), in `[[source: ctlseqs]]`'s shapes, height first. **The two pixel answers need the cell size and are silence and a count without it**, the way an unanswerable `DSR` is; the cell count is always answered. Only the exact one-parameter forms: `CSI 14 ; 2 t` is the *window* with its frame, which this core is never handed, and stays counted. This is the reply an image client sizes its pictures from -- yazi sends `CSI 16 t` in its detection burst and read nothing back before 2026-09-22, so it drew every Sixel at a fixed 600x900. `the_window_queries_answer_from_the_cell_size_and_the_grid`, `without_a_cell_size_only_the_cell_count_is_answered`, `yazi_reads_the_cell_size_from_its_detection_burst`, `every_other_t_is_refused_or_not_xtwinops` |
| `DCS Pa ; Pb ; Ph q ... ST` | Sixel graphics | done — decoded into caller memory with no external decoder (`[[source: vt3xx-sixel]]` is the raster grammar, `[[source: ctlseqs]]` the DCS envelope), placed at the cursor and drawn by all three render paths. **An image follows the row it was placed on**: scrolling into the history, a shorter screen retiring its row and a taller one pulling it back, a narrower screen re-cutting the lines above it, and a reader scrolled back to it. Its column is kept, and it is cropped at the edge rather than moved. It goes when its row does -- past the far end of the history, blanked by a reverse scroll, dropped by a reflow that did not fit -- and each is counted in `placements_expired`. **Each screen owns its images**: the shell's are not drawn over a full-screen program, and the program's are deleted when it leaves. Text over an image frees it. Gates: `baud-core/tests/sixel.rs` (decode and place, chunk invariance, refuse-and-count, text over an image), `baud-core/tests/image_anchor.rs` (every resize and scroll case, asserted by the text beside the image, including foot's blank-line trap and WezTerm's text-on-the-row trap), `baud-render/tests/image.rs` (a pinned frame digest, the one-row scroll, a view scrolled back after a shrink, the per-screen rule) and the pixel oracle. **It follows its row inside the screen too**: a scrolling region, `IL` and `DL` move rows without moving the window, so each moves the images on the rows it moves, and one whose row the operation deletes -- off the top of a region that copies, which is a middle region, or a top-anchored one on the alternate screen or under side margins with at least as many rows under it as in it, or past a region's bottom -- goes and is counted; a top-anchored region that rotates on the primary retires its top line, image and all, to the history; on the alternate screen, which keeps none, the image goes and is counted. A status line under a region keeps its image while the region scrolls, on either screen. **One stated gap**: an image anchored on a wrapped continuation row -- placed after text that wrapped onto its row -- comes back a row higher from a narrow-then-widen round trip, pinned by `an_anchor_on_a_wrapped_row_lands_a_row_higher_after_a_round_trip`; an image printed after a newline starts its line and round-trips exactly. |
| `CSI ? 2004 h/l` | bracketed paste | done — recorded and exposed as `Term::bracketed_paste()` |
| `CSI ? 1004 h/l` | focus reporting | done — `Term::focus` queues `CSI I` on gain and `CSI O` on loss only while the mode is set, and nothing otherwise, because an application that did not ask reads `CSI I` as a keypress. The macOS app polls the window's key status on the tick and before a mouse event and forwards a change (`Driver::poll_focus`), because the window crate's hook reports the *view's* first-responder status, which fired once at `show()` before the driver was installed and never on an app switch -- so **no transition was ever reported on macOS until 2026-09-20**, found by the OSC 8 live check, and a live check reads `CSI I`, `CSI O`, `CSI I` around an app switch now. The Linux app calls it from its keyboard hook. **Dropped uncounted until 2026-09-20**: `1004` was named nowhere in the code and had no row here, while `vim-unicode.raw` sends it on its first line; `set_private_mode`'s wildcard counts now, as the ANSI table's always did. `gain_sends_csi_i_and_loss_sends_csi_o`, `nothing_is_sent_unless_the_application_asked`, `the_vim_recording_enables_it` |
| `CSI ? 1000/1002/1003 h/l` | mouse tracking | done |
| `CSI ? 1006 h/l` | SGR mouse encoding | done — no ordinate limit; see below |
| X10 mouse encoding (`ESC [ M Cb Cx Cy`) | the default, what `?1000` gives without `?1006` | done — positions past 223 refused and counted, not truncated |
| `CSI ? 1001 h/l` | highlight tracking | done — recorded; see the mode table. The "xterm will hang" warning describes an event loop, and `baud-core` has none |
| `CSI ? 2026 h/l` | synchronised output | done — exposed as `Term::synchronized_output()` |
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

### An operation is done on *both screens*, or it is not done

`EL` and `ED` sat at **done**, with tests, and did **nothing at all** on the
alternate screen in `OpMode::Runs`. `erase_line`'s span-truncation fast path
read the `promoted` flag raw; that flag belongs to the primary screen, so on
the alternate one it was false for every row, the erase took the span path, and
it returned without touching a cell. `HELLO WORLD` survived an erase-to-end
from column 6.

Every erase test ran on the primary. So did every scrolling test — full-screen
scrolling in the alternate screen was copying rows for the same reason, at 26x
the cost.

**The enumeration was complete in the wrong dimension.** This file lists the
operations and `coverage.rs` checks each is mentioned; neither asks *in which
state* it was exercised. The alternate screen is not an operation and so has no
row here, but it changes the behaviour of the ones that do — it is where the
run table stops applying, and every full-screen application spends its whole
life there.

Two things came from that and both are gates rather than notes:

* `erases_on_the_alternate_screen_are_not_swallowed_by_the_span_path` runs six
  erases in both op modes on the alternate screen. The mode pair is the point:
  `Computed` was always right, so a fix that broke it to satisfy `Runs` shows
  up instead of passing.
* `baud-core/tests/xterm/alt_screen.rs` requires every function touching the
  run table to be screen-aware. It is structural because the state is
  **invisible**: in `Runs` nothing renders spans, so a wrongly-zeroed run count
  cannot be seen through any public accessor, and the first attempt at a
  behavioural test reproduced the harness trap instead — every case came back
  empty, including one that did nothing.

The reviewer's question for a new operation is therefore not only "is there a
test" but "does it run where applications will run it": both op modes, and the
alternate screen as well as the primary.
