# Two lookups a character, unconditionally — and the design that does fewer

> **Group:** G · **Sources:** `[[source: ugrapheme]]`, `[[source: uax29]]`, `[[source: uax11]]`, `[[source: lemire-transcoding]]` · **Status:** partial

**Dated 2026-10-10.** Written on 2026-08-29 against the core then named
`baud-core`, renamed `baud-core2` later that day and deleted in `2538310`
(2026-09-10); the 410 MB/s below is that core's. Step 1 of "What to try" was
built the same day, `c91936e`, as a palette rather than the packed leaf this
note describes, one two-stage lookup into 37,204 bytes, measured 1.5% slower
and kept as smaller and correct. `Classified::of`
(`baud-domain/src/merged.rs`) has since become one load for the Basic
Multilingual Plane, from a flat table, and the two-stage lookup above it, and
`baud-core` calls it on the per-character path (`baud-core/src/write.rs`). So
baud no longer performs two trie walks a character. Steps 2 and 3 are not
built. The sentences below that said what baud does now are put in the past
where they stand.

## Why this note exists

baud asked Unicode two questions about every non-ASCII character, always, in
this order: `Width::of` for the column count and
`GraphemeClass::of_with_lookback` for the break decision. Each is a two-stage
trie, so each is two dependent loads, and together the tables are **44,480
bytes** -- `WIDTH_INDEX` 4,352, `WIDTH_BLOCKS` 7,360, `BREAK_INDEX` 4,352,
`BREAK_BLOCKS` 28,416.

Priced against `cat-cjk.raw`, that was ~21% of the per-character path, and
one extra `Width::of` cost **5.5%**. The question this note answers is whether
anyone does it for less.

## `ugrapheme` does, and states how

`[[source: ugrapheme]]` reports UAX #29 segmentation at "hundreds of megabytes
or even gigabytes per second", and "less than a nanosecond per character" on
ASCII and Latin-1, reaching around **1.5 GB/s**. Two design choices are named
for it, and both are aimed at exactly the cost above.

**Sparse bitmaps rather than tries.** It stores properties for *every* Unicode
codepoint in **under 11 KB**, against baud's 44 KB, explicitly so the whole set
sits in L1 -- and reports the lookup as "2 loads, a few shifts and maybe a
compare or two". It also notes what baud's own trie comment observes
independently: similar characters cluster, so real text touches only a few
contiguous regions.

**A DFA that minimises lookups per transition.** Rather than the state machine
UAX #29 suggests, it parses the rules as an LL(1) grammar with few states, and
"an attempt is made towards a minimum number of codepoint property lookups to
decide on a state transition", with "an unsigned comparison used as an early
Bloom filter for properties".

That last sentence is the structural difference. **baud performed both lookups
before it knew whether either mattered.** A DFA in a state that can only be
left by one class needs one property, sometimes none.

## What is and is not comparable

`ugrapheme`'s figure is segmentation *alone* -- no grid, no scroll region, no
VT parsing, no cell writes -- so it is not a throughput comparison with baud's
410 MB/s and this note is not claiming one. What compares is the
classification component, and there baud was doing two unconditional trie
walks where the prior art does the minimum the state requires.

Two of baud's own measurements say the same thing from inside:

* **A merged trie is worth 2-3%**, priced before building it, because merging
  *shares* probes rather than skipping a walk.
* **`is_wide_ordinary` as an early test measured flat**, twice, in two
  placements -- four range compares running on all wide characters to skip a
  trie on the ~33% that are not emoji. `ugrapheme`'s framing suggests why:
  their filter is **one** unsigned compare, not four, and it is placed where
  the DFA state already narrows the answer.

## What to try, and what it is worth before trying it

The honest ceiling is the 21%. Even a perfect classifier -- zero cost -- leaves
the slow path at ~51 cycles a character against a target of 8.9, so this is not
the 2.5x. It is the largest single item in the slow path that has a known
better design, which is a different and smaller claim.

Order, cheapest first:

1. **Fold the two tables into one**, keyed on the same `cp >> 8` stage-1 shape
   both already use, with width in two bits and class-plus-lookback in the byte
   the break trie already returns. Priced at 2-3%; also takes the property set
   from 44 KB to something nearer 33 KB.
2. **One unsigned compare as an admission test**, not four range compares, and
   sited where the cluster state already narrows what can follow.
3. **A bitmap representation** only if 1 and 2 land, since it is the largest
   change and its benefit is cache residency, which `NMARK` 512 to 128
   measured flat for a different table.

`[[source: lemire-transcoding]]` is pinned alongside these as the reference for
the decode half; it is not evidence for the classification half, and this repo
has already measured that vectorising decode *costs* here.
