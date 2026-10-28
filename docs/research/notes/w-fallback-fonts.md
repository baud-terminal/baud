# W. Fallback fonts: what other terminals do, and what baud could bundle

**Status: superseded on 2026-10-07 by note AC.** The owner chose: the configured
font, then baud's embedded font built from JetBrains Mono, the Nerd patch and
baud's own patches, then the system's choice at run time, with no custom
inheritance hacks (read as no run-time per-glyph rules; whether build-time lists
count is open, note AC decision 6) and no parts of other fonts. That is closest to option E below, with baud's own
glyphs in place of a bundle of other fonts. The rest of this note stands as the
research. **Status before that: open, on the owner's review page.** The owner's rule of 2026-10-01
is that macOS and Linux draw identical pixels. Today they do not, past the
two embedded faces. Behind the embedded JetBrains Mono and Symbols Nerd Font
Mono, baud reads system files by path (`baud-app/src/config.rs`): on macOS
Menlo, Apple Color Emoji, Apple Symbols, Hiragino Sans GB, Apple SD Gothic
Neo and Kohinoor; on Linux the first DejaVu Sans Mono and the first Noto
Color Emoji found at a short list of usual locations. Rendered through baud's
own CPU path:

- **macOS** draws Hebrew and Arabic as `.notdef`.
- **Linux** draws Han, kana, Hangul, Devanagari, Hebrew, and symbols such as
  U+2460 ①, U+203B ※, U+2103 ℃ and U+216B Ⅻ as `.notdef`.

This note is the record behind the decision: what nine other terminals do,
read at source, and what baud could bundle, measured. The options on the page:

| option | design |
|---|---|
| A | bundle every fallback face, with nothing behind it |
| B | per-OS system discovery, no bundle |
| C | bundle the small scripts only; CJK and emoji from the system |
| D | bundle, with today's per-OS path lists behind it |
| E | bundle, with charset discovery behind it (not built) |

**Jev, 2026-10-01, with baud's principles in each request.** The figures are
probabilities; Jev's bar is 0.9.

- **Design** (request35): D 0.56, A 0.33, E 0.09, C 0.02, B 0.00, confidence
  0.44. Under the bar.
- **Han region** (request34): the SC and KR subsets over a single JP file or a
  per-locale choice, probability 0.97, confidence 0.96. Over the bar. The same
  question asked a few seconds earlier read 0.98 and 0.97.
- **Emoji** (request36): Noto Color Emoji (CBDT) 0.50, Mozilla Twemoji 14
  (COLRv0) 0.50, a tie. The same question asked about five minutes earlier read
  Noto 0.56 and Twemoji 14 0.44. Twemoji 17 with an unverified licence, and
  the system emoji per OS, both read 0.00 each time.

**What a bundle costs.**

- **Launch.** Renderer construction measured 0.56 ms with the bundle against
  0.36 ms with today's stack.
- **Repository.** The pack is 21.17 MiB (`git count-objects -vH`,
  2026-10-01). The bundles below that carry CJK are 13.7 to 24.8 MB of raw
  bytes (decimal MB). Committing the 24.4 to 24.8 MB bundle more than doubles
  the pack if its fonts compress as little as Noto Color Emoji's PNG strikes
  do. How much any
  of these bundles would grow the pack was not measured: git stores blobs
  zlib-compressed, and CID-keyed CFF compresses somewhat where PNG barely
  does, so "more than doubles" is not established for the 15.5 MB Twemoji
  bundle. The alternative is a build-time fetch against pinned hashes, which
  keeps the bytes out of history.

## Prior art, read at source

Each terminal's items below are numbered by the question they answer. The
numbering is read off the items' content, because the list of questions was
lost: 1 what is bundled, 2 the fallback order and how a face is found, 3 the
last resort, 4 emoji, 5 whether the pixels match across systems, 6 how the
CJK region is chosen. A number is absent where the reading recorded nothing
for it.

Read from shallow clones on 2026-10-01, at these commits:
wezterm cab2516 · ghostty d542774 · kitty 8a390c5 · alacritty d692748 + crossfont 0c34d19 ·
foot cb27717 + fcft 1ff7e79 · Windows Terminal (microsoft/terminal) 2b5336c · iTerm2 91411f5 ·
contour d8ce17b · rio 3762438.

### Summary

