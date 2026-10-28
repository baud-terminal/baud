# AR. Theme file formats, and telling them apart by content

Status: built 2026-10-17 for every option the results sections below mark decided, plus the decision 4, X resources and O14; O2, O4, O8 and the empty file stay as built, as recorded there. Sections 0-5 are the design as researched on 2026-10-16. The project asked (2026-10-16, verbatim): "we probably want to autodetect ansi/base16/base24", then "we might want to research on that". The surveys this design rests on are in `docs/research/notes/ar/` (the specs, the peers, and baud today). Every option in section 4 goes to the review before anything is built.


The project, 2026-10-16, verbatim: "we probably want to autodetect
ansi/base16/base24" and "we might want to research on that".

**Sections 0-5 are the design as researched.** Every choice the texts leave
open is in section 4 as an option, marked conforming or not, for the review to decide
at 0.9 both times, with the project able to review. If a decision rests on this
file, the file moves to `docs/research/notes/` in the same session, because a
reboot wipes `/private/tmp`.

It builds on three notes in this directory, and does not repeat their
evidence:

- `specs.md`: the governing texts.
- `peers.md`: how other terminals read theme files.
- `baud.md`: baud's code at `c417553d`, and a probe run through its real
  readers.

baud's lines are cited at `c417553d`.

**Provenance.**

- A source pinned in `docs/research/sources/MANIFEST.toml` is cited by its
  key, for example `tinted-builder`.
- A source fetched today is marked UNPINNED with its commit. No
  `[[source: …]]` may be written for it until it is pinned.
- "Inference" marks a deduction that was not run.

**Scope.**

- vogix is out of scope until vogix 1.0.0-beta.1 (y-palettes.md:16-21).
- tinted8 is a scope question (option O6). Its reader is not designed here.

---

## 0. The design in five lines

1. **Classify, then read.** One pass over the bytes gives exactly one typed
   verdict, and one reader runs on that verdict. baud never tries reader after
   reader. ColorTool does that (`SchemeManager.cs:89-101`, UNPINNED
   microsoft/terminal@9fd41b2), and so does wezterm's sync tool
   (`sync-color-schemes/src/base16.rs:64-77`, UNPINNED wezterm@e2a98e1). In
   both, a failed parse is swallowed, and the format chosen depends on the
   order the parsers are tried.
2. **A verdict comes from the file's own marks**: its line grammar first, then
   a key that only one format has. That is the shape tinted's builder text
   uses inside its own family (`tinted-builder` builder.md:30, :107). It is
   also how tinted-builder tells tinted8 apart, by `scheme.system`
   (`operations/build/utils.rs:72-80`, UNPINNED tinted-builder-rust@ae1cc37).
   iTerm2 tells a light/dark preset the same way, by whether
   `Foreground Color (Light)` is present (`iTermColorPresets.m:329-331`,
   UNPINNED iTerm2@9e29604).
3. **An ambiguity matters only when the readings differ.** If two candidate
   readings give the same `Scheme`, the file is read and the report names
   both candidates. If they give different colours, the file is refused and
   the report names both, with the lines that matched each. This is the
   argument base24's fallback table makes: a base16 file read as base24 gives
   the same colours (`base24-styling` styling.md:9-23).
4. **Every load says what it was read as, and why.** For example, "read as
   base24: `system: "base24"`, line 1", or "read as base16: no `system`, no
   base10-base17 (builder.md:30)". A file that cannot be classified is
   refused with the nearest shape baud saw. It is never loaded as an empty
   scheme.
5. **The extension stops choosing the reader.** It becomes a cross-check that
   adds a report line when it disagrees with the content (option O1). Bare-name
   lookup with `.yaml` and `.yml` added stays as it is.

---

## 1. The formats, and the exact signature of each

### 1.1 Which formats, in three tiers

**Tier R: baud reads these today. Each has a pinned governing text.**

| verdict | governing text | reader |
|---|---|---|
| `Base16` (current nested form, and legacy flat form) | `tinted-builder` builder.md:24-111; `tinted-styling-base16` styling.md:30-47 | `themefile::base16`, themefile.rs:159-315 |
| `Base24` (both forms) | the same, plus `base24-styling` styling.md:9-23 and :45-70 | the same reader |
| `Ghostty` | `ghostty-config-d9840f3` Config.zig:571-573, :782-798; `ghostty-args-d9840f3` 1450-1482 | `themefile::ghostty`, themefile.rs:338-421 |

**Tier N: detected and named, then refused with a message, unless the review opens
the format (O6, O7).** These are the other spellings of "sixteen slots plus a
page and a text colour" that the project's word "ansi" covers (peers.md §10),
and tinted's third system.

