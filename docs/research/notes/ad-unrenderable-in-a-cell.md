# AD. What a cell draws for a code point no face draws

**Status: open, researched 2026-10-08.** Nothing here is decided or built. The
note reopens the rule built in e0a611d (note AB, Q23) and sets out the questions
for Jev and then the owner.

Evidence is tagged as in the other notes: measured, source (a citation to a
manifest key, or file:line at e0a611d), inferred, or unknown. Five read-only
research lenses ran on 2026-10-08 (standards, terminals, engines, frequency, design). Their
scratch directories do not survive a reboot. So every primary source is in
`docs/research/sources/` with a manifest entry, and the captures and
measurement outputs a claim here rests on are under
`docs/research/sources/ad-observed/`, keyed `ad-obs-*`.

## 1. The question

The owner asked for more research on this question. Then the owner asked "are
we doing something outside standard?" and "could be jev is missing context".

**The question.** When no face in baud's stack draws a grapheme cluster whole,
what should the cell draw for the code points the kept face lacks?

**What baud does today (e0a611d).**

- A marked cell takes the first face that covers its whole cluster, after the
  shaper's normalisation. The base's face goes first, then the fallback chain
  (`Stack::select_cluster`, stack.rs:489-499).
- If no face does, the base's face keeps the cluster. Every attached code point
  it lacks that is not default-ignorable is dropped before shaping
  (`Normalised::without_unmapped`, shape.rs:443). The drop is counted in
  `Renderer::marks_unmapped` (renderer.rs:2444-2468), which nothing on screen
  shows.
- The rule covers combining marks and also attachments that are not marks: the
  emoji after a ZWJ, a skin-tone modifier, a flag's second regional indicator,
  and a consonant joined into an Indic conjunct (U+0915 is among those left out
  on the Linux stack, section 5.3). It also covers the parts of a precomposed
  base the face lacks.
- Before e0a611d, the missing code point drew the face's `.notdef` with a
  full-cell advance. The box drifted into the next cell and took every later
  mark with it (Q23).

**Why this reopens A_DROP.**

1. **The standard recommends something visible.** For a character an
   implementation cannot draw, the Unicode Standard recommends a visible
   placeholder, "so that at least it is clear that there are characters
   present—and usually, how many are present" ([[source: unicode18-ch5]]
   § 5.21.6). Chapter 5 says its guidelines are best practice and not
   normative (section 2.2). The drop shows nothing. No request to Jev quoted
   the standard (section 7.1).
2. **One option was never offered.** A_DROP was asked as a question about
   combining marks, with four options: drop the mark, replace the whole cell,
   use the base's face only and drop, or overlay the mark from another face.
   None of them kept the base and drew a placeholder inside the cell, so that
   option was never priced.
3. **The renders of the non-mark half looked wrong.** The review-v13 renders
   (`.claude/research/review-v13/starters/`, which is untracked, sheets a-e
   and noto-a..c; each sheet is copied into `ad-observed/` and keyed
   `ad-obs-review-v13-sheet-<sheet>-<px>`, for example
   [[source: ad-obs-review-v13-sheet-a-28]] and
   [[source: ad-obs-review-v13-sheet-noto-a-56]]) show two problems:
   - AS_BUILT draws a woman where the text says woman technologist.
   - A box drawn with the base face's `.notdef` vanishes under Noto Color Emoji.

   Jev split three ways on this (AS_BUILT 0.47, BOX_PRIMARY 0.29,
   MARKS_ONLY_ELSE_BOX 0.24, at confidence 0.20).
4. **The frequency numbers did not exist yet.** How often each case happens
   on the stacks baud ships is new in this note (section 5).

## 2. What the standards say

### 2.1 Which text, which version

- **The latest version is Unicode 18.0.0, and it is released.**
  `https://www.unicode.org/versions/latest/` redirects to its page, and the
  list of versions gives "Unicode 18.0.0 | Components | 2026"
  ([[source: unicode-enumerated-versions]]). The release page is dated "2026
  September 16" and still carries a draft banner from the beta review, "STATUS:
  This is a preliminary draft page for an upcoming release"
  ([[source: unicode18-release]]). baud's UCD pins are 17.0.0; note AB, Q20
  asks which version the bidi tables should use.
- **§ 5.3 is unchanged from 16.0 to 17.0.** A word-level diff showed only
  no-break spaces ([[source: unicode16-ch5]], [[source: unicode17-ch5]]).
  18.0 changes one paragraph, Default Ignorable Code Points, and adds one after
  it:
  - 17.0: "When such a special-use character is not supported by an
    implementation, it should not be displayed with a visible fallback glyph,
    but instead simply not be rendered at all. The list of such characters
    which should not be rendered with a fallback glyph is defined by the
    Default_Ignorable_Code_Point property".
  - 18.0 adds "usually" ("usually it should not be displayed with a visible
    fallback glyph") and drops the clause "which should not be rendered with a
    fallback glyph". The new paragraph says: "Note that misplaced variation
    selectors (D57h) are an exception to this general behavior. The standard
    requires that misplaced variation selectors not be interpreted, and
    recommends that they be displayed visibly, to obviate certain security
    risks." ([[source: unicode18-ch5]] § 5.3)
- **D57a-D57h are new in 18.0.** Chapter 3 of 17.0 has D57 and D58 and nothing
  between them ([[source: unicode17-ch3]]).
- **UTS #51 is now revision 31** (Version 18.0, 2026-08-28,
  [[source: uts51-r31]]). Every passage quoted below is also in revision 29
  ([[source: uts51-emoji]]); the standards lens matched each one once in both.

### 2.2 How strong the text is

The passages on fallback for an unrenderable character (§ 5.3, § 5.13 and
§ 5.21.6) are in chapter 5, which says of itself, in 16.0, 17.0 and 18.0:

> These recommended guidelines are not normative and are not binding on the
> implementer, but are intended to represent best practice.
> ([[source: unicode18-ch5]], chapter introduction)

D52, D57h and § 23.4 address display elsewhere, with "may" and "should": D52
(a combining character "may" be presented as a base) and D57h with the
paragraph after it (§ 3.6.2) are in chapter 3, the conformance chapter, and
§ 23.4 says an unsupported variation selector "should be invisible and
ignored". D57h also carries a "shall": "A conformant implementation shall not
interpret a misplaced variation selector for any purpose not defined in this
standard." Its display sentence is a "should" ([[source: unicode18-ch3]]).

The normative clauses allow an implementation that cannot draw everything:

> C5 A process shall not assume that it is required to interpret any
> particular coded character sequence. Processes that interpret only a subset
> of Unicode characters are allowed; there is no blanket requirement to
> interpret all Unicode characters.
> ([[source: unicode18-ch3]] § 3.2.2)

> The Unicode Standard does not require that an application be capable of
> interpreting and rendering all Unicode characters so as to be conformant.
> ([[source: unicode18-ch2]] § 2.14.4)

Deletion matters for conformance only in text:

> C7 When a process purports not to modify the interpretation of a valid coded
> character sequence, it shall make no change to that coded character sequence
> ... Replacement or deletion of a character sequence that the process cannot
> or does not interpret does modify the interpretation of the text.
> ([[source: unicode18-ch3]] § 3.2.3)

§ 2.14.2 lists among what "is unacceptable for a conforming implementation":
"To remove or alter uninterpreted code points in text that purports to be
unmodified." ([[source: unicode18-ch2]])

**ISO/IEC 10646 has a device clause that does reach display.** It is § 2.3
c) of the 2010 Final Committee Draft ([[source: iso10646-fcd]]; the published
edition was not read, and its clause number may differ):

> c) Receiving device: A receiving device shall be capable of receiving and
> interpreting any coded representation of characters that are within a
> CC-data-element in accordance with the adopted encoding form and the adopted
> encoding scheme, and shall make any corresponding characters from the
> adopted subset available to the user in such a way that the user can
> identify them. ...
> Any corresponding characters that are not within the adopted subset shall be
> indicated to the user. The way used for indicating them need not distinguish
> them from each other.
> NOTE 1 – The manner in which a user is notified of either an error condition
> or characters not within the adopted subset is not specified by this
> standard.

It binds a device that claims conformance to 10646, and the claim names the
device's adopted subset (§ 2.3). The subset gives no way out for a silent drop
(reading). A character inside the adopted subset must be identifiable, and one
outside it must be indicated, and a dropped code point is neither. Whether
baud claims 10646 device conformance is the owner's call, as UTS #51
conformance is. The verdicts below state this condition once, here, and refer
back to it.

UTS #51 is not part of conformance either: "Conformance to the Unicode Standard
does not require conformance to UTS #51." ([[source: unicode18-ch22]] § 22.10)

**So the answer to "outside standard?" has three parts.**

- **The renderer's drop breaks no Unicode conformance clause (reading).** No
  clause of the Unicode Standard addresses display fallback, and C5 allows a
  process not to interpret the dropped code point. The drop departs from the
  standard's stated best practice (chapter 5) and from the displays UTS #51
  describes for the same cases. It also departs from ISO/IEC 10646 § 2.3 c)
  ("shall be indicated to the user"), if baud claims 10646 device conformance.
- **The core's deletions are a different matter (sections 2.6, 5.2 and 5.4).**
  Copy reads `Term`, so a code point the core refuses is gone from what copy
  emits, and C7 and § 2.14.2 bind what leaves the process. Whether a terminal's
  copy "purports not to modify" the text is the owner's reading to make. On
  screen, the refused code point shows as nothing, which departs from § 5.21.6
  as the renderer's drop does, and from § 2.3 c) under a 10646 claim (section
  2.7).
- **Mode 2027 is a third matter, because baud claims it (section 2.4).** The
  draft's semantics "MUST be adhered to" while the mode is set, baud answers it
  permanently set, and the draft says sub-images "must not be used as a
  fallback" for a ZWJ sequence. baud draws ZWJ sequences as separate
  sub-images today, past their two cells, on every stack it ships, with or
  without a drop (section 2.7 has the counts). That departs from a "must not"
  in a draft its README marks ALPHA.

**A correction to the reading that started this, and where it still holds.**
The sentence "An implementation should not blindly delete such characters, nor
should it unintentionally transform them into something else" is in § 5.3's
paragraph "Reserved and Private-Use Character Codes". That paragraph covers
"Code points that are reserved" and "Code points in the Private Use Area for
which no private agreement exists". It does not cover an assigned character a
font lacks.

It does reach the drop in one case, the one the same paragraph names: "in
practice, applications must deal with unassigned code points or private-use
characters. This may occur, for example, when the application is handling text
that originated on a system implementing a later release of the Unicode
Standard".

- **Reserved code points join a cluster after a ZWJ.** UCD 17.0 lists
  1FAF9..1FAFF and 1FC00..1FFFD, all reserved, as Extended_Pictographic
  ([[source: emoji-data]], lines 1283-1284), among other reserved blocks, so
  GB11 attaches them after a ZWJ.
- **They are not default-ignorable.** DerivedCoreProperties 17.0 gives that
  property to reserved code points only at 2065, FFF0..FFF8 and in
  E0000..E0FFF ([[source: dcp-data]]).
- **So the drop deletes them (measured).** Woman + ZWJ + U+1FAFF, + U+1FC00
  and + U+1FAF9 each keep all three code points in the core. The renderer then
  drops the last one, `marks_unmapped` +1, and draws the bare woman. This
  happens on the macOS stack and on both Linux stacks measured
  ([[source: ad-obs-reserved-tags-vs]]).
- **Text from a later version reaches this today.** U+1FAF9 and U+1FAFA are
  Emoji 18.0 characters ([[source: ucd18-emoji-data]], line 1299). They are
  reserved to baud, which pins UCD 17.0, and no shipped emoji face maps them.

That is the downlevel case § 5.3 names, and the drop deletes the code point
in it.

The paragraph that covers an assigned character a font lacks is
"Interpretable but Unrenderable Characters":

> An implementation may receive a code point to which a character is assigned
> in the Unicode Standard, but be unable to render it, because it lacks a font
> for the code point or is otherwise incapable of rendering it appropriately.
> ... any unrenderable character can only be indicated by some placeholder,
> such as displaying the hexadecimal value of the code point, or other
> meaningful information, such as the script of the character. ... The Unicode
> Consortium maintains an open source Last Resort Font ... that implementations
> can use for such fallback rendering of both assigned and unassigned code
> points.
> ([[source: unicode18-ch5]] § 5.3; the same in [[source: unicode16-ch5]])

### 2.3 Case A: a combining mark the kept face lacks

A mark is a graphic character:

> D50 Graphic character: A character with the General Category of Letter (L),
> Combining Mark (M), Number (N), Punctuation (P), Symbol (S), or Space
> Separator (Zs).
> ([[source: unicode18-ch3]] § 3.6.1)

§ 5.21.6 says what to do with a graphic character the implementation cannot
draw:

> The recommended behavior for display in such cases is to fall back to some
> visible, but generic, glyph display for graphic characters, so that at least
> it is clear that there are characters present—and usually, how many are
> present.

> if an implementation does not support U+0915 क DEVANAGARI LETTER KA, it
> should not ignore that character for display. Displaying nothing would give
> the user the impression that the character does not occur in the text at
> all. The recommendation in that case is to display a “last-resort” glyph or
> a visible “missing glyph” box, instead.
> ([[source: unicode18-ch5]] § 5.21.6, "Fallback Rendering")

§ 5.13 and D52 describe a different case: a mark the implementation has a
glyph for but cannot position on its base. § 5.13's fallbacks are for "an
unknown composed character sequence that is outside of a fixed, renderable
set", and both draw the mark's own glyph:

> One method (Show Hidden) indicates the inability to draw the sequence by
> drawing the base character first and then rendering the nonspacing mark as
> an individual unit, with the nonspacing mark positioned on a dotted circle.

> Another method (Simple Overlap) uses a default fixed position for an
> overlapping zero-width nonspacing mark. This position is generally high
> enough to make sure that the mark does not collide with capital letters.
> ([[source: unicode18-ch5]] § 5.13, "Fallback Rendering")

§ 5.13 also scopes itself: "This discussion assumes the use of proportional
fonts, where the widths of individual characters can vary. Various techniques
can be used with monospaced fonts. In general, however, it is possible to get
only a semblance of a correct rendering for most scripts in such fonts."

D52 lets a mark that cannot be combined be drawn as if it were a base
character: "With isolated combining characters or when a process is unable to
perform graphical combination, a process may present a combining character
without graphical combination; that is, it may present it as if it were a base
character." ([[source: unicode18-ch3]] § 3.6.1)

**Reading.** For a mark no face has, the only text is § 5.21.6's "visible, but
generic, glyph display", which names "Displaying nothing" as what to avoid.
§ 5.13 and D52 apply when some face has the mark (option h in section 6). None
of the four describes dropping it.

### 2.4 Case B: an attachment that is not a mark

**UTS #51 describes a display for each kind** ([[source: uts51-r31]]):

| attachment the font lacks | what UTS #51 says |
|---|---|
| skin-tone modifier | § 2.4.1: "The “Unsupported” rows show how the character would typically appear on a system that does not have a font with that character in it: with a missing glyph indicator." The image in that row is a hollow box ([[source: uts51-img-unsupported]]) after the base. |
| skin-tone modifier, on its own | Under ED-20: "Skin tone modifiers and hair components should be displayed even in isolation". § 2.4: "When used alone, the default representation of these modifier characters is a color swatch." |
| a ZWJ sequence with no combined glyph | § 2.5: "When an emoji ZWJ sequence is sent to a system that does not have a corresponding single glyph, the ZWJ characters are ignored and a fallback sequence of separate emoji is displayed." § 2.2: "Implementations could provide an indication of the composed nature of an unsupported emoji sequence where possible", for example "Display the ZWJ as a visible “glue” character, with zero or very narrow width." |
| colour or direction | § 2.9: "the user should see the fallback appearance showing an indication of the desired color." § 2.10 says the same of direction. |
| a regional-indicator pair with no flag | Annex B.1: "Display each REGIONAL INDICATOR symbol separately as a letter in a dotted square", or "For all unsupported REGIONAL INDICATOR pairs, display the same missing flag glyph" ([[source: uts51-img-missing-flag]]); the second "would indicate that the supported pair was intended to represent the flag of some region, without indicating which one". B.1 opens "Options for presenting an emoji_flag_sequence for which a system does not have a specific flag or other glyph", so it covers any pair, valid or not. Annex B's validity rule says which pairs are "displayed as flags": the valid ones "correspond to two-letter Unicode region subtags as defined in [CLDR], with idStatus = “regular”, “deprecated”, or “macroregion”. For macroregions, only UN and EU are valid." For an invalid pair, § 1.5 C3 adds a constraint, for an implementation that claims UTS #51: no display as a flag, "except for a fallback display depiction indicating the presence of an invalid sequence", and "A singleton emoji Regional Indicator may be displayed as a capital A..Z character with a special display". Which of the 417 non-RGI pairs (section 5.1) are valid is unmeasured: CLDR's region validity data is not pinned. |
| a tag sequence | Annex C: "displayed using a missing emoji glyph if feasible. ... (If the font technology permits, the missing emoji glyph can be overlaid on the tag_base character, thus occupying the same physical dimensions as if the sequence were supported.)" For an implementation that "does not support tag sequences at all": "(The tag characters are normally invisible, and thus only the base character displays.)" |

