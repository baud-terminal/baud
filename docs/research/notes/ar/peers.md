# Theme files in peer terminals: how each one reads them, and whether any picks the format from the content

Research for autodetecting ANSI, base16 and base24 theme files (2026-10-16). This is read-only
research. Nothing in the repo was changed. It lives in the scratchpad, so if a
decision ends up resting on it, it has to move to `docs/research/notes/` in the
same session (a reboot wipes /private/tmp).

Provenance: **VERIFIED** means the file is pinned in
`docs/research/sources/MANIFEST.toml`, and the key is given. **UNPINNED** means
it was fetched today, 2026-10-16, at the commit named. Copies are under
`.../scratchpad/theme-formats/dl/`. Line numbers were re-checked with `grep -n`.
This note does not repeat note Y's light/dark rules, licences or corpus counts:
see `docs/research/notes/y-palettes.md:316-422`.

## 0. Answer first

1. **No terminal chooses a theme format by reading the file's content.** Each
   one chooses by the file's suffix, by where the file is, or by which API
   function the user called:
   - kitty only lists `*.conf` files.
   - wezterm only lists `*.toml` files in its colour-scheme directories.
   - iTerm2 only imports `.itermcolors`.
   - alacritty's `migrate` refuses a file that has no extension.
   - ghostty and foot each have one syntax, so there is nothing to choose.
   - Windows Terminal has no import feature at all.
2. **Choosing by content does exist, but only between variants of one
   family.** Three cases:
   - **tinted's own spec requires it.** If `system` is absent, a builder "MUST
     use the provided colors in the palette to determine if this is a legacy
     scheme in either base16 or base24 format" (`builder.md:30`, `:108`).
     baud already does this (`baud-domain/src/themefile.rs:254-260`).
   - **tinted's reference builder** detects tinted8 from whether a nested
     `scheme.system` key exists.
   - **iTerm2** treats a preset as a light/dark pair if the key
     `Foreground Color (Light)` is present.
3. **One tool tries every parser in turn.** That is ColorTool, in
   microsoft/terminal's `src/tools`. The first parser that succeeds wins, and
   .NET does not document the order the parsers are tried in.
4. **Only wezterm reads base16 YAML directly**, through the Lua call
   `wezterm.color.load_base16_scheme`. It reads only the legacy flat form,
   only base16, and requires `author`. No peer reads base24 YAML or tinted8
   directly. Every other peer gets base16/base24 through files that tinted's
   templates generate.
5. **The strongest evidence for detecting by content: a `.yaml`/`.yml` suffix
   does not mean base16.** Two large collections use `.yml` with a different
   key set, `color_01` to `color_16`, where `color_01` is ANSI 0:
   - iTerm2-Color-Schemes: 357 files.
   - Gogh: at least 1000 files.

   baud's current routing (`baud-app/src/palette.rs:142-146`) sends every one
   of them to the base16 reader. See section 9.

## 1. ghostty

- **Syntax.** A theme file "is simply another Ghostty configuration file. They
  share the same syntax and same configuration options. A theme can set any
  valid configuration option" (`ghostty-config-d9840f3`, Config.zig:571-573,
  VERIFIED).
  - Colours: `palette = N=COLOR` for N from 0 to 255 (782-798). Also
    `background`, `foreground`, `selection-foreground` and
    `selection-background` (711-712), `cursor-color` (855) and
    `cursor-text` (906).
  - Since 1.2.0 a colour value can also be `cell-foreground` or
    `cell-background` (708-710).
  - Entries 16-255 are generated from 0-15 only if `palette-generate` is on,
    and it defaults to false (822).
- **Lookup** (Config.zig:549-569):
  - An absolute path is loaded directly, and "no other directories will be
    searched".
  - Otherwise ghostty looks in `$XDG_CONFIG_HOME/ghostty/themes` or
    `~/.config/ghostty/themes`, then in the resources directory's `themes`.
    The match is case-sensitive, and a relative name may not contain a path
    separator.
  - A light/dark pair is written `light:X,dark:Y` (586-591).
  - This doc block is unchanged at HEAD `e3ebb1d` (Config.zig:548-601,
    UNPINNED, diffed). `src/config/theme.zig:114-123` sends absolute names
    down a separate path.
- **Format detection: none.** Every file is parsed as config. Theme files
  carry no extension: tinted's template writes `themes/ghostty/{{system}}-{{slug}}`
  (tinted-terminal@e64e440 `templates/config.yaml:52-58`, UNPINNED).
