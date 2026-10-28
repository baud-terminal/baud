# AY. Fonts by family name, and a family's own bold and italic

The aim, from 2026-09-29: a font is named by its family and size, not its file, as other terminals do; a family's own bold is used, not faked; and a user can configure a font other than the embedded one (2026-10-07). Read 2026-10-21; every source below is pinned in
`docs/research/sources/MANIFEST.toml`.

## The texts

- **Naming a family.** CSS Fonts 3 5.1 (`[[source: css-fonts-3]]`),
  unchanged in CSS Fonts 4 (`[[source: css-fonts-4-wd-2026-10-16]]`): names
  are matched "case insensitively, using the \"Default Caseless Matching\"
  algorithm ... without normalizing the strings involved", status C and F of
  CaseFolding.txt, and "user agents must match any of these names" where a
  font has several localized family names. The `name` table
  (`[[source: ms-opentype-name]]`) gives the family as nameID 16, else 1,
  and the subfamily as 17, else 2.
- **Picking a face of the family.** CSS Fonts 3 5.2: the family's faces
  narrowed by `font-stretch`, then `font-style` (italic: italic, oblique,
  normal; normal: normal, oblique, italic), then `font-weight` (above 500:
  up then down; 400: 500 first, then down, then up). CSS Fonts 4 5.2 extends
  each step to a face that covers a range -- a variable face -- "as if each
  supported combination of values is a unique font". Bold is 700.
  `font-synthesis` permits synthetic bold and oblique and requires neither.
