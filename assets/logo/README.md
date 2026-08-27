# baud — logo files (6a)

Lockup: `baud ❯ █` — mono wordmark, amber prompt glyph, amber block cursor.

Every mark ships in **dark** and **light** mode, each **fixed** and **blinking**. Blinking runs at 1.06s, the classic VT cursor rate; use fixed anywhere the file may be printed, screenshotted, or shown in a store listing.

## Lockups (300 × 130)
| file | mode | cursor |
| --- | --- | --- |
| `baud-lockup-dark.svg` | dark ground, baked in | fixed |
| `baud-lockup-dark-blinking.svg` | dark ground, baked in | blinking |
| `baud-lockup-light.svg` | transparent, dark type | fixed |
| `baud-lockup-light-blinking.svg` | transparent, dark type | blinking |
| `baud-lockup-mono-black.svg` | single colour | fixed |
| `baud-lockup-mono-white.svg` | single colour | fixed |

## Symbol only, transparent (512)
`baud-mark-dark[-blinking].svg` (bright amber, for dark backgrounds) · `baud-mark-light[-blinking].svg` (deeper amber, for light backgrounds)

## App icon / favicon
`baud-icon-512-dark[-blinking].svg` — amber on near-black · `baud-icon-512-light[-blinking].svg` — near-black on amber · `baud-icon-32-dark.svg` / `-light.svg` — favicon. Corner radius 112 at 512 (22%).

## Colour
| role | hex | oklch |
| --- | --- | --- |
| ground | `#15171B` | oklch(0.19 0.012 250) |
| type on dark | `#F3F4F6` | oklch(0.96 0.004 250) |
| type on light | `#1E2128` | oklch(0.24 0.012 250) |
| amber, dark mode | `#F4A32C` | oklch(0.78 0.16 68) |
| amber, light mode | `#C8801F` | oklch(0.66 0.15 62) |

Light mode uses the deeper amber deliberately — `#F4A32C` fails contrast on white.

## Type
JetBrains Mono — 500 for the wordmark, 700 for the prompt glyph, letter-spacing -0.045em.

**These SVGs reference the font rather than embedding outlines**, so they render correctly wherever JetBrains Mono is installed or webfont-loaded and fall back to system mono elsewhere. Before handing off to a printer or a third party, open one in a vector editor and convert text to outlines.

## Clear space
Minimum clear space on all sides = the cursor width (22px at the 300px lockup). Don't place the lockup on a mid-tone ground; the amber needs the dark or light ground to hold contrast.
