# Y. Palettes: the light/dark pair, the scheme type, the floor, and what the terminal reports

Written 2026-09-29, read against `c836eaf` (branch smooth-scroll); `e4db454`
and `534643a`, which built steps 1 and 2 of section 12 in another session
while this was being written, are described from their commits and tests. Line numbers in baud files are at
`c836eaf` unless a line says otherwise. This note
is V16 of note V, re-researched after the owner's answers of 2026-09-25 and
decided by his answers of 2026-09-29. It synthesises eight source sweeps
(formats, switching, notify, floor, lightset, lscolors, baud's own code, and
vogix). Each sweep was checked by a second agent whose job was to refute it,
and two critics then read the draft, one against the owner's words and one
against baud's architecture. **Every correction a refuter or critic made is
applied here**: a refuted claim is dropped, and a claim nobody could verify
is marked UNVERIFIED with the reason.

**The scope changed on 2026-09-29.** The owner: "i would remove specific
vogix for now until vogix v1.0.0-beta1 work is complete" (session <session>
l.896). So this note designs no vogix reader, no vogix port and no change to
the vogix app. What the research found about vogix is kept, trimmed, in its
own section near the end ("Record: vogix, out of scope until vogix
1.0.0-beta.1").

Labels:

- **VERIFIED**: read at source, at the commit and line given.
- **UNVERIFIED**: not read at source, or not checkable (a legal question, an
  estimate, a closed-source program).
- **computed**: produced by a script in the session scratchpad. Contrast is
  WCAG 2.x: relative luminance with sRGB linearised at 0.04045 and weights
  0.2126, 0.7152, 0.0722, and (L1 + 0.05) / (L2 + 0.05)
  ([[source: wcag22]] "relative luminance" at 3597-3605, "contrast ratio" at
  2866). **Every ratio assumes an opaque background.** The glass is treated
  in its own section.

Sources pinned in `docs/research/sources/MANIFEST.toml` are cited by their
manifest keys in the repository's double-bracket form (group Y, plus keys that
already existed).
Everything else is written as repo@commit path:line and marked UNPINNED.
Names of tests that do not exist yet, and paths that exist only in the
session scratchpad, appear without code formatting, so this note cites
nothing the tree lacks.


## The short answer to the first question: LS_COLORS

The owner's first V16 words were "no what i mean is there terminal has
LS_COLORS i think or somethig like that, what other VTs are doing?"
(<session> l.5295, 2026-09-25), and on
2026-09-29 "how other programs read the 16 colors? from where? is it env var?
or what?" and "where terminals read the colors from LS_COLORS? where is the
16 colors in a distro kept?" (<session> l.838, l.844).

**No terminal sets LS_COLORS, and baud will not.** LS_COLORS is an
environment variable that GNU `ls` reads, and what it holds is **colour
numbers** (SGR parameters such as `01;32`), not colours. A program sends the
number; the terminal's own table of sixteen, and of 256, decides what RGB that
number draws. So there is no distro-wide file of sixteen colours that
programs read: the sixteen live in each terminal's configuration. The levers
baud has are the sixteen it resolves, what it does with bold and faint, and a
contrast floor on what it draws. Section 3 is the evidence.


## 1. What the owner said, in order

Verbatim, from the transcripts, read record by record rather than from
session summaries. Option texts he selected are labelled as option texts, not
as his words. Session <session> is 2026-09-25; session <session> is 2026-09-29.

1. **2026-09-25, on V16** (<session> l.5295): "no what i mean is there
   terminal has LS_COLORS i think or somethig like that, what other VTs are
   doing? as i know there are a bunch like base16/24/ansi/etc and i have also
   vogix16 so maybe research here is needed".
2. **21:57:44Z, an option he selected** (l.6523; the question at l.6522):
   "Theme pair + legible default (Recommended)". The option text: "`[palette]
   light`/`dark` naming vogix16 day/night, base16/base24, or ghostty-format
   files, switched live with macOS appearance (ghostty, kitty, contour,
   Windows Terminal do this). Plus a built-in light sixteen, so GNU ls's bold
   green/cyan (1.19 and 1.09:1 on today's light page) become readable --
   Modus Operandi's is the only AAA set found (text 6.1:1+)." The "6.1:1+"
   and "the only AAA set" are option text, and section 5 corrects the second
   for baud's page.
3. **Same answer, on mode 2031 / DSR 996/997**: "we want to be OS aware if
   possible as well so bg luminance and os appearance , maybe we need more
   research ?". **On ANSI 0 being the page**: "but again vogix16 and ansi and
   base16 all have 16 colors, normal terminal, only diff is base24 i
   beliee... so not sure maybe more research?".
4. **22:53:51Z** (l.6854; the questions at l.6853). On 997 and program
   colours, where the recommended option was "Background drawn; flip drops
   overrides": "dude, we have ghe glass crate that i am the owner off that
   does that already right, we just need to use it , it is already built
   there including tint - osx specific , we will need for linux but that is
   later work". On the mapping, endorsing the recommended option "Published
   mapping + contrast floor": "i like what you recommend so we need to have a
   type so we can also support base24 or future ones, i like it". **The
   option text he endorsed, in full**: "Each format's own published mapping
   (so a scheme looks as it does in kitty, ghostty, wezterm; vogix16 by
   vogix's templates), plus a narrow floor that nudges text only when it would
   be invisible against its own background (ghostty documents 1.1 as a good
   value) -- fixing the invisible pairs of any convention, the built-in light
   theme's included."
5. **On light mode under glass, 2026-09-20** (session <session> l.9700):
   "again, this should align with how osx settings are, isn't that what
   glassterm is doing, it depends on the osx settings".
6. **Standing rules**: "we need to support such protocols fully, always!"
   (<session> l.5251, his message); "i always want you to research first" and
   "it needs to be VERY EXTREMLY OPTIMIZED" (his spelling), which survive only
   in the session's compaction summaries as "the owner, verbatim" (l.7068,
   l.7660), the raw messages predating the compaction and not read; and, for
   caching, tables and residency, baud's own idioms first.
7. **2026-09-29, scope** (<session>): "i don't want to focus on vogix right now
   or anything like that, i want it basic" (l.838); "ok so i started with
   vogix-themes v1.0.0-beta1 so you can see that now its ansi compatible"
   (l.877); "and i would remove specific vogix for now until vogix
   v1.0.0-beta1 work is complete" (l.896).
8. **2026-09-29, the decisions** (<session> l.916, l.926, l.940, l.962). The
   light set: "not sure i follow " (l.916), then, re-asked, the option "Yes,
   Modus Operandi (Recommended)" (l.926). Which setting: "glass term follow
   system as well no?" (l.916), then the option "Appearance (Recommended)"
   (l.926). Program colours on a flip: the option "Reset program colours
   (Recommended)" (l.916). The floor's target: the option "To 4.5:1
   (Recommended)" (l.916); the question named the trigger "below about 1.2:1,
   e.g. executables at 1.19". dim_light: the option "Measure, then set
   (Recommended)" (l.940). A dark set: the option "Yes, research one next
   (Recommended)" (l.940). Reduce Transparency: the option "Yes, opaque then
   (Recommended)" (l.962). Increase Contrast: the option "Yes, stricter floor
   (Recommended)" (l.962), whose question said "fix anything under 3:1".
   Faint and the floor's default: the option "Fix faint; floor on
   (Recommended)" (l.962). **The thresholds 1.2, 3.0 and 4.5 are the
   questions' and options' numbers, not figures he typed.**


## 2. Decisions taken

The owner's decisions of 2026-09-29, in substance. They replace the draft's
twelve owner questions; nothing in this list is open.

1. **Scope.** No vogix reader, no vogix legibility or import rules, no vogix
   port or crate, and no vogix app fixes, until vogix 1.0.0-beta.1 is
   complete. The owner said vogix's new design is ANSI-ordered: 16 slots, ANSI
   0-15, by role.
2. **Scheme files by each format's published mapping.** base16 and base24 by
   tinted-theming's table ([[source: tinted-styling-base16]] v0.4.2, lines
   30-47; [[source: base24-styling]] 45-70). ghostty and other ansi16-style
   files keep their own `background` and `foreground` as the page and the
   text. Entries 16-21 are parsed and not written, so 16-255 stay xterm's.
   **A scheme type that other formats can join later**, as he asked: "we need
   to have a type so we can also support base24 or future ones".
3. **The built-in light sixteen is Modus Operandi**, the sixteen hex values of
   the Emacs `ansi-color` mapping:

       000000 a60000 006800 6f5500 0031a9 721045 005e8b a6a6a6
       595959 972500 00663f 884900 3548cf 531ab6 005f5f ffffff

   credited "Modus Operandi by Protesilaos Stavrou, (C) Free Software
   Foundation, GPL-3.0-or-later", with no file text copied. The Modus `.el`
   files are pinned as `external` (GPL: [[source: modus-themes-el]],
   [[source: modus-operandi-theme-el]], [[source: modus-copying]]); the
   committed test holds the sixteen values as constants.
4. **The palette follows System Settings > Appearance** (AppleInterfaceStyle).
   The glass keeps following Icon & widget style. On the owner's Mac the two
   disagree (Appearance Dark, widget ClearLight), so by default he sees the
   dark half.
5. **An OS flip under `theme = "auto"` resets program colours**: the OSC 4,
   10, 11 and 12 overrides are dropped when the palette flips, the rule kitty,
   foot, contour and Windows Terminal follow. **996 and 997 report the
   dark/light class of what OSC 10 and 11 would answer.** A pinned theme
   reports nothing on an OS flip. The glass crate's `Responder::appearance` is
   used unchanged. **The classifier is WCAG relative luminance of the
   background against the text**, not an Oklab rule (that was vogix's D12 and
   goes with the vogix scope).
   *Correction to the reason given with it*, "as kitty and iTerm2 do": both
   classify the **background alone**, by a luma under 0.5. kitty's
   `is_dark` is BT.601 luma, `0.299 R + 0.587 G + 0.114 B`, over 255, below
   0.5 ([[source: kitty-colors-h]] 66-69, [[source: kitty-colors-c]]
   1060-1063, read from [[source: kitty-window-py]] 1654-1656). iTerm2's is
   0.30/0.59/0.11 below 0.5 (iTerm2@8622e64 NSColor+iTerm.m, UNPINNED; the
   notify sweep's reading, confirmed by its refuter). Neither compares the
   background with the text. The rules agree on baud's two defaults, on
   Solarized dark and light, and on all 602 bundled ghostty themes (section
   4), and differ near mid-grey: at `#808080` on white kitty's rule says light
   and baud's says dark, a 3.95:1 pair that is illegible either way (section
   7).
6. **The contrast floor is on by default**, with a config key to turn it off.
   **Trigger about 1.2**: the endorsed option text ends "-- fixing the
   invisible pairs of any convention, the built-in light theme's included", so
   SGR 30 on a page equal to ANSI 0, and executables at 1.19, are lifted.
   **Target 4.5:1**, lifted by a linear-light mix toward black or white that
   keeps the hue. On baud's light page `0xEF` (computed): `#00ff00` becomes
   `#007f00` at 4.53, `#00ffff` becomes `#007979` at 4.55, `#ffffff` becomes
   `#6c6c6c` at 4.57. The exemptions are decided internally: the per-slot
   rule (the two rendition words differ, and one side is the default or an
   index 0-15), with a 20-byte slot as a measured fallback. **The floor on in
   `Theme` turns `the_palette_swaps_black_and_white_and_nothing_else` in
   `baud-render/tests/reverse_video.rs` red, on purpose**: that test draws
   `?5` then SGR 30, which is `#e5e5e5` on `#d8d8d8`, 1.13:1 (computed), a pair
   the floor exists to lift.