- **Base16 directly: no.** A grep for `base16|base24|yaml` in Config.zig and
  theme.zig matched nothing. The positive controls did match: `palette` 198
  times in Config.zig, and `theme` in theme.zig.

## 2. kitty

- **Syntax.**
  - A line is `name value`, separated by whitespace.
  - A comment counts only if `#` "is the first character in the line"
    (`kitty-conf`, line 1286, VERIFIED).
  - Colours: `foreground` and `background`, then `color0` to `color255`.
    `color0` is `#000000` (`kitty-options-definition`@1e1486e,
    definition.py:2470, 2688, VERIFIED). Also `cursor` (353),
    `cursor_text_color` (369), `selection_foreground` (2641) and
    `selection_background` (2655).
  - Other files are pulled in with `include` (`kitty-conf`, line 1290).
  - The `kitty-conf` line numbers are lines of the HTML copy hashed as
    `kitty-conf`. That copy can no longer be fetched from its URL, so the
    VERIFIED mark can't be checked again against the exact bytes.
- **Themes kitten** (kitty@b7a657d, UNPINNED):
  - "A theme in kitty is just a `.conf` file containing kitty settings." The
    kitten copies the chosen file to `current-theme.conf` and `include`s it
    (`docs/kittens/themes.rst:37-41`).
  - Light/dark switching uses three files: `dark-theme.auto.conf`,
    `light-theme.auto.conf` and `no-preference-theme.auto.conf` (69-71).
  - Your own themes go in `~/.config/kitty/themes` (95-97).
  - The directory scan accepts only `strings.HasSuffix(e.Name(), ".conf")`
    (`tools/themes/collection.go:799`).
- **Content is read, but only for metadata.**
  - `ParseThemeMetadata` (collection.go:475-546) reads `## name:`,
    `## author:`, `## upstream:`, `## blurb:` and `## license:` from the
    leading comment block (493-530).
  - It sets `Is_dark = max(R,G,B) < 115` from `background` (486), defaulting
    to dark (477).
  - This classifies a theme. It does not choose a format.
- **Base16 directly: no.** No match in collection.go or themes.rst. The
  control (`background`/`color0`) did match.

## 3. alacritty

- **Syntax: TOML** (`alacritty-0c405d5-man-5`, alacritty.5.scd, VERIFIED).
  - `[colors.primary]` holds `foreground`, `background`, `dim_foreground` and
    `bright_foreground` (299-325).
  - `[colors.normal]`, `[colors.bright]` and `[colors.dim]` each have keys
    `black` to `white` (407-470).
  - `indexed_colors = [{ index, color }]` covers 16-255 (473-475).
  - `cursor = { text, cursor }`, and `selection = { text, background }`
    (327-334, 398-404). Both accept `CellForeground` and `CellBackground`.
  - Values are "hexadecimal values with a `#` prefix: `#RRGGBB`" (297).
- **Lookup.** There is no themes directory. A theme is a TOML file pulled in
  through `[general] import`, by absolute path, `~/` or relative path (30-47).
  The man page's own example is `"~/.config/alacritty/base16-dark.toml"`
  (44): a base16 scheme already converted to alacritty TOML.
- **Format detection: by extension, and an extension is required.** At
  alacritty@5421470 (UNPINNED), `alacritty migrate` (YAML config to TOML)
  returns `"missing file extension"` for a file without one, and sends `yml`
  to the YAML migrator (`alacritty/src/migrate/mod.rs:67-76`).
- **Base16 directly: no.** The only base16 match is the filename on line 44.

## 4. wezterm

- **Syntax: TOML schemes**, with `[colors]` holding `ansi = [8]`,
  `brights = [8]`, `foreground`, `background`, `cursor_*`, `selection_*` and
  `indexed`, plus `[metadata]` (wezterm@e2a98e1, UNPINNED,
  `config/src/color.rs:130-146`, `689-705`).
- **Lookup.** `color_scheme_dirs`, plus `colors/` under each config directory
  (`config/src/config.rs:1429-1442`). Only names ending in `.toml` are loaded
  (1446-1452), and a scheme name already defined is skipped (1466).
