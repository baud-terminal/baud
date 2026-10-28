# Theme formats: the governing texts (base16, base24, tinted8, "ANSI")

Read-only research for autodetecting ANSI, base16 and base24 theme files (2026-10-16). These notes cover the governing texts first and practice second. Where
tinted-theming's own templates disagree with its specs, the spec wins, and the
disagreement is recorded as the template's departure.

baud's citations are `file:line` at the working tree on 2026-10-16 (master
`c417553d`). "By reading" means the behaviour was traced through the source
and not run.

## 0. Sources, and which are pinned

**Pinned in `docs/research/sources/MANIFEST.toml`**, all group Y. Each one was
re-fetched today from upstream `main`. Its sha256 equals the manifest's, so
upstream has not changed since pinning.

| key | file | upstream commit | sha256 (= manifest) | MANIFEST line |
|---|---|---|---|---|
| `tinted-styling-base16` | `tinted-home-styling.md` | home `38fdf7d` (still HEAD) | `ba027cc4…6975` | 9307 |
| `tinted-builder` | `tinted-home-builder.md` | home `38fdf7d` | `e48dc625…989d` | 9320 |
| `base24-styling` | `tinted-base24-styling.md` | base24 `fb96d3c` (still HEAD) | `f38f3159…5d42` | 9333 |
| `tinted-shell-base16` | `tinted-shell-base16.mustache` | tinted-shell `d1a0085`; HEAD is now `9359c5d`, file unchanged | `e1fc087d…ba65` | 9346 |
| `ctlseqs` | `xterm-ctlseqs.txt` | | | 76 |
| (ECMA-48 5th) | `ecma-48-5th-1991.pdf`, read through `scripts/extract-ecma48.py --section sgr` | | | |
| (xterm man page) | `xterm-xterm.man` | | | |

**UNPINNED.** These were fetched today into
`docs/research/notes/ar/upstream/`.
They have no manifest key, so no `[[source: …]]` may be written for them until
they are pinned (see section 8).

| what | URL @ commit | sha256 |
|---|---|---|
| tinted8 styling spec, **v0.2.0-beta11** | https://github.com/tinted-theming/home/blob/38fdf7d4c74417ea86f1e354c628fc2d2aeb5d96/specs/tinted8/styling.md | `0672b3ae31b57d4152957961d98732749c267aac51f40a20d16173b896bf15fc` |
| tinted8 builder spec | https://github.com/tinted-theming/home/blob/38fdf7d4c74417ea86f1e354c628fc2d2aeb5d96/specs/tinted8/builder.md | `5f80134d743f099ed8a9d4d6c26df3bb836a961582d6a9c9be06e015857ad1d8` |
| tinted-shell `base24.mustache` | https://github.com/tinted-theming/tinted-shell/blob/9359c5da5adec80f374a6ef62178ab3e5613c800/templates/base24.mustache | `eea2ab702b0fbb9966f6e73f0e9172318a49b6458e498fe40bcea305e7f81fc0` |
| tinted-shell `tinted8.mustache` | same commit, `templates/tinted8.mustache` | (in the upstream directory) |
| tinted-shell `templates/config.yaml` | same commit | `f62e923bf9500f19e8fe52d0c3c6a053a1e3fed39210f3d9ddcb71ed08dd7784` |
| base24 README | https://github.com/tinted-theming/base24 @ `fb96d3c`, `README.md` | `8456caaa…329e` |
| schemes corpus | https://github.com/tinted-theming/schemes @ `3ac91137e879502c442ab89ba0f1a98077d0da9d` (default branch `spec-0.11`, the only branch) | a shallow clone, not kept |
| original base16 styling v0.2 | https://github.com/chriskempson/base16/blob/39fb23df970d4d6190d000271dec260250986012/styling.md | `68d2d98c…3ac4` |

## 1. base16: the scheme file and its slots

### 1.1 The file format (`tinted-builder` v0.11.2)

The builder spec defines one scheme format for every palette-based system. It
uses RFC 2119 keywords (`tinted-home-builder.md:4-6`).