| terminal | bundles (in binary/package) | discovers via | emoji | last resort | identical across OS? |
|---|---|---|---|---|---|
| wezterm | JetBrains Mono x16 (~4.4 MB), Roboto x12, Noto Color Emoji **COLRv1** 4.8 MB, Symbols Nerd Font Mono 2.3 MB (`include_bytes!`) | config list → bundled JBM/Noto/Nerd appended as fallbacks → **system** (CoreText `CTFontCreateForString` per char; fontconfig `FcFontList` by charset) → optional `font_dirs` → bundled DB | bundled Noto COLRv1 is in the default fallback list ahead of system | primary font `.notdef` (system LastResort explicitly excluded) | No: system fallback differs per OS |
| ghostty | JetBrains Mono variable (+italic), Symbols Nerd Font; **non-mac only**: Noto Color Emoji **CBDT** 10.7 MB + Noto Emoji (mono) 0.9 MB | config → bundled JBM + Nerd → (mac: Apple Color Emoji by exact family) → per-codepoint discovery: CoreText (Han U+4E00–9FFF and misses go through `CTFontCreateForString`), fontconfig `FcFontSort` | macOS: system Apple Color Emoji (sbix); elsewhere bundled Noto CBDT | U+FFFD from any loaded face, else space | No: emoji itself differs by OS by design |
| kitty | Symbols NERD Font Mono (packaged builds; setup.py copies it into fonts/) | CoreText `CTFontCreateForString`, then a manual walk of all fonts; fontconfig `FcFontSort` with `FcDefaultSubstitute` (+ `color` for emoji) | system (AppleColorEmoji on mac by name; fontconfig `color=true` on Linux) | kitty draws its own hollow-box "missing glyph" sprite | No |
| alacritty (crossfont) | nothing (builtin box drawing only) | mac: `CTFontCopyDefaultCascadeListForLanguages` with **hardcoded "en"** + Apple Symbols; Linux: `FcFontSort` after `FcDefaultSubstitute` | system; **colour outline (COLR) fonts filtered out** on FreeType, bitmap (CBDT) OK | primary font `.notdef` | No |
| foot (fcft) | nothing | fontconfig `FcFontSort` (trim) after `FcDefaultSubstitute`; Linux only | system; picked by fontconfig lang `und-zsye` | primary font glyph for the codepoint (= `.notdef`) | n/a (Linux/BSD only) |
| Windows Terminal | Cascadia Code/Mono (~2.5 MB, installed via appx manifest, not embedded) | DirectWrite `GetSystemFontFallback` + `MapCharacters` with the user locale | system (Segoe UI Emoji, COLR) | U+FFFD looked up through fallback | n/a (Windows only) |
| iTerm2 | nothing | implicit: draws each run with `CTLineCreateWithAttributedString`, so CoreText's own cascade picks | system Apple Color Emoji | CoreText's own (LastResort) — **UNVERIFIED**, OS behaviour not iTerm2 code | n/a (macOS only) |
| contour | nothing | CoreText `CTFontCopyDefaultCascadeListForLanguages(NSLocale.preferredLanguages)`; fontconfig `FcFontSort` after `FcDefaultSubstitute`; DirectWrite; Windows hardcodes seguiemj/seguisym | system ("emoji" family) | U+FFFD from primary face | No |
| rio (sugarloaf) | Cascadia Code NF variable + italic (~4.9 MB), Symbols Nerd Font Mono (1.9 MB) | mac: cascade list + `CTFontCreateForString`; Linux: `FcFontSort` with charset **and an explicit FC_LANG from the environment**; Windows: font-kit | system (mac: Apple Color Emoji opened lazily by path) | primary font (font id 0) `.notdef` | No |

**Nobody bundles a face for CJK, Hangul, Devanagari, Hebrew, Arabic or general symbols.** Every
terminal gets those from the system, with one accidental exception found later (note AA,
section 6, 2026-10-03): rio's bundled Cascadia Code NF carries Hebrew in its Latin face, 27
letters and 10 of the 24 points, so rio draws Hebrew from its own bundle. The bundled set across all nine is: a Latin monospace family, a
Nerd Font symbols face, and (wezterm everywhere / ghostty off-mac) one Noto emoji face.

### Per terminal

#### wezterm (cab2516)
1. Bundled: assets/fonts/ — JetBrainsMono-*.ttf (16 styles, ~270–280 KB each), Roboto-*.ttf
   (12, ~170 KB, UI), NotoColorEmoji.ttf 4,813,824 B, SymbolsNerdFontMono-Regular.ttf 2,333,488 B.
   Embedded by `include_bytes!` in wezterm-font/src/parser.rs:819-865, behind cargo features
   `vendor-*` that wezterm-gui enables by default (wezterm-gui/Cargo.toml:16-24).
   The bundled Noto Color Emoji is the **COLRv1** build (tables COLR v1 + CPAL + glyf; checked on the file).
2. Order: config/src/font.rs:597-616 `font_with_fallback`: user fonts, then bundled JetBrains Mono,
   "Noto Color Emoji", "Symbols Nerd Font Mono" as fallbacks. If still missing,
   wezterm-font/src/lib.rs:316-345: system locator, then `font_dirs` if
   search_font_dirs_for_fallback, then the built-in DB.
   - macOS: locator/core_text.rs:125-175 `CTFontCreateForString(menlo, char)` per codepoint;
     skips `.LastResort`, skips non-regular faces. A broader list `build_fallback_list` at
     core_text.rs:250-290 = Menlo + `cascade_list_for_languages(menlo, AppleLanguages)` + Apple Symbols.
   - Linux: locator/font_config.rs:195-250 — FcFontList with a one-char charset, weight 80 slant 0,
     first pass monospace only, then any spacing. **No lang in the pattern; uses list not sort.**
3. Last resort: shaper/harfbuzz.rs:238-241,333-337 — "accept using the notdef glyph from the base
   font". docs/changelog.md:1398 "Removed Last Resort fallback font"; docs/config/fonts.md:46-49
   still says a "Last Resort" placeholder is drawn (doc is stale vs code).
6. CJK region: macOS via AppleLanguages cascade / CTFontCreateForString (system locale); Linux: FcFontList
   order, no lang hint (effectively unspecified). User can name e.g. 'DengXian' in the fallback list
   (docs/config/fonts.md:30-40).

#### ghostty (d542774)
1. src/font/embedded.zig:9-23: JetBrains Mono variable + variable italic and Nerd Fonts Symbols Only
   are fetched at build time (build.zig.zon:108,113: JetBrainsMono-2.304, NerdFontsSymbolsOnly-3.4.0)
   and `@embedFile`d; res/NotoColorEmoji.ttf 10,673,480 B (**CBDT/CBLC**, checked) and
   res/NotoEmoji-Regular.ttf 880,860 B (monochrome glyf). Everything else in res/ is test-only
   (embedded.zig:25-55; "only embedded if referenced").
