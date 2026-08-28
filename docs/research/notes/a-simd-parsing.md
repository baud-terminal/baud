# SIMD parsing — the decoder is the part with a known better algorithm

> **Group:** A · **Sources:** `[[source: utf8-lookup]]`, `[[source: simdjson]]` · **Status:** partial

## Why this note exists, and why now

Group A has been declared since the corpus was set up with `simdjson` in it and
**no note**. It got one because a measurement asked for it rather than because
the group looked empty.

Profiling the gradual erosion between `2d23dc8` and `9f6ddc3` on multibench —
the first time that range had been measured on a harness that can resolve it —
the largest single step is `036a432`, *"Reject overlong UTF-8, and stop error
recovery eating a valid successor"*:

| commit | spans, streaming |
|---|---|
| `2d23dc8` | 1660.8 / 1690.1 |
| `43f91d4` VT52 and nine private modes | 1672.3 / 1647.5 |
| `8b6b3c3` williams' anywhere block | 1647.6 |
| **`036a432` reject overlong UTF-8** | **1567.1 / 1570.7** |
| `a4cecba` width tables from Unicode data | 1553.3 / 1524.5 |
| `931906a` UAX #29 clustering | 1561.1 |

**That 5% is almost certainly not decoder work, and this note said it was.**
The first draft argued "unlike the width tables this is not a layout ghost,
because an ASCII byte goes through the decoder on every pass". It does not.
`Term::blit` hands `&arena[..]` straight to `View::put_ascii_run`, which writes
cells from raw bytes -- no `Codepoint`, no `Utf8Acc`, no decoding at all -- and
that is the path 95% of streaming bytes take. The decoder sees the other 5%.

So `036a432` looks like the same layout effect as the width tables, and the
honest reading of the profile is that **no step in it has been shown to be work
in a function**. The check that settles it costs nothing and has not been run:
count how many bytes reach the decoder on each payload.

The literature below is still worth having, because the decoder *is* on the hot
path for text that is not ASCII -- the vim recording carries CJK and emoji --
and because knowing the algorithm's price is what makes the size of the
opportunity a measurement rather than a guess. But it is not the explanation
for this 5%, and the two must not be welded together.

## What the literature says that work costs

`[[source: utf8-lookup]]` Table 13, retired instructions per byte, AMD Rome:

| validator | ASCII | 1–2 B | 1–3 B | 1–4 B |
|---|---|---|---|---|
| branchy | 6.0 | 11 | 12 | 12 |
| branchy-ascii | 0.75 | 16 | 17 | 16 |
| **finite-state** | **7.0** | 7.0 | 7.0 | 7.0 |
| **lookup** | **0.21** | 0.97 | 0.97 | 0.97 |

**baud's decoder is the finite-state row.** `Utf8Acc`/`Utf8Min` carry decoder
state across bytes and the parser advances it one byte at a time, which is that
algorithm exactly — and the table says it pays 7.0 instructions per byte
whatever the input, including for ASCII, where the alternatives collapse.

The paper's own summary: the lookup algorithm "outperforms UTF-8 validation
routines used in many libraries and languages by more than 10 times using
commonly available SIMD instructions", and "in all our tests, irrespective of
the input, lookup requires fewer than one retired instruction per byte".

Its rules are the ones baud implements, named the same way: *Too Short*, *Too
Long*, *Overlong* — "the decoded character must be above U+7F for two-byte
characters, U+7FF for three-byte, U+FFFF for four-byte" — *Too Large*, and
*Surrogate*. `036a432` implemented those with a state machine; this is the same
correctness with a different instruction count.

## Why baud is unusually well placed to take it

* **The instruction class is already here.** The algorithm is built on
  `pshufb` / `vpshufb`, and `scan_avx2` already lives in the `_mm256`
  neighbourhood -- its movemask is the one point where an intrinsic's result
  enters the type system. The `Scan` trait and the `feed`/`feed_avx2` split are
  the structure a vectorised decoder would slot into.
