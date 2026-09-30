# J. Text anti-aliasing: weight, blend, hinting and placement

The owner's call, 2026-09-30: baud's text must look the best of any
terminal, so its anti-aliasing is decided on research, with Jev asked each
question and anything under 0.6 sent back for more. This note records what
was found, what was decided, and what was deferred, with the numbers each
decision rests on. It supersedes D7 of `j-rendering.md` on two counts:
baud will now thicken text on macOS, and it will hint at small sizes.

Everything measured here is JetBrains Mono 2.304, baud's rasteriser against
CoreText on this Mac (macOS 27.0), unless a row says otherwise. The CoreText
probes are committed in `scripts/text-probes/` with a table of what each
established.

## Where baud stood

- **baud is not fuzzier than CoreText.** With CoreText's font smoothing off,
  the two draw the same pixels at 2x: ink within 0.3%, stems within
  0.01 px, mean per-pixel difference 0.005-0.007 of full coverage. Both put
  the glyph origin and the baseline on whole pixels.
- **The whole visible difference is weight.** Against CoreText with smoothing
  on, which is what every macOS app draws, baud is 15-18% lighter dark on
  light and 23-28% lighter light on dark.
- **Blending is not the difference.** CoreText's grayscale blend is OVER on
  the stored values, `bg + a(fg - bg)`, measured in sRGB and linear-sRGB
  contexts (`blend.swift`). That is baud's native blend exactly.

## What CoreText's font smoothing is

An outline dilation, and nothing else: the edge profile shows each edge
moved outward, not a reshaped coverage curve (`profile.swift`).

- **Keyed on the text colour alone.** Level `n = clamp(floor(4Y + 0.5), 0, 4)`,
  with Y the Rec.709 luminance of the linearised foreground. The steps fall
  at encoded fills 0.39 / 0.65 / 0.815 / 0.945, which linearise to
  0.125 / 0.375 / 0.625 / 0.875 (`sweep.swift`). The background never enters
  (`fgbg.swift`). Red, blue, magenta and mid grey land in level 1; green,
  yellow and cyan in 2; white in 4; black in 0.
- **The amount.** Total stroke gain `(0.0115 + 0.0050 n) * ppem` device px,
  capped at about 0.60 px (`fine.swift`). It depends on device pixels, not on
  how the scale was reached: 13 pt at 2x and 26 px at 1x are byte-identical.
  So at 2x a black stem gains about 0.30 px and a white one 0.60 px.
- **It is not isotropic.** Pixel dumps at 22-32 px fit a box, not a disc:
  top edges gain 2d, bottom edges gain nothing, left and right edges gain d
  each, corners stay square, with `d = min((0.0058 + 0.0025 n) * ppem, 0.30)`.
  The stem metrics in the probes only see horizontal growth, which is why
  they read as isotropic. The box model matches CoreText at 22-32 px to a
  mean per-pixel error of 0.0145; an isotropic disc of the same d is at 0.059.
  It also fits at 12-16 px. **At 11 px the shape is unexplained**, and was
  measured only in an unflipped (y-up) bitmap context, so "top" must be
  confirmed as glyph-top in baud's y-down raster before it is built.
- **AppKit's default** (`NSAttributedString` into a bitmap) is exactly level 0
  for black text and level 4 for white.

One correction to the prior art as first read: kitty does **not** give every
glyph the maximum level. Its mask context draws in `kCGTextFillStroke` mode
(`[[source: kitty-6d5d0c4-core-text]]`, the function that sets up its alpha-mask context), which
turns smoothing off: its masks are byte-identical with smoothing on and off.
alacritty's white-on-black context does get the maximum.

## Decisions

| # | question | decided | Jev | rests on |
|---|---|---|---|---|
| 1 | weight on macOS | replicate CoreText's dilation: five levels keyed on the text colour | 0.91 | the 15-28% measured gap is all dilation |
| 2 | how it is built | a dilated mask per (glyph, level), box model, in baud-font; no CoreText at run time | 1.00 (0.99 on where) | below |
| 3 | default blend on macOS | native sRGB-encoded OVER plus the dilation | 0.99 | CoreText's own blend |
| 4 | macOS font smoothing turned off (`AppleFontSmoothing = 0`) | honour it: no dilation, as native apps do | 0.84 | Zed reads the same key (`[[source: zed-e8c6c67-gpui-macos-text]]`) |
| 5 | weight on Linux | no dilation | 0.98 | no Linux toolkit or terminal dilates TrueType text |
| 6 | hinting | one runtime vertical-only light auto-hinter for every font; the font's bytecode run at build time only, as a test oracle | 0.95 | below |
| 7 | glyph placement | one grid-wide phase, `(cell_w - primary advance) / 2`, for every glyph | 0.98 | below |

