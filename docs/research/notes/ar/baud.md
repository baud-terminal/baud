# baud today: theme files, the 16/256 mapping, and the contrast floor

Read-only research for autodetecting ANSI, base16 and base24 theme files (2026-10-16). Read at HEAD `c417553d`. The last commit that touched the
files below is `aea1a4aa`. Nothing in the repository was edited:
`git status --short` still shows only the two entries that were there at the start.

Labels: **code** means read in baud's source at the line given. **probe**
means run against baud's real readers by the scratch program described at the
end. **spec** means quoted from a pinned source in
`docs/research/sources/`. **inference** means a deduction that was not run.

---

## 0. The short answer

- **base16 vs base24 is already detected from content, and the spec requires
  it.** Inside the YAML reader, a file with no `system` key is base24 if any
  `base10`-`base17` slot is present, and base16 otherwise
  (`baud-domain/src/themefile.rs:256-260`). Spec: tinted builder.md:30, "if
  `system` is not provided, the builder MUST use the provided colors in the
  palette to determine if this is a legacy scheme in either base16 or base24
  format depending on which colors are provided."
- **The ANSI vs YAML choice is made by file extension only.** `.yaml` and
  `.yml` (any case) go to the base16/base24 reader. Everything else, including
  no extension, goes to the ghostty reader (`baud-app/src/palette.rs:140-146`,
  `:195-205`). No code looks at the content to choose the reader.
- **The two readers fail in opposite ways.**
  - The YAML reader is strict: one bad line refuses the whole file, and baud's
    own half stays.
  - The ghostty reader is lenient: any line it does not know is "ignored" with
    a notice, and the file still loads.
  - So a file in another 16-colour spelling (kitty, Xresources, foot,
    alacritty, a plain list of hex values, or base16 YAML without an
    extension) usually **loads as an empty or partial scheme rather than
    failing**.
  - One spelling **applies the wrong colours**: alacritty TOML. Section
    headers are ignored and the last `background =` wins, so the probe's page
    became the search-match yellow `#ffff00`.
- **The contrast floor (`[palette] floor`, default 1.2) lifts ANSI-indexed
  text that a program paints, whenever the page is the default or an index
  0-15, or the text is the default.** Under base16's published mapping, three
  kinds of pair are exactly 1.00:1 by construction, and all of them are lifted
  to 4.5:1:
  - SGR 30 on the default page;
  - default text on SGR 47;
  - every bright on its own normal, for example SGR 91 on SGR 41.
- **OSC 4/10/11 replies are not floored.** Only the pixel moves.

---

## 1. Where the keys live and how a value becomes a path

**The config (code).**
- `config.toml.example:128-151` documents the `[palette]` section:
  - `floor = 1.2`;
  - `light` and `dark` name a theme file for each half;
  - `theme = "auto" | "dark" | "light"` (`:25-34`) chooses which half is
    drawn.
  - The documented routing rule (`:137-139`): "A `.yaml` or `.yml` file is
    read as a base16 or base24 scheme (tinted-theming), anything else as a
    ghostty theme."
- `baud-app/src/config.rs:744-750` stores `light` and `dark` as strings, with
  surrounding `"` trimmed. Reading the files is not done here.
- `config.rs:1136-1158` parses `floor`:
  - `"off"` turns it off;
  - a number from 1 to 21 becomes `Floor::Lift { trigger: t, target:
    t.max(4.5) }`;
  - anything else keeps the default and adds a complaint.

**Resolving a value to candidate paths**: `palette.rs:102-138`, `candidates`.
- **A value containing `/` is a path.**
  - `~/` expands from `HOME`. With no `HOME`, there are no candidates.
  - A relative path is resolved beside the config file in force.
  - An absolute path is used as given.
- **A bare name** is looked up in three directories, each tried once even if
  two of them are the same:
  1. `themes/` beside the config file in force;
  2. `$XDG_CONFIG_HOME/baud/themes`;
  3. `~/.config/baud/themes`.

  In each directory the name is tried **as given first**, then with `.yaml`,
  then with `.yml`. So `night` beats `night.yaml` in the same directory, and
  the reader then follows whichever file was found (`palette.rs:8-12`).
