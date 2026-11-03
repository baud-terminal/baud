# AC. The embedded font, built from parts

**Status: open, researched 2026-10-07.** The direction is decided and
recorded below. The design questions it leaves are listed in section 6.
Nothing here is built yet.

Evidence is tagged as in the other notes: measured, source (file:line) or
inferred. The full research (the gap, the sources and the build design, with
a critic's checks; the community bridge, a merge prototype, and run-time
fitting) was kept outside the repository; this note is its record.

## 1. What it was decided

On 2026-10-07, after learning that no font baud has, the Nerd fonts included,
draws Hebrew (note AA, sections 6 and 8):

- "we need to create a dir structure so when we create the embedded font is
  being constructed with nerd patches and our patches like adding hebrew ...
  font/langueges/hebrew font/icons/nerd as an example , and maybe we can get the
  arielagor jetbrains mono hebrew ofl here embedded to add it as well sounds like
  a good compremise"; "this will allow us to extended the embedded font with more
  stuff".
- "if these symbols aren't found in the configured font then it should look at
  the system's font"; "our embedded font should have it correct so we don't
  custom patch something like this"; "i don't want to custom hack inheritence".
- "so we need baud font to win and we have a gap as you mention of 1000+ that we
  need to add to baud correctly"; "and everything it goes to menlo , its a gap we
  need to close".
- "just be sure not to copy from menlo and create our own so it looks better and
  align with jetbrains"; "we can have the license with the font"; "i don't want
  parts as we will create our own font patches like nerd".

So fonts resolve in one order with no custom inheritance hacks (read as no
run-time per-glyph rules; whether build-time lists count is decision 6): the configured font (by
default the embedded one), then baud's font built from JetBrains Mono, the Nerd
patch and baud's own patches, then the system's own choice for anything still
missing. baud's own patches are glyph sets baud draws to match JetBrains Mono;
nothing is copied from Menlo or taken as parts of other fonts. Whether "i don't
want parts" withdraws the earlier request for the bridge as a compromise has been
asked and is not answered (section 4 recommends against it). The project removed
the `fonts` list from their config the same day, so this Mac now runs the default
stack.

## 2. Where baud stands

- **The embedded faces** are JetBrains Mono 2.304 in four styles, Symbols Nerd
  Font Mono 3.5.1 and STIX Two Math (source, `baud-app/fonts/`). With no config
  the macOS chain is JetBrains Mono, the icon face, Menlo, Apple Color Emoji,
  Apple Symbols, Hiragino Sans GB, Apple SD Gothic Neo, Kohinoor, then STIX
  (measured with `BAUD_STARTUP`). Behind a configured stack baud appends only the
  icon face and STIX, not JetBrains Mono (`baud-app/src/config.rs:839-845`).
- **A character is drawn by the first face that maps it**, text before colour
  under `emoji = "font"` (`baud-render/src/stack.rs:371-381, 473-482`); a
  combining mark is drawn by its base's face (`renderer.rs:792`); box drawing,
  block elements and braille are baud's own (`boxes.rs:527-550`, 416 code
  points). Powerline separators are not procedural: JetBrains Mono and the icon
  face map them.
- **The Nerd icons overflow their cells today.** The icon face gives every glyph
  a one-em advance, and baud draws every face at the primary's em, so 10,590 of
  its 10,609 inked icons draw past a 0.6-em cell, the widest 1.10 em (measured).
  The run-time fitting by the patcher's rule (the generated patcher table, and
  Nerd scope C, which draws the fitted Nerd glyph over JetBrains Mono's own) and
  baud's own over-wide rule D are decided in the plan and not built for them;
  rule D is built for the letters of the joining scripts alone
  (`baud-render/src/fit.rs`, `docs/bidi.md`).
- **~~There is no run-time system fallback.~~** *Built 2026-10-26*: a
  character no listed face maps is asked of the system the first time a frame
  draws it -- CoreText's `CTFontCreateForString` from the
  primary's bytes on macOS, fontconfig's charset match over baud's own scan
  on Linux -- and the face answered is appended behind the chain
  (`baud-app/src/fallback.rs`). The primary's missing glyph stays only where no
  installed face has the character.

## 3. The gap

Measured with baud's own `Stack::select_presented` over every scalar value
(Regular style, text presentation), held to a Python re-derivation with no
mismatch, with no config and STIX kept as a face ahead of baud's fixed list of
system files (JetBrains Mono, the icon face, STIX, then Menlo, Apple Symbols,
Hiragino Sans GB, Apple SD Gothic Neo, Apple Color Emoji, Kohinoor). That list is
not the system's own run-time choice of section 1, which the build research
measured separately with the CoreText cascade (Menlo 1,587 there).

- **1,994 text code points still come from those system faces**: Apple Symbols
  1,058, Menlo 798, Kohinoor 94, Apple SD Gothic Neo 22, Hiragino 22. Then 40
  combining marks, 1,268 emoji, and about 40,000 Han, Hangul and other CJK. With
  STIX last, as in today's default, 3,728 text code points come from system
  faces: moving STIX up takes 1,734 of them, so whether STIX stays (decision 7)
  decides which figure holds.
- **By block**, the largest: Greek Extended 233 (Menlo), Latin Extended-B 130,
  Dingbats 122 (Menlo), Alchemical Symbols 116, Domino Tiles 100, Devanagari 94
  (Kohinoor), Tai Xuan Jing 87, Runic 81, Deseret 80, Enclosed Alphanumeric
  Supplement 80, Ancient Greek Numbers 66, Yijing 64, Phonetic Extensions 62,
  Playing Cards 58.
- **The "1000+" was a proxy**: 1,391 code points that Menlo and not FiraCode Nerd
  Font draws. Menlo minus JetBrains Mono is 1,596; minus JetBrains Mono, the icon
  face and STIX, 804 (measured).
- **Real programs barely reach it**: of 67,183 non-ASCII scalars in the six
  real-program recordings in `tests/recordings/`, only 7 fall outside the
  embedded faces (measured). **Claude Code's own interface does**: its spinner
  frames · and ✶ draw from JetBrains Mono, ✢ ✳ ✻ (U+2722, U+2733, U+273B) from
  Menlo, and ✽ from STIX once STIX is moved up, so one animation draws from three
  fonts, or from two (JetBrains Mono and Menlo) in today's default stack. Its ✦
  and ✔ also draw from Menlo, and ⑂ from Apple Symbols (measured, from its
  bundle). On Linux the Menlo ones come from DejaVu Sans Mono, a different design,
  or from nothing.
- **Hebrew and Arabic** are mapped by no face at all: 134 and 248 code points.
  Hebrew is in the embedded font since; Arabic, Syriac and every other script
  no listed face maps draw from the face the system answers (2026-10-26):
  Courier New for Arabic and Urdu from JetBrains Mono's bytes, the face
  CoreText also gives Menlo, and Noto Sans Syriac for Syriac (over renders of Courier New, Geeza Pro,
  SF Arabic and Noto Nastaliq Urdu).
- **Which blocks baud should cover itself is a scope decision.** The historic
  and game sets (alchemy, dominoes, runes, playing cards, Yijing and the like)
  come from Apple Symbols on macOS and differ or are missing on Linux, so leaving
  them to the system breaks identical pixels for them.

## 4. Hebrew: the bridge and a patch of baud's own