**Its conformance clauses on fallback.**

- **§ 1.5 C3.** An implementation must not display an invalid flag or tag
  sequence as emoji, "except for a fallback display depiction indicating the
  presence of an invalid sequence" ([[source: uts51-img-invalid-tag]]).
- **Annex C.1.1 and C.1.2**, which are scoped: "The following sections apply
  to conformant implementations that support at least one tag sequence."
  ([[source: uts51-r31]]; the same in [[source: uts51-emoji]], revision 29.)
  - C.1.1: "Images for unsupported valid emoji tag sequences must indicate that
    the sequence image is missing, by showing the base glyph with either a
    following “missing emoji glyph” or with an overlay “missing” glyph. The
    overlay glyph approach is recommended, so that the sequence would have the
    same width as if supported. A tag-unaware implementation (TU) will show
    just the base character."
  - C.1.2: "Images for invalid (but well-formed) emoji tag sequences must not
    be interpreted as if they were regular emoji tag sequences for a different
    appearance. They must instead indicate that there is something wrong with
    the sequence."
- **C.1.1's reason is the closest the standards come to an in-cell
  placeholder.** It recommends the overlay "so that the sequence would have
  the same width as if supported", which is a grid's constraint too (options
  e and a in section 6).

**baud meets C.1.1's second condition (measured): it supports at least one
tag sequence.** England, Scotland and Wales draw as one glyph on every shipped
emoji face ([[source: ad-obs-freq-emoji]], the RGI rows). Whether baud claims
UTS #51 conformance, the first condition, is the owner's call; the Unicode
Standard does not require it (§ 22.10, section 2.2). What baud draws for the
others is whatever the face's own shaping gives, since every face maps the tag
characters and the drop rule never fires
([[source: ad-obs-reserved-tags-vs]], [[source: ad-obs-noto-tag-flags]]):

| sequence | Apple Color Emoji 22.0d3e2 | Noto 2.042 | Noto 2.051 |
|---|---|---|---|
| black flag + `usca` (valid, not RGI) | the bare black flag (`u1F3F4`) | the California flag | Noto's white flag with a "?" |
| black flag + `ushuh` (invalid) | the bare black flag | the white flag with a "?" | the white flag with a "?" |

On macOS that is the tag-unaware display, which C.1.1 and C.1.2 say a
conformant, tag-supporting implementation must not show. On Noto the font's own
glyph meets them.

**When UTS #51 itself drops a modifier.** It says a modifier "should be
suppressed" when the base has no skin visible, and "may be suppressed" when the
base uses a non-realistic skin tone (§ 2.4.1). Both are properties of how the
base is drawn, not of missing font support.

**The core standard omits only default ignorables when unsupported.** It says
to ignore an unsupported ZWJ for display (§ 5.21.6). For the selector of an
unsupported variation sequence it says "display the variation base as if the
variation selector were not present" ([[source: unicode18-ch3]] § 3.6.2, the
paragraph after D57h). § 23.4 says the same in 17.0 and 18.0: "the variation
selector should be invisible and ignored" ([[source: unicode17-ch23]],
[[source: unicode18-ch23]]). It never omits a graphic character.

**ECMA-48 leaves this to the implementor.** Its Annex D (informative),
"Implementation-dependent features", lists "those matters left to the
implementors". Item 13 is "The action to be taken by a device if a control
function or a graphic character is received which the device cannot
implement, because of design limitation or temporary functional disablement"
([[source: ecma48]]; found with `scripts/extract-ecma48.py`, whose positive
control `framed` hits). So ECMA-48 neither requires nor forbids any option in
section 6. DEC's terminals display an error character, a reversed question
mark, for SUB ([[source: williams]]; [[source: vt510-rm-ch4]]), which
`docs/vt-coverage.md` records as out. That is about SUB, not about a character
the terminal cannot draw. The VT220 and VT510 chapters on received characters
say nothing about the second case; the phrase "error character" hits in both
as a control ([[source: vt220-rm-ch4]], [[source: vt510-rm-ch4]]).

**The mode 2027 draft** ([[source: mode2027]]; its README marks it ALPHA,
[[source: mode2027-readme]]) is the only terminal-specific text found that
prescribes a display. baud claims the mode: DECRQM `?2027` answers 3,
permanently set (`docs/vt-coverage.md`, line 234). Its semantics section opens
"The following set of semantics MUST be adhered to if this VT mode 2027 is
enabled", and it says:

> ZWJ emoji are required to be displayed as a single image with a width of 2
> grid cells. The alternate display of ZWJ emoji in a decomposed sequence of
> sub-images must not be used as a fallback as it will break cursor movement
> guarantees. If a ZWJ emoji cannot be rendered the display behavior is
> undefined - for example, a Unicode replacement character U+FFFD could be
> displayed instead.

The text forbids UTS #51's separate-emoji fallback, and it forbids sub-images
whatever their width. Its stated reason, cursor movement, does not narrow the
"must not". baud's core fixes the width whatever is drawn. Reading the text
more narrowly, so that it allows sub-images confined to the two cells, would
be a departure and the owner's call. Q3 marks confined sub-images as
departing. baud draws unconfined sub-images today (section 2.7).

**The Text Terminal Working Group** has not addressed missing glyphs in any
public report through L2/25-263, which is the latest. No file in the L2/2026
directory names the working group ([[source: unicode-l2-2026-index]], fetched
2026-10-08). Its reports are progress reports to the UTC on a design still in
progress ("the rules for scripts in this situation are going to need
iteration"). They are working-group reports, not a standard. L2/25-179 does
say that cell counts never depend on the font: "We have explicitly rejected
any mechanisms that require font-dependent feedback ... In other words, no
layout rules can depend on font data." ([[source: l2-25-179]] p. 1; also
[[source: l2-24-264]], [[source: l2-25-263]]). That sentence is about layout.
A placeholder that overdraws its cells changes no cell count, so this text
alone does not require a placeholder to fit them. Section 6's grid constraint
rests on other grounds.

**Reading.** AS_BUILT draws the base alone: a bare woman, a yellow hand, a lone
letter. UTS #51 describes a base-only display in two places: for a tag-unaware
implementation (Annex C), and for a modifier on a base that shows no skin or a
non-realistic one (§ 2.4.1). It describes none for a missing ZWJ component, a
missing modifier, or a missing second regional indicator.

### 2.5 Case C: nothing draws the cluster

§ 5.21.6 recommends "a “last-resort” glyph or a visible “missing glyph” box",
and § 5.3 says such a character "can only be indicated by some placeholder"
(both quoted above; chapter 5 is not normative). baud draws the primary face's
`.notdef` (stack.rs:500-509). Whether that is visible depends on the face:

- JetBrains Mono's `.notdef` is inked.
- SF Mono's glyph 0 has no outline ([[source: macos-sf-mono-regular]]).
- 56 of the 805 faces in this Mac's system and user font folders have no
  outline and no bitmap for glyph 0, and one more could not be read
  ([[source: ad-obs-notdef-survey]]).
- Noto Color Emoji's has neither an outline nor a bitmap (section 5.1).

The empty glyphs are the fonts' departures. In OpenType, glyph 0 "must be a
special glyph representing a missing character, commonly known as .notdef"
([[source: ms-opentype-cmap-20261008]]). The recommendations add "This glyph
should not be left without an outline as the user will only see what looks
like a space if a glyph is missing and not be aware of the active font’s
limitation" ([[source: ms-opentype-recom]]). The first is a "must" and the
second a "should". SF Mono departs from the "should". Noto Color Emoji is a
bitmap font, but its glyph 0 has neither an outline nor a bitmap, so it shows
the space the recommendation warns against (reading). Options b1 and b3 in
section 6 rely on the kept font following that recommendation.

A face's `.notdef` also differs from face to face and from one OS to another,
which the owner's same-pixels rule does not allow.

### 2.6 What should draw nothing

- **Default ignorables.** § 5.21.6 says of an unsupported ZWJ: "the best
  practice is to ignore it completely for display, without showing a
  last-resort glyph or a visible box in its place." baud's `Ignorable` is
  "drawn never" (buffer.rs:27-29), which matches 17.0. `Ignorable::of` is
  exactly `Default_Ignorable_Code_Point` (buffer.rs:37-46).
- **Not everything invisible is default-ignorable.** § 5.21.6 says that
  prepended concatenation marks "always have a visible display". The
  interlinear annotation characters (U+FFF9..U+FFFB) and the Egyptian
  hieroglyph format controls (U+13430..U+1343F) "should have a visible glyph
  display for fallback rendering". Private-use characters, surrogate code
  points and control characters, "when not interpreted and not displayable by
  normal rendering, should be displayed in fallback rendering with a fallback
  glyph" ([[source: unicode18-ch5]]). None of these is default-ignorable, so
  baud-font does not exempt them, and they are in scope for cases A to C. How
  baud's core cells them was not checked: which of them are width 0 and so
  attach backwards, and what happens to a prepended concatenation mark, which
  comes before its base.
- **18.0 adds an exception.** D57h: "A misplaced variation selector should be
  displayed visibly in text, rather than falling back to no visible display"
  ([[source: unicode18-ch3]] § 3.6.2). § 5.20 names variation-selector and tag
  sequences as a cross-prompt-injection vector: "misplaced format controls and
  variation selectors should rather fallback to visible display (as with
  missing glyphs)". § 5.21.6 adds that "sequences of misplaced tag characters,
  may constitute a security risk if not displayed visibly"
  ([[source: unicode18-ch5]]).
