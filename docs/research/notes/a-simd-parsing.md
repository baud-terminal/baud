# SIMD parsing — the decoder is the part with a known better algorithm

> **Group:** A · **Sources:** `[[source: utf8-lookup]]`, `[[source: simdjson]]` · **Status:** partial

**Dated 2026-10-10.** Written on 2026-08-27 and 2026-08-28 against the core
then named `baud-core`, renamed `baud-core2` on 2026-08-29 and deleted in
`2538310` (2026-09-10). Its `Term::blit`, `feed`/`feed_avx2` split,
`feed_with` and `fast_utf8` were that core's. In `baud-core` an ASCII run
goes through `Paint::blit` to `View::put_ascii_run`, the vector scan is
reached from `run_of_text` (`baud-core/src/write.rs`): `Neon::run` under
`neon-scan`, the default, and `Avx2::run` under `avx2-scan` on a build that
targets AVX2 -- each the safe inherent surface over its `Scan` impl in
`baud-isa` -- and `baud-domain`'s `core::simd` otherwise; `scan_avx2` lives on
in `baud-isa-x86-64`. The lookup decoder this note proposes is implemented in
neither core. `baud-isa`'s `Decode::block` is a table-driven block decoder,
not the lookup validator, and `baud-core` does not call it. Present-tense
sentences below that named the old core are put in the past where they stand.

## Why this note exists, and why now

Group A has been declared since the corpus was set up with `simdjson` in it and
**no note**. It got one because a measurement asked for it rather than because
the group looked empty.

Profiling the gradual erosion between `81238de` and `473a97a` on multibench —
the first time that range had been measured on a harness that can resolve it —
the largest single step is `2931154`, *"Reject overlong UTF-8, and stop error
recovery eating a valid successor"*:

| commit | spans, streaming |
|---|---|
| `81238de` | 1660.8 / 1690.1 |
| `43f8a69` VT52 and nine private modes | 1672.3 / 1647.5 |
| `bf16b2c` williams' anywhere block | 1647.6 |
| **`2931154` reject overlong UTF-8** | **1567.1 / 1570.7** |
| `33570a3` width tables from Unicode data | 1553.3 / 1524.5 |
| `7e92ef1` UAX #29 clustering | 1561.1 |

**That 5% is almost certainly not decoder work, and this note said it was.**
The first draft argued "unlike the width tables this is not a layout ghost,
because an ASCII byte goes through the decoder on every pass". It does not.
`Term::blit` handed `&arena[..]` straight to `View::put_ascii_run`, which writes
cells from raw bytes -- no `Codepoint`, no `Utf8Acc`, no decoding at all -- and
that is the path 95% of streaming bytes take. The decoder sees the other 5%.

So `2931154` looks like the same layout effect as the width tables, and the
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

**baud's decoder is *not* the finite-state row, and this note said it was.**
§ 5 describes that row as a standalone validator which runs its state machine
over every byte — that is why it costs 7.0 even on ASCII, "whatever the input".
baud does two things that machine does not:

* **ASCII never reaches the decoder.** In `baud-core2` `S::run` took printable
  ASCII into `put_ascii_run` before the state machine saw it, which is what
  that core's `utf8_share` reporting **0.0%** on `payload::streaming`
  measured; in `baud-core` `run_of_text` and `Paint::blit` do the same. The
  7.0-on-ASCII column describes work baud does not do.
* **Validation is fused with decoding.** baud does not validate and then
  decode; `utf8_continuation` and `finish`'s floor check happen once, as part
  of producing the codepoint. The paper's validators produce a yes/no and no
  codepoint at all.

Architecturally baud is closest to the paper's **branchy-ascii** — a wide ASCII
bulk test plus a branchy path for the remainder — which § 7 finds "almost twice
as fast as branchy" on ASCII-heavy files for exactly that reason.

So **the 7.0-to-0.97 comparison never described baud**, and quoting it as this
opportunity's size was a category error: a standalone whole-input validator
against a fused, ASCII-bypassing decoder. It is the fourth precondition in this
corpus checked only after it had been relied on.

The paper's own summary: the lookup algorithm "outperforms UTF-8 validation
routines used in many libraries and languages by more than 10 times using
commonly available SIMD instructions", and "in all our tests, irrespective of
the input, lookup requires fewer than one retired instruction per byte".

Its rules are the ones baud implements, named the same way: *Too Short*, *Too
Long*, *Overlong* — "the decoded character must be above U+7F for two-byte
characters, U+7FF for three-byte, U+FFFF for four-byte" — *Too Large*, and
*Surrogate*. `2931154` implemented those with a state machine; this is the same
correctness with a different instruction count.

