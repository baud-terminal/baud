# AL. The cursor trails' flashes: what the texts require, and four ways to bound them

**Status: research, 2026-10-09. Nothing is decided and nothing here is built
into the shipped trail; four prototypes were built on branches, not merged, to
be measured.** (Built since, 2026-10-17: O2, with section 6's
harness first. A repeat pass drops its particles and reads its cap from
the cells under it, text included, and the CPU path's quad takes it: 0
FAIL on the full grid at 1x and 2x: a trail's repeat pass is drawn under the flash thresholds. Since, the same day: the ghost was
counted, failed at 72 pt and takes the same bound, and a selection, the
cursor and the desktop under the dimmed glass are grounds the cap reads) The open questions are in section 8, each with a visual; the frames staged with this note (section 4) are the material.

**The letter.** AL is this note's letter; the XTGETTCAP draft takes AM, and
no key here collides with that draft's.

**Base.** Line numbers in baud's code are at `6541bdf`, where the census and
the four prototypes were made, except where a line says otherwise. HEAD is
`4911bf0`. Between the two, `baud-app/src/trail.rs`, `baud-app/src/config.rs`,
`baud-app/src/bell.rs`, `baud-render/src/` and `MANIFEST.toml` are unchanged.
`baud-app/src/app.rs` changed in `f716c6a` (+71/-4) and `54b7f5f` (+2/-2),
all below its line 881, so its lines up to 881 read the same at HEAD and
later ones sit six lower there. `baud-app/src/linux.rs` changed in both
commits too, and the statement that the Linux driver draws no trail is
quoted at `4911bf0`.

**Where it comes from.** Three read-only investigations (the governing
texts, prior art, and an analysis of baud's code with a CPU replica of the
census), a design of four options, and four prototypes, each a commit on a
branch of its own, run through the shipped census harness on this Mac's GPU:

| option | commit |
|---|---|
| O1 geometry | `f5725ab` |
| O2 intensity | `4ccd622` |
| O3 temporal | `dc2fce3` |
| O4 setting and floor | `43b72d7` (and `dd56cd3` in the glass crate, branch `proto/flash-o4-dim-flashing`) |

None is pushed. Their census files, analysis scripts and 19 frames are staged
for `docs/research/sources/al-observed/` as the `al-obs-*` entries, with the
gates recomputed from their raw runs ([[source: al-obs-gates]]).

**Evidence tags.** **M** measured, with the file it is in; **C** computed
here from measured inputs or from the code; **R** this note's reading of a
text, not the text; **U** unknown or unverified. Every quotation of 24
characters or more was found by this note's checker (kept with the untracked evidence,
`note/check_draft.py`) in the text of the sources,
and the file each was found in was read against the key cited beside it;
the shorter ones were checked by hand. Test counts are as the prototypes
reported them.

**The reading every verdict here is on.** The census pairs transitions into
flashes disjointly, so a pixel needs eight transitions in a second to flash
"more than three" times. Every FAIL, pass and "conforms" below is on that
reading unless it says strict. Two other counts disagree with it (section
1.8, item 7): G19's, under which seven transitions fail, and the chained
(strict) reading, under which five do.

## 0. In plain words

- **What fails today.** At the default 13 pt every trail passes WCAG 2.3.1 in
  every case counted. At 32 pt the fire, the shipped default trail at this
  note's date, and the orb fail when the cursor jumps back and forth on the
  light theme's page. At 72 pt, the View menu's zoom limit, five effects
  fail, and so do the plain quad and the sweep (**M**, section 3). The
  plasma, which became the default on 2026-09-29 and which has
  shipped as the default since 2026-10-10, passes at 32 pt (92% of the
  limit) and fails at 72 pt. A configured font size can go above 72 pt,
  and no size above 72 pt was counted.
- **Why.** `REIGNITE` limits how often an effect is lit, not how often the
  same pixels light up. Each jump inside it sweeps the burning effect back
  over the same band, so one pixel can flash many times in one second: 15
  for the fire at 32 pt, and up to 23 in the census, for the liquid at 32 pt
  2x, 120 Hz (**M**). What keeps 13 pt inside the rule is the
  flashing area, which grows with the font.
- **What the texts allow.** 2.3.1 passes flashing if it is rare (at most
  three flashes in any second), or small (at most a quarter of a 10-degree
  field), or neither a general flash (a luminance change of 0.10 or more
  with the darker state below 0.80) nor a red flash. Only the area test
  depends on the size of what is drawn, and which sizes the test must be run
  at is not settled by any text (section 1.2).
- **Four options were prototyped.** O1 caps every trail's size; O2 draws
  repeat passes too faint to flash; O3 lights an effect at most once a second
  above a 32 pt cell; O4 draws no trail above that cell and follows macOS's Dim
  Flashing Lights. **On the grids each ran, up to 72 pt, none fails 2.3.1 on
  the disjoint reading** (**M**). Each leaves something: O1 leaves a pixel
  flashing as often as the shipped driver (18 a second at 32 pt against 17)
  and changes the look from 21 pt (20 pt at 2x); O2 failed the pixel check
  that backs its construction; O3 still fails the strict reading in 54
  cases at 72 pt; O4 draws no trail of any kind above 24 pt and needs a release of the window crate. No option meets 2.3.2 (Level AAA),
  and the grids differ. The table is in section 4.6.
- **What the options leave out.** The texts take the whole program as the unit,
  and baud flashes in other ways: the visual bell, `SGR 5` blink, the
  alternate-screen fade and the ghost trail, which only O4 touches (section
  3.4). Each option bounds the counted trails only.
- **Nothing is chosen.** Which option, for which effects, which sizes govern,
  which reading governs, whether a system setting governs, and whether a bound may override a trail the config names are open (section 8).

## 1. What the texts say

### 1.1 WCAG 2.2, 2.3.1: the rule, and which of its words bind

[[source: wcag22]], SC 2.3.1 Three Flashes or Below Threshold (Level A):
"Web pages do not contain anything that flashes more than three times in any
one second period, or the flash is below the general flash and red flash
thresholds."

The definition it names, its normative body:

> "a flash or rapidly changing image sequence is below the threshold (i.e.,
> content passes) if any of the following are true: there are no more than
> three general flashes and / or no more than three red flashes within any
> one-second period; or the combined area of flashes occurring concurrently
> occupies no more than a total of .006 steradians within any 10 degree
> visual field on the screen (25% of any 10 degree visual field on the
> screen) at typical viewing distance where: A general flash is defined as a
> pair of opposing changes in relative luminance of 10% or more of the
> maximum relative luminance (1.0) where the relative luminance of the darker
> image is below 0.80; and where 'a pair of opposing changes' is an increase
> followed by a decrease, or a decrease followed by an increase, and A red
> flash is defined as any pair of opposing transitions involving a saturated
> red"

So flashing passes by one of three routes (**R**): **frequency** (at most
three general and at most three red flashes in any second), **area** (at
most .006 sr), or **neither kind of flash** (a change under 0.10, or one
whose darker state is at or above 0.80, is not a general flash, and a change
that involves no saturated red is not a red flash). The definition's
Exception adds a fourth for one pattern: "Flashing that is a fine, balanced,
pattern such as white noise or an alternating checkerboard pattern" with
squares under 0.1 degree; a trail is not that pattern (**R**).

**The notes are informative.** [[source: wcag22]] § 5.1: "Introductory
material, appendices, sections marked as 'non-normative', diagrams, examples,
and notes are informative (non-normative)." So the definition's Note 1 ("using
a 341 x 256 pixel rectangle anywhere on the displayed screen area when the
content is viewed at 1024 x 768 pixels will provide a good estimate of a 10
degree visual field") and Note 3's working definition of a saturated red are
estimates; the binding text is ".006 steradians", "25% of any 10 degree
visual field". A quarter of 341 x 256 is 21,824 (**C**), the figure the
census fails a case above, and a quarter of a 10-degree circular field is
0.00598 sr (**C**), which matches ".006 steradians".

**Software.** [[source: wcag2ict-22]] § 6.2.4.2.1 says of 2.3.1 "This applies
directly as written", and its added Note 3: "This requirement applies to
those visual elements produced by the ICT itself." A trail baud draws is one
(**R**). It also says "the unit of evaluation for non-web software is the
whole software program". It is itself informative, and says of AAA: "This
document does not provide guidance on applying AAA success criteria to
non-web ICT".

### 1.2 The size at which the area test is run

[[source: wcag22-understanding-three-flashes]] (informative): "Content should
be analyzed at the largest scale at which a user may view the content, and at
the standard zoom level of the user agent." The sentence asks for both, not
one or the other, and the next one gives the case: "For example, with a
video that may play in an area of a web page and also at full screen, the
video should be analyzed for risks at full screen." Its note concedes the
gap: "Users that are closer to their screens than the idealized viewing
distance will be affected by flashing areas that normatively pass. The same
problem applies to users who rely on zoom or screen magnification."

**The largest scale a baud user may view is not 72 pt** (**C**, from the
code at `6541bdf`). `ZOOM_MAX`, 72 pt, bounds only the View menu's zoom
(`app.rs` 5191, applied in `zoom` at 5858). The config's `font_size` is
parsed with no bound (`config.rs` 506-507), and the renderer is built at it
through `PxSize::of` (`app.rs` 690), whose only bound is 512 px per em
(`baud-render/src/metrics.rs` 13): 512 pt at 1x, 256 pt at 2x. So
`font_size = 100` launches at 100 pt with no zoom at all.

