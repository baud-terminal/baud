# J: colour-bitmap limits -- a presence index, its bound, CBLC subtables, flip chains

Research for owner questions, 2026-10-02. Round one went to the owner with a
strike bound at 0.63 and a flip rule at 0.88, and the owner answered "too low,
more research". Round two put five readers to work at source, a skeptic on
each, and measured baud and CoreText on hostile fonts; Jev's numbers fell. A
diagnostic then showed the strike options had been posed on the wrong axis,
and round three re-posed every question on one axis. Every figure here is
labelled MEASURED, READ or DERIVED; a derived figure is arithmetic on measured
ones and is not a measurement.

The record entry is "A colour bitmap is drawn from a strike that holds it, and
`flip` is a mirror" in the record, with its follow-ups.

## 1. The walk is the wrong axis

**What is walked** (READ, at `b1c70db`). baud reads colour bitmaps from `sbix`
(`numStrikes`, a uint32; a strike's ppem is a uint16) and `CBLC`/`CBDT`
(`numSizes`, a uint32; ppemX and ppemY are uint8). It defines no `EBLC`,
`bloc` or `EBSC` tag (`baud-font/src/lib.rs`). In production one path asks
whether a glyph has a bitmap: `Stack::fill` in `baud-render/src/stack.rs`,
which decides a 256-codepoint block of the face table the first time a cell
lands in it, and calls `Face::is_colour` for every codepoint of the block, for
every face whose `cmap` reaches it. For a bitmap face that is
`Face::has_bitmap`, a walk over every strike until one holds the glyph; a
glyph none holds walks them all. (`select_presented`, the other caller, is the
reference the table is tested against.) The atlas walks the strikes too, once
per (face, glyph, phase) key, and sorts their ppems before the first probe.

The specification requires asking every strike: "If the application is using
bitmap data to draw text and there is bitmap data for a glyph in any strike,
then the glyph must be drawn using a bitmap from some strike"
(`[[source: ms-opentype-sbix]]`; the same rule in `[[source: apple-ttrm-sbix]]`).
So baud cannot read one strike per size, as FreeType does.

**The principle it breaks.** baud's architecture principle, which rides in
every Jev request: "Everything on the hot path is O(1): no scans, no linear
search ... Judge a change by whether it moves work from runtime into a table."
Asked whether the walk in the block fill satisfies it, Jev answered no, 0.91
at confidence 0.82. Every option of rounds one and two -- 255, 64, `CBLC` 255
with `sbix` 65,535, no bound -- was a bound on that walk.

**The index** (MEASURED, release build, outside the tree:
`scripts/strike-count-build.rs`). One bit per glyph, whether any strike holds
an image for it: at most 8 KiB, since a face holds at most 65,535 glyphs. Built
strike by strike through `Face::bitmap`, the lookup `has_bitmap` makes, and held
equal to `has_bitmap` at every glyph of every font below before it was timed:

| face | strikes x glyphs | build |
|---|---|---|
| Apple Color Emoji | 9 x 3,844 | 0.047-0.058 ms |
| Noto Color Emoji | 1 x 4,046 | 0.052-0.060 ms |
| `sbix`, every glyph empty in every strike | 255 x 65,535 | 38.2-38.4 ms |
| `sbix`, every record a `dupe` loop | 255 x 65,535 | 337-341 ms |
| `sbix`, every glyph empty | 4,096 x 65,535 | 617-624 ms |
| `sbix`, every record a `dupe` loop | 4,096 x 65,535 | 5,491-5,780 ms |
| `CBLC`, one subtable a size | 255 x 65,535 | 55.8-58.3 ms |
| `CBLC`, 255 subtables in one size | 1 x 65,535 | 14.4-18.8 ms |
| `CBLC`, 255 subtables a size | 255 x 65,535 | 3,645-3,685 ms |
| `CBLC`, one subtable a size | 4,096 x 65,535 | 799-807 ms |

The hostile faces are `scripts/strike-count-fonts.py`'s wide fonts: strikes
overlapped at a 4-byte stride, so no two share an offset a build could
deduplicate on. The cost is linear in strikes x glyphs -- 4,096 strikes cost
16.1 times 255 -- at 2.3 ns a strike-glyph pair, 20 ns where every record is a
`dupe` loop the ten-record bound ends, and 0.86 ns more for each `CBLC`
subtable walked. It equals walking every glyph once: the walk pays it a block
at a time on first touch, the index all at once. Faces are opened at launch,
before the first frame (`baud-app/src/host.rs`), so the index is a launch cost,
and the block fill's question becomes one bit.

## 2. The bound the index needs

**Why a bound remains.** An exact index asks every strike about every glyph.
An overlapped strike costs its table eight bytes, so the table's length bounds
neither the strike count nor the build: a 200 MB file can name 25 million
strikes over 65,535 glyphs, about an hour to index (DERIVED). Resolving each
strike's `dupe`s once, not built, would remove the loops' tenfold.

**What each bound costs at launch** (MEASURED at 255 and 4,096, DERIVED from
their linearity elsewhere): 64 strikes about 10 ms, or 85 ms with `dupe` loops;
255 strikes 38 ms, or 0.34 s; 65,535 strikes about 10 s, or 87 s. For `CBLC`
with sorted records and one pass a size (section 3), 64 sizes about 43 ms and
255 about 0.17 s, over a shared 4 MB array.

**A budget on the build's own work.** Counted as every strike-glyph pair the
build asks about plus every `CBLC` subtable record it reads, both known from the
table before the build starts, a budget of 2^20 holds the build to about 21 ms
(`sbix` records that are all `dupe` loops, 20 ns a pair) and 3.5 ms for `CBLC`
(3.3 ns a pair, 0.86 ns a record). Counting pairs alone does not bound `CBLC`: a
4-glyph face could carry 262,144 sizes sharing a 4 MB array of 524,288 records,
and one pass a size would read 2^37 records, about 2 minutes (DERIVED). Apple
Color Emoji's work is 34,596 and Noto Color Emoji's 4,049. A work budget bounds
the build, not the strike count: a one-glyph face may carry 2^20 strikes, and the
atlas, which reads and sorts every strike's ppem once per glyph and size it
draws, would do so over all of them (unmeasured).

**Real fonts** (MEASURED, a census of 32,042 font files from Debian sid and
the archive, Fedora, Arch, openSUSE, FreeBSD 14, Google Fonts, ReactOS and
upstream releases, plus this Mac's 688 and its 363 downloadable font assets):

- The tables baud reads: at most **9** strikes, Apple Color Emoji and Bungee
  Color's `sbix` build (ppems up to 1,024); then 7 (EmojiOne's `sbix`) and 4
  (Noto's 3D `sbix` builds). Every real `CBDT` font has one strike. The most
  strike-glyph pairs in a colour font is Apple Color Emoji's 34,596.
- The tables baud does not read: 18 (Courier New's monochrome `EBLC`), then 14.
- No table repeats a ppem; two list their strikes out of ppem order.

**Specifications** (READ). None sets or recommends a limit; `numStrikes` and
`numSizes` are uint32 everywhere, and `sbix` allows two strikes at one ppem
with different pixel densities, so neither table's ppem range bounds its count.
The only ordering rule is Apple's `bloc`: "The sizes must be sorted in
ascending order."

**Other software** (READ at pinned commits unless marked):

| | bound on the count | strikes asked per glyph |
|---|---|---|
| FreeType 2.14.3 | refuses 65,536 or more, since FreeType 2's first revision (1999), reason unrecorded | one: the size's strike, no fallback |
| CoreText, macOS 27 (MEASURED) | none: opens 65,535 distinct ppems and 1,048,574 of one ppem | walks them for a missing glyph at about 1 ns a strike |
| HarfBuzz 14.5.1 | none; a sanitize budget bounds loading only | every strike's ppem, on every query |
| ttf-parser 0.25.1, fontations | none | every one |
| fontTools | none (`sbix` cannot load past 65,536: ppems must differ) | one probe a strike |
| Skia's Fontations bridge | none | every one to choose a size; its has-a-bitmap check asks only the first strike |
| OTS | does not parse these tables; drops them, or passes them through unread | -- |

kitty, ghostty and wezterm use FreeType or CoreText and add no cap of their
own.

**The threat has been seen.** FreeType's own fuzzer timed out on a font with a
large strike count (oss-fuzz 353, 2016), and its maintainer capped the fuzz
harness, not the library, at 10 strikes: "Malformed fonts often have large
values for the number of bitmap strikes, and FreeType doesn't check the
validity of all bitmap strikes in advance" (commit 6b21d128). A second count
bug was arithmetic overflow (crbug 347835680, fontations). Every
embedded-bitmap CVE found is an overrun, none a strike count.

**Where a font comes from** (READ, `baud-app/src/config.rs`). baud opens only
faces named by path: its built-in stack -- faces compiled into the binary and,
on macOS, six under `/System/Library/Fonts`, which macOS keeps read-only -- or
the user's configured `fonts`. Nothing is discovered from installed fonts, so a hostile table reaches
baud only when a user names its file -- and under no bound it would hang the
terminal at launch, before that user could fix the config from inside it.

**The crate's own contract** (READ, `baud-font/tests/hostile.rs`): "A font file
is the same class of input as bytes from a pty -- whatever the other end
sends". What it tests today is that no input panics, not what an input costs.

## 3. CBLC's index subtables

**The gap** (READ). A `CBLC` size names an IndexSubTableArray of
`numberOfIndexSubTables` records, a uint32 bounded only by the table's length,
and baud's lookup walks it linearly in every size it asks. `MAX_STRIKES` bounds
the sizes, not their subtables.

**What it costs** (MEASURED): 0.86 ns a subtable. Walking, one size of 65,535
subtables costs 14 ms a block; indexing, 255 sizes of 255 subtables take 3.6 s
(the table in section 1). DERIVED: a 4 MB array holds about 524,000 records and
255 sizes may share it, about 0.11 s a glyph and 2 hours to index.

**The rule** (READ, `EBLC`, to which `CBLC` defers): "Records must be sorted
by firstGlyphIndex, and records should not have overlapping glyph ID ranges."
With sorted records one pass over a size's array decides all its glyphs, about
sizes x (subtables + glyphs) steps -- about 60 ms at 255 sizes of 65,535 glyphs
(DERIVED from the one-subtable row) -- and the atlas's lookup can binary-search.
FreeType and HarfBuzz walk linearly; FreeType's CVE-2014-9666 was integer
overflow in an index-subtable count.

**Real fonts** (MEASURED). All 293 `CBLC` sizes in the 3,952 distinct font files
on disk -- the census's kept files and every test font, among them Noto Color
Emoji, Blobmoji, Twemoji and EmojiOne -- have sorted, non-overlapping records,
at most 6 a size (Blobmoji). Monochrome `EBLC`, which baud does not read,
reaches 3,004 (Sazanami Mincho).

## 4. A chain of flip records

`flip` is the `sbix` graphicType Apple ships in Apple Color Emoji since iOS
17.4: its payload names another glyph, drawn mirrored left to right. MEASURED:
the font has 972 `flip` records, 108 glyphs (people facing right) in each of
its 9 strikes, every one naming a `png` present in the same strike, and no
`dupe` records at all. None of 21 `sbix` fonts outside Apple's uses `flip` or
`dupe`. No known font chains records, so no rule below changes a pixel of a
font that exists.

**Nobody documents it** (READ). Apple's `sbix` page names two special
graphicTypes, `dupe` and `mask`, in the live page and in all 50 archived
captures from 2015 to 2026, 16 of them after `flip` shipped, while Apple kept
adding pages to the same manual. OpenType's `sbix` page does not mention it.
Neither says whether redirections may chain, or which strike a `dupe`'s target
is read from. FreeType's lead wrote in 2024 that `flip` is in neither
specification; fontTools found it "by experimentation". UTS #51 lets facing-right
emoji "face either direction, according to vendor practice"
(`[[source: uts51-emoji]]`); Google's Noto and Microsoft's Fluent draw separate
art.

**Two readings.** As an operation, a flip composes: if glyph B is C mirrored
and A is B mirrored, A is C upright, and only a toggle draws that. As an
attribute, a flip marks a glyph as drawn mirrored from the image its chain ends
at, and a flag draws that. With one flip per glyph, as in Apple's font, they
agree.

**CoreText** (MEASURED, macOS 27, offscreen `CTFontDrawGlyphs`; a skeptic wrote
16 predictions before the run and all 16 matched):

- It resolves a chain while walking the strikes in table order from the first:
  at each strike that holds the current glyph it follows at most one `dupe` or
  `flip`, skips strikes that lack it at no cost, and stops at the first holding
  strike whose ppem is at least the requested size, or at the last strike. It
  draws the reached glyph's `png` from there.
- Every chain it draws mirrors once, whatever the number of flips: flip->flip,
  three flips, flip->dupe and dupe->flip. On baud's own fixture
  (`baud-font/tests/fonts/sbix-strikes.ttf`, made loadable by adding `name`,
  `OS/2` and an empty `glyf`), glyph 7 (flip->flip->png) draws mirrored at 0.000
  error, where a toggle predicts upright.
- So a chain of h records draws only if h holding strikes precede or are the
  stopping one, else nothing: on a patched Apple Color Emoji a flip->flip glyph
  draws nothing at 12-20 px and is mirrored from 21 px. A `flip` naming itself
  draws nothing.
- Its strike choice is table order: above every strike it takes the last in the
  table, not the largest. baud's atlas takes the smallest strike at or above
  the size among those that hold the glyph, else the largest; on a table sorted
  by ppem, as Apple's is, the two agree.

**FreeType** (READ). Andrew Murray's merge request 339 (prompted by Pillow
issue 7854) implemented a flag, `flipped = TRUE`, in all seven of its
revisions, committed 2024-09-06. The next day a maintainer made it a toggle in
a commit outside the request whose only stated reason is "Also fixes recursive
flip"; the review had asked for a justification, "absent a specification
reference". No release shipped the flag; 2.14.0 (2025-09) shipped the toggle.
MEASURED on 2.14.3: flip->flip draws upright, three flips mirrored, five fail
its 4-hop bound. After a chain it places the image by the last record's origin
and the last glyph's advance, and a flip swaps pixels only.

**Everything else** (READ). The toggle reaches users through FreeType: Pillow,
hb-ft, Qt, WebKit-Skia on Linux, Firefox local fonts on Linux and Android. mpdf
wrote its own toggle (merged 2026-09-22, untested on Apple's font): "A flip of
a flip is the right way round". allsorts draws no glyph for a second flip. An
unmerged fontations pull request (1631) uses a flag with a 4-record bound, every
hop in one strike. HarfBuzz's own `sbix` code, skrifa, ttf-parser, swash,
cosmic-text and resvg do not support `flip`.

## 5. Round four: where the flip questions and the symbol face stand

The owner gave provisional answers (toggle; across strikes) and asked for more
research where Jev scored low.

**Where hops are read.** A model of CoreText's walk reproduces all 697
recorded CoreText cells across 20 test fonts; reading the whole chain in
CoreText's chosen strike reproduces 499 (MEASURED). The walk follows at most
one record at each strike that holds the current glyph, skips the rest, stops
at the first holding strike at or above the size or else the last holding
strike in table order, and needs an image there. Both specifications require
the glyph to be drawn: Apple, "if any strike has a bitmap for a particular
glyph, the bitmap for that glyph will always be drawn"; Microsoft, "must be
drawn using a bitmap from some strike" (READ). CoreText draws nothing in 84
measured cases where every strike holds the whole chain, 11 of them with only
`dupe` and `png ` records, and takes a glyph's first record from the table's
first strike, so a glyph whose drawn strike holds a complete image was drawn
as another glyph, mirrored. Only CoreText reads across strikes: FreeType 2.14
reads in the chosen strike (MEASURED on a font built to tell the two apart,
with 24 CoreText predictions written first and matched), as do Qt, Skia's
FreeType scaler, OpenJDK, zig-lib-opentype, swash and cherenkov (READ).
CoreText chooses and resolves at the device size, font size times the display
scale (MEASURED).

**How many mirrors.** Apple is the only producer of `flip` found, on macOS and
in the iOS, tvOS, watchOS and visionOS 27 simulator runtimes (1,320 font
files), and no Apple font chains a flip or a `dupe`; every flip glyph is named
`<base>.u27A1`, after the rightward-direction character (MEASURED). UTS #51
section 2.10, since revision 17: "If direction ZWJ sequences are supported and
the base emoji already faces that direction, the direction emoji should be
ignored" (READ); its bearing on glyph records is inferred. No producer, Apple
text, patent or Unicode document gives a chain a meaning.

**Corrections to section 4, from the renderer survey.**

- The toggle reaches users only through FreeType 2.14 and later. FreeType
  2.13.3 and earlier answer any `flip`, single or chained, with
  `Unimplemented_Feature` (MEASURED): the long-term distributions (Debian 13,
  Ubuntu 24.04, Fedora 43, RHEL 10) and the copies vendored in WezTerm on every
  system, kitty's Linux binary and Ghostty's Linux build.
- Chrome on Linux, ChromeOS and Android draws through Fontations, which passes
  a flip's two-byte payload on as a PNG, so nothing draws; "WebKit-Skia on
  Linux" in section 4 is WebKit only.
- Pillow 11.3 (FreeType 2.13.3) errors on a flip and Pillow 12.3 (2.14.3)
  toggles (MEASURED).
- The `mpdf` toggle is Gravity PDF's fork (pull request 292).
- HarfBuzz returns nothing for a flip, so LibreOffice's PDF export draws
  nothing visible and GTK4's GPU renderer draws a flip glyph as a silhouette in
  the text colour (READ, the last step inferred).

**The symbol face** (the owner's screenshot: Claude Code's mode arrows as
boxes, its bullet as a colour emoji). U+23F5 is in no face of baud's stack and
U+23FA only in Apple Color Emoji. Of every installed font only STIX Two Math
2.13 (OFL 1.1, byte-identical to upstream v2.13b171) has both as text, and
macOS's own fallback picks it for both (MEASURED). DejaVu Sans Mono 2.37, the
first Linux fallback, has neither. The unmodified file is 838,652 bytes; baud
embeds 3,718,412 bytes of fonts today. A subset would have to be renamed,
because STIX's licence reserves the name "TM Math".

**Jev, round four:** in the chosen strike 0.93 at 0.87; mirror once 0.67 at
0.51 (toggle 0.32); the two axes together, once in the chosen strike 0.45
against FreeType's toggle 0.35; the symbol face embedded 0.74 at 0.61 (hybrid
0.25, system font only 0.01).

## How the numbers were taken

- `scripts/strike-count-fonts.py` writes every hostile font above, and the
  CoreGraphics-loadable copies, into a directory outside the repository.
- `scripts/strike-count-cost.rs` times the walk and `scripts/strike-count-build.rs`
  the index; each header says how to lift `MAX_STRIKES` for a run and restore it.
- `scripts/strike-count-probe.swift` is the CoreText probe for strike counts;
  `scripts/sbix-probe.swift` in its `flip` mode measured the fixture's chains.
- The readers' and skeptics' own scripts and downloads lived in the session's
  scratchpad and are not kept; their sources are cited above by URL, commit or
  package.

**Two pins drifted.** `ms-opentype-cblc` and `ms-opentype-cbdt` no longer match
their pinned bytes (each fetch is 377 bytes short, as the `sbix` pages were
before their re-pin); quotes from them here are from the 2026-10-02 fetch.

## Where the decisions stand

All are the owner's. Jev's numbers by round, as "choice probability at
confidence":

| question | round 1 | round 2 | round 3 |
|---|---|---|---|
| strike bound on the walk | 255, 0.38 at 0.08; SPLIT 0.63 at 0.53 | SPLIT 0.58 at 0.44 | re-posed below |
| index or walk | -- | -- | index 0.98 at 0.97 |
| bound on the index's build | -- | -- | PAIRS and none tied at 0.43 (0.30); with where fonts come from and what no bound does at launch, PAIRS 0.78 at 0.72; counting `CBLC`'s records too (WORK), 0.95 at 0.94 |
| `CBLC` subtables | -- | binary search 0.58, with a cap 0.41 (0.44) | sorted records required 0.95 at 0.92 |
| flip, the old options | mirror once 0.91 at 0.81, then 0.88 at 0.76 | CoreText exactly 0.38 at 0.16 | split in two below |
| flip, how many mirrors | -- | -- | once 0.82 at 0.74; with both readings stated, toggle 0.52 and once 0.48 (0.27) |
| flip, where hops are read | -- | -- | across strikes as CoreText 0.60 at 0.20 |

What moved them, as far as the requests show (Jev returns no reasons):

- An option's arithmetic was wrong. PAIRS counted strikes x glyphs and missed
  `CBLC`'s subtable records, so it did not bound a `CBLC` build; corrected, the
  same question went from 0.78 to 0.95.
- Options that differed on two axes at once split the vote. Round two's
  subtable options S and SK both binary-search, and its flip options F and C
  both mirror once; asked one axis at a time, the subtable question settled.
- Context changed answers in both directions. The bound's first round-three
  request left out where fonts come from and what no bound does at launch, and
  adding them moved PAIRS from 0.43 to 0.78; the mirror request first left out
  the composition reading, and adding it moved "once" from 0.82 to 0.48.