- **First match wins.** `load` (`palette.rs:150-219`) reads the first
  candidate that exists. A candidate that exists but cannot be read is
  reported as such and is not skipped.

**When the files are read (code).** Both halves are read once at launch,
- on macOS at `baud-app/src/app.rs:772-779`,
- on Linux at `baud-app/src/linux.rs:162-169`.

An appearance flip swaps between two themes already built and reads nothing
(`app.rs:770-771`). `half_theme` (`palette.rs:267-285`) builds each half on
both drivers: the built-in half, then `take_scheme` over it, then the half's
dim, `app_backgrounds`, `pictures` and floor.

**Where complaints go (code).** Every complaint goes to a list that is
printed with `crate::warn!` (`app.rs:776-778`). `warn!` is `eprintln!`
(`baud-app/src/log.rs:82-84`). UNVERIFIED: whether a user who launches the
`.app` bundle ever sees these. Only "stderr" is established.

---

## 2. The base16/base24 reader: what it accepts

Code: `themefile::base16`, `baud-domain/src/themefile.rs:159-315`.

**The YAML subset it reads** (doc comment `:161-166`):
- `key: value` lines;
- values double-quoted, single-quoted or bare;
- `#` comments on their own line, or after a value when the `#` begins the
  value or follows whitespace (`yaml_value`, `:321-336`);
- `palette:` opening the indented slots;
- block scalars (`description: >`) skipped whole;
- `---` skipped.

It accepts both the current nested form and the legacy flat form, where the
slots sit at the top level. Spec: builder.md:106-111, "`system` will be
inferred ... the `palette` children MUST all be top-level keys."

**Slots.** `base00` to `base17`; the two hex digits may be either case
(`slot_of`, `:596-603`). A number past `0x17` (for example `base18`) is not a
slot and is ignored.

**Colour values.** `hex` (`:45-68`) accepts `#rrggbb`, `#rgb`, or either
without the `#`. Anything else is refused with `HexError`: names, `rgb:`
forms, and 8-digit alpha. Spec, builder.md:73: "all color values MUST be in
HTML hex format and MAY be preceded by a `#`."

**Keys it reads besides slots.**
- `system` must be `base16` or `base24`; anything else is refused with
  `UnknownSystem` (`:227-237`). So `tinted8` is refused (probe).
- `variant: light|dark` is recorded as `declared`. Any other value declares
  nothing (`:238-250`). Spec, builder.md:35: "this value could be anything."
- Every other key is ignored, in either form.

**Refusals: one error, at its line, and nothing is loaded**
(`SchemeErrorKind`, `:136-153`):
- a line without `:` (`Syntax`);
- a tab in the indentation (`Syntax`);
- an unclosed quote, or text after a closing quote (`Syntax`);
- a duplicate key (`Duplicate`);
- a bad hex value (`Hex`);
- a required slot missing (`Missing`), reported at the file's last line;
- an unknown `system` (`UnknownSystem`).

**Deciding the format (`:256-261`).**
- An explicit `system` decides, even over the slots present.
- Without `system`, any of `base10`-`base17` present means base24.
- base16 requires all 16 slots, and base24 requires all 24.
- There is no fallback fill. base24 styling.md:9-23 publishes "Base24
  Fallbacks" (base10/11 to base00, base12 to base08, and so on) "for
  reference", but baud does not use them. A `system: "base24"` file missing
  `base10` is refused (test `an_explicit_system_decides_and_a_repeat_is_refused`,
  `baud-domain/tests/themefile.rs:582-607`).

### How base16 and base24 land on baud's 16 slots and 256 entries

Code: `themefile.rs:275-314`, cross-checked against the spec tables (spec:
`tinted-home-styling.md:30-47` v0.4.2 and `tinted-base24-styling.md:45-70`
v0.1.3). The code matches both tables.

| ANSI | base16 (styling.md) | base24 (base24 styling.md) |
|---|---|---|
| 0 | base00 "Black (Background)" | base00 |
| 1-6 | base08, 0B, 0A, 0D, 0E, 0C | same |
| 7 | base05 "White" | base05 |
| 8 | base03 "Bright Black (Gray)" | base03 |
| 9-14 | **the same as 1-6** ("base08 1 and 9", ...) | base12 (9), base14 (10), base13 (11), base16 (12), base17 (13), base15 (14) |
| 15 | base07 | base07 |
| 16-21 | base09, 0F, 01, 02, 04, 06 | same |
| none | none | base10, base11 ("-") |
| page / text | base00 / base05 | base00 / base05 |