What that means for the census rows (**R**): at 13 pt nothing fails, at 72
pt seven trails do, and above 72 pt nothing was counted, under any option.
Every option in section 4 was built to pass at 72 pt and none was measured
above it. Above about 118 pt even the bare cursor fails the area test when a
program jumps it between two adjacent cells, under every option, O4
included (section 3.1). Whether 2.3.1 must hold at a configured size, at a
zoomed one, or only at the default is not settled by the text, and the
answer decides how far any option must reach.

The same page on turning a flash off: "turning the flash off is also not an
option since the seizure could occur faster than most users could turn it
off." So a setting cannot be the only remedy at Level A (**R**).

### 1.3 2.3.2 (Level AAA): no area clause

[[source: wcag22]], SC 2.3.2 Three Flashes: "Web pages do not contain
anything that flashes more than three times in any one second period." Its
Understanding page, [[source: wcag22-understanding-three-flashes-aaa]]:
"this criterion does not allow any flashing that occurs at a frequency
greater than 3 per second, regardless of brightness or size. As a result,
even a single flashing pixel would violate this criterion."

Note AJ section 1.3 reads AAA as guidance for baud, and WCAG2ICT gives none.
Whether 2.3.2 is a goal is open (section 8). Under it, a block
cursor that a program jumps between two cells every 100 ms already flashes
those cells 5 times a second with no trail at all (**M**, every off arm in
section 4), if a jump the program makes counts as a visual element
"produced by the ICT itself" (**R**).

### 1.4 The three ways W3C accepts for 2.3.1

All informative.

- **G19**, [[source: wcag22-technique-g19]]: "Since some users may be using
  screen enlargers, this technique limits the flashing of any size content
  to no more than three flashes in any 1-second period." It counts flashes
  in half-flashes: "if there are seven transitions from dark to light or
  light to dark, it would be 3.5 flashes, which is more than the allowed
  three flashes (six transitions)", and says it can be used for Level A
  "because all failure thresholds in the Level A success criteria involve
  flashing 3.5 flashes or more within one second".
- **G176**, [[source: wcag22-technique-g176]]: "any single flashing event on
  a screen (there is no other flashing on screen) that is smaller than a
  contiguous area of 21,824 sq pixels (any shape), would pass the General and
  Red Flash Thresholds." And: "To address the needs of this group, G19 should
  be used since it is independent of screen resolution or viewing distance",
  the group being users who "enlarge or view their screens closely".
- **G15**, [[source: wcag22-technique-g15]]: test with a tool; its example's
  fix is that "The animation is modified to create a short pause after each
  pair of lightning flashes."

### 1.5 EN 301 549: 2.3.1 as a "shall", and a default mode

[[source: etsi-en-301549-v4-1-1]] (V4.1.1, 2026-09), clause 11.2.3.1, for
software with a user interface: "Non-web software does not contain anything
that flashes more than three times in any one second period, or the flash is
below the general flash and red flash thresholds."

Clause 4.2.9, new in its wording: "the ICT shall provide at least one mode of
operation, that is the default mode, that minimizes the potential for
triggering photosensitive seizures". Its NOTE 2 names the levers: "Limiting
the area and number of flashes per second may contribute towards meeting
this clause." The previous version, [[source: etsi-en-301549-v3-2-1]], had
neither the "shall" nor "that is the default mode".

What this does and does not settle (**R**):
- The shipped default, the fire at 13 pt, passes 2.3.1 in every case
  counted, and so does the decided default, the plasma, at 13 pt (section
  3.1).
- Whether "minimizes" asks for more than passing 2.3.1, up to no trail by
  default, the text does not say.
- **U:** whether the standard has been cited in the Official Journal yet (its
  foreword: "Once the present document is cited in the Official Journal of
  the European Union under that Directive, compliance ... confers ... a
  presumption of conformity"), and whether a terminal emulator falls in the
  European Accessibility Act's scope. Neither was checked.

### 1.6 Broadcast: the source of the thresholds, and a spacing rule

[[source: itu-r-bt1702-3]] (in force per [[source: itu-r-bt1702-index]]),
Annex 1, Guideline 1:
- "Irrespective of luminance, a transition to or from a saturated red is also
  potentially harmful."
- Failure needs both more than 25% of the screen and more than three flashes
  a second, and "successive flashes for which the leading edges are separated
  by 360 ms or more are acceptable in a 50 Hz environment or separated by 334
  ms or more are acceptable in a 60 Hz environment, irrespective of their
  brightness or screen area."
- "a sequence of flashing images lasting more than five seconds might
  constitute a risk even when it complies with the guidelines above."

[[source: ofcom-section-2-guidance-notes]], Annex 1, 3.1.1, gives the same
rule in frames: "successive flashes for which the leading edges are
separated by 9 frames or more are acceptable, irrespective of their
brightness or screen area." Neither Ofcom text states a frame rate; at 25
frames a second, nine frames is 360 ms, BT.1702's 50 Hz figure (**C**, on
an assumed rate).

**`REIGNITE` is 334 ms, BT.1702's 60 Hz figure.** `trail.rs` 65-67: "The
least time between two ignitions of an effect, so it lights at most three
times a second." No tracked file outside the sources cites BT.1702 (the
standards research's `git grep`), and 334 is also the least whole millisecond
above 1000/3 (**C**), so the match may be arithmetic. The difference that
matters: BT.1702 spaces the leading edges of **flashes** at a place on the
screen; `REIGNITE` spaces **ignitions** (section 3.2).

Where the 0.006 sr comes from: the Epilepsy Foundation's expert consensus,
whose abstract ([[source: efa-2005-consensus-pubmed]]) reads "A flash is a
potential hazard if it has luminance >or=20 cd/m2, occurs at a frequency of
>or=3 Hz, and occupies a solid visual angle of >or=0.006 steradians". Its
full text, ISO 9241-391's full text (only its catalogue page,
[[source: iso-9241-391-2016-catalogue]], was read) and Ofcom's Broadcasting
Code were not obtained; they are proposed as `gated`, and nothing here rests
on them.

### 1.7 The operating systems' settings

**Apple, Dim Flashing Lights.** The SDK header,
[[source: macos-sdk-mavideoaccommodations-h]] line 5, scopes it to video:
"Access to user preference for desire to dim flashing or strobing lights in
video content." The documentation's article,
[[source: apple-mediaaccessibility-flashing-lights]], reaches further: "If you
have an animation sequence with flashing lights in a game or app that you can
control programmatically, adjust the animation accordingly." It also says to
"Check the value of the Dim Flashing Lights setting at any time by using"
`MADimFlashingLightsEnabled()`, which exists from macOS 13.3
([[source: apple-madimflashinglightsenabled]]).

Two things about reading it are untested (**U**):
- The header says its change notification is a "CFNotification sent to local
  notification center"; the sample code
  ([[source: apple-responding-to-flashing-lights-setting]]) registers on
  `NotificationCenter.default`. Which one posts on macOS was not tested, nor
  whether the two are one object.
- Whether a poll sees a live toggle without observing either notification.
  The article's "at any time" supports polling, but it is not a test, and
  O4's own doc on `poll_motion` (`43b72d7`, `app.rs` 5476-5479) leaves it
  unverified, since MediaAccessibility may cache the value until the
  notification is delivered. O4's live check read the setting; nothing
  toggled it (section 4.4).

`MAFlashingLightsProcessor` ([[source: apple-maflashinglightsprocessor]],
macOS 14) dims flashing it detects in a frame's surface, documented for
"custom video drawing"; whether it accepts baud's frames is unmeasured.

**Apple's HIG puts Dim Flashing Lights under video and blinking animation
under Reduce Motion**, [[source: apple-hig-accessibility]]: "Allow people to
opt out of flashing lights in video playback." And: "Be cautious with
fast-moving and blinking animations. When you use these effects in excess,
it can be distracting, cause dizziness, and in some cases even result in
epileptic episodes. People who are prone to these effects can turn on the
Reduce Motion accessibility setting."

**Apple, Reduce Bright Effects** (`AXReduceHighlightingEffectsEnabled`, macOS
26.4): its SwiftUI page,
[[source: apple-swiftui-accessibilityreducehighlightingeffects]], asks that
"controls, such as buttons, should be drawn in such a way that minimizes
highlighting and flashing of onscreen elements". No label for it was found in
this Mac's System Settings strings
([[source: macos-27-a11y-settings-loctable]]); that is a search that found
nothing, not proof the setting is absent.

**Microsoft** names photosensitivity under its animation flag,
[[source: ms-systemparametersinfow-656e965]]: "Display features such as
flashing, blinking, flickering, and moving content can cause seizures in
users with photo-sensitive epilepsy."

**Linux** has no Dim Flashing Lights counterpart that the prior-art research
found: GNOME's animation keys and the desktop portal's `reduced-motion` speak
only of motion ([[source: gnome-a11y-interface-schema-2417a27]],
[[source: xdg-portal-settings]]). The Linux driver draws no trail today
(`baud-app/src/linux.rs` 25-27 at `4911bf0`).

### 1.8 Where the texts disagree

1. **Area: 21,824 or 87,296.** The normative definition gives "25% of any 10
   degree visual field" and no pixels. Its informative Note 1 and G176 put
   that field at 341 x 256 pixels, a quarter of which is 21,824;
   [[source: wcag22-understanding-three-flashes]] reads "a flashing area of
   87,296 CSS pixels". The census and note AJ took 21,824.
2. **Frequency: "more than three" or ">or=3 Hz".** WCAG, BT.1702 and Ofcom
   allow three; the EFA abstract says ">or=3 Hz". The consensus full text is
   gated, so whether exactly 3 Hz fails there is **U**.
3. **Dim Flashing Lights' scope.** Video in the header and the HIG; "an
   animation sequence with flashing lights in a game or app" in the article.
