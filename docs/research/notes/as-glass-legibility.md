# AS. Glass legibility: what Apple recommends, and what baud draws

Status: researched 2026-10-17; the regular material, the 0.70/0.60 dims,
Reduce Transparency and Increase Contrast are built on it (the record,
"Regular glass, 4.5:1 dims, and the two display settings"). Section 6 lists
what stays open.

The owner, 2026-10-17, verbatim: "i am thinking our glass is too transparent
so we need more research or something as text is hard to read, we should
follow apple recommendations for osx".

**Provenance.** Apple's pages were fetched on 2026-10-17, the developer pages
from the documentation JSON endpoint and the WWDC and support pages as HTML,
and each is pinned by key in `docs/research/sources/MANIFEST.toml` (group AS)
with its SHA-256. The SDK headers are Xcode's MacOSX27.0.sdk; the Mac runs
macOS 27.0. baud's lines are cited at `dad9260c`. The contrast figures in
sections 3 and 4 are **computed from a model, not measured in pixels**: no
screen capture was taken.

## 1. What Apple says

**The material.** From the HIG's Materials page
([[source: apple-hig-materials]]):

- "Only use clear Liquid Glass for components that appear over visually rich
  backgrounds."
- "Use the regular variant when background content might create legibility
  issues, or when components have a significant amount of text, such as
  alerts, sidebars, or popovers."
- "Don't use Liquid Glass in the content layer." Standard materials are for
  "elements in the content layer, such as app backgrounds".
- "If the underlying content is bright, consider adding a dark dimming layer
  of 35% opacity."
- "Thicker materials, which are more opaque, can provide better contrast for
  text and other elements with fine features."

WWDC25's "Meet Liquid Glass" ([[source: apple-wwdc25-219]]) gives three
conditions for Clear: "The element is on top of media-rich content; your
content layer won't be negatively impacted by a dimming layer; and, the
content sitting above it is bold and bright." Regular "can work anywhere". A
terminal body fails all three: what is behind it is other windows and the
desktop, and its text is small and often dim (ANSI greys, comments).

The SwiftUI `Glass.clear` page ([[source: apple-swiftui-glass-clear]]):
"ensure content remains legible by adding a dimming layer or other treatment
beneath the glass"; its example dims at black 0.3, where the HIG says 35%.

The HIG's Color page ([[source: apple-hig-color]]): "Liquid Glass appears
more opaque in larger elements like sidebars to preserve legibility."
`NSGlassEffectView.h` ([[source: sdk27-nsglasseffectview-h]]) has two styles,
Regular and Clear, a tint colour and a corner radius, and **no opacity or
thickness property**. WWDC25's AppKit session ([[source: apple-wwdc25-310]])
puts content in the glass's `contentView`, which baud already does.

**Contrast.** The HIG's Dark Mode page ([[source: apple-hig-dark-mode]]): "At
a minimum, make sure the contrast ratio between colors is no lower than
4.5:1. For custom foreground and background colors, strive for a contrast
ratio of 7:1, especially in small text." Its Accessibility page
([[source: apple-hig-accessibility]]): "If your app doesn't provide this
minimum contrast by default, ensure it at least provides a higher contrast
color scheme when the system setting Increase Contrast is turned on."

**Reduce Transparency.** `NSAccessibility.h`
([[source: macos-sdk-nsaccessibility-h]], line 99): "UI (mainly window)
backgrounds should not be semi-transparent; they should be opaque." AppKit's
page ([[source: apple-nsworkspace-reduce-transparency]]): "don't use
semitransparent backgrounds in the user interface. For example, use only
opaque windows." The Mac User Guide for macOS 26
([[source: apple-mac-help-display-accessibility-26]]): "Replace the
transparent effect used on some backgrounds in macOS with a solid background,
to improve contrast and readability." The material answers by itself --
"Reduced Transparency, makes Liquid Glass frostier" (WWDC25 219) -- but that
does not reach what baud paints over it. The owner's crate says the same:
"Honouring the setting means **not using the material at all**"
([[source: mlg-accessibility]] 3-16).

