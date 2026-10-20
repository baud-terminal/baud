# AW. The scope calls: printing, the locator, the status line, the keyboard lights, bell volume, the VT525 colours, ReGIS and Tektronix

**Status: 2026-10-20, research; section 2, the locator, was built the same
day (the DECELR rows of `docs/vt-coverage.md`), and the rest is not.** Each section is
**Status: 2026-10-20, research only; section 5, the bell volumes and the
margin bell, was built the same day (its last paragraph).** Each section is
one feature baud reads and drops today: what a user would see if it were
built, what the governing text says, the options, Jev's lean and a
recommendation. Scope is the owner's call (TODO.md). Where Jev reached 0.9 in
both orders the recommendation is Jev's pick as it stands. Below that it is
the lean, said as one. XTSMPOINTER, the pointer-hiding mode, was asked
separately and is not repeated here.

Every option was put to Jev with a render of it beside today's behaviour.
Each request quoted the owner's standing rule of 2026-09-25 verbatim, "full
VT420-class terminal. we need to support such protocols fully, always!". It
also quoted the owner's DA1 pick of 2026-10-15, "Keep ?6c (Recommended)". A
request whose fork touches another owner decision quoted that one as well.
Both orders were asked, uncached, and a fork below 0.9 was asked a second
time with more research. The figures are Jev's confidences, first ask and
then second; the log is lines 1000-1023 of Jev's decisions log.

## Summary

| feature | what a user gets if it is in | recommendation | Jev |
|---|---|---|---|
| Printing (MC) | a program's print request opens the macOS print panel | **in, through the print panel** | 0.86/0.77, then 0.98/0.98 |
| DEC text locator | DEC-style forms and menus answer to a click | **in** | 0.92/0.94 |
| Status line | a program's own status row under the grid | **in**: the window grows a row; both line types (lean) | row 0.98/1.00, then 0.96/0.98; types 0.76/0.71, then 0.41/0.56 |
| DECLL keyboard lights | Num, Caps and Scroll lights drawn on the title bar | **in, drawn** (lean) | split, then 0.91/0.65 |
| Bell volumes | a program can soften or silence the bell, and a margin bell | **in** | 0.96/0.94 |
| DECAC/DECATC (VT525) | a program sets the default and per-attribute colours by index | **out** (lean) | 0.04/0.32, then 0.14/0.42 |
| ReGIS | DEC's vector graphics drawn in the window | **no recommendation**: Jev flips | 0.27/0.43, then IN 0.14 / OUT 0.16 |
| Tektronix 4014 | xterm's second, plotting personality | **out** (lean) | 0.84/0.84 |

## 1. Printing: Media Copy and its modes

**What a user sees.** A program that prints a report, a screen or a stream of
lines opens the macOS print panel. The panel shows what will print and names
the window that sent it, and nothing reaches paper until the user presses
Print. Today the request does nothing and the program is told there is no
printer.

**The text.** Printing is a VT420 feature. `[[source: vt420-rm]]` chapter 11
(pp. 209-215) covers printer controller mode, `CSI 5 i` and `CSI 4 i`: "The
terminal sends characters and control sequences directly to the printer,
without displaying them on the screen". It also covers print page `CSI 0 i`
(the scrolling region unless DECPEX), the composed display `CSI ? 10 i`, all
pages `CSI ? 11 i` and the cursor line `CSI ? 1 i`. Autoprint, `CSI ? 5 i`
and `CSI ? 4 i`, prints a line when the cursor leaves it by LF, FF, VT or an
autowrap. The chapter adds printer-to-host sessions, DECPFF (`?18`) and
DECPEX (`?19`). `[[source: vt510-rm-b]]`'s MC page (5-178) adds `CSI 2 i`,
"Sends screen data through host port". `[[source: dec-std-070]]` 070-7 is the
Printer Port Extension "to the Level 2 and Level 3 Character Cell Display
service class", and 070-1 §1.6.3.4 lists DSR, DECPCM, DECAPM, DECPFF, DECPEX
and MC. `[[source: ctlseqs]]` (1047-1060, 1432-1434) adds xterm's HTML and SVG
dumps, `CSI 10 i` and `CSI 11 i`, and DSR `?15`'s replies, `?10` ready and
`?11` not ready.

