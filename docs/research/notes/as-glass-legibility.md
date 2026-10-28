# AS. Glass legibility: what Apple recommends, and what baud draws

Status: researched 2026-10-17; the regular material, the 0.70/0.60 dims,
Reduce Transparency and Increase Contrast are built on it, and so are the
floor's backdrop bound and the oracle's shipped alphas. Section 6 lists what stays
open.

The problem, 2026-10-17: the glass was too transparent for text to read
easily; the aim is to follow Apple's recommendations on macOS.

**Provenance.** Apple's pages were fetched on 2026-10-17, the developer pages
from the documentation JSON endpoint and the WWDC and support pages as HTML,
and each is pinned by key in `docs/research/sources/MANIFEST.toml` (group AS)
with its SHA-256. The SDK headers are Xcode's MacOSX27.0.sdk; the Mac runs
macOS 27.0. baud's lines are cited at `f6302a23`. The contrast figures in
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
does not reach what baud paints over it. The window crate says the same:
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

## 2. What baud drew at `f6302a23`

- `[glass]` defaulted to the clear material, `dim_dark` 0.22, `dim_light`
  0.12, tints 0.54 and 0.14. The comments justifying clear said the HIG pairs
  it with a dimming layer "over bright content, which is what a terminal body
  is". The HIG does not say that: its dimming advice applies once clear is
  already the right choice.
- Neither Reduce Transparency nor Increase Contrast was read; only Reduce
  Motion was (note Y already recorded this).
- No `config.toml` was in the config directory on the measuring machine,
  only a `config.toml.old` holding `dim_dark = 0.5`, so the screen showed the
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
was fitted to three on-screen captures of the glass following the
appearance, with residuals of -2.9, +3.8 and -0.1 luma. k = 1 bounds the
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

| question | taken |
|---|---|
| default style | regular, not clear |
| default dims | 0.70 dark, 0.60 light |
| Reduce Transparency | opaque cells, the titlebar strip left as material |
| Increase Contrast | floor trigger 3.0, target 4.5 |

**Why clear was not taken.** The first modelling covered only clear and
marked regular and the standard material "unmodelled", so clear was the only
arm with numbers. The aim is to follow Apple, and Apple's text says
regular for a surface with a significant amount of text. The dims hold for
regular as well, because they are set at the bound where the material
darkens nothing.

**Reduce Transparency's opaque cells are note Y's decision 9.** The
titlebar strip left as material is 32 pt, 5.1% of
the default window, which stays material that the system frosts.

**Increase Contrast's floor is note Y's decision 9**, and so, by analogy, is
its overriding `floor = "off"`. A configured trigger or target above 3.0 or 4.5 is kept rather than
lowered. That is this note's reading of "stricter". The brief's "target
4.5" and the code's `max(configured, 4.5)` agree for every config whose
floor is at or below 4.5, because the config's `floor()` already parses
every target to at least 4.5. They differ only for a floor above 4.5, such
as `floor = 7`, which a literal 4.5 would loosen against decision 9's
"stricter floor"; that is not a departure.

## 6. Open

