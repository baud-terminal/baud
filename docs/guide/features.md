# Features

What you get when you run baud on a Mac today. Each item links to the page
that has the detail. What is not built yet is on [Status](status.md).

## The window

- **Liquid Glass.** The window is macOS 26's glass material, with your
  desktop showing through behind the text. How see-through and how dark it
  is can be tuned under [`[glass]`](configuration.md#glass).
- **Light and dark, automatically.** baud follows System Settings >
  Appearance and switches when it does, and it tells programs that ask when
  the appearance changes, so their colours can follow too
  ([configuration: `theme`](configuration.md#top-level)).
- **Your own colours.** Load a base16 or base24 scheme, or a ghostty theme,
  for each of light and dark ([`[palette]`](configuration.md#palette)).
- **Text stays readable.** Text that would be nearly invisible on its
  background is lifted just enough to read, without touching pictures drawn
  in text or colours a program chose on purpose
  ([`floor`](configuration.md#palette)).
- **Zoom.** The View menu makes the text larger or smaller, and returns it
  to its usual size.

## Text and fonts

- **Fonts that work out of the box.** JetBrains Mono, with its ligatures and
  real bold and italic faces, baud's own Hebrew, and the Nerd Font icons
  that prompts and file managers use, all built in. The system's emoji,
  symbol, and Chinese, Japanese and Korean fonts fill in the rest
  ([configuration: `fonts`](configuration.md#top-level)).
- **Emoji drawn as one character.** A family, a flag or a skin-tone emoji
  takes the columns of one character, as the Unicode Standard says, and does
  not break apart into pieces ([vt-coverage](../vt-coverage.md)).
- **Columns that line up.** Character widths come from the Unicode data
  itself, so tables, prompts and editors with Chinese, Japanese, Korean or
  emoji text stay aligned ([vt-coverage](../vt-coverage.md)).
- **Hebrew with its points.** Vowel points and marks attach to their letter
  and are drawn in the right order. Right-to-left layout is not built yet
  ([note AB](../research/notes/ab-right-to-left.md)).
- **Its own text engine.** baud reads fonts and arranges glyphs itself,
  including ligatures, with no third-party font library.

## The cursor

- **Your shape, your blink.** Block, underline or bar; blinking or steady;
  and it respects Accessibility's "prefer non-blinking cursor"
  ([`[cursor]`](configuration.md#cursor)).
- **Trails.** When the cursor jumps, it can leave a plasma glow, a flame,
  lightning, frost, a flying saucer and more. Each is shown in
  [the cursor gallery](../cursor.md). They switch off under Reduce Motion
  unless you choose one.
- **A blink that animates.** While hidden, the cursor can glow, fade, burn
  down or be lifted away, or simply switch off.

## Scrolling and history

- **Scroll back through everything.** The scroll wheel and trackpad move
  through the lines that have scrolled off the top.
- **A scrollbar that behaves like macOS's.** It appears while you scroll,
  widens when you reach for it, and fades away when you stop.
- **Smooth scrolling.** Output slides up instead of jumping, without
  falling behind a fast program ([`[scroll]`](configuration.md#scroll)).
- **Resizing keeps your text.** Making the window narrower or wider rewraps
  the lines rather than cutting them off.

## Mouse, selection and the clipboard

- **Select text, including history.** Drag to select; Option-drag selects a
  rectangle; Shift-click extends a selection. A selection can reach back
  into the scrollback.
- **Copy and paste.** Edit > Copy and Paste, or Cmd-C and Cmd-V. A paste is
  marked as a paste for programs that ask, so an editor does not run pasted
  text as commands ([input-coverage](../input-coverage.md)).
- **Copy from remote programs.** Neovim or tmux running over ssh can put
  text on your Mac's clipboard. No program can read your clipboard
  ([`[clipboard]`](configuration.md#clipboard)).
- **Links.** Cmd-click a hyperlink a program has marked as one to open it.
- **Mouse in programs.** Programs such as vim, htop and tmux get your clicks,
  drags and scrolls; hold Shift to select text instead
  ([input-coverage](../input-coverage.md)).

## The keyboard

- **Every key combination reaches the program.** baud speaks kitty's
  keyboard protocol and xterm's `modifyOtherKeys`, so editors such as Neovim
  and helix can tell Ctrl-I from Tab and bind keys other terminals swallow
  ([input-coverage](../input-coverage.md)).
- **Input methods.** Text you compose with a macOS input method is shown
  as you type it and sent when you commit it.

## Pictures

- **Images in the terminal.** baud shows Sixel images and kitty's graphics
  protocol, so image viewers and file-manager previews show real pictures
  ([vt-coverage](../vt-coverage.md#tier-3--xterm-extensions-in-universal-use)).
  Sixel is not announced to programs unless you turn on
  [`advertise_sixel`](configuration.md#images).
- **Pictures made of text stay solid.** A photo preview drawn with coloured
  characters is kept opaque on the glass so it still reads as a photo
  ([`pictures`](configuration.md#glass)).

## Working with programs

- **Full-screen programs.** vim, less, htop and the like get their own
  screen and give yours back untouched when they quit, with an optional
  fade ([`[screen]`](configuration.md#screen)).
- **No flicker.** Programs that redraw in one go are shown only when the
  redraw is complete.
- **Window titles.** A program can set the window's title.
- **The bell, your way.** A sound, a flash or nothing; and a program can ask
  for the Dock icon to bounce or the window to come forward
  ([`bell`](configuration.md#top-level)).
- **Shell prompts kept on resize.** Shells that mark their prompts redraw
  them cleanly after a resize, because baud leaves the lines the shell is
  about to redraw for the shell to redraw.
- **Compatible with what programs expect.** baud answers to the VT100,
  VT220, VT420 and xterm feature sets, checked against the original manuals
  ([protocols](../protocols/README.md), [vt-coverage](../vt-coverage.md)).

## Built to the standards, and fast

- **The standard is the requirement.** Every control sequence baud handles
  has a row saying what the standard says and what baud does, and every
  known place baud falls short is listed as a defect to fix
  ([vt-coverage](../vt-coverage.md),
  [open departures](../protocol-decisions.md#open-departures)).
- **Fast.** baud's core took in recorded program output faster than
  ghostty's and alacritty's on every recording it was measured on, by up to
  5.8 times ([Performance](performance.md)).
- **Embeddable.** The core that interprets a terminal's output is a library
  any program can use ([Embedding the core](../embedding.md)).