4. **The scale of analysis** (section 1.2).
5. **EN 301 549 4.2.9** changed from "provides at least one mode" to "shall
   provide at least one mode of operation, that is the default mode".
6. **Note AJ quotes a superseded definition.** [[source: wcag22-errata]]:
   "2025-06-27: In the definition for motion animation, amending the
   definition to not exclude blurring (#4040)". Note AJ section 1.3 quotes
   the 2024 wording, which excluded it. An editorial correction to note AJ,
   decided by no question.
7. **How many transitions make "more than three flashes".** The definition
   counts "a pair of opposing changes" as a flash, and its Note 2 says "A
   flash consists of two opposing transitions"; neither says whether one
   transition may belong to two flashes. Three counts follow, each a reading
   (**R**):
   - the census's disjoint pairing, each transition in at most one flash:
     eight transitions in a second fail (`baud-bench/src/flash.rs` 47-52);
   - G19's half-flashes: seven fail, "3.5 flashes";
   - the chained (strict) reading, every adjacent opposing pair a flash:
     five fail.

   On the same series and windows the flashing area under G19's count lies
   between the other two (**C**): a case that passes strict passes G19, and
   one that fails disjoint fails G19. Only a case that fails strict and
   passes disjoint is undecided, and the census has no series that counts
   seven, so none of those was decided. On the shipped driver (O3's base,
   the full 1x grid, as drawn) they are 5 cases at 24 pt, 14 at 26, 41 at
   32 and 242 at 72 (**M**, [[source: al-obs-o3-census]]). Each option's
   are in section 4.6.

## 2. Prior art (practice, not standard)

None of five applications read at pinned commits counts flashes or bounds a
flash rate, and none reads Dim Flashing Lights: kitty `851cb80`, neovide
`c1370a8`, ghostty `246f702`, VS Code `ec01053`, Windows Terminal `7d06b26`.
A regex for `seizure|epilep|photo-?sensitiv|strob|flashing light|three
flashes` matches nothing in their trees but incidental words, and fires on
its positive controls (Microsoft's and Apple's pages). Every WCAG reference
in kitty and ghostty is about contrast. What they do instead, each read at
source (`prior-art/notes.md`); where a cited key is pinned at another commit,
its commit is named, and the quoted text is the same at the pin above:

| mechanism | who | bounds flashes? |
|---|---|---|
| off by default | kitty `cursor_trail 0`; neovide's empty `vfx_mode`; ghostty has none built in; VS Code's smooth caret `off` | only by absence |
| a dwell before the trail follows | kitty: "The trail animation only follows cursors that have stayed in their position for longer than the specified number of milliseconds." ([[source: kitty-options-definition-851cb80]]) | not by design: its stated purpose is UI churn, and kitty's own examples use `cursor_trail 1` ([[source: kitty-custom-shaders-doc-851cb80]]) |
| a distance gate | kitty's `cursor_trail_start_threshold 2` ([[source: kitty-options-definition-851cb80]]); neovide's short animation for jumps of two cells or less (neovide `105fd64`, [[source: neovide-cursor-renderer]]) | no |
| a gesture gate | VS Code's smooth caret `explicit`: "Smooth caret animation is enabled only when the user moves the cursor with an explicit gesture." ([[source: vscode-editoroptions-ec01053]]) | no: a program's jumps draw no animation, a user's do |
| blink rate from the OS, blink timeout | kitty, Windows Terminal, GNOME ([[source: gnome-interface-schema-2417a27]]: `cursor-blink-time` allows a 100 ms cycle) | a timeout bounds the duration; the rate only as far as the OS setting does |
| contrast toggle instead of on/off | Windows Terminal draws SGR 5 blink faint, once a second ([[source: wt-renderer-7d06b26]], and Windows Terminal `bb0541e`, [[source: wt-rendersettings]]) | in effect; the source gives no reason |
| one reduce-motion flag per OS | GTK maps `NSWorkspace` ([[source: gtk-macos-settings-e92c46b]]), `SPI_GETCLIENTAREAANIMATION` ([[source: gtk-win32-misc-e92c46b]]) and the portal's `reduced-motion` ([[source: gtk-wayland-settings-e92c46b]], [[source: xdg-portal-settings]]) onto one setting | not for the cursor |

The counter-examples:
- **neovide restarts its highlight effect on every change of cursor cell**
  (`cursor_jumped` calls `restart`, [[source: neovide-cursor-vfx-c1370a8]]
  179-180), an oval up to three cell heights across at alpha 200/255, with
  nothing limiting the rate: the shape the census found inside `REIGNITE`.
- **kitty's lightning trail is built to flash**:
  [[source: kitty-trail-lightning-851cb80]] reads "// Flash hard, then rapidly
  lose the core, leaving only glow." Its on-screen flash count is unmeasured.

What a kitty-style dwell, a gesture gate, a faint toggle or a per-move
restart would do to baud's census was not measured.

## 3. baud today

### 3.1 What the census counts, and what it found

The census (data
`docs/research/sources/aj-observed/flash-census.tsv`) draws the shipped
driver's frames on the GPU and counts, per pixel, the transitions of the
definition's Note 2, paired into flashes disjointly; a case fails
when the pixels that flash more than three times in one second cover more
than 21,824 pt² of any 341 x 256 pt window. Each count is given **as drawn**
and **apart**, the second with the cursor's own two cells left out. Its
findings (**M**):

- At 13 pt, 1x and 2x, every arm passes in all 3,072 cases, the off arm
  included; the worst is the fire's 10,115 pt² as drawn, and the plasma's
  8,274.
- At 32 pt the fire fails 6 of 192 cases at 1x and 5 at 2x as drawn; the orb
  11 at each. All are on the light page `#EFEFEF`, with the cursor jumping
  back and forth inside `REIGNITE`. The plasma passes at both scales (worst
  20,071, 92% of 21,824) and fails the strict reading in 6 at each (4
  apart).
- At 72 pt (1x only), as drawn: fire 64, orb 61, plasma 30, ufo 10, liquid 7
  of 192 each.
- No single long move failed, at any size; its worst area is 515 pt².

**The plain quad and the sweep**, first counted on the GPU by the
prototypes' base runs (all four ran them at 72 pt; the figures here are
O3's, the shipped driver on the full 1x grid,
[[source: al-obs-o3-analysis]]): the quad fails 78 of 192 cases at 72 pt as
drawn and 78 apart, the sweep 22 and 20; both pass at 32 pt (worst 17,626
and 14,322). These are shipped trails a config can name.

**Where the size crosses.** The O3 base run puts the worst case at 24 pt at
18,675 pt² (the fire, black on light, every 100 ms on the diagonal, 120 Hz),
14% under, and at 26 pt at 19,970 (the orb), 8.5% under (**M**,
[[source: al-obs-o3-analysis]]). The CPU replica found the first failing size
at 28 pt for the fire and the orb, black on light (**M**, replica only,
[[source: al-obs-replica-out3]]). On the strict reading the shipped fire
already fails 5 cases at 24 pt 1x, all at 120 Hz (**M**, O3's base); at 2x,
6 of 8 at 120 Hz (O4's build, which draws the base's frames below its gate;
its base ran 2 of the 8, both failing strict, [[source: al-obs-o4-census]]).
The plasma passes the strict reading at 24 and 26 pt (worst 17,789 and
18,809, O3's base).

**Above 72 pt nothing was counted.** A configured size can reach 512 px per
em (section 1.2). The bare cursor alone then crosses the limit (**C**): one
cell at 72 pt is 43 x 95 = 4,085 pt², the off arm's figure, and a cell's
area grows with the square of the size, so a program that jumps the cursor
between two adjacent cells in one window flashes more than 21,824 pt² from
about 72 x √(21,824 / 8,170) ≈ 118 pt, and one cell alone from about 166
pt. That holds under every option, O4 included, if a program-driven jump
counts as produced by the ICT itself (**R**, section 1.3). How each trail
grows above 72 pt is unmeasured; the ufo's area grows fastest with the cell
(section 3.3), and under O3 its worst at 72 pt is already 76% of the limit.

### 3.2 Why back-and-forth flashes one band again and again

Traced through `baud-app/src/trail.rs` (**C**, `baud/analysis.md` section 2):

1. The first long move ignites: `ignite` (285) records `last_lit` and builds
   an `Ignition` whose path is fixed there.
2. A long move within `REIGNITE` of it retargets: `take`'s in-flight arm
   calls `ignite`, which returns at once (289-294), keeps the burning flame,
   and replaces the moving quad with a new segment.
3. The flame's body follows the live quad, not the fixed path: `frame` hands
   the quad's current corners to the effect (216-224). So each jump sweeps
   the body back over the band, and each pixel in it is covered and uncovered
   once per jump: up to 10 flashes a second at 100 ms between jumps.
4. The flame never goes out between ignitions (its life, 0.45 s, outlasts the
   0.40 s between them), and the next ignition relights the whole path.

`REIGNITE` held every back-and-forth to 8 ignitions in 3 s, as written; one
pixel still flashed up to 23 times in one second (the liquid, at 32 pt 2x,
120 Hz). The module doc has said so since `6541bdf`: "It limits
ignitions, not flashes".

### 3.3 What flashes, and why the light page

**On the light page almost anything visible flashes.** `#EFEFEF` has
relative luminance 0.863. A pixel over it makes a general transition once it
is at or below 0.763, which for black ink over that ground, on the census's
encoded byte-rounded blend, is an opacity of 0.0525; for the dark page's
white ink over `#101010` it is 0.316 (**C**, [[source: al-obs-thresholds]]).
So on the light page the fire's faint halo and tongue tips, the orb's halo
and the plasma's tube all flash, while on the dark page the fire's halo (at
most 0.12) and the plasma's tube (0.28) never reach it, and the orb's halo
reaches it only for white ink (**C**, `baud/analysis.md` sections 1 and 4).

**No single term of the fire is the flashing part.** With the replica's
ceiling arms (**M**, replica, [[source: al-obs-replica-out1]] and
[[source: al-obs-replica-out7]]): removing the halo, the embers or the
tongues each clears all 11 of the fire's failing cases at 32 pt (6 at 1x, 5
at 2x; worst 21,650, 19,444, 21,123), but at 72 pt the emitter quad's body
alone still fails on the light page (26,724, black ink), and the orb's sphere
alone fails at 72 pt on both pages.

**The area grows with the cell.** Every reach, height and particle size is
in units of the cell's height (`effect.rs:178`, set at `trail.rs:296`). The
fire's flashing area grows about linearly with it (median power 1.00 over 88
matched cases), the orb's faster (1.46 over 68) and the ufo's fastest (1.80
over 13) (**M**, [[source: al-obs-census-out]]).

