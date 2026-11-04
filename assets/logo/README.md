# baud — logo files (6a)

Lockup: `baud ❯ █` — mono wordmark, amber prompt glyph, amber block cursor.

**The mark is baud's own drawing of `❯` with the cursor in the next cell.** The ❯ is JetBrains Mono Bold's, placed in a cell as baud's renderer places a glyph, and the block cursor is the very next cell, whole, with no gap: the cell is baud's at 200 px, 120 by 264 with the baseline 204 below its top (width round(0.6 px), height round(1.32 px), baseline round(1.02 px)), scaled to each file. So the logo is the prompt and cursor exactly as a baud window shows them. In every file but the Dock icon the cell is as tall as the cursor was before, on the same rows, and the mark's ink is centred across the icon or under the word. `every_logo_mark_is_baud_drawing_the_prompt_and_cursor` (`baud-app/tests/brand_pictures.rs`) draws the ❯ and a block cursor through baud's CPU renderer at 200 px and requires every file's ❯ (ink height, centre against the cursor, centroid in its cell) and cursor proportions within half a pixel of that drawing; the files sit within 0.19 px of it (0.183 px at most: the 32 px icons' cursor proportions).

Every mark ships in **dark** and **light** mode. **Every file here is fixed.** The six `-blinking` files carry no animation: each is its fixed twin, byte for byte. They keep their names because the README's header and this sheet name them; an animation in them would make the README's header blink.

## Lockups (300 × 130)
| file | mode | cursor |
| --- | --- | --- |
| `baud-lockup-dark.svg` | dark ground, baked in | fixed |
| `baud-lockup-dark-blinking.svg` | dark ground, baked in | fixed (named blinking) |
| `baud-lockup-light.svg` | transparent, dark type | fixed |
| `baud-lockup-light-blinking.svg` | transparent, dark type | fixed (named blinking) |
| `baud-lockup-mono-black.svg` | single colour | fixed |
| `baud-lockup-mono-white.svg` | single colour | fixed |

The word is where it always was (x 40, baseline 88, size 62, letter-spacing -2.8); the ❯'s cell starts at x 192 on the cursor's old rows, 45 to 91; then every glyph moves 10.36 units right, so the ink, from the word's left to the cursor's right, is centred across the box. The launch splash (`baud-render/src/splash.rs`) draws the same lockup, moved by the same 10.362: `the_held_splash_is_the_lockup_files` (`baud-render/tests/splash.rs`) reads `baud-lockup-dark.svg` and holds the splash's held frame to it within a pixel at 1.6 pixels a unit, the block to its rect and the ❯ to its path's ink. The splash draws the word in the renderer's badge face, JetBrains Mono Bold, where the files trace it at 500, so the test holds the word's top, foot and centre, which the two weights share, and not its sides, which the Bold sets 0.68 units further out.

## Symbol only, transparent (512)
`baud-mark-dark[-blinking].svg` (bright amber, for dark backgrounds) · `baud-mark-light[-blinking].svg` (deeper amber, for light backgrounds) · `baud-mark-amber.svg`

## App icon / favicon
`baud-icon-512-dark[-blinking].svg` — amber on near-black · `baud-icon-512-light[-blinking].svg` and `-amber.svg` — near-black on amber · `baud-icon-32-dark.svg` / `-light.svg` — favicon. Corner radius 112 at 512 (22%).

`baud-icon-dock.svg` is the Dock icon: the dark tile at 80% of the canvas, padded to the macOS app-icon grid, its mark in the tile's upper left at 1.2 times the 512 icons' cell. It is a prompt at the start of a terminal's first line, in a terminal's own margin, and big enough to read at the Dock's 32 pt. The ❯'s cell corner is one of the 512 icons' cell widths (76.36 units of the 512 tile) in from the tile's left and top, and the mark is scaled about it; `the_dock_mark_is_a_prompt_in_the_tiles_upper_left` (`baud-app/tests/brand_pictures.rs`) holds it there, in the file and in the copy the app embeds. `baud-icon-dock-16.png` to `-1024.png` are it drawn at the iconset's sizes; `scripts/bundle-macos.sh` packs them into the bundle's `baud.icns`, and the app embeds the SVG for a run outside a bundle. `baud-mark-amber-tight.svg` is the mark in a 300 by 204 box tight to its height, as the title bar showed it before the lockups.

