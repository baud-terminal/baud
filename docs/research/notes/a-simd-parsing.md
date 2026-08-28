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

- **Not read:** the paper's § on how the lookup tables are derived, which is
  what an implementation actually needs. Only the cost tables and the rule list
  have been read so far.
- **NEON.** Whether the arm64 form carries the same ratio is unmeasured here,
  and this box is the one the project measures on.
- **How many bytes reach the decoder, per payload.** This is the first thing
  to measure and it needs no implementation: a counter in the decode path,
  read on `payload::streaming` and on each recording. On streaming the answer
  is probably ~5% and the opportunity is correspondingly small; on the vim
  recording, which carries CJK and emoji, it could be most of the text. Until
  that number exists, the size of this win is unknown and should not be
  estimated -- which is the rule this project already applies to shaping.