**One opacity factor cannot fix it.** The replica's fire at 72 pt on the
light page still fails at 50%, 25% and 10% of its opacity (43,191, 38,129,
27,348; **M**, [[source: al-obs-replica-out2]]).

### 3.4 baud's idioms that apply, and what else flashes

- **Reduce Motion** (commit `bc7cd03`): `[cursor] trail` reads into
  `TrailPick::{Default, Named}` (`config.rs` 343); `TrailPick::effective`
  (363) lets a named trail win over the system, and the reader's comment is
  "A typo must not override an accessibility setting" (341). The rule since 2026-09-25: Reduce Motion means no trail, unless the config names one. The app reads the preference before the
  chase is built and polls it on every tick (`app.rs` 761-769 and
  `poll_motion`); a change goes through `retrail` and `Chase::restyle`,
  which keeps `last_lit`.
- **The display link.** Effects are pure functions of the ignition and its
  age: `effect.rs` 11-12, "Everything here is a pure function of the
  ignition and its age." The driver's state (`last_lit`, the move in flight)
  is keyed on the display link's timestamps. A rate limit can live there; a
  per-pixel history in the renderer would break that purity, which the pixel
  oracle relies on.
- **The ghost trail** has its own driver in `app.rs` (lines 1103-1146) outside
  `trail.rs`. Neither the census nor any prototype bench counted it.
- **Other flashes baud draws, which no prototype touches** (**C**, from the
  code; none counted): the visual bell, a full-screen `VideoFlash` that
  `bell.rs` shows even when a program's `?5` pair came and went unseen
  (module doc, lines 13-20), with no rate limit found in that file; `SGR 5`
  blink (`baud-render/src/cellmap.rs` 490); the alternate-screen fade
  (`app.rs` 83 and 1178); and the ghost trail, except above O4's gate,
  where O4's floor turns it off. Note AH's title-bar LEDs and note AJ's
  launch splash, which reuses the plasma, are designed and not built. Since
  the unit of evaluation is the whole program (section 1.1), no option here
  settles whether baud conforms; each bounds the counted trails.

### 3.5 The gate the options use sits exactly on 24 pt

O2, O3 and O4 (with Dim Flashing Lights off) change nothing while the cell is
at most 32 pt tall; O4 with the setting on removes the default trail at every
size. The bench's cell lines ([[source: al-obs-cell-sizes]]): 24 pt at 1x is
a 14 x 32 px cell, 32 pt; at 2x, 29 x 63 px, 31.5 pt; 25 pt is 33 pt at
both. So 24 pt at 1x sits on the gate with no slack, and the design's
computed 31 pt was wrong by one pixel. The gate test is strictly greater
than, so 24 pt is below it. Every measured cell fits cell px = round(1.32 x
font px) (**C**), which the sizes in section 4.1 use.

## 4. The options

### 4.0 What every prototype shares

**The harness.** Each prototype added the same census harness: `quad` and
`sweep` arms in the bench, the cell height in points passed to
`Chase::frame`, a hash of every frame (`FLASH_HASH`), and PNG output. The
harness is in both builds of each prototype, not behind its feature (O2's
commit: "in both builds"; O4's: unconditional), and the new parameter on
`Chase::frame` is unconditional in all four; only each option's mechanism
sits behind its feature. So each base arm is `6541bdf`'s trail code plus the
harness, which the census-row gate and the comparison with the committed
census tie to the shipped frames.

**Gates, recomputed from the prototypes' raw runs** ([[source: al-obs-gates]],
by [[source: al-obs-gates-tool]]), **M** unless stated:
- the base reproduces the census row `fire 32x1 120 black light
  every-100ms-diag`: 24,944 pt² FAIL, apart 24,366 (all four census files'
  base columns);
- the bench's 4 Hz control block fails and its 2 Hz block passes in every
  run file that carries them: O1 146 pairs in 139 files, O2 51 in 48, O3 94
  in 47, O4 56 in 53;
- the base run twice at 13 pt 1x gives equal frame hashes: O1 482/482 rows
  (480 cases and 2 control blocks), O2 338/338 (336 and 2), O3 1,924/1,924
  (1,920 and 4), O4 290/290 (288 and 2);
- the base and option binaries differ under `cmp` for O1 (both caps) and O3;
  O2's and O4's binaries were not kept (below), so for those two it is as
  the prototypes reported;
- base rows each prototype ran itself equal the committed census in every
  column but the timing: O1 395/395 overlapping rows
  ([[source: al-obs-o1-analyze]]), O3 4,620/4,620
  ([[source: al-obs-o3-analysis]]).

**O2's rows come from three builds** ([[source: al-obs-gates]] section 6):
its option rows from `d9ed4bd` (3,024, at 13, 24, 32 and 72 pt) and the
final `4ccd622` (672, at 26 and 72 pt); its base rows from a first harness
build with no commit kept (1,008, at 24, 32 and 72 pt), `d9ed4bd` (1,680)
and `4ccd622` (1,008). From `d9ed4bd` to `4ccd622` only
`baud-bench/benches/flash.rs` changed (**C**, `git diff --stat`), so the
trail and renderer code is the same. The prototype reported the final
binaries equal to the earlier ones on every hash it took: the 13 pt base,
all 336, and O2 on 12 cases at 32 pt.

**A shared scratch `bin/`, and what rules out a swapped binary.** The O2 and
O4 prototypes built into one scratch `bin/`, where each build replaced the
file of the same name, and kept no binary elsewhere; O1 and O3 kept theirs
in their own directories. As found afterwards ([[source: al-obs-gates]]
section 5), its `flash-base` was last written at 20:35:32 on 2026-10-09,
`flash-o4` at 20:35:55 and `flash-o2` at 20:36:10, and `flash-base` carries
the string `under_dl`, a column name only O2's harness writes (`flash-o2`
carries it too; O1's and O3's base binaries do not). So O4's base binary was
replaced by O2's. When the replacements began is not recorded. Four
controls, each **M**:
- **Each run file's header names its harness.** All 48 of O2's carry O2's
  extra columns and none of O4's 60 does ([[source: al-obs-gates]] section
  4), so no O4 run used O2's binary and no O2 run used O4's. O1's, O3's and
  O4's harnesses write the same columns, so headers cannot tell those three
  apart.
- **Shared base rows agree.** Joined on the case, every base row that two
  sources share is equal on ten verdict columns (the areas, the four
  outcomes and the flash counts), across all four prototypes and the
  committed census, and every frame hash two prototypes both took is equal
  ([[source: al-obs-cross-base]], by [[source: al-obs-cross-base-tool]]).
  O1 ran 1,969 of its 4,080 base rows itself, all shared with O3; the other
  2,111 its analysis copied from the committed census where it had no run,
  so its agreement with the census on 2,496 rows is partly true by
  construction. O2 shares 3,168 of its 3,696 base rows with another source,
  O3 6,432 of 9,600, O4 2,120 of 2,418.
- **Shared base frames agree.** Every base frame that two or more prototypes
  rendered as a PNG is byte-identical across them: 288 frames, 192 of them
  rendered by all four ([[source: al-obs-base-frames]]).
- **What they cannot reach.** A base row no other source shares rests only
  on its own prototype's gates (the census row, the controls, the A/A), and
  those cannot show which binary drew it: O2's 528 (all its new three-cell
  motion), O3's 3,168 (24 and 26 pt on the red grounds and at 120 Hz, and
  quad and sweep cases at 13 and 32 pt), and O4's 298 (25 pt, and ten cases
  at 24 pt 2x).

**What every prototype leaves unmeasured:** sizes above 72 pt, text under
the trail, grounds seen through the glass, jumps longer than one window,
moves 300 ms or more apart, the bar and underline cursors, the ghost trail,
and most of 2x: O1, O2 and O3 ran no 2x at all, and O4 ran 24 pt and some
32 pt at 2x. Timing: every bench `ms` column was taken under heavy parallel
load (load averages of 25-120 where the prototypes recorded one), so no
option has a cost figure; the costs below are counts of work, **C**.

**The grids differ**, so every figure below is scoped to the grid its option
ran. The per-size tables are [[source: al-obs-options-table]], grouped from
the census files by [[source: al-obs-options-table-tool]].

**Frames.** For the one case below, every option has a frame or footprint
staged. The case: the fire, 32 pt at 1x, 60 Hz, black cursor on the light
page, jumping every 100 ms on the diagonal. Today it passes, 68 pt² under
the limit as drawn (21,756; apart 21,010), and fails only on the strict
reading (27,569); the failing census row is its 120 Hz twin (24,944), and no
120 Hz frame is staged. A footprint holds each pixel's value farthest from
the first frame over the run: where the trail ever drew, not where it
flashed.