- **The page and text are held apart from the slots** (`Scheme::page`,
  `Scheme::text`; `:96-98`). For base16 and base24 they are base00 and
  base05, which tinted-shell's OSC 11 and OSC 10 also use (note Y section 4).
  So in these two formats **ANSI 0 is the page colour and ANSI 7 is the text
  colour, by construction.**
- **Entries 16-21** are parsed into `Scheme::extended` and never written to
  the palette (`take_scheme`, `baud-render/src/color.rs:347-366`; test
  `entries_past_fifteen_are_never_written`,
  `baud-render/tests/scheme_theme.rs:77-85`). Entries 16-255 stay xterm's
  cube and grey ramp (`xterm_palette`, `color.rs:590-623`). Under the light
  half, entries 0-15 start as Modus Operandi's (`light_palette`, `:580-587`)
  before the file is laid over them.
- **No notice is given for a YAML file's 16-21.** The "entries 16-255 are not
  used" notice is for ghostty files only (`palette.rs:257-261`).

---

## 3. The ghostty reader: what it accepts

Code: `themefile::ghostty`, `themefile.rs:338-421`. It follows ghostty
d9840f3's own line parser.

**Lines and comments** (spec, `ghostty-d9840f3-args.zig:1450-1482`):
- a line is trimmed of space, tab and CR;
- it is a comment only when its first byte is `#`;
- `key = value`, both sides trimmed, with one pair of **double** quotes taken
  off the value (`unquote`, `:434-439`, double quotes only).

**The seven keys it reads (`:381-412`).**
- `background` (the page) and `foreground` (the text);
- `cursor-color` and `cursor-text`;
- `selection-background` and `selection-foreground`;
- `palette = N=COLOR`, where N is 0-255 parsed as Zig's
  `parseInt(u8, _, 0)`: prefixes `0x`, `0b` and `0o`, `_` separators, and a
  sign. Spec Config.zig:780-798: "N is 0 to 255 ... decimal, binary, octal, or
  hexadecimal".

**Every other key is passed to `ignored(line, key)` and the file still
loads.** `palette.rs:198-204` turns each one into "line N: KEY is not a colour
and is ignored". A line with no `=` passes its whole trimmed text as the
"key" (probe).

**Order matters.** Lines apply in order: a repeated key takes its last value,
an empty value unsets one entry, and `palette =` alone unsets all 256
(`:345-347`, `:388-394`).

**Colours.** Every `RGB.parse` form except X11 names (`colour`, `:542-593`):
- `#` followed by 3, 6, 9 or 12 digits;
- bare 3 or 6 digits;
- `rgb:` and `rgbi:`.

An X11 name refuses the whole file with `ColourName` (`:567-571`).

**Defaults: an empty scheme is a valid result.** Nothing is required. Every
field starts as `None` (`:353-364`), and a scheme that set nothing is
`Ok(...)`.

**How a ghostty file lands on the palette** (`take_scheme`):
- `palette = 0..15` go to slots 0-15;
- `background` and `foreground` are the page and text, independent of slots 0
  and 7;
- entries past 15 are parsed and dropped, with a notice;
- `cursor-*` and `selection-*` are parsed and **used nowhere**. They wait on
  V13's selection setting (note Y step 7, "Not built"; a grep finds no
  reader).

**What `notice` reports for a file that loaded** (`palette.rs:224-262`).
`take_scheme` returns the count of fields left unset, out of 18 (16 slots,
page, text), and `notice` reports:
- that count, when it is not zero;
- a declared `variant` that the page and text contradict;
- a polarity belonging to the other half;
- ghostty entries past 15.

---

## 4. An unknown, mislabelled or mixed file

**The probe** (section 8) fed each input to **both** readers. The "routed to"
column says which reader the extension rule (`palette.rs:142-146`) actually
picks. The inputs are hand-made shapes in each format's spelling, not files
from a published collection.

