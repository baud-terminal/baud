# Configuration

baud needs no configuration. With none it uses JetBrains Mono at 13 points,
follows your Mac's light or dark appearance, and draws its window in Liquid
Glass. A config file changes only what you put in it.

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
| `theme` | `"auto"`, `"dark"`, `"light"` | `"auto"` | Which colours to use. `"auto"` follows System Settings > Appearance and switches with it. |
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
| `floor` | a number from 1 to 21, or `"off"` | `1.2` | The lowest contrast text is allowed against its background. Text below it is made lighter or darker, keeping its colour, so it stays readable. The default only rescues text that is nearly invisible. It also covers text that `[glass] app_backgrounds = "dim"` would make unreadable: a background a program set is measured as the dim draws it, and text readable on it as asked but not as drawn is lifted. |
| `light` | a theme file or name | baud's light colours | The colours used when the light appearance is in effect. |
| `dark` | a theme file or name | baud's dark colours | The colours used when the dark appearance is in effect. |

A theme can be a base16 or base24 scheme (`.yaml` or `.yml`) or a ghostty
theme file. A bare name such as `dark = "night"` is looked for in a `themes`
folder next to the config file, then in `$XDG_CONFIG_HOME/baud/themes/`
and `~/.config/baud/themes/`.

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

When Reduce Motion is on, the blink animation is always `"none"` and the
trail is off, unless you name a trail here.

## [screen]

| key | values | default | what it changes |
|---|---|---|---|
| `transition` | `true`, `false` | `false` | Fade the screen in when a full-screen program such as vim or less opens or closes. |

## [scroll]

| key | values | default | what it changes |
|---|---|---|---|
| `mode` | `"smooth"`, `"normal"` | `"smooth"` | Whether output slides up smoothly or jumps a line at a time. |

## [images]

| key | values | default | what it changes |
|---|---|---|---|
| `pixels_mb` | number, MiB | `95` | How much memory pictures shown in the terminal may use. The default holds three full 4K screens. A picture that does not fit is refused with a warning rather than shown in part. |
| `advertise_sixel` | `true`, `false` | `false` | Tell programs that baud can show Sixel pictures. Turn it on so a file manager such as yazi shows real image previews instead of a picture made of text characters. Plain `kitten icat` waits for this claim too and, without it, gives up after ten seconds: turn it on, or run `kitten icat --transfer-mode=stream`, which skips the check. |

## [clipboard]

| key | values | default | what it changes |
|---|---|---|---|
| `osc52` | `"write"`, `"off"` | `"write"` | Whether a program can copy text to your clipboard, which is how Neovim or tmux over ssh reach your Mac's clipboard. Programs can never read your clipboard. With `"write"`, anything printed in the terminal, a file you `cat` included, can replace what you copied. |
| `body_kb` | number, KiB | `256` | The largest copy a program may make. 256 holds 192 KiB of text. A larger copy is refused whole, never cut short. |

## [glass]

The window's material and how dark it is. Four of these come in pairs, one
for the dark appearance and one for the light, because the same darkening
that suits a dark desktop is far too much over a light one.

| key | values | default | what it changes |
|---|---|---|---|
| `style` | `"clear"`, `"regular"` | `"clear"` | The glass: `"clear"` is thinner and more see-through, `"regular"` is frosted. |
| `dim_dark` | 0 to 1 | `0.22` | How solid the background is under the dark appearance, from 0 (clear) to 1 (opaque). Turn it up if text is hard to read over your desktop. |
| `dim_light` | 0 to 1 | `0.12` | The same, under the light appearance. |
| `app_backgrounds` | `"dim"`, `"opaque"` | `"dim"` | Whether backgrounds a program paints, such as btop's panels, are see-through like the rest (`"dim"`) or solid (`"opaque"`). |
| `pictures` | `"opaque"`, `"dim"` | `"opaque"` | Whether a picture drawn with text characters, such as a file manager's photo preview, is kept solid so it reads as a picture. |
| `tint_dark` | 0 to 1 | `0.54` | How much the glass itself is darkened under the dark appearance. `0` turns it off. |
| `tint_light` | 0 to 1 | `0.14` | The same, under the light appearance. |
| `hue` | 0 to 1 | `0.60` | The colour of that darkening; `0.60` is a blue. |
| `saturation` | 0 to 1 | `0.80` | How strong that colour is. |
| `shadow` | `"auto"`, `"always"`, `"never"` | `"auto"` | Whether the window casts a shadow. `"auto"` casts one only under the dark appearance. |

On Linux only `dim_dark` has an effect, and `theme = "auto"` means dark.

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
