# AM. XTGETTCAP: what baud says about its own capabilities

The question came in as "baud does not answer XTGETTCAP, and neovim and tmux
send it". Half of that holds at source. **tmux neither sends XTGETTCAP nor
answers it**, at either commit read (section 4). neovim does send it, but under
baud's environment it sends one name, `Ms`.

Evidence: the harnesses and their outputs are in
`docs/research/notes/am/`, and line numbers in baud's code are at
`4911bf0`. The third-party sources are cited by key, pinned under group AM.

* `bounds.py` computes the sizes and bounds; its output is
  `bounds-output.txt`.
* `keyprobe-main.rs` drives baud's real key encoder; its output is
  `keyprobe-output.txt`.
* `keymax-main.rs` measures the longest output of baud's real `encode_key`
  over four domains, each enumerated exhaustively; its output is
  `keymax-output.txt`.
* `straddle-probe.rs` feeds baud a DCS split across two reads; its output is
  `straddle-output.txt`.
* `xtgettcap-controls.rs` checks four facts about baud at `4911bf0`: the
  kitty flags query, F21 through `Term::key`, how many hex bytes a `DCS + q`
  body holds today, and BEL in a `DCS + q` body. Its output is
  `xtgettcap-controls-output.txt`, and its checks are cited Q1-Q4. Q4's BEL
  ended the body at `4911bf0`; since `72e5ce0` a BEL in a DCS body is payload
  and only ST ends it (`a_bel_in_a_dcs_body_is_payload_and_only_st_ends_it`).
* `input-caps-output.txt` is a re-run of the repo's own
  `tests/conformance/input_caps.rs`.
* `d1-test-time.rs` is the exhaustive D1 test section 8.3 proposes, built
  unoptimised as a test is; its output, with the run times, is
  `d1-test-time-output.txt`.

Tags:

* **M**: measured by those harnesses. This covers a run of baud's code (the
  key probe, the length probe, the straddle probe, the controls,
  `input_caps.rs`, the D1 test) and a count over the pinned and captured files
  (`bounds.py`'s table sizes, `xterm-table-count.py`).
* **C**: computed. This covers what is read from the code, and arithmetic over
  an assumed reply shape. Every XTGETTCAP reply size and per-read bound below
  is **C**: baud answers no XTGETTCAP, so none can be measured.
* **R**: read at source, cited by file and line.

## 1. What happens today

`DCS + q Pt ST` reaches `Node::Dcs` (`baud-core/src/graph.rs` 821-860). There,
`+` (0x2B) takes the catch-all `_ => edge(Act::Consume, Node::Str)` (858), so
the intermediate is consumed and not stored. The body accumulates in
`strs.pending` up to `OSC_MAX` = 512 bytes (`storage.rs` 611-616), and the
`q` is part of it, so 511 bytes of hex fit after the `q` and 512 are cut by
one (**M**, Q3). At the terminator, `str_end` (`osc.rs` 75-96) finds
`StrKind::Other` and counts `strings_ignored` and `unsupported_total`. No
reply is sent (**C**).

That counting is what lets `every_spec_dcs_form_is_handled_or_counted`
(`tests/conformance/spec_inventory.rs` 637-690) pass for `DCS + q`, `DCS + p`
and `DCS + Q`. `docs/vt-coverage.md` has **no row** for any of the three.

**The body is kept across reads (M).** A 509-byte `DCS + q` body fed in one
read, with its terminator alone in the next, is ended in the second read:
`strings_ignored` stays 0 after the first and is 1 after the second, in both
C1 forms (`straddle-output.txt`, B1 and B2). Section 8.3 rests on this.

## 2. What the texts say

### 2.1 ctlseqs

[[source: ctlseqs]] gives XTGETTCAP at lines 545-582:

> DCS + q Pt ST
> Request Termcap/Terminfo String (XTGETTCAP), xterm. The string following
> the "q" is a list of names encoded in hexadecimal (2 digits per character)
> separated by ; which correspond to termcap or terminfo capability names for
> special keyboard keys. A terminal description will include other
> capabilities, e.g., for cursor movement, which are intentionally not part of
> this interface.
> A few more terminal capabilities are recognized, which are not names of
> special keys:
> o Co for termcap colors (or colors for terminfo colors), and
> o TN for termcap name (or name for terminfo name).
> o RGB for the ncurses direct-color extension. Only a terminfo name is
> provided, since termcap applications cannot use this information.
> These capabilities fall into two categories:
> o Terminal capabilities which may be dynamically adjusted in xterm so they
> do not necessarily match a terminal description.
> o The name of the terminal description, which an application can use to
> obtain the static set of capabilities.
> xterm responds with
> DCS 1 + r Pt ST for valid requests, adding to Pt an = , and the value of
> the corresponding string that xterm would send, or
> DCS 0 + r ST for invalid requests.
> The strings are encoded in hexadecimal (2 digits per character). If more
> than one name is given, xterm replies with each name/value pair in the same
> response. An invalid name (one not found in xterm's tables) ends processing
> of the list of names.

XTSETTCAP is at lines 535-542:

> DCS + p Pt ST
> Set Termcap/Terminfo Data (XTSETTCAP), xterm. The string following the "p"
> is encoded in hexadecimal. After decoding it, xterm will use the name to
> retrieve data from the terminal database. If successful, that overrides the
> termName resource when handling the "tcap" keyboard configuration's
> function- and special-keys, as well as by the Request Termcap/Terminfo
> String control.

Here is what the text settles.

**Scope.** The interface covers keyboard key names, plus `Co`/`colors`,
`TN`/`name` and `RGB`. Every other capability is "intentionally not part of
this interface".

**Valid.** The text defines only the invalid name, "one not found in xterm's
tables" (580-581). A name found there is valid.

**The value.** For a key, the value is "the corresponding string that xterm
would send". It is the key's live sequence, which "do[es] not necessarily
match a terminal description". It is not the database's string.

**The shape.**

* The text gives one response for the whole list: "each name/value pair in
  the same response".
* It gives `;` only between the names of the request ("a list of names ...
  separated by ;"). The `;` between the reply's pairs is xterm's code
  ([[source: xterm-misc]] 5240-5241, section 3), not the text.
* The first invalid name ends the list.
* An invalid request is answered `DCS 0 + r ST`, with no `Pt`.
* Names and values are hexadecimal.

**What the text does not settle.**

* What separates the pairs in the reply.
* What a boolean capability answers. The valid form always carries `=` and a
  value.
* Whether a list cut short by an invalid name keeps the `;` before it.
* The letter case of the hex digits.
* What a terminal other than xterm answers for a key in xterm's table that it
  does not have, or has and sends nothing for.

**Two readings of a key name, used to mark the options below.** A valid
answer rests on two clauses: validity, "found in xterm's tables", and the
value, "the corresponding string that xterm would send". They come apart for
a name in the table whose key sends nothing.

* **The literal reading** takes both clauses as written. Every name in
  xterm's table is valid, and its value is the string the key sends: empty
  when it sends none, which is xterm's own answer for Undo and Clear
  (section 3). For a terminal that is not xterm, the value clause can be read
  two ways, and the marks below give both where they differ:
  * as the string the answering terminal's own key sends, which is how this
    note reads it for every key baud has ("the key's live sequence", above);
  * as xterm's own bytes for that key, which baud does not send for a key it
    lacks.
* **The sending reading** answers a name valid only when baud's key sends a
  string for it, and refuses it otherwise. It reads "the corresponding string
  that xterm would send" as a condition of validity. The text's definition
  of an invalid name does not include that condition.

### 2.2 terminfo, and the entry baud names

baud starts programs with `TERM=xterm-256color` and `COLORTERM=truecolor`
(`baud-app/src/pty.rs` 375-380). The pinned entry,
[[source: terminfo-xterm-256color]], was captured with `infocmp -1`. That
command omits user-defined capabilities.
[[source: terminfo-man5]] 1658-1667: "The `tic` and `infocmp` programs
provide the `-x` option for this purpose".

On this Mac, `infocmp -x -1 xterm-256color` reads the same
`/usr/share/terminfo/78/xterm-256color` from the same ncurses 6.0.20150808. It
shows **61 capabilities the pin does not (M)**:

* `AX`, `XT`, `OTbs`;
* `Cr`, `Cs`, `E3`, `Ms`, `Se`, `Ss`;
* fifty-two modified-key strings, `kDC3` through `kUP7`.

So the entry programs in baud actually read **already advertises
`Ms=\E]52;%p1%s;%p2%s\007`**, as well as `E3`, `Ss`/`Se` and `Cr`/`Cs`. The
capture is [[source: terminfo-xterm-256color-x]].

The names applications ask for beyond the key names are ncurses extensions.
[[source: ncurses-user-caps-5]] 347-420 defines `RGB` (Boolean, numeric or
string). It "asserts that the `set_a_foreground` (`setaf`) and
`set_a_background` (`setab`) capabilities employ direct colors", so as a
terminfo capability it describes an entry like `xterm-direct`, not
`xterm-256color`. Other names have other origins:

* `Smulx`, `Setulc`, `Su` and `Tc` are terminal-project extensions.
  [[source: ncurses-terminfo-src]] 3771-3774 records `Setulc` as a copy of
  kitty's `setal`, under a non-ncurses name. At 8868-8869 it records a
  terminal's developers setting `Tc` "to hint that it supports
  direct-colors".
* `Ms` and `Ss`/`Se` come from the `xterm+tmux` building block (5801-5806).

## 3. xterm's practice

[[source: xterm-misc]] 5162-5249 (`do_dcs`, case `+`), with
[[source: xterm-xtermcap-c]] and [[source: xterm-input-c]]. All are at
`9489b205`. misc.c and charproc.c hash as MANIFEST records them. xtermcap.c,
input.c and xstrings.c were fetched at the same commit and hashed for group
AM (**M**). input.c tests keys with Xlib's key classes (`IsCursorKey`,
`IsKeypadKey`, `IsPFKey`, `IsFunctionKey`, `IsMiscFunctionKey`), which no
pinned xterm file defines ([[source: xlib-xutil-h]] 239-255), over the keysym
values of [[source: xorg-keysymdef]].

* **The table** (xtermcap.c 70-244) has 110 `DATA` rows and 17 modifier
  groups. It answers **319 distinct names: 221 terminfo and 98 termcap**, the
  longest six characters (**M**, `xterm-table-count.py`).
  * That count assumes an xterm built with every conditional row compiled in:
    `USE_EXTENDED_NAMES` (41-44, 190), `XK_ISO_Left_Tab` (166-168), and
    `OPT_ISO_COLORS` with `OPT_DIRECT_COLOR` (180-188). The script expands
    them all.
  * Everything in it is a key, apart from three rows: `Co`/`colors`,
    `Co`/`RGB` and `TN`/`name`.
* **A name has one of three replies, not two.**
  * `lookupTcapByName` returns -2 for a name not in the table (347-374).
    In `do_dcs`, -2 skips the loop, so the reply is exactly `DCS 0 + r ST`.
  * `xtermcapKeycode` returns -1 for a name that is in the table in two
    cases. -1 enters the loop, echoes the name and breaks, so the reply is
    `DCS 0 + r <hex name> ST` (misc.c 5183-5193, 5235-5237).
    * `keyIsDistinct` refuses the row (xtermcap.c 406, 439-442). Both of its
      tests are for Shift alone, `param == SHIFT` (331, 296; `SHIFT` is
      `MOD_NONE + MOD_SHIFT`, 60), so a row that adds Alt or Control to
      Shift, such as `kDN4`, `kDN6` or `kDN8` (196-203, 210), is not refused
      this way. On the VT220 keyboard it refuses every row whose modifier is
      Shift alone, because that keyboard "will not return distinct key
      sequences for shifted cursor-keys" (325-326, 330-334). Under the tcap
      keyboard it refuses such a row when its loaded string is missing or
      equals the unshifted row's (294-321).
    * On the Sun keyboard, a function-key row numbered past 20 is set to -1
      unless its number is 31-47, and so are five of those, 37, 39, 41, 43
      and 45 (416-437).
  * **A name in the table whose key sends nothing is answered valid, with an
    empty value:** `DCS 1 + r <hex name>= ST`. Its code is not negative, so
    misc.c writes the `=` (5195-5196) before it calls `Input()` (5232), and
    `Input()` then writes nothing.
    * The empty value is not an answer designed for a key that sends nothing.
      The query's simulated press carries no bytes (`lookupKeyData`, input.c
      927-932; a Backspace press alone carries one, 933-935), so the value is
      empty whenever `Input()` encodes nothing for the keysym, as for the two
      rows below.
    * The same mechanism answers empty for keys that do send bytes, in two
      configurations. Under `?1037` ([[source: ctlseqs]] 1002),
      `xtermDeleteIsDEL` is true (input.c 893-895), so the simulated Delete
      takes no branch (1318-1326, 1413) and `kdch1` answers empty, while the
      real key sends DEL. Under KAM (ctlseqs 912), `Input()` returns at once
      (input.c 1015-1016), so every key name answers empty.
    * baud has neither mode: SM sets only LNM and IRM (`docs/vt-coverage.md`,
      `CSI h`), and `?1037` has no row. So "xterm's answer for a key that
      sends nothing", below, means its answer in its default configuration.

    Two of xterm's own rows are such names on its default keyboard:
    * `kund`, Undo (xtermcap.c 178). Undo is one of Xlib's miscellaneous
      function keys (`IsMiscFunctionKey`, xlib-xutil-h 254-255; Undo is
      0xFF65, between Select at 0xFF60 and Break at 0xFF6B, keysymdef 263,
      267, 273), so it takes the branch at input.c 1318-1322
      (`ModifyOtherKeys`, which could divert it, refuses such a key at
      655-656).
      `decfuncvalue` has no case for it and returns -1 (1656), and the guard
      at 1379 sends nothing for a negative code.
    * `kclr`, Clear (169). A query's simulated press carries no bytes
      (`lookupKeyData`, input.c 927-932). Clear (0xFF0B, keysymdef 203) is in
      none of the classes the branches at 1318, 1383, 1390 and 1401 test, and
      with no bytes the branch at 1413 is not taken either, so nothing is
      sent.
* **A key's value is a simulated keypress.** misc.c 5227-5232 builds an
  `XKeyEvent` and calls `Input()`. While `tc_query_code` is set, `unparseputc`
  writes each byte as two upper-case hex digits (charproc.c 9467-9479,
  [[source: xterm-reset]]'s file). So the answer follows DECCKM, the keypad
  mode and the modifier resources.
  * Because of this, xterm's own answer for `kf13` is `CSI 25 ~`, the VT220
    F13 (input.c 1605-1650, `decfuncvalue`): its table maps `kf13` to the F13
    keysym.
  * xterm's own `xterm-256color` entry says `kf13=\E[1;2P`, ncurses'
    convention that `kf13` is shift-F1.
  * Past F20 the codes are xterm's own. `decfuncvalue` numbers F21 and up
    from `CSI 42 ~`, because "after F20 the codes are made up and do not
    correspond to any real terminal" (input.c 1631-1635). `xtermcapKeycode`
    turns the table's codes -36 ... -63 into `XK_Fn(36)` ... `XK_Fn(63)`
    (xtermcap.c 412-415). So by the path `kf13` takes, xterm answers
    `kf21`-`kf63` with `CSI 42 ~` ... `CSI 84 ~`.
  * The keypad's corner names are answered as the navigation keys. The table
    gives `ka1`, `kc1`, `ka3` and `kc3` the keysyms `XK_KP_Home`,
    `XK_KP_End`, `XK_KP_Prior` and `XK_KP_Next` (xtermcap.c 161-164). Before
    encoding, on every keyboard but VT220, input.c 1186-1188 moves
    `XK_KP_Home` ... `XK_KP_Begin` onto `XK_Home` ... `XK_Begin`, two runs
    in the same order (keysymdef 248-258, 290-300). So xterm answers `ka1` as
    Home and `kc1` as End (`IsCursorKey`, input.c 1401), and `ka3` and `kc3`
    as `CSI 5 ~` and `CSI 6 ~` (`IsEditKeypad` 212-235, `decfuncvalue`
    1651-1652).
* **The fixed names.**
  * `Co` answers `NUM_ANSI_COLORS` (5199-5204).
  * `RGB` answers the bits per channel when direct colour is on, `8` or
    `r/g/b`, and otherwise `-1` (5205-5222).
  * `TN` answers the `termName` resource (5225-5226).
* **A list.** The `1` or `0` after `DCS` is decided by the first name alone
  (misc.c 5179-5183). After a valid pair, xterm writes the `;` and looks up
  the next name (5239-5244). What follows depends on that name:
  * one not in the table (-2) ends the loop at once (5188), so the response
    ends in a `;` before the `ST`;
  * one that returns -1 is echoed after the `;` with no `=`, and the loop
    breaks (5189-5193, 5235-5237): `DCS 1 + r n1=v1;<hex n2> ST`;
  * one whose key sends nothing is answered `n2=`, and the list goes on.
  This is xterm's code; the text does not say it.
* **Decoding.** `x_decode_hex` accepts either case and refuses an odd number
  of digits ([[source: xterm-xstrings-c]] 96-123). A name must end at the
  body's end or at `;`, or it is -2 (xtermcap.c 402).
* **XTSETTCAP** (xtermcap.c 624-657) calls `setupterm` on the host's database.
  It rings the bell on failure and frees the cached termcap function keys.
  Those strings are read only under the tcap keyboard (`keyboardIsTermcap`,
  `?1050`).
* **On XTSETTCAP, the text and the code differ.**
  * ctlseqs 539-542 says the overridden name is used for the tcap keyboard "as
    well as by the Request Termcap/Terminfo String control", which is
    XTGETTCAP.
  * In xterm's code, XTSETTCAP reaches XTGETTCAP only under the tcap keyboard.
    There `keyIsDistinct` reads the loaded strings (xtermcap.c 289-345), and
    the simulated keypress takes `xtermcapString` (input.c 1289-1293), so the
    answered key values change too.
  * Under any other keyboard nothing changes. `TN` answers
    `resource.term_name` under every keyboard (misc.c 5225-5226).
  * So [[source: ctlseqs]] says one thing and [[source: xterm-xtermcap-c]]
    does another. baud has no tcap keyboard, so under xterm's code XTSETTCAP
    would change no answer of baud's.

## 4. Who asks, under baud's environment

| Asker, at source | What it sends | When | Under baud |
|---|---|---|---|
| neovim `fc3f004`, `runtime/plugin/osc52.lua` 35-92 ([[source: neovim-plugin-osc52-lua]]) | `XTGETTCAP Ms`, one name | at every `UIEnter`, after DA1, if DA1 lacks `52` and `TERM_PROGRAM` is not `Apple_Terminal` | **sent**: baud's DA1 is `CSI ? 6 c` or `CSI ? 6 ; 4 c` (`baud-domain/src/vt/report.rs` 454-466) |
| neovim `fc3f004`, `runtime/lua/vim/_core/defaults.lua` 1004-1027 ([[source: neovim-core-defaults-lua]]) | `Tc;RGB;setrgbf;setrgbb` in one DCS | only if `$COLORTERM` is unset or empty and the TUI found no truecolor in terminfo | **not sent locally**: baud exports `COLORTERM=truecolor`. Sent where a remote shell does not receive `COLORTERM` |
| neovim TUI, `src/nvim/tui/tui.c` 2544-2551 and `tui/input.c` 583-586 ([[source: neovim-tui-c]], [[source: neovim-tui-input-c]]) | nothing by XTGETTCAP | -- | reads `Smulx`/`Su` from local terminfo. It turns styled underlines on from baud's DECRQSS answer `1$r0;4:3m` |
| vim `b2b47bd`, `src/term.c` 1805-1821, 5222-5230, 7645-7670 ([[source: vim-term-c]]) | 37 termcap names: `Co`, 34 key codes and `PS`/`PE` (`RGB` is commented out), **one per DCS**, hex in lower case (`%02x`) | only after DA2 reports xterm version >= 141 | **never**: baud's DA2 is answered with silence (`docs/vt-coverage.md`, `CSI > c`) |
| tmux `94796f6` and `95c1c3d`, every `.c`/`.h`/`.y` file (210 at each) | **nothing**. No `P+q`, `P1+r`, `P0+r`, `xtgettcap` or `tcap` in the tree. Positive controls: the same pattern, under this shell's grep, hits vim's and neovim's senders (term.c 7663, 7665; tty.lua 113), and `\033P` matches six lines of tmux (control.c, image-sixel.c, input.c, tty-keys.c) | -- | tmux learns outer-terminal features from DA and the XTVERSION name ([[source: tmux-tty-keys-c]] 1723-1735). As a pane host, `input_dcs_dispatch` ([[source: tmux-input-c]] 2618-2680) handles Sixel, DECRQSS and passthrough only, so a pane's XTGETTCAP is dropped (`tmux-grep.log`) |

**Locally, the only XTGETTCAP baud receives from these programs is neovim's
`Ms`.** neovim parses the reply in `vim.tty.query`
([[source: neovim-tty-lua]] 88-154):

* It sends every name in one DCS (113).
* It matches only `^\027P1%+r(%x+)(.*)$` (129). A `DCS 0 + r` reply of either
  shape is ignored, and the name resolves as "not found" when the 1000 ms
  timer fires (115-127).
* It decodes a value only when the rest of the reply is `=<hex>` (141-146). It
  then turns `\E` into ESC and strips `%p<n>`, so it accepts both the raw bytes
  and terminfo source text. Any other rest, a bare `=` and no rest at all
  among them, is reported as found with no value (149). wezterm's boolean
  `=31` does match, and is decoded as the value `1`.
* It reads only the first name of a multi-pair response, and not that
  name's value. `%x+` stops at the first `=`, so the rest is `=v1;n2=v2`,
  which fails `^=%x+$`, and the first name is reported as found with no value
  even when its pair carries one (129, 141, 149). In `Tc;RGB;setrgbf;setrgbb`
  the first name is `Tc`, and finding it alone turns truecolor on
  (defaults.lua 1023-1025). A name answered in a DCS of its own is read as its
  own response.

**What answering `Ms` sets off.** A reply matching `^\027]52` sets
`g:termfeatures.osc52` (osc52.lua 77-83). The clipboard provider takes OSC 52
last, and only when all of these hold
([[source: neovim-provider-clipboard-vim]] 186-263):

* `g:clipboard` is unset, or set to `v:false` (186). Setting it to `v:false`
  "explicitly opts-in to using the "builtin" clipboard providers below"
  (185);
* no clipboard tool is found;
* it is not the case that `$TMUX` is set and `tmux` is executable (258-259);
* `termfeatures.osc52` is set (260);
* the `'clipboard'` option is empty (260).

neovim's documentation says the same: OSC 52 is used "if no other
|clipboard-tool| is found and when 'clipboard' is unset"
([[source: neovim-runtime-doc-provider-txt]] 276-278). On macOS it never gets
that far, because `has('mac')` selects `pbcopy` first (clipboard.vim 236). On
a remote host with no clipboard tool and no tmux, it gets that far:

* **Only the `+` and `*` registers reach it.** A clipboard tool "implicitly
  enables the "+" and "*" registers", and only `'clipboard'` set to
  `unnamedplus` sends every operation through the clipboard (provider.txt
  173-175, 180-181). With `'clipboard'` empty, which the automatic choice of
  OSC 52 requires, a plain `y` or `p` uses the unnamed register and never
  asks the terminal.
* **Copying** to `+` or `*` sends OSC 52, which baud drops today
  (`clipboard_writes_dropped`). The owner's V11 is to honour writes, on by
  default; it is not built.
* **Pasting** from `+` or `*`, such as `"+p`, sends `OSC 52 ; c ; ?` and waits
  1 s. It then prints "Waiting for OSC 52 response from the terminal. Press
  Ctrl-C to interrupt..." and waits up to 9 s more before it times out
  ([[source: neovim-ui-clipboard-osc52-lua]] 38-67). Ctrl-C ends the wait.
  baud refuses reads with silence, and that refusal stands either way.
* **A user can choose the provider whatever baud answers.** The file's own
  comment says "Users can opt-in to it by setting g:clipboard manually"
  (clipboard.vim 261-262), and `vim.g.clipboard = 'osc52'` forces it
  (provider.txt 289-291).

## 5. How four terminals answer

| | kitty `9710eb4` | foot 1.21.0 | wezterm `b09b56c` | ghostty `d9840f3` |
|---|---|---|---|---|
| Names | its whole terminfo, bools, numbers and strings, plus `TN`/`name` and two private query families ([[source: kitty-9710eb4-terminfo-py]] 29-74, 529-576; each result its own DCS, [[source: kitty-9710eb4-window-py]] 1755-1757; parsed at [[source: kitty-9710eb4-vt-parser-c]] 728-740) | its whole `foot.info`, plus `Co`, `TN`, `name` and `RGB=8`, made at **build time** into one sorted string table ([[source: foot-generate-builtin-terminfo-py]] 177-208, [[source: foot-meson-build]] 199-207, from [[source: foot-info]]) | `TN`, `Co`, `RGB`, then its compiled terminfo ([[source: wezterm-b09b56c-terminfo]]), parsed at run time ([[source: wezterm-b09b56c-terminalstate-mod]] 40-41, 1307-1370; names split at [[source: wezterm-b09b56c-escape-parser]] 24-49) | its whole terminfo plus `TN`, `Co`, `RGB=8`, as a **comptime** map from hex name to the whole prebuilt reply ([[source: ghostty-d9840f3-terminfo-source]] 75-155, from [[source: ghostty-d9840f3-terminfo-ghostty]]; names upper-cased at [[source: ghostty-d9840f3-dcs]] 173-179) |
| Lookup | Python dict | linear walk of the sorted table, with early exit ([[source: foot-dcs-c]] 84-109) | dictionary | `StaticStringMap`, no run-time work beyond the lookup ([[source: ghostty-d9840f3-stream-handler]] 475-481) |
| `TN` | `Options.term`, `xterm-kitty` by default | `foot`, the build's target name | the program name, `WezTerm` | `xterm-ghostty`, the first of its names |
| A key's value | the terminfo string with `\E` decoded, not the live key | the `foot.info` string, decoded | the compiled terminfo string | the terminfo string, decoded |
| A `%` string | terminfo **source** text, with a literal `\E` | decoded `\E`, `%` left | real bytes, `%` left | source text, as kitty does ([[source: ghostty-d9840f3-terminfo-source]] 93-102) |
| Boolean | `1+r<hex>`, no `=` | `1+r<hex>`, no `=` | `1+r<hex>=31` (`"1"`) | `1+r<hex>`, no `=` |
| Reply form | **one DCS per name** | one DCS per name | one DCS per name | one DCS per name |
| Unknown name | `DCS 0 + r <hex> ST`, and the list goes on | `DCS 0 + r <hex> ST`, and the list goes on ([[source: foot-dcs-c]] 169-172); an empty body gets `DCS 0 + r ST` | `DCS 0 + r <hex> ST`, and the list goes on | **silence** (`orelse continue`) |
| Conforms? | Departs on scope, the value's source, the reply form and the unknown reply | the same four | the same four, and `TN` names a program, not a description | the same, and silence where the text gives a reply |

None of the four answers a key's live sequence. Each answers its own terminfo
text. They can do this because each ships its own entry and sets `TERM` to it.
foot and ghostty make the table at build time, which is the idiom baud's tables
follow.

## 6. baud: what is in place

* **The prefix.** DECRQSS added `Node::DcsDollar` and `Act::DcsQuery`.
  XTGETTCAP needs the same shape for `+` then `q` (or `+` stored into the
  body).
  * graph.rs records what DECRQSS's prefix did to the pinned stride ladder:
    stride 1 went from 26 to 29, and stride 4 grew 8.8% (the ladder's row at
    1359, and the paragraph at 1464-1471). It moved table content and left the
    per-byte loop unchanged.
  * What XTGETTCAP's prefix would move is **unmeasured**. The pins catch it on
    the first run.
* **The reply.** DECRQSS is built whole on the stack, up to `DECRQSS_MAX` =
  124 B, and is copied under one check or dropped and counted (`dcs.rs` 56-96,
  120-193). `REPLY` is 256 B (`storage.rs` 644-650). Note AG 1.5 says that
  under its option A "a future reply whose length depends on stored state
  (XTGETTCAP, ...) must join the budget test or the bound breaks silently".
* **Keys.** `encode_key(KeyEvent, KeyState)` (`baud-domain/src/vt/key.rs`
  373-379) writes into a buffer of `KeyBytes::MAX_LEN` = 48 bytes.
  * The doc comment derives the CSI-u maximum, `CSI 57383 ; 128 : 3 u`, at
    15 B. It says the alternate-key and associated-text sub-fields "will grow
    it again and this bound is re-derived when they land" (275-285).
  * They have landed: `kitty_encode` emits both (461-476). The comment was
    not re-derived. Its own case is 14 B, and the encoder's measured maximum
    is 40 B (section 7.1). So no bound in this note rests on the comment.
    `bc16520` replaced it with the measured 40 B; the line numbers here are `4911bf0`'s.
  * Backspace sends `0x7f`. The comment says which byte it sends is DECBKM's
    (`?67`) choice, which baud does not implement, and that BS is Ctrl-H
    (712-720).
  * `FnKey` is F1-F20 (`baud-domain/src/input.rs` 136-147).
  * F21-F35 are `KeyCode::Pua(PuaKey::F21)` ... `PuaKey::F35` (input.rs
    470-491), kitty numbers 57384-57398 (531-545). "The PUA keys have no legacy
    form; the base layer sends nothing" (key.rs 693-694).
* **The table idiom.** The project builds a compiled database with an
  index-only runtime, where nothing scans. The precedent for this entry is
  `baud_domain::terminfo`. It reads the pinned `xterm-256color.ti` at compile
  time with a `const fn` decoder of terminfo(5)'s escapes
  (`baud-domain/src/terminfo.rs` 284-405, `baud-app/src/bell.rs` 77-91).

## 7. The key probe (M)

`keyprobe-main.rs` names 105 of xterm's terminfo key names that baud's
`KeyCode` can express. It encodes each through baud's real `encode_key` in
two states: reset, and after `smkx` (DECCKM set and keypad application). It
compares each result with the entry. Then, for each entry value that differs,
it encodes the key ncurses' convention names.

* **86 of 95** comparable names agree with the entry in the `smkx` state.
  `kDC` and `kIC` (shifted Delete and Insert) are among them.
* **10 are in neither entry**: `kDN8` ... `kPRV8`, which are
  shift+alt+control.
* **9 differ. Each of the 9 values is a sequence baud sends, for another
  key. None is what baud's key of the same name sends:**
  * `kf13`-`kf20`. terminfo(5) defines `kf13` as "F13 function key"
    ([[source: terminfo-man5]] 809-810), and so on to `kf20`. baud's F13-F20
    send the VT220 `CSI 25 ~` ... `CSI 34 ~`, which is what xterm's own
    XTGETTCAP table answers for these names (section 3). The entry gives
    shift-F1 ... shift-F8 instead (`\E[1;2P` ... `\E[19;2~`), and baud's
    shift-F1 ... shift-F8 send exactly those bytes. So terminfo(5)'s
    definition and xterm's table name the key F13; the entry's value is
    ncurses' convention that `kf13` is shift-F1.
  * `kbs`. The entry says `^H`, which baud sends for Ctrl-H. baud's Backspace
    key sends `^?` (section 6).
* **7 names depend on mode**: `khome`, `kend`, the four arrows and `kent`
  change with DECCKM or DECKPAM. The entry gives their `smkx` forms. A program
  sends the entry's `smkx` before it reads keys, and in that state all seven
  agree.
* The longest sequence encoded is **6 bytes**, against the 48-byte bound.
  This is measured on the base layer: kitty flags off, and `modifyOtherKeys`
  at its default.

**The repo already measures this by value.**
`baud-core/tests/conformance/input_caps.rs` enumerates every byte string
baud's key encoder produces, over every modifier and every mode, and counts
the entry's input capabilities among them. Re-run at `4911bf0` it gives
**xterm-256color: 85 of 88** (M, `input-caps-output.txt`). It misses three
(*since 2026-10-18 the pin is the `-x` capture: 137 of 140*):

* `kb2` (`\EOE`, the keypad centre key), which baud has no code for;
* `kmous` (`\E[M`), the mouse-report prefix, which is not a key;
* `kbs` (`^H`). The enumeration has no text keys, so it never encodes
  Ctrl-H; the key probe does.

Not probed against the entry:

* `kb2`, the one keypad-corner name the entry carries. baud has no code for
  it, and xterm's table does not list it;
* `ka1`, `kc1`, `ka3` and `kc3`, which neither entry carries. xterm's table
  lists them, and xterm answers them as Home, End, PageUp and PageDown
  (section 3). baud's Home, End, PageUp and PageDown keys send all four, and
  section 7.1 encodes them;
* xterm's 58 `kp*` names, which the entry lacks; section 7.1 encodes all 58;
* keys in xterm's table that baud has no code for: Help, Find, Select, Undo,
  Clear, and F36-F63;
* F21-F35, which baud has and the base layer sends nothing for (below);
* termcap aliases.

So the entry's key values are ncurses' convention, and baud sends every one
probed for some key. Against the key xterm's table and terminfo(5) name, they
differ in two places: `kbs`, and `kf13`-`kf20`. A table built from the entry
would also carry what the text excludes: output strings, `u8` (`\E[?1;2c`
against baud's `CSI ? 6 c`), and the static `cols#80`/`lines#24`.

**F21-F35 send something only under kitty's flags (M).** On the base layer
`encode_key` returns `None` for all fifteen. Under each of the 31 nonzero
kitty flags values, with the other fields at their defaults, they send
`CSI 57384 u` through `CSI 57398 u`. F21 does so in all 95,232 key states that
have a flag set, with `ESC [` or `9B` as the introducer, and sends nothing in
all 3,072 that have none (`keymax-output.txt`).
Through `Term::key`, F21 sends nothing until an application pushes
`CSI > 1 u`, and `ESC [57384u` after it (Q2). baud answers kitty's `CSI ? u`
(Q1, `baud-core/src/write.rs` 1668-1677), so a conforming application can turn
the flags on. xterm sends F21 and up as `CSI 42 ~` and on (section 3), which
baud never sends.

### 7.1 How long one key value can be (M)

`keymax-main.rs` runs baud's real `encode_key` at `4911bf0` over four
domains. Each is enumerated exhaustively, and the program's positive controls
pass (`keymax-output.txt`).

"Every key state" means all 98,304 `KeyState`s. That is the five mode bits
the encoder reads (DECCKM, LNM, `?1034`, `?1036`, `?1039`), both keypad
modes, the three `modifyOtherKeys` levels and both formats, both C1 forms,
ANSI and VT52, both encodings, and kitty flags 0-31.

| Domain | Longest | At |
|---|---|---|
| **D0**, `encode_key`'s global maximum. Every `KeyCode`, every modifier byte, press, repeat and release, and text, shifted and base-layout keys absent or present. Every base-layer state, and kitty flags 1-31 in both C1 forms | **40 B** | `ESC [1114109:1114111:1114111;100:2;1114109u` |
| **D3**, the table's keys under any of the 256 modifier bytes. Presses, with no text and no alternates, in every key state | **12 B** | `ESC [27;10;127~`, Backspace with shift and meta under `modifyOtherKeys` level 1. Also `ESC [57376;100u` under kitty's flags |
| **D2**, the table's keys under xterm's eight shift/alt/ctrl states. Presses, with no text and no alternates, in every key state | **11 B** | `ESC [27;3;127~`, Alt-Backspace under `modifyOtherKeys` level 1 |
| the same, with only the kitty flags and C1 form varied and every other field at its default | 10 B | `ESC [57414;2u` |
| **D1**, xterm's table rows exactly: each of the 190 rows a baud key can serve, with the row's own modifiers. Presses, with no text and no alternates, in every key state | **10 B** | `ESC [57413;2u`, `kpADD` (shifted keypad `+`) under kitty's flags |
| D1 on the base layer alone (kitty flags 0) | 9 B | `ESC [27;2;9~`, `kcbt` (Shift-Tab) under `modifyOtherKeys` level 2 |

* **The 40 B is the arithmetic ceiling of `kitty_csi_u`**, not a sample's
  luck. It is `ESC [` (2), a seven-digit key number, `:` and a seven-digit
  shifted key, `:` and a seven-digit base-layout key, `;` and a three-digit
  modifier (`1 + 255`), `:` and an event digit, `;` and seven digits of text,
  then `u`: 2+7+1+7+1+7+1+3+1+1+1+7+1 = 40.
  * No codepoint has more than seven digits, and the modifier field cannot
    pass 256. The base layer's longest is 14 B (D0 in
    `keymax-output.txt`).
  * It needs what a simulated press never carries: associated text
    (flag 16), both alternate keys (flag 4), a repeat with event types
    (flag 2) -- a release carries no text since 2026-10-10 -- and a
    three-digit modifier.
  * D0 samples text keys at every decimal length, 1 to 7 digits; U+0000,
    U+0007 and U+0009 are the one-digit samples. Its kitty half holds the
    other fields at their defaults. A kitty-flagged key that falls through to
    the base layer is still covered by the base-layer half, because
    `encode_base` never reads the flags (key.rs 374 and 421 are the only
    reads).
* **D1 is the domain XTGETTCAP simulates.** xterm builds one `XKeyEvent` per
  name, from the row's key and its modifier state (misc.c 5228-5232,
  xtermcap.c 405-409).
  * The 190 rows exclude the 36 whose key baud has no code for (section 7,
    above): Help, Find and Select, each plain and shifted, Clear, Undo, and
    F36-F63.
  * They serve `ka1`, `kc1`, `ka3` and `kc3` as Home, End, PageUp and
    PageDown, as xterm does (section 3). Those four alone reach at most 4 B,
    `ESC [5~`.
  * They serve `kcbt` as Tab with shift. xterm's row names
    `XK_ISO_Left_Tab` (xtermcap.c 167), which baud has no code for, and
    xterm's own comment says "Shift-tab is often mapped to XK_ISO_Left_Tab"
    (input.c 1146-1147).
  * D2 and D3 are wider, in case a table should ever add modifiers.
* **`modifyOtherKeys` is what takes D2 past D1.** With every field but the
  kitty flags and the C1 form at its default, D2 is 10 B (D2k in
  `keymax-output.txt`). Over every key state, Alt-Backspace under level 1
  makes it 11 B.

## 8. The options

Each option carries a conformance mark, conforms or departs, against the
text and the reading it names (section 2.1), with a mark on each reading
where they differ, and a cost. A description such as "what the four
terminals do" is practice, not a mark.

### 8.1 Which names

**N1: xterm's table (conforms).** The table holds 319 names. A build-time
table maps each name to one of two things:

* the `(KeyCode, Modifiers)` it simulates, for a key;
* a fixed value: `Co`/`colors` = `256`, `RGB` = `8`, and `TN`/`name` = the
  `TERM` the embedder exports.

A key's value comes from `encode_key` under the live `KeyState`, as xterm
calls `Input()`. `ka1`, `kc1`, `ka3` and `kc3` map to Home, End, PageUp and
PageDown, and `kcbt` to Tab with shift (section 7.1).

**Names for keys baud has no code for.** xterm's table lists 36 rows whose key
baud cannot express: Help, Find and Select, plain and shifted (xtermcap.c
72-77), Clear (169), Undo (178), and F36-F63 (131-158). They split in two:

* For Undo and Clear, xterm's own simulated press sends nothing, and xterm
  answers `DCS 1 + r <hex>= ST` (section 3).
* For the rest, xterm sends bytes: unshifted, `CSI 28 ~`, `CSI 1 ~` and
  `CSI 4 ~` for Help, Find and Select (input.c 1640-1653), and `CSI 57 ~` ...
  `CSI 84 ~` for F36-F63 (section 3). baud sends none of them.

Four ways to answer them, marked on each reading (section 2.1):

| Option | Reply | Sending reading | Literal: validity | Literal: value, as baud's key sends | Literal: value, as xterm's bytes |
|---|---|---|---|---|---|
| (a) refuse, the text's invalid reply | `DCS 0 + r ST` | conforms | departs | -- | -- |
| (b) refuse, with the name echoed | `DCS 0 + r <hex> ST` | departs: the text's invalid reply has no `Pt` | departs | -- | -- |
| (c) valid, with an empty value | `DCS 1 + r <hex>= ST` | departs | conforms | conforms | conforms for Undo and Clear; departs for the rest |
| (d) valid, with xterm's bytes | `DCS 1 + r <hex>=<hex bytes> ST` | departs | conforms | departs, except for Undo and Clear, where it is (c) | conforms |

Practice and cost:

* (a) Cost: none beyond the refusal every invalid name takes. Under R1 the
  name ends the list.
* (b) is xterm's -1 reply. xterm gives it a row `keyIsDistinct` refuses, such
  as a cursor key with Shift alone on the VT220 keyboard, or a function key
  the Sun keyboard renumbers away (section 3), not a key that sends nothing;
  for that, xterm in its default configuration answers (c). Cost: as (a),
  plus the name's hex in the reply. Under R1 it ends the list.
* (c) is xterm's own answer for Undo and Clear in its default configuration,
  which comes from a simulated press that carries no bytes (section 3).
  Validity stays fixed, so under R1 the list runs on. neovim's parser decodes
  a value only when the rest of the reply matches `^=%x+$`, so it reads a
  bare `=` as found with no value, as it reads a boolean (tty.lua 141-149).
  No query neovim sends asks for a key name (section 4). Cost: an empty
  value, which every bound already covers.
* (d) answers bytes baud never sends, so an application that binds them waits
  for bytes that do not come. Cost: those bytes are fixed values, not
  `encode_key` output, so neither the D1 test nor the simulated-press
  constant covers them (section 8.3). They enter `XTGETTCAP_MAX` as fixed
  values, as N2's do. The longest is 7 B, such as `CSI 28 ; 2 ~` for `kHLP`
  (shifted Help), within every limit in section 8.3, the two-character ones
  included, which apply because each row also answers under its two-character
  termcap code (`#1` for `kHLP`, xtermcap.c 73) (**C**).

**`kf21`-`kf35` change class with the kitty flags.** baud has these keys, but
the base layer sends nothing for them, and under any kitty flag they send
`CSI 57384 u` ... `CSI 57398 u` (section 7). Four ways to answer them:

| Option | Sending reading | Literal: validity | Literal: value, as baud's key sends | Literal: value, as xterm's bytes |
|---|---|---|---|---|
| (1) follow the live state: refused until a kitty flag is set, then `CSI 57384 u` ... | conforms | departs while no flag is set | conforms once valid | departs |
| (2) refuse them in every state | departs once a flag is set | departs | -- | -- |
| (3) valid, with xterm's `CSI 42 ~` ... `CSI 56 ~` in every state | departs | conforms | departs | conforms |
| (4) valid, empty while the key sends nothing, then `CSI 57384 u` ... once a flag is set | departs while no flag is set | conforms | conforms | departs |

* (1): under R1 the same query then ends at `kf21` before `CSI > 1 u` and
  runs on after it. Cost: none beyond N1's; validity follows `encode_key`
  returning `None` under the live `KeyState`, which N1 reads for every key.
* (2): validity stays fixed, but baud refuses a name for a key it sends.
  Cost: fifteen rows answered as refused; nothing else.
* (3): an application would wait for bytes that do not come; it is (d)
  above. Cost: as (d), fifteen fixed values outside the D1 test, entering
  `XTGETTCAP_MAX` as fixed values; the longest is 5 B, `CSI 56 ~` (**C**).
* (4): validity stays fixed under R1. It is (c) above, xterm's
  default-configuration answer for a key whose simulated press sends nothing,
  and neovim reads the empty answer as found with no value. Cost: as (1),
  with an empty value where (1) refuses.

Question 2 covers which shape, and both sets of options.

**`kcbt` under `modifyOtherKeys` level 2.** N1 answers baud's live
Shift-Tab, as it answers every key: `CSI Z` with `modifyOtherKeys` off and at
level 1, and at level 2 `ESC [27;2;9~` in xterm's format or `ESC [9;2u` in
the CSI-u format (**M**, `keymax-output.txt`).

xterm's answer to its own query for `kcbt` is `CSI Z` at levels 0-2, the
levels baud has. The query simulates the keysym its table names for `kcbt`,
`XK_ISO_Left_Tab`, with the row's state, 0.

* `XK_ISO_Left_Tab` is an editing key to xterm (input.c 251-254), so it
  takes the branch at 1318-1322 (`ModifyOtherKeys` refuses it at 655-656),
  where the reply is `CSI Z` (1355-1357).
* The test before `modifyOtherKey` there asks for level 2 or above and, with
  the shift removed, whether a modifier is left (1359-1360, 520-521, 139).
  The query's event carries the row's state, 0 (misc.c 5231), so the second
  half reads `!min_mod` (input.c 505-506).
* `min_mod` is 1 below level 3 and 0 at level 3 (`SET_MIN_MOD`,
  [[source: xterm-ptyx-h]] 3540, applied at input.c 1021). So at level 2
  `modifyOtherKey` is skipped, and the answer is `CSI Z`.
* At level 2 and above, 1152-1157 give the query's press, which carries no
  bytes, a `'\t'` byte. The branch at 1318 never reads it, so the answer
  stands.
* xterm has a level 3, `mokExtended` (ptyx.h 3397; [[source: ctlseqs]]
  2377-2378), which baud does not. There the test passes, and
  `modifyOtherKey` raises the modifier to 1 (input.c 766-767): xterm answers
  `CSI 27 ; 1 ; 9 ~`, or `CSI 9 ; 1 u` in the CSI-u format.

**A real Shift-Tab press takes one of two paths in xterm's code, by the
keysym the X keymap delivers.** xterm's comment says only that "Shift-tab is
often mapped to XK_ISO_Left_Tab" (1146-1147); which keymap is common is not
settled here.

* **Delivered as `XK_ISO_Left_Tab`**, the press loses its shift at the test
  above and, at levels 0-2, is sent as `CSI Z`, the query's answer.
* **Delivered as `XK_Tab` with Shift**, it is sent as `CSI Z` at levels 0
  and 1, and as `CSI 27 ; 2 ; 9 ~` at level 2.
  * At levels 0 and 1, 1158-1168 turn it into `XK_ISO_Left_Tab`: at level 1
    because its modifier parameter is Shift alone (1160-1163), at level 0
    because Shift is held (1164-1165). It is then sent as `CSI Z` by the
    path above.
  * That conversion is the `else` of the test at 1152, so it runs only at
    `other_keys <= mokUser`, level 1 or below (ptyx.h 3394-3395). At
    level 2 the keysym stays `XK_Tab`, and its modifier parameter is 2,
    Shift (1141-1143).
  * `XK_Tab` is in none of the classes the branches at 1318, 1383, 1390 and
    1401 test, so its one byte takes the branch at 1413, where 1427-1429
    ask `ModifyOtherKeys`. Its level-2 case answers yes for `XK_Tab` with a
    nonzero parameter (720-724).
  * `modifyOtherKey` then writes the parameter and the key's byte, 9
    (1436-1449, 1464-1466): `CSI 27 ; 2 ; 9 ~` in xterm's format (773-776),
    or `CSI 9 ; 2 u` in the CSI-u format (768-771).
  * So on that keymap xterm's code sends what baud sends, at all three
    levels, and only the press its query simulates answers `CSI Z` at
    level 2.

**ctlseqs' own text gives baud's answer for this case.** Its description of
level 2 says "shift-Tab sends CSI 2 7 ; 2 ; 9 ~ rather than CSI Z (the second
parameter is "2" for shift)" ([[source: ctlseqs]] 2373-2375). That is the
byte string baud's Shift-Tab sends there in xterm's format, and the one
xterm's code sends for a Shift-Tab delivered as `XK_Tab`. Its description of
`formatOtherKeys`, which selects the CSI-u format, gives that format as the
same parameters reordered: "When modifyOtherKeys is set to 1, for example
alt-Tab sends CSI 9 ; 3 u (changing the order of parameters)" (2383-2387).
By that rule the example's CSI-u form is `CSI 9 ; 2 u`, baud's `ESC [9;2u`.
So for "the corresponding string that xterm would send" at level 2,
ctlseqs' text gives baud's bytes in both formats. xterm's code gives the
same bytes for a press delivered as `XK_Tab` with Shift, and `CSI Z` for
`XK_ISO_Left_Tab`, the keysym its query simulates.