| input (file name) | routed to | result | what the user gets |
|---|---|---|---|
| ghostty theme, no ext. (`tests/themes/night`) | ghostty | Ok: 8 slots, page, text; `font-family` ignored | correct |
| the same content saved as `night.yaml` | base16 | **Err `Syntax` line 2** | baud's own half, one complaint |
| base16 YAML, no ext. (`day` copied without `.yaml`) | ghostty | **Ok, all 18 unset**; 21 lines "ignored" | **baud's own palette**, with 21 "is not a colour" lines and "18 of the sixteen, the page and the text are unset" |
| base16 YAML, `system: "tinted8"` | base16 | Err `UnknownSystem` | baud's own half, one complaint |
| kitty `.conf` (`background #1d1f21`, `color0 #...`) | ghostty | **Ok, all unset**; every line "ignored" (no `=`) | baud's own palette, plus notices |
| Xresources (`*.color0: #...`) | ghostty | **Ok, all unset**; every line ignored | baud's own palette, plus notices |
| plain list of 16 `#rrggbb` lines (`.txt`) | ghostty | **Ok, all unset, 0 lines ignored**: every line starts with `#`, so each is a comment | baud's own palette; **the only notice is "18 ... unset"** |
| foot `.ini` (`[colors-dark]`, `background=1d1f21`, `regular0=...`) | ghostty | Ok: **page and text taken**, `regularN` ignored | a page and text with baud's sixteen; in a foot file that has both `[colors-dark]` and `[colors-light]`, the last `background=` wins (inference) |
| alacritty `.toml`, double-quoted, with `[colors.selection]` and `[colors.search.matches]` | ghostty | Ok: **page `#ffff00`, text `#000000`**, taken from `[colors.search.matches]` | **wrong colours drawn.** The only signal is a polarity notice if the result is the other half's |
| alacritty `.toml`, single-quoted (`background = '#1d1f21'`) | ghostty | Err `Hex(NotHex)` line 2 (`unquote` strips `"` only) | baud's own half, one complaint |
| any ghostty file with one X11 colour name | ghostty | Err `ColourName` | the whole file refused |

**What the extension rule gets right.**
- The tinted corpus is all `.yaml` or `.yml`. Note Y section 4 says one file
  in it is `.yml`, and builder.md:43 says it is "valid to walk all yaml files".
- ghostty theme files are named by the theme's name. The doc's example is
  `light:Rose Pine Dawn,dark:Rose Pine` (Config.zig:586-591). Not checked on
  the tarball here: that all 602 bundled files have no extension.
- So the failures come from **renamed files** and from **other spellings of
  the 16-colour idea** (kitty, Xresources, foot, alacritty, Windows Terminal
  JSON, `.itermcolors` plist, a bare list). baud has no reader for any of
  these. They fall into the ghostty reader's "ignore and carry on".

**Signals a content detector could already use, from the readers as they
are** (data only, not a design):
- The YAML reader's error kind and line. A `Syntax` error on the first
  non-comment line means "not YAML-shaped".
- The ghostty reader's `ignored` count against the non-comment line count.
  "Every line ignored" means "not ghostty".
- `Scheme::format`, already `Base16`, `Base24` or `Ghostty`.

**What the tests cover.**
- `palette.rs:420-431` (`the_extension_chooses_the_reader`) pins the routing.
- Neither reader is tested on a kitty, alacritty, foot, Xresources or
  plain-list file.
- No fixture is mislabelled. `tests/themes/README.md:6-12` lists only
  `day.yaml`, `sepia.yml`, `broken.yaml` and `night`.

---

## 5. What reaches the 256-entry palette, and what a program sees

- **The palette.** `take_scheme` (`color.rs:347-366`) writes entries 0-15,
  `bg` and `fg` from the scheme, wherever the scheme set them. Entries 16-255
  are never written.
- **Replies.** `host::tell_defaults` (`baud-app/src/host.rs:315-336`) pushes
  the theme's 256 entries, fg and bg into the core. So `OSC 4;N;?`,
  `OSC 10;?` and `OSC 11;?` answer with the **scheme's unfloored** colours.
  `config.toml.example:133-134`: "What a program asked for is unchanged in
  what the terminal reports; only the pixel moves."
- **Overrides.** A program's OSC 4/10/11/12 overrides win over the theme
  (`Theme::entry`, `color.rs:504-509`; `Theme::defaults`, `:482-500`). Under
  `theme = "auto"` an OS flip drops them (note Y decision 5).