- base: frame 39 at 0.650 s ([[source: al-obs-png-base-fire32-f39]]) and the
  footprint ([[source: al-obs-png-base-fire32-footprint]]).

### 4.1 O1, geometry: every trail drawn at its size on a 26 pt cell

**Mechanism** (`f5725ab`). Above a cell height `CAP` of 26 pt, the driver
draws every trail as it is at that cell: the ignition's `unit` becomes
`cell.1 * s` with `s = CAP / cell height`, and the emitter's corners move
inward by a fixed inset, which is the chase of the cursor's rectangle scaled
by `s` about its centre. All in `trail.rs`; no state, no renderer or shader
change. Which moves trail is decided on the real cursor rectangle, as today.
A second build caps at 29 pt.

**Where the cap engages** (**C**, from the cell fit in section 3.5). A 26 pt
cell is JetBrains Mono 20 pt at 1x, so the 26 pt cap engages from 21 pt at
1x and from 20 pt at 2x (a 53 px, 26.5 pt cell); the 29 pt cap from 23 pt
at both. Every 13 pt hash below was taken at 1x; at 2x a 13 pt cell is 17
pt, below either cap, so its frames are unchanged by construction, not by
measurement.

**Conformance, of the trail's flashes.** 2.3.1 (Level A): conforms on its
grid on the disjoint reading, by the area route (G176's shape), at every
size measured up to 72 pt. Its grid never ran 24 pt at 120 Hz, where the
shipped fire's strict failures are. 2.3.2 (AAA): does not; the most flashes
a second at one pixel did not fall (below). G19: not met as a technique,
since flashes at a pixel stay above three a second; on G19's count of the
area, the 26 pt cap passes on its grid (no strict failure) and the 29 pt cap
leaves 2 cases undecided (**C**). It does not hold for a user under an OS
magnifier, which G176 sends to G19. No departure from a text on the route it
claims.

**Measured** ([[source: al-obs-o1-census]]; grid: 13, 24, 26 and 32 pt at 60
Hz on the light and dark pages, 32 pt at 120 Hz on the light page, 72 pt at
60 and 120 Hz on all four grounds; 4,080 cases for the 26 pt cap, 1x only.
The 29 pt cap ran a reduced grid of 2,400: no 26 pt, no 32 pt at 120 Hz, and
72 pt on the light and dark pages only):

| size | base FAIL drawn/apart | 26 pt cap: FAIL, worst drawn/apart | 29 pt cap: FAIL, worst drawn/apart (base on its grid) |
|---|---|---|---|
| 13 | 0/0 | 0/0, identical frames (482/482 hashes) | 0/0, identical frames |
| 24 | 0/0 | 0/0, 13,646 / 13,088 | 0/0, 14,930 / 14,424 |
| 26 | 0/0 | 0/0, 13,851 / 13,182 | not run |
| 32 | 17/14 | 0/0, 15,979 / 15,319 | 0/0, 15,517 / 14,719 (base 4/3; 60 Hz only) |
| 72 | 272/240 | 0/0, 16,785 / 14,658 | 0/0, 18,730 / 16,989 (base 163/147; light and dark only) |

- 13 of the base's 17 failures at 32 pt are at 120 Hz, which the 29 pt cap
  did not run.
- On the strict reading the 26 pt cap fails nothing on its grid; the 29 pt
  cap fails 2 cases at 72 pt (the fire, black on light, jumping across every
  100 and every 200 ms, 120 Hz: 22,511 and 22,194), and passes both apart.
- **The burn time changed**: every effect's lifetime reads `unit`, so 1,520
  of 4,080 cases drew the effect on more frames and 48 on fewer; ignitions
  are equal on every case (**M**, [[source: al-obs-o1-analyze]]).
- **Flashes a second at one pixel did not fall**: at 32 pt the most is 18
  under the cap against 17 for the base on the same grid, and at 72 pt 17
  against 18; it rose on 76 of 720 cases at 32 pt and fell on 78 (**M**).
- Tests, as the prototype reported them: 569 of 569 in `baud-render` and
  `baud-app` without the feature, 273 in `baud-app` with it.

**Cost (C).** About a dozen flops and one division per frame above the cap,
nothing below it. The fire sheds more, smaller embers above the cap (about 36
to 131 at 72 pt on the diagonal, inside the existing clamp of 160). No new
state.

**What a person sees.** Below the cap, nothing changes: 13 pt frames are
byte-identical. Above it, the trail keeps its size on a 26 pt cell, centred
on the cursor's: at 32 pt the flame is 62% of the cell's height (26/42,
**C**), at 72 pt 27% (26/95, **C**), a small flame leaving the middle of a
large block and arriving at the middle of another. Frames: 32 pt, frame 39
([[source: al-obs-png-o1-fire32-f39]]) and the footprint
([[source: al-obs-png-o1-fire32-footprint]]); 72 pt, frame 31 and the
footprint, base and O1 ([[source: al-obs-png-base-fire72-f31]],
[[source: al-obs-png-o1-fire72-f31]],
[[source: al-obs-png-base-fire72-footprint]],
[[source: al-obs-png-o1-fire72-footprint]]).

**Open (U).** With the core as the emitter's hole, the effect may now draw
inside the cursor's cell, which veils the glyph under a block cursor. The
census cannot see it (its cells are blank). The design's stage 2 separates
the two rectangles in the uniform, and needs a shader change, the metallib
rebuilt and the pixel oracle's references; it was not built.

### 4.2 O2, intensity: repeat passes drawn under the thresholds

**Mechanism** (`4ccd622`). Above the 32 pt gate, the first sweep over a
stretch of screen in any second is drawn as today. A later sweep over an
overlapping stretch within a second of it has every record's alpha scaled
by k/255, where k is the largest byte at which that ink over that ground
makes no general or red transition, less one step; so a record at full
alpha reaches k and a fainter one stays under it (`capped`, in
`flash.rs`, a file the prototype adds to `baud-render`). State: a ledger of the last eight
full sweeps (start time and band) in `Chase`; a two-variant strength on
`Vfx` and `Smear`; the cap computed in `baud-render` from the ink and one
ground colour handed to the renderer. No shader change. The caps, out of
255 (**C**: computed by the prototype's exact scan over the blend's rounding
cuts and pinned in [[source: al-obs-o2-astar-tool]] and a unit test; the
records drawn on the red grounds matched them, and none exceeded its cap):

| ground | theme cursor | white | black | red |
|---|---:|---:|---:|---:|
| dark `#101010` | 95 | 79 | 255 | 61 |
| light `#EFEFEF` | 14 | 255 | 12 | 16 |
| red `#CD0000` | 85 | 77 | 124 | 255 |
| dark red `#1A0000` | 4 | 3 | 249 | 175 |

**Conformance, of the trail's flashes.** 2.3.1 (Level A): conforms on its
grid on the disjoint reading, by the area route, for every trail counted
(the ghost was not). The design's second route, that a repeat pass is not a
flash at all by the definition's luminance clause and so holds under any
magnification, **holds only for the quad, the sweep and the liquid, and only
on the definition's measure** (below); for the six trails that draw
particles it does not. On the strict reading it fails nothing on its grid,
so G19's count passes there too (**C**). 2.3.2 (AAA): does not. The red cap
uses WCAG's informative Note 3, and BT.1702 counts red "irrespective of
luminance".

**Measured** ([[source: al-obs-o2-census]]; 3,696 cases a build, 1x only,
over seven motions, the seventh new: three cells cycling every 100 ms). The
grid, as run:
- six arms (off, fire, orb, plasma, quad, sweep) at 13, 24 and 26 pt, at 60
  Hz on the light and dark pages;
- the same six at 32 and 72 pt at 60 Hz on all four grounds; at 120 Hz, 32
  pt on the light page only and 72 pt on all four grounds;
- the ice, lightning, liquid and ufo only at 32 and 72 pt, at 60 Hz on the
  light and dark pages, and the liquid and ufo also at 72 pt and 120 Hz on
  the light page: 56 to 84 cases each.

| size | base FAIL drawn/apart | O2 FAIL | O2 worst drawn / apart | most flashes a second at a pixel, base / O2 |
|---|---|---|---|---|
| 13 | 0/0 | 0/0 | identical frames (336/336 hashes) | 13 / 13 |
| 24 | 0/0 | 0/0 | identical frames (336/336) | 13 / 13 |
| 26 | 0/0 | 0/0 | 1,088 / 179 | 13 / 7 |
| 32 | 17/14 | 0/0 | 1,837 / 515 | 15 / 7 |
| 72 | 309/258 | 0/0 | 10,968 / 569 | 18 / 7 |

Strict: 0 failures at every size. The drawn areas above the gate include the
cursor's own cells, which flash under every option (the off arm alone reads
798 pt² at 32 pt and 4,085 at 72, **M**); apart, the worst is the size of one
long move. The 24 pt frames are identical at 60 Hz, the only rate O2 ran
there; at 120 Hz they are identical by construction.

**Its pixel check failed.** The design's acceptance (its P0.7: no FAIL on
any size or ground, and the 13 and 24 pt hashes equal) is met on its grid.
Beside it the
design set a pixel check to back the construction: over every repeat frame,
no pixel outside the cursor's cells may change by 0.10 or more against the
first frame, and no red transition. Two measures of it are in the census
file (**M**, [[source: al-obs-o2-census]]):
- **As the design wrote it** (`under_raw`, any change of 0.10 or more):
  every trail fails but the sweep. The quad and the liquid reach 0.1368,
  which is white over the light page, where the darker state is above 0.80.
