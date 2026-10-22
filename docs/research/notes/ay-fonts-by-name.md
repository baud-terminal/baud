# AY. Fonts by family name, and a family's own bold and italic

The owner, 2026-09-29: "we shouldn't put where the ttf font is aka
directory, none of the other vts are doing that, they just put the font
name and size right?" and "if there is bold font, baud should not fake it
and use the bold font"; 2026-10-07: "the thing is that people can decide to
config their own font and not use the built in one so we need to support
that". Read 2026-10-21 for lane b141; every source below is pinned in
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
released at once, over eight threads: 2.3-3.5 ms per scan with the files
in the page cache, 12.8 ms the one time they were not, with the picks for
the six default names 0.07-0.29 ms more. CoreText, for comparison: its list of available font
URLs, 16-34 ms; descriptor matching for the six default families, 4.0-5.1
ms. CSS 5.2 over the six default names reproduces the paths and indexes the
default stack named: Menlo.ttc@0, Apple Color Emoji.ttc@0 (not the hidden
`.Apple Color Emoji UI` at 1), Apple Symbols, Hiragino Sans GB's W3 at 300
(400 searches down before up to the W6), Apple SD Gothic Neo@0, Kohinoor
Devanagari@0.

## Decisions (Jev, each asked in both orders)

- Discovery: baud's own scan, one Rust matcher on both systems -- D_SCAN,
  0.72/0.85, then 0.89/0.89 after the CoreText measurement: the lean.
- A family without a face for a style draws it from the face CSS picked,
  unsynthesised -- R_REGULAR, 0.98/0.98: decided.
- Bold italic follows CSS 5.2 (the Bold face for a family with no italic,
  the Italic face for one with no bold italic) -- B_CSS, 0.50/0.60, then
  0.78/0.70 after the practice above: the lean, and the text's.
- A variable face serves the weights its axis covers, drawn at the
  searched value -- V_AXIS, 0.82/0.82, then 0.94/0.96: decided.
- The embedded faces do not answer to a name in `fonts` -- E_OUT 0.67/0.59
  before the research; the re-ask after it chose E_IN_NAMES in both
  orders, 0.58/0.70 (probabilities 0.72/0.80). E_OUT wins only when the
  first ask's probabilities are pooled with the re-ask's, 0.485 to 0.483,
  so the lean depends on that pooling; and the split names (the built
  Regular and Bold are "Proto Hebrew Mono C", the stock italics "JetBrains
  Mono") would make "JetBrains Mono" a family with only italic faces. Taken: E_OUT.