> Each scheme system MUST specify a way of obtaining the following information
> for a given scheme … (`tinted-home-builder.md:28`)

| key | status (`tinted-home-builder.md`) | text |
|---|---|---|
| `system` | required, with one exception: legacy inference (l.30) | "which system this scheme supports. When loading from a yaml file, if `system` is not provided, the builder MUST use the provided colors in the palette to determine if this is a legacy scheme in either base16 or base24 format depending on which colors are provided." |
| `name` | required (l.31) | "the scheme's human readable name." |
| `slug` | optional (l.32) | "If it is not provided, a builder MUST infer it by slugifying the scheme's name." Slugify is defined at l.180-193. |
| `author` | required (l.33) | |
| `description` | optional (l.34) | |
| `variant` | "optional, but recommended" (l.35) | "Currently only "light" and "dark" are used, but this value could be anything." |
| `palette` | required (l.36) | "all colors used by a scheme, often loaded as HTML hex colors. Some scheme systems may place additional restrictions on the colors in the palette." |

**The common (current, nested) form** is at `tinted-home-builder.md:47-69`:
`system`, `name`, `slug`, `author`, `description` and `variant` at the top
level, then `palette:` with `base00`-`base0F` indented under it. Line 73:

> all color values MUST be in HTML hex format and MAY be preceded by a `#`.

Line 43 adds that files and directories whose names start with `.` "MUST be
ignored".

**The legacy (flat) form** is at `tinted-home-builder.md:78-112`. It is
"deprecated and is included for backwards compatibility reasons" (l.80), and
it is "a fallback meant only for the Base16 and Base24 scheme systems" (l.82).
Lines 107-111:

> - `system` will be inferred to be either `base16` or `base24` depending on which bases are provided.
> - all color values MUST be in HTML hex format and MAY be preceded by a `#`.
> - the `palette` children MUST all be top-level keys. It can be assumed that other than `scheme`, `author`, and `description` are `palette` color values.
> - the scheme name MUST be specified using `scheme`, not `name`.

**The text does not say how strictly a file must hold every slot.** It never
writes "MUST provide all 16". It says only that the palette is "all colors
used by a scheme", and the styling guide defines sixteen. The template
variables are generated "for each defined palette token" (l.163). What a
builder does with a missing token is unstated.

**What the text says about hex forms.**
- Input is "HTML hex format", with an optional `#` (l.73, l.108).
- Output is fixed: `{{ token-name }}-hex` is "6-digit hex color value. This
  does not include a leading `#`" (l.165).
- "HTML hex" is defined nowhere in tinted's text. The WHATWG HTML Living
  Standard (fetched today, not pinned) no longer has a "valid simple colour"
  microsyntax. Its only remaining hex syntax is the *legacy* colour parser,
  §2.3.6 (`#colours`). Step 5 of that parser takes a four-character `#rgb` and
  multiplies each digit by 17.
- So "HTML hex" can be read to include `#rgb`. It cannot be read to include a
  bare `rgb` without the `#`, because §2.3.6 requires the `#` for the
  three-digit form.
- Every slot value in the corpus is `"#rrggbb"` (section 7).
- tinted8, by contrast, is explicit (section 4).

### 1.2 What the slots mean (`tinted-styling-base16` v0.4.2)

Ordering, from `tinted-home-styling.md:12` and `:18-24`:

> In order to create a dark scheme, colors `base00` to `base07` should span
> from dark to light. For a light scheme, these color should span from light
> to dark.

**The ANSI mapping is the guide's own table**, at `tinted-home-styling.md:30-47`:

| slot | ANSI | Terminal column | Text Editor column |
|---|---|---|---|
| base00 | 0 | Black (Background) | Default Background |
| base01 | 18 | (Darkest Gray) | Lighter Background (status bars) |
| base02 | 19 | (Dark Gray) | **Selection Background** |
| base03 | 8 | Bright Black (Gray) | Comments, Invisibles, Line Highlighting |
| base04 | 20 | (Light Gray) | Dark Foreground |
| base05 | 7 | White | **Default Foreground, Caret**, Delimiters, Operators |
| base06 | 21 | (Lighter White) | Light Foreground |
| base07 | 15 | Bright White | The Lightest Foreground |
| base08 | **1 and 9** | Red and Bright Red | |
| base09 | 16 | (Orange) | |
| base0A | **3 and 11** | Yellow and Bright Yellow | |
| base0B | **2 and 10** | Green and Bright Green | |
| base0C | **6 and 14** | Cyan and Bright Cyan | |
| base0D | **4 and 12** | Blue and Bright Blue | |
| base0E | **5 and 13** | Magenta and Bright Magenta | |
| base0F | 17 | (Dark Red or Brown) | |

The notes at `:51-53` qualify the table:

> These are just guidelines … Items in parenthesis in the Terminal column do
> not have an identified terminal use … It can't be assumed that the extended
> ANSI colors are set when using a tinted-theming terminal template theme.

Five consequences follow from the text:

- **base16 has ten distinct ANSI colours, not sixteen.** ANSI 9-14 repeat 1-6.
- **The page and the text are named by the text.** base00 is "Default
  Background" and base05 is "Default Foreground". Those are what OSC 11 and
  OSC 10 carry (section 3).
- **The cursor and the selection are named only in the Text Editor column.**
  base05 is "Caret" and base02 is "Selection Background". The Terminal column
  gives neither, and no selection *foreground* is named at all.
- **In a light scheme, ANSI 0 "Black" is the lightest grey and is the page.**
  This follows from base00 being both "Black (Background)" and the start of a
  light-to-dark ramp. SGR 30 on the default page is then invisible by
  construction. Note Y says the same at
  `docs/research/notes/y-palettes.md:756-759`.
- **base16's variant can be derived from the text.** The ramp direction of
  base00→base07 *is* the variant (l.12). A declared `variant` is a separate,
  optional claim.

The original Chris Kempson v0.2 styling (`39fb23d`, unpinned) has **no ANSI
column at all**. It also calls base07 "Light Background (Not often used)",
where v0.4.2 calls it "The Lightest Foreground". The ANSI table is
tinted-theming's addition, and it is now the text.

## 2. base24 (`base24-styling` v0.1.3)

**Brights.** `tinted-base24-styling.md:45-70` re-tabulates base00-0F. It
changes "1 and 9" style entries to the normal colour alone: base08 = 1,
base0A = 3, base0B = 2, base0C = 6, base0D = 4 and base0E = 5. It then adds:

| slot | ANSI | Terminal |
|---|---|---|
| base10 | - | (Darker Black), "Darker Background" |
| base11 | - | (Darkest Black), "The Darkest Background" |
| base12 | 9 | Bright Red |
| base13 | 11 | Bright Yellow |
| base14 | 10 | Bright Green |
| base15 | 14 | Bright Cyan |
| base16 | 12 | Bright Blue |
| base17 | 13 | Bright Magenta |

The slot order is not the ANSI order. 0x13 is yellow and lands on 11; 0x14 is
green and lands on 10. A reader that maps base12-17 to 9-14 in sequence is
wrong for four of the six.

**Fallbacks** (`tinted-base24-styling.md:9-23`), "a mapping between Base24 and
Base16 color codes":

| base24 | falls back to |
|---|---|
| base10, base11 | base00 |
| base12 | base08 |
| base13 | base0A |
| base14 | base0B |
| base15 | base0C |
| base16 | base0D |
| base17 | base0E |

This table is why base16 and base24 agree: a base16 file read through
base24's mapping with these fallbacks gives exactly base16's own ANSI 9-14
(1-6 repeated). **For a file without base1x slots, choosing between the two
formats changes no colour.** The base24 README's compatibility table (README
l.40-49) states the same design: "Base16 ⇒ Base24" builds with a base24
builder. The fallback table is also **the only text that speaks to a partial
base24 file**, meaning some of base10-17 present and some not. The text does
not say whether such a file is valid, nor whether the fallbacks fill it.

**Light and dark** (`:29-37`). In a dark theme, base10-11 "should span from
light to dark, but still darker than `base00`". In a light theme they should
span "from dark to light, but lighter than `base00`". A brights note at
`:80-85` says bright colours "can have a higher luminosity" than their normal
counterparts, measured by OKHSL L. It is not a hard requirement.