Two ways to answer `kcbt` at level 2. `kcbt` is valid on every reading: it is
in xterm's table, and baud's Shift-Tab sends bytes in every state. Section
2.1's value read as xterm's bytes splits here into three columns: by xterm's
text, and by its code for each of the two keysyms, because for this key they
do not all give the same bytes. This note compared the text's examples with
the code for no other key.

| Option | Sending reading | Literal: value, as baud's key sends | Literal: value, as xterm's bytes by its text (2373-2375, 2383-2387) | Literal: value, as xterm's bytes by its code for `XK_Tab` with Shift | Literal: value, as xterm's bytes by its code for `XK_ISO_Left_Tab` (the query's simulated press, or a real one) |
|---|---|---|---|---|---|
| (i) N1: baud's live Shift-Tab, `ESC [27;2;9~` (`ESC [9;2u` in the CSI-u format) | conforms | conforms | conforms in both formats | conforms in both formats | departs |
| (ii) xterm's answer to its own query: `CSI Z` | departs: baud's key does not send it at level 2 | departs | departs | departs | conforms |

* (i) Cost: none beyond N1's. The D1 test covers it: 9 B, the base layer's
  longest D1 value (section 7.1).
* (ii) Cost: a fixed `CSI Z` for one row at level 2, an exception to "a key
  answers its live encoding", outside the D1 test; and an answer baud's
  Shift-Tab does not send there, so an application that binds it waits for
  bytes that do not come.

Cost and consequences of N1:

* **Lookup.** The names are at most 6 bytes, so each packs into a `u64`.
  * A `const`-built open-addressed table, hashed as `StyleId::bucket` hashes,
    with a build-time assertion that no name needs a second probe, is O(1).
  * At 512 to 1,024 slots of a packed key and a 2-byte payload, it is 5 to
    10 KB of `.rodata` (**C**). The slot count is fixed when the hash is
    chosen, and is unmeasured.
  * It is touched only at an XTGETTCAP terminator, never per byte.
* **`TN`.** It needs one embedder setter, since the core does not know
  `TERM`. The setter must bound the length (section 8.3).
* **`RGB` (conforms).** `8` is xterm's answer whenever it draws direct
  colour, whatever `TERM` is. ctlseqs sorts the three fixed names into two
  categories: "Terminal capabilities which may be dynamically adjusted in
  xterm so they do not necessarily match a terminal description", and the
  name of the terminal description (564-571). `TN` is the name, which leaves
  `Co` and `RGB` among the capabilities that may be adjusted. So an `RGB`
  answer that the entry baud names does not carry is the text's own case. Cost: read with user_caps(5)'s meaning
  (section 2.2), the answer says that `xterm-256color`'s `setaf` takes RGB
  values, which it does not: the pinned entry's `setaf` writes `38;5;n` for
  every value past 15 ([[source: terminfo-xterm-256color]] 170). neovim
  reads it only as "truecolor".
* **Tests.** Every answered row needs a test. The exhaustive D1 test of
  section 8.3 is 18,677,760 encodes. Built unoptimised, as the workspace
  builds tests, it ran in 2.6 to 4.3 s on one thread on this Mac, over the
  six passing runs `d1-test-time-output.txt` records: 3.17 s, 4.11 s and
  4.27 s in one session, and 2.63 to 2.65 s in a later one. It goes red when its
  constant is moved off the encoder's 10 B (**M**).
* **neovim locally.** `Ms` is not in xterm's table, so neovim's local `Ms` gets
  `DCS 0 + r ST`. neovim matches only `P1+r` (tty.lua 129), so to neovim
  this is the same as today's silence: the 1000 ms timer still runs. What
  the reply gains is conformance, and an answer for an asker that reads
  `0+r`, as vim does.
* **neovim over ssh.** The four-name query begins with `Tc`, which is not in
  the table either.
  * Under the conforming one-response form (R1), the list ends there, and
    neovim learns nothing about truecolor.
  * Under one DCS per name (R2), `Tc` is refused in its own DCS and `RGB`
    answers `8` in the next, which turns truecolor on (defaults.lua
    1023-1025).

**N2: N1 plus named truths of baud's (departs on scope).** The candidates
are `Tc` and `Su` (booleans), and `Smulx`, `Setulc`, `setrgbf`, `setrgbb`,
`Ss`, `Se`, `Cr`, `Cs`, `E3` and `Sync`. Each would be listed with its value
and with the test that shows baud acts on it.

* **What baud implements.** Each feature is done in `docs/vt-coverage.md`:
  * styled underlines 4:0-4:5 (line 743);
  * direct colour, in both spellings (750-751);
  * the underline colour `58:2::r:g:b` (753);
  * DECSCUSR (790);
  * `OSC 12`/`112` (826);
  * `?2026` (848);
  * `CSI 3 J` (849).
* **The `Se` caveat.** The database's `Se` is `\E[2 q`, a steady block. baud's
  power-on style is configured, and `CSI 0 q` restores it. So `Se` would need
  baud's value, not the database's.
* **`Ms`** is listed separately (section 8.5).
* **The mark: departs on scope.** None of these names is a key name, `Co`,
  `TN` or `RGB`, so each is among the capabilities "intentionally not part of
  this interface" ([[source: ctlseqs]] 550-552). None is "found in xterm's
  tables", so each is an invalid name, whose reply is `DCS 0 + r ST`
  (577, 580-581). Answering them valid departs from both. Section 5 marks the
  four terminals that ship this the same way, and section 8.5 marks the
  converse, refusing `Ms`, as conforming. That all four ship it is practice,
  not a mark.
* **Booleans.** N2 needs a boolean form the text lacks (section 8.6,
  question 4): kitty,
  foot and ghostty write `1+r<hex>`, wezterm writes `=31`, and xterm's own
  valid answer with an empty value is `1+r<hex>=`.
* **The bound.** Each value enters `XTGETTCAP_MAX`, priced at its own name's
  length, since a longer name fills a body with fewer copies (section 8.3).
  With the longest spelling read for each name (the `-x` capture's for `Ss`,
  `Se`, `Cr`, `Cs` and `E3`, and the longest of the four terminals' entries
  for the rest, in the source-text form kitty and ghostty send: `Setulc`
  63 B, `setrgbf`, `setrgbb` and `Sync` 27 B, `Cs` 15 B, `Smulx` 11 B, `Ss`
  10 B, and the rest shorter), N1 plus N2 keeps AG's 96 KiB: a read it opens
  is 96,517 B (R1) or 96,758 B (R2) (**C**, `bounds.py`, a knapsack over every
  mix of names in one body). The name that fills the worst body is `Setulc`,
  and its own limit is 85 B (R1) or 82 B (R2).

**N3: the whole entry compiled (departs on scope, and on the value clause
read as baud's key).** The source is either the pinned `-1` entry (185
capabilities) or the `-x` capture (246).

* **The marks.**
  * Scope: it answers output strings, numbers and booleans that are not key
    names, `Co`, `TN` or `RGB` (ctlseqs 550-552), and that are not in xterm's
    tables, so the text's reply for each is `DCS 0 + r ST` (577, 580-581). It
    departs as N2 does, and on many more names.
  * The value clause read as baud's key: `kbs` answers `^H`, which baud sends
    for Ctrl-H, not for Backspace; and `kf13`-`kf20` answer the bytes baud
    sends for shift-F1..F8, not baud's F13-F20 (section 7). It departs on
    those nine names.
  * The value clause read as xterm's bytes: `kf13`-`kf20` depart here too,
    because xterm answers them `CSI 25 ~` ... `CSI 34 ~` (section 3).
* **Size.** With termcap codes, that is 368 or 429 names, and a table of 4.4
  to 5.2 KB of names and values (**M**, `bounds.py`).
* **Key values.** They are ncurses' convention (section 7):
  * `kbs` differs from the Backspace key.
  * `kf13`-`kf20` name shift-F1..F8. terminfo(5) defines them as F13-F20,
    and xterm's table answers F13-F20.
  * The seven mode-dependent names answer their `smkx` forms, the state the
    entry describes.
  * baud sends every probed value for some key.
* **Scope.** It answers output strings such as `sgr` and `cup`, which the text
  excludes.
* **The ratio.** It moves both terms of note AG's bound.
  * The worst marginal ratio is `Ic` (termcap for `initc`, 97 B), at **40.0
    to 41.2 bytes out per byte in**, against DECRQSS's 22.2 (**C**).
  * A 4096-byte read made wholly of XTGETTCAP queries answers **162,848 to
    167,698 B** (**C**). `bounds.py` prices every body length, so short
    bodies at the read's end are counted.
  * A read opened by the terminator of a straddled full body (section 8.3)
    answers **183,360 B** (R1) **to 188,830 B** (R2), or 188,823 B (R2) with
    the `q` stored (**C**).
  * So AG's 96 KiB no longer holds.

### 8.2 The reply form

**R1: one response, stopping at the first invalid name (conforms).**

* The answer is `DCS 1 + r n1=v1;n2=v2 ST`.
* An invalid first name gets `DCS 0 + r ST` with no name.
* A list cut by an invalid name ends at the last valid pair, without what
  xterm writes after it: the `;` alone for a name not in its table, and the
  `;` with the echoed name for one that returns -1 (section 3). The text gives
  neither, and with either, neovim's parser finds no value in a one-pair
  response (tty.lua 141-149).

The response must be built whole and sent whole or dropped, as DECRQSS now
is. A body cut at `OSC_MAX` is not answered from its prefix, because an
even-length cut can name a different capability. The cut name counts as
invalid, which under R1 ends the list there, and the cut is counted.

neovim's four-name query, with every name answered (`Tc` as a boolean, `RGB`
as `8`, `setrgbf`/`setrgbb` in kitty's source text), is 157 B (**C**). That
needs N2, since N1 refuses `Tc`, `setrgbf` and `setrgbb`. It fits 256 with
nothing else pending.

**R2: one DCS per name, every name answered or refused in turn (departs;
what kitty, foot, wezterm and ghostty do).**

* This departs from "each name/value pair in the same response" and from
  "ends processing".
* Each reply is small and can be dropped and counted alone. The same four
  names, all answered (N2), take 175 B (**C**).
* An empty name is a name too: `;;`, a leading or trailing `;`, or a body of
  `;` alone. If R2 refuses each one in a DCS of its own, a body of 512 `;`
  draws 513 refusals of 7 B, 3,591 B. Section 8.3 carries that.
* Its bound is tighter than R1's: under N1 no name may answer more than 29 B,
  against R1's 32 B (section 8.3).

**Within R2, whether a refusal echoes the name is a choice of its own.**

* `DCS 0 + r ST` is the text's invalid reply.
* `DCS 0 + r <hex> ST`, as kitty, foot and wezterm write (section 5), departs
  from it. It names the refused capability in the reply itself, as an asker
  matching replies by name needs; neovim matches its `1 + r` replies so
  (tty.lua 129-134).
* The echo costs nothing on the bound. A refused name of h hex digits returns
  7 + h bytes for h + 1 in, never more than an empty name's 7 for 1, so the
  largest body of refusals is 3,591 B either way (**C**, `bounds.py`).

ghostty's silence is a departure on both counts: a query recognised and not
answered.

### 8.3 The reply-buffer bound

A body is at most `OSC_MAX` = 512 bytes.

* **This section assumes a prefix node consumes `+ q`**, as `Node::DcsDollar`
  and `Act::DcsQuery` consume DECRQSS's `$ q` (graph.rs 861-868). Then all
  512 bytes carry names.
* Today's path stores the `q`, which leaves 511 (section 1, Q3). Where that
  changes a figure, the figure is given beside it.
* The shortest names, two characters written as four hex digits, fill the
  body with 102 names in 509 bytes either way (**C**, `bounds.py`).

**A body that straddles two reads is answered in the second (M).** baud keeps
a DCS body across reads, and a terminator alone in the next read ends it there
(section 1; `straddle-output.txt`):

* A1-A3: DECRQSS, ended by `ESC \`, by a lone `\` and by a lone `9C`,
  answers in the second read.
* B1-B2: a 509-byte `DCS + q` body is ended in the second read.
* C1: a read of `9C` plus 819 `90 $ q m 9C` owes 820 DECRQSS answers,
  counted as those sent plus those dropped. Every answer is the same 111 B,
  so that is 91,020 B: exactly note AG's formula with a 111 B largest action,
  `111 + floor((4096-1)/5) x 111`.

So the largest single action can open a read with one byte. That is what the
`floor((L-1)/5)` term of AG 1.2's bound charges for. AG's formula is the
straddle-correct bound, not a loose one.

**Under N1** (C1 queries, 7-bit replies), a full body's reply depends on the
longest value one name can answer. "A read it opens" means the full body
ended by the read's first byte, then 4,095 bytes of DECRQSS at 111 B per
5 bytes, then AG's 106 B queued outside `feed`:
`max(1,131, body) + 819 x 111 + 106`. 96 KiB is 98,304 B. The replies are
**C**, from `bounds.py`; the key rows' lengths are **M** (section 7.1).

| Longest value one name answers | One full body, R1 | One full body, R2 | A read it opens, R1 | A read it opens, R2 |
|---|---|---|---|---|
| 48 B, `KeyBytes::MAX_LEN` (a buffer size) | 10,410 B | 11,037 B | **101,425 B** | **102,052 B** |
| 40 B, `encode_key`'s global maximum (D0) | 8,778 B | 9,405 B | **99,793 B** | **100,420 B** |
| 14 B, `TN` = `xterm-256color` | 3,474 B | 4,101 B | 94,489 B | 95,116 B |
| 12 B, the table's keys under any modifier byte, in every key state (D3) | 3,066 B | 3,693 B | 94,081 B | 94,708 B |
| 11 B, the table's keys under shift/alt/ctrl, in every key state (D2) | 2,862 B | 3,591 B | 93,877 B | 94,606 B |
| 10 B, xterm's table rows exactly, in every key state (D1): what XTGETTCAP simulates | 2,658 B | 3,591 B | 93,673 B | 94,606 B |
| 6 B, the key probe's longest (base layer, 105 names, two states) | 1,842 B | 3,591 B | 92,857 B | 94,606 B |

**"One full body, R2" is the larger of two bodies.**

* At 11 B and below, 513 refused empty names, a body of 512 `;`, give
  3,591 B (section 8.2). That needs R2 to refuse each empty name in a DCS of
  its own.
* At 12 B and above, 102 answered names give more, and the 3 bytes left in
  the body carry 3 refused empty names: 3,672 + 21 = 3,693 B at 12 B.
* With the `q` stored (a 511-byte body) there is one refused empty name
  fewer, so every R2 figure falls by 7 B and R1 does not change. In the
  "read it opens, R2" column, 102,052 B becomes 102,045 B, 100,420 B becomes
  100,413 B, 95,116 B (`TN`) becomes 95,109 B, 94,708 B becomes 94,701 B,
  and 94,606 B becomes 94,599 B; the empty-name body is 512 refusals,
  3,584 B.

What follows:

* **Under N1, 96 KiB holds only if no name answers more than 32 B (R1) or
  29 B (R2).** At 33 B, R1 needs 98,365 B; at 30 B, R2 needs 98,380 B. These
  thresholds hold with the `q` stored too.
  * What XTGETTCAP simulates (10 B) is within it, and so are the wider
    domains measured (11 B, 12 B).
  * `encode_key`'s global maximum (40 B) is not, and neither is
    `KeyBytes::MAX_LEN` (48 B). The 40 B is not a simulated press: it needs
    associated text, both alternate keys, a repeat, and a three-digit
    modifier (section 7.1).
  * The limits are for two-character names, which fill a body with the most
    copies. A longer name may answer more: 58 B (R1) or 55 B (R2) for a
    four-character name, 85 B or 82 B for a six-character one (**C**,
    `bounds.py`). That is what prices N2 (section 8.1).
* **The limits assume DECRQSS's richest answer is 111 B.** In the colon form
  of question 6 it is 114 B, one byte more for each of the three direct
  colours the richest rendition holds (`baud-core/tests/vt220/dcs.rs`
  132-133), and AG's never-drops buffer becomes 94,603 B.
  N1's limits then fall to **20 B (R1) and 17 B (R2)**, with the `q` stored
  too (**C**, `bounds.py`).
  * D1 (10 B), D2 (11 B), D3 (12 B) and `TN` (14 B) still fit.
  * N1 plus N2 at the spellings above does not: a read it opens is 98,974 B
    (R1) or 99,215 B (R2), because `Setulc` (63 B) passes its 54 B (R1) or
    51 B (R2) limit.
  * `Ms` as terminfo source text (21 B) passes the two-character limit;
    decoded (17 B) it fits (section 8.5).
* **N1 moves only AG's largest-action term.**
  * A full N1 body is at most 20.2 (R1) or 21.4 (R2) bytes out per byte of
    its 516-byte query, even at 48 B a key (20.3 and 21.6 per body byte). Its
    worst marginal name is 21.6, and an empty name is 7.
  * All are under DECRQSS's 22.2, so the per-byte term stays DECRQSS's. A
    name would have to answer more than 49 B (R2) or 52 B (R1) to pass it.
* **`XTGETTCAP_MAX` is derived from the presses XTGETTCAP simulates.** It
  cannot come from `encode_key`'s global maximum or from `MAX_LEN`: either
  fails AG's budget on day one (40 B and 48 B, above). `encode_key` is not a
  `const fn`, so no const evaluation can run it. The derivation is two parts:
  * A constant for the longest simulated key value enters `XTGETTCAP_MAX`
    beside `TN`'s bound, and under N2 beside each fixed value at its name's
    length. Under options (d), (3) and (ii) of section 8.1, the fixed bytes
    they answer are not `encode_key` output, so they enter the same way, as
    fixed values: at most 7 B (`CSI 28 ; 2 ~`), inside every limit below. `XTGETTCAP_MAX` is const-asserted inside AG's budget, so that
    assert goes red when the key constant passes 32 B (R1) or 29 B (R2), or
    20 B and 17 B with DECRQSS in the colon form.
  * An exhaustive test holds the encoder to the constant. It encodes every
    table row baud serves, with the row's modifiers, as a press with no text
    and no alternates, in every key state: D1, 190 rows by 98,304 states,
    18,677,760 encodes. It requires the longest to *equal* the constant, so
    it goes red when the encoder grows, and also when it shrinks and the
    constant goes slack.
  * D1 is the table's own rows. D2 (11 B) or D3 (12 B) would also fit, if
    margin is wanted over the rows.
* **`TN` needs the same bound.** `TERM` is set by the embedder.
  * A full body of 102 `TN` names grows the reply by 204 B for each byte of
    `TERM`. A full body of `name` holds 57 names, 114 B per byte, or 56 names
    and 112 B with the `q` stored (**C**).
  * So the setter must take a bounded type, and its limit enters
    `XTGETTCAP_MAX`. Otherwise a long `TERM` breaks 96 KiB on its own.
* **A full body's reply is far past `REPLY`** = 256 B, the size today, and is
  dropped and counted there. Short lists fit: neovim's four names, all
  answered under N2, take 157 B (R1) or 175 B (R2) (**C**).
* **`XTGETTCAP_MAX` joins AG's budget test whichever AG option is chosen**, as
  AG 1.5 says a reply "whose length depends on stored state" must.
* **Under N3, AG's 96 KiB is lost** (8.1).

### 8.4 XTSETTCAP

XTSETTCAP loads a host database entry. baud has no run-time database: the
core is `no_std`, and its entry is compiled. baud also has no `?1050`, and
`?1050` has no row either.

**Two readings of "the terminal database".** The text says xterm "will use
the name to retrieve data from the terminal database. If successful, that
overrides the termName resource" ([[source: ctlseqs]] 538-542). The override
is conditional, and the marks below turn on what counts as that database.

* **Reading A: the host's database**, which is what xterm's code reads: it
  calls `setupterm` and rings the bell on failure (xtermcap.c 624-657,
  655-656). baud has none at run time, so on this reading every XTSETTCAP
  fails to retrieve anything, and failure is the text's own path: nothing is
  overridden.
* **Reading B: baud's compiled entries are its terminal database.** Then a
  pinned name is retrieved, and the text has the new name used "as well as
  by the Request Termcap/Terminfo String control", which is XTGETTCAP.

Options 2 and 3 below exist only on reading B. On reading A there is nothing
to retrieve, so for XTSETTCAP each reduces to option 1.

1. **Count it and refuse it**, under its own name, with a row that says why.
   * Reading A: conforms. Every retrieval fails, the text's own path, and
     xterm's code does the same with a bell. It is also what xterm's code
     would change for baud, which has no tcap keyboard: nothing (section 3).
   * Reading B: departs, once baud answers XTGETTCAP, because a compiled
     entry could have been retrieved and the text says XTGETTCAP then uses
     its name.
   * Cost: a counter and a row; nothing is stored.
2. **Switch XTGETTCAP's answers among compiled entries**: the ten baud pins
   besides the `-x` capture, `xterm-256color` among them, and the capture
   too. A name with no compiled entry is refused and counted, as
   xterm rings the bell. Two variants, which differ in what follows the
   entry:
   * **2a: only `TN` follows the entry.** Conforms on reading B: the text's
     override is of the name, and a key's value stays "the corresponding
     string that xterm would send", read as baud's key, which XTSETTCAP does
     not change outside a tcap keyboard. Cost: one stored index into the
     pinned names; `TN`'s bound stays the longest of them, 14 B
     (`xterm-256color`).
   * **2b: the key answers follow the entry too.** Departs on reading B, from
     the value clause read as baud's key: each answer is the entry's string,
     not what baud's key sends, and baud does not send many of them.
     `input_caps.rs` finds 27 of rxvt's 77 input capabilities among baud's
     outputs, and 10 of vt100's 22 (**M**, `input-caps-output.txt`), so an
     application would wait for bytes that do not come. Cost: 2a's index,
     plus the entries' k-prefixed capabilities in the core. Over the ten
     entries that is 946 names and 9,228 B of names and values, and the
     capture's 141 such capabilities add 230 names and 2,526 B. The longest
     value is 7 B, so AG's bound holds (**C**, `bounds.py`). These counts take
     every capability whose name begins with `k`, as `input_caps.rs`'s
     `is_input` tests. They include the boolean `km` and nsterm's
     `kbs=\177`, which `input_caps.rs`'s parser leaves out because they
     carry no `\E` or `^`: nsterm counts 40 here and 39 there.
     * *Since 2026-10-18 every pin is an `infocmp -x -1` capture:* rxvt has
       99 k-prefixed capabilities, nsterm 46, xterm and xterm-16color 141, and
       `bounds.py` counts 1,130 names and 11,466 B over the ten. The counts
       above describe the pins before that day.
3. **Build `?1050` with XTSETTCAP.** Under the tcap keyboard the text and
   xterm's code agree: the keys send the loaded entry's strings, and
   XTGETTCAP answers them (section 3).
   * While `?1050` is set: conforms on reading B, on both value readings,
     since the key answered is the key sent.
   * While it is reset: option 1's mark on whichever reading is taken, unless
     2a or 2b is built too, when it takes that variant's mark.
   * Cost: a new mode with its own row; an encoder path that sends a compiled
     entry's string for each key the entry names, falling back to the normal
     encoding for the rest, as xterm does (input.c 1289-1293); the entries'
     k-prefixed capabilities in the core (9,228 B, above); and every key test
     run under the mode as well.

The proposal is option 1, which is what xterm's code would do for baud, and
the owner decides (question 5).

### 8.5 `Ms`

`Ms` is outside the text's scope: it is not a key name, nor `Co`, `TN` or
`RGB`. It is not in xterm's table, so under N1 it is invalid.

* **Answering it** is N2, and departs on scope as N2 does: `Ms` is among the
  capabilities "intentionally not part of this interface", and not "found in
  xterm's tables" (ctlseqs 550-552, 580-581; section 8.1). It would answer the
  database's own `\E]52;%p1%s;%p2%s\007`.
  * That is true only while baud honours OSC 52 writes. V11 decided that, and
    it is unbuilt.
  * It is also true only while `[clipboard] osc52` is not `"off"`. So the
    answer changes with configuration.
  * Its cost: on a remote host with no clipboard tool, neovim then copies
    through OSC 52, and an explicit `+`/`*` paste waits 1 s and then up to
    9 s, because baud refuses reads (section 4). A plain `p` is unaffected.
  * Its value is 17 B decoded and 21 B as terminfo source text. Both fit
    N1's 32 B (R1) and 29 B (R2). With DECRQSS in the colon form (question 6)
    the decoded value still fits, and the source text passes the 20 B (R1)
    and 17 B (R2) limit (section 8.3).
* **Refusing it** with the chosen invalid reply conforms; that is N1's
  answer. Silence would not: it is ghostty's departure, a query recognised
  and not answered (section 5).
  * Its cost: once V11's writes exist, the same neovim selects no OSC 52
    provider, so `"+y` there copies nothing to the local clipboard.
  * A user can set `g:clipboard = 'osc52'`, which forces the provider, and
    its paste wait, whatever baud answers (section 4).

