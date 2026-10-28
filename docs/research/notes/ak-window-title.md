# AK. The window title: what a program sets, what baud derives, and where it shows

**Status: research, 2026-10-09; revised three times the same day after
read-only reviews; weighed with renders on 2026-10-21 (below).** The request,
2026-10-08: the standard title options, such as the current directory and
the current process. In-repo lines are at 473ff12;
the cited files are unchanged at 2d8afea.

**Decided and built, 2026-10-21**, weighed with renders, with three forks
this note had not posed researched further: question 2 T2; question 4 R1;
question 8 I1; folder, then program; the program by argv[0]; the folder
with home as `~` and past three folders the last three, as kitty's and
ghostty's integrations spell it. Question 1 is open and the proxy icon
waits on it. Built: `OSC 7` in the core, the derived title in
both apps, checked as 6.5 settles. Not re-asked this round, and nothing built for them:
question 5 (Q-A, keep refusing `CSI 21 t`, today's behaviour), question
7a (unsettled; today's B kept) and 7b (L, today's). Question 6 stays
decided, H0.

Where the evidence lives: every source is a key in
`docs/research/sources/MANIFEST.toml` (groups C, E, J and AK); the probes and
their saved output are the `ak-obs-*` keys under
`docs/research/sources/ak-observed/`; the scripts that reproduce the cited
renderings are under `scripts/`. Fetched copies of the `external` sources
(other terminals' code, kernel and SDK files, GPL documents) are not committed
and stay untracked under `.claude/research/title/`.

Evidence tags: **M** measured by the probes [[source: ak-obs-probe-c]],
[[source: ak-obs-proxy-m]] and [[source: ak-obs-lossy-rs]] on this Mac (Apple
M5 Max, macOS 27.0 build 26A5425a, [[source: ak-obs-observations]], whose
`pmset` reading is from 2026-10-28 and not from the runs); **C** read from
code, not run. Every source is cited by its quoted text, with line numbers of the file
as fetched. "rendered N" is line N of a rendering, given as a convenience:
for an HTML page, `python3 scripts/render-html-source.py MODE PAGE.html`
(the mode is named in the source's manifest entry); for an Apple DocC page,
`python3 scripts/render-docc-json.py PAGE.json`; for a man page,
`mandoc -T utf8 PAGE | col -b`. ECMA-35 is searched with
`python3 scripts/extract-ecma35.py docs/research/sources/ecma-35-6th-1994.pdf
--find "NEEDLE"`, ECMA-48 with `python3 scripts/extract-ecma48.py --find
"NEEDLE"`; each prints its positive control first. Anything neither quoted nor
measured is marked UNVERIFIED.

**How this note reads "Pt".** ctlseqs' "Change Window Title to Pt" is read as:
the window title is Pt's text. Decoding Pt's bytes and sanitising its
characters for display (today's control-to-space, trimming, and an empty
title's fallback) are counted as showing Pt; adding other text beside it is
counted as not. By a stricter reading even today's sanitising departs; that
reading is not used here.

## 0. In plain words

A window title has two possible authors. **The program** sets one with
`OSC 0` or `OSC 2`, or on a VT520 with `OSC 21` (DECSWT); these are the
titles the written specifications define. **The terminal** can also look up
facts about the child itself: which process is in the foreground, and what
directory it is in. kitty, wezterm, iTerm2 and Terminal.app show some of
that. alacritty uses the foreground directory only to start things (a new
window or instance, a hint command, the bell command), and xterm reads the
shell's directory only to start a new terminal. ghostty and foot derive
nothing. None of this is in a standard; it is practice, and each terminal
combines the sources its own way.

A third author, between the two, is **the shell** reporting its directory
with `OSC 7`. Apple's Terminal is told the directory this way, and every
terminal studied here except xterm and alacritty accepts it. No standard
defines it: the freedesktop terminal-wg issue asking for one has been open
since 2019.

**The first gate is whether macOS shows a title at all.** The window crate
hides the title text. In a probe window built with the crate's style, AppKit
created no document-icon (proxy) button while the title was hidden (**M**,
section 3.1), although the SDK header says the icon shows for any non-nil
URL. So "current dir, current process" in the macOS titlebar needs the crate
to show its title, or baud to draw its own. That question was already open,
and is folded in here as question 1.

What baud can derive on macOS and what each read cost here (**M**, warm
20,000-call loops over one pid, runs 1-3, 2.6): the foreground process group,
2.5-2.6 us; its short name, 0.16-0.17 us; its directory, 0.44-0.47 us;
argv[0], 3.8-4.1 us for `/bin/sleep`, whose argument area held 33 bytes. One
launch against a different binary (run 4: a nix multi-call binary, a longer
path, a 32,836-byte argument area) read 2.69-3.00, 0.20-0.26, 0.48-0.60 and
5.5-5.9 us; which of those differences accounts for its costs is not
separated. One check made after the process has been idle is unmeasured. Each
read is one baud would have to schedule itself: a foreground job changes
without the terminal hearing about it, since `sleep 60` prints nothing and
sends no key.

## 1. What the texts say

### 1.1 ECMA-48 has no title, and § 9 is scoped to ECMA-35 codes

ECMA-48 defines `OSC` and leaves its meaning to the host. [[source: ecma48]]
§ 8.3.89, extracted with `scripts/extract-ecma48.py --find`:
"OSC is used as the opening delimiter of a control string for operating
system use. The command string following may consist of a sequence of bit
combinations in the range 00/08 to 00/13 and 02/00 to 07/14. The control
string is closed by the terminating delimiter STRING TERMINATOR (ST). The
interpretation of the command string depends on the relevant operating
system."

The same tool found no `title` and no `window` anywhere in the text (0 hits
each), and its positive controls hit (`framed`, 2; `OPERATING SYSTEM COMMAND`,
5). So the absence is real. Reading "the relevant operating system" as xterm's
practice, for a terminal emulator, is this note's interpretation.

**§ 9 widens the repertoire in an 8-bit code and fixes what the high half
means.** § 9, "Transformation between 7-bit and 8-bit coded representations"
(`--find "columns 10 to 15"`, one hit): "In an 8-bit code, the bit
combinations of columns 10 to 15 are permitted to represent ... the contents
of the command string or the character string as part of a control string
(see 5.6) ... In these situations, the bit combinations in the range 10/00 to
15/14 shall have the same meanings as the corresponding bit combinations in
the range 02/00 to 07/14." X3.64's Appendix H, as [[source: williams]] quotes
it (465-481), says the same except for 10/0 and 15/15: "In an 8-bit code, the bit combinations of
columns 10 to 15 (except 10/0 and 15/15) are permitted to represent: ... the
contents of a control string ... In these situations, the bit combinations in
the range 10/1 to 15/14 have the same meanings as the corresponding bit
combinations in the range 2/1 to 7/14." Williams applies it everywhere: "In
all states, these codes are treated identically to GL codes 20-7F" (462-463).
xterm's code names the rule ([[source: xterm-reset]], charproc.c 3433-3436:
"Otherwise, ECMA-48 section 9 recommends parsing controls with a 7-bit table,
precluding the use of 8-bit data") and follows it whenever its UTF-8 title
mode is off, storing `AsciiOf(c)` (charproc.c 3454-3456), which is
`#define AsciiOf(n) (0x7f & (n))` ([[source: xterm-ptyx-h]] 330).

**But ECMA-48 applies only to codes built the ECMA-35 way.** § 1 Scope
(`--find`, one hit): "This ECMA Standard defines control functions and their
coded representations for use in a 7-bit code, an extended 7-bit code, an
8-bit code or an extended 8-bit code, if such a code is structured in
accordance with Standard ECMA-35." [[source: ecma35]] puts ISO/IEC 10646-1's
coded character set, of which UTF-8 is an encoding form, outside that
structure in its own Scope, § 1, NOTE 1: "The coded character set specified
in ISO/IEC 10646-1 has a different structure not in accordance with this
Standard." That holds for baud's default UTF-8, which no escape sequence
selected. Where an escape sequence does the switching, § 15.1 lists "a
designation to switch to, and return from, other coding systems that are not
in accordance with this Standard (15.4)", and § 15.4.2 gives the way back:
"DOCS with Final Byte 04/00, without I byte, shall designate the coding
system of this Standard" (each needle one hit; control "graphic character",
141). ctlseqs names the two selections baud acts on: "ESC % @ Select default
character set. That is ISO 8859-1 (ISO 2022)." and "ESC % G Select UTF-8
character set, ISO 2022." ([[source: ctlseqs]] 312, 314). Note G already
reads UTF-8 this way: "UTF-8 is not a mode inside ISO 2022; it is what you get
by leaving ISO 2022 through ECMA-35 clause 15.4's DOCS door", with ISO/IEC
10646 giving `ESC % G` for UTF-8 (`docs/research/notes/g-text-encoding.md`
200-204, citing [[source: iso10646-fcd]] §12.2).

So the texts that govern a title's high bytes depend on the encoding in force
(this note's reading of the texts):

- **Under `ESC % @`** (ISO 8859-1, an 8-bit code in ECMA-35's structure),
  two texts conflict. ECMA-48 § 9: a byte from A0 to FE means its GL
  counterpart. ctlseqs: titles are ISO-8859-1, "So that is the default
  behavior" (2884-2885), byte for codepoint. baud follows ctlseqs and departs
  from § 9 (5.2).
- **Under UTF-8**, § 9 does not reach the title bytes (they are not codes in
  an ECMA-35 8-bit code, which is also why 5.1 can exempt UTF-8 from
  Williams' C1 rule). Two other texts do speak. The Unicode Standard
  ([[source: unicode18-ch3]]) C8: "When a process interprets a code unit
  sequence which purports to be in a Unicode character encoding form, it
  shall interpret that code unit sequence according to the corresponding code
  point sequence." C10: "... it shall treat ill-formed code unit sequences as
  an error condition and shall not interpret such sequences as characters."
  Note G applies C10 to baud's decoder already
  (`docs/research/notes/g-text-encoding.md` 82-84). And ctlseqs' Title Modes
  speak to UTF-8 titles directly, with ISO-8859-1 as the default and UTF-8 as
  an opt-in: "The utf8Title X resource setting tells xterm to disable a
  reconversion of the title string back to ISO-8859-1, allowing the title
  strings to be interpreted as UTF-8. The same feature can be enabled using
  the title mode control sequence described in this summary." (2891-2894),
  that sequence being XTSMTITLE "Ps = 2 -> Set window/icon labels using
  UTF-8." (1713). xterm.man's default makes UTF-8 the effective default in a
  UTF-8 locale instead (1.3). baud follows xterm.man.

Note G's reading covers code extension, not control-string bytes. Note AE
quotes § 9 in full and reads it as "a tolerance for transformation
artifacts", marking a single-shift option conforming to it
(`docs/research/notes/ae-code-extension.md` 184-190, 803); it decides nothing
about title bytes. No earlier decision on title bytes against these texts was
found (searched `docs/vt-coverage.md` and notes G, AE and AF for "8.3.89",
"command string", "07/14", "8-bit data"). The behaviour was described
without the texts: "the bytes are read in the encoding the terminal is in --
ill-formed UTF-8 as U+FFFD, as the screen shows it, and ISO 8859-1 byte for
codepoint". The choice is open,
question 7.

### 1.2 xterm's control sequences

From [[source: ctlseqs]] (patch #411):

- **Setting.** `OSC Ps ; Pt ST`: "Set Text Parameters, xterm." (2012-2013). "A
  few of these control sequences began with dtterm (codes 0 , 1 , and 2 )"
  (2018-2019); "Code 3 in dtterm sets the working directory for the next
  session. XTerm does that with the spawn-new-terminal action." (2019-2021).
  "Ps = 0 -> Change Icon Name and Window Title to Pt. / Ps = 1 -> Change Icon
  Name to Pt. / Ps = 2 -> Change Window Title to Pt." (2034-2036).
- **OSC 21.** "Ps = 2 1 -> Change Window Title to Pt (DECSWT), VT520." (2126).
  See 1.4.
- **Reporting.** XTWINOPS "Ps = 2 0 -> Report xterm window's icon label.
  Result is OSC L label ST / Ps = 2 1 -> Report xterm window's title.
  Result is OSC l label ST" (1687-1690). The whole family "may be disabled
  using the allowWindowOps resource" (1627-1628).
- **The stack.** "Ps = 2 2 ; 0 -> Save xterm icon and window title on stack.
  ... Ps = 2 3 ; 2 -> Restore xterm window title from stack." (1691-1697);
  "XTWINOPS 2 2 (save/push title) and 2 3 (restore/pop title) accept an
  optional third parameter for direct access to the stack. Parameters in the
  range 1 through 10, may be used to store the title into the stack or
  retrieve the title from the stack without pushing/popping." (1701-1705).
  No depth and no overflow rule is given. "CSI # S Report position on
  title-stack (XTTITLEPOS)", answered as `CSI Pn; Pm # S` with the current
  and maximum index (664-669).
- **Title modes.** XTSMTITLE `CSI > Pm t`: "Ps = 0 -> Set window/icon labels
  using hexadecimal. Ps = 1 -> Query window/icon labels using hexadecimal.
  Ps = 2 -> Set window/icon labels using UTF-8. Ps = 3 -> Query window/icon
  labels using UTF-8." (1707-1718). XTRMTITLE `CSI > Pm T` resets them
  (733-748). DECRQSS lists `> Pm t -> XTSMTITLE` among its queries (498).
- **Encoding.** "As a VT220-emulator, xterm "should" limit the character
  encoding for the corresponding strings to ISO-8859-1. Indeed, it used to be
  the case (and was documented) that window titles had to be ISO-8859-1. This
  is no longer the case. However, there are many applications which still
  assume that titles are set using ISO-8859-1. So that is the default
  behavior. If xterm is running with UTF-8 encoding, it is possible to use
  window- and icon-labels encoded using UTF-8." (2880-2888). "The utf8Title
  X resource setting tells xterm to disable a reconversion of the title
  string back to ISO-8859-1, allowing the title strings to be interpreted as
  UTF-8. The same feature can be enabled using the title mode control
  sequence described in this summary." (2891-2894). On hexadecimal:
  "the characters sent to, or returned by a title control are less
  constrained than the rest of the control sequences. To make them more
  manageable (and constrained), for use in shell scripts, xterm has an
  optional feature which decodes the string from hexadecimal" (2900-2905).

### 1.3 What xterm does with them, at source

xterm patch #411, `xterm-snapshots` 9489b20: charproc.c is
[[source: xterm-reset]], misc.c [[source: xterm-misc]], ptyx.h
[[source: xterm-ptyx-h]], main.h [[source: xterm-main-h]], xterm.man
[[source: xterm-man]].

- **An empty title is a default, not a blank.** misc.c 4141-4148: with no
  data, "case 0: case 1: case 2: buf = strcpy(temp, "xterm");".
- **A long title is dropped, not cut.** misc.c 6142-6145: "Ignore empty or
  too-long requests. ... if (value == NULL || strlen(value) > 1000) return;".
- **UTF-8 only when valid, else ISO 8859-1.** misc.c 6181-6182:
  `titleIsUTF8 = isValidUTF8((Char *) value); if (IsSetUtf8Title(xw) &&
  titleIsUTF8) {`; otherwise "ChangeGroup: assume ISO-8859-1" and every byte
  goes through `OnlyLatin1` (6226-6234). On the UTF-8 path controls are
  replaced: "if (iswcntrl((wint_t) ch)) { memset(nextc, BAD_ASCII, ..."
  (6219-6220).
- **The user can refuse it.** misc.c 6138, `if (!AllowTitleOps(xw)) return;`;
  xterm.man 2497-2500: "allowTitleOps ... Specifies whether control
  sequences that modify the window title or icon name should be allowed. The
  default is "true"."
- **DECSWT only for a VT520.** misc.c 4172-4177: "case OSC_DECSWT: ... if
  (screen->terminal_id < 520) break; /* FALLTHRU */ case OSC_TitleOnly:".
  The xterm FAQ ([[source: xterm-faq]], rendered 4715-4718, 4726-4727,
  4746-4747): DEC "reused [them] in the DEC VT520/VT525 terminals (July 1994)
  as DECSWT (set window title) and DECSIN (set icon name)"; "DEC's syntax
  requires a single character "2" after the OSC"; "DECSIN conflicts with
  xterm's OSC 2".
- **The icon name is set.** `OSC 1` reaches `ChangeIconName` (misc.c
  4163-4170).
- **Reading the title is off by default.** xterm.man 2502-2512: allowWindowOps
  covers "reporting these values and the title or icon name. Each of these
  can be abused in a script; curiously enough most terminal emulators that
  implement these restrict only a small part of the repertoire. ... The
  default is "false"." And 3492-3499, disallowedWindowOps: "The default value
  is GetChecksum,GetIconTitle,GetSelection,GetWinTitle,SetSelection,SetXprop
  (i.e., all except a few "dangerous" operations are allowed)." The FAQ,
  rendered 3861-3864: "The longstanding control sequence for reading the
  window title is something that can be abused in special conditions."
- **The stack is a ring of ten.** ptyx.h 2371, `#define MAX_SAVED_TITLES 10`;
  a push past ten wraps and overwrites (`xtermPushTitle`, misc.c 7976-7988:
  `which = screen->saved_titles.used++; which %= MAX_SAVED_TITLES;`).
- **RIS resets the title modes.** charproc.c 14432 (`if (full) { /* RIS */`)
  and 14471 (`screen->title_modes = screen->title_modes0;`).
- **One derived fact: the shell's directory, for a new terminal.** misc.c
  1098-1115: "Determine the current working directory of the child so that we
  can spawn a new terminal in the same directory", reading
  `PROCFS_ROOT "/%lu/cwd"`; called as `child_cwd = ProcGetCWD(screen->pid);`
  in `HandleSpawnTerminal` (1147). xterm.man 8531-8541: "spawn-new-terminal
  ... Use the "cwd" process entry, e.g., /proc/12345/cwd to obtain the working
  directory of the process which is running in the current xterm."
- **UTF-8 titles follow the locale, for `OSC 0` and `OSC 2` only.**
  charproc.c 797, `Sres(XtNutf8Title, ..., "default")`, and xterm.man
  5845-5847: "default (3) At startup, the resource is set to true or false,
  according to the effective value of the utf8 resource." The UTF-8 path is
  taken only for a string that begins `0;` or `2;` (charproc.c 3439-3444:
  `sp->string_area[0] == '0' || sp->string_area[0] == '2'`), so an `OSC 1`
  icon name is always masked to seven bits. Inside an OSC string xterm does
  not decode at all, even in a UTF-8 terminal: `BeginString` sets
  `sp->parsestate = sos_table` (charproc.c 2238-2243); each `doparsing` call
  ends with `screen->utf8_inparse = (Boolean) ((screen->utf8_mode != uFalse)
  && (sp->parsestate != sos_table));` (6489-6490); `morePtyData` decodes
  only while `utf8_inparse` is set ([[source: xterm-xterm-h]] 1627-1632); so
  `nextPtyData` takes its raw-byte branch, `result = *((data)->next++);`,
  masked to seven bits unless `output_eight_bits` ([[source: xterm-ptydata-c]]
  324-328), whose resource `eightBitOutput` defaults to "true" (charproc.c
  443; xterm.man 3696-3700: "The default is "true", which means that they are
  accepted as is."), and the accumulator stores `AsciiOf` of the byte (3454-3456). The
  comment above the accumulator says "The string content should always be
  8-bit characters." (3385-3386), and `isValidUTF8` on the stored value
  (misc.c 6181) presumes raw bytes. So with the UTF-8 title mode off, and for
  `OSC 1` always, "é" (C3 A9) becomes "C)" (**C**). `ESC % @` does not touch
  the UTF-8 title mode (`switchPtyData`, ptydata.c 340-358), so an xterm
  started in a UTF-8 locale keeps reading titles as UTF-8 if valid, else
  ISO 8859-1, after `ESC % @` too (**C**). **The texts conflict:** ctlseqs
  says ISO-8859-1 "is the default behavior" (2884-2885), while xterm.man and
  `set_utf8_feature` (charproc.c 10031-10047) make UTF-8 the effective
  default in a UTF-8 locale. The title modes' compiled default is 0
  (main.h 285-286, `DEF_TITLE_MODES 0`). Question 7b asks which baud follows.

### 1.4 `OSC 21` means two different things

[[source: ctlseqs]] 2126 makes `OSC 21` DECSWT. kitty uses the same number for
its colour protocol: [[source: kitty-6d5d0c4-color-stack]] 52,
"<OSC> 21 ; key=value ; key=value ; ... <ST>", dispatched as `color_control`
(kitty vt-parser.c 609-611). Neither is a title for baud: DECSWT is a VT520
function, baud's DA identifies it as a VT102 (CONTRIBUTING.md, "The protocol is enumerated"), and xterm ignores DECSWT below 520 (1.3). A terminal that took
`OSC 21` as DECSWT without that guard would show kitty's `foreground=...`
payloads as titles. kitty's use departs from ctlseqs. The guess of
"OSC 21" as a title source is answered: it is DECSWT, and not for baud.

Since 2026-10-22 baud reads `OSC 21` as kitty's colour control, as kitty
and ghostty do, and DECSWT stays out by xterm's VT520 gate
(`docs/vt-coverage.md`, the `OSC 21 ; key=value` and DECSWT rows).

### 1.5 OSC 7: there is no standard

- **Apple's Terminal is told the directory with it**, by its own shell
  support files ([[source: macos-zshrc-apple-terminal]], macOS 27.0
  26A5425a), lines 6-13: "Tell the terminal about the current working
  directory at each prompt. Terminal uses this to display the directory in
  the window title bar and tab bar, and for behaviors including creating a new
  terminal with the same working directory and restoring the working directory
  when restoring a terminal for Resume." Lines 17-20: "Identify the directory
  using a "file:" scheme URL, including the host name to disambiguate local
  vs. remote paths. ... Percent-encode the pathname." Line 38:
  `printf '\e]7;%s\a' "file://$HOST$url_path"`. The bash file is the same,
  with `$HOSTNAME` ([[source: macos-bashrc-apple-terminal]] 10-13, 33).
  These files show that Terminal uses it, not that Terminal originated it;
  only wezterm's docs say so, as practice: OSC 7 "originated in the macOS
  Terminal application" ([[source: wezterm-b09b56c-shell-integration-md]]
  76-78).
- **No written specification exists.** terminal-wg issue 20, "Specify OSC 7
  behavior", opened 2019-11-01 and still open
  ([[source: terminal-wg-issue-20]]): "to my knowledge there is no
  documentation on the behavior of this escape." The terminal-wg
  specifications repository holds no proposal, only `proposals/template.md`
  (its tree, read through the GitLab API at ba3a5e2,
  [[source: ak-obs-observations]]). The issue's comments answered 401
  without a login and were not read.
- **The payload does have standards.** [[source: rfc8089]] § 2: "The "host"
  is the fully qualified domain name of the system on which the file is
  accessible. This allows a client on another system to know that it cannot
  access the file system" (189-192); § 3: "A file URI is considered "local"
  if it has no "file-auth", or the "file-auth" is the special string
  "localhost", or a fully qualified domain name that resolves to the machine
  from which the URI is being interpreted" (236-240); § 5: "Treating a
  non-local file URI as local, or otherwise attempting to perform local
  operations on a non-local URI, can result in security problems" (287-289).
  § 4 asks for UTF-8 before percent-encoding (253-257), which
  [[source: rfc3986]] § 2.1 defines (634-647). Apple's script encodes byte by
  byte with `LC_CTYPE=C`, which yields the same octets for UTF-8 paths
  (**C**, from the script).
- **Variants in use, all practice:**
  - VTE pairs it with `OSC 6` for the current *file*: `_VTE_OSC(VTECWF, 6)`,
    `_VTE_OSC(VTECWD, 7)` ([[source: vte-0-80-parser-osc]] 65-66). It
    accepts any `file:` URI and checks no host (vteseq.cc 1778-1811).
  - iTerm2 documents it as a synonym for `RemoteHost` plus `CurrentDir`:
    "OSC 7 ; [Ps] ST where [Ps] is a file URL with a hostname and a path"
    ([[source: iterm2-doc-escape-codes]], rendered 368-369).
  - kitty's and ghostty's shell integrations send a different scheme with an
    unencoded path: `'\e]7;kitty-shell-cwd://'"$HOST""$PWD"'\a'` (kitty zsh
    integration 240; ghostty's 235).

### 1.6 A "current command": OSC 133 carries none

The semantic-prompt proposal ([[source: semantic-prompts]] 93-95) defines
`OSC "133;C"` as "End of input, and start of output". It has no field for the
command's text. kitty adds one, `'\e]133;C;cmdline=%q\a'` (kitty zsh
integration 227), which is kitty's extension. In practice a "current command"
title comes from the shell sending `OSC 2` before each command: kitty's
`_ksi_preexec` sends `\e]2;` with the command and `_ksi_precmd` the
abbreviated directory (zsh integration 271-281); ghostty does the same when
its `title` feature is on (243-248). baud needs nothing new for this: it is
`OSC 2`.

### 1.7 The nearest written spec: OSC 3008 (UAPI.15)

[[source: uapi-osc-context]] ("UAPI.15 OSC 3008: Hierarchical Context
Signalling", version 1.0, CC-BY-4.0) has a program report its context with
fields including `hostname=`, `comm=` ("The process name (i.e.
`/proc/$PID/comm`, `PR_GET_NAME`)"), `cwd=` ("The current working
directory") and `cmdline=` (162-169). It warns: "All context information
provided like this should be considered auxiliary and – to some degree –
redundant information. Hence, it would be wise for a terminal to enforce
limits" (196-199). kitty ignores it: "this is systemd's useless OSC 3008
context protocol" (kitty window.py 1547). Listed for completeness; whether
any program in use here sends it is UNVERIFIED.

### 1.8 Unicode: what in a title can mislead

[[source: uax9-tr9-52]] (Unicode 18.0.0) § 2 names the explicit formatting
characters, LRE..RLO U+202A-202E and LRI..PDI U+2066-2069, and says of the
overrides: "They are to be avoided wherever possible, because of security
concerns. For more information, see [UTR36]." (rendered 223-224). That is
advice to authors of text, not a rule for a terminal.
[[source: unicode-data]] gives all nine as general category `Cf`, and U+2028
and U+2029 as `Zl` and `Zp` (lines 7432-7438, 7493-7496). Rust's
`char::is_control` "Returns `true` if this `char` has the general category
for control codes", which is `Cc` ([[source: rust-core-char-methods]]
1032-1036).

### 1.9 Wayland

[[source: wp-xdg-shell]] 689-697, `xdg_toplevel.set_title`: "Set a short
title for the surface. This string may be used to identify the surface in a
task bar, window list, or other user interface elements provided by the
compositor. The string must be encoded in UTF-8." There is no subtitle and no
represented file.

## 2. What a terminal may derive itself

### 2.1 The foreground process group

- **POSIX does not cover the master side.** [[source: posix-tcgetpgrp]],
  DESCRIPTION: "The tcgetpgrp() function shall return the value of the
  process group ID of the foreground process group associated with the
  terminal. If there is no foreground process group, tcgetpgrp() shall
  return a value greater than 1 that does not match the process group ID of
  any existing process group." ERRORS: "The tcgetpgrp() function shall fail
  if: ... [ENOTTY] The calling process does not have a controlling terminal,
  or the file is not the controlling terminal." (rendered 16-30). baud-app is
  not the pty's session leader, so on POSIX's terms the call shall fail. The
  macOS man page says the same ([[source: macos-man-tcgetpgrp]] 16-29).
- **Both kernels allow it on the master, as practice.** XNU's pty driver
  answers before the controlling-terminal check ([[source: xnu-tty-dev]],
  tty_dev.c 1023-1031): "case TIOCGPGRP: /* We aviod calling ttioctl on the
  controller since, in that case, tp must be the controlling terminal. */
  *(int *)data = tp->t_pgrp ? tp->t_pgrp->pg_id : 0;". The slave side
  requires the check ([[source: xnu-tty]], tty.c 1266-1270,
  `if (!isctty(p, tp)) { error = ENOTTY;`). Linux: "(tty == real_tty) is a
  cheap way of testing if the tty is NOT a master pty. */ if (tty == real_tty
  && current->signal->tty != real_tty) return -ENOTTY;"
  ([[source: linux-tty-jobctrl]] 470-475), documented in
  [[source: linux-man-tiocspgrp]] (rendered 41-42) as "ENOTTY The terminal is
  not a master pseudoterminal and not our controlling terminal." The library
  page still says the fd "must be the controlling terminal of the calling
  process" and that the functions "are implemented via the TIOCGPGRP and
  TIOCSPGRP ioctls" ([[source: linux-man-tcgetpgrp]] rendered 16-18, 62-63),
  so the master case is documented only on the ioctl's page. With no
  foreground group, XNU's master returns 0 (**C**, the line above), not
  POSIX's "greater than 1".
- **M.** The probe forks an interactive `zsh` on a pty, from a process in a
  different session (as baud-app is). `tcgetpgrp(master)` read the shell's
  pgid at the prompt, the job's pgid while `/bin/sleep 5` ran (matching
  `ps`' PGID and TPGID), and the shell's again afterwards, in three of three
  launches ([[source: ak-obs-probe-runs]]). The probe's own child lookup
  prints "job pid -1"; why that helper misses the child is UNVERIFIED (a race
  with the shell's fork is the guess), and the `ps` columns are the evidence.

### 2.2 Which process is "the" process

A group can hold several processes. kitty picks the highest pid and says why:
"there is no easy way that I know of to know which process is the foreground
process in this group from the users perspective, so we assume the one with
the highest PID ... For example with a shell script such as: #!/bin/bash /
cd /tmp / vim" (kitty child.py 629-638). wezterm uses the group id as a pid
(`CachedLeaderInfo::update`, localpane.rs 100-109). alacritty does the same,
and falls back to the shell's pid when the call fails (daemon.rs 96-99).
Whether a pipeline's group leader can exit while the group lives on, leaving
nothing to read at the pgid, is UNVERIFIED.

### 2.3 Its name

- **`proc_name` returns the registered name, else the 16-byte command name.**
  XNU's libproc ([[source: xnu-libproc-c]], libproc.c 210-232) reads
  `PROC_PIDTBSDINFO` and copies `pbi_name` when it is set, else `pbi_comm`:
  "if (pbsd.pbi_name[0]) { bcopy(&pbsd.pbi_name, ...) } else {
  bcopy(&pbsd.pbi_comm, ...)". It refuses a buffer smaller than `pbi_name`
  with ENOMEM (216-219). The fields are `char pbi_comm[MAXCOMLEN];` and
  `char pbi_name[2 * MAXCOMLEN]; /* empty if no name is registered */`
  ([[source: macos-sdk-sys-proc-info-h]] 72-73), and `MAXCOMLEN` printed 16
  ([[source: ak-obs-probe-runs]]).
- **M:** for `sleep` resolved through nix's coreutils, a multi-call binary
  reached by a symlink, `proc_name` gave `coreutils` and `proc_pidpath` gave
  `/nix/store/...-coreutils-9.11/bin/coreutils`. `sysctl(KERN_PROCARGS2)`
  gave the path as invoked, `.../bin/sleep`, and `argv[0]` `sleep`, which is
  what `ps` showed ([[source: ak-obs-probe-path-sleep]]). The name a person
  expects is argv[0], and it costs the most to read (2.6).
- **What the others read.** None of kitty, wezterm or alacritty calls
  `proc_name`. kitty calls `proc_pidinfo` and `proc_pidpath` and reads
  `KERN_PROCARGS2` (macos_process_info.c 26, 46, 141, 234); wezterm the same
  three (procinfo/src/macos.rs 25, 55, 113, 137); alacritty only
  `proc_pidinfo` (macos/proc.rs 60).
- **Linux:** `/proc/pid/comm`: "Strings longer than TASK_COMM_LEN (16)
  characters (including the terminating null byte) are silently truncated"
  ([[source: linux-man-proc-pid-comm]] rendered 14-16). `/proc/pid/exe` is "a
  symbolic link containing the actual pathname of the executed command" and
  needs a ptrace check ([[source: linux-man-proc-pid-exe]] rendered 8-21).

### 2.4 Its directory

`proc_pidinfo(pid, PROC_PIDVNODEPATHINFO, ...)` fills a
`struct proc_vnodepathinfo { struct vnode_info_path pvi_cdir; ... }`
(proc_info.h 342-345, 741). **M:** after `cd /tmp` it read `/private/tmp`.
The kernel returns the resolved path, as kitty's comment says ("The underlying
code on macos returns a path with symlinks resolved anyway", child.py 34-35).
Linux: `/proc/pid/cwd` is "a symbolic link to the current working directory
of the process", and "Permission to dereference or read (readlink(2)) this
symbolic link is governed by a ptrace access mode PTRACE_MODE_READ_FSCREDS
check" ([[source: linux-man-proc-pid-cwd]] rendered 8-20).

### 2.5 What it cannot read, and what Apple promises

- **A root-owned process, the `sudo` case. M,** against pid 1: `proc_name`
  returned 0 with EPERM; `PROC_PIDVNODEPATHINFO` returned 0 with EPERM;
  `KERN_PROCARGS2` failed with EINVAL; `proc_pidpath` succeeded
  (`/sbin/launchd`). So under `sudo vim` baud could show a path but not a name
  or a directory. Linux gates `cwd` and `exe` on the ptrace check quoted
  above.
- **Apple calls the API private.** [[source: macos-sdk-libproc-h]] 41-44:
  "This header file contains private interfaces to obtain process
  information. These interfaces are subject to change in future releases."
  The header documents none of the three calls, and the SDK ships no man page
  for them.

### 2.6 What each read costs (M)

[[source: ak-obs-probe-c]], ns per call: 20,000 calls a round, five rounds
with the first dropped. Runs 1-3 ([[source: ak-obs-probe-runs]]) are three
launches with the job `/bin/sleep 5`, whose `KERN_PROCARGS2` area held 33
bytes and no environment strings. Run 4
([[source: ak-obs-probe-path-sleep]]) is one launch with `sleep 5` from PATH,
nix's coreutils: a different binary, a longer path, and an area of 32,836
bytes including its environment.

| Read | Runs 1-3 (12 settled rounds) | Run 4 (4 settled rounds) |
|---|---|---|
| `tcgetpgrp(master)` | 2,537-2,638 | 2,690-2,999 |
| `proc_name` | 161-169 | 203-257 |
| `proc_pidpath` | 491-506 (`/bin/sleep`) | 2,252-2,496 (a 72-byte nix path) |
| `proc_pidinfo(PROC_PIDVNODEPATHINFO)` | 444-473 | 482-598 |
| `proc_pidinfo(PROC_PIDTBSDINFO)` | 157-162 | 172-177 |
| `sysctl(KERN_PROCARGS2)` | 3,822-4,085 | 5,540-5,897 |

Sums of the per-read ranges: group + name + directory, 3.1-3.3 us (runs 1-3);
group + argv[0] + directory, 6.8-7.2 us (runs 1-3) and 8.7-9.5 us (run 4).
Run 4 changes the binary, the path and the argument area at once, so its
higher figures are not put down to any one of them. These are warm loops over
one pid; one check made after the process has been idle is unmeasured.
wezterm's comment, "it can take 700us to tcgetpgrp" (localpane.rs 61), is its
own figure on its own machine and is not reproduced here. The Linux reads are
unmeasured: there is no Linux box here.

### 2.7 When the reads would run

A program's `OSC` title can change only during a read, and the app checks it
then (`baud-app/src/app.rs` 4168-4175: "only a read can set one, so this is the
one place it can have changed"). **A derived value has no such moment.** The foreground
group changes when a job starts or ends; a job can print nothing (`sleep
60`), and a script can start one long after the last key. Packet mode reports
no foreground change: its status bits are "data packet", "flush packet",
"stop output", "start output", "no more ^S, ^Q", "now do ^S ^Q" and "state
change of pty driver" ([[source: macos-sdk-sys-ttycom-h]] 128-135). Darwin
does report the process events that usually go with one: kqueue's
`EVFILT_PROC` "returns when the process performs one or more of the requested
events", among them `NOTE_EXIT`, `NOTE_FORK` ("The process created a child
process via fork(2) or similar call") and `NOTE_EXEC`
([[source: macos-man-kqueue]] 353-371). Following a whole tree is gone:
"NOTE_TRACK, NOTE_TRACKERR, and NOTE_CHILD are no longer supported as of
10.5" ([[source: macos-sdk-sys-event-h]] 357). A watch on the shell's pid
sees a job forked and a watch on the job sees it exit; a job brought back
with `fg`, which forks nothing, is UNVERIFIED. The Linux equivalent is
unexamined.

The project's rule is that anything timed is scheduled on the display link or
the poll timeout, never on a timer thread or a wait (memory, "Scheduled,
never waited"). That leaves these shapes, each costing the reads above per
check (no rate is asserted here):

- after every read, as the `OSC` title is checked now (misses a silent job);
- a deadline after every key write, checked on a later display-link tick or
  poll timeout (the job starts after Enter, not at it);
- on a process event: `NOTE_FORK` on the shell and `NOTE_EXIT` on the job,
  each scheduling one check on the next tick (no idle wakeups; one
  registration per watched pid; its gaps above);
- a periodic deadline while the window is visible (catches everything, wakes
  an idle process);
- some combination. wezterm's shape (a 300 ms cache, refreshed by a spawned
  thread on a stale read, localpane.rs 38 and 1044-1066) breaks baud's rule.

Section 6.5 settles which.

## 3. Platform conventions

### 3.1 macOS: NSWindow and the HIG

- `title`: "The string that appears in the title bar of the window or the
  path to the represented file." Discussion: "Setting this property also sets
  the title of the window's miniaturized window."
  ([[source: apple-nswindow-title]], rendered 2, 6). `miniwindowTitle`:
  "Although this property allows you to set the minimized window's title
  explicitly, changing the full-size `NSWindow` object's title (through title
  or setTitleWithRepresentedFilename(_:)) automatically changes the
  minimized window's title as well."
  ([[source: apple-nswindow-miniwindowtitle]], rendered 6). So an icon name
  set there is overwritten by every title set.
- `subtitle` (macOS 11): "A secondary line of text that appears in the title
  bar of the window. ... When this property is an empty string, the system
  removes the subtitle from the window layout."
  ([[source: apple-nswindow-subtitle]], rendered 2, 6). The SDK header:
  "Secondary text that may be displayed adjacent to or below the primary
  title depending on the configuration of the window."
  ([[source: macos-sdk-nswindow-h]] 299-303).
- `representedURL`, the proxy icon: "When the URL specifies a path, the
  window shows an icon in its title bar ... When the URL identifies an
  existing file, the window's title offers a pop-up menu showing the path
  components of the URL. (The user displays this menu by Command-clicking the
  title.)" ([[source: apple-nswindow-representedurl]], rendered 6-13). The
  header: "If url is not nil and its path is not empty, the window will show a
  document icon in the titlebar." (NSWindow.h 329).
- `titleVisibility`: "A value that indicates the visibility of the window's
  title and title bar buttons." "By default, the value of this property is
  NSWindow.TitleVisibility.visible."
  ([[source: apple-nswindow-titlevisibility-property]], rendered 2, 6).
  `.hidden`: "The window hides the title and moves the toolbar up into the
  area previously occupied by the title."
  ([[source: apple-nswindow-titlevisibility-hidden]], rendered 6).
  `titlebarAppearsTransparent`, which the crate also sets: "the title bar
  does not draw its background, which allows all content underneath it to
  show through" ([[source: apple-nswindow-titlebarappearstransparent]],
  rendered 6).
- **M, and it contradicts the header.** The proxy probe built a window with
  the crate's titled style mask (Titled, FullSizeContentView, Closable,
  Miniaturizable, Resizable; `macos-liquid-glass` `window.rs` 44-50), no
  toolbar, a subtitle set, a `representedURL` of `/tmp`, alpha 0, offscreen
  and ordered back. `standardWindowButton(.documentIconButton)` returned an
  unhidden 16x16 button with an image, in the window's view tree, under
  `.visible`, with `titlebarAppearsTransparent` on or off; under `.hidden` it
  returned **nil**, transparent or not, in two runs
  ([[source: ak-obs-proxy-runs]]). Nothing was seen on screen: the window was
  invisible by construction, so this is the button's existence, not its
  display. The header says the icon shows for any non-nil URL with a path,
  and the abstract ties `titleVisibility` to "title bar buttons"; the probe
  sides with the abstract, in this configuration only. The probe's search for
  the title's text field found nothing even under `.visible`, so it cannot
  fire and says nothing about the title text. ghostty's documentation, "The
  proxy icon is only visible with the native macOS titlebar style"
  (Config.zig 3386), points the same way, but ghostty's other titlebar styles
  were not read (its default is `.transparent`, Config.zig 3379), so its
  agreement is UNVERIFIED.
- The HIG, "Toolbars", § Titles ([[source: apple-hig-toolbars]], rendered
  26-29): "Provide a useful title for each window. A title helps people
  confirm their location as they navigate your app, and differentiates
  between the content of multiple open windows. If titling a toolbar seems
  redundant, you can leave the title area empty." "Don't title windows with
  your app name. Your app's name doesn't provide useful information about
  your content hierarchy or any window or area in your app, so it doesn't
  work well as a title." "keep the title under 15 characters long". And
  "Windows" ([[source: apple-hig-windows]], rendered 14): "Avoid creating
  custom window UI. ... don't try to replicate the system-provided
  appearance." These are Apple's guidance, not a standard. baud's fallback
  title today is "baud" (5.2).

### 3.2 Terminal.app

The Terminal User Guide, "Change Profiles Window settings"
([[source: apple-terminal-guide-window-settings]], rendered 75-86): "To change
the title of a Terminal window, enter a name in the field. Use the title
option checkboxes to select the information that appears in the title bar of
a Terminal window. Working directory or document: Displays the working
directory or document name. Path: Displays the directory or document path.
Active process name: Displays the active process name. Arguments: Displays
the arguments. Shell command name: Displays the name of the running shell
command. Profile name: Displays the name of the window's profile. TTY name:
Displays the terminal name. Dimensions: Displays the size of the window.
Command key: Displays the command key you can use to make the window active."
Tabs have their own set, with "Show other items when there is a custom title:
If you created a custom title, this checkbox controls whether the other items
selected for the title are displayed"
([[source: apple-terminal-guide-tab-settings]], rendered 76-82). The "Change
the title" page: "TTY name: ... The name is equivalent to the result of the
tty command in UNIX." ([[source: apple-terminal-guide-change-title]],
rendered 81).

So Terminal composes fixed components, one per checkbox. Its binary on macOS
27.0 26A5425a carries one key per checkbox (`ShowActiveProcessInTitle`,
`ShowRepresentedURLInTitle`, `ShowDimensionsInTitle` and eleven more, found
with `strings`, [[source: ak-obs-observations]]). Which are on by default is
in neither the guide nor the shipped profiles (`Basic.terminal` carries no key
with "title" in it, same source), so it is UNVERIFIED. The same binary carries
`effectiveRepresentedURL.isValidLocalHostFileURL`, which suggests a local-host
check on `OSC 7`; a symbol name, not observed behaviour: UNVERIFIED.

### 3.3 iTerm2

Docs ([[source: iterm2-doc-profiles-general]], rendered 69-82): the Title menu
"contains items which may be separately enabled. They are combined to form
the session's title. ... The current tab title also serves as the window
title." Its items are Session Name, Profile Name, Profile & Session Name, Job,
User, Host, PWD and TTY. "Applications in terminal may change the title.
When enabled, a control sequence can change a session's or window's title."
Source (iTerm2 8622e64): the components are a bitmask (ITAddressBookMgr.h
524-538: SessionName, Job, WorkingDirectory, TTY, Custom, ProfileName,
ProfileAndSessionName, User, Host, CommandLine, Size, TemporarySessionName,
AI). The default is `Job` alone (iTermProfilePreferences.m 1265). On
precedence: "A non-tmux WINDOW title: an explicit OSC 2 window name IS the
window title ... OSC 2 window titles win over tab titles", returned verbatim
(iTermSessionTitleBuiltInFunction.m 292-300). The composed form is
`name (job) — user@host:pwd — tty — cols×rows` (363-428). For `OSC 7` it can
tell a local machine by a token rather than by hostname
(iTermMachineIdentity.swift 5-16). How iTerm2 decodes title bytes was not
read.

## 4. Prior art at source

Every row is practice; none is a standard. Pins: kitty 6d5d0c4, wezterm
b09b56c, ghostty d9840f3, foot 2705e36, alacritty 0c405d5 (v0.15.1), iTerm2
8622e64, xterm patch #411.

| | App title (OSC 0/2) | Shown text | Derived | OSC 7 | Remote OSC 7 | Stack 22/23 | CSI 21 t | Proxy icon |
|---|---|---|---|---|---|---|---|---|
| **xterm** | `allowTitleOps`, on | UTF-8 if valid, else ISO 8859-1; controls replaced; >1000 bytes dropped; empty = "xterm" | the shell's directory, for spawn-new-terminal only (misc.c 1098-1115, 1147) | none | – | ring of 10 | off by default (`disallowedWindowOps`) | – |
| **kitty** | per window; a user override wins (window.py 890-891) | UTF-8 with replacement (window.py 240); `[\0-\x19\x80-\x9f]` deleted, runs of space collapsed (utils.py 159-160); empty = argv[0]'s basename (window.py 779-780) | `{tab.active_exe}`, `{tab.active_wd}` in templates, "expensive, requires syscall" (definition.py 2308-2315) | stored raw (screen.c 3647-3657); a new window starts in it when the child is local and at a prompt (window.py 164-174) | not checked at store | ten, dropping the oldest: `deque(maxlen=10)` (window.py 781, 1870-1879) | not handled: CSI t takes 4, 8, 14, 16, 18, 22, 23 (vt-parser.c 1348-1359) | none found (`representedURL` in no `.m` file) |
| **wezterm** | stored per pane (performer.rs 744-771) | – | process basename when the title is the default "wezterm" (localpane.rs 449-460) | parsed as URL (performer.rs 940-945); wins over derived (localpane.rs 512-518) | not checked | – | off by default, "for security concerns with shells that might otherwise attempt to execute the response" (config.rs 260-267); when on, the program's own title (terminalstate/mod.rs 2176-2186) | – |
| **ghostty** | `title =` forces one (Config.zig 1523-1537) | ≥256 bytes dropped (stream_handler.zig 943-948) | none; the OSC 7 path becomes the title until a program sets one (1550-1554) | `file` or `kitty-shell-cwd` (1500-1505) | **refused**: "OSC 7 is a little sketchy because anyone can send any value from any host (such an SSH session). The best practice terminals follow is to valid the hostname to be local." (1516-1530) | unimplemented (372-376) | off by default: "can expose sensitive information at best and enable arbitrary code execution at worst" (Config.zig 2500-2509) | `representedURL` = OSC 7 path (BaseTerminalController.swift 928-937) |
| **foot** | `locked-title` option (foot.ini.5.scd 433-435) | not UTF-8 and printable: refused (terminal.c 3683-3686, misc.c 48-63); RIS resets to the configured title (terminal.c 2125) | none | stored (osc.c 456-475); a new terminal starts in it (terminal.c 3816-3830) | refused unless `hostname_is_localhost` (osc.c 467) | 128, then refused (csi.c 1409-1413) | compiled out, "Disabled for now, see #1894" (csi.c 1394) | – |
| **alacritty** | `dynamic_title` (alacritty.5.scd 165-169) | each parameter that is not UTF-8 dropped ([[source: alacritty-vte-ansi]] 1350-1354) | the foreground directory, to start a new window (event.rs 845-848) or, through `spawn_daemon` (daemon.rs 63), a new instance (event.rs 817-841), a hint command (1158-1161) and the bell command (1757-1760) | none | – | 4096, dropping the oldest (term/mod.rs 42, 2232-2241) | not handled: CSI t takes 14, 18, 22, 23 (ansi.rs 1739-1745) | – |
| **iTerm2** | wins verbatim | not read | Job, PWD, user, host, TTY, size, command line | `RemoteHost` + `CurrentDir` | shown as `user@host:pwd` | – | – | – |
| **Terminal.app** | a custom title in settings; program titles UNVERIFIED | – | the checkboxes of 3.2 | uses it (1.5) | "disambiguate local vs. remote" | – | – | yes, by ghostty's account: "You can see this very clearly in the macOS built-in Terminal.app titlebar" (Config.zig 3381-3384) |

The short file names resolve to these keys (each pinned at the commit above,
or holding the same bytes; lines are of that file):

| File | Key |
|---|---|
| kitty `window.py` | [[source: kitty-window-py]] |
| kitty `child.py` | [[source: kitty-6d5d0c4-child-py]] |
| kitty `utils.py` | [[source: kitty-6d5d0c4-utils-py]] |
| kitty `macos_process_info.c` | [[source: kitty-6d5d0c4-macos-process-info]] |
| kitty `vt-parser.c` | [[source: kitty-6d5d0c4-vt-parser]] |
| kitty `screen.c` | [[source: kitty-6d5d0c4-screen]] |
| kitty `definition.py` | [[source: kitty-6d5d0c4-options]] |
| kitty zsh integration | [[source: kitty-6d5d0c4-zsh-integration]] |
| wezterm `localpane.rs` | [[source: wezterm-localpane-b09b56c]] |
| wezterm `procinfo/src/macos.rs` | [[source: wezterm-b09b56c-procinfo-macos]] |
| wezterm `performer.rs` | [[source: wezterm-b09b56c-performer]] |
| wezterm `termwindow/mod.rs` | [[source: wezterm-termwindow-mod]] |
| wezterm `get_current_working_dir.md` | [[source: wezterm-b09b56c-get-cwd-md]] |
| wezterm `config.rs` | [[source: wezterm-config-3725482]] |
| wezterm `terminalstate/mod.rs` | [[source: wezterm-b09b56c-terminalstate-mod]] |
| ghostty `stream_handler.zig` | [[source: ghostty-d9840f3-stream-handler]] |
| ghostty `Config.zig` | [[source: ghostty-config-d9840f3]] |
| ghostty `BaseTerminalController.swift` | [[source: ghostty-d9840f3-base-terminal-controller]] |
| ghostty zsh integration | [[source: ghostty-d9840f3-zsh-integration]] |
| foot `osc.c` | [[source: foot-2705e36-osc]] |
| foot `terminal.c` | [[source: foot-terminal-cb27717]] |
| foot `csi.c` | [[source: foot-2705e36-csi]] |
| foot `misc.c` | [[source: foot-2705e36-misc]] |
| foot `foot.ini.5.scd` | [[source: foot-2705e36-foot-ini-5]] |
| alacritty `term/mod.rs` | [[source: alacritty-0c405d5-term-mod]] |
| alacritty `event.rs` | [[source: alacritty-0c405d5-event]] |
| alacritty `daemon.rs` | [[source: alacritty-0c405d5-daemon]] |
| alacritty `macos/proc.rs` | [[source: alacritty-0c405d5-macos-proc]] |
| alacritty `alacritty.5.scd` | [[source: alacritty-0c405d5-man-5]] |
| alacritty `vte` `ansi.rs` | [[source: alacritty-vte-ansi]] |
| iTerm2 `ITAddressBookMgr.h` | [[source: iterm2-8622e64-addressbookmgr-h]] |
| iTerm2 `iTermProfilePreferences.m` | [[source: iterm2-8622e64-profile-preferences]] |
| iTerm2 `iTermSessionTitleBuiltInFunction.m` | [[source: iterm2-8622e64-session-title]] |
| iTerm2 `iTermMachineIdentity.swift` | [[source: iterm2-8622e64-machine-identity]] |
| VTE `vteseq.cc`, `vte.sh.in` | [[source: vte-0-80-vteseq]], [[source: vte-0-80-vte-sh]] |
| macOS `tcgetpgrp.3`, `kqueue.2` (roff) | [[source: macos-sdk-man-tcgetpgrp-3]], [[source: macos-sdk-man-kqueue-2]] |

What the table shows: two shapes. **The program's title wins outright, and
other facts fill in only when it sets none** (wezterm with the process;
ghostty with the OSC 7 directory; kitty only when a template asks for
`tab.active_exe` or `tab.active_wd`). **Or the program's title is one
component among fixed ones** (Terminal.app; iTerm2's tab titles, while its
window title is the program's verbatim). Of the six whose code was read for
`CSI 21 t`, none answers it by default: xterm, ghostty and wezterm keep it
off, foot compiles it out, kitty and alacritty do not handle it. wezterm,
when enabled, answers the program's own title, not a composed one. iTerm2 and
Terminal.app were not checked.

## 5. baud today

### 5.1 The core

- `OSC 0/1/2` set the title and icon buffers (`baud-core/src/osc.rs`
  147-152), and `Term::title` / `Term::icon_name` return them (`term.rs`
  2500-2508). Each is bounded by `OSC_MAX` = 512, truncated, with the
  truncation counted (`storage.rs` 611-616): "A title past this is clipped
  and `Term::string_truncations` says how often". That is practice, like
  xterm's 1000-byte drop and ghostty's 256-byte drop; no text sets a length.
- C0 in a string body is ignored, after [[source: williams]] (`graph.rs`
  1051-1066). DEL and bytes 0x80-0xFF are stored, except that under
  ISO 8859-1 a `9C` ends the string (`graph.rs` 1040-1044).
- **A departure found on the way, not decided here: under ISO 8859-1 the
  other C1 bytes inside a string are stored, not executed (C).**
  `graph.rs` `string()` ends a string only on `9C` and sends 80-9B and 9D-9F
  to `StrPut` (1042-1067); the only cancels taken from any state are `CAN`
  and `SUB` (`graph.rs` 663). Williams' "anywhere" block makes them controls
  in every state: "80-8F,91-97,99,9A / execute" ([[source: williams]]
  263-268), and 9B, 90, 9D and 98/9E/9F enter CSI, DCS, OSC and SOS/PM/APC
  (296-301, 344-349, 397-402, 416-422). In UTF-8 those bytes are
  continuations and the rule does not apply (1.1). An open defect; it
  touches no title option.
- `OSC 7` and `OSC 21` reach `Osc::Unsupported` (`baud-domain/src/vt/osc.rs`
  399) and are counted in `strings_ignored` and `unsupported_total`
  (`osc.rs` 267-270).
- XTWINOPS 20-23 are `WindowOp::Refused` and counted
  (`baud-domain/src/vt/csi.rs` 382-410; `docs/vt-coverage.md` 807: "`CSI 22 ;
  2 t` / `CSI 23 ; 2 t`, the title save and restore vim sends, is the one with
  a real consumer and the one to take first"). XTSMTITLE, XTRMTITLE and
  XTTITLEPOS are counted, not built.
- RIS does not clear the title, as xterm's does not;
  foot resets it (section 4).

### 5.2 The app

`baud-app/src/title.rs` turns the bytes into text: read in the encoding in
force (UTF-8 lossy, `String::from_utf8_lossy` at line 26, or ISO 8859-1 byte
for codepoint), `c.is_control()` mapped to a space, the ends trimmed, an empty
title replaced by "baud" (24-36). `Shown` sends a title only when the bytes
or the encoding changed (43-67). The macOS pump checks after a drain (`app.rs`
4168-4175); the Linux driver does the same after each read (`linux.rs`
372-377) and sends `xdg_toplevel.set_title`.

**Encoding: not settled here.** Under UTF-8 baud decodes with
`String::from_utf8_lossy`, an ill-formed sequence becoming U+FFFD (question
7b's option L); under `ESC % @` it reads byte for codepoint (question 7a's
option B, ctlseqs' default and a departure from § 9).

### 5.3 The window

baud links the window crate as `macos-liquid-glass` 1.0.0-beta.6
(`Cargo.lock` 431-433), whose `src/window.rs`, like 1.0.0-beta.5's, is
byte-identical to glassnote's own checkout at 5fa641f
([[source: ak-obs-observations]]). It sets
`setTitlebarAppearsTransparent(true)` and
`setTitleVisibility(NSWindowTitleVisibility::Hidden)` for the titled shape,
"drop the title text — the surface draws its own" (`src/window.rs` 266-270),
and exposes `set_title` only (476-478): no subtitle, no `representedURL`, no
`miniwindowTitle`. So today a program's title reaches the Window menu,
Mission Control and accessibility, not the titlebar (`docs/vt-coverage.md`
760). In the probe's configuration, which is this style, AppKit created no
document-icon button while the title was hidden (3.1, M).

### 5.4 Two defects found reading it

- **Bidi and separator characters reach AppKit (C).** `char::is_control` is
  `Cc` only (1.8), so LRE..RLO, LRI..PDI and U+2028/U+2029 in the middle of a
  title pass `window_title` unchanged. `trim` removes U+2028 and U+2029 only
  at the ends: "'Whitespace' is defined according to the terms of the Unicode
  Derived Core Property `White_Space`" ([[source: rust-core-str-mod]]
  2150-2153), and both are `White_Space` ([[source: unicode-proplist-17]]
  18-19). So a remote program's overrides reach the Window menu and Mission
  Control. Whether AppKit then reorders the text there is UNVERIFIED. No test
  covers it. No text requires removing them (1.8), so whether this is a
  defect is question 6.
- **The fallback is the app's name**, which the HIG advises against, while it
  allows an empty title area where a title would be redundant (3.1). Guidance,
  not a standard; it is open in question 2.

## 6. Options

Marked **conforming** where a text decides it, **practice** where only other
terminals do, and **departure** where it goes against a text.

### 6.1 Where the text shows (macOS) — the gate

All three set the window title to Pt, so all three conform to ctlseqs; no
text says where a title is displayed. They differ against the HIG's guidance
and in cost.

- **S1. The crate shows its title.** `titleVisibility = .visible`, the title
  drawn by AppKit; `subtitle` and `representedURL` become available
  natively, and the document-icon button existed in the probe (3.1, M).
  Practice: ghostty's native style; in line with the HIG's "Avoid creating
  custom window UI". Cost: a change to the window crate (extend it, never
  duplicate it); the titlebar's look changes; the baud mark and the modem
  LEDs (note AH) share the strip with AppKit's title.
- **S2. baud draws the title in its titlebar strip.** Full control of
  placement and type, and the glass look unchanged. No terminal studied does
  this on macOS, and it is what the HIG's "Avoid creating custom window UI"
  advises against. No native document-icon button in the probe's
  configuration (M), so a proxy icon would be baud's own, with its own drag
  and Command-click menu. Cost: text shaping and drawing in the strip on every
  change, through `baud-font` (unmeasured); the accessibility and Window-menu
  title still set through `set_title`.
- **S3. Leave it hidden** (today). The Window menu, Mission Control and
  accessibility keep the title. Cost: the request stays unmet in the
  titlebar: no directory, no process, no proxy icon there.

On Wayland only `set_title` exists (1.9). Under S1 or S2 the subtitle and
proxy icon are macOS-only, against the memory "same pixels on every OS":
whatever goes in the macOS subtitle needs a Linux home or must be absent.

### 6.2 What the title says, and who wins

- **T1. The program's title only, with a fallback** (today). Conforming:
  ctlseqs defines only the program's title. Cost: no reads; the
  request (directory, process) stays unmet.
- **T2. The program's title wins; derived facts only when it sets none.**
  The fallback becomes the foreground process name or the directory, never
  "baud". Practice: wezterm (the process), ghostty (the OSC 7 directory).
  Conforming while derived facts appear only in place of a missing title; a
  **departure** from ctlseqs 2036 ("Ps = 2 -> Change Window Title to Pt")
  whenever they are added beside a title the program set. Cost: the derive
  reads (2.6) on the schedule of 6.5.
- **T3. Composed from fixed components**, the program's title one of them
  (Terminal.app's checkboxes, iTerm2's tab menu). **Departure** from ctlseqs
  2036: the window title is no longer Pt. iTerm2 itself returns an `OSC 2`
  title verbatim for the window (iTermSessionTitleBuiltInFunction.m
  292-300). Cost: the reads, plus a settings surface.
- **T4. A template string** (kitty's `tab_title_template`, wezterm's Lua
  `format-window-title`). **Departure** from ctlseqs 2036 whenever the
  template adds anything to the program's title. Cost: a template grammar in
  the config and its parser (kitty's runs Python's formatter, definition.py
  2337-2338), plus the reads.

Under any of them the program's title is never altered except by decoding
and sanitising (6.6), and where it is shown beside derived facts it is
visibly separate, so a remote program cannot pose as the local directory.

### 6.3 The proxy icon and the subtitle (macOS, S1 only)

- **P1. `representedURL` from `OSC 7`, local only.** Practice: ghostty.
  RFC 8089 makes "local" decidable (1.5). baud's nearest rule is
  `baud-app/src/link.rs` `admit`, which accepts a `file://` host that is
  empty, `localhost` or equal to one stored name string (100-167). It differs
  from RFC 8089 four ways: § 3's "a fully qualified domain name that resolves
  to the machine" is not tested by a single string compare; any byte outside
  0x21-0x7E is refused, so an unencoded path with a space is; the
  authority-less form `file:/path` (the "local-path" rule, RFC 8089 179-184)
  is refused, because `admit` requires `//` (link.rs 160); and it knows no
  `kitty-shell-cwd` scheme. Cost: one parse and host compare per `OSC 7`
  (every prompt, from Apple's script), one AppKit set when the path changes,
  and a decision on those four differences.
- **P2. Also from the derived directory** when no `OSC 7` arrived. Practice:
  kitty and wezterm fall back the same way for new windows. Cost: the reads.
- **P3. `subtitle` carries the directory or the process**, the title keeps
  the program's text. Practice: ghostty's `window-subtitle =
  working-directory`, "only supported on GTK" (Config.zig 2167-2176); iTerm2's
  tab subtitle. Cost: one AppKit set per change, and the reads if derived; on
  Linux it has no home (6.1).

### 6.4 OSC 7 in the core

Store the URI bounded and counted like `OSC 8`'s, expose it as
`Term::cwd_uri`, and parse in `baud-domain`. Cost: one bounded buffer in
`Storage` (size to choose) and a parse per `OSC 7`. Then:

- **R1. A remote host is refused** for the proxy icon and any local use, and
  counted. Conforming to RFC 8089 § 5's warning: no local operation on a
  non-local URI. Practice in ghostty and foot. Cost: the host test of P1; an
  ssh session's directory is never shown.
- **R2. A remote host is shown as text** (`host:path`) in the title or
  subtitle, never as a proxy icon or a new window's directory. Equally
  conforming to RFC 8089 § 5: showing a remote path performs no local
  operation on it. Practice in iTerm2. Cost: the host test, a place in the
  title or subtitle, and sanitising (6.6) of a remote string. Accepting
  `kitty-shell-cwd://` is practice too (kitty's and ghostty's integrations
  send it); it is not a `file` URL.
- **R0. Any host accepted as given**, no test at the terminal layer. Practice
  in VTE (any `file:` URI, 1.5) and wezterm (parsed, host not checked);
  kitty stores the URL unchecked and declines it for a new window when the
  child is remote by its own test (window.py 164-174). **Departure** from
  RFC 8089 § 5 wherever a remote path is then used locally (a proxy icon, a
  new window's directory). Cost: none to build; a remote program can name a
  local path.
- Whichever is chosen, the decoded path is attacker-controlled and goes
  through 6.6.

### 6.5 Scheduling the derived reads — settled, if any option reads

Only T2-T4, P2 and a derived P3 read anything. If one is chosen, the reads
run **after every read of the pty, and on `EVFILT_PROC` `NOTE_FORK` on the
shell and `NOTE_EXIT` on the job, each scheduling one check on the next
display-link tick**; never on a timer, a thread or a periodic deadline. The
reason is the project's scheduling rule plus coverage: the read catches
whatever a program prints; the process events catch a job that starts or ends
silently; neither wakes an idle process. Its known gap, a job brought back
with `fg` (2.7), is UNVERIFIED and would be closed by a check after each key
write if it proves real. A read whose answer is unchanged sends nothing to
the window (the `Shown` rule extended). Cost per check, warm, on this machine
(2.6): 3.1-3.3 us without argv[0]; 6.8-7.2 us with it for a small argument
area (runs 1-3), 8.7-9.5 us in run 4; one check after idle is unmeasured; one
kqueue registration per watched pid. Under `sudo` the name and directory fail
with EPERM (2.5), and the title must still say something true: the path from
`proc_pidpath`, or nothing derived.

### 6.6 What is shown: sanitising

All three are practice: UAX #9's sentence is advice to authors, not a
terminal rule (1.8), and no text requires removing the characters.

- **H0. Keep today's.** `Cc` to space, trimmed; bidi and separator
  characters pass (5.4). Cost: none; a remote title can carry overrides into
  the Window menu.
- **H1. Also drop the bidi controls** (U+202A-202E, U+2066-2069, and the
  marks U+200E, U+200F, U+061C if wanted) **and U+2028/U+2029.** xterm
  replaces `iswcntrl` characters (1.3); whether the bidi controls (`Cf`)
  count as `iswcntrl` depends on the locale's tables: UNVERIFIED. Cost: one
  more character test per title character; an RTL title that relied on the
  controls may display in the wrong order.
- **H2. Isolate rather than drop:** wrap each untrusted part in FSI ... PDI
  (UAX #9 § 2.4) so it cannot reorder its neighbours, after removing any
  unbalanced controls inside. More faithful for RTL titles, which note AB's
  bidi work will care about. Cost: a balance pass per title and two added
  characters per part; how AppKit renders isolates in the title is
  UNVERIFIED.
- **Length:** keep the core's 512 bytes (practice). The HIG's "under 15
  characters" addresses an app's own titles, not a program's.

### 6.7 The title stack and the icon name

- **The title stack: ctlseqs where it speaks, xterm's practice where it does
  not. Settled.** `CSI 22/23 ; Ps t` with direct access to slots 1-10,
  XTTITLEPOS, XTSMTITLE/XTRMTITLE and DECRQSS `> Pm t`: conforming. vim sends
  22/23 (`docs/vt-coverage.md` 807), and the memory "Protocols fully, always"
  asks for all of it. **Depth and overflow are practice**, since ctlseqs gives
  only the direct-access range: xterm a ring of ten that wraps (misc.c
  7976-7988); kitty ten, dropping the oldest (`deque(maxlen=10)`, window.py
  781); foot 128 then refused (csi.c 1409-1413); alacritty 4096 dropping the
  oldest (term/mod.rs 2235-2241). baud takes xterm's ring of ten, because
  ctlseqs' direct-access range 1-10 is written against that implementation
  and a ring keeps every slot it names addressable. XTSMTITLE's modes 0 and 1
  (hexadecimal) stand alone; modes 2 and 3 (UTF-8 set and query) depend on
  question 7. RIS resets the title modes, as xterm's does (1.3). Cost: ten
  slots, each holding a title and an icon name of up to `OSC_MAX` (10 KiB at
  512 bytes, **C**), and the modes in the reset.
- **The icon name (`OSC 1`): a choice the project made (question 8).** ctlseqs: "Ps =
  1 -> Change Icon Name to Pt." (2035).
  - **I1. To `miniwindowTitle`**, AppKit's nearest counterpart: conforming to
    2035 in that reading. xterm sets the X icon name (`ChangeIconName`,
    misc.c 4163-4170). kitty's handler is `pass  # TODO: Implement this`
    (window.py 1549-1550); foot ignores `OSC 1` (osc.c 1305-1306); others
    UNVERIFIED. AppKit overwrites the miniwindow title on every title set
    (3.1), so baud would re-apply the icon name after each one. Cost: the
    crate must expose `miniwindowTitle`; one extra AppKit set per title
    change; under `OSC 0`, which sets both, the order matters.
  - **I2. Stored only** (today). The sequence is recognised and its effect
    dropped, the state CONTRIBUTING.md warns against ("Never record a mode you do
    not act on"; a sequence the parser recognises and drops "is worse than
    one it does not implement"). Cost: none to build; the departure stays.
- **`OSC 21` stays refused and counted**, with a coverage row saying why:
  DECSWT is the VT520's, baud identifies as a VT102, and kitty uses the number
  for colours (1.4).

### 6.8 `CSI 21 t`

- **Q-A. Keep refusing, counted** (today). Conforming: ctlseqs lets the whole
  family "be disabled using the allowWindowOps resource" (1627-1628).
  Practice: xterm's default `disallowedWindowOps`, ghostty's and wezterm's
  defaults, foot compiled out. Cost: a program that asks gets no answer; one
  that wants to restore a title must use the stack.
- **Q-B. Answer the program's own title, behind an off-by-default setting.**
  Conforming to ctlseqs' shape `OSC l label ST`; practice in wezterm. **Never
  the composed title:** under T2-T4 a composed title holds local facts (the
  directory, the process), and answering with it would let a remote program
  read them. Cost: a setting; a reply of up to `OSC_MAX` + 5 bytes, larger
  than today's 256-byte `REPLY` (note AG), so it joins AG's budget; and the
  risk xterm, ghostty and wezterm name when it is turned on: a title the
  attacker set, echoed back as input.

## 7. Open questions

1. **Show the title on macOS: S1, S2 or S3?** This is the open "should the
   macOS titlebar show a program's title?", and it gates everything else
   visible. All three conform to ctlseqs. S1: AppKit's title, subtitle and
   proxy icon (the button existed in the probe), at the cost of changing the
   crate and the strip's look. S2: baud's own drawing and the glass look kept,
   at the cost of drawing it, making any proxy icon by hand, and going against
   the HIG's guidance. S3: nothing changes, and the request stays unmet in the
   titlebar.
2. **What the title says when the program sets none, and whether derived
   facts appear when it does:** T1 (today, "baud"; conforming), T2 (the
   program's title wins, then the process or the directory; conforming while
   derived facts only replace a missing title), or T3 (composed; a departure
   from ctlseqs 2036)? All three are practice somewhere. The HIG advises a
   useful title rather than the app's name, and allows an empty title area
   where a title would be redundant.
3. **Fixed choices (T3, Terminal.app and iTerm2) or a template string (T4,
   kitty and wezterm)?** Only if the answer to 2 composes; both depart from
   ctlseqs 2036 when they add to the program's title.
4. **`OSC 7` from another host: refuse it (R1, ghostty and foot), show
   `host:path` as text only (R2, iTerm2), or accept any host as given (R0,
   VTE and wezterm)?** R1 and R2 conform to RFC 8089 § 5; R0 departs from it
   wherever the path is then used locally.
5. **`CSI 21 t`: keep refusing (Q-A, conforming, today), or answer the
   program's own title behind an off-by-default setting (Q-B, conforming)?**
6. **Bidi and separator characters in a title: keep today's (H0), drop them
   (H1), or isolate the untrusted text (H2)?** All three are practice.
7. **How title bytes 0x80-0xFF are read.** The governing texts differ by
   encoding (1.1), so two questions. Each option is marked against every
   text that governs its encoding; "Shows" gives display outcomes, "Cost"
   the costs.
   - **7a. Under `ESC % @`** (ISO 8859-1, an ECMA-35 8-bit code). Two texts
     conflict: ECMA-48 § 9 ("shall have the same meanings as" GL) against
     ctlseqs' ISO-8859-1 "So that is the default behavior" (2884-2885).
     - **G. The GL reading.** A0-FE mean 20-7E. Conforming to § 9 (10/00 to
       15/14) and to X3.64 App. H for A1-FE; departs from ctlseqs' default.
       FF: § 9 gives it no meaning (its range stops at 15/14); Williams maps
       it to 7F ("treated identically to GL codes 20-7F", 462-463). Practice:
       xterm whenever its UTF-8 title mode is off (`AsciiOf`, charproc.c
       3454-3456; ptyx.h 330). Shows: an ISO 8859-1 "café" (63 61 66 E9) as
       "cafi"; a UTF-8 "café" (63 61 66 C3 A9) sent anyway as "cafC)". Cost:
       a seven-bit mask in `title.rs`.
     - **B. Byte = ISO 8859-1 codepoint** (baud today). Conforming to
       ctlseqs' default; departs from § 9. Shows: an ISO 8859-1 "café" as
       "café"; a UTF-8 "café" sent anyway as "cafÃ©". Cost: none to build.
     - **X. UTF-8 if the whole title is valid, else ISO 8859-1** (practice:
       xterm started in a UTF-8 locale, after `ESC % @`; 1.3). Departs from
       § 9; for a valid UTF-8 title it departs from ctlseqs' default too, and
       for any other it conforms to it. Shows: both "café" spellings as
       "café". Cost: a whole-title validity pass.
   - **7b. Under UTF-8** (outside ECMA-35's structure, so § 9 does not
     reach it). The Unicode Standard governs the decoding (C8, C10), and two
     texts conflict on the default: ctlseqs' Title Modes (ISO-8859-1 by
     default, UTF-8 when `utf8Title` or XTSMTITLE 2 asks, 2891-2894 and
     1713) against xterm.man's `utf8Title` default (UTF-8 in a UTF-8 locale,
     5845-5847).
     - **L. Lossy UTF-8, an ill-formed sequence as U+FFFD** (baud today,
       `title.rs` 26; kitty, window.py 240). Conforming to C8 and C10: the
       commentary after C10 offers "representing the code unit with a marker
       such as U+FFFD REPLACEMENT CHARACTER", and TUS § 3.9 describes
       maximal-subpart substitution as a practice: "Although the Unicode
       Standard does not require this practice for conformance". Follows xterm.man's default; departs from
       ctlseqs' default. Shows: an ISO 8859-1 "café" (63 61 66 E9) as "caf�";
       E9 A9 41 as "�A", one U+FFFD for two bytes (measured,
       [[source: ak-obs-lossy-out]]). How Rust groups an ill-formed run (the
       "invalid sequence" of at most three bytes, `Utf8Chunk::invalid`,
       [[source: rust-core-str-lossy]] 88-99) was read only as documented;
       whether it matches TUS's maximal subparts exactly is UNVERIFIED. Cost:
       none to build.
     - **X. UTF-8 if the whole title is valid, else ISO 8859-1** (xterm in
       its UTF-8 title mode, misc.c 6181-6182 and 6226-6234). Follows
       xterm.man's default for a valid title; departs from ctlseqs' default
       then, and conforms to it otherwise. Under the reading that a title in a
       UTF-8 stream "purports to be" UTF-8, its fallback departs from C10
       (ill-formed bytes read as characters) and from C8 (the title's
       well-formed sequences read as byte pairs). Shows: "café" (63 61 66 E9)
       as "café"; one stray byte flips the whole title, so 63 61 66 C3 A9 20
       E9 shows "cafÃ© é" where L shows "café �". Cost: a whole-title
       validity pass.
     - **F. Refuse what is not valid UTF-8.** foot refuses the whole title
       (terminal.c 3683-3686); alacritty drops each invalid parameter and
       joins the rest with ";" (ansi.rs 1350-1358). Both meet C10's "shall".
       alacritty's half goes against the commentary right after it: "Silently
       ignoring ill-formed sequences is strongly discouraged because joining
       text from before and after the ill-formed sequence can cause the
       resulting text to take a new meaning." Follows xterm.man's UTF-8
       default; departs from ctlseqs' default. Shows: "café" as 63 61 66 E9
       not at all (foot), or with that parameter missing (alacritty). Cost:
       a program's title can vanish.
     - **K. ctlseqs' letter: decode, then reconvert to ISO-8859-1 by
       default; keep UTF-8 only after XTSMTITLE 2.** Conforming to ctlseqs
       2891-2894 and to C8; departs from xterm.man's default; C10: ctlseqs
       says nothing of ill-formed input, so it depends on the decoder. Shows:
       "café" as "café"; a character above U+00FF as whatever "reconversion"
       makes of it, which ctlseqs does not say (UNVERIFIED). Cost: every UTF-8
       program's non-Latin-1 title is lost unless it sends XTSMTITLE 2, and
       which programs send it is UNVERIFIED.
     - **A. Mask each byte to seven bits, undecoded** (practice: xterm with
       its UTF-8 title mode off, and for `OSC 1` always: charproc.c
       6489-6490, [[source: xterm-ptydata-c]] 324-328, charproc.c 3454-3456;
       1.3). Under the same "purports" reading as X, departs from C8 and C10;
       applies § 9's rule outside the ECMA-35 code 1.1 says § 9 does not
       reach; follows neither ctlseqs' ISO-8859-1 nor xterm.man's UTF-8.
       Shows: "é" (C3 A9) as "C)"; any non-ASCII title unreadable. Cost: a
       seven-bit mask in `title.rs`.

     Decoding first and then masking each character ("é" shown "i") is not
     offered: no terminal read for section 4 does it.

   Whatever is chosen is recorded in `docs/vt-coverage.md`. XTSMTITLE's
   UTF-8 modes follow from it.
8. **The icon name (`OSC 1`): shown as the minimized window's title,
   re-applied after every title set (I1), or stored only (I2, today, the
   recognised-and-dropped state)?**

Settled by this note, not asked: the title stack and its siblings, with
xterm's ring of ten (6.7); the scheduling of derived reads, if any option
reads (6.5); `OSC 21` refused with its reason; `OSC 7` parsed and stored in
the core whatever the answer to 4. Not part of this decision, recorded as
open: the ISO 8859-1 C1 bytes stored inside strings (5.1).