- **Bold.** Bold brightens entries 0-7 to 8-15 (`color.rs:451`). Under base16,
  9-14 copy 1-6, so **bold changes nothing visible for colours 1-6**. Bold
  black (0 to 8) becomes base03.
- **Kitty images.** Landed in `aea1a4aa`: the default background
  is now judged by its drawn colour (`Theme::default_bg`, `color.rs:415-418`).
  "In a theme whose colour 0 equals the background, SGR 40 cells show a deep
  image (kitty the same)", and every base16 or base24 scheme is such a theme.

---

## 6. The contrast floor, on ANSI-indexed colours a program paints with

Code: `Floor`, `baud-render/src/color.rs:86-180`. It is applied once per
rendition in `resolve_slot` (`baud-render/src/renderer.rs:3038-3051`), after
`Theme::resolve` (which applies bold, SGR 7, `?5` and faint) and before alpha.
An explicit `SGR 58` underline colour gets the same test (`:3089-3098`). It
reaches all three render paths through the resolved slot table (note Y
section 8).

**When it fires.** It fires when the pair's WCAG ratio is strictly below the
`trigger` **and** `Floor::covers(text_word, page_word)` (`color.rs:118-122`)
holds. The words are the rendition's colour words, exchanged under SGR 7 and
`?5` the same way the colours were:

| text word | page word | covered? |
|---|---|---|
| any | default (theme page or OSC 11) | yes, unless both are the same non-default word |
| any | index 0-15 | yes, unless both are the same word |
| default | anything | yes |
| index 16-255 or truecolour | index 16-255 or truecolour | **no**: the program chose the pair |
| the same non-default word | the same word | **no** (SGR 31;41 is left alone) |

**What it lifts to.** It mixes the text toward black or white in linear
light, keeping the hue, just far enough to reach `target` = max(4.5,
trigger) (`:123-180`).

**Defaults.** On by default at trigger 1.2 and target 4.5 (`:99-102`).
- `floor = 3` lifts anything below 3:1 to 4.5.
- `floor = 7` lifts to 7.
- `floor = "off"` disables it.
- Note Y decision 9 plans to raise the trigger to 3.0 under macOS Increase
  Contrast (step 8c). That is **unbuilt**: the only `increase_contrast` in
  the tree is in the pinned glass-crate copies under `docs/research/sources/`.

**Consequences specific to ANSI colours (code plus probe):**
- **An index page counts as the terminal's own.** A program that paints both
  sides with indices 0-15 (for example SGR 3x on SGR 4x) is floored whenever
  the pair is below the trigger. A program that **repaints 0-15 through
  OSC 4** is still painting indices below 16, so it is floored too. An
  OSC 11 page counts as the default page (note Y section 8, item 3; test
  `black_text_on_a_black_page_set_by_osc_11_is_lifted`,
  `baud-render/tests/floor.rs:343-376`).
- **Only explicit cube or truecolour pages escape.** That is how chafa
  pictures stay untouched (`floor.rs:317-341`).
- **Under base16, three kinds of pair are exactly 1.00:1 by construction**
  (ANSI 0 = page, ANSI 7 = text, 9-14 = 1-6), and the floor lifts all of
  them:
  - SGR 30 on the default page;
  - default text on SGR 47;
  - every bright on its own normal: SGR 91 on 41, 92 on 42, 31 on 101, and so
    on. The words differ (index 9 against index 1), so the pair is covered.
    SGR 31;41 is the same word and is not lifted.