- **What baud does with a misplaced selector (measured,
  [[source: ad-obs-reserved-tags-vs]]).** A selector is misplaced when it does
  not immediately follow a variation base (D57h). `a` is a variation base
  (D57b), so `a` + VS1 is an unsupported variation sequence, not a misplaced
  selector. The two kinds of misplaced selector take different paths:
  - **After a cluster** (`a` + VS2 + VS1, D57h's example 3; `a` + U+0301 +
    VS1; `a` + VS16 + VS16): the core keeps the selector as an attachment. The
    renderer draws nothing for it, the same as for `a` + VS1. Misplaced tags
    (`a` + TAG a + CANCEL TAG) go the same way.
  - **After nothing** (the first character in a fresh `Term`, or after CR,
    D57h's example 1): the core refuses it and counts it in
    `Term::marks_dropped`. It is gone from the grid, and so from copy.
- **So baud matches 17.0 and departs from 18.0, the released version.** In
  the second case the core deletes the selector.
- **That refusal is not specific to selectors.** `attach` refuses every
  zero-width code point that has nothing to attach to: at column 0
  (paint.rs:763-769 at e0a611d, commented "Dropping it is correct: ... there
  is no cell for it"), and on an empty cell (paint.rs:783-786). Every width-0
  code point reaches `attach` (paint.rs:669-672), so this covers combining
  marks, ZWJ, selectors and tags alike. For a mark, this is a defective
  combining character sequence. D57: "Defective combining character sequences
  occur when a sequence of combining characters appears at the start of a
  string or follows a control or format character. Such sequences are
  defective from the point of view of handling of combining marks, but are not
  ill-formed." ([[source: unicode18-ch3]]). § 5.13 says "Defective combining
  character sequences should be rendered as if they had a no-break space as a
  base character", and D52 allows an isolated combining character to be
  presented "as if it were a base character" ([[source: unicode18-ch5]],
  [[source: unicode18-ch3]]). UAX #29's segments "may also include degenerate
  cases that are not thought of as grapheme clusters, such as an isolated
  control character or combining mark" ([[source: uax29]]): segmentation still
  yields the mark as a segment of its own, not nothing. So for a mark the
  refusal departs
  from § 5.13 and § 5.21.6 (best practice). For a selector it departs from
  D57h (18.0), and for tags from § 5.21.6 (18.0). For all of them, C7 reaches
  copy.

### 2.7 Does baud conform today?

| case | baud at e0a611d | the standard's best practice | verdict |
|---|---|---|---|
| A: a mark the kept face lacks | dropped; counted internally | a visible, generic fallback (§ 5.21.6); § 5.13's Show Hidden and Simple Overlap and D52 draw the mark's own glyph, so they apply only when some face has it | departs from best practice |
| B: a skin-tone modifier | dropped; the bare base draws | the base plus a missing-glyph indicator (UTS #51 § 2.4.1); a modifier is gc=Sk, a graphic character, so § 5.21.6 applies too | departs from § 5.21.6, and from the display UTS #51 describes (§ 2.4.1, whose "Unsupported" rows are descriptive) |
| B: a ZWJ component | dropped; a shorter emoji draws | separate emoji, perhaps with a visible glue (UTS #51 § 2.5, § 2.2); the component is a graphic character (§ 5.21.6) | departs from § 5.21.6, and from the display UTS #51 describes (§ 2.5, descriptive) |
| B: a reserved code point after a ZWJ (text from a later version, such as Emoji 18.0's U+1FAF9) | dropped; the base draws (measured on all three stacks) | not "blindly delete" it; a hex value, a box or a substitute glyph (§ 5.3, "Reserved and Private-Use Character Codes") | departs from § 5.3 |
| B: a flag's second indicator | never on a shipped stack: every emoji face maps all 26 indicators. A pair the face has no flag for draws two letters past its two cells (spill, section 5.1). The letters are the face's own glyphs for a lone indicator: on Apple Color Emoji each is a letter in a dashed square; on Noto Color Emoji 2.042 each is a bare letter with no square (U+1F1E6 U+1F1E6 at 13 and 26 px, measured by baud's renderer, 2026-10-08; Noto 2.047 and 2.051 were not rendered) | letters in dotted squares, or one missing-flag glyph (Annex B.1), for any pair without a flag; for an invalid pair, C3 adds that the depiction must indicate "the presence of an invalid sequence", singleton letters allowed (if baud claims UTS #51) | partly: on Apple Color Emoji the letters are B.1's first option, with dashes where B.1 says dotted, as section 3 reads Terminal.app's dashed letters; on Noto 2.042 they are B.1's letters without its square. For an invalid pair, on either face, they are C3's singleton letters. On both faces they overflow the cluster's cells and cover the next character |
| B: a ZWJ sequence under mode 2027, which baud answers permanently set | drawn as separate sub-images past the two cells, with or without a drop (sections 5.1 and 5.2; [[source: ad-obs-freq-emoji-rows]]). On Noto 2.042 and 2.047: 72 intact sequences with every code point mapped, the 59 intact wrestling sequences after the drop, 103 of the 106 the core alters (102 spill, one drops and spills) and 14 not fully qualified. On Noto 2.051: 103 core-altered and 4 not fully qualified. On macOS: 10 core-altered and 7 not fully qualified. The four bouncing-ball forms spill on every stack | "a single image with a width of 2 grid cells"; sub-images "must not be used as a fallback"; if it "cannot be rendered the display behavior is undefined" (mode 2027) | departs from a "must not" in a draft marked ALPHA, which baud claims at all times |
| B: an unsupported or invalid tag sequence | the face's shaping decides: the bare black flag on Apple Color Emoji, Noto's own "?" flag (section 2.4) | the base with a missing-glyph indicator, overlay recommended (C.1.1); something visibly wrong (C.1.2) | Apple: departs from C.1.1 and C.1.2, if baud claims UTS #51 conformance; Noto: matches |
| C: nothing draws the base, a lone code point | the primary face's `.notdef` | a last-resort glyph or missing-glyph box (§ 5.21.6) | matches when the primary's `.notdef` is inked; not under a configured SF Mono |
| C: nothing draws the base, a cluster with attachments (Hebrew at HEAD, Arabic on macOS, Devanagari and kana + U+3099 on Linux; section 5.3) | the primary face's one `.notdef`; the attachments are dropped and counted only internally. On Linux Hindi that includes consonants: 848 clusters, 1,631 code points left out, U+0915 among them ([[source: ad-obs-freq-replay]]) | a visible glyph "so that at least it is clear that there are characters present—and usually, how many are present"; for U+0915 itself, "Displaying nothing would give the user the impression that the character does not occur in the text at all" (§ 5.21.6) | partly: one box is visible, but it stands for several characters, against "how many are present" |
| default ignorable | nothing | nothing (17.0). In 18.0: a misplaced variation selector visibly (D57h, a chapter 3 "should"); misplaced tag characters and format controls visibly where misuse is a risk (§ 5.20, § 5.21.6, chapter 5 best practice) | matches 17.0; departs from 18.0 for misplaced selectors and misplaced tags, both measured drawing nothing (section 2.6) |
| the core: past eight attached code points, refused by the store, or any zero-width code point with nothing to attach to, at column 0 or on an empty cell: marks, ZWJ, selectors, tags (paint.rs:763-786 at e0a611d) | gone from the grid, so from copy. On screen the refused code point shows as nothing: 95 of the 106 RGI sequences the core alters draw a different emoji on macOS, and on Noto 102-103 spill (section 5.2) | C7: deletion "does modify the interpretation of the text"; for a defective combining sequence, § 5.13's no-break-space base; on screen, § 5.21.6 | copy: the conformance clauses reach this, if copy purports to reproduce the text. Screen: departs from § 5.21.6 and from the displays UTS #51 describes, as row B's drop does; a mark at column 0 departs from § 5.13. Under a 10646 claim, from § 2.3 c) (section 2.2). If baud claims UTS #51 display (C2a) for these sets, C2 requires these sequences to be declared unsupported |

Every row where baud shows nothing for a graphic character also departs from
ISO/IEC 10646 § 2.3 c), under a 10646 device claim (section 2.2). The rows do
not repeat that condition.

## 3. What terminals do

Other terminals are evidence of practice, not of what is right. Where one
departs from the text quoted in section 2, this section records the departure,
which may be a bug, and no option in section 6 takes it as a reason.

**How this was done.**

- Five terminals ran on this Mac (macOS 27.0, 26A5425a) with one test file
  ([[source: ad-obs-cases]]). Each case was printed as `|cluster|x|` under a
  `|q|x|` reference line, so drift shows as a column shift. The contact sheet
  is [[source: ad-obs-terminals-sheet]].
- The test code points:
  - U+1AB0 and U+1DD0: marks no installed font on this Mac maps;
  - U+1FAFF: reserved and Extended_Pictographic, so it joins after a ZWJ;
  - U+0378: unassigned;
  - U+10940 and U+323B0: no font here.
- How common such marks are here: of the 2,059 gc=Mn code points in UCD 17.0,
  `CTFontCreateForString(Menlo, cp)` returns LastResort for 770
  ([[source: ad-obs-mn-coverage]]). 533 of them are marks that are not
  ignorable. The other 237 are variation selectors, which are default-ignorable
  ([[source: dcp-data]]) and should draw nothing anyway.
- The other terminals in the first pass were read at pinned commits and not
  run. Their placement claims are inferred.
- **A supplement (2026-10-08) surveyed 18 more terminals and libraries at
  source, and one refuter checked it.**
  - Read at pinned commits: VTE (GTK3 and GTK4 builds), Konsole with Qt,
    xterm, urxvt, st, mlterm, mintty, xterm.js and three hosts that embed it
    (VS Code, Hyper and Tabby), Warp, and Blink 150.
  - Run on this Mac: Terminal.app again (with CPR, so the cursor's columns
    are measured), CoreText directly, and tmux and zellij under a pty.
  - Its cases add q + U+1AB2 (COMBINING INFINITY, which no font here maps),
    q + U+0316 + U+1AB2 + U+0301, bet + dagesh, woman + ZWJ + U+1FC00, skin
    tones after a base and after a letter, regional-indicator pairs, and
    U+1F0E0, U+0378 and U+1FC00 alone ([[source: ad-obs-supp-mux-cases]]).
  - Where the refuter corrected a row, the corrected row is what appears
    here. Rows it did not re-read say "as surveyed".
  - Blink 150 and CoreText are engines, so they are in section 4. tmux and
    zellij change what reaches baud, so they are in section 3.1.

**The yardstick for the Conforms? column.** Each verdict judges one case
against the text quoted in section 2:

- **A, a mark nothing draws on a base something draws.** § 5.21.6 recommends
  "visible, but generic, glyph display ... so that at least it is clear that
  there are characters present—and usually, how many are present". It names
  "Displaying nothing" as what to avoid. § 5.13's Show Hidden and Simple
  Overlap, and D52, apply when some face has the mark, and they draw the
  mark's own glyph.
- **B, a part of an emoji sequence the face lacks.** The UTS #51 displays
  quoted in section 2.4:
  - § 2.4.1: the base, then a missing-glyph indicator, for a modifier;
  - § 2.5: separate emoji when the system "does not have a corresponding
    single glyph", with the ZWJ ignored;
  - Annex B.1: letters in dotted squares, or one missing-flag glyph.

  Mode 2027 says a ZWJ sequence is "displayed as a single image with a width
  of 2 grid cells" and that sub-images "must not be used as a fallback". It
  binds a terminal that sets `?2027`. Whether a terminal sets it was read
  for two of those below, and baud answers it permanently set (section 2.4):
  - VTE lists the mode and cannot set it ([[source: vte-modes-56bad78]]
    modes.py:1052-1054);
  - mintty offers it, off by default ([[source: mintty-termout-0aea388]]
    termout.c:2741, 2926; [[source: mintty-config-0aea388]] config.c:242).
- **C, no face draws the cluster.** § 5.3: such a character "can only be
  indicated by some placeholder, such as displaying the hexadecimal value of
  the code point, or other meaningful information, such as the script", or
  the Last Resort Font. § 5.21.6: "a “last-resort” glyph or a visible
  “missing glyph” box".
- **Three judgements recur, so each is made once here.**
  - **A placeholder that leaves the cluster's cells** is visible, which is
    what § 5.21.6 asks for. What departs is the overdraw: the placeholder
    covers the next character, and § 5.21.6's "Displaying nothing would give
    the user the impression that the character does not occur" then applies
    to that character.
  - **One placeholder in place of a whole cluster** is visible. Where a face
    draws the base, though, it hides a character that could be shown, and one
    glyph stands for several characters, against "how many are present". This
    is marked "partly". Section 6 judges baud's options a, b1, b2, c and f the
    same way.
  - **"Not formed"**: the terminal's width tables never join a ZWJ, modifier
    or flag sequence, so each part draws in its own cells, even where the font
    has the combined glyph. UTS #51 § 2.5's separate display is for a system
    "that does not have a corresponding single glyph", so it does not describe
    this display. § 2.11.5 says a grapheme cluster is "treated as a single
    unit" (reading: departs). A part no face has is then judged as case C.
- The verdicts are follows, partly (one clause followed and another departed
  from, both named), departs, and unknown. "(inferred)" marks a verdict on
  behaviour that was itself inferred.
- Where a terminal shows nothing, it also departs from ISO/IEC 10646 § 2.3
  c), if it claims 10646 device conformance (section 2.2). No terminal here
  was checked for such a claim, so the verdicts do not repeat it.

| terminal | how known | A: a missing mark, base drawn | B: a missing emoji part | C: nothing draws | base kept | inside the cluster's cells | tells the user | Conforms? |
|---|---|---|---|---|---|---|---|---|
| Terminal.app 2.15 (FiraCodeNF 12) | observed ([[source: ad-obs-terminals-sheet]]), then measured again with CPR by the supplement ([[source: ad-obs-supp-tapp-crop]], [[source: ad-obs-supp-tapp-zoom]], [[source: ad-obs-supp-tapp-cpr]]); closed source, so the mechanism is inferred | CoreText's LastResort "?" box in the next cell or two, while the cursor moves one column. With q + U+1AB0 + U+0301 the acute lands about two cells right. Where one font covers the whole cluster, CoreText moves the cluster there (q + U+0316 to Helvetica), and the below-mark then draws at the top of the next row. A mark only a fallback face has (q + U+05BC) lands on the cell boundary | the woman, then the box. Widths are a sum per code point with ZWJ as one column: woman + ZWJ + U+1FC00 takes 4 columns, and woman + ZWJ + laptop takes 5 although its one glyph fills 2. A skin tone after a non-base draws a swatch; a regional-indicator pair with no flag, two letters in dashed boxes | the same box: one template glyph for every code point, assigned or not | yes | no: the box covers the next character, and a below-mark reaches the next row | no | **A** partly: the box is § 5.21.6's "visible “missing glyph” box", but it covers the next character. **B** partly: the base, then a box, is the shape of § 2.4.1 and § 2.5, but outside the cells; the dashed letters are Annex B.1's first option. **C** partly: visible, and over the next cell |
| iTerm2 3.7.3 (Monaco 12) | observed; read at [[source: iterm2-character-source-20c5b05]], [[source: iterm2-fonttable-20c5b05]] (that commit is not verified as 3.7.3, and which of its two renderers drew the capture is unknown) | the "?" box overlaid on the base, about 1.8 cells wide | the woman in 2 cells, then a box over the next cell | the box | yes | no | no | **A** partly: a visible box (§ 5.21.6), wide enough to cover the next character. **B** partly: the woman kept with a box for the missing part (§ 2.4.1, § 2.5), over the next cell. **C** follows § 5.21.6 (a box); its width was not recorded |
| WezTerm (nix build; its `wezterm-font` tree equals 3725482) | observed; [[source: ad-obs-wezterm-lsfonts]], [[source: wezterm-shaper-harfbuzz]] | JetBrains Mono's `.notdef` one cell right; a later acute stays on the base | the whole cluster falls to face 0: `.notdef`, space, `.notdef`, so the woman is lost | `.notdef` | only when face 0 has the base (中 + U+1AB0 draws two boxes) | no | yes: a "Font problem" toast naming the code points, on by default, once an hour ([[source: wezterm-font-lib-3725482]], [[source: wezterm-config-3725482]]) | **A** partly: a visible box (§ 5.21.6), one cell right, over the next character. **B** partly: one box per missing code point, but the woman, which a face draws, is a box too; § 2.4.1 and § 2.5 keep the base. **C** follows: JetBrains Mono's `.notdef` is inked. The toast is not covered by any quoted text |
| Ghostty 1.3.1 (embedded JetBrains Mono) | observed; [[source: ghostty-run-v131]] run.zig:229-262 | the whole cluster becomes one U+FFFD | the same | the same | no | yes | no | **A** partly: U+FFFD is visible and generic (§ 5.21.6), but it hides a base a face draws, and one glyph stands for every code point in the cluster. **B** partly: the same; for a ZWJ sequence it is mode 2027's example glyph. **C** follows for one code point (§ 5.3's "some placeholder") |
| kitty 0.49.2 (Menlo) | observed; [[source: kitty-fonts-fa9f8da]] fonts.c:2141-2147, [[source: kitty-decorations-fa9f8da]] | kitty's own hollow box, one per cell | depends on order: two boxes if this is the process's first emoji; otherwise Apple Color Emoji is reused without a coverage check, and U+1FAFF vanishes ([[source: ad-obs-kitty-a]], [[source: ad-obs-kitty-a-log]], [[source: ad-obs-kitty-b]], [[source: ad-obs-kitty-b-log]], [[source: ad-obs-cases-e1first]]) | the box, per cell | no (yes in the reuse case) | yes | no | **A** partly: a visible box per cell (§ 5.21.6), but it replaces a base a face draws. **B** in the first state, partly, as A; in the reuse state, departs: U+1FAFF shows as nothing (§ 5.21.6), and it is a reserved code point § 5.3 says not to "blindly delete". **C** follows: a box per cell |
| Alacritty | read, 29dc553: zero-width characters are fetched with `show_missing` false ([[source: alacritty-renderer-text-mod-29dc553]] text/mod.rs:166-168), and that arm loads `Default::default()`, an empty glyph ([[source: alacritty-renderer-text-glyph-cache]], same bytes at 29dc553) | a mark no face has draws nothing | never formed: the width comes from unicode-width, a width-0 code point joins the previous cell with `push_zerowidth`, and anything else starts cells of its own, so the ZWJ rides on the woman's cell and the laptop takes its own ([[source: alacritty-term-term-mod]], the same bytes as at 29dc553, mod.rs:1062-1084) | the primary's `.notdef` | yes | yes | no | **A** departs: nothing is drawn (§ 5.21.6, "Displaying nothing"). **B** not formed; a part no face has is judged as C. **C** follows when the primary's `.notdef` is inked; departs where it is blank |
| Windows Terminal | read; [[source: wt-atlasengine-6676c6f]] AtlasEngine.cpp:891-894, 1189-1236 | one U+FFFD per column step of an unmapped range; a range ending inside a cell draws nothing | not read | U+FFFD | no | yes | no | **A** departs where nothing is drawn (§ 5.21.6); elsewhere partly: U+FFFD is visible, but it replaces the base and counts columns, not code points. **B** unknown. **C** follows (§ 5.21.6), one U+FFFD per column |
| foot / fcft | read; [[source: fcft-1ff7e79]] fcft.c:2388-2393 | the primary face shapes the cluster: the base, then `.notdef` | `.notdef` beside the woman (inferred) | `.notdef` | yes | no (inferred) | no | **A** partly: a visible box (§ 5.21.6) after the base, over the next cell (inferred). **B** partly: the base and a box (§ 2.4.1's shape), outside the cells (inferred). **C** follows when the primary's `.notdef` is inked |
| Contour | read; [[source: contour-openshaper-fbcce4b]], [[source: contour-textrenderer-fbcce4b]] | every glyph no font renders becomes the primary face's U+FFFD, per glyph; the base stays (inferred: it lands one cell right) | the same | U+FFFD | yes | no (inferred) | no | **A** partly: one U+FFFD per missing code point keeps "how many" (§ 5.21.6), but it lands outside the cell (inferred). **B** partly: the same; U+FFFD is mode 2027's example glyph, but here it sits beside the woman, not as one image (inferred). **C** follows (§ 5.21.6) |
| Rio | read; [[source: rio-grid-lib-59203cc]], [[source: rio-macos-59203cc]] | the face comes from the base alone; issue #1519, "diacritical marks do not render", is open ([[source: rio-issue-1519]]) | not read | macOS discovery does not filter LastResort (inferred: it then pre-empts later fallbacks) | yes | unknown | no | **A** unknown for a mark no face has. The open issue reports marks that do not render, which would depart (§ 5.21.6). **B** unknown. **C** unknown; if LastResort is drawn, it is § 5.21.6's “last-resort” glyph (inferred) |
| VTE 0.85 dev, GTK3 build (GNOME Terminal 3.60.0 stable) | read at 56bad78 with Pango 8e74c27, re-read by the refuter; not run. [[source: vte-cc-56bad78]], [[source: vte-fonts-pangocairo-cc-56bad78]], [[source: vte-drawing-context-56bad78]], [[source: pango-itemize-1-57-1]] (the same bytes as at 8e74c27), [[source: pango-shape-c]] | Pango picks a font per character, with no cluster rule (itemize.c:762-831). A mark in a fallback face is a run of its own, shaped alone, starting at q's advance and not anchored to q; GTK3 draws every run (fonts-pangocairo.cc:510-560). With no face, Pango draws its hex box (0316) in the primary face, at an advance of its own (shape.c:159-187); right of q on JetBrains Mono (inferred). A mark at column 0 is dropped unless the previous row soft-wrapped (vte.cc:3446-3467) | never formed: only a width-0 code point joins a cell (vte.cc:3435-3481), so the laptop, a skin tone and a second regional indicator each start a cell (by the surveyor's decode of VTE's width table). ZWJ draws nothing. A part no face maps gets a 6-digit hex box in the primary face | Pango's hex box: 4 digits for the BMP, 6 for astral code points, one per code point; never U+FFFD, never Last Resort | yes | no: left-aligned, overflowing right, clipped only to the row ([[source: vte-drawing-context-56bad78]] drawing-context.cc:150-190) | the digits name the code point; nothing is counted | **A** partly: the hex box is § 5.3's "hexadecimal value of the code point", one per code point, but it overflows the cell (inferred); the column-0 drop departs (§ 5.21.6). **B** not formed; a part no face has follows § 5.3 (hex). **C** follows (§ 5.3), overflowing right when wider than its cells |
| VTE 0.85 dev, GTK4 build (Ptyxis, GNOME Console, GNOME Terminal master) | read as the GTK3 row, re-read by the refuter; [[source: vte-fonts-pangocairo-cc-56bad78]], [[source: vte-fonts-pangocairo-56bad78]], [[source: vte-drawing-gsk-56bad78]] | Pango itemizes as in GTK3, but the GTK4 branch keeps only the first run, line->runs->data (fonts-pangocairo.cc:563-572). So a mark Pango puts in a fallback face is dropped silently even though that face draws it: q + U+0316 draws a bare q. VTE's own header gives this case, "some diacretics is not available in the font chosen for the base character", as the reason the multi-run path exists (fonts-pangocairo.hh:76-81). With no face, the hex box draws only if it lands in q's run | as GTK3; a missing part alone in its cell draws its hex box | Pango's hex box | yes | no: the GTK4 context does no clipping at all (drawing-gsk.hh:66-71) | the digits, when drawn; nothing is counted | **A** departs: a mark a face can draw shows as nothing (§ 5.21.6; § 5.13 and D52 would draw its glyph). This is VTE's departure from its own design, a bug by its own header comment. **B** not formed; a lone missing part follows § 5.3. **C** follows (§ 5.3) |
| Konsole (master, Qt 6 on Linux with fontconfig) | read at a88fae0 with qtbase 2b9b95d, re-read by the refuter with five corrections; not run. [[source: konsole-screen-a88fae0]], [[source: konsole-terminalpainter-a88fae0]], [[source: konsole-terminalfonts-a88fae0]], [[source: qtbase-qfontengine-2b9b95d]], [[source: qtbase-qharfbuzzng-2b9b95d]], [[source: qtbase-qtextengine-2b9b95d]] | Only Mn, Mc, Lo, Cf, emoji, U+20E3 and the soft hyphen join a cell; any other width-0 code point, an enclosing mark other than U+20E3 among them, is dropped uncounted (Screen.cpp:1108-1115). A mark at column 0 is dropped unless it is Hangul (1139-1148). Qt merges fonts per character, so a mark in a fallback face is shaped alone at q's advance (inferred). With no face, glyph 0 of the primary face is painted (qharfbuzzng.cpp:470-484; Qt marks a glyph not to print only for line and paragraph breaks, the soft hyphen and an empty shaping result, qtextengine.cpp:1325-1358): its `.notdef`, about a cell right of q on JetBrains Mono (inferred) | formed: an emoji after a ZWJ, a modifier after a base in U+261D..270D or U+1EFFF..1FAFF (a range test, not Emoji_Modifier_Base), a second regional indicator. A cell holds at most 10 code points; the 11th is dropped silently (Screen.cpp:1244-1250). Emoji cells use the profile's emoji font, Noto Color Emoji when none is named (TerminalFonts.cpp:39-41; TerminalPainter.cpp:1209-1216). A part only another face has is a separate run after the woman; a part no face has draws the emoji face's `.notdef` | the face's own `.notdef` at the cell's x, with its advance; Qt's box engine is skipped as a fallback (qfontengine.cpp:2240-2244) | yes | no: the clip is commented out (TerminalPainter.cpp:1235-1240) | no; an off-by-default debug line | **A** partly: a visible box where the primary's `.notdef` is inked (§ 5.21.6), a cell right (inferred); the enclosing-mark and column-0 drops depart (§ 5.21.6). **B** partly: a part another face has follows § 2.5's separate display, outside the cells; a part no face has draws the emoji face's `.notdef`, which on Noto Color Emoji has no outline and no bitmap (section 5.1), and so departs (inferred); the 11th code point's drop departs. **C** follows where the face's `.notdef` is inked |
| xterm, patch 412 (Xft and core-font paths) | manual, [[source: xterm-manpage-411]]; read at bccd466 ([[source: xterm-411-util-c]], the same bytes; [[source: xterm-fontutils-c-bccd466]]; [[source: xterm-charproc-c-bccd466]]), re-read by the refuter; not run | no clusters and no shaping. A mark takes one of the base cell's combining slots, 2 by default (charproc.c:794); a third is discarded silently. Precomposition is on by default (charproc.c:789). Each mark is overstruck at the base cell's x in a face chosen for it alone. Xft: if no font maps the mark, a dashed one-cell rectangle is drawn over the letter (util.c:4796-4800, 4840-4856; fontutils.c:4397-4422, commented "bounding rectangle, for debugging"), while showMissingGlyphs is on, its default (charproc.c:587). Core fonts: unless the gate at util.c:4995-5000 opens (assumeAllChars, on by default, shuts one of its conditions; charproc.c:584), the missing-glyph routine gets width 0 and draws nothing (util.c:4226-4247, 4380-4400) | never formed: ZWJ is dropped at parse, and each emoji takes its own cells | xterm's own rectangle, one cell (Xft) or the character's width (core fonts); before it, Xft may substitute an ASCII lookalike. The manual: "showMissingGlyphs ... display a box outlining places where a character has been used that the font does not represent. The default is "true"." | yes | yes (Xft: inset inside one cell; a two-column miss is boxed in its left cell only, read) | no | **A** Xft follows: a box per missing mark, inside the cell, over the letter (§ 5.21.6). Core fonts depart: nothing is drawn. The third mark's discard departs. **B** not formed. **C** follows (§ 5.21.6), inside the cell |
| urxvt 9.31 | read at the exg/rxvt-unicode mirror's 5865a43; the draw path re-read by the refuter; not run. [[source: urxvt-rxvtfont-5865a43]] | a mark is precomposed if it can be; otherwise base and mark form a composite drawn by the built-in support font: the base, then the mark overstruck in the base's font if that font has it ("prefer font of first character, for no good reasons"), else in the mark's own (rxvtfont.C:446-469). If no font maps the mark, the support font's default case draws a solid rectangle inset 2 px over the letter (rxvtfont.C:474-492, 1896-1900) | never formed: libc's widths split the sequence. A ZWJ that no listed font maps would be boxed over its base (inferred: it depends on font coverage) | the support font's solid rectangle, inset 2 px within the code point's cells | yes | yes | no | **A** follows: a box per missing mark, inside the cell, over the letter (§ 5.21.6). **B** not formed. A boxed ZWJ would depart from § 5.21.6's "ignore it completely for display, without showing a last-resort glyph or a visible box" (inferred). **C** follows (§ 5.21.6), inside the cells |
| st | read at 04ce0d6; re-read by the refuter; not run. [[source: st-st-c-04ce0d6]], [[source: st-x-c-04ce0d6]] | no combining support. A width-0 rune is written into the cell after its base and the cursor does not move (st.c:2406-2411, 2520-2542), so the next rune overwrites it. A mark survives only if nothing follows it on the line, and then it draws alone in the next cell | never formed: the ZWJ is written and then overwritten by the laptop | per rune: the primary font, a fallback cache, then fontconfig's best match; with no cover, that font's glyph 0 (x.c:1289-1360; the fontconfig and libXft readings are the surveyor's) | yes | no: clipped to the attribute run | no | **A** departs: the mark is overwritten, so it shows as nothing (§ 5.21.6), or it shows in the wrong cell. **B** not formed. **C** follows where the matched font's `.notdef` is inked (inferred) |
| mlterm 3.9.5 (cairo and xft backends) | read at 405185a, as surveyed; not re-checked, not run. [[source: mlterm-vt-char-h-405185a]], [[source: mlterm-ui-font-ft-405185a]] | up to 7 marks per base (vt_char.h:13); more are refused, uncounted. With OT layout off, the default, each mark keeps its own face and is overstruck at the base's x. Cairo: a code point no family has is added to the primary font's charset "To avoid to search it again" and draws the primary's `.notdef` over the letter (ui_font_ft.c:1434-1442). Xft: no fallback, so the configured font's `.notdef` | formed only as pictures: a regional-indicator pair or base + modifier combines when an image exists under emoji_path; otherwise each part draws from a font in its own cell. ZWJ draws nothing | the primary's (cairo) or configured (xft) `.notdef` | yes | marks at the base's x; the clipping option's default was not checked | no | **A** follows where the primary's `.notdef` is inked (§ 5.21.6), over the letter; the eighth mark's refusal departs. **B** not formed except as images. **C** follows where the `.notdef` is inked |
| mintty 3.8.3 (Windows) | read at 0aea388, as surveyed; not re-checked, not run. [[source: mintty-wintext-0aea388]], [[source: mintty-termout-0aea388]] | precomposed only when the font has the precomposed glyph. The base is drawn by Uniscribe with fallback; each mark separately with GDI, overstruck at the base's x and clipped to the base cell (wintext.c:3939-3986). What GDI draws for a mark no font covers is closed source | matched against mintty's own emoji images at paint time; without an image, drawn as text: the ZWJ a mark on the woman, the laptop in its own cell. With `?2027` set, the character after a ZWJ takes width 0 (termout.c:1136-1148) | unknown: delegated to Uniscribe, closed source | yes | yes: clipped | an application can ask: OSC 7771 echoes back only the characters the font has (termout.c:4948-4970; wintext.c:4839-4875) | **A** unknown. **B** unknown for a missing part. **C** unknown. The query is not covered by any quoted text |
| xterm.js 6 and its hosts: VS Code 3a7c8e0, Tabby 467ce8e (6.0.0), Hyper 30ec0a2 (5.3.0, unreleased canary) | read at c58ea36, the widths re-read by the refuter; the hosts as surveyed. [[source: xtermjs-unicodev11-c58ea36]], [[source: xtermjs-textureatlas-c58ea36]], [[source: vscode-terminalconfiguration-3a7c8e0]], [[source: tabby-xtermfrontend-467ce8e]], [[source: tabby-utils-467ce8e]], [[source: tabby-xterm-css-467ce8e]], [[source: hyper-package-30ec0a2]], [[source: hyper-config-default-30ec0a2]] | xterm.js asks no font anything: the cell's string goes to one fillText in the configured family list (TextureAtlas.ts:535-538, 738-742), and the browser decides; in Electron that is Blink 150 (section 4). If no face covers the cluster, Blink commits the primary's glyphs: the letter, then the primary's `.notdef` at its own advance, a cell right on JetBrains Mono (inferred). Which face draws depends on each host's family list: Hyper's (config-default.json:6), or Tabby's, which puts a bundled Source Code Pro before `monospace` (utils.ts:23-32; xterm.css:3-6) | never formed under the unicode11 tables all three hosts load (VS Code: terminalConfiguration.ts:544-551; Tabby: xtermFrontend.ts:205-208; Hyper: package.json:65-73): ZWJ and VS16 have width 0 and join the previous cell, a skin tone is 2 columns, a regional indicator 1 (UnicodeV11.ts:64-67, 80-82, 152-159). Only an opt-in addon builds clusters | the browser's: Blink and WebKit draw the primary font's `.notdef`, Gecko a hex box | yes (Blink) | no: no renderer confines a fallback glyph to its cell | no | **A** partly in Blink: a visible box (§ 5.21.6) a cell right of the letter (inferred), over the next character; where one family covers the whole cluster the mark is drawn. **B** not formed. **C** follows where the primary's `.notdef` is inked, or with Gecko's hex box (§ 5.3); not confined |
| Warp | read at 325d4d4, re-read by the refuter. [[source: warp-cell-glyph-cache-325d4d4]], [[source: warp-grid-renderer-325d4d4]], [[source: warp-fonts-325d4d4]], [[source: fontkit-core-text-a04b225]] | the mark is dropped and the letter kept, even when a face could draw the mark. A cell with a width-0 character is laid out as a string and accepted only if the first run is exactly one glyph; otherwise the first character's glyph is drawn alone (cell_glyph_cache.rs:70-92). One glyph per cell, or none (grid_renderer.rs:1790-1797). A mark survives only if a face composes it with its base into one glyph | never formed: the ZWJ rides on the woman's cell, which draws one composed glyph or else the woman alone; the laptop draws in its own cell | nothing: font-kit returns no glyph for glyph 0 (core_text.rs:294-307), and the renderer skips the cell | yes | one glyph at the cell's origin | no; a log line in debug builds only | **A** departs: a mark shows as nothing whenever it does not compose with its base into one glyph, even when a face draws it (§ 5.21.6; § 5.13 and D52 would draw it). **B** not formed; a part no face has is blank, which departs. **C** departs: a blank cell (§ 5.21.6, "Displaying nothing"; § 5.3, "can only be indicated by some placeholder") |

What the table shows:

- **Two terminals keep the base and draw a placeholder for a missing mark
  inside the base's cell, and both were read, not run.** xterm's Xft path
  draws a dashed one-cell rectangle over the letter, and urxvt a solid one
  inset 2 px. Each draws one box per missing mark, which follows § 5.21.6.
  - Terminal.app, iTerm2, WezTerm, foot, Contour, both VTE builds and Konsole
    also keep the base, but their placeholder leaves the cell. This was
    observed for the first three and read or inferred for the rest.
  - Ghostty, kitty and Windows Terminal stay inside the cells, but lose the
    base.
  - mintty clips its marks to the base cell, but what GDI draws there for a
    mark no font covers is unknown.
- **Eight show nothing for a mark in some state, as baud does:**
  - Alacritty, for a mark no face has;
  - kitty in its reuse state, for a ZWJ component;
  - VTE's GTK4 build, for a mark only a fallback face draws;
  - xterm's core-font path;
  - st, because the next character overwrites the mark;
  - Warp, for any mark that does not compose with its base into one glyph;
  - Windows Terminal, for a range that ends inside a cell;
  - Konsole, for an enclosing mark other than U+20E3.

  Each departs from § 5.21.6. Only baud counts the drop, and only
  internally.
- **Pango's hex box in VTE, and Gecko's under xterm.js in Firefox, name the
  missing code point.** WezTerm tells the user with a toast, and mintty lets an
  application ask. No other terminal here counts or reports a miss.
- **Eight of the twenty never build the emoji clusters of case B.** Both VTE
  builds, xterm, urxvt, st, xterm.js under unicode11, Warp and Alacritty split
  them by width before any font is asked, so case B does not arise in them.
  mintty and mlterm join a sequence only to draw an image of their own.
  Konsole and the terminals observed in the first pass form them.
- **Blanks that remain in the table:** Windows Terminal's B (not read); Rio's
  A, B and C; mintty's A, B and C; the width of iTerm2's C box (not
  recorded); and mlterm's clipping default (not checked).
- **Faces are chosen per code point in Pango, Qt, xterm, urxvt, mlterm and
  st, and per cluster in Blink, CoreText and baud's Q23 rule.** So in the
  first group a cluster can be split across faces in both directions.
- **Storage limits drop attachments before any font is asked, in practice as
  in baud's core (section 5.2):**
  - xterm keeps 2 marks per cell by default;
  - mlterm keeps 7;
  - Konsole keeps 10 code points;
  - tmux keeps 32 bytes (section 3.1);
  - Warp keeps 256 bytes ([[source: warp-cell-325d4d4]] cell.rs:20-33,
    218-249).

  A mark with nothing before it is dropped by VTE and Konsole (Konsole
  excepts Hangul), by tmux, and by baud's core, which drops any zero-width
  code point there (section 2.6). Each of these drops deletes text, which C7
  reaches if a copy purports to reproduce the text (section 2.2). Q7 is
  decided on that clause, not on these terminals.
- **kitty reaches baud's AS_BUILT result by accident.** Once Apple Color Emoji
  is loaded, the woman draws alone and the missing component vanishes. Its
  cache keyed on cell text then keeps whichever result came first.
- **Ghostty, kitty, WezTerm and Blink reject macOS's LastResort font in
  fallback.** The reasons they give are about the fallback search, not about
  how a missing character should look:
  - WezTerm: "Always exclude a last resort font, as it has placeholder glyphs
    for everything" ([[source: wezterm-locator-coretext-3725482]],
    [[source: wezterm-commit-e23ab47]], [[source: wezterm-issue-4877]]).
  - Ghostty: "we need to ignore the last resort font, which just has
    replacement glyphs for all codepoints" ([[source: ghostty-commit-1aa932f]],
    [[source: ghostty-issue-2499]], [[source: ghostty-discovery-8f0dd37]]).
  - kitty added its check in 2018 for a fallback-cache bug: "Fix a character
    that cannot be rendered in any font causing font fallback for all
    subsequent characters that cannot be rendered in the main font to fail"
    ([[source: kitty-commit-55f457c]], #799). The 2019 commit 1fcd6e1 moved
    it into a function of its own, is_last_resort_font, and added a manual
    search for private-use code points ([[source: kitty-commit-1fcd6e1]],
    [[source: kitty-core-text]]).
  - Blink 150 drops a substitute font whose PostScript name is LastResort
    ([[source: chromium150-font-cache-mac]] font_cache_mac.mm:111-115,
    140-145).
- **macOS's LastResort is a stub, not the Unicode Last Resort Font.**
  `/System/Library/Fonts/LastResort.otf` on macOS 27.0 is 2,416 bytes with 7
  glyphs. One template glyph, a "?" in a rounded box 1.10 em wide, stands for
  every code point ([[source: macos-lastresort-otf]], measured with a font
  reader). The refuter read its format 13 cmap the same way: U+0000-D7FF and
  U+E000-10FFFF map to glyph 4, the template, and the two surrogate ranges to
  glyphs 5 and 6. So Terminal.app draws § 5.21.6's box, not § 5.3's
  script glyph. Whether earlier macOS releases shipped per-block glyphs is
  unknown.

### 3.1 What reaches baud through a multiplexer

tmux 3.7c and zellij 0.44.3 sit between the application and baud, and both
rewrite clusters before baud sees them. They are not drawing a placeholder, so
the clauses that reach them are C7 ("Replacement or deletion of a character
sequence that the process cannot or does not interpret does modify the
interpretation of the text") and § 5.21.6 for the screen behind them. Both ran
under a pty with the supplement's cases, using the harness and probe in
[[source: ad-obs-supp-mux-harness]] and [[source: ad-obs-supp-mux-cprprobe]].

| multiplexer | how known | a mark | an emoji sequence | a code point no face draws | Conforms? |
|---|---|---|---|---|---|
| tmux 3.7c (utf8proc 2.11.3) | read at e476c12 and re-read by the refuter ([[source: tmux-screen-write-e476c12]], [[source: tmux-utf8-combined-e476c12]], [[source: tmux-h-e476c12]]); run ([[source: ad-obs-supp-tmux-out]], [[source: ad-obs-supp-tmux-cpr]]) | passed intact, delivered as a rewrite: the base, then for each width-0 code point a BS and the whole cell so far. Dropped in three places: at column 0 (screen-write.c:2685-2687, whose caller returns at 2494-2496); U+3164 always (2667-2669); and past 32 bytes a cell (tmux.h:698-711), where a code point is not combined (screen-write.c:2725-2727). Measured by the supplement, not re-run: an a with 20 acutes goes out live with all 20, and the grid keeps 15 for a redraw | any two regional indicators combine, valid pair or not, and a skin tone combines only after one of 72 listed bases (utf8-combined.c:104-200), so 🍎🏽 and q🏽 stay two cells (widths 4 and 3). ZWJ sequences of 32 bytes or fewer pass whole at width 2; the 41-byte skin-toned family is cut (width 4) | passed unchanged | departs where it drops: the column-0, U+3164 and over-cap code points are deleted (C7, if tmux purports not to modify the text), and the screen behind it then shows nothing for them (§ 5.21.6) |
| zellij 0.44.3 | read at 55a2121 and re-read by the refuter ([[source: zellij-grid-55a2121]], [[source: zellij-terminal-character-55a2121]]); run ([[source: ad-obs-supp-zellij-out]], [[source: ad-obs-supp-zellij-cpr]]) | stripped: a cell holds one char (terminal_character.rs:920-925), and add_character returns on width 0, with the comment "This breaks unicode grapheme segmentation" (grid.rs:1914-1924). Measured: q + U+0316 arrives as q, bet + dagesh as bet, e + U+0301 as e | ZWJ, VS16 and U+20E3 are stripped: woman + ZWJ + laptop arrives as two emoji (zellij width 4), 1️⃣ as 1, ❤️ as ❤. Regional indicators and skin tones pass, each in a cell of its own | passed unchanged | departs: every width-0 code point is deleted before baud sees it (C7, if zellij purports not to modify the text), and the screen shows nothing for the marks (§ 5.21.6) |

What this means for baud:

- **Behind zellij, cases A and B never reach baud.** baud receives bare
  letters and separated emoji, so no renderer option changes what is shown
  there.
- **Behind tmux, the two width models are predicted to disagree** (inferred;
  not measured in baud). A skin tone after a base tmux does not list is two
  cells to tmux and one two-column cluster to a terminal that follows UAX #29.
  The rest of that line would then drift until tmux's next absolute move. The
  same holds for zellij's 👍🏽. Feeding the captured streams through baud's
  core would settle it (section 5.5).
- **tmux's live stream for the cut family** sends the first 30 bytes, then the
  ZWJ as a zero-width cell, the boy, and `BS BS` with the boy and his tone.
  If baud joins the live ZWJ and boy into the family, the backspaces land on
  the family's cell and the toned boy overwrites it (inferred).

## 4. What engines do (measured here)

**The text that governs the browser engines below is CSS Fonts.** It binds
CSS user agents, not terminals. CSS Fonts Module Level 3 is a W3C
Recommendation ([[source: w3c-css-fonts-3]]). Its § 5.2 says: "If a
particular character cannot be displayed using any font, the user agent
should indicate by some means that a character is not being displayed,
displaying either a symbolic representation of the missing glyph (e.g. using a
Last Resort Font) or using the missing character glyph from a default font."
§ 5.3, "Cluster matching", step 1 picks a font that supports the whole cluster,
which is baud's Q23 rule (reading). Step 3 says what to do when no font does: "use the
matching sequence from step 1 to determine the longest sequence that is
completely supported by a font in the font list and attempt to match the
remaining combining characters separately using the rules for single
characters." That is option h (section 6), with § 5.2's missing-glyph
indication as the fallback for a mark no font has.

**The test cases** were ab + cluster + cd, for the three cases and their
no-font variants:

- U+1AB0;
- woman + ZWJ + U+1FC00, which is reserved and Extended_Pictographic in UCD
  18.0, so the cluster still joins ([[source: ucd18-emoji-data]], line 1301);
- U+10D40 (Garay), U+0378 and U+E000.

| engine | how known | a font covers the whole cluster | no font covers it | the missing code point draws as |
|---|---|---|---|---|
| CoreText (macOS 27.0) | measured: [[source: ad-obs-coretext-runs]], [[source: ad-obs-coretext-sheet]], [[source: ad-obs-ctline-fira]] | the whole cluster moves to that font, base included: q + U+0316 + U+0301 to Menlo, woman + ZWJ + laptop to the system Apple Color Emoji | the cluster splits: the covered part keeps its font, and the uncovered code point is a separate LastResort run with a spacing advance of 1.1 em (44 at 40 pt), except 1.7 em (68) when the missing mark comes before another mark (d2) | the stub's one "?" box |
| WebKit (WKWebView) | measured: [[source: ad-obs-webkit]]; read: [[source: webkit-complextext-coretext]], [[source: webkit-fontcascade-coretext]], [[source: webkit-fontcascadefonts]] | the first family font that renders the whole sequence, else the base's font, else the system | the base keeps its face; a run CoreText hands back in LastResort is redrawn with the primary font | the primary font's `.notdef` |
| Chromium (headless Brave 1.95.102) | measured: [[source: ad-obs-chromium]]; read: [[source: chromium-harfbuzz-shaper]], [[source: chromium-font-fallback-iterator]], [[source: chromium-font-cache-mac]] | the whole sequence goes to the font that covers it | the whole unresolved cluster is committed in the first candidate font, so a base that font lacks is lost too; whether the woman survives depended on context. For a mark, this departs from CSS Fonts § 5.3 step 3, which matches the remaining combining characters singly | the primary font's `.notdef`; LastResort is rejected on Mac |
| Blink 150 (the Chromium that Electron 43 pins, under VS Code and Tabby) | read by the supplement, re-read by the refuter: [[source: chromium150-harfbuzz-shaper]], [[source: chromium150-font-cache-mac]] | the same whole-cluster rule: glyph 0 anywhere makes the whole cluster not-defined and sends it on (harfbuzz_shaper.cc:574-600). The system font is asked about one hint code point, the first that is not Common or Inherited (693-746), so for q + U+0316 it is asked about q and never about the mark | the primary's glyphs are committed, so the letter draws with the primary's `.notdef` for the mark, at that glyph's advance: a cell right on JetBrains Mono (inferred). This departs from CSS Fonts § 5.3 step 3, as Chromium does | the primary font's `.notdef`; LastResort is rejected by name (font_cache_mac.mm:111-115, 140-145) |
| Pango 1.57.1 | measured: [[source: ad-obs-pango-marks]], [[source: ad-obs-pango-emoji]]; read: [[source: pango-itemize-1-57-1]], [[source: pango-font-h-1-57-1]] | a font per character; no cluster rule | per character | a hex box of 4 or 6 digits; later marks land on it |
| Firefox 157 | read only: the headless run failed in the sandbox; [[source: gecko-gfxtextrun]], [[source: gecko-gfxfont]], [[source: gecko-missing-glyphs]] | keeps the previous font if that font has the character | per character | a hexbox from a 3x5 mini-font of its own, independent of any font; ignorables get zero width |
| HarfBuzz 13.2.1 | measured with hb-shape; [[source: harfbuzz-13-2-1-buffer]], [[source: harfbuzz-13-2-1-ot-shape]] | (one font) | (one font) | glyph 0 at the font's own `.notdef` advance: 600 units in JetBrains Mono, one full cell, which is baud's drift before e0a611d. The not-found glyph can be set by the caller. |
| DirectWrite | documentation only; [[source: dwrite-mapcharacters]], [[source: dwrite-getglyphindices]] | unknown | "If it returns NULL, that means that no font can render the text, and mappedLength is the number of characters to skip (rendered with a missing glyph)." | glyph 0 |

What the table shows:

- **No engine measured or read drops a non-ignorable code point silently.**
  Every one measured or read in source hides the default ignorables;
  DirectWrite's documentation does not say.
- **When no font covers the cluster, WebKit and Chromium take the box from the
  primary font.** Chromium's box for a laptop missing from a Noto subset was
  JetBrains Mono's `.notdef`, not Noto's. That is practice. What § 5.21.6 asks
  is that the box be visible, and baud's BOX prototype took the base face's
  `.notdef`, which on Noto is empty.
- **CoreText cannot load a CBDT-only font at all.**
  `CTFontManagerCreateFontDescriptorsFromURL` returns nil for the Noto subset
  and for Noto Color Emoji 2.042 and 2.051. It returns a descriptor for the
  Apple subset and for JetBrains Mono. So no CoreText path ever draws Noto's
  empty `.notdef` (measured, [[source: ad-obs-coretext-cbdt-load]]).
- **Two CoreText details:**
  - An empty `kCTFontCascadeListAttribute` does not turn off the system
    cascade.
  - `CTFontCreateForString` on q + U+1AB0 + U+0301 with JetBrains Mono as the
    base returned Menlo, with U+1AB0 at glyph 0
    ([[source: ad-obs-coretext-createforstring]]).
- **CoreText's placement depends on order.** With the missing mark first (q +
  U+1AB0 + U+0301), the LastResort glyph was drawn over q and the acute
  started after a gap (measured; cause unknown).
- **The supplement's CoreText probe at 12 pt agrees on the split and adds two
  details** ([[source: ad-obs-supp-coretext-jbm]] and
  [[source: ad-obs-supp-coretext-fira]], from
  [[source: ad-obs-supp-coretext-probe-swift]]; the render
  [[source: ad-obs-supp-coretext-render]], whose left column is CTLineDraw by
  [[source: ad-obs-supp-coretext-render-swift]]):
  - With JetBrains Mono as the base, q + U+0316 goes whole to Menlo and
    q + U+05BC whole to Courier New. q + U+1AB2 keeps q in JetBrains Mono and
    gives U+1AB2 a LastResort run, drawn at x = 0, over the letter.
  - That run's advance is 20.40 pt (1.70 em) with nothing after it, and 20.43
    before an acute; with FiraCode, 20.59 and 19.88. So 1.7 em is not only the
    case before another mark that the table above gives; the rule behind it is
    unknown. A cell at that size is about 7.2 pt.
  - In q + U+0316 + U+1AB2 + U+0301, the acute, which JetBrains Mono maps,
    still goes to Menlo after the missing mark.
- **These engines lay out proportional text.** None fits a placeholder into a
  fixed count of cells.

## 5. How often each case happens on the stacks baud ships

**Method** (measured with baud's own code at e0a611d, exported into scratch):

- Each sequence went through a fresh `Term`, so the renderer saw the code
  points the core kept, never the code points as typed. It then went through
  `Renderer::face_for` and the renderer's cluster path.
- The stacks were the defaults from `baud-app/src/config.rs`:
  - macOS: JetBrains Mono, the Nerd icons, Menlo, Apple Color Emoji, Apple
    Symbols, Hiragino Sans GB, Apple SD Gothic Neo, Kohinoor, STIX.
  - Linux: JetBrains Mono, the Nerd icons, DejaVu Sans Mono, Noto Color Emoji,
    STIX.
- Rows marked "working tree" use the uncommitted built Regular with Hebrew,
  `baud-app/fonts/built/Regular.ttf`, measured on the build with sha256
  ea07610d…. The tree has since moved to df06669a…. Its Regular and Bold map
  the same Hebrew: 23 of U+05B0-U+05C7 (all but U+05C6), all 27 letters, and
  none of U+0591-U+05AF (measured 2026-10-08).
- The renderer ran at 14 px, which gives 8-px cells.
- Outcomes are classed in this order of precedence:
  1. base unmapped;
  2. drop;
  3. spill: two or more inked glyphs, the last starting at least one cell
     along (half an em on the rows for one face alone, which have no cells;
     their counts match the stack rows);
  4. composite: two or more inked glyphs layered at the cell's origin;
  5. one glyph.
- A spill figure is where the last glyph starts, not where it ends.
- Outputs: [[source: ad-obs-freq-emoji]], [[source: ad-obs-freq-marks]],
  [[source: ad-obs-freq-replay]], [[source: ad-obs-freq-storetest]]. The rows
  behind each per-sequence figure in sections 5.1 and 5.2 are extracted from
  the 27.7 MB results file into [[source: ad-obs-freq-emoji-rows]], and the
  marks each face lacks are in [[source: ad-obs-freq-marks-missing]].

### 5.1 Emoji

RGI 17.0 is 3,953 sequences. That matches the data files' declared totals
([[source: emoji17-sequences]], [[source: emoji17-zwj-sequences]]) and
emoji-test's 3,944 fully-qualified plus 9 components
([[source: emoji17-test]]). The table counts the 3,847 the core keeps as sent
(section 5.2 has the rest).

| emoji face | where it ships | one | composite | spill | drop | base unmapped |
|---|---|---|---|---|---|---|
| Apple Color Emoji 22.0d3e2 ([[source: macos-apple-color-emoji-22]]) | macOS 27.0 | 3,554 | 293 | 0 | 0 | 0 |
| Noto Color Emoji 2.042 ([[source: noto-color-emoji-deb12]], [[source: noto-color-emoji-ubuntu2404]], [[source: noto-color-emoji-fedora40]]) | Debian 12, Ubuntu 24.04 release (and Fedora 40, past its end of life since 2025-05-13, [[source: fedora-releases-eol]]) | 3,696 | 0 | 78 | 59 | 14 |
| Noto Color Emoji 2.047 ([[source: noto-color-emoji-ubuntu2404-updates]]) | Ubuntu 24.04 updates | 3,704 | 0 | 77 | 59 | 7 |
| Noto Color Emoji 2.051 ([[source: noto-color-emoji-2-051-nix]]) | nixpkgs (Debian 13's 2.051 is a different build, unmeasured; below) | 3,847 | 0 | 0 | 0 | 0 |

- **On macOS the drop rule never fires on any RGI sequence.** The zero holds
  only for Apple Color Emoji 22.0d3e2; older macOS emoji fonts are unmeasured.
- **On Apple Color Emoji, 10 intact sequences draw the bare base's glyph:**
  snowboarder and person in bed, each with the five skin tones
  ([[source: ad-obs-freq-emoji-rows]], section D: the inked glyphs equal those
  of the base alone, with nothing dropped). That is the font's own output, not
  baud's drop. It may be UTS #51 § 2.4.1's suppression of a modifier on a base
  with no skin visible (reading).
- **The 14 base-unmapped sequences on Noto 2.042, and the 7 on 2.047, are
  single Emoji 16.0 and 17.0 characters** that face does not map: the harp,
  the shovel, the fingerprint, U+1FAE9, the orca and others (section E of the
  extract). Each is two columns wide and draws as JetBrains Mono's one-cell
  `.notdef`, one glyph at the cluster's origin. That is case C on a shipped
  stack, in the shape of option b1 (section 6).
- **On Noto 2.042 and 2.047 it fires on 59 sequences.** All are Emoji 17.0
  "wrestling" with skin tones, and the dropped code point is U+1FAEF, the fight
  cloud, new in 17.0. What is left draws as two people side by side, the last
  at 2.12 cells, past the cluster's two. It is neither a box nor a single wrong
  emoji.
- **Spill breaks the grid with or without any drop.** On Noto 2.042, 78
  sequences have every code point mapped but no combined glyph, so their
  glyphs run side by side past the two cells. Of these, 66 draw 3 inked glyphs,
  the last starting at 4.38 cells, and 12 draw 2, the second at 2.12
  ([[source: ad-obs-freq-emoji-rows]], section A;
  [[source: ad-obs-reserved-tags-vs]]). Where the last one ends was not
  measured, since the figure is its start. 72 of the 78 are ZWJ sequences, 5
  modifier sequences and 1 a flag (Sark). On 2.047 the 77 are the same 72 ZWJ
  and 5 modifier sequences. For the ZWJ sequences, this is the sub-image
  display mode 2027 forbids (section 2.7).
- **Regional-indicator pairs spill on every stack, macOS included, and never
  drop.** Of the 676 pairs (a count of coverage, not of use; 259 are RGI
  flags), the ones the face has no flag for draw their two letters side by
  side: the 417 that are not RGI flags on Apple Color Emoji, Noto 2.047 and
  Noto 2.051, and those plus Sark (Emoji 16.0) on Noto 2.042. The second letter
  starts 1.75 cells along on macOS and 2.12 on Linux. None drops, because
  every emoji face maps all 26 indicators ([[source: ad-obs-freq-emoji]], set
  `ri`). This is the case UTS #51 Annex B.1 covers (section 2.4).
- **Sequences that are not fully qualified spill too.** Of 1,272, 7 spill on
  the macOS stack, 14 on Noto 2.042 and 2.047, and 4 on Noto 2.051. The four
  forms of man and woman bouncing ball (U+26F9 + ZWJ + U+2640 or U+2642,
  with or without U+FE0F) spill on every stack, the second glyph starting
  1.12 cells along (set `nonfq`; section C of
  [[source: ad-obs-freq-emoji-rows]]). Every one of these spills is a ZWJ
  sequence.
- **Debian 13 and Ubuntu 26.04 ship a 2.051 that is not the one measured.**
  Debian 13 (trixie, the current stable) ships fonts-noto-color-emoji
  2.051-0+deb13u1, and its NotoColorEmoji.ttf hashes dac5c276… and has 4,124
  glyphs, against nixpkgs' 4dcb1382… and 4,027. Both carry the name string
  "Version 2.051;GOOG;noto-emoji:20250818:e92753b…"
  ([[source: noto-color-emoji-deb13]], measured). Ubuntu 26.04 LTS lists
  2.051-1build1, which was not fetched, and Ubuntu 25.10 lists 2.048-1
  (packages.debian.org and packages.ubuntu.com, read 2026-10-08, not pinned).
  So the 2.051 rows describe nixpkgs only. The 2.042 and 2.047 rows describe
  Debian 12 (oldstable) and Ubuntu 24.04 LTS.
- **The face that keeps every dropped emoji cluster on Linux is Noto.** All 60
  (59 intact, one the core altered) are kept by Noto, whose `.notdef` has no
  outline and no bitmap (2.042, 2.047 and 2.051). A box drawn with the kept
  face's `.notdef` would show nothing in all 60. Every other face in either
  chain has an inked `.notdef`: JetBrains Mono (stock and built), the Nerd
  icons, Menlo, Apple Color Emoji, Apple Symbols, Hiragino Sans GB, Apple SD
  Gothic Neo, Kohinoor, STIX Two Math and DejaVu Sans Mono
  ([[source: ad-obs-notdef-survey]], the CHAIN rows).
- **The `emoji` setting (font or system) changes none of these counts.**

### 5.2 The core changes 106 sequences before any font sees them

All 106 are RGI_Emoji_ZWJ_Sequence; the rows are section B of
[[source: ad-obs-freq-emoji-rows]].

- **91 are cut by the attachment limit.** `ClusterSlot::CAPACITY` is 8
  (cell.rs:553), and 95 ten-code-point kiss sequences with two skin tones
  exceed it. 91 keep their first nine code points and lose the final tone.
  The doc comment above the constant names an eleven-code-point family as the
  case that loses its tail (cell.rs:549-552); RGI 17.0's longest sequences are
  ten.
- **The other 4 kisses lose the second person and keep the tail.** For
  example, 1F9D1 1F3FB 200D 2764 FE0F 200D 1F48B 200D 1F9D1 1F3FC is kept as
  … 200D 1F48B 200D 1F3FC. The refused U+1F9D1 is only the eighth
  attachment, and the limit refuses only past the eighth (paint.rs:797 at
  e0a611d), so the limit is not the cause (inferred from the code). The
  frequency lens attributes the refusal to a store collision
  (`.claude/research/ad-workflow-result.json`, untracked; not re-derived
  here).
- **So 15 lose a code point mid-sequence:** 11 sequences of 5 to 8 code points
  and those 4. The 11 lose a middle or final code point because the store's
  two-probe lookup collides with the sequence's own prefixes (cell.rs:613,
  `baud-core/src/paint.rs`). This happens even in a fresh `Term`.
- **On macOS, 95 of the 106 draw a different emoji inside the cell, and
  nothing on screen says so** (94 composite, 1 one glyph). Examples: a kiss
  missing the second person's skin tone, and "man kneeling" drawn without its
  gender ([[source: ad-obs-freq-shot-core-altered-macos]], a render through
  baud of a few of them). That every one of the 95 is a different emoji is
  inferred from what the core kept differing from what was sent; only those
  examples were looked at. One more, "man in tuxedo: medium-light skin tone", loses only its
  U+FE0F and shapes to the same glyph as the intact sequence (glyph 2643;
  [[source: ad-obs-tux-shape]]), so it draws correctly. The other 10 spill.
- **On Noto, nearly all of them spill.** On 2.042 and 2.047, 102 spill and one
  drops (a women-wrestling sequence, whose remaining figures also spill); on
  2.051, 103 spill. Three draw one glyph on every Noto version. All 95 kisses
  spill there, the last glyph starting 6.50 cells along. Since all 106 are ZWJ
  sequences, every one of these spills, and the 10 on macOS, is the sub-image
  display mode 2027 forbids (section 2.7).
- **Over one session it is most of them.** Every RGI sequence written into
  one `Term`, with ED2 between them, leaves 2,703 of the 2,760 sequences that
  carry anything past the base different from what a fresh `Term` keeps. The
  first divergence is the 1,212th sequence in file order, and the 9th when
  only the ZWJ sequences are written ([[source: ad-obs-freq-saturate]]). This
  is the store of section 5.4, which only RIS frees.
- **No renderer option changes these.** The drop rule never sees them. They are
  the failure the AS_BUILT renders showed, a different emoji shown silently,
  arriving from the core.

### 5.3 Combining marks

**What each face maps** ([[source: ad-obs-freq-marks]], general category M in
UCD 17.0):

- **JetBrains Mono 2.304:** 23 of 250 Latin marks; no Hebrew, Arabic or
  Devanagari marks.
- **The working-tree built face:** adds the 20 Hebrew points, and none of the
  31 cantillation marks U+0591-U+05AF.
- **Menlo:** 67 Latin marks; it lacks U+0342, U+0344 and U+0345.
- **No face on either stack** maps U+0342 or U+0345 (the per-face and
  per-stack lists are [[source: ad-obs-freq-marks-missing]]).
- **Kohinoor:** 34 of 53 Devanagari marks.
- **DejaVu Sans Mono 2.37:** 13 of 52 Arabic marks.

**Attested pairs.** Every precomposed letter in six scripts was decomposed.
On any shipped stack, the drop rule fires on none whose base a face draws.
Clusters move to another face instead: 51 Latin and 174 of 249 Greek clusters
move to Menlo or DejaVu Sans Mono. Where no face has the script, the base draws
`.notdef` and the rule drops every mark on those pairs as well: Hebrew at HEAD
on both stacks (34 pairs, 36 marks), Arabic on macOS (8 of 8), Arabic on Linux
(3 pairs whose base DejaVu Sans Mono lacks) and Devanagari on Linux (11 of 11)
([[source: ad-obs-freq-marks]], `kind=nfd`).

The sweep covered those six scripts and had no kana, whose decomposed forms
carry U+3099 and U+309A. The committed recordings do show case A on both
default stacks (below): one U+309A cluster in cat-cjk on macOS, whose base a
face draws (the base was not identified), and one cluster each in cat-cjk and
less-cjk on Linux. The Linux pair is inferred to be a space + U+3099, since a
space is the only base before those marks in the recordings that JetBrains
Mono draws. That pair occurs 6 times in `tests/recordings/utf8/cat-cjk.raw`
and 4 times in `tests/recordings/utf8/less-cjk.raw` (counted).

**Every base with every mark of its script** (a count of coverage, not of
use; `kind=cross`): the rule drops on 8,892 of 13,000 Latin pairs and 3,584 of
4,032 Cyrillic on both stacks, 1,404 of 1,872 Arabic on Linux, 703 of 1,961
Devanagari on macOS, and 837 of 1,377 Hebrew on the working-tree font, which is
27 letters with each of the 31 cantillation marks.

**Real text, by distinct cluster.** The first count uses a fresh `Term` per
line; the count in brackets is one session through a single `Term`. Each text
is shown on a stack that has a face for its script, and then on the default
stack that has none ([[source: ad-obs-freq-replay]]).

| text | stack | clusters the drop rule hits | what is dropped |
|---|---|---|---|
| Genesis 1, the first 757 lines of OSHB's WLC XML ([[source: oshb-wlc-genesis]]) | macOS or Linux, working-tree font | 208 of 346 (22 of 58) | one cantillation mark each, in U+0591-U+05AE |
| the same | macOS and Linux at HEAD, no Hebrew face | 346 of 346 (58 of 58), every one on a `.notdef` base: case C with attachments | every attached code point: 611 left out (84) |
| Bialik, Hachnisini ([[source: bialik-hachnisini]]) | macOS or Linux, working-tree font | 0 of 81 (0 of 45) | — |
| the same | macOS and Linux at HEAD, no Hebrew face | 81 of 81 (45 of 45), every one on a `.notdef` base: case C with attachments | 97 code points left out (51) |
| an Arabic poem with harakat ([[source: ar-muallaqa]]) | Linux (DejaVu) | 0 of 201 (0 of 63) | — |
| the same | macOS, no Arabic face (HEAD and working tree) | 201 of 201 (63 of 63). 200 are on a `.notdef` base, case C with attachments; one has a base a face draws, which is case A | 255 code points left out (71) |
| a Hindi article ([[source: hi-bharat]]) | macOS (Kohinoor) | 0 of 848 (0 of 63) | — |
| the same | Linux, no Devanagari face (HEAD and working tree) | 848 of 848 (63 of 63), every one on a `.notdef` base: case C with attachments | 1,631 code points left out (83), consonants in U+0915-U+0939 among them |
| Iliad book 1 ([[source: perseus-iliad]]) | any | none: the text is precomposed | — |

**Where no face has the script, case C and case A happen together.** This
covers Hebrew at HEAD (every letter), Arabic on macOS, and on Linux Devanagari
and Han, kana and Hangul: the default Linux stack has no CJK face (config.rs at
e0a611d). The base draws the primary's `.notdef`, and the rule drops the
attached code points. On Linux Hindi those include consonants inside conjuncts
(U+0915-U+0939 are among the code points left out).

**The committed recordings**, counted by distinct cluster dropped
([[source: ad-obs-freq-replay]]). The other recordings drop nothing.

- macOS, HEAD:
  - cat-scroll: 1 (U+0651 on a base no face draws);
  - cat-cjk: 1 (U+309A, on a base a face draws: case A);
  - vim-unicode: 3 (Hebrew points on `.notdef` bases).
- Linux, HEAD:
  - cat-cjk: 31, 30 of them on `.notdef` bases (U+3099 in 11, U+309A in 18,
    and three Devanagari vowel signs). The other is case A, inferred above to
    be a space + U+3099;
  - less-cjk: 14, 13 of them on `.notdef` bases (U+3099 and U+309A on kana,
    and the same vowel signs). The other is case A, likewise;
  - vim-unicode: 4 (the Hebrew points, and U+0924, U+0947 and U+094D);
  - cat-scroll: 0, since DejaVu Sans Mono draws its U+0651 cluster whole.

### 5.4 The cluster store refuses 36-71% of attached marks in one session

The store has 63 slots, probes two of them, refuses on a collision or when
full, and keeps every growing prefix as its own slot (note AB, Q7). Refusals,
by `Term::marks_dropped`:

| text | attached marks | refused in one `Term` | refused with a fresh `Term` per line |
|---|---|---|---|
| Bialik | 228 | 81 | 5 |
| Genesis 1 (the OSHB prefix) | 1,742 | 997 | 6 |
| Arabic poem | 2,531 | 1,117 | 171 |
| Hindi article | 18,894 | 13,473 | 6,218 |

The Hindi denominator is not exact: conjunct clusters also attach consonants.

- **Only RIS frees the store.** After the Hindi article, all 228 of Bialik's
  marks were refused across ED2, `?1049` entered and left, or DECSTR. RIS
  brought the count back to 81 ([[source: ad-obs-freq-storetest]]). The one
  `clusters.reset()` is in RIS (ops.rs:1440).
- **The store lives as long as the window.** The app keeps one `Term` per
  window for the window's life (app.rs:766-771).
- **So on a long session the renderer's rule sees only the 29-64% of marks the
  store kept.** The rest are gone from the grid, and so from copy, which reads
  `Term` (app.rs:5731-5738). This is the part C7 and § 2.14.2 reach (section
  2.2).
- **One verse is enough to lose a point in a fresh `Term`.** Genesis 1:1 as
  section 7.3's renders feed it (verse 1 of [[source: sefaria-genesis-1]],
  with the `<big>` tags stripped) keeps the resh of its last word without its
  segol: U+05E8 is kept, U+05B6 is refused, and `marks_dropped` is 1, at 39
  and at 80 columns. The last word fed alone keeps U+05E8 U+05B6 and refuses
  nothing. Measured with baud's core at 5e5e934, whose sources are unchanged
  since e0a611d, on 2026-10-08, after baud's renderer showed the missing
  segol the same day. So what the verse writes before that word decides the
  refusal: the store refusing on a collision or when full, as described
  above (inferred; which of the two, and which earlier cluster, was not
  traced). Every render of the verse lacks that segol whichever renderer
  option draws it, so no option in section 6 can show it (Q7).

### 5.5 Unmeasured

- **How often these sequences are used.** Unicode's only frequency list ranks
  Unicode 12.0 emoji and counts skin-tone variants together
  ([[source: unicode-emoji-frequency]]).
- **Older macOS emoji fonts.**
- **The current Linux emoji packages.** Debian 13's 2.051 build, Ubuntu
  25.10's 2.048 and Ubuntu 26.04's 2.051-1build1 were not measured (section
  5.1).
- **Configured stacks.** Only `config.rs`'s defaults were measured. A
  configured primary, such as SF Mono or Fira Code, changes both how often
  case A happens and whether b1's box is visible (section 2.5).
- **The Metal cell shader and the instance path.** Only the CPU renderer was
  measured.
- **Linux itself.** It was not run. DejaVu Sans Mono 2.37 from nixpkgs stood in
  for the distribution packages, and the counts assume the packages sit at the
  paths `config.rs` probes.
- **What tmux and zellij do to baud's grid.** Their byte streams for the
  supplement's cases are captured ([[source: ad-obs-supp-tmux-out]],
  [[source: ad-obs-supp-zellij-out]]) but were not fed through baud's core,
  so the column drift section 3.1 predicts is unmeasured. Other
  multiplexers, herdr among them, were not examined.

## 6. The design options

**The cases**, named here because the options differ by case:

- **A.** A combining mark the kept face lacks, on a base that face draws.
- **B.** An attachment that is not a mark, which the kept face lacks. That is
  an emoji part (a ZWJ component, a modifier, a second regional indicator, a
  tag), or a letter attached in an Indic conjunct: a consonant joined after a
  virama (GB9c). The rule drops such a consonant whether or not the base is
  drawn. On the Linux stack U+0915 is among the consonants dropped, there on
  `.notdef` bases, so in case C with attachments (measured, section 5.3); a
  conjunct whose first consonant a face draws was not measured. Hangul vowel
  and final jamo after a leading jamo fall under the same rule (inferred; not
  measured).
- **C.** A base no face maps. It draws the primary's `.notdef` today
  (stack.rs:500-509). An unmarked precomposed character whose decomposed mark
  drifts a box (Q23's open item) belongs here too. When the cluster carries
  attachments, they are dropped as well, and the one `.notdef` stands for all
  of them (section 2.7 splits the two).
- **D.** A misplaced variation selector, which 18.0's D57h says to display
  visibly. baud pins UCD 17.0, under which it draws nothing (section 2.6).

**The constraints:**

- **The grid.** The core fixes a cluster's cells whatever is drawn. A
  placeholder that leaves them covers the next character, which section 3's
  overdraw judgement marks as departing from § 5.21.6. For a ZWJ sequence,
  mode 2027, which baud claims, requires "a single image with a width of 2
  grid cells". So a placeholder must fit the cluster's 1 or 2 cells. The
  working group's "no layout rules can depend on font data" is about cell
  counts, not about what is drawn in them, and it comes from a report, not a
  standard (section 2.4).
  - § 2.11.5 says of a grapheme cluster that an implementation "would almost
    never want to break between its elements for rendering, editing, or other
    such text processes; the grapheme cluster is treated as a single unit"
    ([[source: unicode18-ch2]]; the same in [[source: unicode17-ch2]]).
  - UAX #29 adds that "Default grapheme clusters do not necessarily reflect
    text display" ([[source: uax29]] § 3). So several glyphs inside one
    cluster's cells are not ruled out.
  - kitty's text-sizing protocol (OSC 66, text sent with an explicit width)
    leaves overflow to the terminal: "If the text does not fit, the terminal is free to do whatever it
    feels is best, including truncating the text or downsizing the font size
    when rendering it." ([[source: kitty-text-sizing]]) baud does not
    implement OSC 66, so this is one terminal's protocol, recorded as
    practice. The grid constraint rests on the core's fixed widths, § 5.21.6,
    mode 2027 and § 2.11.5.
  - **What happens to a glyph that reaches below its row depends on the draw
    path.** baud has three, and they differ (file:line at e0a611d; the four
    files cited are unchanged at 5e5e934):
    - **The CPU paint clips each row.** `paint` hands every instance of a
      row that row's pixel band (renderer.rs:1661-1692), and `Target::over`
      skips the pixel rows outside it (composite.rs:87-101). A glyph that
      reaches below its row is cut off, not drawn over the next row.
      Sideways, inside the row, it still covers the next character. Measured
      by baud's renderer, 2026-10-08, with a prototype of e that puts a box
      for a mark below under the lowest point already below: in Genesis 1:1
      at 13 px the six boxes are counted as drawn and none of them shows; at
      26 px five show only their top edge, on the row's last pixel row, and
      the sixth does not show. Of the two marks in bet + qamats + etnahta and
      bet + qamats + pashta, only the pashta box above shows, at 13 px
      without its top line; the etnahta box below the qamats is cut off. The
      placement is the prototype's own rule; a placement inside the cell was
      not rendered.
    - **The Metal instance path does not clip to a row.** It draws the
      background pass held to the grid, then resets the scissor to the whole
      target and draws every record (metal.rs:1494-1514), and its glyph
      shader has no clip of its own (read from the code). So the same box
      would be drawn over the top of the next row (inferred: the renders of
      2026-10-08 composited the records in that order on the CPU, which is
      not a GPU capture).
    - **The app's default path on macOS is the cell shader** (app.rs:776-785:
      `Request::Auto` with a GPU gives `Path::Cells`). No placeholder was
      drawn through it, because the prototype hooks only the instance path.
      For real points, a scratch Metal probe found the cell and instance
      paths agree to 0 pixels at 14 and 26 px, and the points below pointed
      letters draw into the next row at 26 px, where the CPU clips
      (`docs/record.md`, "baud's
      own Hebrew is in the embedded font, Regular and Bold, built from
      committed sources"). So a placeholder below the row would likely cross
      into the next row there too (inferred).

    So a placeholder placed below its row is not drawn whole inside its
    cells on any path: on the GPU paths it would cover the top of the next
    row (inferred, above), and on the CPU path it is cut off, so that mark
    shows nothing, or only an edge.
- **Same pixels on every OS.** That is the owner's rule. The placeholder must
  not depend on the configured or system face: their `.notdef` glyphs and the
  OS's last-resort font differ by OS and by configuration. The embedded face's
  `.notdef` (b2) meets the rule while that face is in the binary.
- **No engine.** baud-font and baud-render draw everything.
- **The space available** (measured through `baud_render::Metrics` on the
  embedded JetBrains Mono): 13 px gives 8x17 cells and 26 px gives 16x34. At
  26 px that leaves about 8 px above the cap height and 7 px below the
  baseline; at 13 px, 3-4 px each.

Every cost in this table is unmeasured except d's size; "small" and the like
are the design lens's estimates.

| option | A | B | C | follows the best practice (section 3's yardstick), by case | same pixels on every OS | cost |
|---|---|---|---|---|---|---|
| g. drop, as built | the base, mark gone | a shorter or different emoji | — | A, B: departs (§ 5.21.6, "Displaying nothing"); a reserved code point after a ZWJ departs from § 5.3 ("blindly delete"); under a 10646 claim, from § 2.3 c) (section 2.2) | — | none |
| b1. the primary face's `.notdef` | replaces the cluster | a box | a box (today) | A, B: partly: a visible box, but it replaces a base a face draws, and one box stands for several characters, against "how many are present" (§ 5.21.6). C, a lone code point: follows when the glyph is inked, departs when it is blank; it relies on the configured font following OpenType's recommendation that `.notdef` have an outline (section 2.5). C, a cluster with attachments: partly, one box for several characters | no: it is the configured font's | small |
| b2. the embedded Regular's `.notdef`, whatever is configured | replaces the cluster | a box in the cells | a box | A, B: partly, as b1. C: follows for a lone code point; partly for a cluster with attachments, as b1 | yes | the renderer needs a placeholder face beside the stack |
| b3. the base face's `.notdef` (the BOX prototype) | replaces the cluster | a box, or nothing on Noto | — | A, B: partly, as b1; departs on Noto, where the glyph is blank. It relies on the base face following OpenType's recommendation (section 2.5) | no | small |
| a. a box baud draws, sized to the cluster | replaces the cluster (base lost) | a box in the cells | a box | A, B: partly, as b1. C: follows for a lone code point; partly for a cluster with attachments, as b1 | yes | a procedural mask, about two per size |
| e. a mark-sized placeholder where the mark would go | the base plus a small box above or below | a small box over the base, the overlay UTS #51 C.1.1 recommends for tag sequences "so that the sequence would have the same width" | — | A, B: follows where the placeholder can be told from a real mark and stays inside the cluster's cells: § 5.21.6's visible, generic glyph, one per missing code point, with the base kept. Partly where it reads as a mark, since § 5.21.6 asks for a "generic" glyph (a 3-px box below a Hebrew letter at 13 px reads as a hiriq), or where it leaves its row: drawn over the next row on the GPU paths (section 3's overdraw judgement; inferred), or cut off on the CPU path, where that mark then shows nothing or only an edge (where nothing shows, § 5.21.6's "Displaying nothing", the reading row g carries). The first is predicted in the notes below, not measured; the second was rendered on the CPU path, under a prototype's placement below the lowest point already below (the grid constraint above). For B, it is UTS #51 § 2.4.1's base and indicator for a modifier, although § 2.4.1 shows the indicator after the base where e overlays it, which is C.1.1's form for tags. For a tag sequence it is C.1.1's overlay. For a ZWJ component it is not § 2.5's separate emoji, which mode 2027 forbids | yes | placement without GPOS; the dropped code points must be recorded |
| c. a hex box | replaces the cluster | replaces the cluster | digits | A, B: partly, as b1. C: follows for a lone code point, since § 5.3 names the hexadecimal value; partly for a cluster with attachments, as b1 | only with baud's own digits | at 8 px only without a frame (see below); bounded atlas only if composed from 16 digit masks |
| d. the Last Resort font, embedded | cannot serve A | legible in 2 cells | a framed block glyph | A: cannot serve. B: partly, as b1. C: follows for a lone code point, since § 5.3 names the Last Resort Font; partly for a cluster with attachments, as b1 | yes | +588 KB; illegible in one 8x17 cell (mockup) |
| f. U+FFFD from the embedded face | replaces the cluster | replaces the cluster | U+FFFD | A, B: partly, as b1. U+FFFD is defined as "the general substitute character", which "can be substituted for any “unknown” character in another encoding that cannot be mapped" and "can also be used as one means of indicating a conversion error" (§ 23.8, [[source: unicode18-ch23]]). So on an assigned character a font lacks, it signals an error the text does not contain (reading). For a ZWJ sequence it is mode 2027's example. C: for a reserved code point it is § 5.3's "substitute glyph, such as that commonly shown for U+FFFD"; for an assigned one it follows § 5.21.6 in form, with the same misreading; for a cluster with attachments, partly, as b1 | yes | looks like a decode error |
| h. the missing mark from another face that maps it | when some face has it | — | — | A: follows: § 5.13's Show Hidden and Simple Overlap and D52 draw the mark's own glyph. CSS Fonts § 5.3 step 3 does the same for CSS user agents (section 4) | only when that face ships with baud | only when a shipped face maps the mark; faces mixed inside one cell; positioning without GPOS anchors |

Notes on the rows, from the design lens unless marked:

- **b1** is visible with the embedded JetBrains Mono, whose `.notdef` is a
  cap-height frame with a diagonal, one cell wide (an outer rectangle with two
  triangular counters). It is invisible under a configured SF Mono (section
  2.5).
- **b3** is what the BOX prototype drew. It also counted `clusters_boxed=1` for
  a box it never drew, because `emit` skips a glyph whose atlas slot is empty
  (renderer.rs:2634; `.claude/research/review-v13/starters/inputs/counts.txt`,
  untracked, copied as [[source: ad-obs-review-v13-counts]]).
- **a** is § 5.21.6's "visible “missing glyph” box", drawn by baud. As
  practice, xterm and urxvt draw a box of their own inside the cell over a
  kept base, and kitty one per cell in place of the cluster (section 3). That
  practice is not the reason for a.
  - baud already routes box-drawing code points to procedural masks, but that
    hook is fixed per code point. "Missing" depends on the face, so the trigger
    would be the Partial branch in `shape` (renderer.rs:2444-2468) or the
    `.notdef` the shaper emits.
  - Procedural masks are keyed by a `u32` (atlas.rs:373), so keys above
    U+10FFFF are free for placeholder kinds.
- **e** is the only placeholder that keeps the base readable in case A. (The
  silent drop keeps the base too, but shows nothing.)
  - baud's placement for faces without GPOS, `BaseBox::place`
    (shape.rs:210-270), has no arm for combining classes 10-199, and baud-font
    does not recategorise them. HarfBuzz does
    ([[source: harfbuzz-hb-ot-shape-fallback]], hb-ot-shape-fallback.cc:82-115).
    Without that, a placeholder for a Hebrew point lands on the letter.
  - `Kept` records only how many code points were dropped, not which ones
    (shape.rs:482).
  - At 13 px a 3-px box reads as a dot.
- **c** fits a 16-px cell as a 2x2 grid of 3x5 digits for the BMP, or 3x2 for
  astral code points. In an 8-px cell (arithmetic): with a 1-px frame and 1-px
  padding the 2x2 grid is 11 px wide and does not fit; without a frame it is
  7x11 px and fits, and an astral 2x3 grid is 7x17 px, which fills the 8x17
  cell exactly. Pango draws its digits in the OS "monospace" family at size /
  2.2 ([[source: pango-cairo-font-c]]), so same pixels needs digits of baud's
  own. Composed from 16 digit masks, the atlas stays bounded; rasterised
  whole, it needs one mask per code point (arithmetic). Firefox draws its
  hexbox from a mini-font of its own the same way (section 4), which is
  practice.
- **d** is the Last Resort High-Efficiency font 18.000: 587,864 bytes, 387
  glyphs, OFL 1.1, cmap formats 13 and 4 ([[source: last-resort-he-ttf-18]],
  [[source: last-resort-readme]], [[source: last-resort-license]]). The full
  font is 9.6 MB ([[source: last-resort-ttf-18]]). baud-font reads its format
  13 cmap (measured).
  - It would add 12.9% to the roughly 4.57 MB of fonts embedded now.
  - 386 of its 387 glyphs are a 0.956-em square in a 1.147-em advance (the
    other is 0.375 by 0.833 em in a 0.5-em advance). Two cells hold the square
    unscaled; one cell needs a scale of about 0.63. The README says the block
    name on the frame is "only visible at large size".
  - It cannot serve case A. U+0316 maps to the whole Combining Diacritical
    Marks frame, and U+05B8, U+0591 and U+05D0 all map to one Hebrew frame.
  - It must never join `select_cluster`, because it maps every code point.
  - Its cmap gives visible glyphs to default ignorables (U+200D, U+FE0F,
    U+E007F), so those must be filtered first (measured).
- **f** is § 5.3's suggestion for reserved and private-use code points, and
  mode 2027's example. baud already draws U+FFFD for a UTF-8 decode error, so
  the two would look alike.

**Carrying a placeholder through the renderer:**

- **The run cache needs no new key.** Every option is decided by the face, the
  code points and the span, and the run cache's key already folds all three.
- **The run path has room for a marker.** `Placed` is 8 bytes and carries no
  face or source (cache.rs:29-35; measured `size_of`: `Placed` 8, `Shaped`
  770). There are two ways to mark a placeholder in it:
  - a sentinel glyph id 0xFFFF, which is never a real glyph because a font's
    glyph count is a `u16` (inferred);
  - a tag field, which makes `Placed` 10 bytes and the 2,048-entry run cache
    about 1.53 MiB to 1.91 MiB, +24.5% (arithmetic; its effect on frame time
    is unmeasured).
- **A run never needs more entries.** A placeholder replaces a dropped code
  point one for one.
- **The overlay path already has the shape.** Its `GlyphSource::Procedural`
  covers a procedural glyph.
- **The counter should count what was drawn,** not what was attempted.
- **Copy is unaffected.** It reads `Term`.

| option | visible whatever face is kept | keeps the base in A | same pixels without the embedded face | atlas bounded |
|---|---|---|---|---|
| g | no | yes | — | yes |
| b1 | no | no | no | yes |
| b2 | yes | no | no | yes |
| b3 | no | no | no | yes |
| a | yes | no | yes | yes |
| e | yes | yes | yes | yes |
| c | yes | no | with baud's own digits | only if composed from digit masks |
| d | yes | no | no | yes (387 glyphs) |
| f | yes | no | no | yes |
| h | — | yes | no | yes |

The options can be combined, for example e for case A with a or b2 for cases B
and C.

**Hebrew, the case the owner reads.**

- **Before 22b53b4 no face in the default stacks mapped Hebrew,** so every
  letter was case C.
- **Since 22b53b4,** the embedded Regular and Bold map every point in
  U+05B0-U+05C7; U+05C6, the one they lack, is punctuation. No face in baud's
  stacks maps any of the 31 cantillation marks: not the embedded font, not
  the macOS default chain, not DejaVu Sans Mono. Faces outside the stacks do:
  SF Hebrew, Courier New, Arial Hebrew, Times New Roman and Tahoma on this Mac
  (measured by cmap, 2026-10-08), which only a system tier could reach.
- **Under e, on the working-tree font, Genesis 1:1-5 in Sefaria's Masoretic
  text would show a placeholder on 40 of its 45 words**
  ([[source: sefaria-genesis-1]]; measured, with the thin spaces around the
  paseq counted as spaces, and the `{פ}` paragraph marker and the paseq `׀`
  not counted as words). That is 41 marks, 32 below and 9 above. A
  placeholder put below an existing point below can leave the row. `emit`
  does not clip (renderer.rs:2598-2654), but the CPU paint clips each row,
  so there such a placeholder is cut off rather than drawn over the next
  row; on the GPU paths it would cross into the next row (inferred). The
  grid constraint above gives the three paths and what baud's renderer drew
  on 2026-10-08: on the CPU path, none of Genesis 1:1's six boxes showed at
  13 px.
- **A face with letters and no points shows how dense case A gets.** In the
  Genesis 1:1-5 text of the working tree's untracked
  baud-core/tests/pointed_hebrew.rs, 128 of 197 letters (65%) carry one to
  three points: 102 below, 27 above, and 17 dagesh inside the letter. A
  dot-sized placeholder below reads as a hiriq (inferred from the mockup).
- **One option for the cantillation marks is in the font, not the renderer
  (Q9).** The owner's font rule is that gaps are fixed in the embedded font
  (note AC). Drawing the 31 cantillation marks into baud's Hebrew would take
  them out of case A on every stack, but a placeholder rule would still be
  needed for every other script.
- **The store limit comes first.** With cantillation, Genesis 1:1-5 (Sefaria)
  has 81 distinct letter-and-mark clusters, past the store's 63, and the store
  would need 107 base-and-mark prefixes (measured). The refused marks are gone
  before any renderer option can show them.

## 7. Questions for Jev and the owner

### 7.1 What the earlier Jev requests did not carry

The four requests are in `.claude/research/hebrew-improve/missing-mark/`,
which is untracked; each is copied unchanged into `ad-observed/`
([[source: ad-obs-jev-design]], [[source: ad-obs-jev-design-r2]],
[[source: ad-obs-jev-starters]], [[source: ad-obs-jev-starters-r2]]):

- `jev-design.json` and `jev-design-r2.json`, the second of which decided
  A_DROP at 0.99;
- `jev-starters.json` (0.55 against 0.45, at confidence 0.10);
- `jev-starters-r2.json` (0.47, 0.29 and 0.24, at confidence 0.20).

Set against this note:

| context | design, both asks | starters, both asks |
|---|---|---|
| § 5.21.6: a visible fallback, "how many", "Displaying nothing would give the user the impression that the character does not occur" | no | no |
| § 5.3: a placeholder (hex value, script, the Last Resort Font) | no | no |
| § 5.13 Show Hidden and Simple Overlap, and D52, which draw the mark's own glyph (option h) | no | no |
| chapter 5 is not normative; C5; "blindly delete" covers reserved and private-use code points, and the drop deletes reserved code points after a ZWJ | no | no |
| UTS #51: the modifier's missing-glyph indicator, the flag options, the tag overlay "occupying the same physical dimensions", C.1.1's "must" and its width reason, the glue | no | only "an unsupported ZWJ sequence should display as its components, which a 2-column cell cannot fit" |
| the mode 2027 text on ZWJ emoji | no | no |
| an option that keeps the base and draws a placeholder inside the cell | no: drop, replace the whole cell, base face and drop, overlay | no: both box options replace the whole cluster |
| terminals observed here: Terminal.app, iTerm2, kitty's order dependence, WezTerm's toast | no | no |
| the supplement (section 3): xterm and urxvt box a missing mark inside the cell over a kept base; VTE's GTK4 build and Warp drop marks a face can draw; zellij strips every width-0 code point before baud sees it; each terminal marked against the standard | no | no |
| engines measured: none drops silently; the box comes from the primary font | no | partly: "Browsers draw .notdef for a character no font has; none silently drops a character" |
| frequency: 0 emoji drops on macOS, 59 intact sequences on Noto 2.042 and 2.047 (60 on screen, counting one the core altered), 417-418 regional-indicator pairs spilling on every stack, 0 drops on attested mark pairs whose base a face draws (six scripts, no kana; in the recordings case A does occur, in one cluster on macOS and in one in each CJK recording on Linux), 208 of 346 marked clusters in Genesis 1 | no | no |
| what else reaches the same displays: ISO/IEC 10646 § 2.3 c) ("shall be indicated to the user"); § 5.13's no-break-space base for a defective combining sequence, and the core's refusal of every zero-width code point at column 0; mode 2027's "must not" against the sub-images baud draws today; OpenType's "should not be left without an outline" for `.notdef`; ECMA-48 leaving the matter to the implementor | no | no |
| the core: 106 RGI sequences altered in a fresh `Term`, 95 of them a different emoji on macOS; 2,703 of 2,760 in one session; the store refusing 36-71% of marks over a session | no | no |
| Noto's `.notdef` is empty; the BOX prototype counted a box it never drew | no | the second ask: the empty `.notdef`, yes; the miscount, no |
| the owner's same-pixels rule | no | the second ask: inside BOX_PRIMARY's criterion only |

`principles.json`, which rides in every request, carries "Every protocol claim
traces to a pinned primary source". It has nothing on missing glyphs, on
display best practice, or on same pixels across operating systems.

Some premises in the starters requests were wrong, not only missing. That is
why Q23 was asked again once before, after a refuter found two wrong premises
in the design request. In `jev-starters-r2.json`:

- **BOX_PRIMARY** is described as the primary text face's `.notdef`,
  "JetBrains Mono's, always inked". It is not always inked: under a configured
  SF Mono, glyph 0 is empty (section 2.5).
- **UTS #51** is summarised as asking for components "which a 2-column cell
  cannot fit". That leaves out the displays it describes that do fit: the
  modifier's missing-glyph indicator, and Annex C's overlay "occupying the same
  physical dimensions".
- **When it happens.** The request calls an emoji face older than the text "the
  realistic case", on a default stack it describes as "system monospace" first.
  The default primary is the embedded JetBrains Mono. The frequency is now
  measured: no drops on macOS 27's emoji face, and 59 intact sequences on Noto
  2.042 and 2.047 (60 on screen, counting one the core altered; section 5.1).

### 7.2 The questions

The bar is as before: an answer at 0.9 or above is built; below it, the owner
decides from the renders in 7.3. Each option carries its verdict against the
text quoted in section 2, judged the way section 3's yardstick judges every
terminal. What other terminals do is in section 3. It is practice, and no
criterion below rests on it.

**Q1. Does baud follow chapter 5's best practice for a graphic character it
cannot draw?**

- Options:
  - (i) Follow it: every code point that is not default-ignorable and is not
    drawn shows a visible placeholder. Follows § 5.21.6 and § 5.3. (Misplaced
    selectors and tags, which are default-ignorable, are Q8's.)
  - (ii) Follow it for cases B and C only, and keep the drop for marks.
    Follows for B and C. Departs for A (§ 5.21.6, "Displaying nothing").
  - (iii) Depart from it deliberately and record why, for example the grid.
    Departs from § 5.21.6. Chapter 5 is best practice, not a conformance
    clause (section 2.2), so a recorded departure breaks no Unicode clause.
    Under a 10646 device claim it departs from § 2.3 c) as well (section
    2.2). It is still a departure, and under the owner's rule a departure is
    the owner's call.
- Criteria:
  - § 5.21.6's "visible" and "how many", and the owner's "outside standard?";
  - how often each case happens on the shipped stacks (section 5);
  - whether the base stays readable;
  - how a placeholder reads where marks are dense, as in cantillated Hebrew
    (section 6);
  - what the user can learn today: the counter is internal.
- A note on frequency, which is practice and not a criterion: behind zellij,
  cases A and B never reach baud, because zellij strips every width-0 code
  point first (section 3.1). That stripping is itself a departure (section
  3.1). It lowers how often A and B reach baud, not what the text asks.

**Q2. Case A: a mark the kept face lacks, on a base it draws.**

- Options:
  - g: drop, as built. Departs from § 5.21.6 ("Displaying nothing"), and from
    10646 § 2.3 c) under a 10646 claim (section 2.2).
  - e: a mark-sized placeholder where the mark would go, procedural. Follows
    § 5.21.6 where the placeholder can be told from a real mark and stays
    inside the cluster's cells: visible and generic, one per missing mark,
    with the base kept. Partly where it reads as a mark (a 3-px box below a
    Hebrew letter at 13 px reads as a hiriq) or leaves its row: drawn over the
    next row on the GPU paths (section 3's overdraw judgement; inferred), or
    cut off on the CPU path, so that mark shows nothing or only an edge
    (where nothing shows, "Displaying nothing", as for g). The first is a
    prediction; the second was rendered
    on the CPU path, under the prototype's placement below the lowest point
    already below (section 6, the grid constraint).
  - h then e: the real mark from another face where one has it, otherwise e.
    h follows § 5.13 and D52, which draw the mark's own glyph; CSS Fonts § 5.3
    step 3 does the same for CSS user agents (section 4). e as above.
  - a or b2: replace the whole cluster with a box. Partly: the box is visible
    (§ 5.21.6), but it hides a base a face draws, and one box stands for two
    or more characters, against "how many are present".
- Criteria:
  - the base stays readable;
  - the count stays visible;
  - same pixels on every OS;
  - placement without GPOS, including the Hebrew classes;
  - it fits the cell's rows at 13 px;
  - how it reads in densely pointed or cantillated Hebrew.

**Q3. Case B: an attachment that is not a mark.** It covers a missing ZWJ
component or modifier, a reserved code point after a ZWJ (text from a later
version, section 2.2), and the regional-indicator pairs that are not flags.
It also covers a consonant joined after a virama (GB9c) and Hangul jamo after
a leading jamo, where the kept face lacks them (section 6, case B).

- Options:
  - g: as built where the drop fires, a shorter and different emoji. Departs:
    UTS #51 describes no base-only display for a missing ZWJ component,
    modifier or second regional indicator (section 2.4), and § 5.21.6
    recommends something visible. For a reserved code point after a ZWJ it
    also departs from § 5.3's "blindly delete". Under a 10646 claim it departs
    from § 2.3 c) (section 2.2).
  - As built where the renderer drops nothing: the parts side by side past
    the cluster's cells (spill). This is most of case B as measured: the
    417-418 regional-indicator pairs on every stack, the 78 and 77 intact
    spills on Noto 2.042 and 2.047 (Sark is counted both in 2.042's 78 and in
    its 418), and 4-14 not fully qualified sequences, three of macOS's seven
    altered by the core first (section 5.1). For a regional-indicator pair, partly: Annex B.1's letters (or C3's
    singleton letters, for an invalid pair), outside the cells. On Apple Color
    Emoji each letter sits in a dashed square, the face's own glyph for a lone
    indicator, so it has B.1's square with dashes for dots; on Noto Color
    Emoji 2.042 the letters have no square (section 2.7; measured by baud's
    renderer, 2026-10-08). For a ZWJ sequence, it is UTS #51 § 2.5's "fallback
    sequence of separate emoji", and it departs from mode 2027 ("must not be
    used as a fallback") and from the grid constraint (section 6).
  - A box confined to the cluster's cells, either a (procedural) or b2 (the
    embedded `.notdef`). Partly: a visible box (§ 5.21.6), but it hides the
    base, which UTS #51 § 2.4.1 and § 2.5 both keep.
  - e: an overlay on the base, keeping the base visible with a small box, as
    UTS #51 C.1.1 recommends for tag sequences. Follows § 5.21.6 where the box
    can be told from part of the emoji and stays inside the cells; partly
    otherwise (section 6). For a modifier it is § 2.4.1's base and indicator,
    overlaid where § 2.4.1 shows it after the base. For a ZWJ component it is
    not § 2.5's separate emoji, which mode 2027 forbids.
  - f: U+FFFD, mode 2027's example. For a ZWJ sequence it follows mode 2027,
    which leaves the display "undefined" and gives U+FFFD as an example.
    Against § 5.21.6 it is partly, as a box replacing the cluster is. § 23.8
    defines U+FFFD as the substitute for an unmappable character and a sign
    of a conversion error, so it signals an error the text does not contain
    (section 6).
  - The components side by side, scaled into the two cells. This is baud's own
    adaptation: UTS #51 § 2.5's fallback is "a fallback sequence of separate
    emoji" and does not confine it, and mode 2027 says that display "must not
    be used as a fallback". So it follows UTS #51 § 2.5 in substance and
    departs from mode 2027, which baud claims (section 2.4). Compare ECMA-48
    § 8.3.54, whose note on GCC says "In the simplest case, two components
    may be "half-width" and side-by-side. For example, in Japanese text a pair
    of characters may be presented side-by-side, and occupy the space of a
    normal-size Kanji character." ([[source: ecma48]]). GCC is an explicit
    request, not a fallback, so this is a reading and changes no verdict.
  - For a regional-indicator pair that is not a flag, B.1's two options apply
    to every pair without a flag (section 2.4): the two letters scaled into
    the two cells (B.1's first option, confined; it follows B.1 if each
    letter is drawn in a dotted square), or one missing-flag glyph (its
    second option, which it follows). For an invalid pair, C3 also applies if
    baud claims UTS #51: the depiction must indicate "the presence of an
    invalid sequence". The letters meet C3 through its allowance for a
    singleton shown "as a capital A..Z character with a special display".
    The single missing-flag glyph does not, because B.1 defines it as saying
    a flag "was intended". Which of the 417 are valid is unmeasured.