- **Base16 directly: yes, as a separate API call, never by detection.**
  `wezterm.color.load_base16_scheme(file)` is registered next to
  load_scheme (TOML) and load_terminal_sexy_scheme (JSON)
  (`lua-api-crates/color-funcs/src/lib.rs:129-168`). The user picks the
  reader by picking the function.
  - **Legacy flat form only.** The struct requires `scheme`, `author` and
    `base00`-`base0F` at the top level (`schemes/base16.rs:7-28`). The current
    nested `palette:` form fails because `scheme` and `author` are missing. A
    legacy file without `author` also fails, even though tinted marks
    `author` as metadata.
  - **base24 is read silently as base16.** The struct carries no
    `deny_unknown_fields` (a grep for `serde(` found only the derive), so
    `base10`-`base17` are ignored. The brights repeat the normals and
    base24's own brights are lost.
  - Mapping (base16.rs:57-78):
    - ANSI: tinted's table, with 16-21 set to base09, 0F, 01, 02, 04 and 06.
    - Cursor: `cursor_fg = base00`, `cursor_bg = base05`.
    - Selection: `selection_bg = base05`, `selection_fg = base00`.
  - **The doc disagrees with the code.** The doc example prints
    `"cursor_fg": "#8b8198"`, which is base05
    (`docs/config/lua/wezterm.color/load_base16_scheme.md:68`). The code sets
    `cursor_fg: Some(base_0)` (base16.rs:68).
- **There is an iTerm2 reader, but users cannot call it.**
  `schemes/iterm2.rs` and `gogh.rs` exist (`schemes/mod.rs`), but
  `grep -i iterm` on lib.rs finds nothing. They are used only by the
  `sync-color-schemes` tool (`sync-color-schemes/src/main.rs:14`, `428`).
  The iTerm2 reader takes `Red/Green/Blue Component` and has no
  `Color Space` field (iterm2.rs:7-25), so a P3 file is read as if it were
  sRGB.
- **The sync tool's base16 import** (`sync-color-schemes/src/base16.rs`):
  1. It walks a tarball and filters by extension, `yaml` or `yml` (54-60).
  2. It tries `Base16Scheme::load_file` on each file and keeps only the ones
     that parse (`if let Ok`, 64-77).
  3. It appends ` (base16)` to the name (67).

  This is "the extension, then whatever parses", and failures are dropped
  silently. Its list comes from `chriskempson/base16-schemes-source`
  (10), not from tinted-theming/schemes.

## 5. iTerm2

- **Syntax: an XML plist** with one dict per colour:
  - Keys `Ansi 0 Color` to `Ansi 15 Color`, `Foreground Color`,
    `Background Color`, `Cursor Color`, `Cursor Text Color`,
    `Selection Color`, `Selected Text Color`, `Bold Color`, `Link Color` and
    others.
  - Each dict holds `Red/Green/Blue Component` as floats and an optional
    `Color Space`. The key names are confirmed by ColorTool's reader
    (XmlSchemeParser.cs:38-43) and gen.py (`iterm2cs-gen-py`,
    gen.py:387-416, VERIFIED).
- **Import routing: by extension, at both entry points** (gnachman/iTerm2@9e29604,
  UNPINNED):
  - The open panel allows only `UTType typeWithFilenameExtension:@"itermcolors"`
    (`sources/Settings/ProfilesColorsPreferencesViewController.m:661`).
  - A file opened from Finder goes through
    `[filename hasSuffix:@".itermcolors"]`
    (`sources/AppKit/iTermApplicationDelegate.m:828`).
  - The import itself is just `NSDictionary dictionaryWithContentsOfFile:`
    (`sources/Settings/iTermColorPresets.m:32-34`). Any plist dictionary is
    accepted, and nothing checks that the colour keys are present.
- **Within the format, the light/dark pair is detected from the content:**
  `iTermColorPresetHasModes(preset)` is
  `preset[KEY_FOREGROUND_COLOR COLORS_LIGHT_MODE_SUFFIX] != nil`
  (iTermColorPresets.m:329-331). The suffixes are `@" (Light)"` and
  `@" (Dark)"` (`sources/Settings/Profiles/ITAddressBookMgr.h:141-142`). It
  is the same kind of rule as base16 against base24: whether a key is present
  decides which variant the file is.
- **Base16 directly: no.** Base16 reaches iTerm2 through tinted's
  `.itermcolors` template (tinted-terminal `templates/config.yaml:84-90`).

## 6. foot