7. **dim_light is measured on screen, then set** to the lowest opacity that
   keeps normal text at 4.5:1 over the glass. The current 0.12 came from
   glassterm, which dims black under light text; baud dims a light page under
   dark text, so the number does not transfer.
8. **A readable dark set is researched next**, Modus Vivendi first, after the
   light set lands, and then put to the owner.
9. **Reduce Transparency draws every background opaque** while it is on. That
   overrides the 2026-09-22 rule that application backgrounds stay dim, and
   only then. **Increase Contrast makes the floor stricter**: trigger 3.0,
   target 4.5, accepting that btop's dim greys brighten (`#303030` on
   `#101010` is 1.44:1, `#404040` is 1.84:1, computed).
10. **Faint (SGR 2) is a defect**: it blends toward the background instead of
    halving the channels.
11. **LS_COLORS**: no terminal sets it and baud won't; it chooses colour
    numbers, and the terminal's table chooses the RGB (the section above).
12. **Particles crossing the cursor in the effect trails stay.** Unrelated to
    palettes; recorded here only because it was answered the same day.


## 3. What LS_COLORS is and is not

**LS_COLORS is an environment variable of SGR numbers that GNU ls reads.**
VERIFIED:

- `dircolors.hin` is the file "to help you set the LS_COLORS environment
  variable used by GNU ls" ([[source: coreutils-dircolors-hin]] 1-2). Its
  only filters are COLORTERM and TERM globs (17-45); nothing in it tells light
  from dark. Its values are raw SGR: DIR `01;34`, LINK `01;36`, EXEC `01;32`
  (62-82).
- **When LS_COLORS is unset, ls uses its own built-in table, which differs**:
  `color_indicator[]` ([[source: coreutils-ls-c]] 624-662) has pi `33` where
  dircolors.hin has `40;33`, bd/cd `01;33` where it has `40;33;01`, and no
  `or`.
- ls.c has no OSC 11 and no COLORFGBG (0 hits; control: 6 `\033` literals).
  For colour it writes only SGR; it also writes EL (`\033[K`, 662) and, under
  `--hyperlink`, OSC 8 (4772, 4789).
- baud sets `COLORTERM=truecolor` (`baud-app/src/pty.rs` line 188), which is
  what keeps GNU ls coloured when LS_COLORS is unset (the `known_term_type`
  path, ls.c 2694-2731). Executed: with `TERM=dumb` and no COLORTERM it prints
  no colour even with `--color=always`.

**BSD and macOS ls** use LSCOLORS, a different grammar. Apple's default is
`exfxcxdxbxegedabagacadah`, twelve pairs, none bold
([[source: apple-ls-print-c]] 125). Colour is off unless CLICOLOR or `-G`:
`colorflag` starts at `COLORFLAG_NEVER` and only CLICOLOR or `-G`/`--color`
lifts it, and the function that reads the environment, which does read COLORTERM, is reached only
past that gate, so baud's COLORTERM alone does not turn it on
([[source: apple-ls-ls-c]] 179, 196-228, 304-310). VERIFIED.

**COLORFGBG** is set once at spawn by rxvt-unicode, Konsole and iTerm2 and
read by vim and Emacs's rxvt.el. baud strips the outer terminal's value
(`baud-app/src/pty.rs` line 175) and sets none. A value set at spawn cannot
follow an appearance switch. VERIFIED.

**No terminal read sets LS_COLORS, LSCOLORS or CLICOLOR**: kitty child.py,
ghostty Exec.zig, foot slave.c, wezterm config.rs, alacritty tty/mod.rs and
iTerm2 PTYSession.m, each with its COLORTERM line as the positive control
(UNPINNED except [[source: iterm2-ptysession]]; the iTerm2 clone is sparse, so
that absence holds for PTYSession.m only). Distributions and frameworks set
it: Fedora's colorls.sh, oh-my-zsh, vivid, dircolors-solarized. VERIFIED.

**What the owner actually runs** (read from his home-manager files; VERIFIED
by the lscolors sweep and its refuter):

- `ls`, `ll`, `la`, `lt` are aliases for **lsd 1.2.0**. LS_COLORS is unset, so
  lsd colours names from lscolors-0.20.0's `LS_COLORS_DEFAULT`
  ([[source: lscolors-lib]] 273), GNU's built-in table, sent as `1;38;5;2`,
  `1;38;5;4`, `1;38;5;6`, which baud's bold rule turns into slots 10, 12 and
  14: **1.19, 4.12 and 1.09** on the light page, the same as GNU ls. lsd's
  metadata theme is dark-only ("TODO(zwpaper): check terminal color",
  [[source: lsd-theme-color]] 396-401) and uses cube and ramp colours: user
  230 (1.12), group 187 (1.28), size small 229 (1.10), dates 40/42/36
  (1.70/1.65/2.44), grey 245 (3.00). Its permission colours are ANSI slots
  1-3, which the palette does reach.
- `lls` is `command ls`. His interactive PATH holds no ls but `/bin/ls`, so it
  is **BSD ls with no colour** (no CLICOLOR, no `-G`). GNU ls 9.11 reaches it
  only inside a devenv shell such as baud's.
- `BAT_THEME=ansi`, `MANPAGER=bat -l man`, and delta with `syntax-theme =
  ansi`: bat's ansi theme encodes palette slots ([[source: bat-terminal-rs]]
  6-36), so bat, man pages and git diffs are coloured by ANSI 1-6. Strings
  are slot 2 (1.88 light), numbers 3 (1.48), `markup.bold` bold 3 becomes
  slot 11 (1.07).
- **fzf 0.74.4 picks Dark256 whenever TERM contains 256**, with no background
  detection ([[source: fzf-light-unix]] 22-31). On the light page its prompt
  110 is 2.00, match 108 2.15, spinner 148 1.46, info 144 1.96, header 109
  2.08 (computed).
- starship is configured with directory `white` (1.10 light), git_branch
  `cyan bold` (1.09) and nix_shell `bold yellow` (1.07).
- helix 25.07.1 (`a05c151`) paints its own truecolour background and does no
  light/dark detection. Emacs asks OSC 11 only after a DA2 version reply,
  which baud does not send, so it does not detect under baud either.

**The 201 pairs.** The lscolors sweep collected 201 (tool, element, SGR)
defaults, each with its source line. Resolved through baud's rules (bold
brightens an index below 8 by 8, SGR 2 halves the channels, SGR 7 swaps) and
counted as text on its own background, computed, opaque, unrounded:

| light page, 0x27 on 0xEF | below 1.1 | below 1.2 | below 1.5 | below 3.0 | below 4.5 |
|---|---|---|---|---|---|
| xterm's sixteen (today) | 25 | 44 | 59 | 109 | 155 |
| Modus Operandi, Emacs mapping | 1 | 5 | 12 | 33 | 38 |

Under Modus the five below 1.2 are htop `1;30;43` (1.0076), fd `*.rs`
`38;5;48` (1.1667), lsd user `38;5;230` (1.1245), lsd size-small `38;5;229`
(1.1047) and htop bold white `1;37` (1.1499). **Four of the five are cube
colours or bright white, which a sixteen cannot fix; the floor lifts all
five.**

**Dark mode fails too, and the owner runs system Dark**: 51 of the 201 pairs
are below 4.5 on the dark page, including every plain blue `34` at 2.02
(macOS `ls -G` directories, git upstream, delta file headers). That is
decision 8's research.

**Conclusion.** baud does not set LS_COLORS, LSCOLORS, CLICOLOR or
COLORFGBG. It controls the RGB of 0-15, how bold and faint map onto it, and a
floor on resolved colours. 17 of the 201 pairs are cube or ramp colours, which
only the floor reaches.


## 4. The formats, and the type