**Increase Contrast.** `NSAccessibility.h` line 93: "UI should be presented
with high contrast such as utilizing a less subtle color palette or bolder
lines." AppKit ([[source: apple-nsworkspace-increase-contrast]]): "present a
high-contrast UI." Apple asks that custom colours be tested under both
settings, separately and together ([[source: apple-adopting-liquid-glass]]).

**Apple's own Terminal.** Its guide ([[source: apple-terminal-guide-text-settings]])
offers opacity and blur sliders. Its Clear Dark profile
([[source: macos-terminal-clear-dark]]), is a background at alpha 0.95 with
blur 0.5; Clear Light ([[source: macos-terminal-clear-light]]) is white at
0.93. That is a nearly opaque background, not Liquid Glass and not a 22% dim.

## 2. What baud drew at `dad9260c`

- `[glass]` defaulted to the clear material, `dim_dark` 0.22, `dim_light`
  0.12, tints 0.54 and 0.14. The comments justifying clear said the HIG pairs
  it with a dimming layer "over bright content, which is what a terminal body
  is". The HIG does not say that: its dimming advice applies once clear is
  already the right choice.
- Neither Reduce Transparency nor Increase Contrast was read; only Reduce
  Motion was (note Y already recorded this).
- No `config.toml` was in the config directory on the owner's Mac, only a
  `config.toml.old` holding `dim_dark = 0.5`. So the owner was looking at the
  shipped defaults.
- The material's appearance follows the Icon & widget style key, which read
  ClearLight while the system was Dark, so the dark palette drew over the
  light tint, 0.14 rather than 0.54. The Liquid Glass slider read 0.5;
  the crate example's calibration assumed 0.

## 3. The worst case, computed

Default dark text `#d8d8d8` on the page `#101010` at alpha `a`, source-over
in encoded sRGB over a flat white backdrop, with the material darkening
nothing and no tint. That is the strictest bound, and a flat backdrop is
one blur cannot move. WCAG 2.2 ratio:

| dim | ratio over white |
|---|---|
| 0.22 | 1.15:1 |
| 0.35 | 1.60:1 |
| 0.50 | 2.50:1 |
| 0.95 | 11.96:1 |

4.5:1 needs about 0.67 and 7:1 about 0.79. Light text `#272727` on `#efefef`
over flat black is the light half's worst case. At 0.12 it is 1.13:1.
`the_default_dims_keep_text_readable_over_white_and_black` computes these
with baud's own rounding, no material and no tint: 1.14 and 1.13 at the old
defaults, then 5.07 (dark over white) and 4.62 (light over black) at 0.70 and
0.60. Section 4's figures are higher because they include the material's
darkening and the tint.

## 4. Over real wallpapers, modelled

The model: a blurred wallpaper, then a material gain k, then the tint, the
body dim and the text. Every blend is source-over in encoded sRGB. k = 0.695
was fitted to the record's three on-screen captures ("The glass follows the
appearance now"), with residuals of -2.9, +3.8 and -0.1 luma. k = 1 bounds the
other side. Default dark text, worst window position / median, at tint 0.14
and a 16 pt blur:

| wallpaper (mean luma) | 0.22 | 0.50 | 0.70 | 1.00 |
|---|---|---|---|---|
| Tahoe Evening (72) | 5.21/10.26 | 7.70/11.48 | 9.99/12.29 | 13.35 |
| Sonoma (136, extrapolated) | 3.74/6.35 | 6.20/8.74 | 8.89/10.71 | 13.35 |
| iMac Yellow (213, extrapolated) | 3.14/3.77 | 5.48/6.23 | 8.27/8.90 | 13.35 |
| flat white (k-fit, tint 0.14, extrapolated) | 2.94 | 5.22 | 8.04 | 13.35 |

