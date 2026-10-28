# Where a multi-codepoint cluster lives — three terminals, three answers

> **Group:** F · **Sources:** `[[source: kitty-text-cache]]`, `[[source: kitty-line]]`, `[[source: ghostty-page]]`, `[[source: uax29]]` · **Status:** partial

**Dated 2026-10-10.** Written on 2026-08-29 against the core then named
`baud-core`, renamed `baud-core2` later that day and deleted in `2538310`
(2026-09-10). Every "baud" below that keys on position, holds `MarkSlot`s or
truncates is that core. `baud-core` took this note's direction:
`ClusterStore` (`365650b`, 2026-09-09; `baud-domain/src/cell.rs`) is keyed on
the cluster's content, a cell finds its cluster by the index in its
`CellFlags`, and the store is 63 usable slots probed twice that **refuses**
rather than evicts, so a cluster that finds no slot, or has more than
`ClusterSlot::CAPACITY` (8) continuations, keeps its base and loses its tail,
counted in `marks_dropped`. The record has it as "The cluster store is
content-keyed, and the index rides in the cell". The sentences below that
said "today" are put in the past where they stand.

## Why this note exists

A grapheme cluster can be more codepoints than a cell can hold, so every
terminal that segments text needs somewhere else to put the rest. baud put
them in a table **keyed on the cell's position**. This note is what the others
do, and it exists because the position key produced four correctness bugs in
one session and a measurable tax on top.

The test that makes it a design question rather than a preference: **does being
correct cost anything?** In baud it does. Grapheme segmentation is 25-45% of
UTF-8 throughput — the reason the CJK comparison against ghostty is not like
for like, since libghostty-vt does not segment unless an application asks — and
two separate corrections to the mark path cost 3% and 2.5-3%. A design that
fits the problem does not charge for correctness.

## The three

| | where the extra codepoints live | keyed on | how a cell finds them |
|---|---|---|---|
| **baud** (`baud-core2`, as written) | `[MarkSlot; 512]`, each a fixed `[u32; 8]` | **position** — surface, physical row, column, row generation | hash the position, probe |
| **kitty** | `TextCache`: a monotonic arena of codepoint runs | **content** | the cell's `ch_or_idx` **is** the index |
| **ghostty** | `grapheme_alloc`, a chunked slice allocator | position (cell offset within a page) | `grapheme_map`, after `Cell.content_tag` says there is one |

Two of the three reach for the cell before the table. ghostty's
`appendGrapheme` tests `content_tag` first, so the map is touched only for
cells that have graphemes; baud gained the same bit this session
(`CellFlags::MARKED`) after a cell rewritten in place kept the marks of the
character it replaced.

Only kitty keys on **content**, and only kitty reads by **index**.

## The split kitty makes, which is the transferable part

`[[source: kitty-text-cache]]` carries both structures at once:

* an `array` of `Chars`, read by direct index — `tc_chars_at_index()`,
  `tc_first_char_at_index()`, `tc_num_codepoints` all take a `char_type idx`;
* a `chars_map`, hashed by content, used **only when interning**.

So the hash runs once per *new* cluster, on the write path, to establish
sharing. Every read is an array index. That is the distinction worth carrying:
O(1) by hashing and O(1) by indexing are not the same O(1), and the hot path
here is reads.

baud did the opposite when this was written. It hashed on **every** attachment, and a ZWJ
family is six attachments to one cell — six position rebuilds, six hashes, six
probes, six read-modify-writes of the same cell. Measured by rebuilding that
prefix once more per attachment: **2.5% on cat-cjk, 3% on less-cjk**, and zero
on btop, which has no marks.

## What the position key costs, itemised

Each of these is a consequence of keying on *where* rather than *what*, and
each is measured or was a bug:

| | |
|---|---|
| the key must carry surface, physical row **and** a row generation | 3%, because a position names different cells over time |
| a cell rewritten in place kept its predecessor's marks | fixed by `CellFlags::MARKED` |
| marks on a wide base landed on the `WIDE_TAIL` | fixed by `LeadCol` |
| identical emoji cannot share a slot | 512 slots × 44 bytes, probed at random |
| `MarkSlot::CAPACITY` is a fixed 8 | a four-person family with skin tones is 11 codepoints and **was truncated** (`baud-core`'s `ClusterSlot::CAPACITY` is 8 too, and refuses such a cluster: the status above) |

The last row is the one a content-keyed arena removes outright rather than
raising: kitty's arena stores runs of the length they happen to be, so there is
no capacity to choose and no cliff to document.

## The obstacle, named rather than skipped

Indices in cells cannot be evicted freely, and `[[source: kitty-text-cache]]`
says so in its own comment: *"Interned cell texts are referenced from cells by
index, so entries cannot be removed"*. kitty's answer is a garbage collection —
`tc_gc_begin`, then `tc_gc_map_index()` remaps every live cell index, re-interning
the entries that survive.

baud cannot copy that cheaply. It has no allocator, `Storage` is caller-provided
and fixed, and the roots are the viewport **and** the scrollback spans. But it
does not need the same answer: the style table and the `OSC 8` link table
were both bounded, content-addressed and **evicted on collision**, and the
contract was already written down — a cell that loses its entry keeps its base character.

What differs is the consequence. A cell that loses its style renders in the
wrong colour; a cell that loses its cluster would render the wrong *text*, and
worse, a cell pointing at a re-used slot would render someone else's text with
no way to tell. So the eviction contract that is fine for renditions is not
obviously fine for content, and closing that gap — an epoch in the spare flag
bits, a validity check, or accepting and documenting it — is the piece to
settle before any of this is written.

## What this is worth, stated before anyone builds it

The rebuild half is **2.5-3%**, and the *whole* side table is **5%** --
`ceiling-no-attach` stubs `attach_mark` outright, cells byte-identical, and
gives cat-cjk 433.3-441.1 against 415.9-417.8. So 5% is the hard ceiling on
everything in this note, and a redesign that recovers half of it is worth
2.5%. Sharing slots between repeated emoji saves memory and evictions rather
than time. `is_break`, at 14%, is untouched: deciding *where* a cluster ends is
a different question from where to put it, and no storage change makes UAX #29
cheaper.

So this is a correctness and design argument carrying a few percent, and it
should be proposed as that. It is not a route to the 2.4x that cat-cjk needs,
and this note would be misread if it were filed as one.
