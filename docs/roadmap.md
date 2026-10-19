# Roadmap

What is being built, what is planned, and what has shipped, by area. Ticked
items have shipped, with the day they landed; items marked *In progress* are
being built now; the rest are planned. Nothing here is a promised date.

[Programs just work](#programs-just-work) · [Speed](#speed) · [Text, fonts and emoji](#text-fonts-and-emoji) · [Keyboard, mouse and clipboard](#keyboard-mouse-and-clipboard) · [Scrollback, selection and resizing](#scrollback-selection-and-resizing) · [Window, colours and bell](#window-colours-and-bell) · [The cursor](#the-cursor) · [Pictures](#pictures) · [Platforms and settings](#platforms-and-settings) · [Embedding the core](#embedding-the-core)

## Programs just work

- [ ] **The remaining xterm settings** — Each of the xterm settings baud does not act on yet is built or has its reason written down. *In progress.*
- [ ] **A hidden cursor moves with scrolling output** — A cursor a program hid and placed stays on its text while output scrolls.
- [ ] **A saved cursor follows its text through a resize** — A position a program saved stays on the same text when the window is resized.
- [ ] **Answer programs that ask what the terminal can do** — neovim and tmux ask the terminal which features it has; baud will answer them directly.
- [ ] **Choose a terminal generation** — Programs can ask baud to behave as a VT100, VT220 or VT420, and every answer it gives follows that choice.
- [ ] **Folder, notification and progress messages from programs** — Programs can tell baud the folder they are in, post a desktop notification or show a progress bar.
- [ ] **More answers to programs' status questions** — More of the questions programs ask about the cursor and the terminal's settings get a true answer.
- [ ] **More colour controls for programs** — Programs can set the selection and pointer colours, name colours, and save and restore the palette.
- [ ] **More from shell prompt marks** — Clicking to move the cursor within a command line, and the exit status of each command.
- [ ] **More messages from programs, and colour names** — More of the messages programs send are understood, and colours can be given by name.
- [ ] **More scrolling and tab-stop controls** — Sideways scrolling and the remaining tab-stop clears.
- [ ] **Protected areas the ISO way** — Programs can protect text from erasing with the ISO controls as well as DEC's.
- [ ] **Rectangle editing** — Programs can copy, fill and erase rectangles of the screen, as on a VT420.
- [ ] **Save and restore colours and styles** — Programs can push the current colours and styles and pop them back.
- [ ] **Save and restore the whole terminal state** — Programs that save the terminal's state and put it back later can do so.
- [ ] **The rest of the old character-set switching** — Single and locking shifts and the second character table that some older programs use.
- [ ] **Window position reports** — Programs can ask where the window is on screen and get a true answer, beside the size reports already answered.
- [x] **The old terminal-ID request answers** — Programs that identify the terminal with ESC Z, the VT100's original request, now get the same answer as the modern one; VT52 mode keeps its own reply. (2026-10-16)
- [x] **Cursor up and down stop at the scroll margins** — Inside a scrolling region, cursor up and down now stop at its edge, as DEC's text, xterm and ghostty do. (2026-10-15)
- [x] **New lines and cursor moves stay inside side margins** — With left and right margins set, as in a vertical split, a new line starts at the left margin and cursor moves stop at the margins, as DEC's VT420 does. (2026-10-15)
- [x] **Side margins are finished, VT420-style** — Text written past the right margin's corner goes where a VT420 and xterm put it, completing side margins: rectangle scrolling, line starts, cursor moves and the corner all follow DEC's text. The reference terminal now agrees on every case tested, at no speed cost. (2026-10-15)
- [x] **Programs can now change the terminal's width and ask for it** — The 80/132 column switch resizes the window when the program allows it, and the width request sequence is answered. (2026-10-14)
- [x] **Rendition colours no longer change on text already on screen** — When a program uses very many distinct colour and style combinations, the terminal frees unused ones before reusing any, so existing text keeps its colour. Before this, a cell could pick up another cell's colour. (2026-10-14)
- [x] **Several reset and cursor-save sequences now behave as the standard says** — A soft reset leaves the cursor where it is. DECALN resets margins and origin mode. CSI s and CSI u with a parameter are no longer taken as save and restore. HT and BS respect side margins. (2026-10-14)
- [x] **Side margins and scroll regions follow DEC's rules** — Origin mode counts from the margins. A zero bottom margin means the last line. Scrolls under side margins move only the rectangle. (2026-10-13)
- [x] **Three more character sets draw correctly** — DEC Supplemental, JIS Katakana and JIS Roman are translated, and unused positions draw DEC's error mark. (2026-10-12)

## Speed

- [ ] **Faster emoji sequences** — Emoji families, flags and other joined emoji are taken in faster.
- [ ] **Faster zoom** — Zooming keeps the glyphs it has already drawn instead of drawing every one again.
- [ ] **Less GPU work per frame** — Fewer drawing passes for the text grid.
- [ ] **Less work when only the cursor moves** — A frame where only the cursor or its trail changed is drawn with less work.
- [ ] **Smoother window drags** — A drag step that cannot be shown yet is not drawn at all.

## Text, fonts and emoji

- [ ] **Hebrew in every style** — Italic and bold italic Hebrew with its points, to match the regular and bold already built in. *In progress.*
- [ ] **Hebrew points in fonts without positioning data** — Points on some Hebrew letters can land one cell to the right in fonts that do not position them. *In progress.*
- [ ] **Choose fonts by name** — Name a font family and size instead of pointing at a font file. File paths keep working.
- [ ] **Real bold and italic for your own fonts** — When you choose your own font, its bold and italic faces are used instead of the regular one.
- [ ] **Right-to-left text** — Hebrew and Arabic lines laid out right to left, with the cursor and selection following.
- [x] **Double-height and double-width lines** — Banner lines drawn twice as wide, or twice as wide and twice as tall over two lines, as vttest and DEC-era programs draw them. (2026-10-19)
- [x] **Generated text tables are checked against their generators** — A tool that rebuilds the grapheme-cluster table had been writing one wrong byte since August; the shipped table was right. The tool is fixed and the test suite now fails if either generated table drifts from its source. (2026-10-16)
- [x] **The character-grouping rules can no longer disagree with themselves** — The machine that decides where one on-screen character ends and the next begins kept its state in two forms that could drift apart. They are now two separate types, so neither can be built out of step with the other; the text path runs slightly fewer instructions as a result and draws exactly the same cells. (2026-10-16)
- [x] **A setting makes East Asian ambiguous characters two columns wide** — Set [text] ambiguous_width = "wide" and characters like ±, α and ① take two columns, as CJK fonts draw them. Nerd Font icons stay one column so prompts stay aligned. Programs can ask for the setting with ?1021. (2026-10-15)
- [x] **Brackets take the script of the text around them** — A bracket pair inside Hebrew, Arabic or Greek text is shaped with that text's script, as Unicode's script rules ask, so brackets no longer split a word's shaping. (2026-10-15)
- [x] **Hebrew ligatures now form in Apple fonts such as Lucida Grande** — Apple-format font tables are now read in the order the font asks for in right-to-left text. The eight Hebrew fonts on macOS now match HarfBuzz on every test cluster. (2026-10-15)
- [x] **Hebrew vowel points now sit on their letters in fonts without positioning data** — Each point is placed above, below or beside its letter the way HarfBuzz places it. On one test font the result now matches HarfBuzz on every cluster, up from 19 of 70. (2026-10-15)
- [x] **Text being composed follows the wide setting** — Under the East Asian wide setting, text you are still composing with an input method takes the width the terminal will give it. (2026-10-15)
- [x] **Cell clusters survive long documents** — The store for accented and joined characters now holds 1024 clusters and reuses slots that are no longer on screen. A pointed Hebrew Genesis passage that lost 45 marks now loses none. (2026-10-14)
- [x] **Glyphs too complex to draw show a placeholder instead of nothing** — A glyph over the point budget now falls back to the next font that has it, then to the font's missing-glyph box, as Unicode requires. (2026-10-14)
- [x] **Each script is shaped with its own font rules** — Hebrew, Arabic, Indic and other scripts now use their own OpenType tables, falling back to the default set as the standard says. One replacement character is drawn per bad byte group. (2026-10-13)
- [x] **More variable and CFF2 fonts load and draw correctly** — CFF2 outlines are read with their variation blends, and a damaged CFF2 face is refused at load. Several glyph placement fixes matched HarfBuzz on edge cases. (2026-10-12)

## Keyboard, mouse and clipboard

- [ ] **Key releases and modifier keys on macOS** — Programs that ask will learn when a key is let go and when Shift, Control, Option or Command is pressed on its own. The Linux version already sends them. *In progress.*
- [ ] **Copy on select** — Selecting text copies it, on by default with a setting to turn it off, and the selection colour can be chosen.
- [ ] **Keyboard lock and local echo** — Two old modes: a program can lock the keyboard, or have typed text shown without waiting for it.
- [ ] **Keypad and Backspace key modes** — Programs can choose what the keypad and the Backspace key send, as on DEC terminals.
- [ ] **Plain web addresses become clickable** — Cmd-click opens a web address a program printed as plain text, not only links the program marked.
- [ ] **Programs choose whether Shift selects** — A program that uses the mouse can say whether Shift-drag still selects text.
- [ ] **See where a link goes before opening it** — Holding Cmd marks the linked text and shows its address.
- [ ] **The remaining key-modifier modes** — The last of xterm's settings for how modified keys are sent.
- [x] **Keypad arrows reach programs that ask for every key, on Linux** — With NumLock off, the keypad's arrows, Home, End, Page Up/Down, Insert, Delete and the centre key are sent as kitty's own keypad keys, or as the keys they stand for in older modes. A Mac keyboard has no NumLock, so on macOS the keypad still types digits. (2026-10-16)
- [x] **Programs can copy to the macOS clipboard** — Programs can set your clipboard. Reads are still refused. It can be turned off in the config. Input method composition text is drawn at the cursor. (2026-10-11)
- [x] **Every mouse reporting mode works, and a fast wheel scroll reaches the program whole** — The X10, UTF-8, urxvt and SGR pixel modes work. Slow wheel movement adds up, and a slow program no longer freezes the window. Whether scrolling works under a multiplexer is still waiting on your check. (2026-10-09)

## Scrollback, selection and resizing

- [ ] **A selection survives a change of height** — Making the window taller or shorter keeps what you selected.
- [ ] **Search the scrollback** — Find text in the history.
- [ ] **The scrollbar follows a settings change at once** — Switching between always-visible and overlay scrollbars in System Settings shows without waiting for the next scroll.
- [ ] **Widening the window brings back cut text** — A row cut off by narrowing the window comes back when it is widened again.
- [x] **Wrapped lines re-wrap correctly after a resize** — A line is marked as wrapped only when its text really ran past the edge, whatever way the output arrived. The fix also costs a little less work per line than the first version. (2026-10-15)
- [x] **The terminal remembers where you scrolled to** — Your place in the history is kept through output, resizes and clears, and returns to the live screen when the program redraws that line or you clear the history. (2026-10-14)
- [x] **Programs can clear the scrollback** — Programs can clear scrollback from either screen, and a window scrolled into history returns to the live screen. (2026-10-09)

## Window, colours and bell

- [ ] **Readable colours on the light and dark glass** — Check the built-in light and dark colours for contrast over the see-through window, and tune them where they fall short. *In progress.*
- [ ] **A launch splash** — baud's colour logo, gone in a burst of plasma by the time the shell is ready, with a setting to turn it off.
- [ ] **A prompt pinned to the bottom** — An optional mode that keeps the shell prompt at the bottom of the window.
- [ ] **A slide when full-screen programs open and close** — The screen slides between your shell and a full-screen program, as an option beside today's fade.
- [ ] **A window title with the folder and the running program** — When a program sets no title, the window shows the current folder and what is running in it.
- [ ] **Activity lights in the title bar** — Small modem-style lights for bytes coming in and going out, with a setting to turn them off.
- [ ] **Blinking text follows Dim Flashing Lights** — macOS's Dim Flashing Lights setting will calm blinking text too.
- [ ] **Cmd-N opens a new window** — Open another window, including after the last one closed.
- [x] **Reverse video swaps every cell's colours** — With the reverse-screen mode on, foreground and background swap on every cell, including cells with explicit colours, as the DEC standard says. (2026-10-14)
- [x] **The bell now rings on macOS** — A bell sounds when the window is focused. When it is not, programs can ask for the Dock icon to bounce or for the window to come forward. A config option picks sound, flash or none, and flash is off under Reduce Motion. (2026-10-14)

## The cursor

- [ ] **A hollow cursor when the window is in the background** — The cursor shows as an outline while another window has the keyboard.
- [x] **The cursor trail and see-through text match on CPU and GPU** — The cursor trail and text in a semi-transparent colour could be one shade apart between the CPU and GPU drawing paths; both now round the same way and the pixel comparison checks them. Drawing over the see-through window got about half a percent cheaper. (2026-10-16)
- [x] **Fire and UFO cursor blinks** — Two more blink animations: the cursor burns down into embers and relights, or a little beam lifts it away and sets it back. Set [cursor] blink_animation to "fire" or "ufo"; plasma stays the default. (2026-10-15)
- [x] **The cursor blinks evenly, with a plasma glow** — 333 ms on and 333 ms off, glowing like the plasma trail while it is off. [cursor] blink_animation picks fade or a plain blink, blink_interval_ms sets the speed, and Reduce Motion always uses the plain blink. (2026-10-15)

## Pictures

- [ ] **Animated pictures and pictures placed beside others** — Animated pictures, and pictures placed relative to other pictures, the last parts of the kitty picture protocol. *In progress.*
- [ ] **Solid backgrounds for Sixel images** — A Sixel image that asks for a solid background gets one, including under reverse video and on the see-through window.
- [x] **A picture that can't be placed says so** — When the terminal has no room for another picture placement, the program now gets an error it can act on instead of being told the picture is up. (2026-10-16)
- [x] **A refused picture file no longer pushes out older pictures** — When a PNG sent as a file can't be placed, older pictures waiting to be placed stay, exactly as when the file is missing, so the reply reveals nothing about your files. (2026-10-16)
- [x] **Background pictures show through tinted glass** — On the see-through window, a bottom-layer picture now shows dimmed through coloured cells, the way the desktop does, and a cell painted the same colour as the default background shows it like a plain cell. Both are your picks; frames without such a picture cost nothing extra. (2026-10-16)
- [x] **Background pictures sit behind coloured text** — A program can put a picture on the very bottom layer, so it shows only where text has the plain background and stays hidden behind coloured cells, the selection and the cursor, as the kitty protocol describes. Drawn the same on every drawing path. (2026-10-16)
- [x] **Deleting a picture gives its memory back** — After a full-window picture is shown and deleted, the terminal falls back to within 0.3 MB of its idle memory, where 19.5 MB (a PNG) or 8.4 MB (raw pixels) used to stay. The next picture takes about 0.3 ms longer per 8 MB to write. Measured on macOS; the Linux half is built but not yet run. (2026-10-16)
- [x] **Even more picture memory goes back** — A Sixel picture that is refused, moved or cut off now gives its memory back too: a refused full-window picture leaves 0.25 MB above idle, where it left 8.6 MB. Memory shared by two neighbouring pictures goes back once both are gone. (2026-10-16)
- [x] **Full-window pictures fit, at no cost when idle** — Image memory is now only used once a picture is written into it, so the default holds three 4K screens while an idle terminal stays at about 12 MB instead of 111. Queries from programs testing for picture support are always answered and never remove a stored picture. (2026-10-16)
- [x] **PNG decoding 14% faster** — Compressed image data now decodes through lookup tables built at compile time; a full-window picture takes 32 ms instead of 37, and 4K pictures 233 ms instead of 284. Large pictures will decode in the background between frames, so the terminal never stalls on one. (2026-10-16)
- [x] **PNG decoding three times faster** — Checksums, unfiltering and copying now work on many bytes at once: a full-window picture decodes in 11 ms instead of 30, and a 4K picture in 150 ms instead of 232. (2026-10-16)
- [x] **Picture files can't be used to probe your disk** — A program asking the terminal to show a picture from a file gets the same single error whatever went wrong -- missing, unreadable, not a picture, or refused after reading -- and the terminal ends up in the same state either way, as the kitty protocol requires. Temporary picture files are deleted only inside the folder that was checked. (2026-10-16)
- [x] **Pictures are cropped, scaled and layered as asked** — Pictures now show just the part a program asks for, scaled into the cells it names without distortion, offset inside their first cell, and under the text when asked. They clip at a scroll region's edge as text does. The CPU and both GPU paths draw the same pixels. (2026-10-16)
- [x] **Pictures can be sent as files** — Programs on the same machine can hand baud a picture as a file path, a temporary file or shared memory instead of sending its bytes through the terminal. Only regular files are read, never anything under /proc, /sys or /dev, a file swapped mid-read is refused, temporary files are removed only from a temp folder, and every failure looks the same to the program. (2026-10-16)
- [x] **Pictures from image tools such as kitten icat and yazi** — Programs that show pictures the kitty way, such as kitten icat, can now send raw or PNG pictures in pieces, place, replace and delete them, and get answers to their queries. Large pictures decode between frames, so typing never stalls; on Linux too. When image memory fills, the oldest pictures not on screen make room. Cropping, scaling and layering are drawn in a later step. (2026-10-16)
- [x] **Pictures over the see-through window look the same on every drawing path** — Text, cursors and semi-transparent pictures drawn over the translucent window now get identical colours from the CPU and GPU paths (they differed by up to 47 per channel), following the W3C compositing rule. This matters most on Linux, which draws on the CPU. (2026-10-16)
- [x] **Pictures through tmux stay put while scrolling** — During a smooth scroll, the row sliding off the top keeps drawing its placeholder picture instead of blinking out, on every drawing path. Placeholder pictures are now complete: your picks settled how a missing picture and its position marks behave. (2026-10-16)
- [x] **Pictures work through tmux** — Programs can show a picture as special placeholder characters, which tmux and other multiplexers pass along as ordinary text, so the picture moves, scrolls and copies with the text around it. kitten icat --unicode-placeholder places pixel-exact, on every drawing path. Every picture size kitten allows is exact, where a full-screen picture used to lose most of its position marks. (2026-10-16)
- [x] **Raw-pixel pictures arrive ten times faster** — Picture data is now read in bulk instead of a byte at a time: a full-window raw picture takes under 5 ms instead of 46, and a full-window PNG 13 ms instead of 18, since its first data block is also checked once instead of twice. (2026-10-16)
- [x] **Images can be partly transparent, and decode as they arrive** — Pictures with soft edges and shadows now blend over your text instead of being drawn solid, while opaque pictures and Sixel images cost the same as before. PNG decoding is 12% cheaper and can resume between reads, ready for kitty's image protocol, which sends pictures in pieces. (2026-10-15)
- [x] **Interlaced PNG images now decode** — baud's PNG reader handles interlaced images, which the PNG standard requires of every reader. It matches ImageMagick on all 155 interlaced images on this Mac, and ordinary PNGs decode about 1% faster. (2026-10-15)
- [x] **Programs can control how Sixel pictures scroll, share colours and place the cursor** — The display, shared-colour and cursor-beside-image modes work as DEC and xterm define, and programs can ask how many colours and pixels are available. Colours a picture never defines now take the text colour, as the DEC standard says. (2026-10-15)
- [x] **Large Sixel images are decoded as they arrive** — Sixel previews, such as yazi photo and PDF previews, no longer hit a fixed body size limit. The limit is now the image's pixel size. Before this, a few were refused and showed blank, and a stopgap raised the cap to 3 MiB first. (2026-10-14)

## Platforms and settings

- [ ] **Mouse, wheel, selection and paste on Linux** — The Linux version takes keyboard input only so far; the mouse, the wheel, selecting text and pasting come next. *In progress.*
- [ ] **Check Linux on a real screen** — The Linux version has run only headless; next is looking at what it draws on a real display.
- [ ] **Clipboard and middle-click paste on Linux** — Copy and paste through the desktop's clipboard, the primary selection and middle-click paste.
- [ ] **Compose key, dead keys and input methods on Linux** — Typing accented letters and composing Chinese, Japanese or Korean text on Linux.
- [ ] **Cursor trails, blinking text, fades and the scrollbar on Linux** — The drawing code already does all four; the Linux window has to drive them.
- [ ] **GPU drawing on Linux** — Draw the Linux version on the GPU with Vulkan, as the macOS app draws with Metal.
- [ ] **Redraw only what changed on Linux** — Send the compositor only the rows that changed, instead of the whole window every frame.
- [ ] **Smoother frames and pointer shapes on Linux** — Frame timing from the compositor, the right pointer shape over text, and input methods.
- [ ] **Zoom, links and light and dark on Linux** — Ctrl + and Ctrl - to zoom, clickable links, and following the desktop's light or dark setting.
- [x] **Linux and macOS give the same shade over the see-through window** — Both systems now round colours through one shared function; before, about half of all colour and transparency combinations came out one step darker on Linux. (2026-10-16)
- [x] **A baud opened from the Dock starts your shell in your home folder** — It no longer reads a config.toml from whatever folder it started in. Relative font and theme paths are read beside the config file. (2026-10-14)
- [x] **baud requires macOS 26 and says so on older systems** — On macOS older than 26, baud prints a clear message and exits instead of crashing. (2026-10-13)

## How this page stays true

This page is generated from `docs/data/roadmap.toml`, the same planning list
the progress dashboard reads, by `scripts/gen-features.py`. Every item marked
shipped is claimed by a feature in [the feature list](guide/features.md) or
named there as not a feature, and the build fails when one is neither.