- **With the definition's condition** (`under_dl`, a change counted only
  where the darker state is below 0.80, which the prototype added part-way
  through its runs): the quad 0.0998, the liquid 0.0994 and the sweep
  0.0601 pass, with no general or red transition; the fire 0.510, ice 0.463,
  orb 0.323, lightning 0.211, plasma 0.204 and ufo 0.176 fail, each with
  general and red transitions.

Which measure the check means is the definition's question, not the
design's: the condition is the definition's own words, so this note reads
the conditioned measure as the one that decides a flash (**R**). No record
ever carried more than its cap. The cause is overlap: premultiplied
blending compounds a particle over a band or over another particle, so the
cap holds per record and not per pixel. A run with the particle records left
out (wrong by construction) -- 96 trail cases at 72 pt and 60 Hz, eight
trails (the sweep not among them), white and black ink on both pages over
three motions, plus 4 control blocks -- kept every trail under 0.10 on the
conditioned measure (at most 0.0971) with no general or red transition; on
the raw measure six of the eight reach 0.1368 and the orb and the plasma
0.1279, all on the light page (**M**, [[source: al-obs-o2-drop-dark]],
[[source: al-obs-o2-drop-light]]).

**Cost (C).** Per long move, a band test against up to eight rectangles; per
change of ink or ground, a scan of up to about 767 alpha intervals; per
repeat frame, one multiply per record. *As of the prototype at
`4ccd622`; the landed build caps the CPU path's quad and reads the ground
from the cells under the pass.* `baud-render`'s CPU drawing path
ignores the cap, and the macOS driver draws a trail through it whenever
`BAUD_GPU=0` forces that path or no Metal device is found, every effect then
running as the quad (`app.rs` 130-140, 160-171 and 805-809 at `6541bdf`); the
Linux driver draws none (section 1.7). On that path O2 bounds nothing, and the
shipped quad fails 78 of 192 cases at 72 pt (section 3.1); O1's, O3's and
O4's mechanisms are in `trail.rs` and `config.rs` and reach it by
construction, unmeasured. In the app the ground is one colour (the default
background); mixed backgrounds, text and glass are unmeasured, and the
production answer (the least cap over the cells under the band, or a
per-pixel clamp in the shader) is open. (Built since: the least cap over
the colours the cells under the pass draw, a selection and the cursor
among them, and a dimmed page over a white and a black desktop)

**What a person sees.** Up to 24 pt, nothing changes. Above, a single jump
and the first sweep over any stretch in a second look as today; a jump back
over it within the second draws the whole effect scaled down, its brightest
record at the cap: on the light page 12/255 to 16/255 for the black, theme
and red cursors, 4.7-6.3% (**C**), a faint ghost; on the dark page 79/255
and 95/255 for the white and theme cursors, 31% and 37% (**C**). Frames:
frame 19, the second move, where a repeat is first drawn (base
[[source: al-obs-png-base-fire32-f19]], O2
[[source: al-obs-png-o2-fire32-f19]]); frame 39
([[source: al-obs-png-o2-fire32-f39]]); the footprint
([[source: al-obs-png-o2-fire32-footprint]]).

### 4.3 O3, temporal: one ignition or sweep a second above the gate

**Mechanism** (`dc2fce3`). Above the 32 pt gate, `REIGNITE` becomes one
second. A jump within that second rides the burning flame while it lasts;
once it is out, the driver's existing "nothing burns" arm draws nothing, so
the cursor jumps bare until the second is up. The quad and the sweep, which
have no ignition, snap a long move that starts within a second of the start
of the last sweep drawn, and a snap cuts a sweep still in flight
(`self.moving = None`). One new field, `last_swept`, kept across `snap` and
`restyle` as `last_lit` is. All in `trail.rs`, keyed on the display link's
timestamps; no renderer or shader change. The effects' rule is the replica's
variant v4 gated by size; the smear rule is O3's own, since v4 ran only the
seven effects.

**Conformance, of the trail's flashes.** 2.3.1 (Level A): conforms on the
full 1x grid on the disjoint reading, by the area route. On the strict
reading it fails 54 cases at 72 pt as drawn and 30 apart (the fire 17/12,
the orb 20/12, the plasma 12/6, the ufo 5/0), and keeps the shipped fire's 5
at 24 pt; each of those is undecided on G19's count (**C**). 2.3.2 (AAA):
does not; above the gate a pixel still flashes up to 11 times a second (the
ufo at 32 pt), from what one ignition draws. It meets neither G19 as a
technique nor BT.1702's spacing at a pixel: one ignition a second is wider
than BT.1702's 334 ms, but BT.1702 spaces flashes at a place, not
ignitions.

**Measured** ([[source: al-obs-o3-census]], [[source: al-obs-o3-analysis]];
the full 1x grid: 10 arms including off, quad and sweep, at 13, 24, 26, 32
and 72 pt, 60 and 120 Hz, four grounds, four inks, six motions; 9,600 cases
a build):

| size | base FAIL drawn/apart, strict | O3 FAIL drawn/apart, strict | O3 worst drawn / apart | most flashes a second at a pixel, base / O3 |
|---|---|---|---|---|
| 13 | 0/0, 0 | 0/0, 0; identical frames (1,924/1,924 hashes) | 10,115 / 9,979 | 18 / 18 |
| 24 | 0/0, 5 | 0/0, 5; identical frames (1,924/1,924) | 18,675 / 18,288 | 17 / 17 |
| 26 | 0/0, 14 | 0/0, 0 | 5,469 / 4,925 | 17 / 9 |
| 32 | 17/14, 58 | 0/0, 0 | 6,425 / 5,627 | 17 / 11 |
| 72 | 272/240, 514 | 0/0, 54 | 16,535 / 12,531 (the ufo) | 18 / 9 |

- The quad and the sweep above the gate flash only the bare cursor's cells
  (798 pt² at 32, 4,085 at 72, apart 0).
- The O3 rows agree with the replica's v4 on all 1,232 shared keys, same
  verdict, within 11 pt² (**M**).
- Above the gate 63% as many frames draw a trail as in the base (692,096 of
  1,097,936 trail frames at 26, 32 and 72 pt, the off arm left out; **M**).
- Tests, as the prototype reported them: 568 of 568 without the feature, 572
  with it.

**Cost (C).** One comparison per move the trail takes; nothing per frame.

**What a person sees.** Up to 24 pt, nothing changes. Above, at most one
flame a second: back-and-forth reads as one flame following the cursor for
about half a second, then a bare cursor until the second is up; a single
jump looks as today; the quad and the sweep sweep once a second and snap the
rest. Frames: frame 39 ([[source: al-obs-png-o3-fire32-f39]]); frame 43 at
0.717 s, after the shipped trail has relit at 0.6 s, where O3 shows only the
first flame's last embers (base [[source: al-obs-png-base-fire32-f43]], O3
[[source: al-obs-png-o3-fire32-f43]]); the footprint
([[source: al-obs-png-o3-fire32-footprint]]), which covers about the same
band as the base's, because O3 draws the same band fewer times.

### 4.4 O4, a system setting on an always-on floor

**Mechanism** (`43b72d7`, with `dd56cd3` in the glass crate). Two parts.
- **The floor.** While the cell is taller than the 32 pt gate, every trail
  is off, named ones and the ghost included, and the block cursor jumps.
  `TrailPick::effective` takes the cell height beside the system's motion
  setting; a zoom across the gate goes through `retrail`, as a Reduce Motion
  change does. The floor needs no setting, which section 1.2's reading asks
  of a Level A remedy; the prototype's own doc puts it as "Level A has no
  opt-in" (`43b72d7`, `config.rs` 385), a reading, not a text (**R**).
- **The setting.** `MADimFlashingLightsEnabled()` is resolved at run time
  (`dlopen`/`dlsym`, false when absent), because baud's minimum macOS is
  13.0 (`packaging/macos/Info.plist` 14) and the function is 13.3's. It lives in the window crate, `macos-liquid-glass`, beside `reduce_motion`, is read
  at launch and polled on every tick, and `BAUD_FLASHING=dim|full` fixes it
  for checks. With it on, the default trail is off at every size; a named
  trail is still drawn below the gate, taking the Reduce Motion rule of 2026-09-25 for this setting too.

**Conformance, of the trail's flashes.** 2.3.1 (Level A): conforms on its
grid on the disjoint reading; above the gate nothing is drawn but the
cursor, and below it today's trails pass on every case run. Below the gate
the shipped fire keeps its strict failures at 24 pt (5 at 1x, 6 at 2x),
undecided on G19's count (**C**). 2.3.2 (AAA): met for the default trail's
own pixels only with Dim Flashing Lights on, and only because the setting
turns the default off, which any option could do; with it off, trails below
the gate flash a pixel up to 17 times a second. The setting follows Apple's
article for the default trail, against its header's video scope (section
1.8); for a named trail below the gate it changes nothing, a departure from
the article's "adjust the animation accordingly" that rests on the Reduce Motion rule (a choice the project made, Q10). Whether its default mode
"minimizes" in EN 301 549's sense is open.

**Measured** ([[source: al-obs-o4-census]], [[source: al-obs-o4-summary]];
2,928 cases). The grid, as run (**M**, from the census file):
- at 60 Hz and 1x, 13, 24, 25, 26, 32 and 72 pt on the light and dark pages
  for the six minimum arms (off, fire, orb, plasma, quad, sweep), four inks,
  six motions; the ice, lightning, liquid and ufo at 13 and 24 pt on both
  builds, and at 32 and 72 pt on O4 beside its off arm (72 pt on the light
  page only);