**The community bridge** (`arielagor/jetbrains-mono-hebrew` at 7a4a86d, OFL;
Alef's Hebrew merged into JetBrains Mono) is a four-commit proof of concept from
one day, built on an untagged JetBrains Mono 2.305 (measured). Through baud:

- its build drops JetBrains Mono's `GSUB` (every ligature) and replaces its
  `GPOS` and `GDEF`, so it can only sit behind the real JetBrains Mono, never
  replace it;
- shin and sin draw identically, the shuruk dot falls inside the vav, and points
  below the letters are clipped at the bottom of the cell (baud-render's CPU path;
  Metal untested);
- its points sit on their letters in baud only by two accidents: its marks are
  drawn pre-shifted to x = -300, which baud keeps because it draws static outlines
  at raw coordinates and ignores the left side bearing, and its `GPOS` lists only
  `hebr`, so baud applies no lookup and the mark stays at the pen. HarfBuzz honours
  the zero side bearing the bridge writes, which undoes the pre-shift. With
  Hebrew-script shaping in baud, every point moves one cell left (measured).

The request was for the bridge as a compromise; the research recommends against
it, for the reasons above.

**A patch of baud's own works as a mechanism** (a merge prototype into
JetBrains Mono, using Liberation Mono's Hebrew only to prove the pipeline;
measured through baud): points land on their letters (57 of 58 test clusters,
each rendered alone; holam on bet spills 2 px into the previous cell) when the
patch appends its Hebrew lookups to JetBrains Mono's existing `DFLT` and `latn`
`mark` and `ccmp` features and adds mark-to-base lookups for dagesh, the shin and
sin dots and holam haser. JetBrains Mono's own glyphs, advances and shaping stay
identical, and the result passes OTS. Its costs are shaping time and a merge per
style. Over 2,402 lines of Latin source text, uncached, on battery, in three
launches, the variant described slows baud's per-character shaping by 36.0-36.7%,
partial ones by 25.1-31.1%; folding its placements into one lookup costs about
4-5%, resolved in one launch of three. The effect on frame time, given the run
cache, is unmeasured. **Longer pointed text loses points whatever the font**:
baud-core kept at most 63 distinct clusters and freed them only on a full
reset, and `marks_dropped` was 45 over Genesis 1:1-5 (measured), a core limit to
fix before Hebrew can be called done. *Fixed 2026-10-14*: 1024 slots, eight
probes and a sweep of the dead, and 0 dropped over the five verses. The shipped patch needs baud's
own Hebrew drawings (section 1). Without the bridge, the interim under the
font order (configured, then embedded, then the system) is the system
fallback, once it exists: asked with
`CTFontCreateForString` for Hebrew behind FiraCode Nerd Font, CoreText answers
Courier New (measured 2026-10-07).

## 4a. Hebrew: the Cascadia-based prototype, four rounds

With the bridge and the generated letters rejected, a font survey put
Cascadia Mono's Hebrew (Liron Lavi Turkenich; OFL with the Reserved Font Name
"Cascadia Code") first. It is sans and gives the narrow letters long heads.
Against a refit Noto it was not a clear choice, and it needed improving a
lot. Four rounds followed on 2026-10-07. Rounds 1-3 each had an independent
critic; round 4 was re-measured without one. The build scripts, fonts,
renders and critic logs were kept outside the repository; this section is
the record.

**What it is.** Cascadia's wght-400 Hebrew from the GitHub release
(v2407.24), re-weighted to JetBrains Mono's stems (90/82), 630 units tall,
centred in the 600-unit cell, merged into a copy of JetBrains Mono Regular 2.304.
It appends one mark-to-base lookup to JetBrains Mono's `mark` feature and one
mark-to-mark lookup to its `mkmk` feature, plus one GDEF mark filtering set.
Gated on every output: JetBrains Mono's 1743 glyphs, `hmtx`, `cmap`, GDEF
classes, `GSUB` and first five `GPOS` lookups unchanged; OTS clean; Latin through
baud byte-identical to stock JetBrains Mono at 28 and 56 px.

**What the rounds did** (all measured through baud's scratch renderer and
HarfBuzz unless marked):

- All 20 points drawn with JetBrains Mono's dot (116 units); stock Cascadia
  Mono maps only 8, so sheva, the hatafs, hiriq, tsere, segol, qubuts, meteg and
  rafe draw as boxes (its `cmap`). Marks follow JetBrains Mono's convention:
  zero advance, ink placed by anchors. Unattached points sit at the heights of its
  U+0307 and U+0323.
- Below-points share one centre per non-descending letter: the spread is 0.0
  units, after round 2's anchor search had moved them 146-158 units sideways.
- Meteg attaches to the vowel before it through `mkmk`, 24 units to its left.
  A filtering set lets it step past a dagesh. baud places it where HarfBuzz does,
  checked at 200 px, and control fonts with the lookup unhooked fail as
  predicted. In round 4 it passes 217 of 297 letter-vowel-meteg clusters
  (round 3: 224; the 7 lost are on qof, below), by the round-3 critic's script.
- Two-row points have dot gaps of at least 40 and stay at or above -275,
  except as below.
- **baud places a mark on a whole pixel.** Moving sheva and the hatafs 10
  units lower under ג ט ל ס ש renders byte-identical to the unmoved font at 28 px
  and differs at 56 px (`cmp` on the PPMs). At 28 px, round 3's sheva touched ש ט ס:
  the pixel row between letter and dot read 93-127 on a 16-216 scale. So round 4
  puts those points on -293, one pixel lower at 28 and 56 px. The darkest row
  between them then reads 16-31, background or nearly. The cost is that the
  lowest dot reaches the line's last pixel row.
- Lamed's holam has two options 39 units apart: L1 beside the mast tip, 70.2
  units from a neighbouring shin's dot, and L2 above the mast inside the cell. At
  28 px L1 touches the mast's corner.