**What each format carries** (formats sweep, VERIFIED at the commits named;
the refuter's corrections applied):

| format | what it carries | lands in ANSI 0-15 |
|---|---|---|
| ansi16-style (ghostty, kitty, Windows Terminal, iTerm2-Color-Schemes files) | the sixteen plus background, foreground, cursor, cursor text, selection: 22 values | all 16, distinct; page and text separate |
| base16 | 16 slots | 0-8 and 15 from 10 slots; 9-14 copy 1-6; base09/0F/01/02/04/06 go to 16-21 |
| base24 | 24 slots | all 16 distinct; six in 16-21; base10/11 have no ANSI number |
| tinted8 | 8 anchors, derived brights and extras | 0-7 given, 8-15 derived unless given; its black is text in a light scheme |

- **base16's published mapping** is [[source: tinted-styling-base16]] v0.4.2
  lines 30-47: base00 = 0 "Black (Background)", base03 = 8, base05 = 7,
  base07 = 15, base08 = "1 and 9", base0B "2 and 10", base0A "3 and 11",
  base0D "4 and 12", base0E "5 and 13", base0C "6 and 14", and base09, 0F,
  01, 02, 04, 06 = 16-21. The original chriskempson/base16@39fb23d styling.md
  has no ANSI mapping at all (UNPINNED). [[source: tinted-shell-base16]] sends
  it as OSC 4 0-21 (59-82), OSC 10 = base05 and OSC 11 = base00 (95-97), and
  `OSC 12;7` (55, 102). ghostty's, kitty's and Windows Terminal's tinted
  templates and wezterm@b09b56c's base16.rs copy the same table (UNPINNED).
- **base24** ([[source: base24-styling]] 45-70): base12 = 9, base14 = 10,
  base13 = 11, base16 = 12, base17 = 13, base15 = 14; base10 and base11 have
  none.
- **The tinted corpus** (tinted-theming/schemes@50f6e3b, UNPINNED): 352 base16
  files (102 light, 250 dark) and 209 base24 (34 light, 175 dark), all in the
  current nested format, every value double-quoted with `#`. `variant` is
  optional by spec and present in all 561. One file is `.yml`, not `.yaml`.
  The reference builder, tinted-builder-rust v0.16.0, requires `system`
  against its own spec ([[source: tinted-builder]]) and is GPL-3.0-only;
  wezterm's loader reads only the legacy flat format. VERIFIED.
- **ghostty theme files** (the manifest's older `ghostty-config` is
  `9bbb9b2`, whose lines differ; these are at `d9840f3`): a line is a comment
  only when its first byte is `#` ([[source: ghostty-args-d9840f3]] 1460), so
  a `#` inside a value is data; `palette = N=COLOR`
  ([[source: ghostty-config-d9840f3]] 780-798); a theme "can set any valid
  configuration option" (571-576); a pair is `light:X,dark:Y` (586-591).
  `palette-generate` defaults to false (822), and 16-255 are always filled
  with xterm's cube ([[source: ghostty-color-d9840f3]] 9-47), so the doc's
  "generated by default" is ambiguous wording, not a contradiction. ghostty
  bundles release-20260810-152212-0173c3c ([[source: ghostty-themes-tgz]],
  78,218 bytes compressed, 624,640 uncompressed; an earlier draft said 34 MB):
  602 files, 601 byte-identical to iTerm2-Color-Schemes@99d9701's ghostty/
  directory.
- **iTerm2-Color-Schemes is not the authors' colours.** Every generated file
  passes through `tools/gen.py`'s WCAG pass, which pushes Foreground, ANSI 1-14
  and Cursor Color to at least 1.75:1 on the background, Cursor Text against
  Cursor, and Selected Text against Selection ([[source: iterm2cs-gen-py]]
  253-326; ANSI 0 and 15 exempt by `range(1,15)` at 262). 217 of 627 non-P3
  themes differ from their source in ANSI 1-14. VERIFIED.
- **Across the 145 light themes of that ghostty/ directory**, 36 have ANSI 0
  within 1.5:1 of the page and 64 have ANSI 15 within 1.5:1, with no overlap:
  100 distinct themes (computed; the 1.5 threshold is arbitrary). So
  near-invisible pairs occur in every format family, not only base16.
- **Formats that carry both appearances in one file**: foot's
  `[colors-dark]`/`[colors-light]` and nine `.itermcolors` files with
  `(Light)`/`(Dark)` keys (which gen.py drops). Windows Terminal pairs at the
  settings level (`SchemePair`).

**Light or dark: the rules agree off-centre only.** ghostty's list-themes
uses Rec.709 luma below 0.5, gen.py Rec.709 below 127, kitty max(R,G,B) below
115 for themes, tinted an optional field. Over the 657 collection files the
first two give 512 dark and 145 light; kitty's theme rule differs on 9
saturated mid-tones (computed). baud's rule (decision 5) is the WCAG
comparison of page and text. Over the 602 files of the pinned tarball, it and
a Rec.709 luma-below-0.5 rule on the background give the same answer for
every file: 473 dark, 129 light, 0 disagreements (computed on 2026-09-29; the
657 are iTerm2-Color-Schemes' ghostty/ directory, a different set, and the
WCAG rule was not run over them).

**baud's config tokenizer cannot read most of these.** `strip_comment`
(`baud-app/src/config.rs` lines 902-913) cuts at the first `#` outside double
quotes, and `strings` reads only double-quoted strings, so ghostty's `palette
= 0=#000000` reads as `0=`, kitty's `color0 #000000` and YAML give no fields,
and single-quoted Alacritty TOML loses its values (dotted headers are already
handled; only single quotes are missing). VERIFIED by reading and by a
line-for-line Python port.

**Licences** (VERIFIED): tinted-theming specs and schemes, tinted-shell,
iTerm2-Color-Schemes (collection only: "the copyright/license for each
individual theme belongs to the author of that theme"), Windows Terminal and
ghostty are MIT; kitty-themes is GPLv3 overall with per-file licences (224 of
412 declare one); Modus is GPL-3.0-or-later, (C) Free Software Foundation.

**The type** (decision 2):

- **Sixteen slots in ANSI order**, which is what every format lands on.
- **A page and a text colour held apart from the slots.** base16 and base24
  fill them from base00 and base05, as tinted-shell's OSC 10/11 do. An
  ansi16-style file fills them from its own `background` and `foreground`.
  A future format that defines ANSI 0 and 7 as its page and text (the
  owner's ANSI-ordered vogix design) fills them from slots 0 and 7, which is a
  reader's choice, not a new pipeline.
- **Polarity is derived, never read**: decision 5's WCAG rule applied to the
  page and the text. A declared polarity (tinted `variant`) is checked
  against it and a mismatch is reported.
- **Extended entries 16-21** (base16, base24) are kept in the parsed value and
  not written into the palette, so entries 16-255 stay xterm's; ghostty's own
  doc argues legacy programs assume xterm's cube.
- **Cursor and selection colours**, which every ansi16-style format carries,
  are parsed and **feed V13's decided setting**: note V decided a selection
  colour setting that defaults to reverse video. A scheme's selection colour
  is a value that setting can take; the setting stays the chooser.
- **The mapping is per format.** Adding a format is a new reader and a new
  table onto the same type.


## 5. The built-in light sixteen

**The bound first.** For any entry x between the text and the page in
luminance, (x as text on the page) × (the text on x as a background) equals
the page/text ratio exactly. On 0x27/0xEF that ratio is 12.9915, so no entry
can be better than √12.9915 = **3.6044** both ways; the dark pair's bound is
3.6538 (computed). **A light sixteen must choose between colour text and
colour backgrounds under default text.** A second identity sets the cost of
AAA: C(x, white) × C(x, black) = 21, so a colour at 7:1 on white is at most
3:1 on black, which is where GNU ls puts its `40;3x` roles. Any page/text pair
at 20.25:1 or more reaches 4.5 both ways (text up to `#060606` on `#ffffff`,
or any page from `#fbfbfb` over `#000000`), so baud's 12.99 page is not one.

**Modus Operandi, source values.** The terminal mappings are at
[[source: modus-themes-el]] 677-718, inside
`modus-themes-common-palette-mappings` (defconst at 597), which every Modus
palette appends (Operandi at 1066). The Emacs `ansi-color` mapping resolves to
decision 3's sixteen. As text on baud's 0xEF page (computed): 18.26, 6.97,
6.13, 6.14, 9.08, 9.74, 6.14, 2.12, 6.09, 7.08, 6.14, 6.09, 6.13, 8.34, 6.52,
1.15. **Every colour entry is at least 6.09**; GNU ls and lsd executables go
from 1.19 to 6.14 and symlinks from 1.09 to 6.52; SGR 30 stays 18.26 and SGR
90 is 6.09.

- **Not AAA on baud's page.** The option text called Modus "the only AAA set
  found". On its own `#ffffff` page Modus clears 7:1; on 0xEF **nothing
  reaches 7**, and Modus gives 6.09-6.13 at its weakest colour entries. Of
  ghostty's 129 bundled light themes and wezterm@b09b56c's 198, only Modus
  Operandi and its Deuteranopia and Tritanopia variants clear 7:1 on their own
  page; the next is Xcode Light hc at 6.91 (computed).
- **Its costs, stated**: colour backgrounds under 0x27 text read 1.33-2.13 (the
  3.60 bound makes that side lose); GNU ls colour-on-colour roles with the
  dircolors database read ow `34;42` 1.48, pi/sg/tw 2.98, or 2.58, su 3.29,
  st 4.29 (with GNU's built-in table, which is what lsd uses, pi is 6.14 and
  bd/cd 6.09); **SGR 1;37 becomes slot 15 = `#ffffff` at 1.15:1**, where plain
  37 is 2.12. The floor lifts the last, so decisions 3 and 6 depend on each
  other: the legible default needs the floor on.
- **The ports are not the source.** Every port traced (iTerm2-Color-Schemes,
  ghostty's bundle, kitty-themes) has ANSI 15 = `#595959`, from kitty-themes;
  no upstream release since 4.0.0 has it. On Modus's own `#ffffff` page the
  upstream `#ffffff` is 1.00:1, which is why the ports changed it. foot carries
  the pre-4.0 palette; wezterm's two variants swap 0 and 7. The author's own
  terminal exporter (modus-themes-exporter@a19c4b0, UNPINNED) differs again on
  entries 7, 10 and 11. So the Emacs mapping is the one that does not depend
  on the page, and it is the one decided.
- **Licence**: [[source: modus-operandi-theme-el]] line 3 reads "Copyright (C)
  2019-2026 Free Software Foundation", GPL-3.0-or-later (12-15); GPL-3.0
  section 10 says "You may not impose any further restrictions"
  ([[source: modus-copying]] 463), and baud is CC BY-NC-SA 4.0 (`Cargo.toml`
  line 57), whose NC is a further restriction. MIT terminals ship Modus values
  without GPL text (ghostty via the collection, foot with a "See:" line,
  wezterm compiled in); 37 CFR 202.1(a) lists "mere variations of
  typographic ornamentation, lettering or coloring" as not subject to
  copyright ([[source: ecfr-37-202-1]]). **Whether a chosen 16-value set is
  protectable is UNVERIFIED and not legal advice.** Decision 3's form is the
  defensible one: sixteen hex values in baud's own table, credited, no file
  text copied, and the `.el` files `external` so no GPL text enters the tree.

**The alternatives on baud's page** (lightset sweep, computed, refuter's
corrections applied), kept so the choice is legible:

| set | colour text min on 0xEF | ls ex, ln | notes |
|---|---|---|---|
| xterm (today) | 1.07 | 1.19, 1.09 | |
| **Modus, Emacs mapping** | **6.09** | **6.14, 6.52** | decided |
| Terminal.app Clear Light | 1.33 | 1.93, 1.33 | ships, not the default |
| iTerm2 default (Light) | 1.07 | 1.38, 1.07 | |
| GNOME / Ptyxis light | 1.73 | 1.73, 1.76 | Ptyxis forces dark by default |
| One Half Light, Tango, Solarized, Catppuccin, GitHub | 1.08-2.81 | | MIT, none legible on 0xEF |

**Bold-as-bright stays on.** It is xterm's default (`boldColors` True,
[[source: xterm-reset]] charproc.c:633), iTerm2's, Windows Terminal's,
wezterm's and baud's; ghostty, alacritty, foot, GNOME Terminal and Ptyxis
default it off. With Modus it trades emphasis, not legibility, except SGR 1;37
(1.15, floored). Turning it off does not rescue xterm's sixteen (1.88, 1.72).

**Ptyxis and Terminal.app ship a light set and default to dark**: Ptyxis's
`[Light]` face under interface-style `'dark'`; Terminal.app's Clear Light (9
of 16 entries distinct from Clear Dark; green 2.75, yellow 2.23, cyan 2.00 on
its own page) with Clear Dark as the default. Whether Terminal.app selects
Clear Light under the light appearance is UNVERIFIED. Among the terminals
read, none defaults to a distinct light sixteen; that is a census of the ones
read, not of all terminals.


## 6. Switching live, and what happens to program colours

The switching sweep read eleven implementations at fixed commits. VERIFIED
unless marked:

| terminal | pair | trigger | OSC 4 on a switch | OSC 10/11/12 on a switch | OSC 4 query | DECSTR / RIS on OSC 4 |
|---|---|---|---|---|---|---|
| ghostty | `light:X,dark:Y` | NSApp.effectiveAppearance (KVO); GTK via libadwaita | kept | kept | every entry | RIS keeps |
| kitty | three auto.conf files | effectiveAppearance; the portal on Linux | **dropped** | **dropped** | every entry | both drop, and 10/11/12 too |
| foot | `[colors-dark]`/`[colors-light]` | key binding or SIGUSR1/2 only | **dropped** | **dropped** | every entry | RIS drops |
| wezterm | Lua `get_appearance()` | viewDidChangeEffectiveAppearance; the portal | kept, pane forks the whole palette | kept | every entry | RIS drops the fork |
| contour | profile `colors: {light, dark}` | Qt, only while the GUI theme is system | **dropped** | **dropped** | every entry | both drop |
| Windows Terminal | `colorScheme: {light, dark}` | WM_SETTINGCHANGE | **dropped** 0-15 | **dropped** | every entry | RIS restores defaults |
| iTerm2 | per-profile (Light)/(Dark) keys | viewDidChangeEffectiveAppearance | **kept by design** | kept | every entry | RIS drops |
| VTE, Ptyxis, GNOME Terminal | Ptyxis faces | libadwaita | kept | kept | every entry | **both reset 0-255, keep 10/11/12** |
| alacritty | none | none | kept | kept | every entry | RIS keeps |
| xterm | none | none | – | – | every entry | **both reset all, keep 10-19** |
| **baud, decided** | built-in or named files | glass crate `Responder::appearance`, `auto` only | **dropped** | **dropped** | every entry (`e4db454`) | both drop, keep 10/11/12 |

- **ghostty keeps overrides** because `DynamicPalette.changeDefault`
  re-applies the masked entries ([[source: ghostty-color-d9840f3]] 361-376).
  **iTerm2 keeps them on purpose**: OSC colours are written to the base, Light
  and Dark keys, "Writing all three makes the override survive both
  appearance switches" ([[source: iterm2-ptysession]] 17552-17565). **kitty
  documents dropping them**: "the next time the OS changes its color mode,
  any dynamic changes will be overridden" ([[source: kitty-themes-rst]]
  78-79). foot's `term_theme_apply` overwrites fg, bg and the table
  ([[source: foot-terminal-cb27717]] 2085-2096); contour's
  `resetColorPalette` replaces the whole live palette
  ([[source: contour-terminal-cpp]] 5506-5515).
