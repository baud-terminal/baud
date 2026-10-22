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

- **baud reads the file when it starts.** Quit and reopen it after an edit.
- **A mistake does not stop baud.** A key it does not know, or a value it
  cannot use, is skipped with a warning and the default is kept. Warnings go
  to the terminal baud was started from ([Installing](install.md#if-something-looks-wrong)).
- **Paths can be relative.** A font or theme path that does not start with
  `/` or `~/` is read from the folder the config file is in.

The file is TOML: `key = value` lines, with `[section]` headings for the
groups below. Text values go in double quotes; numbers and `true`/`false`
do not.

## Top level

These keys go at the top of the file, before any `[section]`.

| key | values | default | what it changes |
|---|---|---|---|
| `font_size` | number, points | `13` | The text size. |
| `padding` | number, points | `8` | The space between the text and the window's edges. `0` removes it. |
| `theme` | `"auto"`, `"dark"`, `"light"` | `"auto"` | Which colours to use. `"auto"` follows System Settings > Appearance and switches with it; on Linux it follows the desktop's light or dark setting, and uses light where the desktop states no preference. |
| `colorspace` | `"srgb"`, `"display-p3"` | `"srgb"` | How colours are read. Leave it at `"srgb"` unless you want a theme's colours shown in the wider Display P3 range. |
| `vs16_width` | `"two"`, `"one"` | `"two"` | How wide an emoji such as ❤️ is when it is written with the emoji selector. Use `"one"` only if your shell or editor measures it as one column and the cursor lands in the wrong place. |
| `emoji` | `"font"`, `"system"` | `"font"` | Whether symbols such as ⚡ are drawn as text from your font, or in colour as macOS draws them. |
| `bell` | `"sound"`, `"flash"`, `"none"` | `"sound"` | What happens when a program rings the bell while baud's window is in front: the alert sound, a brief flash of the screen, or nothing. |
| `fonts` | list of font file paths | baud's built-in fonts | The fonts to draw with, first choice first. A later font fills in characters the earlier ones lack. A `.ttc` file can take `@N` to pick a font inside it. |
| `font_fallback` | `true`, `false` | `true` | Whether baud's built-in fonts stay behind a `fonts` list you set, so Hebrew, Nerd Font icons and maths symbols still draw when your fonts lack them. |

The built-in fonts are JetBrains Mono, with its ligatures and real bold and
italic faces, baud's own Hebrew, and the Nerd Font icons, with the system's
Menlo, emoji, symbol and Chinese, Japanese and Korean fonts behind them.

## [text]

| key | values | default | what it changes |
|---|---|---|---|
| `ambiguous_width` | `"narrow"`, `"wide"` | `"narrow"` | How many columns characters such as box-drawing lines, Greek and Cyrillic letters take. Set `"wide"` only to match a program set up to count them as two, such as vim with `ambiwidth=double`. |

## [palette]

| key | values | default | what it changes |
|---|---|---|---|
| `floor` | a number from 1 to 21, or `"off"` | `1.2` | The lowest contrast text is allowed against its background. Text below it is made lighter or darker, keeping its colour, so it stays readable. The default only rescues text that is nearly invisible. It also covers text that `[glass] app_backgrounds = "dim"` would make unreadable: a background a program set is measured as the dim draws it, and text readable on it as asked but not as drawn is lifted, and so is text the dim takes from 3:1 or more to below 3:1. Box-drawing lines, such as the dividers between panes, are drawn in the colour the program asked for; block characters used for graphs are not exempt. While Increase Contrast is on in System Settings, or on Linux the desktop's high-contrast setting, text below 3:1 is lifted to 4.5:1 (or to your own floor, if it is stricter), even with `"off"`. |
| `light` | a theme file or name | baud's light colours | The colours used when the light appearance is in effect. |
| `dark` | a theme file or name | baud's dark colours | The colours used when the dark appearance is in effect. |

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
it. baud tells which by what is in the
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
| `blink` | `"auto"`, `true`, `false` | `"auto"` | Whether the cursor blinks. `"auto"` blinks unless Accessibility's "prefer non-blinking cursor" is on. |
| `blink_interval_ms` | number, milliseconds | macOS's setting, else `333` | How long the cursor stays shown, and then hidden, in each blink. |
| `blink_animation` | `"plasma"`, `"fade"`, `"fire"`, `"ufo"`, `"none"` | `"plasma"` | What the cursor does while it is hidden. `"none"` switches it straight off. |
| `stop_blinking_after` | number, seconds | `0` | Stop blinking, with the cursor shown, after this long without typing, clicking or pasting. `0` means never. |
| `trail` | `"plasma"`, `"fire"`, `"lightning"`, `"ice"`, `"ufo"`, `"orb"`, `"liquid"`, `"quad"`, `"sweep"`, `"ghost"`, `"off"` (`true` also means the default) | `"plasma"` | The effect left behind when the cursor jumps. [The cursor gallery](../cursor.md) shows each one. |
| `trail_scroll` | `"ride"`, `"stay"` | `"ride"` | Whether a trail in flight moves up with the text when output scrolls. |

When Reduce Motion is on, or on Linux the desktop's reduced-motion
setting, the blink animation is always `"none"` and the trail is off,
unless you name a trail here.

## [screen]

| key | values | default | what it changes |
|---|---|---|---|
| `transition` | `true`, `false` | `false` | Fade the screen in when a full-screen program such as vim or less opens or closes. |
| `checksum` | `"on"`, `"off"` | `"on"` | Whether a program that asks for a checksum of part of the screen gets one. Test suites such as vttest use it to check what they drew. A program can ask about one character at a time and so read back what is on your screen, including what other programs printed there; `"off"` stops that, and the request then gets no answer. |
| `logging` | `"on"`, `"off"` | `"on"` | Whether a program can log everything it sends to a file it names, as xterm's logging does. The log is appended to, and only a file you own that no one else may write is used; with no name it is `baud.log.<host>.<date and time in UTC>.<pid>` in the folder your shell started in, and a relative name is read from there. A program on the other end of an ssh connection, or a file you cat, can otherwise append to any such file, your shell's startup files included; `"off"` stops that. |
| `printing` | `"on"`, `"off"` | `"on"` | Whether a program can print. Each print opens the macOS print panel, which shows what will print, and nothing reaches paper until you press Print; a program that sends its output straight to the printer hides it from the screen until it is done, and the window's title says "Printing" meanwhile. Programs are told there is a printer only when macOS lists one. `"off"` tells every program there is no printer. Linux has no print panel yet, so programs there are always told there is none. |
| `notifications` | `"on"`, `"off"` | `"on"` | Whether a program can post a desktop notification, such as a build saying it finished. One shows while baud's window is not focused, or whenever the program asks for that; at most four come at once, then one a second. On macOS the first asks your permission, and the baud app must be the one in its bundle. `"off"` posts none, and a program asking whether baud shows notifications hears nothing back. A program's progress bar shows either way. |
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
| `pixels_mb` | number, MiB | `95` | How much memory pictures shown in the terminal may use. The default holds three full 4K screens. A picture that does not fit is refused with a warning rather than shown in part. |
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
| `operations` | `"on"`, `"off"` | `"on"` | Whether a program can move, resize, minimise, raise, lower, maximise or full-screen baud's window, as xterm lets it. A test suite such as vttest asks this way, and so does a program that sizes the window to fit what it draws. A program on another machine, or a file you `cat`, can also move the window off the screen or hide it; `"off"` refuses every request. Programs can still ask where the window is and how big it is either way. |

## [pointer]

| key | values | default | what it changes |
|---|---|---|---|
| `hide_while_typing` | `"on"`, `"off"` | `"on"` | Whether typing hides the mouse pointer until you move the mouse, as xterm does. With `"on"` it stays visible while a program is using the mouse (vim with mouse support, htop, tmux), so you can still click; `"off"` never hides it. A program can choose differently for itself, and a reset returns to this. |

## [font]

| key | values | default | what it changes |
|---|---|---|---|
| `changes` | `"on"`, `"off"` | `"on"` | Whether a program can change the font, as xterm lets it: switch to another of the fonts baud has loaded (the embedded font, which answers to its family name, "Proto Hebrew Mono C" for now -- "JetBrains Mono" does not, as only the styled faces carry it -- or one this file lists), or make the text bigger or smaller a step at a time, within 6 to 72 points; a step past them is refused, where Bigger and Smaller in the View menu stop at the bound. On Linux, which has no size keys, the step and the bounds are in pixels of the configured size. `"off"` refuses every change. A program can ask which font is in use either way. |

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
| `tint_dark` | 0 to 1 | `0.54` | How much Liquid Glass itself is darkened under the dark appearance. `0` turns it off. This and the next three keys change Liquid Glass only. |
| `tint_light` | 0 to 1 | `0.14` | The same, under the light appearance. |
| `hue` | 0 to 1 | `0.60` | The colour of that darkening; `0.60` is a blue. |
| `saturation` | 0 to 1 | `0.80` | How strong that colour is. |
| `shadow` | `"auto"`, `"always"`, `"never"` | `"auto"` | Whether the window casts a shadow. `"auto"` casts one only under the dark appearance. |

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