- Narrow letters, five variants: A is Cascadia's 292 head (0.49 of the cell),
  B a 372 head with a kotz, C a right foot, D a 312 head with a kotz on vav, yod
  and zayin, and E a foot on both sides. A width ladder put the point where vav
  starts to read as resh at about 348 (0.58). In the same reading B's vav reads
  as resh or dalet, C's as a Latin l and E's as a dotless i (E's zayin as I); A
  and D read as themselves, and A's vav still floats. All of it was judged by
  non-native readers from baud renders, so a native reader checks it.

**Still wrong:**

- Meteg fails on 80 of 297 clusters (23 since 2026-10-26, FONTLOG's CHANGELOG).
  With hataf segol it sits 20 units out of
  the cell, with qubuts 6, and on final letters it leaves the cell. On qof it
  lands on the leg with every vowel: round 3 had moved qof's vowels to the cell
  edge to make room, and round 4 put them back under the bowl.
- Rare combinations overflow the cell sideways: qof with hataf segol by 104
  units, final tsadi with qubuts by 140.
- After "1" an unattached holam vanishes into the digit, as JetBrains Mono's
  own U+0307 does.
- Only Regular existed after round 4; section 4b has the other three styles.
- Cascadia's `name` table carries a Microsoft copyright preamble (nameID 13)
  that ships with the font, which is open. The Reserved Font Name
  forces a rename.
- A page of pointed text still loses points in baud-core's cluster store,
  whatever the font: 4 on the eight test lines, drawn plain.

**After round 4**, weighed with the still-wrong list: the Cascadia base
first, ahead of a commissioned design, licensing Narkiss, a refit Noto and a
JuliaMono base. Part of its lead is that only Cascadia was iterated (Noto
refit had one round). On lamed the lean is L2; on narrow letters, A (then C
and D; B and E least). A first weighing that carried round 3's meteg count,
and described C's and E's feet as JetBrains Mono's way of filling i and l
without their misreads, leaned E on narrow letters; the framing moved the
narrow answer and not the base.

**A choice the project made.** Shown beside stock Cascadia, David Mono,
JuliaMono and the bridge, all through baud, baud's version read better
(2026-10-07): baud builds its Hebrew on this prototype. Narrow letters and
lamed's holam stay open (questions 1a and 1b, section 5). Cascadia's Bold carries
the same 39 Hebrew code points as its Regular; its Italic carries none, so the
Italic will be a slant.

## 4b. Hebrew in four styles, and which way italic leans

Built on 2026-10-07 in four steps: a build, an independent critic, a fix
round, and a second critic. The research on lean direction ran beside them.
The working files were kept outside the repository, and this section is the
record.

**What was built.**

- **Bold.** Cascadia's variable font at wght 700, its maximum and the closest
  to JetBrains Mono Bold's stems. Dilation and erosion take up the remainder,
  and the built stems measure 125 vertical and 113 horizontal, which are JetBrains
  Mono Bold's.
- **Italic.** The upright sheared 9° about y = 315, outlines and anchors in one
  transform. That is JetBrains Mono Italic's angle (`post.italicAngle` -9.0).
  Both leans are built: right, with the Latin, and left.
- **Bold Italic.** The Bold, sheared the same two ways.
- **Merging.** Each style merges into that style's own JetBrains Mono file.

There are 21 fonts, counting the A/D and L1/L2 variants. All 21 pass the
generalised gate (29 of 29 against their own style's JetBrains Mono, whose 5
`GPOS` lookups stay unchanged) and OTS. Latin through baud is byte-identical to
stock JetBrains Mono at 28 and 56 px in 42 of 42 renders. Regular's tables
equal round 4's except the `head` dates.

**baud's style path, proven.** The scratch renderer loads styled faces with
`Stack::with_roles`, the call the app makes, and writes an SGR into every cell.

- Stock JetBrains Mono under SGR 1, 3 and 1;3 is byte-identical to drawing that
  style's file as the primary face.
- With only the Regular Hebrew in the stack, bold and italic Hebrew are
  byte-identical to Regular Hebrew. So baud today draws them unsynthesised, as
  `Stack::effective` says.
- Styled Hebrew drawn through the merged files is byte-identical to the styled
  file drawn alone. Critic 1 checked all 11 styled builds; critic 2 made 72
  comparisons across 18 builds.

**Vowel points in Bold: not finished.**

- **The builder's Bold** used JetBrains Mono Bold's dot (134), a 30-unit gap
  between the sheva's dots and a -300 floor. Critic 1 found the sheva and hatafs
  touching ס, and in the right-leaning Bold Italic ש and ט, at 28 px: 71 to 89 on
  round 4's single-column measure, against Regular's 16-31. It also found a text
  gate failure on dagesh (21.1-24.0 against 25).
- **The fix round** shrank the vowel dot to 120 and the dagesh to 104, and split
  the sheva's rest position (-36) from its position under ג ט ל ס ש (-58).
  Every dipping cell then read 16 at 28 px, and no gate failed.
- **Critic 2** measured the gap between the two dots in two dimensions and
  found them fused, no dark row between them, under every letter:

  | Bold sheva, dot-to-dot | 26 px | 32 px | 38 px |
  |---|---|---|---|
  | fix round | 132 | 134 | 116 |
  | builder | 83 | 71 | 44 |
  | Regular | 109 | 66 | 16 |

  The fix round's single-column measure had counted the fused pair as one dot.
  It had also cleared the builder's fusion at 30, 36 and 42 px. On the Retina
  even sizes from 24 to 44 px, each version fuses at four. A fused sheva is a
  short bar, close to a meteg.
- **The italics** also got worse at 32 and 38 px. For example Italic right went
  52 → 94 at 32 px and 16 → 69 at 38 px, because their re-chosen floors moved
  the pixel phase.
- **Regular at 26 px**, baud's default size here, also touches under the dipping
  letters (135) and fuses its sheva's dots (109), by critic 2's measure.

**The third pass.** It scored three things together at every size from 20 to
56 px, with the ranking fixed before any search:

- the gap to the letter (critic 2's two-dimensional bottleneck);
- the gap between the sheva's dots;
- how much of the lower dot falls below the cell.

The thresholds: a gap of 40 or less is clean, 41-90 faint, above 90 touching. A
cut of or less is clean, up to 0.179 faint, and above that cut. 0.179 is
Regular's own value at 28 px.

- **What changed.** The Bold vowel dot went from 120 × 106 to 120 × 98, and the
  gap between the sheva's dots from 30 to 40. Under ג ט ל ס ש the sheva and
  hatafs now drop 26 units below their rest position, a whole pixel at every
  size from 20 to 56. The same geometry goes into both Bold Italics.
- **What stayed.** The dagesh (104 × 92) and every letter are unchanged.
  Regular and the regular-weight Italics are byte-identical to the fix round.

Critic 2's measure of the gap between the sheva's dots, worst over the probe:

| Sheva, dot-to-dot | 24 px | 26 px | 28 px | 32 px | 38 px |
|---|---|---|---|---|---|
| Bold, fix round | 109 | 132 | 48 | 134 | 116 |
| Bold, third pass | 62 | 124 | **35** | 80 | **35** |
| Bold Italic right, third pass | 48 | 123 | **35** | 80 | **35** |
| Regular (unchanged) | 37 | 109 | 55 | 66 | 16 |

- **At 28 px**, the project's size, the letter gap (16) and the gap between the
  dots (35) are clean in Bold and both Bold Italics. The cut is faint,
  against Regular's 0.179.
- **At 38 px** all three are clean.
- **At 32 px** the dots are faint (80). The Bold Italics' hataf segol reads 91,
  one grey level into touching.
- **At 26 px**, baud's default size here, the dots still fuse (124) and the
  lower dot is cut. The letter gap is clean (21).

**Why 26 px cannot be clean with round dots.**

- At 26 px baud's cell puts the baseline at round(26.52) = 27 and the cell
  height at round(34.32) = 34 (`baud-render/src/metrics.rs`). That leaves 7
  pixel rows below the baseline.
- Under ג ט ל ס ש, one of those rows is the letter's overshoot and one must be
  air. The remaining 5 rows must hold a dot, air and a dot.
- That fits only dots about 80 units tall. A dot that short holds Regular's ink
  only if it is about 150 wide, which is a dash.
- Regular reads touching, touching, clean at 26 px too. So the problem is
  two-row points at baud's default size, not Bold alone.

The three options for 26 px are:

1. as built;
2. no drop under ג ט ל ס ש, Regular's way, which in the simulator makes the
   letters touch at 20-26 px;
3. a short or dash-shaped dot, which changes the dot at every size.

**All three options, rendered** (fourth pass; candidate fonts, not gated). Each
cell gives letter gap / dot-to-dot / cut:

| Bold | 26 px | 28 px | 30 px |
|---|---|---|---|
| 1, as built | 21 / 124 / | 16 / 35 / | clean |
| 2, no drop at 26 px | 100 / 124 / 0.00 | as 1 | as 1 |
| 3, round dot 90 × 80 | 18 / 18 / 0.01 | 16 / 74 / 0.00 | dots 75 |
| 3, dash 150 × 80 | 18 / 19 / 0.01 | 16 / 84 / 0.00 | dots 92 |
| Regular, as built | 135 / 109 / 0.01 | 74 / 55 / 0.18 | |
| Regular, dash 150 × 80 | 39 / 16 / 0.02 | 16 / 78 / 0.00 | dots 86 |

Notes on the table:

- I re-rendered critic 2's probe with each candidate font. Every value
  matches the agent's, except two.
  - At 28 px the round dot reads 74 here against the agent's 77.
  - At 30 px option 3 reads 75 (round) and 92 (dash) on the plain sheva. The
    agent's measure, which also takes in the hatafs, reads fused at 30 px for
    both.
- **Option 2** also touches at 24 px.
- **Option 3:**
  - The round dot carries about 59% of Regular's ink. The dash keeps Regular's
    ink but draws every two- and three-dot point as short dashes, which shows at
    56 px.
  - Its Bold candidates also shorten the plain qamats to 192 units. Otherwise
    the hataf qamats, 224 tall, rises onto the letter.

**What was chosen** (2026-10-08, weighing every cost above):

- **26 px:** as built, rather than the round dot or the dash.
- **If a short dot is chosen:** Regular gets it too.
- **The dot:** 120 × 98, against a rounder 119 × 102.
- **The dagesh:** keep 104, rather than 106 or redrawing letters for an equal
  dagesh.

The 26 px choice and the dagesh stay open as questions 1d and 1e.

**Checked independently.** I re-rendered critic 2's probe on the third pass's
fonts with critic 2's own renderer. Controls of the fix round came out
byte-identical to critic 2's renders. Critic 2's scripts then give the same
dot-to-dot table and the same 8-connected regression list as the agent's
report. Every gate passes on all 21 fonts:

- the generalised gate, 29 of 29;
- OTS;
- critic 2's strip-and-compare gate, which fails a fresh 1-unit perturbation;
- Latin identical to stock;
- the style path, 72 of 72;
- no meteg cluster newly failing;
- no overpaint in 282 renders.

**What got worse against the fix round.** 45-53 size-metric pairs improved per
bold style. These got worse:

- At 20 px the cut went from clean to faint in all three bold styles. The
  26-unit drop rounds to a pixel there; the fix round's 22 rounded to none.
- Some marks fused where they sit side by side in one row: the hataf segol's
  two dots, and a hataf's bar against its dot. These went from faint to fused in
  Bold at 21 px, and in Bold Italic right at 20 and 29 px. Fusion of this kind
  also stands, unchanged, at 21-23 px in Bold and at 20-21 px in Bold Italic
  left.
- Single-row points under ג ט ל ס ש:
  - 34 cells crossed a line on critic 2's 4-connected gap. 34 did on the
    8-connected gap, plus הְ and רְ in Bold at 21 px (40 → 43). Most crossed
    by 5 grey levels or fewer: 25 of 34, and 30 of 36.
  - The largest are in Bold Italic left: שִׁ at 35 px, 87 → 115; לִ at 46 px,
    40 → 91; לֵ at 40 px, 84 → 99. Bold has one: סֵ at 35 px, 83 → 94.
  - The anchors did not move. These points have the flatter dot's top at the
    same height.

**What stays open.** The dot is 120 × 98, 3% less ink than Regular's and flatter
than JetBrains Mono Bold's own (width over height 1.22 against 1.13). Only dots
98 units tall read clean between the dots at 28 px. The dagesh, 104 × 92, is now
smaller than the vowel dot; the three prior-art fonts keep the two equal. 106 is
the ceiling, because 108 fails the clearance check in Bold Italic right.

**Meteg in the right lean.** Round 3's critic script (297 letter-vowel-meteg
clusters) passes:

| Regular | Bold | Italic right | Italic left | Bold Italic right | Bold Italic left |
|---|---|---|---|---|---|
| 217 | 172 | 125 | 261 | 55 | 236 |

The right shear moves points under a letter about 70 units left. JetBrains Mono
Italic's own dot-below sits 67 units left of its upright one, so that is the same
direction. Meteg, left of its vowel, then leaves the cell. The cell-fit step
moves the vowels' base anchors back into the cell but not meteg's mark-to-mark
position. The third pass left this unfitted: its counts are the ones above, and
Bold Italic right still passes 55.

**The fourth pass fitted it** (`fit_meteg` in the scratch build, right lean only).

- **How it works.** Where a vowel and its meteg together overflow the cell more
  than the upright pair did, that letter's base anchor for the vowel moves
  right. Three limits apply:
  - the vowel moves at most as far as the shear moved it at its own height,
    73-80 units;
  - the vowel never comes closer to its letter than before;
  - the vowel alone never overflows the cell more than before.
- **The meteg itself was already right.** Its mark-to-mark relation to the vowel
  was the upright one, sheared: 23.7 units to the vowel against 24.0 upright. The
  shear carries vowel and meteg left together.
- **Why not move the vowel only before a meteg.** baud's shaper takes GPOS types
  4, 6 and 9 only, and the gates hold GSUB byte-identical to JetBrains Mono's.

I re-ran round 3's meteg check on the fourth pass's fonts:

| Meteg passes, of 297 | Regular | Bold | Italic right | Italic left | Bold Italic right | Bold Italic left |
|---|---|---|---|---|---|---|
| third pass | 217 | 172 | 125 | 261 | 55 | 236 |
| fourth pass | 217 | 172 | **185** | 261 | **143** | 236 |

- **No cluster newly fails** in any of the 21 fonts. I compared every table
  against the third pass's fonts. Only the 8 right-lean files differ, and only in
  `GPOS`, `head` and `name`; the other 13 are table-identical.
- **The right lean still trails upright.** It fails 32 (Italic) and 29 (Bold
  Italic) clusters that upright passes, all with meteg out of the cell, and
  passes none that upright fails. The left lean passes 46 and 66 that upright
  fails.
- **In all 27 sheva clusters of the right lean, meteg's box is not left of the
  sheva's.** The slanted lower dot reaches further left.
  - In 26 of them the 20-unit clearance holds (23.7 units to the sheva), so
    this is the boxes overlapping, not ink touching.
  - With ק, meteg lands on the leg, as it does with every vowel in Regular.
- **The cost.** A base anchor serves its letter and vowel with or without a
  meteg. So 106 (Italic) and 166 (Bold Italic) of the 297 letter-vowel anchors
  moved right in ordinary pointed text, by 2-78 units, median 54 / 62. Those
  vowels sit roughly where the upright puts them, instead of following the
  slant.
  - On the probe, 68 cells got better and 21 worse in Italic right, and 51 / 11
    in Bold Italic right. The per-size categories are unchanged.
  - The worst cells at 26 px are שֱׁ in Italic right, 82 → 103, and שֻׁ in Bold
    Italic right, 87 → 98.
- **A looser fit,** letting the vowel come closer to its letter, would give 210 /
  157 at 77 / 65 worse cells. That was simulated only.
- **Gates on the changed files:** the generalised gate 29 of 29, OTS, critic 2's
  gate (which fails a fresh perturbation), the clearance sets, overpaint, the
  style path and Latin identity all pass.
- **Keeping the fit** (2026-10-08, weighing the costs above): keep the
  strict fit, over a looser fit and no fit. Settled.

**Which way italic Hebrew leans: the evidence.** Each lean was measured as the
stem angle against the font's own upright, with JetBrains Mono Italic reading
+9.0 as the positive control.

- **Latin-first families that added Hebrew lean it right, at the Latin angle.**
  That holds for 11 lineages: Monotype's Arial, Courier New and Times New Roman;
  Liberation/Arimo/Cousine/Tinos, whose Hebrew outlines are not Monotype's;
  Open Sans; Rubik (22 letters an exact 12° shear); Google Sans; FiraGO; DejaVu
  Sans; Libertine/Biolinum; Bona Nova; GNU FreeFont; and Everson Mono. In a
  Latin-first family the shear records what the build did, not necessarily a
  judgement about Hebrew.
- **Hebrew-first families split.**
  - Right: Maxim Iorsh's Culmus obliques (Aharoni, Frank Ruehl, Miriam Mono,
    Nachlieli), and Monotype's Hebrew-only New Peninim MT Inclined (about 16°,
    while its `post.italicAngle` says 0).
  - Left: Yoram Gnat's four Culmus families, and Iorsh's David CLM Italic. In
    Simple CLM the Hebrew is the upright sheared exactly -13° while the Latin in
    the same file leans +13°.
