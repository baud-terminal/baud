# AZ. A window in the background

What a terminal draws while another window has the keyboard: the cursor,
and on macOS the selection. Read 2026-10-22 for lane b144; every source
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

Jev, uncached, both orders, with baud's own CPU renders; every figure is
Jev's confidence:

- **Whether the cursor changes at all** is not decided. The TODO item says
  "then the owner's call", and Jev leaned to leaving it unchanged in two
  rounds: 0.83/0.77, and 0.59/0.61 after the HIG text and the survey were
  added. It goes to the owner.
- **If a hollow cursor is approved, every shape hollow** is Jev's lean,
  0.50/0.68, against a block alone (xterm, iTerm2). The lane's task named
  the other rule, "a bar or underline cursor keeps its shape", as xterm's
  code and iTerm2 draw it, so the owner sees both against the render.
- **Conditional on a hollow cursor:** a trail still runs while unfocused,
  0.96/0.90; `?1004` changes nothing, 1.00/1.00.

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