2. src/font/SharedGridSet.zig:256-330 adds bundled JBM (all 4 styles) and Nerd symbols as fallbacks
   after the configured fonts. `:341-373` on Darwin prefers system "Apple Color Emoji" by exact family.
   `:375-405` the bundled Noto emoji faces are added **only when not Darwin** ("Mac is expected to always
   have the Apple Emoji available"). Per-codepoint discovery CodepointResolver.zig:95-217 (steps 1-7).
   - CoreText discovery.zig:450-512: Han U+4E00–9FFF goes straight to `CTFontCreateForString`
     ("properly takes into account system locale", cites Chromium LocaleInFonts); otherwise normal
     descriptor discovery, then `CTFontCreateForString` if empty (issue #2499); LastResort rejected
     (`:609-610`).
   - fontconfig discovery.zig:270-300: `FcConfigSubstitute` + `FcDefaultSubstitute` + `FcFontSort`
     with charset and `spacing=mono` (`:118-153`).
3. shaper/run.zig:228-249: U+FFFD from any face, else space.
5. Not identical: emoji face differs mac vs Linux by design; CJK etc. come from each system.
6. CJK region: CoreText's locale-aware choice for Han; fontconfig's `FcDefaultSubstitute` lang
   (from locale — fontconfig behaviour, not ghostty code).

#### kitty (8a390c5)
1. Bundles only Symbols NERD Font Mono: setup.py:1052-1066 `add_builtin_fonts` copies
   SymbolsNerdFontMono-Regular.ttf into fonts/; official builds download it
   (bypy/devenv.go:29 NerdFontsSymbolsOnly). Used as builtin descriptor in core_text.m:368-392.
2. macOS kitty/core_text.m:437-449 `CTFontCreateForString`; on LastResort or failure,
   manually_search_fallback_fonts `:403-435` walks the entire font collection (Nerd font first for
   PUA). Emoji presentation: `AppleColorEmoji` by name first (`:505-509`). Bold/italic applied to the
   fallback (`:454-477`).
   Linux kitty/fontconfig.c:441-500: pattern + `FcConfigSubstitute` + `FcDefaultSubstitute` +
   `FcFontSort` (cached candidate list), `color=true` for emoji presentation (`:450`).
3. fonts.c:2141-2146 MISSING_FONT → sprite `MISSING_GLYPH`, drawn by decorations.c:46-59
   `add_missing_glyph` = a hollow rectangle the cell size. Platform-independent tofu.
6. fontconfig default lang via `FcDefaultSubstitute`; CoreText's own locale choice.

#### alacritty / crossfont (alacritty d692748, crossfont 0c34d19)
1. Nothing bundled (no font files in either repo); builtin box drawing in
   alacritty/src/renderer/text/glyph_cache.rs:210-215.
2. macOS crossfont/src/darwin/mod.rs:66-92: `cascade_list_for_languages(&ct_font, ["en"])` with
   comment "TODO fixme, hardcoded en for english", plus "Apple Symbols". Lookup is a linear walk
   (`:130-150`). Linux src/ft/mod.rs:355-410: `config_substitute` + `default_substitute` +
   `fc::font_sort`; fallbacks that are **colour outline fonts are dropped** ("we can not render
   them", `:388-394`) — i.e. COLR emoji unsupported, CBDT bitmap emoji fine.
3. src/ft/mod.rs:259-261 / darwin/mod.rs:144-150 return MissingGlyph with the primary's glyph 0;
   alacritty draws it (glyph_cache.rs:227-235).
6. macOS: always English cascade → Han from the "en" cascade regardless of user locale.
   Linux: fontconfig default lang.

#### foot / fcft (cb27717 / 1ff7e79)
1. Nothing bundled; foot links fontconfig (foot/meson.build:155).
2. fcft.c:567-570 `FcDefaultSubstitute` + `FcFontSort(..., trim=FcTrue)`; per-fallback langset kept
   (`:1202-1340`).
3. fcft.c:2135-2150: "No font claimed this glyph - use the primary font anyway" → `.notdef`.
4. Emoji: system; presentation enforced via fallback langset containing `und-zsye`
   (fcft.c ~2096-2105).
6. fontconfig lang from `FcDefaultSubstitute`.

#### Windows Terminal (2b5336c)
1. Cascadia Code/Mono + italics in res/fonts (736/541/715/524 KB), registered as package fonts in
   src/cascadia/CascadiaPackage/Package.appxmanifest:246-248 (installed with the app, not in the exe).
2. AtlasEngine: AtlasEngine.cpp:47 `GetSystemFontFallback`; user font list prepended with a
   `IDWriteFontFallbackBuilder` (AtlasEngine.api.cpp:686-726); `MapCharacters` with
   `_p.userLocaleName` from `GetUserDefaultLocaleName` (AtlasEngine.cpp:738-743,989-1027).
   Primary falls back to Consolas if not found (api.cpp:707-719).
3. U+FFFD resolved through the same fallback (AtlasEngine.cpp:1189-1205).
4. System emoji (Segoe UI Emoji) via DirectWrite fallback — UNVERIFIED in code (OS mapping).
6. DirectWrite locale-aware mapping (user locale).

#### iTerm2 (91411f5)
1. No font files in the repo.
2. No explicit fallback call (`CTFontCreateForString`/cascade not present in sources/). Glyphs are
   rendered by `CTLineCreateWithAttributedString` (sources/MetalRenderer/Glue/iTermRegularCharacterSource.m:100),
   so CoreText's implicit font substitution picks the fallback. User can set a separate
   "non-ASCII font" (sources/Fonts/FontTable.swift range map). Default font `NSFont.userFixedPitchFont`
   (FontTable.swift:40-42).
3/4/6. CoreText's own behaviour (LastResort tofu, Apple Color Emoji, locale-ordered cascade) — UNVERIFIED
   at iTerm2 source; it is the OS doing it.

#### contour (d8ce17b)
1. No font files.
2. CoreText src/text_shaper/CoreTextLocator.mm:122-165 `CTFontCopyDefaultCascadeListForLanguages(font,
   NSLocale.preferredLanguages)`, optionally filtered to a configured allow-list. fontconfig
   FontconfigLocator.cpp:299-303,527-531 `FcDefaultSubstitute` + `FcFontSort(trim)`. Windows hardcodes
   seguiemj.ttf/seguisym.ttf for "emoji" (`:388-392`).
3. OpenShaper.cpp:253,495-510 replaces unshaped glyphs with U+FFFD from the primary face.
6. macOS NSLocale preferred languages; fontconfig default lang.

#### rio (3762438)
1. sugarloaf/src/font/constants.rs:1-25 embeds CascadiaCodeNF.ttf 2,548,236 B and CascadiaCodeNFItalic.ttf
   2,352,780 B (variable wght); rio-fonts/src/lib.rs:21-24 embeds SymbolsNerdFontMono-Regular.ttf
   1,900,052 B. No emoji or CJK bundled.
2. sugarloaf/src/font/mod.rs:300-420 lazy discovery: mac `macos::discover_fallback`
   (CTFontCreateForString) + default cascade list opened lazily by path (mod.rs:982-995, "keeps us from
   pulling the 200 MB Apple Color Emoji file into FONT_DATA_CACHE"); Linux font/linux.rs:56-145
   `FcFontSort` with charset, spacing, and **explicit FC_LANG from the environment**
   (`:104-116`, "so CJK gets the right regional variant (zh-CN vs zh-TW vs ja vs ko)").
3. mod.rs:668-680: find_best_font_match returns `(0, false)` — primary font, i.e. `.notdef`.
6. Linux: env lang; mac: CoreText.

### Cross-OS identity
No terminal makes CJK/Hangul/Indic/RTL/symbol fallback identical across macOS and Linux. The only
deterministic parts anywhere are the bundled faces themselves: wezterm (JBM + Noto COLRv1 emoji + Nerd,
same bytes on every OS, ahead of system fallback) comes closest — but anything outside those three faces
still comes from CoreText vs fontconfig. ghostty deliberately diverges on emoji (Apple on mac, Noto CBDT
elsewhere). kitty is the only one whose *last resort* is OS-independent (its own drawn box).

### Size discussions
Nothing found in docs/changelogs about bundling CJK being too large (grepped docs/changelogs for
cjk/unifont near bundle/size). Only size-adjacent remarks: rio's comment about not loading the 200 MB
Apple Color Emoji; ghostty's note that test fonts are not embedded unless referenced. Issue trackers
were not searched (not available offline) — UNVERIFIED whether such discussions exist there.


## Candidates

Every candidate was downloaded and measured on 2026-10-01 with fontTools
4.66.1. "Codepoints" is the union of every Unicode cmap subtable, over every
face of a collection. The coverage figures are cmap hits over the codepoints
of seven test strings: 漢字かなカナ (6), 한국어 (3), हिन्दी (6), עברית (5),
العربية (7), ①※℃Ⅻ⎇ (5), and 😀👨‍👩‍👧🏳️‍🌈 (10, of which three are U+200D
and one U+FE0F).

**Most sha256 hashes are lost.** The files and the table that recorded every
hash sat in a scratch directory that a reboot wiped the same day. Two
survive from the measurement and are given below; a third, DejaVu Sans
Mono's, was taken separately. The rest must be re-taken when a bundle is
built, and a pinned fetch computes them anyway.

**Three emoji faces were re-fetched the same evening** from the URLs below,
to redraw the review page's emoji panel, and hash as follows. Noto Color
Emoji matches the manifest's pin for it (`noto-color-emoji-cbdt`), and
Twemoji 17 matches the hash that survived; Mozilla's Twemoji 14 is new:

| file | bytes | sha256 |
|---|---|---|
| NotoColorEmoji.ttf, v2.051 | 10,673,480 | 72a635cb3d2f3524c51620cdde406b217204e8a6a06c6a096ff8ed4b5fd6e27b |
| Twemoji.ttf, breadgrocery v17.0.3 | 1,805,812 | 18b63ff478996f98c40504ca53691efc399264457c41f2f87b4d35a00512cb20 |
| Twemoji.Mozilla.ttf, v0.7.0 | 1,474,284 | 6d90152ee0d29e82fe2a87793af5aa4b7ad13e6538360889e141e81ed299ee8e |

**What was measured and what was read.** Formats, sizes, coverage and the
two surviving hashes were measured with fontTools. The candidate measurement
did not run baud-font. The session that built the review page later rendered
some of these faces through baud (the Arabic fit below, and the 0.56 ms
construction figure); which files it rendered is not recorded here. So what
baud-font cannot read rests on reading its source alone, at 6a2eff5.

### Scripts and symbols

One row a font. The byte counts are the unhinted OTF, the unhinted TTF and
the full TTF. Within each Noto release zip they are the members
NotoSansX/unhinted/otf/NotoSansX-Regular.otf,
NotoSansX/unhinted/ttf/NotoSansX-Regular.ttf and
NotoSansX/full/ttf/NotoSansX-Regular.ttf, where X is the font's name. The
unhinted builds carry the script alone; the full builds add Latin. Every
variant of a font has the same coverage of the test strings. A 3/10 on the
emoji string is the string's three U+200D, not an emoji.

| font | upstream | version | licence | bytes | outline | codepoints (unhinted / full) | coverage |
|---|---|---|---|---|---|---|---|
| Noto Sans Hebrew | https://github.com/notofonts/hebrew/releases/download/NotoSansHebrew-v3.001/NotoSansHebrew-v3.001.zip | 3.001 | OFL-1.1 | 13,840 / 16,836 / 66,156 | CFF / glyf / glyf | 147 / 464 | Hebrew 5/5, emoji string 3/10, nothing else |
| Noto Sans Arabic | https://github.com/notofonts/arabic/releases/download/NotoSansArabic-v2.013/NotoSansArabic-v2.013.zip | 2.013 | OFL-1.1 | 107,952 / 142,140 / 271,652 | CFF / glyf / glyf | 1,250 / 1,551 | Arabic 7/7, emoji string 3/10; proportional (see Arabic below) |
| Noto Sans Devanagari | https://github.com/notofonts/devanagari/releases/download/NotoSansDevanagari-v2.007/NotoSansDevanagari-v2.007.zip | 2.007 | OFL-1.1 | 146,592 / 184,228 / 277,680 | CFF / glyf / glyf | 281 / 551 | Devanagari 6/6, emoji string 3/10 |
| Noto Sans Symbols | https://github.com/notofonts/symbols/releases/download/NotoSansSymbols-v2.003/NotoSansSymbols-v2.003.zip | 2.003 | OFL-1.1 | 91,616 / 145,508 / 264,032 | CFF / glyf / glyf | 840 / 1,107 | symbols 3/5: ① Ⅻ ⎇ |
| Noto Sans Symbols 2 | https://github.com/notofonts/symbols/releases/download/NotoSansSymbols2-v2.008/NotoSansSymbols2-v2.008.zip | 2.008 | OFL-1.1 | 404,576 / 671,568 / 1,233,128 | CFF / glyf / glyf | 2,641 / 2,955 | symbols 0/5, emoji string 1/10; it is arrows, dingbats, chess, braille |
| Noto Sans Math | https://github.com/notofonts/math/releases/download/NotoSansMath-v3.000/NotoSansMath-v3.000.zip | 3.000 | OFL-1.1 | 479,308 / 657,440 / 1,015,684 | CFF / glyf / glyf | 2,919 / 3,033 | Arabic 6/7, but those are its Arabic math-alphabet letters, with no joining forms: not an Arabic fallback |
| Noto Sans Mono | https://github.com/notofonts/latin-greek-cyrillic/releases/download/NotoSansMono-v2.014/NotoSansMono-v2.014.zip, member NotoSansMono/unhinted/ttf/NotoSansMono-Regular.ttf | 2.014 | OFL-1.1 | 388,660, unhinted TTF only | glyf | 3,490 | symbols 2/5: ※ ℃; Hebrew, Arabic and Devanagari 0; Latin, Greek, Cyrillic |
| DejaVu Sans Mono | https://github.com/dejavu-fonts/dejavu-fonts/releases/download/version_2_37/dejavu-fonts-ttf-2.37.zip, member dejavu-fonts-ttf-2.37/ttf/DejaVuSansMono.ttf | 2.37 | Bitstream Vera and DejaVu terms, permissive | 340,712, TTF | glyf | 3,322 | Arabic 7/7, monospaced; Hebrew, Devanagari, CJK and the symbol string 0 |

For comparison, Menlo (the system's /System/Library/Fonts/Menlo.ttc, four
faces, 2,728 codepoints, not redistributable) covers none of the seven
strings.

**Hebrew, added 2026-10-03 (note AA, section 6).** The Hebrew test string above
has no vowel points, and the table has no monospaced Hebrew face. Measured
since: coverage and widths with fontTools, and placement by drawing through
baud's own renderer:

- **Noto Sans Hebrew 3.001** (the full TTF above, 66,156 bytes) is proportional.
  Its letters run 0.295-0.730 em against a 0.6 em cell. In baud, shin's ink runs
  into lamed's cell and touches it. Its points sit on their letters, because it
  lists its mark positioning under the Latin and default scripts as well as
  Hebrew, and baud shapes with those (measured, its `GPOS`).
- **Liberation Mono 2.1.5**, the upstream release (319,508 bytes, sha256
  `f2b83c76...`, SIL OFL 1.1 with Reserved Font Names Arimo, Tinos, Cousine and
  Liberation), has 27 of 27 letters at a fixed 0.6001 em, all 20 points and the
  31 cantillation marks. Its marks have zero advance; nixpkgs' rebuild, first
  measured, gives them full width, and baud draws the two alike. Placed last
  it fills only boxes, but behind this note's minimal bundle it draws no
  Hebrew, since Noto Sans Hebrew comes first (note AA, section 8).
- **Cascadia Code 2407.024** (upstream, 598,060 bytes, OFL 1.1) has 27 letters
  and 8 of the 20 points, no cantillation, at a fixed 0.586 em. Its Cascadia
  Mono is Windows Terminal's bundled default face.
- **Courier New**, macOS's own, is the only monospaced Hebrew face installed on
  macOS. It cannot be shipped.

In baud, Liberation Mono's and Courier New's points land one cell to the right
of their letters, and Cascadia's partly so, because baud shapes with the Latin
script; placing them is per-script shaping. The Hebrew face is on the owner's
review page (v8).

### CJK

The Noto files are at the noto-cjk tag Sans2.004 (commit 523d033), fetched
from https://raw.githubusercontent.com/notofonts/noto-cjk/Sans2.004/Sans/
followed by the path in the second column. The Source Han Sans files are at
the source-han-sans release branch, commit a4f7cf9 of 2025-06-25 (release
2.005R), fetched from
https://raw.githubusercontent.com/adobe-fonts/source-han-sans/release/
followed by the path. Every file is OFL-1.1. "Test strings" is the Han and
kana string, the Hangul string and the symbol string.

| file | path | version | bytes | outline | codepoints | test strings | corpus misses | Han / Ext A / Hangul in cmap |
|---|---|---|---|---|---|---|---|---|
| NotoSansJP-Regular.otf | SubsetOTF/JP/NotoSansJP-Regular.otf | 2.004 | 4,533,028 | CFF, CID-keyed | 16,732 | 6/6, 0/3, 4/5 | 1,596: Hangul 996, Han 597, kana 3 | 12,747 / 203 / 0 |
| NotoSansKR-Regular.otf | SubsetOTF/KR/NotoSansKR-Regular.otf | 2.004 | 4,644,748 | CFF, CID-keyed | 23,174 | 6/6, 3/3, 4/5 | 1,062: Han 1,059, kana 3 | 8,138 / 93 / 11,172 |
| NotoSansSC-Regular.otf | SubsetOTF/SC/NotoSansSC-Regular.otf | 2.004 | 8,331,336 | CFF, CID-keyed | 30,890 | 6/6, 0/3, 4/5 | 999: Hangul 996, kana 3 | 20,976 / 6,582 / 0 |
| NotoSansTC-Regular.otf | SubsetOTF/TC/NotoSansTC-Regular.otf | 2.004 | 5,683,368 | CFF, CID-keyed | 20,745 | 6/6, 0/3, 4/5 | 1,539: Hangul 996, Han 540, kana 3 | 15,383 / 575 / 0 |
| NotoSansCJKjp-Regular.otf | OTF/Japanese/NotoSansCJKjp-Regular.otf | 2.004 | 16,467,736 | CFF, CID-keyed | 44,810 | 6/6, 3/3, 4/5 | 3, complete | 20,976 / 6,582 / 11,172 |
| NotoSansMonoCJKjp-Regular.otf | Mono/NotoSansMonoCJKjp-Regular.otf | 2.004 | 16,424,156 | CFF, CID-keyed | 44,810 | 6/6, 3/3, 4/5 | 3, complete | 20,976 / 6,582 / 11,172; fixed-width Latin, otherwise the same design |
| NotoSansCJK-Regular.ttc | OTC/NotoSansCJK-Regular.ttc | 2.004 | 19,484,784 | CFF, ten faces | 44,810 | 6/6, 3/3, 4/5 | 3, complete | 20,976 / 6,582 / 11,172 |
| NotoSansJP-VF.ttf | Variable/TTF/Subset/NotoSansJP-VF.ttf | 2.004 | 9,590,732 | glyf + gvar, wght 100 to 900, default 100 | 16,732 | as the JP subset | 1,596 | as the JP subset |
| NotoSansKR-VF.ttf | Variable/TTF/Subset/NotoSansKR-VF.ttf | 2.004 | 10,415,420 | glyf + gvar, wght 100 to 900, default 100 | 23,174 | as the KR subset | 1,062 | as the KR subset |
| SourceHanSansJP-Regular.otf | SubsetOTF/JP/SourceHanSansJP-Regular.otf | 2.005 | 4,562,224 | CFF, CID-keyed | 16,742 | 6/6, 0/3, 4/5 | 1,597: Hangul 996, Han 598, kana 3 | 12,746 / 203 / 0; the Noto JP subset's design |
| SourceHanSansHW-Regular.otf | OTF/JapaneseHW/SourceHanSansHW-Regular.otf | 2.005 | 16,517,248 | CFF, CID-keyed | 44,853 | 6/6, 3/3, 4/5 | 3, complete | 20,992 / 6,592 / 11,172 |

Not downloaded: the variable OTFs (for example NotoSansCJKjp-VF.otf,
30,766,960 B, and the subset NotoSansJP-VF.otf, 8,128,756 B) and the variable
OTC. That they are CFF2 was judged from how noto-cjk publishes them, not
measured.

**The test strings are too short to choose a CJK font; the corpus decides.**
`tests/recordings/utf8/cat-cjk.raw`, `tests/recordings/utf8/less-cjk.raw` and
`tests/recordings/utf8/vim-unicode.raw`, decoded as UTF-8, use 3,162 distinct
codepoints in Han, kana and Hangul: Han 1,975, Hangul 996, Hiragana 96,
Katakana 95 (3,155 in cat-cjk, 2,152 in less-cjk, 14 in vim-unicode). Three
of the Hiragana are unassigned (U+3040, U+3097, U+3098) and miss in every
font, so a miss of 3 means complete.

- **Every regional subset misses on its own.** The KR subset passes all three
  CJK test strings and still misses 1,059 of the corpus's Han. Against the Han
  alone: JP misses 597, TC 540, KR 1,059, SC none; every subset but KR misses
  the 996 Hangul.
- **Pairs.** SC and KR together miss 3, which is complete, at 12,976,084 B in
  two files. JP and KR miss 555, TC and KR 487, JP, TC and KR together 262.
- **The smallest complete option** is the SC and KR subsets, 12,976,084 B.
  Their Han is drawn in the Simplified Chinese design, Japanese text included.
- **The smallest complete single file** is NotoSansCJKjp-Regular.otf,
  16,467,736 B, CID-keyed CFF, with Japanese Han forms.
- Which region's glyph forms to ship is the owner's call. Jev's answer is the
  SC and KR pair, above.

Every complete candidate is CID-keyed CFF. The only glyf CJK faces are the
`*-VF.ttf` subsets, which miss what their subsets miss.

### Emoji

| file | upstream | version | licence | bytes | format | codepoints | emoji string | notes |
|---|---|---|---|---|---|---|---|---|
| NotoColorEmoji.ttf | https://raw.githubusercontent.com/googlefonts/noto-emoji/v2.051/fonts/NotoColorEmoji.ttf | 2.051 | OFL-1.1 | 10,673,480 | CBDT/CBLC PNG strikes, no outlines | 1,501 | 9/10: no U+FE0F in its cmap | ligates the family and 🏳 ZWJ 🌈; a shaper has to drop U+FE0F first |
| NotoColorEmoji-noflags.ttf | https://raw.githubusercontent.com/googlefonts/noto-emoji/v2.051/fonts/NotoColorEmoji-noflags.ttf | 2.051 | upstream OFL-1.1; its name table carries no licence string | 9,833,416 | CBDT/CBLC | 1,475 | 9/10 | |
| Noto-COLRv1.ttf | https://raw.githubusercontent.com/googlefonts/noto-emoji/v2.051/fonts/Noto-COLRv1.ttf | 2.051 | OFL-1.1 | 4,991,984 | glyf + COLR v1: no version-0 base records, 3,993 version-1 | 1,499 | 9/10 | baud-font reads no colour from it (below); v2.051 ships no COLR v0 build |
| Twemoji.Mozilla.ttf | https://github.com/mozilla/twemoji-colr/releases/download/v0.7.0/Twemoji.Mozilla.ttf | 0.7.0, 2022-10-28 | Apache-2.0 for the build, CC-BY-4.0 for the art | 1,474,284 | glyf + COLR v0, 3,689 base glyphs | 1,418 | 10/10 | Twemoji 14, so it stops at Unicode 14; ligates the family and 🏳 FE0F ZWJ 🌈, not 🏳 ZWJ 🌈 |
| Twemoji.ttf (breadgrocery) | https://github.com/breadgrocery/twemoji-colr/releases/download/v17.0.3/Twemoji.ttf | 17.000, release v17.0.3 of 2026-06-27 | CC-BY-4.0 for the art; the repository has no LICENSE file | 1,805,812 | glyf + COLR v0, 4,009 base glyphs | 1,453 | 10/10 | a fork of mozilla/twemoji-colr; carries Unicode 16 and 17 (U+1FA8F SHOVEL, U+1FAEA DISTORTED FACE); sha256 18b63ff478996f98c40504ca53691efc399264457c41f2f87b4d35a00512cb20 |
| Twemoji_GlyfColr0.ttf (Nerixyz) | https://github.com/Nerixyz/twemoji-colr-font/releases/download/v0.0.2/Twemoji_GlyfColr0.ttf | 1.000, release v0.0.2 of 2025-11-20 | CC-BY-4.0 for the art; the repository has no LICENSE file | 2,064,468 | glyf + COLR v0, 5,189 base glyphs | 1,453 | 10/10 | carries U+1FA8F and U+1FAEA; the same release ships CBDT, sbix and CFF and CFF2 COLR builds; sha256 0a75e5744aba8ba9692bf814522d24f85b4846e9619d26bbacebc2af386d385a |

Any Twemoji build carries the art's CC-BY-4.0 attribution obligation. The two
Twemoji 17 forks' licences are unverified. Building COLR v0 from Noto's SVGs
with nanoemoji is a further option, unmeasured.

### Bundle totals

One Regular face of each, in bytes. The first column takes the unhinted OTFs
(CFF), the second the unhinted TTFs (glyf); the CJK and emoji files are the
same in both columns. Each unparenthesised row adds to the unparenthesised
row before it; a row in parentheses replaces that row's addition.

| bundle | CFF | glyf |
|---|---|---|
| minimal: Hebrew, Arabic, Devanagari, Symbols, Symbols 2 | 764,576 | 1,160,280 |
| plus CJK complete on the corpus: the SC and KR subsets, 12,976,084 | 13,740,660 | 14,136,364 |
| (plus CJK as one file, NotoSansCJKjp-Regular.otf, 16,467,736) | 17,232,312 | 17,628,016 |
| plus current emoji: Twemoji 17 COLR v0 (breadgrocery v17.0.3), 1,805,812 | 15,546,472 | 15,942,176 |
| (plus emoji: Mozilla Twemoji 14 COLR v0, 1,474,284) | 15,214,944 | 15,610,648 |
| (plus emoji: Noto Color Emoji CBDT, 10,673,480) | 24,414,140 | 24,809,844 |

- **A gap in the minimal bundle.** None of its fonts has ※ or ℃. They come
  from a CJK face, every one of which maps ①※℃Ⅻ but not ⎇, or from Noto Sans
  Mono (388,660 B). With the CJK rows added, the symbol string is complete:
  ① Ⅻ ⎇ from Noto Sans Symbols, ※ ℃ from the CJK face.
- **Arabic from DejaVu Sans Mono instead of Noto Sans Arabic** (see Arabic
  below) adds 232,760 B to the CFF column and 198,572 B to the glyf column.

### What baud-font cannot read

Read from baud-font's source at 6a2eff5, not by running it on these files.
`baud-font/src` is unchanged between 6a2eff5 and a1d2b94, where the line
numbers below were read.

- **Noto-COLRv1.ttf gets no colour.** `baud-font/src/colr.rs` reads COLR
  version 0, including the version-0 arrays inside a version-1 table, and not
  the BaseGlyphPaintRecords that version 1 adds. This file's COLR table has
  no version-0 base records and 3,993 version-1 records, so baud would draw
  its monochrome glyf outlines at best.
- ~~**CFF2: every `*-VF.otf` and the variable OTC.** Nothing in baud-font
  parses CFF2; `baud-font/src/bake.rs` names the tag only in its list of
  tables a bake refuses.~~ Read since 2026-10-11 (`baud-font/src/cff2.rs`;
  the record, "CFF2 outlines are read, blends and all"), at any
  coordinates, and a face whose CFF2 does not read is refused at load. The
  bake still refuses a CFF2 face: it writes `glyf` instances.
- **SVG fonts** are not read.
- **The glyf variable subsets can be read but default to Thin.**
  NotoSansJP-VF.ttf and NotoSansKR-VF.ttf are glyf with gvar, which baud-font
  reads, and their wght axis defaults to 100. A bundle would set wght 400 or
  bake a Regular instance, and they still miss what their subsets miss.
- What it does read, for the record: TrueType, CFF including CID-keyed
  fonts (ROS, FDArray and FDSelect, `baud-font/src/cff.rs`), collections,
  `sbix` PNG strikes and CBDT image formats 17, 18 and 19
  (`baud-font/src/bitmap.rs`), and cmap formats 0, 4, 6, 12 and 13 plus 14
  for variation sequences (`baud-font/src/cmap.rs`, `baud-font/src/ivs.rs`).

### Arabic is unjoined with any font

baud's shaper reads the script `latn`, else `DFLT`, and wants five features:
`ccmp`, `calt`, `liga`, `mark` and `mkmk`, with `liga` off for the grid; every
other feature answers no (`baud-font/src/layout.rs`, lines 31-39, 626-642
and 713-717). Arabic's joining forms come from the `init`, `medi` and `fina`
features, which baud applies under no script. So every font draws Arabic in
its unjoined forms.

What the font still decides is the fit. Rendered for the review page, Noto
Sans Arabic is proportional and its letters crowd in one-cell slots. DejaVu
Sans Mono 2.37 is monospaced and fits. That file is 340,712 B, sha256
b4a6c3e4faab8773f4ff761d56451646409f29abedd68f05d38c2df667d3c582; the hash
was taken in the session that built the review page, not in the candidate
measurement above.

## A finding that changed the emoji panel

The review page's Apple Color Emoji row was rendered at 40 px. At that size
baud drew the family (👨‍👩‍👧), U+1FAE0 MELTING FACE and U+1FA77 PINK HEART
as nothing. The cause is baud's choice of bitmap strike, which is being
fixed, and not Apple's font: on this Mac, CoreText finds both codepoints in
Apple Color Emoji's cmap and shapes the family to one glyph from it. So that
row showed baud's strike choice rather than what Apple's emoji look like,
and it is not evidence about Apple's set until the fix lands.

**The fix landed in 34759c1** -- a strike is chosen per glyph among the
strikes that hold it, and `flip` is drawn -- and the panel was redrawn with
it: Apple's row is complete at 40 px, and the other three rows did not
change. The page says the earlier panel was wrong. CLAUDE.md, "A colour
bitmap is drawn from a strike that holds it", has the measurements.

## Does a bundled face follow a configured stack? (2026-10-03)

The owner embedded STIX Two Math last in the default stacks (2026-10-02; the
record's "STIX Two Math is embedded, last in the default stacks"). A
configured `fonts` replaces the default stack, and behind it baud appends only
the embedded icon face, under `font_fallback`. So the fix does not reach a
configured stack, the owner's included. Whether STIX should follow a
configured stack too (FOLLOW) or stay in the default stack only
(DEFAULT_ONLY) went to Jev at 0.54, so the prior art was read at source for
this one axis.

Read from clones on 2026-10-03: WezTerm cab2516 · Ghostty befcdfd · kitty
7d37453 · Alacritty d692748 with crossfont 0.8.1 (a93d1ee, the version it
locks) · foot cb27717 with fcft 1ff7e79 · iTerm2 91411f5.

| terminal | a configured font keeps the bundled faces? | keeps the system cascade? | order behind a configured font | switch |
|---|---|---|---|---|
| WezTerm | yes: JetBrains Mono, Noto Color Emoji and Symbols Nerd Font Mono are appended to every list (`config/src/font.rs:597-617`) | yes: CoreText per character, or fontconfig | user list → bundled names (an installed copy of a family wins) → system → built-in database | system only: `font_locator = "ConfigDirsOnly"`; bundled only at build time (`vendor-*` features) |
| Ghostty | yes: JetBrains Mono and Symbols Nerd Font; Noto emoji off macOS; Apple Color Emoji by name on macOS (`src/font/SharedGridSet.zig:158-408`) | yes: CoreText or fontconfig, regular style | user families → embedded → emoji → discovery | none at runtime |
| kitty | yes: Symbols Nerd Font Mono, loaded unconditionally (`kitty/main.py:375`) | yes | `font_family` and `symbol_map` → system → bundled Nerd (on macOS the Nerd face comes earlier, for the private use area only) | none |
| Alacritty | bundles none | yes: CoreText's cascade for "en" plus Apple Symbols (crossfont `src/darwin/mod.rs:66-93`), or `FcFontSort` | family → system | none |
| foot | bundles none | yes: fontconfig's sorted list, appended last (fcft `fcft.c:1349-1353`) | primary → user fallbacks → fontconfig | none |
| iTerm2 | bundles none | yes: CoreText's implicit substitution in `CTLine` | ASCII font, non-ASCII font, special exceptions → CoreText | none found |

* **Every terminal that bundles faces keeps them behind a configured font,
  with no runtime switch.** Two maintainers were asked for one and declined:
  kitty #7866 ("adding a builtin nerd font doesn't change anything except that
  now those matches actually render the symbols instead of squares") and
  WezTerm #3349 (the user's list is tried first, so no switch is needed).
  WezTerm's docs say so: "WezTerm will still append its default fallback to
  whatever list you specify" (`docs/config/fonts.md:41-42`).
* **The closest analogue to STIX, a face a terminal names itself for
  coverage, follows too.** WezTerm's macOS fallback always includes Menlo and
  Apple Symbols; Menlo went in as "the only font on macOS that contains
  U+2718" (#849, and the changelog). crossfont always appends Apple Symbols,
  and Ghostty always appends Apple Color Emoji. No terminal bundles a math or
  symbols face.
* **On this Mac STIX Two Math is the only system font with U+23F5**, of 387
  files scanned (the first face of each collection), and CoreText returns it
  for U+23F5 and U+23FA by each route these terminals call:
  `CTFontCreateForString`, the default cascade list, and `CTLine`. So each of
  them draws Claude Code's arrow behind a configured font from the face baud
  embeds. That is inferred from the API calls, not from running each terminal.
* **What the prior art shows going wrong is order, not following.** Ghostty
  #7204 (its bundled Nerd glyph for U+2665 pre-empting Menlo's) and #12200
  (bundled Noto Emoji pre-empting text glyphs) are bundled faces placed ahead
  of better ones. baud puts STIX last.

**What FOLLOW changes in the owner's stack**, from CoreText's character sets,
crediting each code point to the first face in the stack that has it (not
baud's presentation-aware choice): of STIX's 4,611 code points, 1,877 are
boxes today and would draw from STIX, U+23F5 among them. 43 held only by
Apple Color Emoji would turn from colour to STIX's text glyph under the
default `font` rule, U+23FA among them (U+23E9-23F3, U+26BD, U+26C4, U+26F5,
U+2702, U+2709, U+2B1B and others), as 40 did in the default stack when the
owner chose the embed.

**Jev:** FOLLOW 0.54 with baud's own precedent only; 0.88 (confidence 0.76)
with the prior art; 0.90 (confidence 0.79) with the measured consequence and
the owner's earlier trade. On the review page 2026-10-03.