- **The monospaced fonts.**
  - Right: Courier New, Cousine/Liberation Mono, Miriam Mono CLM, FreeMono and
    Everson Mono.
  - Left: JuliaMono, 5°, against its Latin's 10° right.
  - Upright: David Mono and the JetBrains Mono bridge.
  - No Hebrew in any italic: Cascadia's 125 italic faces.
- **Writers for right:**
  - Bruno Maag (Ubuntu's Hebrew, 2010) concedes that reading-direction logic
    points left, but chose right because Hebrew sits beside Latin and its digits.
  - John Hudson leaned Adobe Hebrew Italic right for bilingual harmony and shared
    punctuation (2008). He added that dalet and resh "become attenuated" when
    leaning left (2013).
  - Michal Sahar (2017) followed right-handed handwriting.
  - The Shuneet designer (2016) switched to right because Ctrl-I gives Israelis a
    right lean.
- **Writers for left:**
  - AlefAlefAlef's Yaki Hasid (2013) writes that the Hebrew slant leans left, and
    that a right-leaning Hebrew is *muteh* rather than *natui*. Avraham Kornfeld's
    Frank-Ri (2019) leans everything left 14°, its Latin included. Both
    translations are a non-native reader's and unverified.
  - Ismar David's 1950s David Cursive leaned left.
  - Jonathan Rosenne (2009) wrote "forward", read here as left.
- **Neutral:**
  - W3C's r12a calls it a personal choice.
  - The CSS Working Group resolved in 2023 (csswg-drafts#8914) that an engine
    which synthesizes oblique SHOULD offer both directions, and in 2025 to add
    left/right keywords.
- **Terminals.** Every synthetic oblique read at source shears right without
  consulting the script: FreeType, fontconfig, Blink, WebKit, iTerm2, WezTerm,
  ghostty and Wine's DirectWrite. On this Mac, kitty's italic Hebrew falls back
  to Courier New Italic (right, 12°). ghostty and WezTerm draw it upright.

**What it means for baud.** baud's digits, quotes and Latin come from JetBrains
Mono Italic, which leans right, and are gated unchanged. A left lean therefore
puts both directions on one line, as in `git commit -m "תיקון באג"` or
`v1.2 בְּרֵאשִׁית`. The two remedies left-leaning fonts used are both
unavailable: Frank-Ri leaned its Latin left, and Khaled Hosny kept digits and
punctuation upright.

**The evidence does not settle the default lean.** The Latin-first families
and the matched angle argue for right; the Hebrew-first tradition and the
meteg counts argue for left. Both leans were re-rendered from the fourth
pass's fonts for question 1c, for a native reader to decide.

## 4c. The italics, built (2026-10-24)

Question 1c was answered on 2026-10-24: italic and bold italic Hebrew lean
right, with the Latin. A choice of 2026-10-07 asks for the italic optically
corrected like JetBrains Mono's italic rounds, not a plain shear. So the question became what JetBrains Mono's italic does to its rounds.

**What JetBrains Mono's italic does, read at source.** Its italic masters
(`[[source: jbm-glyphs-italic]]`) carry italicAngle = 9 and are drawn as their own
outlines. Read node for node against the upright masters
(`[[source: jbm-glyphs-upright]]`):

- The italic is the upright sheared 9 degrees about y ~ 278. On 26 compatible
  letters and figures, the on-curve nodes sit 0.4 units off the sheared upright on
  average horizontally (ExtraBold 0.3). a, e, f, y, Q and nine are redrawn, not
  sheared. (A first count said 29 letters; the tool measures 26.)
- Stems keep their horizontal width (90) and bars their height (82). The combining
  dots are the sheared upright.
- **The bowls' joints move.** Of the 350 ends of the Regular master's 175
  right-angle round corners, 10 move more than 3 units along their straight. All
  are a bowl's curve/straight joint, in o, O and q: o's outer 12.6-12.7 units
  (212 -> 200, 338 -> 351), O's outer 10.1 (210 -> 200, 521 -> 531), q's inner -10
  (190 -> 200, 360 -> 350). In ExtraBold only O's outer moves, 10.1; o is pure
  shear there. The acute and the obtuse corner move alike (12.6 and 12.7), so the
  move is no function of the sheared angle, and no kind's mean exceeds 0.6 units.
  What it does is level the joints. Paired side by side, five upright bowls have
  an outer straight that stops short of the inner one across the stroke: o (10
  units), O (9-10), q (4-5), g (4-5) and e (10); in ExtraBold, O (10). The italic
  levels o and O (the outer joints move to the inner ones' heights) and q (its inner
  joints move in by 10), each onto a joint height the family already uses (200/350
  as b, d, p and c; 200/530 as C and G). It leaves g short (pure shear) and e (redrawn
  with the same gap). So it is a correction made on the roundest bowls, not a rule
  every italic bowl obeys (`docs/research/notes/ac/italic/invariant.py`). The
  effect is optical. The shear thins a bowl's obtuse corner (o: 78.5 units along
  the diagonal, against 92.6 at the acute), and the italic has 3.1-3.2% more ink
  there than the sheared upright, 0.2-0.6% more at the acute.
- **The handles at round corners change.** The shear makes a corner 81 or 99
  degrees. Against the upright (a shear keeps the ratio of handle to tangent
  distance), the fit through 1 is `k = 1 + K (theta - 90)`, with K by what meets
  the corner: 0.00022 with a stroke on both sides (14 corners, 1 corrected), 0.00091
  with one (132), 0.00104 curved on both (29). ExtraBold: 0.00018, 0.00112, 0.00073.
  The bowls carry it (o O C G b d p q s and the figures, up to 0.94 and 1.065). The
  arches of n, h and m are pure shear.

The scripts, run on the two pinned sources, and their output are in
`docs/research/notes/ac/italic/`: `docs/research/notes/ac/italic/ends.py` (the arc
ends), `docs/research/notes/ac/italic/joints.py` (the joint heights),
`docs/research/notes/ac/italic/jbm_nodes.py` (the nodes and the handle fits) and
`docs/research/notes/ac/italic/diag.py` (the diagonal thickness).

**Built.** `baud-app/fonts/glyphs/languages/hebrew/build/optical.py` does both, in
that order, before the shear.

- Hebrew's bowls stop short of level by more than JetBrains Mono's ever did: 6.6 to
  31.2 units in Italic (samekh 31.2 on both sides, tet 26.9 and 6.6, shin 26.9 and
  23.6, mem 7.6) and 23.2 to 45.5 in Bold Italic (samekh 45.5, shin 37.1, mem 23.2),
  measured on each weight's own outlines. A level is not a magnitude, so nothing is
  interpolated toward ExtraBold. Each outer joint moves toward its inner one by the
  gap, at most 13 units, JetBrains Mono's largest move. The arc is scaled along its
  straight, so the joint stays on it and both ends keep their tangents.
- Each right-angle round of a letter (43 arcs in Italic, 39 in Bold Italic) takes
  its kind's handle fit. A straight piece of zero length no longer counts as a
  stroke: Cascadia's Bold outlines start contours on runs of them, and the first
  build read Bold samekh's and mem's bowl corners as corners between two straights,
  so they took that kind's smaller slope. Italic uses the Regular master's K. Bold Italic is wght
  700, interpolated three quarters toward ExtraBold.

Points stay plain-sheared, as JetBrains Mono's dots are. With both parts off the
build reproduces the earlier gated plain-shear files byte for byte
(`scripts/build-fonts.sh --italics`). Against the plain shear, the correction moves
12 Italic and 14 Bold Italic glyphs. The levelling moves samekh, tet, shin and mem in
Italic by up to 13 units, and samekh, shin and mem in Bold Italic. The cell fit
re-runs against the corrected outline and moves two point anchors: hataf segol
under tet in Italic (x 246 to 248) and under shin in Bold Italic (x 256 to 268).
Meteg passes 185 and 143 of 297, the same as the plain shear's.

**How far.** The handles first. Three candidates were rendered: A, the fit
over all 175 corners, as one K; B, the fit over the 53 corners JBM visibly
corrected (0.00266); C, each kind's own fit. B over A at first, but the split
by kind showed that B gives Hebrew's stroke-to-stroke corners about 12 times
what JetBrains Mono gives its own, so C: each corner gets what JetBrains Mono
gives a corner of its kind.

An early statement that the italic departs from the shear only in the
handles, with Hebrew's arc counts as 88 and 95, was wrong on both. The joints
were weighed separately, with renders: C (handles only), D (level in full)
and D13 (level by at most 13). C was rejected. Against the plain shear, D thickens Hebrew's obtuse
corners 4.6-5.1% and its acute ones 3.7-4.0%. D13 gives 1.4-3.0% and
0.5-0.7%, which is JetBrains Mono's pattern, so D13. Two further statements
were wrong and were corrected: that no point anchor moves (two do), and that
every JetBrains Mono italic bowl ends level (g and e do not). Neither touches
the diagonal thickness, which is what separates D from D13. The four renders
it rests on are
`docs/research/notes/ac/italic/overlay_full.png`,
`docs/research/notes/ac/italic/overlay_capped.png`,
`docs/research/notes/ac/italic/overlay_bold.png` and
`docs/research/notes/ac/italic/sheet28.png`, made by
`docs/research/notes/ac/italic/overlay.py` and
`docs/research/notes/ac/italic/italic_sheet.rs` (built as an example of baud-render).

**Visible effect.** At 28 px through baud's renderer, the handle correction alone
changes 497 pixels of a 5-row sheet against the plain shear, by at most 17 of 255 in
a channel. Levelling by 13 changes 345 more, by at most 17 (Bold Italic 259, 16). At
large sizes the bowl bottoms of samekh, tet and shin read fuller at the obtuse
corner.

## 4d. Dalet and zayin, kept apart (2026-10-24)

**The defect.** As round 1 left them, ד (U+05D3) and ז (U+05D6) were each a bar over
one leg, the roofs 418 and 380 wide, both centred on the cell, and dalet's leg 64
units right of zayin's (Bold 60). Round 1 had made them so: it evened dalet 458 ->
418 with the other wide letters and widened zayin's head 336 -> 380
(`baud-app/fonts/glyphs/languages/hebrew/build/design.py`, EDITS). At 14 px, baud's
rasteriser put the two legs' offsets under their roofs 0.80-0.92 px apart over the
four styles, and "זו" read "דו" (found by the bidi wrap research).

**What governs.** The Unicode chart's reference glyphs are "not prescriptive"
(`[[source: unicode18-chart-0590]]`, page 1); its ז is narrow, its ד as wide as ר
with the leg at the right. The rule for the letters' form is "no letter should
resemble another" (`[[source: shulchan-arukh-oc-36]]`, 36:1), and the scribal rules
name what keeps each pair apart (`[[source: sofer-alefbet]]`): dalet's roof long and
its corner square "so that it doesn't look like a resh", with "a good heel"; zayin's
head passing "over on both sides so it doesn't resemble a vav". Design Museum Holon
lists dalet/resh and vav/zayin among the sister letters whose distinction a Hebrew
typeface's success rests on (`[[source: dmh-letters-typefaces-with-character]]`).
These are identity criteria, not proportions for a monospaced sans.

**Prior art, measured from the outlines.** In Arial Hebrew, Noto Sans Hebrew, Rubik,
Heebo, SF Hebrew and Courier New the dalet roof is 1.48-1.85 times the zayin head,
and in the proportional sans faces the roof passes dalet's leg by about one stem
(0.95-1.24). A monospaced cell cannot give dalet that width, so its leg's place has
to carry the distinction.

**The options**, all built and drawn through baud
(`docs/research/notes/ac/dalet/options-14.png`, the words at 14 px in four styles;
`docs/research/notes/ac/dalet/options-120.png`, the letters at 120 px): keep; H45
and H30, the leg moved right until the roof passes it by 45 or 30 units; W40H60 and
W40H45, the roof widened back to 458 with heels of 60 or 45; Z40, zayin's head
shortened to 300. Legs apart at 14 px: H45 1.41-1.51 px, H30 1.61-1.72, W40H60
1.49-1.59, W40H45 1.64-1.80; Z40 alone 0.83-0.90. Moving dalet's leg right brings it
toward resh's, whose leg ends the roof (dalet against resh: 1.88/1.81 px as built,
1.24/1.18 with H45, 1.07/1.00 with H30, Regular/Bold); in the renders a heel of 15
or 0 made ד read as ר, and Z40 makes zayin's head as wide as vav's (300 against
292).