* **The correctness bar is already met and tested.** Overlong rejection,
  surrogate rejection and the error-recovery rule are pinned by tests against
  the standard's own vectors. A replacement has an oracle to differ against
  rather than a specification to re-read.
* **ASCII is the common case and the one that gains most.** 0.21 against 7.0.

## What the scalar half already bought

`fast_utf8` decodes one complete sequence in place instead of paying three or
four `control` dispatches. Measured, non-overlapping: **btop +7.5%, vim +7.1%**
(`MEASUREMENTS.md`). That is not the lookup algorithm -- it removes
dispatch overhead, not decode instructions -- but it establishes the shape a
vectorised version needs: a place in `feed_with` where a run of text is handled
outside the state machine.

**The remaining prize is 0.97 against 7.0.** Those are `[[source: utf8-lookup]]`
Table 13's figures for multi-byte input, and they are the same algorithm as the
0.21 -- 0.21 is simply its cost when the input is ASCII. baud cannot reach 0.21
on CJK because multi-byte inherently costs more, but 0.97 against a
finite-state decoder's 7.0 is roughly sevenfold on the 14.5% and 28.2% of bytes
measured above.

Two things stand between here and there, and neither is fatal:

* **Lookup validates; baud must decode.** It answers well-formed or not, and
  baud needs the codepoint, the width and a cell. simdutf builds decoding on
  the same technique, so this is a matter of reading how rather than whether.
* **~~Block algorithms need runs.~~** Measured, by `scripts/utf8-runs.py`,
  over the checked-in recordings:

  | recording | mean run | p50 | p90 | multi-byte bytes | **in runs >=16** | in runs >=32 |
  |---|---|---|---|---|---|---|
  | btop | 15.5 | 10 | 29 | 76,032 | **96.9%** | 71.1% |
  | vim | 15.1 | 10 | 22 | 39,086 | **69.2%** | 48.3% |
  | asciiquarium | 4.4 | 4 | 7 | 0 | — | — |

  **The mean is the misleading statistic and the byte share is the honest
  one.** Runs average ~15 bytes, shorter than the ~32 that made vectorising the
  ASCII scan lose 12%. But multi-byte bytes are not spread evenly across runs;
  they *cluster in the long ones*, so 96.9% of btop's live in a run of at least
  sixteen. Counting runs says "too short", counting bytes-in-runs says the
  opposite, and the second is what a block decoder actually processes.

  The bar is also far lower than the ASCII case. That scan was replacing about
  one branch-predicted compare per byte; this replaces a decoder
  `[[source: utf8-lookup]]` puts at 7.0 instructions per byte.

* **Portability is not an obstacle, which was assumed rather than checked.**
  The algorithm needs `pshufb`; `core::simd`'s `swizzle_dyn` is that operation
  and lowers to NEON `tbl` on aarch64 and SSSE3 on x86-64. So this can be one
  portable implementation behind the existing `Scan` trait rather than a third
  entry point beside `feed` and `feed_avx2`, and -- unlike `scan_avx2` -- it
  would be *testable on the machine this project is developed on*.

## What this note does not claim

* **Nothing is implemented.** As with `[[source: dpfsm]]` in note B, this is a
  proposal with a number attached, not a change.
* **The 5% is not this opportunity's size and probably not related to it.**
  See above: the bulk path does not decode. Quoting the two together would be
  the "measurement plus a plausible story" error note D already records.
* **The paper's numbers are x86 and this box is arm64.** Table 13 is AMD Rome.
  `pshufb` has a NEON counterpart (`tbl`) and simdutf ships both, but the
  instruction counts here are not measured on Apple silicon and must not be
  quoted as if they were. Note D records that this box cannot read counters
  without an Xcode install.
* **`[[source: simdjson]]` is cited in this group and is about structural
  scanning, not decoding.** baud already does the scanning half. The decoding
  half is what this note is about, and they should not be conflated the way
  "SIMD parsing" as a phrase invites.