- **On macOS every terminal read follows NSAppearance**, by KVO on
  `NSApp.effectiveAppearance` or by `viewDidChangeEffectiveAppearance`; none
  reads the Icon & widget style. That is decision 4's majority. On Linux the
  signal is the Settings portal's `org.freedesktop.appearance color-scheme`,
  0 no preference, 1 dark, 2 light, with `SettingChanged`
  ([[source: xdg-portal-settings]] 22-56); kitty and wezterm read it directly,
  libadwaita reads it itself ([[source: libadwaita-style-manager]]). GNOME
  reports light as 0 ([[source: kitty-themes-rst]] 84-89).
- **What decision 5 costs, stated**: a program that set its colours once at
  startup loses them on a flip. That includes pywal-style shells, cava's OSC 4
  entries, and the owner's vogix shell hook, which sends OSC 4 0-15 and OSC
  10/11 once from bash/zsh initExtra (vogix@ad2e814
  nix/modules/home-manager/default.nix:627-666, UNPINNED). kovid argued in
  kitty#9253 for keeping escape-set colours (issue text, not commit-pinned).
  The rule is what makes the reported class equal both the OS appearance and
  the page's luminance after every flip.
- **baud today** already switches on `Responder::appearance`: `apply_style`
  calls `set_theme(theme_for(appearance))` and `tell_defaults`, and
  `set_default_colors` writes only the embedder defaults, so every override
  survives (`baud-app/src/app.rs` lines 5574-5608,
  `baud-core/src/term.rs` lines 2298-2307 at `c836eaf`). A pinned theme
  returns early (5593-5600). The palette itself is xterm's in both themes
  (`baud-render/src/color.rs` lines 156-193).


## 7. Mode 2031 and DSR 996/997

**The spec** is [[source: contour-palette-notifications]]. VERIFIED:

- "Send `CSI ? 996 n` to the terminal to explicitly request the current color
  preference (dark mode or light mode) by the operating system" (23-24).
- The replies are `CSI ? 997 ; 1 n` for dark and `CSI ? 997 ; 2 n` for light,
  and nothing else (28-31).
- `CSI ? 2031 h/l` enables unsolicited reports "for color palette updates",
  "not just sent when dark/light mode has been changed by the operating system
  / desktop, but also if the user explicitly changed color scheme, e.g. by
  configuration" (35-41), and "only ... when the palette has been updated due
  to a change in the terminal emulator's color palette" (43-47).
- **The query is worded as the OS preference and the notification as the
  palette.** The spec never says which colours classify a palette, whether
  an application's OSC 10/11 counts, or whether to report on enable. Under
  decision 5's reset rule, the OS preference and the palette's class agree
  after every flip under `auto`, so the tension closes there.
- **contour does not implement its own "by configuration" clause**: a config
  reload reaches `updateColorPreference`, which returns early on an unchanged
  preference ([[source: contour-session-cpp]] 791-794), so it neither changes
  the live palette nor sends 997.

**What others report** (notify sweep, refuter's corrections applied): the
effective background including an application's OSC 11 (kitty,
[[source: kitty-window-py]] 1654-1669; iTerm2; VTE; xterm.js); the terminal's
chosen theme (ghostty, whose 997 is `conditional_state.theme`,
[[source: ghostty-termio-d9840f3]] 743-756; foot's active theme); the default
foreground (contour). ghostty-GTK under its default `auto` reports the
desktop's preference whenever one is stated (libadwaita,
[[source: libadwaita-style-manager]] 315-318), so it is a hybrid. wezterm,
alacritty and xterm have no 2031 (each with a positive control).

**Consumers fall in two classes:**

- **Trigger** (neovim, fish, lazygit): a 997 only prompts an OSC 11 re-query.
  neovim sends `?2031h` **only when DECRQM ?2031 answers 2**
  ([[source: neovim-tui-c]] 225-266), needs exactly two parameters and
  re-queries 100 ms later ([[source: neovim-tui-input-c]] 771-800). lazygit
  falls back to trusting 997 when OSC 11 is not answered.
- **Trust** (helix master, yazi, tmux, zellij, kitty kittens): the flag is
  the theme decision. **helix is two versions here**: the installed 25.07.1
  (`a05c151`) neither sends 996 nor reads OSC 11; master sends 996 with OSC 11
  and DA1 in one burst and subscribes only if the 997 arrives before DA1
  ([[source: helix-termina-079a789]] 120-167). tmux sends `?2031h?996n` to the
  outer terminal when TERM is VT100-like ([[source: tmux-tty-c]] 386-389;
  baud's xterm-256color has `clear=\E[H\E[2J`,
  [[source: terminfo-xterm-256color]] line 23), answers a pane's 996 itself
  and stays silent when it does not know ([[source: tmux-input-c]]
  3715-3738).
- **The reply must be exactly `CSI ? 997 ; 1|2 n`**: neovim needs two params,
  kitty's kittens read the byte before `n`, termina, yazi and zellij match
  exactly, and tmux#4269 rejected `997;0n`.
- **No report on `2031 h`**: tmux removed one after it raced an exiting
  program and typed `997;2n` into weechat (tmux#4787, issue text, not
  commit-pinned); every verified implementation sends none; the spec is
  silent.

**Classifiers disagree only where the pair is illegible anyway.** On baud's
two defaults and Solarized dark and light every rule read agrees (computed).
At `#808080` on white, four rule groups say light (BT.601 as kitty, neovim and
ghostty-mac use it; iTerm2; tmux; CIE L*) and six say dark (yazi, VTE,
colorsaurus, contour, the Oklab comparison, xterm.js), and that pair is
3.95:1, below AA. baud's WCAG comparison says dark there too: the page's
relative luminance 0.216 is below the text's 1.0.

**baud today** (VERIFIED at `c836eaf`): no 2031, 996 or 997 anywhere in
baud-core, baud-domain, baud-app or baud-render (control: `?2026` in
`baud-domain/src/vt/mode.rs`); DECRQM ?2031 answers 0, not recognised, so
neovim would never subscribe; `CSI ? 996 n` falls to `Csi::Unsupported`,
counted and silent. **baud has no XTSAVE**, so nothing needs saving. The
precedent for an embedder-triggered, mode-gated report already exists:
`Term::focus` queues a report only when `?1004` is set.