**2, the mechanism.** The alternative was one undilated mask plus a
per-level coverage table in the shader, which keeps colour out of the atlas.
It is not visually equivalent: against the dilated mask, 19-35% of touched
pixels differ by more than 8/255 on high-contrast schemes, baud's default
`#d8d8d8` among them (level 3). 10-15% of the added ink lands on pixels with
zero coverage, which no table can reach, and a pixel's coverage cannot tell a
top edge (+2d) from a bottom edge (+0). Against CoreText at 22-32 px no
per-pixel function gets below 0.064 mean error. The masks stay inside baud's
design: the level is a typed five-variant enum, computed once per rendition
slot in the resolved-style cache, carried in the atlas key, which still fits a
word. Nothing is copied and nothing is added per cell. On the recordings, the
glyph-and-level keys are at most 2.15x the glyph keys (btop and
asciiquarium, about 59 KB), 1.00x for cat, less and the CJK recordings.

**5, Linux.** GTK4 (and so VTE, GNOME Terminal and Ptyxis) is always
grayscale (`[[source: gtk-f9b494e-gtksettings]]` maps any antialiasing to
`CAIRO_ANTIALIAS_GRAY`), slight-hinted at every scale since 4.18
(`[[source: gtk-f9b494e-gtkwidget]]`), and blends in encoded sRGB with no
darkening. Qt6, Chrome and Firefox do the same for TrueType fonts; Skia's
Linux 1.2/0.2 gamma and contrast apply to LCD text only. FreeType's own stem
darkening is meant for linear blending, "glyphs will appear heavy and fuzzy"
otherwise (`[[source: freetype-c30faba-ftdriver]]`, the darkening
parameters). The biggest difference between baud and the native Linux look
is slight hinting at 1x, which 6 addresses.

**6, hinting.** The owner asked to combine a vertical zone snap with
running the font's bytecode. JetBrains Mono's bytecode is ttfautohint
output, which is FreeType's auto-hinter compiled into TrueType: y-only, with
an x-height increase limit of 14 readable from its `fpgm`. FreeType's light
auto-hinter set to that limit puts every zone on the same pixel as the
bytecode at 9-24 ppem; per-glyph residue remains, mostly on punctuation. The
variable JetBrains Mono has no `fpgm` at all, so the planned baked instances
are unhinted. And FreeType's LIGHT mode already means "auto-hint every
TrueType font" (`[[source: freetype-c30faba-ftobjs]]`, the autohint
decision), the mode fontconfig's default `hintslight` selects. The v40
interpreter's design ignores x moves (`[[source: freetype-c30faba-ttinterp-h]]`;
`[[source: freetype-subpixel-hinting]]`), so running it on this font would
add nothing a y-only auto-hinter does not. So: one auto-hinter at run time,
and FreeType run at build time over the embedded faces' bytecode as a gate on
it, never linked.

CoreText does a third thing at 1x (`zones.swift`, `snapdet.swift`): a
vertical snap only at exact integer device ppem up to 18, the x-height
overshoot rounded, the cap snapped except at 14 and 15 px, baseline and
descender left alone, for every font tried. It is recorded and not copied.
**Open:** what the auto-hinter does at a fractional ppem, such as 13.5 px
after a zoom.