**What others do.** xterm pipes MC output to `printerCommand`. Its manual: "a
shell command to which xterm will open a pipe when the first MC (Media Copy)
command is initiated. The default is an empty string ... the printer is
disabled" (`[[source: xterm-man]]` 5154-5159; `[[source: xterm-reset]]`
4747-4757). iTerm2 prints `CSI 0 i` through the macOS print system and
collects `CSI 5 i` ... `CSI 4 i` into a buffer it then prints, yet it answers
DSR `?15` with "no printer" (`[[source: iterm2-7ce037e-vt100terminal]]`
2704-2715 and 1338-1340). kitty and foot send MC to their unknown-sequence
branch (`[[source: kitty-9710eb4-vt-parser-c]]` 1455-1461;
`[[source: foot-csi-c-cb27717]]` 2260-2268).

**Safety.** Under any option two kinds of request stay refused and counted
for safety. `CSI 2 i` would type the screen's text back into the program's
input, the reason the XTWINOPS 20 and 21 title reports are refused. xterm's
file dumps and `?46` logging write files a remote program names. Printer
controller mode is built, not refused: it hides output from the screen until
`CSI 4 i`, as the text says, and the app would mark a run while it collects,
so the screen going quiet is explained.

**Options.**
- OUT: as today. DSR `?15` keeps answering "no printer".
- IN_PRINT_PANEL: on macOS each job opens the print panel
  (`[[source: apple-nsprintoperation-showsprintpanel]]`, "A Boolean value
  that determines whether the print operation displays a print panel").
  - A print is one job; an autoprint run, `?5 i` to `?4 i`, is one; a
    controller run, `5 i` to `4 i`, is one. There is never a panel a line.
  - DSR `?15` answers from whether macOS lists a printer.
  - Printer-to-host stays refused and counted: the VT420's sessions,
    `CSI ? 9 i` and `CSI ? 8 i` (pp. 213-214), and the VT510's `CSI 6 i` and
    `CSI 7 i`, on its MC page. The print panel sends nothing back. This
    refusal is part of the option Jev took at 0.98.
  - Linux waits for a print path.
- IN_COMMAND: xterm's route, a command the user configures, empty by default.

**Building it.**
- Core: the print state (autoprint, controller mode, DECPEX, DECPFF) and the
  text of a screen, page or line handed to the embedder as a job.
- Printer controller mode sends the byte stream to the printer before it
  reaches the grid. That is a change on the per-byte path, so it needs
  CLAUDE.md's measurement of write amplification and working set.
- macOS app: the print panel. Linux app: no print path, so DSR `?15` keeps
  answering "no printer" there.

**Jev.** IN_PRINT_PANEL, 0.86 and 0.77 on the first ask. With a job defined
and the `?15` answer made concrete, the second ask gave 0.98 and 0.98.
IN_COMMAND's probability was 0.03 at most.

**Recommendation: in, through the print panel.** This is the one option that
builds the whole VT420 printer and still prints nothing without the user.

## 2. DEC's text locator: DECELR, DECSLE, DECEFR, DECRQLP

**What a user sees.** A forms or menu program written for DEC terminals
answers to a click: it gets the button, the cell (or pixel) and the page,
in order with the keys typed. Today the click goes to baud's selection, and
a program that asks is told there is no locator.

**The text.** `[[source: dec-std-070]]` 070-13, the Text Locator Extension,
is "support for locator or pointing devices on ANSI text and graphic
terminals". "Both can be implemented at any conformance level." Primary DA
reports it as parameter 29, which baud never sends: DA1 stays `?6c`. "Each
locator report shall include: (1) the specific event that initiated the
report; (2) the current state of the locator keys; and (3) the coordinates
of the input cursor", and reports queue with keystrokes in strict order
(070-13 §1, §1.2, §3). `[[source: ctlseqs]]` gives the controls and their
values:
- DECELR, `CSI Ps ; Pu ' z`, 1842-1853: off, on or one-shot, in cells or
  pixels.
- DECSLE, `CSI Pm ' {`, 1859-1867: report button down and up.
- DECEFR, `CSI Pt ; Pl ; Pb ; Pr ' w`, 1776-1786: a one-shot filter
  rectangle.
- DECRQLP, `CSI Ps ' |`, and the report `CSI Pe ; Pb ; Pr ; Pc ; Pp & w`,
  1907-1934.
- DSR `?55` answers "CSI ? 5 0 n Locator available ... or CSI ? 5 3 n No
  Locator"; `?56` "CSI ? 5 7 ; 1 n Mouse" (1445-1450).

**What others do.** xterm builds all of it (`[[source: xterm-reset]]`
5518-5596). iTerm2, kitty and foot do not. iTerm2 answers `?55` with `50`,
which ctlseqs reads as "Locator available", under a comment saying it is
unimplemented (`[[source: iterm2-7ce037e-vt100terminal]]` 1356-1367); the
reply it sends is `CSI ? 50 n` (`[[source: iterm2-7ce037e-vt100output]]`
1480-1482).

**Safety.** None beyond the mouse modes baud already has. It reports the
pointer only while a program has asked.

**What changes besides the controls.** DSR `?55` and `?56` answer "available"
and "mouse". xterm's feature queries (`OSC 60` to `62`) name the DEC locator
as honoured. A program finds
the locator through those, since DA1 does not name it. Linux waits on its
pointer.

**Building it.** Core: DECELR, DECSLE and DECEFR state and the DECLRP
report, beside the mouse reports `Term` already encodes. macOS app: the
pointer events it already takes. Linux app: when its pointer input lands.

**Jev.** IN, 0.92 and 0.94.

**Recommendation: in.**

**Built 2026-10-20**, in the core and both apps. Five forks the text left
open went to Jev: DECOM in pixel units counts from the text area
(PIXELS_ABSOLUTE, 0.96/0.94), DECELR 0 turns off any mouse protocol
(ALL_OFF, 0.92/0.92), the fourth button is the first past three
(FIRST_EXTRA, 0.98/0.98), a reversed rectangle is taken as given (lean
AS_GIVEN, 0.89/0.85 on the second ask), and the wheel scrolls as with no
protocol (lean AS_UNTRACKED, 0.53/0.38 on the second ask).

## 3. The status line: DECSSDT and DECSASD

**What a user sees.** A program can open one more row under the grid that is
its own, for mail counts, a clock or a build state. The window grows to hold
it, and the program's screen keeps every row. In the indicator type the row
shows baud's own state in reverse video. Today the program's status text goes
nowhere.

**The text.** `[[source: vt420-rm]]` pp. 219-221, a VT420 feature: "The line
below the last line of the user window is reserved for the status line ...
as an indicator of the terminal's current state, or as a window the host can
use to display application-specific messages". The indicator shows "Page
number, Text cursor position (line, column), Printer status, Modem status".
DECSASD selects 0 main or 1 status line. DECSSDT selects 0 none, 1 indicator
or 2 host-writable. Only column parameters position the cursor on the line;
DECSTR and DECSCL exit it, and RIS "Erases and exits the status line".
`[[source: dec-std-070]]` 070-14 calls it "a Level 3 feature (Level 2
extension)" (§14.1, §14.2, pp. 14-4 and 14-5). It lists the types as No
Status Line (factory default), Indicator ("not mandatory for workstation or PC terminal emulators") and
Host Writable. "On a windowed display, the size of the terminal window may
change to include the status line."