- at 120 Hz: every 24 pt 1x case on the light page (144, none of whose 60 Hz
  base reached 80% of 21,824); at 32 and 72 pt 1x, the 98 cases whose 60 Hz
  base read at least 80%, with 30 off-arm twins; and 24 pt 2x on the light
  page for the default and black inks (40);
- at 2x: 24 pt for the six minimum arms on the four back-and-forth motions,
  and 32 pt for the fire, orb and off on the light page at 60 Hz only; eight
  of those 32 pt cases read at least 80% and have no 120 Hz run.

| size | base FAIL drawn/apart | O4 FAIL | O4 worst drawn / apart | most flashes a second, base / O4 |
|---|---|---|---|---|
| 13 | 0/0 | 0/0 | identical frames | 14 / 14 |
| 24, 1x | 0/0 | 0/0 | 18,675 / 18,288, identical frames | 16 / 16 |
| 24, 2x | 0/0 (base run on 10 cases) | 0/0 | 19,124 / 18,700 | 17 / 17 |
| 25, 26 | 0/0 | 0/0 | 495, 714 / 0 (the cursor) | 13 / 5 |
| 32, 1x | 14/11 | 0/0 | 798 / 0 (the cursor) | 15 / 5 |
| 32, 2x | 2/2 | 0/0 | 798 / 0 | 13 / 5 |
| 72 | 146/141 | 0/0 | 4,085 / 0 (the cursor) | 15 / 5 |

- Above the gate every trail case hashes equal to the off arm's (1,362 of
  1,362); below it every case whose base was run hashes equal to the base
  (1,114; the other 222 are 24 pt at 2x with no base run) (**M**).
- Below the gate the strict reading still fails: 5 cases at 24 pt 1x and 6
  at 2x (the fire, worst 23,551).
- Live, at 2x, with `BAUD_FLASHING` set (**M**, [[source: al-obs-o4-live]]):
  at 13 pt `dim` turns the default fire off and a named fire stays; at 24 pt
  (a 31.5 pt cell) the fire is drawn; at 25 and 32 pt the trail is off with
  either value, and the ghost, run only at 32 pt, is off there. With the
  variable unset, the real setting read `Full`; it was read, not changed.
- Tests, as the prototype reported them: 568 of 568 without the feature, 274
  in `baud-app` with it, and the glass crate's 2.

**Cost (C).** The floor is evaluated at launch, on a rescale and on a
setting change. The poll adds one `dim_flashing_lights()` call per
display-link tick, which is once a frame; a standalone probe timed it at
5.6-5.9 ns against 12.1 ns for `reduce_motion()` (**U**: reported by the
prototype, outside the app, under load, and its output was not kept;
[[source: al-obs-o4-pollcost-tool]]). It cannot land until the window crate ships a release with `dim_flashing_lights`: the prototype reaches
`dd56cd3` through a `[patch.crates-io]` entry with an absolute path to a
glassnote worktree on this machine. Where the lookup lives is a choice:
`baud-app` already resolves an accessibility function at run time through
`sys.rs` (`steady_caret_symbol`, `pty.rs` 1148), so it could live there
instead of in the crate.

**Linux.** The Linux driver draws no trail today, so the floor has nothing
to act on there, and no Dim Flashing Lights counterpart was found to read
(section 1.7). If the trail comes to Linux, whether the floor and the
setting follow it is open; the project's rule that macOS and Linux draw the
same pixels was set for text, and whether it extends to trails is open.

**What a person sees.** With the setting off, up to 24 pt nothing changes.
At 25 pt and above there is no trail of any kind, whatever the config names:
the block cursor jumps ([[source: al-obs-png-o4-fire32-footprint]]). Zooming
across 24 pt turns the trail off on that frame, and back on. With Dim
Flashing Lights on, the default trail is gone at every size: the 13 pt fire
([[source: al-obs-png-base-fire13-f39]]) becomes the bare cursor
([[source: al-obs-png-o4-dim13-f39]]; the off arm, which is what the table
test makes the default under Dim, since the bench does not read the
setting).

### 4.5 Levers measured on the replica only

The CPU replica runs the shipped driver and counter, with the pixel oracle's
references in place of the GPU; it matched 528 of 528 census verdicts
([[source: al-obs-replica-matrix]]). Over 616 back-and-forth cases a size
(7 effects, 4 motions, 2 rates, 11 ink/ground pairs, 1x):