## Claims relied on

| claim | source | where |
|---|---|---|
| A finite-state UTF-8 validator costs ~7 instructions per byte on all inputs | `[[source: utf8-lookup]]` | Table 13 |
| The lookup algorithm costs 0.21 instructions per byte on ASCII and <1 on any input | `[[source: utf8-lookup]]` | Table 13, Abstract |
| It is >10× faster than routines in common libraries, using commonly available SIMD | `[[source: utf8-lookup]]` | Abstract |
| The validation rules are Too Short, Too Long, Overlong, Too Large, Surrogate | `[[source: utf8-lookup]]` | § 3 |
| It is built on `pshufb`/`vpshufb` | `[[source: utf8-lookup]]` | § 4 |

## Gaps and open questions

- **~~Not read: how the lookup tables are derived.~~** Read, and the tables
  now exist. `[[source: utf8-lookup]]` § 5 builds three 16-entry tables indexed
  by the high and low nibbles of the first byte and the high nibble of the
  second; bits 0-6 encode Table 8's seven error patterns, and a bit set in all
  three ANDed lookups means the pair is invalid. Bit 7 marks a continuation
  pair, which § 6.2 uses for the long forms and which is not itself an error.

  `scripts/gen-utf8-tables.py` derives them, **and does not transcribe them**.
  A wrong entry here does not look wrong -- it makes the validator *accept*
  invalid UTF-8, and `E0 80 AF` decoding to `/` is a bug this project has
  already had. So the seven patterns are written as predicates, the tables fall
  out by construction, and all **65,536 byte pairs** are checked against a
  reference decoder written from the definition of UTF-8 rather than from the
  patterns, so agreement is evidence instead of tautology.

  It earned that on the first run, catching an error a transcription would have
  shipped: `F4 80`..`F4 8F` is `U+100000..U+10FFFF` and valid, and a
  too-large pattern of "lo(b1) >= 4 and any continuation" rejects it. Table 8
  row 6 reads `10|01...11` -- the continuation's high nibble is 9, A or B, and
  8 belongs to row 5 with the overlong four-byte form.

      HI1: 01 01 01 01 01 01 01 01 00 00 00 00 06 02 1a 62
      LO1: 37 07 03 03 43 63 63 63 63 63 63 63 63 6b 63 63
      HI2: 02 02 02 02 02 02 02 02 35 55 4d 4d 02 02 02 02

  What remains before code: § 6.2's rule for 3- and 4-byte sequences, which the
  two-byte tables do not cover, and deciding where a block validator sits
  relative to `fast_utf8` -- validation is not decoding, and baud needs the
  codepoint.
- **NEON.** Whether the arm64 form carries the same ratio is unmeasured here,
  and this box is the one the project measures on.
- **~~How many bytes reach the decoder, per payload.~~** Measured.
  `Term::utf8_share` counts bytes entering a multi-byte lead or continuation --
  not `bytes_slow`, which is mostly control sequences:

  | payload | reaches the decoder |
  |---|---|
  | `payload::streaming` | **0.0%** |
  | `payload::tui` | 0.0% |
  | asciiquarium | 0.0% |
  | **btop** | **14.5%** |
  | **vim (CJK/emoji)** | **28.2%** |

  **This note's own estimate was wrong, and wrong for the reason this project
  already records.** It reasoned from `payload::streaming` -- pure ASCII, so
  0% -- and concluded the opportunity was "correspondingly small". Real
  terminal output is not ASCII: btop draws with `U+250x` box characters and vim
  with CJK, and both are three bytes in UTF-8. That is "payloads that don't
  exercise the case" from `CLAUDE.md`'s measurement-failure list, applied to a
  research estimate instead of a benchmark.

  So 14-28% of real bytes go through a decoder `[[source: utf8-lookup]]` prices
  at 7 instructions per byte against lookup's <1. That is worth building.
