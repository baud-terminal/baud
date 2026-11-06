# Configuration

baud needs no configuration. With none it uses JetBrains Mono at 13 points,
follows your Mac's light or dark appearance, and draws its window in the
Mac's standard see-through material. A config file changes only what you put in it.

## Where the file goes

Copy [config.toml.example](../../config.toml.example) to
`~/.config/baud/config.toml` and delete every line you do not want to
change. The example explains each key in more detail than this page.

baud looks in three places and reads the first file it finds:

1. the file named by the `BAUD_CONFIG` environment variable;
2. `$XDG_CONFIG_HOME/baud/config.toml`, when `XDG_CONFIG_HOME` is set to a
   full path;
3. `~/.config/baud/config.toml`.

It does not read a config from the folder it was started in.

- **What you save shows at once.** baud watches the file, the theme files
  it names and the fonts it names by path, and applies a save to the open
  window a moment after it lands, whether you edit in place, your editor
  saves to a new file and swaps it in, or you create the file for the first
  time. A config that is a link, as home-manager makes one, is watched
  where the link is: switching to a new generation shows at once, and so
  does an edit through the link to the file it names. A few settings wait
  for the next launch: see [When a change shows](#when-a-change-shows).
  `config_reload = "off"` turns this off.
- **A mistake does not stop baud.** A key it does not know, or a value it
  cannot use, is skipped with a warning and the default is kept. A line it
  cannot read at all, one that is not `key = value` or a list that is never
  closed, is skipped when baud starts; when you save it later, baud keeps
  the settings it had. Either way a row at the bottom of the window says
  what was wrong, until a save fixes it. Warnings also go to the terminal
  baud was started from ([Installing](install.md#if-something-looks-wrong)).
- **Paths can be relative.** A font or theme path that does not start with
  `/` or `~/` is read from the folder the config file is in.

The file is TOML: `key = value` lines, with `[section]` headings for the
groups below. Text values go in double quotes; numbers and `true`/`false`
do not.

## When a change shows

Almost every setting shows in the open window as soon as you save:
colours, the theme and its files, the selection and cursor colours, fonts
and their size, padding, the cursor's shape, blink and trail, the bell,
emoji, scrolling, the screen transition, the exit-status marks, the glass's
dim, tint and shadow, `[text] ambiguous_width` and `join_spacing_marks`,
and what programs may do
(`[window]`, `[font]`, `[clipboard] osc52` and `copy_on_select`,
`[launch] splash` turned off while the logo shows,
`[screen]`'s logging, checksum, substitute mark, notifications and print
choices, `[pointer]`), and `[images] animation`.

These keep their old value in the windows already open, because each was
built with them, and a new window (Cmd-N, or Ctrl+Shift+N on Linux) takes
the new one: `colorspace`, `[glass] style`, `[images] pixels_mb` and
`advertise_sixel`, `vs16_width`, `[screen] printing`,
`[clipboard] body_kb` and `[shell] integration` and `program`, which are
how a shell is started.

- **A new `ambiguous_width` or `join_spacing_marks` measures the next
  character.** Text already on the screen keeps the columns it was written
  with.
- **On Linux, padding, the scroll mode, the glass's tint and shadow and
  the activity lights** are not drawn, so a change to them shows nothing
  there; the Linux window has no title bar to hold the lights.
- **A size you zoomed to stays.** A new `font_size` shows only while the
  window is at the old one; Actual Size then returns to the new one.
- **Colours a program set stay** until it resets them, as they do when the
  theme changes.
- **A font a program chose is replaced** when you change `fonts`; a save
  that leaves `fonts` alone keeps it.
- **A deleted file keeps the settings in force**, and the row at the bottom
  says so until the file is back.
- **An empty file is the defaults**, as it is when baud starts with one.
- **While the message row shows**, the window gives it the bottom line, and
  programs are told the window is a line shorter until it goes.

## Top level

These keys go at the top of the file, before any `[section]`.

| key | values | default | what it changes |
|---|---|---|---|
| `font_size` | number, points | `13` | The text size. |
| `padding` | number, points | `8` | The space between the text and the window's edges. `0` removes it. |
| `theme` | `"auto"`, `"dark"`, `"light"` | `"auto"` | Which colours to use. `"auto"` follows System Settings > Appearance and switches with it; on Linux it follows the desktop's light or dark setting, and uses light where the desktop states no preference. |
| `colorspace` | `"srgb"`, `"display-p3"` | `"srgb"` | How colours are read. Leave it at `"srgb"` unless you want a theme's colours shown in the wider Display P3 range. |
| `vs16_width` | `"two"`, `"one"` | `"two"` | How wide an emoji such as ❤️ is when it is written with the emoji selector. Use `"one"` only if your shell or editor measures it as one column and the cursor lands in the wrong place. A program that asks for grapheme cluster mode gets two columns either way, as that mode requires. |
| `emoji` | `"font"`, `"system"` | `"font"` | Whether symbols such as ⚡ are drawn as text from your font, or in colour as macOS draws them. |
| `bell` | `"sound"`, `"flash"`, `"none"` | `"sound"` | What happens when a program rings the bell while baud's window is in front: the alert sound, a brief flash of the screen, or nothing. |
| `fonts` | list of font names, or font file paths | baud's built-in fonts | The fonts to draw with, first choice first, each named by its family, such as `"Fira Code"` or `"Menlo"`, in any letter case. A later font fills in characters the earlier ones lack. A single face can be named by its PostScript name, such as `"FiraCode-Bold"`, or its full name, such as `"Fira Code Bold"`; it is then the regular text's face and its family supplies bold and italic. baud looks the names up among the fonts installed on the machine once, when it starts (on Linux, in the folders your fontconfig configuration lists, `/etc/fonts/fonts.conf` or the file `FONTCONFIG_FILE` names and your own `~/.config/fontconfig/fonts.conf`, leaving out the fonts its `<selectfont>` rules reject); a name it cannot find is skipped with a warning, and if none is found the built-in fonts are used. A value with a `/` or ending in `.ttf`, `.otf`, `.ttc` or `.otc` is a font file instead, and a `.ttc` file can take `@N` to pick a font inside it. Bold and italic text uses the bold and italic faces of the first font in the list that is found; a style it has none for is drawn from its nearest face, never faked. baud's built-in fonts can be named too, and are used ahead of an installed copy: `"Proto Hebrew Mono C"`, the built-in text font's working name, with its bold and italic faces, `"Symbols Nerd Font Mono"` and `"STIX Two Math"`. |
| `font_fallback` | `true`, `false` | `true` | Whether baud's built-in fonts stay behind a `fonts` list you set, so Hebrew, Nerd Font icons and maths symbols still draw when your fonts lack them. Either way, a character none of your fonts or baud's has comes from the font your system picks for it. |
| `config_reload` | `"on"`, `"off"` | `"on"` | Whether a saved change to this file, or to a theme or font file it names, shows in the open window. With `"off"`, changes wait for the next launch; the save that turns it off is the last one applied. |

The built-in fonts are JetBrains Mono, with its ligatures and real bold and
italic faces, baud's own Hebrew, and the Nerd Font icons, with the system's
Menlo, emoji, symbol and Chinese, Japanese and Korean fonts behind them.

## [text]

| key | values | default | what it changes |
|---|---|---|---|
| `ambiguous_width` | `"narrow"`, `"wide"` | `"narrow"` | How many columns characters such as box-drawing lines, Greek and Cyrillic letters take. Set `"wide"` only to match a program set up to count them as two, such as vim with `ambiwidth=double`. |
| `join_spacing_marks` | `"on"`, `"off"` | `"off"` | Whether a vowel sign written after its letter, such as the ि and ा of Hindi, a visarga, or Thai's and Lao's SARA AM, takes no column of its own. `"off"` gives it a column, as Linux shells and vim count, so the cursor stays where they expect; set `"on"` if you mostly use the Mac's own bash or zsh, or tmux, which count it as none. A program that asks for grapheme cluster mode, such as Neovim, gets a whole syllable in one cell either way. |

## [palette]

| key | values | default | what it changes |
|---|---|---|---|
| `floor` | a number from 1 to 21, or `"off"` | `1.2` | The lowest contrast text is allowed against its background. Text below it is made lighter or darker, keeping its colour, so it stays readable. The default only rescues text that is nearly invisible. It also covers text that `[glass] app_backgrounds = "dim"` would make unreadable: a background a program set is measured as the dim draws it, and text readable on it as asked but not as drawn is lifted, and so is text the dim takes from 3:1 or more to below 3:1. Box-drawing lines, such as the dividers between panes, are drawn in the colour the program asked for; block characters used for graphs are not exempt. While Increase Contrast is on in System Settings, or on Linux the desktop's high-contrast setting, text below 3:1 is lifted to 4.5:1 (or to your own floor, if it is stricter), even with `"off"`. |
| `light` | a theme file or name | baud's light colours | The colours used when the light appearance is in effect. |
| `dark` | a theme file or name | baud's dark colours | The colours used when the dark appearance is in effect. |
| `selection` | a colour (`"#rrggbb"`, `"rgb:rr/gg/bb"` or an X11 name), or `"none"` | the theme file's, else reverse video | The colour behind selected text, on both appearances; it wins over a theme file's. The find field, its current match and a region a program highlights take it too. With a colour here and no selection text colour, from `selection_text` or the theme file, each cell keeps its own text colour. `"none"` takes a theme file's colour away. |
| `selection_text` | a colour, or `"none"` | the theme file's, else reverse video | The colour of selected text. With both keys left out and no selection colours in the theme file, or both set to `"none"`, a selection is drawn in reverse video: each cell's text and background exchanged. A program can set its own selection colours, and its reset returns to these. |
| `cursor` | `"baud"`, `"theme"`, a colour, or `"none"` | `"baud"` | The cursor's colour, on both appearances. `"baud"` is baud's amber, the logo's, `#F4A32C`, on the dark and the light theme and on each reversed. On the light theme it reads 1.80:1 against the background, under the 4.5:1 WCAG asks of text, by choice; for the contrast, set `cursor = "#986100"` and `cursor_text = "#EFEFEF"`. On a theme file's background, or one a program sets, it is chosen against the background then drawn, deepened or lightened until it stands out as clearly as WCAG asks of text (4.5:1). Where the window lets the desktop show through the background, the contrast depends on what is behind it. `"baud"` keeps the amber over a theme file's cursor; `"theme"` asks for the theme file's cursor colour and text where it names them. A colour set here wins over a theme file's. Every trail and blink animation takes the cursor's colour. `"none"`, or a theme file's none under `"theme"`, makes a block cursor its cell's text and background exchanged, the classic cursor, and a bar or underline the text colour. A program can set its own cursor colour, and its reset returns to this one. |
| `cursor_text` | a colour, or `"none"` | under baud's amber, the logo's near-black where it reads and else the background; under `"theme"`, the theme file's; under any other colour, the background | The colour of the text under a block cursor. Under baud's amber it is the logo's near-black, `#15171B`, on the dark and the light theme, and on a theme file's background chosen against it, the background where the near-black would not read; each reads at 4.5:1 on the amber. It stays when a program sets the cursor's colour. |

baud's own colours are xterm's sixteen on dark and Modus Operandi's on
light, with five made readable on their page: on dark, red, blue, magenta
and bright blue are lighter, and on light, colour 7, the light grey, is
darker, each keeping its hue. Every colour but the page's own black or
white then reads at 4.5:1 as text. As backgrounds the five go the other
way: the default text on any of them reads below 3:1. A theme you set is
drawn in its own colours; only a colour it leaves out is baud's.

A theme can be a base16 or base24 scheme, a ghostty, kitty, foot or
alacritty theme (alacritty's TOML, or its older YAML), an iTerm2 colour
preset (`.itermcolors`, its colours converted as macOS converts them), a
Windows Terminal colour scheme, X resources, or a list of 16 colours, one
`#rrggbb` a line, such as pywal's `colors` file. X resources are read as
`xrdb` loads them: their `#define`, `#ifdef` and `#include "file"` lines
are run, an included file is looked for next to the one including it, and
only the entries every program reads (`*color0`, `*.color0`) are used; an
entry for one program, such as `URxvt*color0`, is skipped and named. A
kitty theme's `oklch()` and `lab()` colours are drawn as kitty draws them,
and one outside what the screen can show as kitty's theme preview shows
it. A kitty theme's none and background mean what they mean in kitty:
"cursor none" draws the cursor in the colours of the cell under it until
a cursor colour is set over it, "cursor_text_color background" draws the
text under the cursor in its cell's background, and "selection_foreground
none" keeps each selected cell's own text colour over a selection colour
and selects in reverse video without one. baud tells which by what is in the
file, not by its name, and says what it read it as; a file whose name says
another format is still read, and the name is mentioned. A foot file or
iTerm2 preset holding both a dark and a light theme is refused, so save the
one you want on its own. Other theme formats are recognised and named, and
a file that mixes two formats is refused with the line of each. A bare name
such as `dark = "night"` is looked for in a `themes` folder next to the
config file, then in `$XDG_CONFIG_HOME/baud/themes/` and
`~/.config/baud/themes/`, as given and then with `.yaml` and `.yml`.

## [cursor]

| key | values | default | what it changes |
|---|---|---|---|
| `shape` | `"block"`, `"underline"`, `"bar"` | `"block"` | The cursor's shape. A program can still ask for another. |
| `blink` | `"auto"`, `true`, `false` | `"auto"` | Whether the cursor blinks. `"auto"` blinks unless Accessibility's "prefer non-blinking cursor" is on; `false` holds it steady. |
| `blink_interval_ms` | number, milliseconds | macOS's setting, else `250` | How long the cursor stays shown, and then hidden, in each blink: two blinks a second by default. |
| `blink_animation` | `"none"`, `"plasma"`, `"fade"`, `"fire"`, `"ufo"` | `"none"` | What the cursor does while it is hidden. `"none"` switches it straight off and on, a plain blink. |
| `stop_blinking_after` | number, seconds | `0` | Stop blinking, with the cursor shown, after this long without typing, clicking or pasting. `0` means never. |
| `trail` | `"plasma"`, `"fire"`, `"lightning"`, `"ice"`, `"ufo"`, `"orb"`, `"liquid"`, `"quad"`, `"sweep"`, `"ghost"`, `"off"` (`true` also means the default) | `"plasma"` | The effect left behind when the cursor jumps. [The cursor gallery](../cursor.md) shows each one. |
| `trail_scroll` | `"ride"`, `"stay"` | `"ride"` | Whether a trail moves with the text when it scrolls: a trail in flight rides output's scroll, and a cursor a program scrolls with its line draws no trail. |
| `guide` | `"on"`, `"off"` | `"off"` | Whether the row the cursor is on is highlighted with a pale band the window's width, iTerm2's cursor guide. A program can turn it on or off for itself until the terminal is reset. |
| `unfocused_hollow` | `"on"`, `"off"` | `"off"` | Whether the cursor turns into a hollow box while another window has the keyboard, so you can see which window you are typing in. Every shape, the bar and underline too, becomes the box; `"off"` leaves it as it is in front. |

When Reduce Motion is on, or on Linux the desktop's reduced-motion
setting, the blink animation is always `"none"` and the trail is off,
unless you name a trail here.

## [screen]

| key | values | default | what it changes |
|---|---|---|---|
| `transition` | `"on"`, `"off"` | `"off"` | Slide the screen in when a full-screen program such as vim or less opens or closes: the program's screen rises from below over the shell's as it opens, and the shell's comes back down from above as it exits, in a quarter of a second, with whatever the program draws meanwhile shown at once. The screen slides in faint and comes up to full once it lands, so moving text stays under the flash limits of the web accessibility guidelines (WCAG 2.3.1). With Reduce Motion on, the new screen fades in instead. |
| `badge` | `"on"`, `"off"` | `"on"` | Whether a program's badge is shown: a large faint label in the top right of the window, behind the text, such as the host you are on or the branch you are working in, which programs and shell scripts written for iTerm2 set. |
| `wrap_words` | `"on"`, `"off"` | `"on"` | Whether finished text wraps at its words. A word that does not fit at the end of a line moves whole to the next one, where `"off"` cuts it at the last column as the program wrote it. In a paragraph that mixes directions, a run of the other language the wrap would cut -- its words falling on two lines -- gets a line of its own, laid out its own way against its own edge -- English left to right at the left, in a Hebrew paragraph -- and the paragraph carries on below it; a short run that fits stays where it is, and a single foreign word that does not fit drops to the next line inline, as any word does. It covers text you scroll back to, and, with a shell that marks its prompts (shell integration, `OSC 133`), the output above the prompt you are at; the line you are typing, a command still running and full-screen programs keep the program's lines, and so does output the window was narrowed past while the shell waited at its prompt, until the window is as wide again as when that text was written. The extra lines push older text up into the scrollback. Copying gives the text as the program wrote it either way, except while a program has set `?96`, when a copy follows the lines as drawn. A path wraps before its slashes rather than after them, `/home` then `/jordan`, and a path after a space moves to the next line whole, so `cd` is never left with a lone `/`; a Windows path (`C:\`, `\\server`) wraps at its backslashes, and a URL keeps `https://` and its host together. Fractions and dates such as `1/2` and `06/07/99`, and words such as `w/o` and `and/or`, wrap as Unicode's line breaking algorithm wraps them. This is a tailoring of that algorithm (UAX #14), which it requires to be disclosed: inside a path it allows a break before `/` where the algorithm forbids one, and removes the break it allows after `/`. |
| `wrap_marker` | `"on"`, `"off"` | `"on"` | Whether a small hook arrow, ↪, is drawn in the window's padding beside each line `wrap_words` carried on, on the right for a right-to-left line, so a line that starts `/jordan` does not read as a path the program printed. It takes no column, the program is told the same size, and copying never includes it. A full-screen program's screen, the line you are typing and lines a program wrapped at the edge have none, and with `[window] padding = 0` there is no room for it. |
| `exit_status` | `"on"`, `"off"` | `"off"` | Whether each command's result is shown beside its prompt: a filled dot when it succeeded, a cross when it failed and a ring when it was cancelled, in your theme's blue, red and grey, for a shell that marks its prompts (shell integration, which sends `OSC 133`). The marks have a column of their own at the left, so the window has one column fewer for text while this is on; a command still running, or one the shell sent no result for, has no mark. |
| `prompt_at_bottom` | `"on"`, `"off"` | `"off"` | Whether your shell's prompt sits on the window's last row. `"on"` draws each new prompt there: in a new window, after `clear`, after Control-L, and when your shell redraws its prompt after the window grows (bash and zsh do). A prompt of several lines lands whole, its last line on the last row. Earlier output stays where it was, with blank rows between it and the prompt, and once the window is full every prompt is on the last row anyway. Making the window smaller moves its top rows into the scrollback, as it does whenever the prompt is on the last row, and making it larger again brings them back. It needs a shell that marks its prompt and where your typing begins (shell integration that sends `OSC 133` `A` and `B`, as baud's own does for the zsh and bash it starts -- see `[shell] integration` -- and as fish, nushell and Ghostty's shell integration do), and it leaves full-screen programs such as vim alone. Turned on while you are at a prompt, that prompt moves down at once; turned off, nothing moves back. |
| `checksum` | `"on"`, `"off"` | `"on"` | Whether a program that asks for a checksum of part of the screen gets one. Test suites such as vttest use it to check what they drew. A program can ask about one character at a time and so read back what is on your screen, including what other programs printed there; `"off"` stops that, and the request then gets no answer. |
| `substitute_mark` | `"on"`, `"off"` | `"off"` | Whether a program's substitute character (SUB, which marks a character lost on the way, and which also cancels an escape sequence cut short) shows as a mark where it arrived, as DEC's terminals and xterm show it. `"off"` shows nothing there. |
| `substitute_character` | one character, such as `"?"` or `"\u2426"` | `"\u2426"` (␦) | The mark `substitute_mark` shows: one character that is one cell wide under every width setting, so a character such as § or ¿, one cell by default and two under `ambiguous_width = "wide"`, is refused. A character that does not fit is named in the message row and ␦ is used. |
| `logging` | `"on"`, `"off"` | `"on"` | Whether a program can log everything it sends to a file it names, as xterm's logging does. The log is appended to, and only a file you own that no one else may write is used; with no name it is `baud.log.<host>.<date and time in UTC>.<pid>` in the folder your shell started in, and a relative name is read from there. A program on the other end of an ssh connection, or a file you cat, can otherwise append to any such file, your shell's startup files included; `"off"` stops that. |
| `printing` | `"on"`, `"off"` | `"on"` | Whether a program can print. Each print opens the macOS print panel, which shows what will print, and nothing reaches paper until you press Print; a program that sends its output straight to the printer hides it from the screen until it is done, and the window's title says "Printing" meanwhile. Programs are told there is a printer only when macOS lists one. `"off"` tells every program there is no printer. Linux has no print panel yet, so programs there are always told there is none. |
| `notifications` | `"on"`, `"off"` | `"on"` | Whether a program can post a desktop notification, such as a build saying it finished. One shows while baud's window is not focused, or whenever the program asks for that; at most four come at once, then one a second. A program written for iTerm2 can show a picture from a file on your machine in one; nothing about the file goes back to the program. On macOS the first asks your permission, and the baud app must be the one in its bundle. `"off"` posts none, and a program asking whether baud shows notifications hears nothing back. A program's progress bar shows either way. |
| `print_colour` | `"on"`, `"off"` | `"off"` | Whether a picture a program prints (a ReGIS hard copy: its drawing, Sixel images and the text around them) prints in colour. `"off"` prints it black and white, as DEC's VT340 did by default: every pixel with any colour is inked. |
| `print_background` | `"on"`, `"off"` | `"off"` | Whether a picture printed in colour prints the screen's background too, rather than leaving the paper white there. |
| `print_rotated` | `"on"`, `"off"` | `"off"` | Whether a printed picture is turned a quarter, counter-clockwise, so a large one runs down the page. |
| `print_rgb` | `"on"`, `"off"` | `"off"` | Whether a picture printed in colour names its colours in RGB rather than HLS on the way to the printer. It changes nothing on paper. |

## [scroll]

| key | values | default | what it changes |
|---|---|---|---|
| `mode` | `"smooth"`, `"normal"` | `"smooth"` | Whether output slides up smoothly or jumps a line at a time. |

## [images]

| key | values | default | what it changes |
|---|---|---|---|
| `pixels_mb` | number, MiB | `95` | How much memory pictures shown in the terminal may use. The default holds three full 4K screens. A picture that does not fit is refused with a warning rather than shown in part. On macOS it also bounds the picture a program written for iTerm2 names for a notification: a larger one is left out, and the notification's text still shows. |
| `animation` | `"os"`, `"on"`, `"off"` | `"os"` | Whether animated pictures play. `"os"` plays them unless Reduce Motion is on or "Auto-play animated images" is off in macOS's Accessibility settings, or on Linux the desktop's reduced-motion setting is on. `"on"` always plays them, and `"off"` keeps each one still at the frame it shows. A change shows at once, and so does a change to either macOS setting. |
| `advertise_sixel` | `true`, `false` | `false` | Tell programs that baud can show Sixel pictures. Turn it on so a file manager such as yazi shows real image previews instead of a picture made of text characters. Plain `kitten icat` does not accept baud's default answer to its terminal question, so without this setting it waits ten seconds and gives up: turn it on, or run `kitten icat --transfer-mode=stream`, which skips the check. |

## [clipboard]

| key | values | default | what it changes |
|---|---|---|---|
| `osc52` | `"write"`, `"off"` | `"write"` | Whether a program can copy text to your clipboard, which is how Neovim or tmux over ssh reach your Mac's clipboard, or on Linux the clipboard or the primary selection the program names (a copy naming neither, as tmux's does, goes where your own selection goes: the primary selection, unless a program has switched that to the clipboard). Programs can never read your clipboard. With `"write"`, anything printed in the terminal, a file you `cat` included, can replace what you copied. |
| `body_kb` | number, KiB | `256` | The largest copy a program may make. 256 holds 192 KiB of text. A larger copy is refused whole, never cut short. |
| `copy_on_select` | `"on"`, `"off"` | `"on"` | Whether letting go of a selection copies it. On a Mac `"on"` puts it on the clipboard, replacing what Cmd-C copied; `"off"` leaves the clipboard to Cmd-C, unless a program has asked for copy-on-select itself. On Linux a selection becomes the primary selection, which a middle click pastes, or the clipboard while a program has asked for that (xterm's ?1041); this key changes neither. |

## [window]

| key | values | default | what it changes |
|---|---|---|---|
| `activity_lights` | `"on"`, `"title_bar"`, `"bottom_line"`, `"top_line"`, `"off"` | `"on"` | Where nine small lights showing what the line is doing are drawn, named and ordered as in a 1988 U.S. Robotics Courier manual's table of its front-panel lights, with FAX in ARQ's place as on the V.34 Courier: HS (High Speed) while the program's output floods in, each read of it as full as the line allows, a looser sense than a modem's; AA (Auto Answer) when baud answers the program on its own, with a reply nobody typed; CD (Carrier Detect) while the program in front is ssh with its connection up; OH (Off Hook) while a program other than the shell is in front; RD (Receive Data) when baud receives data from the program; SD (Send Data) when baud sends data to it (your keys, a paste, the terminal's answers); CS (Clear to Send), as on a modem, while the program takes what baud sends, out while baud holds bytes it has not taken; FAX while a picture is shown; SYN while the program holds the screen to draw it all at once. A light baud may not read, such as CD for an ssh run under `sudo`, is drawn as a ring rather than dark. Point at a light and what it means shows at once, drawn by baud the same way on macOS and Linux, and stays while the pointer moves onto it; a key, Escape among them, hides it. The same text is the light's help for VoiceOver. A light stays on a fifth of a second after the last thing it shows and never flashes more than three times a second; with Reduce Motion on in macOS's Accessibility settings it stays on two seconds after the last and changes at most once every two seconds, so a stream of activity is one steady light; with Dim Flashing Lights on a lit light is drawn without its glow. The cursor's blink settings do not change the lights. In a narrow window the leftmost lights are left out rather than covering the window's buttons. `"title_bar"` makes them a modem's panel at the right of the title bar, baud's logo and the nine on one plate, each lamp over its letters, macOS's place; with the lights anywhere else there is no plate in the title bar and the logo is beside its buttons; `"bottom_line"` on a thin line of their own under the program's last line, and under the status line a program shows, only as tall as the lights (15 points), less than a text row at the 13 point default, Linux's place, whose window has no title bar; `"top_line"` the same line above the program's first line; `"on"` is your system's place. In full screen the lights stay in the title bar, so where macOS hides it until you move the pointer to the top of the screen they are hidden with it; `"bottom_line"` or `"top_line"` keeps them in view there. `"title_bar"` on Linux has no title bar to go in, so the lights go on the bottom line and the config notice says so. A program is never told about the line: the rows it is told and the text area in pixels leave it out, a report of the whole window's size includes it, nothing a program sends moves it, and smooth scrolling slides the text past it while it holds still. In the title bar and on the line alike the lights sit on a rounded plate filled with your theme's background, their letters in its foreground, and in the title bar each lamp in a socket and the plate in a bevel, both made from that background, and every lamp is one of your theme's sixteen colours exactly, never lightened, darkened or mixed: lit, a dot in the colour `activity_lights_colours` gives it, with a soft glow of that colour (`activity_lights_glow`); unlit, a dot half the size, so lit and unlit differ in size as well as colour; a light baud may not read, a ring in the colour your foreground is, or colour 7. An unlit light is colour 8 in every set. The plate keeps the lights readable whatever the window shows behind the title bar, Liquid Glass included, and the lights follow your theme when it changes. Where the window can grow, it grows by the line when the setting changes and the program keeps its rows; in full screen the text gives the line its height. On Linux, CD is not built yet: it is always dark there, even with ssh in front, rather than the ring a light baud may not read is drawn as. On Linux, which has no desktop setting for Dim Flashing Lights, a lit light always glows; the three-a-second limit still holds. `"off"` takes them all away and reads nothing for them. |
| `activity_lights_colours` | `"meaning"`, `"iec"`, `"modem"` | `"meaning"` | Which of your theme's sixteen colours each activity light is lit in. `"meaning"`, baud's own: one colour for each kind of thing, and CS green for a clear line. On a dark theme HS bright green (colour 10), AA bright yellow (11), CD blue (4), OH cyan (6), RD green (2), SD yellow (3), CS green (2), FAX magenta (5), SYN bright magenta (13); on a light theme, where a theme's yellow is usually darkened toward brown, AA cyan (6), OH bright blue (12) and SD bright cyan (14) instead. `"iec"`, the colours the IEC 60073 standard gives an indicator's meaning: green (2) for HS, CD and CS, CS lit being the line's normal condition, cyan (6) for FAX and SYN, white (7) for the rest. `"modem"`, every light red (1), as the front-panel lamps of the modems the letters come from were. Whether your theme is dark or light is read from its background against its foreground. Any other value keeps `"meaning"` and the config notice says so. |
| `activity_lights_glow` | a number from `0` to `1` | `0.4` | How strongly a lit activity light glows: a soft halo of its own colour starting a point off the dot, so the dot itself reads against what it stands on, its plate or its socket, as it would with no glow. `0` draws no glow, `1` the strongest. A value outside 0 to 1 is held to it, and one that is not a number keeps `0.4`; the config notice says which. With Dim Flashing Lights on in macOS's Accessibility settings no light glows, whatever this says. |
| `status_line_lights` | `"off"`, `"states"` | `"off"` | Whether DEC's indicator status line, when a program shows it, also carries five of the activity lights as words in its free columns after the printer: CD at column 62, OH at 66, CS at 70, FAX at 73 and SYN at 77. Each word is there while what it shows holds and follows it within the 80 milliseconds DEC's standard gives the line, without the lights' calm pace or Reduce Motion's two seconds. CD or OH reads `CD?` or `OH?` when baud cannot tell: the system would not let baud look at the program, or, for CD on Linux, baud has no way to look yet. HS, AA, RD and SD never go there, since the line shows what is true and not what just happened, and the lights keep all nine wherever they are drawn. Past 37 points of the default font, JetBrains Mono, the words are left out, so they never flash past the web accessibility guidelines' limit (WCAG 2.3.1). A word the window is too narrow to hold whole is left out, as every field on the line is. Hold the pointer over a word to see what it means, the same text as the light's own hover text, in a box above the line drawn the same on macOS and Linux: it stays while the pointer is on the word or on the box, goes when the pointer leaves both or the word goes, and any key you type hides it (the key still reaches the program). Both macOS and Linux. |
| `tearing` | `"on"`, `"off"` | `"off"` | Whether the desktop may show a frame the moment it is drawn, even partway through the screen's refresh, which can cut a moving picture in two along a line: the least delay between a key and its letter on screen, at that price. Only where the desktop offers it (the Wayland `tearing-control` protocol), and the desktop still decides, by what the screen can do and the window's state. Saving it takes effect with the next frame. Linux only: on macOS frames always wait for the refresh. |
| `operations` | `"on"`, `"off"` | `"on"` | Whether a program can move, resize, minimise, raise, lower, maximise or full-screen baud's window, as xterm lets it. A test suite such as vttest asks this way, and so does a program that sizes the window to fit what it draws. A program on another machine, or a file you `cat`, can also move the window off the screen or hide it; `"off"` refuses every request. Programs can still ask where the window is and how big it is either way. |

## [launch]

| key | values | default | what it changes |
|---|---|---|---|
| `splash` | `"on"`, `"off"` | `"on"` | Whether each window opens with baud's colour logo in the middle of it once the window is on screen, the first window and every one you open after it: a burst of plasma draws it from left to right and a second carries it straight back out from left to right, in one movement, gone about half a second after it starts, the plasma in the colour of each glyph it passes over. Your keys go to the shell the whole time, and the shell's output and prompt show the moment they arrive, in front of the logo, which plays around them. With reduced motion on -- macOS's Reduce Motion, or the Linux desktop's setting once the desktop has reported it -- the logo shows whole and fades once your shell has drawn something, or half a second after the window is on screen if the shell draws nothing by then. It stays under the flash limits of the web accessibility guidelines (WCAG 2.3.1). `"off"` turns it off; saving `"off"` while the logo shows takes it down at once. A window you open with Cmd-N, File > New Window, the Dock or Ctrl+Shift+N plays its own, from the moment it is on screen. Nothing waits for it: the window's shell starts before the window is drawn, and the logo plays alongside it. |

## [shell]

| key | values | default | what it changes |
|---|---|---|---|
| `integration` | `"on"`, `"off"` | `"on"` | Whether baud marks your shell's prompts for it, so the output above the prompt you are at counts as finished: it wraps at its words like the rest (`[screen] wrap_words`), each command's result can show beside its prompt (`[screen] exit_status`), and a list of completions stays on the screen. It covers the zsh, bash and fish baud starts for you (`$SHELL`). Your own startup files -- `.zshenv` to `.zlogin` in your `ZDOTDIR` or home, `/etc/profile` and `.bash_profile`, `config.fish` -- run as they always do, in the same order, and baud exports nothing of its own to the commands you run. fish 4 and later mark their own prompts, and baud adds nothing to them; in fish 3 baud marks each prompt and command but not where your input begins, which only a `fish_prompt` of baud's in place of yours could. Startup files that already mark the prompt, as WezTerm's, kitty's or Ghostty's shell integration does, keep their own marks. In zsh and bash your startup files can test the shell variable `BAUD_SHELL_INTEGRATION`, and unsetting it keeps baud's marks out of that shell; fish reads its configuration before baud's marks are added. bash 4.4 or later gets the marks; macOS's own `/bin/bash` (3.2) and bash before 4.4 start as before, with no marks. bash-preexec 0.6.0 and 0.7.0, and Atuin's setup, which loads it, run as without baud; a bash that loads Atuin gets none of baud's marks, as Atuin's functions spell marks of their own. In bash, these differ from a shell without baud: from bash 5.1 a `PROMPT_COMMAND` you did not export is an array, yours first as it was and baud's entry last, so what your entries print counts as the last command's output; in that array your entries read `$_` at the first prompt as the name of one of baud's functions rather than what your startup files left in it; an exported `PROMPT_COMMAND` passes baud's two entries on to the shells bash starts, where they mark nothing; with `PROMPT_COMMAND` exported or bash before 5.1, your own entries read the command's `$?`, but `PIPESTATUS` only as its last status and `$_` as the name of one of baud's functions; a DEBUG trap of yours also runs before the commands of baud's entries, so one that starts a timer, as a command-duration prompt's does, starts it at the prompt; and an exported `PS1`, `PS2` or `PS0` keeps baud's marks while a command runs when another entry follows baud's in `PROMPT_COMMAND`, as bash-preexec's does, or when the line begins with a subshell `( ... )` or `coproc`. `"off"` starts every shell as any other terminal does. |
| `program` | a path | unset | The shell each new window starts, by its full path -- `/run/current-system/sw/bin/zsh`, or `~/.nix-profile/bin/fish` under your home folder -- in place of `$SHELL`. Unset, a window starts `$SHELL` as baud was given it, else the system's shell (`/bin/zsh` on macOS, `/bin/sh` on Linux). baud starts the program as it starts `$SHELL`: as a login shell, its prompts marked when its name is zsh, bash or fish (`integration`), and the shell and the commands it runs see it as their `$SHELL`. Opened from the Dock or Finder, baud is given the shell your account names, which can be another shell than the one your other terminals run -- macOS's own zsh rather than one Nix installed, say, whose startup files then do work again -- and this names the one you mean. A path that is not absolute, or that names no file you may run, shows a notice in the window, which starts the shell it would start without the line. A change shows in the next window. |

## [keyboard]

How keys behave before a program asks otherwise: the four keyboard modes a DEC terminal lets you set in its Set-Up screen. A program can still change each one. Saving a different value while baud runs, with `config_reload` on (the default), changes it in the open windows at once and leaves the other modes as the programs set them.

| key | values | default | what it changes |
|---|---|---|---|
| `auto_repeat` | `"on"`, `"off"` | `"on"` | Whether a key held down repeats. A new window starts with this, and a full reset returns to it. |
| `application_cursor_keys` | `"on"`, `"off"` | `"off"` | Whether the arrow keys send the codes a program asks for with application cursor keys (DECCKM), `ESC O A` rather than `ESC [ A`. A new window starts with this, and a full reset returns to it; a soft reset (DECSTR) turns it off, as a DEC terminal's does. |
| `new_line` | `"on"`, `"off"` | `"off"` | Whether Return sends a carriage return and a line feed, and a line feed a program prints also goes back to the first column (new line mode, LNM). A new window starts with this, and a full reset returns to it. |
| `application_keypad` | `"on"`, `"off"` | `"off"` | Whether the numeric keypad sends the codes a program asks for with the application keypad (DECKPAM), such as `ESC O p` for 0, rather than its digits. As on the VT420 and VT510, which do not keep this setting, it is not kept: changing it switches the open windows' keypad, and a new window or a reset starts with digits. |

## [pointer]

| key | values | default | what it changes |
|---|---|---|---|
| `hide_while_typing` | `"on"`, `"off"` | `"on"` | Whether typing hides the mouse pointer until you move the mouse, as xterm does. With `"on"` it stays visible while a program is using the mouse (vim with mouse support, htop, tmux), so you can still click; `"off"` never hides it. A program can choose differently for itself, and a reset returns to this. |

## [font]

| key | values | default | what it changes |
|---|---|---|---|
| `alternatives` | a list of up to nine font names or files | `[]` | The fonts a program can switch its text to, as `SGR 11` to `SGR 19` ask for the first to the ninth alternative font. Name each as in `fonts`: a family, or a file. An empty name leaves that font unset, so `["", "Fira Code"]` sets only the second. Text in a font with no name here draws in your ordinary font, as `SGR 10` does. Bold and italic text in an alternative font draws from that family's own bold and italic, and a character the font lacks comes from your ordinary fonts. A name no installed font has is left out, and the message row says so. |
| `fraktur` | a font name or file | `""` | The face a program's Fraktur (Gothic) text draws in, as `SGR 20` asks for. Name it as in `fonts`. baud carries no Fraktur face of its own, so with none named Fraktur text draws in your ordinary font. Bold and italic come from that family's own faces, and a character the face lacks comes from an alternative font the text is also in, then your ordinary fonts. A name no installed font has is left out, and the message row says so. |
| `changes` | `"on"`, `"off"` | `"on"` | Whether a program can change the font, as xterm lets it: switch to another of the fonts baud has loaded (the embedded font, which answers to its family name, "Proto Hebrew Mono C" for now -- "JetBrains Mono" does not, as only the styled faces carry it -- or one this file lists), or make the text bigger or smaller a step at a time, within 6 to 72 points; a step past them is refused, where Bigger and Smaller in the View menu stop at the bound. On Linux the step and the bounds are in points too, drawn at the pixels of the scale the desktop asks for. `"off"` refuses every change. A program can ask which font is in use either way. |

## [glass]

The window's material and how solid it is. Several of these come in pairs,
one for the dark appearance and one for the light, because the same
darkening that suits a dark desktop is far too much over a light one.

| key | values | default | what it changes |
|---|---|---|---|
| `style` | `"standard"`, `"regular"`, `"clear"` | `"standard"` | The window's material. `"standard"` is the Mac's standard see-through window material, the one Apple puts behind an app's content: the desktop shows through faintly, and Apple draws it subdued while another window is in front. `"regular"` is Liquid Glass, frosted; `"clear"` is thinner, more see-through Liquid Glass. |
| `dim_dark` | 0 to 1 | `0.70` | How solid the background is under the dark appearance, from 0 (clear) to 1 (opaque). The default keeps text readable at Apple's minimum contrast even over a white window behind; turn it down to see more of the desktop. |
| `dim_light` | 0 to 1 | `0.60` | The same, under the light appearance, where the default keeps text readable over a black window behind. |
| `app_backgrounds` | `"dim"`, `"opaque"` | `"dim"` | Whether backgrounds a program paints, such as btop's panels, are see-through like the rest (`"dim"`) or solid (`"opaque"`). |
| `pictures` | `"opaque"`, `"dim"` | `"opaque"` | Whether a picture drawn with text characters, such as a file manager's photo preview, is kept solid so it reads as a picture. |
| `sixel_backgrounds` | `"opaque"`, `"dim"`, `"clear"` | `"opaque"` | How a Sixel image's solid background draws: solid, as on an opaque window; see-through over the text, which shows through it faded (`"dim"`); or clearing what is under it to the see-through background (`"clear"`). |
| `tint_dark` | 0 to 1 | `0.54` | How much Liquid Glass itself is darkened under the dark appearance. `0` turns it off. This and the next three keys change Liquid Glass only. |
| `tint_light` | 0 to 1 | `0.14` | The same, under the light appearance. |
| `hue` | 0 to 1 | `0.60` | The colour of that darkening; `0.60` is a blue. |
| `saturation` | 0 to 1 | `0.80` | How strong that colour is. |
| `shadow` | `"auto"`, `"always"`, `"never"` | `"auto"` | Whether the window casts a shadow. `"auto"` casts one only under the dark appearance. |

A status bar drawn in reverse video, such as the ones less and vim show,
and the whole screen while a program turns on reverse-screen mode (`?5`),
are drawn a little more solid than the dim when that is what their text
needs to read at the same minimum contrast: on the light appearance at its
default dim, 0.68 instead of 0.60, on every material and on Linux alike.
The window stays see-through behind
them, and the dark appearance's default already reads, so it is unchanged.

While Reduce Transparency is on in System Settings, Accessibility, Display,
every background and the margin around the text are drawn solid, whatever
the dims say, and go back to see-through when it is turned off.

On Linux `theme = "auto"` means dark, so `dim_dark` is used unless
`theme = "light"`; `style`, the tints, `hue`, `saturation` and `shadow` have
no effect there.

## Keys baud no longer reads

A config written for an older baud keeps working: baud accepts these keys
and prints a warning for each. Three of them do nothing now, and
`[glass] images` is still read, as `app_backgrounds`.

| key | what replaced it |
|---|---|
| `background_alpha` | `[glass]` `dim_dark` and `dim_light` |
| `font_smoothing` | nothing: text is drawn the same way on every system |
| `[images] body_kb` | nothing: `pixels_mb` is the only image limit |
| `[glass] images` | `app_backgrounds`, which it is read as |

## Environment variables

| variable | what it does |
|---|---|
| `BAUD_CONFIG` | A config file to read instead of the usual places. |
| `XDG_CONFIG_HOME` | Where to look for `baud/config.toml` before `~/.config`. |
| `BAUD_LOG` | How much baud says: `err`, `warn` (the default), `info`, `debug` or `verbose`. |

This page is held to the code: a test fails when a key the config reader
accepts is missing from it, or when it lists a key the reader does not
accept.
