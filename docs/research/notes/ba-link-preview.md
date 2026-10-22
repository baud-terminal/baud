# BA. Holding Cmd over a link: prior art, the modifiers and every perturbation

What a terminal shows of a link's address before it is opened, and where
its own bars sit against a program's images. Read 2026-10-22 for lanes
b149 and b150; every source below is pinned in
`docs/research/sources/MANIFEST.toml`. The decisions and what was built
are the record's, "Holding Cmd over a link shows where it goes".

## What other terminals show

Read at source: Ghostty shows every hovered URL in a bottom-left banner
that moves right under the pointer and escapes controls, bidi and
invisible characters (`[[source: ghostty-urlhoverbanner]]`,
`[[source: ghostty-untrustedurl]]`); kitty draws the target in a bar
along the top, the bottom when the link is high, the host in punycode
(`[[source: kitty-hyperlink-target]]`,
`[[source: kitty-sanitize-url]]`); iTerm2 underlines on Command and puts
an OSC 8 link's URL in the corner that covers it least (`[[source:
iterm2-hover-url-view]]`); WezTerm underlines and shows no target
(`[[source: wezterm-hover]]`); Terminal.app has no source to read. The
OSC 8 spec asks terminals to "let the user know the URI upfront"
(`[[source: osc8-spec]]`).

Jev was asked twice in both orders with renders of each option
(`baud-render`'s CPU path). E_BYTES, every byte outside printable ASCII
as `\xNN`, is so that no control, bidi control or look-alike letter is
drawn (`[[source: rfc3986]]` 1.2.1, `[[source: utr36-15]]` 2.5).

## The refusal words

The refusal words are not all the ones Jev was shown: "no host, or a
file on another computer" covers both of `Refused::Host`'s cases, and "a
mail link with extra headers" every header but `subject` and `body`,
where the request said "a mail link that adds hidden recipients".

## The bar's look

The bar's look, cell-aligned text in the selection's colours, was asked
on its own with renders of the bar as built, a framed bar and one lifted
off the grid, and a system tooltip described: Jev decided K_SELECTION,
0.96/0.99. A program's text can copy it exactly, and with a Sixel or
kitty image any look on the grid; the bar shows only while the chord is
held over a link.

## The modifiers and the view

On Linux a change of the modifiers is an event of its own; on macOS the
window crate hands none, so the pointer's cell is kept whatever the
modifiers, and each display-link tick, while the pointer is over the
text, reads `NSEvent.modifierFlags` and refreshes the preview before it
decides whether to draw, owing a frame when it changes: a press or
release of Command alone shows at the next tick, on an idle screen with
a steady or hidden cursor too
(`a_press_of_command_alone_shows_on_the_next_frame`,
`the_display_link_tick_reads_the_link_preview`). Every read builds its
spot from the view shown then, as a click would, so a window held on its
line while output retires lines, or moved by the wheel, previews the
link under the pointer and not the one the last event saw
(`a_held_preview_follows_the_view_as_output_moves_it`,
`a_wheel_under_a_held_preview_reads_the_new_view`,
`a_held_preview_of_a_uri_in_the_text_reads_the_lines_shown`,
`a_held_preview_follows_the_window_over_the_history`).

## Over a program's images

An image over the text, every Sixel and a kitty image at z >= 0, was
drawn last on all three paths, so a picture on the bar's row hid part of
the address or kept one of its own where the bar shows. kitty, the
graphics protocol's author, draws its visual bell, scrollbar and
hyperlink target after the images
(`[[source: kitty-hyperlink-target]]`, `draw_cells_with_layers()`);
Ghostty's banner and iTerm2's URL view are views over the surface. Jev,
asked in both orders with renders of each, leaned to the bars over the
images (O_BARS_OVER at 0.54 and 0.46; re-asked after kitty's order was
read, 0.26, and O_IMAGE_OVER at 0.36 reversed: the lean, three asks of
four).

## Every perturbation

Each red and restored: no damage on the preview's rows; the instance
path without the underline (the oracle and the CPU test); bytes from
`0x80` drawn as they decode; a cut from the end alone; a preview without
the keyboard; the bar never moving; no refusal label; a refresh that
ignores the modifiers held now; a modifiers event on every
`wl_keyboard.modifiers` (`a_change_of_the_modifiers_is_an_event`); a
refresh with the `back` of the last pointer event (the four held-preview
tests); the pointer's cell kept only while the chord is held (the press
test and `ctrl_held_over_a_link_previews_it`); the tick's call to the
hover poll deleted, its owed frame dropped, or the poll moved before the
focus poll (`the_display_link_tick_reads_the_link_preview`). Over
images: at the base every overlay case fails, the image's pixels
unchanged under it; the CPU path's images blitted after the overlays
again, or a row holding both left to its damage (the second frame loses
the bar); the GPU records drawn before the images in the instance arm,
the cell arm or both (the arms disagree, or the bar is the image's). The
driver's count of retired lines cancels out of what the preview reads,
so a stale one cannot be seen.