## Why baud is unusually well placed to take it

* **The instruction class is already here.** The algorithm is built on
  `pshufb` / `vpshufb`, and `scan_avx2` already lives in the `_mm256`
  neighbourhood, in `baud-isa-x86-64` since `2960dd9` -- its movemask is the one
  point where an intrinsic's result enters the type system. The `Scan` trait
  and `baud-core2`'s `feed`/`feed_avx2` split were the structure a vectorised
  decoder would slot into; in `baud-core` it is `baud-isa`'s `Decode` trait
  beside `Scan`, which `baud-core` does not call.
* **The correctness bar is already met and tested.** Overlong rejection,
  surrogate rejection and the error-recovery rule are pinned by tests against
  the standard's own vectors. A replacement has an oracle to differ against
  rather than a specification to re-read.
* **ASCII is the common case and the one that gains most.** 0.21 against 7.0.

## What the scalar half already bought

`baud-core2`'s `fast_utf8` decoded one complete sequence in place instead of
paying three or four `control` dispatches. Measured, non-overlapping: **btop +7.5%, vim +7.1%**
(`MEASUREMENTS.md`). That is not the lookup algorithm -- it removes
dispatch overhead, not decode instructions -- but it establishes the shape a
vectorised version needs: a place in `feed_with`, that core's loop, where a run
of text is handled outside the state machine.

**What is left is unsized, and smaller than this note claimed.** The dispatch
overhead was the large, measurable part and `fast_utf8` took it. What a
block validator could still remove is the per-character validation *inside* the
fused decode -- a handful of masks and one floor comparison -- while
`Width::of`, the cluster break and the cell write are untouched by any UTF-8
algorithm and are plausibly the larger share of what remains.

Sizing that needs a profile of the **recordings alone**, and the harness could
not produce one when this was written: `baud-bench` ran the synthetic payloads
in the same process, so a `dtrace` sample was dominated by `put_ascii_run` and
everything of interest was inlined into `feed_with`. `ingest2` runs one payload
a process now (`cargo bench -p baud-bench --bench ingest2 -- <payload> <reps>`),
so the profile can be taken; this note has not taken it (dated 2026-10-10). Until that exists, the remaining upside
is **unmeasured, and this note will not estimate it** -- estimating it from a
payload that does not exercise it is the mistake made here once already.

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
  entry point beside `baud-core2`'s `feed` and `feed_avx2`, and -- unlike
  `scan_avx2` -- it
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
  quoted as if they were. Note D recorded that this box could not read
  counters without an Xcode install; it could, through Xcode's own `xctrace`
  (note D, "Answered since"), and none were read for this note.
* **`[[source: simdjson]]` is cited in this group and is about structural
  scanning, not decoding.** baud already does the scanning half. The decoding
  half is what this note is about, and they should not be conflated the way
  "SIMD parsing" as a phrase invites.

## Claims relied on

| claim | source | where |
|---|---|---|
| A **standalone** finite-state UTF-8 validator costs ~7 instructions per byte on all inputs, ASCII included — a machine baud is not | `[[source: utf8-lookup]]` | Table 13, § 5 |
| `branchy-ascii` — a wide ASCII bulk test plus a branchy remainder, which is baud's shape — is almost twice `branchy` on ASCII-heavy files | `[[source: utf8-lookup]]` | § 7 |
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

      HI1: 01 01 01 01 01 01 01 01 80 80 80 80 06 02 1a 62
      LO1: b7 87 83 83 c3 e3 e3 e3 e3 e3 e3 e3 e3 eb e3 e3
      HI2: 02 02 02 02 02 02 02 02 b5 d5 cd cd 02 02 02 02

  **§ 6.2's long-form rule is implemented too**, so the generator now models the
  whole validator rather than half of it: "we expect two continuations if the
  previous byte matches `111|`, or if the byte before that matches `1111|`",
  compared against where two continuations actually occur (bit 7). If the two
  disagree anywhere, the input is invalid.

  Two things there are easy to get wrong and both were:

  * **"Previous" is previous to the *pair*, not to the index.** The pair judged
    at `i` is `(data[i-1], data[i])`, so the byte before it is `data[i-2]`.
    Reading it as `data[i-1]` rejects `E0 A0 80` -- U+0800, valid.
  * **Bit 7 is not an error.** It records that a pair is two continuations,
    which is what § 6.2 counts. Masking it into the error test rejects every
    three- and four-byte character.

  Verified against CPython's decoder -- an independent oracle, not one written
  beside the thing it checks -- over **all 10,321,920 complete three-byte
  strings**, every complete one- and two-byte string, and 122,496 four-byte
  boundary combinations. Strings ending mid-sequence are excluded and counted,
  because the block validator is not responsible for them: in `baud-core2`
  that was what `State::Utf8` carried across a read boundary and what
  `fast_utf8` declined.

  What remained before code, in that core: deciding where a block validator
  sat relative to `fast_utf8`. Validation is not decoding, and baud needs the
  codepoint, the width and a cell out the other side.