**7, placement.** baud rasterises at phase 0 and leaves the rounding slack
on the right. A grid-wide centring phase is ghostty's CoreText rule
(`[[source: ghostty-d9840f3-coretext]]`, "so that they aren't weirdly off to
the left"). Measured with baud's rasteriser and its own cursor and box
rules:

- text sits centred in the block cursor to 0.00 px at every size, against up
  to 0.57 px off under phase 0;
- spill past the cell edge halves and splits between the sides (with
  dilation at Bold 14 px, 8 glyphs cross against 21);
- heavy box frames look better at 7 of 10 sizes;
- sharpness is unchanged (within 0.03 of phase 0), and so is spacing,
  because one phase cancels between neighbours;
- one phase covers Regular, Bold and the baked instances, which share the
  600-unit advance.

A per-face or per-glyph phase is ruled out: centring a Nerd icon's 1-em
advance throws its ink 3-7 px sideways, and a phase depending on the span
would have to join the atlas key.

## Deferred

**LCD subpixel anti-aliasing.** Three rounds; Jev ended at 0.75 for an
opt-in that is off by default and disables itself on translucency, scaling,
rotation, OLED and an unknown subpixel order. The measurements favour off:

- a threshold written before measuring asked for a mean stem-contrast gain of
  0.05; LCD gives +0.021 at the primary eye model, and no blur level
  reaches the threshold;
- what it does reliably is even out stem contrast across glyphs, by about 40%;
- under the native blend it makes white-on-dark, baud's default, up to 14%
  lighter, and a colour-vision model predicts visible fringes on dark
  backgrounds; only linear per-channel blending removes both
  (`[[source: freetype-c30faba-ftlcdfil]]`), which means a blend change and
  re-tuning the dilation;
- baud's glass makes every background translucent, where no stack renders LCD
  (`[[source: gtk-blog-fractional-fonts]]`: GTK4's "antialiasing for fonts is
  always grayscale" because its compositing has no component alpha);
- the human evidence is thin and dated: one study against grayscale found
  grayscale losing at threshold acuity at 96 dpi, another found no gain in
  reading speed or comfort against tuned grayscale, with a preference for
  moderate ClearType.

The owner deferred it until the rest of the text work ships.

**An alternative blend option.** The owner asked for "linear plus an alpha
curve" beside native. kitty's curve (`[[source: kitty-6d5d0c4-cell-slang]]`,
`foreground_contrast_new`; defaults in `[[source: kitty-6d5d0c4-options]]`)
without dilation is kitty's exact look, but it reverses macOS's rule that
lighter text is drawn heavier: on dark backgrounds the correlation of weight
with text luminance goes from +0.97 to -0.93, and dim saturated colours come
out 22-27% heavier at 1x. With the dilation added it is too heavy in 18 of
20 configurations, and no retuning of the curve fixes that. ghostty's
`linear-corrected` (`[[source: ghostty-renderer-shaders-shaders]]`,
`cell_text_fragment`; `[[source: ghostty-d9840f3-config]]`) with the
dilation keeps the weight within 2.6% and the ordering, and makes edges
colour-correct; an exact-luminance variant closes its remaining gaps on
opaque backgrounds. But on baud's translucent backgrounds every in-shader
correction drifts 5-27% in weight from native, because the wallpaper under
the glass is unknown, and native's own fringe is under one just-noticeable
difference everywhere except green on red. Jev picked ghostty's rule at
0.79; the research favours no option. Deferred by the owner.

## Found on the way

- **Nerd icons are drawn 1.75 cells wide.** Every glyph in Symbols Nerd Font
  Mono has a 1-em advance, and baud scales fallback faces by the em, so at
  14 px an icon is 14 px wide in an 8 px cell and runs into the next one.
- **CJK from a fallback face sits 1-3 px left of centre.** ghostty scales
  such faces to an ideograph width instead.
- **baud's curve flattening cuts small curves.** A quadratic that deviates
  from its chord by less than 0.577 px is drawn as the chord (the ab_glyph
  rule the rasteriser is held to), up to about 0.14 px inside the curve at
  13 px.

## Jev

Every question went to Jev with baud's principles and the evidence of the
round; each answer is in the decision log.

| question | answers, in the order asked |
|---|---|
| blend space / default blend | native 0.82; native 0.99 |
| weight on macOS | dilation 0.91 |
| where and how to dilate | in baud-font 0.99; dilated masks 1.00 |
| honour smoothing turned off | yes 0.84 |
| hinting | vertical snap 0.74; auto-hinter with a bytecode oracle 0.95 |
| Linux weight | none 0.41; none 0.98 |
| placement | phase 0 0.35; centre 0.62; grid-wide centre 0.98 |
| LCD | off 0.44; off 0.71; opt-in 0.79; opt-in 0.75 |
| dilation under the curve option | off 0.08; off 0.82 |
| the alternative blend option | linear-corrected 0.76; linear-corrected 0.79 |
