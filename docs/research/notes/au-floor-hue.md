# AU. How the contrast floor keeps a lifted colour's hue

Status: decided 2026-10-18: option A. Each floor
keeps the reading of "hue" it is built on, and nothing changed in code:
the glass floor lifts in OKLab, keeping OKLab hue and as much chroma as
sRGB allows, and
the opaque floor lifts by decision 6's linear-light mix (record, "The
opaque floor keeps decision 6's mix").

**What a user sees.** At the default trigger, nothing differs between
the options on a dark theme: no glyph on any recording's final screen
lifts on the opaque floor, and the glass floor draws the same pixels
under every option that lifts in OKLab there. The difference shows under
Increase Contrast, and mostly on ANSI blue `#0000ee`. On an opaque
window it lifts to `#6b6bf1` (a lavender) under the mix; the OKLab lift
would draw an azure `#3871ff`. On the dark glass it draws `#9bbcff` at
the default trigger and `#adb8ff` under Increase Contrast, because the
opaque pass runs first.

## The question

Decision 6 lifts a colour that misses its contrast target "just far
enough" and keeps "each colour's hue". Toward black every option draws the worked examples the same. Toward white the readings part:
the mix keeps colorimetric hue (CIE xy and u,v angles, and sRGB channel
order) and turns a saturated blue purple; the OKLab lift keeps the hue
OKLab was fitted to (`[[source: ottosson-oklab]]`, fitted to "the
uniform perceived hue data used to derive IPT") and can reverse channel
order on dark blues (btop's `#3c318c` to `#b4b6ff`).

## Options measured

Every recording at 200x50, both themes, the glass and the opaque body,
default and Increase Contrast:

| Option | Rule | Merged colours | Mean CAM16 hue moved, IC opaque dark |
|---|---|---|---|
| A | glass OKLab, opaque mix (as built) | 921 | 2.79 |
| B | the mix on both | 2446 | 2.79 |
| C | OKLab on both | 911 | 9.71 |
| D8 | OKLab, falling back where channel order would flip | 905 | 9.71 |
| F | the mix toward the brightest colour of the same chromaticity | 930 | 2.52 |

B washes btop's meters out to full: under CAM16, 1340 of 1942 dark-glass
glyphs are left with no hue (colourfulness below 5, a threshold chosen
for this). F's small CAM16 movement is largely built in: it keeps
chromaticity, and CAM16 hue barely moves at fixed chromaticity (`#cd0000`
to `#ff0000` moves 0.03 degrees). C would cost 108-132 ns once per
rendition slot, not per byte.

## The case each way

For C: the CIE defines hue as a perception, and names a mix toward white
at constant dominant wavelength a change in hue (the Abney effect). CSS
Color 4 maps out-of-gamut colours at constant OKLCh hue
(`[[source: css-color-4-20261009]]` §14.2, normative for gamut mapping
only), and CSS Color 5 calls the sRGB mix of white and blue "a bit
purple". Five of the seven Apple increased-contrast dark colours that sit
more than 0.02 OKLab from either lift sit nearer the OKLab lift. And no
text treats a solid backdrop differently from a see-through one.

Against C, and for A: on saturated blue, the colour that lifts under
Increase Contrast, CIECAM16 sides with the mix (opaque: C moves blue 10.1
degrees, the mix 2.8; glass: 17.8 against 4.4, under all five viewing
conditions tried). The words "keeps each colour's hue" were written to
describe the linear-light mix. iTerm2 and xterm.js, the two terminals
that reliably reach 4.5:1, land nearer the mix on all four test colours.
C would overturn the opaque floor's own decision and fail two of the criteria fixed before measuring
(the opaque test, and no channel-order reversal on a final screen).

## The decision

Six options were weighed on three renders: A, B, A' (the opaque mix, glass D8), D8, C and F. The options that keep the mix on the opaque floor led, none of them clearly. The research recommended C; A, as built, was chosen.

## What other apps do

The question, 2026-10-18: what other apps, Apple's among them, do. Read at source or from shipped assets,
nothing run on screen; ANSI blue `#0000ee` lifted on `#101010`:

| App | When it adjusts | How | Blue becomes |
|---|---|---|---|
| Warp, Chrome's own UI | to 4.5:1 | blend toward white in encoded sRGB | `#6b6bf5` (the mix's neighbour) |
| xterm.js (VS Code, Tabby, JetBrains) | to its minimum ratio | encoded sRGB, 10% steps | `#6a6af7` |
| iTerm2 | to a brightness difference | encoded sRGB | `#7a7af6` on `#1e1e1e` |
| DevTools' suggestion | on request | HSV | `#7676ff` on `#1e1e1e` |
| Zed | to APCA Lc 45 | HSL | `#8888ff` on black, 7.01:1 |
| Windows Terminal | only in Windows High Contrast | OKLab lightness, a and b kept | `#256fff` where it acts, 3.81:1 |
| libadwaita accents | OKLab L >= 0.85 | OKLab, per-channel clip | `#65bcff`, 20 degrees of OKLCH hue toward cyan |
| Terminal.app | on by default, no target | CIELAB lightness band | `#663bff`, 3.29:1 |
| Material (HCT) | design time | CAM16 hue held | `#5761ff` on black |
| Apple Increase Contrast colours, Xcode High Contrast | designed | by hand | nearer OKLab where the two lifts differ |

Three things follow. Every runtime floor that reaches its target lands
next to the mix (Warp 0.007 OKLab from it, 0.057 from the OKLab lift).
Apple's designed palettes sit nearer the OKLab lift on the colours where
the two can be told apart (opaque dark 5 of 6, vibrant dark 2 of 2,
Xcode High Contrast 4 of 4), and Apple changes no static colour such as
`#0000ee` under Increase Contrast. No app reads hue one way on a
translucent surface and another on an opaque one; those that see alpha
ignore it, assume an opaque background, or, like DevTools, composite
over the worst backdrop. So 4A puts the opaque floor with the runtime
floors and the glass floor with Apple's designed colours; that split by
surface has no precedent, and neither single rule has a case strong
enough to overturn the decision. The research recommended keeping 4A.

Under Reduce Transparency every background is drawn at alpha 1, where
the glass lift leaves a colour unchanged and only the mix draws, so the
open item below appears only with Increase Contrast on and Reduce
Transparency off.

**Open:** under Increase Contrast the dark glass draws ANSI blue 13
OKLCH degrees nearer purple than at the default trigger, because the
opaque mix runs before the glass lift.