| variant | 32 pt FAIL drawn, strict | 72 pt FAIL drawn, strict | worst at 72 |
|---|---|---|---|
| v1: a retarget inside `REIGNITE` puts the effect out and snaps | 0, 33 | 3, 280 | 26,211 (fail) |
| v3: at most three drawn sweeps in any second, the rest snap | 0, 5 | 0, 112 | 20,835 |
| v4: `REIGNITE` = 1 s (O3's effect rule, without the gate) | 0, 0 | 0, 54 | 16,535 |
| the shipped driver, same keys | 17, 58 | 172, 353 | |

None was prototyped on the GPU but v4. A kitty-style dwell was not measured.

### 4.6 The four side by side

Every figure is **M** on that option's own grid (section 4.0), 1x unless
stated, and every pass and FAIL is on the disjoint reading unless the row
says otherwise. "Above the gate" means above each option's own cap or gate.

| | O1, cap 26 pt | O1, cap 29 pt | O2 | O3 | O4 |
|---|---|---|---|---|---|
| passes 2.3.1 by | area | area | area; the luminance route for the quad, sweep and liquid on the definition's measure only | area | nothing drawn above the gate; area below |
| cases run | 4,080 | 2,400 (no 26 pt, no 32 pt at 120 Hz, 72 pt on light and dark only) | 3,696 (four extra effects at 32 and 72 pt only) | 9,600 (full 1x grid) | 2,928 (reduced, some 2x) |
| FAIL drawn/apart, 32 pt (base, same grid) | 0/0 (17/14) | 0/0 (4/3) | 0/0 (17/14) | 0/0 (17/14) | 0/0 (14/11) |
| FAIL drawn/apart, 72 pt (base) | 0/0 (272/240) | 0/0 (163/147) | 0/0 (309/258) | 0/0 (272/240) | 0/0 (146/141) |
| worst above the gate, drawn / apart | 16,785 / 14,658 | 18,730 / 16,989 | 10,968 / 569 | 16,535 / 12,531 | 4,085 / 0 |
| strict FAIL drawn/apart above the gate | 0 | 2/0 (72 pt) | 0 | 54/30 (72 pt) | 0 |
| G19's count (C) | passes on its grid | 2 cases undecided | passes on its grid | 54 cases undecided | passes above the gate |
| strict at 24 pt, the shipped fire's 5 (120 Hz) | not run (60 Hz only) | not run | kept, by construction (60 Hz only run) | kept (5) | kept (5 at 1x, 6 at 2x) |
| most flashes a second at a pixel, 32 / 72 pt (base, same grid) | 18 / 17 (17 / 18) | 15 / 18 (13 / 18) | 7 / 7 (15 / 18) | 11 / 9 (17 / 18) | 5 / 5, the cursor (15 / 15) |
| 13 pt frames (at 2x by construction) | identical at 1x | identical at 1x | identical at 1x | identical at 1x | identical at 1x (setting off) |
| 24 pt | changed (worst 13,646) | changed | identical at 60 Hz | identical | identical |
| its own acceptance | met on its grid | met on a reduced grid | met; its pixel check not met (as written: all but the sweep; on the definition's measure: the six particle trails) | met | met |
| 2.3.2 (AAA) | no | no | no | no | for the default trail, with the setting on, by turning it off |
| holds under an OS magnifier | no | no | for the repeats of the quad, sweep and liquid | no | only at sizes above the gate, where nothing but the cursor is drawn |
| touches | `trail.rs` | `trail.rs` | `trail.rs`, `baud-render` | `trail.rs` | `trail.rs`, `config.rs`, `app.rs`, a release of the glass crate |
| open | glyph veil (stage 2: shader, metallib, oracle references); burn time changed on 1,520 cases; 2x | as the 26 pt cap | particle overlap, ground in the app, glass; the CPU path; 2x | 54 strict failures at 72 pt; 2x | no trail above 24 pt; poll against a live toggle; most of 2x |

None was run above 72 pt, where the bare cursor alone fails from about 118
pt under every option (section 3.1).

### 4.7 Combinations

O4's floor can carry any of O1-O3 above the gate in place of Off, and
O1's cap and O3's limit compose. Dim Flashing Lights can be read with any
option, not only O4: it is a separate lever (Q3). No combination was built
or measured.

## 5. Found on the way, decided by no question

- **The quad and the sweep fail at 72 pt on shipped code** (section 3.1).
  The census did not run them; that measurement is owed.
- **Note AJ section 1.3 quotes the 2024 definition of motion animation**,
  which a 2025-06-27 erratum corrected (section 1.8).
- **~~The ghost trail has never been counted.~~** Counted since, by a
  `ghost` arm that runs the ghost's rules from `trail.rs`: it passes up to
  32 pt and fails at 72 pt, 1x and 2x, and takes O2's bound.
- **24 pt sits exactly on O2's, O3's and O4's 32 pt gate** (section 3.5); a
  font whose cell rounds one pixel taller puts 24 pt above it.
- **A configured `font_size` outside `PxSize::of`'s range stops the launch**
  (**C**, a reading, not run): `app.rs` 690 builds the renderer with
  `PxSize::of(cfg.font_size * scale as f32).expect("size")`, and
  `PxSize::of` refuses a size that is not finite, not above zero, or above
  512 px per em, while `config.rs` 506-507 takes any number.
- **The visual bell has no rate limit that was found** (**C**), so how often
  a program can make it flash the whole screen is unmeasured (section 3.4).

## 6. Red-first tests, whichever option

- The census harness's additions (the smear arms, the frame hash, the cell
  height in points) land first, with the bench's own controls, and the base
  re-run reproduces the committed census row for row.
- A `trail.rs` test per mechanism at a cell above the gate, beside a control
  at 17 pt that keeps today's behaviour, each seen red against the shipped
  driver: the prototypes' tests are on their branches.
- The acceptance: the census at 13, 24, 26, 32 and 72 pt, 1x **and 2x**, both
  rates, every arm, with no FAIL drawn or apart, and the 13 pt hashes equal
  to the base; and, if a larger size is set as governing (Q5), the
  sizes up to it.
- A census series that counts seven transitions in a second, beside the
  disjoint and strict ones, so G19's count can be decided (section 1.8).
- ~~A ghost-trail arm, counted before the ghost is claimed either way.~~
  Built: the census's `ghost` arm.
- ~~A census series over the glass page at alphas 179 and 153 on the GPU
  path.~~ Built: `FLASH_GLASS=1`, each frame over a white and a black
  desktop ([[source: al-obs-b76-glass]]), and how often the glass blanks
  a repeat pass over the recordings' screens ([[source: al-obs-b76-screens]]): on the glass a repeat pass rarely draws nothing, and a switch of trail keeps the second.

## 7. Unmeasured

Sizes above 72 pt (every option); 24 pt at 120 Hz under O1 (and under O2
except by construction); G19's count, for any case; grounds through the
glass; the ghost trail; the visual bell's rate; the bar and underline
cursors; any combination; any option's cost in time; O1's veiled glyph;
the launch splash's plasma under any option; whether macOS posts the Dim
Flashing Lights change to the local or the default notification centre;
whether a poll of the setting sees a live toggle; what kitty's dwell would
do here. The ghost was counted since, and grounds through the glass are
read at note AS's bound of a white and a black desktop, held by a test on
the CPU path and, since, by the census's glass series. A
coloured desktop is not read, and neither is a grey one between white
and black: a glyph's edge over such a desktop is a mixture inside the
triangle of text, page over white and page over black, and only that
triangle's edges are scanned. O2's 2x above 24 pt, 72 pt at 2x, text under the trail and its
CPU path were measured since ([[source: al-obs-b68-analysis]]).

## 8. Open questions

Each comes with a visual; each option is put with the same measures, from section 4.6.

1. **Which option, or which combination?** On its own grid, each has 0 FAIL
   on the disjoint reading up to 72 pt, and none was run above. The same
   five measures for each (section 4.6 has the rest): what it does and on
   how many cases; the most flashes a second at a pixel at 32 / 72 pt,
   against the base on the same grid; the strict reading; what changes
   below the gate and the look above it; what is open or owed.
   - **O1** draws every trail at its size on a 26 pt cell (4,080 cases; a
     29 pt cap on a reduced 2,400). Flashes a second 18 / 17 (base 17 /
     18). Strict: none under the 26 pt cap, 2 at 72 pt under the 29 pt
     cap; 24 pt at 120 Hz not run. Changes from 21 pt at 1x (20 pt at 2x);
     above, a small trail at the middle of a large cell. Owed: the glyph
     veil's stage 2 (a shader change), and the burn time changed on 1,520
     cases.
   - **O2** dims repeat passes above a 32 pt cell (3,696 cases). Flashes a
     second 7 / 7 (base 15 / 18). Strict: none on its grid; the shipped
     fire's 5 at 24 pt kept by construction (not run at 120 Hz). Nothing
     changes up to 24 pt; above, a repeat pass is a faint ghost. Owed: its pixel
     check failed on the six particle trails (and, as the design wrote it,
     on all but the sweep); the ground in the app, text and glass are open.
   - **O3** lights at most once a second above a 32 pt cell (the full
     9,600). Flashes a second 11 / 9 (base 17 / 18). Strict: 54 at 72 pt
     (30 apart), and the shipped fire's 5 at 24 pt kept. Nothing changes up
     to 24 pt; above, one flame a second and a bare cursor between. Owed:
     those strict cases are undecided on G19's count.
   - **O4** draws no trail above a 32 pt cell (2,928 cases, some 2x).
     Flashes a second 5 / 5, the cursor's own cells (base 15 / 15). Strict:
     none above the gate; the shipped fire's 5 at 24 pt (6 at 2x) kept.
     Nothing changes up to 24 pt; at 25 pt and above, no trail of any kind.
     Owed: a release of the glass crate, and whether its poll sees a live
     toggle.

   Combinations are unmeasured. Reading Dim Flashing Lights is a separate
   lever, in Q3.
2. **Every effect, or per effect?** On the shipped driver only the fire and
   the orb fail at 32 pt. At 72 pt the fire, orb, plasma, liquid, quad and
   sweep fail, and so does the ufo as drawn, but every one of the ufo's
   failures is the cursor's own cell (0 apart). The lightning and the ice
   never fail on the disjoint reading; on the strict reading at 72 pt the
   ice fails 27 cases (O3's base) and the lightning 3 (all on O2's
   three-cell motion on the light page, and 0 apart). Each prototype acts on
   every trail; a per-effect gate or cap is unmeasured.
3. **Should a system setting govern, and which?** It is a lever separate
   from Q1: any option can read one. Dim Flashing Lights (O4 builds it;
   Apple's header and HIG scope it to video, its article to "an animation
   sequence with flashing lights in a game or app"; whether a poll sees a
   live toggle is untested), Reduce Bright Effects (no Mac label found), or
   Reduce Motion alone (already read). Linux draws no trail today and its
   settings speak only of motion; if the trail comes to Linux, whether the
   setting's behaviour follows it is part of this question.
4. **The default trail.** The plasma became the default on 2026-09-29, replacing the fire; it was not yet built (note AJ's section 3.2). So the shipped default is still the fire. (Built since,
   2026-10-10: the plasma is the shipped default, the fire kept by name, the flash bounding unchanged. The questions below stand.) Both, on the
   shipped driver (**M**; disjoint drawn/apart, then strict):

   | size | fire | plasma |
   |---|---|---|
   | 13 pt | pass (worst 10,115), strict 0 | pass (worst 8,274), strict 0 |
   | 24 pt (1x, O3's base) | pass (18,675), strict 5 | pass (15,018), strict 0 |
   | 26 pt (1x, O3's base) | pass (19,888), strict 11 | pass (15,875), strict 0 |
   | 32 pt, 1x and 2x | FAIL 6/4 and 5/4, strict 26 and 25 | pass (20,071), strict 6 at each (4 apart) |
   | 72 pt, 1x | FAIL 64/62, strict 96 | FAIL 30/25, strict 44 |

   Under the options at 72 pt the plasma fails nothing on the disjoint
   reading; on the strict reading O3 leaves 12 (6 apart) and the others
   none. The questions: does the 2026-09-29 decision stand with these
   figures; does the option chosen in Q1 apply to the default as built at
   this note's date (the fire) or as decided (the plasma, built since
   2026-10-10); and does it also cover note AJ's launch splash, which
   reuses the plasma over a logo-sized area that no option measured?
   EN 301 549 4.2.9 asks for a default mode that
   "minimizes the potential for triggering photosensitive seizures";
   whether that means passing 2.3.1 or no trail by default is not said.
5. **Which sizes must pass?** Two separate questions, since the
   Understanding page asks for "the largest scale at which a user may view
   the content" and "the standard zoom level":
   - **A configured font size, with no zoom.** `font_size` reaches 512 px
     per em. If such sizes govern, every option is unmeasured above 72 pt,
     and from about 118 pt the bare cursor alone fails under every option
     (**C**, and **R** for whether a program's jumps count).
   - **A zoom step.** The View menu zooms to 72 pt. If zoomed sizes govern,
     seven trails fail at 72 pt today; if only the default size governs,
     nothing fails today.
6. **The gate**, if O2, O3 or O4: a 32 pt cell, where 24 pt at 1x sits
   exactly on it; and **O1's cap**, 26 pt or 29 pt (the 29 pt cap ran a
   reduced grid and fails 2 strict cases at 72 pt), and whether to build its
   stage 2.
7. **Is 2.3.2 (Level AAA) a goal?** No option meets it for every trail. Dim
   Flashing Lights meets it for the default trail's own pixels by turning
   the default off, which any option could do on that setting. And a
   program that jumps the cursor every 100 ms flashes the cursor's own cells
   5 times a second under every option, if that counts as the ICT's own
   flashing (**R**).
8. **Which reading governs a verdict**: the census's disjoint pairing (fails
   at eight transitions in a second), G19's half-flashes (seven), or the
   strict chained reading (five)? On the disjoint reading every option
   passes on its grid. On the strict reading O1 at 26 pt and O2 pass on
   theirs, O1 at 29 pt fails 2, O3 fails 54 at 72 pt, and the shipped fire
   fails 5 at 24 pt, which O2, O3 and O4 keep. On G19's count every one of
   those strict failures is undecided until a census series counts seven.
9. **The ghost trail**: count it first, then the same option, or O4's floor.
   (Counted and bounded since, 2026-10-17: it failed at 72 pt only, and
   takes O2's bound, its sweeps weighed on a ledger of their own, the recommended answer of that day)
10. **May a bound override a trail the config names?** All four prototypes
    do, above a size: O1-O3 change named trails, and O4 removes them, the
    ghost included. The rule of 2026-09-25 keeps a named trail over Reduce Motion ("A typo must not override an accessibility setting", as
    the config reader puts it), and O4 extends that rule to Dim Flashing
    Lights below the gate. The reading that Level A leaves no room for an
    opt-in (section 1.2) is this note's, not a text's (**R**). Does a size
    bound outrank a named trail, and does a system setting?