**Sources that conflict.** For an omitted DECSSDT parameter, the VT420 manual
marks 0 "(default)" and `[[source: ctlseqs]]` (2001-2005) marks 1. At power-on,
070-14 has no status line and the VT420 has the indicator. Which one baud
takes is an open fork for the build. The second ask put 070-14's to Jev, under
which nothing changes for anyone until a program asks.

The two texts also set opposite rules for what the line shares with the main
display. The VT420 (p. 221): with the host-writable line selected, "most of
the control functions that affect the main display also affect the status
line", and a list of exceptions follows. 070-14 (§14.2, p. 14-4): "In
general, controls which affect some portion of the Main Display do not affect
the contents or the ANSI state of the Status Display", with its exceptions
noted in that section. Which text governs is an open fork for the build; it
was not put to Jev.

**What others do.** xterm builds it under its StatusLine window operation,
"Resize window to provide a VT320-style status line" (`[[source: xterm-man]]`
3588-3589). Its default `disallowedWindowOps` does not list StatusLine
(`[[source: xterm-main-h]]` 150-175). Its indicator text is a resource,
version, position and time by default (4224-4231). iTerm2, kitty and foot
have none.

**Options.**
- OUT.
- HOST_WRITABLE: types 0 and 2, with DECSSDT 1 refused, as 070-14 permits.
- BOTH: the two types, and the indicator showing what is true of baud, which
  is page 1, the cursor's line and column, and the printer's state. It shows
  no modem field, since a pty has no modem.

A second fork is where the row comes from: the window grows a row, or the
grid gives one up. Window operations a program asks for are on by default,
the owner's decision of 2026-10-19 ("4 it is"). Where the window cannot grow
(full screen, or window operations turned off) the grid gives up its last
row.

