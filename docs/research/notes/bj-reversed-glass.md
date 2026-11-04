# BJ. A reversed default page on the glass is raised until its text reads

**What changed.** baud drew every background at the window's dim, the reversed default one included. On the light theme at its dim, 0.60 (byte 153), the bar a program draws in `SGR 7` with the default colours -- less's prompt and search matches, vim's `StatusLine`, `StatusLineNC`, `VertSplit`, `TabLineFill` and `IncSearch` -- read 3.58:1 over a white backdrop and 4.01:1 over the light standard material, where the page's own text reads 4.62. DEC's indicator status line, drawn as `SGR 7` in the default colours, read the same, and so did the whole screen under `?5`. Now a cell whose own `SGR 7` is set, and every cell under `?5`, whose drawn page is a default draws that page at the lowest alpha, never below the dim, at which the default background as text reads 4.5:1 over every grey from black to white (`Theme::raised`, `Theme::reversed_alpha`). The margin a presenter clears to under `?5` takes the same alpha, so the gutter carries no seam.

**The texts.**
- WCAG 2.2 SC 1.4.3: "a contrast ratio of at least 4.5:1"; contrast is measured "with respect to the specified background over which the text is rendered in normal usage" (`[[source: wcag22]]`). Under a page drawn at alpha that background is the composite, source-over (`[[source: w3c-compositing-1]]`), read against the part of the backdrop "most like the text" (`[[source: wcag22-technique-f83]]`).
- DEC STD 070 § 5.6.2.5: "Reverse rendition is achieved by using the current value of the foreground color as the cell background"; § 5.6.2 requires each rendition to be distinguishable "without affecting the ability to recognize the character pattern within the cell"; the colour values are "implementation defined", which is what allows a translucent page (`[[source: dec-std-070]]` pages 5-101 and 5-102). 070-14 p. 14-5: the indicator's background "is the reverse of the Main Display background".
- ECMA-48 § 8.3.117 names SGR 7 "negative image" and says nothing about alpha (`[[source: ecma48]]`).

**The bytes**, by `Behind::over`'s model, encoded-sRGB blending with `Argb::mix`'s rounding, and found again by `reverse_raise.rs` from the drawn pixels:

| theme, over black to white | dim | raised | reads |
|---|---|---|---|
| light | 153 | 173 | 4.57 |
| dark | 179 | 179 (needs 146) | 6.60 |

**One backdrop on every system.** The raise reads black to white whatever material the theme's backdrop names, though the glass floor reads the standard material's narrower greys on macOS's default style, where 165 would do. The `?5` page is also the margin and the lights' line's page, and `the_line_draws_the_same_pixels_on_macos_and_on_linux` holds that line to the same pixels on the standard material and on Linux; a raise by the theme's backdrop drew 165 there and 173 on Linux, and that test went red. So the standard material draws 173 too, above what it needs. One theme draws one set of pixels on every arm: the CPU arm the Linux driver draws with and both Metal arms read the alpha from the one resolved slot. Blending in linear light would need about 0.865; the byte holds for the blending the model states.

**Not opaque.** An opaque reversed page meets § 5.6.2.5 to the letter and is what kitty, ghostty, alacritty, foot and wezterm draw by default (read at source: kitty v0.50.0 `background.slang` 336-346, ghostty d9840f3 `generic.zig` 3077-3099, alacritty v0.17.0 `content.rs` 214-219, foot 1.28.0 `render.c` 779-783, wezterm 20240203 `screen_line.rs` 836-846); iTerm2 v3.7.7 keeps it translucent unless "Keep background colors opaque" is on. Under `?5` an opaque page makes the window a solid slab, and ghostty, foot and iTerm2 keep that page translucent. The raise keeps both surfaces glass and reads at the bound the dims hold for the normal page.

**Keyed on bits and words.** The raise reads the cell's `SGR 7` bit, the screen's `?5`, and whether the drawn page's word is the default; never the colour drawn. An explicit colour equal to the default foreground keeps the dim. So does every page an application set, reversed or not, and an `SGR 7` cell inside `?5` is raised with the page around it.

**Tests.** `baud-render/tests/reverse_raise.rs`: the bar, the indicator and the `?5` page read at 4.5 or more over every grey from black to white on the canvas's pixels, at the bytes above, on either material; one byte lower reads below 4.5; the raise is one byte on every material; every other background keeps the dim; the dark theme is unchanged. Each light reading was 3.580:1 before. `lights_line.rs` no longer exempts the light theme's reversed line: its letters read at 4.5 on the raised page, where at the dim no colour reached it. `baud-present-macos/tests/cells.rs`, `a_reversed_default_page_is_raised_alike_on_every_arm`: both GPU arms and the CPU arm draw the bar and the `?5` page alike at 173, with the margin at the page's alpha.

**Draw time only.** The byte is found when the theme or the stored default pair changes (`OSC 10`/`11`), in a walk of at most 256 alphas, and rides in the resolved slot as its alpha; the parser, the grid and the per-byte path are untouched.

**Open.**
- A reversed cell with a colour of its own keeps the dim and its glass floor, and still reads below 4.5 over black to white: on the light theme 14 of `SGR 7;30`-`37` and `90`-`97` (2.41 to 3.37), 6 of 16 over the standard material, 7 of 16 on the dark theme.
- A set colour as text on a reversed default page, `SGR 7;40`-`47` and `100`-`106`, reads 1.00 to 1.21 on the light theme over black to white with the page raised, and 1.00 to 1.17 at the dim; `SGR 7;107` reads 5.25 raised and 4.12 at the dim. The page is raised by its word, so a `?5` screen has no hole at a set background; the glass floor leaves a default page's text alone, on the normal page too.
- A Sixel's `dim` and `clear` fills take the window's dim and the normal default background, so under `?5` they leave a patch at the dim.
- A configured floor target above 4.5 does not raise the page further; the raise holds the bound the dims hold.