At 0.22 the worst position passed 4.5:1 on 5 of 21 wallpapers. With k = 1
that fell to 2. The light half at 0.12 was the worst case in the study:
its median passed on 2 of 21 wallpapers. 0.70 and 0.60 are the lowest dims
that keep normal text at 4.5:1 at the worst position on all 21, under both
bounds. Panels fared worse than the default text: at 0.22, none of btop's
letters reached 4.5:1 over white, and 40% did at 0.50.

The model leaves out the material's rim and highlight and the slider at 0.5,
and it never models regular against clear. No data exists to set that
difference, so none was invented.

## 5. Decisions

Jev, uncached, `decisions.jsonl` 716-719 (716/717 the first round and its
order-reversed replicate, 718/719 a re-ask of the two open questions after
more research):

| question | Jev | taken |
|---|---|---|
| default style | clear, 0.92 / 0.95 | **not taken**: regular |
| default dims | 0.70 dark, 0.60 light, 1.00 / 0.96, DECIDED | taken |
| Reduce Transparency | opaque cells, titlebar strip left as material, 0.60 / 0.65 then 0.80 / 0.66 | the lean, below the bar |
| Increase Contrast | floor trigger 3.0, target 4.5, 0.71 / 0.73 then 0.88 / 0.87 | the lean, below the bar |

**Why clear was not taken.** The request modelled only clear and marked
regular and the standard material "unmodelled". So Jev picked the only arm
that had numbers. The owner asked to follow Apple, and Apple's text says
regular for a surface with a significant amount of text. The dims hold for
regular as well, because they are set at the bound where the material
darkens nothing.

**Reduce Transparency's opaque cells are the owner's** (note Y, decision 9,
"Yes, opaque then"). Jev's lean covers only the 32 pt titlebar strip, 5.1% of
the default window, which stays material that the system frosts.

**Increase Contrast's floor is the owner's** (note Y, decision 9), and so is
its overriding `floor = "off"`, by analogy (note Y; the owner can reverse
it). A configured trigger or target above 3.0 or 4.5 is kept rather than
lowered. That is this note's reading of "stricter". The brief's "target
4.5" and the code's `max(configured, 4.5)` agree for every config whose
floor is at or below 4.5, because the config's `floor()` already parses
every target to at least 4.5. They differ only for a floor above 4.5, such
as `floor = 7`, which a literal 4.5 would loosen against decision 9's
"stricter floor"; that is not a departure.

## 6. Open

- **For the owner: a standard material instead of glass for the body.**
  Apple puts Liquid Glass on controls and standard materials on "app
  backgrounds" ([[source: apple-hig-materials]]). For a see-through window
  that is `NSVisualEffectView` blending behind the window
  ([[source: apple-nsvisualeffectview-underwindowbackground]]: "create a
  sense of peeking through the back of the window"), picked "based on its
  intended use" ([[source: apple-nsvisualeffectview-2026-10-17]]). It needs
  the owner's crate, so it is the owner's call.
- **An on-screen capture of regular against clear at 0.70/0.60**, which would
  replace section 4's model with pixels.
- **The floor measures against the page as stated**, not as seen through the
  glass: `lift_on_glass` mixes the page with the default page at the dim.
  Under the light half at 0.22 that lifted btop's `#cccccc` on its black page
  to `#4b4b4b`, which then passed 4.5:1 over no wallpaper modelled. Its
  effect at 0.60 is unmeasured.
- **Palette colours that fail with no glass at all** (dim 1.0): dark faint
  4.07, ansi1 3.26, ansi4 `#0000ee` 2.02, ansi5 4.06, ansi12 4.01; light faint
  2.96, ansi7 2.12. These are the palette's, not the glass's.
- **Under Increase Contrast at trigger 3.0, btop's greys did not change**
  (0 of 4234 glyphs on dark btop), where note Y predicted they would
  brighten. Not checked.
- **The CPU/GPU pixel oracle's glass arms** (`glass_floor_gate` and the
  image-over-glass gate in `baud-present-macos/tests/cells.rs`) still run
  only at alpha 56 and do not exercise the shipped alphas 179 (dark) and
  153 (light).
- **Linux reads neither setting**; it has no counterpart wired.