**Building it.** 070-14 calls the host-writable line "a permanent separate
logical display, that contains its own ANSI text state and settings". So
the core gains a second, one-row display with its own cursor and state,
DECSASD routing writes to it, and the exceptions of whichever text governs
(above). All three
render paths draw a row outside the grid, and both apps grow the window.

**What changes.** DECRQSS answers `$ ~` and `$ }` as valid. xterm's manual
names StatusLine among its window operations; baud's `OSC 62` list does not
carry the name today and would gain it.

**Jev.** On the row, GROW_WINDOW: 0.98 and 1.00, then 0.96 and 0.98. On the
types, BOTH leads: 0.76 and 0.71, then 0.41 and 0.56. HOST_WRITABLE's
probability was 0.10-0.24.

**Recommendation: in.** The window grows a row, Jev's pick at 0.9 in both
orders, if the owner takes the line in. Both types is the lean, below the
bar. Refusing the indicator would leave a VT420
control value unbuilt, which the standing rule asks against. 070-14's "not
mandatory" permits leaving it out; it does not ask for that.

## 4. DECLL: the keyboard lights

**What a user sees.** A program that lights the keyboard's LEDs to show its
state gets three small lights on baud's title bar, Num, Caps and Scroll,
beside the lights note AH designs. The keyboard's own Caps Lock light is
never touched. Today nothing lights.

**The text.** `[[source: vt100-ug-ch3]]`: "Load the four programmable LEDs
on the keyboard", 0 clearing L1-L4 and 1-4 lighting one, with no gate.
`[[source: vt510-rm-b]]` p. 5-70 has three LEDs, Num, Caps and Scroll Lock:
0 all off, 1-3 on, 21-23 off. It adds: "DECLL controls keyboard LEDs
independently of any keyboard state. The use of LEDs for this purpose
conflicts with their use as keyboard state indicators ... For DECLL to
function, DECKLHIM must be set." DECKLHIM is `CSI ? 110 h`, reset by default
(p. 5-64). The VT420 manual has no DECLL. `[[source: ctlseqs]]` (1536-1543)
lists it as "VT100" with the VT510's three LEDs and no gate.

**Sources that conflict.** The VT100 lights on every DECLL; the VT510 only
under DECKLHIM. xterm's code passes the same `True` for 21-23 as for 1-3,
`xtermShowLED(screen, (Cardinal) (op - 20), True)`, where ctlseqs says
"Extinguish" (`[[source: xterm-reset]]` 5009-5043). The function's
definition is outside the pinned files, so whether that lights the LED is
unverified. iTerm2,
kitty and foot have none; kitty reports a bare `q` as unknown
(`[[source: kitty-9710eb4-vt-parser-c]]` 1418).

**Options.**
- OUT.
- DRAWN: every DECLL acts, as the VT100 and xterm; `?110` is not recognised.
- DRAWN_GATED: build `?110` too, so DECLL acts only once a program sets it,
  as the VT510.
- KEYBOARD: drive the physical LEDs. A Caps Lock light that does not match
  Caps Lock misleads the user about what they type.

**Building it.** Core: three lights and an accessor. macOS app: indicators
on the title bar, in the owner's window crate, where note AH plans its
lights. Linux app: it waits, as note AH's lights do.

**Jev.** First ask split: DRAWN led one order and DRAWN_GATED the other, at
confidences 0.31 and 0.27. With the gate's reason made explicit (the VT510
gates DECLL because its LEDs are also the lock lights, which drawn lights are
not), DRAWN led both orders, at confidences 0.91 and 0.65.

**Recommendation: in, drawn, as the VT100 does.** This is the lean, below the
bar in one order. It shares the title bar with note AH's lights, which are
not built.

## 5. Bell volumes: DECSWBV and DECSMBV

**What a user sees.** A program, or a line in a shell's start-up file, can
make the bell softer or silent. A margin bell, off unless a program turns it
up, can ring as the cursor nears the right margin. Today the bell always
rings at the system's volume and there is no margin bell.

**The text.** `[[source: vt510-rm-b]]` and `[[source: vt520-rm]]` give the
values. DECSWBV, `CSI Ps SP t`, is the warning bell "for errors and receipt of
Ctrl G": 1 off, 2-4 low, and "5, 5, 7, 8, 0 or none" high, the default (the
repeated 5 is the manuals' own). DECSMBV, `CSI Ps SP u`: "1, 0 or none Off
(default)", 2-4 low, 5-8 high. The VT520 describes it as the margin bell
"which sounds as the current position nears the right margin"; the VT510 calls
it the warning bell there. The VT420 manual has neither.