### 8.6 Booleans

Only N2 and N3 answer a boolean; under N2 they are `Tc` and `Su`. Every form
below sits inside an option that already departs on scope (section 8.1). The
marks here are against the valid reply's shape: "DCS 1 + r Pt ST for valid
requests, adding to Pt an = , and the value of the corresponding string that
xterm would send" ([[source: ctlseqs]] 574-575). A boolean has no string, so
the text gives it no value. The forms differ in which part of the shape they
keep.

| Form | Who writes it | The `=` | The value |
|---|---|---|---|
| `1+r<hex>` | kitty, foot, ghostty | departs: dropped | departs: none given |
| `1+r<hex>=31`, the value `1` | wezterm | conforms: kept | departs: `1` is not "the corresponding string that xterm would send", since a boolean has none, and the text defines no other value |
| `1+r<hex>=`, an empty value | xterm, for a name whose value is empty (section 3) | conforms: kept | conforms if a capability with no string is read as having the empty string; departs if it is read as having no value at all, and on that reading no form conforms |

Costs, with every other N2 value as section 8.1 prices it (**C**,
`bounds.py`, the question 4 section):

* `1+r<hex>`: nothing beyond the name. N1 plus N2 keeps AG's 96 KiB, 96,517 B
  (R1) or 96,758 B (R2), as in section 8.1.