| verdict | primary documentation | status of that text |
|---|---|---|
| `Tinted8` | tinted8 styling and builder, v0.2.0-beta11 | UNPINNED home@38fdf7d; a beta spec |
| `ColorNN` (Gogh, iTerm2-Color-Schemes `.yml`) | `iterm2cs-gen-py` gen.py:387-416 (the key map is the definition) | pinned. Gogh `themes/0x96f.yml:2-29` is UNPINNED Gogh@2fbe9bb |
| `Kitty` | `kitty-conf` line 1286; `kitty-options-definition` definition.py:2470, :2688 | `kitty-conf` pinned by digest only (the URL now serves other bytes); `kitty-options-definition` pinned |
| `Foot` | `foot-2705e36-foot-ini-5` foot.ini.5.scd:1025-1105 | pinned |
| `Alacritty` (TOML) | `alacritty-0c405d5-man-5` alacritty.5.scd:297-475 | pinned |
| `AlacrittyYaml` (alacritty's config before 0.13) | `alacritty/src/migrate/mod.rs:67-76` | UNPINNED alacritty@5421470 |
| `Wezterm` (TOML) | `config/src/color.rs:130-146`, :689-705 | UNPINNED wezterm@e2a98e1 |
| `ITermPlist` | key names in `iterm2cs-gen-py` gen.py:387-416; `iTermColorPresets.m` | gen.py pinned; iTerm2 UNPINNED |
| `WindowsTerminal` (JSON) | `doc/cascadia/profiles.schema.json:3347-3420` | UNPINNED terminal@9fd41b2 |
| `Xresources` | resource names in `xterm-man` (color0-15 2912-2940, background 2663, foreground 4014, cursorColor 3233, highlightColor 4075, highlightTextColor 4180) | resource names pinned; the Xlib resource-file syntax is not pinned |
| `PlainList` (16 lines of `#rrggbb`) | pywal's `colors` template, filled by `util.Color.__str__` = `hex_color` (util.py:22-23) from `rgb_to_hex` `"#%02x%02x%02x"` (util.py:173-175) | UNPINNED pywal@236aa48. The repository is archived; last push 2024-01-27 |

**Tier X: not a theme format.**

- `Mixed`: two families in one file, with the lines of each.
- `Unknown`: the nearest shape, such as "YAML with none of `system`,
  `palette`, `base00`, `color_01` or `scheme.system`".
- `NotText`: a binary plist (`bplist00`). Bytes that are not UTF-8 are
  read as bytes; a format whose text requires UTF-8 refuses them in its
  reader.

**The verdict is an enum in `baud-domain`, beside the readers.**
`audit_all.sh` gates `baud-domain` at zero `bool` fields, so "is it tinted"
cannot be a flag.

- It has one variant per row above, plus `Mixed`, `Unknown` and `NotText`.
- Each variant carries its evidence: the `Line` and the key that decided it.
  That is what lets the report name the reason.
- `Format` (themefile.rs:84-88) stays as the type of what was *read*. The
  verdict is what was *seen*.

### 1.2 Signatures

**Normalising.** The detector sees the bytes as each reader will see them:

- A leading UTF-8 BOM is stripped. Without this, a first-byte check fails.
- Lines are split on LF, with CR trimmed, as `trim_with(raw, b" \t\r")` does
  (themefile.rs:370).
- A *significant* line is any line that is not blank and not a comment.

What counts as a comment depends on the family, which gives the first trap:

> **Trap 1, `#`.** Comments start with `#` in YAML, ghostty
> (`ghostty-args-d9840f3` 1450-1466), kitty (`kitty-conf` 1286), TOML and
> foot. Every line of a `PlainList` also starts with `#`. So the plain-list
> test runs **before** `#` lines are discarded (D1 below). Under ghostty's own
> grammar a plain list is an all-comment, empty file, and baud's probe shows
> exactly that today: "18 … unset" and nothing else (baud.md §4).
> `Xresources` comments start with `!`, and INI comments with `;`.

**The line classes**, for the line-oriented families. The key token is the
maximal run of `[A-Za-z0-9_.*-]` after the indentation. After it, spaces and
tabs are skipped, and the next byte gives the class:

| next byte | key token holds `*`, or starts with `.` | class |
|---|---|---|
| `=` | either | `Eq` (ghostty, foot without a section, `base00 = …`) |
| `:` | yes | `Xres` (`*.color0:`, `URxvt*background:`, `XTerm.vt100.foreground:`) |
| `:` | no | `Colon` (YAML) |
| anything else, after at least one space | no | `Space` (kitty) |
| a value with no separator at all | | `Bare` (one class, mixed only if anything else is present) |

> **Trap 2, Xresources is colon-shaped.** `*.color0: #1d1f21` read by baud's
> YAML subset gives the key `*.color0` and an *empty* value, because a `#`
> that begins the value starts a YAML comment (`yaml_value`,
> themefile.rs:321-336). The file then fails as `Missing(base00)`. So the
> `*`/`.` test must come before a line is classed as `Colon`. tinted's keys
> never contain `.` or `*` (corpus, specs.md §7). `color_01` contains neither.
>
> **Trap 3, a ghostty value can contain `:`.** Examples are
> `palette = 1=rgb:cc/66/66` and `font-family = "A: B"`. The class is set by
> the byte right after the key token, not by the first `:` anywhere on the
> line, so these lines are `Eq`.
>
> **Trap 4, YAML block scalars.** In `description: >` or `|`, the
> continuation lines are indented deeper and carry no separator. The detector
> skips them exactly as the reader does (`block`, themefile.rs:203-205). Then
> a tinted file with a long description is not `Mixed`.

**The signature of each format.** "Has" means one line is enough. "Only"
means every significant line is of this form.

| verdict | document shape | deciding signature (unambiguous) |
|---|---|---|
| `PlainList` | lines | **only** lines matching `^#[0-9A-Fa-f]{6}$`, and **exactly 16** of them (O9 covers the count and the bare form) |
| `ITermPlist` | first byte `<`; `<?xml`, `<!DOCTYPE plist` or `<plist` | has `<key>Ansi 0 Color</key>`. A plist without it is `Unknown(Plist)` |
| `WindowsTerminal` | first byte `{` (or a `[` that opens JSON, told from a TOML header by the regex below) | an object with `background`, `foreground` and `purple`. `purple` is WT's name for magenta (schema 3347-3420); no other format uses it. A settings.json with `"schemes": [` holds several schemes, and which one to use is a question for the user, so it is refused with that question |
| `Foot` | first significant line `[colors-dark]` or `[colors-light]` | the section header itself (foot.ini.5.scd:1025-1042). `[colors]` and `[colors2]` give `Foot` with "removed in foot 1.28.0" (foot CHANGELOG, UNPINNED cb27717 139-147) |
| `Alacritty` | a TOML header line `^\[\[?[A-Za-z0-9_."-]+\]\]?\s*(#.*)?$` | has a header `[colors.primary]`, `[colors.normal]`, `[colors.bright]`, `[colors.dim]`, `[colors.cursor]` or `[colors.selection]` (alacritty.5.scd:301, :409, :430), or inline `colors.primary.` keys |
| `Wezterm` | TOML | `[colors]` holding `ansi = [` or `brights = [` (color.rs:130-146) |
| `Tinted8` | `Colon` | a top-level `scheme:` with an **empty** value, then an indented `system:` child (tinted8 styling l.31-52). The value `tinted8` gives `Tinted8`; any other value gives `Unknown(tinted scheme.system = X)` |
| `Base16`/`Base24` | `Colon` | has a top-level `system:` with the value `base16`/`base24`, **or** has any `baseXX` slot (XX two hex digits ≤ 0x17), at the top level (legacy form, builder.md:107-111) or under `palette:` (current form, :47-69). Which of the two: section 2, step D4 |
| `ColorNN` | `Colon` | has a top-level `color_01`…`color_16` (gen.py:390-405) |
| `AlacrittyYaml` | `Colon` | a top-level `colors:` with an indented `primary:` or `normal:` child |
| `Xresources` | `Xres` lines | **only** `Xres` lines (plus `!` comments and `#define`/`#include` cpp lines) whose resource ends in `color0`-`color15`, `foreground`, `background`, `cursorColor`, `highlightColor` or `highlightTextColor` |
| `Ghostty` | `Eq` lines | has one of `palette`, `background`, `foreground`, `cursor-color`, `cursor-text`, `selection-background`, `selection-foreground` (themefile.rs:381-412), **and** no foot-only key |
| `Foot` without a section | `Eq` lines | has a foot-only key: `regular0-7`, `bright0-7`, `dim0-7`, or a bare number `0`-`255` as the key (foot.ini.5.scd:1053-1105) |
| `Kitty` | `Space` lines | has `color0`…`color255`, `foreground`, `background`, `cursor`, `cursor_text_color`, `selection_foreground` or `selection_background` (definition.py:353, :369, :2470, :2641, :2655, :2688) |

**Keys shared by two formats, and why most of them do no harm:**

- `background`, `foreground`, `selection-foreground` and
  `selection-background` mean the same in ghostty and in foot. Both accept
  bare 6-digit hex:
  - foot: "RRGGBB … without prefix" (foot.ini.5.scd:1031);
  - ghostty: `colour`, themefile.rs:545-593.

  A sectionless `Eq` file that uses only these keys reads to the same
  `Scheme` either way. By principle 3 it loads as `Ghostty`, and the report
  says "also valid foot".
- The `Colon` keys in tinted, `ColorNN` and `AlacrittyYaml` do not overlap:
  `baseXX`, `color_NN` and `colors.*`. A file holding two of these sets is
  `Mixed`.

### 1.3 The cases the task names

| case | example | verdict | what happens |
|---|---|---|---|
| base16 written as plain YAML `key: value` with no `palette:` and no `system` | `scheme: "X"`, `author: "Y"`, `base00: "181818"` … | `Base16` (legacy form) | read. The legacy form is tinted's own (builder.md:78-112); `system` "will be inferred" (:107) |
| base16 slots written in ghostty syntax | `base00 = #181818` | `Eq` lines, no ghostty colour key, `base00` seen | `Unknown(Eq)`, reported as "base16 slot names in `key = value` syntax; no spec defines this form". Option O10 |
| ghostty file that sets only the sixteen | `palette = 0=…` … `palette = 15=…`, no `background`/`foreground` | `Ghostty` | read. Page and text **stay baud's own**, with the existing "2 … unset" notice (palette.rs:224-231). ghostty's own `background` default is its own colour, `282C34`, not palette 0 (`ghostty-config-d9840f3` Config.zig:601; foreground :605). ECMA-48 makes 39 and 49 implementation-defined (specs.md §5), and note Y decision 2 keeps the page and text apart from the slots (y-palettes.md:400-422). Taking slot 0 as the page would be a departure (O11) |
| base24 file missing some brights, **with** `system: "base24"` | 20 of the 24 slots | `Base24` | refused today as `Missing(base1x)` (themefile.rs:261-274). Option O3 |
| base24 file missing some brights, **without** `system` | base00-0F and base12 only | `Base24` by today's inference (any one base1x, themefile.rs:256-260), then refused | option O2 (how many base1x make base24) and option O3 |
| `system: base16` together with base1x slots | | `Base16`. `system` is the file's own claim, and inference applies only "if `system` is not provided" (builder.md:30) | read as base16; **the report names the unused base1x slots**. Today they are dropped silently (themefile.rs:277-281) |
| the same slot at the top level and under `palette:` | | `Base16`/`Base24` | refused as `Duplicate` (themefile.rs:214-218). The text forbids neither, but YAML forbids a duplicate key only within one mapping. These are two mappings, so this is a case the text does not cover. Kept as is, and the message names both forms |
| tinted slots together with `color_NN` | `base00:` … and `color_01:` … | `Mixed(Base16 at line a, ColorNN at line b)` | refused, naming both |
| YAML lines together with ghostty lines | `base00: "…"` and `palette = 0=…` | `Mixed(Colon at a, Eq at b)` | refused, naming both. Today a `.yaml` name gives `Syntax` at the first `Eq` line, and no extension makes every YAML line "ignored" |
| ghostty `palette` together with kitty `color0` | `palette = 0=#…` and `color0 #…` | `Mixed(Eq, Space)` | refused |
| alacritty TOML with search colours | `[colors.search.matches] background = "#ffff00"` | `Alacritty` | **today this draws the wrong page**, `#ffff00` (baud.md §4, probe). With detection it is named and refused (or read, under O7) |
| tinted8 | `scheme:` + `  system: "tinted8"` | `Tinted8` | "tinted8 (beta spec 0.2.0) is not read" instead of today's `Missing(base00)` (specs.md §6.2) |
| `color_NN` `.yml` | iTerm2-Color-Schemes, Gogh | `ColorNN` | named and refused, or read under O7. Today: `Missing(base00)` (peers.md §9) |
| a ghostty file with no colour key | only `font-family = …` | `Ghostty`, empty | option O12. Today it loads empty with "18 unset" |

---

## 2. Detecting by content

**Input**: the bytes of the first candidate path that exists. Candidate
order is unchanged (`candidates`, palette.rs:102-138).

**Output**: a `Verdict` with its evidence. Then either one reader's result or
one refusal, and one report line naming what was detected.

The steps run in order. The first step that decides, ends.

- **D0. Not text.** If the file starts with `bplist00`, the verdict is
  `NotText(binary plist)`, reported as "an iTerm2 binary plist; save it as
  XML". Otherwise a leading BOM is stripped and noted; bytes that are not
  UTF-8 decide nothing here.
- **D1. Plain list.** If every non-blank line, trimmed, matches
  `^#[0-9A-Fa-f]{6}$` and there are exactly 16 of them, the verdict is
  `PlainList`. This runs before any `#` line is read as a comment (Trap 1).
  Fewer or more such lines with nothing else present gives
  `Unknown(n hex lines)`. It is not read as a partial palette.
- **D2. Document shape**, from the first significant byte. Here `#`, `!` and
  `;` lines are skipped as comments, and hex-only lines are counted as
  significant.
  - `<`: XML. The verdict is `ITermPlist` if `<key>Ansi 0 Color</key>` is
    present, else `Unknown(XML)`.
  - `{`, or `[` followed by `{`, `"` or whitespace and then `{`: JSON. The
    verdict is `WindowsTerminal` by its signature, else `Unknown(JSON)`.
  - a TOML or INI header line: section headers decide, giving `Foot`,
    `Alacritty` or `Wezterm` (§1.2). Anything else is `Unknown(INI/TOML,
    first section [x])`.
  - anything else: go to D3.
- **D3. Line grammar.** Each significant line is classed `Eq`, `Colon`,
  `Xres`, `Space` or `Bare` (§1.2), with block-scalar continuations skipped
  (Trap 4).
  - **One class only**: go to D4 with that family.
  - **More than one class**: the verdict is `Mixed`, with the first line of
    each class.

  The rule is unanimity, not a majority. A majority would read the lines that
  agree and drop the rest without saying so, which is guessing.
- **D4. The key decides, inside the family.**
  - **`Colon`**:
    1. A top-level mapping `scheme:` with a `system` child gives `Tinted8`
       (or `Unknown(scheme.system = X)`).
    2. Then collect the marks: `system:` (top level), `baseXX` (top level or
       under `palette:`), `color_NN` (top level), and `colors:` with
       `primary:` or `normal:`.
    3. Exactly one kind of mark gives that verdict. More than one kind gives
       `Mixed`. None gives `Unknown(YAML)`.
    4. For tinted, base16 against base24 is decided as follows: an explicit
       `system` decides, and its value must be `base16` or `base24`, else
       `UnknownSystem`. Otherwise base10-base17 decide. The threshold is O2;
       today any one is enough (themefile.rs:256-260).
  - **`Eq`**: ghostty colour keys and no foot-only key give `Ghostty`.
    Foot-only keys and no ghostty-only key (`palette`, `cursor-color`,
    `cursor-text`) give `Foot`. Both kinds give `Mixed`. Only `baseXX` keys
    give `Unknown`, as in §1.3. No colour key at all gives `Ghostty`, empty
    (O12).
  - **`Space`**: a kitty colour key gives `Kitty`, else `Unknown(name value
    lines)`.
  - **`Xres`**: an xterm colour resource gives `Xresources`, else `Unknown`.
- **D5. Read.** Only the verdict's reader runs. A reader's error is reported
  as that format's error, together with the verdict's evidence, for example
  "read as base24 (`system: "base24"`, line 1): line 26: base15 is missing".
  The YAML reader stays strict (one error refuses the file). The ghostty
  reader stays lenient about non-colour keys, because a ghostty theme "can set
  any valid configuration option" (Config.zig:571-573), and each such key is
  still reported.
- **D6. Cross-check the extension.** Each extension implies a family:

  | extension | family |
  |---|---|
  | `.yaml`, `.yml` | `Colon` |
  | `.conf` | `Kitty` |
  | `.toml` | TOML |
  | `.ini` | `Foot` |
  | `.itermcolors` | `ITermPlist` |
  | `.json` | JSON |
  | `.Xresources`, `.Xdefaults` | `Xresources` |
  | none | nothing implied |

  If the extension implies a family and the verdict is not in it, one report
  line follows: "named `.yaml`, but the content is a ghostty theme (line 1
  `background = …`)". Whether the file still loads is O1.