- Criteria:
  - what the user sees in place of the intended emoji: AS_BUILT shows a
    different one with nothing to say so;
  - how often it happens: no drops on macOS 27's emoji face, 59 intact
    sequences on Noto 2.042 and 2.047 (60 on screen, counting one the core
    altered), and 417-418 regional-indicator pairs spilling on every stack
    (section 5.1);
  - whether the base stays visible;
  - visible on every emoji face (Noto's empty `.notdef` rules out b3);
  - stays inside 2 cells (mode 2027);
  - same pixels.
- Related: on Apple Color Emoji an unsupported or invalid tag sequence draws
  the bare black flag, the display C.1.1 and C.1.2 rule out for a conformant
  implementation that supports tag sequences, if baud claims UTS #51
  conformance (section 2.4). The face's shaping produces it, not the drop
  rule, so a renderer rule for it would be separate.

**Q4. Case C: nothing draws the base.**

- Options, each judged against § 5.21.6 and § 5.3, first for a lone code
  point:
  - b1, as today. Follows when the primary's `.notdef` is inked. Departs when
    it is blank, as under a configured SF Mono.
  - b2. Follows: § 5.21.6's "visible “missing glyph” box".
  - a. Follows, for the same reason.
  - c (hex). Follows: § 5.3 names "the hexadecimal value of the code point".
  - d (Last Resort). Follows: § 5.3 names the Last Resort Font.
  - f (U+FFFD). For a reserved code point, § 5.3 names "another substitute
    glyph, such as that commonly shown for U+FFFD". For an assigned one it
    follows § 5.21.6 in form, as a visible, generic glyph, but § 23.8 defines
    U+FFFD as the substitute for an unmappable character and a sign of a
    conversion error, so it signals an error the text does not contain.
- For a cluster with attachments (Hebrew at HEAD, Arabic on macOS, Devanagari
  and kana + U+3099 on Linux), every option above that draws one glyph for
  the whole cluster is partly: one glyph stands for several characters,
  against "how many are present" (§ 5.21.6). b1 is that case today, with the
  attachments dropped and counted only internally (section 2.7). Combining
  one of them with e for the attachments would keep "how many" visible.
- Criteria:
  - visible whatever font is configured (b1 fails under SF Mono);
  - same pixels;
  - how much it says (a code point or a script) against how legible it is at
    8x17;
  - its size in the binary.

**Q5. One placeholder or several?** One glyph for A, B and C, or distinct ones:
a box for a missing character and a small box for a missing mark. The text
asks that the placeholder be visible and generic (§ 5.21.6) and allows it to
say more, such as the code point or the script (§ 5.3). ISO/IEC 10646 asks
only that such characters be indicated: "The way used for indicating them need
not distinguish them from each other" (§ 2.3 c), section 2.2). Either choice
follows both.

**Q6. Should the user be told?**

- Options: the counter only, as today; a log line; a one-time notice. As
  practice, WezTerm shows a toast and mintty answers an application's OSC
  7771 query (section 3).
- No quoted text requires a notice. The Unicode Standard's guidance is about
  what the screen shows, which Q1-Q4 decide. ISO/IEC 10646 requires, under a
  10646 claim, that such characters "be indicated to the user", and its NOTE
  1 says "The manner in which a user is notified ... is not specified by this
  standard" (section 2.2). So an on-screen placeholder can be the indication,
  and a notice would be one more manner, not a requirement.
- Criteria: whether the placeholder on screen already tells the user what
  § 5.21.6 wants them to know, against the noise a notice adds.

**Q7. The core (the owner's call, outside the renderer).**

- No renderer option fixes the 95 different emoji on macOS in a fresh `Term`,
  which come from `ClusterSlot::CAPACITY` and the store's collisions, or the
  2,703 of 2,760 sequences that differ in one session. Nor does any option fix
  the store's refusals over a session (note AB, Q7), or the core's refusal of
  any zero-width code point with nothing to attach to, at column 0 or on an
  empty cell: marks, ZWJ, selectors and tags (section 2.6).
- Options: decide the renderer now and the core separately, or decide them
  together. Neither choice is a verdict by itself. The core's deletions are
  what C7 reaches, if copy purports to reproduce the text (section 2.2). On
  screen they show as nothing, which departs from § 5.21.6, and for a mark at
  column 0 from § 5.13's "should be rendered as if they had a no-break space
  as a base character"; under a 10646 claim they depart from § 2.3 c)
  (section 2.2). If baud claims UTS #51 display (C2a) for the ZWJ set, C2
  requires the sequences the core alters to be declared unsupported.
- Also: does copy "purport not to modify" the text (C7)?
- Section 3 records the same class of drop in xterm, mlterm, Konsole, tmux
  and Warp. That is practice, and not a reason either way.

**Q8. Misplaced variation selectors and tag characters.** 18.0, the released
version, says misplaced selectors should display visibly (D57h, a chapter 3
"should"). For tags, § 5.20 says "misplaced format controls and variation
selectors should rather fallback to visible display (as with missing glyphs)",
and § 5.21.6 that "sequences of misplaced tag characters, may constitute a
security risk if not displayed visibly" (chapter 5, best practice). Neither
chapter 5 passage is in 17.0. baud pins UCD 17.0 (note AB, Q20 asks the same
for the bidi tables). Measured (section 2.6): a selector that follows a
cluster reaches the renderer and draws nothing, and so do misplaced tags (`a`
+ TAG a + CANCEL TAG); a selector that follows nothing is refused by the core,
and the code says a tag there is refused the same way (paint.rs:763-769;
inferred, not measured).

- Options:
  - Follow 17.0, as today. Follows 17.0. Departs from D57h's "should be
    displayed visibly" in 18.0, the released version, and from § 5.20 and
    § 5.21.6 for misplaced tags (best practice). The core's refusal deletes
    the code point (C7, section 2.6).
  - Follow 18.0: a placeholder for a misplaced selector or tag sequence that
    reaches the renderer, and keeping the refused ones in the core. Follows
    D57h, and § 5.20 and § 5.21.6 for tags.

**Q9. Hebrew cantillation.**

- Options:
  - Draw the 31 marks into baud's Hebrew (note AC). This follows the owner's
    rule that gaps are fixed in the embedded font, and takes them out of case
    A on every stack. The marks then draw as themselves, so no fallback text
    applies to them.
  - Rely on the placeholder chosen in Q2. This carries the verdict of the Q2
    option chosen.

### 7.3 Renders the owner must see before deciding

**What exists.**

- **The review-v13 starters** (`.claude/research/review-v13/starters/`, sheets
  a-e and noto-a..c, at 28 and 56 px; copied as
  `ad-obs-review-v13-sheet-<sheet>-<px>`, for example
  [[source: ad-obs-review-v13-sheet-noto-b-28]]) show HEAD, AS_BUILT and BOX,
  where BOX is the base face's `.notdef`. On the Noto sheets the BOX arm draws
  two blank columns while its counter reported `clusters_boxed=1`
  ([[source: ad-obs-review-v13-counts]]).
- **The design lens's sheets** draw real outlines at baud's measured cell
  sizes, but they are librsvg mockups, not baud's pixels.

**What is needed.** Renders through baud's own renderer at 13 px (8x17 cells)
and 26 px (16x34).

Rows:

- g (as built), a, b2, e, c, d, f;
- b1 under a configured SF Mono, to show the invisible box.

Columns:

- q + U+0316 on JetBrains Mono (A);
- bet + qamats + a cantillation mark on the working-tree font (A, Hebrew);
- a line of Genesis 1:1 with cantillation (how dense A gets);
- woman + ZWJ + laptop on an Apple Color Emoji subset and on a Noto subset (B);
- woman + ZWJ + U+1FAF9, an Emoji 18.0 character, on every shipped stack (B,
  a reserved code point);
- hand + skin tone (B);
- a regional-indicator pair that is not a flag, such as U+1F1E6 U+1F1E6,
  which spills on every shipped stack (B). AA is user-assigned in ISO 3166,
  so it is probably not a valid region subtag (inferred; CLDR's validity data
  is not pinned). B.1 applies to it, and C3 constrains it too. A valid pair
  without a flag would show the case C3 leaves alone, so the column could
  carry one as well;
- the black flag + `usca` on Apple Color Emoji, which draws the bare flag (a
  tag sequence);
- U+10D40, Garay (C, narrow);
- U+1FAE9 on Noto 2.042 (C, wide);
- a CJK character, and kana + U+309A, on the Linux stack (C, and C with A);
- a wrestling sequence with skin tones on Noto 2.042, one of the 59, which
  spills today;
- a ten-code-point kiss that the core alters, which shows a wrong emoji in
  every row, so the owner sees what no option fixes.

A framed hex box cannot fit an 8-px cell, by arithmetic. The unframed grids
fit: 7x11 px for a BMP code point and 7x17 px for an astral one (section 6).
Render both, so the owner sees the difference.