* `1+r<hex>=`: 1 B more per boolean name, and the same figures: no boolean
  is among the names that fill the worst body. It reads the same as option (c)'s empty key
  value. The names are disjoint, so an asker that knows what it asked for
  can tell them apart.
* `1+r<hex>=31`: 3 B more per boolean name, and the same figures.
* With DECRQSS in the colon form, all three give 98,974 B (R1) or 99,215 B
  (R2), over 96 KiB because of `Setulc` (section 8.3), not because of the
  booleans.
* neovim reads the bare form and the `=` form alike, as found with no value
  (tty.lua 141-149). It decodes `=31` as the value `1`. Its truecolor check
  asks only whether `Tc` was found (defaults.lua 1017-1025), so any of the
  three turns truecolor on.

## 9. Found on the way, decided by no question

* **`docs/vt-coverage.md` has no rows for `DCS + q`, `DCS + p` or `DCS + Q`**
  (XTGETXRES). It has none for `?1050`-`?1061` either.
* **The pinned entry is not the entry programs read.** The `-1` capture lacks
  the `-x` capabilities (section 2.2).
  * *Since 2026-10-18 it is:* the conformance fixture is the `-x` capture,
    and the suite replays all six strings below, `Cs` with a colour for its
    string. The rest of this item describes the pin before that day.
  * The conformance suite therefore replays none of the six output strings
    the database adds: `Cr`, `Cs`, `E3`, `Ms`, `Se` and `Ss`.
  * baud acts on five of them. `Ms` writes are dropped and counted, pending
    V11.
  * The capture is pinned as a source only
    ([[source: terminfo-xterm-256color-x]]); it does not join the
    conformance profiles. Added as an eleventh `profile!` at `ffa436f`, it
    fails on `Cs` alone, 1 of 88 output capabilities. The suite's `expand`
    writes the digit `1` for every `%d`, `%c` and `%s`
    (`baud-core/tests/conformance/terminfo.rs` 96-97), so
    `Cs=\E]12;%p1%s\007` replays as an OSC 12 whose colour is `1`, which
    names no colour. The fault is in the replayed value, not the
    capability: the same OSC 12 carrying `rgb:ff/00/00` leaves
    `Term::unsupported` at 0. The capture's manifest entry lists what would
    move if a later change made it join.