**The glass crate provides the macOS half, unchanged.**
`Responder::appearance(Light | Dark)` fires from
`viewDidChangeEffectiveAppearance` on the system light/dark axis
([[source: mlg-drawable]] 190-195, 255-269, 420-429; the pin is the owner's
repository at `ccf01c8`, whose file hashes equal the 1.0.0-beta.5 crate baud
builds), and baud already routes it through `Driver::appearance` to
`apply_style`. The crate's "tint" is not a light/dark signal:
`WidgetStyle::tint()` is always `None` without the off-by-default
`private-spi` feature, and `GlassSurface::set_tint_color` tints the material.
(baud enables the crate's default features plus `drawable`.) On the owner's
machine the two axes disagree: AppleInterfaceStyle is Dark and
AppleIconAppearanceTheme is ClearLight.

**The design for 2031, 996 and 997** (decision 5, with the architecture
critic's change):

- **One value, what OSC 10 and 11 would answer.** 996 and 997 report the class
  of the colours baud's OSC 10/11 queries would answer at that moment: dark
  when the background's WCAG relative luminance is below the foreground's. So
  996 can never contradict OSC 11. When OSC 11 would be silent (no embedder
  default), 996 is silent too, as tmux is.
- **One embedder call, one report at most.** The embedder's change is one
  core call, for example a set-theme-colours call taking the foreground, the
  background, the table and whether to drop the overrides. It computes the
  class before and after the **whole** change and queues one 997 when they
  differ and the mode is set. There is **no stored last-reported byte**: the
  draft's byte had no defined lifecycle (whether it updates while the mode is
  off, on a 996 answer, on RIS, at the launch pair of `tell_defaults` calls at
  `baud-app/src/app.rs` lines 790 and 4761), and two separate calls, a drop
  then a push, could report a transient class or miss a real one. Queued
  through the core's private reply queue after the new colours are in place, so the OSC 11
  re-query that follows gets the new colour. `apply_style` runs in `pump`
  before the drain and replies are flushed after it, so the 997 leaves in the
  same pump (critic-verified).
- **Never from the application's own escapes**: OSC 4/10/11/12/104/110-112
  and `?5` queue nothing (kitty, VTE, contour and ghostty agree; xterm.js,
  which reports on every colour write, is the counter-example). They do change
  the next 996 answer.
- **DECRQM ?2031 answers the real bit, reset at power-on**, so neovim
  subscribes. RIS clears the mode. Nothing is sent on `2031 h`.
- **A pinned theme sends nothing on an OS flip**: nothing an application draws
  on changed, and there is no standard channel for the raw OS appearance; 997
  must not be overloaded with it. **This drops OS awareness under a pinned
  theme**, which the owner's "os appearance" wish would have wanted; the
  research found no channel that carries it.
- **The glass material, the widget style and dim_light never enter 997**,
  since OSC 11 cannot express them either.
- **Under `?5`**, once V17 makes OSC 11 answer the shown colour, 996 follows
  it; no unsolicited report for `?5`. V17 waited on xterm's misc.c, which is
  now pinned ([[source: xterm-misc]]).
- **One residual hazard is not fixed by any option**: helix master restores
  the background it queried at startup with an explicit OSC 11 on exit
  ([[source: helix-termina-079a789]] 295-306), which re-installs an
  old-polarity override after a mid-session flip until the next palette
  change. kitty has the same exposure.
- **Linux** is the same core logic plus the portal's `color-scheme` and
  `SettingChanged` in the Linux driver, which today resolves `auto` to dark
  (`baud-app/src/linux.rs` lines 120-127). Deferred by the owner ("we will
  need for linux but that is later work"); the portal's value 0 is left
  undecided until then.


## 8. The contrast floor

**What the implementations do** (floor sweep, VERIFIED at the commits named,
refuter's corrections applied):

| terminal | default | rule | exact equality | measured against |
|---|---|---|---|---|
| ghostty `minimum-contrast` | 1 (off) | below the minimum, **replace with pure white or black** ([[source: ghostty-shaders-d9840f3]] 111-124) | changed | the premultiplied cell colour over the global background, so **black under glass** |
| kitty text_fg_override_threshold | 0 (off) | `%` mode snaps to black or white; ratio mode moves HSLuv L | ratio mode exempts it | the stated, opaque colour |
| iTerm2 Minimum Contrast | 0.0 | a brightness **difference**, not WCAG, moved along encoded RGB | changed | the stated colour, alpha ignored |
| Windows Terminal adjustIndistinguishableColors | Never (Indexed in high contrast) | Oklab L moved when deltaE_OK < 0.5 ([[source: wt-colorfix]], [[source: wt-rendersettings]]) | **exempt** (`fg != bg`) | the stated colour |
| wezterm text_min_contrast_ratio | nil | WCAG test, Oklab L set to a luminance, a bug at 1085 ([[source: wezterm-color-types]] 1079-1085) | exempt, but compares alpha, so it fails under glass ([[source: wezterm-screen-line]] 861-871) | the stated colour |
| alacritty | – | only a cursor fallback at 1.5 | – | – |
| foot, VTE | – | none | – | – |

- **ghostty's doc** ([[source: ghostty-config-d9840f3]] 769-779): "If you
  want to avoid invisible text (same color as background), a value of 1.1 is a
  good value. If you want to avoid text that is difficult to read, a value of 3
  or higher is a good value. The higher the value, the more likely that text
  will become black or white." **So the option the owner endorsed mixed two
  mechanisms**: ghostty snaps, it does not nudge, and at 1.1 it turns symlinks
  to `#000000` at 18.26:1. ghostty's code exempts box drawing, block
  elements, legacy computing and Powerline, not braille
  ([[source: ghostty-cell-d9840f3]] 294-345); its doc names only emoji and
  images. Decision 6 keeps the owner's "nudge" and lifts by a mix, not a snap.
- **iTerm2 applies its per-appearance keys** when separate light and dark
  colours are on ([[source: iterm2-ptysession]] 5864-5869).
- **Do not port wezterm's arithmetic or kitty's ratio mode.** wezterm sets
  Oklab L to a WCAG luminance and reads `increased_ratio` from `reduced_col`
  ([[source: wezterm-color-types]] 1079-1085). kitty's ratio mode depends on an
  unresolved matrix-orientation question in hsluv.slang (UNVERIFIED), so its
  figures are dropped as evidence.
- **WCAG**: [[source: wcag22]] defines relative luminance (3597-3605) and the
  ratio (2866), whose note measures "with respect to the specified background
  over which the text is rendered in normal usage"; SC 1.4.3 is 4.5:1 (782),
  1.4.6 7:1 (873), 1.4.11 3:1 for non-text (1022). The WCAG 3 draft says "The
  contrast algorithm used in WCAG 3 is yet to be determined"
  ([[source: wcag3-wd]] 980, 1030, 1873) and never names APCA (0 hits).

**What a 1.2 trigger catches**, strict `<` (computed):

| pair | ratio | below 1.1 | below 1.2 |
|---|---|---|---|
| SGR 93, xterm yellow on 0xEF | 1.0708 | yes | yes |
| SGR 96, symlinks | 1.0905 | yes | yes |
| SGR 37, htop default text | 1.0955 | yes | yes |
| SGR 30 on the dark page 0x10 | 1.1036 | no | yes |
| `?5` then SGR 30 on Theme::dark (the reverse_video test) | 1.1315 | no | yes |
| SGR 97 on 0xEF | 1.1499 | no | yes |
| SGR 92, executables | 1.1934 | no | yes |

Over the 201 pairs: 25 below 1.1 and 44 below 1.2 under xterm light; 1 and 5
under Modus (section 3). **The palette fixes ls; the floor catches what the
palette cannot**: cube colours, bright white, and palette collisions such as
SGR 30 on a base16 page (1.00 by construction under the published mapping).

**Exact equality is where the implementations split, and it is the case that
matters.** Under base16, SGR 30 on the page and default text on SGR 47 are
exactly 1.00. A floor that exempts resolved equality (wezterm, Windows
Terminal, kitty's ratio mode) leaves exactly those unfixed. **And a floor
that exempts nothing wrecks pictures drawn in text**: in
`tests/previews/chafa-240.ans` 229 of 487 glyph cells, and in
`tests/previews/chafa-truecolour.ans` 590 of 834, are below 1.2:1 because
neighbouring pixels are close in colour (computed).

**The per-slot rule** (decision 6's internal choice). This is this
synthesis's own proposal; no sweep or refuter checked it, and no
implementation does exactly this (Windows Terminal's Indexed mode, which
requires both sides to be default or 0-15, is the nearest). The floor lifts a
slot's text only when:

1. its contrast is below the trigger;
2. the two rendition words differ (the same non-default `ColorWord` on both
   sides is a deliberate choice; `ColorWord` is a tagged u32 with
   `PartialEq`, `baud-domain/src/style.rs` lines 357-361); and
3. at least one side is a **default** word or an index 0-15. **Stated as the
   code does it**: a DEFAULT word resolves through `Theme::defaults`, which
   consults the application's OSC 10/11 first (`baud-render/src/color.rs`), so
   "the default" includes an application's OSC 11 page, and **truecolour or
   cube text on the default page is in scope**. That is the normal shape of
   btop and helix, not only palette collisions.

Computed over the 201 pairs: every pair below the trigger is lifted under
xterm light, Modus light and xterm dark (none is exempted by 2 or 3). Computed
over the two chafa captures: **0 cells lifted**, because chafa sets both sides
as explicit truecolour or cube colours. On the recordings at the dark page, no
truecolour or 256-colour foreground is below 1.2 (btop's lowest is `#303030`
at 1.44; asciiquarium's black-on-SGR 40 glyphs are the same word on both
sides), so the nine render digests should survive a 1.2 floor (the
architecture critic's inference from scans; **not rendered**). **Its holes,
stated**:

- a 16-colour picture (chafa `--colors 16`, both sides 0-15) is still floored,
  because the rule never consults the picture finder; no such capture exists
  here, so it is unmeasured;
- a deliberate hide with explicit RGB equal to the default page (`38;2;` equal
  to the page on a default background) is floored, where Windows Terminal and
  wezterm leave it;
- box drawing in an index colour on the default page is floored; ghostty
  exempts it, iTerm2 does not;
- **under Increase Contrast** (trigger 3.0), btop's graph greys `#303030`
  (1.44, 734 uses in `btop.raw`) and `#404040` (1.84, 981 uses) on the default
  page are lifted. Decision 9 accepts that. A test pins which of these pairs
  the rule lifts at each trigger.

**The fallback** keeps the unfloored text colour in the slot, so per-cell
exemptions (box drawing, block elements, braille or not, Powerline, and the
picture finder's cells) can pick it: `Resolved` goes from 16 to 20 bytes (+256
KiB per copy at 65,536 slots), `Resolved::BYTES`, the shader's `struct Style`
and the `.metallib` change, and nothing asserts `size_of::<Resolved>() ==
BYTES` today (`baud-render/src/cellmap.rs` lines 418-428), so a missed update
silently drops the app off the cell path. It is built only if a measured hole
matters.

**What the floor lifts to** (decision 6): the text colour mixed toward black
or white in linear light, just far enough to reach **4.5:1**, keeping the
hue: `#00ff00` to `#007f00` at 4.53, `#00ffff` to `#007979` at 4.55,
`#ffffff` to `#6c6c6c` at 4.57, all on 0xEF (computed). Toward black when the
background is lighter than the text would need, toward white otherwise; when
neither reaches 4.5 (a mid-grey page), the direction with the higher reachable
ratio, stopping at the extreme. ghostty's snap is the alternative it does not
take.

**Where it lives, exactly** (the architecture critic's correction; the draft
said "at the end ... before `with_alpha`", which contradicts itself because
`bg.with_alpha(self.theme.bg_alpha)` is `resolve_slot`'s second statement,
`baud-render/src/renderer.rs` line 1836):

- **Immediately after `theme.resolve` returns the opaque (fg, bg)**, before
  `with_alpha`, and before `theme.underline(...)` is called, so the SGR 58
  default follows the floored fg and `UNDERLINE_FOLLOWS_FG` (set at 1865) still
  holds on floored cells.
- **Faint runs inside `Theme::resolve`, before the floor** (decision 10). The
  floor may lift faint text that falls below the trigger; a test pins that.
- An explicit SGR 58 underline colour gets the same rule against the
  background, or it is never floored.
- It is cached per slot like every other rendition rule: **a steady frame pays
  nothing**. It re-runs on a full pass (a theme, video or colour-epoch change)
  or for newly written slots. Its parameters live in `Theme`, so `ThemeGen`
  covers a change of trigger.
- The selection and block-cursor swaps preserve the ratio (they exchange both
  sides and force both opaque), though the visible highlight colour on a
  floored cell moves with the text. **The `OSC 12` cursor fill bypasses it**:
  one cell a frame, left alone.
- **Every render path gets its colours from `resolve_slot`** (critic-verified:
  `resolve_styles_into`, `sync_styles`' replay and full pass, `convert_row`,
  overlay records, the predicted echo and the cell shader's table). Pictures
  read colour words, not resolved colours. So the floor reaches all three paths
  once and the `.metallib` does not change under the per-slot rule.
- The linearisation table is `baud-domain`'s, one 256-entry sRGB decode shared
  with the 996 classifier, not a second copy in baud-render.

**Cost** (floor sweep's scratch bench, not baud code; refuter-corrected): the
check is about 1.3-2.9 ns a slot with a 256-entry linearisation table; a lift
is 27-54 ns a failing slot. **A full pass resolves live slots only** while the
table has room, typically a few hundred, so the floor adds well under a
microsecond. **The worst case is larger and is measured before it is
claimed**: once the rendition table has filled, a full pass resolves every
slot, 65,536 at the app's provisioning (`sync_styles`, `baud-render/src/renderer.rs`
lines 1797-1805), and a chafa or Sixel preview mints thousands of renditions.
After that, every `?5` flip, flash edge, theme flip or OSC 4/10/11 write costs
a full pass plus the floor on every slot, per buffer set. An arm times
`sync_styles` on a filled table with and without the floor.

**The pixel oracle cannot check the floor**: both GPU arms read one resolved
table, so a wrong floor is drawn identically twice. It needs its own reference
test against the WCAG formula.


## 9. Glass, dim_light, and the accessibility settings

- **What is drawn.** Every background carries `bg_alpha`, from `[glass]
  dim_dark` 0.22 and `dim_light` 0.12, which `Chan::from_unit` rounds to 56 and
  31. The owner's own config sets `dim_dark = 0.5` and no `dim_light`. The dim
  follows the palette's axis, AppleInterfaceStyle; the material follows the
  Icon & widget style (`baud-app/src/app.rs` lines 1818-1846).
- **0.12 is glassterm's value**, adopted on 2026-09-20 when the owner chose
  "every knob, like glassterm" (<session> l.9700; `config.toml.example`).
  glassterm dims black under light text; baud applies the number to a 0xEF page
  under dark text. **That is why decision 7 measures instead of keeping it.**
  That light mode is a light terminal was settled on 2026-09-25, by the
  selected option's "a built-in light sixteen ... on today's light page".
- **What a page alpha does to default text over the worst backdrop**
  (computed; text drawn opaque over the blended page):

  | page | backdrop | 3:1 needs | 4.5:1 needs |
  |---|---|---|---|
  | light 0xEF, text 0x27 | black | 0.186 linear / 0.467 encoded | 0.309 / 0.590 |
  | dark 0x10, text 0xD8 | white | 0.809 / 0.555 | 0.891 / 0.671 |

  At the default 0.12, 0x27 over black is 2.18:1 (linear) or 1.13:1
  (encoded); at 0.5, 6.85 or 3.36. **Which space Core Animation blends in is
  UNVERIFIED**, and the real backdrop is the tinted material, not black, so
  these are bounds. Decision 7's measurement settles both.
- **The one measurement on record is weak.** CLAUDE.md records the light body
  at about (78.1, 86.4, 101.6), sd 18.6, the mean of the window's lower half on
  one wallpaper, from a screenshot of a P3 panel read as sRGB.
- **So the floor measures against the stated, opaque colour** (WCAG 2.2's
  "specified background"; kitty, Windows Terminal, iTerm2 and wezterm), never
  ghostty's premultiplied one, which under glass is black and would turn a
  light theme's text white at a trigger of 3. **What is seen under glass is the
  dim's job, and the dim is chosen by measurement.**
- **The luma instrument's premise is stale.** `scripts/luma-swatch.sh` says
  "opaque truecolour background", which stopped being true when every
  background took the dim; reproducing the CLAUDE.md measurement needs
  `[glass] app_backgrounds = "opaque"` or dims of 1.0. It is fixed before the
  measurement is taken.
- **Reduce Transparency and Increase Contrast** are read by the glass crate
  (`accessibility::reduce_transparency` and `increase_contrast`,
  [[source: mlg-accessibility]] 58-70). baud reads neither today; under Reduce
  Transparency baud's own cells stay translucent, which contradicts the
  crate's doc: "Honouring the setting means not using the material at all"
  ([[source: mlg-accessibility]] 3-16). Decision 9 honours both. The Reduce
  Motion poll on the tick is the precedent (about 0.04 us a read).
- **Linux draws a pinned `theme = "light"` at `dim_dark`**
  (`baud-app/src/linux.rs` line 133), where macOS uses `dim_light`.
  `config.toml.example` documents dim_dark on Linux; the comment's reason at
  128-130 ("unreachable ... for as long as Auto is") is wrong for a pinned
  light theme. A defect fix, not a question.


## 10. OSC 4 queries and resets, under "protocols fully, always"

**xterm's misc.c is pinned**, at the same snapshot as [[source: xterm-reset]]:
[[source: xterm-misc]]. VERIFIED:

- `ReportAnsiColorRequest` answers any entry's current colour, gated only by
  `AllowColorOps` (2636-2659).
- `ResetAnsiColorRequest` with an empty buffer resets all `MAXCOLORS` entries
  (3051-3085); `ReallyReset`'s common path calls it on both resets
  ([[source: xterm-reset]] charproc.c:14402-14407, with `CASE_RIS` calling
  `VTReset(xw, True, True)` and `CASE_DECSTR` `VTReset(xw, False, False)` at
  6149-6156).
- `ResetColorsRequest`, which resets OSC 10-19, is called only from the OSC
  110-119 dispatch (around 4291). So xterm keeps OSC 10-12 on both resets.
- **This is also the pin V17 waits on**, so V17 can proceed.

**baud at `c836eaf`** (VERIFIED):

- `OSC 4 ; n ; ?` is answered only for an overridden entry
  (`baud-core/src/osc.rs` lines 268-290), because "no built-in palette to
  report" (`baud-core/src/term.rs` lines 2272-2279), while
  `docs/vt-coverage.md` said "done -- including the ? query".
- `hard_reset` and `soft_reset` touch no colour (`baud-core/src/ops.rs`).

**The OSC 4 half is built, at `e4db454`**, which landed while this note was
written: it takes placement (a) below, and its record is its commit message
and `baud-core/tests/palette_query.rs`. It is described here, not re-verified
by this note. **The resets are built too, at `534643a`**: one primitive,
`Paint::reset_palette`, serves OSC 104 with no parameter and `soft_reset`
(which `hard_reset` calls), writing through `set_palette` so the colour epoch
moves only when an entry was overridden; its tests are
`baud-core/tests/palette_reset.rs` and a renderer test on incremental damage.
The sfte cases below followed in their own commit, and what they found is
in the next paragraph.

**The design** (the architecture critic's changes applied):

- **Answer every entry.** The embedder hands the core the theme's 256
  entries; the query answers the override, else the embedder's entry, else
  silence. **Placement**: (a) widen the existing `palette` slice to 512
  `ColorWord`s, overrides in 0..256 and the embedder's in 256..512, the
  `Storage::new` assert at `>= 512` and `Owned` allocating 512, `StrBuf::new`
  keeping its six arguments; or (b) a seventh `StrBuf` field. (a) is the one
  taken at `e4db454` (`PALETTE_LEN`, `EMBEDDER_PALETTE`). **Either
  way `baud-bench/benches/ingest2.rs` builds `Storage` and `StrBuf` by hand
  (lines 300-343) and changes in the same commit**, because
  `scripts/check-features.sh` builds every bench arm.
- **This reverses three recorded decisions, named**: `term.rs` 297-306 (the
  embedder defaults went into `Cold` "rather than in `StrBuf` so no
  caller-provided buffer grows a field"), `term.rs` 2272-2279 and `osc.rs`
  273-278 (the core "deliberately holds no default palette ... a second copy
  here would be one to keep in step"). Those three doc comments are rewritten
  in the same commit (`e4db454` rewrote the `term.rs` ones and the `osc.rs`
  one). The second copy is held in step by a gate: after `tell_defaults`,
  every `OSC 4` answer equals the theme's entry, for all 256 entries and both
  themes (`every_osc_4_answer_is_the_colour_the_theme_draws`, in
  `baud-app/src/host.rs`), the invariant `set_default_colors` already states
  for fg and bg.
- **`Term::palette` stays override-only**, because `Theme::entry` treats
  `None` as "use your own".
- **The table moves no colour epoch.** The renderer never resolves through the
  core's copy, and `the_colour_epoch_moves_exactly_when_a_resolve_input_moves`
  already pins that the embedder's query defaults do not move it
  (`baud-core/tests/style_log.rs` line 267). The new setter gets the same step.
- **The table is pushed inside `host::tell_defaults`**, which has four call
  sites (`baud-app/src/app.rs` lines 790, 4761 and 5604, and
  `baud-app/src/linux.rs` line 146), not at each theme-building site.
- `querying_an_unset_colour_is_silent` feeds a bare `Term` with no table and
  stays green.
- **Resets follow xterm**: DECSTR and RIS clear every OSC 4 override and keep
  OSC 10/11/12, through one primitive with a scope, which the palette-switch
  drop (decision 5) reuses with the wider scope (OSC 4, 10, 11, 12). The colour
  epoch moves only when a value changes. The sources conflict on DECSTR: xterm
  and VTE reset OSC 4 on both resets; foot resets on RIS only, and fg and bg
  too ([[source: foot-term-reset]] terminal.c:2147-2162 at 1.21.0); kitty and
  contour reset 10/11/12 as well. **This repository already takes xterm as the
  authority for what DECSTR resets**, so xterm's rule is the design.
- **The oracle is sfte, not baud-difftest.** baud-difftest compares only each
  cell's character and whether its background word is set; it never compares
  RGB, the palette or reply bytes, so an OSC 4 override followed by RIS cannot
  differ there, and a "pinned difference" could not be expressed. The
  sfte harness compares reply bytes, so the oracle is two new sfte cases,
  "OSC 4 set, RIS, OSC 4 query" and "OSC 4 set, DECSTR, query", pinned at
  whatever sfte answers. The sfte harness's baud side never calls
  `tell_defaults`, so after the change baud is silent there after a reset.
  **Built, and sfte turned out to answer no `OSC 4` query at all**: a third
  case, OSC 4 set then queried with no reset, is pinned in `MUST_DIFFER` as
  baud's `rgb:1212/3434/5656` against sfte's empty reply. So the two reset
  cases agree by both sides being silent. They gate baud's side -- with the
  clear removed from `soft_reset` both fire and the harness exits 1 -- and
  carry no evidence from sfte's.


## 11. The design

Each rule is written once, in the crate where its inputs live.

**baud-domain (no_std, pure):**

- **One sRGB decode**: a generated 256-entry sRGB-to-linear table, emitted as
  byte strings per the repo's generator rule, and WCAG relative luminance of an
  8-bit colour from it. The 996 classifier and the floor both use it.
- **A hex reader** (`#rrggbb`, `#rgb`, bare hex) returning
  `baud_domain::Color::Rgb` (`baud-domain/src/style.rs` line 232), not a second
  `Rgb` type. `Chan::from_u8` (line 199) and `ColorWord::rgb` (371) are not
  `const fn`; either make them `const` or drop the `const` requirement. X11
  names, `rgb:`, `#rrggbbaa` and keywords (`none`, `background`, `cell-*`) are
  refused and reported, one test a form.
- **Readers** in the `baud-domain/src/terminfo.rs` pattern, bytes to value,
  refusing what the grammar leaves unsettled: the base16 and base24 YAML subset
  (current nested form, legacy flat form, block scalars skipped, `.yml` and
  `.yaml`) and ghostty theme files (own tokenizer, `#` a comment only at line
  start, a key allowlist: `palette`, `background`, `foreground`,
  `cursor-color`, `cursor-text`, `selection-background`,
  `selection-foreground`). kitty, Windows Terminal JSON, tinted8 and
  Alacritty TOML later; `.itermcolors` never (read the generated ghostty/
  files).

**baud-core:**

- The embedder's 256-entry table; OSC 4 queries answer every entry (built,
  `e4db454`).
- Resets per xterm (built, `534643a`); the clear widens to a scope for the
  palette switch (OSC 4, 10, 11, 12).
- Mode 2031 (a `ModeBit`; `c836eaf` already made the bitset bound count the
  variants), DSR 996, the exact 997 report, the WCAG class of what OSC 10/11
  would answer, and at most one unsolicited 997 per embedder call, from a
  before/after comparison inside that call. Nothing grows `Cold` for it.

**baud-render:**

- **The scheme type**: sixteen slots, page, text, optional 16-21, optional
  cursor and selection, the format; polarity derived by the WCAG rule.
- **`Theme::light()` gets Modus Operandi** in entries 0-15; 16-255 stay
  xterm's. `xterm_palette()` and `Theme::dark()` do not change. The doc comment
  on `Theme::light` (`baud-render/src/color.rs` lines 162-176, "Choosing the
  numbers is the owner's call and is not made here") is rewritten in the same
  commit.
- **The floor** in `resolve_slot`, placed as section 8 says, trigger and target
  in `Theme`.
- **Faint** mixes the drawn text colour toward the colour it is drawn on, after
  the reverse swap, inside `Theme::resolve`.

**baud-app:**

- **`[palette]`**, placed after the last top-level key (`fonts`) and before
  `[cursor]`, since a section header swallows every top-level key after it.
  Values quoted, because `strip_comment` cuts an unquoted `#`:

      [palette]
      # light = "~/.config/baud/themes/day.toml"
      # dark = "~/.config/baud/themes/night.yaml"
      # floor = 1.2   # the contrast floor's trigger; "off" turns it off

- **`~` and bare names are expanded** with the same HOME/XDG logic
  `read_first` uses (`baud-app/src/config.rs` line 833); config.rs has no tilde
  expansion today, so the example's path would be read literally. A bare name
  is looked up in `~/.config/baud/themes/`.
- **`floor` has its own ratio parser**, 1.0 to 21.0 or `"off"`. `unit()`
  (line 766) clamps to 0-1 with a warning, so 1.2 through it would become 1.0,
  which is off.
- **`apply()` stays pure** (line 438): it stores path strings and values.
  Loading theme files is I/O and has its **own complaint list**, with its own
  test against committed fixture files; the example gate,
  `the_committed_example_parses_to_what_it_documents`, asserts the section's
  keys land, not that the files load.
- **Precedence, stated.** `light` and `dark` name the two halves;
  `theme = "auto" | "dark" | "light"` picks which half is drawn, as it does
  today; unset halves are the built-ins (xterm's dark sixteen, Modus light).
  On Linux, where `auto` resolves to dark, a named light half is reachable only
  with `theme = "light"`, and the example says so. Under Increase Contrast the
  trigger is the larger of the config's and 3.0; while Increase Contrast is on
  it overrides `floor = "off"`, as Reduce Transparency overrides the dim
  (decided here by analogy with decision 9; the owner can reverse it).
- **Both themes are built once, at Boot.** `apply_style` runs on the main
  thread in `pump`, so reading theme files at a flip would put disk I/O in the
  frame loop and repeat the warnings. A flip selects one of two ready Themes
  and their core tables.
- **The accessibility poll has its own path.** `apply_style` returns early for
  a pinned theme before `set_theme` (`baud-app/src/app.rs` lines 5593-5600),
  so an Increase Contrast or Reduce Transparency change routed through it would
  never reach a pinned-theme user. The poll rebuilds whichever theme is in
  force, pinned or auto, and calls `set_theme` and `tell_defaults`; the pinned
  early return governs only the appearance-driven palette swap.
- **On an OS flip under `auto`**: if the palette changed, one core call drops
  the application's colours, pushes the table and the defaults, and lets the
  core report 997 if the class moved. The pinned branch: no drop, no report.
- **Linux**: a pinned light theme at `dim_light`; the portal later.

**What does not change:** the per-character path; the shader and the
`.metallib` under the per-slot rule; entries 16-255; bold-is-bright; LS_COLORS
and the child's environment. **What does change on purpose**: the
`reverse_video.rs` pin named in decision 6, and any render golden the floor
moves, listed by running `cargo test -p baud-render` and `-p
baud-present-macos` with the floor on before anything is claimed.


## 12. Build order

One commit per step (a finished feature is squashed to one commit before the
next starts). Every step: red first at the levels named (unit, hand model,
recorded where a recording exists), then one perturbation per mechanism, each
red at its own assertion, then the measurement owed. Every baud-core step runs
`cargo test --workspace` before commit, because the alacritty and sfte oracles
live outside baud-core. Test names below that do not exist yet are written
without code formatting.

**For every baud-core step, CLAUDE.md's two questions are answered no**: the
paths are cold (a query, a reset, a mode, an embedder call), nothing is
written per ingested byte, and the palette slice sits behind the `&mut StrBuf`
reference `Paint` already carries, not in `Paint` or `Cold`. An ingest sweep
still runs, to show it.

0. **ModeBit's bound. Done at `c836eaf`**: the const assertion now counts the
   variants (`variant_count::<ModeBit>() <= 32`) instead of naming a `LAST`
   that had gone stale twice. Perturbed: at a bound of 18 the new assertion
   fails the build where the old one compiled; `ingest2` built before and
   after at one path is byte-identical (the commit's own record).
1. **OSC 4 answers every entry** (baud-core, baud-app, baud-bench). **Built at
   `e4db454`**; its commit records red first (three of five tests in
   `baud-core/tests/palette_query.rs` red against a stub, two controls), four
   perturbations and an `ingest2` sweep with every range overlapping. What
   the step asked for, for the record:
   - Unit: an unset entry answered from the embedder's table; an override wins;
     entries 16 and 255 answered (cube and ramp, pushed, not re-derived); OSC
     104 returns an entry to the table; the table moves no colour epoch; read
     sizes 1, 3, 7 and whole; `querying_an_unset_colour_is_silent` stays green.
   - Hand model: after `tell_defaults`, every one of the 256 answers equals the
     theme's entry, under both themes.
   - Recorded: none; no recording or golden fixture carries an OSC 4 query
     (critic-verified).
   - Perturbations: answer overrides only (the old rule); the table over an
     override; the table not pushed in `tell_defaults`; the setter moving the
     epoch.
   - Measure: an `ingest2` sweep, base and change built at one path, an A/A
     copy, rotated, first round dropped, the nine recordings and mix,
     `cells_written` identical, on mains. Expected no result.
2. **Resets per xterm** (baud-core). **Built at `534643a`** (red first,
   three perturbations, an `ingest2` sweep with every range overlapping, per
   its commit); the sfte oracle cases followed in their own commit. What the
   step asked for:
   - Unit: an OSC 4 override is gone after RIS and after DECSTR; OSC 10/11/12
     survive both; the epoch moves once, and not at all with no override live.
   - Hand model: a program sets OSC 4 1, then DECSTR, then SGR 31 draws in the
     theme's red (a renderer test through `Term`).
   - Recorded: none sends OSC 4 then a reset.
   - Oracle: the two new sfte cases of section 10; baud-difftest is not used.
   - Perturbations: clear on RIS only (foot's rule); clear 10/11/12 too
     (kitty's); move the epoch unconditionally.
   - Measure: `ingest2` sweep as step 1. The coverage rows for DECSTR and RIS
     name OSC 4; `ops.rs` comments cite [[source: xterm-misc]] 3051-3085 and
     [[source: xterm-reset]] 14402-14407.
3. **Mode 2031, DSR 996, 997, and the one embedder call** (baud-domain,
   baud-core).
   - Unit: DECRQM ?2031 answers 2 then 1; the 996 reply bytes exact for dark
     and light; 996 silent with no defaults; no bytes on `2031 h`; an embedder
     flip light to dark queues exactly one `997;1n`, and an identical second
     call none; a drop-then-push inside one call that leaves the class
     unchanged queues nothing, and one that changes it queues one; an
     application OSC 11 that changes the class queues nothing but changes the
     next 996 answer; the 996 reply precedes a DA1 reply sent in the same read;
     RIS clears the mode; chunk invariance at read sizes 1 through whole; the
     classifier's WCAG comparison against a std reference on all 256 greys and
     seeded random pairs.
   - Hand model: neovim's handshake (DECRQM ?2031, then `?2031h` only on
     answer 2), an embedder flip, then its OSC 11 re-query answered with the
     new colour.
   - Recorded: capture tmux starting inside baud with
     `scripts/capture-tui.py` (tmux sends `?2031h?996n` to a VT100-like outer
     terminal), commit it with a README, replay: the 996 reply exact and before
     the DA1 reply.
   - Perturbations: report on `2031 h`; report on an escape-originated OSC 11;
     answer 996 from the embedder default ignoring an override; two reports
     from one call (compare per sub-change instead of before/after); DECRQM
     answering 1 at power-on; a third value (`997;0n`).
   - Measure: `ingest2` sweep as step 1.
4. **The app switch with the reset** (baud-app, macOS; Linux's pinned light at
   `dim_light` is its own commit).
   - Both themes built at Boot; the auto branch selects one and makes the one
     core call; the pinned branch's early return unchanged.
   - Unit (the pure helper): a flip under auto with a changed palette drops and
     reports; a flip with an identical palette does neither; pinned does
     neither; no theme file is read after Boot.
   - Source gate in the shape of `every_reader_of_the_window_asks_the_clamp`:
     `apply_style` calls the helper, and nothing else calls the drop.
   - Perturbations: the drop after the push (the report then sees the old
     class); a drop on the pinned branch; a file read at the flip; the Linux
     pinned light at `dim_dark` again.
   - Measure: idle unchanged by construction (no per-tick work added); a flip
     is one full pass of live slots, already the case. Stated, not swept.
5. **Modus Operandi as the built-in light sixteen** (baud-render).
   - A const table in `color.rs`, entries 0-15 of `Theme::light()`, with the
     attribution line; 16-255 and `Theme::dark()` untouched.
   - Unit: the sixteen equal the constants in the test (decision 3's values;
     the `.el` files are `external` and not read in CI); every colour entry 1-6
     and 9-14 at least 6.0:1 as text on 0xEF; ls ex and ln through the bold
     rule at least 6.0; `xterm_palette()` unchanged (a digest); the core's OSC
     4 answer for entry 2 under the light theme is `006800` (step 1's gate
     under the new table).
   - Hand model: an incremental frame after `set_theme` with a different entry
     repaints an SGR 32 cell (a palette swap moves no cell word).
   - Recorded: capture GNU `ls --color=always`, lsd and `bat --theme=ansi` of a
     small file inside baud with `scripts/capture-tui.py`, commit it with a
     README, replay under `Theme::light()` and assert the drawn ex and ln
     pairs at least 6:1 (red today at 1.19 and 1.09); a new pinned light golden
     digest for one recording.
   - Perturbations: xterm's sixteen back in light; the port's ANSI 15
     (`#595959`); the table written into 16-31.
   - Measure: frame by construction (values only); stated.
6. **The floor, and faint** (baud-render, baud-domain's decode).
   - Faint first, inside `Theme::resolve`: a linear-light mix of the drawn text
     toward the colour it is drawn on, after the reverse swap. Unit: on 0xEF
     faint default text is lower contrast than normal (today 16.16 against
     12.99, red); on 0x10 likewise; SGR 2;7 dims the text drawn, not the band.
     Perturbations: mix toward black (the old rule); mix before the swap; mix
     in encoded space. No recording sends SGR 2, so no recorded golden moves.
   - The floor, placed as section 8 says, on in `Theme`. Unit: a reference test
     against the WCAG formula, every lifted pair at 4.5 or above with its hue
     kept; exact boundaries (1.1934 lifted at 1.2, 1.2 not); the same
     non-default word on both sides untouched; explicit-on-explicit untouched;
     SGR 30 on a page equal to ANSI 0 lifted; `UNDERLINE_FOLLOWS_FG` still set
     on floored cells; faint text below the trigger lifted; which btop greys
     are lifted at trigger 1.2 (none) and 3.0 (both). While here, a const
     assert that `size_of::<Resolved>() == Resolved::BYTES`.
   - **Red on purpose**: `the_palette_swaps_black_and_white_and_nothing_else`
     changes its expected colour to the floored one, with a comment that the
     flash case is one the floor exists for.
   - Hand model: the 201 pairs committed as a test table, resolved through
     baud's real `Theme::resolve` under light and dark: none below the trigger
     remains (red today: 44 under xterm light at 1.2).
   - Recorded: `tests/previews/chafa-240.ans` and
     `tests/previews/chafa-truecolour.ans` draw pixel-identical with and
     without the floor (red if the provenance test is removed: 229 and 590
     cells change); step 5's ls capture.
   - Perturbations: exempt nothing; exempt resolved equality instead of word
     equality; drop the provenance test; lift to the trigger, not the target;
     the floor after `with_alpha`; the floor between the underline call and its
     test.
   - Measure: `frame` and `frame_gpu`'s convert column with the floor on and
     off, `FRAME_GLASS=dim`, rotated, with an A/A copy; a `sync_styles`
     full-pass arm on a **filled** rendition table (a chafa or Sixel session,
     then a `?5` flip) with and without the floor; a steady frame costs nothing
     by construction; the `.metallib` unchanged.
7. **Readers, the scheme type, and `[palette]`** (baud-domain, baud-render,
   baud-app). One commit per format.
   - The hex reader; base16/base24 YAML; ghostty files. Fixtures: a light and a
     legacy-flat base16, a base24, a hand-made ghostty file with a `#` inside a
     value; each refused form is one complaint and one test.
   - The type: base08 lands on 1 and 9; 16-21 parsed and not written; a
     declared `variant` that disagrees with the WCAG rule is reported; an
     ansi16-style file's own page kept.
   - The config: the keys read; `~` expanded; `floor = 1.2` read as 1.2, not
     clamped to 1.0; a top-level `light` does not reach the section; theme-file
     complaints on their own list.
   - Perturbations: a `#` inside a ghostty value read as a comment; a legacy
     flat file refused; 16-21 written; an ansi16 page taken from color00;
     `floor` through `unit()`.
   - Measure: `scripts/startup-bench.py`, interleaved arms without and with a
     `[palette]` naming two files, an A/A copy, first launch dropped: the first
     present and the wall, since files are read at launch.
8. **Glass** (baud-app, scripts).
   - 8a: fix `scripts/luma-swatch.sh`'s premise, its own commit.
   - 8b: measure on screen which space Core Animation blends in, and the page
     under the light appearance, recorded in CLAUDE.md; then set `dim_light`
     to the lowest opacity keeping normal text at 4.5:1 (decision 7).
   - 8c: Reduce Transparency draws every background opaque; Increase Contrast
     raises the floor to trigger 3.0, target 4.5; both polled on the tick on
     their own path. Unit: the pure helpers (settings to `Theme` fields), a
     pinned theme reached. Perturbations: Reduce Transparency ignored; Increase
     Contrast applied without `set_theme` (the cache then serves stale slots);
     the poll routed through the pinned early return. Measure: `scripts/idle-cpu.py`
     for the poll, expected inside the floor like Reduce Motion's.
9. **A readable dark sixteen**: research, Modus Vivendi first, then the owner
   (decision 8). Not designed here.

**Separate, now unblocked**: V17 (OSC 4/10/11 act on the colour shown under
`?5`), using [[source: xterm-misc]]; once built, 996 follows it and no
unsolicited report is sent for `?5`.


## 13. Defects found in passing

1. **SGR 2 is inverted on the light theme**: halving the channels makes faint
   default text 0x13, at 16.16:1 where normal text is 12.99:1
   (`baud-render/src/color.rs` lines 38-42, 250-252). No recording sends SGR 2
   (measured: zero in every recording; control: SGR 1 5,210 times in
   asciiquarium.raw). Decision 10.
2. **Linux draws a pinned light theme at `dim_dark`** (`baud-app/src/linux.rs`
   line 133).
3. **`docs/vt-coverage.md` overstated the OSC 4 query** at `c836eaf`;
   `e4db454` rewrote the row.
4. **`scripts/luma-swatch.sh` states a premise that stopped holding** on
   2026-09-19.
5. **Nothing asserts `size_of::<Resolved>() == Resolved::BYTES`**
   (`baud-render/src/cellmap.rs` lines 418-428); a missed update makes `lend`
   return `None` and the app leaves the cell path with a warning only.
6. **Stale or stranded docs**: `Theme::light`'s own doc (`color.rs` 162-176)
   states the opposite policy from decision 3; `color.rs` 178-179 cites
   Apple's Basic as a light theme that redefines the sixteen, but macOS 27.0's
   Basic.terminal has no colour keys, so it draws Terminal.app's compiled-in
   palette, which was not read (the file fact stands; "the citation is wrong"
   does not follow); "eight" `[glass]` keys at `config.rs` 7, 13 and 123 and
   `config.toml.example` 164, where the closed set is ten; `set_theme`'s doc
   still speaks of per-row rendition digests (`renderer.rs` 934-937); the
   `PendingStyle` doc says the StyleObserver writes it, which it does not
   (`app.rs` 1787-1795, 4719); `as_rgb`'s doc sits above `as_index`
   (`baud-domain/src/style.rs` 382-386), and `Driver`'s doc is stranded above
   `PendingStyle` (`app.rs` 1786); `config.rs` 933-934 and the example say
   Apple Symbols supplies braille and box drawing, which baud draws itself;
   `XColor::parse`'s doc contradicts its scaling for `#RGB`
   (`baud-domain/src/vt/osc.rs` 206-207 against 262).
7. **Reduce Transparency is not honoured**, against the glass crate's doc.
   Decision 9.


## 14. Corrections to the record

- **ghostty does not nudge**; the endorsed option text described a mechanism no
  implementation has at 1.1.
- **The endorsed option text was truncated in the draft**; section 1 quotes it
  in full, and its last clause is what sets the trigger at 1.2.
- **kitty and iTerm2 classify dark/light from the background alone** (BT.601
  and 0.30/0.59/0.11 luma below 0.5), not by WCAG against the text; decision 5's
  rule is baud's own.
- **iTerm2's Minimum Contrast is applied per appearance** when separate colours
  are on; V16 said it was not applied.
- **WCAG's formula and 3:1 figure are pinned** ([[source: wcag22]]); V16
  carried them UNPINNED.
- **Among the terminals read, none defaults to a light sixteen**: Ptyxis and
  Terminal.app ship one and do not default to it.
- **The owner's `ls` is lsd, and `lls` is uncoloured BSD ls**; V16 assumed GNU
  ls on PATH. lsd's names still land at 1.19 and 1.09.
- **dircolors.hin and ls.c's built-in table differ** (pi, bd/cd, or).
- **helix does not detect the background in the installed version**; only
  master sends 996. Emacs does not detect under baud (no DA2 reply).
- **bat and delta do not bypass the palette for this owner**: BAT_THEME=ansi
  and delta's ansi theme route through slots 1-6.
- **The override split is a split**: ghostty, iTerm2 (deliberately), VTE and
  alacritty keep; kitty, foot, contour and Windows Terminal drop. Decision 5
  takes the second.
- **ghostty's 997 is the selected theme** (Termio.zig 743-756); its RIS report
  cannot fire under its defaults; ghostty-GTK auto is an OS/palette hybrid.
- **contour sends no 997 on a config reload**, against its own spec's clause.
- **baud has no XTSAVE**; a recommendation to save 2031 in it is moot.
- **The Modus ports' ANSI 15 (`#595959`) is from kitty-themes**, not any
  upstream release.
- **The Modus Emacs mapping's backgrounds under 0x27 are 1.33-2.13**, not
  1.33-2.12 (884900 is 2.134).
- **"Only black on white reaches 4.5 both ways" is too narrow** (section 5).
- **The floor's lift cost is 27-54 ns a failing slot**, not 55-60; a full pass
  covers live slots only until the table fills, and every slot after.
- **wezterm's equality exemption compares alpha**, so it fails for default
  backgrounds in a translucent window.
- **ghostty's exemption set excludes braille**; baud's procedural set
  (`baud-render/src/boxes.rs`) includes braille and excludes legacy computing
  and Powerline.
- **The ghostty themes tarball is 78,218 bytes**, not 34 MB, and its WCAG pass
  (in iTerm2-Color-Schemes) alters 217 themes, not 7.
- **baud-difftest cannot pin an OSC 4 reset difference**: it compares
  characters and whether the background word is set. sfte can.
- **"At all three theme sites"**: `tell_defaults` has four call sites.
- **ModeBit's stale bound is fixed** (`c836eaf`), by counting variants rather
  than deriving a `LAST`.


## 15. Record: vogix, out of scope until vogix 1.0.0-beta.1

**Not designed and not built**, by the owner's 2026-09-29 word (section 1,
item 7). This section records what was read so the work is not redone. The
Vogix Design System is **unpublished**: on 2026-09-27, `git ls-remote` showed
GitHub `main` = tag v0.2.0 = `ca0ad76`, and no published ref contained
`c79140a`. On 2026-09-29 the local vogix16-themes checkout is still at
`c79140a` on branch vogix-design-system, and the owner says he has started
"vogix-themes v1.0.0-beta1", which is not in that checkout. **So every vogix
file below is cited as a local path at the commit named, UNPINNED, never as a
manifest key**, and everything here describes `c79140a`, not the beta.

**The spec at `c79140a`** (vogix16-themes@c79140a docs/design-system.md,
UNPINNED, VERIFIED by reading):

- **A theme is 16 colours, one per ANSI slot, named by role** (41-50): 0
  background, 1 danger, 2 success, 3 warning, 4 link, 5 highlight, 6 active, 7
  foreground, and 8-15 their brights. "A terminal given these 16 colors in ANSI
  order is themed correctly with no further configuration" (52-53). This is the
  owner's "ansi compatible" (<session> l.877).
- **Dark and light are defined once**, in Oklab lightness (85-88; register
  decision D12). baud's classifier is not this rule (decision 5).
- **Section 6, Legibility** (158-191): 4.5:1 on background for foreground,
  foreground-bright and every role and its bright; 3:1 for background-bright;
  none for background. A consumer accepts a palette and warns, or corrects it
  at runtime; background is never changed.
- **Section 7 is Motion (193); importing is section 8 (210-258)**, not section 7
  as a working checklist and a memory file said.
- **Register decision D7** (docs/research/notes/v-decisions.md:61-68): the
  runtime lives in the Vogix app, and "Reverses if: a second consumer needs the
  computation as a library". If baud ever consumes it, the register points at a
  library, not a second copy.

**What SGR does under a palette whose page is ANSI 0 and text ANSI 7**
(computed over the 50 theme files at `c79140a`): SGR 30 on the page and default
text on SGR 47 are 1.00; default text on SGR 41-46 is below 3:1 in all 300
(theme, role, variant) combinations. **The floor (decision 6) lifts the text in
these pairs** whatever the format, which is its endorsed "invisible pairs of any
convention".

**The section 8 reference import** (scripts/vogix.py:493-497 at `c79140a`)
takes color00-15 only and compares ANSI 0 with ANSI 7 for polarity; applied to
the 657 ghostty-collection files, 106 of 145 light themes read as dark
(computed). **The shipping vogix engine keeps an ansi16 file's own background**
(vogix@ad2e814 nix/modules/ansi16-import.nix:101-109, UNPINNED), but its
bash/zsh integration sends OSC 11 from palette line 1, which for ansi16 themes
is color00 (home-manager/default.nix:641-644, 658-662; the ansi16
console.palette.vogix line 1), and OSC-set colours win in ghostty, kitty and
alacritty ([[source: alacritty-display-content]] line 105). So the page a vogix
shell shows is color00. Decision 2 takes each file's own `background`, which
departs from both the spec's section 8 and that hook.

**Where ANSI 4-6 "broke"** (vogix and vogix16-themes history, VERIFIED):

| when | what |
|---|---|
| `d3106b0` (2026-01-19) to `ca0ad76` | the design doc says ANSI 4-6 and 12-14 are "Monochromatic foreground shades" |
| `80dcfbe` (2026-04-06) | the console generator switches to link/highlight/active |
| `87ea81d` (2026-09-23) | three disagreeing tables fixed; `nixos.nix` had mapped by base16 position |
| `6a09f21` (2026-09-25) | "monochrome again", on a scratch branch, never pushed |
| `c79140a` (2026-09-26) | 4 link, 5 highlight, 6 active, with distinct brights |

It broke because the docs never followed the code: vogix@ad2e814
nix/modules/applications/README.md:60-66 still says "blue, magenta, cyan →
foreground colors". **The 2026-09-25 22:58Z MUST** ("in vogix first 8 are
monochromatic and next 8 are functional", "that is a MUST") reads naturally as
the old file layout, base00-07 greys and base08-0F functional, which `c79140a`
keeps as roles 1-6 functional; it is recorded, not re-asked.

**vogix-app items found, not acted on** (his repository, out of scope): the
README above; templates repeating the normals in ANSI 9-14
(console.palette.vogix:10-15) where the design has distinct brights; three
polarity rules where the spec has one; no ls or eza template, though
design-system.md:271-272 says "eza / ls — permission errors `danger`, broken
links `warning`; file types stay in the foreground scale". The vogix app
themes ghostty, kitty and foot only through that shell OSC integration, which
decision 5's reset erases on a flip until a new shell.

**The bold rule meets a role-ordered palette well**: a light palette whose
brights are darker than its normals puts GNU ls bold green, blue and cyan at
about 9-13:1 on its own page (computed on the `c79140a` day themes). That is a
property of the palette, available to any format that carries such brights.

**If vogix returns to scope after 1.0.0-beta.1**: a reader for its theme files
onto the type in section 4 (page = slot 0, text = slot 7), and whether baud
consumes the legibility computation as a library (D7), are the two questions;
the rest of this note does not change.


## 16. What is not built, measured or verified

- **Nothing designed here is built** except step 0 (`c836eaf`), step 1
  (`e4db454`) and step 2 (`534643a`, its sfte cases after it), built by
  another session while this note was written.
- **Unmeasured**: the floor's cost in baud (a full pass on a filled table);
  startup with theme files read at launch; which space Core Animation blends
  in; the light page under glass; a 16-colour picture under the per-slot rule;
  the WCAG classifier over the 657 iTerm2-Color-Schemes files (the 602 bundled
  ghostty themes were run: 0 disagreements with Rec.709); whether the nine render
  digests survive the floor (inferred, not rendered).
- **UNVERIFIED**: the legal status of shipping Modus values; Terminal.app's
  switching and its compiled-in Basic palette; iTerm2's classifier line (read
  by the sweep at 8622e64, not pinned here); kitty's ratio mode outputs; how Qt
  reads the portal; Claude Code's 2031 behaviour (closed source; the only
  witness, runner PR 564, also claims wezterm sends 2031, which is false at
  wezterm@b09b56c).
- **Unpublished**: the Vogix Design System and vogix-themes 1.0.0-beta.1.