- **Probe numbers.** These are plain renditions only, with no bold, faint or
  reverse. Every foreground in {default, 0-15} was tried on every background
  in {default, 0-15}: 289 pairs, same-word pairs included and never lifted.
  The ratios were printed with a separate f64 WCAG formula, not baud's
  `LINEAR` table; the lifted colours are baud's own `Floor::lift`.
  - `tests/themes/day.yaml` (a light base16 fixture) as the light half, at
    1.2: **50 pairs lifted**. Examples: SGR 30 on the page,
    `f4f4f0`/`f4f4f0` 1.00, became `70706e` at 4.50. Default text on SGR 47,
    `202020`/`202020`, became `878787` at 4.54. SGR 91 on 41 became `d4b7b7`
    at 4.51.
  - A hand-typed dark base16 scheme (`in/hm-dark.yaml`; shaped like tinted's
    `default-dark` but **not** diffed against the corpus) as the dark half,
    at 1.2: **52 lifted**. SGR 30 on the page, `181818`, became `818181` at
    4.56. Default text on SGR 47, `d8d8d8`, became `5e5e5e`. Default text on
    SGR 43 (`f7ca88`, 1.07) became `5a5a5a`.
  - The same scheme at `floor = 3`: **194 of 289 lifted**. For example, every
    colour on SGR 46 or 47 and on bright white (SGR 107) is pushed dark.
- **What a TUI theme would see** (inference, not measured on herdr, which is
  the herdr report's job):
  - a theme that paints "black" text or "white" panels by index on a base16
    or base24 page gets those cells **recoloured** to a 4.5:1 grey or tint;
  - the same theme painted in truecolour on a truecolour page is untouched;
  - truecolour **text on the default page** is still covered, and is lifted
    if it is below the trigger.

  So, with the floor on, the same theme can look different depending on
  whether it paints by index or by RGB, and on whether its page is the
  default.

---

## 7. Rules already recorded that bind a change here

- **Each format's published mapping** (note Y decision 2): "Each format's own published mapping ... plus a narrow floor"
  (`docs/research/notes/y-palettes.md:132-215`, item 4 of section 1), with a type that can carry base24 or later formats. `Scheme` is that type (`themefile.rs:90-122`).
- **The 2026-09-25 request, before the readers were built**: research the formats there are (base16, base24, ANSI and others), noting that most carry sixteen colours and base24 more (y-palettes.md:69-71, :82-85).
- **vogix is out of scope** until vogix 1.0.0-beta.1 (2026-09-29,
  y-palettes.md:16-21 and section 15).
  - Its design at `c79140a` is 16 slots in ANSI order named by role: 0
    background, 1 danger, 2 success, 3 warning, 4 link, 5 highlight, 6 active,
    7 foreground, and 8-15 the brights.
  - Its importer places base16 by greys and is guarded by 4.5:1, which is
    **not** tinted's table (memory `vogix16-ansi-mapping`).
  - If vogix returns, its reader would take page = slot 0 and text = slot 7
    onto the same `Scheme` (y-palettes.md:1582-1585).
  - What vogix calls an "ansi16 file" is the ghostty/kitty-style file with
    its own `background` (y-palettes.md:1541-1550).
- **Polarity is derived, never read.** `variant` is checked against the
  page-versus-text WCAG rule and reported, never used in its place
  (`Scheme::polarity`, `themefile.rs:113-122`; note Y decision 5).
- **Standard first**. For this topic the governing texts in the manifest are:
  - tinted styling.md v0.4.2 (`tinted-styling-base16`);
  - base24 styling.md 0.1.3 (`base24-styling`);
  - builder.md (`tinted-builder`);
  - ghostty's args.zig, Config.zig and color.zig at d9840f3.

  All are `status = "retrieved"`. No pinned source specifies kitty,
  alacritty, foot, Xresources or Windows Terminal scheme files.

---

## 8. Reproducing the probe

The probe is a separate cargo package with path dependencies on
`baud-domain` and `baud-render`. Its own `target/` is outside the
repository, so nothing is written to the baud tree.

```
P=docs/research/notes/ar/probe
cd $P && CARGO_TARGET_DIR=$P/target cargo build --release
$P/target/release/probe in/*                          # both readers on every input
$P/target/release/floor in/day-noext light            # floor sweep, trigger 1.2
$P/target/release/floor in/hm-dark.yaml dark 3.0      # trigger 3.0
```

- Sources: `$P/src/main.rs` (the readers) and `$P/src/floor.rs` (the floor
  sweep).
- Inputs: `$P/in/` holds `Xresources`, `alacritty.toml`,
  `alacritty-single.toml`, `day-noext`, `foot.ini`, `hm-dark.yaml`,
  `kitty.conf`, `night.yaml`, `plain16.txt` and `tinted8.yaml`.
- Built with the devenv nightly: rustc 1.96.0-nightly 2026-04-06.
