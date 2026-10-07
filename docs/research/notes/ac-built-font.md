# AC. The embedded font, built from parts

**Status: open, researched 2026-10-07.** The owner's direction is decided and
recorded below. The design questions it leaves go to Jev and then to the owner's
review page. Nothing here is built yet.

Evidence is tagged as in the other notes: measured, source (file:line) or
inferred. The full research is kept untracked in `.claude/research/font-build/`
(the gap, the sources and the build design, with a critic's checks) and
`.claude/research/hebrew-patch/` (the community bridge, a merge prototype, and
run-time fitting).

## 1. What the owner decided

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
asked and is not answered (section 4 recommends against it). The owner removed
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
  baud's own over-wide rule D are decided in TODO.md and not built (source; no
  branch carries them).
- **There is no run-time system fallback.** A character no listed face maps
  draws the primary's missing glyph (source).

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

The owner asked for the bridge as a compromise; the research recommends against
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
baud-core keeps at most 63 distinct clusters and frees them only on a full
reset, and `marks_dropped` was 45 over Genesis 1:1-5 (measured), a core limit to
fix before Hebrew can be called done. The shipped patch needs baud's
own Hebrew drawings (section 1). Without the bridge, the interim under the
owner's order is the system fallback, once it exists: asked with
`CTFontCreateForString` for Hebrew behind FiraCode Nerd Font, CoreText answers
Courier New (measured 2026-10-07).

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
  devenv's pinned nixpkgs and across machines it is unmeasured, and the build
  must pin `USE_HARFBUZZ_REPACKER=False`, which changed the bytes in the merge
  prototype (measured).
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

## 6. Open decisions

From the build research. Jev's first answers (2026-10-07; probability of its choice,
then its confidence; none is settled under the 0.9 bar read as confidence) follow
items 1-3 and 5-12 in brackets; items 4, 13 and 14 were not asked. All go to the owner with pictures:

1. Where the icons come from: Nerd's fitted release glyphs (recommended), the
   patcher run by baud, or the patcher's rules ported. [release, 0.88 at 0.82]
2. Icons in the Regular only or in every style (the owner's 2026-09-29 call was
   one icon copy). [Regular only, 0.91 at 0.82]
3. The tool: the script with committed outputs (recommended), `build.rs`, or Rust.
   [script, 0.65 at 0.48]
4. JetBrains Mono's four statics as the base, or styles baked from the variable
   font (note Z, step 6).
5. Nerd scope C under the new order: its 13 overrides of JetBrains Mono's own
   glyphs inside the built font in all four styles, or none. [overrides, 0.60 at 0.20]
6. Whether "no per-glyph rules" also covers build-time per-glyph lists (the
   reading here: it forbids run-time inheritance rules only). [Jev leans the other
   way, also build-time, 0.63 at 0.26; it is the owner's rule to read]
7. STIX Two Math: a part by the owner's rule; its symbols are candidates for a
   baud patch, and until then it stays a face, is merged, or goes. [stays a face,
   0.93 at 0.89]
8. The built font behind a configured stack: the Regular only, or a chain per style.
   [Regular only, 0.83 at 0.66]
9. macOS system fallback: the cascade at launch, per character, or both. [cascade at
   launch, 0.92 at 0.88]
10. Linux system fallback: linked, `dlopen`ed or a subprocess. [linked, 0.46 at 0.19]
11. The family's name: anything but a Reserved Font Name; keeping "JetBrains
    Mono" is not barred by a Reserved Font Name, but it misnames a modified font
    and is JetBrains' trademark, to which the OFL grants no right. [a new name,
    0.86 at 0.72]
12. Inputs committed, or fetched by pinned hash at build time. [committed, 0.60 at
    0.20]
13. Which gap blocks baud draws itself, in what order, and which stay with the
    system: Claude Code's and common TUI symbols first is the measured case.
14. How baud's own glyphs are made per block: generated to JetBrains Mono's
    metrics (shapes, arrows, operators, and phonetic letters, which do not
    decompose), composed from its own letters and accents where they decompose
    (232 code points measured, mostly Latin Extended Additional and -B, plus 49 of
    Greek Extended's 233, whose marks need new anchors), or drawn (Hebrew).
15. Whether the bridge stays as the interim Hebrew the owner asked for, now that
    the research recommends against it (asked, not answered).
16. baud-core's 63-cluster limit, which drops vowel points in longer pointed
    Hebrew: to fix before Hebrew is done (found by the merge research).