- **D7. Report.** Exactly one "read as …" line per loaded file, every time,
  added to the existing notices (palette.rs:224-262).

**Why this order.**

- D1 must come before comment-stripping.
- D2's document shapes are mutually exclusive by their first byte, so each is
  read whole, never line by line. Classed line by line, a plist or a JSON
  file would come out `Mixed`.
- D3 then D4 is the builder text's own order: first which family the file is
  in, then which system within it.

**What the extension still does.**

1. It still widens bare-name lookup (`night`, then `night.yaml`, then
   `night.yml`; palette.rs:131-136).
2. It is the cross-check in D6.

It no longer chooses the reader. palette.rs:8-12 records that the
extension-chooses-reader rule was "below 0.6, the written rule". The review never
decided it. The bare-name order was decided at the review, below today's 0.9
bar, and that order still decides which file's bytes are read when both
`night` and `night.yaml` exist (O13).

**The open dependency: where the report goes.** "Report what was detected,
never guess silently" needs the user to see the report. Today every notice
goes through `crate::warn!`, which is `eprintln!` (log.rs:82-84;
app.rs:776-778). It is unverified whether a user who launches the `.app`
bundle ever sees stderr (baud.md §1). So until that channel exists, the
design's promise is kept only on a terminal launch. This should be decided
alongside the detector, not after it.

---

## 3. How each format lands on the sixteen, the page and the text

The target is baud's `Scheme` (themefile.rs:92-111): `ansi[16]`, `page`,
`text`, `extended[240]`, `cursor`, `cursor_text`, `selection`,
`selection_text`. That is the texts' own data model:

- OSC 4 entries 0-15 (`ctlseqs` 2040-2047);
- OSC 10/11/12/17/19 (`ctlseqs` 2115-2124);
- xterm's resources (`xterm-man`).

No text gives the ANSI colours RGB values (ECMA-48 §8.3.117 names them only;
specs.md §5). So every mapping below is the format's own.

### 3.1 Tier R (built; the code matches the texts, baud.md §2)

| | base16 (`tinted-styling-base16` 30-47) | base24 (`base24-styling` 45-70) | ghostty (Config.zig:711-906) |
|---|---|---|---|
| 0 | base00 | base00 | `palette = 0=` |
| 1-6 | base08, 0B, 0A, 0D, 0E, 0C | same | `palette = 1..6=` |
| 7 | base05 | base05 | `palette = 7=` |
| 8 | base03 | base03 | `palette = 8=` |
| 9-14 | base08, 0B, 0A, 0D, 0E, 0C ("1 and 9" …) | base12, base14, base13, base16, base17, base15. This is not slot order: 0x13 is yellow and goes to 11, 0x14 is green and goes to 10 | `palette = 9..14=` |
| 15 | base07 | base07 | `palette = 15=` |
| 16-21 | base09, 0F, 01, 02, 04, 06, parsed, never written (styling.md:53) | same | 16-255 parsed, never written, with a notice |
| page / text | base00 / base05 ("Default Background" / "Default Foreground") | same | `background` / `foreground`, independent of 0 and 7 |
| cursor / selection | **unset** (themefile.rs:308-311); O5 | same | `cursor-color`, `cursor-text`, `selection-*`, parsed, unused until V13 |

### 3.2 Tier N (the mapping each would get if the review opens it, O7)

| format | 0-7 | 8-15 | page / text | cursor | selection | notes |
|---|---|---|---|---|---|---|
| `Tinted8` | `palette.black…white` (styling l.93-134) | `*-bright`; derived by HSL when absent (builder l.171-198) | `ui.global.normal.background`/`.foreground`, else the variant-mirrored defaults (builder l.520-556) | `ui.cursor.normal.*`, else the defaults | `ui.selection.*`, else the defaults | `variant` is **input** here, which collides with "polarity is derived, never read" (y-palettes.md:409-411). tinted-shell's tinted8 template uses the old key order `ui.global.foreground.normal`; the text wins (specs.md §3) |
| `ColorNN` | `color_01`…`color_08` (`color_01` = "Ansi 0 Color", gen.py:390) | `color_09`…`color_16` (gen.py:398-405) | `background` / `foreground` | `cursor`, `cursor_text` | `selection`, `selection_text` | gen.py fills missing keys from `fallback_color_map` (418-424), for example Selection from Foreground, then runs its WCAG colour adjustment, adjust_colors_for_wcag (459). So its *generated* themes differ from the YAML. Reading the YAML gives the author's colours, not iTerm2-Color-Schemes' published ones (inference) |
| `Kitty` | `color0`…`color7` | `color8`…`color15` (16-255 for `extended`) | `background` / `foreground` (definition.py:2470) | `cursor`, `cursor_text_color` (353, 369) | `selection_background`, `selection_foreground` (2641, 2655) | the value `none` is meaningful for cursor and selection (cell colour or reverse); it needs a mapping, not a refusal |
| `Foot` | `regular0-7` | `bright0-7`; `0`-`255` by number (1053-1105) | `background` / `foreground` | `cursor` = two space-separated values, text first and then cursor (1044-1051) | `selection-foreground` / `-background` (1151) | values are bare `RRGGBB` (1031). A file with both `[colors-dark]` and `[colors-light]` is a light/dark *pair*, a second axis (O8) |
| `Alacritty` | `[colors.normal]` black, red, green, yellow, blue, magenta, cyan, white (409-428) | `[colors.bright]` (430-450) | `[colors.primary]` background / foreground (299-325) | `cursor = { text, cursor }` (327-334) | `selection = { text, background }` (398-404) | `CellForeground`/`CellBackground` are cell-relative, not colours; `[colors.dim]`, `[colors.search.*]` and `[colors.hints.*]` are not palette entries. `indexed_colors` covers 16-255 (473-475) |
| `AlacrittyYaml` | as above, YAML spelling | | | | | superseded by TOML in alacritty 0.13; `migrate` exists for it |
| `Wezterm` | `ansi[0..8]` | `brights[0..8]` | `background` / `foreground` | `cursor_bg`, `cursor_fg` | `selection_bg`, `selection_fg` | `indexed` covers 16-255 |
| `ITermPlist` | `Ansi 0-7 Color` | `Ansi 8-15 Color` | `Background Color` / `Foreground Color` | `Cursor Color`, `Cursor Text Color` | `Selection Color`, `Selected Text Color` | components are floats with an optional `Color Space` (sRGB, P3, Calibrated). Reading P3 as sRGB is wezterm's defect (peers.md §4). A `(Light)`/`(Dark)` pair is a second axis (O8) |
| `WindowsTerminal` | black, red, green, yellow, blue, **purple**, cyan, white | `bright*` | `background` / `foreground` | `cursorColor` (no text colour) | `selectionBackground` (no text colour) | `Color` is `#rgb` or `#rrggbb` (schema 11-16) |
| `Xresources` | `color0-7` | `color8-15` | `background` / `foreground` | `cursorColor` | `highlightColor` / `highlightTextColor`, which default to reverse video (`xterm-man` 4075, 4180) | values can be X11 names, which baud refuses today (`ColourName`) |
| `PlainList` | lines 1-8 | lines 9-16 | none: stays baud's own | none | none | baud's choice, and **not** ghostty's grammar (O9) |