**Sources that conflict.** `[[source: ctlseqs]]` (1720-1743) and xterm's code
read DECSWBV 0 as off and DECSMBV 0 as high (`[[source: xterm-reset]]`
6428-6481), the reverse of both DEC manuals. Which reading baud takes is an
open fork for the build, for Jev; by baud's rule the DEC text is the
requirement and xterm is practice.

**What others do.** xterm scales its bell by the stored volume
(`[[source: xterm-misc]]` 1347-1392). It rings a margin bell when the user
types near the right margin, off by default (`marginBell`,
`[[source: xterm-man]]` 4449-4452). iTerm2, kitty and foot have neither.

**The owner's bell and the volume.** The macOS app rings with NSBeep, "Plays the system beep"
(`[[source: apple-nsbeep]]`), which takes no volume. This is the owner's bell
decision of 2026-10-13, "System sound + Dock bounce (Recommended)". "Low" can
be built by playing the user's alert sound through NSSound at a lower volume,
whose value "does not affect the systemwide volume"
(`[[source: apple-nssound-volume]]`). Building only "off" would leave the
protocol half built. Linux rings nothing yet, and the volumes wait on it.

**Building it.** Core: the two volumes and the margin bell's trigger, with
accessors. macOS app: the bell decision in the app takes a volume, NSBeep
for high and NSSound for low. Linux app: it rings nothing yet.

**Jev.** IN, 0.96 and 0.94.

**Recommendation: in**, both volumes and the margin bell, on macOS first.
What rings the margin bell is a fork for the build: the VT520 says "as the
current position nears the right margin", and xterm rings it on typed keys.
Jev's 0.96 and 0.94 were given for an IN whose margin bell rings "as typed
text nears the right margin", xterm's trigger. The request quoted the VT520's
text but did not offer its reading as a choice.

**Built 2026-10-20 (lane b118).** DEC STD 070 settles the trigger: in
070-6 § 6.6.5.2 (pp. 6-45 and 6-46, `[[source: dec-std-070]]`) a normal
keystroke arms the margin bell and a character printed across RIGHT
MARGIN - 8 on the same line rings it, so xterm's key-time test and the VT520's
"current position" are each half of one rule. Jev decided 0 by DEC (1.00
and 1.00), the trigger STD070 (0.94 and 0.95), eight columns (0.97 and
0.97) and the new-line test at the write (0.94 and 0.94); the leans taken
were TWO_SWITCHES for `?44` (0.37/0.27, then 0.27/0.14), the DECRQSS
values 1, 2 and 5 (0.18/0.58, then 0.12/0.56) and low sounding as high on
macOS (0.61/0.70, then 0.69/0.67), where no documented call plays the
user's alert sound below its volume (`[[source: apple-nssound]]`,
`[[source: apple-audioservicesplaysystemsound]]`). Log lines 1036-1037,
1042-1045. The rows are `CSI Ps SP t`, `CSI Ps SP u` and `?44` in
`docs/vt-coverage.md`.

## 6. The VT525's colour assignments: DECAC, DECATC and DECSTGLT

**What a user sees.** A VT525 program sets the screen's default colours, and
a colour for each attribute combination, by palette index. Today the user's
theme stays as it is.

**The text.** `[[source: vt520-rm]]` (pp. 5-16, 5-22, 5-23, 5-151), VT525
only. DECAC "selects the foreground and background colors used for normal
text in ANSI SGR color mode and the window frame colors". DECATC "Selects the
colors used for text attribute combinations", sixteen of bold, reverse,
underline and blink. DECSTGLT selects mono (0), alternate colour (1, 2) or
ANSI SGR colour (3, the default), and DECATC's colours show only under 1 or 2.
DECATCBM (`?115`) and DECATCUM (`?114`) say whether blink and underline
still blink and underline there. Each is reportable by DECRQSS or DECRQM. The
VT420 and VT510 manuals have none of it. `[[source: ctlseqs]]` 1954-1984.

**What others do.** xterm acts on DECAC's normal-text pair and stores DECATC
only when its terminal ID is 525 or more (`[[source: xterm-reset]]`
5936-5985). Its default identity is set at build time: the manual gives its
default as a placeholder filled in when xterm is built (`[[source: xterm-man]]`
3289-3294). The second ask told Jev it is a VT420, which the pinned files do
not show, so the 0.14/0.42 lean rests partly on an unpinned premise. iTerm2,
kitty and foot have none.