* **Comments in the key encoder and the core say what is no longer so.**
  * `bc16520` rewrote all four below to say what the code does; their line
    numbers are `4911bf0`'s.
  * key.rs 275-285 derives the CSI-u maximum at 15 B and says the
    alternate-key and associated-text sub-fields "will grow it again and this
    bound is re-derived when they land". They have landed (`kitty_encode`,
    461-476), and the comment was not re-derived. Its own case,
    `CSI 57383 ; 128 : 3 u`, is 14 B, and the encoder's maximum is 40 B
    (section 7.1). `MAX_LEN` = 48 still covers it.
  * The module doc says "a key is at most a dozen bytes" (key.rs 10-11).
  * key.rs 86, 369 and 418, and `baud-core/src/term.rs` 471-472 and
    1841-1843, say kitty's `CSI ? u` stays refused. `write.rs` 1668-1677
    answers it, and baud answers it at `4911bf0` (Q1).
  * key.rs 410-419 also says that under kitty's flags the text keys and the
    keypad send nothing, and 439-440 that "The keypad's kitty PUA numbers
    arrive with the alternate-key work, so it still sends nothing". Under
    them D0 encodes text keys, and D1 encodes the keypad, for example
    `CSI 57413 ; 2 u` (section 7.1).
* **baud's Shift-Tab under `modifyOtherKeys` level 2 sends `ESC [27;2;9~`
  (M); xterm answers its own `kcbt` query with `CSI Z`** at levels 0-2,
  because the query simulates `XK_ISO_Left_Tab`, whose shift is removed
  before xterm asks whether a modifier is left (section 8.1).
  * xterm's code sends a real Shift-Tab by the keysym the X keymap delivers.
    Delivered as `XK_ISO_Left_Tab`, it is `CSI Z` at levels 0-2.
  * Delivered as `XK_Tab` with Shift, it is `CSI Z` at levels 0 and 1, where
    input.c 1158-1168 turn it into `XK_ISO_Left_Tab`. At level 2 that
    conversion does not run (1152), and the press goes out through
    `ModifyOtherKeys` (720-724) and `modifyOtherKey` (773-776) as
    `CSI 27 ; 2 ; 9 ~`, baud's bytes.
  * ctlseqs' own description of level 2 gives the same bytes: "shift-Tab
    sends CSI 2 7 ; 2 ; 9 ~ rather than CSI Z" ([[source: ctlseqs]]
    2373-2375).
  * So on that keymap, xterm's answer to its query differs from what its
    own key sends at level 2. Which baud follows for `kcbt` is in
    question 2.