**Weighed** with four renders, and then with the dalet-resh, 26 px,
vav-zayin and other-pair measurements: H45, zayin kept, no other pair
redrawn. The first sheet showed the italics at 14 px only; the committed
sheets add them at 12 and 16 px, and no pair there loses its feature. That
sheet left out the pair the change moves dalet toward besides resh: vav, a 292-wide head over a right-end leg with no heel. With the leg
at the right end, ד and ו are told apart by width and heel, 418 against 292 (1.76 px
at 14 px) with H45 or H30 and 458 against 292 (2.32 px) with the W40 options; the
shift-minimised coverage difference between them, a before/after screen only (it
ranks ח/ם closest in the font), is 0.154-0.156 as built, 0.122-0.129 with H45 and
0.157-0.218 with W40 (Regular, Bold). Weighed again on the dalet alone with
that premise and a render of words where the two touch, the lean is taken:
**H45**. With it the roof keeps its width, and dalet's leg stands further
from resh's than with H30. Zayin stays variant A's, so open
question 1a is untouched, and no other pair is redrawn.

**Built** as round 5, `baud-app/fonts/glyphs/languages/hebrew/build/distinct.py`:
dalet's leg and the roof's underside where it meets the leg move right until the
roof passes the leg by 45 units, in the frame round 1 edits in, so Bold reads its
own leg and the italics shear the result. Each style changes in that glyph, its
anchors and head's checksum alone. The dagesh moves 24 units right (Bold 32); in the
italics the cell fit moves hataf segol 58 units right (Bold Italic 74). OTS passes;
gate.py and cgate.py print the same lines as before; the meteg check passes the same
217, 172, 185 and 143 clusters; the italics still differ from the plain shear in 12
and 14 glyphs. The legs stand 1.41-1.51 px apart at 14 px
(`zayin_and_dalet_legs_stand_apart_at_14_px`).