**Format.** base24 uses the builder's scheme format unchanged, with
`system: "base24"` (base24 README l.70 points at the builder spec).

## 3. tinted-shell: tinted-theming's own terminal mapping (practice)

The templates are tinted-theming's reference terminal output. They are
practice, and they agree with the text for base16 and base24.

**base16** (`tinted-shell-base16.mustache`):
- `color00`-`color15` = base00, 08, 0B, 0A, 0D, 0E, 0C, 05, 03, then **09-14
  as copies of 01-06** (`$color01`…), then base07 (l.8-23).
- 16-21 = base09, 0F, 01, 02, 04, 06 (l.24-29).
- These go out as `OSC 4;N;rgb:rr/gg/bb ST` for N = 0-21 (l.53, l.58-82).
- `OSC 10` = base05 and `OSC 11` = base00 (l.95-97). `OSC 11` can be skipped
  with `BASE16_SHELL_SET_BACKGROUND=false`.
- The cursor gets `put_template_custom 12 ";7"`, commented "cursor (reverse
  video)" (l.102). No colour is sent for it.
- **Only on iTerm2** (l.85-93) does it send cursor = base05, cursor text =
  base00, selection = base02, selected text = base05 and bold = base05,
  through iTerm2's proprietary `OSC P`.

**base24** (`base24.mustache` @ `9359c5d`, unpinned) differs only at l.17-22:
9 = base12, 10 = base14, 11 = base13, 12 = base16, 13 = base17, 14 = base15.
That is the styling table exactly. Its fg, bg, cursor and iTerm2 branch
(l.85-104) are base16's.

**tinted8** (`tinted8.mustache` @ `9359c5d`, unpinned):
- 0-7 = `palette.{black…white}.normal` and 8-15 = `.bright`.
- fg and bg are `ui.global.foreground.normal` and `ui.global.background.normal`.
- **This departs from the current tinted8 text.** The spec's keys are
  `ui.global.normal.foreground` and `ui.global.normal.background` (styling
  l.323-324, builder l.532-533). They were renamed by home commit "tinted8:
  Standardise ui.* properties to end in background/foreground" (2026-08-12).
  The template still uses the older order. The text wins.

Its `config.yaml` pins `tinted8-builder` and `tinted8-styling` to `"~0.2.0"`
(l.9-14).

## 4. tinted8 (exists; v0.2.0-beta11, in `home/specs/tinted8/`)

vogix is set aside until its beta. tinted8 is a separate system:
tinted-theming's own eight-anchor format, with **11 schemes in the corpus**.

**File structure** (styling l.31-52, example l.59-87):

```yaml
scheme:
  system: "tinted8"
  supports:
    styling-spec: "0.2.0"
  author: "…"
  name: "…"          # at least one of name / slug / family
variant: "dark"       # top level, required
palette:
  black: "#131721"    # ANSI 0
  red: …              # 1
  green: … yellow: … blue: … magenta: … cyan: …
  white: "#e6e1cf"    # ANSI 7
syntax: …             # optional
ui: …                 # optional
```

- **Required** (styling l.407-410): `scheme.system`,
  `scheme.supports.styling-spec`, `scheme.author`, `variant`, one of
  `scheme.name|slug|family`, and `palette` with all eight colours.
- **`variant` must be "exactly `dark` or `light`"** (l.411).
- **Hex is `#RRGGBB`** (styling l.413). The builder spec, inputs section
  (l.24-26): "All color values must be HTML-style hex (`#RRGGBB`). Builders
  must accept both uppercase and lowercase hex digits on input".

**ANSI mapping** (styling l.93-134):
- 0-7 = black, red, green, yellow, blue, magenta, cyan, white.
- 8-15 = `black-bright` … `white-bright`, written as flat hyphenated keys.
- Brights are **derived when absent**, from the HSL formulas at builder
  l.171-198: ΔL = 0.12 added to L, with saturation ×1.08, ×1.00 or ×0.90 by L.