* **DECRQSS spells direct colour the legacy way, and neovim's probe rejects
  it.** `dcs.rs` 280-294 writes `48;2;r;g;b` and `38;5;n`.
  * [[source: ctlseqs]] 1287-1289 says, of SGR input, that "xterm allows
    either colons (standard) or semicolons (legacy)".
  * For DECRQSS's reply it says only "replacing the Pt with the corresponding
    CSI string" (499-501), so the text does not fix the reply's spelling.
  * xterm answers `48:2::r:g:b`, `38:5:n`, and `3n`/`9n` below 16
    ([[source: xterm-misc]] 3280-3299). It reports no underline colour:
    nothing in `xtermFormatSGR` (3251-3316) writes a `58`, where the same
    search finds its `;38:2::` (3282).
  * neovim's truecolor probe (defaults.lua 1040-1080) splits on `;` and
    returns without deciding on baud's answer (**C**).
  * So **over ssh, both of neovim's truecolor detections fail against baud
    today**: the XTGETTCAP query gets silence, and the DECRQSS spelling is
    rejected. Which spelling to answer is question 6.

## 10. Red-first tests (any option)

* `Co`, `TN` and `RGB` answer exactly, under `S7C1T` and `S8C1T`.
* `TN` equals the `TERM` `pty.rs` exports, and the setter refuses a `TERM`
  longer than its bound.
* An unknown name, an empty body, an odd number of digits and a non-hex byte
  each get the chosen invalid reply.
* Empty names inside a list get the chosen invalid reply: `;;`, a leading
  and a trailing `;`, and a body of `;` alone. Under R2 there is one reply per
  empty name, and a body of 512 `;` stays inside the bound (3,591 B).
* `kf21` answers as decided both before and after `CSI > 1 u`
  (question 2).
* Each of the 36 names for keys baud has no code for (Help, Find, Select,
  Undo, Clear, F36-F63) answers as question 2 decides, and a list holding one
  ends or runs on as that choice says.
* `ka1`, `kc1`, `ka3` and `kc3` answer what Home, End, PageUp and PageDown
  send: `CSI H` (`SS3 H` with DECCKM), `CSI F` (`SS3 F`), `CSI 5 ~` and
  `CSI 6 ~` (**M**, `keymax-output.txt`).
* `kcbt` answers `CSI Z` with `modifyOtherKeys` off and at level 1. At
  level 2 it answers as question 2 decides: under (i), what baud's Shift-Tab
  sends, `ESC [27;2;9~` in xterm's format and `ESC [9;2u` in the CSI-u
  format; under (ii), `CSI Z` (section 8.1).
* Lower-case hex is accepted, and the name is echoed as sent (vim sends
  `%02x`).
* `kcuu1` answers `CSI A` with DECCKM reset and `SS3 A` with it set. `kbs`
  answers `7F`. Under N1, `kf13` answers `CSI 25 ~`: N1 follows xterm's table
  and terminfo(5), where `kf13` is F13, not ncurses' shift-F1.
* A list whose second name is invalid ends as the chosen form says.
* A body cut at `OSC_MAX` counts `string_truncations`, and **the cut name is
  treated as invalid**. An even-length cut could name a different
  capability.
* A full body whose terminator arrives alone in the next read is answered in
  that read, in both C1 forms (`straddle-probe.rs`'s B cases, with a reply).
* A sweep over every pending reply length up to `REPLY`: the reply arrives
  whole, or is dropped and counted once, as DECRQSS's sweep does.
* XTSETTCAP does what question 5 decides; under option 1 it is counted and
  changes nothing.
* `XTGETTCAP_MAX` is in AG's budget test, built from the simulated-press
  constant, `TN`'s bound, and every fixed value the chosen options answer
  (N2's, and those of section 8.1's (d), (3) or (ii)), each at its name's
  length, so the const-assert fails when any of them passes
  32 B (R1) or 29 B (R2), or the colon-form limits if question 6 takes it.
* The exhaustive D1 test: every table row baud serves, as a press with no
  text and no alternates, in every key state. Its longest equals the
  simulated-press constant.
* A property at `PROPTEST_CASES=20000` whose generator emits XTGETTCAP with
  valid, invalid and truncated lists in both C1 forms: the replies do not
  depend on where the reads fell.
* A fixture of neovim's startup, recorded in baud, answers `Ms` as decided.

## 11. The owner's questions

Every option below carries a mark, conforms or departs, against the text and
the reading named, and a cost. Section 8 holds the derivations.

1. **Which names?**
   * **N1**, xterm's table: keys from the live encoder, plus `Co`, `TN` and
     `RGB`. **Conforms**: every name it answers is "found in xterm's
     tables", and a key's value is the string baud's key sends (2.1, 8.1).
     The keys baud lacks, `kf21`-`kf35` and `kcbt` at level 2 carry marks of
     their own, under question 2.
     * Cost: a `const`-built open-addressed table of 5 to 10 KB of `.rodata`
       (**C**), touched only at an XTGETTCAP terminator, whose slot count is
       fixed when the hash is chosen and is unmeasured. It also needs a `TN`
       setter that takes a bounded type, since the core does not know `TERM`,
       and a test for every answered row. The exhaustive D1 test is
       18,677,760 encodes, and ran in 2.6 to 4.3 s unoptimised on one thread
       over the six passing runs recorded (**M**, 8.1, 8.3).
     * neovim locally is unchanged. Its one local query, `Ms`, is not in
       xterm's table, so it gets `DCS 0 + r ST`, which neovim ignores; its
       1 s timer still runs (8.1).
     * For AG's 96 KiB, no name may answer more than 32 B (R1) or 29 B (R2),
       keys and `TERM` alike (8.3). The presses XTGETTCAP simulates are at
       most 10 B (D1), or 12 B under any modifier byte (D3), so they fit.
       `encode_key`'s global maximum, 40 B, does not fit, but it is not a
       simulated press. With DECRQSS in the colon form (question 6) the
       limits are 20 B and 17 B, and D1-D3 and `TN` still fit.
     * Its `RGB` answer, `8`, is xterm's, and **conforms**: ctlseqs counts
       `RGB` among the capabilities that "may be dynamically adjusted in
       xterm so they do not necessarily match a terminal description"
       (564-571). Cost: read with user_caps(5)'s meaning, it says
       `xterm-256color`'s `setaf` takes RGB values, which it does not;
       neovim reads it only as truecolor (8.1).
     * What it answers for the 36 names whose key baud lacks, for
       `kf21`-`kf35` and for `kcbt` at level 2 is question 2.
   * **N2**, N1 plus named truths of baud's (`Tc`, `Su`, `Smulx`, `Setulc`,
     `setrgbf`/`setrgbb`, `Ss`/`Se`, `Cr`/`Cs`, `E3`, `Sync`). **Departs on
     scope.** None of these is a key name, `Co`, `TN` or `RGB`, so each is
     among the capabilities "intentionally not part of this interface"
     (ctlseqs 550-552). None is "found in xterm's tables", so the text's reply for
     each is `DCS 0 + r ST` (577, 580-581). Section 5 marks the four
     terminals that ship this the same way.
     * Cost: a row and a test for each name. Each value is true only while
       baud acts on it, and `Se` needs baud's own value, not the database's.
       It also needs a boolean form (question 4).
     * Priced at each name's own length with the longest spelling read, it
       keeps AG's 96 KiB: 96,517 B (R1) or 96,758 B (R2) (8.1). With DECRQSS
       in the colon form it does not (98,974 B / 99,215 B), because `Setulc`
       passes its limit (8.3).
   * **N3**, the whole compiled entry. **Departs on scope**, as N2 does and
     on far more names, since it also answers output strings and numbers
     (8.1). **It departs on the value clause read as baud's key** for `kbs`,
     which answers baud's Ctrl-H, and for `kf13`-`kf20`, which answer baud's
     shift-F1..F8. For `kf13`-`kf20` it also departs on the value read as
     xterm's bytes, since xterm answers the VT220 F13-F20, `CSI 25 ~` and on.
     * baud sends every one of these values for some key: Ctrl-H sends `^H`,
       and shift-F1..F8 send the entry's `kf13`-`kf20` (section 7).
     * Cost: a table of 4.4 to 5.2 KB of names and values (**M**). AG's
       96 KiB is lost: a read opened by a straddled full body answers
       183,360 B (R1) to 188,830 B (R2) (**C**, 8.1).
