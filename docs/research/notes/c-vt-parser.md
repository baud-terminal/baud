# C — the terminal parser as specified

*Group C of `docs/research/sources/MANIFEST.toml`. Primary source:
`[[source: williams]]`, Paul Flo Williams' state diagram for the DEC ANSI
parser, which is the de-facto specification every modern terminal is written
against.*

This note exists to answer one question: **baud implements Williams' machine and
then adds a state he does not have. Why is that not a bug?**

`docs/vt-coverage.md` asserts the answer in one sentence and cites this file for
it. For most of the project's life this file did not exist, so the assertion
rested on nothing.

## What baud takes

All thirteen states, unmodified in structure: `ground`, `escape`,
`escape_intermediate`, `csi_entry`, `csi_param`, `csi_intermediate`,
`csi_ignore`, `dcs_entry`, `dcs_param`, `dcs_intermediate`, `dcs_passthrough`,
`dcs_ignore`, `osc_string`, `sos_pm_apc_string`.

Three of Williams' rules earn their keep repeatedly and are worth restating
because each has already caught a real bug here:

1. **A control string must be consumed, never executed.** `sos_pm_apc_string`
   not existing was a security bug: `ESC _ … ESC \` fell through to `ground`
   and its payload ran as commands, so a stream could hide `CSI 2 J` inside an
   APC string and clear the screen.
2. **A bare `ESC` cancels a control string.** Not "ESC begins a terminator" —
   *cancels*. An APC test here asserted the stronger, wrong property until the
   source was read, and two DCS tests repeated the mistake later. The guarantee
   is precisely that a payload containing no `ESC` is inert.
3. **`ESC \` is the only string terminator.** Code that assumed the byte after
   `ESC` was the backslash, without checking, swallowed a NEL — a
   chunk-invariance bug that predated the state-machine work.

## What baud adds, and why

A fourteenth state, `Utf8 { remaining, acc }`.

**Williams' parser is defined over bytes and predates UTF-8 as terminal
practice.** In it, every byte ≥ `0x80` is `ground`'s "print" action. That was
correct for a VT100: `0x80..=0xFF` was GR, one byte per glyph. It is wrong for
a terminal that receives UTF-8, where a single character arrives as two to four
bytes that must be accumulated before anything can be printed.

Three ways to handle that, and baud takes the third:

| approach | why not |
|---|---|
| Decode before the state machine | Needs a complete character to exist before parsing, so it cannot handle a character split across a `read(2)`. The kernel splits wherever it likes. |
| Decode inside the `ground` print action, buffering in a side channel | Works, but the buffer is parser state that the state machine does not know about — so "what state am I in" stops being answerable from the state alone, which is the property the machine exists to provide. |
| **A state** | The partial character *is* parser state, so it is modelled as one. Chunk-invariance then follows from the same argument as every other split sequence, and is tested by the same property. |

The cost of getting this wrong was measured, not hypothesised: an earlier
version advanced the cursor once per continuation byte, so a 3-byte CJK
character advanced three columns instead of two and corrupted every position
after the first non-ASCII character on a line. Both operands were `usize`, so it
compiled.

## What this does not claim

- Not that Williams is wrong. His parser is correct for what it specifies —
  a byte-oriented DEC terminal. UTF-8 is outside its scope, not contrary to it.
- Not that the extra state is free. It adds a branch to the `ground` hot path.
  The domain-type port that introduced it measured +5.0%, −0.4%, −3.4%, −0.9%
  across four PTY runs, ranges overlapping throughout — so: no measured cost,
  which is a weaker claim than "free".
- Not that grapheme clusters are handled. They are not: `Width` is computed per
  codepoint and should be per cluster. A variation selector changes the width of
  the *preceding* codepoint, and a ZWJ family sequence is five codepoints
  rendering as one double-width glyph. That is an open gap, recorded in the
  README's "Not done".

## Claims relied on elsewhere

- `docs/vt-coverage.md` Tier 0: "all thirteen states" plus the `Utf8` addition.
- `baud-core/tests/vt_parser_states.rs`: the cancel and terminator rules.
- `baud-core/tests/properties.rs::ingest_is_chunk_invariant`: the property the
  state-as-state choice exists to make provable.