- **NEON.** Whether the arm64 form carries the same ratio is unmeasured here,
  and this box is the one the project measures on.
- **~~How many bytes reach the decoder, per payload.~~** Measured.
  `baud-core2`'s `Term::utf8_share` counted bytes entering a multi-byte lead
  or continuation -- not `bytes_slow`, which was mostly control sequences:

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

## The decoder is not the bottleneck, and the literature is how that was settled

> Added 2026-08-29. Source: `[[source: simdutf]]`.

This note's standing gap was "deciding where a block validator sits relative to
`fast_utf8`. Validation is not decoding, and baud needs the codepoint, the
width and a cell out the other side." The obvious next step looked like a SIMD
*decoder* to go with the SIMD validator.

`[[source: simdutf]]` says not to build one, and its Table 6(b) is why. That
table is an **Apple M1** — the same architecture class this project measures on,
so it is a comparable number rather than the cross-machine kind `CLAUDE.md`
rules out — and it reports validating UTF-8 to UTF-16 transcoding in
gigacharacters per second:

| Chinese | ICU | LLVM | finite | simdutf |
|---|---|---|---|---|
| Gchar/s | 0.43 | 0.23 | 0.43 | **1.2** |

A Chinese character is three UTF-8 bytes, so even ICU's ordinary decoder is
about **1.3 GB/s** of input. baud's *entire* `cat-cjk.raw` pipeline — decode,
width, clustering, cell write, scroll — runs at **0.31 GB/s**. A decoder four
times faster than the whole of baud cannot be what is limiting baud.

**Confirmed against baud's own code rather than inferred.** Adding a second,
identical decode to `fast_utf8` — the probe that cannot be contaminated,
because the output is unchanged — costs **8-10%**:

| payload | real | with a second decode |
|---|---|---|
| `utf8/cat-cjk.raw` | 302 | 278 (**-8%**) |
| `payload::cjk` | 363 | 326 (**-10%**) |
| `utf8/less-cjk.raw` | 310 | 286 (**-8%**) |

So decoding is ~9% of the CJK path. simdutf is 2.8x ICU on M1; applying that
ratio to baud's 9% would return about **6%** of total, for a SIMD transcoder,
its tables, and a second code path to keep correct against
`GraphemeBreakTest.txt` and the overlong-form tests. That is not the change to
make.

**Where the time actually is**, by ceiling arm, after this session's work:

| part | share of `cat-cjk.raw` |
|---|---|
| the cell write | ~29% (whole write free = +40%) |
| grapheme clustering | **10-15%** |
| UTF-8 decode | ~9% |
| `Width::of` | ~5% |

Clustering was 25-45% before the lookback tables were folded into the class
trie's high nibble; that change is in `MEASUREMENTS.md` and took cat-cjk +28%.

**What the paper does support is the shape, not the decoder.** Its § 1
describes the pattern every fast transcoder shares: an ASCII fast path, then
block processing with a shuffle table. baud has the first — `put_ascii_run`
writes eight cells a store — and has nothing corresponding for non-ASCII, which
goes one codepoint at a time through `put_glyph`. Table 5's Latin column makes
the size of that asymmetry plain: **18 Gchar/s** with an ASCII fast path
against 1.2-1.9 for the Asian languages, in the same library.

So the lead is a **bulk path for runs of same-width non-ASCII text**, and the
paper's own numbers say the win is in the cell write rather than in the decode
that feeds it.

## Claims relied on

| claim | source | where |
|---|---|---|
| Validating UTF-8 to UTF-16, Chinese, on Apple M1: ICU 0.43, LLVM 0.23, finite 0.43, simdutf 1.2 Gchar/s | `[[source: simdutf]]` | Table 6(b) |
| Non-validating, AMD Rome: Latin 18 Gchar/s against Chinese 1.7 | `[[source: simdutf]]` | Table 5 |
| Asiatic characters including Chinese and Japanese require three UTF-8 bytes | `[[source: simdutf]]` | § 2 |
| A byte-permute driven by a lookup table is the key ingredient of most SIMD transcoders | `[[source: simdutf]]` | § 1 |
| Omitting validation gives only a modest gain, so validating transcoders are what engineers should use | `[[source: simdutf]]` | § 6 |
