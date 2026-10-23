# AB. Right-to-left text

**Status: research. Bidi is not built; phase 0 of per-script shaping is (Q6's postscript), and section 11 records the four choices it left: Jev decided three, and the owner picked C2 for the fourth on 2026-10-13, built 2026-10-14 (11.4).** This note brings together five research reports and a critic's checks of them, all taken on 2026-10-06 and 2026-10-07. The five reports cover:

- the specifications;
- terminals that do bidi;
- terminals and applications that do not;
- full text engines;
- where RTL would live in baud.

Every baud citation is at e138e34 on branch `smooth-scroll`, not master, so line numbers may differ on master [measured: `git rev-parse HEAD`].

Evidence is tagged as follows:

- **[source]**: file:line at a named commit, or a section of a pinned spec.
- **[doc]**: a URL and what it says.
- **[measured]**: a command and its output.
- **[inferred]**: reasoning from the above.

Where the critic found that a claim does not hold, this note gives the corrected form and marks it "(corrected)". The working files were copied out of the session scratchpad and are kept untracked in `.claude/research/rtl/research/<key>/` (scratchpad paths below map there), with the Windows deep dive in `.claude/research/rtl/windows/`. The sources in section 9 are not yet pinned in the manifest.

## 1. What the owner asked

**What the owner said.**

- The owner asked for RTL support, "the best one - windows level", with research "beyond vts". At first they said not to start until they gave the word [source: docs/research/notes/aa-shell-locale.md:13-15 @e138e34].
- On 2026-10-07 the owner said: "yes start the rtl research", "also you drew it ltr not rtl which is how hebtrew is written", "and rtl is a must have feature for rtl langs", and "and rtl we need to see how windows does it , windows terminal , etc, windows has the best hebrew support and we need to do it better" / "best rtl support that is sorry" (TODO.md, verbatim).
- The same day the owner answered Q22 below: "and yeah if these symbols aren't found in the configured font then it should look at the system's font" (note AC records the font order it set).

**Where baud stands today.**

- There is no bidi. Hebrew draws in logical order, left to right [source: baud-core/src/paint.rs:651-731 @e138e34].
- Shaping selects the `latn` LangSys, else `DFLT`, for every face whatever the script [source: baud-font/src/layout.rs:713-717 @e138e34]. Some faces, such as Liberation Mono, put their Hebrew mark anchors only under `hebr`. With those faces the vowel points land one cell to the right [source: aa-shell-locale.md:181-186, 248-253].
- Note J's D15 recorded "no bidi (a grid-level concern per the terminal-wg spec, owned by the VT core's mode bits if ever)" and "No Arabic joining" [source: docs/research/notes/j-rendering.md:244-247 @e138e34].

## 2. The specifications

### 2.1 What each source is, and its status

| source | status | how much was read |
|---|---|---|
| terminal-wg BiDi recommendation (Egmont Koblinger) | v0.1 draft, 2019-01-29, CC-BY 4.0. It calls itself "work in progress" that "might receive backward incompatible updates" [source: about.html; index.html Foreword]. Repo HEAD is cc8686a (2019-01-31) with one tag, 0.1 (40de51d). It has 10 issues, all open, and 0 merge requests; last activity was 2025-01-20 [measured: GitLab API; `git ls-remote`]. | all 35 pages and all 10 issue threads |
| ECMA-48 5th edition (1991) | standard, already pinned (`ecma-48-5th-1991.pdf`) | §6.1, §7.1-7.4, §8.3.106/108/111/114/120/125/130/137 and Annex F, through `scripts/extract-ecma48.py` with its positive controls passing [measured] |
| ECMA TR/53 2nd edition (1992) | technical report | decoded with a local LZW reader. The 1st edition (1990) was not read. |
| UAX #9 | rev 52 (Unicode 18.0.0, 2026-09-01) is current. Rev 51 (17.0.0) is the version whose bytes the manifest pins. | both revisions |
| UTS #55 | rev 5 (2024-01-29) | in full |
| UTC Text Terminal Working Group | not standards: the proposal L2/23-107 and the working-group report L2/23-194. The opposition paper L2/23-153 was fetched but not read. | see 2.6 |
| DEC VT510 manual | vt100.net transcription | DECRLM, DECRLCM, DECHEM, DECHEBM, chapter 2 |
| xterm ctlseqs and FAQ | ctlseqs is already pinned | searched, with positive controls |

**One existing pin is broken and one is incomplete** [measured; repairs in 9.0]:

- **`uax9-bidi`** points at the unversioned URL `https://www.unicode.org/reports/tr9/`. That URL now serves rev 52 (3c281b38…, 188,933 B). The pinned hash 25ebbc5b… (187,487 B) is exactly `tr9-51.html`, so re-fetching the pinned URL fails its hash.
- **`terminal-wg-bidi`** still verifies, but it covers only the index page (5d5f728a…, 22,225 B, still byte-identical on 2026-10-06), which holds no requirements, only a Foreword and Conventions; every requirement is on the subpages. Both entries also give `year = 2025`, though the terminal-wg document dates from 2018-2019 [source: MANIFEST.toml:2939-2949, 3014-3024].

**VTE's version claim.** VTE's source says it implements "Terminal-wg/bidi v0.2" [source: VTE src/bidi.cc header @56bad78]. No v0.2 was ever published: the spec repo has only `master` and tag `0.1` [measured: `git ls-remote`]. What VTE's "v0.2" contains could not be confirmed. It may include unpublished changes, such as forcing DCSM as issue #9 proposes.

**How the spec was received.** Koblinger announced it on the Unicode list on 2019-01-29 under "Proposal for BiDi in terminal emulators" [doc: unicode.org/mail-arch/unicode-ml/y2019-m01/0335.html]. It drew about 100 replies under that subject in January-February 2019, and about 100 more in spin-off threads [measured: unicode-ml by-date indexes]. Richard Wordingham's shaping objections led to issue #7. Eli Zaretskii (Emacs) argued, in the spin-off thread "Bidi paragraph direction in terminal emulators", that per-line autodetection gives "completely random" results, and that led to #8 [doc: y2019-m02/0052.html]. The published text was never revised.

### 2.2 The terminal-wg MUST/SHOULD list

The strength column uses the spec's own wording. Where the spec states something as fact with no modal verb, the entry says "stated". Page names are relative to https://terminal-wg.pages.freedesktop.org/bidi/.

| # | requirement | strength | source |
|---|---|---|---|
| 1 | A paragraph is the run of rows between hard newlines, and soft-wrapped rows belong to it. An emulator without the concept "will have to introduce and track it". Koblinger in issue #2: "Not 'could' but 'should', or 'has to'". | must (effectively) | recommendation/paragraphs.html; issue #2, 2019-01-30 |
| 2 | Each paragraph stores three properties: implicit or explicit, autodetect on or off, and LTR or RTL. That gives 6 stored modes and 4 layouts (implicit LTR, implicit RTL, explicit LTR, explicit RTL); autodetect is ignored in explicit mode. The properties are per paragraph, not global (issue #1: "No, absolutely not!") and not per line (issue #3). | stated | recommendation/basic-modes.html |
| 3 | BiDi is display-only. The model is unchanged except that it gains paragraphs and a few bits per paragraph. Cells move only within their own row, though where they move depends on the whole paragraph. In existing documentation, "left" and "right" now mean towards preceding and towards following model columns. | stated | recommendation/display-only.html |
| 4 | Implicit mode is the default: "contrary to ECMA TR/53 and ECMA 48, this implicit mode must be the default." | must | basic-modes.html |
| 5 | At level 1, "BiDi control characters are discarded". That covers all twelve: LRM, RLM, ALM, LRE, RLE, LRO, RLO, LRI, RLI, FSI, PDF and PDI. | must (at level 1) | basic-modes.html; future-improvement-ideas/implicit-level2.html |
| 6 | Implicit LTR or RTL: run UBA over the whole paragraph with that paragraph direction, place cells by the logical-to-visual map, and align to the left or right edge. On mirroring: "counterpart glyphs are used ... according to the BiDi algorithm". "Arabic shaping is performed on the paragraph, before splitting to lines." | stated | basic-modes.html |
| 7 | Explicit RTL lays cells out "in the exact reverse order" and mirrors mirrorable glyphs. The emulator "doesn't perform shaping either". Issue #7, still open, reverses that last point (see the open issues below and Q13). | stated, contested | basic-modes.html; issue #7 |
| 8 | There is no separate alignment concept: each paragraph aligns according to its direction. | stated | bidi-intro/rtl-bidi-text.html |
| 9 | Trailing erased cells map to the end of the line in the paragraph direction. Trailing erased cells in the middle of a paragraph, left by a resize or an early wide-character wrap, "should be ignored when feeding the paragraph's contents to the BiDi algorithm" and "should also be skipped when rendering". The spec covers only *trailing* erased cells; erased cells between characters are not specified (Q8). | should | recommendation/erased-cells.html; critic |
| 10 | A combining mark stays in its base's cell: "No combining accent should move to another cell for display purposes, not even with BiDi." Spacing marks (Mc) are undecided. | should | recommendation/combining.html |
| 11 | Images "must not be flipped by default in RTL mode", and their alignment "should be revised" so that it mirrors. An inline image counts as U+FFFC. | must / should | recommendation/misc.html |
| 12 | Box drawing (U+2500-257F), block elements (U+2580-259F) and "presumably many other characters" are not mirrored. terminal-wg's `?2500` (VTE's experimental mode) mirrors only U+2500-257F; extending it to block elements or other glyphs is left as an open question. mintty's `?2500` covers U+2500-259F (section 3). | stated / may | misc.html |
| 13 | Titles and notifications may be passed through unchanged. OSC 8 link targets are always LTR and in logical order. | may | misc.html |
| 14 | Placing the scrollbar and padding is the terminal's own choice. UBA should run only when the screen updates, and caching and an LTR-only fast path are allowed. Algorithm tweaks are allowed only as non-default settings. What to truncate on resize is unspecified. | should / may | misc.html |
| 15 | Long paragraphs may be capped, for example by rendering paragraphs over 500 lines as explicit while keeping their direction. With finite scrollback, extra hidden lines may be kept for context. | may | misc.html |
| 16 | The cursor follows its character. "At the very least", the I-beam moves to the right side of its cell in explicit RTL and over an RTL-resolved character. In the pending-wrap state the spec prefers ("more consistent") that the cursor disappear. | should; preference (pending wrap) | recommendation/cursor.html |
| 17 | "whenever a mouse event is reported back to an app, it undergoes the inverse transformation." | stated | display-only.html |
| 18 | Selection works on the model, and copying keeps the original codepoint, never the mirrored glyph. The spec calls mlterm's selection of a visual region "Bizarre". | should | display-only.html (selection on the model); bidi-intro/rtl-bidi-text.html (copying keeps the codepoint); prior-work/terminals.html ("Bizarre") |
| 19 | Arrow-key swapping, `CSI ? 1243 h/l`, "(perhaps temporary)": a global mode, on by default, that swaps Left and Right "whenever the cursor is inside a RTL paragraph", using the autodetected direction. "There must be a way to turn off this mode." Apps save it, disable it and restore it on exit, and "Emulators that don't implement saving and restoring should go ahead and implement it." "Home and End should not be swapped." | must / should | recommendation/arrow-key-swapping.html |
| 20 | The normal and alternate screens "should behave identically". | should | future-improvement-ideas/ecosystem.html |
| 21 | There is no escape sequence for the conformance level; a terminal uses the highest level it implements. | should | escape-sequences.html |

**Escape sequences** [source: recommendation/escape-sequences.html]:

- **BDSM.** `CSI 8 h` selects implicit and `CSI 8 l` selects explicit. The default is the high state, so "a generic reset operation would set this flag".
- **SCP: `CSI Ps1 ; Ps2 SP k`.**
  - Ps1 is 1 for LTR and 2 for RTL. The spec adds 0, "the terminal's default". Both parameters default to 0.
  - Ps2 values 1 and 2 "don't need to be supported", and 2 "should definitely not be supported".
  - SCP applies to implicit and explicit paragraphs alike, and also sets the fallback direction.
  - "Apps mustn't rely on the default being LTR."