2. **Which reply form?**
   * **R1**, one response that stops at the first invalid name, with
     `DCS 0 + r ST` for an invalid request. **Conforms.**
     * Cost: the response is built whole and sent whole or dropped, as
       DECRQSS's is. Under N1 no name may answer more than 32 B.
     * neovim over ssh gets no truecolor answer unless `Tc` is answered,
       which needs N2, because the list ends at `Tc` (8.1).
   * **R2**, one DCS per name. **Departs** from "each name/value pair in the
     same response" and from "ends processing". It is what all four
     terminals do.
     * Cost: a tighter bound, where no name may answer more than 29 B against
       R1's 32 B. A body of 512 `;` draws 3,591 B of refusals (8.3).
     * Under N1 it gives neovim over ssh truecolor from `RGB`.
     * Within R2, a refusal either echoes the name or does not.
       `DCS 0 + r <hex> ST`, as kitty, foot and wezterm write, **departs**
       from the text's invalid reply. `DCS 0 + r ST` **conforms**. Cost: the
       echo adds the name's hex to each refusal and nothing to the bound
       (8.2).
   * For either form, a name in xterm's table for a key baud has no code for
     (Help, Find, Select, Undo, Clear, F36-F63) gets one of four answers
     (8.1, where each is marked on both readings):
     * (a) `DCS 0 + r ST`. **Conforms** on the sending reading; **departs**
       from the literal reading's validity. Cost: none beyond the refusal
       every invalid name takes. Under R1 it ends the list.
     * (b) `DCS 0 + r <hex> ST`. **Departs** on both readings. It is xterm's
       -1 reply, for a row `keyIsDistinct` refuses or a Sun-keyboard function
       key, not for a key that sends nothing. Cost: as (a), plus the name's
       hex. Under R1 it ends the list.
     * (c) valid with an empty value, `DCS 1 + r <hex>= ST`. **Conforms** on
       the literal reading's validity and on its value clause read as baud's
       key. Read as xterm's bytes, it **conforms** for Undo and Clear and
       **departs** for the rest. It **departs** on the sending reading. It is
       xterm's default-configuration answer for Undo and Clear, which comes
       from a simulated press with no bytes (section 3). It keeps validity
       fixed under R1, and neovim reads it as found with no value. Cost: an
       empty value, which every bound covers.
     * (d) xterm's own bytes, such as `CSI 28 ~` for Help. **Conforms** on
       the literal reading with the value read as xterm's bytes; **departs**
       on the sending reading, and on the value read as baud's key except
       for Undo and Clear, where it is (c). Cost:
       fixed values outside the D1 test, entering `XTGETTCAP_MAX` at up to
       7 B (`CSI 28 ; 2 ~`), within every limit (8.3). An application that
       binds them waits for bytes baud never sends.
   * `kf21`-`kf35` are keys baud has. The base layer sends nothing for them,
     and kitty's flags send `CSI 57384 u` ... `CSI 57398 u` (8.1). Four
     choices:
     * (1) Follow the live state: refused until an application sets a kitty
       flag, then valid. **Conforms** on the sending reading. **Departs**
       from the literal reading's validity while no flag is set. Once valid
       it **conforms** on the value read as baud's key and **departs** on
       the value read as xterm's bytes. Cost: none beyond N1's, since every
       key answer reads the live `KeyState`. Under R1 the same list then
       ends at `kf21` in one state and not the other.
     * (2) Refuse them in every state. **Departs** on the literal reading in
       every state, and on the sending reading once a flag is set, because
       baud would refuse a name for a key it sends. Cost: fifteen rows
       answered as refused; nothing else.
     * (3) Answer xterm's `CSI 42 ~` ... `CSI 56 ~`. **Conforms** on the
       literal reading with the value read as xterm's bytes. **Departs** on
       the sending reading and on the value read as baud's key, because baud
       never sends those bytes. Cost: fifteen fixed values outside the D1
       test, entering `XTGETTCAP_MAX` at 5 B. An application that binds them
       waits for bytes that do not come.
     * (4) Valid, with an empty value while the key sends nothing, and
       `CSI 57384 u` ... once a flag is set. **Conforms** on the literal
       reading with the value read as baud's key; **departs** on the value
       read as xterm's bytes, and on the sending reading while no flag is
       set. It keeps validity fixed under R1. It is xterm's
       default-configuration answer for a key whose simulated press sends
       nothing, and neovim reads the empty answer as found with no value.
       Cost: as (1), with an empty value where (1) refuses.
   * `kcbt` at `modifyOtherKeys` level 2. `kcbt` is valid on every reading,
     so the marks are on its value. xterm answers its own query with
     `CSI Z` at levels 0-2, the levels baud has, because the query
     simulates `XK_ISO_Left_Tab`. A real Shift-Tab that the X keymap
     delivers as `XK_Tab` with Shift is sent by xterm's code as
     `CSI 27 ; 2 ; 9 ~` at level 2, and ctlseqs' own description of level 2
     says shift-Tab sends `CSI 2 7 ; 2 ; 9 ~` (2373-2375) (8.1). Two
     choices:
     * (i) N1's answer, baud's live Shift-Tab: `ESC [27;2;9~`, or
       `ESC [9;2u` in the CSI-u format. **Conforms** on the sending reading,
       on the value read as baud's key, and on the value read as xterm's
       bytes by ctlseqs' text in both formats: xterm's format is its example
       at 2373-2375, and the CSI-u form is that example's parameters
       reordered, as the text describes for `formatOtherKeys` (2383-2387).
       **Conforms** too on the value read as xterm's bytes by its code for a
       press delivered as `XK_Tab` with Shift. **Departs** on the value read
       as xterm's bytes by its code for `XK_ISO_Left_Tab`, the keysym its
       query simulates. Cost: none beyond N1's; the D1 test covers it, at
       9 B.
     * (ii) xterm's answer to its own query, `CSI Z`. **Conforms** on the
       value read as xterm's bytes by its code for `XK_ISO_Left_Tab`, the
       simulated press, or a real press delivered as that keysym.
       **Departs** on the sending reading, on the value read as baud's key,
       on the value read as xterm's bytes by its text, and by its code for a
       press delivered as `XK_Tab` with Shift. Cost: one fixed row outside
       the D1 test, an exception to "a key answers its live encoding". An
       application that binds it waits for bytes baud's Shift-Tab does not
       send at level 2.
3. **`Ms`** is outside the text's scope: it is not a key name, nor `Co`,
   `TN` or `RGB`, and it is not in xterm's tables (8.5). Three choices:
   * (1) Answer it once V11's writes are built and on. **Departs on scope**,
     as N2 does.
     * Cost: on a remote host with no clipboard tool, neovim then copies
       through OSC 52. An explicit `"+` or `"*` paste waits 1 s and then up
       to 9 s, because reads stay refused. A plain `p` is unaffected.
     * The answer is true only while `[clipboard] osc52` is not `"off"`, so
       it changes with configuration.
     * Its value enters `XTGETTCAP_MAX`. It fits decoded (17 B). As terminfo
       source text (21 B) it does not fit if DECRQSS takes the colon form
       (8.5).
   * (2) Answer it once V11 is built, with that paste wait addressed
     separately. **Departs on scope**, as (1) does.
     * Cost: (1)'s, wait included. The only thing baud could do to end
       neovim's wait is answer the read, and that refusal stands either way
       (section 4).
     * So, as written, this is (1). It becomes a choice of its own only if a
       way to end the wait outside baud's reply is named, and none is named
       here.
   * (3) Always refuse it, with the chosen invalid reply. **Conforms**: under
     N1 it is not in xterm's table.
     * Cost: on a remote host with no clipboard tool, neovim then has no
       OSC 52 copy once V11's writes exist, unless the user sets
       `g:clipboard = 'osc52'`. That forces the provider, and its paste
       wait, whatever baud answers.
4. **Booleans** (only under N2 or N3, both of which already depart on scope).
   The text gives no form. Each form is marked against "adding to Pt an = ,
   and the value of the corresponding string that xterm would send"
   (ctlseqs 574-575; 8.6). A boolean has no string.
   * `1+r<hex>`, as kitty, foot and ghostty write. **Departs**: it drops the
     `=`, and gives no value. Cost: nothing beyond the name.
   * `1+r<hex>=31`, wezterm's value `1`. **Conforms** on the `=`.
     **Departs** on the value: `1` is a value the text does not define.
     Cost: 3 B more per boolean name.
   * `1+r<hex>=`, xterm's own shape for a valid name with an empty value.
     **Conforms** on the `=`. On the value it **conforms** if a capability
     with no string is read as having the empty string. It **departs** if a
     boolean is read as having no value at all, and on that reading no form
     conforms. Cost: 1 B more per boolean name.
   * Under N2, none of the three moves AG's bound: 96,517 B (R1) or
     96,758 B (R2) (**C**, `bounds.py`). neovim reads the bare form and the
     `=` form alike, as found with no value. It decodes `=31` as the value
     `1`. Any of the three turns its truecolor on (tty.lua 141-149,
     defaults.lua 1017-1025).
5. **XTSETTCAP**, where the text and xterm's code differ (section 3, 8.4).
   The text says the name retrieves data "from the terminal database. If
   successful, that overrides" (ctlseqs 538-542). There are two readings of
   that database:
   * Reading A: the host's database, which xterm's code reads. baud has none
     at run time, so every XTSETTCAP fails, the text's own path; xterm rings
     the bell (xtermcap.c 655-656).
   * Reading B: baud's compiled entries are its database.

   Options 2 and 3 exist only on reading B. On reading A, each reduces to
   option 1.
   1. **Count it and refuse it.** **Conforms** on reading A, where every
      retrieval fails, and matches what xterm's code would change for baud,
      which is nothing. **Departs** on reading B once XTGETTCAP is live,
      because an entry could have been retrieved. Cost: a counter and a row;
      nothing is stored.
   2. **Switch XTGETTCAP's answers among baud's compiled entries**: the ten
      pinned besides the `-x` capture, and the capture. A name with no entry
      is refused and counted.
      * **2a, only `TN` follows the entry.** **Conforms** on reading B. Key
        values stay what baud's keys send, which XTSETTCAP does not change
        outside a tcap keyboard. Cost: one stored index into the pinned
        names. `TN`'s bound stays 14 B (`xterm-256color`).
      * **2b, the key answers follow the entry too.** **Departs** on reading
        B, from the value clause read as baud's key. Each answer is the
        entry's string, and baud's keys often do not send it: baud sends 27
        of rxvt's 77 input capabilities and 10 of vt100's 22 (**M**).
        * Cost: 2a's index, plus the entries' k-prefixed capabilities in the
          core. That is 946 names and 9,228 B over the ten, and 230 names
          and 2,526 B more for the capture (**C**).
        * The longest value is 7 B, so AG's bound holds.
        * An application that binds these waits for bytes that do not come.
   3. **Build `?1050` and XTSETTCAP together.** **Conforms** on reading B
      while `?1050` is set, where the text and xterm's code agree and the
      key answered is the key sent. While it is reset, it takes option 1's
      mark, on whichever reading is taken, or 2a's or 2b's if that variant is
      built too. Cost: a new mode and its row, and an encoder path that sends
      an entry's strings with the normal encoding as fallback. It also needs
      the same 9,228 B, and every key test run under the mode.
6. **Adjacent: DECRQSS's colour spelling.** Both forms conform: the text says
   only "replacing the Pt with the corresponding CSI string" (499-501), and
   its "colons (standard) or semicolons (legacy)" (1287-1289) is about SGR
   input.
   * Keep the semicolon form. It conforms. Cost: neovim's DECRQSS truecolor
     probe splits the answer on `;` and returns without deciding on this
     spelling (defaults.lua 1052-1055), so over ssh without `COLORTERM`
     neovim gets no truecolor from DECRQSS (section 9).
   * Answer in the colon form, as xterm does for 38 and 48. It conforms, and
     only it passes neovim's probe. Cost: one function writes all three
     colours (`dcs.rs` 265-294), and the colon form adds a byte to each
     direct colour, so the richest answer grows from 111 to 114 B, and
     `DECRQSS_MAX`'s three-colour term with it (`dcs.rs` 75); AG's
     never-drops buffer returns to 94,603 B; and N1's limits fall from 32 B
     (R1) and 29 B (R2) to 20 B and 17 B (8.3). D1-D3 and `TN` still fit;
     N1 plus N2 at the spellings read does not, and neither does `Ms` as
     terminfo source text (questions 1 and 3).