- **Syntax: INI** (`foot-2705e36-foot-ini-5`, foot.ini.5.scd, VERIFIED).
  - Sections `[colors-dark]` and `[colors-light]` (1025-1042), with
    `initial-color-theme` choosing between them (384-388).
  - Keys: `foreground`, `background`, `regular0`-`7`, `bright0`-`7`,
    `dim0`-`7` and `0`-`255` (1053-1105). Also `cursor` as two values
    separated by a space (1044-1051) and `selection-foreground` /
    `selection-background` (1151).
  - Values are "RRGGBB format (i.e. plain old 6-digit hex values, without
    prefix)" (1031-1032).
  - Theme files come in through `include`, which needs an absolute or `~/`
    path (110-121).
- **The task's `[colors]` no longer exists.**
  - At 2705e36, `config.c` (`foot-config`, sha256 matches the manifest's
    `bc8c134e...`) has only `"colors-dark"` and `"colors-light"` in its
    section table (3129-3130).
  - An unknown section logs `"invalid section name"` (3280-3284).
  - foot's CHANGELOG at HEAD cb27717 (UNPINNED): 1.26.0 deprecated
    `[colors]`/`[colors2]` (285-335), and 1.28.0 lists them under "Removed"
    (139-147).
  - Both generators already write the new names. tinted writes
    `[colors-{{scheme-variant}}]`
    (`templates/foot-base16.mustache:8`), which gives the invalid
    `[colors-]` for a scheme with no `variant`. iTerm2-Color-Schemes writes
    `[colors-dark]`, or `[colors-light]` with `initial-color-theme=light`
    (`tools/templates/foot.ini`).
- **Format detection: none. Base16 directly: no.** No match in the man page or
  the HEAD config.c. The control (`regular0`/`background`) did match.

## 7. Windows Terminal

- **Syntax: JSON** `schemes` entries in settings.json (microsoft/terminal@9fd41b2,
  UNPINNED, `doc/cascadia/profiles.schema.json:3347-3420`).
  - Keys: `name`, `background`, `foreground`, `cursorColor`,
    `selectionBackground`, `black`, `red`, `green`, `yellow`, `blue`,
    `purple`, `cyan` and `white`, plus `bright*` versions of the eight colour
    names. That is 20 colours.
  - There is no cursor-text colour, no selection-text colour and nothing for
    16-255.
  - `Color` must match `^#[A-Fa-f0-9]{3}(?:[A-Fa-f0-9]{3})?$` (11-16).
  - A light/dark pair is a `SchemePair { light, dark }` of scheme names
    (308-323).
- **Import: none.** The schema's description sends users to another tool:
  "ColorTool is a great tool you can use to create and explore new color
  schemes" (3348).
- **ColorTool** (`src/tools/ColorTool`, same commit) is a console-host tool,
  not something Windows Terminal runs. It is **the only peer found that tries
  parsers by content.**
  - `GetScheme` runs `GetParsers().Select(p => p.ParseScheme(name)).FirstOrDefault(x => x != null)`
    (`SchemeManager.cs:89-94`).
  - The parsers are found by reflection over `Assembly.GetTypes()` (96-101).
    Microsoft's documentation for `Assembly.GetTypes` names no order, so the
    order the parsers are tried in is undocumented.
  - Each parser looks in its own search paths. Those include the bare name
    with no extension (17-43), so a file with no extension goes to every
    parser in turn.
  - The parsers:
    - `.itermcolors` (`XmlSchemeParser.cs:45`): any document that parses as
      XML stops the path search (127-148), and then all 16 colours are
      required (73-84).
    - `.ini` (`IniSchemeParser.cs:21`).
    - `.json` in concfg's format (`JsonParser.cs:18`), not Windows Terminal's.
  - **What this is:** whichever parser succeeds first is the format. A failed
    parse means "try the next one", never "report this error".

## 8. Converters

### iTerm2-Color-Schemes (`tools/gen.py`)

- **Two input formats, each from its own directory:**
  - `schemes/*.itermcolors`, read with `plistlib` (gen.py:367-385, 533).
  - `yaml/*.yml` (433-470, 541).
  - YAML schemes overwrite iTerm schemes of the same name (539).
- **Its YAML is not base16.** The keys are `color_01`-`color_16`, where
  **`color_01` is `Ansi 0 Color`** (390) and `color_16` is `Ansi 15 Color`
  (405). Then `background`, `foreground`, `cursor`, `cursor_text`,
  `selection`, `selection_text`, `bold`, `link` and more (387-416).