- `-dim` variants, gray, orange and brown are also derived, and have no ANSI
  number.

**Polarity is a different convention from base16's.**
- `palette.black` is "The darkest anchor color. Background in dark schemes,
  text in light schemes" (l.95). `white` is the mirror (l.102).
- So in a light tinted8 scheme, ANSI 0 stays dark. In a light base16 scheme,
  ANSI 0 is the light page.

**The page, text, cursor and selection come from `ui.*` or from builder
defaults that mirror on `variant`** (builder l.370-380, table l.520-556):

| key | dark | light |
|---|---|---|
| `ui.global.normal.background` (page) | black-normal | white-normal |
| `ui.global.normal.foreground` (text) | white-normal | black-normal |
| `ui.cursor.normal.background` | white-normal | black-normal |
| `ui.cursor.normal.foreground` | black-normal | white-normal |
| `ui.selection.background` | black-bright | white-dim |
| `ui.selection.foreground` | white-normal | black-normal |

**This conflicts with a recorded baud decision.** Note Y's type says
"**Polarity is derived, never read** … A declared polarity (tinted `variant`)
is checked against it" (`docs/research/notes/y-palettes.md:409-411`). For
tinted8 the declared `variant` is *input*. It decides which anchor is the
page. A tinted8 reader cannot derive polarity before it knows the page, and
it cannot know the page without the variant. The decision was written for
base16, base24 and ghostty, where page and text are given directly. It has to
be reopened for tinted8, or tinted8's own rule recorded as a scoped exception.

**Status.** The version is `0.2.0-beta11`. The latest home commit, of
2026-08-22, is "tinted8: 0.2.0-beta11 (#181) - Prepare for a 0.2.0 release".
It is a beta spec.

## 5. What "ANSI" means as a theme format

**No governing text defines an "ANSI theme file", and none assigns RGB values
to the ANSI colours.** What the texts give is a *data model*. Each concrete
file syntax (ghostty `palette = N=`, kitty `colorN`, Alacritty TOML, Windows
Terminal JSON, `.itermcolors`) is practice.

| element | governing text | what it says |
|---|---|---|
| 8 colours, by name only | ECMA-48 5th §8.3.117 (pinned PDF, via `scripts/extract-ecma48.py --section sgr`) | 30-37 "black/red/green/yellow/blue/magenta/cyan/white display", 40-47 the backgrounds. No values. |
| the default colours | same | 39 "default display colour (implementation-defined)", 49 "default background colour (implementation-defined)" |
| 8 brights | `xterm-ctlseqs.txt:1251-1271` | "If 16-color support is compiled, the following aixterm controls apply … the aixterm colors are the bright versions of the ISO colors": 90-97 and 100-107 |
| the 16 as palette entries | `xterm-ctlseqs.txt:2040-2047` (OSC 4) | "The color numbers correspond to the ANSI colors 0-7, their bright versions 8-15, and if supported, the remainder of the 88-color or 256-color table." |
| fg, bg, cursor, selection | `xterm-ctlseqs.txt:2115-2124` | OSC 10 "VT100 text foreground", 11 "VT100 text background", 12 "text cursor color", 17 "highlight background", 19 "highlight foreground"; resets at 2256-2265 |
| the same as resources | `xterm-xterm.man` | `color0`-`color7` "the colors for the ISO-6429 extension", defaulting to black, red3, green3, yellow3, a dark blue, magenta3, cyan3 and gray90, "chosen to allow the colors 8–15 to be used as brighter versions" (2912-2940). `background` (2663), `foreground` (4014), `cursorColor` (3233). `highlightColor` (4075): "If not specified (i.e., matching the default foreground), reverse video is used". `highlightTextColor` (4180) likewise. |
| bold colour | `xterm-ctlseqs.txt:2055` (OSC 5, special colours), `colorBD` (`xterm-xterm.man:3005`) | optional |

So an "ANSI" scheme, in the texts' terms, has:
- **16 indexed colours** (OSC 4 0-15), plus
- **foreground and background**, held apart from the 16 (OSC 10/11; ECMA-48
  makes 39/49 implementation-defined, so they are not slots 7 and 0), plus