**What building it means.** DECATC without DECSTGLT and the two modes would
be a mode recorded and not acted on. So in is all five: DECAC, DECATC,
DECSTGLT, `?114` and `?115`. DECSTGLT's mono and alternate-colour maps are a
second colour model for every SGR colour.

**Building it.** Core: the default pair, sixteen attribute pairs, the map
and the two modes, each answered by DECRQSS or DECRQM. The renderer resolves
every cell's colour through the selected map on all three paths, so both
apps follow.

**Jev.** OUT, 0.04 and 0.32. The second ask added that baud already builds
VT520 controls (DECSCUSR, and the bell volumes above), so "VT420-class" has
been read as a floor. OUT again, 0.14 and 0.42. Every ask described IN as
"all four controls and both modes"; it is three controls and two modes, the
five above.

**Recommendation: out**, the lean, below the bar. This agrees with note AV,
which leaves it to the DA-level question (note AP). DECRQSS answering invalid
is truthful meanwhile.

## 7. ReGIS and Tektronix 4014

**What a user sees.** A plotting program's vector output draws in the window:
lines, curves, fills and text, as a Sixel image draws today. Today the bytes
are read and dropped.

**The text.** `[[source: dec-std-070]]` 070-8, the ReGIS Graphics Extension,
§8.1.3: "It is not the intent of this standard to specify what products,
hardware or software, must implement ReGIS. The decision to support ReGIS or
not will be made by the appropriate marketing and engineering organizations".
070-1 §1.6.3.5 lists its instructions: screen, position, writing, vector,
curve, text, report, load character set and macrograph. ReGIS was on the
VT125, VT240 and VT330/340, beside Sixel, which baud built; the VT420 has
neither. Tektronix 4014 is no DEC text. `[[source: ctlseqs]]` (3245-3300)
documents xterm's: "Most of these sequences are standard Tektronix 4014
control sequences", without the write-through and defocused modes.

**Safety.** A ReGIS macrograph report sends stored text back to the host,
and 070-8 §8.5.7.4.1 says so itself: "unrestricted allowance for reporting
of macrograph contents will sometimes result in system security problems.
Devices are therefore allowed the freedom to restrict macrograph reports. The
preferred fallback is the empty report". An in build would take that
fallback.

**What others do.** xterm supports ReGIS only when configured as a VT125,
VT240, VT241, VT330 or VT340. Its Tektronix mode opens a second window with
its own emulator and graphics-input mode (`[[source: xterm-man]]`, `tekInhibit`
5581). iTerm2 answers only XTSMGRAPHICS's ReGIS geometry item
(`[[source: iterm2-7ce037e-vt100terminal]]` 5434). kitty and foot have
neither.

**Building it.** ReGIS needs a parser and a vector rasteriser in the core,
drawing into the image store as Sixel does. The three render paths already
draw images, so both apps follow. Tektronix needs a surface of its own.

**Options and Jev.** The first ask offered OUT, ReGIS only, or both, and gave
OUT at 0.27 and 0.43. The second ask split the fork. On ReGIS Jev flipped:
IN 0.14 in one order and OUT 0.16 in the other. On Tektronix it gave OUT
0.84 and 0.84.

**Recommendation.** No recommendation on ReGIS, which is an owner question.
In builds a second graphics language whole, to 070-8; out writes 070-8's own
"not the intent" as the reason. Tektronix: out, the lean, since it is a
second terminal personality with no DEC text.

## Found and not fixed

- The out-of-scope row for ReGIS and Tektronix in `docs/vt-coverage.md` gives
  "Historical; no modern application emits them". Nothing here checked that,
  and gnuplot is the obvious program to check. The row should carry 070-8's
  "not the intent" and the owner's call once made.
- DECSWBV and DECSMBV: ctlseqs and xterm reverse the DEC manuals' reading of
  0, and the VT510's DECSMBV page names the warning bell (section 5). Built
  by DEC's reading. The VT520's own Section 2.12.7 calls high the default
  of all three volumes, against its DECSMBV page, its Table 4.7.3 and its
  Set-Up's "Margin bell volume Off": two to one, and off is built.
- DECLL 21-23: xterm's code passes the value it passes to light, where its
  document says extinguish; what that does is unverified (section 4).
- iTerm2 answers the locator status query "available" with no locator
  (section 2).
- baud's `OSC 62` window-operation list omits xterm's StatusLine name, which
  is truthful while there is no status line (section 3).