- 357 `yaml/*.yml` and 815 `schemes/*.itermcolors` at HEAD c379d7a (tree
  listing, UNPINNED).
- At c379d7a the YAML reader gained `wcag_adjust: false`, which skips the
  1.75:1 contrast pass for one file (HEAD gen.py:443-448, 458-459, 469-470).
  The pinned 99d9701 copy does not have it.
- The output covers more than 40 targets: ghostty, kitty, alacritty,
  wezterm, foot, windowsterminal, Xresources and others (top-level
  directories at c379d7a).

### Gogh

- Gogh-Co/Gogh@2fbe9bb, UNPINNED. `themes/*.yml` use the same `color_01`
  (Black) to `color_16` (Bright White) keys, plus `background`,
  `foreground`, `cursor`, `name`, `author` and `variant`
  (`themes/0x96f.yml:2-29`).
- The contents API lists 1000 `.yml` files, which is that API's cap, so the
  real count is at least 1000.

### tinted-theming

- **Builder spec** (`tinted-builder`, builder.md, VERIFIED):
  - Current form: `system`, `name`, `slug`, `author`, `description`,
    `variant` and nested `palette:` (24-71).
  - Legacy flat form: `scheme`, `author`, `description` and the slots at the
    top level. "It can be assumed that other than `scheme`, `author`, and
    `description` are `palette` color values" (82-111).
  - Whether a file is base16 or base24 must be **inferred from which slots
    are present** when `system` is missing (30, 108). That is the governing
    text for detecting by content, and it sits inside the YAML family. The
    spec does not say which slots count: any of `base10`-`base17`, or all of
    them.
- **Reference builder departs from that text** (tinted-builder-rust@ae1cc37,
  UNPINNED):
  - The library's `Scheme::from_yaml` (`tinted-builder/src/scheme.rs:27-46`)
    has a doc comment that says "Defaults to Base16 if no `system` field is
    found". The code returns `SchemeMissingProperty("system")` instead (46).
  - The CLI's loader (`tinted-builder-rust/src/operations/build/utils.rs:48-93`)
    maps `system: base24` to Base24 and **any other string** to Base16
    (64-68). A nested `scheme.system: tinted8` maps to Tinted8 (72-80).
    Otherwise it fails with "E111: Missing required field `system`" (85-89).
  - Nothing infers the system from the slots. It does detect from content,
    by where a key sits, which is how tinted8 is told apart.
  - It accepts files only by `.yaml`/`.yml` and refuses anything else with
    E111 (utils.rs:21-36).
- **tinty** (tinty@858837c, UNPINNED) takes the system from the scheme
  *name's* prefix, `base16-…`, `base24-…` or `tinted8-…`, and refuses a name
  without one (`src/operations/apply.rs:26-66`).
- **tinted-terminal templates** (`templates/config.yaml`) cover alacritty,
  ConEmu, foot, ghostty, iTerm2, Kermit, kitty, Konsole, Mintty, PuTTY,
  QTerminal, Rio, Termite, Windows Terminal and wezterm, each for base16,
  base24 and tinted8.
  - The ghostty base16 template (`templates/ghostty-base16.mustache`) writes
    a header comment `# Scheme System: {{ scheme-system }}` (4), then
    `palette = 0-21` (8-29) and `background`/`foreground`/`cursor-color`
    (32-34).
  - It writes `selection-background = base05` and
    **`selection-foreground = base02`** (35-36).
  - It also writes non-colour keys: `search-*` (37-40) and `macos-icon-*`
    (45-47). Under baud's ghostty reader each of these gives one "is not a
    colour and is ignored" notice (`palette.rs:198-204`).

### The selection colour for base16: the text against two practices

- The text: tinted's styling table defines base02 as "Selection Background"
  only in its **Text Editor** column. Its Terminal column gives base02 no
  selection role ("(Dark Gray)", ANSI 19) (`tinted-styling-base16`,
  styling.md:34, VERIFIED). base24's table is the same
  (`base24-styling`, styling.md:49).
- Two practices, which disagree with each other:
  - wezterm: background base05, text base00 (base16.rs:71-72).
  - tinted's own ghostty template: background base05, text base02.
- Neither practice uses base02 as the background. baud leaves `selection`
  and `cursor` as `None` for base16 and base24
  (`baud-domain/src/themefile.rs:308-311`). That is the only choice the text
  does not contradict.

## 9. What baud does today, and where each peer format would go