- optionally **cursor** (OSC 12) and a **selection pair** (OSC 17/19), each
  defaulting to reverse video when unset, as xterm's resources do.

**A cursor *text* colour has no governing text.** xterm has no OSC for it and
draws the cursor "by filling the background of a text cell"
(`xterm-xterm.man:3236-3238`). ghostty's `cursor-text` and iTerm2's `Pm` are
practice. baud's `Scheme` already carries exactly this model:
`ansi[16]`, `page`, `text`, `extended[240]`, `cursor`, `cursor_text`,
`selection` and `selection_text` (`baud-domain/src/themefile.rs:92-111`).

**How each tinted system lands on that model:**

| | 0-15 distinct | page / text | cursor | selection |
|---|---|---|---|---|
| ANSI-style file | 16 | its own bg/fg | its own, if given | its own, if given |
| base16 | 10 (9-14 = 1-6) | base00 / base05 (text) | base05 "Caret" (Text Editor column only) | bg base02 "Selection Background"; fg unnamed |
| base24 | 16 | base00 / base05 | as base16 | as base16 |
| tinted8 | 8 given, 8 derived | `ui.global.normal.*`, else variant-mirrored defaults | `ui.cursor.normal.*`, else defaults | `ui.selection.*`, else defaults |

## 6. What the texts say about detection, and where baud stands

### 6.1 What the text specifies

- **base16 vs base24 is specified, and only for those two.** If `system` is
  present, it decides. If absent, the builder "MUST use the provided colors
  in the palette to determine … base16 or base24" (`tinted-home-builder.md:30`,
  `:107`).
- **Legacy vs current form** is told apart by structure. Legacy has a scalar
  `scheme:` holding the name and the slots at the top level. Current has
  `name:` and a `palette:` mapping (`:47-69` against `:86-104`).
- **tinted8 collides with legacy base16 on the key `scheme`.**
  - In legacy base16/24, `scheme:` is a **scalar** (the name).
  - In tinted8, `scheme:` is a **mapping** that holds `system: "tinted8"`.
  - Both put `variant` and `palette` at the top level.
  - A detector must tell a scalar `scheme:` from a mapping `scheme:`. The
    discriminator the text gives is `scheme.system`.
- **base16/base24/tinted8 against an ANSI-style file has no text at all.** No
  spec defines a detector across families. tinted's files are YAML. The
  ANSI-style syntaxes are each their own grammar (ghostty `key = value`, kitty
  `key value`, TOML, JSON, plist). Telling them apart is a decision baud
  makes, not one any text makes.
- **Variant.** For base16 and base24 it is optional and "could be anything".
  The ramp direction is derivable from the slots. For tinted8 it is required,
  exactly `dark|light`, and it changes the colours (section 4).

### 6.2 baud today, against the text

- **The reader is chosen by file extension, not content.** `.yaml`/`.yml` in
  any case go to base16/base24, and everything else goes to ghostty
  (`baud-app/src/palette.rs:140-146`, used at `:195-205`). An extensionless
  base16 file goes to the ghostty reader.
- **base16 vs base24 follows the text.** `system` decides if present
  (`baud-domain/src/themefile.rs:227-237`; any other value is
  `UnknownSystem`). Otherwise base24 is chosen when any of base10-17 is
  present (`:254-260`), then all 16 or 24 slots are required (`:261-274`).
- **A tinted8 file is misreported (by reading).**
  - `scheme:` with an empty value at indent 0 is not `palette`, so it is
    ignored.
  - Its nested `system: "tinted8"` is skipped as "a nested map baud does not
    read" (`:207-212`).
  - `variant` is read.
  - `palette:` opens, but `black`, `red` and the rest are not slot names
    (`slot_of`, `:595-603`), so nothing is stored.
  - With no `system`, inference gives base16 (`:256-260`), and the file is
    refused as `Missing(Slot(0))` at its last line (`:269-273`).
  - The complaint names a missing `base00` instead of saying tinted8 is not
    read.