**The other sister pairs** at 12-16 px, four styles
(`docs/research/notes/ac/dalet/groups-before.png`,
`docs/research/notes/ac/dalet/groups-after.png`): each keeps a feature it is told
by. He's gap is 200 units (2.8 px at 14 px; Bold 183), final mem closes its base
where het is open, bet's base passes its leg by 82 where kaf stops at a round
corner, gimel's foot stands apart from its leg where nun's base runs whole, samekh's
corners are rounded 58-61 units in where final mem's lower ones are square, tav's
foot is 157 units against he's 90, ayin's base stops at 400 where tsadi's runs to
495, vav's head stands left of its leg where zayin's passes over both sides, and the
final forms descend 180. Dalet now stands nearer vav and resh than it did (above).

**Found, then fixed (2026-10-25).** Bold's and Bold Italic's zayin head ends slanted
22 units: round 1's widening moved the points above y 540, and Bold's head underside
is at 517, so only the top corners moved. The band now starts at y 500, under both
weights' head, so each end moves whole and stands square, as Regular's does; Regular
and Italic are byte for byte as before
(`zayin_head_ends_stand_square_in_every_style`). The 12-16 px words and the
letters at 200 px before and after are in `docs/research/notes/ac/zayin/`.

**Dalet and resh, kept apart by the corner (2026-10-25).** Round 5 put dalet's leg
at its roof's right end, where resh's stands, so their legs' offsets under their roofs
are 1.24 px (Regular) and 1.18 px (Bold) apart at 14 px, under the 1.26 px the zayin
test asks. The scribal rule names the corner, not the leg: dalet's square "so that it
doesn't look like a resh", with a heel. Read at 14 px as the ink filling a 60-unit
square at each letter's outer top-right corner, the italics' lean taken out, dalet's
is 0.72, 0.65, 0.64 and 0.74 full (Regular, Bold, Italic, Bold Italic) and resh's
0.06, 0.09, 0.03 and 0.00; vav's 0.58-0.76, kaf's and final kaf's 0.00-0.09, bet's
0.00 (12 px: dalet 0.37-0.50, resh 0.05-0.13; 16 px: 0.71-0.85 and 0.03-0.11). The
option of a rounder resh, both corner arcs 50 units larger (R50, drawn only as a
render), takes resh's square from nearly empty to empty and its top past kaf's and
final kaf's, which share its corner now. Rendered at 12-16 px in four styles and at
200 px (`docs/research/notes/ac/resh/`). Decided: KEEP. Resh is unchanged, and
`dalet_corner_is_square_and_resh_corner_round_at_14_px` holds dalet's square at least
half full and resh's at most a quarter, in all four styles.

## 4e. Zayin narrowed from the left (2026-10-28)

**The defect.** Round 5 set the legs apart and left zayin's head 380 units wide (Bold
394) under dalet's 418-unit roof (Bold 438), 1.10 times it, so at large sizes ז still
read as ד: at 26 px the head was 9.9 px against a 10.7 px roof, at 12 px 4.6 against
5.0. Cascadia's own Hebrew, read from its wght 400 instance, has the roof 983 units
against a 720-unit head, 1.37 times it; round 1's widening of zayin (336 -> 380) and
evening of dalet (458 -> 418) took that to 1.10.

**What governs.** The scribal hand writes "The letter Zayin should be 1 X 3 kulmusim"
and "The letter Vov should be 1 X 3 kulmusim", a kulmus being "the width of the tip of
the quill", and dalet's roof "3 kulmusim long" (`[[source: hasofer-alphabet]]`): zayin
is in vav's width class and dalet's roof three times as wide. Zayin's head passes
"over on both sides so it doesn't resemble a vav" (`[[source: sofer-alefbet]]`), and
no letter may resemble another (`[[source: shulchan-arukh-oc-36]]`, 36:1). Type sorts
the letters into width classes too: Monotype's Hebrew matrices "had letters in only two
widths, full width and one-half width", and a type designer's review sets letters at
"3/4 of the square, rather than 4/4" (`[[source: typedrawers-hebrew-critique]]`; the
thread does not say which letters are half width). Vav and zayin are sister letters
whose distinction a Hebrew typeface's success depends on, as dalet and resh are
(`[[source: dmh-letters-typefaces-with-character]]`). These fix the class, not a
monospaced sans's proportion: the design gives the narrow letters heads that fill their
cells. No typographic source read states zayin's width against dalet's; the scribal
widths and the measured faces are what set the pair here.

**Prior art, measured from the outlines.** Dalet's ink is 1.48 (Arial Hebrew, Arial),
1.56 (Courier New), 1.57-1.68 (Lucida Grande, SF Hebrew, New Peninim, Raanana) and 2.09
(Tahoma) times zayin's. In five of six faces zayin's head passes its leg by more on
the right than on the left (Arial Hebrew 79 and 142 per 1000 em, Arial 78 and 142);
Courier New, the monospaced one, passes it evenly.

**The options**, built through the build and drawn through baud at 12, 14, 18, 26 and
32 px in four styles (`docs/research/notes/ac/zayin/options-words-upright.png`,
`docs/research/notes/ac/zayin/options-words-italic.png`,
`docs/research/notes/ac/zayin/options-letters-26-32.png`,
`docs/research/notes/ac/zayin/options-letters-12-14.png`). Read from the rasterised
bars, the italics' lean taken out, at 12-32 px, the least over sizes: K, keep: the roof
1.08 times the head and 0.40 px wider, the head passing the leg by 1.39 px on the right
and 1.84 on the left (Bold); L340, the left end in 40: 1.21, 0.90 px, 1.48 and 1.29;
D307, both ends in to Cascadia's ratio: 1.34, 1.34 px, 1.00 and 1.37; L300, the left
end in 80: 1.37, 1.51 px, 1.50 and 0.62, passing the leg 81 units left and 129 right
(Bold 67 and 122); LN300, L300 with the neck's reach halved: 1.37, 1.43 px, 1.55 and
0.72. Narrowing from the right would move the leg toward the
head's right end, where dalet's and vav's legs stand, so no right-only option was
offered. A first ask also showed S330 (both ends in 25) and LN340.

