# AA. The shell's locale, and Hebrew

**Status: the locale half is built** (2026-10-03; `docs/record.md`, "A shell
started from the Dock is given a locale"); the face, the Backspace flag's
route, Linux and the per-language step are open (section 7).

The owner's bug of 2026-10-03, with the screenshot: Hebrew typed at the zsh
prompt in baud showed, letter by letter, `�<0099>�<0097>�<0099>`. The owner:
"sorry but zsh works fine with hebtrew in mac terminal". The cause is the
shell's locale and the fix is baud's; with it built the letters still cannot
draw, because no face baud has maps Hebrew, and they show as the first face's
missing-glyph box. RTL -- reordering, mirroring, an RTL cursor -- is not touched
here: the owner asked for it ("the best one - windows level", research "beyond
vts") and said not to start that research until they say so.

Evidence is tagged as in the rest of these notes: measured (run, with the
command), source (read at file:line) or inferred. The probes are in
`scripts/locale/`; the full reports of the workflow that gathered this are kept
untracked in `.claude/research/hebrew-locale/`.

## 1. What happened

Each Hebrew letter is two bytes in UTF-8: yod U+05D9 is `D7 99`, het U+05D7 is
`D7 97`. In a locale that is not UTF-8, zsh's line editor reads each byte as a
character of its own, and the C locale maps a byte to the code point of the
same value. zsh prints a character raw when it is printable and as `<%.04x>`
otherwise (zsh `Src/Zle/zle_refresh.c:1271-1377`, source); with its Unicode 9
tables (`Src/ztype.h:76-80`, source; the symbols are in the owner's zsh,
measured with `nm`), U+00D7 is printable and goes out as the raw byte `D7`,
and U+0099, a C1 control, comes out as the text `<0099>`. baud reads a lone
`D7` followed by a byte that cannot continue it as one malformed sequence and
draws U+FFFD (`baud-core/src/graph.rs:706-711`, `write.rs:449-452`, source).

`scripts/locale/zle_repro.py` runs the owner's zsh (`-f -i`, so no rc file is
read) on a pseudo-terminal opened as baud opens one, with the environment
launchd gives an app plus baud's four variables, and types the bytes (measured,
on Apple's zsh 5.9 and the nix zsh 5.9.2 alike):

| typed | no locale variable | `LANG=en_US.UTF-8` |
|---|---|---|
| `D7 99` | `d7 08 d7 1b5b376d <0099> 1b5b32376d` | `d7 99` |
| what zsh reports | `CODESET=US-ASCII`, `LANG` unset | `CODESET=UTF-8` |

`LC_CTYPE=UTF-8` and `LANG=he_IL.UTF-8` also come back intact; `LANG=en_IL.UTF-8`,
a name macOS does not have, breaks exactly as no variable does -- an unchecked
locale name is as bad as none. `export LANG=en_US.UTF-8` inside the broken
shell fixes the very next letter (measured).

## 2. Why the shell has no locale

baud gives its child its own environment, less 48 outer-terminal identity names
(`OUTER`, `baud-app/src/pty.rs:116-180`, whose only `LC_` names are iTerm2's
`LC_TERMINAL*`), plus `TERM`, `COLORTERM`, `TERM_PROGRAM` and
`TERM_PROGRAM_VERSION` (`OURS`, `:187-193`), built in `child_env` (`:220-243`)
and passed to `execve`; the macOS and Linux drivers share it (source). Before
2026-10-03 nothing in baud set `LANG` or `LC_*`; since then `child_env` sets
`LANG` when the parent names no locale (section 7, item 1). The bundle's
`Info.plist` has no `LSEnvironment` (measured, each search with a positive
control).

An app the Dock or Finder starts inherits launchd's environment, which has no
locale: `launchctl getenv LANG` is empty, and the live environments of two
apps launchd started (KeepingYouAwake, Discord, read with `ps -wwE`) carry
`PATH`, `HOME` and `SHELL` and no `LANG` or `LC_*` (measured). None of the
owner's zsh startup files sets one (measured, with controls). So a baud started
from the Dock handed zsh no locale, and zsh ran in C, until the fill of
2026-10-03.

A baud started from a terminal is different: `open -g target/baud.app` run from
a shell gave the app that shell's whole environment, `LANG=en_US.UTF-8` and the
devenv `PATH` among it (measured 2026-10-03), and baud passes `LANG` on. That is
why the check of 2026-09-22 (`docs/record.md`, run from a Terminal.app shell)
could not catch this, and why the owner's Terminal.app shell works: a Terminal
session's environment has `LANG=en_US.UTF-8` while Terminal's own launchd
environment has none and no startup file sets it (measured), so Terminal, or
the `login` it runs, sets it (inferred).

The owner's three reports fit this. On 2026-09-29: "when i switch to hebrew i
don't see the hebrew letters at all"; on 2026-10-02: "i know i told you that
hebrew render as box with ?"; on 2026-10-03 the screenshot. The bundle has
existed since 2026-09-14, so Dock launches could explain all three; FiraCode
Nerd Font Mono draws U+FFFD as a black diamond with "?", and its missing-glyph
box also carries a "?" (measured, rendered).

## 3. What other terminals do

Read at source (Ghostty befcdfd, kitty 7d37453, WezTerm cab2516, Alacritty
d692748, iTerm2 91411f5, foot cb27717); Terminal.app 2.15 from its binary's
disassembly (sha256 596e8940..., macOS 27.0; the second round, section 8):

| terminal | when | sets | from | checked, then |
|---|---|---|---|---|
| Ghostty | `LANG` unset or empty | `LANG` | the user's locale, `ll_CC.UTF-8` (`src/os/locale.zig:21-27, 93-120`) | `setlocale`; an invalid `LANG` becomes `en_US.UTF-8`, on every OS (`:30-76`) |
| kitty | `LANG` not in the environment | `LANG` | the identifier, then `lang_country`, then, for a language other than English, the first `/usr/share/locale/<lang>_*` sorted by name, then `en_US` (`kitty/main.py:391-410`) | the bare name with `newlocale`; the step's pick and `.UTF-8` appended unchecked |
| WezTerm | `LANG` unset or empty | `LANG` | `ll_CC.UTF-8`, or `ll.UTF-8` | `setlocale(LC_CTYPE)`; `en_US.UTF-8` unchecked |
| Alacritty | the process locale is C | `LC_ALL`, else `LC_CTYPE=UTF-8` | the user's locale | `setlocale`; issue #7360 asks it to stop setting `LC_ALL`, and PR #7421, which deleted the locale code, was closed: "You just broke macOS entirely" |
| iTerm2 | always, in its default mode | `LANG`, else `LC_CTYPE` | preferred languages and region (`PTYSession.m:3328-3374`) | `setlocale` per candidate; a dialog, then `LC_CTYPE=UTF-8` |
| Terminal.app | a profile setting, `SetLanguageEnvironmentVariables` | `LANG`, else `LC_CTYPE=UTF-8` | `identifier.UTF-8`, `lang_country.UTF-8`, the bare forms, then every `/usr/share/locale/<lang>_*` as `.UTF-8` in directory order, English included | `setlocale` per candidate; `LC_CTYPE=UTF-8` only if `setlocale` accepts it |
| foot (Linux) | never for the child | -- | -- | its own `C.UTF-8`, then `en_US.UTF-8` |

Ghostty, kitty and WezTerm do not validate their final fallback; Terminal.app
and iTerm2 do. None of the six fills an unset `LANG` on Linux. ssh forwards locale variables by default here
(`/etc/ssh/ssh_config.d/100-macos.conf`: `SendEnv LANG LC_*`; measured), so
whatever baud sets travels to remote hosts as Terminal's does.

## 4. A locale name can be checked before it is handed over

`scripts/locale/validate_probe.c` (measured, macOS 27): `newlocale` with
`LC_ALL_MASK` (63) followed by `nl_langinfo_l(CODESET)` (`CODESET` = 0) reading
`UTF-8` accepts `en_US.UTF-8`, `he_IL.UTF-8`, `en_GB.UTF-8` and `C.UTF-8`, and
refuses bare `en_US` (an empty codeset), `C` and `POSIX` (`US-ASCII`),
`en_IL.UTF-8`, `en_US.utf8`, `bogus_XX.UTF-8` -- and `UTF-8`, which passes under
`LC_CTYPE_MASK` (2) alone: the whole mask matters, because `LANG` feeds every
category and a shell's `setlocale(LC_ALL, "")` fails as a whole. An empty name
reads the process's own environment and must be refused before the call.
`locale_t` is 8 bytes and `nl_item` 4 here; the Linux values must come from
the headers.

`scripts/locale/cflocale_probe.c`: CoreFoundation's current locale reads the
user's preferences, not the environment (`LANG=fr_FR.UTF-8` does not move it;
a command-line override to `he_IL` does); on this Mac it reports `en_US`,
language `en`, region `US`, so a derived name is `en_US.UTF-8`. A region
override such as `en_US@rg=ilzzzz` reports region `IL`, and `en_IL.UTF-8`
does not exist -- a derivation must fall back. `scripts/locale/cold_probe.c`:
in a fresh process the first CoreFoundation read takes 1.2-2.4 ms (a further run
up to 4.8 ms), a validation 0.18-0.31 ms, `C.UTF-8` 0.05-0.07 ms; warm, each is
under 0.3 us (three rounds). Inside baud, measured once the fill was built,
over twelve launches with launchd's environment: 0.376-0.890 ms by
`shell_locale`'s own clock, median 0.719. The fresh-process figure does not
describe the read there (measured). The shell is spawned before AppKit starts
(`baud-app/src/app.rs:4561-4564`), but after CoreGraphics's `display_scale`, an
AppKit class call and `Boot::prepare` (`:4570`, `:4575`, `:4588`; source).
Whether any of these is why the read is cheaper is unmeasured. The launch's
`pty` mark does not resolve the read (the record has the ranges).

