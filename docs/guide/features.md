# Features

What baud does for you today, by area. Every line is backed by a test or a
measurement in the repository, and the build fails if that proof disappears,
so this list cannot quietly fall out of date. What is coming next is on the
[roadmap](../roadmap.md).

Lines with no mark work in the macOS app and in the shared terminal core,
which the Linux version also runs; the Linux version has run only headless so
far. *macOS* marks what only the macOS app does, and *Linux, headless so far*
what only the Linux version does. **Partly** says what part is missing, and
**For now** marks a stopgap until its replacement lands.

[Programs just work](#programs-just-work) · [Speed](#speed) · [Text, fonts and emoji](#text-fonts-and-emoji) · [Keyboard, mouse and clipboard](#keyboard-mouse-and-clipboard) · [Scrollback, selection and resizing](#scrollback-selection-and-resizing) · [Window, colours and bell](#window-colours-and-bell) · [The cursor](#the-cursor) · [Pictures](#pictures) · [Platforms and settings](#platforms-and-settings) · [Embedding the core](#embedding-the-core) · [Where baud is behind](#where-baud-is-behind)

## Programs just work

Shells, editors and full-screen programs get the terminal they expect, and a broken one can't upset it.

- **No wrong answers** — A question baud cannot answer truthfully gets no reply rather than a wrong one, and a setting baud does not have is reported as unknown, never as off. <sub>[proof](../../baud-core/tests/color_scheme.rs) · [reference](../vt-coverage.md#tier-2--vt220--vt420)</sub>
- **A damaged escape code can't clear your screen** — A broken or cut-off control sequence is thrown away instead of being half-obeyed, and hidden data in the stream is never printed. <sub>[proof](../../baud-core/tests/esc_abandons.rs) · [reference](../vt-coverage.md#tier-0--the-parser-state-machine)</sub>
- **No flooding memory** — A giant title, link or other string a program sends is cut to size, so it can't fill memory. <sub>[proof](../../baud-core/tests/xterm/osc.rs)</sub>
- **Compatible with what programs expect** — baud answers to the VT100, VT220, VT420 and xterm feature sets, checked against the original manuals, and works under every common terminal type from xterm-256color to vt100. <sub>[proof](../../baud-core/tests/conformance/input_caps.rs)</sub>
- **Programs can tell what they are talking to** — A program that asks gets baud's name and version, and the VT100's original question gets the same answer as the modern one. <sub>[proof](../../baud-core/tests/xterm/keypad_version.rs) · [reference](../vt-coverage.md#csi-operations)</sub>
- **One letter, two meanings, kept apart** — A program asking to highlight a region no longer scrolls the screen: two controls that share a letter are told apart by their arguments. <sub>[proof](../../baud-core/tests/conformance/spec_inventory.rs) · [reference](../vt-coverage.md#csi-operations)</sub>
- **Cursor movement and editing** — Cursor moves, inserting and deleting, erasing, tab stops, repeats and the alignment test behave as programs expect. <sub>[reference](../vt-coverage.md#csi-operations)</sub>
- **A soft reset keeps you where you were** — A soft reset leaves a full-screen program on its screen, with its history and mouse tracking. <sub>[proof](../../baud-core/tests/xterm/soft_reset_scope.rs) · [reference](../vt-coverage.md#tier-2--vt220--vt420)</sub>
- **Protected text and saved settings** — Text a program protects survives its selective erase, and programs can save and restore their own settings. <sub>[proof](../../baud-core/tests/vt220/selective_erase.rs) · [reference](../vt-coverage.md#tier-2--vt220--vt420)</sub>
- **Cursor save and restore** — Saving the cursor keeps its position, colours, wrap, origin and protection. **Partly:** two of the character-set details are not saved yet. <sub>[reference](../vt-coverage.md#esc-sequences)</sub>
- **132 columns when a program asks** — A program that asks for 132 columns, and is allowed to, gets a wider window, and a reset puts your width back. *macOS.* <sub>[proof](../../baud-app/src/width.rs) · [reference](../vt-coverage.md#csi-operations)</sub>
- **Exact page widths** — Programs can ask to keep the screen across a column change, or for an exact page width. **Partly:** both are honoured in part; each row names the part. <sub>[reference](../vt-coverage.md#csi-operations)</sub>
- **VT52 mode** — Very old software that speaks the VT52's language, keyboard included, still runs. <sub>[proof](../../baud-core/tests/vt52/sequences.rs) · [reference](../vt-coverage.md#esc-sequences)</sub>
- **8-bit replies for programs that ask** — Replies and keys switch to 8-bit control codes when a program asks. <sub>[proof](../../baud-core/tests/vt220/replies.rs)</sub>

## Speed

Measured on real recordings and real keystrokes, with the numbers kept in [MEASUREMENTS.md](../../MEASUREMENTS.md).

- **Takes in output fast** — On real recorded program output, baud's core was faster than ghostty's and alacritty's on all nine recordings, from 1.19 to 5.80 times ghostty's rate; on the Chinese, Japanese and Korean recordings baud also does more work, grouping every character. <sub>[measured](../../MEASUREMENTS.md#against-other-implementations-on-real-capture)</sub>
- **Ahead through a real terminal connection** — On one Mac, through the same kind of connection a shell uses, baud led four other terminals on four of five recordings and used the least CPU on all but one. *macOS.* **For now:** measured before scrollback existed and not taken again since. <sub>[measured](../../MEASUREMENTS.md#five-terminals-through-a-pty)</sub>
- **What you type shows before the shell answers** — At a prompt your shell marks, a typed character is drawn about 3 ms sooner, and quietly taken back if the shell shows something else. *macOS.* <sub>[proof](../../baud-render/tests/predicted.rs) · [measured](../../MEASUREMENTS.md#a-keys-echo-drawn-before-the-pty-answers-row-9)</sub>
- **Keys first, even during a flood** — A key typed while a program streams output is drawn at once, about 2.8 ms sooner, instead of waiting for the next frame. *macOS.* <sub>[measured](../../MEASUREMENTS.md#pacing-a-keystrokes-frame-by-completion-row-6)</sub>
- **About 3 ms from key to screen** — A keystroke reaches the screen in about 3 ms through cat on a 120 Hz Mac, and `BAUD_KEYLAT=1` prints your own. *macOS.* <sub>[proof](../../baud-app/src/latency.rs) · [measured](../../MEASUREMENTS.md#keystroke-to-pixel)</sub>
- **Quiet when idle** — An idle window draws nothing and uses about half a percent of one core; a steady cursor costs no frames at all. *macOS.* <sub>[proof](../../baud-app/src/caret.rs) · [measured](../../MEASUREMENTS.md#idle-cpu-row-11-measured-no-change)</sub>
- **No half-drawn screens** — Programs that redraw in one go, such as btop, neovim and helix, never show a half-finished screen, and a program that stalls can't hold the window for more than 33 ms. <sub>[synchronised output](../vt-coverage.md#tier-3--xterm-extensions-in-universal-use) · [proof](../../baud-core/tests/xterm/embedder_modes.rs) · [reference](../vt-coverage.md#tier-3--xterm-extensions-in-universal-use)</sub>
- **Quick to start** — The GPU shaders ship already built, which took a cold launch from 390 ms to 134 ms when it was measured. *macOS.* <sub>[proof](../../baud-present-macos/tests/metallib.rs) · [measured](../../MEASUREMENTS.md#the-shipped-metallib-row-1-kept)</sub>
- **Smooth resizing and zoom** — A resize or zoom step costs well under a microsecond of the core's time against an 8 ms frame. *macOS.* <sub>[measured](../../MEASUREMENTS.md#a-drag-steps-core-work-row-10-measured-no-change)</sub>
- **Light on memory** — A 200 by 50 terminal adds about 0.3 MB of real memory until it fills, because space nothing has written costs nothing. **For now:** measured on the previous core and argued, not measured again, on this one. <sub>[measured](../../MEASUREMENTS.md#storage)</sub>

## Text, fonts and emoji

Text that lines up, in any script, with fonts that work out of the box.

- **Fonts built in** — JetBrains Mono is built in, with its ligatures and its real bold, italic and bold-italic faces, so text looks right with no setup. <sub>[proof](../../baud-app/src/config.rs)</sub>
- **Ligatures that keep the grid** — Ligatures such as => and != take exactly the cells of the characters they replace, in every style and in your own fonts. <sub>[proof](../../baud-font/tests/shape.rs)</sub>
- **Nerd Font icons built in** — The icons prompts, file managers and git tools draw work out of the box, one cell wide. <sub>[proof](../../baud-render/tests/narrow_in_wide.rs)</sub>
- **Maths and technical symbols** — A built-in maths font draws technical symbols, including the ones Claude Code shows. <sub>[proof](../../baud-app/src/host.rs)</sub>
- **Hebrew built in** — Hebrew letters and points are drawn from baud's own monospace Hebrew, made to match the built-in font, in regular and bold. <sub>[proof](../../baud-render/tests/hebrew_embedded.rs)</sub>
- **Italic Hebrew** — Italic Hebrew is drawn upright in the regular weight. **For now:** the built-in italic faces carry no Hebrew yet. <sub>[proof](../../baud-app/src/embedded.rs)</sub>
- **Pointed Hebrew keeps every point** — Every vowel point and cantillation mark stays on its letter, even several in one cell, however the output was split and after it scrolls into history. <sub>[proof](../../baud-core/tests/cluster_bounds.rs)</sub>
- **Every script, from your Mac's fonts** — Chinese, Japanese, Korean, Devanagari, colour emoji and extra symbols draw out of the box from the Mac's own fonts, and Menlo fills in some 1,600 characters JetBrains Mono lacks. *macOS.* <sub>[proof](../../baud-app/src/config.rs)</sub>
- **A fixed list of fallback fonts** — A character the first font lacks comes from the next font that has it, one character at a time. *macOS.* **Partly:** the system fonts searched are a fixed list, so a script outside it and outside your own fonts shows boxes. <sub>[proof](../../baud-render/tests/refused_glyph.rs)</sub>
- **A box, never a blank** — A character no font can draw shows a missing-glyph box, never an empty cell. <sub>[proof](../../baud-render/tests/refused_glyph.rs)</sub>
- **Accents no font can draw** — If your font lacks one accent on a letter, the letter's other accents still show. **Partly:** an accent no installed font can draw is left off rather than shown as a placeholder, which the Unicode Standard asks for. <sub>[proof](../../baud-render/tests/missing_mark.rs)</sub>
- **Your own fonts** — List your own fonts in order, each filling in what the ones before it lack, including one face of a font collection; baud's built-in fonts stay behind yours unless you turn that off. <sub>[proof](../../baud-app/src/config.rs)</sub>
- **Bold and italic in your own fonts** — With your own fonts, bold and italic text is drawn. **Partly:** it uses your font's regular face for now, because fonts can't yet be named by family. <sub>[proof](../../config.toml.example)</sub>
- **Variable fonts** — Variable fonts and the TrueType, OpenType, CFF2 and collection formats are read and drawn. **Partly:** there is no setting yet to pick a weight. <sub>[proof](../../baud-render/tests/varied_face.rs)</sub>
- **A bad font can't crash it** — A broken or malicious font file cannot crash the terminal. <sub>[proof](../../baud-font/tests/hostile.rs)</sub>
- **Its own text engine** — baud reads fonts and arranges glyphs itself, each script by its own font rules, matching HarfBuzz on 576 test cases, with no third-party font library. <sub>[proof](../../baud-render/tests/script_runs.rs)</sub>
- **Sharp at every size** — The font size is in points, scaled for the display, so text stays sharp on Retina screens and keeps its size when the window moves between screens. *macOS.* <sub>[proof](../../baud-render/tests/rescale.rs)</sub>
- **Zoom** — Cmd +, Cmd - and Cmd 0 or the View menu zoom from 6 to 72 points without touching your settings file, and the window keeps its size. *macOS.* <sub>[proof](../../baud-core/tests/zoomcycle.rs)</sub>
- **Colour emoji** — Apple Color Emoji is drawn crisply at every size, and Noto Color Emoji style fonts work too. <sub>[proof](../../baud-render/tests/apple_strikes.rs)</sub>
- **Layered colour fonts** — Layered colour fonts are drawn in their flat-colour form. **Partly:** gradient glyphs are not drawn. <sub>[proof](../../baud-font/src/colr.rs)</sub>
- **Emoji stay whole** — Families, flags, skin tones and other joined emoji each stay one character, taking the columns of one. <sub>[proof](../../baud-core/tests/marks.rs)</sub>
- **Choose how wide an emoji is** — An emoji with the emoji-style selector takes two cells, as tmux, fish and helix count it, or one if you choose. <sub>[proof](../../baud-core/tests/vs16_width.rs)</sub>
- **Keycap emoji** — Keycap emoji such as #️⃣ and 1️⃣ show. **Partly:** they stay one cell wide until their width is decided. <sub>[proof](../../baud-domain/src/vs16.rs)</sub>
- **Symbols as text or as emoji** — Choose whether symbols such as a lightning bolt draw as text or in colour; text that asks for one always gets it, and a change redraws at once. <sub>[proof](../../baud-render/tests/emoji_presentation.rs)</sub>
- **No colour font, still a symbol** — A request for a colour emoji with no colour font installed shows the plain symbol, not a box. <sub>[proof](../../baud-render/tests/emoji_presentation.rs)</sub>
- **Columns that line up** — Chinese, Japanese and Korean take two columns and marks take none, with widths from Unicode's own data, so tables, prompts and editors stay aligned. <sub>[proof](../../baud-core/tests/encoding/width.rs)</sub>
- **Ambiguous-width characters, your way** — Characters whose width differs between Western and East Asian fonts take one column or two, to match vim's ambiwidth, and programs can ask which. <sub>[proof](../../baud-core/tests/encoding/width.rs)</sub>
- **Box drawing under wide ambiguous characters** — With ambiguous characters set to two columns, box-drawing lines and blocks still draw. **Partly:** each is drawn one cell wide, in the first of its two cells. <sub>[reference](../vt-coverage.md#encoding-utf-8-or-iso-8859-1-and-8-bit-c1-controls-only-under-the-second)</sub>
- **No half characters left behind** — Overwriting half of a wide character clears the other half. <sub>[proof](../../baud-core/tests/wide_halves.rs)</sub>
- **Characters made of pieces stay together** — Accented letters, Indic conjuncts and emoji sequences stay one character: all 766 of Unicode's own test cases pass. <sub>[proof](../../baud-core/tests/encoding/graphemes.rs)</sub>
- **A thousand accented clusters at once** — About 1,024 different accented clusters fit on screen and in history at once, and those no longer shown are recycled. **Partly:** past that limit, extra accents are dropped. <sub>[proof](../../baud-core/tests/cluster_sweep.rs)</sub>
- **Broken text can't eat good text** — Broken UTF-8 shows one replacement mark per bad piece and never swallows the good character after it, disguised characters are refused, and a character split across two reads appears whole. <sub>[proof](../../baud-core/tests/text.rs)</sub>
- **Latin-1 for older systems** — Programs can switch to Latin-1 and back, with the 8-bit controls old software sends, a Latin-1 keyboard and an eight-bit Meta key. <sub>[proof](../../baud-core/tests/encoding/latin1.rs)</sub>
- **Line drawing from old programs** — Lines and corners from older curses programs show as lines. **Partly:** one position of the 32 still shows an underscore. <sub>[proof](../../baud-core/tests/vt100/charsets.rs) · [reference](../vt-coverage.md#esc-sequences)</sub>
- **National character sets** — Fourteen national character sets of old DEC terminals, DEC Supplemental and JIS Katakana show the right characters. <sub>[proof](../../baud-core/tests/vt100/charsets.rs) · [reference](../vt-coverage.md#esc-sequences)</sub>
- **Other old character sets** — JIS-Roman, DEC Technical and the DEC Hebrew, Greek, Turkish and Cyrillic sets are recognised. **Partly:** most of their characters still show as plain ASCII. <sub>[reference](../vt-coverage.md#esc-sequences)</sub>
- **Boxes without gaps** — Box-drawing lines, blocks and shades join with no hairline gap at every size, whatever the font, and braille dots, as in btop's graphs, are sharp. <sub>[proof](../../baud-render/tests/boxes.rs)</sub>
- **Glyphs sit centred** — Each glyph is centred in its cell, and one narrower than its two cells is centred across both. <sub>[proof](../../baud-render/tests/centring.rs)</sub>
- **Smoothing to suit the colour** — Text is smoothed to suit the colour it is drawn in, so light on dark and dark on light both read cleanly. <sub>[proof](../../baud-render/tests/contrast.rs)</sub>
- **Nearly invisible text made readable** — Text too close in colour to its background is lifted to a readable contrast, keeping its hue, while colours a program chose on purpose and pictures drawn with text are left alone. Adjustable, or off. <sub>[proof](../../baud-render/tests/floor.rs)</sub>
- **Faint text on any theme** — Faint text fades toward the background on light and dark themes alike. <sub>[proof](../../baud-render/tests/faint.rs)</sub>
- **Every text style** — Bold, faint, italic, strikethrough, reversed and hidden text, with hidden text fully invisible on the see-through window. <sub>[proof](../../baud-render/tests/hidden_text.rs) · [reference](../vt-coverage.md#csi-operations)</sub>
- **Five underline styles** — Single, double, curly, dotted and dashed underlines, each in its own colour if a program asks. <sub>[proof](../../baud-render/tests/underline_styles.rs)</sub>
- **Blinking text** — Slow and fast blinking text, each at its own rate, held steady under Reduce Motion. *macOS.* <sub>[proof](../../baud-app/src/app.rs)</sub>
- **Rare text styles** — Overline, framed and encircled text, Fraktur, alternate fonts and ideogram marks are remembered and reported back to programs. **Partly:** they are not drawn yet. <sub>[proof](../../baud-core/tests/sgr/ideogram.rs)</sub>
- **Text art keeps its colours** — Colourful text art and photo previews drawn with characters keep every cell's colour, up to one distinct colour per cell of the screen. <sub>[proof](../../baud-core/tests/marks.rs)</sub>

## Keyboard, mouse and clipboard

Every key reaches the program, and copying and pasting stay safe.

- **Every key sends what programs expect** — Arrows, Home and End, Insert and Delete, Page Up and Down, F1 to F20 and the keypad, with Shift, Ctrl and Alt, in every mode programs switch between. <sub>[reference](../input-coverage.md#the-key-tables-are-extracted-not-transcribed)</sub>
- **Option works as Meta** — Alt or Option works as Meta out of the box. <sub>[reference](../vt-coverage.md#csi-operations)</sub>
- **Keys that look alike, told apart** — Programs that ask can tell apart keys that normally send the same thing, such as Ctrl-I and Tab, and bind combinations other terminals swallow. <sub>[modifyOtherKeys](../vt-coverage.md#tier-3--xterm-extensions-in-universal-use) · [reference](../vt-coverage.md#tier-3--xterm-extensions-in-universal-use)</sub>
- **Full keyboard reporting** — Every key and modifier unambiguous, repeats reported, alternate keys and the text a key types, for editors such as Neovim and helix. <sub>[kitty keyboard protocol](../vt-coverage.md#tier-3--xterm-extensions-in-universal-use) · [proof](../../baud-core/tests/kitty_encode.rs) · [reference](../vt-coverage.md#tier-3--xterm-extensions-in-universal-use)</sub>
- **Key releases** — Programs that ask are told when a key is let go. *Linux, headless so far.* **Partly:** only the Linux version reports releases; the macOS app does not yet report releases or modifier keys pressed alone. <sub>[proof](../../baud-present-wayland/tests/handshake.rs)</sub>
- **Held keys repeat** — A held key repeats, and a program can switch repeat off. <sub>[proof](../../baud-present-wayland/tests/handshake.rs) · [reference](../vt-coverage.md#csi-operations)</sub>
- **Cmd belongs to the menus** — Cmd shortcuts go to the menus and never type into the program. *macOS.* <sub>[proof](../../baud-app/src/app.rs)</sub>
- **Input methods and dead keys** — Text you compose with a macOS input method is drawn underlined at the cursor, at the width it will take, and Return, Delete and the arrows edit it instead of reaching the shell. *macOS.* <sub>[proof](../../baud-render/tests/composition.rs)</sub>
- **Backspace erases the whole letter** — Backspace after a Hebrew, accented or other multi-byte letter erases the whole letter. <sub>[proof](../../baud-app/src/pty.rs)</sub>
- **The mouse in programs** — Clicks, drags, scrolls and hovers reach programs such as vim, htop and tmux, with Shift, Ctrl and Option, in every report format including pixel-precise. *macOS.* <sub>[SGR mouse](../vt-coverage.md#tier-3--xterm-extensions-in-universal-use) · [proof](../../baud-core/tests/xterm/mouse.rs) · [reference](../vt-coverage.md#tier-3--xterm-extensions-in-universal-use)</sub>
- **Programs can highlight what you drag** — A program can have the region you drag over drawn as a highlight. *macOS.* <sub>[proof](../../baud-app/src/app.rs) · [reference](../vt-coverage.md#tier-3--xterm-extensions-in-universal-use)</sub>
- **The wheel scrolls pagers** — The wheel scrolls less, man and git log, and a long fling arrives whole, even through tmux. *macOS.* <sub>[proof](../../baud-app/src/wheel.rs) · [reference](../vt-coverage.md#tier-3--xterm-extensions-in-universal-use)</sub>
- **Programs know when you switch away** — Programs that ask are told when you switch to the window and away from it. <sub>[reference](../vt-coverage.md#tier-3--xterm-extensions-in-universal-use)</sub>
- **Shift selects inside mouse programs** — Hold Shift to select text inside a program that uses the mouse; the choice holds until you let go. *macOS.* <sub>[proof](../../baud-app/src/app.rs)</sub>
- **Copy and paste** — Cmd-C copies the selection and Cmd-V pastes. *macOS.* <sub>[proof](../../baud-render/tests/select.rs)</sub>
- **Pastes can't run commands** — A paste is marked as one for programs that ask, so a line break in it never runs a command and pasted text can't break out of the paste. *macOS.* <sub>[bracketed paste](../vt-coverage.md#tier-3--xterm-extensions-in-universal-use) · [proof](../../baud-app/src/app.rs) · [reference](../vt-coverage.md#tier-3--xterm-extensions-in-universal-use)</sub>
- **A long paste never freezes the window** — A long paste, or one into a stopped program, is fed as the program reads it, and keys typed afterwards arrive afterwards. *macOS.* <sub>[proof](../../baud-app/src/pty.rs)</sub>
- **Remote programs can copy to your clipboard** — Programs, even over ssh or inside tmux, can put text on your clipboard; you can turn it off, and a copy over the size limit is refused whole with a warning. *macOS.* <sub>[OSC 52](../vt-coverage.md#tier-3--xterm-extensions-in-universal-use) · [proof](../../baud-app/src/clip.rs) · [reference](../vt-coverage.md#tier-3--xterm-extensions-in-universal-use)</sub>
- **No program can read your clipboard** — Requests to read the clipboard get no answer, and no setting turns them on. <sub>[proof](../../baud-core/tests/xterm/osc.rs)</sub>
- **Links open safely** — Cmd-click opens web and mail links programs mark as links; a link to a file is shown in Finder, never launched, and any other kind is refused. *macOS.* <sub>[hyperlinks](../vt-coverage.md#tier-3--xterm-extensions-in-universal-use) · [proof](../../baud-app/src/link.rs) · [reference](../vt-coverage.md#tier-3--xterm-extensions-in-universal-use)</sub>
- **A link goes where it says** — A link never leads anywhere but the address the program wrote: when the link table is full, the text stays plain. <sub>[proof](../../baud-core/tests/xterm/hyperlinks.rs)</sub>

## Scrollback, selection and resizing

Your history stays where you left it, through output, selection and resizing.

- **Scroll back through everything** — The wheel and trackpad move through the lines that scrolled off the top, with trackpad movement adding up to whole lines. *macOS.* <sub>[proof](../../baud-app/src/wheel.rs)</sub>
- **Reading back stays put** — New output doesn't drag you down while you read back; typing, pasting or clearing the history takes you back to the live screen. *macOS.* <sub>[proof](../../baud-app/src/app.rs)</sub>
- **A scrollbar like macOS's** — It appears while you scroll and fades away, widens when you point at it, and can be dragged or clicked; with "Show scroll bars: Always" it gets its own strip beside the text. *macOS.* <sub>[proof](../../baud-app/src/reveal.rs)</sub>
- **Smooth scrolling** — Output slides up instead of jumping, never more than one row behind, so a fast program never makes it lag; it can be set to jump instead. *macOS.* **Partly:** it slides only on the default GPU path, and lines with accents or ligatures jump rather than slide. <sub>[proof](../../baud-render/src/scroll.rs) · [measured](../../MEASUREMENTS.md#smooth-scrolls-per-frame-cost-measured-under-the-floor)</sub>
- **Select anything, history included** — Drag to select, double-click for a word, triple-click for a line, Shift-click to extend and Option-drag for a rectangle, reaching back into the scrollback. *macOS.* <sub>[proof](../../baud-render/src/select.rs)</sub>
- **You never copy text that is gone** — A selection disappears when the text under it is redrawn, scrolled away, reset or switched. *macOS.* <sub>[proof](../../baud-app/src/app.rs)</sub>
- **Resizing rewraps** — Making the window narrower or wider rewraps long lines instead of cutting them, and keeps your history. <sub>[proof](../../baud-core/tests/reflow.rs)</sub>
- **One prompt after a resize** — Resizing at a prompt your shell marks leaves one prompt, not a stack of repeats. <sub>[proof](../../baud-core/tests/resize_recorded.rs) · [reference](../vt-coverage.md#tier-3--xterm-extensions-in-universal-use)</sub>
- **Resizing keeps your place** — Resizing while scrolled back keeps you on the line you were reading, and full-screen programs redraw into the new size as you drag. *macOS.* <sub>[proof](../../baud-app/src/app.rs)</sub>
- **Full-screen programs give your screen back** — vim, less and htop get a screen of their own, and your earlier output comes back exactly as it was when they exit. <sub>[proof](../../baud-core/tests/legacy_alt_screen.rs) · [reference](../vt-coverage.md#tier-3--xterm-extensions-in-universal-use)</sub>
- **Headers and status lines stay still** — Scroll regions and left and right margins keep a program's headers, status lines and side panes still, and lines leaving a region at the top go into your history. <sub>[proof](../../baud-core/tests/scrollback.rs) · [reference](../vt-coverage.md#csi-operations)</sub>
- **Clear means clear** — Clearing the scrollback empties the history and leaves the screen; a reset leaves nothing of the old text behind. <sub>[proof](../../baud-core/tests/scrollback.rs) · [reference](../vt-coverage.md#tier-3--xterm-extensions-in-universal-use)</sub>

## Window, colours and bell

A see-through window that follows your Mac's appearance.

- **Liquid Glass** — The window is macOS's glass, clear or frosted, with how see-through, how tinted and how shadowed it is set separately for light and dark. *macOS.* <sub>[proof](../../baud-app/src/config.rs)</sub>
- **Panels see-through, pictures solid** — Backgrounds programs paint, such as btop's panels, let the glass through, or stay solid if you prefer, while photos drawn with characters stay solid so they don't dissolve. *macOS.* <sub>[proof](../../baud-render/tests/pictures.rs)</sub>
- **See-through on Linux** — The background's see-through level and the panel and picture rules carry over to Linux, with the same shades as on macOS. *Linux, headless so far.* **Partly:** nobody has looked at the result on a real Linux screen yet. <sub>[proof](../../baud-app/src/linux.rs)</sub>
- **A margin around the text** — 8 points around the text by default, or none, with the first line below the window buttons. *macOS.* <sub>[proof](../../baud-app/src/width.rs)</sub>
- **Window titles** — A title a program sets is cleaned of hidden codes and set on the window, where the Window menu, Mission Control and accessibility tools show it. *macOS.* **Partly:** the glass window does not draw it in its title bar. <sub>[proof](../../baud-app/src/title.rs)</sub>
- **Light and dark, automatically** — baud follows System Settings and switches the moment you do, from the first frame, or you can pin one. *macOS.* <sub>[proof](../../baud-app/src/app.rs)</sub>
- **Programs follow the appearance too** — When the appearance flips, colours a program changed are reset, and programs that asked are told, so their colours can follow. *macOS.* <sub>[proof](../../baud-app/src/app.rs) · [reference](../vt-coverage.md#tier-3--xterm-extensions-in-universal-use)</sub>
- **Light and dark on Linux** — The Linux version uses dark colours, or light if you choose. *Linux, headless so far.* **Partly:** the desktop's own light or dark setting is not read yet. <sub>[proof](../../baud-app/src/linux.rs)</sub>
- **Colours built in** — xterm's sixteen colours on dark, and Modus Operandi's on light. <sub>[proof](../../baud-app/src/host.rs)</sub>
- **Your own themes** — A base16, base24 or ghostty theme for each of light and dark; a broken theme, or a light one used for dark, is reported and baud's colours are kept. <sub>[proof](../../baud-app/src/palette.rs)</sub>
- **Programs can set and ask colours** — Programs can set and reset the palette and default colours, ask for them, and ask whether your theme is dark or light; reverse video swaps every colour. <sub>[proof](../../baud-core/tests/palette_query.rs) · [reference](../vt-coverage.md#tier-3--xterm-extensions-in-universal-use)</sub>
- **Millions of colours** — 16, 256 and 24-bit colour, in both spellings programs use. <sub>[proof](../../baud-core/tests/sgr/subparams.rs) · [reference](../vt-coverage.md#csi-operations)</sub>
- **Reduce Motion respected** — With macOS's Reduce Motion on there is no default cursor trail, a plain blink, still blinking text and no flash for the bell, and switching it takes effect within a frame. *macOS.* <sub>[proof](../../baud-app/src/config.rs)</sub>
- **A fade for full-screen programs** — An optional fade when a full-screen program opens or exits, off by default. *macOS.* <sub>[proof](../../baud-app/src/config.rs)</sub>
- **The bell, your way** — The Mac's alert sound, a short flash or nothing; a burst rings once, and a bell in a background window never makes a sound. *macOS.* <sub>[proof](../../baud-app/src/bell.rs)</sub>
- **A bell can ask for attention** — A program can ask for the Dock icon to bounce, or for the window to come forward without taking the keyboard. *macOS.* <sub>[proof](../../baud-core/tests/xterm/bell_modes.rs) · [reference](../vt-coverage.md#tier-3--xterm-extensions-in-universal-use)</sub>
- **The bell on Linux** — A bell on Linux is counted. *Linux, headless so far.* **Partly:** it rings nothing yet. <sub>[reference](../vt-coverage.md#c0-controls)</sub>

## The cursor

Your shape and blink, and an optional trail when it jumps.

- **Your shape** — Block, underline or bar, which programs can still change, as vim does in insert mode, and which a reset returns to. <sub>[proof](../../baud-app/src/config.rs) · [reference](../vt-coverage.md#tier-2--vt220--vt420)</sub>
- **Programs can hide and recolour it** — Programs can hide, show and recolour the cursor, and every shape takes the colour. <sub>[proof](../../baud-core/tests/xterm/osc.rs) · [reference](../vt-coverage.md#csi-operations)</sub>
- **Your blink** — The cursor follows macOS's "prefer non-blinking cursor" and its caret rate, or your own interval; it blinks only while the window is in front, shows at once when you type, and can stop after a quiet spell. *macOS.* <sub>[proof](../../baud-app/src/config.rs)</sub>
- **vim can't stop your blink** — vim's start-up and exit codes no longer stop a cursor you set to blink. <sub>[proof](../../baud-core/tests/cursor_blink_rule.rs) · [reference](../vt-coverage.md#csi-operations)</sub>
- **A blink that animates** — While blinked off, the cursor can glow like plasma, fade, burn into embers, be lifted away by a tiny flying saucer, or simply switch off. *macOS.* <sub>[proof](../../baud-render/src/caret.rs)</sub>
- **Trails** — When the cursor jumps it can leave a short trail of plasma, fire, lightning, ice, a flying saucer and more; typing never leaves one, the fiery ones never strobe, and a trail follows helix even when helix hides its cursor. Each is shown in [the cursor gallery](../cursor.md). *macOS.* <sub>[proof](../../baud-app/src/trail.rs)</sub>
- **Trails on the CPU path** — When the Mac's GPU can't be used, the CPU path still draws a trail, in the same shades as the GPU. *macOS.* **Partly:** every trail style is drawn as the plain stretching one there. <sub>[proof](../../baud-app/src/app.rs)</sub>

## Pictures

Image viewers and file-manager previews show real pictures.

- **Inline images** — Images from img2sixel, chafa, yazi and gnuplot show inline, partly transparent where they ask, at any length, decoded as they arrive. <sub>[Sixel](../vt-coverage.md#tier-3--xterm-extensions-in-universal-use) · [proof](../../baud-core/tests/sixel.rs) · [reference](../vt-coverage.md#tier-3--xterm-extensions-in-universal-use)</sub>
- **Solid image backgrounds** — A Sixel image is drawn whatever background it asks for. **Partly:** an image that asks for a solid background is drawn without it. <sub>[proof](../../baud-core/tests/sixel.rs)</sub>
- **Image scrolling and colour options** — Images that scroll or stay put, the cursor left beside the image, private colours per image, and a colour query. <sub>[proof](../../baud-core/tests/sixel_modes.rs) · [reference](../vt-coverage.md#tier-3--xterm-extensions-in-universal-use)</sub>
- **Images announced only when you say so** — baud tells programs it draws Sixel only once you turn on `advertise_sixel`, so file managers keep their text previews until then. <sub>[proof](../../baud-app/src/config.rs)</sub>
- **Pictures from image tools** — Raw, compressed and PNG pictures from kitten icat, yazi and timg, sent in pieces of any size and decoded quickly. <sub>[kitty graphics protocol](../vt-coverage.md#tier-3--xterm-extensions-in-universal-use) · [proof](../../baud-core/tests/kitty_graphics.rs) · [reference](../vt-coverage.md#tier-3--xterm-extensions-in-universal-use)</sub>
- **Pictures placed, cropped and layered** — Pictures are placed, cropped, scaled and layered under or over text, behind coloured text when asked and through the tinted glass, and scroll with it. **Partly:** scaled pictures use blocky nearest-pixel sampling for now. <sub>[proof](../../baud-render/tests/kitty_draw.rs) · [reference](../vt-coverage.md#tier-3--xterm-extensions-in-universal-use)</sub>
- **Pictures through tmux** — Pictures sent as placeholder characters work inside tmux and editors, and stay put while scrolling. <sub>[proof](../../baud-core/tests/kitty_placeholder.rs) · [reference](../vt-coverage.md#tier-3--xterm-extensions-in-universal-use)</sub>
- **Pictures go when asked** — Programs can delete pictures; typing over them does not erase them, and clearing the screen or a reset does. <sub>[proof](../../baud-core/tests/kitty_graphics.rs) · [reference](../vt-coverage.md#tier-3--xterm-extensions-in-universal-use)</sub>
- **Picture memory under control** — Pictures use up to 95 MiB by default, adjustable, and nothing until one arrives; the oldest off-screen ones make room, one too big is refused whole with a warning, and a deleted one gives its memory back. <sub>[proof](../../baud-app/src/pages.rs)</sub>
- **Pictures from files, safely** — Pictures can be read from a file, a temporary file or shared memory, and a program can't use this to read system files or to probe yours. *macOS.* **Partly:** a relative path is accepted for now, and the Linux side has not run. <sub>[proof](../../baud-app/src/kitty_files.rs) · [reference](../vt-coverage.md#tier-3--xterm-extensions-in-universal-use)</sub>
- **Pictures stay with their line** — Pictures stay with their line through scrolling, resizing and a shell redrawing its prompt. <sub>[proof](../../baud-core/tests/image_anchor.rs)</sub>
- **Image tools know the cell size** — Image tools can ask the cell and window size in pixels, and are told the new size after every resize or zoom. <sub>[proof](../../baud-core/tests/inband.rs) · [reference](../vt-coverage.md#tier-3--xterm-extensions-in-universal-use)</sub>

## Platforms and settings

Where baud runs, and the one settings file.

- **macOS app** — Needs macOS 26 or later, draws on the GPU with Metal and falls back to the CPU; on an older Mac it says what it needs and quits instead of crashing. *macOS.* <sub>[proof](../../baud-app/tests/macos_floor.rs)</sub>
- **Linux** — The same terminal core on Wayland, drawn on the CPU, with keyboard input and window titles. *Linux, headless so far.* **Partly:** it has run only on a headless compositor: no mouse, paste, clipboard or bell yet, font sizes are in pixels, and GPU drawing has not started. <sub>[proof](../../baud-present-wayland/tests/handshake.rs)</sub>
- **Your shell, set up right** — baud starts your login shell, in your home folder when opened from the Dock and with your language setting, or one command with `BAUD_CMD`. <sub>[proof](../../baud-app/src/pty.rs)</sub>
- **A clean environment** — Programs see baud with full colour, and nothing of the terminal you started it from. <sub>[proof](../../baud-app/src/pty.rs)</sub>
- **One optional settings file** — Keep only what you change in one file; baud never reads a stray file from the folder it started in. <sub>[proof](../../baud-app/src/config.rs)</sub>
- **A typo never stops baud starting** — An unknown setting is skipped with a warning naming it, a bad value falls back and baud says so, and settings from older versions are explained rather than dropped. <sub>[proof](../../baud-app/src/config.rs)</sub>
- **An example file that does what it says** — The example settings file is checked by a test to do what its comments say. <sub>[proof](../../baud-app/src/config.rs)</sub>
- **Quiet unless asked** — baud is quiet by default, and `BAUD_LOG` turns on its messages. <sub>[proof](../../baud-app/src/log.rs)</sub>

## Embedding the core

The part of baud that understands a program's output, as a library.

- **A library any program can use** — The core that interprets a terminal's output is a library with no operating-system dependency, and the guide's example is compiled with every build. <sub>[proof](../../baud-core/tests/embedding.rs)</sub>

## Protocols

The protocols and extensions baud speaks, for anyone checking a particular
one. Every control sequence has its own row in the
[coverage table](../vt-coverage.md), and every key in the
[input table](../input-coverage.md).

| protocol | from | reference |
|---|---|---|
| synchronised output | the terminal working group | [row](../vt-coverage.md#tier-3--xterm-extensions-in-universal-use) |
| modifyOtherKeys | xterm | [row](../vt-coverage.md#tier-3--xterm-extensions-in-universal-use) |
| kitty keyboard protocol | kitty | [row](../vt-coverage.md#tier-3--xterm-extensions-in-universal-use) |
| SGR mouse | xterm | [row](../vt-coverage.md#tier-3--xterm-extensions-in-universal-use) |
| bracketed paste | xterm | [row](../vt-coverage.md#tier-3--xterm-extensions-in-universal-use) |
| OSC 52 | xterm | [row](../vt-coverage.md#tier-3--xterm-extensions-in-universal-use) |
| hyperlinks | VTE and iTerm2 | [row](../vt-coverage.md#tier-3--xterm-extensions-in-universal-use) |
| Sixel | DEC | [row](../vt-coverage.md#tier-3--xterm-extensions-in-universal-use) |
| kitty graphics protocol | kitty | [row](../vt-coverage.md#tier-3--xterm-extensions-in-universal-use) |

## Where baud is behind

Where other terminals do more today, and what baud does not do yet, with a
link where the [roadmap](../roadmap.md) plans it.

- **A fixed list of fallback fonts** — most terminals search every installed font for a character the others lack; baud searches a fixed list.
- **Bold and italic in your own fonts** — other terminals find your font's bold and italic faces by family name. [Roadmap](../roadmap.md#text-fonts-and-emoji)
- **Layered colour fonts** — kitty and wezterm draw the gradient form of layered colour fonts.
- **Rare text styles** — ghostty and wezterm draw overlined text.
- **Key releases** — the macOS app does not yet tell programs when a key is let go, or when a modifier key is pressed alone. [Roadmap](../roadmap.md#keyboard-mouse-and-clipboard)
- **Links open safely** — plain web addresses printed as text are not clickable yet, and a link's address is not shown before it opens. [Roadmap](../roadmap.md#keyboard-mouse-and-clipboard)
- **Window titles** — every other terminal draws the window title in its title bar. [Roadmap](../roadmap.md#window-colours-and-bell)
- **Solid image backgrounds** — foot and wezterm fill the solid background a Sixel image asks for. [Roadmap](../roadmap.md#pictures)
- **Linux** — the Linux version has run only headless, with no mouse, paste, clipboard or GPU drawing yet. [Roadmap](../roadmap.md#platforms-and-settings)
- **Search** — Searching the scrollback is not built yet. [Roadmap](../roadmap.md#scrollback-selection-and-resizing)
- **Right-to-left text** — Hebrew and Arabic are shown left to right, in the order the program sent them; reordering is not built yet. [Roadmap](../roadmap.md#text-fonts-and-emoji)
- **Copy on select** — Selecting text does not copy it yet; Cmd-C does. [Roadmap](../roadmap.md#keyboard-mouse-and-clipboard)
- **More than one window** — baud opens one window; Cmd-N for another is not built yet. [Roadmap](../roadmap.md#window-colours-and-bell)

## How this page stays true

This page is generated from `docs/data/features.toml` by
`scripts/gen-features.py`, which fails the build when a feature's proof is gone,
when a feature marked built names none, when a done item on the
[roadmap](../roadmap.md) is missing from this list, or when this page has been
edited by hand. `feature_list_matches_its_data_and_its_proofs` runs it with
every test suite.