- **SPD: `CSI Ps SP S`.** Apps "should not emit" it. A terminal may treat SPD 0 and SPD 3 as aliases of SCP 1 and SCP 2.
- **`?2500` (box mirroring) and `?2501` (autodetect).** Both numbers are "(temporary)" and "not meant to be final". Autodetect defaults to off, a choice the spec itself rests on "a few weak arguments" and says "might change".
- **How the sequences apply** is marked "[draft]":
  - A sequence takes effect on a paragraph only when the cursor is at the paragraph's first logical position.
  - A linefeed into a new paragraph gives that paragraph the current modes.
  - When a wrap joins two paragraphs, the first one's properties survive.
  - ED 0, 1 and 2 turn the affected rows into one-line paragraphs.
  - SU and SD make every boundary hard and give inserted lines the current modes.
  - When DCH or EL splits a paragraph, both halves keep its properties.
  - A new bottom line created by scrolling gets the current modes.
  - Cursor movement never applies modes (issue #3).

**Open issues.** None has been folded into the text [source: GitLab issue threads, saved JSON]:

- #1: mintty reads SCP 0 as autodetect. Koblinger rejected that: "An escape sequence of three possible parameters cannot encode four possible values."
- #3: should DECSC/DECRC save the BiDi modes? Koblinger: "Probably restoring, yes. Whichever, it should indeed be part of the spec."
- #4: LAM-ALEF width. mintty (2019) argued for a locale-consistent 2-cell default with an optional mode; Koblinger answered that shaping would be reworked. On #7 (2019-02-22) Koblinger wrote that wcwidth "must be respected", so the ligature takes 2 cells. On #4 (2025-01-20, the repository's last activity) mintty announced an optional single-cell LAM/ALEF mode under DECSET 2521.
- #5: UAX #9 does specify autodetection (P2/P3, which HL1 may override). Koblinger accepted the correction.
- #6: a third SCP parameter for autodetection.
- #7: shaping in explicit mode.
- #8: autodetect per segment (separated by blank lines or prompts) rather than per paragraph.
- #9: "DCSM forced to DATA (SET state)".
- #10: DECSTBM should hard-terminate the rows at the region's boundaries.

### 2.3 ECMA-48 controls, and where terminal-wg departs from them

All rows: [source: docs/research/sources/ecma-48-5th-1991.pdf via scripts/extract-ecma48.py, positive controls passing].

| control | sequence | ECMA-48 | terminal-wg |
|---|---|---|---|
| BDSM §7.2.1 | SM/RM 8 | Reset is EXPLICIT. Set is IMPLICIT, where "all bi-directional aspects of data are handled by the device itself". | Kept, with the default inverted to implicit and the scope set per paragraph; ECMA does not say what the scope is. |
| DCSM §7.2.3 | SM/RM 9 | Contradicts itself (see below). | The recommendation pages do not specify DCSM. The TR/53 review calls presentation-component operation "a terrible idea". Open issue #9 proposes revoking DCSM with "DCSM forced to DATA (SET state)", and VTE does this. |
| SCP §8.3.111 | `CSI Ps1;Ps2 SP k` | "No parameter default values". Ps1: 1 is LTR, 2 is RTL. Ps2: 0 is implementation-dependent, 1 updates the presentation line from the data line, 2 does the reverse. New in the 5th edition (Annex F.6). | Kept. Adds Ps1 = 0, makes both defaults 0, refuses Ps2 = 2. |
| SPD §8.3.130 | `CSI Ps1;Ps2 SP S` | Defaults 0;0. Applies to the whole presentation component: 0 is horizontal LTR, 3 is horizontal RTL. | Discouraged; aliases allowed. |
| SDS §8.3.114 and SRS §8.3.137 | `CSI Ps ]` and `CSI Ps [` | Mark nested directed or reversed strings in the data component. The active position inside such a string is undefined. | Rejected, because they "cannot separate two adjacent pieces of text having the same resolved embedding level" (ecma-tr53-review.html; issue #1). |
| SIMD §8.3.120 | `CSI Ps ^` | Sets the direction of implicit movement, and with it whether CR and NEL go to line home or line limit (Annex F.7). | Rejected. |
| SAPV §8.3.108 | `CSI Ps... SP ]` | Under an RTL path, 3 mirrors paired characters and 4 mirrors math operators. Values 5-8 force Arabic letter forms. 13 and 14 turn on contextual shaping, with and without LAM-ALEF. | Mentioned only as future work. |

ECMA-48 also says three general things about modes [source: §7.1, §8.3.105; ECMA-48 flat offsets ~61248 and ~130650, per the critic]:

- modes are deprecated;
- it recommends that each mode's reset state be its initial state (§7.1);
- RIS returns the device to its initial state, which "may imply, if applicable", setting modes to their reset state (§8.3.105).

**Disagreements, with both sides named.**

- **DCSM's reset state.**
  - ECMA-48 §7.2.3 and Table 6 give reset = PRESENTATION and set = DATA.
  - ECMA-48 §7.3.4 c) says "DATA (the reset state)".
  - TR/53 §7.1 repeats "DATA (the reset state)". But its advice that "BDSM should be set to EXPLICIT and DCSM to PRESENTATION" to keep LTR behaviour reads most naturally if PRESENTATION is the reset state [inferred].
  - terminal-wg #9 and VTE treat DATA as SET. VTE's comment reads: "DCSM defaults to RESET in ECMA, forced to SET in Terminal-wg/bidi" (`src/modes.py:188` @56bad78).
  - Any DECRQM answer for mode 9 depends on which reading baud takes.
- **What a reset does to BDSM.**
  - ECMA-48 §8.3.105: RIS returns the device to its initial state, which "may imply, if applicable", setting modes to their reset state. §7.1 recommends that a mode's reset state be its initial state.
  - terminal-wg makes BDSM's initial state the set state (IMPLICIT), and a generic reset makes BDSM implicit.
  - VTE and iTerm2 follow terminal-wg [source]. VTE's BDSM defaults to true, and its DCSM defaults to true and cannot be written (modes.py:175-189). iTerm2's commit 91411f5 says the modes are "restored by a terminal reset".
  - A terminal that follows terminal-wg and has RIS return to IMPLICIT conforms to RIS, and departs only from §7.1's recommendation. That departure is what baud has to record.
- **TR/53's Worked Example 4.**
  - terminal-wg reads its end as showing that "implicit mode ignores control characters, even SCP".
  - TR/53's Note A.2 says only that "the absence of bi-directional control functions allows only limited results. The absence of SCP, for instance, did not allow to have paragraphs of different presentation directions".
  - The note describes that example's data stream, which contains no bidi controls; it does not state a rule. The terminal-wg reading is an interpretation [source: TR/53 2nd edition, decoded offset 37803; ecma-tr53-review.html].

### 2.4 UAX #9

- **The unit.**
  - The algorithm separates paragraphs (P1) and finds each paragraph's level (P2-P3). It then resolves explicit levels (X1-X10), and weak types, neutrals and implicit levels per isolating run sequence (W1-W7, N0-N2, I1-I2). Finally it reorders (L1-L4).
  - L1-L4 "act on a per-line basis and are applied after any line wrapping". "Paragraphs may also be determined by higher-level protocols."
  - max_depth is 125 [source: tr9-52 §3, §3.3, §3.4].
- **Direction detection.**
  - P2 finds the first L, AL or R character, skipping isolate contents. P3 sets level 1 if that character is AL or R.
  - HL1 lets a higher-level protocol set the level outright, use P2/P3 with an RTL default, or use some other heuristic, including looking at neighbouring paragraphs.
  - Mapped onto terminal-wg: "autodetect with LTR fallback" is exactly P2/P3. "Autodetect with RTL fallback" is the HL1 bullet that defaults to level 1. SCP 1 and SCP 2 are HL1 setting the level outright [source: tr9-52 §3.3.1, §4.3 HL1].
- **The clauses a terminal relies on.**
  - Hard newlines delimit paragraphs under P1 and §3 ("Paragraphs may also be determined by higher-level protocols"). terminal-wg's finite-scrollback "hidden context lines" fit HL5.
  - terminal-wg's 500-line cap renders long paragraphs as explicit mode, that is, not reordered by UBA, and no HL1-HL6 permits that, so the cap departs from UAX9-C1 [inferred]. Running UBA over a truncated segment under HL4/HL5 would be a conformant alternative [inferred].
  - HL6 allows mirroring characters that lack Bidi_Mirrored, which box mirroring needs [source: tr9-52 §4.3].
- **Marks and mirroring.**
  - L3: after reversal, marks come before their RTL base, and must be swapped back if the renderer expects them after it.
  - L4: "A character is depicted by a mirrored glyph if and only if (a) the resolved directionality of that character is R, and (b) the Bidi_Mirrored property value of that character is Yes" [source: tr9-52 §3.4].
- **Shaping.**
  - Shaping applies logically after I2, within a level run [source: tr9-52 §3.5].
  - §3.5.1 takes line-break widths from shaped glyphs. A grid uses fixed per-codepoint widths instead [inferred]. In terminal-wg #7 Koblinger wrote that wcwidth "must be respected". #4's opener favours a 2-cell default, and mintty now offers a 1-cell mode (`?2521`).
- **Conformance.**
  - UAX9-C1 requires display "in the order described by Section 3".
  - UAX9-C2 allows only HL1-HL6 as higher-level protocols.
  - §4.2 says conforming systems "generally" fall into four classes. The lowest that reorders at all is "implicit", meaning the algorithm plus LRM, RLM and ALM [source: tr9-52 §4, §4.2].
  - A terminal that reorders but discards LRM, RLM and ALM, as terminal-wg level 1 directs, therefore fits none of the four [inferred].
- **Tests.**
  - BidiTest.txt (18.0.0) has 490,846 data lines. It tests bidi classes, not characters, and assumes a single paragraph, a single line and no brackets; L3 and L4 are not tested.
  - BidiCharacterTest.txt (18.0.0) has 91,707 lines. It covers brackets, including nesting at depths 62, 63 and 64.
  - "Implementations are expected to correctly cover all test cases in both test files" [source: BidiTest.txt:19 (18.0.0)]. The line counts above: [measured: `grep -v -E '^(#|@|\s*$)' … | wc -l`].
  - The data in both files is byte-identical between 17.0.0 and 18.0.0 apart from comments [measured: diff of non-comment lines].
- **Box drawing.**
  - All 160 code points in U+2500-259F are bidi class ON with Bidi_Mirrored = N, and none appears in BidiMirroring.txt. L4 never mirrors them. HL6 may. terminal-wg's `?2500` covers only the 128 box-drawing code points; mintty's covers all 160.
  - Positive control: U+0028 is ON, is mirrored, and maps to U+0029 [measured: DerivedBidiClass, UnicodeData and BidiMirroring, all 18.0.0].
- **Changes from 17.0.0 to 18.0.0 (corrected).**
  - BidiBrackets grows from 128 to 130 entries, adding U+2E62/2E63.
  - BidiMirroring grows from 428 to 438 data lines. The ten new lines are five pairs: 221D↔1DB10, 2E62↔2E63, 1DB03↔1DB04, 1DB05↔1DB06 and 1DB08↔1DB09 [measured: data-line diff of BidiMirroring 17.0.0 (a2f16fb8…) against 18.0.0 (cd54810e…). The spec research had listed only three pairs].
  - Rev 52's other changes are editorial: new wording about digits, and a warning that Bidi_Mirroring_Glyph does not preserve size in UTF-16 (U+221D maps to U+1DB10) [source: tr9-52 Modifications].
- **Terminals.** UAX #9 rev 52 never mentions terminals, monospace or the console: those terms get 0 hits, against 106 lines for the control term "paragraph" [measured: grep over the tr9-52 text].

### 2.5 UTS #55 and the level-1 rule

**What UTS #55 assumes.** UTS #55 says that terminals, being unaware of lexical structure, cannot be expected to apply its source-code ordering (§4.1). Instead the source itself should carry marks such as LRM, inserted by tools (§5.2) and allowed by the language, so that code echoed in a diagnostic is "displayed correctly even in the language-unaware terminal" [source: tr55-5.html]. That works only on a terminal that runs UBA and honours LRM [inferred].

**Where it conflicts.** It conflicts with the terminal-wg *text*, whose level 1 discards LRM. It does not conflict with VTE for any bidi control that follows a base character in the same paragraph:

1. VTE gives width 0 to U+200E/F, U+202A-202E, U+2066-2069 and U+061C [source: VTE src/unicode-width.hh @56bad78, generated from `g_unichar_iszerowidth` by unicode-width-generate.py].
2. It combines every width-0 character onto the previous cell. At column 0 it reaches into the previous soft-wrapped row [source: vte.cc:3435-3480 @56bad78].
3. bidi.cc:214 and :522 then pass every codepoint in the cell's string to FriBidi [source: bidi.cc @56bad78; vteunistr.cc:165-175].

So VTE stores such a control on the previous cell (vte.cc:3435-3480) and passes it to FriBidi in logical order. Controls that open a paragraph (column 0 after a hard newline) are dropped. VTE therefore implements most of the level-2 idea, but not its paragraph-start storage or its attach-to-next rule for opening controls [inferred from source; not run]. baud is in the same position, including the column-0 drop (Q4; paint.rs:766-769).

### 2.6 The UTC Text Terminal Working Group

**L2/23-107, "Proper Complex Script Support in Text Terminals"** (Renzhi Li, Dustin Howett, Peter Constable; the L2 register lists "Renzhi Li, et al", 2023-04-24) is the April 2023 proposal that led UTC #175 to create the working group. It is not a working-group draft, and L2/23-153 formally opposed its specifics (L2/23-194). Only Howett is tied to Windows Terminal in the evidence (facts.json); the other authors' affiliations are unverified.

What it asks for [source: L2/23-107, extracted text, sections "BiDi and Cursor Movement" and "Selection and Hit Testing"]:

- The screen buffer must keep paragraph boundaries ("BiDi embedding level analysis is done per-paragraph").
- It must keep embedding levels, recalculated whenever a paragraph's text changes.
- It must keep "special controlling data (such as an embedding level override)" for TUIs that lay out text themselves.
- "Reordering is done line-by-line."
- On the cursor: "While doing text writing is mainly using the logical position, the cursor movement operates at visual level, i.e., when the terminal decides to move the cursor upward, it will need to look up a logical position in the previous line that has the same column in its visual counterpart."
- A selection may be a logical range or a visual rectangle. Both need hit testing that converts visual positions to logical ones.

**The rest of the record:**

- L2/23-194 is the working group's report to UTC #176 [doc: unicode.org/L2/L2023/23194-text-terminal-wg-report.pdf].
- The Unicode blog for July 2025 says the group was created at UTC #175 and that, after a slow start, "the working group is now meeting regularly" [doc: blog.unicode.org/2025/07/].
- A post to the C++ SG16 list on 2023-12-07 says the author of the opposition paper, L2/23-153, "has since joined the group" [doc: lists.isocpp.org/sg16/2023/12/4015.php].
- L2/23-153 could not be text-extracted and is unread.
- The Windows deep dive read the group's later reports [doc: windows/wt/docs; facts.json]: L2/24-264 (2024-11-06), L2/25-179 and L2/25-263 (2025-10-24), all by Fraser Gordon (chair). They split applications into simple and complex, plan versioned escape-sequence negotiation, and keep drafts in a private GitHub repo. On cursor movement, 25-179 says only that it "should be defined" and calls display versus logical order an open complication. It calls UAX #9 support "at least in part ... a prerequisite". 25-263 then records "a desire in the group to not be dependent on terminals supporting UAX 9". None of the three adopts L2/23-107's visual cursor model, and the drafts themselves are private and unread.

**Disagreement.** L2/23-107 makes terminal cursor movement visual. terminal-wg keeps the model, and every cursor movement in it, logical. There "left" means towards preceding model columns, and "in implicit mode we cannot implement visual walking of the cursor ... the movement is always logical"; arrow-key swapping is offered instead [source: display-only.html; arrow-key-swapping.html]. L2/23-107 is a proposal and terminal-wg a v0.1 draft; neither is final.

### 2.7 DEC and xterm

**DEC's RTL support was visual-order.**

- DECRLM (`CSI ? 34 h/l`, VT510) makes the cursor move right to left. BS moves right. CR and NEL go to the rightmost column. With DECAWM, wrapping goes to the right margin of the next line. Column numbering does not change [source: vt100.net/docs/vt510-rm/DECRLM.html].
- The vt100.net DECRLM page lists ICH/DCH/ED/EL/DECSED/DECSEL/ECH and CUP/HVP/CUF/CUB under two identical "not affected" headings (lines 127 and 141). One of them is probably a transcription error, so the manual's intent is unconfirmed [source].
- Related modes: DECRLCM `?96` (right-to-left copy), DECHEM `?36` (Hebrew encoding) and DECHEBM `?35` (Hebrew keyboard) [source: vt100.net pages; chapter2.html §2.5.8-2.5.9].
- DECRLM changes the data layer, which conflicts with terminal-wg's display-only model, and the terminal-wg spec never mentions it [inferred; measured: grep for DECRLM over the spec text returned rc=1, while the control "DECSET" hit 6 times (5 on escape-sequences, 1 on misc)].

**xterm documents no bidi.**

- ctlseqs has no SCP, SPD, BDSM, `?2500`, `?2501` or `?1243`, and none of DEC's `?34` or `?96`. Its `?35` is rxvt font-shifting, which collides with DECHEBM [measured: docs/research/sources/xterm-ctlseqs.txt. Positive controls: SL/SR at lines 589/594, DECSCUSR at 1545, and `1 0 4 9` with three hits, two of them the `Ps = 1 0 4 9` mode lines].
- xterm patch #411 ignores DECRLM (`case srm_DECRLM: /* ignore */`, charproc.c:8035, 8389, 8837).
- Its README.i18n says xterm "does not support bi-directional or RTL languages". That file is Tomohiro Kubota's 2003 document, not a statement by Dickey [source: xterm-411.tgz README.i18n:1, 195-197; xterm.log.html:11507]. No current statement from the maintainer was found.

**Who wants the DEC modes.** j4james, a Windows Terminal collaborator, argues for DEC's RTL modes over the VTE proposal (WT #538, 2020) [doc]. Contour accepts DECRLM, DECHEM and DECRLCM and reports them truthfully, but "nothing reads the bit" [source: contour docs/internals/vt-conformance.md:130-137 @d8ce17b].

## 3. What terminals do

Every row was read at the named commit unless marked otherwise. "Unit" is the span of text UBA is resolved over.

| terminal | implicit / explicit | sequences | direction | unit | shaping | defaults | library | known problems |
|---|---|---|---|---|---|---|---|---|
| **VTE** 56bad78 (GNOME Terminal b23813a is a front end) | both, per paragraph | BDSM, SCP (0 = LTR, with a FIXME), SPD 0/3, `?2500` (U+2500-257F only), `?2501`, `?1243` | SCP; P2/P3 when `?2501` is on | paragraph (soft wraps joined), capped at 500 rows, beyond which it falls back to explicit | FriBidi presentation forms, without the LAM-ALEF ligature; per paragraph in implicit mode, per visual word in explicit mode | bidi on; BDSM implicit; `?1243` on; `?2500` and `?2501` off | GNU FriBidi (a build option, default on) | #142 gap after LAM-ALEF; #143 cursor direction for every shape; #144 right-align RTL glyphs; #145 overall RTL mode; #2785 a mode set before a cursor move differs from one set after, plus a request that DECSC/DECRC save modes; #2939 broken Arabic in GNOME Console on 0.82 |
| **mintty** 0aea388 | both, applied per line | BDSM, SCP (0 = autodetect with LTR fallback), SPD 0/3, `?2500` (U+2500-259F), `?2501`, `?7796`, `?77096`, `?2521`; no `?1243` | autodetect, on by default | line; only the autodetected direction carries across soft wraps | Uniscribe turned off for RTL chunks, drawn with ExtTextOutW | `Bidi=2` (on; `Bidi=1` turns it off on the alternate screen); `?2501` on | its own minibidi (from PuTTY/Arabeyes) | #592 vim and tmux broken, which led to `--nobidi`; #837 isolates and embeddings reversed; its own TODO calls isolate support "not 100%" |
| **WezTerm** cab2516 | implicit; explicit RTL right-justifies the line and lays its clusters out right to left, each cluster's cells kept LTR. With bidi off, a cluster breaks at every whitespace boundary, so words appear in reverse order [source: screen_line.rs:445-475, 745-749; cellcluster.rs:122-123; harfbuzz.rs:224-231; not run] | BDSM (as an on/off switch), SCP; no `?2500`, `?2501` or `?1243` | config `bidi_direction`: LTR, RTL, AutoLTR, AutoRTL | each attribute/whitespace cluster; clusters also break after a run of more than 2 spaces | HarfBuzz, script guessed per buffer | `bidi_enabled` false; not documented on wezterm.org | wezterm-bidi, its own Rust crate, which its README says passes all of BidiTest and BidiCharacterTest | cursor and selection ignore bidi; vim scrolling leaves new rows unreordered; RTL text that crosses a colour change comes out misordered [inferred] |
| **iTerm2** 91411f5 | implicit; explicit leaves text as written ("Explicit right-to-left layout is not implemented") | BDSM and SCP (added 2026-09-30, recorded per line, following WezTerm); DECRQM 8; no `?2500`, `?2501` or `?1243` | SCP recorded on the line; otherwise first strong character, but only when `detectParagraphDirection` is on (default NO); otherwise LTR | paragraph (soft wraps joined) | CoreText | bidi on, including the alternate screen (#12131: vim reversed; fixed in 2025 by reordering the alternate screen by default) | CoreText's UBA | #13102 TUI popups distorted; empty cells treated as spaces for the sake of Claude Code/Ink |
| **Konsole** c8bc143 | implicit only | none; OSC 50 `BidiRenderingEnabled=false` turns bidi off for the whole session | `BidiLineLTR` on (the default) forces level 0, so RTL lines are never right-aligned; off gives UBIDI_DEFAULT_LTR | screen line | ICU `u_shapeArabic` presentation forms, with LAM-ALEF ligatures undone by hand | on since 2011 (e718e93bf); `BidiLineLTR` and `BidiTableDirOverride` on | ICU | 516273 controls at line start ignored; 508523 vim -A/-H line numbers wander; 519738 crash in bidiMap; bracket mirroring probably absent [inferred] |
| **mlterm** 405185a | implicit only | OSC 5379 `bidi_mode` only; no BDSM in its table of ECMA modes | `normal` (autodetect, the default), `left`, `right` | line | HarfBuzz with `-otl`, script taken from a codepoint-range table | `use_ctl` on | FriBidi | #52 Arabic in Neovim missing until a newline arrives; #122 crash on LAM + tashkil + ALEF; selected a visual region in 3.8.6 (Koblinger, 2019; not re-checked at 405185a, though vt_screen.c's region functions still take visual coordinates) |
| **Terminal.app** 2.15 (macOS 27) | unconfirmed | none documented | Koblinger, 2019: implicit LTR, no autodetect | per line (2019) | imports CoreText line-layout symbols [measured] | no setting documented [doc] | CoreText [inferred] | the second-hand reports disagree (see below) |

**Per-terminal details.**

- **VTE** [source: vte@56bad78].
  - Resolution covers the whole paragraph through `fribidi_get_par_embedding_levels_ex()`, then each row through `fribidi_reorder_line()` (bidi.cc:338-361, 549-564, 617-636). The cap is at vtedefines.hh:130. There is no bidi above 65535 columns.
  - Defaults: modes.py:175-188 and 1218-1235. SCP is at vteseq.cc:8488-8537 and SPD at 8895-8939.
  - Modes are stored per row. They apply only in column 0 of a paragraph's first row (vte.cc:3754-3780).
  - Anything resolved RTL is mirrored per cell. Box drawing is mirrored through a hand-written table (bidi.cc:737-770).
  - An I-beam over an RTL character moves to the cell's right edge, and gains a direction tick when the paragraph mixes directions. Mouse positions map from visual to logical. Arrow swapping uses the paragraph's direction (vte.cc:1621-1627, 5543-5570, 6744-6755, 9908-9925).
  - The API calls `vte_terminal_set_enable_bidi()` and `vte_terminal_set_enable_shaping()` both default to true (vtegtk.cc:6093-6178). GNOME Terminal maps them to gsettings keys and offers no UI for them (gnome-terminal@b23813a gschema.xml:201-210).
  - Erased cells in the middle of a row are converted to spaces before UBA runs (bidi.cc:520-522).
  - Unresolved: a 2026-10-05 tester on VTE 0.84.1 (Ptyxis) reports that VTE gets bracket mirroring wrong and analyses wrapped rows separately [doc: Salah-Sal on ghostty#11079]. Both conflict with the source reading (the local tree reports 0.85.0). The tester's examples are hard-newline output, and "mirroring" may mean bracket pairing. Neither was reproduced.
- **mintty** [source: mintty@0aea388].
  - Defaults are at config.c:224 and docs/mintty.1:3246-3256. The sequences are documented in wiki/CtrlSeqs.md:186-245, labelled experimental, and implemented at termout.c:2700-2760 and 3753-3771.
  - Per-line resolution is at termline.c:1040-1150, whose own comment calls it "a mess".
  - RTL font fallback: mintty probes the configured font for א and ا. If either is missing, it draws RTL runs in `FontRTL` (default Courier New), "as mintty does not make use of Windows font substitution in this case" (wintext.c:858-863, 3136-3145).
  - Clicks are turned into selection points through the logical mapping (termmouse.c:455-480).
  - Implementers disagree about `?2501`. mintty's author argued in terminal-wg #5 that P2/P3 make autodetect the natural default; the spec kept it off [doc].
- **WezTerm** [source: wezterm@cab2516].
  - The options are at config/src/config.rs:801-806 and term/src/config.rs:208-216. The changelog calls the feature "Experimental (and incomplete!)" (docs/changelog.md:1717).
  - Clusters split at attribute changes and at whitespace (`whitespace_run > 2`), and `resolve_paragraph` runs once per cluster (wezterm-surface/src/cellcluster.rs:86-166).
  - RTL lines are laid out right to left by cluster (screen_line.rs:91-92, 445-475), but the cells inside each cluster stay in LTR order, so explicit RTL is partial [inferred].
  - The cursor's x is the logical column. The code says "TODO: remember logical/visual mapping for selection" (screen_line.rs:364-366, 464).
  - Users in #784 confirm the cursor and selection problems [doc: github.com/wezterm/wezterm/issues/784]. j4james wrote in the same thread: "You need to pick a side: you can't support actual RTL software and non-RTL software at the same time."
- **iTerm2** [source: iTerm2@91411f5].
  - Paragraph layout is in sources/Drawing/BidiDisplayInfo.swift:140-280. Preferences are at iTermPreferences.m:757 and iTermAdvancedSettingsModel.m:930-962. The commit message of 91411f5 describes the BDSM and SCP work.
  - The comment on `disableBidiInAlternateScreen` names "certain agent CLIs" that order their text themselves. It also says the default came from #12131, where a Persian user's vim was reversed in iTerm2 but correct in Terminal.app.
  - Empty cells are treated as spaces before UBA runs, because Claude Code and Ink address columns absolutely (sources/Drawing/BidiDisplayInfo.swift:237-256, `replacingNulWithSpace`, @91411f5).
- **Konsole** [source: konsole@c8bc143].
  - `bidiMap` runs over one screen line (TerminalDisplay.cpp:3477-3626; `ubidi_setPara` at 3615, paragraph level at 3611). Defaults are at Profile.cpp:142-144.
  - Each cell is drawn with its own `drawText`, and LRO is prepended when bidi is off (TerminalPainter.cpp:39, 340-480, 789-805).
  - Mouse positions map through `bidiMap` (TerminalDisplay.cpp:1767-1775).
  - Commit 76f879cd7 (2022-08-26) replaced Qt bidi over attribute runs with ICU per line, closing Koblinger's bug 403729 [source: konsole history]. The spec's description of Konsole restarting BiDi at every attribute change (prior-work/terminals.html) is therefore true of 18.04, and stale since 2022.
- **mlterm** [source: mlterm@405185a].
  - Resolution is in vtemu/libctl/vt_bidi.c:118-139 and 222-330. OSC 5379 is documented at doc/en/PROTOCOL:106.
  - Mirroring is a heuristic on runs of reversed visual indices, which a lone RTL neutral fails (vt_line_bidi.c).
  - The script table is at uitoolkit/libotl/hb.c:495-534. README.bidi notes that with HarfBuzz "all glyphs are left-justified".
- **Terminal.app** [doc + measured].
  - Apple's Terminal User Guide (macOS 27, Terminal 2.15) documents no BiDi setting. Positive control: the East Asian Ambiguous width option is documented.
  - The binary imports `CTLineCreateWithAttributedString` and the CTRun accessors, and contains the `baseWritingDirection` selectors. It has no "Writing Direction" menu entry; positive control: "Default Font Size" is present [measured: strings, nm -u, plutil].
  - Its behaviour is known only second-hand, and the two reports disagree. Koblinger (2019, from screenshots): implicit LTR, per line, no autodetect, no way to turn it off [source: prior-work/terminals.html]. iTerm2 #12131 (2025): "correct order and shaping" inside vim, which implies it reorders the alternate screen [doc]. Neither report is current or first-hand, and nobody captured it on this Mac.

**Sequence support at the commits read** [inferred from the facts above]:

| sequence | VTE | mintty | WezTerm | iTerm2 | Konsole | mlterm | WT / conhost | Terminal.app |
|---|---|---|---|---|---|---|---|---|
| BDSM `CSI 8 h/l` | yes | yes | on/off only | on/off only | no | no | no | none documented |
| SCP `CSI Ps SP k` | yes (0 = LTR) | yes (0 = autodetect) | yes | yes | no | no | no | none documented |
| SPD 0/3 as alias | yes | yes | no | no | no | no | no | none documented |
| `?2500` | U+2500-257F | U+2500-259F | no | no | no | no | no | none documented |
| `?2501` | default off | default on | no | no | no | no | no | none documented |
| `?1243` | default on | no | no | no | no | no | no | none documented |
| own switch | API call | `?7796`, `?77096`, `Bidi=` | config | setting | OSC 50 | OSC 5379 | none | none documented |

**What users report across trackers** [doc: the issue threads above]:

- Pure implicit reordering breaks RTL-aware applications: vim -H/-A, tmux, TUI popups. That is why every implementer added a way to turn it off, and why Windows chose logical order (section 4).
- The open complaints are:
  - cursor and selection that ignore bidi (WezTerm);
  - stale redraws (WezTerm, mlterm);
  - the width of the LAM-ALEF ligature (VTE #142, mintty `?2521`).

**The Hebrew-point bug** [inferred from the per-terminal sources]:

- Every terminal read picks the script per cell or per run; none fixes one script for the whole line.
  - VTE gives each cell its own Pango layout.
  - Konsole draws each cell separately.
  - Windows Terminal runs AnalyzeScript.
  - mlterm maps U+0590-05FF to HB_SCRIPT_HEBREW.
  - iTerm2 lays out through CoreText.
  - WezTerm and kitty let HarfBuzz guess the script per buffer.
- Where HarfBuzz guesses the script per buffer (WezTerm, kitty, Ghostty's HarfBuzz backend), a buffer that mixes Latin and Hebrew still shapes the Hebrew marks as Latin: WezTerm with bidi off and no space between them, or one Ghostty face covering both scripts; kitty unchecked. Per-script itemisation (mlterm's range table, CoreText, AnalyzeScript) and per-cell layout (VTE, Konsole) avoid it.
- None was tested on screen for Hebrew point placement.
- baud's fixed `latn`/`DFLT` choice is the outlier.

## 4. Who does not, and why

Most refusals are scoped to effort, maintenance or performance. kitty's and tmux's maintainers also hold that RTL is the application's job, not the terminal's, and Alacritty's reported "extremely little interest" before later citing unfamiliar complexity. Quotes come from the issue trackers [doc]; behaviour comes from source.

### Ghostty: no bidi

**How it shapes, by backend** [source: ghostty@befcdfd harfbuzz.zig:278-281, 297-299 and coretext.zig:176-193, 524-541; ghostty@d9840f3 coretext.zig:176-202]:

- The HarfBuzz backend forces LTR and lets `hb_buffer_guess_segment_properties()` fill in the script. A Hebrew or Arabic run is therefore shaped with its own script and laid out left to right.
- The CoreText backend forces embedding level 0.

**Which backend runs (corrected).** macOS uses the CoreText backend by default; Linux uses fontconfig/FreeType with HarfBuzz [source: src/font/backend.zig:60 and src/font/shape.zig:31 at both befcdfd and d9840f3]. CoreText does its own script itemisation [inferred]. On every backend, a run also ends wherever the font index changes [source: run.zig:255 at both commits: `if (font_info.idx != current_font) break;`].

So a mixed run such as "`file.txt שלום`" is shaped as Latin only on a FreeType/HarfBuzz build where one face covers both Latin and Hebrew. With a Latin-only primary face, the Hebrew lands in a fallback run of its own.

**What the maintainers say:**

- Mitchell Hashimoto (#1442, 2024-02-02) read the spec. He does not plan it "in the near term" because it "requires a very significant amount of effort", says "One day, I'd like to support this", and is open to renderer/shaping-only partial support.
- jcollie (#14559, 2026-10-06): nobody has "gained the confidence of the existing maintainers that they'll be around long-term".
- jcollie (#11079, 2026-08-07): "None of the core maintainers are fluent in any RTL language".

**Status.** PR #11079, which reorders each row by UAX #9 in the renderer (its own label is "shaping only": no cursor, selection or model changes), is open and unmerged. #1442 has 62 thumbs-up, and the vouch bot closed seven other RTL PRs. Ghostty's docs say grapheme clusters in Arabic and Hebrew render correctly, "although only left-to-right text is supported" [doc: ghostty.org/docs/features].

### kitty: no bidi, but HarfBuzz reverses each run

**Mechanism** [source: kitty@7d37453 kitty/fonts.c:835, 1226-1227, 2283; kitty@6d5d0c4 kitty/fonts.c:1220-1221 (the line numbers differ between the two commits)]:

- Each run goes through `guess_segment_properties`, so an RTL run is reversed.
- Runs break at every space. Each word is reversed while the words themselves stay in LTR order.
- `force_ltr` (default no) exists for use with FriBidi [source: kitty@7d37453 kitty/options/definition.py:66-87].

**What the maintainer says.** Kovid Goyal: "RTL needs to be supported by applications that run in the terminal, not terminal emulators" (#1105, 2018). He will not accept "significant negative performance impact on normal (i.e. no bidi) usage" (#2109, 2020). #2109 has 23 thumbs-up, and 6 duplicates have been closed into it.

### Alacritty: no shaping at all

Alacritty has no shaping engine [source: alacritty@d692748 glyph_cache.rs:48, 110, 122; zero references to harfbuzz, rustybuzz or swash, against 15 for freetype].

- Chris Duerr: "It's complexity that none of the maintainers are interested/familiar with" (#663, 2025-11-28). #663 has 52 thumbs-up.
- PR #7872 implemented the terminal-wg mode tracking behind a crate feature, at no measurable cost. It was closed unmerged in 2025.

### foot: shaping per grapheme only

foot shapes each grapheme cluster on its own [source: foot@cb27717 render.c:949-950; fcft@1ff7e79 README.md:62-74]. Hebrew points sit inside their cluster, Arabic does not join, and nothing is reordered.

- dnkl: "there's no plan to implement this" (#756, 2021).
- In #2049, an LRM after a character forced a fallback font for that cell. The fix that shipped reverted the preservation of zero-width grapheme breaks [source: CHANGELOG.md:625-628].

### xterm

See section 2.7.

### Contour

Contour accepts DECRLM, DECHEM and DECRLCM but acts on none of them. "Right-to-left (RTL) text is explicitly not handled" [source: contour@d8ce17b docs/vt-extensions/unicode-core.md:8].

Its HarfBuzz shaper sets LTR and leaves non-Latin scripts to `guess_segment_properties`. Runs from libunicode's run_segmenter carry a per-run script [source: contour@d8ce17b OpenShaper.cpp:351-362, 522-526; TextRenderer.cpp:1313-1339]. That run_segmenter splits at script boundaries is inferred; libunicode was not read.

### xterm.js

xterm.js has no bidi. Its DOM renderer merges same-style cells into one span, so the browser reorders text within a row by accident. Its WebGL renderer draws cell by cell. What a VS Code user sees therefore depends on the GPU setting [source: xterm.js@c58ea36 DomRendererRowFactory.ts:186-231].

- Tyriar: "There are cases where this will fall over though (differing styles, line breaks)" (#701, 2025).
- jerch on PR #5623 (2026): any implementation "must abide by this specification ... as a minimum, otherwise we introduce just another broken bidi implementation".
- #701 has 49 thumbs-up.

### Windows Terminal and conhost

**AtlasEngine shapes by script and never applies bidi** [source: windows-terminal@35f29bd src/renderer/atlas/AtlasEngine.cpp:1040-1140; DWriteTextAnalysis.cpp:86-89, 169-172]:

- It calls AnalyzeScript, then GetGlyphs with `isRightToLeft = 0`.
- `GetParagraphReadingDirection` returns LTR, and `SetBidiLevel` returns E_NOTIMPL.

The local extract at `.claude/research/open-calls/face/wt` has no `.git`, so its commit, 2b5336c, could not be verified. Its AtlasEngine.cpp (38e4648c…) and DWriteTextAnalysis.cpp (4f15e81a…) are byte-identical to the clone at 35f29bd, so the claims above hold for either [measured].

**conhost's GDI path deliberately presents RTL in logical order.** It sets `fOverrideDirection = TRUE` "to present RTL text in logical order in order to be compatible with applications like vim -H" [source: gdi/paint.cpp:452-490 @35f29bd]. That came out of #12294 and was merged as PR #12722 on 2022-03-21 [doc]. In #12294, j4james wrote that it is "almost impossible to write an RTL application on terminals that don't work this way".

**History.** The old DxEngine called AnalyzeBidi and reversed runs of RTL [source: CustomTextLayout.cpp @78de114]. AtlasEngine became the default in v1.19.10292.0 (2024-01-30, #16277), and DxEngine was removed in v1.21.1272.0 (2024-05-07, #16278) [doc: release notes]. Users reported reversed RTL as a regression (#19880 and #17904, both closed as duplicates of #538). lhecker answered in #17904 (2024) that logical order was intentional so that vim -A works, and "definitely not a bug".

**The maintainers disagree:**

- j4james (2020) wants the DEC modes and no implicit reordering by default.
- lhecker (2026-03): "It doesn't have to be perfect in the first PR; it just needs to be decent"; on #20159 (2026-05) he asked for the line-level segmentation refactor up front.
- DHowett (2024) says RTL is not officially supported.
- #538 has been open since 2019, with 48 thumbs-up.

**Three community PRs are open** [doc: gh api snapshots, critic; the Windows deep dive's #20410 snapshot]:

- #20159 (2026-04-29) calls AnalyzeBidi and reverses RTL glyphs per run. Mixed text "falls back to LTR direction per run". lhecker commented on it that the fix "necessitates a refactor"; it was refactored at his request and still has no line-level reordering.
- #20377 (2026-06-30) is rendering-only and runs AnalyzeBidi per line with L2 ordering. It leaves cursor, selection, hit testing, bracket mirroring and paragraph direction out of scope.
- #20410 puts bidi rendering behind a global setting, and adds a visual-to-logical mapping for mouse selection. zadjii asked for recordings.

### tmux and GNU screen

Neither reorders, and both drop the bidi control sequences (BDSM, SCP). tmux adds isolates at vertical pane borders when terminfo has `Bidi` (section 5). Whether LRM, RLM and the isolates pass through unchanged was not checked.

## 5. Applications, and the double-reversal problem

These rows were re-checked at current commits, not taken from the spec's 2018 notes.

| application | does its own bidi? | sends BDSM or SCP? | notes |
|---|---|---|---|
| Emacs 9bc5661 | yes: `bidi-display-reordering` defaults to t, so on a TTY it writes visual order | no (positive control: `\e[?2004h` at lisp/term/xterm.el:1081) | by default it shows bidi controls as spaces on text terminals, so it never sends them to the terminal [source: doc/emacs/mule.texi:1968-1987, 2023-2027; lisp/international/characters.el:1879-1890, 1979-1982, 2013] |
| vim 5979d7d | no bidi; `rightleft` and `vim -H` mirror the whole window, and are designed for terminals that do not reorder | no (positive control: `?1049h` found) | see the vim notes below the table |
| Neovim 898e6f8 | no; `termbidi` off; `-H` kept | no (positive control: 2004 found) | [source: runtime/doc/options.txt:6929-6939; src/nvim/main.c:1242] |
| the stock vim on this Mac | built `-arabic -rightleft` | — | [measured: `vim --version`, /usr/bin/vim 9.1] |
| less ddfc834 | no reordering; bidi controls count as composing and are "sent to the terminal" by default | no | [source: fmt.uni, compose.uni, charset.c:899-918] |
| readline f95e814, zsh 8cc5ead | no bidi; both redraw by cursor positioning | no | see the readline/zsh notes below the table |
| tmux 209d389 | no | drops BDSM (only modes 4 and 34 are handled, input.c:1994-2016) and SCP (no `SP k` entry) | since 3.2 it wraps pane content at vertical borders in PDI/LRI, when the client is UTF-8 and terminfo has `Bidi` (screen-redraw.c:102-103, 1380-1388, 1729-1730; tmux.1:8840-8843) [source] |
| GNU screen 9d8b0ff | no | drops SM 8; DEC `?34`, `?35` and `?36` are commented out | no isolate mitigation [source: src/ansi.c:1070, 1136-1138, 1679-1699] |
| Midnight Commander 23a260a | its viewer lists RTL as an "Absolutely non-goal" | no | on a reordering terminal, a Hebrew filename's row swaps the name and size columns when the next column is numeric (#4138, reproduced on VTE 0.58.3) [source: src/viewer/ascii.c:39-44; doc] |
| bicon 48720c0 | a pty wrapper that runs FriBidi over each printable run between control characters | no | a full-screen app that writes a line in pieces gets each piece reversed separately [source: bicon_read.c:83-93, 135-155; inferred] |
| pi-bidi (2026) | reorders an AI CLI's transcript itself | no | "Do not use this if your terminal already does bidi ... Reordering twice puts the text back the way it started" [doc: README] |

**vim** [source: vim@5979d7d]:

- `termbidi` hands bidi and shaping to the terminal, but `-H` sets `rightleft` anyway (src/main.c:2423-2431).
- With `rightleft` off, vim's own Arabic shaping gives the logically first letter its final form (src/testdir/test_arabic.vim:314-360).
- On a reordering terminal, side-by-side windows mix across the '|' separator. The docs offer `fillchars=vert:l` and call it "a workaround" (runtime/doc/arabic.txt:179-184).

**readline and zsh:**

- macOS libc `wcwidth` returns 1 for U+05B0, U+05BC, U+064B and U+0651, and -1 for LRM, RLM, LRI and PDI [measured: C probe under en_US.UTF-8].
- readline's fallback table for a broken wcwidth covers only the Latin combining blocks (rlmbutil.h:178-195). Editing pointed Hebrew therefore misplaces the cursor, whatever the terminal does [source + inferred].
- zsh swaps in its own width table when configure detects a broken wcwidth (configure.ac:2405-2411). Which table the owner's nix zsh uses was not checked.

**Double reversal: either default breaks something** [inferred from the rows above and the terminal-wg spec]:

- *If baud reorders by default (implicit):*
  - `cat`, `ls`, `git` and the prompt read correctly.
  - Everything that already orders RTL text itself breaks: Emacs, `vim -H` and `rightleft`, vim's presentation forms, bicon, fribidi, pi-bidi, and probably Claude Code. A Windows console fork's README says Claude Code reorders RTL text itself and lists claude.exe as a visual-order app; Claude Code's 2.1.74 changelog fixed RTL rendering in Windows Terminal, conhost and the VS Code terminal [doc]. Its behaviour on macOS terminals is unchecked.
  - None of vim, neovim, Emacs or bicon sends `RM 8` (checked at source); pi-bidi and the fribidi CLI were not read. tmux and screen would drop `RM 8` anyway.
  - Columnar TUIs break too.
- *If baud does not reorder:* the RTL-aware tools work, and plain output reads backwards.

**What breaks under per-row reordering.** A native Arabic speaker tested Ghostty PR #11079, which reorders each row implicitly, on 2026-10-05 [doc: Salah-Sal on ghostty#11079]. The failures group by cause:

- *Per-line direction autodetection* (another contributor's `auto_ltr` variant, not the PR's LTR default): `/etc/app` shows as `etc/app/`, URLs break apart, tree branches and bat line numbers jump right, and CSV rows that start with Arabic flip.
- *A bug in this PR:* colours stay at logical columns while glyphs move (the tester traces it to `addGlyph`; stock Ghostty and VTE colour correctly), so adjacent Arabic filenames that UBA merges into one run look like a single file.
- *Suspected by the tester:* tab-separated columns swap because tabs are stored as blank cells.
- *Listed by the tester as shared with VTE, not blockers:* rows of only Arabic and digits reverse as a whole, and wrapped rows are analysed separately.

The tester asked for a configurable base direction, defaulting to LTR.

**The positions on each side:**

- Koblinger, on xterm.js #701 (2019): once the terminal shuffles characters, it becomes "mathematicaly provably impossible to have proper BiDi-aware text editing-viewing experience (e.g. vim, emacs...) on top of that platform" [doc].
- j4james makes the same point from the other side: RTL TUIs are "fairly straightforward on terminals that leave RTL characters exactly where you put them" (WT #12294) [doc].
- terminal-wg's answer is to have two modes: "There isn't a single mode of operation that can cover all use cases" [source: bidi-intro/why-terminals-are-special.html].

**The owner's own workload is untested on screen.** iTerm2 makes two special cases for agent CLIs: empty cells become spaces, and an opt-out disables bidi on the alternate screen for CLIs that order their own text [source: iTerm2@91411f5]. A Windows console fork's README says Claude Code reorders RTL text itself and lists claude.exe as a visual-order app, and Claude Code's 2.1.74 changelog fixed RTL rendering in Windows Terminal, conhost and the VS Code terminal [doc]; its behaviour on macOS terminals is unchecked. iTerm2 #13102 is a RatatuiRuby TUI whose popups bidi distorts [doc]. No researcher ran Claude Code or any other agent CLI with RTL text under a reordering terminal. A default that reorders the alternate screen should be tested against these first (Q17).

**tmux isolates and baud (corrected).** tmux emits its isolates only when terminfo carries `Bidi`. baud ships no terminfo entry of its own: it advertises `TERM=xterm-256color` from a pinned entry [source: baud-app/src/bell.rs:67-74; baud-app/src/pty.rs:189 @e138e34]. Getting tmux's isolates would first need a baud entry to be shipped and installed, or users to set `terminal-overrides`. Even then the isolates do nothing at terminal-wg level 1, which discards them; they help only at level 2 [inferred].

## 6. "Windows level": what full text engines do, and what a grid can do

### 6.1 What every engine does

- **The paragraph is the unit, and reordering happens per line.**
  - Uniscribe "must be used for an entire paragraph", and ScriptLayout reorders runs one line at a time [source: MicrosoftDocs win32@e103fa4 Intl/displaying-text-with-uniscribe.md:33, 90-94; sdk-api@c12073e usp10/nf-usp10-scriptlayout.md:84].
  - DirectWrite: AnalyzeBidi's range "should not arbitrarily split the middle of paragraphs" [source: sdk-api@c12073e dwrite/nf-dwrite-idwritetextanalyzer-analyzebidi.md:90-93].
  - ICU calls `ubidi_setLine` after `ubidi_setPara` [source: icu@4e68996 ubidi.h:1240-1256]. FriBidi separates paragraph levels from `fribidi_reorder_line()` [source: fribidi@24f15ee lib/fribidi-bidi.h:64-120].
  - For baud, this means resolving levels over the soft-wrapped logical line and then reordering each row's cells. Running UBA per row, which a grid makes tempting, would give levels that differ from every engine above [inferred].
- **Paragraph direction.**
  - DirectWrite has no automatic detection; the application supplies the direction [source: dwrite/ne-dwrite-dwrite_reading_direction.md; a grep over dwrite, dwrite_1, dwrite_2 and dwrite_3 for natural, detect and first strong found nothing, while the control 'reading direction' hit 13 files].
  - XAML's `TextReadingOrder.DetectFromContent`, the default in Windows 10 text controls, uses the first strong character, and "will not affect the alignment of the text" [source: winrt-api@8448d5e textreadingorder.md].
  - CoreText and AppKit's "natural" direction is P2/P3 [source: MacOSX27.0.sdk CTParagraphStyle.h:131-149; NSText.h:22].
  - Pango's `auto_dir` gives a paragraph with no strong character its neighbours' direction, which HL1 allows [source: pango@8e74c27 pango-layout.c:905-929].
  - Microsoft's guidance: the first strong character decides both reading order and alignment [source: MicrosoftDocs/globalization@a0735c7 text-directionality.md:76-83].
  - Apple's HIG: align a paragraph "based on its language", and never reverse the digits of a number [doc: HIG right-to-left.json].
  - Measured with CoreText's natural direction: `$ 12 (` lays out LTR, while `shalom (alef bet) 12` lays out RTL with `12` at x=0 [measured: engines/probe/probe, Courier New 16 pt, macOS 27.0 26A5425a].
- **Mirroring is per glyph.**
  - In `shalom (alef bet) 12`, the logical `(` is drawn with the glyph `parenright` and the logical `)` with `parenleft`. Each is 9.6 px wide, one cell [measured: probe].
  - HarfBuzz substitutes the Bidi_Mirroring_Glyph in RTL buffers, or sets the `rtlm` feature [source: harfbuzz@4dd9449 hb-ot-shape.cc:655-672].
- **Numbers and neutrals follow UBA.** In `abc ABG 123 def`, where ABG stands for three Hebrew letters, the display order is `abc 123 GBA def`. The number lands to the left of the Hebrew word because a European number after R resolves inside the RTL run. This is correct UBA output, and a terminal that runs UBA will show it too [measured: probe 'mixed LTR'].
- **A caret has two edges.**
  - Uniscribe uses leading and trailing edges [source: win32 Intl/displaying-the-caret-in-bidirectional-strings.md:11-24].
  - CoreText gives a primary and a secondary offset [source: CTLine.h:469-522]. Pango has strong and weak cursors [source: pango-layout.c:2563-2578].
  - On `abc ABG 123 def`, CoreText gives split carets at three indices. Every offset falls on a cell edge (9.6 px pitch), so a grid can draw both carets [measured: probe].
- **Arrow keys are visual in AppKit, selectable in Word, and logical in Microsoft's guidance.**
  - In NSTextView, with both TextKit 1 and TextKit 2, `moveRight:` from index 0 of `abc ABG def` visits 0 1 2 3 4 6 5 7 8 9 10 11, while `moveForward:` is logical [measured: probe].
  - Word offers `wdCursorMovementLogical` and `wdCursorMovementVisual`. Its default is unconfirmed [source: VBA-Docs@4a944c5 Word.WdCursorMovement.md].
  - Microsoft's globalization guidance describes logical movement [source: text-directionality.md:121-127].
  - The Ctrl+Left/Right Shift direction keys are documented for the Win32 EDIT control and for RichEdit. Windows 11 Notepad is a RichEdit plain-text control (Sargent, 2022), and RichEdit plain text always uses the UBA [doc]. Notepad's current menu items and default caret movement were not confirmed, because no Windows machine was available.
- **Selection is a logical range.**
  - AppKit's Shift+Right keeps one contiguous logical range, which can look discontiguous on screen [measured: probe, ranges 2+1, 2+2, 2+2, 2+2, 2+5, 2+6; the reading is inferred].
  - TextKit 2 adds visual and multiple-rectangle selection [source: NSTextSelectionNavigation.h:80-90]. Word has Block and Continuous visual selection [source: Word.WdVisualSelection.md].
  - Word's `AddBiDirectionalMarksWhenSavingTextFile` adds bidi controls on export, a precedent for adding marks when copying [source].
- **Digits.**
  - Windows substitutes native digits by context, never, or always (LOCALE_IDIGITSUBSTITUTION). Word's options are Arabic, Hindi, Context and System [source: win32 Intl/digit-shapes.md:25-40; Word.WdArabicNumeral.md].
  - Apple's HIG says Hebrew uses Western digits [doc].
- **Hebrew shaping needs the right script tag.**
  - HarfBuzz tries the text's own script tags (`hebr`), then `DFLT`, `dflt` and `latn` [source: hb-ot-layout.cc:564-611].
  - The Hebrew shaper's `gpos_tag` is `hebr`. When the GPOS script chosen is anything else, GPOS is disabled and marks are placed by combining class (`fallback_mark_positioning`) [source: hb-ot-shaper-hebrew.cc:204; hb-ot-shaper.hh:158-162; hb-ot-shape.cc:135-136, 161, 198-207].
  - HarfBuzz therefore never uses a face's `latn` or `DFLT` mark anchors for Hebrew, and baud uses only those.
- **Arabic joining needs context across runs.**
  - Each character's joining type drives a state machine through the features isol, fina, fin2, fin3, medi, med2 and init, then rlig, then calt. The docs say to pass the whole paragraph as context [source: hb-ot-shaper-arabic.cc:101-111, 185-235, 299-316; hb-buffer.cc:1997-2005].
  - A terminal that shapes one same-attribute run at a time must therefore pass the neighbouring cells as context across SGR boundaries [inferred].
- **Arabic in a grid, measured in Courier New.**
  - Three behs give three distinct glyphs (initial, medial, final), each 9.6 px wide, so joining fits a grid [measured: probe3].
  - Lam+alef becomes one glyph, 9.6 px wide: one cell, where wcwidth allocates two (libc gives U+0644 = 1 and U+0627 = 1) [measured].
  - baud gives lam+alef two columns, computed from East Asian Width and general category with no font involved [source: baud-domain/src/quantity.rs:530; baud-core/tests/encoding/width.rs:1-11, 30-41; inferred].
  - Only Courier New was measured.
- **How engines handle the cell a ligature frees.**
  - ICU's `u_shapeArabic`, meant for "legacy display technology (simple terminals)", can put the spare space near the ligature, at the end, at the beginning, or automatically (LAMALEF NEAR, END, BEGIN, AUTO) [source: ushape.h:30-35, 106-194, 336-424].
  - FriBidi writes U+FEFF before the ligature to keep the length the same. Its `FRIBIDI_FLAG_SHAPE_ARAB_CONSOLE` is meant for "grid terminals with no mark rendering capabilities": it also forms console ligatures and folds marks into presentation forms [source: fribidi-arabic.c:128-153; fribidi-arabic.h:38-58].
  - Uniscribe divides the lam-alef glyph into four caret areas, the leading and trailing halves of lam and of alef [source: managing-caret-placement-and-hit-testing.md:13-18]. In a grid where the ligature keeps two cells, these map onto half-cell hit-testing [inferred].

### 6.2 What a cell grid can do, and what it cannot

| possible in a grid, and the terminal's job | not possible, or not the terminal's job |
|---|---|
| levels resolved per paragraph, then whole cells (a base plus its marks, a wide cell as one unit) reordered per row; this satisfies L3 for free | visual caret movement for applications: arrow keys go to the application, which moves its own cursor logically. terminal-wg offers arrow swapping instead [source: arrow-key-swapping.html] |
| mirroring each cell at draw time | proportional cluster widths, justification and kashida |
| numbers and neutrals placed per UBA; RTL rows set flush right | ligatures that change the column count (lam-alef) without a padding policy |
| Hebrew points shaped under `hebr`, inside the base cell | bidi for cursor-addressed TUI output, where the terminal cannot know the paragraph boundaries |
| Arabic joining at one cell per letter, given a monospace Arabic face and context across cells | |
| split carets drawn on cell edges; mouse hit-testing through the inverse map plus a half-cell flip | |
| copy from the logical model, with visual or block selection as an option | |
| system-font fallback per cluster, preferring monospace faces | |

All rows: [inferred from 6.1].

### 6.3 Three readings of "Windows level"

1. **Windows terminals as they are.** There is no implicit reordering. conhost forces logical order on purpose (#12722), and AtlasEngine has no bidi. The open PRs (#20159, #20377, #20410) work per run or per line and fall short of terminal-wg (section 4). #20159 and #20377 are rendering-only; #20410 also touches selection, mapping mouse selection from visual to logical. Taken literally, this reading means not reordering.
2. **The quality of Windows GUI text engines.** The Uniscribe/DirectWrite paragraph model, with split carets, mirroring and shaping, applied to a grid. This reading points to terminal-wg implicit mode plus the grid items in 6.2.
3. **The UTC proposal L2/23-107** (Renzhi Li, Dustin Howett of Windows Terminal, Peter Constable, who owns Microsoft's OpenType spec pages per their ms.author [inferred]).
   - It asks for paragraph boundaries and embedding levels kept in the buffer, override controls, line-by-line reordering, and *visual* cursor movement. That last item conflicts with terminal-wg.
   - It is a proposal, not a working-group draft. The working group's later reports moved: L2/25-179 calls partial UAX #9 support a prerequisite, and L2/25-263 (2025-10) records "a desire in the group to not be dependent on terminals supporting UAX 9". The group's drafts are in a private repository and were not seen. The opposition paper L2/23-153 is unread.

Note AA already recorded that Windows Terminal never reorders RTL text, so "Windows level" cannot mean matching Windows Terminal's reordering [source: aa-shell-locale.md:260-262]. Which reading the owner means is Q1.

## 7. Where it lives in baud

All file:line citations are at e138e34 on `smooth-scroll` [source] unless tagged otherwise.

### 7.1 Integration map

| area | where | what RTL needs there |
|---|---|---|
| core text model | baud-core/src/paint.rs:651-731 (glyph), 1021-1035 (wrap) | nothing. The grid already stores text in logical order, which is terminal-wg's model. |
| per-cell bits | baud-domain/src/cell.rs:57-62, 165-228 | No bits are free. `CellFlags` holds PROTECTED, GRAPHICS, MARKED, a 7-bit LinkId and a 6-bit ClusterId, and `PackedCell` is the measured 8 bytes. No per-cell level or mirror bit fits, so the renderer has to compute them or a side structure has to hold them. |
| wide cells | cell.rs:16-18; paint.rs:705-708; baud-render/src/cellmap.rs:93-98; shaders.metal:1221 | The lead cell and its `WIDE_TAIL` must stay adjacent and draw as one glyph. In an RTL mirror the tail lands left of its lead, so the glyph origin moves to the tail's visual column. |
| marks | paint.rs:669-672, 762-808; write.rs:1333-1336; width_data.rs:21-23 | Characters of class Mn, Me and Cf attach to the previous cell's cluster. Reordering whole cells therefore keeps each mark with its base, which is terminal-wg's combining rule and the effect FriBidi's REORDER_NSM flag produces. |
| bidi controls | as above; UCD 17.0.0 unicode-derivedgeneralcategory.txt:3543, 3549, 3552 and unicode-graphemebreakproperty.txt:58, 61, 64, 67 | LRM, RLM, ALM, the embeddings and the isolates are Cf with GCB=Control. baud therefore *stores* them as marks today rather than discarding them. `attach` drops a control at column 0 (paint.rs:766-769), after an empty cell (783-786, as when a TUI addresses columns over erased cells [inferred]), when the cluster already holds 8 continuations (797-801), and when the store refuses (805-808). Level 2 needs each path handled. |
| cluster capacity | cell.rs:498-502, 575-646; docs/vt-coverage.md:362-367; paint.rs:803-808 | 63 slots; new clusters are refused, and slots are freed only by RIS. See 7.2. |
| Arabic Prepend | paint.rs:669-672 with write.rs:1333-1336; unicode-graphemebreakproperty.txt:21-25 | U+0600-0605, U+06DD, U+0890-0891 and U+08E2 are Cf with GCB=Prepend, so they attach to the previous cell. *Measured 2026-10-10* by `every_line_of_the_suite_clusters_through_the_terminal` (baud-core/tests/encoding/graphemes.rs), which feeds every line of GraphemeBreakTest.txt through `Term`: GB9b joins the Prepend to what follows, and baud does the reverse. Printable ASCII after a Prepend is never put to the cluster machine, so it takes a cell of its own: `÷ 0061 ÷ 0600 × 0062 ÷` (line 764) renders `a` with U+0600 attached and `b` beside it, and `÷ 06DD × 0020 ÷` (line 365) after a letter the same. So a digit after ARABIC NUMBER SIGN is not swallowed; it is split from the sign it belongs to, which joins the cell before. A character after a Prepend that is not printable ASCII continues it and joins that same cell, the letter's: the `06DD × X` lines among 337-368, after a letter, inside the test's pin of clusters that start zero-width [measured]. Both are open departures. |
| paragraphs | paint.rs:1033, 1095; baud-domain/src/grid.rs:303-308, 533-544; history.rs (Wrap) | Already exist. `RowExtent`'s WRAP bit is exactly terminal-wg's soft/hard row end. |
| per-paragraph properties | baud-domain/src/store.rs:161-191 | Cannot ride in `RowExtent`. Its 15 low bits all belong to the mark, and `reached()` takes one unmasked max over the whole word, so a flag bit would win the max and freeze the mark. The masked alternative measured 3% on cat-cjk (store.rs:176-183). |
| new storage | baud-core/src/storage.rs:100, 322-401, 754-770; grid.rs:550-556, 719-761, 1843-1930; grid/reflow.rs:53-60, 141-198; term.rs:~1124-1161 | A per-physical-row slice parallel to `used`, on both surfaces. Every operation that moves `used` must move it too: copy_row, insert_lines_in and delete_lines_in, erase_ring, fill_row and fill_at, reflow, swap_surface, and the viewport un-rotation. |
| applying modes | paint.rs:1021-1035 (wrap), 1093-1100 (line_feed), 1102-1105 (index, 7.6% of cat-cjk) | terminal-wg's draft rules for LF, wrap joins, ED, SU/SD and DCH/EL map onto these sites [doc]. The spec gives SU, SD, DCH and EL as examples ("e.g.", "and possibly other similar sequences"). IL and DL are not named, so applying the SU/SD rule to them reads that invitation [inferred]. Issue #10 (DECSTBM) is open. |
| pending wrap | baud-core/src/term.rs:2084-2110 | `Term::cursor()` clamps away the one-past-the-margin state, so the renderer cannot hide the cursor there, as terminal-wg prefers. The core would have to expose it. |
| modes | baud-domain/src/vt/mode.rs:99-113, 124-235, 369-400; baud-core/src/ops.rs:1309-1315, 1562-1661 | BDSM needs an `AnsiMode` variant for 8, some state, and a `mode_state` arm. `ModeBit` uses 20 of its 32 bits. |
| sequences today | csi.rs:742-773, 869-952; mode.rs:108-113 | Every bidi sequence is counted rather than silently dropped. BDSM falls to `AnsiMode::Other`, and DECRQM 8 answers 0. SCP and SPD hit the intermediate branch's Unsupported arm; only `SP q` is handled there. SRS, SDS and SIMD hit the default Unsupported arm. `?2500`, `?2501` and `?1243` become `PrivateMode::Other`. |
| XTSAVE/XTRESTORE | csi.rs:835-860 | Missing; the private-marker finals handled are h, l, u, n(996), J and K. terminal-wg's `?1243` design relies on them. *Built 2026-10-09:* every mode DECSET acts on is saved, so `?1243` needs a slot in `PrivateMode::slot` when it is built, which `every_mode_decset_changes_is_saved_and_restored_and_no_other_is` enforces. |
| coverage gates | docs/vt-coverage.md; baud-core/tests/coverage.rs:71, 163, 234, 292; conformance/enumerated.rs:146, 183; spec_inventory.rs; api_coverage.rs:70; docs.rs:260; audit_all.sh:114-117, 199, 207, 406-412 | There is no bidi row [measured: grep; the control DECSTBM hits 2 times]. Partial, parsed and todo rows are forbidden, and every `out` row needs a reason. ctlseqs lists none of these sequences, so spec_inventory cannot force them [measured: ctlseqs' SM list is 2, 4, 12, 20]. baud-domain must have 0 `bool` fields. |
| keys | term.rs:2471-2510; baud-domain/src/vt/key.rs:519-520; docs/input-coverage.md:59 | Arrow swapping means Term::key swaps Left and Right when the cursor's paragraph resolves RTL. With autodetect on, that needs P2/P3 over the paragraph; where that runs is Q3. `scripts/scan-audit.py` counts only the ingest loops in write.rs, and Term::key runs once per keystroke. |
| mouse | term.rs:2253-2275; baud-app/src/app.rs:4221, 4283, 5151, 5236, 5760-5810 | `cell_under` is the single place that turns pixels into a cell, behind mouse reports, the wheel, OSC 8 hit tests and selection. The visual-to-logical inverse goes there. It is macOS-only for now, because the Wayland driver has no pointer. |
| render path choice | app.rs:120-170, 776-799, 850-859; baud-present-macos/tests/cells.rs:1076 | The cell path is the default, then the instance path, then the CPU path. The pixel oracle holds the paths equal, so any RTL scene must pass on all three. |
| instance and CPU paths | baud-render/src/renderer.rs:1464, 1491-1532, 1586, 2044, 2101-2105, 2218-2246, 2446, 2502; cache.rs:14 | These read only visible rows and mark rows dirty one at a time. In implicit mode a row depends on its whole paragraph, which may begin in scrollback. `convert_row` writes backgrounds, shapes and glyphs at the logical x = col × cell_w. A per-row logical-to-visual map goes here, while fg lookups and shaping stay logical. |
| shaping unit | baud-font/src/shape.rs:1-25; buffer.rs:1-15 | The cluster index is the cell index, and each base glyph sits at its cell's origin, so cells can be permuted after shaping in logical order. Whether any GSUB or GPOS lookup depends on buffer direction (the RightToLeft flag, reverse chaining) is not established. |
| cell shader | baud-present-macos/src/shaders.metal:1074-1077, 1115-1126, 1159-1303 | The instance id becomes (col, row), and the cell is read at that col. The background, glyph origin, wide lookahead, cursor test, selection test, the bar at the left edge and the smooth-scroll slide row all assume logical equals visual. RTL needs a per-row visual-to-logical index buffer (like the overlay byte at buffer(4)), or a mirror flag for rows that are a plain mirror. |
| cell path damage | cellmap.rs:139-238, 394-440; overlay.rs:191-215 | No per-row damage: GlyphMap and the overlay walk every visible cell every frame. Bits 26-31 of MapEntry's pad word are free for a "needs bidi" flag. |
| glyph key | cellmap.rs:113-133; shaders.metal:1222-1223; overlay.rs:1-15, 54-62, 634-667 | The key is codepoint, style and span. Mirrored glyphs (L4) and joining forms depend on context, not on the codepoint alone, so they need a key bit or the overlay's record route. Overlay records are placed at the logical x. |
| cursor geometry | renderer.rs:939-966; app.rs:1137-1141, 4510-4519 | `cursor_px`, the cursor trail's `cursor_rect` and the IME rectangle all use the logical col, and put the bar at the cell's left edge. |
| IME | app.rs:4192 | `marked_text` is a no-op, so preedit text is never drawn. This matters for Persian and Arabic input methods that compose. |
| selection | baud-render/src/select.rs:1-27, 381-465; shaders.metal:1115-1126 | Logical throughout, so copy stays correct. A stream selection's highlight becomes visually discontiguous in mixed-direction rows. |
| images, pictures | renderer.rs:1293-1384, 1466-1468 | Anchored to logical cells. terminal-wg: images are not flipped by default (a protocol may add a flip flag); alignment "should be revised". Box-drawing characters are class ON and reorder in RTL paragraphs. |
| script selection | baud-font/src/layout.rs:713-723, 727-777; gsub.rs:151-237; gpos.rs:128; renderer.rs:207, 641-664 | `lang_sys` takes no script argument. Per-script shaping needs one, plus per-(face, script) pairs, digests and extent, itemisation from Scripts.txt, and a mark sort by HarfBuzz's modified (SBL) combining classes (Q6). *Built 2026-10-13* but for the mark sort, and paired punctuation since 2026-10-15: see Q6's postscript. |
| Arabic joining | layout.rs:612-643; gsub.rs:4-10; gpos.rs:1-10; buffer.rs:11-15; cache.rs:14 | Features are fixed at {ccmp, calt, liga} plus mark and mkmk. Only GSUB types 1, 4, 6 and 7 and GPOS types 4, 6 and 9 are implemented. The glyph buffer may only shrink, and runs are cut at 32 cells and at face changes. Joining needs per-glyph feature masks, rlig, possibly GSUB types 2 (which breaks the shrink-only rule) and 8, GPOS cursive, and context across run cuts. |
| fonts | baud-app/src/config.rs:840-846, 1101-1135; baud-render/src/stack.rs:122-131, 248-254, 357-475 | The fallback chain is fixed at startup. A configured `fonts` list gets only the embedded symbols face and STIX appended. A face is chosen by the base codepoint. Until Q23 marks drew in the base's face. Since Q23 (2026-10-07), a cell with marks takes the first face that draws its whole cluster: the base's face first, then the unstyled fallback chain in order. If no face does, the base's face keeps the cluster, and the attached codepoints it lacks are dropped and counted in `Renderer::marks_unmapped`. |
| Wayland | baud-present-wayland (src/shm.rs, window.rs) | Not mapped against the visual-column changes. "Same pixels on every OS" requires the RTL scene to pass there too. |

### 7.2 The cluster store blocks pointed text, with or without bidi

**The limit.** ClusterStore holds 63 slots and probes twice. When it is full it refuses rather than evicts, and only RIS frees it [source: baud-domain/src/cell.rs:498-502, 575-646; docs/vt-coverage.md:362-367]. When `intern` refuses, `attach` keeps the cell's earlier cluster: the new mark is dropped, and only counted [source: baud-core/src/paint.rs:803-808].

**How fast real text hits it.** Measured in baud: over a hand-typed pointed Genesis 1:1-5 (no cantillation; 56 distinct clusters, 67 distinct prefixes), `marks_dropped` is 0 for verse 1, 10 by verse 2 and 45 by verse 5, the same with every font [measured: hebrew-patch results.txt [13]; note AC]. Two things cause refusals: `intern` probes two slots and refuses on collision (cell.rs:630-646), and `attach` interns every growing prefix as a permanent slot (paint.rs:787-808) [source]. In a model of `intern`, not a baud run, the first refusal in Pango's pointed Genesis 1 comes at character 33 with 14 slots used. That text holds 175 distinct base+mark clusters, 102 with the cantillation stripped [measured by the critic: pango@8e74c27 utils/test-hebrew.txt, sha256 39e80e5e…]; that is how many the text needs, not when loss starts.

**What follows.**

- Vowel points stop appearing within a few verses of pointed Hebrew, whatever per-script shaping does (measured above). Vocalised Arabic should behave the same [inferred; not run in baud].
- Each attached LRM or RLM also mints a new (base, marks) key.
- Per-script LangSys alone does not fix even a single cluster: with `hebr`, שָׁ keeps its shin dot one cell right until marks are sorted by HarfBuzz's modified (SBL) combining classes (hb-unicode.hh:320-349 @13.2.1) [measured in hebrew-patch [14], [16]]. Marks the font does not anchor (Cascadia's qamats on bet) stay misplaced either way.

This needs a red test pinning `marks_dropped > 0` on that sample, and it gates phase 0 (Q7).

*Built 2026-10-14* (docs/record.md, "The cluster store holds 1024 clusters and sweeps the dead ones"): the owner chose 1024 slots, eight probes, the cluster index in the cell's `Content` and a finalized hash, and a sweep of the live cells once all 1024 slots are used. `marks_dropped` over the five verses is 0, 0 and 0 where it was 0, 10 and 45 (`baud-core/tests/cluster_bounds.rs`), and 0 over Pango's pointed Genesis 1, where it was 329.

### 7.3 Constraints

- **No engine dependencies.**
  - HarfBuzz, FreeType, rustybuzz and swash are reference literature only [source: CLAUDE.md "Next"].
  - UAX #9 and its tables would be written in-house, generated by a script beside `scripts/gen-width-tables.py` and `scripts/gen-grapheme-tables.py` as two-stage tries and tested against the pinned UCD [source: baud-domain/src/width_data.rs:1-16; grapheme_data.rs:1-6].
  - The table architecture is a compiled database with an index-only runtime [memory: baud table architecture].
- **Where the algorithm can live.**
  - baud-core is `no_std`, has no alloc, is `forbid(unsafe_code)` and has zero crates.io dependencies. baud-domain is `no_std` and `forbid(unsafe)`, with zerocopy as its only dependency [source: baud-core/src/lib.rs:32-46; baud-domain/src/lib.rs:18-28].
  - UBA's stacks are fixed in size: max_depth 125 gives a 127-entry directional status stack, and BD16 gives a 63-entry bracket stack. The full algorithm can therefore live in baud-domain with caller-provided scratch space, gated by BidiTest and BidiCharacterTest, and be called from baud-render [inferred; tr9-51].
  - That matches terminal-wg's split: the model holds the bits and the view transforms.
- **The data needed.**
  - The pinned UnicodeData.txt 17.0.0 has Bidi_Class and Bidi_Mirrored for assigned code points. It lacks the defaults for unassigned code points, which DerivedBidiClass.txt gives in 25 `@missing` lines [measured].
  - A class fits in 5 bits, since there are 23 values [measured].
  - Also needed: BidiBrackets (128 entries in 17.0.0), BidiMirroring (428 data lines in 17.0.0) and both test files. Shaping also needs ArabicShaping, DerivedJoiningType, Scripts, ScriptExtensions and PropertyValueAliases (section 9).
- **The protocol rules.**
  - CLAUDE.md: "Never record a mode you do not act on" [source].
  - vt-coverage forbids partial rows. No BiDi mode row can be `done` until the renderer acts on it; until then the modes stay counted-unsupported, or go in as `out` with a reason [source: baud-core/tests/coverage.rs:234, 292].
  - The owner's rule that protocols are supported "fully, always" [memory: protocols fully, always] rules out shipping a partial version, such as iTerm2's missing explicit-RTL layout or WezTerm's per-cluster explicit RTL.
- **The performance rule.**
  - Any baud-core change that writes more bytes per input byte, or touches more per byte, needs a measurement, not an argument [source: CLAUDE.md "The rule that overrides style"].
  - Storing per-row properties at every LF and scroll touches `Paint::index`, which is 7.6% of cat-cjk. That needs an interleaved `ingest2` A/B with the first round discarded. Dropping Cf controls in the write path, which level 1 requires, needs the same.
  - On the renderer side, the frame bench's CPU column is needed on both arms.
- **The font oracle (from the critic).**
  - baud's shaping is gated "exact against hb-shape on 480 fixtures" [source: CLAUDE.md].
  - Take a face whose Hebrew marks are reachable only under `latn`/`DFLT` and which has no `hebr` script. HarfBuzz disables GPOS for it and positions the marks by combining class, ignoring the anchors; baud today uses those anchors. HarfBuzz also prefers `DFLT` over `latn` for non-Latin text, which is the reverse of baud's order [source: harfbuzz@4dd9449 as cited in 6.1; layout.rs:713-723].
  - Per-script LangSys that keeps today's `latn`/`DFLT` fallback will therefore disagree with the oracle for such faces. Noto Sans Hebrew 2.003 and 3.001 both carry a `hebr` GPOS script [measured: hebrew_candidates_pitch.out, notoadv.out], so they shape the same under Q6 (a) or (b). The merge prototype gives baud's built font a `hebr` script (merge.py:415; results.txt:538), and the shipped build should keep that record. It does since 2026-10-08: the built Regular and Bold carry a `hebr` record naming DFLT's Script table (note AC, section 5a).
- **Same pixels on every OS** [memory]. The owner's answer to Q22 reconciles the two: same pixels holds for everything baud ships, and system fallback differs per OS only where the alternative is a box (TODO.md:2557-2562). That fallback picks Courier New on macOS, and whatever fontconfig picks on Linux.

## 8. Design questions for Jev and the owner

Each question needs a rendered picture of its options on the review page before it is asked, and a Jev score [memory: every question has a visual; Jev in every decision]. Several of them are coupled, and the couplings are stated.

**Q1. What does "Windows level" mean?**

- (a) Match Windows terminals: no reordering. *For:* conhost forces logical order on purpose and AtlasEngine has no bidi (section 4), and RTL-aware applications keep working.
- (b) Windows GUI text-engine quality on a grid: terminal-wg implicit mode plus the items in 6.2. *For:* it fits "the best one", and VTE is a working precedent.
- (c) The direction of the UTC proposal L2/23-107, including visual cursor movement. *For:* it was co-authored by Windows Terminal's Dustin Howett, and Peter Constable owns Microsoft's OpenType spec pages per their ms.author [inferred]. *Against:* it is a proposal, not a working-group draft; the group's later reports do not adopt its visual cursor model (2.6); and it conflicts with terminal-wg.
- (d) terminal-wg in full, with both modes, so that applications choose. *For:* the spec's own argument that no single mode covers every use.

**Q2. The default mode.**

- (a) Implicit. *For:* it is the terminal-wg default; VTE, mintty, iTerm2, Konsole and mlterm all reorder by default; `cat`, `ls` and `git` read correctly.
- (b) No reordering until an application or the user asks. *For:* Windows Terminal, conhost and xterm do not reorder, and WezTerm defaults to off. kitty reverses each RTL word by default, and offers `force_ltr` as a user switch to turn that off, for use with FriBidi. *Against:* plain output reads backwards.
- (c) Implicit on the primary screen, off on the alternate screen. *For:* mintty's `Bidi=1` and iTerm2's opt-out. *Against:* terminal-wg says the two screens "should behave identically".
- Evidence against (a): Emacs and vim -H order RTL themselves and send no RM 8 (section 5). Agent CLIs that order their own text would break too (documented for Claude Code on Windows only; untested here): an iTerm2 code comment says "certain agent CLIs" pre-order their text, but no agent CLI was tested and none was checked for RM 8 (Q17).
- Coupled with Q3, Q5 and Q17.

**Q3. The coupled defaults: autodetect (`?2501`), the default paragraph direction, and arrow swapping (`?1243`).**

- These three must be chosen together. The arrow-key page argues that `?1243` on by default stays backward compatible *because* autodetect is off and the default direction is LTR [source: arrow-key-swapping.html; critic].
- Autodetect: off per terminal-wg and VTE; on per mintty; behind an advanced setting, off by default, in iTerm2.
- Direction: always LTR, or taken from the locale or a setting. terminal-wg says applications must not assume LTR, and lets the terminal's default come from settings or the locale.
- Where first-strong detection runs for arrow swapping:
  - (i) the embedder passes in the direction the renderer resolved;
  - (ii) the core caches the first strong character per row at write time, an ingest cost to measure;
  - (iii) autodetect stays out of the core.
- Implementing `?1243` to spec needs XTSAVE/XTRESTORE, which baud lacks (7.1).
  *Built 2026-10-09.*

**Q4. Level 1 or level 2.**

- (a) Level 1, per the terminal-wg text: discard the twelve controls. In baud that is a write-path change in baud-core, because baud stores them today, so it needs an ingest2 A/B.
- (b) Level 2: use the controls baud already stores as marks. *For:* VTE implements most of it (2.5); UTS #55 expects LRM to work; and UAX #9 §4.2 says conforming systems "generally" fall into four classes. The lowest that reorders at all ("Implicit bidirectionality") supports ALM, RLM and LRM, so a terminal that reorders but discards them (terminal-wg level 1) fits none of the four. Level 2 also needs each of `attach`'s drop paths handled: column 0, after an empty cell, a cluster already holding 8 continuations, and a store refusal (7.1).
- Level 2 interacts with Q7, because every control mints a cluster key.

**Q5. What RIS and DECSTR do to BDSM, and what DECRQM answers for mode 9.**

- ECMA-48: RIS returns the device to its initial state, which "may imply, if applicable", setting modes to their reset state (§8.3.105); §7.1 recommends that a mode's reset state be its initial state. For BDSM that reset state is EXPLICIT.
- terminal-wg: a generic reset makes BDSM implicit. VTE and iTerm2 follow terminal-wg.
- DCSM: ECMA-48 contradicts itself (2.3). Open terminal-wg issue #9 proposes forcing it to DATA ("SET state"), and VTE does this.
- Following terminal-wg departs only from ECMA-48 §7.1's recommendation, not from a requirement; whichever baud chooses is recorded with that scope.
- Related, from issue #3: should DECSC/DECRC save the BiDi modes? Koblinger: "Probably restoring, yes"; VTE #2785 asks for it.

**Q6. Phase 0: per-script shaping without bidi, and the hb-shape oracle.**

Phase 0 means four things: a script argument for `Layout::lang_sys`, per-(face, script) caches, itemisation from Scripts.txt, and a mark sort by HarfBuzz's modified (SBL) combining classes (7.2).

- (a) Match HarfBuzz: try `hebr` first, then `DFLT`, `dflt` and `latn`, and fall back to positioning marks by combining class when the GPOS script is not `hebr`. *For:* the oracle stays exact. *Against:* faces whose Hebrew anchors sit only under `latn`/`DFLT` lose anchor placement.
- (b) Keep baud's `latn`/`DFLT` fallback after `hebr`. *For:* today's anchors survive. *Against:* it departs from hb-shape for those faces, so the fixture gate needs an exception.

How runs are itemised [inferred from the critic's correction of the Ghostty claim]:

- Runs break at a face change (renderer.rs:2245-2247), which is enough only while Hebrew comes from a fallback face, as in the system-fallback interim. Under the owner's 2026-10-07 direction, Hebrew is patched into the embedded font (TODO.md:2545-2556; note AC), so the default face covers both scripts and runs must be split by script (Scripts.txt/ScriptExtensions), as Contour does.
- A face that covers both Latin and Hebrew, such as Courier New or Liberation Mono used as the primary face, needs runs split by script, as Contour does.

Phase 0 alone does not fix pointed text; see Q7.

*Built 2026-10-13, all but the mark sort* (docs/record.md, "OpenType script selection, phase 0"). The OpenType text settles the order: "An application should use a DFLT script table if there is not a script table associated with the specific script of the text being formatted, or if the text does not have a specific script" [source: ms-opentype-chapter2, ScriptList table]. So a run takes its script's registered tags, then `DFLT`; `latn` is kept after `DFLT` only for a face with neither, because the text is silent there and that was today's behaviour. That is neither (a) nor (b) as written: it drops (b)'s `latn` before `DFLT`, and it keeps `GPOS` on for Hebrew where (a) would turn it off for a face with no `hebr` table, since the text sends that text to `DFLT`'s anchors. Itemisation follows UAX #24 5.1-5.3 [source: uax24], and since 2026-10-15 paired punctuation too: a bracket pair takes the script of the enclosing text, as 5.1 says it "should therefore match the script of the enclosing text", with the pairs from BidiBrackets.txt [source: unicode-bidibrackets] matched by UAX #9's BD16 [source: uax9-bidi] (docs/record.md, "A bracket pair takes the script of the text enclosing it"); quotation marks keep the preceding script, since 5.1 says it "is not possible to tell from the character code alone" which way one faces; runs are cut where the lookups a script selects on the face change, so on the embedded text faces no run is cut: the built Regular and Bold's `DFLT`, `latn` and `hebr` name the same lookups, as stock JetBrains Mono Italic and Bold Italic's `DFLT` and `latn` do, and the symbols face has no `GSUB` or `GPOS`. STIX Two Math, the embedded math face, is the exception: its `DFLT`, `latn`, `grek` and `cyrl` select 4, 3, 3 and 2 `GSUB` lookups, so Latin beside Greek is two runs on it. Digests, pairs and extent are per face over every script's lookups, because each is a property of a lookup. The mark sort is not built: no text requires an order between Hebrew points of different classes (canonical order and HarfBuzz's SBL order are both canonically equivalent), and the choice is recorded as a Jev question with Courier New's evidence that canonical order leaves a second point one cell right. Jev's answers to all four choices, the measurements, and the patch for the one the owner decided: section 11.

**Q7. Cluster store capacity.**

- The store has 63 slots, probes two slots and refuses on collision or when full, and is freed only by RIS; `attach` interns every growing prefix as a permanent slot (7.2). Measured in baud, `marks_dropped` reaches 10 by verse 2 and 45 by verse 5 of a hand-typed pointed Genesis 1:1-5 (7.2).
- (a) Grow the store as the rendition table was [source: CLAUDE.md, the "Do not change these" table]. Unlike StyleId (its own u16 field), ClusterId is 6 bits of a full CellFlags word [source: cell.rs:216-228, 498-502]. Growth needs a wider cell, bits taken from LinkId (NLINK=128 uses all 7), or an indirection. The table also needs a longer probe than two, or it still refuses on collision.
- (b) Reclaim clusters that are no longer on screen or in scrollback. Alone it cannot hold one screen of pointed text: Pango's pointed Genesis 1 (13 lines) has 175 distinct marked clusters, 102 without cantillation, all > 63. Reclaim must also free prefix slots superseded when a cluster grows [inferred].
- (c) Leave it bounded and keep counting refusals. *Against:* vowel points vanish.
- CLAUDE.md requires the attacker-keyed style and link tables to stay bounded. Cluster keys are attacker-supplied too, so by the same reasoning any option must stay bounded [inferred].
- It needs a red test, pinning `marks_dropped > 0` on that sample, before any choice is made.
- *Answered 2026-10-13 by the owner and built 2026-10-14*: (a) and (b) together, 1024 slots with the index in the cell's `Content` above U+10FFFF, eight probes, and a sweep of the live cells that frees what no cell names (7.2's postscript). It stays bounded: a cluster whose eight slots hold others is refused and counted, before the store fills and after a sweep. The eight-continuation cap (`ClusterSlot::CAPACITY`) was left open by the owner and is kept and stated in `docs/vt-coverage.md`.

**Q8. Erased cells between characters.**

terminal-wg specifies only trailing erased cells.

- (a) Treat `Content::EMPTY` between characters as a space (bidi class WS). *For:* VTE (bidi.cc:520-522) and iTerm2 do this, and iTerm2 does it specifically for Claude Code/Ink (iTerm2@91411f5 sources/Drawing/BidiDisplayInfo.swift:237-256).
- (b) Skip them, as terminal-wg skips trailing ones. *Against:* the runs on either side merge, which is the suspected cause of the tab failure the Ghostty tester reported.

**Q9. Where the per-paragraph properties live.**

- A new per-row Storage slice on both surfaces (7.1) is the only placement found that does not break `RowExtent` or `CellFlags`. Its ingest cost must be measured before commit.
- Paragraph cap: terminal-wg suggests falling back to explicit beyond about 500 lines (VTE's value), plus extra hidden scrollback lines so the oldest visible paragraph still resolves.

**Q10. How RTL rows are drawn on the default cell path.**

- (a) A per-row visual-to-logical map buffer, plus a mirror flag for rows that are a plain mirror (explicit RTL rows, and implicit rows that are entirely RTL).
- (b) Route every cell of an RTL row through overlay records. This also needs background records, because the background pass draws at the logical x.
- (c) Fall back to the instance path for frames that contain RTL rows.

The pixel oracle must cover whichever option is chosen.

**Q11. Mirrored glyphs and joining forms on the cell path.**

- Either a glyph-map key bit fed by a per-cell flag, or the overlay.
- In either case, an LTR fast-path bit could use MapEntry's free pad bits 26-31.

**Q12. Arabic joining: reverse D15?**

If yes, it brings:

- per-glyph feature masks and rlig;
- possibly GSUB types 2 and 8 and GPOS cursive;
- context across the 32-cell run cut and across SGR changes;
- a monospace Arabic face.

It also needs a lam-alef policy, because wcwidth's two columns meet a one-cell glyph:

- (a) spread or centre the ligature over two cells;
- (b) leave a blank cell beside it (ICU's LAMALEF NEAR);
- (c) fill the spare slot with U+FEFF (FriBidi);
- (d) refuse the ligature, which breaks the mandatory rlig;
- (e) offer a single-cell mode like mintty's `?2521`.

In terminal-wg #7 (open), Koblinger wrote that wcwidth "must be respected" and that the joined lam-alef "will take up 2 cells". In #4 (open), mintty's author argued for keeping the locale's widths, and later added a single-cell mode (`?2521`). Neither is spec text. Only Courier New was measured.

**Q13. Shaping in explicit mode (issue #7, open).**

- (a) The terminal shapes the visual contents. This is Koblinger's reversal in #7; VTE shapes per visual word.
- (b) Presentation forms pass through untouched. This is the spec text and mintty's position.
- (c) A per-paragraph switch, as floated in #7.

vim with `arabicshape` already emits presentation forms.

**Q14. Cursor.**

- Movement: logical plus arrow swapping (terminal-wg), or visual (L2/23-107).
- Shape: the I-beam on the right side of its cell for RTL-resolved characters and in explicit RTL, plus an optional ⎡/⎤ direction hint (VTE).
- Pending wrap: hide the cursor there, which needs the core to expose the state (term.rs:2084-2110).

**Q15. Selection.**

- A logical stream, which is today's model and AppKit's and terminal-wg's; and/or a visual or block option (TextKit 2, Word, L2/23-107).
- What an Option-drag block selection means over an RTL paragraph.
- Whether to add marks or isolates when copying, following Word's precedent.

**Q16. Box drawing, pictures and images.**

- `?2500` box mirroring. terminal-wg calls VTE's `?2500` experimental, and mintty labels its bidi sequences experimental. VTE mirrors U+2500-257F; mintty mirrors U+2500-259F (box drawing plus block elements).
- How text-drawn pictures behave in RTL paragraphs, since box and block characters are neutral and reorder.
- Whether image alignment mirrors. terminal-wg: images are not flipped by default (a protocol may add a flip flag); alignment "should be revised".

**Q17. The alternate screen and agent CLIs.**

- Reorder it by default, as iTerm2 does (and reportedly Terminal.app)?
- Or not, as mintty's `Bidi=1` and iTerm2's opt-out (which names agent CLIs) allow?
- No researcher tested Claude Code or any other agent CLI. Test before choosing.

**Q18. How applications discover baud's bidi support.**

- DECRQM 8 is the only probe today. terminal-wg's feature-reporting page offers no mechanism, and baud has no terminfo entry of its own and no XTGETTCAP row.
- tmux needs `Bidi` in terminfo for its isolates. Emacs and vim would need some way to see that the terminal reorders before they switch it to explicit mode.
- Options:
  - ship and install a baud terminfo entry;
  - answer XTGETTCAP;
  - rely on DECRQM alone.

**Q19. The ECMA controls terminal-wg does not adopt.**

- SRS, SDS, SIMD and SAPV: `out` rows with a reason, or implemented?
- SPD 0/3 as aliases of SCP 1/2?
- DEC's DECRLM, DECRLCM and DECHEM: they are data-layer modes that conflict with display-only, but j4james argues for them.

**Q20. Unicode version for the bidi tables.**

- 17.0.0 matches baud's other UCD pins and rev 51.
- 18.0.0 adds two brackets and five mirroring pairs (2.4).
- The test files' data is identical in both, and both sets of bytes are saved.

**Q21. Native digits and user direction toggles.**

- Substitute Arabic-Indic digits by context, as Windows and Word do, or always draw ASCII digits, as Apple's HIG says for Hebrew?
- Offer a user-side paragraph-direction toggle, such as macOS's Cmd+Ctrl+Left/Right (`makeBaseWritingDirection*`, measured in StandardKeyBinding.dict) or Rich Edit's Ctrl+Right Shift? These would compete with SCP and with applications.

**Q22. The owner's request: fall back to the system font when the configured font lacks a symbol.** (Answered by the owner on 2026-10-07; note AC carries the design.)

The request is quoted in section 1.

Answered 2026-10-07: configured font, then the embedded (built) font, then the system's choice for anything nothing shipped has (option c). Same pixels holds for everything baud ships; system fallback differs per OS only where the alternative is a box (TODO.md:2557-2562). Note W is superseded by AC. Hebrew is to come from baud's own patch on JetBrains Mono, not Liberation Mono (TODO.md:2554). Whether the arielagor bridge stays as the interim is the open part (AC §1).

Evidence:

- The fallback stack is fixed at startup. A configured `fonts` list gets only the embedded symbols face and STIX appended, never a system face [source: config.rs:840-846, 1101-1135; stack.rs:248-254].
- No face in the owner's stack, in the macOS or Linux defaults, or among the six embedded faces maps any Hebrew character [source: aa-shell-locale.md:154-166].
- Starting from baud's JetBrains Mono, CoreText's `CTFontCreateForString` picks Courier New for Hebrew, for a pointed shin, for Arabic beh and for lam-alef. The language hints `he` and `ar` do not change the choice. Every advance is 9.60 or 0.00, and a base and its marks stay in one face [measured: engines/probe/probe2 on baud-app/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf].
- Starting from a proportional base face, CoreText instead picks Lucida Grande and Geeza Pro, neither of them monospace. Starting from Menlo, it picks Courier New [measured].
- `kCTFontURLAttribute` returns a file path for a system fallback face. CoreText can therefore be used only to *find* the face while baud-font parses it, which keeps the no-engine rule [measured; inferred].
- `kCTFontCascadeListAttribute` would let baud prefer monospace faces [source: CTFontDescriptor.h:124-130].

Options:

- (a) System fallback through CoreText on macOS and fontconfig on Linux, as asked. *Against:* the face Stack borrows a fixed set of faces, so it would have to grow at runtime. It also conflicts with same pixels on every OS: macOS lands on Courier New, whose licence is unchecked, while Linux resolves through fontconfig coverage.
- (b) Bundle a monospace Hebrew face, such as Liberation Mono (Jev scored it 0.75 in note AA). Superseded: Hebrew is to come from baud's own patch on JetBrains Mono (TODO.md:2554). No Arabic face was measured besides Courier New.
- (c) Bundled faces first, the system last. This is the owner's answer.

If fallback ever selects faces per whole cluster, it must leave default-ignorable characters (ZWNJ, ZWJ, LRM, RLM, ALM, the embeddings and the isolates) out of the coverage test. Ghostty (run.zig:352-380 @befcdfd) and foot (#2049) fell back, or drew U+FFFD, because of them. This was not a problem in baud while it selected the face by base codepoint, and it already skipped `Ignorable::Yes` glyphs when drawing (baud-font shape.rs:15-17) [source; critic]. Since Q23 (2026-10-07) baud does select a marked cell's face per whole cluster, and its coverage test, `Normalised::covered` in baud-font/src/shape.rs, exempts every `Ignorable::Yes` codepoint, the variation selectors among them. `mark_coverage::an_ignorable_is_not_a_mark_a_face_must_have` pins it on a stack where only the second face maps ZWJ and ZWNJ.

**Q23. A cluster whose marks one face does not all draw.** (Decided by Jev on 2026-10-07 and built the same day.)

**The bug.** baud chose a cluster's face from its base codepoint alone. When
that face lacked one of the marks, the shaper mapped the mark to `.notdef`.
JetBrains Mono classes `.notdef` as a base with a full-cell advance. So a box
drew one cell to the right, and every later mark landed over the box instead of
over the letter.

Measured on q + U+0316 + U+0301 with JetBrains Mono alone:

- q draws at 0, the box at +1 cell, and the acute at +1 cell.
- HarfBuzz (uharfbuzz on the same file) gives the same positions: q 600,
  `.notdef` 600, the acute 0 at offset 0. So the shaper is faithful, and the
  fix belongs above it.
- Nothing counted the loss.

**What others do, read at source** (a read-only workflow, 2026-10-07; every line is
cited at the commit named, and nothing was built or run):

| Terminal | Face for a cluster with marks | When no face draws them all | Counted |
|---|---|---|---|
| Ghostty (befcdfd, run.zig:318-382) | First of: the base's face, then each mark's own fallback face, that maps the base and every mark except FE0E, FE0F and ZWJ | The whole cell becomes U+FFFD; the letter is lost (run.zig:229-262) | no |
| kitty (7d37453, fonts.c:652-854) | One face must map the base and every rendered mark; a single mark may be met by a precomposed form; else a fallback search for the whole cell text | The whole cell draws kitty's missing-glyph box (fonts.c:2141-2147) | no |
| foot / fcft (cb27717 / 1ff7e79, fcft.c:2262-2394) | First face in the whole fallback list whose charset has every codepoint, ZWJ exempt | The primary face with `.notdef`, baud's old failure (fcft.c:2391-2393) | no |
| WezTerm (372548, harfbuzz.rs:412-493) | Reshapes any cluster holding `.notdef` with the next face, recursively | The primary face with `.notdef`; its box drifts a cell right too | no |
| Alacritty (d692748, glyph_cache.rs:200-265) | No shaping: each mark from its own first covering face, drawn at the base cell's origin | A mark no face has draws nothing | no |
| Windows Terminal (099afa4, AtlasEngine.cpp:880-1236) | DirectWrite's `MapCharacters` decides, and its cluster rule is closed source; its maintainer says it splits ZWJ emoji (#18167) | One U+FFFD per cell the unmapped range reaches the end of, at the cell's width (1218-1232). A range that ends inside a cell, such as a missing mark before another mark, draws nothing | no |

Ghostty fixed exactly this bug in 2023 (commit e6edf3105: "combining
characters ... rendering as boxes"; issue #337, PR #339).

The default-ignorables have broken this kind of coverage test elsewhere:

- Ghostty exempts only FE0E, FE0F and ZWJ. It added ZWJ (42c4f5271, #914)
  because "U+200D isn't meant to be provided by any font".
- foot #2049: an LRM inside a grapheme sent fcft looking for a font that
  covers it.
- kitty exempts the whole non-rendered set: Cf, Other_Default_Ignorable and
  VS1-VS16 (gen/wcwidth.py:1129-1131).

**Decided (Jev, 0.99 at confidence 0.98, every cost below in the request).**

Jev was asked three times:

- **First ask, earlier the same day, before the prior art above was read.**
  It offered three fixes and put a face per whole cluster at 0.83
  (confidence 0.74, under the bar). That option kept the base's face, box
  included, when no face covers the cluster.
- **Second ask, with the prior art and the costs in the request.** The
  no-cover case was split into dropping and replacing, and A_DROP scored
  0.99.
- **Third ask, after a refuter found two of the second request's premises
  wrong (corrected under the costs below).** A_DROP scored 0.99 at
  confidence 0.99.

- A cell with marks draws from the first face that covers the base and every
  mark. The candidates are the face its base would get, then the fallback chain
  in order.
- Coverage is judged after the shaper's own normalisation, so a precomposed
  form counts. Default-ignorables and variation selectors are never required.
- If no face covers the cluster, the base keeps its face, and only the marks
  that face cannot draw are dropped, before shaping, and counted.
- Unmarked cells take the old path.

The costs Jev weighed:

- **Faces can mix inside a word.** Once note AC's Hebrew is built, a
  configured font with Hebrew letters but no points will draw pointed letters
  from baud's embedded face. Ghostty, kitty, foot and WezTerm all accept this.
  Today no embedded face maps Hebrew, and a configured stack appends only the
  icon face and STIX, so those points are dropped and counted.
- **A styled face is not in the fallback chain.** So a bold letter whose bold
  face lacks a mark draws from a regular face, as in Ghostty. Today this
  reaches the default stack. The embedded JetBrains Mono Bold, Italic and
  Bold Italic lack U+0305, U+0313 and U+0316, which Menlo maps, so a bold
  q + U+0305 draws from Menlo Regular
  (`mark_coverage::a_bold_cluster_its_bold_face_cannot_draw_falls_to_the_chain`).
  A configured `fonts` stack has no styled face at all (config.rs:840-846).
  - The second request to Jev said the embedded font had every point in every
    style, so this cost reached only configured styled fonts, which baud does
    not load.
  - Both premises were wrong: the first describes note AC's planned font, not
    the built one.
  - The third ask corrected both, and the answer stood.
- **A coverage test runs per marked cell, remembered per cluster.** Only the
  run path is priced: the frame bench's convert step, on synthetic marked
  payloads. Where every face has the marks it read +1.7%, with the ranges
  overlapping, so there is no result. The cell path's overlay is the app's
  default path, and it is unmeasured. See the record entry.

The alternatives scored 0.01 or less:

- replacing the whole cell (Ghostty, kitty);
- the base's face only, dropping marks;
- overlaying each missing mark from another face (Alacritty).

**Built (2026-10-07).** One definition answers every path:

- `Normalised` in baud-font/src/shape.rs is the shaper's normalisation, moved
  out of `push_cluster` unchanged. `covered()` and `without_unmapped()` judge
  the same sequence the shaper maps.
- `Stack::select_cluster` in baud-render/src/stack.rs walks the base's face and
  then the fallback chain. `Renderer::face_for` in baud-render/src/renderer.rs
  asks it for a cell with marks and remembers the answer per cluster. So a
  marked cell costs a memo probe a frame rather than a walk of the chain, or
  two probes when it breaks a run, since it is asked again as the next run's
  first (`Renderer::face_of_cell`).
  - On a run-cache miss, `shape` asks a second memo once per marked cell:
    whether the face draws the cluster whole.
  - A cell with no marks reads the face table, as before.
- The run path, the overlay's marks pass and the overlay's ligature window all
  ask `face_for`. A marked cell whose cluster moved face leaves the window and
  is drawn by the marks pass.
- `without_unmapped` repeats normalise-then-drop until a pass drops nothing.
  A reviewer found that one pass is not enough. U+1F82 decomposes three levels
  deep, and the normalisation takes two. So for U+1F82 + U+0301 the first pass
  kept U+1F00 as the base and dropped only U+0345, its ypogegrammeni. The
  shaper then took U+1F00 apart again, and U+0313, which JetBrains Mono lacks,
  drew as an uncounted box one cell right. Since 2026-10-18 a base the face
  cannot draw in parts stays whole, one `.notdef`, and its decomposition typed
  draws the same (the record, "A letter no face draws is one box, typed either
  way");
  `mark_coverage::a_base_the_face_cannot_draw_in_parts_is_one_box_typed_either_way`
  pins that.
- `Renderer::marks_unmapped` counts what was dropped, per shaping: every
  codepoint after the first of the normalised cluster that the kept face lacks
  and that is not default-ignorable.
  - That includes attached codepoints that are not combining marks. The parts
    of a precomposed base the face lacks were counted too (U+1F82 + U+0301
    counted 2, for U+0345 and U+0313) until 2026-10-18, when such a base
    became one `.notdef` and counts nothing.
  - A ligature window shaped in chunks counts a cell only in the chunk that
    owns it.
  - Not counted: codepoints the normalisation cuts past its sixteenth, and marks
    on box-drawing, block or braille cells.

The tests are `baud-render/tests/mark_coverage.rs` and
`baud-render/tests/missing_mark.rs`. The cost is in the record (docs/record.md,
"A cluster no face draws whole drops what its face lacks, counted").

**Open.**

- **The owner's call: attached codepoints that are not marks.** As built, the
  drop also covers an emoji after a ZWJ, a skin-tone modifier and a flag's
  second regional indicator. On an emoji face that lacks a newer member, a
  ZWJ sequence draws what is left of it. This comes from reading the code, not
  from a probe: woman + ZWJ + laptop on a face without the laptop draws a bare
  woman, a different emoji, and nothing on screen says so.
  - The alternative drops only combining marks. A cluster with any other
    codepoint its face lacks draws as one `.notdef` box in its cell, counted.
  - Jev put the built rule at 0.55 against 0.45 (confidence 0.10), so the owner
    decides from renders of both.
- **Pre-existing, outside the brief.** An unmarked precomposed character its
  face lacks, such as U+1F00 on JetBrains Mono, still decomposes in the shaper
  and draws an uncounted box one cell right.
  - On the cell path, which is the default, `Renderer::single` keeps only the
    first glyph, so it also loses marks the face has.
  - U+1FB0 draws as a bare alpha without its breve, and U+1E09 as ç without its
    acute, though JetBrains Mono maps U+0306 and U+0301 and the run path draws
    both. The two paths draw such a cell differently.
- **Pre-existing, widened by this change.** During a smooth scroll, the row
  leaving the screen draws each marked cell's letter alone, from the cell map,
  until the row has slid out. The cell shader reads no overlay mask for that
  row; see the comment in `the_cell_arm_draws_what_the_instance_arm_draws`,
  baud-present-macos/tests/cells.rs.
  - Its marks were always lost there.
  - A letter whose variation selector moves it to another face already drew
    from the letter's own face there.
  - Now a cluster that `face_for` moves to another face for its marks does too,
    for example q+U+0305 to Fira.
- **As designed, noted for the owner.** The walk down the chain tests the cmap
  only:
  - it ignores presentation, so U+263A U+FE0F U+0301 draws a text smiley with
    the acute;
  - a text keycap moves to a colour face that maps its two codepoints, even
    when that face has no keycap ligature.
    - On the Noto test subset, which maps both codepoints but has no keycap
      ligature, U+20E3 then draws as its own glyph 17 px right of the `1`,
      outside the cell, with `marks_unmapped` at 0. That comes from a
      reviewer's probe on [JetBrains Mono, Noto subset], not re-run.
    - Apple Color Emoji forms the ligature, and nothing spills.
    - HEAD was not run on this case.

**Q24. A cluster that starts after a control, for Jev.** (Open departures, found 2026-10-10 while making every control end the cluster before it; `docs/record.md`, "A control ends the grapheme cluster before it".)

Since that change a control ends the cluster before it for every continuation that **takes a column**: a SpacingMark (GB9a), or an Extend with a width such as `U+09BE` or `U+1F3FF` (GB9), after a line feed, a tab, a cursor move, an erase, `NUL`, `DEL`, any escape or control sequence or control string, takes a cell of its own, as GB4 and GB5 give it ([[source: uax29]]). Two cases departed; the first was built on 2026-10-23 (item 1), and both are pinned by `every_line_of_the_suite_clusters_through_the_terminal` and `a_control_function_ends_the_cluster_before_it` (baud-core/tests/encoding/graphemes.rs).

**1. A zero-width codepoint after a control.** **Built 2026-10-23 for combining marks and default-ignorables as (a), the mark stored alone** (Jev, OWN_CELL_MARK_ALONE, 0.96/0.99, against (b) at 0.03/0.01): a combining mark takes the cursor's cell with `Content::DEFECTIVE` for its base, drawn on U+00A0 by every render path, copied, searched and selected as the mark; a default-ignorable one is dropped and counted (Jev's lean, 0.22/0.48). A Prepend, or a zero-width codepoint of class Control such as U+200B, after a control still joins the cell before the cursor, an open departure (vt-coverage's cluster row). The record, "A combining mark after a control takes a cell of its own". What follows is the state before. `Paint::glyph` sent every codepoint of width 0 to `Paint::attach` whatever the break decision, because a zero-width codepoint has no cell to take. So after a control a combining mark, a ZWJ, a variation selector or a Prepend still goes where it went before the change:

- after `BS`, `CUB` or `CHA` back over a letter it joins that letter: `ab BS U+0301` puts the acute on the `a`;
- after a control that moves nothing (`NUL`, `DEL`, `BEL`, a mode change, an `OSC`) or that lands beside a letter (`CUP`, a C1) it joins the cell before the cursor: `ab NUL U+0308` puts the diaeresis on the `b`;
- after `LF`, `HT`, `CUF`, `ED 2` or `EL 1`, where the cell before the cursor is empty, it is dropped and counted in `marks_dropped`; so is a width-1 SpacingMark that follows it in the same cluster (suite line 84 after a letter: `LF U+0308 U+0903` drops both).

GB4 and GB5 make each of these a cluster of its own, and the Unicode Standard names the case: "Defective combining character sequences occur when a sequence of combining characters appears at the start of a string or follows a control or format character" (D57, [[source: unicode18-ch3]]), and "Defective combining character sequences should be rendered as if they had a no-break space as a base character" (§ 5.13, [[source: unicode18-ch5]]). So NUL and DEL, which end the cluster for a continuation with a width, do not end it for a zero-width one.

- (a) **Its own cell, with a placeholder base.** The mark takes the cursor's cell on a base the core supplies and stores it as a cluster of its own, so it renders as § 5.13 recommends. The base is U+00A0, § 5.13's text, or U+25CC DOTTED CIRCLE, the "Show Hidden" fallback § 5.13 describes ("the nonspacing mark positioned on a dotted circle"). D52 allows the mark to be drawn "as if it were a base character" instead, which a cell can do with no base at all. *Conforms* to GB4/GB5 and § 5.13 (a recommendation). *Costs:* a cell write and a cluster-store slot per defective sequence, on the cold `attach` path, and a decision about what copy returns for the base (C7 reaches copy, note AD § 2.6); the placeholder must not be mistaken for text the application wrote.
- (b) **Attach as today.** *Departs* from GB4/GB5 and from § 5.13's recommendation; the mark lands on a letter the application did not put it on, or vanishes. Nothing new is written.

Note AD § 2.6 records the same refusal at column 0 and on an empty cell for selectors and tags, and its options apply here too.

**2. REP.** `Paint::repeat` writes its copies through `Paint::glyph`, which asks the machine nothing, and then tells the machine one copy went past, starting it from the state a control leaves. So a copy never joins what came before it, and a continuation joins the last copy:

- `U+1F1E6 CSI b U+1F1E7` writes A, A and B, and pairs the B with the copy: A, then the flag AB. Read as the text it expands to, A A B, GB12/13 pair the first two and leave B alone.
- `U+1100 CSI b U+1161` writes two cells, L, then the copy L with V; GB6 makes L L V one cluster.
- `ab CSI b U+0903` is `a`, `b`, and the copy `b` with the visarga, which both readings agree on.

GB4 read on the `ESC` of `CSI b` breaks before the copy, which is what baud does; read on the expanded text it would not. [[source: ecma48]] 8.3.103 says REP indicates that "the preceding character in the data stream, if it is a graphic character ... is to be repeated n times", which says what is written and not how it clusters. *For Jev:* (a) keep the break before the copy, GB4 on the sequence; (b) treat the copies as the text they expand to, joining a copy to the cluster before it where the rules say so, which needs the machine walked across each copy rather than started again.

**3. Whether NUL and DEL end the cluster at all.** Both are Control to GB4 and GB5, and `[[source: gbtest]]` line 121 is `÷ 0000 ÷ 0903 ÷`, so baud ends the cluster at them, for a continuation that takes a column. Against that, [[source: ecma48]] 8.3.88 says NUL characters "may be inserted into, or removed from, a data stream without affecting the information content of that stream", and the parser discards both; read that way the visarga after `x NUL` joins the `x`. Implemented at face value of both texts: NUL changes nothing on the screen and is still a Control. For a zero-width codepoint they already do not end it (item 1): `ab NUL U+0308` puts the diaeresis on the `b`.

A plain SGR, which an earlier draft of the change left joining, ends the cluster like every other control sequence: its `ESC` is a Control to GB4, so leaving it joining would be a departure, not a choice. What conforming costs: against the same change with SGR left joining, `btop` 2.0% lower on the medians, ranges overlapping, and the other four recordings within 1% (`docs/record.md`, the same section).

## 9. Sources to pin

### 9.0 Repairs to existing manifest entries

- `uax9-bidi`: point its URL at `https://www.unicode.org/reports/tr9/tr9-51.html`. The hash stays 25ebbc5b… (187,487 B), and that revision matches baud's 17.0.0 UCD. Add rev 52, listed in 9.2, only if Q20 moves to 18.0.0.
- `terminal-wg-bidi`: keep the index hash, which is still byte-identical. Document the entry as index-only and add the recommendation subpages listed in 9.2. Correct `year = 2025` to 2019 (v0.1; copyright 2018-2019).

How the lists below were built [measured]:

- The pin lists of the five research keys and the critic were merged with the critic's hashes, giving 373 entries for 345 unique URLs, and no URL carried two different hashes.
- The Windows deep dive's lists (windows/wt/pins.json and windows/text/sources_to_pin.json) are not merged: 149 further URLs, including L2/24-264, L2/25-179, L2/25-263, the 2023 and 2025 L2 registers, and the WT #12722 and #20410 snapshots. They must be added before pinning. WT #19802 was saved (wt/gh/threads) but is in no list.
- Every saved copy was re-hashed, and all matched their reported sha256 and byte counts.
- 13 saved files are byte-identical to a manifest entry: the 12 in 9.1 plus the terminal-wg index, which merge_pins.py drops. The other 332 are new. One of them, the unversioned UAX #9 URL, should not be pinned, and 76 are issue-tracker or API snapshots that will not stay byte-stable.
- The `file://` rows are local copies of macOS SDK headers and system files.
- Where two keys read the same project at different commits, both commits are listed and each fact cites its own: Ghostty at d9840f3 and befcdfd, kitty at 6d5d0c4 and 7d37453.

The merged pin list so far is `synthesis/pins-merged.tsv` in the scratchpad (sha256 `92ccb68a6d23ffa0ac8b1aa7ab64dce5da9e3a1bd8f8f5f0a45232c90b8849ba`, 77199 bytes; every row re-hashed against its saved copy). The TSV has 344 rows: every unique URL except the terminal-wg index. It is not authoritative until the Windows deep dive's lists are merged into it. The tables below copy it: 9.1 to 9.3 in full, and 9.4 and 9.5 as summaries by project, to keep this report one reply long. The full rows of 9.4 and 9.5 are in the TSV and in `synthesis/section9-full.md`. The TSV also carries the unversioned UAX #9 URL (same bytes as `tr9-52.html`), which should not be pinned.

### 9.1 Already in the manifest

These saved files are byte-identical to an existing manifest entry [measured: the sha256 of the saved copy equals the manifest hash]. Cite them by the existing key.

| saved URL | sha256 | bytes | manifest key | note |
|---|---|---|---|---|
| https://www.unicode.org/reports/tr9/tr9-51.html | `25ebbc5bc05ec302677ace0e3118e454e12da5e1e7ebec386aabd6e585c8cebe` | 187487 | `uax9-bidi` | manifest URL is unversioned and now serves rev 52; repin to this URL (9.0) |
| https://raw.githubusercontent.com/wezterm/wezterm/cab25161054c50fd6c705db4ceefef0f1e5a9575/wezterm-gui/src/termwindow/render/screen_line.rs | `791fd925b0e229f2f4b103c1a6e3bb88a92b7392f59305477dfe7bd18d679623` | 37932 | `wezterm-screen-line` | same bytes at a different commit (manifest names b09b56c) |
| https://raw.githubusercontent.com/wezterm/wezterm/cab25161054c50fd6c705db4ceefef0f1e5a9575/wezterm-font/src/shaper/harfbuzz.rs | `62456019495034ad29bec16eae68b1eda20b643f82d2b0e4896c17495b38da30` | 41456 | `wezterm-shaper-harfbuzz` | same bytes at a different commit (manifest names 2afb836) |
| https://raw.githubusercontent.com/ghostty-org/ghostty/d9840f3c8fc230c7768ae760c412974e9fc923bb/src/font/shaper/coretext.zig | `f5e541e4da646d9c972ee5e385da3aaa048c9003901e4dbda6969a68684652de` | 92525 | `ghostty-font-shaper-coretext` | same bytes at a different commit (manifest names 9bbb9b2) |
| https://raw.githubusercontent.com/kovidgoyal/kitty/6d5d0c440603ad9bdf6dcd599f73f6dde21acb44/kitty/options/definition.py | `d121fedddbecbfa0e76aba0e11ecfe1cb11946dd362bc097b83da54093b9a107` | 160774 | `kitty-6d5d0c4-options` | same URL: already pinned |
| https://raw.githubusercontent.com/ghostty-org/ghostty/befcdfd2c3a1cb24d9ec886e93c95b2b5daa7028/src/font/shaper/harfbuzz.zig | `cce3a6e6ac8492f4c402401aafe2c1ea2b078c0faf7b900bc7d54659cb6480fc` | 75420 | `ghostty-font-shaper-harfbuzz` | same bytes at a different commit (manifest names 9bbb9b2) |
| https://raw.githubusercontent.com/ghostty-org/ghostty/befcdfd2c3a1cb24d9ec886e93c95b2b5daa7028/src/font/shaper/run.zig | `db733c86a1c4454ee17bb838f6af889d3bffa0d693b6e09f22ab3765695462b7` | 15967 | `ghostty-font-shaper-run` | same bytes at a different commit (manifest names 9bbb9b2) |
| https://raw.githubusercontent.com/alacritty/alacritty/d692748d3f61253ebe9f5094320120d22f6a046f/alacritty/src/renderer/text/glyph_cache.rs | `16ed3af099ed00b18771f3419c6be434becce8b63eb39e8f48c68a9093f87f70` | 10548 | `alacritty-renderer-text-glyph-cache` | same URL: already pinned |
| https://codeberg.org/dnkl/foot/raw/commit/cb2771788998f6a6288e2de8273c55a8214551e0/render.c | `97280b358e859d2c4f40f7ddf0b55311d7d66f9a5d9fe6e2c221611c6e0187cb` | 187364 | `foot-render` | same bytes at a different commit (manifest names 2705e36) |
| https://codeberg.org/dnkl/foot/raw/commit/cb2771788998f6a6288e2de8273c55a8214551e0/terminal.c | `27b3dfbf4cf953884632b1b798c876922fa2f11aaac68213ef1a42d4fb419544` | 148115 | `foot-terminal-cb27717` | same URL: already pinned |
| https://raw.githubusercontent.com/tmux/tmux/209d389a81ae565b82e9da745ef17e72e543d477/input.c | `38e4be06a43ab7cb7992307c253785b015c8b00bd4ab694bea28a1429a33c618` | 89384 | `tmux-input-c` | same bytes at a different commit (manifest names 94796f6) |
| https://raw.githubusercontent.com/harfbuzz/harfbuzz/4dd94495551175867004f879a132ac0d0ea873e2/src/hb-ot-shape.cc | `c50cabe89c03daf95c3b9e9b0994d92e44aa0dc3b1b57d30413c90a0a8429ef2` | 39900 | `harfbuzz-hb-ot-shape` | same bytes at a different commit (manifest names 64e2b53) |

### 9.2 Specifications, standards work and reference pages (new)

| url | sha256 | bytes | note |
|---|---|---|---|
| http://blog.unicode.org/2025/07/ | `ea35939991e577a77f1baf4036543f99015d44a21a524bb2e41b12e49854f551` | 199178 |  |
| https://ecma-international.org/wp-content/uploads/ECMA_TR-53_2nd_edition_june_1992.pdf | `912dfcdb813716bfa8ca5f2886896cbd4a3c1e4fc31ac3b21dae1b064f920e99` | 96531 |  |
| https://gitlab.freedesktop.org/terminal-wg/bidi/uploads/302700b010961296cbf94137bf3d6315/bidi.txt.arab | `e44dea892837dcfaeb3a259b11f754ee2974d3fde1ad3ccac7789facaca01b14` | 12760 |  |
| https://harfbuzz.github.io/harfbuzz-hb-buffer.html | `3a28c19bc14bf7a7c0c456b923f1d4b08a4707f1d258150c284a662bcc8f6cd6` | 243186 |  |
| https://invisible-island.net/archives/xterm/xterm-411.tgz | `969be283670deadd66934865c4de6c5ab045e3a3facc2b228decf91a20d8c36c` | 1633400 |  |
| https://invisible-island.net/xterm/xterm.faq.html | `1f40b887fc671ca431bc50886800d57a894cede49eed744347a75d592062d749` | 495945 |  |
| https://learn.microsoft.com/en-us/typography/opentype/spec/scripttags | `46b6bd7ade1401475602ba269ec873460c90bceffe664094a5a6b04e29deedbd` | 60789 |  |
| https://learn.microsoft.com/en-us/typography/script-development/arabic | `9892d13decbb75090bd5dc53b523238a83992ce6aa8a4b39d90121675276d216` | 85598 |  |
| https://learn.microsoft.com/en-us/typography/script-development/hebrew | `e461972f02dae9901d873d0ae009e343e5837ba49f96914e2b8d6cc16694ded3` | 71662 |  |
| https://lists.isocpp.org/sg16/2023/12/4015.php | `7b94821fbdd6471bbfbeed3aa9d1069345c038f74672c92997ea9b0cc44aba35` | 5890 | URL inferred from the list path pattern |
| https://terminal-wg.pages.freedesktop.org/bidi/about.html | `87790180c434bcc98f6417b0da400b29fbc92d955a4a05fc5358f86dca0ffa38` | 19799 |  |
| https://terminal-wg.pages.freedesktop.org/bidi/bidi-intro/ | `79292d79784222392a3b87b48f26f14371fd5acb93dfcf373e9750178a659599` | 20382 |  |
| https://terminal-wg.pages.freedesktop.org/bidi/bidi-intro/rtl-bidi-text.html | `4de00ab64e591d5df8f3d3eae37c7767251cf0b45b77ab5dee6016a0d8d5e1a3` | 34101 |  |
| https://terminal-wg.pages.freedesktop.org/bidi/bidi-intro/rtl-ui.html | `cb95661af63ab17570895c46aae8d12dca0a16027c5772485bc95d01b73010af` | 20793 |  |
| https://terminal-wg.pages.freedesktop.org/bidi/bidi-intro/think-rtl.html | `59fcac299167845a4774fc322d627ed104d747ce0b29ea33ba891230a7c6bd0e` | 21151 |  |
| https://terminal-wg.pages.freedesktop.org/bidi/bidi-intro/why-terminals-are-special.html | `175073acaaaff3286094e2916f92866f7df8a5c98e1e81c7a5e12fb00af41459` | 24060 |  |
| https://terminal-wg.pages.freedesktop.org/bidi/ecma-tr53-review.html | `a880791ad999be5b77537b8a779b2fb7e6635ebe51337bbf3fbd224adeb145c4` | 29594 |  |
| https://terminal-wg.pages.freedesktop.org/bidi/future-improvement-ideas/ | `f90fb0b9a01bce6ae345444239e0f105c82b18a2f85e758005212864d99639b2` | 20902 |  |
| https://terminal-wg.pages.freedesktop.org/bidi/future-improvement-ideas/cursor-strategies.html | `72ace13e46b8c86a4b17806222408123b44d7ad87662d624da4f042d513ef594` | 22445 |  |
| https://terminal-wg.pages.freedesktop.org/bidi/future-improvement-ideas/ecosystem.html | `10651c2b286d88f5496b708d8b15eb9707a17e5b8ad786ce03fcddf08602fbf8` | 24318 |  |
| https://terminal-wg.pages.freedesktop.org/bidi/future-improvement-ideas/explicit-level2.html | `ff973ef65f91a716eac209196a7c48f957190b3669d98eb3935fe36a70d6b0ab` | 25299 |  |
| https://terminal-wg.pages.freedesktop.org/bidi/future-improvement-ideas/feature-reporting.html | `ad6f190ac408a313f494667382be697d3a3dd66cde76433c5633c2cda2f8f139` | 19463 |  |
| https://terminal-wg.pages.freedesktop.org/bidi/future-improvement-ideas/implicit-level2.html | `7c70f952ef965b7a68713b27becee73fc6a34a7d040380da183c3145b0931304` | 26764 |  |
| https://terminal-wg.pages.freedesktop.org/bidi/future-improvement-ideas/paragraph.html | `bc8c81044d75f74b42550f3ed90b327bd4012f1992e48ba4eb0741d5c86d30cc` | 19955 |  |
| https://terminal-wg.pages.freedesktop.org/bidi/future-improvement-ideas/writing-directions.html | `e2b5a9c7a3e38b857553ca4406e160e7b88774bfc5bf6e02d6e1bbeef7f83409` | 19988 |  |
| https://terminal-wg.pages.freedesktop.org/bidi/implementations.html | `92593d78ba89c222e599fee6fce23971f18920c5ac194a66a206f2a351e3b144` | 20628 |  |
| https://terminal-wg.pages.freedesktop.org/bidi/prior-work/ | `daf23d58f98660ac5868b0ec30706d7a0ced3e9bc37dc9ba231fa62017845973` | 20124 |  |
| https://terminal-wg.pages.freedesktop.org/bidi/prior-work/applications.html | `962ce91e4d52cd0aa5ff7c4aa0ac066ca61dad7c276ed83d7951fa7d49487e27` | 20841 |  |
| https://terminal-wg.pages.freedesktop.org/bidi/prior-work/docs.html | `4e620b065be4f65d9fe3a4ab3ca114f37e9f5de489389cd2d8567085acc1c115` | 20062 |  |
| https://terminal-wg.pages.freedesktop.org/bidi/prior-work/terminals.html | `6ea8f4917c2ab2a10e14e8f8bdb866a1996ce356eb2142402a560ff97d916321` | 25407 |  |
| https://terminal-wg.pages.freedesktop.org/bidi/rationale-and-dropped-ideas/ | `84f80767c57450c4963279da7b545c3f528c422e138065ce0f8fb31f93362e01` | 20329 |  |
| https://terminal-wg.pages.freedesktop.org/bidi/rationale-and-dropped-ideas/emulated-explicit-mode.html | `9d745d1b1e6bd5df21b7a107e402becf5aa5a59ae41b2798239cedac2e7ffd29` | 20020 |  |
| https://terminal-wg.pages.freedesktop.org/bidi/rationale-and-dropped-ideas/how-the-escape-sequences-apply.html | `c6e6be8ecfbbe6d2a622e3514b5456d671347c83df515a4ab80d1ad2aabe8ca7` | 23056 |  |
| https://terminal-wg.pages.freedesktop.org/bidi/rationale-and-dropped-ideas/more-arrow-key-swapping-modes.html | `bffa0d207b0e1d9695282ffa090da4ef359e2b24343298f56507804004cb0e18` | 20500 |  |
| https://terminal-wg.pages.freedesktop.org/bidi/recommendation/ | `3c34ef381478739d65f0b9bd468eb3053375f6dee4f2e854fe4bb37c7bf05a53` | 21017 |  |
| https://terminal-wg.pages.freedesktop.org/bidi/recommendation/arrow-key-swapping.html | `69d3abc624ce844ab723fbbb7c071b4d0f2c5ae3bff8ce9b83f3fe0140ea8ffc` | 23070 |  |
| https://terminal-wg.pages.freedesktop.org/bidi/recommendation/basic-modes.html | `e3beb7e7f805db77974c08d354f54e2439830163ca3ea551733bf5b7d80d241b` | 29303 |  |
| https://terminal-wg.pages.freedesktop.org/bidi/recommendation/combining.html | `1102cde45dd0b98e0a5435239c2c86b451cf57a97a63e1a395227ee4c83b3002` | 25808 |  |
| https://terminal-wg.pages.freedesktop.org/bidi/recommendation/cursor.html | `ac87b3aebb3f706f39dd311cd15df8f713e1a7e074253fe037d6c6b877703db1` | 23024 |  |
| https://terminal-wg.pages.freedesktop.org/bidi/recommendation/display-only.html | `3893c9baaef99b31197f1fb61b88daee16c506a4a5ff7ab6cc08734e213117ac` | 23775 |  |
| https://terminal-wg.pages.freedesktop.org/bidi/recommendation/erased-cells.html | `85b14c0adb4cfaea8b058aa86df4ead75aa62a12e474a41fc0bf40594954e467` | 21409 |  |
| https://terminal-wg.pages.freedesktop.org/bidi/recommendation/escape-sequences.html | `7a63f9bcb72e0f418ac9650750491c884c0d8b4e98d4546f35c99f1bc010418d` | 37243 |  |
| https://terminal-wg.pages.freedesktop.org/bidi/recommendation/misc.html | `27de786f5607b4c678bdee7972008f9803ea8fe4d394134ccc068606d4e89736` | 31113 |  |
| https://terminal-wg.pages.freedesktop.org/bidi/recommendation/paragraphs.html | `f780d8d92d2e3821b6cfe316ecd1e107d8308e13e7fcc8b968aded710cfca8fb` | 20233 |  |
| https://unicode.org/mail-arch/unicode-ml/Archives-Old/UML016/0189.html | `bca9eb35c0920dea6e912eba3de403e851c9a617cd8fc7af26a2e38292eb43c0` | 8974 |  |
| https://vt100.net/docs/vt510-rm/DECHEBM.html | `9f97bc416ffedec7a7379efe8d2739606a2c97f7ad1763ca45bf0e26352885f6` | 20694 |  |
| https://vt100.net/docs/vt510-rm/DECHEM.html | `ec7b0897bd7059d03d5773a97df9ef1de4b8dc20faa8cec01374cdc3e365565e` | 19780 |  |
| https://vt100.net/docs/vt510-rm/DECRLCM.html | `0765bb1b2df0a2c9eefd713b89a0951c66406e11b5bc6d51c1c2179a4b5990e2` | 20014 |  |
| https://vt100.net/docs/vt510-rm/DECRLM.html | `7405344c8262e7a76e82ab850369f3c6b769890e8f7cdb677c503f1dd04ec998` | 22324 |  |
| https://vt100.net/docs/vt510-rm/chapter2.html | `377424ba9caffbe64ea8cf4ac37f798554732cc38ec08ed56c60cc28c93c7580` | 111478 |  |
| https://www.unicode.org/L2/L2023/23107-terminal-suppt.pdf | `c6d6bbcceef601fd011a98f10b4950f2fb8153d9b997d2d5b8011be70c844ee6` | 1291094 |  |
| https://www.unicode.org/L2/L2023/23153-opposition.pdf | `771681d630e0a60a49ab55d0d5142c4b469b0d7e54421d8178391922e31ad5c2` | 433707 | fetched, not read |
| https://www.unicode.org/L2/L2023/23194-text-terminal-wg-report.pdf | `276d8917afe9b4486eccf0bc9c063528b921d4e9f35f612de6e9823487da72fb` | 67033 |  |
| https://www.unicode.org/Public/17.0.0/ucd/ArabicShaping.txt | `39afa01e680e27d0fd10b67a9b27be13fbaa3d0efecfb5be45991de9a0d267d0` | 41441 |  |
| https://www.unicode.org/Public/17.0.0/ucd/BidiBrackets.txt | `dadbaf38a0d0246e5b805bf8725cb81b7c621f93d030595635f5ba2c2f179428` | 8891 |  |
| https://www.unicode.org/Public/17.0.0/ucd/BidiCharacterTest.txt | `a3e6e905ab5afbe318a96df5401d0372a04cd73ef139ab5e3cf0ae241c255488` | 6880771 |  |
| https://www.unicode.org/Public/17.0.0/ucd/BidiMirroring.txt | `a2f16fb873ab4fcdf3221cb1a8a85a134ddd6ed03603181823ff5206af3741ce` | 26827 |  |
| https://www.unicode.org/Public/17.0.0/ucd/BidiTest.txt | `888bdfc8090652272d1f859cdb00ae659e2dc6c26740be61ef1d03998a687620` | 7959988 |  |
| https://www.unicode.org/Public/17.0.0/ucd/PropertyValueAliases.txt | `64e9a5f76f7a1e8b5a47d6a1f9a26522a251208f5276bdfa1559dac7cf2e827a` | 81858 |  |
| https://www.unicode.org/Public/17.0.0/ucd/ScriptExtensions.txt | `ec2107e58825a1586acee8e0911ce18260394ac8b87e535ca325f1ccbeb06bc6` | 20707 |  |
| https://www.unicode.org/Public/17.0.0/ucd/Scripts.txt | `9f5e50d3abaee7d6ce09480f325c706f485ae3240912527e651954d2d6b035bf` | 192460 |  |
| https://www.unicode.org/Public/17.0.0/ucd/extracted/DerivedBidiClass.txt | `4867b4b7f0731ed1bfcd34cc6251211ff1542541fce0734b6fbda139ee80b3a4` | 173433 |  |
| https://www.unicode.org/Public/17.0.0/ucd/extracted/DerivedJoiningType.txt | `f39ebe974825d6736aee15582250307aa532b2cfab3caf3f86bd23fddc9c5c4d` | 40635 |  |
| https://www.unicode.org/Public/18.0.0/ucd/BidiBrackets.txt | `4b3b62e4a14b84ee752808c810c602534921c09a4a1bf78cfbee566d66c125b3` | 8992 |  |
| https://www.unicode.org/Public/18.0.0/ucd/BidiCharacterTest.txt | `045b24d2c8ab066951bd32fe8c6b4de34647f72b5b1c7df0265f24ab53573e01` | 6880771 |  |
| https://www.unicode.org/Public/18.0.0/ucd/BidiMirroring.txt | `cd54810ebf52f0e61a730c8b9cb25975de6c85f6d788a559b416afd548923fd6` | 27294 |  |
| https://www.unicode.org/Public/18.0.0/ucd/BidiTest.txt | `9af2f882a4ab50912e388f069a673b94eacd82fa6d07d20a3ff7f3c759e905aa` | 7959988 |  |
| https://www.unicode.org/Public/18.0.0/ucd/UnicodeData.txt | `0736451de439ae7baf1425136617da495e09ee5afbe6e394374db7009ea08950` | 2243593 |  |
| https://www.unicode.org/Public/18.0.0/ucd/extracted/DerivedBidiClass.txt | `d9e23222522551348ea1ccfbb4f62efbf98982afb95840f8959c08ed992c5607` | 176412 |  |
| https://www.unicode.org/mail-arch/unicode-ml/y2019-m01/0335.html | `a04f6fabbc41159aa619df54497364525dfad3da67f2e2ba5d92dcf7619bf0a9` | 8150 |  |
| https://www.unicode.org/mail-arch/unicode-ml/y2019-m02/0052.html | `9c3898a810f4449a64885078cdb438f09f15031914b427a39e17ba492eaa9a72` | 8867 |  |
| https://www.unicode.org/reports/tr55/tr55-5.html | `e99fa8594286dff7eb0808c082c457730962708ac4a981c76ee4335ccdebd5f6` | 187158 |  |
| https://www.unicode.org/reports/tr9/tr9-52.html | `3c281b38800bad85b16bf3a169b8cb693d8a2dbceb60903feb26e68a2659ee8a` | 188933 |  |

### 9.3 Text engines and platform documentation (new)

| url | sha256 | bytes | note |
|---|---|---|---|
| file:///Library/Developer/CommandLineTools/SDKs/MacOSX27.0.sdk/System/Library/Frameworks/AppKit.framework/Headers/NSParagraphStyle.h | `34449554f39405761b8b3624ca9a1ec78a881f86fbc3d550733518ea1293849a` | 30415 | local copy of a file on this Mac |
| file:///Library/Developer/CommandLineTools/SDKs/MacOSX27.0.sdk/System/Library/Frameworks/AppKit.framework/Headers/NSText.h | `a0f9b246d07bff5799f4dd7da81c2d385b0aca80d382cb1fe54f78a8388bf223` | 10571 | local copy of a file on this Mac |
| file:///Library/Developer/CommandLineTools/SDKs/MacOSX27.0.sdk/System/Library/Frameworks/AppKit.framework/Headers/NSTextSelection.h | `e583277305d2f54b996fd5ab06f9bd4c9e01e9d448f7bc2dfa2b771671491c59` | 6133 | local copy of a file on this Mac |
| file:///Library/Developer/CommandLineTools/SDKs/MacOSX27.0.sdk/System/Library/Frameworks/AppKit.framework/Headers/NSTextSelectionNavigation.h | `0594c27736ade16ccdd5a875c82e5179167b63aae7f0521b1543ca2de0baee4e` | 20600 | local copy of a file on this Mac |
| file:///Library/Developer/CommandLineTools/SDKs/MacOSX27.0.sdk/System/Library/Frameworks/CoreText.framework/Headers/CTFont.h | `e9c47f154ed3f6267556f04bde22c5a3f6f08748b8c87f7c6ff0f3f8165c4bb7` | 83565 | local copy of a file on this Mac |
| file:///Library/Developer/CommandLineTools/SDKs/MacOSX27.0.sdk/System/Library/Frameworks/CoreText.framework/Headers/CTFontDescriptor.h | `e0b1a00342376ed5679f1d74a13802091299536b7c3bf5ee75b070e50a1e7328` | 34849 | local copy of a file on this Mac |
| file:///Library/Developer/CommandLineTools/SDKs/MacOSX27.0.sdk/System/Library/Frameworks/CoreText.framework/Headers/CTLine.h | `416e11cafbf0437b982afe34897294de64d365cdd447cb9f3b69cce589030e83` | 19508 | local copy of a file on this Mac |
| file:///Library/Developer/CommandLineTools/SDKs/MacOSX27.0.sdk/System/Library/Frameworks/CoreText.framework/Headers/CTParagraphStyle.h | `6ae0439fea31b2f5db7ec629b867f8b38a3af89638d8eca3e72b73b256175e30` | 18688 | local copy of a file on this Mac |
| file:///Library/Developer/CommandLineTools/SDKs/MacOSX27.0.sdk/System/Library/Frameworks/CoreText.framework/Headers/CTRun.h | `4fa63db0292f183e83840c316845d973816117b8a076b06d24781c15af25f91f` | 20272 | local copy of a file on this Mac |
| file:///Library/Developer/CommandLineTools/SDKs/MacOSX27.0.sdk/System/Library/Frameworks/CoreText.framework/Headers/CTStringAttributes.h | `72b6b288840c1b964eaecfbdce6a8758c14d0159b3b5361d7dc18ae1c5a5652c` | 20526 | local copy of a file on this Mac |
| file:///Library/Developer/CommandLineTools/SDKs/MacOSX27.0.sdk/System/Library/Frameworks/CoreText.framework/Headers/CTTypesetter.h | `eb20e3035f3b5e9ada9265b020d4d0918c1e488e8cb90f6fb7e2e04f781523f0` | 10343 | local copy of a file on this Mac |
| file:///System/Library/Frameworks/AppKit.framework/Resources/StandardKeyBinding.dict | `500c1d3a0afb3f9bcca83a9ec20f128b5b802c699bd336634d20bebfecad0390` | 4551 | local copy of a file on this Mac |
| https://developer.apple.com/tutorials/data/design/human-interface-guidelines/right-to-left.json | `0c47d2b7b7c45cf65e4de155e84433f19980ab1ae8d12f09d7df213483e0f539` | 62460 |  |
| https://developer.apple.com/tutorials/data/documentation/coretext/ctlinegetoffsetforstringindex(_:_:_:).json | `1262faea5273ed366c3f02994ca06bd0be5a611f7ecb77ebc3d1a4a53bd89746` | 20376 |  |
| https://gitlab.gnome.org/GNOME/pango/-/raw/8e74c27c113c98664b77272d16b14bbce542c184/pango/itemize.c | `6785e8899b13acb1a11d19906ce08239a3273f813fba0ef55f6562eb5957ee72` | 53351 |  |
| https://gitlab.gnome.org/GNOME/pango/-/raw/8e74c27c113c98664b77272d16b14bbce542c184/pango/pango-layout.c | `bfdc9ffc57ebea066ce2675df39b0ee92b2f290e98fdc1155f58b9bfba705091` | 237154 |  |
| https://gitlab.gnome.org/GNOME/pango/-/raw/8e74c27c113c98664b77272d16b14bbce542c184/pango/pangofc-fontmap.c | `2e216c0afa76f975669972d8cde0b8174a97968786937bca1fad6e8541ee4f96` | 106338 |  |
| https://gitlab.gnome.org/GNOME/pango/-/raw/8e74c27c113c98664b77272d16b14bbce542c184/utils/test-hebrew.txt | `39e80e5ee3f5355eda25ccefec3ca0095e5da8f88780621abdc0617988d0b664` | 2693 |  |
| https://raw.githubusercontent.com/MicrosoftDocs/VBA-Docs/4a944c587afa362c5bc2490211f50736181cfc62/api/Word.Options.AddBiDirectionalMarksWhenSavingTextFile.md | `66dff50abe6a4605304356e1a223fedd8d5bce7b9ebe9693db805eeb0c26fcf5` | 1121 |  |
| https://raw.githubusercontent.com/MicrosoftDocs/VBA-Docs/4a944c587afa362c5bc2490211f50736181cfc62/api/Word.Options.VisualSelection.md | `058b5691dda19ae1c21372de9d2ef21c1ab5f8e93947ef3c1ce74bd13936f938` | 1047 |  |
| https://raw.githubusercontent.com/MicrosoftDocs/VBA-Docs/4a944c587afa362c5bc2490211f50736181cfc62/api/Word.WdArabicNumeral.md | `e3007fafc9a1a253a7cae3bf2f0cb19f368be814523a21c30e5a0de29e192fec` | 668 |  |
| https://raw.githubusercontent.com/MicrosoftDocs/VBA-Docs/4a944c587afa362c5bc2490211f50736181cfc62/api/Word.WdCursorMovement.md | `bb45d5a895ebe850301b21c78678800d89fddcb18af14d188a4166ea869ff165` | 646 |  |
| https://raw.githubusercontent.com/MicrosoftDocs/VBA-Docs/4a944c587afa362c5bc2490211f50736181cfc62/api/Word.WdVisualSelection.md | `f66fa44093a38cabcba2f249867e2f8722b74dc0a538cf820b808145f9efb4df` | 600 |  |
| https://raw.githubusercontent.com/MicrosoftDocs/globalization/a0735c714b71837cf2330725da7fbbadfcb3d198/globalization/fonts-layout/text-directionality.md | `0db582da00242c60fff16c85277cd053ebbb1a6a7b31a425f626f4fb62f25785` | 9760 |  |
| https://raw.githubusercontent.com/MicrosoftDocs/sdk-api/c12073e417d5780fe796278ada21b90cef1b0568/sdk-api-src/content/dwrite/ne-dwrite-dwrite_reading_direction.md | `eeabec3cd040acf432bf6b6efa91ddc0def41e8c9b36aaa0c163ed324c1657f4` | 3082 |  |
| https://raw.githubusercontent.com/MicrosoftDocs/sdk-api/c12073e417d5780fe796278ada21b90cef1b0568/sdk-api-src/content/dwrite/nf-dwrite-idwritetextanalysissink-setbidilevel.md | `830bde79258f0e0216078eba333b7760a2e8d6b7fc2b6abff84031c7a53c9e43` | 3024 |  |
| https://raw.githubusercontent.com/MicrosoftDocs/sdk-api/c12073e417d5780fe796278ada21b90cef1b0568/sdk-api-src/content/dwrite/nf-dwrite-idwritetextanalysissource-getparagraphreadingdirection.md | `bac8b7262cb94468ed69a7a8b0b247ae7417b572abc983807a6c953b2f27af2d` | 2760 |  |
| https://raw.githubusercontent.com/MicrosoftDocs/sdk-api/c12073e417d5780fe796278ada21b90cef1b0568/sdk-api-src/content/dwrite/nf-dwrite-idwritetextanalyzer-analyzebidi.md | `b5cc770dafa1e6dbc1b8b55dfb8de9d7ffac39d931b57b923fbf2d746249dfd3` | 3354 |  |
| https://raw.githubusercontent.com/MicrosoftDocs/sdk-api/c12073e417d5780fe796278ada21b90cef1b0568/sdk-api-src/content/dwrite/nf-dwrite-idwritetextlayout-hittesttextposition.md | `a0ddcb247b4e65783bc16dba9b9be7172207ea4afeb98d8fa128bb7fb60e8398` | 3773 |  |
| https://raw.githubusercontent.com/MicrosoftDocs/sdk-api/c12073e417d5780fe796278ada21b90cef1b0568/sdk-api-src/content/dwrite/ns-dwrite-dwrite_cluster_metrics.md | `c0fddb76390137183998b8ddc895623bfb31bc221ec267e3ca4bee265a705bf4` | 2338 |  |
| https://raw.githubusercontent.com/MicrosoftDocs/sdk-api/c12073e417d5780fe796278ada21b90cef1b0568/sdk-api-src/content/dwrite_2/nf-dwrite_2-idwritefactory2-getsystemfontfallback.md | `b6fcc948b3582cdd864f8cbafa6891b464eb5206bb665b01373f9d5f2a5d10c1` | 2408 |  |
| https://raw.githubusercontent.com/MicrosoftDocs/sdk-api/c12073e417d5780fe796278ada21b90cef1b0568/sdk-api-src/content/dwrite_2/nf-dwrite_2-idwritefontfallback-mapcharacters.md | `f97886f9a3a2140908bcfcbd1c850d6eb608f1baba642b2b84fb6bd7f835b0e5` | 4117 |  |
| https://raw.githubusercontent.com/MicrosoftDocs/sdk-api/c12073e417d5780fe796278ada21b90cef1b0568/sdk-api-src/content/usp10/nf-usp10-scriptlayout.md | `4508f361eaf62d89cb544cb27a091142692b0ee4504c4b5dba2f311eb52ebb99` | 5121 |  |
| https://raw.githubusercontent.com/MicrosoftDocs/sdk-api/c12073e417d5780fe796278ada21b90cef1b0568/sdk-api-src/content/usp10/ns-usp10-script_analysis.md | `115037ddd3c0f0fb0773e4efab730bc0426b932ef758311a442872f8b32b12d4` | 10190 |  |
| https://raw.githubusercontent.com/MicrosoftDocs/sdk-api/c12073e417d5780fe796278ada21b90cef1b0568/sdk-api-src/content/usp10/ns-usp10-script_state.md | `40af4bf5ce18f198a5f943128212b510689fc7113923053a913f134dbeaffc41` | 8795 |  |
| https://raw.githubusercontent.com/MicrosoftDocs/win32/e103fa4e8810bd8d42c4777e17081e24dbe62dbd/desktop-src/Controls/about-rich-edit-controls.md | `cbc1432900fae492d39bc4180a2b0dae1d9b5750dba472c41cf4ccc304de1e3d` | 64125 |  |
| https://raw.githubusercontent.com/MicrosoftDocs/win32/e103fa4e8810bd8d42c4777e17081e24dbe62dbd/desktop-src/Controls/edit-controls-text-operations.md | `6f5ce1e346cc720cc764c28c84109b026dd03a39d17386b91938eb1197f5e9b2` | 21729 |  |
| https://raw.githubusercontent.com/MicrosoftDocs/win32/e103fa4e8810bd8d42c4777e17081e24dbe62dbd/desktop-src/Intl/digit-shapes.md | `c886fd6988b410a66406347e8ed1963c4a1fad6fb1e4f980a8072977410ce62d` | 4976 |  |
| https://raw.githubusercontent.com/MicrosoftDocs/win32/e103fa4e8810bd8d42c4777e17081e24dbe62dbd/desktop-src/Intl/displaying-text-with-uniscribe.md | `016b9c22e8cebc40ab2ae62703c6ae92617a67f1d8c7f54a65b78ae9bb3e3ac5` | 11720 |  |
| https://raw.githubusercontent.com/MicrosoftDocs/win32/e103fa4e8810bd8d42c4777e17081e24dbe62dbd/desktop-src/Intl/displaying-the-caret-in-bidirectional-strings.md | `6748a232e72e17fdf292b447c45b060077afd125153e752f4d72260ce51f74ea` | 2612 |  |
| https://raw.githubusercontent.com/MicrosoftDocs/win32/e103fa4e8810bd8d42c4777e17081e24dbe62dbd/desktop-src/Intl/locale-idigitsubstitution.md | `bd85abdde0db23a68f7a084042eef377a7474677b08b6996081f28a6ac5fcf9a` | 2087 |  |
| https://raw.githubusercontent.com/MicrosoftDocs/win32/e103fa4e8810bd8d42c4777e17081e24dbe62dbd/desktop-src/Intl/managing-caret-placement-and-hit-testing.md | `4cafceb3ef2ab5081eb3a36151449854d50263ae8493c680cd5f7497ddf84100` | 5359 |  |
| https://raw.githubusercontent.com/MicrosoftDocs/winrt-api/8448d5eecfbc2ed903f659f350841dcb4888bc8b/windows.ui.xaml/textreadingorder.md | `aafbcb9acdbb756974cfab0554f6f27a9b818fb6c2602b8d2df1e17121cf4072` | 3181 |  |
| https://raw.githubusercontent.com/fribidi/fribidi/24f15eee832eafa5d63319b666d50e488668ce31/lib/fribidi-arabic.c | `9ef6ac9e1df638b7ebe73c9ae31347937bf44f41315f7b999153f7f1b118d400` | 5363 |  |
| https://raw.githubusercontent.com/fribidi/fribidi/24f15eee832eafa5d63319b666d50e488668ce31/lib/fribidi-arabic.h | `e585515fc7eb12ff40f5de7a8fd51e5d08fda60842ceeb90f23b67f59074b2a6` | 2752 |  |
| https://raw.githubusercontent.com/fribidi/fribidi/24f15eee832eafa5d63319b666d50e488668ce31/lib/fribidi-bidi.h | `819ec96d0a479a8516260b1a3b3993a244214ad6c0586d304249a691e1b5b274` | 6253 |  |
| https://raw.githubusercontent.com/fribidi/fribidi/24f15eee832eafa5d63319b666d50e488668ce31/lib/fribidi-flags.h | `819bf2e462e20a3f79a9f959be173550aa609f12eeecbf62e87109b8034dfb20` | 2038 |  |
| https://raw.githubusercontent.com/fribidi/fribidi/24f15eee832eafa5d63319b666d50e488668ce31/lib/fribidi-unicode.h | `21ee4bf3f7a6dd19e5b35566cdf10a2cdf22b52d01aa3fe467d2da8f11cec2fe` | 3352 |  |
| https://raw.githubusercontent.com/fribidi/fribidi/24f15eee832eafa5d63319b666d50e488668ce31/lib/fribidi.h | `f6773db72a15f8b1ba0aecc489cf92151219d0b691e4a2be91a3eaccdc9e7709` | 4829 |  |
| https://raw.githubusercontent.com/harfbuzz/harfbuzz/4dd94495551175867004f879a132ac0d0ea873e2/src/hb-buffer.cc | `58bd6aa7a3e93d21fe02041c53d9de7dea9e9cbc6d1826583ebff42952539245` | 61952 |  |
| https://raw.githubusercontent.com/harfbuzz/harfbuzz/4dd94495551175867004f879a132ac0d0ea873e2/src/hb-ot-layout.cc | `40630ee65cfefcb871767d9f2b1c5a080940e4ca8a50cd6ee3df5efd3de7047f` | 94876 |  |
| https://raw.githubusercontent.com/harfbuzz/harfbuzz/4dd94495551175867004f879a132ac0d0ea873e2/src/hb-ot-map.cc | `17c8b65055ea954427883e2e11d5aa1316b971b1ce2b72434be31d544ccc9b8f` | 13280 |  |
| https://raw.githubusercontent.com/harfbuzz/harfbuzz/4dd94495551175867004f879a132ac0d0ea873e2/src/hb-ot-map.hh | `725b9a504b772bcef2fe2d26608802bcb1ea7a50f73895ffa748976e30577bad` | 9790 |  |
| https://raw.githubusercontent.com/harfbuzz/harfbuzz/4dd94495551175867004f879a132ac0d0ea873e2/src/hb-ot-shaper-arabic.cc | `f58d84e3715daa1c1704d70be61e8a9464487f453f414c84a0e4530609eb0d72` | 25779 |  |
| https://raw.githubusercontent.com/harfbuzz/harfbuzz/4dd94495551175867004f879a132ac0d0ea873e2/src/hb-ot-shaper-hebrew.cc | `4dbb5128ec416028ae347ddac1047a13f5e43cc69df5c3101554577667862a01` | 5816 |  |
| https://raw.githubusercontent.com/harfbuzz/harfbuzz/4dd94495551175867004f879a132ac0d0ea873e2/src/hb-ot-shaper.hh | `3eb84b0a89a8c4dbee0930e66bd071bb374d76e9f2748ac441d84e79dc34cce3` | 12102 |  |
| https://raw.githubusercontent.com/unicode-org/icu/4e689969f24a31e32efcd590cabd944ca3550352/icu4c/source/common/unicode/ubidi.h | `ea8bf13458735dee8695e7099383b02a6ccd6382296c5bc7ef4b5f96ca722517` | 97307 |  |
| https://raw.githubusercontent.com/unicode-org/icu/4e689969f24a31e32efcd590cabd944ca3550352/icu4c/source/common/unicode/ushape.h | `f81bb11d208a5dd0f44925ad0b2220e8bc0ad5ccd03285e9d59e0d87433c409c` | 18430 |  |

### 9.4 Terminal and application source (new; summary)

124 files. Each is the raw file at the named commit; the rows give the project, the commit and how many files were saved from it.

| project | commit | files |
|---|---|---|
| dnkl/fcft | 1ff7e79 | 2 |
| dnkl/foot | cb27717 | 2 |
| readline | f95e814 | 2 |
| screen | 9d8b0ff | 1 |
| GNOME/gnome-terminal | b23813a | 2 |
| GNOME/vte | 56bad78 | 15 |
| utilities/konsole | c8bc143 | 8 |
| MidnightCommander/mc | 23a260a | 1 |
| alacritty/alacritty | d692748 | 1 |
| arakiken/mlterm | 405185a | 9 |
| behdad/bicon | 48720c0 | 2 |
| contour-terminal/contour | d8ce17b | 8 |
| emacs-mirror/emacs | 9bc5661 | 5 |
| ghostty-org/ghostty | befcdfd | 1 |
| gnachman/iTerm2 | 91411f5 | 6 |
| gwsw/less | ddfc834 | 4 |
| kovidgoyal/kitty | 6d5d0c4 | 1 |
| kovidgoyal/kitty | 7d37453 | 3 |
| microsoft/terminal | 35f29bd | 4 |
| microsoft/terminal | 78de114 | 1 |
| mintty/mintty | 0aea388 | 10 |
| neovim/neovim | 898e6f8 | 4 |
| tmux/tmux | 209d389 | 5 |
| vim/vim | 5979d7d | 10 |
| wezterm/wezterm | cab2516 | 8 |
| xtermjs/xterm.js | c58ea36 | 1 |
| zsh-users/zsh | 8cc5ead | 3 |

Pages that are not files in a repository, in full:

| url | sha256 | bytes |
|---|---|---|
| https://ghostty.org/docs/features | `d95f6abca36ec67fed462248232de15b49eff98dfdace19ffcbe7b2cc29332b5` | 94460 |
| https://github.com/gnachman/iTerm2/commit/91411f53619af602bd434ab355e674c1faa831e3 | `84db06aaae80f4c16cd17fd217f3150624330b48deb586252f56f2be3afe0bca` | 1350 |
| https://support.apple.com/guide/terminal/trmladvn/mac | `7e8452404921ab490783ed087445de38e12b26081df2ec909ef210e0bb76ac42` | 575471 |
| https://support.apple.com/guide/terminal/trmltxt/mac | `7e54b00e14e73540d0ace90e9bbdea972597a43c32a1b18996bbb149f458505e` | 575731 |
| https://support.apple.com/guide/terminal/use-international-character-encodings-trml15220/2.15/mac/27 | `70060df674085d45471796c9c714af10b1585e1c159076926b3906177377ecff` | 564775 |

### 9.5 Issue-tracker and API snapshots (new; summary; not byte-stable)

76 snapshots. Re-fetches may differ (avatars, reaction counts), so pin the saved bytes or a rendered text, not the URL.

| tracker | snapshots |
|---|---|
| GNOME/vte | 7 |
| MidnightCommander/mc | 2 |
| alacritty/alacritty | 3 |
| arakiken/mlterm | 2 |
| bugs.kde.org | 3 |
| contour-terminal/contour | 1 |
| dnkl/foot | 5 |
| ghostty-org/ghostty | 5 |
| gnachman/iterm2 | 2 |
| kovidgoyal/kitty | 7 |
| microsoft/terminal | 13 |
| mintty/mintty | 5 |
| terminal-wg/bidi | 12 |
| tmux/tmux | 4 |
| wezterm/wezterm | 1 |
| xtermjs/xterm.js | 4 |

## 10. What is still unknown

### Resolved while writing this synthesis

- **Windows Terminal's commit.** Resolved for the files the bidi claims rest on: they are byte-identical in the local extract and at 35f29bd (section 4).
- **The SG16 post the critic saved.** Now read (2.6).
- **The BidiMirroring count.** Corrected (2.4).
- **The Ghostty and tmux/terminfo claims.** Corrected (sections 4 and 5).
- **VTE's conformance level.** Restated from source (2.5).
- **The cluster-store failure (7.2).** Measured in baud: `marks_dropped` reaches 10 by verse 2 and 45 by verse 5 of a hand-typed pointed Genesis 1:1-5. The red test still has to be written; it should pin `marks_dropped > 0` on that sample.
- **Which Windows Terminal releases made AtlasEngine the default and removed DxEngine.** v1.19.10292.0 and v1.21.1272.0, from the release notes (section 4).

### Still open

1. **The UTC Text Terminal Working Group.** L2/23-153 is unread. The group's drafts are private; its public reports through 2025-10 neither adopt nor reject L2/23-107's visual cursor model.
2. **Arabic Prepend characters.** Tested 2026-10-10 (7.1): a Prepend joins the cell before it, printable ASCII after it takes a cell of its own, and any other character after it joins the cell before the Prepend. The standard joins the Prepend to what follows; both are open departures, pinned by line in the test.
3. **Terminal.app's behaviour.** Known only second-hand, and the two reports disagree. It could be captured on this Mac in minutes.
4. **The owner's own workload.** Claude Code and other agent CLIs, and TUI popups, have not been tested under any reordering terminal.
5. **Terminals not read at source.** PuTTY (its current source), cosmic-term/cosmic-text, hterm, Rio, Warp, JediTerm, Termux and QTerminal/qtermwidget. The multiplexers and proxies Zellij and mosh may swallow BDSM/SCP as tmux does; not checked.
6. **Behaviour inferred from source but never run:**
   - WezTerm misordering RTL across colour changes, and its explicit RTL working only per cluster;
   - Konsole's bracket mirroring;
   - VTE passing stored controls to FriBidi;
   - how mintty's ExtTextOutW avoids GDI reordering its already-visual text a second time.
7. **Windows behaviour with no primary source and no machine to test on:**
   - Notepad's current menu items and default caret movement;
   - Word's default caret mode.
8. **Older standards.** The DEC manual's intent for DECRLM is unclear, because the page has two "not affected" lists. TR/53's 1st edition (1990), the one ECMA-48 incorporated, was not read.
9. **Libraries and fonts:**
    - whether libunicode's run_segmenter splits runs at script boundaries;
    - whether any GSUB/GPOS behaviour baud would need depends on buffer direction;
    - monospace Arabic faces other than Courier New;
    - Courier New's licence.
10. **Shell widths.** Which wcwidth table the owner's nix zsh uses, and whether nix's bash/readline was built with WCWIDTH_BROKEN.
11. **Unmapped parts of baud:**
    - the Wayland/CPU presentation path, against the visual-column changes;
    - IME preedit, which is never drawn because `marked_text` is a no-op.
12. **Snapshots and sampling:**
    - The GitLab and GitHub issue JSON will drift; pin the saved bytes or a rendered text instead.
    - The 2019 Unicode-list thread was sampled at only two messages. Wordingham's shaping messages, which drove issue #7, were not read.
    - foot #756's reaction count is unknown.
    - A report that bicon fails on ARM macOS is unconfirmed.
13. **VTE's "v0.2".** It could not be confirmed, because no v0.2 was ever published.

### For the parent

- The owner's font request: done (TODO.md:2557-2562); record Q22's answer as option (c).
- Commit this note before the scratchpad is lost, and pin the sources.
- Render each section 8 question as a visual before asking it.

## 11. Script selection after phase 0: the four questions Q6 left (2026-10-13)

Q6's postscript built phase 0 at 6b53a66 and left four choices the texts do not settle. They were put to Jev twice: round 2026-10-13f asked all four (decisions.jsonl lines 527 and 528, the second a replicate with the options reversed), and round 2026-10-13g asked Q-A and Q-C again with two research lanes' measurements (lines 529 and 530). The rounds' requests and results are kept untracked in `.claude/research/jev-program/round-2026-10-13f/script-select/` and `round-2026-10-13g/script-select/`; decisions.jsonl lives outside the repository, beside `jev.py`. The data, harnesses and patch the decisions rest on are committed in [ab/](ab/), listed in 11.6. Every figure below is taken from those files. This section's baud citations are at bc39889, not the e138e34 of the note's header.

### 11.1 What Jev answered

Confidence, then the probabilities, as decisions.jsonl records them. The rule since 2026-10-10: 0.9 or above both times decides, departures included, and the owner can veto; below 0.9 needs more research or a reframe.

| question | options, as the requests marked them | 527 | 528 | 529 | 530 | result |
|---|---|---|---|---|---|---|
| Q-A: the Script table after `DFLT`, for a face with neither the run's script nor `DFLT` | A1_LATN, built; A2_NONE; A3_DFLT_LOWER; all three marked conforming, because the ScriptList text is silent past `DFLT` | A1 0.80 (0.87 / 0.12 / 0.01) | A1 0.83 (0.89 / 0.08 / 0.03) | A1 0.92 (0.95 / 0.05 / 0.00) | A1 0.92 (0.95 / 0.05 / 0.00) | **decided: A1_LATN, as built** |
| Q-B: Hebrew whose `GPOS` has no `hebr` table | B1_DFLT_ANCHORS, built (conforming); B2_GPOS_OFF, HarfBuzz's (departing where the face has `DFLT`, silent where only `latn` is chosen) | B1 0.98 (0.99 / 0.01) | B1 0.94 (0.97 / 0.03) | | | **decided: B1_DFLT_ANCHORS, as built** |
| Q-C: two Hebrew points of different classes in one cell | C1_CANONICAL, built; C2, a different option in each round: 13f's C2_SBL sorts by HarfBuzz's classes and "then makes HarfBuzz's one Hebrew swap", 13g's C2_HB_CLASSES is the patch, "without HarfBuzz's swap" (the swap fires 0 times in both corpora, 11.4); all marked conforming | C1 0.78 (0.89 / 0.11) | C1 0.50 (0.75 / 0.25) | C1 0.36 (0.68 / 0.32) | C1 0.30 (0.65 / 0.35) | **below 0.9; the owner picked C2, 2026-10-13 (11.4)** |
| Q-D: the order of an Indic or Myanmar script's tags | D1_V2_FIRST, built (conforming); D2_OLD_FIRST (conforming for the Indic pairs, departing for Myanmar "if that reaches lookup"); D3_DEV3, HarfBuzz's unregistered `dev3` (conforming) | D1 0.97 (0.98 / 0.02 / 0.00) | D1 0.95 (0.97 / 0.03 / 0.00) | | | **decided: D1_V2_FIRST, as built** |

So Jev's answers change nothing built; the owner's pick of C2 for Q-C (11.4) did, built 2026-10-14. Q-B keeps `GPOS` on for Hebrew under `DFLT`'s anchors, reversing the `GPOS`-off half of Q6's (a), which earlier askings (decisions.jsonl lines 137, 295 and 296) leaned to at 0.46, 0.78 and 0.8 and never decided. Q-D tries registered tags only, `v.2` first (`dev2` then `deva`, `mym2` then `mymr`).

**Two labels disagree.** The record's phase 0 section lists Q-A's `dflt` option and Q-D's `dev3` option as departing, because neither is a registered script tag; the requests marked both conforming, on the ground that no quoted text limits the tags an application looks up. For `dflt` the texts below favour the record's label (AFDKO: "`DFLT` is a valid value for the `script` tag, and `dflt` is not"). Neither decision turns on it: A3 drew 0.01, 0.03, 0.00 and 0.00, and D3 0.00 both times.

### 11.2 Q-A: what comes after `DFLT`

**The texts.** None of them says what an application does for a face with neither the run's script nor `DFLT`.

- OpenType's ScriptList text has the `DFLT` rule and nothing past it [[source: ms-opentype-chapter2]]; the registry lists `DFLT` and nothing past it [[source: ms-opentype-scripttags]].
- Microsoft's script-tag page as archived in 2005 (it says "last updated 14 October 2002"): "When the ScriptList table is searched for a script, and no entry is found, and there is an entry for the 'dflt' script, then this entry must be used" [[source: ms-opentype-scripttags-2005]]. Its own table spells the tag `DFLT`, and the 2005 chapter 2 spells it 'DFLT' too: "The 'DFLT' Script table should be used if there is not an explicit entry for the script being formatted" [[source: ms-opentype-chapter2-2005]]. So the lowercase spelling is the `DFLT` rule mistyped, the "typo" HarfBuzz cites ("MS site has had typos and many fonts use it now :(. including many versions of DejaVu Sans Mono!", hb-ot-layout.cc:503-504 [[source: harfbuzz-hb-ot-layout]]), and it too says nothing past `DFLT`. Later Wayback snapshots answered 429, so when the spelling was corrected is not known.
- "Developing OpenType Fonts for Standard Scripts" uses "dflt" as a language-system tag, not a script [[source: ms-opentype-standard-scripts]].
- Adobe's feature-file specification, a font tool's document and not normative, is the only text found that speaks past `DFLT`: the `DFLT`/`dflt` pair "is used if a program cannot find a match in the font to the current writing script and language. If it is not in your font, then all the rules may be invisible to the program", which describes A2's behaviour as something that "may" happen, and "`DFLT` is a valid value for the `script` tag, and `dflt` is not" (lines 1130-1134, 1250 [[source: afdko-feature-file-06171944]]).

**The survey** [measured: [ab/qa/counted.txt](ab/qa/counted.txt), over [ab/qa/survey.jsonl](ab/qa/survey.jsonl) (sha256 `fd2d2372ef71ea6d358b748765da63e0e2e322614bbefbed0a01acf5d106772d`, as counted.txt records it) from [ab/qa/survey_run.py](ab/qa/survey_run.py)]. 837 faces on this Mac and in the repository, 2 of which could not be parsed; 541 have a `GSUB` or `GPOS` ScriptList; 323 have a `DFLT` Script table; none has a Script table tagged 'dflt'. A raw table walk over the 420 font files ([ab/qa/langdflt2.py](ab/qa/langdflt2.py) over [ab/qa/fontlist.txt](ab/qa/fontlist.txt), re-run for this note: [ab/qa/langdflt2.out](ab/qa/langdflt2.out)) finds no Script record tagged 'dflt' and none of the 1,414 LangSys records tagged 'dflt' or `DFLT`, and counts 556 `DFLT` and 437 `latn` Script records; the lane's fontTools count was 554 and 437, the two extra being NotoSansPhagsPa's, which fontTools could not parse. Every repository face with a ScriptList, 24 of them, has `DFLT`, so Q-A arises only for a system fallback face; whether baud's fallback ever picks one of the faces below is unmeasured.

**The faces where the options shape differently.** 40 faces lack `DFLT` and have a `latn` default LangSys naming lookups baud applies. They were shaped under three builds of `git archive bc39889` that differ only at `baud-font/src/layout.rs:768` -- A1 `[DFLT, LATN]` as built, A2 `[DFLT]`, A3 `[DFLT, dflt, LATN]` -- by a harness that clusters and resolves scripts as `baud-font/tests/shape.rs` does ([ab/qa/shapeq.rs.txt](ab/qa/shapeq.rs.txt)), checked first against two lines of `script.txt`. **A3 equals A1 on every line** (the final files hash alike). A1 and A2 differ on 28 faces ([ab/qa/faces-differ.tsv](ab/qa/faces-differ.tsv)), and only on strings that resolve to Common or Greek; digits never differed on a face that has a script table. Each of the 11 non-Hiragino faces had 134,143 to 375,886 strings tried, each Hiragino face 48,474; the distinct strings that differ number 5,951 on each of Arial's four faces, Times New Roman Italic and Bold Italic, and Tahoma; 27,718 on Times New Roman, 28,078 on its Bold and on Tahoma Bold, 27,599 on Microsoft Sans Serif, and 2 on each Hiragino face (`02E5 02E9` and `02E9 02E5`). Notation `glyph=cell@dx,dy`; hb-shape is 13.2.1 with the script fixture's features, script guessed:

| face | string | A1 (`latn`), = A3 | A2 (nothing) | hb-shape |
|---|---|---|---|---|
| Arial, Arial Bold | 25CC 0301 | `2825=0@0,0\|1171=0@883,-295` | `…1171=0@1217,0` | = A1 |
| Arial Italic, Arial Bold Italic | 25CC 0301 | `2111\|807=0@883,-295` | `807=0@1217,0` | = A1 |
| Times New Roman | 25CC 0301 | `2825\|1171=0@888,-208` | `1171=0@1217,0` | = A1 |
| Times New Roman Bold | 25CC 0301 | `2825\|1171=0@838,-208` | `@1217,0` | = A1 |
| Times New Roman Italic | 25CC 0301 | `2095\|791=0@638,-208` | `@1217,0` | = A1 |
| Times New Roman Bold Italic | 25CC 0301 | `2095\|791=0@718,-178` | `@1217,0` | = A1 |
| Tahoma | 25CC 0301 | `2731\|589=0@1168,47` | `@1217,0` | = A1 |
| Tahoma Bold | 25CC 0301 | `2731\|589=0@1158,67` | `@1217,0` | = A1 |
| Microsoft Sans Serif | 25CC 0301 | `2498\|682=0@308,7` | `@1217,0` | = A1 |
| those 11 faces, the tone letters | 02E5 02E9 | one ligature glyph (Arial `2001=0`) | two glyphs (`1964=0\|1968=1`) | = A1 |
| Times New Roman, its Bold, Microsoft Sans Serif (Greek; no `grek` table) | 03B1 0308 0301 | TNR `302\|2119=0@1073,0\|1171=0@1353,-68` | `1171=0@1073,0` | = A1 |
| Tahoma Bold (Greek) | 03B1 0308 0301 | `…589=0@1839,390` | `589=0@1289,0` | = A2, confounded (11.5) |
| 17 Hiragino faces (Mincho ProN #0 and #2, Maru Gothic W4 #1, Kaku Gothic W1, W2, W4, W5 #0-1 and W3, W6 #0-2) | 02E5 02E9 | `15857=0@0,0` | `15851=0@0,0\|15855=1@0,0` | = A1 |

Of the 112 final lines (4 on each of the 28 faces), A1 and A2 differ on 43, and hb-shape equals A1 on 42 and A2 on 1, the confounded Tahoma Bold line [measured: [ab/qa/final-A1.tsv](ab/qa/final-A1.tsv), `final-A2.tsv`, `final-hb.tsv`]. **The control:** fontTools copies with `latn` removed from `GSUB` and `GPOS` (Arial, Times New Roman, Tahoma Bold, Microsoft Sans Serif, Hiragino Sans W3) make hb-shape equal A2 on every string, and unmodified round-trip copies leave it equal to A1 and baud's output unchanged ([ab/qa/droptag.py](ab/qa/droptag.py)). So "= A1" means HarfBuzz picks `latn`, not that the two agree by chance. On Krungthep, Ayuthaya, Silom, Plantagenet Cherokee and the eight PT Sans faces, whose `latn` carries lookups baud applies, nothing differed among 11,836, 31,377, 14,023, 9,443 and 252 strings each; the generator skipped chain-context `GSUB` lookups, and those it skipped begin on Latin letters or, for PT Sans, on a mark that follows only a Latin or Cyrillic letter.

**A3 on a face that has 'dflt'.** DejaVu Sans Mono 2.20 has one [[source: dejavu-fonts-ttf-2-20]]; 2.37 has none [[source: dejavu-fonts-ttf-2-37]], and neither is in the repository. Its `GSUB` and `GPOS` scripts are `arab`, `cyrl`, `dflt`, `grek`, `lao ` and `latn`, with no `DFLT`; the 'dflt' tables name no features, and `latn` has `ccmp` and `mark`, whose lookup 7 is a SinglePos with `XAdvance -1233` ([ab/qa/dejavu220.jsonl](ab/qa/dejavu220.jsonl)). baud shapes all 115,672 strings tried alike under A1, A2 and A3. The lookup that makes HarfBuzz's 'dflt' path differ, 7, is a type baud does not apply: it applies `GPOS` lookup types 4, 6 and 9 only (`baud-font/src/gpos.rs:1-10`). Why `latn`'s `GSUB` lookup 1, which baud does apply, changed none of the strings was not checked against its coverage. HarfBuzz takes 'dflt' and differs from forced `latn` on 1,196 strings, every one holding U+0328, which GDEF classes as a base, so nothing zeroes its advance unless lookup 7 runs: `0031 0328` gives `[20=0+1233|668=0+1233]` on the 'dflt' path and `[…|668=0+0]` with `latn`, where baud gives `[20=0@0,0|668=0@1233,0]` under all three. With 'dflt' removed, HarfBuzz's guessed result equals forced `latn` on every line. So A3 is visible in HarfBuzz and not in baud today.

**Decided at 529-530: A1, as built.** A2 would drop `latn`'s mark placement and tone-letter ligature on the 28 faces above, where hb-shape agrees with A1 on 42 of 43 lines.

### 11.3 Q-B and Q-D

Both were decided at 527-528 and both are as built (11.1). Q-B follows the ScriptList text, which sends a script with no table of its own to `DFLT`; HarfBuzz's `GPOS`-off fallback was 0.01 and 0.03. Q-D follows the registry, the only text that orders a pair ("Fonts supporting Unicode encoding of Myanmar script should use 'mym2' instead" [[source: ms-opentype-scripttags]]); for the Indic pairs, which it lists in no order, D1 was decided at 0.97 and 0.95.

### 11.4 Q-C: the order of Hebrew points, the owner's pick

**The question.** Canonical order (C1, built) sorts qamats (18) before dagesh (21) and shin dot (24); HarfBuzz sorts Hebrew points by modified combining classes (C2) that put shin and sin dot, dagesh, rafe and holam first [[source: harfbuzz-hb-unicode]]. On Courier New C1 hands the font shin, qamats, shin dot, and its `ccmp` does not form the shin-dot ligature, so the dot lands a cell right (record, "OpenType script selection, phase 0").

**The standard, quoted first.** Both orders are canonically equivalent: D70 defines canonical equivalents as sequences whose full canonical decompositions are identical, and D108-D109 (§3.11.5) define a reorderable pair as two adjacent characters with ccc(A) > ccc(B) > 0 [[source: unicode-ch3]]. §3.11.2, right after the "only a guideline" passage quoted below: "The only normative use of combining class values is as input to the Canonical Ordering Algorithm, where they are used to normatively distinguish between sequences of combining marks that are canonically equivalent and those that are not." The rest of chapter 3:

- §3.6.3 P5 [Normative], Nondistinct order: "Nonspacing marks with different, non-zero combining classes may occur in different orders without affecting either the visual display of a combining character sequence or the interpretation of that sequence ... The order in which they occur in the combining character sequence does not matter for the display or interpretation of the resulting grapheme cluster." And: "Rendering systems should be prepared to deal with common typed-in sequences and with canonically reordered sequences."
- §3.2.2 C6: "Ideally, an implementation would always interpret two canonical-equivalent character sequences identically."
- §3.11.2, D105: fixed-position classes are "an historical artifact of an earlier stage in its development"; and "This has turned out to be particularly true for some of the fixed position classes for Hebrew and Arabic, for which a distinct combining class is no guarantee that there will be no typographic interaction for rendering. Because of these considerations, particular combining class values should be taken only as a guideline regarding issues of typographic interaction of combining marks."

Chapter 23: Hebrew accents and marks "were assigned fixed-position combining classes despite the fact that they interact typographically" [[source: unicode17-ch23]]. Chapter 9: "<sheva, meteg> ... Because these marks are canonically ordered, this sequence is preserved under normalization", and `<meteg, CGJ, sheva>` for meteg on the right [[source: unicode-ch9]]. **None of these names the order a renderer hands points to a font in, but P5's "should" leans to C2.** P5's "Rendering systems should be prepared to deal with common typed-in sequences and with canonically reordered sequences" was not among the texts the 529-530 request quoted. The committed data says which sequences those are. All 13 Genesis order-differs clusters are typed in canonical order already, P5's "canonically reordered sequences" ([ab/qc/corpus-gen5-differ.txt](ab/qc/corpus-gen5-differ.txt): the typed and C1 columns are equal on every line). 40 of Pango's 41 are typed in C2's order, a "common typed-in sequence" that C1 reorders away from ([ab/qc/corpus-pango-differ.txt](ab/qc/corpus-pango-differ.txt); the 41st, `05E9 05BC 05C1 05B8`, 2 occurrences, is typed in neither order). On Courier New, Arial Hebrew and New Peninim MT, C1 renders neither kind as hb-shape does, and C2 renders both: C2 = hb-shape on 13, 13 and 12 of the 13 Genesis clusters, on 13, 13 and 12 of Pango's 16 points-only clusters and on 9, 0 and 7 of its 25 cantillated ones, where C1 matches 0 of each (the tables below; on Arial Hebrew the engines read different tables). The other reading, that canonically equivalent spellings shape alike, is C6's, not P5's, and both arms meet it (below). So P5's "should" leans to C2 on those faces.

**Below the standard.**

- The Unicode FAQ, not normative: "the rendering engine could reorder the marks internally in a different order if it wanted to, as long as that order was canonically equivalent. In particular, any permutation of the non-zero CCC values can be used", and the values "can only be permuted, not combined or split" [[source: unicode-faq-normalization]]. C2 is such a permutation (checked below).
- The SBL Hebrew manual, v1.5, by its font's vendor about its own font [[source: sbl-hebrew-manual-1-5]]: "the latest generation of Hebrew shaping engines, such as Uniscribe for Windows Vista and Office 2007, perform corrective mark ordering during rendering, to ensure that normalised sequences can be correctly rendered by fonts built to a common specification" (p. 10), and "The SBL Hebrew font is able to correctly render any of these sequences" (p. 8: the extraction cannot read the printed numbers of pages 7-9, and pages 10-12 and 20-22 print their own PDF index). Table 1 (p. 12) recommends a group order -- shin or sin dot, dagesh or mapiq, rafe, holam, then the below marks "as they occur from right-to-left" -- and p. 11 says the table "corresponds, also, to the recommendations for custom normalisation in Appendix B, page 21". Appendix B (pp. 21-22; the manual's p. 10 calls it page 20, line 396 of round g's untracked `src/sbl-hebrew-man-v1-5.txt`) is the same scheme written as classes: Table 1's group 6, the below marks as they occur from right to left, is Appendix B's single merged class 220, with shin dot 10, sin dot 11, dagesh 21, rafe 23 and holam 27 before it. HarfBuzz follows SBL's group order for shin and sin dot, dagesh, rafe and holam. Inside the below marks it keeps distinct classes in a fixed order SBL does not give, because the FAQ forbids an engine to merge classes ("can only be permuted, not combined or split"). C2 is therefore HarfBuzz's modified classes, not "SBL order" as a whole; the patch's doc comment keeps the lane's wording.
- John Hudson elsewhere: the 2003 unicode-ml messages are about normalisation breaking Biblical Hebrew, not about what order a font expects [[source: unicode-ml-2003-06-0340]], [[source: unicode-ml-2003-07-0120]]; on Typedrawers in 2023 he advises mark filtering so a font's ligature forms whatever the order [[source: typedrawers-4728]].
- Mozilla bug 662055 [[source: mozilla-bug-662055]]: comment 17, "the canonical order is not the order supported by most font designers, who assume 'logical' order"; comment 29 cites the FAQ's permute-only rule; comments 38-39 move sheva and hiriq after patah and qamats, which is Mozilla's change, not SBL's.
- The engine-documentation project, opentype-shaping-documents at bd64cf1, lines 285-286: "For `<hebr>` text runs, normalizing the sequence of marks using the Unicode _canonical combining class_ of each mark should be sufficient" [[source: osd-opentype-shaping-hebrew]]. It is the only text found that names an order for a Hebrew engine; it names C1, it is not normative, and Courier New contradicts it.
- Microsoft's Hebrew page names no order for the marks [[source: ms-hebrew-script-development]]. A FontLab VOLT forum post was not retrieved (403 directly, 429 from Wayback).

**C2, built as a patch and measured.** [ab/c2-sbl-sort.patch](ab/c2-sbl-sort.patch) applies to bc39889 (`git apply --check`). It adds `SORT_CLASS`, a 256-entry table built at compile time mapping classes 10..26 as hb-unicode.hh:333-349 does -- 10→22, 11→15, 12→16, 13→17, 14→23, 15→18, 16→19, 17→20, 18→21, 19→14, 20→24, 21→12, 22→25, 23→13, 24→10, 25→11, 26→26 -- and uses it only in the insertion sort in `baud-font/src/shape.rs`; `glyph.ccc`, which the fallback placer reads, and the composition-blocking key keep Unicode's classes. Its reach, measured:

- Hebrew only: every class in 10..26 is in U+05B0-05C7 or FB1E.
- Composition blocking is moot: no composition row has a Hebrew point as its second element (0 of 961; the control, U+0301 as second element, finds 117).
- It is a permutation: HarfBuzz's values for 10..26 sort back to exactly 10..26.
- Where the two orders agree, output is byte-identical on every face (0 differences).
- It fails the audit budget. `git apply` at bc39889, then `audit_all.sh`, gives "FAIL baud-font: 13 raw-integer sites, 61 signature (limit 12 and 60)", from `shape.rs`'s `i as u8` (a raw-integer site) and `fn sort_class(...) -> u8` (a signature site); the base passes at 12 and 60. Budgets are never raised, so applying the patch requires typing those two sites first.
- The control, Courier New shin + qamats + shin dot: C1 `[697, 661, 669@+1229]`, C2 `[711, 661]`, which is hb-shape's `[661, 711]` in logical order.
- `cargo test -p baud-font` on C2: 136 passed, 0 failed, over 18 test binaries ([ab/qc/test-c2.log](ab/qc/test-c2.log)); `every_script_fixture_shapes_as_harfbuzz_does` passes on both arms ([ab/qc/test-c1-script.log](ab/qc/test-c1-script.log)). The fixtures cannot tell the arms apart: `script.txt`'s 26 Hebrew lines, 13 on each face, have no cell with two marks.
- Invariance: the 360 canonically equivalent spellings of the 120 corpus clusters with two or more marks ([ab/qc/perms.txt](ab/qc/perms.txt)) shape alike within each arm on the built Regular, Courier New, Arial Hebrew and SF Hebrew.

**The corpora.** A cluster is a base and the marks after it; counts are distinct / occurrences.

| corpus | Hebrew clusters | two or more marks | canonical order ≠ C2's |
|---|---|---|---|
| Genesis 1:1-5, hand-typed ([ab/qc/t-gen5.txt](ab/qc/t-gen5.txt), `ESC[?25l` stripped before use) | 70 / 209 | 13 / 17 | 13 / 17 |
| Pango `test-hebrew.txt` [[source: pango-8e74c27-test-hebrew]] | 190 / 641 | 107 / 169 | 41 / 70 |

HarfBuzz's three-mark Hebrew reorder (hb-ot-shaper-hebrew.cc:165-190 [[source: harfbuzz-hb-ot-shaper-hebrew]]) fires 0 times in either, and neither holds CGJ. Of the order-differs clusters, Pango's split 16 / 40 points only and 25 / 30 with cantillation.

**Per face** [measured: [ab/qc/tables.md](ab/qc/tables.md), the full table by subset; [ab/qc/attrib.md](ab/qc/attrib.md)]. Each distinct cluster shaped alone by both arms and by hb-shape 13.2.1 with the `script.txt` header's features; "= hb" is the same set of (glyph, x, y) relative to the cell; "outside" is a mark whose ink centre falls outside [0, advance). The built faces are the main checkout's Regular and Bold, byte-identical to the scratch tree's copies, whose name field reads "Proto Hebrew Mono C Regular/Bold".

| face | Genesis, order differs (13/17): C1 = hb / C2 = hb | outside its cell: C1 / C2 / hb | Genesis, all (70/209): C1 = hb → C2 = hb | Pango, all (190/641): C1 = hb → C2 = hb |
|---|---|---|---|---|
| built Regular | 13/17 / 13/17 | 0/0 / 0/0 / 0/0 | 70/209 → 70/209 | 91/512 → 91/512 |
| built Bold | 13/17 / 13/17 | 0/0 / 0/0 / 0/0 | 70/209 → 70/209 | 91/512 → 91/512 |
| Courier New | 0/0 / 13/17 | 13/17 / 0/0 / 0/0 | 54/186 → 67/203 | 101/494 → 123/542 |
| Courier New Bold | 0/0 / 13/17 | 13/17 / 0/0 / 0/0 | 54/186 → 67/203 | 101/494 → 123/542 |
| Arial Hebrew | 0/0 / 13/17 | 10/12 / 0/0 / 0/0 | 57/192 → 70/209 | 62/444 → 75/481 |
| Arial Hebrew Scholar | 0/0 / 13/17 | 10/12 / 0/0 / 0/0 | 57/192 → 70/209 | 87/477 → 100/514 |
| SF Hebrew | 13/17 / 13/17 | 0/0 / 0/0 / 0/0 | 70/209 → 70/209 | 190/641 → 190/641 |
| SF Hebrew Rounded | 13/17 / 13/17 | 0/0 / 0/0 / 0/0 | 70/209 → 70/209 | 190/641 → 190/641 |
| New Peninim MT | 0/0 / 12/16 | 0/0 / 0/0 / 0/0 | 36/140 → 48/156 | 74/381 → 93/424 |
| Raanana | 2/3 / 2/3 | 0/0 / 0/0 / 0/0 | 19/96 → 19/96 | 42/279 → 42/279 |
| Corsiva Hebrew | 0/0 / 0/0 | 0/0 / 0/0 / 0/0 | 26/104 → 26/104 | 52/287 → 52/287 |
| Lucida Grande | 0/0 / 0/0 | 0/0 / 0/0 / 0/0 | 26/108 → 26/108 | 56/302 → 56/302 |
| Arial Unicode MS | 0/0 / 0/0 | 13/17 / 13/17 / 0/0 | 14/81 → 14/81 | 15/210 → 15/210 |

- **No cluster that matches hb-shape under C1 stops matching under C2**, on any face or corpus: regressions 0 / 0 on all 13 faces, and C2 newly matches 13 / 22 clusters (Genesis / Pango) on Courier New and its Bold, 13 / 13 on Arial Hebrew and Arial Hebrew Scholar, and 12 / 19 on New Peninim MT (attrib.md).
- Courier New and New Peninim MT read the same table in both engines (`GSUB` `hebr`; `morx`). On Arial Hebrew and Arial Hebrew Scholar, HarfBuzz shapes through `morx` (hb-ot-shape.cc:60-65 [[source: harfbuzz-hb-ot-shape]]) and baud through `GSUB`, so they compare different tables.
- What the remaining mismatches are, none of them the order: HarfBuzz composes FB1D-FB4F presentation forms when a face has no `GPOS` `mark` (hb-ot-shaper-hebrew.cc:81), and baud does not; on Corsiva Hebrew, Lucida Grande and Arial Unicode MS, 15, 15 and 16 Genesis mismatches carry one, all 13 order-differs clusters among them; Raanana's fallback puts a point at dy -4 under HarfBuzz and 0 under baud, with every glyph id equal; Courier New's 67 Pango residuals differ only in position. Arial Hebrew's 115 Pango clusters whose glyph ids differ under C2 (attrib.md) each hold meteg or a cantillation mark, and C2's output holds glyph 268 in every one, with as many glyphs as hb-shape's, which is 11.5's `glyph268` defect; 28 are order-differs clusters, whose ids differ in both arms. New Peninim MT's 18 Genesis and 41 Pango clusters whose ids differ are none of them order-differs clusters or FB forms, and C1's output equals C2's on every one; on 10 and 24 of them hb-shape draws fewer glyphs than baud (`05E8 05B5`: hb-shape one glyph, 51; baud the letter and the point, 167 and 124). Why baud's substitution differs there is unmeasured [counted: [ab/qc/residuals.txt](ab/qc/residuals.txt), by `residuals.py` over `measure.json.gz`]. On the built faces, Pango's 25 / 30 cantillated order-differs clusters are `.notdef` in both arms: the font has no cantillation glyphs.

**Cost: no result** [measured: [ab/qc/timing.txt](ab/qc/timing.txt), [ab/qc/counted.txt](ab/qc/counted.txt)]. The arms were built at one path; rebuilt copies C1′ and C2′ are byte-identical to C1 and C2 and serve as the same-source pair; the two arms' `__text` differ by 64 bytes (0x54280 against 0x542c0). Six rounds rotated, the first dropped, on AC, at a load average of 12-14 on 18 cores from other work; 850 clusters in text order; nanoseconds a cluster, median [range]:

| mode, face | C1 | C1′ | C2 | C2′ |
|---|---|---|---|---|
| normalisation only, Courier New | 57.57 [55.61-59.42] | 56.96 [55.92-60.26] | 57.45 [56.96-59.40] | 57.22 [55.32-57.92] |
| normalisation only, built Regular | 71.95 [70.76-76.58] | 73.97 [72.89-76.49] | 72.35 [71.04-74.69] | 73.00 [71.47-73.70] |
| full shaping, Courier New | 2065.74 [2009.58-2179.68] | 2070.43 [2028.53-2076.58] | 2055.38 [1985.90-2327.13] | 2051.92 [2006.85-2056.77] |
| full shaping, built Regular | 6195.18 [6047.97-6411.68] | 6242.39 [6162.68-6431.44] | 6205.63 [5999.91-6782.20] | 6232.01 [6104.90-6426.70] |

Every range overlaps, and the gap between arms is inside the gap between same-source copies. Full shaping ran without the lookup digests, so the normalisation rows are the ones that isolate the change. Rule 1 by argument: C2 writes the same code points in another order, the swaps it makes depend on the order the text was typed in, and the working set grows by the 256-byte table.

**Why Jev did not decide it.** C1 was the pick in all four runs, at 0.78, 0.50, 0.36 and 0.30. The two C2s were different options: 527-528's C2_SBL, with HarfBuzz's swap, drew 0.11 and 0.25, and 529-530's C2_HB_CLASSES, the patch without the swap, drew 0.32 and 0.35. On these corpora they would shape alike, because the swap fires 0 times in both; 13f's was not built. Neither request carried P5's "should be prepared" sentence. The options as asked, both conforming:

- **C1, canonical order (built).** Conforming: D108-D109's order. Against: on Courier New, Arial Hebrew and New Peninim MT a second point lands outside its letter or off hb-shape where C2 matches.
- **C2, HarfBuzz's modified classes (the patch).** Conforming: a permutation of the fixed-position classes, canonically equivalent, within the FAQ's rule. For: matches hb-shape on every cluster C1 matches, and on 13, 13 and 12 more distinct Genesis clusters on those faces; P5's "should" leans to it on those three faces. Against: no normative text names it, and the one engine document that names an order names C1.

Not built: an Appendix B arm (merged classes, outside the FAQ's rule) and HarfBuzz's three-mark reorder (0 occurrences). That the vowel-before-meteg order is the same in both arms is derived from the table (every modified vowel class is below meteg's 25), not measured.

**The owner's answer.** The owner, 2026-10-13, asked in the session with C1 and C2 put side by side, picked C2 (HarfBuzz's order); Jev had 0.36/0.30 (decisions.jsonl 529-530). C2 is to be built in its own commit, with baud-font's two raw-integer sites typed first so audit_all.sh stays at 12/60.

**Built, 2026-10-14** (the record, "Two Hebrew points in one cell reach the face in HarfBuzz's order"): `CombiningClass::sort_key`, with neither of the patch's two sites, so baud-font stays at 12 and 60. Over the 43 faces of the survey list that map shin and two or more points, no cluster that matched hb-shape stops matching, and this note's `tables.md` regenerates byte for byte from that commit's `measure.json` (`ab/qc-built/measure.json.gz`). `glyph.ccc` and the composition blocking keep Unicode's classes, as the patch left them.

### 11.5 Found and not fixed

- ~~**Arial Unicode MS** (it has GDEF glyph classes): baud keeps the marks' advances in the pen, so marks land at +1153 and +1890 and sit outside their cell in 56 of 70 distinct Genesis clusters (128 of 209 occurrences), in both arms.~~
- ~~**Courier New, two marks below:** HarfBuzz spreads them to ±250, baud stacks them at 0 -- including ch. 9's `<meteg, CGJ, sheva>` and the patah-and-hiriq of Jerusalem ([ab/qc/cgj.txt](ab/qc/cgj.txt)); CGJ blocks the sort alike in both arms. A holam alone sits 25 units off hb-shape's position.~~
- ~~**Arial Hebrew:** baud's `GSUB` turns meteg and the cantillation marks into `glyph268`.~~
- ~~**`GPOS` SinglePos is not applied.** baud applies `GPOS` lookup types 4, 6 and 9 only, so DejaVu Sans Mono 2.20's `latn` `mark` lookup 7 (`XAdvance -1233`) never runs; on that face U+0328 keeps its advance whichever table is chosen.~~
- **The four above, built, 2026-10-23** (the record, "Hebrew points on Arial Unicode MS, Courier New and Arial Hebrew sit as HarfBuzz sets them"): `GPOS` types 1, 2 and 8 run; a Hebrew run whose `GPOS` selects no Script table is placed as on a face with none and composes the presentation forms (Jev 0.97/1.00); a face carrying `GSUB` and a `morx` baud runs whole shapes through `morx` (Jev 0.91/0.94). All three faces match hb-shape on every distinct cluster of both corpora. DejaVu Sans Mono 2.20's lookup 7 now runs; what it does to U+0328 there is unmeasured. Still open: Arial Hebrew Scholar, which HarfBuzz positions with `kerx` format 4, unbuilt.
- **hb-shape depends on the file's bytes on three faces:** on `03B1 0308 0301` it leaves the acute unplaced on the original Tahoma Bold, Tahoma and Times New Roman Italic, and places it exactly as baud does (`589=0@550,390`) on fontTools round-trip copies with the script records unchanged. Tahoma and TNR Italic have a `grek` table, where A1 = A2. This is why 11.2's one hb = A2 line is confounded; its cause is unmeasured.
- **The Q-C lane's report says "Every other face is unchanged"** after listing Courier New, Arial Hebrew and New Peninim MT; its own table moves Arial Hebrew Scholar too, 57/192 → 70/209 on Genesis and 87/477 → 100/514 on Pango. The tables above are generated from tables.md, not the report.
- **The record's labels for Q-A's A3 and Q-D's D3** (departing) disagree with the requests' (conforming); 11.1.
- **~~Raanana's fallback puts a point at dy -4 under HarfBuzz and 0 under baud~~ (11.4). Built, 2026-10-15** (the record, "Hebrew points on a face with no GPOS are placed as HarfBuzz places them"): the fallback placer reads `recategorize_combining_class`'s Hebrew classes, and Raanana and Raanana Bold match hb-shape on every distinct cluster of both corpora, 70/209 and 190/641, from 19/96 and 42/279. Those figures shaped Raanana with the grid set, whose `-rlig` turns its default-on flag 0x2 off; at the chain defaults baud runs, it was 45/158 and 120/471 until `morx` walked right-to-left runs in logical order (the record, "A `morx` subtable walks a right-to-left run in the order its coverage names").
- **Correction to 11.4's attribution of the presentation forms on Lucida Grande and Corsiva Hebrew.** They shape through `morx`, and on a `morx` face HarfBuzz replaces the Hebrew shaper with its dumber one (hb-ot-shape.cc:99-101 [[source: harfbuzz-hb-ot-shape]]), so `compose_hebrew` (hb-ot-shaper-hebrew.cc:81) does not run there. hb-shape's `--trace` shows the ligature formed by a `morx` chain subtable: 4 on Lucida Grande, 1 on Corsiva Hebrew and on New Peninim MT. **Found and built, 2026-10-15** (the record, "A `morx` subtable walks a right-to-left run in the order its coverage names"): those subtables run in reverse layout order, which on a right-to-left run is logical order, and baud walked them backwards; it now reads the run's direction from its script, and every distinct cluster of both corpora matches hb-shape on all eight faces.

### 11.6 The files in ab/

All copied from the session scratchpad on 2026-10-13, except `qc/residuals.py` and `qc/residuals.txt`, written for this note, and `qc/counted.txt`, whose header now names the committed files; the large intermediate outputs (the per-string `out*-A*.tsv`, 30-323 MB each, and the candidate lists) and the modified system fonts in the perturbation set are not committed.

- [ab/c2-sbl-sort.patch](ab/c2-sbl-sort.patch): arm C2, sha256 `8527558c8fe1ac3afc28b10ff9cb66a147147b07fff0bf50b3429c2b3813a81c`, the bytes the 529-530 request carried. Apply at bc39889 with `git apply docs/research/notes/ab/c2-sbl-sort.patch`; as it stands it fails `audit_all.sh` (baud-font at 13 and 61 against 12 and 60), so its `i as u8` and its `-> u8` signature are typed before it lands (11.4).
- `ab/qa/`: the 420 font files surveyed (`fontlist.txt`), the survey runner, its library and its output (`survey_run.py`, `survey.py`, `survey.jsonl`, 413,644 bytes), the raw table walk and its re-run output (`langdflt2.py`, `langdflt2.out`), the string generators (`gen.py`, `gen2.py`, `gen3.py`), the comparison and count scripts, the harness (`shapeq.rs.txt`, an example in a scratch copy of `baud-font`), the 28 faces, the 112 final lines (`final-in.tsv`, `final-A1.tsv`, `final-A2.tsv`, `final-hb.tsv`; `final-A3.tsv` hashed equal to A1's and is not kept), DejaVu 2.20's survey row, and `counted.txt`.
- `ab/qc/`: the harness (`qc.rs.txt`), `make_c2.py`, the measurement scripts, the corpora as clustered (`corpus-gen5.txt`, `corpus-pango.txt`, their `-occ.json` occurrence counts and `-differ.txt` order-differs lists, `clusters-all.txt` and its UTF-8 form), the per-cluster results (`measure.json.gz`, gzip of the lane's 1.1 MB `measure.json`, sha256 `420b0f477fad79edaa2778b8aac234de879ae044f5813f04a6ce2f3c4fcf1253`), `tables.md`, `attrib.md`, `residuals.py` and its output `residuals.txt`, the 850 timed clusters (`bench-clusters.txt`), `timing.txt` and `timing.sh.txt`, the 360 spellings, the CGJ probes, both test logs, the Genesis text and `counted.txt`. `tables.py` over `measure.json` and the two `-occ.json` files regenerates `tables.md` byte for byte (checked for this note); `attrib.py` and `measure.py` also need fontTools and the faces.

- `ab/qc-built/`, added 2026-10-14 for the build of C2: the 43 faces (`hebfaces.py`, `hebfaces.tsv`), the survey over them (`measure_all.py`, which is `ab/qc/measure.py` with that face list, `measure-all.json.gz`, the gzip of its output, and `survey.py`'s count of it, `survey.txt`), `ab/qc/measure.py`'s own output at that commit (`measure.json.gz`), and the timing and instruction counts with their scripts (`timing.txt`, `instr.txt`, `frame-instr.txt`, `tab.py`).

- `ab/qc-fallback/`, added 2026-10-15 for the fallback placer's Hebrew classes: `corpus_cmp.py`, which shapes `ab/qc/clusters-all.txt` with hb-shape and the `qc` harness built from the commit before and from that commit, on Raanana, Raanana Bold, New Peninim MT, Corsiva Hebrew and Lucida Grande, and its output `corpus.txt`.

Harness sources carry a `.txt` suffix so the formatter leaves the bytes that ran.