- **A partial base24 file is refused** (`:261-274`). The text leaves this
  open. base24's fallback table (section 2) is a reading under which it would
  load.
- **A file that says `system: base16` and also carries base1x slots** is read
  as base16. Its base1x values are parsed into `slots` and never used
  (`:213-220`, `:277-281`). The text says nothing about this case.
- **base16/base24 cursor and selection are left unset**
  (`themefile.rs:308-311`). The text names base05 as caret and base02 as
  selection background, but only in the Text Editor column. Whether that
  column binds a terminal is a reading. tinted-shell applies it only on
  iTerm2 (section 3). Note Y defers cursor and selection to V13 in any case
  (`y-palettes.md:415-418`).
- **Hex.**
  - `hex()` takes `#rrggbb`, `#rgb`, and either without `#`
    (`themefile.rs:43-68`).
  - `#rgb` ×17 matches WHATWG §2.3.6.
  - Bare `rgb` without `#` is outside both §2.3.6 and tinted8's `#RRGGBB`.
    tinted8 itself has no reader in baud.
  - The doc comment's "each digit doubled, as HTML hex has it" holds for the
    `#` form only.
- **Entries 16-21** are parsed and not written (`themefile.rs:18-19`,
  `:303-307`). This is consistent with styling note l.53: the extended
  colours cannot be assumed set.

## 7. The corpus (practice, measured today)

tinted-theming/schemes @ `3ac9113`, counting `.yaml`/`.yml` only:

| system | files | `.yml` | without `system:` | with `variant` | light | slot values not `"#rrggbb"` |
|---|---|---|---|---|---|---|
| base16 | 361 | 1 (`cyberpunk.yml`) | 0 | 361 | 106 | 0 |
| base24 | 210 | 0 | 0 | 210 | 34 | 0 |
| tinted8 | 11 | 0 | 0 | 11 | 4 | 0 palette values |

- Every base24 file has all 24 slots. No base16 file has a base1x slot. No
  slot key is lower-case (`base0a`).
- One base16 file writes `system: base16` unquoted. The other 570 base16 and
  base24 files quote it.
- Note Y counted 352 base16 (102 light) and 209 base24 (34 light) at
  `50f6e3b` (`y-palettes.md:340-343`). The corpus has grown since by 9 base16
  files and 1 base24 file.
- **Legacy flat files and system-less files are absent from tinted's corpus.**
  Detection by slot inference matters only for files from elsewhere: old
  `base16-schemes` copies, and wezterm, whose loader reads only the legacy
  form (`y-palettes.md:344-346`).

## 8. Open points and pinning candidates

**Pinning candidates**, so that later notes can cite them:
- tinted8 styling and builder at home `38fdf7d`
- tinted-shell `base24.mustache` and `tinted8.mustache` at `9359c5d`
- the base24 README at `fb96d3c`

All are MIT (`LICENSES/tinted-*-MIT.txt` already cover home, base24 and
tinted-shell). Optionally also pin WHATWG HTML §2.3.6, if the `#rgb` reading
is to be cited.

**Readings the text leaves open** (stated, not decided):
1. Whether a base24 file missing some base1x slots loads through the fallback
   table or is refused. baud refuses it today.
2. Whether "HTML hex" admits `#rgb`. The corpus never uses it. tinted8 says
   `#RRGGBB` only.
3. Whether base16's Text Editor column ("Caret" base05, "Selection
   Background" base02) supplies a terminal's cursor and selection.
4. How a tinted8 reader squares with "polarity derived, never read", given
   that the variant picks the page.
5. What content signal tells YAML (tinted) from ghostty, kitty and the other
   ANSI-style syntaxes. No text covers it, so it is baud's design choice.
   Section 6.1 gives the in-family discriminators that *are* textual:
   `system`, `scheme.system`, scalar against mapping `scheme`, and base1x
   presence.

**Future systems.**
- base17 (named in the base24 README l.34, l.66) is **archived**. Its last
  push was 2023-12-17.
- tinted8 is the live successor inside tinted-theming.
- BaseNext is listed as "DRAFT" in the base24 README (l.34). Its status was
  not checked.