baud has three readers: `Format::{Base16, Base24, Ghostty}`
(`baud-domain/src/themefile.rs:84-88`). Base16 and base24 share the reader
`themefile::base16`, which picks between them from `system`, or else from
whether any of `base10`-`base17` is present (254-260). That is tinted's rule,
read as "any one is enough".

The choice between the YAML reader and the ghostty reader is made by
extension alone:

- `is_tinted` returns true for `.yaml`/`.yml` in any case (`baud-app/src/palette.rs:142-146`).
- Everything else, including a file with no extension, goes to the ghostty
  reader (195-205).
- A bare name is also tried with `.yaml` and `.yml` added (101-138).

**Where each peer format would go.** These rows come from reading
palette.rs:142-146 and 195-205. None of it was run.

| file | its format | baud sends it to | likely outcome |
|---|---|---|---|
| `x.yaml` / `x.yml`, tinted nested or flat, base16/base24 | tinted | base16 reader | read (the intended case) |
| `yaml/x.yml` from iTerm2-Color-Schemes | `color_01`..`16` YAML | base16 reader | refused: base00 missing (the reader ignores every other key) |
| Gogh `themes/x.yml` | `color_01`..`16` YAML | base16 reader | refused, same reason |
| tinted8 `.yaml` | tinted8 | base16 reader | refused (no base00-0F) |
| ghostty theme (no extension) | ghostty | ghostty reader | read |
| kitty `x.conf` | `colorN value` | ghostty reader | every line ignored: there is no `=` |
| `x.itermcolors` | XML plist | ghostty reader | every line ignored |
| alacritty / wezterm `x.toml` | TOML tables | ghostty reader | `[colors.normal]` keys such as `black = "#…"` ignored (not a ghostty key) |
| foot `x.ini` | INI, bare hex | ghostty reader | `foreground=839496` would likely read (ghostty accepts hex without `#`); `regular0`, `[colors-dark]` and the rest ignored |
| Windows Terminal `x.json` | JSON | ghostty reader | every line ignored |

What "ignored" leads to (an empty scheme being refused, or a half scheme being
used) depends on palette.rs past line 205, which this research did not trace.

## 10. Scope: what "ansi" covers

The word "ansi", as the request used it, covers at least seven syntaxes, and baud reads one of
them. They are all "sixteen slots, plus a page and a text colour" (note Y's
ansi16-style), spelled differently:

- ghostty `palette = N=`
- kitty `colorN`
- alacritty TOML `normal`/`bright` names
- wezterm TOML `ansi`/`brights` arrays
- foot `regularN`/`brightN`
- Windows Terminal JSON names (with `purple`)
- `.itermcolors` plist
- `color_01`-`color_16` YAML (Gogh, iTerm2-Color-Schemes), which is
  1-based, so `color_01` is ANSI 0

tinted8 is a third tinted system. tinted-builder detects it by its nested
`scheme.system`, and tinted-terminal ships it for every terminal. Whether baud
reads it is an open question about scope, not something to build.

## 11. What the evidence says, for the decision

- **The governing text** covers only the step inside the YAML family. It is
  tinted builder.md:30 and :108, and baud follows it. No standard governs
  choosing *between* families. Every peer leaves that to the suffix, the
  location or the user's choice of API call.
- **Practice that detects from content:**
  - iTerm2's light/dark check and tinted-builder's tinted8 check both work by
    "a key that only one variant has".
  - ColorTool works by "the first parser that succeeds", in an undocumented
    order, and it swallows failures. That is the shape to avoid if errors are
    to be reported.
  - wezterm's sync tool is the same shape, also silent: the extension, then
    whatever parses.
- **The case that breaks extension routing** is `.yml` itself. Three
  incompatible key sets share it: tinted, `color_NN`, and tinted8. Each is
  identified by a top-level key only it has:
  - tinted: `base00`, or `palette.base00`.
  - Gogh/iTerm2-Color-Schemes: `color_01`.
  - tinted8: `scheme.system`.

  So within YAML, a check for which key is present decides the format
  without trying parsers. Outside YAML, the first non-comment line already
  tells the formats apart:
  - `<?xml` / `<plist` for a plist.
  - `{` for JSON.
  - `[section]` for INI or TOML.
  - `key = N=` for ghostty.
  - `name value` with no `=` for kitty.

  This is an observation about the formats, not a design. Choosing a design reads the standards first.