- **A face's style.** `usWeightClass` is the weight ("400 Normal (Regular)
  ... 700 Bold", `[[source: ms-opentype-os2-2026-10-21]]`), as CSS's
  Appendix A names it; `fsSelection` bit 0 is ITALIC, bit 9 OBLIQUE, bit 5
  BOLD (the style-link bit, clear on Apple SD Gothic Neo's 700 face).

## Where the installed fonts are

fontconfig's own defaults (`[[source: fontconfig-configure-2-15-0]]`): on
Darwin `/System/Library/Fonts`, `/Library/Fonts`, `~/Library/Fonts` and the
system's MobileAsset font directories; elsewhere `/usr/share/fonts`, plus
`$XDG_DATA_HOME/fonts` and `~/.fonts` (`[[source: fontconfig-fonts-conf-2-15-0]]`)
and the X11 font directories that exist. Font Book installs to
`~/Library/Fonts` or `/Library/Fonts` (`[[source: apple-font-book-settings]]`).
CoreText also lists 230 files inside `FontServices.framework` (fonts for
Apple's apps) and knows which fonts Font Book disabled
(`kCTFontEnabledAttribute`, `[[source: macos-sdk-ctfontdescriptor-h]]`); a
scan sees neither.

## Practice, read at source

| terminal | setting | how the styled faces are found | no such face |
|---|---|---|---|
| ghostty | `font-family`, repeatable (`[[source: ghostty-config-8f0dd37]]`) | the family searched per style by CoreText/fontconfig, scored codepoint > monospace > style name > italic > bold (`[[source: ghostty-discovery-8f0dd37]]`, `[[source: ghostty-sharedgridset-8f0dd37]]`) | synthesised, on by default |
| kitty | `font_family`, `bold_font auto` | weight and slant distance within the regular face's family; a variable face's axes (`[[source: kitty-fonts-common-fa9f8da]]`, `[[source: kitty-fonts-core-text-fa9f8da]]`) | regular; fontconfig's shear on Linux |
| alacritty | `font.normal.family` (`[[source: alacritty-font-0-15-1]]`) | the first face whose bold and italic traits both match (`[[source: crossfont-darwin-0-8-1]]`) | regular; fontconfig's embolden on Linux |
| WezTerm | `wezterm.font(family, {weight, style})` | its own reader and CSS Fonts 3 5.2 (`[[source: wezterm-parser-3725482]]`) | synthesised |

For a Regular+Bold family, kitty, ghostty and WezTerm all draw bold italic
from the Bold face; alacritty draws it Regular.

## Measured on this Mac

A scan of the macOS directories: 388 files, 805 faces, each read only for
its directory, `name`, `OS/2`, `head`, `post` and `fvar`, from a mapping
released at once, over eight threads: 2.3-3.5 ms per scan with the files in
the page cache, 6.9-12.8 ms in the first round of a first process, with the
picks for the six default names 0.07-0.29 ms more. CoreText, for
comparison: its list of available font URLs, 16-34 ms; descriptor matching
for the six default families, 4.0-5.1 ms. CSS 5.2 over the six default
names reproduces the paths and indexes the default stack named:
Menlo.ttc@0, Apple Color Emoji.ttc@0 (not the hidden
`.Apple Color Emoji UI` at 1), Apple Symbols, Hiragino Sans GB's W3 at 300
(400 searches down before up to the W6), Apple SD Gothic Neo@0, Kohinoor
Devanagari@0.

## Decisions

- Discovery: baud's own scan, one Rust matcher on both systems, chosen after the CoreText measurement.
- A family without a face for a style draws it from the face CSS picked,
  unsynthesised.
- Bold italic follows CSS 5.2 (the Bold face for a family with no italic,
  the Italic face for one with no bold italic), the text's reading and the practice above.
- A variable face serves the weights its axis covers, drawn at the searched value.
- The embedded faces answer to a name in `fonts`, ahead of an installed
  copy, as the research favoured; keeping them out of `fonts` was built first and corrected.
- Which names: the font's build gives the two stock italics the built
  family's nameIDs 1, 4 and 6, so the four embedded faces are one family,
  "Proto Hebrew Mono C" until decision 11 names it -- N_SHARE. The name table: nameID 1 "should be shared among at most four
  fonts that differ only in weight or style" (`[[source: ms-opentype-name]]`).
  Left as they were, the stock italics alone would answer to "JetBrains
  Mono", and with none installed CSS 5.2 would draw regular text from the
  Italic. The icon face and STIX Two Math answer to their own names.
- A name is looked up as a PostScript name (nameID 6), then a full name
  (nameID 4, US English else the first record, passed over where it is
  one of the face's own family names, nameID 16 or 1), then a family --
  kitty's order (`[[source: kitty-fonts-core-text-fa9f8da]]`, `[[source: kitty-fonts-fontconfig-fa9f8da]]`), over family first. On this
  Mac 264 of the 690 family names the catalogue held at the base are also
  some face's full or PostScript name, always a face of that family
  (fontTools). The pass-over takes nameID 1 as well as 16: a weight-split
  family's "Seravek Light" is its Light face's full name and nameID 1,
  and found as that face it drew bold and italic from all of Seravek,
  where the family search keeps them among the Light faces. With it, 689
  of the 690 family names the catalogue held at the base resolve to the
  same regular, bold, italic and bold italic faces, at the same axis
  values, as the family search gave; the other, "STIX Two Math", picks
  the embedded face, as the embedded faces come first, and the 692 now
  held add the embedded faces' two. 47 of the 692 are reached as a
  PostScript name, and none as a full name (a one-off sweep through
  baud's own resolve, not kept). Passing over only the preferred family
  (nameID 16 else 1), 77 of the 690 besides STIX Two Math resolved
  differently. CSS gives these names to `local()` only
  (`[[source: css-fonts-3]]` 4.3), which states no case rule; the
  family's caseless match is applied to them as a reading.
- On Linux, where a fontconfig configuration is found, its directories are
  the scan's whole list, the fixed list only where none is, as fc-list and the fontconfig-based terminals see exactly that set. Two
  departures from fontconfig stay open: a configuration that fails (a
  malformed root file, or an `<include>` without `ignore_missing` that is
  missing or malformed) fontconfig drops whole for its fallback
  configuration (`[[source: fontconfig-fcinit-2-15-0]]`), where baud keeps
  what it read before the failure; and with no `fonts.conf` found, that
  fallback also reads the system's and the user's `conf.d` and the
  user's `fonts.conf`, where baud takes its fixed list.