- **Open: a standard material instead of glass for the body.**
  Apple puts Liquid Glass on controls and standard materials on "app
  backgrounds" ([[source: apple-hig-materials]]). For a see-through window
  that is `NSVisualEffectView` blending behind the window
  ([[source: apple-nsvisualeffectview-underwindowbackground]]: "create a
  sense of peeking through the back of the window"), picked "based on its
  intended use" ([[source: apple-nsvisualeffectview-2026-10-17]]). It needs
  the window crate, so it is open.
  [Note AT](at-standard-materials.md) has the research: Apple's text, the
  shipped materials and modelled renders.
- **An on-screen capture of regular against clear at 0.70/0.60**, which would
  replace section 4's model with pixels.
- ~~**The floor measures against the page as stated**~~ **Built**: the
  floor reads text against the page at its alpha over anything from black
  to white, section 3's bound, and against the part most like the text
  ([[source: wcag22-technique-f83]]). Where no colour reaches 4.5 against the
  range the text goes as far as 3, and to black or white only where 3 is
  out of reach too: at
  0.60 every lift on helix's and vimhl's dark pages is one of those, and
  most land near white. Under the light half at 0.22 btop's `#cccccc` on
  its black page, which went to `#4b4b4b`, now goes white (1.69:1); at
  0.60 it reads 3.58 and moves under neither rule.
- **Distinct colours lifted to one: built for colours with a hue.** A
  lifted colour keeps its OKLab hue and as much chroma as sRGB
  holds, at the same target, so btop's red meter part and grey track stay
  apart on the glass (0.095 in OKLab where the linear-light mix left
  0.017). Decision 6's "just far enough" and its hue are kept, as the fix allowed; the move from the linear-light mix to OKLab is open, with the renders. On a black page at 0.70 the room from 4.5:1
  to white is 1.91:1, so no lift makes two lifted colours SC 1.4.11's 3:1
  apart ([[source: wcag22-understanding-non-text-contrast]]). **Open**: greys, which have no hue, still land on one grey (btop's
  fade, `#404040` to `#979797`, all `#bcbcbc`; herdr's `#f7f7f7` and
  `#ffffff` on the opaque floor); keeping them apart means landing above
  decision 6's "just far enough". A spread over the room above the target did that, and is withdrawn, as decision 6 rules it out. The opaque floor keeps the mix,
  which washes the hue where it lifts toward white (a dark theme, or
  Increase Contrast's trigger 3); whether it follows is open too.
- **Text on the default page is left out of the glass floor.** Its own text
  is held at the bound by the dims (section 3); palette text on it is not:
  at 0.70/0.60 cava's bars, git-log's colours and yazi-walk's read below 3
  against the range. Not decided.
- **A lift that keeps distinct colours distinct.** At dark 0.70 btop's
  meter and graph ground, `#404040` on `#000000` (asked 2.03), is lifted
  to `#bcbcbc`, so every meter reads full. The cause is the lift rule,
  which takes every colour below the trigger to one target, and not the
  choice of bound: at k = 1 the range lifts the ground and the two
  endpoints would keep it dark, but at section 4's fitted k = 0.695 both
  the range and the endpoints lift it. btop's process-list fade,
  `#4e4e4e`-`#979797`, goes to the same grey, accepted as decision 2 of 2026-10-17. A
  lift that keeps distinct colours distinct is being built next: WCAG 2.2
  SC 1.4.11 asks 3:1 "against adjacent color(s)" of the "parts of
  graphics required to understand the content" ([[source: wcag22]]).
- **Palette colours that fail with no glass at all** (dim 1.0): dark faint
  4.07, ansi1 3.26, ansi4 `#0000ee` 2.02, ansi5 4.06, ansi12 4.01; light faint
  2.96, ansi7 2.12. These are the palette's, not the glass's. The floor at
  1.2 moves none; Increase Contrast's 3 lifts dark ansi4, light ansi7 and
  light faint. The light values are note Y's decision 3, faint decision 10,
  and decision 8 reserves a readable dark set for after research, Modus Vivendi first, which is to be researched first. **Open**, with renders of keep against OKLCH-lightness
  corrections (dark ansi1 `#e93327`, ansi4 `#3771ff`, ansi5 `#d61bd5`,
  ansi12 `#6469fe`; light ansi7 `#6c6c6c`). Leaving these slots to the
  floor draws the same as keeping them at the default trigger 1.2,
  because the floor moves none of the failing slots, which is why the
  renders show only keep against the corrections. Under Increase
  Contrast the two differ: dark ansi4, light ansi7 and light faint are
  lifted.
  **2026-10-18, the recommendations taken**: the corrections are built (note Y and 8). With no glass, dark ansi1 now
  reads 4.50, ansi4 4.50, ansi5 4.51, ansi12 4.52 and light ansi7 4.57,
  so of this bullet's slots only faint, dark 4.07 and light 2.96, stays
  below 4.5, and under Increase Contrast only light faint is lifted. As
  pages the trade goes the other way: default text on dark ansi1, 4, 5
  and 12 reads 2.97, 2.97, 2.96 and 2.95 (4.10, 6.59, 3.29 and 3.33
  before), and on light ansi7 2.84 (6.14), computed.
- **Under Increase Contrast at trigger 3.0, btop's greys did not change**
  (0 of 4234 glyphs on dark btop), where note Y predicted they would
  brighten. Not checked.
- ~~**The CPU/GPU pixel oracle's glass arms** run only at alpha 56~~
  **Built**: `glass_floor_gate` and `cpu_page_gate` in
  `baud-present-macos/tests/cells.rs` run at 56, 179 (dark) and 153
  (light).
- **Linux reads neither setting.** The Settings portal's
  `org.freedesktop.appearance` `contrast` (0 no preference, 1 higher
  contrast) is in version 2 of the interface
  ([[source: xdg-portal-settings-v2]]); there is no transparency key, so
  Reduce Transparency has no Linux counterpart. The driver speaks no D-Bus:
  reading the key needs a first-party client over the session bus socket
  (SASL EXTERNAL, `ReadOne`, `SettingChanged`), the way baud-wayland speaks
  Wayland. That fits the no-dependency rule, and it is a protocol to build
  and a Linux machine to run it on: open.