**The review**, with the four renders, in both orders each time: L300 at probability
0.56 and 0.58, confidence 0.47 and 0.50; re-asked once with Cascadia's ratio, D307 and
the vav measurements: L300 at 0.89 and 0.91, confidence 0.86 and 0.89. That ask's
overhangs read the leg from the median of a few inked columns, which mis-reads a 1-2 px
stem (D307's Bold right overhang as 0.51 px); asked again with the figures above, the
request's summary of the earlier research still carrying the 0.51: L300 at 0.82 and
0.82, confidence 0.77 and 0.77. Under the bar each time, so the lean is
taken: **L300**. It changes what narrow-letter variant A
means for zayin; vav, yod and final nun are as A had them, and the call stays open.

**Built** as round 6 (`baud-app/fonts/glyphs/languages/hebrew/build/distinct.py`):
every point of the head's left end, above y 500 and left of the leg, moves 80 units
right; the leg, the neck and the right end stay, and the glyph re-centres 40 units
left. Each style changes in zayin's and זּ's outline and side bearing, their base
records in GPOS and head's checksum alone; OTS passes. The legs now stand 1.93-2.02 px
apart at 14 px (1.41-1.51 before). `dalet_roof_is_wider_than_zayin_head_from_12_to_32_px`
holds the roof at least 1.3 times the head and a pixel wider, and
`zayin_head_passes_its_leg_where_dalet_roof_ends_at_its_heel` holds zayin's head past
its leg by a Regular stem on the right and 40 units on the left, and dalet's heel at
60 units at most, at every size from 12 to 32 px.

Found, not fixed: the scribal source asks for vav's top right to be "completely
rounded, because if it's squared it would look like a Zayin"
(`[[source: hasofer-alphabet]]`); baud's vav has a square corner.

## 4f. Devanagari from Noto Sans Devanagari (2026-11-02)

**What it is.** The four built faces carry Noto Sans Devanagari 2.007 (OFL 1.1, no
Reserved Font Name) as an interim, as the Hebrew began from Cascadia's: baud's own
Devanagari, designed for the grid, is the end state. `scripts/build-fonts.sh` fetches the
variable font by the URL and sha256 `baud-app/fonts/glyphs/languages/devanagari/SOURCE`
pins, instances it at wght 400 (Regular, Italic) and 700 (Bold, Bold Italic) with its
width axis kept, and `baud-app/fonts/glyphs/languages/devanagari/build/merge.py` merges each instance
after the Hebrew stage's glyphs and lookups; FONTLOG.txt, THE DEVANAGARI, lists what each
table gains. LICENSE names the four files, so no new font file was added.

**Choices, each against its alternatives.**

| choice | taken | against | why |
|---|---|---|---|
| where the width axis lives | a variable `wdth` axis in each built face, `HIDDEN_AXIS`, Devanagari only | static narrowed glyph sets reached by private features; the wdth-100 glyphs scaled horizontally | OpenType's registered `wdth` text names this use ("select a width variant ... to fit a span of text into a target width"); the steps would snap and need unregistered features; scaling is not the design's narrowing |
| scale | 1.000, Noto's units | 1.026, JetBrains Mono's x-height over Noto Latin's | the headline sits 15 units (2.4%) lower against the Latin than Noto designed, 0.2 px at 14 px; any other scale rounds every point, advance, anchor and delta and gives up shaping exactly as the pinned file |
| the italics | upright Noto at the face's weight | a 9-degree plain shear | Noto draws no italic and baud synthesises no style; a plain shear distorts Noto's rounds where the Hebrew's shear was corrected for them. Open: a slanted Devanagari |
| the dotted circle in a Devanagari run | Noto's, by one substitution under `nukt` | JetBrains Mono's with Noto's anchors, as they are or mapped onto its ring | the shaper inserts U+25CC from the cmap after `locl` and `ccmp`; Noto's mark lookups are made for its own circle; the substitution keeps shaping equal to the pinned file on every line |
| `post.isFixedPitch` | 0 | JetBrains Mono's 1 | "Set to 0 if the font is proportionally spaced" (OpenType `post`); the Devanagari is |

**What was measured.** Each style keeps every existing glyph's stored bytes and metrics,
every cmap entry, every lookup compiled alone, every language system's features, every
GDEF class and mark set (digests read from the files before and after). GSUB keeps stock
JetBrains Mono's lookup block byte for byte; 772 bytes of its old LangSys and FeatureParams
tables lie inside that block and stay there unreferenced. With hb-shape 14.5.1 on the 399
lines of `baud-font/tests/shape/devanagari.txt`, the built Regular shapes as the pinned
static Noto Regular glyph for glyph under one renaming (only the space keeps the face's
own glyph and advance), and as the variable font at wdth 62.5 and 81.25; Italic the same
at 62.5; Bold is within one unit of Noto's own static Bold on 7 lines (two glyphs, the
rounding of an instance at wght 700) and within 2 units of the variable font at wght 700
and wdth 62.5. The built faces grow from 304-312 KB to 585-595 KB each: Regular
303,608 to 584,584 bytes, Bold 308,912 to 591,656, Italic 307,400 to 588,292 and Bold
Italic 311,876 to 594,540. Startup's cost of the larger faces and of GSUB's 632 lookups (626 in the italics)
is unmeasured.

## 5. The build

A design checked against the repository and a working prototype (fontTools
4.63.0, in scratch):

- **The tree**, under `baud-app/fonts/`: `base/jetbrains-mono/` (today's four
  files, unmodified), `icons/nerd/` (a pinned Nerd release, its licence, a
  selection), `languages/hebrew/`, `symbols/<set>/`, each with a pinned source, a
  licence, a selection of code points and a fitting rule; and `built/` with the
  four built styles, a manifest of every input's and output's hash, and the
  built font's own licence carrying every component's copyright line.
- **The tool**: a fontTools script run by a developer, its outputs committed and
  held by tests. The prototype (fontTools 4.63.0 from an unpinned `<nixpkgs>`) is
  byte-identical across reruns and hash seeds on this Mac (measured); under
  devenv's pinned nixpkgs and across machines it is unmeasured. This design said
  the build must pin `USE_HARFBUZZ_REPACKER=False`, which changed the bytes in
  the merge prototype (measured). The Hebrew build pins it True instead: with
  False, or with no uharfbuzz, GPOS comes out different (same length, equal XML)
  and the outputs miss the gated bytes (section 5a).
  Building in `build.rs` would put Python in every build for no gain; a Rust
  writer extending `baud-font`'s `bake.rs` is possible but needs `cmap`, layout
  and `GDEF` merging, with the script as its oracle.
