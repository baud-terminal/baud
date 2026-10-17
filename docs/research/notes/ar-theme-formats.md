# AR. Theme file formats, and telling them apart by content

Status: research, 2026-10-16. Nothing here is decided or built. The owner asked (2026-10-16, verbatim): "we probably want to autodetect ansi/base16/base24", then "we might want to research on that". The surveys this design rests on are in `docs/research/notes/ar/` (the specs, the peers, and baud today). Every option in section 4 goes to Jev before anything is built.


The owner, 2026-10-16, verbatim: "we probably want to autodetect
ansi/base16/base24" and "we might want to research on that" (TODO.md, the item
"Detect a theme file's format from its content").

**This is a design. Nothing in it is decided.** Every choice the texts leave
open is in section 4 as an option, marked conforming or not, for Jev to decide
at 0.9 both times, with the owner able to veto. If a decision rests on this
file, the file moves to `docs/research/notes/` in the same session, because a
reboot wipes `/private/tmp`.

It builds on three notes in this directory, and does not repeat their
evidence:

- `specs.md`: the governing texts.
- `peers.md`: how other terminals read theme files.
- `baud.md`: baud's code at `33975a05`, and a probe run through its real
  readers.

baud's lines are cited at `33975a05`.

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

**Tier N: detected and named, then refused with a message, unless Jev opens
the format (O6, O7).** These are the other spellings of "sixteen slots plus a
page and a text colour" that the owner's word "ansi" covers (peers.md §10),
and tinted's third system.

| verdict | primary documentation | status of that text |
|---|---|---|
| `Tinted8` | tinted8 styling and builder, v0.2.0-beta11 | UNPINNED home@38fdf7d; a beta spec |
| `ColorNN` (Gogh, iTerm2-Color-Schemes `.yml`) | `iterm2cs-gen-py` gen.py:387-416 (the key map is the definition) | pinned. Gogh `themes/0x96f.yml:2-29` is UNPINNED Gogh@2fbe9bb |
| `Kitty` | `kitty-conf` kitty-docs-conf.html:1286; `kitty-options-definition` definition.py:2470, :2688 | pinned |
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
extension-chooses-reader rule was "below 0.6, the written rule". Jev never
decided it. The bare-name order was decided at Jev 0.65, below today's 0.9
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

### 3.2 Tier N (the mapping each would get if Jev opens it, O7)

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

## 4. Options where the texts leave a choice (for Jev)

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

Lean: put a and b to Jev. c reads as guessing.

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

**O7. Which ANSI-style syntaxes baud reads** (the owner's "ansi").

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

**O13. The bare-name order** (Jev 0.65, below the 0.9 bar; palette.rs:8-12).

- a. Keep "as given, then `.yaml`, then `.yml`".
- b. When more than one of them exists, refuse and name both.

Content detection does not change this, but it raises the stakes: `night`
and `night.yaml` can now be different formats with the same name.

---

## 5. Tests

Every test is red first, against `33975a05`, with the failure seen.

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
- Empty, and comments only: `Unknown(empty)`, refused (O12).
- `color0 #000000` alone: `Kitty`.
- `[colors-dark]` alone: `Foot`, empty.

**5.4 Mixed and ambiguous files**

- `base00:` lines and `palette = ` lines: `Mixed(Colon, Eq)`.
- `base00:` and `color_01:`: `Mixed(Base16, ColorNN)`.
- `palette = 0=…` and `color0 …`: `Mixed(Eq, Space)`.
- `system: "base16"` and `base12`: `Base16`, with a report naming base12 as
  unused.
- Partial base24 without `system`: per O2 and O3.
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
CLAUDE.md)

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
- The palette.rs:8-12 header records the decision and its Jev confidence.

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
   the scheme's own colours (`tell_defaults`, host.rs:315-336). TODO.md
   records that herdr sends 258 colour queries. If herdr builds its theme
   from those replies and then paints **by index 0-15** on the default page
   or an index page, the floor recolours any pair below 1.2. What herdr asked
   for and what is drawn then differ, which is the owner's symptom. If herdr
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
- `peers.md`'s UNPINNED peer sources, for any Tier N format Jev opens:
  - wezterm `e2a98e1`;
  - iTerm2 `9e29604`;
  - microsoft/terminal `9fd41b2`;
  - alacritty `5421470`;
  - Gogh `2fbe9bb`;
  - tinted-builder-rust `ae1cc37`;
- pywal `236aa48` (`pywal/templates/colors`, `pywal/util.py`; copies in
  `dl/pywal/`), if O9b or O9c is chosen.

## Jev, 2026-10-17

Asked once, original and order-reversed replicate (decisions.jsonl 664 and
665, jev-1.13.0, neither cached). O7 was split into one question per format,
and O5's option c into its two practices.

