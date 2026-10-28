# AZ. A window in the background

What a terminal draws while another window has the keyboard: the cursor,
and on macOS the selection. Read 2026-10-22; every source
below is pinned in `docs/research/sources/MANIFEST.toml`. Terminal.app is
closed source and was not read; its behaviour is not counted either way.

## The texts

- **xterm's manual** (`[[source: xterm-manpage-411]]`), `alwaysHighlight`:
  "By default (if this resource is false), a hollow text cursor is
  displayed whenever the pointer moves out of the window or the window
  loses the input focus." OTHER FEATURES: "If the window is the focus
  window, then the text cursor is highlighted no matter where the pointer
  is."
- **xterm's code** (`[[source: xterm-charproc-c-bccd466]]`), `ShowCursor`:
  `filled = (screen->select || screen->always_highlight) &&
  isCursorBlock(screen)` (13743). An unfilled box draws the cell's text as
  it is and an outline over the cell in the cursor colour (13973-13978).
  An underline or bar cursor is drawn in its own shape whatever the focus
  (13853-13871).
- **Focus reporting** (`[[source: ctlseqs]]`): `?1004` sends `CSI I` and
  `CSI O` as the window gains and loses focus. Nothing ties it to how the
  cursor is drawn, and no terminal below tests it before drawing.
- **Apple** (`[[source: apple-hig-windows]]`): "The system gives main,
  key, and inactive windows different appearances to help people visually
  identify them", and "Make sure custom windows use the system-defined
  appearances. People rely on the visual differences between windows to
  help them identify the foreground window and know which window will
  accept their input."
- **AppKit's unemphasized selection** (`[[source: sdk27-nscolor-h]]`
  232-235): `unemphasizedSelectedTextBackgroundColor` and
  `unemphasizedSelectedTextColor`, "e.g. when the associated
  control/view/window is not key". Resolved through AppKit in sRGB under
  each appearance, never sampled from the screen: aqua `#dcdcdc` with text
  `#000000`, darkAqua `#464646` with text `#ffffff`. An `NSTextView`'s
  default `selectedTextAttributes`, read headless from AppKit, carry a
  background and a text colour both, so the text view recolours selected
  text.

## Practice

| terminal | unfocused block | unfocused bar or underline | where |
|---|---|---|---|
| xterm | hollow | kept | `[[source: xterm-charproc-c-bccd466]]` 13743 |
| iTerm2 | a frame | kept | `[[source: iterm2-7ce037e-cursor]]` 146-218, 331-334; `[[source: iterm2-7ce037e-metal-per-frame-state]]` 456-472 |
| kitty | hollow (its cursor_shape_unfocused) | hollow | `[[source: kitty-shaders-c-851cb80]]` 1019; `[[source: kitty-options-definition-851cb80]]` 398-407 |
| ghostty | hollow | hollow | `[[source: ghostty-cursor-9bbb9b2]]` 59-60 |
| foot | hollow (its unfocused-style) | hollow | `[[source: foot-render-cb27717]]` 645-655 |
| alacritty | hollow (its unfocused_hollow) | hollow | `[[source: alacritty-display-content]]` 58-59 |
| wezterm | outlined block | outlined block | `[[source: wezterm-e2a98e1-render-mod]]` 731-737 |

The outline is one pixel in xterm, one logical pixel in foot
(`min(scale, cell w, cell h)`, 342-356) and a one-point `NSFrameRect` in
iTerm2. baud's bar and underline are a line of `max(underline thickness,
2)` pixels, 2 at the default size on a 2x display: one point.

kitty's trail has no focus test (`[[source: kitty-cursor-trail-851cb80]]`
74-84; drawn at `[[source: kitty-shaders-c-851cb80]]` 3021-3023).

iTerm2 dims the selection of a view that is not in front: its colour 2/3
of the way to grey 0.5 (`[[source: iterm2-7ce037e-ptytextview]]`
6578-6580, `[[source: iterm2-nscolor-9e29604]]` 756-778). No other
terminal read changes its selection colour with the focus.

## Decisions

Each was weighed with baud's own CPU renders:

- **Whether the cursor changes at all**: no text settled it, and on 2026-10-22 it became a setting, built that day as `[cursor] unfocused_hollow`, `"on"` or
  `"off"`, off by default, today's look; on, every shape is the hollow
  box (below), a trail runs and `?1004` changes nothing, the two
  conditional choices below.
- **How thick the box's sides are**: the bar's line, `max(thickness, 2)`
  pixels, one point at the default size on a 2x display. kitty draws its
  box at 1pt (`[[source: kitty-decorations-851cb80]]` 231-238), foot at
  one logical pixel, iTerm2 at one point; alacritty
  (`[[source: alacritty-display-cursor]]` 20-32) and WezTerm
  (`[[source: wezterm-e2a98e1-customglyph]]` 654-676, 5051-5125) at
  their bar's thickness; xterm and ghostty at one device pixel
  (`[[source: ghostty-metrics-9bbb9b2]]` 31-34,
  `[[source: ghostty-special-9bbb9b2]]` 300-324). Chosen from baud's own renders of 1 px, the bar's line and 15% of the cell, and WezTerm's stroke.
- **With a hollow cursor, every shape is hollow**, against a block alone (xterm, iTerm2). The other rule, "a bar or underline cursor keeps its shape", is how xterm's code and iTerm2 draw it; both were rendered.
- **Conditional on a hollow cursor:** a trail still runs while unfocused; `?1004` changes nothing.

Each option against the texts above:

- An unchanged cursor is not xterm's documented default: with
  `alwaysHighlight` false, "a hollow text cursor is displayed whenever
  ... the window loses the input focus". A hollow block conforms to it.
- A hollow bar or underline is not xterm's code, which draws both in
  their own shape whatever the focus (13853-13871); a bar or underline
  kept conforms to it. The manual's "hollow text cursor" names no shape.
- `?1004` changing nothing conforms: ctlseqs ties focus reporting to no
  drawing.
- Whether a trail runs while unfocused is covered by no text quoted
  above; kitty's trail has no focus test, which is practice, not text.
- The HIG's "if you use custom implementations, you need to do this work
  yourself" covers "a window's background and button appearances"; it
  names neither a cursor nor a selection.

The selection on macOS:

- **Reverse video in an inactive window** takes AppKit's unemphasized
  pair: the colours AppKit documents for a selection
  "when the associated control/view/window is not key". No reading settled it. V13 chose reverse video
  for the selection after a side-by-side look; it did not decide how an
  inactive window draws it. baud has no selection in the system's colours,
  so reverse video is the one look that takes the pair. Taking it conforms
  to the header; unchanged follows no text.
- **A selection colour of its own** (`[palette] selection`, a theme file,
  `OSC 17`/`19`) is drawn unchanged in an inactive window. No text above covers it:
  AppKit's pair stands in for AppKit's own selection colours, and iTerm2
  is the one terminal read that changes a colour of its own.