## Title bar
| file | appearance | type | amber |
| --- | --- | --- | --- |
| `baud-lockup-dark-tight.svg` | dark | `#F3F4F6` | `#F4A32C` |
| `baud-lockup-light-tight.svg` | light | `#1E2128` | `#C8801F` |

The window's title strip shows the full lockup beside the traffic lights, in the form for the window's light or dark (`baud-app/src/mark.rs`, which embeds both through `baud-app/assets/logo/`). Each is `baud-lockup-dark.svg` or `baud-lockup-light.svg`, word and mark, scaled so the ❯'s cell is 168 units tall, the cell of `baud-mark-amber-tight.svg`, on no ground: the strip is the ground. The box is snug to the ink across, from the word's left to the cursor's right, so the picture ends where the cursor does, and centred on the cell's rows up and down, so the mark sits on the strip's centre line. Both forms share one box, so a swap of appearance moves nothing. The app draws them at the points per unit the mark alone had, so the mark keeps its size and the word is added before it. `the_title_bars_lockups_are_the_lockups_cropped` (`baud-app/tests/brand_pictures.rs`) holds each to its lockup within one unit, and holds the app's embedded copy to the file.

## Pictures for GitHub
| file | what | size |
| --- | --- | --- |
| `baud-github-org.png` | the organisation picture: `baud` stacked over `❯ █`, the word `#F3F4F6` and the mark `#F4A32C` on `#15171B`, the ground filling the square to its edges so GitHub's own rounded corners apply, with baud's plasma on the word and the mark | 500 × 500 |
| `../readme/social.png` | the repository's link picture: the lockup and the tagline on the ground, over amber telegraph tape punched `baud` (`assets/readme/README.md`) | 1280 × 640 |

The stacked arrangement sets the word over the mark's row, both centred, three quarters of the old 22-unit cursor apart, with more than a cursor width of clear space to every edge. It keeps the word legible at the 24 px a repository page shows the picture at, where the one-line lockup's is not. `baud-github-org.svg` is the picture without the plasma.

**The plasma is baud's own**, on the word and on the mark. Two lights are laid over the SVG's drawing, each drawn by baud's CPU renderer (`baud-render/examples/brand_plasma.rs`). On the word: the launch splash's first pass at 0.0999 s, 0.1 ms before the second lights, seed `BA0D276F`, its box ending where the word does, in the word's colour and under the glyphs, as the splash draws it. On the mark: the cursor's plasma trail 0.12 s into a move from the ❯'s cell into the cursor's, seed 3, drawn at 200 px and scaled to the picture's cell, in the amber. Left of the word's end less half the splash cursor's height, the plasma's unit, the word's light is the splash's own frame pixel for pixel, which the example checks before it writes; with both lights at zero the picture is its SVG's drawing to the pixel, which the generator checks. The generator prints the frame times and seeds when it runs.

Both pictures are within GitHub's limits ([[source: github-docs-profile-picture]], [[source: github-docs-social-preview]]): opaque RGB, under 1 MB, at the sizes GitHub recommends. `the_github_pictures_meet_githubs_limits` (`baud-app/tests/brand_pictures.rs`) holds them there.

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

**Every file carries the font's own outlines and names no font.** Before, the lockups, symbols and icons named JetBrains Mono in text, so each viewer drew the ❯ from whatever face it had: JetBrains Mono's rode high and narrow beside the cursor, a Mac's fallback stood wider and centred, and the Dock icon fell back differently again in rsvg-convert and in AppKit. `every_logo_is_outlines_and_names_no_font` (`baud-app/tests/brand_pictures.rs`) holds every SVG here to paths and shapes.

`scripts/make-brand-pictures.py` draws every file here and the link picture from that one mark, and every PNG is its SVG drawn by the pinned resvg (the organisation picture's with the plasma over it), byte for byte from the pinned font and tools; its `--check` compares, and fails on a file here it does not draw. It takes 400 and 700 from `baud-app/fonts/jetbrains-mono/` and the 500 from the release archive ([[source: jbm-2304-release]]), which no committed file carries. Edit the script, not these files. The glyphs are the font's, under its OFL: `JetBrainsMono-OFL.txt` is beside them.

## Clear space
Minimum clear space on all sides = the cursor width (20.9 at the 300 lockup). The Dock icon's mark is the exception: its margin is one of the 512 icons' cursor widths, a terminal's own margin, where its own 1.2 times larger cursor is 91.64 wide. Don't place the lockup on a mid-tone ground; the amber needs the dark or light ground to hold contrast.