---

## 4. Options where the texts leave a choice

Each option is marked **conforming** (the text allows it), **departure** (the
text forbids it, or a format's own grammar reads the file another way), or
**no text** (practice only). Where nothing departs, a lean is given. Leans are
not decisions.

**O1. What the extension does.**

| option | | |
|---|---|---|
| a. Content decides; a mismatch with the extension is reported and the file is read | no text | lean |
| b. Content decides; a mismatch is refused, naming both | no text | |
| c. The extension decides, and content only checks it | no text | |
| d. The extension decides alone (today) | no text | refuted by `.yml` meaning three key sets (peers.md §0 point 5) |

No text governs choosing between families (specs.md §6.1; peers.md §11). The
only nearby text: tinted says "it is valid to walk all yaml files and read
them directly" (builder.md:43). It names files by extension and decides the
system by content.

**O2. How many base10-base17 make an unlabelled file base24** (builder.md:30,
"depending on which colors are provided").

- a. Any one (today, themefile.rs:256-260). Conforming.
- b. All eight. Conforming.
- c. All eight, and anything between one and seven is refused as
  "neither". Conforming.

The text does not set the threshold. Under O3a, (a) and (b) differ only in
which message a partial file gets.

**O3. A base24 file missing some of base10-base17.**

- a. Refuse (today). Conforming: the text never says a partial file is valid.
- b. Fill from base24's fallback table (styling.md:9-23), with a notice
  naming each filled slot. Conforming in the reading that the table is
  normative, since it is published "for reference".
- c. Read as base16 when `system` is absent. This conflicts with O2a's
  inference.

Lean: put a and b to the review. c reads as guessing.

**O4. Hex forms in tinted files** ("HTML hex format and MAY be preceded by a
`#`", builder.md:73).

- a. Keep `#rrggbb`, `rrggbb`, `#rgb` and `rgb` (today, themefile.rs:43-68).
  Bare `rgb` is a **departure** from the only HTML definition, WHATWG §2.3.6
  (UNPINNED), whose 3-digit form requires `#`.
- b. Drop bare `rgb` and keep `#rgb`. Conforming under that reading.
- c. Only `#rrggbb`/`rrggbb`. Conforming. The corpus is 100% `"#rrggbb"`
  (specs.md §7). tinted8 says `#RRGGBB` only.

**O5. base16/base24 cursor and selection.**

- a. Unset (today). Conforming; the Terminal column names neither.
- b. Use the Text Editor column: base05 "Caret" and base02 "Selection
  Background", with selection text unnamed (styling.md:34, :37). Conforming
  only if that column binds a terminal, which is a reading.
- c. Follow wezterm (selection base05 on base00) or tinted's ghostty template
  (base05 with text base02). **Departure**: these contradict the only
  selection role the text names. The two practices also disagree with each
  other (peers.md §8).

V13's setting stays the chooser (y-palettes.md:415-418).

**O6. tinted8.**

- a. Detect it and refuse with "tinted8 is not read". Conforming, and the
  lean.
- b. Read it, with `variant` as input, recorded as a scoped exception to
  "polarity derived, never read". Conforming to tinted8's text; it reopens
  note Y decision 5.
- c. Read it and derive polarity from `palette.black` against
  `palette.white`. **Departure**: tinted8 sets the page by variant (builder
  l.520-556).

Under any of these the spec is a beta (0.2.0-beta11). Its sources must be
pinned before any reader cites them.

**O7. Which ANSI-style syntaxes baud reads**.

- a. ghostty only; every other syntax is detected and named. Conforming.
- b. Add some or all of `Kitty`, `Foot`, `Alacritty`, `ColorNN`,
  `ITermPlist`, `WindowsTerminal` and `Xresources`, each to its own document.
  Conforming per format.

Pinned primary text exists for kitty, foot, alacritty, `ColorNN` (gen.py)
and the Xresources resource names. wezterm, iTerm2 and Windows Terminal would
need pinning first.

Each format added means a reader, a table onto `Scheme`, and fixtures. Note Y
decision 2 says "Adding a format is a new reader and a new table onto the
same type" (y-palettes.md:419-420).

**O8. Files that hold a light/dark pair.** foot with both sections, an iTerm2
preset with `(Light)` keys, and a Windows Terminal `SchemePair`.

- a. Refuse with "holds a pair; name one half".
- b. Read the half that matches the `[palette]` half the file is named for.

Conforming either way to each format's text. This is a second axis, apart
from format, and needs its own decision.

**O9. The plain list.**

- a. Do not read it; detect it and say "16 hex lines (pywal's `colors`?) are
  not a theme format baud reads". Conforming to ghostty's grammar.
- b. Read 16 lines as 0-15, with page and text kept as baud's own.
  **Departure** from ghostty's grammar, under which the file is all comments.
  The only practice found is pywal, archived.
- c. As (b), and also accept bare `rrggbb` lines and 8-line lists. **No
  text.**

**O10. tinted slot names in a non-YAML grammar** (`base00 = …`).

- a. Refuse with the explanation. Conforming; tinted is YAML (builder.md:30,
  :43).
- b. Read it. **No text**.

**O11. A 16-colour file with no page or text.**

- a. Keep baud's own page and text (today). Conforming: ghostty defaults
  `background` to its own colour (Config.zig:601), not to palette 0, and
  ECMA-48 makes 39/49 implementation-defined.
- b. Page = slot 0 and text = slot 7. **Departure** for ghostty, kitty, foot,
  alacritty and xterm, all of which hold the two apart. It is the vogix-shell
  practice that note Y already departs from (y-palettes.md:1541-1550).

**O12. A file that reads but sets no colour baud uses** (a ghostty file with
only non-colour keys).

- a. Load it empty, with the "18 … unset" notice (today).
- b. Refuse with "sets no colour".

Both are conforming. (b) is closer to "never guess silently", because an
empty scheme draws baud's own palette while the user believes a theme is in
force.

**O13. The bare-name order** (palette.rs:8-12).

- a. Keep "as given, then `.yaml`, then `.yml`".
- b. When more than one of them exists, refuse and name both.

Content detection does not change this, but it raises the stakes: `night`
and `night.yaml` can now be different formats with the same name.

---

## 5. Tests

Every test is red first, against `c417553d`, with the failure seen.

Fixtures are hand-made in each format's spelling, like the existing ones
(`tests/themes/README.md`), and none is from a published collection. A
corpus-derived fixture would carry tinted's MIT licence, which is already
under `LICENSES/`.

**5.1 The headline red tests**

1. `alacritty.toml`, double-quoted, with `[colors.search.matches]
   background = "#ffff00"`. Today the page is `#ffff00` (baud.md §4).
   Expected: the verdict is `Alacritty`, and the drawn page is **never**
   `#ffff00`. Refused under O7a; under O7b, read with page =
   `[colors.primary].background`.
2. A base16 file with no extension (`day` without `.yaml`). Today: loaded
   empty, with 21 lines "ignored". Expected: `Base16`, read, with the
   colours equal to `day.yaml`'s.
3. A ghostty theme saved as `night.yaml`. Today: `Syntax` at line 2.
   Expected: `Ghostty`, read, colours equal to `night`'s, and the extension
   reported per O1.
4. `tinted8.yaml`. Today: `Missing(base00)`. Expected: `Tinted8`, named
   (O6).
5. `color-nn.yml` (`color_01`…`color_16`, `background`, `foreground`).
   Today: `Missing(base00)`. Expected: `ColorNN`, named or read (O7).
6. `Xresources` (`*.color0: #1d1f21` …). Expected: `Xresources`, **not**
   `Colon`/YAML (Trap 2).

**5.2 Positive controls, one per readable verdict, correctly named.**

- `day.yaml` gives `Base16`.
- A base24 file gives `Base24`, with 9-14 = base12, 14, 13, 16, 17, 15, in
  that order.
- `night` gives `Ghostty`.
- If built (O7b): one each for kitty, foot, alacritty, `ColorNN`, plist and
  JSON.

Each asserts the verdict **and** the report line. A detector that returns
`Unknown` for everything must fail these.

**5.3 Minimal files**

- One line, `palette = 0=#000000`: `Ghostty`, 17 unset.
- The legacy flat base16 file, no `system`, 16 slots and `scheme:`:
  `Base16`.
- The current form without `system`: `Base16` by inference.
- 16 hex lines: `PlainList` (O9).
- 15 hex lines and 17 hex lines: `Unknown(n hex lines)`.
- Empty, and comments only: `Empty`, loaded with nothing set and the notice
  that the sixteen, the page and the text stay baud's own (decided
  2026-10-17, load_empty_with_notice 0.98 / 0.98:
  TOML's grammar allows an empty document, and ghostty, kitty, foot and
  alacritty each load one and set nothing). This line read `Unknown(empty)`,
  refused, until then.
- `color0 #000000` alone: `Kitty`.
- `[colors-dark]` alone: `Foot`, empty.

**5.4 Mixed and ambiguous files**

- `base00:` lines and `palette = ` lines: `Mixed(Colon, Eq)`.
- `base00:` and `color_01:`: `Mixed(Base16, ColorNN)`.
- `palette = 0=…` and `color0 …`: `Mixed(Eq, Space)`.
- `system: "base16"` and `base12`: `Base16`, with a report naming base12 as
  unused.
- Partial base24 without `system`: any one of base10-base17 makes it base24
  (O2, any_one 1.00 / 1.00), and then it is refused
  at its first missing slot (O3).
- Partial base24 with `system: "base24"`: `Missing(base15)` today, filled
  under O3b.
- A slot at the top level and under `palette:`: `Duplicate`, and the message
  names both forms.
- A sectionless `Eq` file with only `background=1d1f21` and
  `foreground=…`: `Ghostty`, "also valid foot", and the same `Scheme` as
  reading it as foot (principle 3).
- Sectionless `regular0=…` and `palette = 0=…`: `Mixed`.

**5.5 Edge bytes**

- A CRLF copy of every positive control gives the same verdict.
- A BOM-prefixed copy gives the same verdict.
- A tinted file with a multi-line `description: >` is not `Mixed` (Trap 4).
- A ghostty `palette = 1=rgb:cc/66/66` line is `Eq`, not `Colon` (Trap 3).
- `bplist00…` gives `NotText(binary plist)`.

**5.6 Properties** (run at `PROPTEST_CASES=20000` before trusting, per
)

- The verdict is a total function: every byte string gives exactly one
  verdict, and no panic.
- Comment lines and blank lines inserted anywhere outside a block scalar do
  not change the verdict.
- For each readable format, a generated valid file gives that verdict.
- No reader other than the verdict's ever runs. A counting test double
  around the two readers asserts one call per load. This is what keeps the
  ColorTool shape out.

**5.7 A perturbation per mechanism.** Each must turn a named test red:

- drop D1, then 5.3 `PlainList` fails;
- drop the `*`/`.` test, then 5.1.6 fails;
- drop block-scalar skipping, then the 5.5 description test fails;
- switch unanimity to majority, then 5.4 `Mixed` fails;
- remove the D6 report, then 5.1.3 fails on its report line.

**5.8 What moves in the same commit**

- `the_extension_chooses_the_reader` (palette.rs:420-431) is **replaced** by
  the content test. It is not deleted.
- `config.toml.example:137-139` currently says the extension decides.
- `docs/guide/configuration.md:67-70`, held by
  `the_configuration_page_lists_every_key_the_reader_knows`
  (config.rs:3048).
- `tests/themes/README.md` gains the new fixtures.
- The palette.rs:8-12 header records the decision and its reason.

**5.9 A measurement, not a test.** Run the detector once over
tinted-theming/schemes @ `3ac9113`: 361 base16, 210 base24 and 11 tinted8
(specs.md §7). Expect 582 of 582 correct, with zero `Mixed` and zero
`Unknown`. Record the result in the note. A corpus run is not checked in,
because the corpus is not in the repository.

---

## 6. What bears on the herdr report (lane b56)

All of this is inference from code and the probe in baud.md §6. Nothing here
was measured on herdr, which is b56's job.

1. **Detection does not change the floor.** The floor acts on renditions,
   after a theme is built (`resolve_slot`, renderer.rs:3038-3051). The format
   reader only decides the sixteen, the page and the text. What detection
   changes is **which files load**: more base16 and base24 files will now
   load (for example extensionless ones), and every base16 file has three
   kinds of pair at exactly 1.00:1 by construction, all lifted to 4.5:1 at
   the default `floor = 1.2`:
   - SGR 30 on the default page;
   - default text on SGR 47;
   - every bright on its own normal.

   On the probe this was 50 to 52 of 289 plain pairs (baud.md §6).
2. **Replies are unfloored, and pixels are floored.** OSC 4/10/11 answer with
   the scheme's own colours (`tell_defaults`, host.rs:315-336). The plan
   records that herdr sends 258 colour queries. If herdr builds its theme
   from those replies and then paints **by index 0-15** on the default page
   or an index page, the floor recolours any pair below 1.2. What herdr asked
   for and what is drawn then differ, which is the project's symptom. If herdr
   paints in **truecolour on a truecolour page**, the floor leaves it alone
   (`Floor::covers`, color.rs:118-122). Truecolour **text on the default
   page** is still covered.
3. **A misrouted theme file changes what herdr sees.** If the `[palette]`
   file in force is one of the shapes in baud.md §4, baud draws its own
   palette (kitty, Xresources, plain list, extensionless base16), or draws a
   wrong page (double-quoted alacritty, `#ffff00`), and herdr's queries are
   answered from that. So b56 should record which `[palette] light`/`dark`
   values and files were in force for each capture. This design read nothing
   under `~/.config`.
4. **Bold under base16** changes nothing visible for colours 1-6, because 9-14
   copy 1-6 (color.rs:451). A herdr theme that relies on bold-brightening
   would look flat on a base16 page. That is the format's mapping, not a baud
   defect.
5. **Increase Contrast's trigger of 3.0** (note Y step 8c) is unbuilt. At
   trigger 3.0 the probe lifted 194 of 289 pairs on a dark base16 scheme. If
   b56 compares against a build where it is enabled, that figure, not 52, is
   the one that applies.

---

## 7. Sources to pin before any of this is cited

These are none of them in MANIFEST.toml yet:

- tinted8 styling and builder at home `38fdf7d`;
- tinted-shell `base24.mustache` and `tinted8.mustache` at `9359c5d`;
- the base24 README at `fb96d3c`;
- WHATWG HTML §2.3.6, if O4 rests on it;
- `peers.md`'s UNPINNED peer sources, for any Tier N format the review opens:
  - wezterm `e2a98e1`;
  - iTerm2 `9e29604`;
  - microsoft/terminal `9fd41b2`;
  - alacritty `5421470`;
  - Gogh `2fbe9bb`;
  - tinted-builder-rust `ae1cc37`;
- pywal `236aa48` (`pywal/templates/colors`, `pywal/util.py`; copies in
  `dl/pywal/`), if O9b or O9c is chosen.

## Decisions, 2026-10-17

Weighed once. O7 was split into one question per format,
and O5's option c into its two practices.

| option | original | replicate | status |
|---|---|---|---|
| O6 tinted8 | detect and refuse | same | **decided** |
| O10 tinted slots in `=` syntax | refuse with the explanation | same | **decided** |
| O11 16 colours, no page or text | keep baud's own | same | **decided** |
| O12 keys but no colour used | load empty with the unset notice | same | **decided** |
| O1 extension | content decides, mismatch reported | same | open |
| O2 base24 threshold | any one slot | same | open |
| O3 partial base24 | refuse | refuse | open |
| O4 hex forms | drop bare `rgb` | same | open |
| O5 cursor and selection | unset | unset | open |
| O7 kitty / foot / alacritty / ColorNN / iTerm2 / Windows Terminal / Xresources | split, kitty flipped | | open, the project's scope |
| O9 plain list | detect and refuse | same | open |
| O13 bare-name order | keep | refuse when several exist | open, flipped |

O12's pick changes section 5's expected refusal for a file with keys and no
colour; a truly empty or comments-only file was not asked and stays open. O8
does not apply until an O7 format is read. What would settle the rest: O1, a
check that a user who opens the app sees the "read as" report; O3, what
tinted-builder-rust does with a base24 file missing one base1x slot; O4,
WHATWG section 2.3.6 pinned and quoted; O5, cursor and selection asked
separately; O7 and O9, the project's scope for "ansi"; O13, the project.

## Decisions again, 2026-10-17

**The project answered O7, O9 and O13** (2026-10-17, "use the recommended
answers and continue", accepting decision 4 of that day's page): read all
four further families -- kitty and foot; alacritty, TOML and the YAML before
0.13; iTerm2's `.itermcolors` and Windows Terminal's JSON schemes; X
resources and a plain list of 16 colours such as pywal's `colors` -- by
content, a refusal only where reading is unsafe; and a bare name matching
both `night` and `night.yaml` keeps today's order, exact name first. Gogh's
`color_NN` YAML and wezterm's TOML are not among the four: they are detected
and refused by name.

**With the facts section 2 said were missing:**

| option | first ask | re-ask | status |
|---|---|---|---|
| O1 a name that disagrees with the content | read and report | read and report | **decided** |
| O3 a base24 file missing base1x | refuse | fill from the table (724/725) | reopened: the first ask's premise was wrong;, so the refusal stays |
| O5 cursor, base16/base24 | unset | | **decided**, today's |
| O5 selection, base16/base24 | unset | | **decided**, today's |
| O4 hex forms | keep all four | keep all four | open; today's stays |
| O8 a light/dark pair in one file | read the named half | read the named half | open; refused naming both |
| an empty or comments-only file | load empty | load empty | open; today's (load empty) stays |
| iTerm2 `Calibrated` or no `Color Space` | convert as macOS's generic RGB | | **decided** (682/683) |
| a float component to 8 bits | round | round | **decided** (684/685) |
| O2 base24 threshold, unlabelled | any one | any one | open; today's (any one) stays (694-697) |

The facts asked with:

- **O1.** Every theme notice goes to stderr through `warn!` (log.rs:82-84).
  On this Mac every process launchd started from Finder or the Dock that
  was looked at -- Finder, Dock, 1Password and its helpers -- has fd 2 on
  `/dev/null` (`lsof`, 2026-10-17); baud.app was not launched for it, so
  that it does the same is inferred. The code path agrees: `warn!` is
  `eprintln!` (baud-app/src/log.rs:82-84), and nothing in baud-app or
  packaging/ redirects fd 2 (no dup2 or freopen, and no
  StandardErrorPath or LSEnvironment). A user who opens the `.app` sees no
  notice and no refusal; only a shell launch shows them.
- **O2.** The same builder never infers the system from the slots: with no
  `system` its CLI fails "E111: Missing required field `system`"
  ([[source: tinted-builder-rust-utils-ae1cc37]] 48-93), against
  builder.md:30's MUST. With O3 decided, "any one" and "all eight, else
  refuse" give every file the same result and differ only in the words.
- **O3.** tinted-builder-rust `ae1cc37` refuses a base24 scheme missing any
  of its 24 keys ([[source: tinted-builder-rust-base24-ae1cc37]] 8-11,
  86-93) and never applies the fallback table.

**O3, re-asked 2026-10-17 with the premise corrected.** The
above rested on "no sentence says a partial file is valid or that the table
fills one", and that is wrong. Two texts fill a partial file from
styling.md's table ([[source: base24-styling]] 9-23): the spec's own example
builder ([[source: base24-builder-fb96d3c]] 1-5, 77-83) and
tinted-builder-python ([[source: tinted-builder-python-4c93d49]] 48, 58-65),
and the README promises base16 themes on base24 templates
([[source: base24-readme-fb96d3c]] 40-49). Three refuse one: the schema
requires all 24 ([[source: base24-schema-fb96d3c]] 213-241),
base16-builder-go fails "Missing colors from base24 palette"
([[source: base16-builder-go-systems-c12f583]] 75-85), and
tinted-builder-rust, as above. All 210 base24 files in the corpus have all
24 slots. The review, both orders, uncached: fill from
the table. The pick changed and stayed far, so
nothing is built: baud still refuses a partial base24 file at its first
missing slot, and the lean, fill, is the project's to take or veto.
- **O4.** WHATWG HTML, "2.3.6 Legacy colors"
  ([[source: whatwg-html-legacy-colors]], the page of 16 October 2026): the
  ×17 branch is taken only for a four-character `#rgb`; otherwise the `#`
  is removed and the string split into three equal components, so a bare
  `abc` is (0x0A, 0x0B, 0x0C). tinted-builder-rust doubles any three digits
  ([[source: tinted-builder-rust-color-ae1cc37]] 343-373), as baud does.

**Two refinements of section 2, each from a format's text, built.** A line
with no `=` is a ghostty line, because ghostty's parser takes the whole line
as a key (`ghostty-args-d9840f3` 1468-1494), so it joins `key = value` lines
rather than making a file `Mixed`. And foot's colour keys outside a section
are not a foot theme: "The default section is usually unnamed"
(`foot-2705e36-foot-ini-5` 9-11) is `[main]`, which has no colours, so a
sectionless `regular0=` is refused as outside a section; with no foot
reading of a sectionless file, "also valid foot" does not arise.

## X resources: what was built, and what is open (2026-10-17)

**The plain list is read** (the project's O9): sixteen `#rrggbb` lines are
ANSI 0 to 15, the page and the text baud's own (O11).

**X resources stay detected and refused.** Each choice its reader needs
was put to the review with the texts and re-asked once with more facts; none
reached 0.9 on both asks:

| question | first ask | re-ask | the texts |
|---|---|---|---|
| the C preprocessor | run object-like `#define`/`#undef` and `#ifdef`/`#ifndef`/`#else`/`#endif` with xrdb's `COLOR` defined | same | xrdb passes the file "through the C preprocessor" by default with `COLOR` "defined if, and only if" the class is a colour one ([[source: xrdb-man-aaf6f3d]]) |
| which names are baud's | only names any client matches, `*color0`, `*.color0`, `?.color0` | same | Xrm's grammar ([[source: libx11-ch15-b8d5a70]]); a program's entry matches only that program |
| `#include` | refuse naming it | same | "IncludeFile lines are interpreted by replacing the line with the contents of the specified file" (the same) |
| a 16-bit component to 8 bits | nearest (0.14 / 0.26) | the nearest of `i * 257`, a tie to the lower (0.92 / 0.88) | XParseColor's `#` forms are "the most significant bits" ([[source: libx11-ch06-b8d5a70]]), so `#ab0000` is red `0xab00`; an 8-bit TrueColor X server takes the nearest entry of a map filled with `i * 257` ([[source: xserver-colormap-98aed9d]] 981-985, 1243-1285; [[source: xserver-micmap-98aed9d]] 119-138), which is `0xaa` |

The last row decides nearly every value: under the X server's reduction
`#ab0000` is drawn as `#aa0000`, and under the high byte as written. It is
the project's to settle, or the review's with a fact these asks did not have.

**Built, 2026-10-17, on the decision's "use the recommended answers".** The
research round re-asked all four with a corpus measurement and corrected
facts, none reached 0.9 on both
asks, and the project took each lean. Two leans flipped from the table above
because two facts in 690-693 were false: the X server does not round, it
resolves through `miResolveColor`'s `(v >> 8) * 257`
([[source: xserver-fbcmap-mi-98aed9d]] 57-61, [[source: xserver-micmap-98aed9d]]
95-112), and xterm and libXft take the top bits themselves
([[source: xterm-misc-9489b20]] 2725-2733, [[source: libxft-xftcolor-6b333a3]]
81-95); and following an include is what the text says to do
([[source: libx11-ch15-b8d5a70]] 176).

| question | lean taken | built |
|---|---|---|
| the C preprocessor | the subset with xrdb's `COLOR` (0.72 / 0.72) | C comments, splices, object-like `#define`/`#undef`, `#ifdef`/`#ifndef`/`#else`/`#endif`; `#if`, `#elif`, a macro with arguments and `#include <f>` refuse the file by name; another directive is passed over and named, as cpp prints nothing for it and xrdb does not look at cpp's status ([[source: xrdb-c-aaf6f3d]] 1448-1453) |
| which names | every client's only (0.14 / 0.61) | one loose component, `*color0` or `*.color0`; a program's entry is passed over naming the program |
| `#include` | follow it (0.76 / 0.60) | `#include "f"` beside the including file, macros carried both ways, nested to Xrm's 100 ([[source: libx11-xrm-b8d5a70]] 245, 1621-1650); a file that cannot be read is passed over and named |
| 16 bits to 8 | the high byte (0.84 / 0.86) | `#ab0000` is 0xab, `#fff` is `#f0f0f0`, `rgb:f/f/f` 0xff ([[source: libx11-parsecol-b8d5a70]] 52-86, [[source: libx11-lrgb-b8d5a70]] 1286-1375) |

On the corpus -- tinted-xresources b97e7eca (575), iTerm2-Color-Schemes
c379d7a3 `Xresources/` (815) and xcolors b424d25e `themes/` (113) -- the
reader gives the same scheme from each file as from clang 21's
`cpp -P -DCOLOR` output of it, 1503 of 1503. Tests:
`baud-domain/tests/themexresources.rs` and three in `baud-app/src/palette.rs`,
red on f6302a23 (refused as "X resources, which baud does not read").

**Open, found while building.**

- `#include <f>` is refused: cpp searches the system's include path, which
  no theme in the corpus uses.
- A class name (`*Foreground`) is passed over as not a colour; Xrm matches
  it, below a name at the same level ([[source: libx11-ch15-b8d5a70]]
  272-290). None in the corpus.
- A later line wins. `xrdb -load` keeps a repeated name at its first line
  and its last value ([[source: xrdb-c-aaf6f3d]] 260-287), so where `*color0`
  and `*.color0` alternate, the X server's database can hold the other one.
- A file of nothing but `#include` lines and comments is detected as empty:
  `#` starts a comment in every other format.
- A display's symbols (`WIDTH`, `PLANES`, `CLASS`) are not defined, and
  neither are cpp's own (`__STDC__`, `linux`, `unix`).
- X colour names, `rgbi:`, CIE and TekHVC values refuse the file, as before.
  Any name-shaped word refuses, not only rgb.txt's: the X server's own
  table holds 30 names X.Org's rgb.txt at 749182c does not (aqua, crimson,
  rebeccapurple, the web and x11 variants;
  [[source: xserver-oscolor-98aed9d]] 65-849).
- **Superseded 2026-10-18**: a value is read by `XColor::parse`, the
  parser `OSC 4` reads with, so names, `rgbi:`, CIE and TekHVC values are
  colours, and what it refuses -- a name the server lacks, `rgb:1/2/3/4` --
  is passed over and named, as xterm skips it.

## kitty's names and wide-gamut colours (2026-10-17)

**kitty's colour names are read.** kitty looks a value up as a name before
any other form ([[source: kitty-colors-1e1486e]] 1525-1526), in the 753
names of [[source: kitty-color-names-1e1486e]]: X.Org's rgb.txt at
749182c ([[source: xorg-rgb-749182c]]) lower-cased, plus `debianred`
(line 161). That one name is Debian's: requested in 2004
([[source: debian-bug-243288]], its value 215 7 81 sampled from the Debian
logo), added in Debian's xorg 1:7.2-4
([[source: debian-xorg-changelog-7-7-26]] 1464), and Debian's rgb.txt
differs from X.Org's by that line ([[source: debian-xorg-rgb-7-7-26]] 478).

**The licence row is closed (the project's recommendation A, 2026-10-17).** A
name and a value are facts: "Words and short phrases such as names" are not
subject to copyright ([[source: ecfr-37-202-1]]), and Compendium III says
"Facts are not copyrightable", nor names, nor "color in and of itself"
([[source: usco-compendium3-ch300]] 313.3(C), 313.4(C), 313.4(K)). The
oracle file holds baud's probes, rgb.txt's names and the numbers kitty's
parser gives for them, and no kitty text: "The output of a program is not,
in general, covered by the copyright on the code of the program"
([[source: gnu-gpl-faq-20260113]], #WhatCaseIsOutputGPL). The row in
`docs/research/third-party.toml` now reads MIT-open-group for rgb.txt and
DebianRed as a name and a value from Debian's file, kept. Debian's
copyright file for xorg carries MIT-style and GPL-2+ stanzas without saying
which covers its rgb.txt; the row rests on the name and value being
facts, not on that mapping. `scripts/gen-colour-names.py --kitty` holds the two equal; the
table is a perfect hash built from rgb.txt. A word that is not a name is
skipped with its line, as kitty skips it. 950 values run through kitty's
own functions (`scripts/kitty-colour-oracle.sh`, the GPL source fetched by
hash and never committed) are read as kitty reads them, and so is which
`oklch(`/`lab(` values kitty accepts, C's `strtod` forms included.

**O14. How an `oklch(` or `lab(` colour is drawn: open.** kitty's code and
its documentation ([[source: kitty-wide-gamut-docs-1e1486e]]) disagree,
and both differ from [[source: css-color-4-20261009]]:

| | kitty's code | kitty's docs | CSS Color 4 |
|---|---|---|---|
| a colour outside sRGB | each linear channel clipped (the chroma search never runs: oklch_to_srgb has clamped already) | CSS Color 4's chroma reduction | chroma reduction, JND 0.02 |
| a neutral grey (chroma < 1e-5) | L taken as linear light: `lab(10 0 0)` #828282 | "very dark neutral gray" | #1b1b1b |
| Lab's white | D65, no adaptation | not named | D50, Bradford |

On 1015 values, kitty's functions against the spec's sample code: OKLCH
inside sRGB with chroma, 209 of 209 bytes equal; OKLCH outside, median 20
and at most 228 apart (`oklch(0.95 0.4 30)`: #ff0000 against #ffe4dc);
Lab inside sRGB, none equal, at most 103. The review: one question, kitty's code; re-asked once apart,
a chromatic colour inside sRGB by kitty's math 0.92 / 0.89, and the grey
and out-of-gamut cases refuse. Below the bar, a file holding
a value kitty reads as `oklch(` or `lab(` stays refused at its line
(`WideGamut`), and one kitty refuses is skipped. The project's, or the review's with
a fact these asks did not have.

**Built, 2026-10-17.** The research round re-asked O14 in three cases
(uncached) and the project took the recommended
answer for each:

| case | The review | drawn |
|---|---|---|
| L at or past 0 or 1, or not finite | kitty's C, its docs and CSS agree at 0 and 1; for a number that is not finite kitty's Go refuses the value ([[source: kitty-colorspaces-go-1e1486e]] 254-259, 301-305) and its C draws black or white, which baud follows | black or white |
| chromatic, linear sRGB inside [0, 1] before any clip | kitty_math, decided | kitty's C arithmetic |
| a grey, chroma under 1e-5 | kitty_grey, lean | L's sRGB encoding, L taken as linear light, as kitty draws it |
| outside sRGB | kitty_go_chroma_search, lean | the chroma search of kitty's Go code ([[source: kitty-colorspaces-go-1e1486e]] 101-230), which its themes kitten previews with ([[source: kitty-themes-ui-go-1e1486e]] 178-195) and which keeps [[source: kitty-pr-9325]]'s promise of CSS Color 4 gamut mapping |

The numbers are the ones kitty's terminal reads, `%` and C's `strtod`
forms included; for Lab the case is decided on kitty's C numbers and its
linear channels before the clip, and an `out` Lab takes the Go path from
the start. `scripts/kitty-colour-oracle.sh` now runs kitty's Go too and
names each value's case: `baud-domain/tests/kitty-colours.txt` holds the 950
values and the research grid's 1350, 2300 rows (35 edge, 135 grey, 415 in,
845 out; on 744 `out` rows kitty's terminal and its Go code draw different
bytes), and baud draws every one byte for byte. baud-domain has no maths
library, so `baud-domain/src/themefile/oklab.rs` writes the functions kitty calls; the
bytes are the same whether Go and C fuse multiply-adds or not. The docs'
own example `oklch(0.7 0.25 25)` draws `#ff5655`, what kitty's preview
shows, where kitty's terminal draws `#ff3d46`.

kitty's preview reads `%` another way. Its Go strips the `%` before it
checks for one, so an `L%` comes out white, and it never divides a chroma
or hue percentage ([[source: kitty-colorspaces-go-1e1486e]] 262-264,
323-342). For a `%` value outside sRGB, baud therefore draws neither what
kitty's terminal shows nor what its preview shows. This is open.

## Bytes that are not UTF-8 (2026-10-17)

**The detector reads bytes.** A byte that is not UTF-8 decides no verdict,
so a ghostty theme with a Latin-1 comment loads as it did before
detection, and so do kitty (which reads its config with `errors='replace'`,
[[source: kitty-conf-utils-1e1486e]] 468) and foot. Three
formats' own texts require UTF-8, and their readers refuse other bytes,
naming the line: TOML ("A TOML file must be a valid UTF-8 encoded Unicode
document", [[source: toml-1-0-0]] 44), JSON exchanged as a file
([[source: rfc8259-json]] 8.1) and XML that declares no encoding or UTF-8
([[source: xml-1-0-5e]] 4.3.3). A plist declaring another encoding is read
as bytes. base16 and alacritty's YAML read bytes, as base did.

**Found 2026-10-17, not fixed.** kitty lower-cases a value with Python's
`str.lower()`, which turns some non-ASCII characters into ASCII (U+212A
KELVIN SIGN becomes `k`), so a name spelled with one resolves in kitty and
is skipped by baud. The oracle feeds only ASCII values and cannot see it.

## The kitty reader against kitty's code, and its re-derivation (2026-10-25)

**What a user sees: nothing changes.** Every kitty theme value baud reads
draws the same bytes it drew before: all 2855 values of
`baud-domain/tests/kitty-colours.txt`, and 10,000,000 random Lab and
10,000,000 random OKLCH values compared with the code before, in each of
four seeds. Two simplifications moved a byte off kitty on random values and
were not taken: an inside Lab colour drawn without the round trip through
OKLCH, and the search's distance measured in linear light. What changed is
where the code came from. A publication audit found two pieces that followed kitty's GPL-3.0
code step for step: the colour reader in `terminals.rs`, and the conversions
and chroma search in `oklab.rs`. Each is now written from the rules below,
which come from the colour syntaxes' own texts, Ottosson's OKLab, CIE Lab and
CSS Color 4. The rules also keep kitty's departures from those texts, by the
owner's call on O14 ("use the recommended answers", 2026-10-17) and by this
reader's own rule that it keeps what the format's parser keeps.

### The pins and the oracle

kitty's release tag v0.49.2 (b2cb871): [[source: kitty-colors-v0492]] and
[[source: kitty-colorspaces-go-v0492]]. Both hash the same as the 1e1486e
copies the code was written from. Neither is committed. The oracle script
used to carry its own copy of parse_color() with the Python calls taken
out. It now compiles kitty's body (colors.c 1510-1542) from the fetched,
hash-checked file. Its case notes watch the arguments of kitty's
lab_to_oklch() and oklch_to_srgb_gamut_map() go by, through two macros,
where before they restated parse_oklch(). On all 2855 values it prints
the same bytes as the script at acb4bcad. It also prints a values file
with no colour outside sRGB, which the old script's last step emptied.
555 values were added: the length limits, the comment cut on a tab, the
grey threshold, the `#` and `rgb:` forms' unread digits, dark and light Lab
colours outside sRGB, eight Lab colours whose search numbers fall below
black, and six Lab colours inside sRGB whose round trip through OKLCH moves
a byte.

### Line by line, at acb4bcad

| element | kitty v0.49.2 | baud before | reading |
|---|---|---|---|
| the value's prologue | colors.c 1505-1527: lower-case, trim isspace, under 2 bytes nothing, the comment cut (first space after a leading `#`, else first `#` and trim), the name, under 4 bytes nothing | `kitty_colour`: the same tests in the same order and the same branch shape | **step for step** |
| `#` forms | 1168-1184: 3 digits each doubled; 6, 9, 12: two digits at 0, len/3, 2len/3 | the same split, `kitty_component` mirroring parse_single_color() | step for step |
| `rgb:` | 1186-1201, 1531-1533: length 6, a 32-byte buffer, strtok on `/`, three pieces | the same tests, the strtok named | step for step |
| `oklch(`, `lab(` | 1432-1482: minimum length, `)`, the head, a 256-byte buffer, strtok on space and comma, parse_double_intensity() | kitty_functional(): the same tests in the same order | step for step |
| C's strtod | strtod_l, 1149-1155 | `c_double`, written from C and macOS's strtod_l probed | baud's |
| OKLab matrices | 1274-1284, 1305-1316; Go 37-90 | the same numbers and sums | **Ottosson's**: his post's code ([[source: ottosson-oklab]]) number for number, public domain or MIT; kitty took them too |
| sRGB curve | 1252-1255, 1291-1294 | the same | IEC 61966-2-1 as CSS Color 4 10.2 gives it |
| Lab to linear sRGB | 1398-1417; Go 181-207 | lab_linear() | CIE's inverse with delta = 6/29 ([[source: wikipedia-cielab-1370652355]]), D65 (0.95047, 1, 1.08883) ([[source: lindbloom-chromadapt]]), the sRGB D65 matrix ([[source: lindbloom-rgb-xyz-matrix]]) |
| libm | the platform's | `ln`, `exp`, `pow`, `cbrt`, `sin_cos`, `atan2`, `fmod` by series | baud's |
| C's OKLCH and Lab paths | oklch_to_srgb(), lab_to_oklch() (1266-1289, 1405-1430) | c_oklch_linear(), c_lab_to_oklch(): the Lab one clip, encode, decode and signed pow root, step for step | step for step |
| Go's conversions | oklchToSrgb(), labToOklch(), srgbToOklab() | go_oklch_srgb(), go_lab_to_oklch(), go_srgb_to_oklab() | step for step |
| the chroma search | Go 100-178 | go_gamut_map(): the same three early returns, the same loop and names, the same final clip | **term for term** |
| the cases | none: kitty's C and Go never meet | `oklch`, `lab` choose by O14's table | baud's |

**Which steps the bytes force.** Each step was changed on purpose and run
against the rows. Six changes keep every byte:

- the hue in radians as h (pi/180) on both paths;
- Lab's case numbers without the encode and decode;
- the cube root in place of the signed pow root in those numbers;
- the hue as 180 atan2 / pi;
- the search's distance measured from the OKLCH colour itself to its clip
  in linear light;
- the search's own edge and grey tests removed. The eight rows whose
  search numbers fall below black draw black without them too, so no row
  can tell, and they stay because kitty's Go has them.

A Lab colour inside sRGB drawn straight from its linear channels, without
the round trip through OKLCH, alters bytes: on 10,000,000 random Lab
values, 65 drew one step off kitty, every one of them `in` (for example
lab(97.0553 0.00731 -0.00644): kitty #f7f6f7, straight #f6f6f7). The
rewrite keeps the round trip. Of the six, it takes the first four. It
does not take the fifth: on 30,000 random OKLCH and Lab values run
through the oracle, that change drew `lab(88.555 46.1727 -63.043)` one
step off kitty in green. The distance stays as kitty's Go measures it.

Twelve changes alter bytes:

- a Lab colour inside sRGB drawn straight, as above;
- CSS Color 4's return of the last clipped colour;
- CSS's JND test before the search;
- a JND of 0.0199;
- a convergence of 0.0002;
- a grey threshold of 9e-6;
- `#rgb` scaled by 16;
- the name looked up after the 4-byte test;
- a tab ending a `#` value's comment;
- `rgb:`'s limit at 33 bytes;
- the functional limit at 257;
- `rgb:`'s empty pieces kept.

Four of these (the straight Lab colour, the grey threshold, the tab and
the 33 bytes) changed bytes only on the 555 added rows. So the bytes force
the rules below, and they do not force kitty's expression.

### The rules the code is now written from

**A value.**

1. Trim C's white space (space, tab, LF, VT, FF, CR). Case does not count.
2. A comment ends the value. If the value begins with `#`, it ends at the
   first space. Otherwise it ends at the first `#`, and what is left is
   trimmed again.
3. The value is a colour name in kitty's table, or one of the forms below.
   Anything else is refused. kitty's tests for at least 2 and at least 4
   bytes add nothing to this, because no name and no form is that short.
4. `#` and 3 hex digits: each digit d is 17d. This is CSS Color 4's 5.2:
   #123 is #112233. X11 would put the digit in the high bits instead
   ([[source: xlib-ch06-b8d5a70]]).
5. `#` and 6, 9 or 12 characters: each third's first two characters are
   read as one byte, which is X11's most significant bits
   ([[source: libx11-parsecol-b8d5a70]]). The rest of each third is not
   read. This is kitty's departure: X11 refuses a character that is not a
   hex digit.
6. `rgb:` and fewer than 32 bytes. Split at `/` and drop the empty pieces.
   Each of the first three pieces is one hex digit times 17, or its first
   two characters as a byte, which is the high byte of X11's scaled value
   for 1 to 4 digits. Further pieces are not read. This is kitty's
   departure: X11 reads 1 to 4 hex digits and nothing else.
7. `oklch(` or `lab(`, a `)` at the end, and fewer than 256 bytes between
   them. Split at spaces and commas and drop the empty pieces. Each of the
   first three pieces is one whole C strtod number with no range error. A
   `%` at its end divides it by 100 for OKLCH and by 1 for Lab. Further
   pieces are not read.
8. `rgbi:` is never read: kitty's branch for it tests the byte after the
   `i` for a second `i`.

**A colour.** OKLCH: L is clamped to [0, 1], with a NaN taken as 1. A C
below 0 is 0. h is h mod 360, truncated, so it keeps h's sign. Lab: L is
clamped to [0, 100], and the colour is carried to linear sRGB by the Lab
row of the table.

- **Edge.** If a number is not finite or L <= 0, the colour is black. If
  L >= 1, it is white.
- **Grey.** If C < 1e-5, every channel is L's sRGB encoding, with L taken
  as linear light. This is kitty's departure: CSS's grey is OKLab(L, 0, 0).
- **Inside.** If the linear channels lie in [0, 1] before any clip, an
  OKLCH colour is their encoding. A Lab colour is drawn as its OKLCH, as
  kitty draws it: the OKLCH its edge and grey tests read, carried back to
  linear sRGB, each channel clipped to [0, 1], then encoded.
- **Outside.** At constant L and h, bisect C on [0, C0]. A mid C is kept
  as the low end if its colour is inside sRGB, or if its deltaEOK to its
  clip is below the JND, 0.02 (CSS Color 4 14.2.1; deltaEOK is the
  Euclidean distance in OKLab, 20.3). The distance is taken between the
  colour encoded to sRGB and the same colour with each channel clipped to
  [0, 1], both decoded back to OKLab. The search stops when the ends are
  1e-4 apart (14.2.2's epsilon) and draws the clip of the low end. Three of
  CSS's steps are not taken, and these are kitty's departures: the JND test
  of the clipped origin before the search, the min_inGamut flag, and the
  early return within epsilon of the JND. The search applies the edge and
  grey tests again to the numbers it is given.
- **Lab's numbers.** For edge and grey, Lab's OKLab is taken from its
  linear channels with those below 0 raised to 0. For the search, it is
  taken from the channels as they are, with its hue in [0, 360). These are
  the two sets of numbers kitty's C and Go read.

### the review

the review got the comparison and the rules above, the excerpts of both kitty files
and of baud's code, the request's words on the licence and on O14, and the
three routes. The request was asked twice, the second time with the order
reversed, and neither answer was cached:

- **The reader:** DERIVATIVE.
- **oklab.rs:** DERIVATIVE.
- **Which route:** B_RE_DERIVE in both orders. That
  clears the bar, so it is decided under the rule of 2026-10-10. The project
  may veto it.

### What the code is now

- `kitty_colour` cuts the comment, looks the name up, and then matches
  each form by its prefix, each form with its own limits.
- `oklab.rs` has one conversion each way between OKLab and linear sRGB,
  one from Lab, the edge and grey test, and the search. The separate C and
  Go copies of each conversion are gone, and so are kitty's line citations.
- Every case of the 2855 rows draws as before. Three further checks were
  run through the oracle and none differed: 30,000 random OKLCH and Lab
  values, 30,000 random strings from the forms' characters, and 20,000
  near-valid values of every form, 10,813 of them colours. None of the
  three sets is committed. The first has a reach: a one-step difference at
  about 6.5 in a million Lab values is below what 30,000 values can see.
  A comparison of the code with the code before, on 10,000,000 random Lab
  and 10,000,000 random OKLCH values in each of four seeds, is the check
  that found one, and the code as committed differs from the code before
  on none of them.
- Each rule was broken on purpose again in the new code, and the oracle test
  went red for each: the JND, the grey threshold, Lab's negatives raised,
  the low end drawn, the tab, the 33 bytes, the empty pieces and the
  doubled digit. The search's own edge test is the exception: removed, it
  stays green, and nothing pins it. Drawing an inside Lab colour straight
  goes red on the six rows added for it; a round trip through OKLab alone,
  without the polar step, stays green on them and on 10,000,000 random Lab
  values. The polar step stays because the rule draws the colour as its
  OKLCH, and carrying OKLCH back to OKLab is that step.

### What is still kitty's, and the limits of this

kitty contributed the behaviour: which forms it reads, its departures from
X11 and CSS Color 4, the grey shortcut, the search's shape and its three
constants. These are rules and numbers, and the project's O14 call is what
keeps them. None of them is code. The re-derivation was written by an agent
that had read kitty's files for the audit, so it is not a clean room in the
two-team sense. None of this is legal advice.

**One difference no file reaches.** kitty's parse_color() cuts the comment
with strchr(), and strchr() reads past the trimmed length. So a value that
ends in a tab and then a space is cut at that space in kitty, and baud does
not cut it. Neither reader is ever handed such a value: kitty's config
parser and baud's both trim the line first.
