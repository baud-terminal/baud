# baud — logo files (6a)

Lockup: `baud ❯ █` — mono wordmark, amber prompt glyph, amber block cursor.

Every mark ships in **dark** and **light** mode. **Every file here is fixed.** The six `-blinking` files carry no animation: each draws the same pixels as its fixed twin and differs from it only by whitespace. They keep their names because the README's header and this sheet name them. Giving them the animation this sheet once described would make the README's header blink, so the sheet was corrected instead.

## Lockups (300 × 130)
| file | mode | cursor |
| --- | --- | --- |
| `baud-lockup-dark.svg` | dark ground, baked in | fixed |
| `baud-lockup-dark-blinking.svg` | dark ground, baked in | fixed (named blinking) |
| `baud-lockup-light.svg` | transparent, dark type | fixed |
| `baud-lockup-light-blinking.svg` | transparent, dark type | fixed (named blinking) |
| `baud-lockup-mono-black.svg` | single colour | fixed |
| `baud-lockup-mono-white.svg` | single colour | fixed |

## Symbol only, transparent (512)
`baud-mark-dark[-blinking].svg` (bright amber, for dark backgrounds) · `baud-mark-light[-blinking].svg` (deeper amber, for light backgrounds)

## App icon / favicon
`baud-icon-512-dark[-blinking].svg` — amber on near-black · `baud-icon-512-light[-blinking].svg` — near-black on amber · `baud-icon-32-dark.svg` / `-light.svg` — favicon. Corner radius 112 at 512 (22%).

`baud-icon-dock.svg` is the Dock icon: the dark icon at 80% of the canvas, padded to the macOS app-icon grid. Its mark keeps the 512 icons' geometry (the ❯ at x 130), so with the font's ❯ it sits about 15 units, 12 px at 512, right of centre, as the 512 icons do when drawn in JetBrains Mono; the stand-in glyph it replaced happened to fill that space. Whether to centre it is the owner's call. `scripts/bundle-macos.sh` rasterises it into the bundle's `baud.icns`, and the app embeds it for a run outside a bundle. `baud-mark-amber-tight.svg` is the mark cropped to its strokes, for the titlebar.

## Pictures for GitHub
| file | what | size |
| --- | --- | --- |
| `baud-github-org.png` | the organisation picture: `baud` stacked over `❯ █`, ink `#15171B` on an amber `#F4A32C` ground, the amber filling the square to its edges so GitHub's own rounded corners apply | 500 × 500 |
| `../readme/social.png` | the repository's link picture: the lockup and the tagline on the ground, over amber telegraph tape punched `baud` (`assets/readme/README.md`) | 1280 × 640 |

Both arrangements are new to this sheet. The stacked one sets the word over the lockup's own mark row, the ❯ and cursor at the lockup's proportions, both centred and three quarters of a cursor width apart, with more than a cursor width of clear space to every edge. It keeps the word legible at the 24 px a repository page shows the picture at, where the one-line lockup's is not. Both are within GitHub's limits ([[source: github-docs-profile-picture]], [[source: github-docs-social-preview]]): opaque RGB, under 1 MB, at the sizes GitHub recommends. `the_github_pictures_meet_githubs_limits` (`baud-app/tests/brand_pictures.rs`) holds them there.

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

**The lockups, symbols and 512 and 32 icons reference the font rather than embedding outlines**, so they render correctly wherever JetBrains Mono is installed or webfont-loaded and fall back to system mono elsewhere. Before handing off to a printer or a third party, open one in a vector editor and convert text to outlines.

**The Dock icon, the titlebar mark and the GitHub pictures carry the font's own outlines.** `scripts/make-brand-pictures.py` traces them from JetBrains Mono 2.304 and draws the PNGs, byte for byte from the pinned font and tools (its `--check` compares). It takes 400 and 700 from `baud-app/fonts/jetbrains-mono/` and the 500 from the release archive ([[source: jbm-2304-release]]), which no committed file carries. Edit the script, not those files. With the ❯ as text, the Dock icon showed a system face's ❯ wherever JetBrains Mono was not installed: rsvg-convert, which draws the bundle's icon, and AppKit's NSImage, which draws it for a run outside a bundle, each fell back to a different face. With the outline, both draw the font's ❯. The glyphs are the font's, under its OFL: `JetBrainsMono-OFL.txt` is beside them.

## Clear space
Minimum clear space on all sides = the cursor width (22px at the 300px lockup). Don't place the lockup on a mid-tone ground; the amber needs the dark or light ground to hold contrast.