- **The icons** taken from Nerd's own JetBrains Mono release, whose patcher has
  already fitted them to JetBrains Mono 2.304's cell. The prototype with the
  icons in the Regular only and Liberation's stand-in Hebrew in all four styles
  totals 3,439,636 bytes, against 3,718,412 for today's four JetBrains Mono files
  and the icon face (STIX's 838,652 extra either way); with the icons in every
  style, 10,259,096 (measured). Running the patcher ourselves is deterministic on
  one machine but differs from the release in 274 code points, for a reason not
  established (measured).
- **The order**: the whole built font appended right behind a configured one
  (`config.rs:839-845`), and the system asked at run time. On macOS, reading
  `CTFontCopyDefaultCascadeListForLanguages` once at launch costs 2.5-3.0 ms cold
  and about 30 us warm and needs no renderer change; per character,
  `CTFontCreateForString` costs 67 us on average over 157,898 code points and
  1.6-1.7 ms on a cold first call (measured; no per-call maximum was taken), and
  needs faces added mid-session, which the renderer does not support today
  (source: `stack.rs:288-296`, `cellmap.rs:179-196`). On Linux, `FcFontSort` through a link
  dependency, `dlopen` or `fc-match`, all unmeasured.
- **Tests**: input and output pins; a coverage manifest per style (the prototype's
  Regular maps exactly 1,363 + 10,607 + 133 = 12,103 code points); JetBrains Mono
  untouched (no changed outline or advance; baud's shaper gives identical output on
  33,682 recording runs, none with a combining mark, and on 14,283 letter-and-mark
  clusters checked separately); a pixel test per component; the licences shipped; and a
  determinism check that rebuilds and compares.

## 5a. The Hebrew build, built (2026-10-08)

Hebrew typed into baud drew every letter as JetBrains Mono's `.notdef` box.
The Cascadia-based Hebrew (section 4a) had been gated in four styles since
2026-10-07, but it was never merged into the font the app loads. This section is
what was built that day.

**The tree**, the interim layout:

- `baud-app/fonts/jetbrains-mono/` stays the stock base.
- `glyphs/languages/hebrew/` holds Cascadia Mono's variable font, its OFL,
  SOURCE, both anchor files, `casanchors.py` and `build/`.
- `built/` holds `Regular.ttf`, `Bold.ttf`, MANIFEST, OFL.txt, FONTLOG.txt and
  AUTHORS.txt.

The section 5 design's `base/` and `icons/` moves wait.

**The tool.** `scripts/build-fonts.sh` runs a Python pinned to nixpkgs
`7dd199b0` by `fetchTarball`.

- Every step runs under `env -i` with only `SOURCE_DATE_EPOCH`,
  `PYTHONHASHSEED`, `PYTHONDONTWRITEBYTECODE`, `HEBREW_WORK` and the recipe's
  variables.
- It refuses to write unless the outputs equal the hashes it expects.
- `--check` rebuilds into a temporary directory and compares every MANIFEST
  line. It fails on a one-unit change to a design constant (measured).
- The four italic recipes, both leans, also reproduce (`--italics`). Nothing
  embeds them.
- fontTools' GPOS repacker, uharfbuzz, is pinned on. Section 5's "pin False"
  would have given other GPOS bytes (same length, equal XML), so HarfBuzz is a
  build-time dependency. Nothing of it reaches baud at run time.

**The bytes.**

- `built/Regular.ttf` (`df06669a...`) and `built/Bold.ttf` (`a02eb8b1...`) are
  `bi/bold2/final3`'s Regular-A-L2 and Bold-A-L2 plus two conformance fixes.
- A pre-commit review found the gated bytes departed from:
  - the OFL-FAQ, Q3.7: "Any trademark notices must remain in any derivative
    fonts", where nameID 7 had dropped Cascadia's;
  - Microsoft's Hebrew script spec, which registers `hebr`, where the font had
    no `hebr` record. Without it, HarfBuzz's Hebrew shaper ignores the anchors.
- So nameID 7 now carries JetBrains' notice and then Cascadia's, each byte for
  byte from its source font. A `hebr` record naming DFLT's Script table is
  spliced into GSUB and GPOS.
- Removing that 6-byte record gives the gated tables exactly. Every glyph, cmap,
  metric, GDEF and lookup byte is unchanged.
- Under HarfBuzz's Hebrew shaper, the clusters of pointed Genesis 1:1 that
  differ from Latin shaping went from 18 of 34 to 0.
- baud's shaping did not move (0 of 1,646 clusters), because it reads `latn`
  first.
- Both departures are fixed now, because a departure from the standard or
  the licence is a defect.

**What the app embeds.** Regular and Bold are the built files, using the
interim defaults of questions 1a and 1b (1a = A, 1b = L2), a choice the
project made.
Italic and Bold Italic stayed stock, so italic
Hebrew drew upright Regular from the chain until 1c was answered; since 2026-10-24
they carry the italic Hebrew (section 4c).

**A configured stack is followed by the embedded Regular, then the icon face,
then STIX** (`config.rs`, the `FontFallback::Embedded` branch). This is
decision 8, weighed again with its measured scope, and still interim.

- By cmap it moves 458 code points for a FiraCode stack (450 for the former
  default, FiraCode Nerd Font Mono):
  - the 52 Hebrew;
  - 38 that were boxes;
  - 360 that drew from STIX;
  - 8 that drew from the icon face.
- The default stack already resolves those 360 and 8 from JetBrains Mono.
- ~~No system tier follows a configured stack yet (decision 9).~~ The system
  tier follows every stack since 2026-10-26, `font_fallback = false`
  included.

**Unmeasured:**

- the Latin shaping and frame cost of GPOS going from 5 lookups to 7;
- the binary-size delta (the font data grows 12,628 B);
- Linux pixels;
- any committed test drawing the built fonts on Metal.

**Two corrections to earlier sections:**

- Overpaint is not evidence about a font, because baud-render's CPU compositor
  clips each glyph to its own row's band.
- Section 4a's "Regular's tables equal round 4's except the head dates" is
  stale: the files were byte-identical, head included.

**Known defects added to FONTLOG** (not in sections 4a and 4b):

- Meteg spelled with CGJ draws on the vowel (measured, Regular and Bold; drawn right
  of the vowel since 2026-10-26, FONTLOG's CHANGELOG).
- U+20AA is unmapped although the code-page 1255 bit is set (mapped since
  2026-10-25, baud's own glyph; FONTLOG's CHANGELOG).
- U+05C6, U+0591-05AF and U+FB1D-FB4F are unmapped (all mapped since
  2026-10-25, with U+05EF-05F2; FONTLOG's CHANGELOG).
- Bold meteg's per-vowel failures (since 2026-10-26 only where the cell has no place
  for meteg on its side, FONTLOG's CHANGELOG).

**Still open:**

- 1a-1e.
- The family name: OFL-FAQ Q5.4 allows neither word of "Cascadia Code", and it
  should not be "JetBrains Mono".
- ~~Shear or optically corrected italic.~~ Optically corrected, section 4c.
- ~~The italic lean.~~ Right, decided on 2026-10-24.
- ~~The licence call.~~ Answered on 2026-11-03: the built font ships under the
  OFL 1.1, after CascadiaMono.ttf was checked byte for byte against
  Microsoft's v2407.24 release (FONTLOG.txt, Licence; hebrew/SOURCE).

The licence call, as it stood: the built files carry Microsoft's copyright
line, with the Reserved Font Name, in nameID 0, and JetBrains Mono's OFL sentence
in nameID 13. They do not carry the Cascadia release binary's "Microsoft supplied
font ... Any other use is prohibited".

**Push blockers:**

- ~~The repository LICENSE (CC BY-NC-SA 4.0) has no exception for
  `baud-app/fonts/**`.~~ LICENSE now carves the fonts out, built and stock,
  in its section "Files this licence does not cover", because OFL condition 5
  requires them to be "distributed entirely under this license";
  `baud-font/tests/licences.rs` holds every font in the tree to it. The
  Hebrew build modules are named there as undecided, not assigned.
- Nothing packages the OFL for Linux.

## 6. Open decisions

From the build research. The leaning answer of 2026-10-07 follows items 1-3
and 5-12 in brackets; none is settled. Items 4, 13 and 14 have none. Each is
decided with pictures:

1. Where the icons come from: Nerd's fitted release glyphs (recommended), the
   patcher run by baud, or the patcher's rules ported. [release]
2. Icons in the Regular only or in every style (the choice of 2026-09-29 was
   one icon copy). [Regular only]
3. The tool: the script with committed outputs (recommended), `build.rs`, or Rust.
   [script]
4. JetBrains Mono's four statics as the base, or styles baked from the variable
   font (note Z, step 6).
5. Nerd scope C under the new order: its 13 overrides of JetBrains Mono's own
   glyphs inside the built font in all four styles, or none. [overrides]
6. Whether "no per-glyph rules" also covers build-time per-glyph lists (the
   reading here: it forbids run-time inheritance rules only). [the lean is the other
   way, also build-time]
7. STIX Two Math: a part by the font-inheritance rule; its symbols are
   candidates for a baud patch, and until then it stays a face, is merged, or
   goes. [stays a face]
8. The built font behind a configured stack: the Regular only, or a chain per style.
   [Regular only; built 2026-10-08 as the interim, and still the interim
   when weighed again with its measured scope (section 5a)]
9. macOS system fallback: the cascade at launch, per character, or both. [cascade at
   launch; *weighed again 2026-10-26* with the measured costs and
   coverage -- the cascade maps 33 of 161 scripts' first letters, the
   per-string answer 133, Syriac only the second -- per string, built as the
   lean; asked when a frame meets the character rather than at launch, and
   for the characters drawn rather than their whole block, the lean]
10. Linux system fallback: linked, `dlopen`ed or a subprocess. [linked;
    *weighed again 2026-10-26* with a fourth option, fontconfig's charset match
    over baud's own scan: that, built as the lean]
11. The family's name: anything but a Reserved Font Name; keeping "JetBrains
    Mono" is not barred by a Reserved Font Name, but it misnames a modified font
    and is JetBrains' trademark, to which the OFL grants no right. [a new name]
12. Inputs committed, or fetched by pinned hash at build time. [committed;
    for the Hebrew build, the variable font committed, built as the interim
    (section 5a)]
13. Which gap blocks baud draws itself, in what order, and which stay with the
    system: Claude Code's and common TUI symbols first is the measured case.
14. How baud's own glyphs are made per block: generated to JetBrains Mono's
    metrics (shapes, arrows, operators, and phonetic letters, which do not
    decompose), composed from its own letters and accents where they decompose
    (232 code points measured, mostly Latin Extended Additional and -B, plus 49 of
    Greek Extended's 233, whose marks need new anchors), or drawn (Hebrew).
15. Whether the bridge stays as the interim Hebrew the request was for, now that
    the research recommends against it (asked, not answered).
16. baud-core's 63-cluster limit, which drops vowel points in longer pointed
    Hebrew: to fix before Hebrew is done (found by the merge research).
    *Fixed 2026-10-14*: the store holds 1024 and sweeps the dead.
