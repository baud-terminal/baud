# Roadmap

What is being built, what is planned, and what has shipped, by area. Ticked
items have shipped, with the day they landed; items marked *In progress* are
being built now; the rest are planned. Nothing here is a promised date.

[Programs just work](#programs-just-work) · [Speed](#speed) · [Text, fonts and emoji](#text-fonts-and-emoji) · [Keyboard, mouse and clipboard](#keyboard-mouse-and-clipboard) · [Scrollback, selection and resizing](#scrollback-selection-and-resizing) · [Window, colours and bell](#window-colours-and-bell) · [The cursor](#the-cursor) · [Pictures](#pictures) · [Platforms and settings](#platforms-and-settings)

## Programs just work

- [ ] **More messages from programs, and colour names** — Colours can be given by name, such as red or lime green, and programs can name the window and its icon, set the pointer's shape and ask for the font. Programs written for iTerm2 can set the cursor's shape, erase the history, tell the title which folder and host they are in, copy text, recolour the screen, ask for attention, ask the cell size, highlight the row the cursor is on, mark lines to jump back to, post notifications, switch to Unicode 8's widths, keep user variables, show PNG, JPEG and GIF pictures inline, animated GIFs playing, and report their features, and show a badge; iTerm2's pictures in other formats and a few more are still to come. *In progress.*
- [x] **A hidden cursor moves with scrolling output** — A cursor a program hid and placed stays on its text while output scrolls. (2026-10-22)
- [x] **Answer programs that ask what the terminal can do** — Programs such as neovim and tmux can ask which bytes each key sends, how many colours there are and which terminal this is, and neovim run over ssh finds 24-bit colour from baud's answer. (2026-10-22)
- [x] **Choose a terminal generation** — Programs can ask for VT100, VT220, VT320 or VT420 behaviour and for 7- or 8-bit controls, and read back which they have; baud then does only what that level defines, keys included, and its identity reply stays VT102. (2026-10-22)
- [x] **More from shell prompt marks** — Clicking in the command line you are typing moves the shell's cursor there, on a Mac Command-Up and Command-Down jump between prompts, and a mark beside each prompt can show whether its command succeeded, failed or was cancelled. (2026-10-22)
- [x] **Programs set colours the kitty way** — Programs written for kitty can read and set the terminal's colours with kitty's own colour message, beside the xterm messages baud already answers. (2026-10-22)
- [x] **Save and restore the whole terminal state** — Programs can read back the cursor's place, the tab stops, the colour table and the whole terminal state in one report, and put any of them back later. (2026-10-22)
- [x] **The remaining xterm settings** — Programs can switch xterm's own settings, among them jumping to new output or to a keypress, reverse wrap, hiding the scrollbar, copying a selection to the clipboard and keeping a selection's text after a program writes over it. (2026-10-22)
- [x] **Folder, notification and progress messages from programs** — Programs can tell baud the folder they are in, post a desktop notification or show a progress bar. (2026-10-21)
- [x] **More answers to programs' status questions** — The questions a VT420 answers, about the cursor's page, the printer, the keyboard, the locator and stored macros, get a true answer. (2026-10-21)
- [x] **A saved cursor follows its text through a resize** — A position a program saved on the main screen stays on the same text when the window is resized, so restoring it lands where the program left off. (2026-10-20)
- [x] **More colour controls for programs** — Programs can set the selection's colours, give colours by name, colour bold, underlined and other styled text a colour of its own, and save the palette and put it back. (2026-10-20)
- [x] **Save and restore colours and styles** — Programs can push the colours and styles they are drawing in, or the whole palette, and pop them back, ten levels deep. (2026-10-20)
- [x] **More scrolling and tab-stop controls** — Programs can slide the screen's text left or right, clear every kind of tab stop, and set line tab stops for vertical tabs. (2026-10-19)
- [x] **Protected areas the ISO way** — Programs can fence off part of the screen with the ISO controls so that erasing leaves it alone, as well as protecting text DEC's way. (2026-10-19)
- [x] **The rest of the old character-set switching** — Single and locking shifts, and the upper half of the character table, so older programs can switch between their four character sets. (2026-10-19)
- [x] **Window position reports** — Programs can ask where the window is on screen and get a true answer, beside the size reports already answered. On Linux the desktop does not say, so baud gives no answer rather than a wrong one. (2026-10-19)
- [x] **Rectangle editing** — Programs can copy, fill, erase and change the styles of a rectangle of the screen, and read back its checksum, as on a VT420. (2026-10-18)
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

- [x] **Faster zoom** — Zooming back to a recent size keeps the glyphs already drawn at it instead of drawing every one again. (2026-10-24)
- [x] **Less GPU work per frame** — While none of the styles a program has used underlines, strikes through, frames or otherwise decorates text, the GPU skips the pass that draws those lines, and the pass for letters that reach above their row when no letter does: a frame of btop or of CJK text takes a fifth to a third of the GPU time it did. (2026-10-24)
- [x] **Smoother window drags** — While you drag a window's edge, baud no longer prepares a frame it cannot show yet: a step that comes while the last frame is still on its way to the screen waits; the next frame is drawn at the size the window has by then, and the last at the size you let go at. (2026-10-24)
- [x] **Faster emoji sequences** — Emoji families, flags and other joined emoji are taken in faster, and many different ones on the screen at once no longer lose pieces. (2026-10-23)
- [x] **Less work when only the cursor moves** — A frame where only the cursor, its blinking, its trail or the cursor guide changed redraws just the rows they cover, on both GPU paths and in the copy the CPU path makes to the screen. (2026-10-23)

## Text, fonts and emoji

- [ ] **Right-to-left text** — Hebrew and Arabic lines laid out right to left, with the cursor and selection following. *In progress.*
- [x] **Hebrew in every style** — Italic and bold italic Hebrew with its points, to match the regular and bold already built in. (2026-10-24)
- [x] **Hebrew points on the last few fonts** — Hebrew points sit on their letters with Courier New, Arial Unicode MS and Arial Hebrew too: two points under one letter side by side, points inside their letter's cell, and meteg and the cantillation marks drawn. (2026-10-23)
- [x] **Choose fonts by name** — Name a font family and size instead of pointing at a font file. File paths keep working. (2026-10-22)
- [x] **Real bold and italic for your own fonts** — When you choose your own font, its bold and italic faces are used instead of the regular one. (2026-10-22)
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
- [x] **Copy on select** — Selecting text copies it, on by default with a setting to turn it off, and the selection takes the colours you or your theme choose. (2026-10-22)
- [x] **See where a link goes before opening it** — Holding Cmd marks the linked text and shows its address. (2026-10-22)
- [x] **Keypad and Backspace key modes** — Programs can choose whether the keypad types digits or sends its own codes, whether Backspace sends DEL or BS and whether Delete sends DEL, as on DEC terminals, and ask which they have. (2026-10-20)
- [x] **Keyboard lock and local echo** — Two old modes: a program can lock the keyboard, or have typed text shown on the screen as well as sent. (2026-10-19)
- [x] **Programs choose whether Shift selects** — A program that uses the mouse can say whether Shift-drag still selects text, or whether Shift reaches the program for every button but the one that selects. (2026-10-19)
- [x] **The remaining key-modifier modes** — Programs can choose how the arrows, function keys, keypad and other keys carry Shift, Control and Alt, as every one of xterm's settings allows, and ask which is in force. (2026-10-19)
- [x] **Plain web addresses become clickable** — A web address a program prints as plain text opens on Cmd-click, as a marked link does, even when it wraps onto the next line. (2026-10-18)
- [x] **Keypad arrows reach programs that ask for every key, on Linux** — With NumLock off, the keypad's arrows, Home, End, Page Up/Down, Insert, Delete and the centre key are sent as kitty's own keypad keys, or as the keys they stand for in older modes. A Mac keyboard has no NumLock, so on macOS the keypad still types digits. (2026-10-16)
- [x] **Programs can copy to the macOS clipboard** — Programs can set your clipboard. Reads are still refused. It can be turned off in the config. Input method composition text is drawn at the cursor. (2026-10-11)
- [x] **Every mouse reporting mode works, and a fast wheel scroll reaches the program whole** — The X10, UTF-8, urxvt and SGR pixel modes work. Slow wheel movement adds up, and a slow program no longer freezes the window. Whether the wheel scrolls under a multiplexer such as herdr still needs a check on a real Mac. (2026-10-09)

## Scrollback, selection and resizing

- [x] **The scrollbar follows a settings change at once** — Switching between always-visible and overlay scrollbars in System Settings shows at once, without waiting for the next scroll. (2026-10-22)
- [x] **Search the scrollback** — Find text in the history and on the screen: every match marked, the newest selected, the window following as you step through them. (2026-10-21)
- [x] **A selection survives a change of height** — Making the window taller or shorter keeps what you selected on its lines; a change of width, or a full-screen program, still clears it. (2026-10-20)
- [x] **A taller window brings back cut text** — Rows a shorter window cut off below the cursor come back when the window grows again, if they held any text. (2026-10-20)
- [x] **Wrapped lines re-wrap correctly after a resize** — A line is marked as wrapped only when its text really ran past the edge, whatever way the output arrived. The fix also costs a little less work per line than the first version. (2026-10-15)
- [x] **The terminal remembers where you scrolled to** — Your place in the history is kept through output, resizes and clears, and returns to the live screen when the program redraws that line or you clear the history. (2026-10-14)
- [x] **Programs can clear the scrollback** — Programs can clear scrollback from either screen, and a window scrolled into history returns to the live screen. (2026-10-09)

## Window, colours and bell

- [ ] **A window title with the folder and the running program** — When a program sets no title, the window shows the current folder and what is running in it; on a Mac the title bar does not draw it yet. *In progress.*
- [ ] **A slide when full-screen programs open and close** — The screen slides between your shell and a full-screen program, as an option beside today's fade.
- [ ] **Cmd-N opens a new window** — Open another window, including after the last one closed.
- [x] **A launch splash** — baud's colour logo, gone in a burst of plasma by the time the shell is ready, with a setting to turn it off. On a Mac with Reduce Motion on, it fades instead. (2026-10-24)
- [x] **A prompt pinned to the bottom** — An optional mode that keeps the shell prompt at the bottom of the window, for a shell that marks its prompts. (2026-10-24)
- [x] **Activity lights in the title bar** — Small modem-style lights for bytes coming in and going out, with a setting to turn them off. On macOS; the Linux window has no title bar. (2026-10-24)
- [x] **Blinking text follows Dim Flashing Lights** — With macOS's Dim Flashing Lights on, blinking text keeps its rates but dims in its off phase instead of vanishing, and switching the setting shows at once. (2026-10-22)
- [x] **Readable colours on the light and dark glass** — Every built-in light and dark colour but the page's own reads at 4.5:1 on its page, and the see-through window is dimmed enough to keep text readable over a white or a black wallpaper. (2026-10-18)
- [x] **Reverse video swaps every cell's colours** — With the reverse-screen mode on, foreground and background swap on every cell, including cells with explicit colours, as the DEC standard says. (2026-10-14)
- [x] **The bell now rings on macOS** — A bell sounds when the window is focused. When it is not, programs can ask for the Dock icon to bounce or for the window to come forward. A config option picks sound, flash or none, and flash is off under Reduce Motion. (2026-10-14)

## The cursor

- [x] **A hollow cursor when the window is in the background** — With the setting on, the cursor shows as an outline while another window has the keyboard. (2026-10-22)
- [x] **The cursor trail and see-through text match on CPU and GPU** — The cursor trail and text in a semi-transparent colour could be one shade apart between the CPU and GPU drawing paths; both now round the same way and the pixel comparison checks them. Drawing over the see-through window got about half a percent cheaper. (2026-10-16)
- [x] **Fire and UFO cursor blinks** — Two more blink animations: the cursor burns down into embers and relights, or a little beam lifts it away and sets it back. Set [cursor] blink_animation to "fire" or "ufo"; plasma stays the default. (2026-10-15)
- [x] **The cursor blinks evenly, with a plasma glow** — 333 ms on and 333 ms off, glowing like the plasma trail while it is off. [cursor] blink_animation picks fade or a plain blink, blink_interval_ms sets the speed, and Reduce Motion always uses the plain blink. (2026-10-15)

## Pictures

- [x] **Animated pictures and pictures placed beside others** — Animated pictures, and pictures placed relative to other pictures, the last parts of the kitty picture protocol. (2026-10-23)
- [x] **Solid backgrounds for Sixel images** — A Sixel image that asks for a solid background gets one, including under reverse video and on the see-through window. (2026-10-22)
- [x] **A picture that can't be placed says so** — When the terminal has no room for another picture placement, the program now gets an error it can act on instead of being told the picture is up. (2026-10-16)
- [x] **A refused picture file no longer pushes out older pictures** — When a PNG sent as a file can't be placed, older pictures waiting to be placed stay, exactly as when the file is missing, so the reply reveals nothing about your files. (2026-10-16)
- [x] **Background pictures show through tinted glass** — On the see-through window, a bottom-layer picture now shows dimmed through coloured cells, the way the desktop does, and a cell painted the same colour as the default background shows it like a plain cell. Frames without such a picture cost nothing extra. (2026-10-16)
- [x] **Background pictures sit behind coloured text** — A program can put a picture on the very bottom layer, so it shows only where text has the plain background and stays hidden behind coloured cells, the selection and the cursor, as the kitty protocol describes. Drawn the same on every drawing path. (2026-10-16)
- [x] **Deleting a picture gives its memory back** — After a full-window picture is shown and deleted, the terminal falls back to within 0.3 MB of its idle memory, where 19.5 MB (a PNG) or 8.4 MB (raw pixels) used to stay. The next picture takes about 0.3 ms longer per 8 MB to write. Measured on macOS; the Linux half is built but not yet run. (2026-10-16)
- [x] **Even more picture memory goes back** — A Sixel picture that is refused, moved or cut off now gives its memory back too: a refused full-window picture leaves 0.25 MB above idle, where it left 8.6 MB. Memory shared by two neighbouring pictures goes back once both are gone. (2026-10-16)
- [x] **Full-window pictures fit, at no cost when idle** — Image memory is now only used once a picture is written into it, so the default holds three 4K screens while an idle terminal stays at about 12 MB instead of 111. Queries from programs testing for picture support are always answered and never remove a stored picture. (2026-10-16)
- [x] **PNG decoding 14% faster** — Compressed image data now decodes through lookup tables built at compile time; a full-window picture takes 32 ms instead of 37, and 4K pictures 233 ms instead of 284. Large pictures decode between frames, so the terminal never stalls on one. (2026-10-16)
- [x] **PNG decoding three times faster** — Checksums, unfiltering and copying now work on many bytes at once: a full-window picture decodes in 11 ms instead of 30, and a 4K picture in 150 ms instead of 232. (2026-10-16)
- [x] **Picture files can't be used to probe your disk** — A program asking the terminal to show a picture from a file gets the same single error whatever went wrong -- missing, unreadable, not a picture, or refused after reading -- and the terminal ends up in the same state either way, as the kitty protocol requires. Temporary picture files are deleted only inside the folder that was checked. (2026-10-16)
- [x] **Pictures are cropped, scaled and layered as asked** — Pictures now show just the part a program asks for, scaled into the cells it names without distortion, offset inside their first cell, and under the text when asked. They clip at a scroll region's edge as text does. The CPU and both GPU paths draw the same pixels. (2026-10-16)
- [x] **Pictures can be sent as files** — Programs on the same machine can hand baud a picture as a file path, a temporary file or shared memory instead of sending its bytes through the terminal. Only regular files are read, never anything under /proc, /sys or /dev, a file swapped mid-read is refused, temporary files are removed only from a temp folder, and every failure looks the same to the program. (2026-10-16)
- [x] **Pictures from image tools such as kitten icat and yazi** — Programs that show pictures the kitty way, such as kitten icat, can now send raw or PNG pictures in pieces, place, replace and delete them, and get answers to their queries. Large pictures decode between frames, so typing never stalls; on Linux too. When image memory fills, the oldest pictures not on screen make room. Cropping, scaling and layering followed the same day. (2026-10-16)
- [x] **Pictures over the see-through window look the same on every drawing path** — Text, cursors and semi-transparent pictures drawn over the translucent window now get identical colours from the CPU and GPU paths (they differed by up to 47 per channel), following the W3C compositing rule. This matters most on Linux, which draws on the CPU. (2026-10-16)
- [x] **Pictures through tmux stay put while scrolling** — During a smooth scroll, the row sliding off the top keeps drawing its placeholder picture instead of blinking out, on every drawing path. Placeholder pictures are now complete, down to how a missing picture and its position marks behave. (2026-10-16)
- [x] **Pictures work through tmux** — Programs can show a picture as special placeholder characters, which tmux and other multiplexers pass along as ordinary text, so the picture moves, scrolls and copies with the text around it. kitten icat --unicode-placeholder places pixel-exact, on every drawing path. Every picture size kitten allows is exact, where a full-screen picture used to lose most of its position marks. (2026-10-16)
- [x] **Raw-pixel pictures arrive ten times faster** — Picture data is now read in bulk instead of a byte at a time: a full-window raw picture takes under 5 ms instead of 46, and a full-window PNG 13 ms instead of 18, since its first data block is also checked once instead of twice. (2026-10-16)
- [x] **Images can be partly transparent, and decode as they arrive** — Pictures with soft edges and shadows now blend over your text instead of being drawn solid, while opaque pictures and Sixel images cost the same as before. PNG decoding is 12% cheaper and can resume between reads, ready for kitty's image protocol, which sends pictures in pieces. (2026-10-15)
- [x] **Interlaced PNG images now decode** — baud's PNG reader handles interlaced images, which the PNG standard requires of every reader. It matches ImageMagick on all 155 interlaced images on this Mac, and ordinary PNGs decode about 1% faster. (2026-10-15)
- [x] **Programs can control how Sixel pictures scroll, share colours and place the cursor** — The display, shared-colour and cursor-beside-image modes work as DEC and xterm define, and programs can ask how many colours and pixels are available. Colours a picture never defines now take the text colour, as the DEC standard says. (2026-10-15)
- [x] **Large Sixel images are decoded as they arrive** — Sixel previews, such as yazi photo and PDF previews, no longer hit a fixed body size limit. The limit is now the image's pixel size. Before this, a few were refused and showed blank, and a stopgap raised the cap to 3 MiB first. (2026-10-14)

## Platforms and settings

- [ ] **Check Linux on a real screen** — The Linux version has run only headless; next is looking at what it draws on a real display.
- [ ] **GPU drawing on Linux** — Draw the Linux version on the GPU with Vulkan, as the macOS app draws with Metal.
- [x] **Cursor trails, blinking text, fades and the scrollbar on Linux** — Blinking text, fades, the scrollbar and every cursor trail -- the fire, the plasma and the rest -- on Linux, with the same pixels as on a Mac. (2026-10-23)
- [x] **Saved settings show at once** — Save the settings file, or a theme or font file it names, and the open window follows a moment later: colours, fonts, the cursor, padding and the rest. Built for the Mac; the Linux driver has not been run with it. A save baud cannot read keeps the settings you had and says what was wrong on a row at the bottom of the window. config_reload = "off" turns it off. (2026-10-22)
- [x] **Clipboard and middle-click paste on Linux** — Copy and paste through the desktop's clipboard, the primary selection and middle-click paste. (2026-10-21)
- [x] **Compose key, dead keys and input methods on Linux** — Typing accented letters with the Compose key and dead keys, and composing Chinese, Japanese or Korean text through the desktop's input method. (2026-10-21)
- [x] **Mouse, wheel, selection and paste on Linux** — The mouse in programs, the wheel, selecting text, the scrollbar and pasting, on Linux as on macOS. (2026-10-21)
- [x] **Redraw only what changed on Linux** — Each frame hands the desktop only the rows that changed, instead of the whole window. (2026-10-21)
- [x] **Smoother frames and pointer shapes on Linux** — Frames drawn when the desktop asks and timed for the moment they reach the screen, the right pointer shape over text, and input methods. (2026-10-21)
- [x] **Zoom, links and light and dark on Linux** — Ctrl + and Ctrl - to zoom, Ctrl-click to open links, and following the desktop's light or dark setting. (2026-10-21)
- [x] **Linux and macOS give the same shade over the see-through window** — Both systems now round colours through one shared function; before, about half of all colour and transparency combinations came out one step darker on Linux. (2026-10-16)
- [x] **A baud opened from the Dock starts your shell in your home folder** — It no longer reads a config.toml from whatever folder it started in. Relative font and theme paths are read beside the config file. (2026-10-14)
- [x] **baud requires macOS 26 and says so on older systems** — On macOS older than 26, baud prints a clear message and exits instead of crashing. (2026-10-13)

## How this page stays true

This page is generated from `docs/data/roadmap.toml`, the same planning list
the progress dashboard reads, by `scripts/gen-features.py`. Every item marked
shipped is claimed by a feature in [the feature list](guide/features.md) or
named there as not a feature, and the build fails when one is neither.