| option | original | replicate | status |
|---|---|---|---|
| O6 tinted8 | detect and refuse (0.94) | same (0.97) | **decided** |
| O10 tinted slots in `=` syntax | refuse with the explanation (0.98) | same (0.98) | **decided** |
| O11 16 colours, no page or text | keep baud's own (0.98) | same (0.90) | **decided** |
| O12 keys but no colour used | load empty with the unset notice (0.92) | same (0.96) | **decided** |
| O1 extension | content decides, mismatch reported (0.69) | same (0.48) | open |
| O2 base24 threshold | any one slot (0.59) | same (0.43) | open |
| O3 partial base24 | refuse (0.91) | refuse (0.89) | open |
| O4 hex forms | drop bare `rgb` (0.62) | same (0.62) | open |
| O5 cursor and selection | unset (0.83) | unset (0.83) | open |
| O7 kitty / foot / alacritty / ColorNN / iTerm2 / Windows Terminal / Xresources | split, kitty flipped | | open, the owner's scope |
| O9 plain list | detect and refuse (0.38) | same (0.64) | open |
| O13 bare-name order | keep (0.24) | refuse when several exist (0.18) | open, flipped |

O12's pick changes section 5's expected refusal for a file with keys and no
colour; a truly empty or comments-only file was not asked and stays open. O8
does not apply until an O7 format is read. What would settle the rest: O1, a
check that a user who opens the app sees the "read as" report; O3, what
tinted-builder-rust does with a base24 file missing one base1x slot; O4,
WHATWG section 2.3.6 pinned and quoted; O5, cursor and selection asked
separately; O7 and O9, the owner's scope for "ansi"; O13, the owner.

## The owner, and Jev again, 2026-10-17

**The owner answered O7, O9 and O13** (2026-10-17, "use the recommended
answers and continue", accepting decision 4 of that day's page): read all
four further families -- kitty and foot; alacritty, TOML and the YAML before
0.13; iTerm2's `.itermcolors` and Windows Terminal's JSON schemes; X
resources and a plain list of 16 colours such as pywal's `colors` -- by
content, a refusal only where reading is unsafe; and a bare name matching
both `night` and `night.yaml` keeps today's order, exact name first. Gogh's
`color_NN` YAML and wezterm's TOML are not among the four: they are detected
and refused by name.

**Jev, with the facts section 2 said were missing** (decisions.jsonl 678/679,
re-asked 680/681, jev-1.13.0, none cached):

| option | first ask | re-ask | status |
|---|---|---|---|
| O1 a name that disagrees with the content | read and report (0.84 / 0.94) | read and report (0.90 / 0.92) | **decided** |
| O3 a base24 file missing base1x | refuse (1.00 / 1.00) | | **decided**, today's |
| O5 cursor, base16/base24 | unset (1.00 / 1.00) | | **decided**, today's |
| O5 selection, base16/base24 | unset (0.98 / 0.90) | | **decided**, today's |
| O4 hex forms | keep all four (0.45 / 0.59) | keep all four (0.44 / 0.49) | open; today's stays |
| O8 a light/dark pair in one file | read the named half (0.50 / 0.56) | read the named half (0.82 / 0.90) | open; refused naming both |
| an empty or comments-only file | load empty (0.76 / 0.88) | load empty (0.74 / 0.72) | open; today's (load empty) stays |

The facts asked with:

- **O1.** Every theme notice goes to stderr through `warn!` (log.rs:82-84).
  On this Mac every process launchd started from Finder or the Dock that
  was looked at -- Finder, Dock, 1Password and its helpers -- has fd 2 on
  `/dev/null` (`lsof`, 2026-10-17); baud.app was not launched for it, so
  that it does the same is inferred. A user who opens the `.app` sees no
  notice and no refusal; only a shell launch shows them.
- **O3.** tinted-builder-rust `ae1cc37` refuses a base24 scheme missing any
  of its 24 keys (`tinted-builder/src/scheme/base24.rs` 8-11, 86-93) and
  never applies the fallback table.
- **O4.** WHATWG HTML, "2.3.6 Legacy colors" (page of 16 October 2026): the
  ×17 branch is taken only for a four-character `#rgb`; otherwise the `#`
  is removed and the string split into three equal components, so a bare
  `abc` is (0x0A, 0x0B, 0x0C). tinted-builder-rust doubles any three digits
  (`color.rs` 343-373), as baud does.

**Two refinements of section 2, each from a format's text, built.** A line
with no `=` is a ghostty line, because ghostty's parser takes the whole line
as a key (`ghostty-args-d9840f3` 1468-1494), so it joins `key = value` lines
rather than making a file `Mixed`. And foot's colour keys outside a section
are not a foot theme: "The default section is usually unnamed"
(`foot-2705e36-foot-ini-5` 9-11) is `[main]`, which has no colours, so a
sectionless `regular0=` is refused as outside a section; with no foot
reading of a sectionless file, "also valid foot" does not arise.