## 5. Found on the way: Backspace and a whole UTF-8 character

baud's `forkpty` passes no termios (`pty.rs:568-573`), so the pseudo-terminal
starts with the kernel default `-iutf8`. In line-input mode -- `cat`, `read` --
a Hebrew letter followed by Backspace leaves its lead byte `d7` in the line;
with `iutf8` the line is empty; the control `ab` Backspace leaves `61` every
time (`scripts/locale/iutf8_probe.py`, measured, two runs identical). zsh's own
editor is unaffected (inferred). Ghostty (`src/pty.zig:172-179`), kitty and
Alacritty set the flag on the master before any child exists; foot sets it in
the child before `exec` (`slave.c:248-261`). baud's `forkpty` creates and forks
in one call, so the three routes are: pass a termios into `forkpty` (built by
hand, since a Dock launch has no tty to copy), `openpty` then `fork` and
`login_tty`, or `tcsetattr` on the child's terminal before `execve` (which
`tcsetattr`'s async-signal safety permits, against `pty.rs:216-219`'s rule that
the child does nothing but the exec).

## 6. Hebrew once the locale is right

No face baud draws with maps a single Hebrew code point: 0 of the 27 letters
U+05D0-05EA and 0 of 24 points, in every face of the owner's stack (FiraCode
Nerd Font Mono, Hiragino Sans GB, Apple SD Gothic Neo, Apple Color Emoji, Apple
Symbols), the macOS default (Menlo among them), the Linux default and all six
embedded faces (`scripts/locale/verify_fonts.py` with fontTools, and a CoreText
reader; measured, with each face's own code point as the control). baud draws
such a character with the first face's missing-glyph box
(`baud-render/src/stack.rs:473-482`, source). Width is already right: U+05D9 is
one column (`baud-core/tests/encoding/width.rs:32-41`; libc agrees).

Candidates measured: on this Mac, Courier New is the only monospaced face with
Hebrew, and it is what CoreText itself falls back to. Liberation Mono 2.1.5
(SIL OFL 1.1 with Reserved Font Names "Arimo", "Tinos", "Cousine" and
"Liberation") has 27 of 27 letters and all 24 code points U+05B0-05C7, its
letters at 1229/2048 em like its Latin. The file measured here was nixpkgs'
rebuild (314,744 bytes), which gives every glyph 1229 units; the upstream
release (319,508 bytes) gives its 51 Hebrew marks zero advance, and is the file
a pin would take (section 8). baud draws the two identically, since it zeroes
a mark's advance. Noto Sans Hebrew 2.003 is proportional, its letters
0.235-0.703 em, and 3.001, the version note W lists, 0.295-0.730 em; the widest
is wider than a 0.6 em cell. The Nerd Font build of Cascadia Code has 27 letters and 10 points
at a fixed 1200/2048; upstream Cascadia Code is OFL 1.1. Liberation Mono placed
last fills 556 boxes in the owner's stack, 413 in the macOS default and 455 in
the Linux default -- 133 of each Hebrew -- and takes nothing from another
face; placed before STIX it would take 332, 38 and 57 code points from it
(counted by the first face that maps each, not baud's colour-aware choice).

Vowel points would not sit on their letters yet: baud's shaper reads the Latin
script or the default only (`baud-font/src/layout.rs:713-717`), and under the
Latin script Courier New and Liberation Mono place dagesh and qamats one cell
to the right where the Hebrew script puts them on the letter (HarfBuzz as a
stand-in; inferred for baud). That is per-script shaping, filed with the RTL
work and not started. Without bidi, Hebrew shows in typed order, left to right.

Corrections to note W this brings: W:83 ("nobody bundles ... Hebrew") is wrong
for rio, whose Cascadia Nerd Font carries Hebrew inside its Latin face; W lists
no monospaced Hebrew candidate (Liberation Mono, Courier New); W:270 does not
say Noto Sans Hebrew is proportional; W's Hebrew test string has no points.

## 7. Open decisions

1. **How baud gives the shell a locale: built.** `LANG` is set only when
   `LC_ALL`, `LC_CTYPE` and `LANG` are all unset or empty. An empty `LANG` is
   replaced, never doubled, and `LC_TERMINAL*` is not a locale. The value is
   the first candidate that validates as in section 4: the user's identifier
   without its `@` keywords, with `.UTF-8`, then language and region, then
   `en_US.UTF-8`, then `C.UTF-8`. Nothing is set if none validates. Jev chose this at 1.00,
   confidence 0.99. Two sub-questions scored under the bar and are built the
   cautious way, for the owner to decide:
   - **kitty's per-language step**, left out (with 0.53, without 0.47,
     confidence 0.06). With it, a Hebrew speaker whose region macOS lacks
     would get `he_IL.UTF-8` rather than `en_US.UTF-8`.
   - **Linux**, which passes its environment on as before (inherit 0.56, fill
     0.44, confidence 0.12). No terminal surveyed fills there, and validating
     in a nix-built parent proves nothing about the host libc the shell uses.
2. **The Backspace flag**: Jev would set `IUTF8` (0.98, confidence 0.96). The
   route is open: a termios into `forkpty` 0.59, `openpty` and `login_tty`
   0.21, `tcsetattr` in the child 0.20, confidence 0.39. Not built.
3. **Which face draws Hebrew**: folded into note W's open fallback-font
   question, with Liberation Mono as the candidate that fits the cell and is
   the same on every system, to be re-measured against W's amended bundle and
   rendered before it is asked.

Each open one goes to the owner's review page, with its picture.

## 8. The second round (2026-10-04)

The face and the per-language step were researched again, because Jev scored
both under the bar; the Backspace route and Linux reran after a usage limit
stopped them. The evidence is untracked, in `.claude/research/open-calls/`.
Asked again with these facts, Jev chose Liberation Mono at 0.75 against waiting
for note W at 0.24, confidence 0.68 (first round 0.60, confidence 0.50), and
flipped on the step: without it 0.60, with it 0.40, confidence 0.21. Neither
is settled, so both go to the owner.

**The face.**

- **The file a pin would take is the upstream release**, 319,508 bytes, sha256
  `f2b83c763e8afd21709333370bed4774337fae82267937e2b5aea7e2fbd922c1`, not
  nixpkgs' rebuild. The two differ only in `hmtx` (measured). OFL-FAQ 5.9 makes
  a rebuild a Modified Version, which may not carry the reserved names (doc).
- **Placed last in the owner's stack, it only fills boxes.** By baud's own
  selector (`Stack::select_presented` over every scalar value, under text,
  colour and the system default), it fills 556 code points and moves none
  from another face (measured). 133 are Hebrew: 27 letters, 20 points, 31
  cantillation marks, 46 presentation forms and 9 others. The other 423 are
  phonetic and Latin Extended letters, combining marks, archaic Cyrillic,
  Coptic and U+20BF.
- **A mark draws in its base's face.** `a` + U+0346 draws FiraCode's `a` and
  FiraCode's missing glyph for the mark, though Liberation and STIX both map
  it (measured). So a face missing a point draws a box for it. Cascadia Mono
  2407.24, the default face Windows Terminal bundles (microsoft/terminal
  2b5336c), has 8 of the 20 points and no cantillation. Sheva, hiriq, tsere and
  segol would draw as boxes (inferred from its `cmap` and the measured rule).
- **Liberation's Hebrew anchors are reachable under `hebr` only** (measured, its
  `GPOS`). baud shapes with `latn` or the default, which is why its points sit
  one cell right. Per-script shaping would place them with the font's own
  anchors. Noto Sans Hebrew lists the same kind of lookups under `latn` and
  `DFLT` as well, which is why its points already sit on their letters
  (measured).
- **Cost, in one harness**: `Face::parse` 0.21 us against STIX's 0.50 us, and
  4.2-4.3 us more on the first frame's block-0 fill, ranges disjoint
  (measured). Each binary grows by 319,508 bytes.
- **No terminal embeds Liberation Mono** (searched in nine trees). fontconfig
  makes it Linux's stand-in for Courier New, the face CoreText falls back to
  for Hebrew (source).
- **Windows Terminal shapes by script and does not reorder right-to-left
  text.** It never sets a bidi level, and conhost presents RTL text in logical
  order for `vim -H` (source). This is one fact; the RTL research is not
  opened.
- **Order decides against note W.** Behind W's minimal bundle, whose Noto Sans
  Hebrew comes first, Liberation draws no Hebrew. Behind Noto Sans Mono it
  draws 14 code points (measured). Embedding it now amounts to deciding W's
  Hebrew slot now.
- **Noto Sans Hebrew 3.001** covers 134 Hebrew code points, Liberation's 133 and
  U+05EF. Placed last it fills those 134 and moves nothing (measured).

**The per-language step.**

- **Terminal.app 2.15 does the step**, read from its binary. It tries every
  `/usr/share/locale/<lang>_*` in directory order, English included, so
  English in Israel gets `en_NZ.UTF-8` (measured, a reimplementation). kitty
  does it sorted by name with English excluded; Ghostty and WezTerm give
  `en_US.UTF-8` (source).
- **kitty's maintainer chose it over baud's current answer.** On 2023-07-01
  they merged a fallback to `en_US` (PR #6409, 503a0bdd), then put the step in
  front of it 44 minutes later (22c2687432) (source).
- **The step changes no character handling.** All 83 UTF-8 locales on macOS 27
  link `LC_CTYPE` to `C.UTF-8`'s (measured). It changes dates, money, numbers,
  collation and messages.
- **Under `he_IL.UTF-8`, `date` and `ls -l` print Hebrew month names**
  (measured). baud cannot draw them until a face lands (inferred).
- **What it changes:** kitty's rule changes baud's answer for 211 of the 1060
  identifiers CoreFoundation offers (measured). Its picks are alphabetical
  (`es` to `es_AR`, `fr` to `fr_BE`) and ignore the script (`zh-Hant` to
  `zh_CN`).
- **Cost:** one directory listing, about 0.1 ms cold and 75-91 us warm, only on
  the step's path (measured). It costs the owner nothing, since `en_US`
  validates first.
